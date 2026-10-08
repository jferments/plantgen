//! Environment tools: the "open L-system" half of plant growth.
//!
//! After each step is drawn, the configured tools look at the scene and
//! write values that the next step's rules read:
//!
//! - `light@1`: light from an overcast sky through the plant's own foliage
//!   (Beer-Lambert in a voxel grid, after the shadow voxels of Pałubicki et
//!   al. 2009) and through the synthetic [`Neighbourhood`] it grows in.
//! - `space@1`: space colonization (Runions et al. 2007): attraction points
//!   in a crown envelope pull the apices that perceive them.
//! - `vigour@1`: the Borchert-Honda resource model: light collected by the
//!   tips flows to the base and back out, split by apical control.
//! - `pipe@1`: the pipe model of stem radii (Shinozaki et al. 1964).
//! - `host@1`: the distance and direction from each module to the wood of
//!   the host a climber, epiphyte or parasite grows on (plant forms F7).
//!
//! Every tool is deterministic: the same scene gives the same values on every
//! machine.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::derive::Clock;
use super::expr::{ENV_FIELDS, EnvField, EnvValues, NO_ENV, Query, Scope, eval};
use super::program::{Program, SymbolKind, ToolConfig, ToolKind};
use super::turtle::{NodeKind, Scene};
use super::{GrowthError, Limits};
use crate::conditions::Surroundings;
use crate::math::{self, Vec3};
use crate::rng::{hash_words, unit};

/// The synthetic stand a plant variant grows in. The compiler grows each
/// variant inside one of these, so a forest-grown tree really has a high,
/// narrow crown and an open-grown one keeps its low limbs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Neighbourhood {
    /// Shade per metre of neighbouring canopy above a point, 1/m. Zero is
    /// open ground.
    pub density: f64,
    /// Top of the neighbouring canopy as a multiple of the plant's height.
    pub relative_height: f64,
    /// Lowest top of the neighbouring canopy, in metres: an older stand
    /// overhead.
    pub canopy: f64,
    /// Distance between stems, in metres. Foliage farther than half of it
    /// from the trunk reaches into the neighbours' crowns.
    pub spacing: f64,
    /// Neighbours stand only on the −X side (a stand edge).
    pub one_sided: bool,
}

impl Neighbourhood {
    pub const OPEN: Self = Self {
        density: 0.0,
        relative_height: 0.0,
        canopy: 0.0,
        spacing: 0.0,
        one_sided: false,
    };

    /// Fraction of the sky light arriving along `direction` (a unit vector
    /// pointing up toward the sky) that reaches `point` through the
    /// neighbours, for a plant currently `height` metres tall.
    ///
    /// The neighbours form a canopy layer up to `top`, the higher of
    /// `relative_height · height` and `canopy`. A ray from a point `above`
    /// metres below `top` crosses `above / cos θ` metres of it, more where
    /// the point reaches into the neighbours' crowns (farther than half the
    /// spacing from the trunk). At a stand edge only rays passing over the
    /// −X side cross the full canopy.
    #[must_use]
    pub fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        if self.density <= 0.0 {
            return 1.0;
        }
        let top = (self.relative_height * height).max(self.canopy);
        let above = (top - point.y).max(0.0);
        // Near-horizontal rays are capped at five times the vertical path.
        let rise = direction.y.max(0.2);
        let path = above / rise;
        let half = self.spacing * 0.5;
        let radial = math::sqrt(point.x * point.x + point.z * point.z);
        let lateral = if half > 0.0 {
            1.0 + (radial - half).max(0.0) / half
        } else {
            1.0
        };
        let side = if self.one_sided {
            // Where the ray is halfway through the canopy layer.
            let middle = point.x + direction.x / rise * above * 0.5;
            0.3 + 0.7 * math::smoothstep(half, -half, middle)
        } else {
            1.0
        };
        math::exp(-self.density * path * lateral * side)
    }
}

/// Values of one tool's settings at one step.
fn settings(config: &ToolConfig, globals: &[f64], clock: Clock, stack: &mut Vec<f64>) -> Vec<f64> {
    let scope = Scope {
        globals,
        locals: &[],
        env: &NO_ENV,
        t: clock.t,
        dt: clock.dt,
        age: 0.0,
        step: f64::from(clock.step),
        key: 0,
    };
    config
        .settings
        .iter()
        .map(|code| eval(code, &scope, stack))
        .collect()
}

fn invalid(tool: &'static str, message: String) -> GrowthError {
    GrowthError::Tool { tool, message }
}

/// State a tool keeps between steps.
#[derive(Debug, Default)]
pub struct ToolState {
    /// Attraction points already consumed, by lattice cell.
    killed: HashSet<(i32, i32, i32)>,
    /// Lattice spacing fixed at the first step, so cells keep their identity.
    lattice: Option<f64>,
}

/// A host plant's wood as `host@1` sees it: its segments as capsules, in a
/// grid of `HOST_CELL` cells for nearest-point queries (plant forms F7).
#[derive(Debug, Clone, PartialEq)]
pub struct Host {
    capsules: Vec<(Vec3, Vec3, f64)>,
    cells: std::collections::HashMap<(i32, i32, i32), Vec<u32>>,
}

/// Edge of a host grid cell, metres.
pub const HOST_CELL: f64 = 0.5;

