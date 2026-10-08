//! Light from an overcast sky among a plant's own leaves, for drawing its
//! wood: the light model of `light@1` (`crate::lsys::tools`) evaluated
//! after growth, through the leaves of one keyframe, at any point.
//!
//! Renderers light wood by the sun and an even sky, so limbs deep in a
//! crown would read as brightly as a stem in the open, where photographs
//! show them dark. The mesh darkens bark by this light instead
//! (`crate::mesh`), on the scale leaves are coloured by
//! ([`crate::looks::sunlit`]).
//!
//! Each living organ adds the leaf area its card draws (its cut-out's
//! share of the card, [`crate::templates::fill_share`], times the card's
//! area) to the voxel holding it, giving a leaf area density `D` per voxel.
//! Along each sky direction `j`, with path length `ℓ_j` per layer,
//!
//! ```text
//! τ_j = k · ℓ_j · (Σ D over the voxels beyond the point's + ½ D of its own voxel)
//! light = Σ_j w_j · exp(−τ_j) / Σ_j w_j
//! ```
//!
//! with `light@1`'s sky directions and weights, its default voxel and its
//! extinction coefficient for leaves spread at random. Only the plant's own
//! leaves shade: the neighbours of a forest-grown plant are the scene's.

use crate::graph::PlantGraph;
use crate::looks::Look;
use crate::math::{self, Vec3};

/// Voxel edge, metres: `light@1`'s default.
const CELL: f64 = 0.5;

/// Light extinction per unit of leaf area density: `light@1`'s value for
/// leaves spread at random through each voxel.
const EXTINCTION: f64 = 0.5;

/// Most voxels a field holds; a larger plant gets coarser voxels.
const MAX_VOXELS: usize = 1 << 19;

/// Sky directions as voxel steps `(dx, dz)` per layer up, in rings of
/// equal zenith angle, as `light@1`'s: straight up, then 45°, 54.7°,
/// 63.4° and 70.5° from vertical.
const SKY_RINGS: [&[(i64, i64)]; 5] = [
    &[(0, 0)],
    &[(1, 0), (0, 1), (-1, 0), (0, -1)],
    &[(1, 1), (-1, 1), (-1, -1), (1, -1)],
    &[(2, 0), (0, 2), (-2, 0), (0, -2)],
    &[(2, 2), (-2, 2), (-2, -2), (2, -2)],
];

/// One sky direction: its voxel step, path length per layer in voxels, and
/// weight.
struct Direction {
    step: (i64, i64),
    cells_per_layer: f64,
    weight: f64,
}

/// The sky directions with `light@1`'s weights for an overcast sky seen
/// by a small sphere: each ring stands for the band of zenith angles
/// halfway to its neighbours, weighted by the band's solid angle and the
/// overcast radiance `(1 + 2 cos θ) / 3`, shared among its directions.
fn directions() -> Vec<Direction> {
    #[allow(clippy::cast_precision_loss)]
    let zenith: Vec<f64> = SKY_RINGS
        .iter()
        .map(|ring| {
            let (dx, dz) = ring[0];
            math::atan(math::sqrt((dx * dx + dz * dz) as f64))
        })
        .collect();
    let mut out = Vec::new();
    for (index, ring) in SKY_RINGS.iter().enumerate() {
        let low = if index == 0 {
            0.0
        } else {
            f64::midpoint(zenith[index - 1], zenith[index])
        };
        let high = if index + 1 == SKY_RINGS.len() {
            math::PI * 0.5
        } else {
            f64::midpoint(zenith[index], zenith[index + 1])
        };
        let solid_angle = 2.0 * math::PI * (math::cos(low) - math::cos(high));
        let radiance = (1.0 + 2.0 * math::cos(zenith[index])) / 3.0;
        #[allow(clippy::cast_precision_loss)]
        let weight = solid_angle * radiance / ring.len() as f64;
        for &(dx, dz) in *ring {
            #[allow(clippy::cast_precision_loss)]
            let (x, z) = (dx as f64, dz as f64);
            out.push(Direction {
                step: (dx, dz),
                cells_per_layer: math::sqrt(x * x + 1.0 + z * z),
                weight,
            });
        }
    }
    out
}

/// The sky light field of one keyframe's leaves.
pub struct SkyLight {
    cell: f64,
    /// Voxel index of the grid's first voxel on each axis.
    low: [i64; 3],
    size: [usize; 3],
    density: Vec<f32>,
    directions: Vec<Direction>,
    /// Per direction, each voxel's leaf area density summed with every
    /// voxel's past it along the direction.
    beyond: Vec<Vec<f32>>,
    total_weight: f64,
}

