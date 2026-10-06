//! Roots above the ground (plant forms F6): prop roots, aerial roots,
//! knees, breathing roots (pneumatophores) and surface roots, drawn from a
//! species' root description into the plant's wood mesh.
//!
//! A [`Roots`] description says how many of each a mature plant has; a
//! plant `a` years old has `round(n · min(1, a / mature_age))` of each, so
//! they appear as it grows. Every root is a tube along a quadratic Bézier
//! curve, its radius narrowing from its foot to its end, placed round the
//! stem by the golden angle and jittered by hashes of the plant's first
//! segment, so a plant keeps its roots from keyframe to keyframe:
//!
//! - a **prop root** leaves the stem at a height up to `prop_height` of the
//!   plant's height and arches out and down to the ground `prop_reach`
//!   metres beyond the stem (red mangrove);
//! - an **aerial root** hangs from a branch of the crown, from the point
//!   where the branch is `aerial_reach` of the crown's radius out, straight
//!   down to the ground, and thickens into a pillar (strangler figs,
//!   banyans);
//! - a **knee** is a cone rising `knee_height` from a ring `knee_reach`
//!   metres round the stem (bald cypress);
//! - a **pneumatophore** is a thin pencil `pneumatophore_height` tall, many
//!   scattered over a disc `pneumatophore_reach` round the stem (black
//!   mangrove);
//! - a **surface root** runs out along the ground from the stem's foot,
//!   half sunk, `surface_length` long.
//!
//! The roots are geometry only: the plant graph, its shading and its pipe
//! radii are unchanged. Prop, aerial and surface roots are drawn on every
//! level that draws the stem; knees and pneumatophores only on the nearest
//! two.

use serde::{Deserialize, Serialize};

use crate::graph::PlantGraph;
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::{Mesh, Vertex};
use crate::rng::{mix64, unit};

/// A species' roots above the ground. Counts are a mature plant's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Roots {
    /// Age at which a plant has all its roots, years.
    pub mature_age: f64,
    pub prop: u32,
    /// Highest a prop root leaves the stem, as a share of the plant's height.
    pub prop_height: f64,
    /// How far beyond the stem a prop root reaches the ground, metres.
    pub prop_reach: f64,
    /// A prop root's radius at the stem, metres.
    pub prop_radius: f64,
    pub aerial: u32,
    /// Where on its branch an aerial root hangs, as a share of the crown's radius.
    pub aerial_reach: f64,
    /// An aerial root's radius at the ground, metres.
    pub aerial_radius: f64,
    pub knees: u32,
    pub knee_height: f64,
    pub knee_reach: f64,
    pub knee_radius: f64,
    pub pneumatophores: u32,
    pub pneumatophore_height: f64,
    pub pneumatophore_reach: f64,
    pub surface: u32,
    pub surface_length: f64,
    /// A surface root's radius at the stem, metres.
    pub surface_radius: f64,
}

impl Default for Roots {
    fn default() -> Self {
        Self {
            mature_age: 20.0,
            prop: 0,
            prop_height: 0.25,
            prop_reach: 1.5,
            prop_radius: 0.04,
            aerial: 0,
            aerial_reach: 0.6,
            aerial_radius: 0.05,
            knees: 0,
            knee_height: 0.4,
            knee_reach: 1.5,
            knee_radius: 0.08,
            pneumatophores: 0,
            pneumatophore_height: 0.2,
            pneumatophore_reach: 3.0,
            surface: 0,
            surface_length: 1.5,
            surface_radius: 0.06,
        }
    }
}

/// Sides round a root.
const AROUND: u32 = 6;