impl Host {
    /// The host whose wood is `capsules`: each a segment's start, end and
    /// radius, in the guest's frame (the host stands at the origin).
    #[must_use]
    pub fn new(capsules: Vec<(Vec3, Vec3, f64)>) -> Self {
        let mut cells: std::collections::HashMap<(i32, i32, i32), Vec<u32>> =
            std::collections::HashMap::new();
        for (index, &(a, b, r)) in capsules.iter().enumerate() {
            let low = cell_of(a.min(b) - Vec3::new(r, r, r));
            let high = cell_of(a.max(b) + Vec3::new(r, r, r));
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    for z in low.2..=high.2 {
                        #[allow(clippy::cast_possible_truncation)]
                        cells.entry((x, y, z)).or_default().push(index as u32);
                    }
                }
            }
        }
        Self { capsules, cells }
    }

    /// The distance from `point` to the host's surface (negative inside
    /// its wood) and the unit direction to the nearest surface point, when
    /// the host is within `reach`.
    #[must_use]
    pub fn nearest(&self, point: Vec3, reach: f64) -> Option<(f64, Vec3)> {
        let low = cell_of(point - Vec3::new(reach, reach, reach));
        let high = cell_of(point + Vec3::new(reach, reach, reach));
        let mut best: Option<(f64, Vec3)> = None;
        let mut seen = HashSet::new();
        for x in low.0..=high.0 {
            for y in low.1..=high.1 {
                for z in low.2..=high.2 {
                    let Some(list) = self.cells.get(&(x, y, z)) else {
                        continue;
                    };
                    for &index in list {
                        if !seen.insert(index) {
                            continue;
                        }
                        let (a, b, r) = self.capsules[index as usize];
                        let axis = b - a;
                        let along = (point - a).dot(axis) / axis.dot(axis).max(1.0e-12);
                        let closest = a + axis * along.clamp(0.0, 1.0);
                        let offset = closest - point;
                        let centre = offset.length();
                        let surface = centre - r;
                        if surface <= reach && best.is_none_or(|(d, _)| surface < d) {
                            best = Some((surface, offset * (1.0 / centre.max(1.0e-9))));
                        }
                    }
                }
            }
        }
        best
    }
}

fn cell_of(point: Vec3) -> (i32, i32, i32) {
    #[allow(clippy::cast_possible_truncation)]
    let cell = |v: f64| (v / HOST_CELL).floor() as i32;
    (cell(point.x), cell(point.y), cell(point.z))
}

/// Results of running the tools on one scene.
#[derive(Debug, Default)]
pub struct ToolOutput {
    /// Environment values per querying module, in string order.
    pub env: Vec<(u32, EnvValues)>,
    /// Light at each organ (1 when there is no light tool).
    pub organ_light: Vec<f64>,
}

/// Run every configured tool on a scene.
///
/// # Errors
///
/// Fails on invalid tool settings or when a grid or point limit is exceeded.
#[allow(clippy::too_many_arguments)]
pub fn run(
    program: &Program,
    globals: &[f64],
    organ_area: &[f64],
    clock: Clock,
    scene: &Scene,
    surroundings: &dyn Surroundings,
    state: &mut ToolState,
    limits: &Limits,
    seed: u64,
) -> Result<ToolOutput, GrowthError> {
    let mut stack = Vec::new();
    let (offsets, children) = scene.children();
    let has_children = |node: u32| offsets[node as usize + 1] > offsets[node as usize];
    let anchors: Vec<bool> = scene
        .queries
        .iter()
        .map(|query| !has_children(query.node))
        .collect();

    let (query_light, organ_light) = match program.tool(ToolKind::Light) {
        Some(config) => {
            let values = settings(config, globals, clock, &mut stack);
            light(scene, &anchors, organ_area, &values, surroundings, limits)?
        }
        None => (
            vec![1.0; scene.queries.len()],
            vec![1.0; scene.organs.len()],
        ),
    };

    let space_queries: Vec<bool> = scene
        .queries
        .iter()
        .map(
            |query| match program.symbols[usize::from(query.symbol)].kind {
                SymbolKind::Module { queries } => queries & Query::Space.bit() != 0,
                _ => false,
            },
        )
        .collect();
    let space = match program.tool(ToolKind::Space) {
        Some(config) => {
            let values = settings(config, globals, clock, &mut stack);
            colonize(scene, &space_queries, &values, state, limits, seed)?
        }
        None => vec![(0, Vec3::ZERO); scene.queries.len()],
    };

    let flux = match program.tool(ToolKind::Vigour) {
        Some(config) => {
            let values = settings(config, globals, clock, &mut stack);
            let lit = program.tool(ToolKind::Light).is_some();
            Some(vigour(
                scene,
                &offsets,
                &children,
                &query_light,
                lit,
                &values,
            )?)
        }
        None => None,
    };

    // The host: distance and direction to its surface from each module.
    let host_reach = program.tool(ToolKind::Host).map(|config| {
        let values = settings(config, globals, clock, &mut stack);
        values[0].max(0.0)
    });

    let mut env = Vec::with_capacity(scene.queries.len());
    for (index, query) in scene.queries.iter().enumerate() {
        let mut values = [0.0; ENV_FIELDS];
        values[EnvField::Light as usize] = query_light[index];
        if let Some(flux) = &flux {
            let node = query.node as usize;
            values[EnvField::Vigour as usize] = flux.vigour[node];
            values[EnvField::Qsum as usize] = flux.light[node];
            values[EnvField::Nseg as usize] = f64::from(flux.segments[node]);
            values[EnvField::Ntip as usize] = f64::from(flux.tips[node]);
        }
        let (count, direction) = space[index];
        values[EnvField::Space as usize] = f64::from(count);
        values[EnvField::Sx as usize] = direction.x;
        values[EnvField::Sy as usize] = direction.y;
        values[EnvField::Sz as usize] = direction.z;
        values[EnvField::Px as usize] = query.position.x;
        values[EnvField::Py as usize] = query.position.y;
        values[EnvField::Pz as usize] = query.position.z;
        values[EnvField::Hx as usize] = query.heading.x;
        values[EnvField::Hy as usize] = query.heading.y;
        values[EnvField::Hz as usize] = query.heading.z;
        values[EnvField::Order as usize] = f64::from(query.order);
        values[EnvField::Height as usize] = scene.height;
        if let Some(reach) = host_reach {
            let found = surroundings
                .host()
                .and_then(|host| host.nearest(query.position, reach));
            let (distance, direction) = found.unwrap_or((reach + 1.0, Vec3::ZERO));
            values[EnvField::Gd as usize] = distance;
            values[EnvField::Gx as usize] = direction.x;
            values[EnvField::Gy as usize] = direction.y;
            values[EnvField::Gz as usize] = direction.z;
        }
        env.push((query.module, values));
    }
    Ok(ToolOutput { env, organ_light })
}

