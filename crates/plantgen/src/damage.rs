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
/// other way).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Break {
    pub segment: u32,
    pub toward: Vec3,
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
}

/// How far past the wood the stub of a branch broken at its foot reaches
/// beyond its parent's surface, in its own radii: the branch collar a torn
/// branch leaves.
const COLLAR: f64 = 0.5;

/// The length of the segment that keeps a stub's end its full radius, m
/// (the tube of a tip narrows over its last segment).
const HOLD: f64 = 0.001;

/// The plant `graph` broken at `breaks`. A break inside a piece another
/// break already took is skipped. Each piece is laid on the ground: turned
/// about its break towards the wind until the line to its wood's centre
/// lies level, then lowered until its broken end rests on the ground (its
/// crown flattened there: the wood under it reaches into the ground).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn apply(graph: &PlantGraph, breaks: &[Break]) -> Damaged {
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
    for (b, run) in &taken {
        let first = &graph.segments[b.segment as usize];
        let axis = (first.end - first.start).normalize_or(Vec3::Y);
        let full = (first.end - first.start).length();
        let tension = level_away(b.toward, axis);
        // A branch broken at its foot keeps its collar outside its
        // parent's wood; an axis broken along itself keeps a sliver.
        let reach = match first.parent {
            Some(parent) if first.lateral => {
                let host = &graph.segments[parent as usize];
                let host_axis = (host.end - host.start).normalize_or(Vec3::Y);
                let sine = axis.cross(host_axis).length().max(0.3);
                host.radius / sine + COLLAR * first.radius
            }
            _ => 0.25 * first.radius,
        }
        .min(full.max(HOLD * 2.0));
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
            radius: first.radius,
            tension,
            segment: stub,
            level,
            seed: first.id,
        });
        pieces.push(fallen(graph, b, run, axis, tension, level));
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

/// The piece `run` (segments in graph order, its first the broken one)
/// taken out of `graph` and laid on the ground.
fn fallen(
    graph: &PlantGraph,
    b: &Break,
    run: &[u32],
    axis: Vec3,
    tension: Vec3,
    level: u8,
) -> Piece {
    let mut index_of = std::collections::HashMap::with_capacity(run.len());
    for (new, old) in run.iter().enumerate() {
        index_of.insert(*old, u32::try_from(new).unwrap_or(u32::MAX));
    }
    let pivot = graph.segments[b.segment as usize].start;
    // Where its wood's volume sits, to lay its length level.
    let mut volume = 0.0;
    let mut centre = Vec3::ZERO;
    for old in run {
        let segment = &graph.segments[*old as usize];
        let v = segment.radius * segment.radius * (segment.end - segment.start).length();
        volume += v;
        centre += (segment.start + segment.end) * (0.5 * v);
    }
    let centre = if volume > 0.0 {
        centre * (1.0 / volume)
    } else {
        pivot + axis
    };
    let length = (centre - pivot).normalize_or(axis);
    let toward = Vec3::new(b.toward.x, 0.0, b.toward.z).normalize_or(Vec3::X);
    // Down the wind, its own way round where it already leans.
    let lean = Vec3::new(length.x, 0.0, length.z);
    let target = (toward * 2.0 + lean.normalize_or(toward)).normalize_or(toward);
    let turn = Turn::between(length, target);
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
            segment.start = pivot + turn.apply(segment.start - pivot);
            segment.end = pivot + turn.apply(segment.end - pivot);
            segment.left = turn.apply(segment.left);
            segment
        })
        .collect();
    let mut organs: Vec<GraphOrgan> = graph
        .organs
        .iter()
        .filter_map(|organ| {
            let new = index_of.get(&organ.segment?)?;
            let mut moved = organ.clone();
            moved.segment = Some(*new);
            moved.position = pivot + turn.apply(organ.position - pivot);
            moved.heading = turn.apply(organ.heading);
            moved.left = turn.apply(organ.left);
            Some(moved)
        })
        .collect();
    // Lowered until its broken end rests on the ground; its crown
    // flattens there, the wood under it pressed into the ground.
    // Its butt slides off the stump it broke from, down the fall.
    let base = &segments[0];
    let drop = Vec3::Y * (base.radius - base.start.y) + target * (2.0 * base.radius);
    for segment in &mut segments {
        segment.start += drop;
        segment.end += drop;
    }
    for organ in &mut organs {
        organ.position += drop;
    }
    let first = &segments[0];
    let height = segments
        .iter()
        .map(|segment| segment.end.y.max(segment.start.y))
        .fold(0.0, f64::max);
    let end = End {
        point: first.start,
        axis: turn.apply(-axis),
        radius: first.radius,
        tension: turn.apply(tension),
        segment: 0,
        level,
        seed: rng::combine(first.id, 0x5049_4543),
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
    let reach = toughness.reach() * 2.0 * r;
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
            }],
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
                },
                // Inside the piece already taken: skipped.
                Break {
                    segment: 3,
                    toward: Vec3::X,
                },
            ],
        );
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
