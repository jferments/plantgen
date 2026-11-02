//! A plant broken at its break sites (plant motion S5, slice B2).
//!
//! [`apply`] takes a grown, drawn [`PlantGraph`] and the places it breaks
//! ([`Break`]: a graph segment whose start section fails, and the way the
//! wind blew), and gives back the plant left standing, with a short stub of
//! each broken axis, and each piece that fell, as graphs of their own that
//! [`crate::mesh`] draws as it draws any plant. [`fracture`] draws the torn
//! end of each stub and each piece: green wood that fails in bending tears
//! out in splinters on its tension side, the side the wind came from,
//! longest there and nearly square on the side crushed in compression,
//! with the bark peeled back a little from the break; how far the
//! splinters reach follows how tough the wood is ([`Toughness`]).
//!
//! Nothing here changes a plant that is not broken: a host asks for it
//! with the sites the motion record gives (`sites[]`), or a render with
//! `plantc render --break`.

use crate::graph::{GraphOrgan, GraphSegment, PlantGraph};
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng;

/// Where a plant breaks: through the start of graph segment `segment`,
/// under wind blowing towards `toward` (level; its tension side faces the
/// other way) at `wind` m/s at 10 m, which carries what falls free down
/// the wind (0 lets it fall straight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Break {
    pub segment: u32,
    pub toward: Vec3,
    pub wind: f64,
}

/// How a break tears ([`tear`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tear {
    /// Clean through: a branch off at its collar, an axis through its
    /// section.
    Clean,
    /// Through the tension side only: the crushed fibres of the
    /// compression side hold, and the piece swings down about them and
    /// stays on (a greenstick break: a top hung on its stump, a branch
    /// hanging from its break).
    Hinged,
    /// A branch too thick for a collar tears out of its stem, taking a
    /// strip of the stem's wood below it and leaving a torn wound.
    Split,
}

/// A torn end to draw: on a stub of the standing plant, or on a fallen
/// piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct End {
    /// The break section's centre, the way the torn wood points (out of
    /// the wood it belongs to) and the wood's radius there, m.
    pub point: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    /// The level direction from the section's centre to its tension side.
    pub tension: Vec3,
    /// The segment that bears it in its own graph, for binding.
    pub segment: u32,
    /// The wind hierarchy level of the wood: 0 stem, 1 branch, 2 twig.
    pub level: u8,
    /// Seeds its splinters.
    pub seed: u64,
    /// How far its splinters reach, against a break straight across the
    /// grain (1): a wound torn along the grain leaves short ones.
    pub torn: f64,
}

/// A fallen piece: its graph, laid on the ground, and its torn end.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub graph: PlantGraph,
    pub end: End,
}

/// The plant after it broke.
#[derive(Debug, Clone, PartialEq)]
pub struct Damaged {
    /// What still stands, each broken axis ending in a short stub.
    pub standing: PlantGraph,
    /// The torn end of each stub, in `standing`.
    pub stubs: Vec<End>,
    /// What fell, in the order of the breaks that made them.
    pub pieces: Vec<Piece>,
    /// How each of those breaks tore.
    pub tears: Vec<Tear>,
}

/// How far past the wood the stub of a branch broken at its foot reaches
/// beyond its parent's surface, in its own radii: the branch collar a torn
/// branch leaves.
const COLLAR: f64 = 0.5;

/// The length of the segment that keeps a stub's end its full radius, m
/// (the tube of a tip narrows over its last segment).
const HOLD: f64 = 0.001;

/// A branch at least this share of its stem's radius at their junction
/// has no collar to break at, and splits out of the stem: arborists' rule
/// that a collar forms only round a branch under about half its stem's
/// diameter (codominant and near-codominant stems tear out).
pub const SPLIT_RATIO: f64 = 0.5;

/// How far below a split branch the strip of stem wood it takes runs, in
/// the branch's radii, and how thick that strip is.
const STRIP: f64 = 4.0;
const STRIP_RADIUS: f64 = 0.5;

/// How deep a split branch's socket and wound sit in the stem, in the
/// branch's radii.
const SOCKET: f64 = 0.3;

/// How deep the scar a split leaves sits under the stem's surface, in the
/// branch's radii: nearly flush.
const WOUND_SINK: f64 = 0.1;

/// How far the splinters of a split's wound reach, against a break across
/// the grain: the strip peels away along the grain and leaves the scar
/// ragged but nearly flush.
const WOUND_TORN: f64 = 0.15;

/// Green wood's density, kg/m³, for a falling piece's weight: freshly
/// felled wood is about as heavy as water, a little less.
const GREEN: f64 = 900.0;

/// A fallen piece's leaves, kg per m² of one side, as the motion record
/// assumes where unstated, and their drag coefficient tumbling.
const LEAF_MASS: f64 = 0.2;
const LEAF_CD: f64 = 0.5;

/// Wood's drag coefficient across the wind (a cylinder) and air's
/// density, kg/m³.
const WOOD_CD: f64 = 1.2;
const AIR: f64 = 1.2226;

/// Standard gravity, m/s².
const GRAVITY: f64 = 9.806_65;

/// How far round from straight down the wind a piece that falls free may
/// come to lie, radians either way: it tumbles as it falls.
const TUMBLE: f64 = 0.7;

/// How the break through the start of segment `first` of `graph` tears,
/// for wood of toughness `toughness` (`children`: each segment's). A
/// branch, or an arm of a fork, broken at its foot splits out of its stem
/// when it is at least [`SPLIT_RATIO`] of the stem's radius, else breaks
/// at its collar. An axis broken along itself hinges (stays on by its
/// compression side) with the chance [`Toughness::hinge`], drawn from the
/// segment's id; else it breaks clean through.
fn tear(graph: &PlantGraph, children: &[Vec<u32>], first: u32, toughness: Toughness) -> Tear {
    let Some(segment) = graph.segments.get(first as usize) else {
        return Tear::Clean;
    };
    if let Some(parent) = junction(graph, children, first) {
        let host = &graph.segments[parent as usize];
        return if segment.radius >= SPLIT_RATIO * host.radius {
            Tear::Split
        } else {
            Tear::Clean
        };
    }
    if rng::unit(rng::combine(segment.id, 0x4849_4E47_4544)) < toughness.hinge() {
        Tear::Hinged
    } else {
        Tear::Clean
    }
}