impl SkyLight {
    /// The field of `graph`'s organs living at its age, each with the leaf
    /// area its look among `looks` draws, over a grid that also holds the
    /// plant's wood. `None` when the plant has no living organ.
    #[must_use]
    pub fn of(graph: &PlantGraph, looks: &[Look]) -> Option<Self> {
        let drawn: Vec<f64> = looks
            .iter()
            .map(|look| {
                let shape = &look.shape;
                crate::templates::fill_share(shape)
                    * shape.aspect()
                    * (1.0 + shape.cross().unwrap_or(0.0))
            })
            .collect();
        let living = |shed: Option<f64>| shed.is_none_or(|shed| shed > graph.age);
        let occluders: Vec<(Vec3, f64)> = graph
            .organs
            .iter()
            .filter(|organ| living(organ.shed))
            .map(|organ| {
                let area = drawn.get(usize::from(organ.organ)).copied().unwrap_or(0.0);
                (organ.position, area * organ.size * organ.size)
            })
            .filter(|(_, area)| *area > 0.0)
            .collect();
        if occluders.is_empty() {
            return None;
        }
        let mut lower = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut upper = -lower;
        let points = occluders.iter().map(|(point, _)| *point).chain(
            graph
                .segments
                .iter()
                .filter(|segment| living(segment.shed))
                .flat_map(|segment| [segment.start, segment.end]),
        );
        for point in points {
            lower = Vec3::new(
                lower.x.min(point.x),
                lower.y.min(point.y),
                lower.z.min(point.z),
            );
            upper = Vec3::new(
                upper.x.max(point.x),
                upper.y.max(point.y),
                upper.z.max(point.z),
            );
        }
        let mut cell = CELL;
        let (low, size) = loop {
            let index = |value: f64| index(value, cell);
            let low = [index(lower.x), index(lower.y), index(lower.z)];
            let high = [index(upper.x), index(upper.y), index(upper.z)];
            let size =
                [0, 1, 2].map(|axis| usize::try_from(high[axis] - low[axis] + 1).unwrap_or(1));
            if size[0].saturating_mul(size[1]).saturating_mul(size[2]) <= MAX_VOXELS {
                break (low, size);
            }
            cell *= 2.0;
        };
        let volume = cell * cell * cell;
        let mut field = Self {
            cell,
            low,
            size,
            density: vec![0.0; size[0] * size[1] * size[2]],
            directions: directions(),
            beyond: Vec::new(),
            total_weight: 0.0,
        };
        for (point, area) in &occluders {
            if let Some(voxel) = field.voxel(*point) {
                let at = field.flat(voxel);
                #[allow(clippy::cast_possible_truncation)]
                let added = (area / volume) as f32;
                field.density[at] += added;
            }
        }
        field.total_weight = field
            .directions
            .iter()
            .map(|direction| direction.weight)
            .sum();
        field.beyond = field
            .directions
            .iter()
            .map(|direction| field.sweep(direction.step))
            .collect();
        Some(field)
    }

    /// Share of open-sky light reaching `point` through the leaves, 0 to 1;
    /// 1 outside the grid.
    #[must_use]
    pub fn at(&self, point: Vec3) -> f64 {
        let Some(voxel) = self.voxel(point) else {
            return 1.0;
        };
        let own = f64::from(self.density[self.flat(voxel)]);
        let mut light = 0.0;
        for (direction, beyond) in self.directions.iter().zip(&self.beyond) {
            let past = self
                .next(voxel, direction.step)
                .map_or(0.0, |next| f64::from(beyond[self.flat(next)]));
            let depth = EXTINCTION * self.cell * direction.cells_per_layer * (past + 0.5 * own);
            light += direction.weight * math::exp(-depth);
        }
        (light / self.total_weight).clamp(0.0, 1.0)
    }

    /// Each voxel's density summed with every voxel's past it along
    /// `step`, by one sweep from the top layer down.
    fn sweep(&self, step: (i64, i64)) -> Vec<f32> {
        let [nx, ny, nz] = self.size;
        let mut beyond = vec![0.0_f32; self.density.len()];
        for y in (0..ny).rev() {
            for z in 0..nz {
                for x in 0..nx {
                    let here = [x, y, z];
                    let past = self
                        .next(here, step)
                        .map_or(0.0, |next| beyond[self.flat(next)]);
                    beyond[self.flat(here)] = self.density[self.flat(here)] + past;
                }
            }
        }
        beyond
    }

