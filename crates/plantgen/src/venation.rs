//! Leaf veins grown by space colonization (plant roadmap P4; Runions,
//! Fuhrer, Lane, Federl, Rolland-Lagan and Prusinkiewicz 2005, "Modeling
//! and visualization of leaf venation patterns").
//!
//! A leaf's veins grow in its blade's frame: a point `(a, t)` lies `t`
//! along the blade from its base (0) to its tip (1) and `a` across it, both
//! in blade lengths. A palmate leaf's frame has its origin where the main
//! veins meet and the blade's radius as its unit. The painter's primary
//! veins (a midrib, or a palmate leaf's main veins) seed the pattern, as
//! the first veins a leaf primordium lays down do.
//!
//! Auxin sources are scattered through the blade at least a spacing apart
//! (a Poisson disc by dart throwing), in two rounds: sparse for the
//! secondary veins, then dense for the tertiary. Each step every source
//! pulls the vein node nearest it. A pulled node grows a new node a step
//! $`D`$ toward its sources on each side of it, bent toward the leaf's tip
//! (or outward, on a palmate leaf) by the apical bias $`\beta`$:
//!
//! ```math
//! \mathbf n' = \mathbf n + D\,\widehat{\textstyle\sum_s \widehat{\mathbf s - \mathbf n} + \beta\,\hat{\mathbf b}}
//! ```
//!
//! A node grows only inside the blade. A source dies once a node comes
//! within its kill distance, so the veins are open: they branch but never
//! join. Every tooth and lobe has a source at its tip, so a vein runs into
//! each, as in craspedodromous leaves.
//!
//! A vein is as wide as the veins it feeds, by Murray's law from its tips,
//! $`w^3 = \sum_i w_i^3`$. [`Venation::near`] gives the grown veins near a
//! point with their widths as shares of the widest, which the painter
//! draws at the widths its templates can show.
//!
//! The growth uses no random generator: every draw is a hash of the leaf
//! and the draw's number, so a look's veins are the same every time.
//!
//! The code keeps the short names of the geometry: a point `p`, a
//! segment's ends `a` and `b`, a share `s` along it, `t` along the blade.
#![allow(clippy::many_single_char_names)]

use crate::rng::{combine, mix64, unit};

/// A point of a blade's frame, `(a, t)`.
pub type Point = (f64, f64);
/// A grown vein's segment: where it starts, where it ends, and its
/// half-width at the end, blade lengths.
pub type Segment = (Point, Point, f64);

/// How the veins of one leaf grow (see the module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Growth {
    /// Spacing of the sources of the secondary veins, blade lengths.
    pub secondary: f64,
    /// Spacing of the sources of the tertiary veins.
    pub tertiary: f64,
    /// Apical bias of the secondary and the tertiary veins.
    pub bias: (f64, f64),
    /// Bend the veins outward from the frame's origin (palmate leaves)
    /// rather than toward `t` (pinnate leaves).
    pub outward: bool,
}

impl Default for Growth {
    fn default() -> Self {
        Self {
            secondary: 1.0 / 7.0,
            tertiary: 1.0 / 20.0,
            bias: (0.9, 0.25),
            outward: false,
        }
    }
}

/// A grown vein node: where it is, the node it grew from, and its
/// half-width in blade lengths.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Node {
    at: (f64, f64),
    parent: Option<u32>,
    width: f64,
    primary: bool,
}

/// Half-width of a vein tip, blade lengths.
const TIP: f64 = 0.0011;
/// The step of the secondary veins, and of the tertiary.
const STEPS: (f64, f64) = (0.012, 0.007);
/// Kill distance over spacing.
const KILL: f64 = 0.4;
/// Dart throws per source the blade could hold.
const THROWS: usize = 24;
/// At most this many growth steps a round.
const ROUNDS: usize = 260;
/// Edge length of the lookup grid's cells, blade lengths.
const CELL: f64 = 0.02;
/// How far from a vein [`Venation::near`] finds it, blade lengths.
pub const REACH: f64 = 0.03;

/// A leaf's grown veins, ready to look up.
#[derive(Debug, Clone, PartialEq)]
pub struct Venation {
    nodes: Vec<Node>,
    /// The lookup grid: its origin, cells across and along, and each
    /// cell's grown segments (by child node).
    origin: (f64, f64),
    cells: (usize, usize),
    grid: Vec<Vec<u32>>,
    /// The widest grown vein's half-width.
    widest: f64,
}