/// Sky directions for `light@1` as voxel steps `(dx, dz)` per layer up,
/// in rings of equal zenith angle: straight up, then 45°, 54.7°, 63.4° and
/// 70.5° from vertical, alternating between the axes and the diagonals.
const SKY_RINGS: [&[(i64, i64)]; 5] = [
    &[(0, 0)],
    &[(1, 0), (0, 1), (-1, 0), (0, -1)],
    &[(1, 1), (-1, 1), (-1, -1), (1, -1)],
    &[(2, 0), (0, 2), (-2, 0), (0, -2)],
    &[(2, 2), (-2, 2), (-2, -2), (2, -2)],
];

/// One sky direction: its voxel step, unit vector, path length per layer in
/// cells, and weight.
struct SkyDirection {
    step: (i64, i64),
    unit: Vec3,
    cells_per_layer: f64,
    weight: f64,
}

/// The sky directions with weights for a standard overcast sky seen by a
/// small sphere: each ring stands for the band of zenith angles halfway to
/// its neighbours, weighted by the band's solid angle and the overcast
/// radiance `(1 + 2 cos θ) / 3`, shared among the ring's directions.
fn sky_directions() -> Vec<SkyDirection> {
    #[allow(clippy::cast_precision_loss)]
    let zenith: Vec<f64> = SKY_RINGS
        .iter()
        .map(|ring| {
            let (dx, dz) = ring[0];
            math::atan(math::sqrt((dx * dx + dz * dz) as f64))
        })
        .collect();
    let mut directions = Vec::new();
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
            let cells_per_layer = math::sqrt(x * x + 1.0 + z * z);
            directions.push(SkyDirection {
                step: (dx, dz),
                unit: Vec3::new(x, 1.0, z) / cells_per_layer,
                cells_per_layer,
                weight,
            });
        }
    }
    directions
}

/// The `light` tool's settings: voxel size, extinction coefficient and the
/// leaf area of a living tip.
fn light_settings(values: &[f64]) -> Result<(f64, f64, f64), GrowthError> {
    let [cell, extinction, bud] = values[..] else {
        return Err(invalid("light", "wrong number of settings".into()));
    };
    if !(cell >= 0.01 && cell.is_finite()) {
        return Err(invalid(
            "light",
            format!("cell must be at least 0.01 m, found {cell}"),
        ));
    }
    if !(extinction >= 0.0 && extinction.is_finite() && bud >= 0.0 && bud.is_finite()) {
        return Err(invalid(
            "light",
            format!(
                "extinction and bud must be finite and not negative; found {extinction} and {bud}"
            ),
        ));
    }
    Ok((cell, extinction, bud))
}

/// Everything that receives and casts shade, with its own leaf area: the
/// queries first (living tips add `bud`), then the organs.
fn light_receivers(
    scene: &Scene,
    anchors: &[bool],
    organ_area: &[f64],
    bud: f64,
) -> Vec<(Vec3, f64)> {
    let queries = scene
        .queries
        .iter()
        .zip(anchors)
        .map(|(query, anchor)| (query.position, if *anchor { bud } else { 0.0 }));
    let organs = scene.organs.iter().map(|organ| {
        let area = organ_area[usize::from(organ.symbol)] * organ.size * organ.size;
        (organ.position, area)
    });
    queries.chain(organs).collect()
}

