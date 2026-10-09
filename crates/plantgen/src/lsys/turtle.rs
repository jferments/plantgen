//! Turtle interpretation: from a module string to segments, organs and the
//! branching topology the environment tools work on.
//!
//! The turtle frame follows *The Algorithmic Beauty of Plants*: heading `H`,
//! left `L` and up `U` with `H × L = U`. The plant starts at the origin with
//! `H` along +Y.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::derive::{Clock, ModuleString, choose};
use super::expr::{NO_ENV, Scope, eval};
use super::program::{POP, PUSH, Program, SymbolKind, Turtle};
use super::{GrowthError, Limits};
use crate::cores::{self, Lease};
use crate::math::{self, Frame, Vec3};
use crate::rng::{Lineage, combine};

/// Element kinds hashed into segment and organ identities.
pub const KIND_SEGMENT: u64 = 0x7365_676d;
pub const KIND_ORGAN: u64 = 0x6f72_6761;

/// Radius of segments drawn before any `!` command, in metres.
pub const DEFAULT_WIDTH: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub id: u64,
    pub node: u32,
    pub start: Vec3,
    pub end: Vec3,
    /// Radius set with `!` when the segment was drawn.
    pub width: f64,
    pub born: f64,
    /// Branch order: the bracket depth the segment was drawn at.
    pub order: u16,
    /// 0 for wood drawn with `F`; for a segment drawn with a declared
    /// body, that body's index plus one.
    pub body: u8,
    /// The turtle's left vector while the segment was drawn, which orients
    /// a flattened body; zero for wood.
    pub left: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrganInstance {
    pub id: u64,
    pub symbol: u16,
    pub position: Vec3,
    pub frame: Frame,
    pub size: f64,
    pub born: f64,
    /// The segment the organ sits on, if one was drawn before it.
    pub segment: Option<u32>,
    /// The topology node the organ feeds in the pipe model.
    pub node: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Segment(u32),
    Query(u32),
}

/// A node of the branching topology: every segment and every module that
/// declared queries. A node's subtree is what follows it in its branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub parent: Option<u32>,
    /// True for the first node of a `[ ]` branch.
    pub lateral: bool,
    pub kind: NodeKind,
}

/// A module that declared queries, where the turtle found it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryPoint {
    /// Index of the module in the string.
    pub module: u32,
    pub symbol: u16,
    pub node: u32,
    pub position: Vec3,
    pub heading: Vec3,
    pub order: u16,
}

/// The drawn plant at one step.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub segments: Vec<Segment>,
    pub organs: Vec<OrganInstance>,
    pub nodes: Vec<Node>,
    pub queries: Vec<QueryPoint>,
    /// Highest point of any segment or organ, in metres above the plant's
    /// base. An organ reaches its size along its heading, as a card that
    /// stands on it does.
    pub height: f64,
}

impl Scene {
    /// Children of every node, in creation order, as offsets into a flat list.
    #[must_use]
    pub fn children(&self) -> (Vec<u32>, Vec<u32>) {
        let mut counts = vec![0_u32; self.nodes.len() + 1];
        for node in &self.nodes {
            if let Some(parent) = node.parent {
                counts[parent as usize + 1] += 1;
            }
        }
        for index in 1..counts.len() {
            counts[index] += counts[index - 1];
        }
        let mut fill = counts.clone();
        let mut list = vec![0_u32; self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                let slot = &mut fill[parent as usize];
                list[*slot as usize] = u32::try_from(index).unwrap_or(u32::MAX);
                *slot += 1;
            }
        }
        (counts, list)
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    position: Vec3,
    frame: Frame,
    width: f64,
    depth: u16,
    tropism: Vec3,
    elasticity: f64,
    node: Option<u32>,
    segment: Option<u32>,
    lateral: bool,
}

/// Identity of the string module being drawn and its element counters.
struct Owner {
    lineage: Lineage,
    born: f64,
    segments: u32,
    organs: u32,
    expansions: u64,
}

pub struct Interpreter<'a> {
    program: &'a Program,
    globals: &'a [f64],
    limits: &'a Limits,
    stack: Vec<f64>,
    /// An argument list for each depth of interpretation, kept between
    /// modules.
    arguments: Vec<Vec<f64>>,
    /// The last scene's sizes, so the next one is laid out once.
    sizes: [usize; 4],
    /// Saved states a piece of the string may not pop ([`Piece`]), and
    /// whether it popped one.
    floor: usize,
    breached: bool,
}