/// Draw the roots of `graph` into `mesh` in `colour` (the bark's), those of
/// the nearest levels only when `near`.
#[allow(clippy::too_many_lines)]
pub fn build(graph: &PlantGraph, roots: &Roots, colour: [f32; 3], near: bool, mesh: &mut Mesh) {
    let Some(stem) = graph.segments.first() else {
        return;
    };
    let seed = stem.id;
    let foot = Vec3::new(stem.start.x, 0.0, stem.start.z);
    let share = if roots.mature_age > 0.0 {
        (graph.age / roots.mature_age).clamp(0.0, 1.0)
    } else {
        1.0
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = |n: u32| (f64::from(n) * share).round() as u32;
    let born = (graph.age - 1.0).max(0.0);
    let stem_radius = stem.radius;
    let golden = math::radians(137.507_764);
    let jitter = |i: u32, salt: u64| unit(mix64(seed ^ mix64(u64::from(i) ^ salt)));

    // The stem's points from its foot up, to find where a prop root leaves it.
    let mut trunk = vec![(stem.start, stem.radius)];
    let mut at = 0usize;
    while let Some(next) = graph
        .segments
        .iter()
        .position(|s| s.parent == u32::try_from(at).ok() && !s.lateral)
    {
        trunk.push((graph.segments[next].start, graph.segments[next].radius));
        at = next;
    }
    trunk.push((graph.segments[at].end, graph.segments[at].radius * 0.8));
    let on_stem = |height: f64| {
        trunk
            .iter()
            .find(|(point, _)| point.y >= height)
            .or(trunk.last())
            .copied()
            .unwrap_or((foot, stem_radius))
    };

    for i in 0..count(roots.prop) {
        let angle = golden * f64::from(i) + jitter(i, 1) * 0.5;
        let out = Vec3::new(math::cos(angle), 0.0, math::sin(angle));
        let height = graph.height * roots.prop_height * (0.25 + 0.75 * jitter(i, 2));
        let (point, radius) = on_stem(height);
        let start = point + out * radius;
        let reach = roots.prop_reach * (0.6 + 0.4 * jitter(i, 3)) * (0.5 + 0.5 * share);
        let end = Vec3::new(point.x, 0.0, point.z) + out * (radius + reach);
        let control = start + out * (reach * 0.7) + Vec3::Y * (height * 0.25);
        let r = roots.prop_radius * (0.7 + 0.3 * jitter(i, 4)) * (0.4 + 0.6 * share);
        tube(
            start,
            control,
            end,
            r.min(radius),
            r * 0.8,
            10,
            colour,
            born,
            mesh,
        );
    }

    // Aerial roots hang from the crown's larger branches.
    let crown: Vec<usize> = graph
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.order >= 1 && s.end.y > 1.5 && s.radius > 0.02)
        .map(|(index, _)| index)
        .collect();
    let crown_radius = crown
        .iter()
        .map(|&i| horizontal(graph.segments[i].end - foot))
        .fold(0.0, f64::max);
    for i in 0..count(roots.aerial) {
        if crown.is_empty() || crown_radius <= 0.0 {
            break;
        }
        let wanted = crown_radius * roots.aerial_reach * (0.6 + 0.6 * jitter(i, 5));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let pick = crown[(jitter(i, 6) * crown.len() as f64) as usize % crown.len()];
        let segment = &graph.segments[pick];
        let distance = horizontal(segment.end - foot);
        if distance < wanted * 0.5 {
            continue;
        }
        let top = segment.end;
        let bottom = Vec3::new(top.x, 0.0, top.z);
        let r = roots.aerial_radius * (0.5 + 0.5 * jitter(i, 7)) * share;
        tube(
            bottom,
            (bottom + top) * 0.5,
            top,
            r.max(0.004),
            (r * 0.3).max(0.003),
            6,
            colour,
            born,
            mesh,
        );
    }

    // Surface roots run out along the ground, half sunk.
    for i in 0..count(roots.surface) {
        let angle = golden * f64::from(i) + jitter(i, 8);
        let out = Vec3::new(math::cos(angle), 0.0, math::sin(angle));
        let start = foot + out * (stem_radius * 0.8);
        let length = roots.surface_length * (0.6 + 0.4 * jitter(i, 9)) * share;
        let side = Vec3::new(-out.z, 0.0, out.x) * (length * 0.25 * (jitter(i, 10) - 0.5));
        let end = start + out * length + side;
        let r = roots.surface_radius * (0.6 + 0.4 * jitter(i, 11)) * share;
        tube(
            start,
            (start + end) * 0.5 + side,
            end,
            r,
            r * 0.2,
            6,
            colour,
            born,
            mesh,
        );
    }

    if !near {
        return;
    }
    for i in 0..count(roots.knees) {
        let angle = golden * f64::from(i) + jitter(i, 12);
        let distance = stem_radius + roots.knee_reach * math::sqrt(jitter(i, 13));
        let base = foot + Vec3::new(math::cos(angle), 0.0, math::sin(angle)) * distance;
        let height = roots.knee_height * (0.4 + 0.6 * jitter(i, 14)) * share;
        let tip = base + Vec3::Y * height;
        tube(
            base,
            (base + tip) * 0.5,
            tip,
            roots.knee_radius * (0.6 + 0.4 * jitter(i, 15)),
            0.004,
            3,
            colour,
            born,
            mesh,
        );
    }
    for i in 0..count(roots.pneumatophores) {
        let angle = golden * f64::from(i) + jitter(i, 16);
        let distance = stem_radius + roots.pneumatophore_reach * math::sqrt(jitter(i, 17));
        let base = foot + Vec3::new(math::cos(angle), 0.0, math::sin(angle)) * distance;
        let height = roots.pneumatophore_height * (0.5 + 0.5 * jitter(i, 18)) * share;
        let tip = base
            + Vec3::Y * height
            + Vec3::new(jitter(i, 19) - 0.5, 0.0, jitter(i, 20) - 0.5) * (height * 0.15);
        tube(
            base,
            (base + tip) * 0.5,
            tip,
            0.006,
            0.003,
            2,
            colour,
            born,
            mesh,
        );
    }
}

