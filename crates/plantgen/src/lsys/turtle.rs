//! Turtle interpretation: from a module string to segments, organs and the
//! branching topology the environment tools work on.
//!
//! The turtle frame follows *The Algorithmic Beauty of Plants*: heading `H`,
//! left `L` and up `U` with `H × L = U`. The plant starts at the origin with
//! `H` along +Y.

use super::derive::{Clock, ModuleString, choose};
use super::expr::{NO_ENV, Scope, eval};
use super::program::{Program, SymbolKind, Turtle};
use super::{GrowthError, Limits};
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
}

impl<'a> Interpreter<'a> {
    #[must_use]
    pub fn new(program: &'a Program, globals: &'a [f64], limits: &'a Limits) -> Self {
        Self {
            program,
            globals,
            limits,
            stack: Vec::with_capacity(32),
        }
    }

    /// Draw a string.
    ///
    /// # Errors
    ///
    /// Fails if a limit is exceeded or an interpretation rule produces a
    /// non-finite parameter.
    pub fn interpret(&mut self, string: &ModuleString, clock: Clock) -> Result<Scene, GrowthError> {
        let mut scene = Scene::default();
        let mut state = State {
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
        let mut saved = Vec::new();
        for (index, module) in string.modules.iter().enumerate() {
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
                &mut state,
                &mut saved,
                &mut scene,
                clock,
                0,
            )?;
            if let SymbolKind::Module { queries } =
                self.program.symbols[usize::from(module.symbol)].kind
                && queries != 0
            {
                let query = index_u32(scene.queries.len());
                let node = add_node(&mut scene, &mut state, NodeKind::Query(query));
                scene.queries.push(QueryPoint {
                    module: index_u32(index),
                    symbol: module.symbol,
                    node,
                    position: state.position,
                    heading: state.frame.h,
                    order: state.depth,
                });
            }
        }
        Ok(scene)
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
                for item in &*rule.successor {
                    let mut args = Vec::with_capacity(item.args.len());
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
                    self.draw(
                        item.symbol,
                        &args,
                        owner,
                        state,
                        saved,
                        scene,
                        clock,
                        depth + 1,
                    )?;
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
}