/// Light from an overcast sky through the plant's foliage and its
/// neighbours, as a fraction of open-sky light.
///
/// Organs add their leaf area `area · size²` to the voxel holding them and
/// living tips add `bud` square metres, giving a leaf area density `D` per
/// voxel. Along each sky direction `j`, with path length `ℓ_j` per layer,
///
/// ```text
/// τ_j = k · ℓ_j · (Σ D over the voxels beyond the receiver's + ½ (D_own voxel − own area / cell³))
/// light = Σ_j w_j · T_j · exp(−τ_j) / Σ_j w_j
/// ```
///
/// where `k` is the extinction coefficient and `T_j` the neighbourhood's
/// transmission. The voxel sums along each direction come from one sweep of
/// the grid per direction, so the cost is linear in voxels and receivers.
fn light(
    scene: &Scene,
    anchors: &[bool],
    organ_area: &[f64],
    values: &[f64],
    surroundings: &dyn Surroundings,
    limits: &Limits,
) -> Result<(Vec<f64>, Vec<f64>), GrowthError> {
    let (cell, extinction, bud) = light_settings(values)?;
    let receivers = light_receivers(scene, anchors, organ_area, bud);
    let split = scene.queries.len();
    let directions = sky_directions();
    let total_weight: f64 = directions.iter().map(|direction| direction.weight).sum();
    let neighbours = |point: Vec3, direction: &SkyDirection| {
        surroundings.transmission(point, scene.height, direction.unit)
    };
    if receivers.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let index = |value: f64| -> i64 {
        // Positions are bounded by the segment limit times segment length;
        // saturating conversion keeps absurd programs from wrapping.
        #[allow(clippy::cast_possible_truncation)]
        let cell_index = (value / cell).floor() as i64;
        cell_index
    };
    let mut low = [i64::MAX; 3];
    let mut high = [i64::MIN; 3];
    for (point, _) in &receivers {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let cell_index = index(value);
            low[axis] = low[axis].min(cell_index);
            high[axis] = high[axis].max(cell_index);
        }
    }
    let size = [0, 1, 2].map(|axis| high[axis] - low[axis] + 1);
    let voxels = size
        .iter()
        .map(|s| u128::try_from(*s).unwrap_or(u128::MAX))
        .product::<u128>();
    if voxels > limits.max_voxels as u128 {
        return Err(GrowthError::Limit {
            what: "light voxels",
            limit: limits.max_voxels as u64,
        });
    }
    let [nx, ny, nz] = size.map(|s| usize::try_from(s).unwrap_or(0));
    let voxel_of = |point: Vec3| -> [usize; 3] {
        let to = |value: f64, axis: usize| usize::try_from(index(value) - low[axis]).unwrap_or(0);
        [to(point.x, 0), to(point.y, 1), to(point.z, 2)]
    };
    let flat = |[x, y, z]: [usize; 3]| (y * nz + z) * nx + x;
    // The voxel one step along a direction from `[x, y, z]`, if inside.
    let next = |[x, y, z]: [usize; 3], (dx, dz): (i64, i64)| -> Option<[usize; 3]> {
        let x = usize::try_from(i64::try_from(x).ok()? + dx).ok()?;
        let z = usize::try_from(i64::try_from(z).ok()? + dz).ok()?;
        (x < nx && y + 1 < ny && z < nz).then_some([x, y + 1, z])
    };

    let volume = cell * cell * cell;
    let mut density = vec![0.0_f64; nx * ny * nz];
    let cells: Vec<[usize; 3]> = receivers
        .iter()
        .map(|(point, _)| voxel_of(*point))
        .collect();
    for (voxel, (_, own)) in cells.iter().zip(&receivers) {
        density[flat(*voxel)] += own / volume;
    }

    let mut light = vec![0.0_f64; receivers.len()];
    let mut beyond = vec![0.0_f64; nx * ny * nz];
    for direction in &directions {
        // beyond[v] = Σ D over v and every voxel past it along the direction.
        for y in (0..ny).rev() {
            for z in 0..nz {
                for x in 0..nx {
                    let here = [x, y, z];
                    let past = next(here, direction.step).map_or(0.0, |voxel| beyond[flat(voxel)]);
                    beyond[flat(here)] = density[flat(here)] + past;
                }
            }
        }
        let length = extinction * cell * direction.cells_per_layer;
        for (receiver, ((point, own), voxel)) in receivers.iter().zip(&cells).enumerate() {
            let past = next(*voxel, direction.step).map_or(0.0, |voxel| beyond[flat(voxel)]);
            let shared = (density[flat(*voxel)] - own / volume).max(0.0);
            let depth = length * (past + 0.5 * shared);
            light[receiver] += direction.weight * neighbours(*point, direction) * math::exp(-depth);
        }
    }
    for value in &mut light {
        *value = (*value / total_weight).clamp(0.0, 1.0);
    }
    let organs = light.split_off(split);
    Ok((light, organs))
}

/// A uniform hash grid for neighbour searches.
struct PointGrid {
    cell: f64,
    cells: HashMap<(i64, i64, i64), Vec<u32>>,
}

impl PointGrid {
    fn new(cell: f64, points: impl Iterator<Item = (u32, Vec3)>) -> Self {
        let mut grid = Self {
            cell,
            cells: HashMap::new(),
        };
        for (index, point) in points {
            grid.cells.entry(grid.key(point)).or_default().push(index);
        }
        grid
    }

    #[allow(clippy::cast_possible_truncation)]
    fn key(&self, point: Vec3) -> (i64, i64, i64) {
        (
            (point.x / self.cell).floor() as i64,
            (point.y / self.cell).floor() as i64,
            (point.z / self.cell).floor() as i64,
        )
    }

    /// Indices stored in the 27 cells around `point`. The order depends only
    /// on insertion order within each cell, and callers take minima, so the
    /// result never depends on hash-map layout.
    fn near(&self, point: Vec3) -> impl Iterator<Item = u32> + '_ {
        let (x, y, z) = self.key(point);
        (-1..=1).flat_map(move |dx| {
            (-1..=1).flat_map(move |dy| {
                (-1..=1).flat_map(move |dz| {
                    self.cells
                        .get(&(x + dx, y + dy, z + dz))
                        .map(|list| list.iter().copied())
                        .into_iter()
                        .flatten()
                })
            })
        })
    }
}

/// Radius of the crown envelope at relative height `h` in `[0, 1]`.
fn envelope_radius(shape: f64, radius: f64, h: f64) -> f64 {
    if !(0.0..=1.0).contains(&h) {
        return -1.0;
    }
    // Shapes are whole numbers in the language; anything else is rounded.
    #[allow(clippy::cast_possible_truncation)]
    match shape.round() as i64 {
        // Cone: widest at the bottom, a point at the top.
        2 => radius * (1.0 - h),
        // Paraboloid: a rounded cone.
        3 => radius * math::sqrt(1.0 - h),
        // Cylinder.
        4 => radius,
        // Ellipsoid.
        _ => radius * math::sqrt((1.0 - (2.0 * h - 1.0) * (2.0 * h - 1.0)).max(0.0)),
    }
}

const SALT_POINT: u64 = 0x7370_6163;