impl<'a> Interpreter<'a> {
    #[must_use]
    pub fn new(program: &'a Program, globals: &'a [f64], limits: &'a Limits) -> Self {
        Self {
            program,
            globals,
            limits,
            stack: Vec::with_capacity(32),
            arguments: Vec::new(),
            sizes: [0; 4],
            floor: 0,
            breached: false,
        }
    }

    /// Draw a string.
    ///
    /// A long string is drawn in pieces side by side: a piece hands a long
    /// branch, with the turtle as it stands at its `[`, to another thread
    /// and goes on past its `]`. The pieces are stitched together in
    /// string order with their parts renumbered, so the scene is the one
    /// drawing in order gives; a step whose pieces do anything that would
    /// not stitch (a branch that pops past its `[`, an error, a limit) is
    /// drawn again in order.
    ///
    /// # Errors
    ///
    /// Fails if a limit is exceeded or an interpretation rule produces a
    /// non-finite parameter.
    pub fn interpret(&mut self, string: &ModuleString, clock: Clock) -> Result<Scene, GrowthError> {
        let splits = string.len() >= PARALLEL_MODULES
            && self.program.interpretations[usize::from(PUSH)].is_empty()
            && self.program.interpretations[usize::from(POP)].is_empty();
        let lease = Lease::take(if splits { cores::per_task() } else { 1 });
        let scene = if lease.threads() > 1 {
            // Pieces small enough for the threads to share them evenly.
            let big = (string.len() / (lease.threads() * 6)).max(4096);
            self.in_pieces(string, clock, lease.threads(), big)
        } else {
            None
        };
        drop(lease);
        let scene = match scene {
            Some(scene) => scene,
            None => self.in_order(string, clock)?,
        };
        self.sizes = [
            scene.segments.len(),
            scene.organs.len(),
            scene.nodes.len(),
            scene.queries.len(),
        ];
        Ok(scene)
    }

    /// The string drawn module by module, in order.
    fn in_order(&mut self, string: &ModuleString, clock: Clock) -> Result<Scene, GrowthError> {
        // Room for a little more than the last step drew.
        let room = |size: usize| size + size / 8;
        let [segments, organs, nodes, queries] = self.sizes;
        let mut scene = Scene {
            segments: Vec::with_capacity(room(segments)),
            organs: Vec::with_capacity(room(organs)),
            nodes: Vec::with_capacity(room(nodes)),
            queries: Vec::with_capacity(room(queries)),
            height: 0.0,
        };
        let mut state = START_STATE;
        let mut saved = Vec::new();
        self.floor = 0;
        for index in 0..string.len() {
            self.module(string, index, &mut state, &mut saved, &mut scene, clock)?;
        }
        Ok(scene)
    }

    /// Draw module `index` of `string`, and its query point if it declared
    /// queries.
    fn module(
        &mut self,
        string: &ModuleString,
        index: usize,
        state: &mut State,
        saved: &mut Vec<State>,
        scene: &mut Scene,
        clock: Clock,
    ) -> Result<(), GrowthError> {
        let module = &string.modules[index];
        let mut owner = Owner {
            lineage: module.lineage,
            born: module.born,
            segments: 0,
            organs: 0,
            expansions: 0,
        };
        let params = string.params(module);
        self.draw(
            module.symbol,
            params,
            &mut owner,
            state,
            saved,
            scene,
            clock,
            0,
        )?;
        if let SymbolKind::Module { queries } =
            self.program.symbols[usize::from(module.symbol)].kind
            && queries != 0
        {
            let query = index_u32(scene.queries.len());
            let node = add_node(scene, state, NodeKind::Query(query));
            scene.queries.push(QueryPoint {
                module: index_u32(index),
                symbol: module.symbol,
                node,
                position: state.position,
                heading: state.frame.h,
                order: state.depth,
            });
        }
        Ok(())
    }

    /// The string drawn in pieces of more than `least` modules on up to
    /// `threads` threads, or `None` where the pieces would not stitch into
    /// the scene drawing in order gives.
    fn in_pieces(
        &self,
        string: &ModuleString,
        clock: Clock,
        threads: usize,
        least: usize,
    ) -> Option<Scene> {
        let matches = bracket_matches(string);
        let next = AtomicUsize::new(1);
        let drawn: Mutex<Vec<(usize, Piece)>> = Mutex::new(Vec::new());
        let root = PieceTask {
            id: 0,
            from: 0,
            to: string.len(),
            state: START_STATE,
        };
        let draw = |task: PieceTask, hand: &dyn Fn(PieceTask)| {
            let mut interpreter = Interpreter::new(self.program, self.globals, self.limits);
            let id = task.id;
            let piece = interpreter.piece(string, &matches, &task, least, clock, &next, hand);
            drawn
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((id, piece));
        };
        cores::tasks(vec![root], threads, false, &draw);
        let mut drawn = drawn
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drawn.sort_by_key(|(id, _)| *id);
        let pieces: Vec<Piece> = drawn.into_iter().map(|(_, piece)| piece).collect();
        stitch(&pieces, self.limits)
    }

