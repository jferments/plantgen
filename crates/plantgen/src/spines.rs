//! Spines: what each areole of a fleshy body carries.
//!
//! The body mesher (see [`crate::body`]) leaves one [`Tuft`] per areole:
//! where it sits, which way it faces, and a seed. A tuft becomes its spines
//! only when it is drawn, from its body's [`SpineSet`]s ([`expand`]), so a
//! package stores 61 bytes per areole rather than the spines' triangles, and
//! a renderer can expand the few thousand tufts nearest the camera on the
//! GPU with the same arithmetic. Every draw is a hash of the tuft's seed and
//! the spine's number ([`draw`]), in single precision as a shader computes
//! it.
//!
//! A spine is a tapered solid of three sides along a centreline of three
//! pieces that bends by its set's curve and, over its last two, its hook
//! ([`solid`]); the felt is a low six-sided cone round the areole. Farther
//! away an areole is two cards whose templates are rendered from one tuft's
//! spines: the star seen face on and the fan seen from the side ([`star`],
//! [`fan`]).

use crate::body::{BodyLook, SpineSet};
use crate::math::Vec3;
use crate::mesh::{Mesh, Vertex};

/// Most spines one areole may carry, over all its sets.
pub const MAX_TUFT_SPINES: u32 = 64;

/// Most spine sets one body may have.
pub const MAX_SPINE_SETS: usize = 8;

/// Vertices and triangles of one spine's solid: three rings of three and
/// the tip; three pieces of six triangles but the last of three.
pub const SPINE_VERTICES: usize = 10;
pub const SPINE_TRIANGLES: usize = 15;

/// Vertices and triangles of one areole's felt.
pub const FELT_VERTICES: usize = 7;
pub const FELT_TRIANGLES: usize = 6;

/// Where along a spine its two joints sit, as shares of its length, and its
/// radius there as a share of the base's.
const JOINTS: [f32; 2] = [0.45, 0.8];
const TAPER: [f32; 3] = [1.0, 0.62, 0.3];

/// Where a spine's joints sit for a hook of `hook` degrees. A hook is the
/// curl at the very tip: past 90 degrees the shaft runs to 72% of the
/// length, the next piece turns half the hook and the last the whole, so a
/// fishhook is an open J rather than a tip folded flat on its shaft.
fn hook_joints(hook: f32) -> [f32; 2] {
    let k = (hook / 90.0).clamp(0.0, 1.0);
    [
        JOINTS[0] + (0.72 - JOINTS[0]) * k,
        JOINTS[1] + (0.88 - JOINTS[1]) * k,
    ]
}

/// One areole of a body: where its spines grow from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuft {
    /// Centre of the areole on the body's surface, plant frame, metres.
    pub position: [f32; 3],
    /// Outward unit normal of the surface there.
    pub normal: [f32; 3],
    /// Unit vector along the surface toward the body's tip.
    pub up: [f32; 3],
    /// Length of its spines as a share of their full length: below 1 on a
    /// young areole near a growing tip.
    pub scale: f32,
    /// How far its spines have turned their old colour, 0 to 1.
    pub grey: f32,
    /// Draws every spine of the tuft (see [`draw`]).
    pub seed: u32,
    /// Index of the body type among the program's bodies.
    pub body: u8,
    /// Plant age at which the areole appears and is shed, in years;
    /// infinity when it is never shed.
    pub born: f32,
    pub shed: f32,
    /// Where the areole sits along its body segment, from its base (0)
    /// to its tip (1): a pad's lower areoles carry fewer of a set that
    /// keeps to the upper ones ([`thinning`]).
    pub along: f32,
}

/// One spine, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spine {
    /// Base, the two joints and the tip, plant frame, metres.
    pub points: [[f32; 3]; 4],
    /// Unit vector across the spine, perpendicular to the plane it bends in.
    pub across: [f32; 3],
    /// Radius at the base, metres.
    pub radius: f32,
    /// Thickness over width of the cross-section.
    pub flat: f32,
    /// How far the cross-section turns from base to tip, degrees.
    pub twist: f32,
    pub colour: [f32; 3],
    pub tip: [f32; 3],
}