/// Space colonization (Runions, Lane and Prusinkiewicz 2007). Attraction
/// points sit one per cell of a jittered lattice inside the envelope. A point
/// within `kill` of the plant is consumed for good; otherwise it attracts the
/// nearest apex that declared `queries space` within `influence` and inside
/// its perception cone. Each apex gets the count and the mean direction.
fn colonize(
    scene: &Scene,
    space_queries: &[bool],
    values: &[f64],
    state: &mut ToolState,
    limits: &Limits,
    seed: u64,
) -> Result<Vec<(u32, Vec3)>, GrowthError> {
    let [shape, base, height, radius, density, influence, kill, angle] = values[..] else {
        return Err(invalid("space", "wrong number of settings".into()));
    };
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("space", "every setting must be finite".into()));
    }
    if !(density > 0.0 && influence > 0.0 && kill >= 0.0) {
        return Err(invalid(
            "space",
            format!("density and influence must be positive; found {density} and {influence}"),
        ));
    }
    let spacing = *state.lattice.get_or_insert(math::pow(density, -1.0 / 3.0));
    let mut result = vec![(0_u32, Vec3::ZERO); scene.queries.len()];
    if height <= base || radius <= 0.0 {
        return Ok(result);
    }
    #[allow(clippy::cast_possible_truncation)]
    let range = |low: f64, high: f64| {
        (
            (low / spacing).floor() as i64,
            (high / spacing).floor() as i64,
        )
    };
    let (x0, x1) = range(-radius, radius);
    let (y0, y1) = range(base, height);
    let span = |(a, b): (i64, i64)| u128::try_from(b - a + 1).unwrap_or(0);
    if span((x0, x1)) * span((x0, x1)) * span((y0, y1)) > limits.max_points as u128 {
        return Err(GrowthError::Limit {
            what: "attraction points",
            limit: limits.max_points as u64,
        });
    }

    let plant_points: Vec<Vec3> = scene
        .segments
        .iter()
        .flat_map(|segment| [segment.start, segment.end])
        .chain(scene.queries.iter().map(|query| query.position))
        .collect();
    // Separate grids sized to each search radius keep both searches to the
    // 27 cells around a point.
    let plant = PointGrid::new(
        kill.max(0.05),
        plant_points
            .iter()
            .enumerate()
            .map(|(index, point)| (u32::try_from(index).unwrap_or(u32::MAX), *point)),
    );
    let apices = PointGrid::new(
        influence,
        scene
            .queries
            .iter()
            .enumerate()
            .filter(|(index, _)| space_queries[*index])
            .map(|(index, query)| (u32::try_from(index).unwrap_or(u32::MAX), query.position)),
    );
    let cone = math::cos(math::radians(angle.clamp(0.0, 180.0)));
    let mut sums = vec![Vec3::ZERO; scene.queries.len()];
    let kill_sq = kill * kill;
    let influence_sq = influence * influence;
    let to_i32 = |value: i64| i32::try_from(value).unwrap_or(i32::MAX);

    for y in y0..=y1 {
        for z in x0..=x1 {
            for x in x0..=x1 {
                let key = (to_i32(x), to_i32(y), to_i32(z));
                if state.killed.contains(&key) {
                    continue;
                }
                let point = attraction_point(seed, [x, y, z], spacing);
                let relative = (point.y - base) / (height - base);
                let radial = math::sqrt(point.x * point.x + point.z * point.z);
                if radial > envelope_radius(shape, radius, relative) {
                    continue;
                }
                let consumed = plant.near(point).any(|index| {
                    (plant_points[index as usize] - point).length_squared() <= kill_sq
                });
                if consumed {
                    state.killed.insert(key);
                    continue;
                }
                if let Some((distance_sq, index)) =
                    claimant(scene, &apices, point, influence_sq, cone)
                {
                    sums[index as usize] +=
                        (point - scene.queries[index as usize].position) / math::sqrt(distance_sq);
                    result[index as usize].0 += 1;
                }
            }
        }
    }
    for (entry, sum) in result.iter_mut().zip(sums) {
        entry.1 = sum.normalize_or(Vec3::ZERO);
    }
    Ok(result)
}

/// The attraction point of lattice cell `cell`: jittered inside the cell by
/// draws keyed on the cell, so it never moves between steps.
fn attraction_point(seed: u64, cell: [i64; 3], spacing: f64) -> Vec3 {
    let [x, y, z] = cell;
    // Lattice coordinates are small whole numbers; the casts keep their bits.
    #[allow(clippy::cast_sign_loss)]
    let hash = hash_words(&[seed, SALT_POINT, x as u64, y as u64, z as u64]);
    #[allow(clippy::cast_precision_loss)]
    let point = Vec3::new(
        (x as f64 + unit(hash)) * spacing,
        (y as f64 + unit(crate::rng::mix64(hash))) * spacing,
        (z as f64 + unit(crate::rng::mix64(hash ^ 0x9e37_79b9))) * spacing,
    );
    point
}

/// The apex that takes an attraction point: the nearest one that perceives
/// it within `influence_sq` and inside its cone, with its squared distance.
/// Buds at one node share a position, so a tie goes to the bud facing the
/// point most directly, then to the earlier bud.
fn claimant(
    scene: &Scene,
    apices: &PointGrid,
    point: Vec3,
    influence_sq: f64,
    cone: f64,
) -> Option<(f64, u32)> {
    let mut best: Option<(f64, f64, u32)> = None;
    for index in apices.near(point) {
        let query = &scene.queries[index as usize];
        let offset = point - query.position;
        let distance_sq = offset.length_squared();
        if distance_sq > influence_sq || distance_sq <= 0.0 {
            continue;
        }
        let facing = query.heading.dot(offset) / math::sqrt(distance_sq);
        if facing < cone {
            continue;
        }
        let better = best.is_none_or(|(distance, most_facing, earliest)| {
            distance_sq
                .total_cmp(&distance)
                .then(most_facing.total_cmp(&facing))
                .then(index.cmp(&earliest))
                .is_lt()
        });
        if better {
            best = Some((distance_sq, facing, index));
        }
    }
    best.map(|(distance_sq, _, index)| (distance_sq, index))
}

/// Per-node results of the Borchert-Honda model.
pub struct Flux {
    pub light: Vec<f64>,
    pub vigour: Vec<f64>,
    pub segments: Vec<u32>,
    /// Tips after each node in its branch, not counting the node itself.
    pub tips: Vec<u32>,
}