/// How far back along its axis a break may sit from the junction its axis
/// leaves and still break there, in the wood's radii: within two of its
/// diameters it is the junction that fails.
const NEAR_JUNCTION: f64 = 4.0;

/// Where a break through the start of segment `first` of `graph` falls:
/// at the junction its axis leaves, where that is within
/// [`NEAR_JUNCTION`] radii back along an axis that bears nothing else on
/// the way; else where it is.
fn foot(graph: &PlantGraph, children: &[Vec<u32>], first: u32) -> u32 {
    let reach = NEAR_JUNCTION * graph.segments[first as usize].radius;
    let mut at = first;
    let mut walked = 0.0;
    loop {
        if junction(graph, children, at).is_some() {
            return at;
        }
        let segment = &graph.segments[at as usize];
        let Some(parent) = segment.parent else {
            return first;
        };
        let up = &graph.segments[parent as usize];
        walked += (up.end - up.start).length();
        if walked > reach || children[parent as usize].len() != 1 {
            return first;
        }
        at = parent;
    }
}

/// An axis running on past a branch at least this share of its own
/// radius is one arm of a fork: codominant stems.
pub const CODOMINANT: f64 = 0.7;

/// The segment that segment `first` of `graph` leaves at a junction: its
/// parent, where it is a branch, or where it runs on past a branch nearly
/// as thick as itself (at least [`CODOMINANT`] of it), one arm of a fork;
/// none where it runs on along its own axis past thinner branches.
fn junction(graph: &PlantGraph, children: &[Vec<u32>], first: u32) -> Option<u32> {
    let segment = graph.segments.get(first as usize)?;
    let parent = segment.parent?;
    let forked = || {
        children[parent as usize].iter().any(|&other| {
            other != first && graph.segments[other as usize].radius >= CODOMINANT * segment.radius
        })
    };
    (segment.lateral || forked()).then_some(parent)
}