/// A well-mixed 32-bit hash (PCG), as the renderer's shaders compute it.
#[must_use]
pub const fn pcg(value: u32) -> u32 {
    let state = value.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// Draw `field` of spine `spine` of set `set` of the tuft with `seed`:
/// uniform in `[0, 1)`. Spine 255 holds the set's own draws.
#[must_use]
pub fn draw(seed: u32, set: u32, spine: u32, field: u32) -> f32 {
    let key = (set << 12) | ((spine & 0xFF) << 4) | (field & 0xF);
    // 24 bits fit an f32's mantissa exactly.
    #[allow(clippy::cast_precision_loss)]
    let top = (pcg(seed ^ pcg(key)) >> 8) as f32;
    top / 16_777_216.0
}

/// The set-level spine number.
const SET: u32 = 255;

/// How many spines set `set` puts on the areole with `seed`.
#[must_use]
pub fn count(set: &SpineSet, index: u32, seed: u32) -> u32 {
    let [low, high] = set.count;
    let high = high.max(low);
    // A share of a small whole number, floored.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let extra = (draw(seed, index, SET, 15) * (high - low + 1) as f32) as u32;
    low + extra.min(high - low)
}

/// Share of a set's spines an areole `along` its segment keeps when the
/// set keeps to the upper areoles by `distal` (see
/// [`crate::body::SpineSet::distal`]).
#[must_use]
pub fn thinning(distal: f32, along: f32) -> f32 {
    (1.0 - 2.0 * distal * (1.0 - along)).clamp(0.0, 1.0)
}

/// How many of set `set`'s `n` spines the areole with `seed` `along` its
/// segment keeps: the share [`thinning`] gives, rounded up or down by a
/// draw, so the lowest areoles of a set kept above have none and the ones
/// between have some.
#[must_use]
pub fn kept_count(set: &SpineSet, index: u32, seed: u32, along: f32, n: u32) -> u32 {
    let share = thinning(single(set.distal), along);
    if share >= 1.0 {
        return n;
    }
    // A share of a small whole number, rounded by a draw.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let kept = (n as f32 * share + draw(seed, index, SET, 13)) as u32;
    kept.min(n)
}

/// Share of an areole's spine, by count and length, that an areole
/// `along` its segment keeps: 1 when no set keeps to the upper areoles.
#[must_use]
pub fn kept(look: &BodyLook, along: f32) -> f64 {
    let (mut all, mut kept) = (0.0, 0.0);
    for set in &look.spines {
        let amount = f64::from(set.count[0] + set.count[1].max(set.count[0])) * set.length;
        all += amount;
        kept += amount * f64::from(thinning(single(set.distal), along));
    }
    if all > 0.0 { kept / all } else { 1.0 }
}

/// Mean share of a set's spines its areoles keep over a segment, for a
/// set that keeps to the upper areoles by `distal`.
fn mean_thinning(distal: f64) -> f64 {
    if distal <= 0.5 {
        1.0 - distal
    } else {
        0.25 / distal
    }
}

fn v(a: [f32; 3]) -> [f32; 3] {
    a
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let length = dot(a, a).sqrt();
    if length > 1e-12 {
        scale(a, 1.0 / length)
    } else {
        fallback
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// `d` turned by `angle` radians about the unit `axis` (Rodrigues).
fn rotate(d: [f32; 3], axis: [f32; 3], angle: f32) -> [f32; 3] {
    let (sin, cos) = (libm::sinf(angle), libm::cosf(angle));
    add(
        add(scale(d, cos), scale(cross(axis, d), sin)),
        scale(axis, dot(axis, d) * (1.0 - cos)),
    )
}

const TAU: f32 = std::f32::consts::TAU;

fn radians(degrees: f32) -> f32 {
    degrees * (std::f32::consts::PI / 180.0)
}

// Spec values are f64 for the JSON; spines are single precision.
#[allow(clippy::cast_possible_truncation)]
fn single(value: f64) -> f32 {
    value as f32
}

/// The length of spine `i` of set `index` on the tuft with `seed`, at
/// `scale` of its full length.
fn spine_length(set: &SpineSet, index: u32, seed: u32, i: u32, scale: f32) -> f32 {
    single(set.length)
        * scale
        * (1.0 + single(set.length_spread) * (2.0 * draw(seed, index, i, 2) - 1.0))
}

/// Each spine set's spines on a grown areole with `seed`, as [`expand`]
/// draws them: each one's length, metres. The checks count and measure
/// an areole's spines by this (spec rework E6c).
#[must_use]
pub fn areole_spines(look: &BodyLook, seed: u32) -> Vec<Vec<f32>> {
    let mut total = 0;
    look.spines
        .iter()
        .enumerate()
        .take(MAX_SPINE_SETS)
        .map(|(index, set)| {
            // At most eight sets.
            #[allow(clippy::cast_possible_truncation)]
            let index = index as u32;
            let n = count(set, index, seed).min(MAX_TUFT_SPINES - total);
            total += n;
            (0..n)
                .map(|i| spine_length(set, index, seed, i, 1.0))
                .collect()
        })
        .collect()
}

/// The spines of `tuft`, drawn from its body's look, appended to `out`.
/// Its old colour comes from the look's greying.
// One spine's geometry from its draws: clearer as one piece, and in the
// shader's own operations (its means are sums halved, not midpoints).
#[allow(clippy::too_many_lines, clippy::manual_midpoint)]
pub fn expand(tuft: &Tuft, look: &BodyLook, out: &mut Vec<Spine>) {
    let normal = normalize(tuft.normal, [0.0, 0.0, 1.0]);
    let up = normalize(
        sub(tuft.up, scale(normal, dot(tuft.up, normal))),
        [0.0, 1.0, 0.0],
    );
    let side = cross(normal, up);
    let felt = single(look.areoles.felt_radius);
    let aged = look.greying.as_ref().map(|greying| greying.colour);
    let mut total = 0;
    for (index, set) in look.spines.iter().enumerate().take(MAX_SPINE_SETS) {
        // At most eight sets.
        #[allow(clippy::cast_possible_truncation)]
        let index = index as u32;
        let n = kept_count(
            set,
            index,
            tuft.seed,
            tuft.along,
            count(set, index, tuft.seed),
        )
        .min(MAX_TUFT_SPINES - total);
        total += n;
        let all_round = set.arc >= 359.9;
        let rotation = draw(tuft.seed, index, SET, 14);
        let tip_colour = set.tip.unwrap_or(set.colour);
        let (colour, tip_colour) = match aged {
            Some(old) => (
                mix(set.colour, old, tuft.grey),
                mix(tip_colour, old, tuft.grey),
            ),
            None => (set.colour, tip_colour),
        };
        for i in 0..n {
            // Spine numbers are small.
            #[allow(clippy::cast_precision_loss)]
            let (fi, fn_) = (i as f32, n as f32);
            let azimuth = if all_round {
                single(set.aim)
                    + 360.0 * (fi + rotation) / fn_
                    + (draw(tuft.seed, index, i, 0) - 0.5) * 0.35 * 360.0 / fn_
            } else {
                single(set.aim)
                    + single(set.arc) * ((fi + 0.5) / fn_ - 0.5)
                    + (draw(tuft.seed, index, i, 0) - 0.5) * 0.7 * single(set.arc) / fn_
            };
            let elevation = single(set.angle)
                + single(set.angle_spread) * (2.0 * draw(tuft.seed, index, i, 1) - 1.0);
            let length = spine_length(set, index, tuft.seed, i, tuft.scale);
            let curve = single(set.curve) * (0.8 + 0.4 * draw(tuft.seed, index, i, 3));
            let (az, el) = (radians(azimuth), radians(elevation));
            let toward = add(scale(up, libm::cosf(az)), scale(side, libm::sinf(az)));
            let direction = add(scale(normal, libm::sinf(el)), scale(toward, libm::cosf(el)));
            // Bending about this axis by a negative angle turns the spine
            // from straight out toward its azimuth and from there back
            // toward the body.
            let axis = normalize(cross(toward, normal), side);
            let base = add(
                add(tuft.position, scale(normal, felt * 0.25)),
                scale(toward, felt * 0.45),
            );
            let bend = |share: f32| radians(curve * share);
            let hook = single(set.hook);
            let joints = hook_joints(hook);
            let pieces = [
                (joints[0], rotate(direction, axis, -bend(joints[0] * 0.5))),
                (
                    joints[1] - joints[0],
                    rotate(
                        direction,
                        axis,
                        -(bend((joints[0] + joints[1]) * 0.5) + radians(hook * 0.5)),
                    ),
                ),
                (
                    1.0 - joints[1],
                    rotate(
                        direction,
                        axis,
                        -(bend((joints[1] + 1.0) * 0.5) + radians(hook)),
                    ),
                ),
            ];
            let mut points = [base; 4];
            for (piece, (share, heading)) in pieces.iter().enumerate() {
                points[piece + 1] = add(points[piece], scale(*heading, length * share));
            }
            out.push(Spine {
                points,
                across: axis,
                radius: single(set.width) * 0.5,
                flat: single(set.flat),
                twist: single(set.twist),
                colour,
                tip: tip_colour,
            });
        }
    }
}

/// The local direction of `spine` at ring `ring` (0 to 2).
fn ring_heading(spine: &Spine, ring: usize) -> [f32; 3] {
    let p = &spine.points;
    let piece = |k: usize| normalize(sub(p[k + 1], p[k]), spine.across);
    match ring {
        0 => piece(0),
        1 => normalize(add(piece(0), piece(1)), piece(1)),
        _ => normalize(add(piece(1), piece(2)), piece(2)),
    }
}

/// One vertex of a spine's solid: its position, normal and colour.
/// Vertices 0 to 8 are the three rings, three each; 9 is the tip.
#[must_use]
pub fn solid_vertex(spine: &Spine, vertex: usize) -> ([f32; 3], [f32; 3], [f32; 3]) {
    if vertex >= 9 {
        let heading = ring_heading(spine, 2);
        return (spine.points[3], heading, spine.tip);
    }
    let (ring, corner) = (vertex / 3, vertex % 3);
    let heading = ring_heading(spine, ring);
    let across = spine.across;
    let inward = normalize(cross(heading, across), v(spine.across));
    // Corners at a third of a turn from each other, one facing along the
    // bend plane at the base, turned by the twist so far along.
    let along = [0.0, JOINTS[0], JOINTS[1]][ring];
    #[allow(clippy::cast_precision_loss)]
    let angle = TAU * corner as f32 / 3.0 + radians(spine.twist * along);
    let (c, s) = (libm::cosf(angle), libm::sinf(angle));
    let out = add(scale(inward, c), scale(across, s));
    let radius = spine.radius * TAPER[ring];
    let offset = add(
        scale(inward, c * radius * spine.flat),
        scale(across, s * radius),
    );
    (
        add(spine.points[ring], offset),
        normalize(out, heading),
        mix(spine.colour, spine.tip, along * along),
    )
}

/// The triangles of a spine's solid, as vertex numbers of
/// [`solid_vertex`], counter-clockwise seen from outside. A ring's corners
/// run clockwise seen from the spine's tip.
#[must_use]
pub fn solid_triangles() -> [[u8; 3]; SPINE_TRIANGLES] {
    let mut out = [[0; 3]; SPINE_TRIANGLES];
    let mut next = 0;
    for ring in 0..2u8 {
        for corner in 0..3u8 {
            let a = ring * 3 + corner;
            let b = ring * 3 + (corner + 1) % 3;
            let (c, d) = (a + 3, b + 3);
            out[next] = [a, d, b];
            out[next + 1] = [a, c, d];
            next += 2;
        }
    }
    for corner in 0..3u8 {
        out[next] = [6 + corner, 9, 6 + (corner + 1) % 3];
        next += 1;
    }
    out
}

/// One vertex of a tuft's felt: 0 is the top, 1 to 6 the rim.
#[must_use]
pub fn felt_vertex(tuft: &Tuft, look: &BodyLook, vertex: usize) -> ([f32; 3], [f32; 3]) {
    let normal = normalize(tuft.normal, [0.0, 0.0, 1.0]);
    let up = normalize(
        sub(tuft.up, scale(normal, dot(tuft.up, normal))),
        [0.0, 1.0, 0.0],
    );
    let side = cross(normal, up);
    let felt = single(look.areoles.felt_radius);
    if vertex == 0 {
        return (add(tuft.position, scale(normal, felt * 0.4)), normal);
    }
    #[allow(clippy::cast_precision_loss)]
    let angle = TAU * (vertex - 1) as f32 / 6.0;
    let out = add(scale(up, libm::cosf(angle)), scale(side, libm::sinf(angle)));
    (
        add(
            add(tuft.position, scale(out, felt)),
            scale(normal, -felt * 0.1),
        ),
        normalize(add(scale(out, 0.6), normal), normal),
    )
}

fn vector(a: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(a[0]), f64::from(a[1]), f64::from(a[2]))
}

/// Every tuft's felt and spines as solids, for drawing on the CPU (the
/// preview renderer). `looks` holds one look per body type, as tufts name
/// them.
#[must_use]
pub fn tuft_mesh(tufts: &[Tuft], looks: &[&BodyLook]) -> Mesh {
    tuft_mesh_within(tufts, looks, 1.0)
}

/// [`tuft_mesh`] with each tuft keeping `share` of its spines (at least
/// one), its longest: an export drawn to a budget (`crate::budget`).
#[must_use]
pub fn tuft_mesh_within(tufts: &[Tuft], looks: &[&BodyLook], share: f64) -> Mesh {
    let mut mesh = Mesh::default();
    let mut spines = Vec::new();
    let triangles = solid_triangles();
    for tuft in tufts {
        let Some(look) = looks.get(usize::from(tuft.body)) else {
            continue;
        };
        let shed = tuft.shed.is_finite().then_some(f64::from(tuft.shed));
        let born = f64::from(tuft.born);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        let felt = look.areoles.felt;
        for vertex in 0..FELT_VERTICES {
            let (position, normal) = felt_vertex(tuft, look, vertex);
            mesh.push(Vertex {
                position: vector(position),
                normal: vector(normal),
                uv: [0.0, 0.0],
                color: [felt[0], felt[1], felt[2], 1.0],
                born,
                shed,
                level: 0,
                segment: crate::mesh::NO_SEGMENT,
            });
        }
        for rim in 0..6u32 {
            mesh.indices
                .extend_from_slice(&[first, first + 1 + rim, first + 1 + (rim + 1) % 6]);
        }
        spines.clear();
        expand(tuft, look, &mut spines);
        keep_longest(&mut spines, share);
        for spine in &spines {
            let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for vertex in 0..SPINE_VERTICES {
                let (position, normal, colour) = solid_vertex(spine, vertex);
                mesh.push(Vertex {
                    position: vector(position),
                    normal: vector(normal),
                    uv: [0.0, 0.0],
                    color: [colour[0], colour[1], colour[2], 1.0],
                    born,
                    shed,
                    level: 3,
                    segment: crate::mesh::NO_SEGMENT,
                });
            }
            for triangle in &triangles {
                mesh.indices
                    .extend(triangle.iter().map(|&corner| first + u32::from(corner)));
            }
        }
    }
    mesh
}

/// Triangles [`tuft_mesh_within`] draws for `tufts`, without drawing
/// them.
#[must_use]
pub fn tuft_triangles(tufts: &[Tuft], looks: &[&BodyLook], share: f64) -> usize {
    let mut spines = Vec::new();
    tufts
        .iter()
        .filter_map(|tuft| {
            let look = looks.get(usize::from(tuft.body))?;
            spines.clear();
            expand(tuft, look, &mut spines);
            keep_longest(&mut spines, share);
            Some(FELT_TRIANGLES + spines.len() * SPINE_TRIANGLES)
        })
        .sum()
}

/// Keep `share` of `spines` (at least one), the longest, in their order.
fn keep_longest(spines: &mut Vec<Spine>, share: f64) {
    if share >= 1.0 || spines.len() <= 1 {
        return;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let kept = ((spines.len() as f64 * share.max(0.0)).ceil() as usize).clamp(1, spines.len());
    let length = |spine: &Spine| {
        let [base, _, _, tip] = spine.points;
        let d = sub(tip, base);
        dot(d, d)
    };
    let mut order: Vec<usize> = (0..spines.len()).collect();
    order.sort_by(|&a, &b| {
        length(&spines[b])
            .total_cmp(&length(&spines[a]))
            .then(a.cmp(&b))
    });
    let mut keep = vec![false; spines.len()];
    for &index in &order[..kept] {
        keep[index] = true;
    }
    let mut index = 0;
    spines.retain(|_| {
        index += 1;
        keep[index - 1]
    });
}

/// A tuft at the origin facing +Z with its tip toward +Y, as the card
/// templates draw it: full length, not greyed.
#[must_use]
pub fn sample_tuft(seed: u32) -> Tuft {
    Tuft {
        position: [0.0; 3],
        normal: [0.0, 0.0, 1.0],
        up: [0.0, 1.0, 0.0],
        scale: 1.0,
        grey: 0.0,
        seed,
        body: 0,
        born: 0.0,
        shed: f32::INFINITY,
        along: 1.0,
    }
}

/// Extent of a card drawn for one areole, metres: how far its spines reach
/// across the surface from the areole (`across`) and out from it (`out`),
/// from `samples` sample tufts so every tuft of the body fits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    pub across: f32,
    pub out: f32,
}

/// Seeds of the tufts a body's card templates and reach are drawn from.
const SAMPLE_SEEDS: [u32; 4] = [0x5EED_0001, 0x5EED_0002, 0x5EED_0003, 0x5EED_0004];

/// How far the spines of `look` reach, with a margin, for sizing its cards.
#[must_use]
pub fn reach(look: &BodyLook) -> Reach {
    let mut spines = Vec::new();
    for seed in SAMPLE_SEEDS {
        expand(&sample_tuft(seed), look, &mut spines);
    }
    let felt = single(look.areoles.felt_radius);
    let mut reach = Reach {
        across: felt * 1.2,
        out: felt * 0.6,
    };
    for spine in &spines {
        for point in spine.points {
            reach.across = reach.across.max(point[0].abs()).max(point[1].abs());
            reach.out = reach.out.max(point[2]);
        }
    }
    // Spines vary in length beyond the samples' draws.
    let spread = look
        .spines
        .iter()
        .map(|set| single(set.length_spread))
        .fold(0.0, f32::max);
    Reach {
        across: reach.across * (1.05 + spread * 0.5),
        out: reach.out * (1.05 + spread * 0.5),
    }
}

/// A card template drawn from spines: coverage, brightness and accent per
/// texel, `size` texels a side, rows from the card's base.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub aspect: f64,
    pub coverage: Vec<f32>,
    pub brightness: Vec<f32>,
    pub accent: Vec<f32>,
    /// The spines' mean colour, by the area they cover: the card's colour.
    pub colour: [f32; 3],
}

/// Which view of a tuft a card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Face on, along the areole's normal: the star of spines round the
    /// felt.
    Star,
    /// From the side, across the stem: the fan of spines standing out.
    Fan,
}