/// The Borchert-Honda resource model as used by Pałubicki et al. (2009).
///
/// Light `Q` collected by the tips (1 per tip without a light tool) is summed
/// toward the base. The base receives `v = min(alpha · Q_base, max)`; at each
/// node the resource is split between the children as
///
/// ```text
/// v_c = v · w_c Q_c / Σ w Q,   w = lambda on the continuing axis, 1 - lambda on laterals
/// ```
///
/// so `lambda` near 1 gives a dominant leader (excurrent) and near 0.5 a
/// spreading crown (decurrent).
fn vigour(
    scene: &Scene,
    offsets: &[u32],
    children: &[u32],
    query_light: &[f64],
    lit: bool,
    values: &[f64],
) -> Result<Flux, GrowthError> {
    let [lambda, alpha, cap] = values[..] else {
        return Err(invalid("vigour", "wrong number of settings".into()));
    };
    if !(0.0..=1.0).contains(&lambda) || !alpha.is_finite() || cap.is_nan() {
        return Err(invalid(
            "vigour",
            format!("lambda must be 0 to 1 and alpha finite; found {lambda} and {alpha}"),
        ));
    }
    let count = scene.nodes.len();
    let mut light = vec![0.0; count];
    let mut segments = vec![0_u32; count];
    let mut tips = vec![0_u32; count];
    for index in (0..count).rev() {
        let node = scene.nodes[index];
        let span = offsets[index] as usize..offsets[index + 1] as usize;
        let leaf = span.is_empty();
        let mut total = 0.0;
        let mut segment_count = u32::from(matches!(node.kind, NodeKind::Segment(_)));
        let mut tip_count = 0;
        for &child in &children[span] {
            let child = child as usize;
            total += light[child];
            segment_count += segments[child];
            let child_is_tip = offsets[child + 1] == offsets[child]
                && matches!(scene.nodes[child].kind, NodeKind::Query(_));
            tip_count += tips[child] + u32::from(child_is_tip);
        }
        tips[index] = tip_count;
        if leaf && let NodeKind::Query(query) = node.kind {
            total += if lit {
                query_light[query as usize]
            } else {
                1.0
            };
        }
        light[index] = total;
        segments[index] = segment_count;
    }
    let mut vigour = vec![0.0; count];
    for index in 0..count {
        let node = scene.nodes[index];
        if node.parent.is_none() {
            vigour[index] = (alpha * light[index]).min(cap);
        }
        let span = offsets[index] as usize..offsets[index + 1] as usize;
        if span.is_empty() {
            continue;
        }
        let weight = |child: u32| {
            if scene.nodes[child as usize].lateral {
                1.0 - lambda
            } else {
                lambda
            }
        };
        let denominator: f64 = children[span.clone()]
            .iter()
            .map(|&child| weight(child) * light[child as usize])
            .sum();
        for &child in &children[span] {
            vigour[child as usize] = if denominator > 0.0 {
                vigour[index] * weight(child) * light[child as usize] / denominator
            } else {
                0.0
            };
        }
    }
    Ok(Flux {
        light,
        vigour,
        segments,
        tips,
    })
}

/// Radii from the pipe model and the leaf area each segment carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Pipes {
    /// Pipe-model radius per segment.
    pub radii: Vec<f64>,
    /// Leaf area (m²) of the organs above each segment.
    pub leaf_area: Vec<f64>,
    /// Ring area each segment adds per unit of time: `rings` times its leaf
    /// area (Pressler's law).
    pub ring_rate: f64,
}

/// The pipe model: a segment's radius `r` satisfies
///
/// ```text
/// r^n = Σ r_child^n + (tips above it) · tip^n + Σ (organ · size)^n
/// ```
///
/// with the exponent `n` usually between 2 and 3. The pipe radius describes
/// the living plant at one moment; the growth loop also adds a yearly ring
/// of area `rings · leaf area above` to every segment (Pressler's law: ring
/// area is proportional to the foliage above), so stems keep thickening
/// after their crowns stop growing. A segment's radius is the larger of
/// the two, and never shrinks.
///
/// # Errors
///
/// Fails on invalid settings.
pub fn pipe(scene: &Scene, organ_area: &[f64], values: &[f64]) -> Result<Pipes, GrowthError> {
    let [exponent, tip, organ, rings] = values[..] else {
        return Err(invalid("pipe", "wrong number of settings".into()));
    };
    // Written so that NaN settings fail too.
    let valid = (1.0..=4.0).contains(&exponent)
        && tip > 0.0
        && organ >= 0.0
        && rings >= 0.0
        && rings.is_finite();
    if !valid {
        return Err(invalid(
            "pipe",
            format!(
                "exponent must be 1 to 4, tip positive, organ and rings not negative; found {exponent}, {tip}, {organ} and {rings}"
            ),
        ));
    }
    let (offsets, children) = scene.children();
    let count = scene.nodes.len();
    let mut flow = vec![0.0; count];
    let mut leaf_area = vec![0.0; count];
    let tip_flow = math::pow(tip, exponent);
    for organ_instance in &scene.organs {
        if let Some(node) = organ_instance.node {
            flow[node as usize] += math::pow(organ * organ_instance.size.abs(), exponent);
            leaf_area[node as usize] += organ_area[usize::from(organ_instance.symbol)]
                * organ_instance.size
                * organ_instance.size;
        }
    }
    for index in (0..count).rev() {
        let span = offsets[index] as usize..offsets[index + 1] as usize;
        if span.is_empty() && matches!(scene.nodes[index].kind, NodeKind::Query(_)) {
            flow[index] += tip_flow;
        }
        let from_children: f64 = children[span.clone()]
            .iter()
            .map(|&child| flow[child as usize])
            .sum();
        let area_above: f64 = children[span]
            .iter()
            .map(|&child| leaf_area[child as usize])
            .sum();
        flow[index] += from_children;
        leaf_area[index] += area_above;
    }
    Ok(Pipes {
        radii: scene
            .segments
            .iter()
            .map(|segment| {
                let value = flow[segment.node as usize];
                if value > 0.0 {
                    math::pow(value, 1.0 / exponent)
                } else {
                    tip
                }
            })
            .collect(),
        leaf_area: scene
            .segments
            .iter()
            .map(|segment| leaf_area[segment.node as usize])
            .collect(),
        ring_rate: rings,
    })
}