fn horizontal(v: Vec3) -> f64 {
    math::sqrt(v.x * v.x + v.z * v.z)
}

/// A tube along the quadratic Bézier curve `a`, `b`, `c` in `steps`
/// pieces, its radius going from `r0` to `r1`, with a cone's point at `c`
/// when `r1` is near zero.
#[allow(clippy::too_many_arguments)]
fn tube(
    a: Vec3,
    b: Vec3,
    c: Vec3,
    r0: f64,
    r1: f64,
    steps: u32,
    colour: [f32; 3],
    born: f64,
    mesh: &mut Mesh,
) {
    if r0.is_nan() || r0 <= 0.0 || (c - a).length() <= 1.0e-6 {
        return;
    }
    let point = |t: f64| a * ((1.0 - t) * (1.0 - t)) + b * (2.0 * (1.0 - t) * t) + c * (t * t);
    let tangent =
        |t: f64| ((b - a) * (2.0 * (1.0 - t)) + (c - b) * (2.0 * t)).normalize_or(Vec3::Y);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    let mut side = any_perpendicular(tangent(0.0));
    for step in 0..=steps {
        let t = f64::from(step) / f64::from(steps);
        let along = tangent(t);
        // Carry the side round the curve so the rings do not twist.
        side = (side - along * side.dot(along)).normalize_or(any_perpendicular(along));
        let other = along.cross(side);
        let radius = r0 + (r1 - r0) * t;
        let centre = point(t);
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let normal = side * math::cos(theta) + other * math::sin(theta);
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            mesh.push(Vertex {
                position: centre + normal * radius,
                normal,
                uv: [k as f32 / AROUND as f32, t as f32],
                color: [colour[0], colour[1], colour[2], 1.0],
                born,
                shed: None,
                level: 0,
            });
        }
    }
    for step in 0..steps {
        for k in 0..AROUND {
            let a0 = first + step * AROUND + k;
            let a1 = first + step * AROUND + (k + 1) % AROUND;
            let (b0, b1) = (a0 + AROUND, a1 + AROUND);
            mesh.indices.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphSegment;

    fn trunk(height: f64) -> PlantGraph {
        let segment = |index: u32, y0: f64, y1: f64| GraphSegment {
            id: 42,
            parent: index.checked_sub(1),
            lateral: index == 0,
            order: 0,
            start: Vec3::new(0.0, y0, 0.0),
            end: Vec3::new(0.0, y1, 0.0),
            left: Vec3::ZERO,
            radius: 0.2,
            born: 0.0,
            shed: None,
            body: 0,
        };
        PlantGraph {
            age: 30.0,
            height,
            segments: (0..10)
                .map(|i| {
                    segment(
                        i,
                        f64::from(i) * height / 10.0,
                        f64::from(i + 1) * height / 10.0,
                    )
                })
                .collect(),
            organs: Vec::new(),
        }
    }

    #[test]
    fn prop_roots_reach_the_ground_beyond_the_stem() {
        let roots = Roots {
            prop: 8,
            prop_height: 0.3,
            prop_reach: 2.0,
            ..Roots::default()
        };
        let mut mesh = Mesh::default();
        build(&trunk(10.0), &roots, [0.4, 0.3, 0.2], true, &mut mesh);
        assert_eq!(mesh.triangle_count(), 8 * 10 * AROUND as usize * 2);
        let lowest = mesh.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(lowest < 0.05, "a prop root reaches the ground: {lowest}");
        let widest = mesh
            .positions
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0, f32::max);
        assert!(widest > 1.2, "beyond the stem: {widest}");
        let highest = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(highest <= 3.3, "from at most 30 % of the height: {highest}");
    }

    #[test]
    fn knees_and_pneumatophores_only_near_and_with_age() {
        let roots = Roots {
            knees: 10,
            pneumatophores: 40,
            ..Roots::default()
        };
        let mut far = Mesh::default();
        build(&trunk(10.0), &roots, [0.4; 3], false, &mut far);
        assert_eq!(far.triangle_count(), 0);
        let mut near = Mesh::default();
        build(&trunk(10.0), &roots, [0.4; 3], true, &mut near);
        assert_eq!(
            near.triangle_count(),
            (10 * 3 + 40 * 2) * AROUND as usize * 2
        );
        let mut young = trunk(10.0);
        young.age = 5.0;
        let mut few = Mesh::default();
        build(&young, &roots, [0.4; 3], true, &mut few);
        assert!(few.triangle_count() < near.triangle_count() / 2);
    }
}