    /// Draw one piece: modules `task.from..task.to` from the turtle
    /// `task.state`, handing on (`hand`) every branch of more than `least`
    /// modules that opens at the piece's own level, as a new task.
    #[allow(clippy::too_many_arguments)]
    fn piece(
        &mut self,
        string: &ModuleString,
        matches: &[u32],
        task: &PieceTask,
        least: usize,
        clock: Clock,
        next: &AtomicUsize,
        hand: &dyn Fn(PieceTask),
    ) -> Piece {
        let mut scene = Scene::default();
        let mut state = task.state;
        // Below the root, the `[` that opened the piece is saved first:
        // popping it would reach outside the piece.
        let mut saved = if task.id == 0 {
            Vec::new()
        } else {
            vec![state]
        };
        let level = saved.len();
        self.floor = level;
        self.breached = false;
        let mut children = Vec::new();
        let mut valid = true;
        let mut index = task.from;
        while index < task.to {
            if string.modules[index].symbol == PUSH && saved.len() == level {
                let close = matches[index] as usize;
                if close < task.to && close - index > least {
                    let id = next.fetch_add(1, Ordering::Relaxed);
                    let mut inner = state;
                    inner.depth = inner.depth.saturating_add(1);
                    inner.lateral = true;
                    children.push(Child {
                        id,
                        at: [
                            index_u32(scene.segments.len()),
                            index_u32(scene.organs.len()),
                            index_u32(scene.nodes.len()),
                            index_u32(scene.queries.len()),
                        ],
                        node: inner.node,
                        segment: inner.segment,
                    });
                    inner.node = inner.node.map(|_| START);
                    inner.segment = inner.segment.map(|_| START);
                    hand(PieceTask {
                        id,
                        from: index + 1,
                        to: close,
                        state: inner,
                    });
                    index = close + 1;
                    continue;
                }
            }
            if self
                .module(string, index, &mut state, &mut saved, &mut scene, clock)
                .is_err()
            {
                valid = false;
                break;
            }
            index += 1;
        }
        if self.breached || saved.len() != level {
            valid = false;
        }
        Piece {
            scene,
            children,
            valid,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        symbol: u16,
        params: &[f64],
        owner: &mut Owner,
        state: &mut State,
        saved: &mut Vec<State>,
        scene: &mut Scene,
        clock: Clock,
        depth: usize,
    ) -> Result<(), GrowthError> {
        let program = self.program;
        let rules = &program.interpretations[usize::from(symbol)];
        if !rules.is_empty() {
            owner.expansions += 1;
            let scope = Scope {
                globals: self.globals,
                locals: params,
                env: &NO_ENV,
                t: clock.t,
                dt: clock.dt,
                age: clock.t - owner.born,
                step: f64::from(clock.step),
                // No step in the key: a module looks the same every step.
                key: combine(owner.lineage.0, owner.expansions),
            };
            if let Some(rule) = choose(rules, &scope, &mut self.stack)? {
                if depth >= self.limits.max_interpretation_depth {
                    return Err(GrowthError::Rule {
                        span: rule.span,
                        message: format!(
                            "interpretation is nested more than {} deep",
                            self.limits.max_interpretation_depth
                        ),
                    });
                }
                if self.arguments.len() <= depth {
                    self.arguments.resize_with(depth + 1, Vec::new);
                }
                for item in &*rule.successor {
                    let mut args = std::mem::take(&mut self.arguments[depth]);
                    args.clear();
                    for code in &*item.args {
                        let value = eval(code, &scope, &mut self.stack);
                        if !value.is_finite() {
                            return Err(GrowthError::Rule {
                                span: rule.span,
                                message: format!(
                                    "a parameter of `{}` evaluated to {value}",
                                    program.symbols[usize::from(item.symbol)].name
                                ),
                            });
                        }
                        args.push(value);
                    }
                    let drawn = self.draw(
                        item.symbol,
                        &args,
                        owner,
                        state,
                        saved,
                        scene,
                        clock,
                        depth + 1,
                    );
                    self.arguments[depth] = args;
                    drawn?;
                }
                return Ok(());
            }
        }
        match &program.symbols[usize::from(symbol)].kind {
            SymbolKind::Turtle(turtle) => self.turtle(*turtle, params, owner, state, saved, scene),
            SymbolKind::Body { index } => {
                self.forward(params[0], index.saturating_add(1), owner, state, scene)
            }
            SymbolKind::Module { .. } => Ok(()),
            SymbolKind::Organ { .. } => {
                if scene.organs.len() >= self.limits.max_segments {
                    return Err(GrowthError::Limit {
                        what: "organs",
                        limit: self.limits.max_segments as u64,
                    });
                }
                let id = owner.lineage.element(KIND_ORGAN, owner.organs);
                owner.organs += 1;
                let frame = organ_frame(state.frame, params);
                let reach = state.position + frame.h * params[0];
                scene.height = scene.height.max(state.position.y).max(reach.y);
                scene.organs.push(OrganInstance {
                    id,
                    symbol,
                    position: state.position,
                    frame,
                    size: params[0],
                    born: owner.born,
                    segment: state.segment,
                    node: state.node,
                });
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn turtle(
        &mut self,
        turtle: Turtle,
        params: &[f64],
        owner: &mut Owner,
        state: &mut State,
        saved: &mut Vec<State>,
        scene: &mut Scene,
    ) -> Result<(), GrowthError> {
        let frame = state.frame;
        match turtle {
            Turtle::Push => {
                saved.push(*state);
                state.depth = state.depth.saturating_add(1);
                state.lateral = true;
            }
            Turtle::Pop => {
                if saved.len() <= self.floor {
                    self.breached = true;
                }
                if let Some(previous) = saved.pop() {
                    *state = previous;
                }
            }
            Turtle::Cut => {}
            Turtle::Forward => self.forward(params[0], 0, owner, state, scene)?,
            Turtle::Move => state.position += frame.h * params[0],
            Turtle::Left => state.frame = frame.rotated(frame.u, math::radians(params[0])),
            Turtle::Right => state.frame = frame.rotated(frame.u, -math::radians(params[0])),
            Turtle::Down => state.frame = frame.rotated(frame.l, math::radians(params[0])),
            Turtle::Up => state.frame = frame.rotated(frame.l, -math::radians(params[0])),
            Turtle::RollRight => state.frame = frame.rotated(frame.h, math::radians(params[0])),
            Turtle::RollLeft => state.frame = frame.rotated(frame.h, -math::radians(params[0])),
            Turtle::Around => state.frame = frame.rotated(frame.u, math::PI),
            Turtle::Level => state.frame = level(frame),
            Turtle::Width => state.width = params[0].max(0.0),
            Turtle::Tropism => {
                state.tropism = Vec3::new(params[0], params[1], params[2]);
                state.elasticity = params[3];
            }
            Turtle::Steer => {
                let target = Vec3::new(params[0], params[1], params[2]);
                if target.length() > 1e-12 {
                    let target = target.normalize_or(frame.h);
                    let angle = math::acos(frame.h.dot(target));
                    let axis = frame.h.cross(target);
                    let axis = if axis.length() > 1e-9 {
                        axis.normalize_or(frame.l)
                    } else {
                        frame.l
                    };
                    state.frame = frame.rotated(axis, angle * params[3].clamp(0.0, 1.0));
                }
            }
        }
        Ok(())
    }
}

impl Interpreter<'_> {
    /// Draw a segment of length `length` along the heading, of wood (`body`
    /// 0) or of a declared body, then apply the tropism.
    fn forward(
        &self,
        length: f64,
        body: u8,
        owner: &mut Owner,
        state: &mut State,
        scene: &mut Scene,
    ) -> Result<(), GrowthError> {
        if scene.segments.len() >= self.limits.max_segments {
            return Err(GrowthError::Limit {
                what: "segments",
                limit: self.limits.max_segments as u64,
            });
        }
        let frame = state.frame;
        let start = state.position;
        let end = start + frame.h * length;
        let index = index_u32(scene.segments.len());
        let node = add_node(scene, state, NodeKind::Segment(index));
        scene.segments.push(Segment {
            id: owner.lineage.element(KIND_SEGMENT, owner.segments),
            node,
            start,
            end,
            width: state.width,
            born: owner.born,
            order: state.depth,
            body,
            left: if body == 0 { Vec3::ZERO } else { frame.l },
        });
        owner.segments += 1;
        scene.height = scene.height.max(end.y);
        state.position = end;
        state.segment = Some(index);
        if state.elasticity != 0.0 {
            let axis = frame.h.cross(state.tropism);
            let strength = axis.length();
            if strength > 1e-12 {
                state.frame = frame.rotated(axis / strength, state.elasticity * strength);
            }
        }
        Ok(())
    }
}

fn index_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// The turtle at the start of a string.
const START_STATE: State = State {
    position: Vec3::ZERO,
    frame: Frame::UPRIGHT,
    width: DEFAULT_WIDTH,
    depth: 0,
    tropism: Vec3::ZERO,
    elasticity: 0.0,
    node: None,
    segment: None,
    lateral: false,
};

/// Strings shorter than this are drawn on one thread.
const PARALLEL_MODULES: usize = 50_000;

/// In a piece's own scene, the node or segment the turtle stood on where
/// the piece begins, which lies in the piece that handed it on.
const START: u32 = u32::MAX;

/// A piece of the string to draw: the modules after a `[` up to its `]`
/// (the whole string for the first), from the turtle as it stood there.
struct PieceTask {
    id: usize,
    from: usize,
    to: usize,
    state: State,
}

/// A branch a piece handed on: its piece, how many parts of each kind
/// (segments, organs, nodes, queries) the piece had drawn when it did, and
/// the node and segment the turtle stood on, as the piece numbers them.
struct Child {
    id: usize,
    at: [u32; 4],
    node: Option<u32>,
    segment: Option<u32>,
}

/// A drawn piece: its scene, numbered from 0 with [`START`] for where it
/// begins, the branches it handed on, in order, and whether it stitches.
struct Piece {
    scene: Scene,
    children: Vec<Child>,
    valid: bool,
}

/// For each `[` of the string, the index of its `]` (`u32::MAX` if none).
fn bracket_matches(string: &ModuleString) -> Vec<u32> {
    let mut matches = vec![u32::MAX; string.len()];
    let mut open = Vec::new();
    for (index, module) in string.modules.iter().enumerate() {
        if module.symbol == PUSH {
            open.push(index);
        } else if module.symbol == POP
            && let Some(at) = open.pop()
        {
            matches[at] = index_u32(index);
        }
    }
    matches
}

/// The pieces' scenes as one, in string order: each piece's parts with
/// its branches' parts where it handed them on, renumbered; `None` if a
/// piece does not stitch or the scene passes a limit.
fn stitch(pieces: &[Piece], limits: &Limits) -> Option<Scene> {
    if pieces.iter().any(|piece| !piece.valid) {
        return None;
    }
    let count = |scene: &Scene| {
        [
            scene.segments.len(),
            scene.organs.len(),
            scene.nodes.len(),
            scene.queries.len(),
        ]
    };
    // A child's id is always greater than its parent's: sizes from the
    // last piece up, offsets from the first down.
    let mut sizes: Vec<[usize; 4]> = pieces.iter().map(|piece| count(&piece.scene)).collect();
    for id in (0..pieces.len()).rev() {
        for child in &pieces[id].children {
            let size = sizes[child.id];
            for kind in 0..4 {
                sizes[id][kind] += size[kind];
            }
        }
    }
    let [segments, organs, nodes, queries] = sizes[0];
    if segments > limits.max_segments || organs > limits.max_segments {
        return None;
    }
    let (offsets, starts) = place(pieces, &sizes);
    let mut scene = Scene {
        segments: Vec::with_capacity(segments),
        organs: Vec::with_capacity(organs),
        nodes: Vec::with_capacity(nodes),
        queries: Vec::with_capacity(queries),
        height: pieces
            .iter()
            .fold(0.0, |height, piece| height.max(piece.scene.height)),
    };
    let mut order = vec![(0_usize, [0_usize; 4], 0_usize)];
    // Depth first: a piece's parts up to each branch, the branch, then on.
    while let Some((id, mut at, mut child)) = order.pop() {
        let piece = &pieces[id];
        let until = piece
            .children
            .get(child)
            .map_or_else(|| count(&piece.scene), |next| next.at.map(|at| at as usize));
        let number =
            |kind: usize, index: u32| renumber(pieces, &sizes, &offsets, &starts, id, kind, index);
        for segment in &piece.scene.segments[at[SEGMENT]..until[SEGMENT]] {
            scene.segments.push(Segment {
                node: number(NODE, segment.node),
                ..*segment
            });
        }
        for organ in &piece.scene.organs[at[ORGAN]..until[ORGAN]] {
            scene.organs.push(OrganInstance {
                segment: organ.segment.map(|index| number(SEGMENT, index)),
                node: organ.node.map(|index| number(NODE, index)),
                ..*organ
            });
        }
        for node in &piece.scene.nodes[at[NODE]..until[NODE]] {
            scene.nodes.push(Node {
                parent: node.parent.map(|index| number(NODE, index)),
                kind: match node.kind {
                    NodeKind::Segment(index) => NodeKind::Segment(number(SEGMENT, index)),
                    NodeKind::Query(index) => NodeKind::Query(number(QUERY, index)),
                },
                ..*node
            });
        }
        for query in &piece.scene.queries[at[QUERY]..until[QUERY]] {
            scene.queries.push(QueryPoint {
                node: number(NODE, query.node),
                ..*query
            });
        }
        if let Some(next) = piece.children.get(child) {
            at = next.at.map(|at| at as usize);
            child += 1;
            order.push((id, at, child));
            order.push((next.id, [0; 4], 0));
        }
    }
    Some(scene)
}

/// Where each piece's parts begin in the stitched scene, by kind, and the
/// node and segment its branch begins on, renumbered. A child's id is
/// greater than its parent's, so parents come first.
fn place(pieces: &[Piece], sizes: &[[usize; 4]]) -> (Vec<[usize; 4]>, Starts) {
    let mut offsets = vec![[0_usize; 4]; pieces.len()];
    // Where each piece's branch begins: its node and segment, renumbered.
    let mut starts: Starts = vec![(None, None); pieces.len()];
    for id in 0..pieces.len() {
        let mut before = [0_usize; 4];
        for child in &pieces[id].children {
            for kind in 0..4 {
                offsets[child.id][kind] =
                    offsets[id][kind] + child.at[kind] as usize + before[kind];
            }
            let size = sizes[child.id];
            for kind in 0..4 {
                before[kind] += size[kind];
            }
            starts[child.id] = (
                child
                    .node
                    .map(|node| renumber(pieces, sizes, &offsets, &starts, id, NODE, node)),
                child.segment.map(|segment| {
                    renumber(pieces, sizes, &offsets, &starts, id, SEGMENT, segment)
                }),
            );
        }
    }
    (offsets, starts)
}

/// Each piece's start: the node and segment its branch begins on.
type Starts = Vec<(Option<u32>, Option<u32>)>;

const SEGMENT: usize = 0;
const ORGAN: usize = 1;
const NODE: usize = 2;
const QUERY: usize = 3;

/// The index in the stitched scene of part `index` of kind `kind` in piece
/// `id`: its piece's offset, its own index, and every branch the piece
/// handed on before drawing it; [`START`] is where the piece begins.
fn renumber(
    pieces: &[Piece],
    sizes: &[[usize; 4]],
    offsets: &[[usize; 4]],
    starts: &Starts,
    id: usize,
    kind: usize,
    index: u32,
) -> u32 {
    if index == START {
        let (node, segment) = starts[id];
        let start = if kind == NODE { node } else { segment };
        return start.unwrap_or(START);
    }
    let before: usize = pieces[id]
        .children
        .iter()
        .take_while(|child| child.at[kind] <= index)
        .map(|child| sizes[child.id][kind])
        .sum();
    index_u32(offsets[id][kind] + index as usize + before)
}

fn add_node(scene: &mut Scene, state: &mut State, kind: NodeKind) -> u32 {
    let index = index_u32(scene.nodes.len());
    scene.nodes.push(Node {
        parent: state.node,
        lateral: state.lateral,
        kind,
    });
    state.lateral = false;
    state.node = Some(index);
    index
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::lsys::derive::Deriver;
    use std::collections::BTreeMap;

    fn scene(source: &str) -> Scene {
        let program = Program::compile(source).unwrap();
        let globals = program.resolve_params(&BTreeMap::new()).unwrap();
        let limits = Limits::default();
        let string = Deriver::new(&program, &globals, &limits)
            .axiom(3, 1.0)
            .unwrap();
        let clock = Clock {
            step: 0,
            t: 0.0,
            dt: 1.0,
        };
        Interpreter::new(&program, &globals, &limits)
            .interpret(&string, clock)
            .unwrap()
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn forward_draws_up_and_turns_rotate_the_heading() {
        let s = scene("lsystem p 1; axiom F(2) +(90) F(1) [ &(90) F(1) ] F(1);");
        assert_eq!(s.segments.len(), 4);
        assert!(close(s.segments[0].end, Vec3::new(0.0, 2.0, 0.0)));
        // `+` turns the heading toward the turtle's left, which is +X.
        assert!(close(s.segments[1].end, Vec3::new(1.0, 2.0, 0.0)));
        // `&` pitches down, toward -U, which is +Z here; `]` restores the frame.
        assert!(close(s.segments[2].end, Vec3::new(1.0, 2.0, 1.0)));
        assert!(close(s.segments[3].end, Vec3::new(2.0, 2.0, 0.0)));
        assert_eq!(s.segments[2].order, 1);
        assert_eq!(s.height, 2.0);
    }

    #[test]
    fn organs_raise_the_height_by_their_reach() {
        let s = scene(
            "lsystem p 1; organ leaf leaf;
             axiom F(1) leaf(0.5) [ &(180) leaf(2) ] [ +(90) leaf(3) ];",
        );
        // The upright leaf reaches 0.5 m above the stem; the hanging and
        // the level one reach no higher than their bases.
        assert!((s.height - 1.5).abs() < 1e-9);
    }

    #[test]
    fn topology_marks_laterals_and_queries_attach_to_the_branch() {
        let s = scene(
            "lsystem p 1; module A queries position;
             axiom F(1) [ +(30) F(1) A ] F(1) A;",
        );
        assert_eq!(s.nodes.len(), 5);
        assert_eq!(s.queries.len(), 2);
        // Lateral segment is the first node of its branch.
        assert!(s.nodes[1].lateral);
        assert_eq!(s.nodes[1].parent, Some(0));
        // The lateral apex follows the lateral segment on the same branch.
        assert_eq!(s.nodes[2].parent, Some(1));
        assert!(!s.nodes[2].lateral);
        // The main segment continues from the base segment.
        assert_eq!(s.nodes[3].parent, Some(0));
        assert!(!s.nodes[3].lateral);
        let (offsets, children) = s.children();
        let base: Vec<u32> = children[offsets[0] as usize..offsets[1] as usize].to_vec();
        assert_eq!(base, [1, 3]);
    }

    #[test]
    fn tropism_bends_toward_its_vector() {
        let s = scene("lsystem p 1; axiom +(90) T(0, -1, 0, 0.3) F(1) F(1) F(1);");
        let last = s.segments[2];
        let direction = (last.end - last.start).normalize_or(Vec3::ZERO);
        assert!(direction.y < -0.1, "{direction:?}");
    }

    #[test]
    fn steer_turns_toward_a_world_direction() {
        let s = scene("lsystem p 1; axiom steer(1, 0, 0, 1) F(1) steer(0, 0, 1, 0.5) F(1);");
        assert!(close(s.segments[0].end, Vec3::new(1.0, 0.0, 0.0)));
        let second = (s.segments[1].end - s.segments[1].start).normalize_or(Vec3::ZERO);
        let expected = Vec3::new(1.0, 0.0, 1.0).normalize_or(Vec3::ZERO);
        assert!(close(second, expected), "{second:?}");
    }

    #[test]
    fn interpretation_rules_draw_without_changing_the_string() {
        let s = scene(
            "lsystem p 1; module I(n); organ leaf leaf;
             axiom I(3);
             interpret I(n) -> F(n) [ leaf(0.5) ];",
        );
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0].end.y, 3.0);
        assert_eq!(s.organs.len(), 1);
        assert_eq!(s.organs[0].size, 0.5);
        assert_eq!(s.organs[0].segment, Some(0));
    }
}

/// `frame` turned so its left points level (`$`), or as it is when it heads
/// straight up or down.
fn level(frame: Frame) -> Frame {
    let left = Vec3::Y.cross(frame.h);
    if left.length() > 1e-9 {
        let l = left.normalize_or(frame.l);
        Frame {
            h: frame.h,
            l,
            u: frame.h.cross(l),
        }
        .orthonormalized()
    } else {
        frame
    }
}

/// The frame an organ is placed in: the turtle's, or for an organ called
/// with its own turn, `organ(size, roll, pitch[, level])`, the turtle's
/// rolled by `roll` (`/`), pitched down by `pitch` (`&`) and, with a level
/// above 0, levelled (`$`), the same operations `[ /(roll) &(pitch) $
/// organ(size) ]` applies, without changing the turtle.
fn organ_frame(frame: Frame, params: &[f64]) -> Frame {
    let [_, roll, pitch, rest @ ..] = params else {
        return frame;
    };
    let rolled = frame.rotated(frame.h, math::radians(*roll));
    let pitched = rolled.rotated(rolled.l, math::radians(*pitch));
    match rest.first() {
        Some(flag) if *flag > 0.0 => level(pitched),
        _ => pitched,
    }
}

#[cfg(test)]
mod organ_turn_tests {
    use crate::conditions::Conditions;
    use crate::grow::{GrowthSettings, grow};
    use crate::lsys::{Limits, Program};

    #[test]
    fn an_organ_with_its_own_turn_lies_as_the_bracketed_turn_puts_it() {
        let grow_with = |successor: &str| {
            let program = Program::compile(&format!(
                "lsystem p 1; organ leaf leaf; module A; axiom /(30) &(20) F(1) A;
                 rule A -> {successor};"
            ))
            .unwrap();
            let settings = GrowthSettings {
                seed: 3,
                dt: 1.0,
                years: 1.0,
                keyframes: vec![1.0],
                conditions: Conditions::preset(crate::spec::Environment::Open),
                limits: Limits::default(),
                host: None,
            };
            let growth = grow(&program, &[], &settings).unwrap();
            growth.keyframes[0].organs.clone()
        };
        let bracketed = grow_with("[ /(70) &(55) $ leaf(0.3) ] [ /(250) &(40) leaf(0.2) ] F(0.5)");
        let turned = grow_with("leaf(0.3, 70, 55, 1) leaf(0.2, 250, 40) F(0.5)");
        assert_eq!(bracketed.len(), 2);
        for (a, b) in bracketed.iter().zip(&turned) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.heading, b.heading);
            assert_eq!(a.left, b.left);
            assert_eq!(a.size.to_bits(), b.size.to_bits());
        }
    }