/// The blade (lamina) a leaf's veins grow in: whether a point of its frame lies
/// inside, the box that holds it, its primary veins as polylines, and the
/// tips of its teeth and lobes.
pub struct Lamina<'a> {
    pub inside: &'a dyn Fn((f64, f64)) -> bool,
    pub bounds: ((f64, f64), (f64, f64)),
    pub primaries: Vec<Vec<(f64, f64)>>,
    pub tips: Vec<(f64, f64)>,
}

fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn length(a: (f64, f64)) -> f64 {
    a.0.hypot(a.1)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    length(sub(a, b))
}

fn unit_vector(a: (f64, f64)) -> (f64, f64) {
    let l = length(a);
    if l > 1e-12 {
        (a.0 / l, a.1 / l)
    } else {
        (0.0, 0.0)
    }
}

/// Distance from `p` to the segment `a`-`b`, and how far along it the
/// nearest point lies (0 to 1).
fn to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let d = sub(b, a);
    let span = d.0 * d.0 + d.1 * d.1;
    let s = if span > 0.0 {
        (((p.0 - a.0) * d.0 + (p.1 - a.1) * d.1) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (distance(p, (a.0 + d.0 * s, a.1 + d.1 * s)), s)
}

impl Venation {
    /// Grow the veins of `blade` (see the module), drawing from `seed`.
    #[must_use]
    pub fn grow(blade: &Lamina, growth: &Growth, seed: u64) -> Self {
        let mut nodes: Vec<Node> = Vec::new();
        // The primaries, one node a secondary step apart.
        for line in &blade.primaries {
            let mut previous: Option<u32> = None;
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let steps = (distance(a, b) / STEPS.0).ceil().max(1.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let count = steps as usize;
                let first = usize::from(previous.is_some());
                for k in first..=count {
                    #[allow(clippy::cast_precision_loss)]
                    let s = k as f64 / steps;
                    nodes.push(Node {
                        at: (a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s),
                        parent: previous,
                        width: 0.0,
                        primary: true,
                    });
                    previous = u32::try_from(nodes.len() - 1).ok();
                }
            }
        }
        let mut sources: Vec<(f64, f64)> = Vec::new();
        for (round, (spacing, step, bias)) in [
            (growth.secondary, STEPS.0, growth.bias.0),
            (growth.tertiary, STEPS.1, growth.bias.1),
        ]
        .into_iter()
        .enumerate()
        {
            let mut live = scatter(
                blade,
                spacing,
                &nodes,
                &sources,
                combine(seed, round as u64),
            );
            if round == 0 {
                // The tips of teeth and lobes, each fed by a vein.
                live.extend(
                    blade
                        .tips
                        .iter()
                        .copied()
                        .filter(|&tip| (blade.inside)(tip)),
                );
            }
            sources.extend(live.iter().copied());
            colonize(
                blade,
                &mut nodes,
                &mut live,
                spacing * KILL,
                step,
                bias,
                growth.outward,
            );
        }
        murray(&mut nodes);
        Self::indexed(nodes, blade.bounds)
    }

    fn indexed(nodes: Vec<Node>, bounds: ((f64, f64), (f64, f64))) -> Self {
        let margin = 0.01;
        let origin = (bounds.0.0 - margin, bounds.0.1 - margin);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cells = (
            (((bounds.1.0 - bounds.0.0 + 2.0 * margin) / CELL).ceil() as usize).max(1),
            (((bounds.1.1 - bounds.0.1 + 2.0 * margin) / CELL).ceil() as usize).max(1),
        );
        let mut grid = vec![Vec::new(); cells.0 * cells.1];
        for (index, node) in nodes.iter().enumerate() {
            let Some(parent) = node.parent else { continue };
            if node.primary {
                continue;
            }
            let (a, b) = (nodes[parent as usize].at, node.at);
            let low = (a.0.min(b.0) - REACH, a.1.min(b.1) - REACH);
            let high = (a.0.max(b.0) + REACH, a.1.max(b.1) + REACH);
            let (x0, y0) = cell_of(origin, cells, low);
            let (x1, y1) = cell_of(origin, cells, high);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid[y * cells.0 + x].push(u32::try_from(index).unwrap_or(u32::MAX));
                }
            }
        }
        let widest = nodes
            .iter()
            .filter(|node| !node.primary)
            .map(|node| node.width)
            .fold(TIP, f64::max);
        Self {
            nodes,
            origin,
            cells,
            grid,
            widest,
        }
    }

    /// The grown (secondary and finer) veins within [`REACH`] of `(a, t)`:
    /// how far each is (blade lengths) and its width there as a share of
    /// the widest grown vein's. The primaries are the painter's.
    pub fn near(&self, a: f64, t: f64) -> impl Iterator<Item = (f64, f64)> + '_ {
        let p = (a, t);
        let (x, y) = cell_of(self.origin, self.cells, p);
        self.grid
            .get(y * self.cells.0 + x)
            .into_iter()
            .flatten()
            .filter_map(move |&index| {
                let node = &self.nodes[index as usize];
                let from = &self.nodes[node.parent? as usize];
                let (d, s) = to_segment(p, from.at, node.at);
                let width = from.width + (node.width - from.width) * s;
                (d < REACH).then_some((d, width / self.widest))
            })
    }

    /// The grown nodes: where each is and where it grew from; for tests
    /// and pictures.
    #[must_use]
    pub fn segments(&self) -> Vec<Segment> {
        self.nodes
            .iter()
            .filter(|node| !node.primary)
            .filter_map(|node| {
                let parent = node.parent?;
                Some((self.nodes[parent as usize].at, node.at, node.width))
            })
            .collect()
    }
}

