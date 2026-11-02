//! A tree felled with a saw (plant motion S5, slice B3).
//!
//! [`fell`] cuts a grown, drawn [`PlantGraph`] through its stem the way a
//! sawyer fells a tree with the conventional undercut of the USDA Forest
//! Service's *Chain Saw and Crosscut Saw Training Course* (Missoula
//! Technology and Development Center, 0667-2C01, 2006, chapter 4): a level
//! cut a third of the way into the stem on the side it is to fall, a cut
//! sloping down to meet it at 45 degrees so the face does not close until
//! the tree is committed, and a level back cut from the other side a little
//! above the notch's floor that stops short of the notch, leaving a strip of
//! holding wood. "Undercutting and back cutting construct the hinge that
//! controls the direction and fall of the tree." The guidebook's rectangle
//! for a 24-inch tree puts the back cut 2 inches above the notch's floor
//! (the stump shot) and leaves 2 inches of holding wood: each a twelfth of
//! the stem's diameter ([`Notch::CONVENTIONAL`]).
//!
//! The tree turns about its hinge towards the notch; "as the face closes,
//! the holding wood breaks", and it falls the rest of the way free. What
//! [`fell`] gives back: the stump standing, the log laid on the ground
//! straight down the fall with its butt just past the stump, the hinge
//! (where it was, how long and thick, for a host that runs the fall
//! itself), and the two sawn ends ([`Kerf`]), which [`kerf`] draws: flat
//! sawn faces showing the bark, the growth rings and any heartwood, the
//! step of the stump shot, and the hinge's fibres torn out between them.

use crate::damage::{self, Break, Fall, Grain, Tear, Toughness};
use crate::graph::{GraphSegment, PlantGraph};
use crate::looks::Flare;
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng;

/// The cuts that fell a tree, as shares of the stem's diameter at the cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Notch {
    /// How far the notch's level cut runs into the stem.
    pub depth: f64,
    /// The angle the sloping cut makes with the level one, radians.
    pub angle: f64,
    /// How far above the notch's floor the back cut runs: the stump shot.
    pub step: f64,
    /// The holding wood left between the back cut and the notch: the
    /// hinge's thickness.
    pub hinge: f64,
}

impl Notch {
    /// The Forest Service guidebook's conventional undercut (chapter 4): a
    /// level cut "at least one-third of the tree's diameter", a sloping cut
    /// "at a 45-degree angle", the back cut 2 inches above the level cut and
    /// stopping 2 inches short of the notch on a 24-inch tree.
    pub const CONVENTIONAL: Self = Self {
        depth: 1.0 / 3.0,
        angle: math::PI / 4.0,
        step: 1.0 / 12.0,
        hinge: 1.0 / 12.0,
    };
}

/// Where and which way a tree is felled: the notch's floor `height` m
/// above the stem's foot, the notch facing `toward` (level), cut as
/// `notch` says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    pub height: f64,
    pub toward: Vec3,
    pub notch: Notch,
}

/// Which side of the cut a [`Kerf`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The stump's top: the notch's floor, the hinge's torn fibres and the
    /// step up to the back cut.
    Stump,
    /// The log's butt: the sloping face of the notch, the hinge's fibres
    /// and the back cut.
    Butt,
    /// The notch cut in the standing tree before the back cut: its floor
    /// and its sloping face, the wood behind still whole.
    Notched,
}

/// The sawn end of a stump or of a log's butt, for [`kerf`] to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kerf {
    pub side: Side,
    /// The stem's axis at the notch's floor, as the stem stood.
    pub origin: Vec3,
    /// Up the stem as it stood, and towards the fall (the notch's face),
    /// square to it; both carried with the log where it lies.
    pub up: Vec3,
    pub front: Vec3,
    /// The stem's radius at the cut, with its root flare, and its bark's
    /// thickness, m.
    pub radius: f64,
    pub bark: f64,
    pub notch: Notch,
    /// Growth rings across the section: the plant's age in years.
    pub rings: u32,
    /// The segment that bears it in its graph, for binding, and the wind
    /// hierarchy level of the wood (0, the stem).
    pub segment: u32,
    pub level: u8,
    pub seed: u64,
}

/// The hinge a tree turns on as it falls, as it stood: its middle, the
/// level line it runs along (square to the fall), its length across the
/// stem, its thickness (from the notch to the back cut) and its height
/// (the stump shot), m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hinge {
    pub centre: Vec3,
    pub along: Vec3,
    pub length: f64,
    pub thickness: f64,
    pub height: f64,
}

/// A tree after it was felled.
#[derive(Debug, Clone, PartialEq)]
pub struct Felled {
    /// What still stands: the stump, and anything rooted beside it.
    pub standing: PlantGraph,
    /// The log, crown and all, laid on the ground down the fall.
    pub log: PlantGraph,
    /// The same log as it stood, notched but not yet cut through: drawn
    /// on `standing` with `notch`, the tree as the sawyer leaves it before
    /// the back cut.
    pub upright: PlantGraph,
    pub notch: Kerf,
    /// The sawn ends: the stump's in `standing`, the butt's in `log`.
    pub stump: Kerf,
    pub butt: Kerf,
    pub hinge: Hinge,
    /// The way it fell.
    pub toward: Vec3,
}

/// The gap left between the stump's front and the log's butt where it
/// lies, in the stem's radii: the butt drops just past the stump.
const CLEAR: f64 = 0.15;