    fn voxel(&self, point: Vec3) -> Option<[usize; 3]> {
        let at = |value: f64, axis: usize| {
            usize::try_from(index(value, self.cell) - self.low[axis])
                .ok()
                .filter(|&index| index < self.size[axis])
        };
        Some([at(point.x, 0)?, at(point.y, 1)?, at(point.z, 2)?])
    }

    /// The voxel one layer up along `step` from `[x, y, z]`, if inside.
    fn next(&self, [x, y, z]: [usize; 3], (dx, dz): (i64, i64)) -> Option<[usize; 3]> {
        let x = usize::try_from(i64::try_from(x).ok()? + dx).ok()?;
        let z = usize::try_from(i64::try_from(z).ok()? + dz).ok()?;
        (x < self.size[0] && y + 1 < self.size[1] && z < self.size[2]).then_some([x, y + 1, z])
    }

    fn flat(&self, [x, y, z]: [usize; 3]) -> usize {
        (y * self.size[2] + z) * self.size[0] + x
    }
}

/// The voxel index along one axis of a coordinate, for voxels of edge
/// `cell`. Plant coordinates are tens of metres, so the cast keeps them.
#[allow(clippy::cast_possible_truncation)]
fn index(value: f64, cell: f64) -> i64 {
    (value / cell).floor() as i64
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::graph::{GraphOrgan, GraphSegment};
    use crate::looks::Shape;

    fn leaf(position: Vec3) -> GraphOrgan {
        GraphOrgan {
            id: 0,
            organ: 0,
            segment: None,
            position,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 0.5,
            born: 0.0,
            shed: None,
            light: 1.0,
        }
    }

    fn stem(top: f64) -> GraphSegment {
        GraphSegment {
            id: 0,
            parent: None,
            lateral: false,
            order: 0,
            start: Vec3::ZERO,
            end: Vec3::new(0.0, top, 0.0),
            radius: 0.1,
            born: 0.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
        }
    }

    fn looks() -> Vec<Look> {
        let organs = std::collections::BTreeMap::from([(
            "leaf".to_string(),
            crate::looks::OrganLook {
                shape: Shape::Simple(crate::looks::Simple::default()),
                colour: None,
                shade: None,
                accent: None,
                face_up: 0.0,
                solid: None,
                bend: None,
                form: None,
                season: None,
                families: None,
            },
        )]);
        crate::looks::resolve(
            [("leaf", crate::lsys::program::OrganKind::Leaf)],
            &organs,
            [0.1, 0.3, 0.1],
            [0.05, 0.1, 0.05],
        )
    }

    /// A ball of leaves 3 m across round (0, 6, 0), on a stem.
    fn crown() -> PlantGraph {
        let mut organs = Vec::new();
        for i in -6_i32..=6 {
            for j in -6_i32..=6 {
                for k in -6_i32..=6 {
                    let p = Vec3::new(f64::from(i), f64::from(j), f64::from(k)) * 0.25;
                    if p.length() <= 1.5 {
                        organs.push(leaf(p + Vec3::new(0.0, 6.0, 0.0)));
                    }
                }
            }
        }
        PlantGraph {
            age: 10.0,
            height: 7.5,
            segments: vec![stem(6.0)],
            organs,
        }
    }

    #[test]
    fn wood_inside_a_crown_sees_less_sky_than_wood_below_or_above_it() {
        let graph = crown();
        let field = SkyLight::of(&graph, &looks()).unwrap();
        let inside = field.at(Vec3::new(0.0, 6.0, 0.0));
        let below = field.at(Vec3::new(0.0, 2.0, 0.0));
        let top = field.at(Vec3::new(0.0, 7.4, 0.0));
        // About exp(-k D r) through 1.5 m of leaf area density D near 3.
        assert!(inside < 0.2, "inside {inside}");
        assert!(below > inside && top > inside, "below {below}, top {top}");
        // Under the crown the sky overhead is gone, but not the sky round it.
        assert!(below > 0.3 && below < 0.95, "below {below}");
        assert_eq!(field.at(Vec3::new(0.0, 50.0, 0.0)), 1.0);
    }

    #[test]
    fn shed_organs_and_leafless_plants_cast_no_shade() {
        let mut graph = crown();
        for organ in &mut graph.organs {
            organ.shed = Some(5.0);
        }
        assert!(SkyLight::of(&graph, &looks()).is_none());
    }
}