/// The colour of `look`'s spines seen together: each set's colour, a third
/// of the way to its tip's, weighted by the set's mean count, length and
/// width. The felt's colour for a body without spines. Cards drawn from the
/// spines take this colour, and their templates' brightness is relative to
/// it.
#[must_use]
pub fn colour(look: &BodyLook) -> [f32; 3] {
    let mut sum = [0.0_f64; 3];
    let mut weight_sum = 0.0;
    for set in &look.spines {
        let weight =
            f64::from(set.count[0] + set.count[1].max(set.count[0])) * 0.5 * set.length * set.width;
        let mean = mix(set.colour, set.tip.unwrap_or(set.colour), 0.35);
        for channel in 0..3 {
            sum[channel] += f64::from(mean[channel]) * weight;
        }
        weight_sum += weight;
    }
    if weight_sum > 0.0 {
        // Colours are 0 to 1.
        #[allow(clippy::cast_possible_truncation)]
        sum.map(|channel| (channel / weight_sum) as f32)
    } else {
        look.areoles.felt
    }
}

/// The template of `view` for `look`'s spines, `size` texels a side.
/// Spines thinner than a texel are drawn a texel wide so they survive the
/// alpha cut; the felt is the accent.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn template(look: &BodyLook, view: View, size: usize) -> Drawn {
    let reach = reach(look);
    let mut spines = Vec::new();
    expand(&sample_tuft(SAMPLE_SEEDS[0]), look, &mut spines);
    let felt = single(look.areoles.felt_radius);
    // Card units: the card's length is 1 along y, from its base.
    let (length, aspect) = match view {
        View::Star => (2.0 * reach.across, 1.0),
        View::Fan => (reach.out, 2.0 * reach.across / reach.out.max(1e-6)),
    };
    // A star card's left is the tuft's normal × up, -X for the sample
    // tuft; a fan card's is up, +Y.
    let project = |p: [f32; 3]| -> (f32, f32) {
        match view {
            View::Star => (-p[0] / length, p[1] / length + 0.5),
            View::Fan => (p[1] / length, p[2] / length),
        }
    };
    let colour = colour(look);
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mean_luminance = luminance(colour).max(1e-4);
    // Pieces in card units: ends, radii at the ends, colours at the ends.
    #[allow(clippy::cast_precision_loss)]
    let widest_texel = 0.55 * aspect.max(1.0) / size as f32;
    let mut pieces = Vec::with_capacity(spines.len() * 3);
    for (index, spine) in spines.iter().enumerate() {
        // A spine thinner than a texel is drawn a texel wide, so it
        // survives the alpha cut. Only so many of them are drawn that they
        // hide what the spines themselves would: a card of hair-fine
        // radials would otherwise draw white.
        let mean = spine.radius * (TAPER[0] + TAPER[1] + TAPER[2]) / 3.0 / length;
        #[allow(clippy::cast_possible_truncation)]
        let draw = crate::rng::unit(crate::rng::mix64(index as u64 ^ 0x5D1E)) as f32;
        if draw >= mean / widest_texel {
            continue;
        }
        let radii = [
            spine.radius * TAPER[0],
            spine.radius * TAPER[1],
            spine.radius * TAPER[2],
            0.0,
        ];
        let along = [0.0, JOINTS[0], JOINTS[1], 1.0];
        for piece in 0..3 {
            pieces.push((
                project(spine.points[piece]),
                project(spine.points[piece + 1]),
                radii[piece] / length,
                radii[piece + 1] / length,
                luminance(mix(spine.colour, spine.tip, along[piece] * along[piece]))
                    / mean_luminance,
                luminance(mix(
                    spine.colour,
                    spine.tip,
                    along[piece + 1] * along[piece + 1],
                )) / mean_luminance,
            ));
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let texel = 1.0 / size as f32;
    let aspect_f = aspect;
    let felt_card = felt / length;
    let mut drawn = Drawn {
        aspect: f64::from(aspect),
        coverage: Vec::with_capacity(size * size),
        brightness: Vec::with_capacity(size * size),
        accent: Vec::with_capacity(size * size),
        colour,
    };
    for row in 0..size {
        for column in 0..size {
            let mut cover = 0.0;
            let mut bright = 0.0;
            let mut tint = 0.0;
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (
                    ((column as f32 + dx) * texel - 0.5) * aspect_f,
                    (row as f32 + dy) * texel,
                );
                // The felt: a disc face on, a low mound from the side.
                let in_felt = match view {
                    View::Star => (x * x + (y - 0.5) * (y - 0.5)).sqrt() < felt_card,
                    View::Fan => x.abs() < felt_card && y < felt_card * 0.4,
                };
                let mut hit: Option<f32> = None;
                for &(a, b, ra, rb, la, lb) in &pieces {
                    let (ex, ey) = (b.0 - a.0, b.1 - a.1);
                    let span = ex * ex + ey * ey;
                    let t = if span > 0.0 {
                        (((x - a.0) * ex + (y - a.1) * ey) / span).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let (px, py) = (a.0 + ex * t - x, a.1 + ey * t - y);
                    let distance = (px * px + py * py).sqrt();
                    let radius = (ra + (rb - ra) * t).max(texel * 0.55 * aspect_f.max(1.0));
                    if distance < radius {
                        // Rounder toward the middle of the spine.
                        let shading = 0.8 + 0.3 * (1.0 - distance / radius);
                        let value = (la + (lb - la) * t) * shading;
                        hit = Some(hit.map_or(value, |old: f32| old.max(value)));
                    }
                }
                if let Some(value) = hit {
                    cover += 1.0;
                    bright += value;
                } else if in_felt {
                    cover += 1.0;
                    bright += 1.0;
                    tint += 1.0;
                }
            }
            drawn.coverage.push(cover / 4.0);
            if cover > 0.0 {
                drawn.brightness.push((bright / cover).clamp(0.0, 1.99));
                drawn.accent.push((tint / cover).clamp(0.0, 1.0));
            } else {
                drawn.brightness.push(1.0);
                drawn.accent.push(0.0);
            }
        }
    }
    drawn
}

/// Share of a body's surface its spines and felt hide, seen from outside,
/// for `area` square metres of surface per areole: the weight of the spine
/// colour where a far level draws only the body.
#[must_use]
pub fn cover(look: &BodyLook, area: f64) -> f64 {
    if area <= 0.0 {
        return 0.0;
    }
    let felt = look.areoles.felt_radius;
    let mut hidden = std::f64::consts::PI * felt * felt;
    for set in &look.spines {
        let count = f64::from(set.count[0] + set.count[1].max(set.count[0]))
            * 0.5
            * mean_thinning(set.distal);
        let lying = crate::math::cos(crate::math::radians(set.angle)).max(0.2);
        // Thin spines hide less than their outline: 0.35 keeps the far
        // levels' mean colour with the nearest level's across the cacti.
        hidden += count * set.length * lying * set.width * 0.35;
    }
    (hidden / area).clamp(0.0, 0.85)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{Areoles, BodyLook};

    fn look(sets: Vec<SpineSet>) -> BodyLook {
        BodyLook {
            areoles: Areoles {
                felt_radius: 0.003,
                ..Areoles::default()
            },
            spines: sets,
            ..BodyLook::plain([0.1, 0.25, 0.08])
        }
    }

    fn set(count: [u32; 2], angle: f64, aim: f64, arc: f64) -> SpineSet {
        SpineSet {
            count,
            angle,
            aim,
            arc,
            ..SpineSet::default()
        }
    }

    #[test]
    fn the_hash_matches_the_shaders_pcg() {
        // Values from types.wgsl's `pcg`, which lod.rs mirrors.
        assert_eq!(pcg(0), 129_708_002);
        assert_eq!(pcg(1), 2_831_084_092);
        let value = draw(7, 1, 2, 3);
        assert!((0.0..1.0).contains(&value));
        assert_eq!(value, draw(7, 1, 2, 3));
        assert_ne!(value, draw(7, 1, 2, 4));
    }

    #[test]
    fn a_tuft_draws_its_counts_and_the_same_spines_every_time() {
        let look = look(vec![
            set([8, 12], 15.0, 0.0, 360.0),
            set([1, 1], 60.0, 180.0, 0.0),
        ]);
        let tuft = sample_tuft(42);
        let mut first = Vec::new();
        expand(&tuft, &look, &mut first);
        let radial = count(&look.spines[0], 0, 42);
        assert!((8..=12).contains(&radial));
        assert_eq!(first.len(), radial as usize + 1);
        let mut second = Vec::new();
        expand(&tuft, &look, &mut second);
        assert_eq!(first, second);
        // The single central points out and down.
        let central = first.last().unwrap();
        let tip = central.points[3];
        assert!(tip[2] > 0.0 && tip[1] < 0.0, "{tip:?}");
    }

    #[test]
    fn radial_spines_lie_near_the_surface_all_round() {
        let look = look(vec![set([12, 12], 10.0, 0.0, 360.0)]);
        let mut spines = Vec::new();
        expand(&sample_tuft(3), &look, &mut spines);
        let mut angles: Vec<f32> = spines
            .iter()
            .map(|spine| {
                let tip = spine.points[3];
                assert!(tip[2] < 0.6 * (tip[0].hypot(tip[1])), "lies flat: {tip:?}");
                tip[1].atan2(tip[0])
            })
            .collect();
        angles.sort_by(f32::total_cmp);
        // Spread all round: no gap wider than a quarter turn.
        let mut widest = angles[0] + TAU - angles[angles.len() - 1];
        for pair in angles.windows(2) {
            widest = widest.max(pair[1] - pair[0]);
        }
        assert!(widest < TAU / 4.0, "gap {widest}");
    }

    #[test]
    fn a_set_kept_above_thins_toward_the_base() {
        let mut look = look(vec![SpineSet {
            count: [6, 6],
            distal: 1.0,
            ..SpineSet::default()
        }]);
        let at = |look: &BodyLook, along: f32| {
            let mut spines = Vec::new();
            let tuft = Tuft {
                along,
                ..sample_tuft(4)
            };
            expand(&tuft, look, &mut spines);
            spines.len()
        };
        assert_eq!(at(&look, 1.0), 6, "the tip keeps the whole set");
        assert_eq!(at(&look, 0.3), 0, "the lower half has none");
        let middle = at(&look, 0.8);
        assert!((3..=5).contains(&middle), "{middle}");
        look.spines[0].distal = 0.0;
        assert_eq!(at(&look, 0.0), 6, "an even set is whole at the base");
        assert!((kept(&look, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_twisted_spine_turns_its_cross_section() {
        let flat = look(vec![SpineSet {
            count: [1, 1],
            angle_spread: 0.0,
            arc: 0.0,
            length_spread: 0.0,
            flat: 0.3,
            ..SpineSet::default()
        }]);
        let mut twisted = flat.clone();
        twisted.spines[0].twist = 180.0;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        expand(&sample_tuft(9), &flat, &mut a);
        expand(&sample_tuft(9), &twisted, &mut b);
        // The wide corner's offset from the spine's axis at each ring.
        let wide = |spine: &Spine, ring: usize| {
            normalize(
                sub(solid_vertex(spine, ring * 3 + 1).0, spine.points[ring]),
                [0.0; 3],
            )
        };
        assert!(
            dot(wide(&a[0], 0), wide(&a[0], 2)) > 0.99,
            "an untwisted spine keeps its face"
        );
        assert!(
            (dot(wide(&b[0], 0), wide(&a[0], 0)) - 1.0).abs() < 1e-5,
            "the base is as before"
        );
        // Half a turn over the spine reaches 0.8 × 180° = 144° at the last ring.
        assert!(
            dot(wide(&b[0], 0), wide(&b[0], 2)) < -0.5,
            "the ribbon turns"
        );
    }

    #[test]
    fn a_hook_bends_the_tip_back() {
        let straight = look(vec![SpineSet {
            count: [1, 1],
            angle: 60.0,
            angle_spread: 0.0,
            aim: 180.0,
            arc: 0.0,
            length: 0.05,
            length_spread: 0.0,
            ..SpineSet::default()
        }]);
        let mut hooked = straight.clone();
        hooked.spines[0].hook = 150.0;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        expand(&sample_tuft(9), &straight, &mut a);
        expand(&sample_tuft(9), &hooked, &mut b);
        let heading = |spine: &Spine| normalize(sub(spine.points[3], spine.points[2]), [0.0; 3]);
        let first = normalize(sub(a[0].points[1], a[0].points[0]), [0.0; 3]);
        assert!(dot(heading(&a[0]), first) > 0.99);
        assert!(dot(heading(&b[0]), first) < -0.5, "a fishhook turns back");
        // An open J: the tip stands clear of the shaft, and the hook takes
        // only the end of the spine.
        let p = &b[0].points;
        let off = sub(p[3], p[0]);
        let across = sub(off, scale(first, dot(off, first)));
        let gap = dot(across, across).sqrt();
        assert!(
            gap > 0.1 * 0.05,
            "the hook's tip folds onto its shaft: {gap}"
        );
        assert!(
            dot(sub(p[1], p[0]), first) > 0.65 * 0.05,
            "the shaft runs on"
        );
    }

    #[test]
    fn solids_close_and_taper_to_the_tip() {
        let look = look(vec![set([3, 3], 30.0, 0.0, 360.0)]);
        let mesh = tuft_mesh(&[sample_tuft(5)], &[&look]);
        assert_eq!(mesh.vertex_count(), FELT_VERTICES + 3 * SPINE_VERTICES);
        assert_eq!(mesh.triangle_count(), FELT_TRIANGLES + 3 * SPINE_TRIANGLES);
        // Every triangle faces away from its spine's centreline.
        let mut spines = Vec::new();
        expand(&sample_tuft(5), &look, &mut spines);
        for (index, spine) in spines.iter().enumerate() {
            for triangle in solid_triangles() {
                let corners: Vec<[f32; 3]> = triangle
                    .iter()
                    .map(|&corner| solid_vertex(spine, usize::from(corner)).0)
                    .collect();
                let face = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
                let centre = scale(add(add(corners[0], corners[1]), corners[2]), 1.0 / 3.0);
                let ring = usize::from(triangle.iter().copied().min().unwrap() / 3).min(2);
                let axis = spine.points[ring];
                let outward = sub(centre, axis);
                let heading = ring_heading(spine, ring);
                let radial = sub(outward, scale(heading, dot(outward, heading)));
                assert!(
                    dot(face, radial) >= -1e-12,
                    "spine {index} triangle {triangle:?}"
                );
            }
        }
    }

    #[test]
    fn templates_cover_the_spines_and_mark_the_felt() {
        let look = look(vec![
            set([10, 14], 15.0, 0.0, 360.0),
            set([2, 4], 70.0, 180.0, 90.0),
        ]);
        let star = template(&look, View::Star, 64);
        let covered = star.coverage.iter().filter(|&&c| c >= 0.5).count();
        assert!(covered > 40, "star covers {covered} texels");
        let centre = 32 * 64 + 32;
        assert!(star.accent[centre] > 0.5, "the felt is the accent");
        let fan = template(&look, View::Fan, 64);
        assert!(fan.aspect > 0.5);
        assert!(fan.coverage.iter().any(|&c| c >= 0.5));
        assert_eq!(template(&look, View::Star, 64), star);
    }
}