    /// Pieces drawn side by side and stitched give the scene drawing in
    /// order gives: branches handed on at every level, organs and queries
    /// on the branch they open, and interpretation rules with their own
    /// brackets; a piece that fails leaves the step to drawing in order.
    #[test]
    fn pieces_stitch_into_the_scene_drawn_in_order() {
        use crate::lsys::derive::{Clock, Deriver};
        use crate::lsys::turtle::Interpreter;
        use std::collections::BTreeMap;
        let source = "
            lsystem pieces 1;
            module A(n) queries light;
            module I(n);
            organ leaf foliage area 0.02;
            tool light@1 { cell = 0.3 };
            axiom A(0);
            rule A(n) : n < 7 -> F(1) leaf(0.1) [ +(30) A(n + 1) leaf(0.2) ] /(90) I(n)
                [ -(20) &(10) A(n + 1) ] A(n + 1);
            interpret I(n) -> F(0.5) [ leaf(0.5) ] [ &(20) F(0.2) ];
        ";
        let program = Program::compile(source).unwrap();
        let globals = program.resolve_params(&BTreeMap::new()).unwrap();
        let limits = Limits::default();
        let mut deriver = Deriver::new(&program, &globals, &limits);
        let mut string = deriver.axiom(3, 1.0).unwrap();
        for step in 0..7 {
            let clock = Clock {
                step,
                t: f64::from(step),
                dt: 1.0,
            };
            string = deriver.derive(&string, &[], clock).unwrap();
        }
        let clock = Clock {
            step: 7,
            t: 7.0,
            dt: 1.0,
        };
        let mut interpreter = Interpreter::new(&program, &globals, &limits);
        let in_order = interpreter.in_order(&string, clock).unwrap();
        assert!(in_order.segments.len() > 500 && !in_order.queries.is_empty());
        for (threads, least) in [(1, 0), (4, 8), (3, 40), (2, 300)] {
            let pieces = interpreter
                .in_pieces(&string, clock, threads, least)
                .expect("the pieces stitch");
            assert_eq!(pieces, in_order, "{threads} threads, pieces over {least}");
        }
        // A piece that fails leaves the step to drawing in order, which
        // reports the error.
        let failing = Program::compile(
            "lsystem p 1; module U(x); axiom F [ F [ F U(0) F ] F ] F; interpret U(x) -> F(1 / x);",
        )
        .unwrap();
        let globals = failing.resolve_params(&BTreeMap::new()).unwrap();
        let string = Deriver::new(&failing, &globals, &limits)
            .axiom(1, 1.0)
            .unwrap();
        let mut interpreter = Interpreter::new(&failing, &globals, &limits);
        assert!(interpreter.in_pieces(&string, clock, 2, 0).is_none());
        assert!(interpreter.interpret(&string, clock).is_err());
    }
}