/// Shading area per unit size² of every organ symbol (0 for other symbols).
///
/// # Errors
///
/// Fails if an area is negative or not finite.
pub fn organ_areas(program: &Program, globals: &[f64]) -> Result<Vec<f64>, GrowthError> {
    let mut stack = Vec::new();
    program
        .symbols
        .iter()
        .map(|symbol| match &symbol.kind {
            SymbolKind::Organ { area, .. } => {
                let scope = Scope {
                    globals,
                    locals: &[],
                    env: &NO_ENV,
                    t: 0.0,
                    dt: 0.0,
                    age: 0.0,
                    step: 0.0,
                    key: 0,
                };
                let value = eval(area, &scope, &mut stack);
                if value.is_finite() && value >= 0.0 {
                    Ok(value)
                } else {
                    Err(GrowthError::Settings(format!(
                        "organ `{}` has area {value}; it must be zero or positive",
                        symbol.name
                    )))
                }
            }
            _ => Ok(0.0),
        })
        .collect()
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::lsys::derive::Deriver;
    use crate::lsys::turtle::Interpreter;
    use std::collections::BTreeMap;

    struct Setup {
        program: Program,
        globals: Vec<f64>,
        scene: Scene,
    }

    fn setup(source: &str) -> Setup {
        let program = Program::compile(source).unwrap();
        let globals = program.resolve_params(&BTreeMap::new()).unwrap();
        let limits = Limits::default();
        let string = Deriver::new(&program, &globals, &limits)
            .axiom(5, 1.0)
            .unwrap();
        let clock = Clock {
            step: 0,
            t: 0.0,
            dt: 1.0,
        };
        let scene = Interpreter::new(&program, &globals, &limits)
            .interpret(&string, clock)
            .unwrap();
        Setup {
            program,
            globals,
            scene,
        }
    }

    fn tools(setup: &Setup, neighbourhood: &Neighbourhood) -> ToolOutput {
        let areas = organ_areas(&setup.program, &setup.globals).unwrap();
        let clock = Clock {
            step: 0,
            t: 0.0,
            dt: 1.0,
        };
        run(
            &setup.program,
            &setup.globals,
            &areas,
            clock,
            &setup.scene,
            neighbourhood,
            &mut ToolState::default(),
            &Limits::default(),
            5,
        )
        .unwrap()
    }

    fn leaf(position: Vec3) -> crate::lsys::turtle::OrganInstance {
        crate::lsys::turtle::OrganInstance {
            id: 0,
            symbol: 16,
            position,
            frame: crate::math::Frame::UPRIGHT,
            size: 1.0,
            born: 0.0,
            segment: None,
            node: None,
        }
    }

    fn bud(position: Vec3) -> crate::lsys::turtle::QueryPoint {
        crate::lsys::turtle::QueryPoint {
            module: 0,
            symbol: 17,
            node: 0,
            position,
            heading: Vec3::Y,
            order: 0,
        }
    }

    #[test]
    fn a_roof_of_leaves_shades_what_is_under_it_more_than_what_is_beside_it() {
        // A 14 m square roof of leaves, 3 m up, one leaf per voxel.
        let mut scene = Scene::default();
        for i in -14..=14 {
            for j in -14..=14 {
                scene
                    .organs
                    .push(leaf(Vec3::new(f64::from(i) * 0.5, 3.0, f64::from(j) * 0.5)));
            }
        }
        scene.queries = vec![
            bud(Vec3::ZERO),
            bud(Vec3::new(12.0, 0.0, 0.0)),
            bud(Vec3::new(0.0, 5.0, 0.0)),
        ];
        let mut organ_area = vec![0.0; 18];
        organ_area[16] = 0.5;
        let (queries, organs) = light(
            &scene,
            &[true, true, true],
            &organ_area,
            &[0.5, 0.5, 0.0],
            &Neighbourhood::OPEN,
            &Limits::default(),
        )
        .unwrap();
        let [under, beside, above] = queries[..] else {
            panic!("three buds");
        };
        assert!(under < 0.35, "a bud under the roof gets {under}");
        assert!(
            beside > under + 0.3 && beside < 1.0,
            "a bud beside the roof gets {beside}"
        );
        assert_eq!(above, 1.0);
        // Nothing is above the roof, and a leaf does not shade itself.
        assert!(organs.iter().all(|light| *light == 1.0));
    }

    #[test]
    fn foliage_shades_buds_in_a_grown_scene() {
        let setup = setup(
            "lsystem p 1; module B queries light; organ leaf foliage area 0.5;
             tool light@1 { cell = 0.5, extinction = 1 };
             axiom [ B ] F(2) [ leaf(1) leaf(1) leaf(1) ] [ f(5) B ];",
        );
        let output = tools(&setup, &Neighbourhood::OPEN);
        let low = output.env[0].1[EnvField::Light as usize];
        let far = output.env[1].1[EnvField::Light as usize];
        assert!(low < 0.95, "bud under foliage gets {low}");
        assert!((far - 1.0).abs() < 1e-12, "bud in the open gets {far}");
    }

    #[test]
    fn a_stand_shades_low_points_more_than_the_top() {
        let stand = Neighbourhood {
            density: 0.15,
            relative_height: 1.0,
            canopy: 0.0,
            spacing: 4.0,
            one_sided: false,
        };
        let up = Vec3::Y;
        let top = stand.transmission(Vec3::new(0.0, 20.0, 0.0), 20.0, up);
        let low = stand.transmission(Vec3::new(0.0, 5.0, 0.0), 20.0, up);
        let wide = stand.transmission(Vec3::new(5.0, 5.0, 0.0), 20.0, up);
        assert!((top - 1.0).abs() < 1e-12);
        assert!(low < 0.2 && wide < low);
        // Slanting light crosses more of the neighbours' canopy.
        let slant = Vec3::new(1.0, 1.0, 0.0).normalize_or(up);
        assert!(stand.transmission(Vec3::new(0.0, 5.0, 0.0), 20.0, slant) < low);
        // At a stand edge the open +X side is brighter, most of all for
        // light arriving over the open ground.
        let edge = Neighbourhood {
            one_sided: true,
            ..stand
        };
        let point = Vec3::new(3.0, 5.0, 0.0);
        assert!(
            edge.transmission(point, 20.0, up)
                > edge.transmission(Vec3::new(-3.0, 5.0, 0.0), 20.0, up)
        );
        let from_open = Vec3::new(1.0, 1.0, 0.0).normalize_or(up);
        let from_stand = Vec3::new(-1.0, 1.0, 0.0).normalize_or(up);
        assert!(
            edge.transmission(point, 20.0, from_open) > edge.transmission(point, 20.0, from_stand)
        );
    }

    #[test]
    fn vigour_splits_by_apical_control() {
        let setup = setup(
            "lsystem p 1; module A queries vigour;
             tool vigour@1 { lambda = 0.75, alpha = 1 };
             axiom F(1) [ +(45) F(1) A ] F(1) A;",
        );
        let output = tools(&setup, &Neighbourhood::OPEN);
        let lateral = output.env[0].1;
        let main = output.env[1].1;
        // Two tips, Q = 1 each; base vigour 2; main gets 0.75 / (0.75 + 0.25).
        assert!((main[EnvField::Vigour as usize] - 1.5).abs() < 1e-12);
        assert!((lateral[EnvField::Vigour as usize] - 0.5).abs() < 1e-12);
        assert_eq!(main[EnvField::Qsum as usize], 1.0);
    }

    #[test]
    fn markers_report_their_whole_branch() {
        let setup = setup(
            "lsystem p 1; module S queries vigour; module A queries vigour;
             tool vigour@1 { };
             axiom F(1) [ S F(1) [ F(1) A ] F(1) A ] F(1) A;",
        );
        let output = tools(&setup, &Neighbourhood::OPEN);
        let marker = output.env[0].1;
        assert_eq!(marker[EnvField::Qsum as usize], 2.0);
        assert_eq!(marker[EnvField::Nseg as usize], 3.0);
        assert_eq!(marker[EnvField::Ntip as usize], 2.0);
        let tip = output.env[1].1;
        assert_eq!(tip[EnvField::Ntip as usize], 0.0);
    }

    #[test]
    fn a_host_s_surface_is_found_within_reach() {
        // A trunk 0.2 m thick from the ground to 5 m, and a limb out to +X.
        let host = Host::new(vec![
            (Vec3::ZERO, Vec3::new(0.0, 5.0, 0.0), 0.2),
            (Vec3::new(0.0, 4.0, 0.0), Vec3::new(3.0, 4.5, 0.0), 0.08),
        ]);
        let (distance, direction) = host.nearest(Vec3::new(-1.0, 2.0, 0.0), 2.0).unwrap();
        assert!((distance - 0.8).abs() < 1e-9, "{distance}");
        assert!((direction - Vec3::X).length() < 1e-9);
        // Under the limb, the limb is nearest.
        let (distance, direction) = host.nearest(Vec3::new(2.0, 3.0, 0.0), 2.0).unwrap();
        assert!(
            distance < 1.5 && direction.y > 0.9,
            "{distance} {direction:?}"
        );
        // Out of reach, nothing.
        assert!(host.nearest(Vec3::new(10.0, 2.0, 0.0), 2.0).is_none());
        // Inside the wood the distance is negative.
        assert!(host.nearest(Vec3::new(0.05, 1.0, 0.0), 2.0).unwrap().0 < 0.0);
    }

    #[test]
    fn space_colonization_pulls_apices_and_consumes_points() {
        let setup = setup(
            "lsystem p 1; module A queries space;
             tool space@1 { shape = 4, base = 2, height = 4, radius = 1, density = 50,
                            influence = 3, kill = 0.3, angle = 180 };
             axiom F(1) A;",
        );
        let areas = organ_areas(&setup.program, &setup.globals).unwrap();
        let clock = Clock {
            step: 0,
            t: 0.0,
            dt: 1.0,
        };
        let mut state = ToolState::default();
        let output = run(
            &setup.program,
            &setup.globals,
            &areas,
            clock,
            &setup.scene,
            &Neighbourhood::OPEN,
            &mut state,
            &Limits::default(),
            5,
        )
        .unwrap();
        let values = output.env[0].1;
        assert!(values[EnvField::Space as usize] > 10.0);
        // The envelope is straight above, so the pull points up.
        assert!(values[EnvField::Sy as usize] > 0.9, "{values:?}");
        assert!(state.killed.is_empty());
    }

    #[test]
    fn pipe_model_thickens_toward_the_base() {
        let setup = setup(
            "lsystem p 1; module A queries position; tool pipe@1 { exponent = 2, tip = 0.01 };
             axiom F(1) [ F(1) A ] [ F(1) A ] F(1) A;",
        );
        let pipes = pipe(&setup.scene, &[], &[2.0, 0.01, 0.0, 0.0]).unwrap();
        // Three tips above the base: r = sqrt(3) * 0.01.
        assert!((pipes.radii[0] - 3.0_f64.sqrt() * 0.01).abs() < 1e-12);
        assert!((pipes.radii[1] - 0.01).abs() < 1e-12);
    }
}