/// The plant `graph` broken at `breaks`, its wood of toughness
/// `toughness`, in wind on the profile `drag`. A break inside a piece
/// another break already took is skipped. Each break tears as [`tear`]
/// says. A hinged piece swings down the wind about its break's
/// compression side until it rests on the ground or hangs. A break within
/// two diameters of the junction its axis leaves moves to the junction. A stem broken
/// clean through topples: it is turned about its break towards the wind
/// until the line to its wood's centre lies level, then lowered until its
/// broken end rests on the ground, its butt slid off the stump. A branch
/// broken off falls free: it lands level, turned as it tumbled, with its
/// wood's centre below where it was and carried down the wind as far as
/// the wind could push it while it fell. A crown that lands flattens
/// there: the wood under it reaches into the ground.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn apply(
    graph: &PlantGraph,
    breaks: &[Break],
    toughness: Toughness,
    drag: &crate::breakage::Drag,
) -> Damaged {
    let n = graph.segments.len();
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (index, segment) in graph.segments.iter().enumerate() {
        if let Some(parent) = segment.parent {
            children[parent as usize].push(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }
    let mut removed = vec![false; n];
    let mut taken: Vec<(Break, Vec<u32>)> = Vec::new();
    let mut ordered: Vec<Break> = breaks
        .iter()
        .copied()
        .filter(|b| (b.segment as usize) < n)
        .map(|b| Break {
            segment: foot(graph, &children, b.segment),
            ..b
        })
        .collect();
    ordered.sort_by_key(|b| b.segment);
    ordered.dedup_by_key(|b| b.segment);
    for b in ordered {
        if removed[b.segment as usize] {
            continue;
        }
        let mut run = vec![b.segment];
        let mut cursor = 0;
        while cursor < run.len() {
            let at = run[cursor] as usize;
            removed[at] = true;
            run.extend_from_slice(&children[at]);
            cursor += 1;
        }
        run.sort_unstable();
        taken.push((b, run));
    }
    // The standing plant: every segment not taken, and per break a stub
    // of its first segment held at its full radius.
    let mut remap = vec![u32::MAX; n];
    let mut standing = PlantGraph {
        age: graph.age,
        height: graph.height,
        segments: Vec::with_capacity(n),
        organs: Vec::new(),
        structures: graph.structures.clone(),
    };
    for (index, segment) in graph.segments.iter().enumerate() {
        if removed[index] {
            continue;
        }
        remap[index] = u32::try_from(standing.segments.len()).unwrap_or(u32::MAX);
        let mut kept = segment.clone();
        kept.parent = segment.parent.map(|parent| remap[parent as usize]);
        standing.segments.push(kept);
    }
    let mut stubs = Vec::with_capacity(taken.len());
    let mut pieces = Vec::with_capacity(taken.len());
    let mut tears = Vec::with_capacity(taken.len());
    for (b, run) in &taken {
        let first = &graph.segments[b.segment as usize];
        let axis = (first.end - first.start).normalize_or(Vec3::Y);
        let full = (first.end - first.start).length();
        let tension = level_away(b.toward, axis);
        let how = tear(graph, &children, b.segment, toughness);
        tears.push(how);
        let r = first.radius;
        // A branch broken at its foot keeps its collar outside its
        // parent's wood, or, split out, leaves a socket in it; an axis
        // broken along itself keeps a sliver.
        let mut strip = None;
        let reach = match junction(graph, &children, b.segment) {
            Some(parent) => {
                let host = &graph.segments[parent as usize];
                let host_axis = (host.end - host.start).normalize_or(Vec3::Y);
                let sine = axis.cross(host_axis).length().max(0.3);
                let surface = host.radius / sine;
                if how == Tear::Split {
                    let out = (axis - host_axis * axis.dot(host_axis)).normalize_or(Vec3::X);
                    let skin = first.start + out * host.radius;
                    // The scar the strip leaves: pale torn wood down the
                    // stem from the socket, one ragged patch a radius.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let patches = STRIP.round().max(1.0) as u64;
                    for patch in 0..patches {
                        #[allow(clippy::cast_precision_loss)]
                        let down = (patch as f64 + 0.5) * r;
                        stubs.push(End {
                            point: skin - host_axis * down - out * (WOUND_SINK * r),
                            axis: out,
                            radius: STRIP_RADIUS * r,
                            tension: -host_axis,
                            segment: remap[parent as usize],
                            level: level_of(host.order),
                            seed: rng::combine(first.id, 0x0057_4F55_4E44 + patch),
                            torn: WOUND_TORN,
                        });
                    }
                    let mut wood = first.clone();
                    wood.id = rng::combine(first.id, 0x0053_5452_4950);
                    wood.parent = Some(0);
                    wood.lateral = true;
                    wood.start = skin - host_axis * (STRIP * r) - out * (SOCKET * r);
                    wood.end = first.start + axis * surface;
                    wood.radius = STRIP_RADIUS * r;
                    strip = Some(wood);
                    surface - SOCKET * r
                } else {
                    surface + COLLAR * r
                }
            }
            _ => 0.25 * r,
        }
        .clamp(HOLD * 2.0, full.max(HOLD * 2.0));
        let tip = first.start + axis * reach;
        let parent = first.parent.map(|parent| remap[parent as usize]);
        let stub = u32::try_from(standing.segments.len()).unwrap_or(u32::MAX);
        let mut kept = first.clone();
        kept.parent = parent;
        kept.end = tip;
        standing.segments.push(kept);
        let mut hold = first.clone();
        hold.id = rng::combine(first.id, 0x5354_5542);
        hold.parent = Some(stub);
        hold.lateral = false;
        hold.start = tip;
        hold.end = tip + axis * HOLD;
        standing.segments.push(hold);
        let level = level_of(first.order);
        stubs.push(End {
            point: tip,
            axis,
            radius: r,
            tension,
            segment: stub,
            level,
            seed: first.id,
            torn: 1.0,
        });
        let off = Fall {
            axis,
            tension,
            level,
            tear: how,
            strip,
        };
        pieces.push(fallen(graph, b, run, &off, drag));
    }
    for organ in &graph.organs {
        match organ.segment {
            Some(segment) if removed[segment as usize] => {}
            Some(segment) => {
                let mut kept = organ.clone();
                kept.segment = Some(remap[segment as usize]);
                standing.organs.push(kept);
            }
            None => standing.organs.push(organ.clone()),
        }
    }
    standing.height = standing
        .segments
        .iter()
        .map(|segment| segment.end.y.max(segment.start.y))
        .fold(0.0, f64::max);
    Damaged {
        standing,
        stubs,
        pieces,
        tears,
    }
}

fn level_of(order: u16) -> u8 {
    match order {
        0 => 0,
        1 => 1,
        _ => 2,
    }
}

/// The level direction opposite `toward`, square to `axis`: where a
/// section bent by wind blowing towards `toward` is in tension.
fn level_away(toward: Vec3, axis: Vec3) -> Vec3 {
    let away = Vec3::new(-toward.x, 0.0, -toward.z);
    let square = away - axis * axis.dot(away);
    if square.length() > 1.0e-6 {
        return square.normalize_or(Vec3::X);
    }
    // Wind along the axis: any side.
    let side = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    (side - axis * axis.dot(side)).normalize_or(Vec3::X)
}

/// How a piece comes off: its broken segment's axis and tension side, its
/// wood's level, how it tore, and the strip of stem wood a split takes.
struct Fall {
    axis: Vec3,
    tension: Vec3,
    level: u8,
    tear: Tear,
    strip: Option<GraphSegment>,
}

/// How far the wind at `wind` m/s at 10 m, on the profile `drag`, carries
/// a piece of wood mass `mass` kg and drag area `area` m² (`Cd A`) down
/// the wind while it falls from `height` m, m: starting still, it is
/// pushed by the drag of the wind past it, `m dv/dt = ½ ρ Cd A (U − v)²`,
/// at the mean wind over its fall (the wind at half its height). At most
/// its height: what falls lands within a tree's height of it.
fn drift(mass: f64, area: f64, height: f64, wind: f64, drag: &crate::breakage::Drag) -> f64 {
    if wind <= 0.0 || height <= 0.0 || area <= 0.0 || mass <= 0.0 {
        return 0.0;
    }
    let u = wind * drag.profile(0.5 * height).max(0.0);
    if u <= 0.0 {
        return 0.0;
    }
    let t = math::sqrt(2.0 * height / GRAVITY);
    // dv/dt = (U − v)² / (U τ) with τ = m / (½ ρ Cd A U): v = U s/(1+s),
    // s = t/τ, and the distance U (t − τ ln(1 + t/τ)).
    let tau = mass / (0.5 * AIR * area * u);
    let distance = u * (t - tau * math::ln(1.0 + t / tau));
    distance.clamp(0.0, height)
}

/// The piece `run` (segments in graph order, its first the broken one)
/// taken out of `graph` and laid on the ground as `fall` says.
#[allow(clippy::too_many_lines)]
fn fallen(
    graph: &PlantGraph,
    b: &Break,
    run: &[u32],
    fall: &Fall,
    drag: &crate::breakage::Drag,
) -> Piece {
    let axis = fall.axis;
    let mut index_of = std::collections::HashMap::with_capacity(run.len());
    for (new, old) in run.iter().enumerate() {
        index_of.insert(*old, u32::try_from(new).unwrap_or(u32::MAX));
    }
    let first = &graph.segments[b.segment as usize];
    let pivot = first.start;
    let r = first.radius;
    let mut segments: Vec<GraphSegment> = run
        .iter()
        .map(|old| {
            let mut segment = graph.segments[*old as usize].clone();
            segment.parent = segment
                .parent
                .and_then(|parent| index_of.get(&parent).copied());
            if segment.parent.is_none() {
                segment.lateral = false;
            }
            segment
        })
        .collect();
    if let Some(strip) = &fall.strip {
        segments.push(strip.clone());
    }
    let mut organs: Vec<GraphOrgan> = graph
        .organs
        .iter()
        .filter_map(|organ| {
            let new = index_of.get(&organ.segment?)?;
            let mut moved = organ.clone();
            moved.segment = Some(*new);
            Some(moved)
        })
        .collect();
    // Its wood's volume and where it sits, its weight and its drag.
    let mut volume = 0.0;
    let mut centre = Vec3::ZERO;
    let mut area = 0.0;
    for segment in &segments {
        let length = (segment.end - segment.start).length();
        let v = math::PI * segment.radius * segment.radius * length;
        volume += v;
        centre += (segment.start + segment.end) * (0.5 * v);
        area += WOOD_CD * 2.0 * segment.radius * length;
    }
    let centre = if volume > 0.0 {
        centre * (1.0 / volume)
    } else {
        pivot + axis
    };
    let leaves: f64 = organs
        .iter()
        .map(|organ| 0.5 * organ.size * organ.size)
        .sum();
    let mass = GREEN * volume + LEAF_MASS * leaves;
    area += LEAF_CD * leaves;
    let length = (centre - pivot).normalize_or(axis);
    let toward = Vec3::new(b.toward.x, 0.0, b.toward.z).normalize_or(Vec3::X);
    let lean = Vec3::new(length.x, 0.0, length.z);
    // Turned about `about` by `turn`, then moved by `shift`.
    let (about, turn, shift) = if fall.tear == Tear::Hinged {
        // Swung down about the compression side's fibres, down the wind
        // and under its own weight, until its wood meets the ground or it
        // hangs.
        let about = pivot - fall.tension * r;
        let swing = toward - Vec3::Y * 0.5;
        let hinge = length
            .cross(swing)
            .normalize_or(Vec3::Y.cross(toward).normalize_or(Vec3::Z));
        // It bears on its own axis, to its tip, and on its thick wood.
        let mut on_axis = vec![false; segments.len()];
        on_axis[0] = true;
        for index in 1..segments.len() {
            if let Some(parent) = segments[index].parent {
                on_axis[index] = !segments[index].lateral && on_axis[parent as usize];
            }
        }
        let main: Vec<&GraphSegment> = segments
            .iter()
            .zip(&on_axis)
            .filter(|(segment, axis)| **axis || segment.radius >= 0.25 * r)
            .map(|(segment, _)| segment)
            .collect();
        let mut chosen = Turn::about(hinge, 0.0);
        let mut lowest = length.dot(-Vec3::Y);
        for step in 1..=90 {
            let angle = f64::from(step) * math::PI / 90.0;
            let trial = Turn::about(hinge, angle);
            let down = trial.apply(length).dot(-Vec3::Y);
            if down < lowest - 1.0e-9 {
                break;
            }
            let touches = main.iter().any(|segment| {
                let y = |p: Vec3| about.y + trial.apply(p - about).y;
                y(segment.start).min(y(segment.end)) < segment.radius
            });
            chosen = trial;
            lowest = down;
            if touches {
                break;
            }
        }
        (about, chosen, Vec3::ZERO)
    } else if fall.level == 0 {
        // A stem topples down the wind, its own way round where it
        // already leans, and its butt slides off the stump down the fall.
        let target = (toward * 2.0 + lean.normalize_or(toward)).normalize_or(toward);
        let turn = Turn::between(length, target);
        let shift = Vec3::Y * (r - pivot.y) + target * (2.0 * r);
        (pivot, turn, shift)
    } else {
        // A branch falls free and tumbles, landing level with its wood's
        // centre below where it was, carried down the wind.
        let spin = (rng::unit(rng::combine(first.id, 0x5455_4D42_4C45)) * 2.0 - 1.0) * TUMBLE;
        let target = Turn::about(Vec3::Y, spin)
            .apply(toward * 2.0 + lean.normalize_or(toward))
            .normalize_or(toward);
        let turn = Turn::between(length, target);
        let landed = pivot + turn.apply(centre - pivot);
        let carried = drift(mass, area, centre.y, b.wind, drag);
        let to = centre + toward * carried;
        let shift = Vec3::new(to.x - landed.x, r - pivot.y, to.z - landed.z);
        (pivot, turn, shift)
    };
    let place = |p: Vec3| about + turn.apply(p - about) + shift;
    for segment in &mut segments {
        segment.start = place(segment.start);
        segment.end = place(segment.end);
        segment.left = turn.apply(segment.left);
    }
    for organ in &mut organs {
        organ.position = place(organ.position);
        organ.heading = turn.apply(organ.heading);
        organ.left = turn.apply(organ.left);
    }
    let base = &segments[0];
    let height = segments
        .iter()
        .map(|segment| segment.end.y.max(segment.start.y))
        .fold(0.0, f64::max);
    let end = End {
        point: base.start,
        axis: turn.apply(-axis),
        radius: r,
        tension: turn.apply(fall.tension),
        segment: 0,
        level: fall.level,
        seed: rng::combine(base.id, 0x5049_4543),
        torn: 1.0,
    };
    Piece {
        graph: PlantGraph {
            age: graph.age,
            height,
            segments,
            organs,
            structures: graph.structures.clone(),
        },
        end,
    }
}

/// A rotation, as the shortest turn between two directions.
#[derive(Debug, Clone, Copy)]
struct Turn {
    axis: Vec3,
    cos: f64,
    sin: f64,
}

impl Turn {
    fn between(from: Vec3, to: Vec3) -> Self {
        let (from, to) = (from.normalize_or(Vec3::Y), to.normalize_or(Vec3::Y));
        let cross = from.cross(to);
        let sin = cross.length();
        let cos = from.dot(to);
        if sin < 1.0e-9 {
            if cos > 0.0 {
                return Self {
                    axis: Vec3::Y,
                    cos: 1.0,
                    sin: 0.0,
                };
            }
            let side = if from.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
            return Self {
                axis: from.cross(side).normalize_or(Vec3::X),
                cos: -1.0,
                sin: 0.0,
            };
        }
        Self {
            axis: cross * (1.0 / sin),
            cos,
            sin,
        }
    }

    /// A turn of `angle` radians about `axis`, right-handed.
    fn about(axis: Vec3, angle: f64) -> Self {
        Self {
            axis: axis.normalize_or(Vec3::Y),
            cos: math::cos(angle),
            sin: math::sin(angle),
        }
    }

    /// Rodrigues' formula.
    fn apply(&self, v: Vec3) -> Vec3 {
        let k = self.axis;
        v * self.cos + k.cross(v) * self.sin + k * (k.dot(v) * (1.0 - self.cos))
    }
}

/// How tough a wood is, from the Wood Handbook's (FPL-GTR-282, 2021,
/// table 5-11a) work to maximum load of clear green wood against its
/// specific gravity `G`: `148 G^1.22` kJ/m³ for softwoods, `229 G^1.52`
/// for hardwoods. Brash wood, light for its class, fails "abruptly without
/// splintering" (the Handbook's glossary); tough wood tears out in long
/// splinters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Toughness {
    /// Work to maximum load, kJ/m³.
    pub work: f64,
}

impl Toughness {
    /// The work to maximum load of the middling wood the reach of
    /// splinters is scaled against, kJ/m³: the softwood function at
    /// `G` 0.45.
    pub const MIDDLING: f64 = 55.7;

    /// The toughness of wood of class `class` (`softwood` or `hardwood`)
    /// and basic density `basic`, kg/m³.
    #[must_use]
    pub fn of(class: &str, basic: f64) -> Self {
        let gravity = (basic / 1000.0).max(0.05);
        let work = if class == "softwood" {
            148.0 * math::pow(gravity, 1.22)
        } else {
            229.0 * math::pow(gravity, 1.52)
        };
        Self { work }
    }

    /// The toughness of a species with characters `traits`: its class of
    /// wood and its basic density, a middling 450 kg/m³ where unstated.
    #[must_use]
    pub fn of_traits(traits: &serde_json::Map<String, serde_json::Value>) -> Self {
        let basic = traits
            .get("wood_density_basic_kg_m3")
            .and_then(crate::checks::Stated::of)
            .map_or(crate::motion::GENERIC[0].1, |stated| {
                stated
                    .typical
                    .unwrap_or(f64::midpoint(stated.min, stated.max))
            });
        Self::of(crate::motion::wood_class(traits), basic)
    }

    /// How far a break's longest splinters reach past its section, in the
    /// wood's diameters: one for the middling wood, more for tougher, held
    /// between 0.35 and 1.6.
    #[must_use]
    pub fn reach(&self) -> f64 {
        (self.work / Self::MIDDLING).sqrt().clamp(0.35, 1.6)
    }

    /// The chance that an axis broken along itself hinges ([`Tear::Hinged`]):
    /// `work / (work + MIDDLING)`, an even chance for the middling wood,
    /// more for tougher. Tough green wood bends and tears through its
    /// tension side before its compression side lets go (a greenstick
    /// break); brash wood snaps clean across (the Handbook's glossary).
    /// The odds themselves are assumed: no source counts them.
    #[must_use]
    pub fn hinge(&self) -> f64 {
        self.work / (self.work + Self::MIDDLING)
    }
}

/// The colours of freshly broken wood, linear RGB: its sapwood and, if it
/// has a distinct one, its heartwood and the share of the radius it fills.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grain {
    pub sapwood: [f32; 3],
    pub heartwood: Option<([f32; 3], f64)>,
    /// The bark's colour, for the rim left on the stub.
    pub bark: [f32; 3],
}