fn cell_of(origin: (f64, f64), cells: (usize, usize), p: (f64, f64)) -> (usize, usize) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let clamp = |value: f64, count: usize| (value.max(0.0) as usize).min(count - 1);
    (
        clamp((p.0 - origin.0) / CELL, cells.0),
        clamp((p.1 - origin.1) / CELL, cells.1),
    )
}

/// The nodes by grid cell, for the nearest node to a source and the
/// nodes near a point while the veins grow.
struct Nodes {
    origin: (f64, f64),
    cells: (usize, usize),
    grid: Vec<Vec<u32>>,
}

impl Nodes {
    fn new(bounds: ((f64, f64), (f64, f64)), nodes: &[Node]) -> Self {
        let margin = 0.05;
        let origin = (bounds.0.0 - margin, bounds.0.1 - margin);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cells = (
            (((bounds.1.0 - bounds.0.0 + 2.0 * margin) / CELL).ceil() as usize).max(1),
            (((bounds.1.1 - bounds.0.1 + 2.0 * margin) / CELL).ceil() as usize).max(1),
        );
        let mut index = Self {
            origin,
            cells,
            grid: vec![Vec::new(); cells.0 * cells.1],
        };
        for (n, node) in nodes.iter().enumerate() {
            index.insert(u32::try_from(n).unwrap_or(u32::MAX), node.at);
        }
        index
    }

    fn insert(&mut self, node: u32, at: (f64, f64)) {
        let (x, y) = cell_of(self.origin, self.cells, at);
        self.grid[y * self.cells.0 + x].push(node);
    }

    /// The nodes in the cells within `rings` of the cell holding `p`.
    fn near(&self, p: (f64, f64), rings: usize) -> impl Iterator<Item = u32> + '_ {
        let (x, y) = cell_of(self.origin, self.cells, p);
        let (x0, y0) = (x.saturating_sub(rings), y.saturating_sub(rings));
        let (x1, y1) = (
            (x + rings).min(self.cells.0 - 1),
            (y + rings).min(self.cells.1 - 1),
        );
        (y0..=y1).flat_map(move |cy| {
            (x0..=x1).flat_map(move |cx| self.grid[cy * self.cells.0 + cx].iter().copied())
        })
    }

    /// Whether any node lies within `radius` of `p`.
    fn any_within(&self, nodes: &[Node], p: (f64, f64), radius: f64) -> bool {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rings = (radius / CELL).ceil() as usize;
        self.near(p, rings)
            .any(|n| distance(p, nodes[n as usize].at) < radius)
    }

    /// The node nearest `p`, searching rings outward until no farther ring
    /// could hold a nearer one.
    fn nearest(&self, nodes: &[Node], p: (f64, f64)) -> Option<u32> {
        let mut best: Option<(f64, u32)> = None;
        let most = self.cells.0.max(self.cells.1);
        for rings in 0..=most {
            let (x, y) = cell_of(self.origin, self.cells, p);
            let (x0, y0) = (x.saturating_sub(rings), y.saturating_sub(rings));
            let (x1, y1) = (
                (x + rings).min(self.cells.0 - 1),
                (y + rings).min(self.cells.1 - 1),
            );
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    // Only the ring's own cells.
                    if rings > 0 && cy != y0 && cy != y1 && cx != x0 && cx != x1 {
                        continue;
                    }
                    for &n in &self.grid[cy * self.cells.0 + cx] {
                        let d = distance(p, nodes[n as usize].at);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, n));
                        }
                    }
                }
            }
            #[allow(clippy::cast_precision_loss)]
            if let Some((d, n)) = best
                && d <= rings as f64 * CELL
            {
                return Some(n);
            }
        }
        best.map(|(_, n)| n)
    }
}