/// Bark 0.06 of the stem's radius where nothing better is known: the bark
/// look's own default, 0.03 of the diameter
/// ([`crate::bark_look::Thickness::at`]).
pub const BARK_SHARE: f64 = 0.06;

/// The stem `graph` felled as `cut` says, its bark as thick as the
/// species' bark look says for the stem's diameter at the cut (`None`:
/// [`BARK_SHARE`] of the radius), with the stem's root flare `flare`;
/// `None` if it has no stem standing at that height.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn fell(
    graph: &PlantGraph,
    cut: &Cut,
    bark: Option<&crate::bark_look::Thickness>,
    flare: Option<&Flare>,
    drag: &crate::breakage::Drag,
) -> Option<Felled> {
    let toward = Vec3::new(cut.toward.x, 0.0, cut.toward.z).normalize_or(Vec3::X);
    let mut g = graph.clone();
    let root = g
        .segments
        .iter()
        .position(|segment| segment.parent.is_none() && segment.order == 0)?;
    let foot = g.segments[root].start.y;
    let floor = foot + cut.height.max(0.0);
    // The stem's own axis, up from its foot.
    let chain = |g: &PlantGraph, from: usize| -> Vec<usize> {
        let mut chain = vec![from];
        let mut at = from;
        loop {
            let next = g.segments.iter().position(|segment| {
                segment.parent == u32::try_from(at).ok() && !segment.lateral && segment.order == 0
            });
            match next {
                Some(next) => {
                    chain.push(next);
                    at = next;
                }
                None => return chain,
            }
        }
    };
    let stem = chain(&g, root);
    let crossing = |g: &PlantGraph, stem: &[usize], y: f64| {
        stem.iter().copied().find(|at| {
            let segment = &g.segments[*at];
            segment.start.y <= y && y < segment.end.y
        })
    };
    let below = crossing(&g, &stem, floor)?;
    let lower = &g.segments[below];
    let up = (lower.end - lower.start).normalize_or(Vec3::Y);
    let share = (floor - lower.start.y) / (lower.end.y - lower.start.y).max(1.0e-9);
    let point = lower.start + (lower.end - lower.start) * share;
    let r_cut = radius_at(&g, below, share);
    // The flared radius the stem is drawn at there: the mesh widens a
    // root stem by `amount` fading over `height`, between buttresses by
    // `1 - ridge`; their mean is used all round.
    let flared = flare.map_or(1.0, |flare| {
        let fade = math::exp(-(floor - foot).max(0.0) / flare.height.max(1.0e-6));
        let lobes = if flare.buttresses > 0 { 0.5 } else { 1.0 };
        1.0 + flare.amount * fade * (1.0 - flare.ridge * (1.0 - lobes))
    });
    let radius = r_cut * flared;
    let diameter = 2.0 * radius;
    let top = cut.notch.depth * diameter * math::tan(cut.notch.angle);
    // The stem is cut at the notch's floor and, for the log, where the
    // sloping cut leaves the bark: the wood between is drawn by the kerfs.
    let upper = split(&mut g, below, point);
    let stem = chain(&g, upper);
    let rise = point + up * top;
    let into = crossing(&g, &stem, rise.y).unwrap_or(*stem.last()?);
    let butt_start = if into == upper && (rise - point).length() < 1.0e-9 {
        upper
    } else {
        split(&mut g, into, rise)
    };
    let n = g.segments.len();
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (index, segment) in g.segments.iter().enumerate() {
        if let Some(parent) = segment.parent {
            children[parent as usize].push(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }
    // Between the two cuts: the notch's wedge and the hinge.
    let gap: Vec<usize> = chain(&g, upper)
        .into_iter()
        .take_while(|at| *at != butt_start)
        .collect();
    // Anything growing out of the gap goes with the log.
    for at in &gap {
        for child in children[*at].clone() {
            if child as usize != butt_start && !gap.contains(&(child as usize)) {
                g.segments[child as usize].parent =
                    Some(u32::try_from(butt_start).unwrap_or(u32::MAX));
            }
        }
    }
    for organ in &mut g.organs {
        if organ
            .segment
            .is_some_and(|segment| gap.contains(&(segment as usize)))
        {
            organ.segment = Some(u32::try_from(butt_start).unwrap_or(u32::MAX));
        }
    }
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (index, segment) in g.segments.iter().enumerate() {
        if let Some(parent) = segment.parent {
            children[parent as usize].push(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }
    let mut run = vec![u32::try_from(butt_start).unwrap_or(u32::MAX)];
    let mut cursor = 0;
    while cursor < run.len() {
        let at = run[cursor] as usize;
        run.extend_from_slice(&children[at]);
        cursor += 1;
    }
    run.sort_unstable();
    let mut removed = vec![false; n];
    for at in run.iter().map(|at| *at as usize).chain(gap.iter().copied()) {
        removed[at] = true;
    }
    // The log as it stood, from the notch's floor up, before the next
    // line widens its first section.
    let mut whole: Vec<usize> = gap.clone();
    whole.extend(run.iter().map(|at| *at as usize));
    let as_stood = g.clone();
    // The log's first section is drawn at the stem's flared radius, so
    // its wood meets the butt's kerf.
    g.segments[butt_start].radius = radius;
    let laid = radius * (1.0 + CLEAR) + top;
    let (piece, placed) = damage::fallen(
        &g,
        &Break {
            segment: u32::try_from(butt_start).unwrap_or(u32::MAX),
            toward,
            wind: 0.0,
        },
        &run,
        &Fall {
            axis: up,
            tension: -toward,
            level: 0,
            tear: Tear::Clean,
            strip: None,
            beside: 0.0,
            laid: Some(laid),
        },
        drag,
    );
    // The stump: everything not taken, its stem ending at the cut and held
    // at its radius there by a short last segment.
    let mut remap = vec![u32::MAX; n];
    let mut standing = PlantGraph {
        age: g.age,
        height: g.height,
        segments: Vec::with_capacity(n),
        organs: Vec::new(),
        structures: g.structures.clone(),
    };
    for (index, segment) in g.segments.iter().enumerate() {
        if removed[index] {
            continue;
        }
        remap[index] = u32::try_from(standing.segments.len()).unwrap_or(u32::MAX);
        let mut kept = segment.clone();
        kept.parent = segment.parent.map(|parent| remap[parent as usize]);
        standing.segments.push(kept);
    }
    let stump_top = remap[below];
    let mut hold = g.segments[below].clone();
    hold.id = rng::combine(hold.id, 0x0053_5455_4D50);
    hold.parent = Some(stump_top);
    hold.lateral = false;
    hold.start = point;
    hold.end = point + up * 1.0e-4;
    hold.radius = r_cut;
    let hold_index = u32::try_from(standing.segments.len()).unwrap_or(u32::MAX);
    standing.segments.push(hold);
    for organ in &g.organs {
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
    let front = (toward - up * up.dot(toward)).normalize_or(Vec3::X);
    let bark = bark
        .map_or(BARK_SHARE * radius, |thickness| thickness.at(diameter))
        .clamp(0.0, 0.5 * radius);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings = g.age.floor().max(1.0) as u32;
    let seed = rng::combine(g.segments[below].id, 0x4645_4C4C);
    let stump = Kerf {
        side: Side::Stump,
        origin: point,
        up,
        front,
        radius,
        bark,
        notch: cut.notch,
        rings,
        segment: hold_index,
        level: 0,
        seed,
    };
    let notched = Kerf {
        side: Side::Notched,
        seed: seed ^ 0x4E07,
        ..stump
    };
    let butt = Kerf {
        side: Side::Butt,
        origin: placed.point(point),
        up: placed.direction(up),
        front: placed.direction(front),
        segment: 0,
        seed: seed ^ 0xB077,
        ..stump
    };
    let behind = radius - cut.notch.depth * diameter;
    let middle = behind - 0.5 * cut.notch.hinge * diameter;
    let hinge = Hinge {
        centre: point + front * middle + up * (0.5 * cut.notch.step * diameter),
        along: up.cross(front).normalize_or(Vec3::Z),
        length: 2.0 * math::sqrt((radius * radius - behind * behind).max(0.0)),
        thickness: cut.notch.hinge * diameter,
        height: cut.notch.step * diameter,
    };
    // The log as it stood: the same segments, unmoved, from the notch's
    // floor up, at their own radii; `notch_out` cuts the notch from its
    // wood.
    let mut index_of = std::collections::HashMap::with_capacity(whole.len());
    for (new, old) in whole.iter().enumerate() {
        index_of.insert(
            u32::try_from(*old).unwrap_or(u32::MAX),
            u32::try_from(new).unwrap_or(u32::MAX),
        );
    }
    let upright = PlantGraph {
        age: g.age,
        height: g.height,
        segments: whole
            .iter()
            .map(|old| {
                let mut segment = as_stood.segments[*old].clone();
                segment.parent = segment
                    .parent
                    .and_then(|parent| index_of.get(&parent).copied());
                if segment.parent.is_none() {
                    segment.lateral = false;
                }
                segment
            })
            .collect(),
        organs: g
            .organs
            .iter()
            .filter_map(|organ| {
                let new = index_of.get(&organ.segment?)?;
                let mut kept = organ.clone();
                kept.segment = Some(*new);
                Some(kept)
            })
            .collect(),
        structures: g.structures.clone(),
    };
    Some(Felled {
        standing,
        log: piece.graph,
        upright,
        notch: notched,
        stump,
        butt,
        hinge,
        toward,
    })
}

/// The radius of the stem segment `at` a share `share` of the way along
/// it: its own radius tapering to its stem's next segment's.
fn radius_at(g: &PlantGraph, at: usize, share: f64) -> f64 {
    let segment = &g.segments[at];
    let next = g
        .segments
        .iter()
        .find(|other| other.parent == u32::try_from(at).ok() && !other.lateral)
        .map_or(segment.radius, |other| other.radius);
    segment.radius + (next - segment.radius) * share.clamp(0.0, 1.0)
}

/// Segment `at` of `g` cut in two at `point` on it: it ends there, and a
/// new segment, which its axis's children and its organs beyond the point
/// now grow from, carries on to where it ended. The new segment's index.
fn split(g: &mut PlantGraph, at: usize, point: Vec3) -> usize {
    let new = g.segments.len();
    let new_index = u32::try_from(new).unwrap_or(u32::MAX);
    let old = g.segments[at].clone();
    let length = (old.end - old.start).length().max(1.0e-12);
    let share = ((point - old.start).length() / length).clamp(0.0, 1.0);
    let mut upper: GraphSegment = old.clone();
    upper.id = rng::combine(old.id, 0x0055_5050_4552);
    upper.parent = Some(u32::try_from(at).unwrap_or(u32::MAX));
    upper.lateral = false;
    upper.start = point;
    upper.radius = radius_at(g, at, share);
    let axis = (old.end - old.start) * (1.0 / length);
    for segment in &mut g.segments {
        if segment.parent == u32::try_from(at).ok() {
            let along = (segment.start - old.start).dot(axis);
            if along >= share * length - 1.0e-9 {
                segment.parent = Some(new_index);
            }
        }
    }
    for organ in &mut g.organs {
        if organ.segment == u32::try_from(at).ok()
            && (organ.position - old.start).dot(axis) > share * length
        {
            organ.segment = Some(new_index);
        }
    }
    g.segments[at].end = point;
    g.segments.push(upper);
    new
}

/// The sawn end `kerf` of wood of toughness `toughness`, coloured `grain`,
/// with about `edge` m between its vertices. The section is cut into
/// bands, a growth ring's light earlywood and dark latewood each (rings
/// merged where the edge is too coarse to show them all), heartwood where
/// the grain has one and bark round the outside, and sectors round it;
/// each cell is clipped to the parts of the cut: the notch's floor (or its
/// sloping face on the butt), the holding wood torn through, and the back
/// cut a stump shot higher. Steps between them are walls of bare wood, the
/// bark rises round the outside to the highest face, and the hinge's
/// fibres stand out of the torn strip: up from the stump, down from the
/// butt, as long as the wood is tough. Flat shaded.
#[must_use]
#[allow(clippy::too_many_lines)]
#[allow(clippy::many_single_char_names, clippy::items_after_statements)]
pub fn kerf(kerf: &Kerf, toughness: Toughness, grain: &Grain, edge: f64) -> Mesh {
    let mut out = Mesh::default();
    let r = kerf.radius.max(1.0e-4);
    let d = 2.0 * r;
    let edge = edge.max(r / 200.0).max(1.0e-4);
    let up = kerf.up.normalize_or(Vec3::Y);
    let front = (kerf.front - up * up.dot(kerf.front)).normalize_or(Vec3::X);
    let side = up.cross(front);
    let at = |u: f64, v: f64, y: f64| kerf.origin + front * u + side * v + up * y;
    let draw = |salt: u64| rng::unit(rng::combine(kerf.seed, salt));
    let notch = kerf.notch;
    // The parts, along the fall: the notch's floor from `back` to the
    // front, the holding wood from `heel` to `back`, the back cut behind.
    let back = r - notch.depth * d;
    let heel = back - notch.hinge * d;
    let shot = notch.step * d;
    let torn = 0.5 * shot;
    let block = notch.depth * d * math::tan(notch.angle);
    let stump = kerf.side == Side::Stump;
    let notched = kerf.side == Side::Notched;
    // Each part's face height at `u` above the notch's floor. The stump's
    // floor is lifted a hair so it does not fight the stem's own end.
    let lift = (0.01 * r).max(0.003);
    let face = |part: usize, u: f64| match (stump, part) {
        (true, 0) => lift,
        (false, 0) => (u - back).max(0.0) * math::tan(notch.angle),
        (_, 1) => torn,
        _ => shot,
    };
    let part_of = |u: f64| {
        if u >= back {
            0
        } else if u >= heel {
            1
        } else {
            2
        }
    };
    let bounds = [
        (back, f64::INFINITY),
        (heel, back),
        (f64::NEG_INFINITY, heel),
    ];
    // The faces to draw, each a part of the section at a height: the
    // stump's or the butt's own three, or the notch's floor and its
    // sloping face in the standing tree.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Face {
        Own,
        Floor,
        Slope,
    }
    let slope = |u: f64| (u - back).max(0.0) * math::tan(notch.angle);
    let caps: Vec<(usize, f64, f64, Face)> = if notched {
        vec![
            (0, back, f64::INFINITY, Face::Floor),
            (0, back, f64::INFINITY, Face::Slope),
        ]
    } else {
        bounds
            .iter()
            .enumerate()
            .map(|(part, (lo, hi))| (part, *lo, *hi, Face::Own))
            .collect()
    };
    // Facing out of the wood: up from the stump, down from the butt.
    let outward = if stump { up } else { -up };
    let wood = grain.sapwood;
    let shade = |colour: [f32; 3], k: f32| [colour[0] * k, colour[1] * k, colour[2] * k, 1.0];
    let triangle = |out: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, want: Vec3, colour: [f32; 4]| {
        let mut normal = (b - a).cross(c - a);
        if normal.length() < 1.0e-14 {
            return;
        }
        let (b, c) = if normal.dot(want) < 0.0 {
            normal = -normal;
            (c, b)
        } else {
            (b, c)
        };
        let normal = normal.normalize_or(want);
        let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
        for p in [a, b, c] {
            out.push(Vertex {
                position: p,
                normal,
                uv: [0.0, 0.0],
                color: colour,
                born: 0.0,
                shed: None,
                level: kerf.level,
                segment: kerf.segment,
            });
        }
        out.indices.extend_from_slice(&[base, base + 1, base + 2]);
    };
    // The bands out from the pith: each growth ring's earlywood and
    // latewood, then the bark. Each year's ring its own width, as the
    // years' growing seasons differ; merged in fours, twos or more where
    // the edge is too coarse to show every one.
    let inside = (r - kerf.bark).max(0.5 * r);
    let rings = kerf.rings.max(1) as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let finest = ((4.0 * inside / edge).floor() as usize).clamp(1, 150);
    let merged = rings.div_ceil(finest);
    let shown = rings.div_ceil(merged);
    let widths: Vec<f64> = (0..shown)
        .map(|ring| 0.6 + 0.8 * draw(0x5749_0000 + ring as u64))
        .collect();
    let total: f64 = widths.iter().sum();
    let mut bands: Vec<(f64, f64, [f32; 4])> = Vec::with_capacity(2 * shown + 1);
    let mut r0 = 0.0;
    for (ring, width) in widths.iter().enumerate() {
        let r1 = r0 + inside * width / total;
        let late = r0 + 0.72 * (r1 - r0);
        let base = match grain.heartwood {
            Some((colour, heart)) if r1 <= heart * inside => colour,
            _ => wood,
        };
        #[allow(clippy::cast_possible_truncation)]
        let jitter = (0.96 + 0.08 * draw(0x5249_0000 + ring as u64)) as f32;
        bands.push((r0, late, shade(base, 1.08 * jitter)));
        bands.push((late, r1, shade(base, 0.8 * jitter)));
        r0 = r1;
    }
    bands.push((inside, r, shade(grain.bark, 1.0)));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let sectors = ((2.0 * math::PI * r / edge).round() as usize).clamp(24, 160);
    #[allow(clippy::cast_precision_loss)]
    let angle_of = |sector: usize| 2.0 * math::PI * sector as f64 / sectors as f64;
    for (r0, r1, colour) in &bands {
        for sector in 0..sectors {
            let (a0, a1) = (angle_of(sector), angle_of(sector + 1));
            let cell = [
                (r0 * math::cos(a0), r0 * math::sin(a0)),
                (r1 * math::cos(a0), r1 * math::sin(a0)),
                (r1 * math::cos(a1), r1 * math::sin(a1)),
                (r0 * math::cos(a1), r0 * math::sin(a1)),
            ];
            for &(part, lo, hi, kind) in &caps {
                let piece = clip(&cell, lo, hi);
                if piece.len() < 3 {
                    continue;
                }
                let outward = if kind == Face::Floor || (kind == Face::Own && stump) {
                    up
                } else {
                    -up
                };
                // The torn strip is rough, darker than sawn wood.
                let colour = if part == 1 {
                    [colour[0] * 0.85, colour[1] * 0.85, colour[2] * 0.85, 1.0]
                } else {
                    *colour
                };
                let p = |(u, v): (f64, f64)| {
                    let y = match kind {
                        Face::Own => face(part, u),
                        Face::Floor => lift,
                        Face::Slope => slope(u),
                    };
                    at(u, v, y)
                };
                for k in 1..piece.len() - 1 {
                    triangle(
                        &mut out,
                        p(piece[0]),
                        p(piece[k]),
                        p(piece[k + 1]),
                        outward,
                        colour,
                    );
                }
            }
        }
    }
    // The steps between the parts, straight across the stem.
    let bare = shade(wood, 0.82);
    let steps: &[(f64, usize, usize)] = if notched {
        &[]
    } else {
        &[(back, 0, 1), (heel, 1, 2)]
    };
    for &(u, lower, upper) in steps {
        let half = math::sqrt((r * r - u * u).max(0.0));
        if half <= 0.0 {
            continue;
        }
        let (y0, y1) = (face(lower, u), face(upper, u));
        let (a, b, c, e) = (
            at(u, -half, y0),
            at(u, half, y0),
            at(u, half, y1),
            at(u, -half, y1),
        );
        triangle(&mut out, a, b, c, front, bare);
        triangle(&mut out, a, c, e, front, bare);
    }
    // The bark round the outside, from the stem's own end to the faces
    // (the stump) or from the faces to where the log's wood resumes (the
    // butt); split where the parts meet so each run is one part.
    let mut angles: Vec<f64> = (0..=sectors).map(angle_of).collect();
    for u in [back, heel] {
        if u.abs() < r {
            let a = math::acos(u / r);
            angles.push(a);
            angles.push(2.0 * math::PI - a);
        }
    }
    angles.sort_by(f64::total_cmp);
    // Ridges and furrows of the bark: each run round its own shade.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bark_at = |a: f64| {
        let furrow = draw(0x4241_0000 ^ (a * 1.0e4) as u64);
        shade(grain.bark, (0.6 + 0.5 * furrow) as f32)
    };
    for pair in angles.windows(2) {
        let (a0, a1) = (pair[0], pair[1]);
        if a1 - a0 < 1.0e-9 {
            continue;
        }
        let part = part_of(r * math::cos(f64::midpoint(a0, a1)));
        let rim = |a: f64, y: f64| at(r * math::cos(a), r * math::sin(a), y);
        let out_at =
            front * math::cos(f64::midpoint(a0, a1)) + side * math::sin(f64::midpoint(a0, a1));
        // A notched stem keeps its own bark round the notch (`notch_out`).
        if notched {
            continue;
        }
        let (low0, low1, high0, high1) = if stump {
            (
                0.0,
                0.0,
                face(part, r * math::cos(a0)),
                face(part, r * math::cos(a1)),
            )
        } else {
            (
                face(part, r * math::cos(a0)),
                face(part, r * math::cos(a1)),
                block,
                block,
            )
        };
        if high0 - low0 < 1.0e-9 && high1 - low1 < 1.0e-9 {
            continue;
        }
        let (p0, p1, p2, p3) = (rim(a0, low0), rim(a1, low1), rim(a1, high1), rim(a0, high0));
        let skin = bark_at(a0);
        triangle(&mut out, p0, p1, p2, out_at, skin);
        triangle(&mut out, p0, p2, p3, out_at, skin);
    }
    // The hinge's fibres, torn out of the holding wood: bundles standing
    // up from the stump and hanging from the butt, the longest about a
    // stump shot for middling wood.
    let half = math::sqrt((r * r - back * back).max(0.0));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let across = ((2.0 * half / edge.max(0.25 * notch.hinge * d)).round() as usize).clamp(4, 200);
    let rows = if notched { 0 } else { 3_usize };
    let reach = shot * toughness.reach();
    let fibre = shade(wood, 0.92);
    #[allow(clippy::cast_precision_loss)]
    for row in 0..rows {
        let (u0, u1) = (
            heel + (back - heel) * row as f64 / rows as f64,
            heel + (back - heel) * (row + 1) as f64 / rows as f64,
        );
        for cell in 0..across {
            let (v0, v1) = (
                -half + 2.0 * half * cell as f64 / across as f64,
                -half + 2.0 * half * (cell + 1) as f64 / across as f64,
            );
            let u_mid = f64::midpoint(u0, u1);
            let v_mid = f64::midpoint(v0, v1);
            if u_mid * u_mid + v_mid * v_mid > inside * inside {
                continue;
            }
            let salt = (row as u64) << 20 | cell as u64;
            let length = reach * (0.2 + 0.8 * draw(0xF1B0_0000 ^ salt));
            let sign = if stump { 1.0 } else { -1.0 };
            let (y0, y1) = (torn, torn + sign * length);
            // A bundle narrower than its cell, its tip torn aslant.
            let inset = 0.15 * (u1 - u0);
            let (u0, u1, v0, v1) = (u0 + inset, u1 - inset, v0 + inset, v1 - inset);
            let tip = |u: f64, v: f64| {
                let slant = 0.3 * length * ((u - u0) / (u1 - u0).max(1.0e-9) - 0.5);
                at(u, v, y1 + sign * slant)
            };
            let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                let out_dir = {
                    let (mu, mv) = (
                        f64::midpoint(a.0, b.0) - u_mid,
                        f64::midpoint(a.1, b.1) - v_mid,
                    );
                    front * mu + side * mv
                };
                let (p0, p1) = (at(a.0, a.1, y0), at(b.0, b.1, y0));
                let (p2, p3) = (tip(b.0, b.1), tip(a.0, a.1));
                triangle(&mut out, p0, p1, p2, out_dir, fibre);
                triangle(&mut out, p0, p2, p3, out_dir, fibre);
            }
            let (t0, t1, t2, t3) = (tip(u0, v0), tip(u1, v0), tip(u1, v1), tip(u0, v1));
            triangle(&mut out, t0, t1, t2, outward, fibre);
            triangle(&mut out, t0, t2, t3, outward, fibre);
        }
    }
    out
}

/// Cuts the notch `kerf` out of `wood`, the stem as it stood above the
/// notch's floor: the wood under the sloping cut, in front of the notch's
/// back, goes, its triangles clipped along the cut.
pub fn notch_out(wood: &mut Mesh, kerf: &Kerf) {
    let r = kerf.radius.max(1.0e-4);
    let d = 2.0 * r;
    let up = kerf.up.normalize_or(Vec3::Y);
    let front = (kerf.front - up * up.dot(kerf.front)).normalize_or(Vec3::X);
    let back = r - kerf.notch.depth * d;
    let rise = math::tan(kerf.notch.angle);
    // Above the sloping cut where positive.
    let above = |p: [f32; 3]| {
        let q = Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])) - kerf.origin;
        q.dot(up) - (q.dot(front) - back) * rise
    };
    let side: Vec<f64> = wood.positions.iter().map(|p| above(*p)).collect();
    let bound = wood.segments.len();
    let cut = |wood: &mut Mesh, a: usize, b: usize| -> u32 {
        let t = side[a] / (side[a] - side[b]);
        #[allow(clippy::cast_possible_truncation)]
        let t32 = t as f32;
        let mix = |x: f32, y: f32| x + (y - x) * t32;
        let index = u32::try_from(wood.positions.len()).unwrap_or(u32::MAX);
        let (pa, pb) = (wood.positions[a], wood.positions[b]);
        wood.positions
            .push([mix(pa[0], pb[0]), mix(pa[1], pb[1]), mix(pa[2], pb[2])]);
        let (na, nb) = (wood.normals[a], wood.normals[b]);
        let n = [mix(na[0], nb[0]), mix(na[1], nb[1]), mix(na[2], nb[2])];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1.0e-9);
        wood.normals
            .push([n[0] / length, n[1] / length, n[2] / length]);
        let (ua, ub) = (wood.uvs[a], wood.uvs[b]);
        wood.uvs.push([mix(ua[0], ub[0]), mix(ua[1], ub[1])]);
        let (ca, cb) = (wood.colors[a], wood.colors[b]);
        wood.colors.push([
            mix(ca[0], cb[0]),
            mix(ca[1], cb[1]),
            mix(ca[2], cb[2]),
            mix(ca[3], cb[3]),
        ]);
        wood.births.push(wood.births[a]);
        wood.sheds.push(wood.sheds[a]);
        wood.levels.push(wood.levels[a]);
        if a < bound {
            let segment = wood.segments[a];
            wood.segments
                .resize(index as usize, crate::mesh::NO_SEGMENT);
            wood.segments.push(segment);
        }
        index
    };
    let triangles = std::mem::take(&mut wood.indices);
    let mut kept = Vec::with_capacity(triangles.len());
    for t in triangles.as_chunks::<3>().0 {
        let corners = [t[0] as usize, t[1] as usize, t[2] as usize];
        let inside = corners.map(|at| side[at] >= 0.0);
        match inside.iter().filter(|keep| **keep).count() {
            3 => kept.extend_from_slice(t),
            0 => {}
            count => {
                // Walk the triangle's edges, keeping what lies above the
                // cut and crossing it where an edge does.
                let mut polygon = Vec::with_capacity(4);
                for k in 0..3 {
                    let (a, b) = (corners[k], corners[(k + 1) % 3]);
                    if inside[k] {
                        polygon.push(u32::try_from(a).unwrap_or(u32::MAX));
                    }
                    if inside[k] != inside[(k + 1) % 3] {
                        polygon.push(cut(wood, a, b));
                    }
                }
                debug_assert_eq!(polygon.len(), count + 2);
                for k in 1..polygon.len() - 1 {
                    kept.extend_from_slice(&[polygon[0], polygon[k], polygon[k + 1]]);
                }
            }
        }
    }
    wood.indices = kept;
}