impl Grain {
    /// Fresh wood, a light yellowish tan, where nothing better is known.
    pub const SAPWOOD: [f32; 3] = [0.52, 0.36, 0.17];
}

/// The torn end `end` of wood of toughness `toughness`, coloured `grain`,
/// with about `edge` m between its vertices round the section. Green wood
/// tears apart in bundles of fibres: the section is cut into cells, rings
/// out from the centre and sectors round it, and each cell stands out of
/// the section as a prism to its own height, its top torn aslant. On the
/// tension side cells stand tallest and a few long slivers peel out
/// further; on the compression side the wood is crushed nearly square. The
/// bark round it is peeled back a little. Flat shaded, every face its own
/// vertices.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn fracture(end: &End, toughness: Toughness, grain: &Grain, edge: f64) -> Mesh {
    let mut out = Mesh::default();
    let r = end.radius.max(1.0e-4);
    let reach = toughness.reach() * 2.0 * r * end.torn.clamp(0.0, 1.0);
    let axis = end.axis.normalize_or(Vec3::Y);
    let u = (end.tension - axis * axis.dot(end.tension)).normalize_or(Vec3::X);
    let v = axis.cross(u);
    let draw = |salt: u64| rng::unit(rng::combine(end.seed, salt));
    // Cells about `width` across, finer than the tube's ring: at most 6
    // rings and 40 sectors on the outer ring.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let outer = ((2.0 * math::PI * r / (0.6 * edge).max(1.0e-4)).ceil() as usize).clamp(8, 40);
    #[allow(clippy::cast_precision_loss)]
    let width = 2.0 * math::PI * r / outer as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings = ((r / width).round() as usize).clamp(1, 6);
    let towards = |a: f64| u * math::cos(a) + v * math::sin(a);
    let wood_colour = |rho: f64, salt: u64| -> [f32; 4] {
        let base = match grain.heartwood {
            Some((colour, share)) if rho < share => colour,
            _ => grain.sapwood,
        };
        #[allow(clippy::cast_possible_truncation)]
        let shade = (0.8 + 0.3 * draw(salt ^ 0xC010)) as f32;
        [base[0] * shade, base[1] * shade, base[2] * shade, 1.0]
    };
    let triangle = |out: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, colour: [f32; 4]| {
        let normal = (b - a).cross(c - a).normalize_or(axis);
        let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
        for p in [a, b, c] {
            out.push(Vertex {
                position: p,
                normal,
                uv: [0.0, 0.0],
                color: colour,
                born: 0.0,
                shed: None,
                level: end.level,
                segment: end.segment,
            });
        }
        out.indices.extend_from_slice(&[base, base + 1, base + 2]);
    };
    // A quad a b c d, counter-clockwise seen from outside.
    let quad = |out: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, d: Vec3, colour: [f32; 4]| {
        triangle(out, a, b, c, colour);
        triangle(out, a, c, d, colour);
    };
    let floor = end.point - axis * (0.02 * r);
    for ring in 0..rings {
        #[allow(clippy::cast_precision_loss)]
        let (inner, outer_radius) = (
            r * ring as f64 / rings as f64,
            r * (ring + 1) as f64 / rings as f64,
        );
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let sectors = ((2.0 * math::PI * outer_radius / width).round() as usize).max(3);
        let rho = (inner + outer_radius) / (2.0 * r);
        let turn = draw(0x7000 | ring as u64) * 2.0 * math::PI;
        for sector in 0..sectors {
            #[allow(clippy::cast_precision_loss)]
            let a0 = turn + 2.0 * math::PI * sector as f64 / sectors as f64;
            #[allow(clippy::cast_precision_loss)]
            let a1 = turn + 2.0 * math::PI * (sector + 1) as f64 / sectors as f64;
            let middle = f64::midpoint(a0, a1);
            let salt = (ring as u64) << 16 | sector as u64;
            let tension = f64::midpoint(1.0, math::cos(middle));
            // Taller towards the tension side and the outside, each
            // bundle torn to its own length; some pull out much further.
            let mut height = reach
                * (0.04 + 0.45 * tension * tension * (0.5 + 0.5 * rho))
                * (0.3 + 1.0 * draw(salt));
            if draw(salt ^ 0xF1B2) < 0.1 + 0.4 * tension * tension {
                height += reach * tension * (0.3 + 0.7 * draw(salt ^ 0x5911));
            }
            // Its top is torn aslant: one corner higher.
            let lean = height * (0.15 + 0.5 * draw(salt ^ 0x1EA4));
            let corner = |a: f64, radius: f64| end.point + towards(a) * radius;
            let (p0, p1, p2, p3) = (
                corner(a0, inner),
                corner(a1, inner),
                corner(a1, outer_radius),
                corner(a0, outer_radius),
            );
            let tilt = if draw(salt ^ 0x7111) < 0.5 { 1.0 } else { -1.0 };
            let top = |p: Vec3, along: f64| p + axis * (height - lean * (0.5 + 0.5 * tilt * along));
            let (t0, t1, t2, t3) = (top(p0, -1.0), top(p1, 1.0), top(p2, 1.0), top(p3, -1.0));
            let low = |p: Vec3| p - end.point + floor;
            let colour = wood_colour(rho, salt);
            let side = wood_colour(rho, salt ^ 0x51DE);
            // Top, then its walls: outer, inner, and its two sides.
            quad(&mut out, t0, t3, t2, t1, colour);
            if ring + 1 == rings {
                // The outside: bark below where it peeled, wood above.
                let peel = 0.1 + 0.6 * draw(salt ^ 0x2000);
                let (m3, m2) = (
                    low(p3) + (t3 - low(p3)) * peel,
                    low(p2) + (t2 - low(p2)) * peel,
                );
                let bark = [grain.bark[0], grain.bark[1], grain.bark[2], 1.0];
                let out3 = |p: Vec3| p + (p - end.point - axis * axis.dot(p - end.point)) * 0.04;
                quad(
                    &mut out,
                    out3(low(p3)),
                    out3(low(p2)),
                    out3(m2),
                    out3(m3),
                    bark,
                );
                quad(&mut out, m3, m2, t2, t3, side);
            } else {
                quad(&mut out, low(p3), low(p2), t2, t3, side);
            }
            if ring > 0 {
                quad(&mut out, low(p1), low(p0), t0, t1, side);
            }
            quad(&mut out, low(p0), low(p3), t3, t0, side);
            quad(&mut out, low(p2), low(p1), t1, t2, side);
        }
    }
    // A few long slivers peeling out of the tension side: flat strips
    // narrowing to a ragged tip, leaning out.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let slivers = (1.0 + 3.0 * toughness.reach()) as usize;
    for sliver in 0..slivers {
        let salt = 0x4000 | sliver as u64;
        let a = (draw(salt) - 0.5) * 1.4;
        let around = towards(a);
        let at = end.point + around * (r * (0.8 + 0.18 * draw(salt ^ 1)));
        let lean = around * (0.1 + 0.25 * draw(salt ^ 2));
        let length = reach * (0.8 + 0.8 * draw(salt ^ 3)) * (0.6 + 0.4 * math::cos(a));
        let half = r * (0.05 + 0.07 * draw(salt ^ 4));
        let thick = half * 0.35;
        let up = (axis + lean).normalize_or(axis);
        let across = up.cross(around).normalize_or(v);
        let tip = at + up * length + across * (half * (draw(salt ^ 6) - 0.5));
        let (b0, b1) = (at + across * half, at - across * half);
        let (b2, b3) = (b1 - around * thick, b0 - around * thick);
        let colour = wood_colour(0.95, salt ^ 5);
        // Outer face, inner face, and its two edges.
        triangle(&mut out, b0, b1, tip, colour);
        triangle(&mut out, b2, b3, tip, colour);
        triangle(&mut out, b1, b2, tip, colour);
        triangle(&mut out, b3, b0, tip, colour);
    }
    out
}