/// Sources a `spacing` apart inside the blade, clear of the veins and of
/// the sources already placed.
fn scatter(
    blade: &Lamina,
    spacing: f64,
    nodes: &[Node],
    placed: &[(f64, f64)],
    seed: u64,
) -> Vec<(f64, f64)> {
    let index = Nodes::new(blade.bounds, nodes);
    let ((x0, y0), (x1, y1)) = blade.bounds;
    let area = (x1 - x0) * (y1 - y0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let throws = ((area / (spacing * spacing)).ceil() as usize).max(1) * THROWS;
    let mut sources: Vec<(f64, f64)> = Vec::new();
    for n in 0..throws {
        let h = combine(seed, n as u64);
        let p = (x0 + (x1 - x0) * unit(h), y0 + (y1 - y0) * unit(mix64(h)));
        if !(blade.inside)(p) {
            continue;
        }
        let clear = |q: &(f64, f64)| distance(p, *q) >= spacing;
        if sources.iter().all(clear)
            && placed.iter().all(clear)
            && !index.any_within(nodes, p, 0.5 * spacing)
        {
            sources.push(p);
        }
    }
    sources
}

/// Grow `nodes` toward `sources` until every source is reached or no node
/// can grow (see the module).
#[allow(clippy::too_many_arguments)]
fn colonize(
    blade: &Lamina,
    nodes: &mut Vec<Node>,
    sources: &mut Vec<(f64, f64)>,
    kill: f64,
    step: f64,
    bias: f64,
    outward: bool,
) {
    let mut index = Nodes::new(blade.bounds, nodes);
    for _ in 0..ROUNDS {
        // Kill the sources a node has reached.
        sources.retain(|&s| !index.any_within(nodes, s, kill));
        if sources.is_empty() {
            return;
        }
        // Each source pulls its nearest node, from its left or its right.
        let mut pulls: Vec<(u32, bool, (f64, f64))> = Vec::new();
        for &s in sources.iter() {
            let Some(nearest) = index.nearest(nodes, s) else {
                return;
            };
            let node = &nodes[nearest as usize];
            let heading = heading(nodes, node, outward);
            let to = unit_vector(sub(s, node.at));
            let left = heading.0 * to.1 - heading.1 * to.0 > 0.0;
            match pulls
                .iter_mut()
                .find(|(n, side, _)| *n == nearest && *side == left)
            {
                Some(pull) => pull.2 = (pull.2.0 + to.0, pull.2.1 + to.1),
                None => pulls.push((nearest, left, to)),
            }
        }
        let mut grew = false;
        for (parent, _, sum) in pulls {
            let node = nodes[parent as usize];
            let toward = bias_of(node.at, outward);
            let pull = unit_vector(sum);
            let direction = unit_vector((pull.0 + bias * toward.0, pull.1 + bias * toward.1));
            let next = (
                node.at.0 + step * direction.0,
                node.at.1 + step * direction.1,
            );
            if !(blade.inside)(next) || index.any_within(nodes, next, 0.5 * step) {
                continue;
            }
            nodes.push(Node {
                at: next,
                parent: Some(parent),
                width: 0.0,
                primary: false,
            });
            index.insert(u32::try_from(nodes.len() - 1).unwrap_or(u32::MAX), next);
            grew = true;
        }
        if !grew {
            return;
        }
    }
}

/// The way a node grows: from its parent, or for a root the bias.
fn heading(nodes: &[Node], node: &Node, outward: bool) -> (f64, f64) {
    node.parent
        .map(|parent| unit_vector(sub(node.at, nodes[parent as usize].at)))
        .filter(|h| h.0 != 0.0 || h.1 != 0.0)
        .unwrap_or_else(|| bias_of(node.at, outward))
}

/// The apical bias's direction at `p`: toward the tip, or outward from
/// the origin.
fn bias_of(p: (f64, f64), outward: bool) -> (f64, f64) {
    if outward {
        let away = unit_vector(p);
        if away == (0.0, 0.0) { (0.0, 1.0) } else { away }
    } else {
        (0.0, 1.0)
    }
}

/// Widths by Murray's law from the tips: children come after their
/// parents, so one pass from the last node back sums them.
fn murray(nodes: &mut [Node]) {
    let mut cubes = vec![0.0_f64; nodes.len()];
    for index in (0..nodes.len()).rev() {
        if nodes[index].primary {
            continue;
        }
        let cube = if cubes[index] > 0.0 {
            cubes[index]
        } else {
            TIP * TIP * TIP
        };
        nodes[index].width = cube.cbrt();
        if let Some(parent) = nodes[index].parent {
            cubes[parent as usize] += cube;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An elliptic blade with a midrib, as a simple leaf's frame holds it.
    fn ellipse() -> (impl Fn(Point) -> bool, Vec<Vec<Point>>) {
        let inside = |p: (f64, f64)| {
            let t = p.1;
            (0.0..=1.0).contains(&t) && p.0.abs() < 0.25 * (1.0 - (2.0 * t - 1.0).powi(2)).sqrt()
        };
        (inside, vec![vec![(0.0, 0.0), (0.0, 0.97)]])
    }

    fn grown() -> Venation {
        let (inside, primaries) = ellipse();
        let blade = Lamina {
            inside: &inside,
            bounds: ((-0.26, 0.0), (0.26, 1.0)),
            primaries,
            tips: Vec::new(),
        };
        Venation::grow(&blade, &Growth::default(), 7)
    }

    #[test]
    fn veins_grow_inside_the_blade_and_branch_from_the_midrib() {
        let (inside, _) = ellipse();
        let veins = grown();
        let segments = veins.segments();
        assert!(segments.len() > 100, "{}", segments.len());
        assert!(segments.iter().all(|&(_, b, _)| inside(b)));
        // Secondaries leave the midrib on both sides.
        let sides = segments
            .iter()
            .filter(|&&(a, _, _)| a.0 == 0.0)
            .map(|&(_, b, _)| b.0 > 0.0)
            .collect::<Vec<_>>();
        assert!(sides.iter().any(|&left| left) && sides.iter().any(|&left| !left));
    }

    #[test]
    fn secondaries_lean_toward_the_tip() {
        // The first step of each vein leaving the midrib points up the
        // leaf, as pinnate secondaries do (40 to 70 degrees from it).
        let veins = grown();
        let angles: Vec<f64> = veins
            .segments()
            .iter()
            .filter(|&&(a, _, _)| a.0 == 0.0)
            .map(|&(a, b, _)| crate::math::degrees((b.0 - a.0).abs().atan2(b.1 - a.1)))
            .collect();
        assert!(!angles.is_empty());
        #[allow(clippy::cast_precision_loss)]
        let mean = angles.iter().sum::<f64>() / angles.len() as f64;
        assert!((30.0..80.0).contains(&mean), "{mean}");
    }

    #[test]
    fn veins_feeding_more_are_wider() {
        // Murray's law: a vein is at least as wide as each vein it feeds,
        // and its tips are the narrowest.
        let veins = grown();
        let nodes = &veins.nodes;
        for node in nodes.iter().filter(|node| !node.primary) {
            assert!(node.width >= TIP * 0.999);
            if let Some(parent) = node.parent {
                let parent = &nodes[parent as usize];
                if !parent.primary {
                    assert!(parent.width >= node.width);
                }
            }
        }
        let widest = nodes.iter().map(|node| node.width).fold(0.0, f64::max);
        assert!(widest > 2.0 * TIP, "{widest}");
    }

    #[test]
    fn a_vein_runs_into_each_tooth() {
        let (inside, primaries) = ellipse();
        let tips = vec![(0.2, 0.4), (-0.2, 0.4), (0.18, 0.7)];
        let blade = Lamina {
            inside: &inside,
            bounds: ((-0.26, 0.0), (0.26, 1.0)),
            primaries,
            tips: tips.clone(),
        };
        let veins = Venation::grow(&blade, &Growth::default(), 3);
        let segments = veins.segments();
        for tip in tips {
            let reach = segments
                .iter()
                .map(|&(_, b, _)| distance(b, tip))
                .fold(f64::INFINITY, f64::min);
            assert!(
                reach < Growth::default().secondary * KILL + 0.01,
                "{tip:?} {reach}"
            );
        }
    }

    #[test]
    fn the_veins_are_the_same_every_time_and_found_where_they_run() {
        let (a, b) = (grown(), grown());
        assert_eq!(a, b);
        let &(from, to, _) = a
            .segments()
            .iter()
            .max_by(|x, y| x.2.total_cmp(&y.2))
            .unwrap();
        let middle = (f64::midpoint(from.0, to.0), f64::midpoint(from.1, to.1));
        // The widest vein, at its middle: on it, and the widest.
        let (d, share) = a
            .near(middle.0, middle.1)
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .unwrap();
        assert!(d < 1e-9 && share > 0.95, "{d} {share}");
        assert!(a.near(0.0, -0.5).next().is_none());
    }
}