/// The convex polygon `cell` in (u, v) clipped to `lo <= u < hi`.
fn clip(cell: &[(f64, f64)], lo: f64, hi: f64) -> Vec<(f64, f64)> {
    let keep = |polygon: &[(f64, f64)], inside: &dyn Fn(f64) -> bool, edge: f64| {
        let mut kept = Vec::with_capacity(polygon.len() + 2);
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            let (ina, inb) = (inside(a.0), inside(b.0));
            if ina {
                kept.push(a);
            }
            if ina != inb {
                let t = (edge - a.0) / (b.0 - a.0);
                kept.push((edge, a.1 + (b.1 - a.1) * t));
            }
        }
        kept
    };
    let mut polygon = cell.to_vec();
    if lo.is_finite() {
        polygon = keep(&polygon, &|u| u >= lo, lo);
    }
    if hi.is_finite() && polygon.len() >= 3 {
        polygon = keep(&polygon, &|u| u < hi, hi);
    }
    polygon
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breakage::Drag;
    use crate::graph::GraphOrgan;

    /// A straight stem 10 m tall in five segments, 30 cm across at its
    /// foot, with a branch and a leaf high up.
    fn tree() -> PlantGraph {
        let segment =
            |id: u64, parent: Option<u32>, lateral: bool, start: Vec3, end: Vec3, radius: f64| {
                GraphSegment {
                    id,
                    parent,
                    lateral,
                    order: u16::from(lateral),
                    start,
                    end,
                    radius,
                    born: 0.0,
                    shed: None,
                    body: 0,
                    left: Vec3::ZERO,
                    within: None,
                    young: 0.0,
                }
            };
        let y = |h: f64| Vec3::Y * h;
        let segments = vec![
            segment(1, None, false, y(0.0), y(2.0), 0.15),
            segment(2, Some(0), false, y(2.0), y(4.0), 0.13),
            segment(3, Some(1), false, y(4.0), y(6.0), 0.11),
            segment(4, Some(2), false, y(6.0), y(8.0), 0.08),
            segment(5, Some(3), false, y(8.0), y(10.0), 0.05),
            segment(6, Some(3), true, y(8.0), y(8.0) + Vec3::Z * 2.0, 0.03),
        ];
        PlantGraph {
            age: 40.0,
            height: 10.0,
            segments,
            organs: vec![GraphOrgan {
                id: 1,
                organ: 0,
                segment: Some(5),
                position: y(8.0) + Vec3::Z * 2.0,
                heading: Vec3::Y,
                left: Vec3::X,
                size: 0.1,
                born: 0.0,
                shed: None,
                light: 1.0,
                within: None,
                adult: None,
            }],
            structures: Vec::new(),
        }
    }

    fn felled() -> Felled {
        let cut = Cut {
            height: 0.3,
            toward: Vec3::X,
            notch: Notch::CONVENTIONAL,
        };
        fell(&tree(), &cut, None, None, &Drag::default()).expect("a stem at 0.3 m")
    }

    #[test]
    fn the_stump_stands_to_the_cut_and_the_log_lies_down_the_fall() {
        let felled = felled();
        assert!(
            (felled.standing.height - 0.3001).abs() < 1.0e-6,
            "{}",
            felled.standing.height
        );
        assert_eq!(felled.toward, Vec3::X);
        // The log lies level along +X, beyond the stump, on the ground.
        for segment in &felled.log.segments {
            for p in [segment.start, segment.end] {
                assert!(p.x > 0.0, "{p:?}");
                assert!(p.y <= 2.2 + 1.0e-9, "lies low: {p:?}");
            }
        }
        let first = &felled.log.segments[0];
        assert!(first.start.y < 0.2, "{:?}", first.start);
        let along = (first.end - first.start).normalize_or(Vec3::Y);
        assert!(along.x > 0.99, "{along:?}");
        // The leaf and branch went with it.
        assert_eq!(felled.log.organs.len(), 1);
        assert!(felled.standing.organs.is_empty());
    }

    #[test]
    fn the_hinge_is_a_twelfth_thick_and_spans_most_of_the_stem() {
        let felled = felled();
        let d = 2.0 * felled.stump.radius;
        assert!((felled.hinge.thickness - d / 12.0).abs() < 1.0e-12);
        assert!((felled.hinge.height - d / 12.0).abs() < 1.0e-12);
        // At least 80 % of the diameter, as fellers aim for.
        assert!(felled.hinge.length > 0.8 * d, "{}", felled.hinge.length);
        assert!(felled.hinge.along.dot(Vec3::X).abs() < 1.0e-9);
        // Just ahead of the stem's axis, towards the fall.
        assert!(felled.hinge.centre.x > 0.0 && felled.hinge.centre.x < 0.5 * felled.stump.radius);
    }

    #[test]
    fn the_stump_shows_its_floor_its_step_and_its_rings() {
        let felled = felled();
        let grain = Grain {
            sapwood: Grain::SAPWOOD,
            heartwood: None,
            bark: [0.2, 0.15, 0.1],
        };
        let mesh = kerf(&felled.stump, Toughness { work: 55.7 }, &grain, 0.01);
        let r = felled.stump.radius;
        let d = 2.0 * r;
        let mut floor = false;
        let mut step = false;
        let mut colours = std::collections::BTreeSet::new();
        for (p, c) in mesh.positions.iter().zip(&mesh.colors) {
            let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            assert!(x * x + z * z <= (r + 1.0e-4) * (r + 1.0e-4), "{p:?}");
            if x > r / 3.0 + 1.0e-3 && (y - 0.3).abs() < 1.0e-3 {
                floor = true;
            }
            if x < r / 6.0 - 1.0e-3 && (y - 0.3 - d / 12.0).abs() < 1.0e-3 {
                step = true;
            }
            #[allow(clippy::cast_possible_truncation)]
            colours.insert((c[0] * 1000.0) as i32);
        }
        assert!(floor, "the notch's floor at the cut");
        assert!(step, "the back cut a stump shot higher");
        assert!(colours.len() > 10, "rings: {}", colours.len());
    }

    #[test]
    fn the_butt_carries_the_notch_s_sloping_face_where_the_log_lies() {
        let felled = felled();
        let grain = Grain {
            sapwood: Grain::SAPWOOD,
            heartwood: None,
            bark: [0.2, 0.15, 0.1],
        };
        let mesh = kerf(&felled.butt, Toughness { work: 55.7 }, &grain, 0.01);
        let r = felled.butt.radius;
        // Lying: its sawn end faces back towards the stump and sits on the
        // ground beyond it.
        assert!(felled.butt.up.x > 0.99, "{:?}", felled.butt.up);
        for p in &mesh.positions {
            let (x, y) = (f64::from(p[0]), f64::from(p[1]));
            assert!(x > r, "past the stump: {p:?}");
            assert!(y > -1.0e-3 && y < 2.0 * r + 1.0e-3, "{p:?}");
        }
    }

    #[test]
    fn clipping_keeps_the_part_of_a_cell_inside() {
        let cell = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let kept = clip(&cell, 0.25, 0.75);
        assert_eq!(kept.len(), 4);
        for (u, _) in kept {
            assert!((0.25..=0.75).contains(&u));
        }
        assert!(clip(&cell, 2.0, f64::INFINITY).is_empty());
    }

    #[test]
    fn the_notched_tree_stands_whole_from_the_floor_with_the_notch_cut_from_its_wood() {
        let felled = felled();
        let floor = felled.notch.origin.y;
        // The tree as it stood starts at the notch's floor, at the stem's
        // own radius there.
        let foot = felled
            .upright
            .segments
            .iter()
            .find(|segment| segment.parent.is_none())
            .expect("a foot");
        assert!((foot.start.y - floor).abs() < 1.0e-9);
        assert!(foot.radius < felled.notch.radius * 1.01);
        // Wood in the notch goes; wood behind it and above it stays,
        // clipped along the sloping cut.
        let r = felled.notch.radius;
        let mut wood = Mesh::default();
        let mut point = |x: f64, y: f64| {
            wood.push(Vertex::plain(
                Vec3::new(x, floor + y, 0.0),
                Vec3::X,
                [0.5, 0.4, 0.3],
            ))
        };
        let inside = [
            point(0.95 * r, 0.01),
            point(0.9 * r, 0.02),
            point(0.95 * r, 0.03),
        ];
        let behind = [point(-r, 0.0), point(-0.9 * r, 0.0), point(-r, 0.1)];
        let across = [point(r, 0.0), point(r, 2.0 * r), point(0.9 * r, 2.0 * r)];
        for triangle in [inside, behind, across] {
            wood.indices.extend_from_slice(&triangle);
        }
        notch_out(&mut wood, &felled.notch);
        assert_eq!(wood.triangle_count(), 1 + 2);
        let back = r - felled.notch.notch.depth * 2.0 * r;
        let rise = math::tan(felled.notch.notch.angle);
        for at in &wood.indices {
            let p = wood.positions[*at as usize];
            let (u, h) = (f64::from(p[0]), f64::from(p[1]) - floor);
            assert!(h - (u - back) * rise > -1.0e-4, "{u} {h} is in the notch");
        }
    }
}