/// Every vertex of `more` added to `mesh`.
pub fn append(mesh: &mut Mesh, more: &Mesh) {
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    mesh.positions.extend_from_slice(&more.positions);
    mesh.normals.extend_from_slice(&more.normals);
    mesh.uvs.extend_from_slice(&more.uvs);
    mesh.colors.extend_from_slice(&more.colors);
    mesh.births.extend_from_slice(&more.births);
    mesh.sheds.extend_from_slice(&more.sheds);
    mesh.levels.extend_from_slice(&more.levels);
    // Segments may run short of the vertices; pad before extending.
    if !more.segments.is_empty() || !mesh.segments.is_empty() {
        mesh.segments
            .resize(first as usize, crate::mesh::NO_SEGMENT);
        mesh.segments.extend_from_slice(&more.segments);
        mesh.segments
            .resize(mesh.positions.len(), crate::mesh::NO_SEGMENT);
    }
    mesh.indices
        .extend(more.indices.iter().map(|index| index + first));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breakage::Drag;

    /// Wood so brash it never hinges, and so tough it always does.
    const BRASH: Toughness = Toughness { work: 0.0 };
    const TOUGH: Toughness = Toughness { work: 1.0e12 };

    /// A stem of four segments up the Y axis with a level branch of two
    /// from the top of its second, a leaf on each.
    fn tree() -> PlantGraph {
        let segment = |id: u64,
                       parent: Option<u32>,
                       lateral: bool,
                       order: u16,
                       start: Vec3,
                       end: Vec3,
                       radius: f64| GraphSegment {
            id,
            parent,
            lateral,
            order,
            start,
            end,
            radius,
            born: 0.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
            young: 0.0,
        };
        let y = |h: f64| Vec3::Y * h;
        let segments = vec![
            segment(1, None, false, 0, y(0.0), y(1.0), 0.1),
            segment(2, Some(0), false, 0, y(1.0), y(2.0), 0.09),
            segment(3, Some(1), false, 0, y(2.0), y(3.0), 0.08),
            segment(4, Some(2), false, 0, y(3.0), y(4.0), 0.07),
            segment(5, Some(1), true, 1, y(2.0), y(2.0) + Vec3::X, 0.04),
            segment(
                6,
                Some(4),
                false,
                1,
                y(2.0) + Vec3::X,
                y(2.0) + Vec3::X * 2.0,
                0.03,
            ),
        ];
        let organ = |id: u64, segment: u32, position: Vec3| GraphOrgan {
            id,
            organ: 0,
            segment: Some(segment),
            position,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 0.1,
            born: 0.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        };
        PlantGraph {
            age: 10.0,
            height: 4.0,
            segments,
            organs: vec![organ(1, 3, y(4.0)), organ(2, 5, y(2.0) + Vec3::X * 2.0)],
            structures: Vec::new(),
        }
    }

    #[test]
    fn a_branch_broken_at_its_foot_leaves_a_collar_and_falls_whole() {
        let graph = tree();
        let damaged = apply(
            &graph,
            &[Break {
                segment: 4,
                toward: Vec3::Z,
                wind: 0.0,
            }],
            BRASH,
            &Drag::default(),
        );
        let standing = &damaged.standing;
        // The stem's four, the stub and the segment that holds its end.
        assert_eq!(standing.segments.len(), 6);
        assert_eq!(standing.organs.len(), 1);
        let stub = &damaged.stubs[0];
        let host = 0.09;
        let reach = host / 1.0 + COLLAR * 0.04;
        assert!((stub.point - (Vec3::Y * 2.0 + Vec3::X * reach)).length() < 1.0e-9);
        assert!((stub.radius - 0.04).abs() < 1.0e-12);
        let ends = standing.end_radii();
        assert!(
            (ends[stub.segment as usize] - 0.04).abs() < 1.0e-12,
            "the stub keeps its radius"
        );
        // The tension side faces the wind.
        assert!((stub.tension - (-Vec3::Z)).length() < 1.0e-9);
        let piece = &damaged.pieces[0];
        assert_eq!(piece.graph.segments.len(), 2);
        assert_eq!(piece.graph.organs.len(), 1);
        assert!(piece.graph.segments[0].parent.is_none());
        assert_eq!(piece.graph.segments[1].parent, Some(0));
        // Its broken end rests on the ground.
        let base = &piece.graph.segments[0];
        assert!((base.start.y - base.radius).abs() < 1.0e-9);
    }

    #[test]
    fn a_stem_snapped_along_itself_takes_everything_above() {
        let graph = tree();
        let damaged = apply(
            &graph,
            &[
                Break {
                    segment: 2,
                    toward: Vec3::X,
                    wind: 0.0,
                },
                // Inside the piece already taken: skipped.
                Break {
                    segment: 3,
                    toward: Vec3::X,
                    wind: 0.0,
                },
            ],
            BRASH,
            &Drag::default(),
        );
        assert_eq!(damaged.tears, vec![Tear::Clean]);
        assert_eq!(damaged.pieces.len(), 1);
        assert_eq!(damaged.pieces[0].graph.segments.len(), 2);
        // Two below, the branch's two, the stub and its hold.
        assert_eq!(damaged.standing.segments.len(), 6);
        assert!(damaged.standing.height < 2.1);
        // The piece lies level, down the wind.
        let piece = &damaged.pieces[0].graph;
        let span = piece.segments[1].end - piece.segments[0].start;
        assert!(span.y.abs() < 0.2 * span.length(), "{span:?}");
        assert!(span.x > 0.0);
    }

    #[test]
    fn a_branch_half_its_stems_thickness_splits_out_with_a_strip() {
        let mut graph = tree();
        graph.segments[4].radius = 0.06;
        let damaged = apply(
            &graph,
            &[Break {
                segment: 4,
                toward: Vec3::Z,
                wind: 0.0,
            }],
            BRASH,
            &Drag::default(),
        );
        assert_eq!(damaged.tears, vec![Tear::Split]);
        // A scar down the stem below the junction, and the socket's end.
        assert_eq!(damaged.stubs.len(), 5);
        for wound in &damaged.stubs[..4] {
            assert!(wound.point.y < 2.0, "{:?}", wound.point);
            assert!(
                (wound.axis - Vec3::X).length() < 1.0e-9,
                "it faces out of the stem"
            );
        }
        assert!(damaged.stubs[3].point.y < 2.0 - 3.0 * 0.06);
        let socket = &damaged.stubs[4];
        assert!(
            socket.point.x < 0.09,
            "the socket sits inside the stem's surface"
        );
        // The piece takes the strip of stem wood, running down the stem.
        let piece = &damaged.pieces[0].graph;
        assert_eq!(piece.segments.len(), 3);
        let strip = &piece.segments[2];
        assert_eq!(strip.parent, Some(0));
        assert!((strip.radius - STRIP_RADIUS * 0.06).abs() < 1.0e-12);
    }

    #[test]
    fn a_break_just_past_a_junction_falls_at_the_junction() {
        // The branch's second segment, one radius past its foot... is
        // a metre on: too far, it breaks where it is. Shorten its first.
        let mut graph = tree();
        graph.segments[4].end = Vec3::Y * 2.0 + Vec3::X * 0.1;
        graph.segments[5].start = graph.segments[4].end;
        let children = {
            let mut children = vec![Vec::new(); graph.segments.len()];
            for (index, segment) in graph.segments.iter().enumerate() {
                if let Some(parent) = segment.parent {
                    children[parent as usize].push(u32::try_from(index).unwrap());
                }
            }
            children
        };
        assert_eq!(foot(&graph, &children, 5), 4);
        assert_eq!(foot(&tree(), &children, 5), 5);
    }

    #[test]
    fn tough_wood_hinges_and_the_top_hangs_from_its_stump() {
        let graph = tree();
        let damaged = apply(
            &graph,
            &[Break {
                segment: 2,
                toward: Vec3::X,
                wind: 0.0,
            }],
            TOUGH,
            &Drag::default(),
        );
        assert_eq!(damaged.tears, vec![Tear::Hinged]);
        let piece = &damaged.pieces[0].graph;
        let stub = &damaged.stubs[0];
        // Its butt stays at the stump, against its compression side.
        let butt = piece.segments[0].start;
        assert!(
            (butt - stub.point).length() < 3.0 * 0.09,
            "{butt:?} {:?}",
            stub.point
        );
        // Two metres long from a break two metres up: it swings over
        // down the wind until its tip meets the ground, nearly hanging.
        let tip = piece.segments[1].end;
        assert!(tip.y < 0.2 && tip.y > -0.01, "{tip:?}");
        assert!(tip.x > 0.2, "down the wind: {tip:?}");
        assert!(tip.z.abs() < 1.0e-9);
    }

    #[test]
    fn a_branch_falls_free_and_the_wind_carries_it() {
        let graph = tree();
        let at = |wind: f64| {
            let damaged = apply(
                &graph,
                &[Break {
                    segment: 4,
                    toward: Vec3::Z,
                    wind,
                }],
                BRASH,
                &Drag::default(),
            );
            let piece = &damaged.pieces[0].graph;
            let mut volume = 0.0;
            let mut centre = Vec3::ZERO;
            for segment in &piece.segments {
                let v = segment.radius * segment.radius * (segment.end - segment.start).length();
                volume += v;
                centre += (segment.start + segment.end) * (0.5 * v);
            }
            centre * (1.0 / volume)
        };
        // Still air: it lands under where its wood was.
        let still = at(0.0);
        let was = Vec3::new(1.0, 2.0, 0.0);
        assert!(
            (still.x - was.x).abs() < 0.3 && still.z.abs() < 0.3,
            "{still:?}"
        );
        assert!(still.y < 0.2);
        // A gale carries it down the wind, no further than it fell.
        let gale = at(30.0);
        assert!(gale.z > still.z + 0.5, "{gale:?} {still:?}");
        assert!(gale.z - still.z <= 2.0 + 1.0e-9);
    }

    #[test]
    fn the_wind_carries_light_leafy_wood_further_than_heavy() {
        let drag = Drag::default();
        let light = drift(5.0, 2.0, 10.0, 25.0, &drag);
        let heavy = drift(500.0, 2.0, 10.0, 25.0, &drag);
        assert!(light > heavy && heavy > 0.0, "{light} {heavy}");
        assert!(light <= 10.0);
        assert!(drift(5.0, 2.0, 10.0, 0.0, &drag).abs() < 1.0e-12);
    }

    #[test]
    fn tough_wood_splinters_further_than_brash() {
        let brash = Toughness::of("softwood", 300.0);
        let tough = Toughness::of("hardwood", 700.0);
        assert!(tough.reach() > brash.reach());
        let middling = Toughness::of("softwood", 450.0);
        assert!((middling.reach() - 1.0).abs() < 0.01);
        let end = End {
            point: Vec3::ZERO,
            axis: Vec3::Y,
            radius: 0.2,
            tension: Vec3::X,
            segment: 3,
            level: 0,
            seed: 7,
            torn: 1.0,
        };
        let grain = Grain {
            sapwood: Grain::SAPWOOD,
            heartwood: None,
            bark: [0.2, 0.15, 0.1],
        };
        let mesh = fracture(&end, tough, &grain, 0.05);
        assert!(mesh.triangle_count() > 50);
        // The tension side stands higher than the compression side.
        let side = |sign: f64| {
            mesh.positions
                .iter()
                .filter(|p| f64::from(p[0]) * sign > 0.1)
                .map(|p| f64::from(p[1]))
                .fold(0.0, f64::max)
        };
        assert!(side(1.0) > 2.0 * side(-1.0), "{} {}", side(1.0), side(-1.0));
        assert!(mesh.segments.iter().all(|s| *s == 3));
        // Nothing reaches below the section by more than the rim's lip.
        assert!(mesh.positions.iter().all(|p| p[1] > -0.02));
    }
}
