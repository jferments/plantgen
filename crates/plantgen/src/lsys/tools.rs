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
//! - `space@2` (growth plan G1): the same, with points that return once
//!   the plant parts that took them are shed (`renew`), an outline widest
//!   at any share of the crown's depth and of any fullness (`widest`,
//!   `fullness`), lobes round its edge (`lobes`, `lobe_depth`), and billows,
//!   its edge moved in and out by smooth noise (`bumps`, `bump_size`).
//! - `vigour@1`: the Borchert-Honda resource model: light collected by the
//!   tips flows to the base and back out, split by apical control.
//! - `pipe@1`: the pipe model of stem radii (Shinozaki et al. 1964).
//! - `host@1`: the distance and direction from each module to the wood of
//!   the host a climber, epiphyte or parasite grows on (plant forms F7).
//! - `light@2` (G3): `light@1`'s sky, with the substrate (the conditions'
//!   distance field, or level soil) shading the plant like solid foliage,
//!   and light from below: each direction under the horizon sees the
//!   first substrate it meets, reflecting its material's albedo of the
//!   sky's light, through the plant's foliage on the way.
//! - `space@3` (G3): contact. Living tips are solid spheres of `radius`;
//!   each reads how many others it touches and the way out from among
//!   them.
//! - `substrate@1` (G3): the distance, normal and material of the
//!   substrate at each module.
//!
//! Every tool is deterministic: the same scene gives the same values on every
//! machine.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use super::derive::Clock;
use super::expr::{ENV_FIELDS, EnvField, EnvValues, NO_ENV, Query, Scope, eval};
use super::program::{Program, SymbolKind, ToolConfig, ToolKind};
use super::turtle::{NodeKind, Scene};
use super::{GrowthError, Limits};
use crate::conditions::Surroundings;
use crate::cores::{self, Lease};
use crate::math::{self, Vec3};
use crate::rng::{hash_words, unit};
use crate::substrate::{self, SubstrateField};

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
/// A tool's settings this step. A draw in a setting is keyed on the plant's
/// seed alone, so it is one value for the plant's whole life.
fn settings(
    config: &ToolConfig,
    globals: &[f64],
    clock: Clock,
    seed: u64,
    stack: &mut Vec<f64>,
) -> Vec<f64> {
    let scope = Scope {
        globals,
        locals: &[],
        env: &NO_ENV,
        t: clock.t,
        dt: clock.dt,
        age: 0.0,
        step: f64::from(clock.step),
        key: hash_words(&[seed, SALT_SETTINGS]),
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
    /// The attraction points drawn so far, and which `space@1` consumed.
    points: Lattice,
    /// Lattice spacing fixed at the first step, so cells keep their identity.
    lattice: Option<f64>,
}

/// The attraction points of the lattice cells a space tool has visited,
/// kept from step to step in a box of cells that grows with the envelope,
/// so each point is drawn once: its position, its azimuth round the stem
/// and its billows' noise, which never change, and whether `space@1` has
/// consumed it.
#[derive(Debug, Default)]
struct Lattice {
    /// The box's lowest cell and its extent, x, y and z.
    low: [i64; 3],
    size: [i64; 3],
    cells: Vec<LatticePoint>,
    /// The bump size the cells' noise was drawn at, as bits.
    noise_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct LatticePoint {
    drawn: bool,
    noise_drawn: bool,
    killed: bool,
    point: Vec3,
    theta: f64,
    noise: f64,
}

impl Lattice {
    /// Make the box hold every cell from `low` to `high`, keeping the
    /// points drawn so far. The box grows with a margin, so an envelope
    /// that grows a little each step moves the points only now and then.
    fn cover(&mut self, low: [i64; 3], high: [i64; 3]) {
        let old_high = [0, 1, 2].map(|axis| self.low[axis] + self.size[axis] - 1);
        let empty = self.cells.is_empty();
        if !empty && (0..3).all(|axis| low[axis] >= self.low[axis] && high[axis] <= old_high[axis])
        {
            return;
        }
        let margin = |axis: usize| (high[axis] - low[axis] + 1) / 8 + 2;
        let new_low = [0, 1, 2].map(|axis| {
            let wanted = low[axis] - margin(axis);
            if empty {
                wanted
            } else {
                wanted.min(self.low[axis])
            }
        });
        let new_high = [0, 1, 2].map(|axis| {
            let wanted = high[axis] + margin(axis);
            if empty {
                wanted
            } else {
                wanted.max(old_high[axis])
            }
        });
        let size = [0, 1, 2].map(|axis| new_high[axis] - new_low[axis] + 1);
        let count = size
            .iter()
            .map(|s| usize::try_from(*s).unwrap_or(0))
            .product();
        let mut cells = vec![LatticePoint::default(); count];
        let index = |at: [i64; 3]| -> usize {
            let [x, y, z] = [0, 1, 2].map(|axis| at[axis] - new_low[axis]);
            usize::try_from((y * size[2] + z) * size[0] + x).unwrap_or(0)
        };
        if !empty {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    for x in 0..self.size[0] {
                        let cell = self.cells
                            [self.index([x + self.low[0], y + self.low[1], z + self.low[2]])];
                        cells[index([x + self.low[0], y + self.low[1], z + self.low[2]])] = cell;
                    }
                }
            }
        }
        self.low = new_low;
        self.size = size;
        self.cells = cells;
    }

    /// Where cell `at` is kept; the box must hold it.
    fn index(&self, at: [i64; 3]) -> usize {
        let [x, y, z] = [0, 1, 2].map(|axis| at[axis] - self.low[axis]);
        usize::try_from((y * self.size[2] + z) * self.size[0] + x).unwrap_or(usize::MAX)
    }
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
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
            let values = settings(config, globals, clock, seed, &mut stack);
            if config.version >= 2 {
                light_below(scene, &anchors, organ_area, &values, surroundings, limits)?
            } else {
                light(scene, &anchors, organ_area, &values, surroundings, limits)?
            }
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
        Some(config) if config.version >= 3 => {
            let values = settings(config, globals, clock, seed, &mut stack);
            contact(scene, &anchors, &space_queries, &values)?
        }
        Some(config) => {
            let values = settings(config, globals, clock, seed, &mut stack);
            colonize(
                scene,
                &space_queries,
                &values,
                config.version,
                state,
                limits,
                seed,
            )?
        }
        None => vec![(0, Vec3::ZERO); scene.queries.len()],
    };

    let flux = match program.tool(ToolKind::Vigour) {
        Some(config) => {
            let values = settings(config, globals, clock, seed, &mut stack);
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
        let values = settings(config, globals, clock, seed, &mut stack);
        values[0].max(0.0)
    });

    let ground = program
        .tool(ToolKind::Substrate)
        .map(|_| substrate_of(surroundings));

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
        if let Some(field) = &ground {
            read_substrate(&mut values, field, program, query);
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

    // Receivers in one voxel with the same leaf area of their own see the
    // same foliage along every direction: each such group takes one depth
    // and one exponential per direction.
    let (groups, group_of) = light_groups(&cells, &receivers, &flat, nx * ny * nz);
    // A row per direction: `exp(−τ_j)` for every group. The directions
    // share the threads, each sweeping its own copy of the grid.
    let mut factors = vec![0.0_f64; directions.len() * groups.len()];
    let sweep = |direction: &SkyDirection, row: &mut [f64], beyond: &mut [f64]| {
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
        for (factor, (voxel, own)) in row.iter_mut().zip(&groups) {
            let past = next(*voxel, direction.step).map_or(0.0, |voxel| beyond[flat(voxel)]);
            let shared = (density[flat(*voxel)] - own / volume).max(0.0);
            let depth = length * (past + 0.5 * shared);
            *factor = math::exp(-depth);
        }
    };
    let large = nx * ny * nz + receivers.len() > 20_000;
    in_rows(
        &directions,
        &mut factors,
        groups.len(),
        nx * ny * nz,
        large,
        &sweep,
    );
    let mut light = directions_light(
        &directions,
        &factors,
        &receivers,
        &group_of,
        &|point, direction| neighbours(point, direction),
        surroundings.clear_sky(),
    );
    for value in &mut light {
        *value = (*value / total_weight).clamp(0.0, 1.0);
    }
    let organs = light.split_off(split);
    Ok((light, organs))
}

/// What the plant grows on: the conditions' substrate, or level soil.
fn substrate_of(surroundings: &dyn Surroundings) -> Cow<'_, SubstrateField> {
    surroundings
        .substrate()
        .map_or_else(|| Cow::Owned(SubstrateField::flat()), Cow::Borrowed)
}

/// `substrate@1`'s readings at a module that asks for them: distance,
/// normal and surface material.
fn read_substrate(
    values: &mut EnvValues,
    field: &SubstrateField,
    program: &Program,
    query: &super::turtle::QueryPoint,
) {
    let SymbolKind::Module { queries } = program.symbols[usize::from(query.symbol)].kind else {
        return;
    };
    if queries & Query::Substrate.bit() == 0 {
        return;
    }
    let normal = field.normal(query.position);
    values[EnvField::Sd as usize] = field.distance(query.position);
    values[EnvField::Snx as usize] = normal.x;
    values[EnvField::Sny as usize] = normal.y;
    values[EnvField::Snz as usize] = normal.z;
    values[EnvField::Smat as usize] = f64::from(field.surface_material(query.position));
}

/// `light@2`'s settings: `light@1`'s, and how far past the plant the
/// substrate is looked at.
fn light_below_settings(values: &[f64]) -> Result<(f64, f64, f64, f64), GrowthError> {
    let (cell, extinction, bud) = light_settings(&values[..3])?;
    let reach = values[3];
    if !(reach >= 0.0 && reach.is_finite()) {
        return Err(invalid(
            "light",
            format!("reach must be finite and not negative, found {reach}"),
        ));
    }
    Ok((cell, extinction, bud, reach))
}

/// Light under `light@2` (G3): `light@1`'s overcast sky through the
/// plant's foliage and its neighbours, with the substrate in the voxel
/// grid. The grid reaches `reach` past the plant's receivers on every side
/// (and below them); a voxel whose centre lies inside the substrate is
/// solid. A sky direction that meets a solid voxel brings no light; each
/// of the mirrored directions under the horizon brings the light the
/// first solid voxel it meets reflects, through the foliage on the way:
///
/// ```text
/// light = (Σ_up w_j · T_j · exp(−τ_j) + Σ_down w_j · T_j · R · a_hit · exp(−τ_j)) / Σ_up w_j
/// ```
///
/// where `a_hit` is the albedo of the solid voxel's material and
/// `R = 7/9` is the radiance of a level, matte surface under the overcast
/// sky over the sky's zenith radiance, so a receiver can see more than the
/// open sky alone (at most 2).
#[allow(clippy::too_many_lines)]
fn light_below(
    scene: &Scene,
    anchors: &[bool],
    organ_area: &[f64],
    values: &[f64],
    surroundings: &dyn Surroundings,
    limits: &Limits,
) -> Result<(Vec<f64>, Vec<f64>), GrowthError> {
    let (cell, extinction, bud, reach) = light_below_settings(values)?;
    let receivers = light_receivers(scene, anchors, organ_area, bud);
    let split = scene.queries.len();
    if receivers.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let field = substrate_of(surroundings);
    let directions = sky_directions();
    let total_weight: f64 = directions.iter().map(|direction| direction.weight).sum();
    // The radiance of a matte surface under the overcast sky.
    let reflected = 7.0 / 9.0;

    let index = |value: f64| -> i64 {
        #[allow(clippy::cast_possible_truncation)]
        let cell_index = (value / cell).floor() as i64;
        cell_index
    };
    let mut low = [i64::MAX; 3];
    let mut high = [i64::MIN; 3];
    for (point, _) in &receivers {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            low[axis] = low[axis].min(index(value - reach));
            high[axis] = high[axis].max(index(value + reach));
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
    let flat_index = |[x, y, z]: [usize; 3]| (y * nz + z) * nx + x;
    let step_to = |[x, y, z]: [usize; 3], (dx, dz): (i64, i64), up: bool| -> Option<[usize; 3]> {
        let x = usize::try_from(i64::try_from(x).ok()? + dx).ok()?;
        let z = usize::try_from(i64::try_from(z).ok()? + dz).ok()?;
        let y = if up {
            (y + 1 < ny).then_some(y + 1)?
        } else {
            y.checked_sub(1)?
        };
        (x < nx && z < nz).then_some([x, y, z])
    };

    let volume = cell * cell * cell;
    let mut density = vec![0.0_f64; nx * ny * nz];
    let cells: Vec<[usize; 3]> = receivers
        .iter()
        .map(|(point, _)| voxel_of(*point))
        .collect();
    for (voxel, (_, own)) in cells.iter().zip(&receivers) {
        density[flat_index(*voxel)] += own / volume;
    }
    // The substrate's albedo in each solid voxel; negative in open air.
    let mut solid = vec![-1.0_f64; nx * ny * nz];
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                #[allow(clippy::cast_precision_loss)]
                let at = |axis: usize, i: usize| {
                    (low[axis] + i64::try_from(i).unwrap_or(0)) as f64 + 0.5
                };
                let centre = Vec3::new(at(0, x), at(1, y), at(2, z)) * cell;
                if field.solid(centre) {
                    let material = field.material_at(centre);
                    solid[flat_index([x, y, z])] = substrate::ALBEDO[usize::from(material)];
                }
            }
        }
    }

    let mut light = vec![0.0_f64; receivers.len()];
    let mut beyond = vec![0.0_f64; nx * ny * nz];
    // The albedo of the first solid voxel past each voxel; negative where
    // the direction leaves the grid in open air.
    let mut meets = vec![-1.0_f64; nx * ny * nz];
    for up in [true, false] {
        for direction in &directions {
            let layers: Vec<usize> = if up {
                (0..ny).rev().collect()
            } else {
                (0..ny).collect()
            };
            for y in layers {
                for z in 0..nz {
                    for x in 0..nx {
                        let here = flat_index([x, y, z]);
                        if solid[here] >= 0.0 {
                            beyond[here] = 0.0;
                            meets[here] = solid[here];
                            continue;
                        }
                        let (past, hit) =
                            step_to([x, y, z], direction.step, up).map_or((0.0, -1.0), |voxel| {
                                let v = flat_index(voxel);
                                (beyond[v], meets[v])
                            });
                        beyond[here] = density[here] + past;
                        meets[here] = hit;
                    }
                }
            }
            let length = extinction * cell * direction.cells_per_layer;
            // Reflected light reaches the ground through the neighbours as
            // the sky does along the mirrored direction.
            let mirrored = direction.unit;
            for (receiver, ((point, own), voxel)) in receivers.iter().zip(&cells).enumerate() {
                let (past, hit) = step_to(*voxel, direction.step, up).map_or((0.0, -1.0), |v| {
                    let v = flat_index(v);
                    (beyond[v], meets[v])
                });
                let here = flat_index(*voxel);
                let shared = (density[here] - own / volume).max(0.0);
                let depth = length * (past + 0.5 * shared);
                let open = surroundings.transmission(*point, scene.height, mirrored);
                let gain = if up {
                    // The sky, unless the substrate stands in the way.
                    if hit >= 0.0 { 0.0 } else { 1.0 }
                } else if hit >= 0.0 {
                    reflected * hit
                } else {
                    0.0
                };
                light[receiver] += direction.weight * open * gain * math::exp(-depth);
            }
        }
    }
    for value in &mut light {
        *value = (*value / total_weight).clamp(0.0, 2.0);
    }
    let organs = light.split_off(split);
    Ok((light, organs))
}

/// Contact under `space@3` (G3): living tips (the queries that end a
/// branch) are solid spheres `radius` metres round. Each querying module
/// reads how many living tips other than itself lie within twice the
/// radius (touching it) and the unit direction out from among them: the
/// sum of the unit vectors from each to it, weighted by how far it
/// overlaps, `(1 − d / 2r)`; zero when nothing touches it.
fn contact(
    scene: &Scene,
    anchors: &[bool],
    space_queries: &[bool],
    values: &[f64],
) -> Result<Vec<(u32, Vec3)>, GrowthError> {
    let radius = values[0];
    if !(radius > 0.0 && radius.is_finite()) {
        return Err(invalid(
            "space",
            format!("radius must be above 0, found {radius}"),
        ));
    }
    let reach = 2.0 * radius;
    #[allow(clippy::cast_possible_truncation)]
    let grid = PointGrid::new(
        reach,
        scene
            .queries
            .iter()
            .enumerate()
            .filter(|(index, _)| anchors[*index])
            .map(|(index, query)| (index as u32, query.position)),
    );
    let mut out = vec![(0, Vec3::ZERO); scene.queries.len()];
    for (index, query) in scene.queries.iter().enumerate() {
        if !space_queries[index] {
            continue;
        }
        let mut count = 0_u32;
        let mut away = Vec3::ZERO;
        let mut near: Vec<u32> = grid.near(query.position).collect();
        near.sort_unstable();
        for other in near {
            if other as usize == index {
                continue;
            }
            let offset = query.position - scene.queries[other as usize].position;
            let distance = offset.length();
            if distance >= reach {
                continue;
            }
            count += 1;
            if distance > 1.0e-12 {
                away += offset * ((1.0 - distance / reach) / distance);
            }
        }
        out[index] = (count, away.normalize_or(Vec3::ZERO));
    }
    Ok(out)
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

/// Items placed at points, in a uniform grid over the points' bounding
/// box, each cell's items stored together in the order given: neighbour
/// searches without hashing, reading memory in order. Cells are at least
/// `cell` wide, and wider where the box would pass `GRID_CELLS` cells
/// along an axis.
struct Grid<T> {
    cell: f64,
    low: [i64; 3],
    size: [i64; 3],
    /// Where each cell's items start in `items`, and one past the last.
    starts: Vec<u32>,
    items: Vec<T>,
}

/// The most cells a `Grid` spans along an axis.
const GRID_CELLS: f64 = 256.0;

impl<T: Copy> Grid<T> {
    fn new(cell: f64, points: &[(Vec3, T)]) -> Self {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for (point, _) in points {
            for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
        let extent = (0..3)
            .map(|axis| high[axis] - low[axis])
            .fold(0.0, f64::max);
        let cell = if extent.is_finite() {
            cell.max(extent / (GRID_CELLS - 2.0))
        } else {
            cell
        };
        let mut grid = Self {
            cell,
            low: [0; 3],
            size: [0; 3],
            starts: vec![0],
            items: Vec::new(),
        };
        if points.is_empty() {
            return grid;
        }
        let keys: Vec<[i64; 3]> = points.iter().map(|(point, _)| grid.key(*point)).collect();
        for axis in 0..3 {
            let (first, last) = keys.iter().fold((i64::MAX, i64::MIN), |(a, b), key| {
                (a.min(key[axis]), b.max(key[axis]))
            });
            grid.low[axis] = first;
            grid.size[axis] = last - first + 1;
        }
        let cells = grid
            .size
            .iter()
            .map(|s| usize::try_from(*s).unwrap_or(0))
            .product::<usize>();
        let mut starts = vec![0_u32; cells + 1];
        let flat: Vec<usize> = keys
            .iter()
            .map(|key| grid.flat(*key).unwrap_or(0))
            .collect();
        for at in &flat {
            starts[at + 1] += 1;
        }
        for at in 1..starts.len() {
            starts[at] += starts[at - 1];
        }
        let mut next = starts.clone();
        let mut slots: Vec<Option<T>> = vec![None; points.len()];
        for ((_, item), at) in points.iter().zip(&flat) {
            slots[next[*at] as usize] = Some(*item);
            next[*at] += 1;
        }
        grid.items = slots.into_iter().flatten().collect();
        grid.starts = starts;
        grid
    }

    #[allow(clippy::cast_possible_truncation)]
    fn key(&self, point: Vec3) -> [i64; 3] {
        [
            (point.x / self.cell).floor() as i64,
            (point.y / self.cell).floor() as i64,
            (point.z / self.cell).floor() as i64,
        ]
    }

    /// Where cell `key` is kept, if the box holds it.
    fn flat(&self, key: [i64; 3]) -> Option<usize> {
        let [x, y, z] = [0, 1, 2].map(|axis| key[axis] - self.low[axis]);
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return None;
        }
        usize::try_from((y * self.size[2] + z) * self.size[0] + x).ok()
    }

    /// The squared distance from `point` to cell `key`'s box, a nanometre
    /// short on each axis: no item of the cell lies nearer.
    fn gap_sq(&self, key: [i64; 3], point: Vec3) -> f64 {
        let gap = |cell: i64, value: f64| {
            #[allow(clippy::cast_precision_loss)]
            let low = cell as f64 * self.cell;
            let high = low + self.cell;
            ((low - value).max(value - high) - 1e-9).max(0.0)
        };
        let (gx, gy, gz) = (
            gap(key[0], point.x),
            gap(key[1], point.y),
            gap(key[2], point.z),
        );
        gx * gx + gy * gy + gz * gz
    }

    /// The items in cell `key`, in the order given.
    fn cell_items(&self, key: [i64; 3]) -> &[T] {
        match self.flat(key) {
            Some(at) => &self.items[self.starts[at] as usize..self.starts[at + 1] as usize],
            None => &[],
        }
    }

    /// The cells to search round a point's own for points within `reach`,
    /// nearest first: each as its offset and the squared distance below
    /// which no point of it lies from any point of the centre cell.
    fn search(&self, reach: f64) -> Vec<([i64; 3], f64)> {
        #[allow(clippy::cast_possible_truncation)]
        let rings = (reach / self.cell).ceil() as i64 + 1;
        let cell_sq = self.cell * self.cell;
        let mut offsets = Vec::new();
        for dx in -rings..=rings {
            for dy in -rings..=rings {
                for dz in -rings..=rings {
                    #[allow(clippy::cast_precision_loss)]
                    let gap = |d: i64| (d.abs() - 1).max(0) as f64;
                    let near = (gap(dx) * gap(dx) + gap(dy) * gap(dy) + gap(dz) * gap(dz))
                        * cell_sq
                        * (1.0 - 1e-9);
                    if near <= reach * reach {
                        offsets.push(([dx, dy, dz], near));
                    }
                }
            }
        }
        offsets.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        offsets
    }
}

impl Grid<Vec3> {
    /// The cells round `point`'s own, its own first: with cells at least
    /// a search's reach wide, every item within reach lies in one of them.
    const NEAR: [[i64; 3]; 27] = {
        let mut offsets = [[0; 3]; 27];
        let mut at = 1;
        let mut d = 0;
        while d < 27 {
            let offset = [d % 3 - 1, d / 3 % 3 - 1, d / 9 - 1];
            if offset[0] != 0 || offset[1] != 0 || offset[2] != 0 {
                offsets[at] = offset;
                at += 1;
            }
            d += 1;
        }
        offsets
    };

    /// Whether a point of the grid lies within `reach_sq` (squared) of
    /// `point`; the cells are at least the reach wide. Cells whose box lies
    /// out of reach are passed over.
    fn within(&self, point: Vec3, reach_sq: f64) -> bool {
        let [x, y, z] = self.key(point);
        Self::NEAR.iter().any(|[dx, dy, dz]| {
            let key = [x + dx, y + dy, z + dz];
            self.gap_sq(key, point) <= reach_sq
                && self
                    .cell_items(key)
                    .iter()
                    .any(|part| (*part - point).length_squared() <= reach_sq)
        })
    }
}

/// The cells of a grid (and round it) that some item may lie within a
/// search's reach of: every cell within the reach of an occupied cell, by
/// the same bound the search uses. A point in an unmarked cell has no item
/// within reach.
struct Reach {
    low: [i64; 3],
    size: [i64; 3],
    marked: Vec<bool>,
}

impl Reach {
    fn new<T: Copy>(grid: &Grid<T>, search: &[([i64; 3], f64)]) -> Self {
        let rings = search
            .iter()
            .flat_map(|(offset, _)| offset.iter().map(|d| d.abs()))
            .max()
            .unwrap_or(0);
        let low = grid.low.map(|value| value - rings);
        let size = grid.size.map(|value| value + 2 * rings);
        let count = size
            .iter()
            .map(|s| usize::try_from(*s).unwrap_or(0))
            .product::<usize>();
        let mut reach = Self {
            low,
            size,
            marked: vec![false; count],
        };
        for y in 0..grid.size[1] {
            for z in 0..grid.size[2] {
                for x in 0..grid.size[0] {
                    let key = [x + grid.low[0], y + grid.low[1], z + grid.low[2]];
                    if grid.cell_items(key).is_empty() {
                        continue;
                    }
                    for ([dx, dy, dz], _) in search {
                        if let Some(at) = reach.flat([key[0] + dx, key[1] + dy, key[2] + dz]) {
                            reach.marked[at] = true;
                        }
                    }
                }
            }
        }
        reach
    }

    fn flat(&self, key: [i64; 3]) -> Option<usize> {
        let [x, y, z] = [0, 1, 2].map(|axis| key[axis] - self.low[axis]);
        if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return None;
        }
        usize::try_from((y * self.size[2] + z) * self.size[0] + x).ok()
    }

    /// Whether cell `key` may have an item within reach.
    fn marked(&self, key: [i64; 3]) -> bool {
        self.flat(key).is_some_and(|at| self.marked[at])
    }

    /// For a row of boxes along x from `low` (y and z fixed, x from
    /// `low.x` to `high.x`): the first x key of `grid` the row overlaps,
    /// and for each x key on from it whether a cell of `grid` at that key
    /// that the row's boxes overlap is marked.
    fn row<T: Copy>(&self, grid: &Grid<T>, low: Vec3, high: Vec3) -> (i64, Vec<bool>) {
        let (a, b) = (grid.key(low), grid.key(high));
        let marks = (a[0]..=b[0])
            .map(|x| (a[1]..=b[1]).any(|y| (a[2]..=b[2]).any(|z| self.marked([x, y, z]))))
            .collect();
        (a[0], marks)
    }
}

/// Whether a box along a row reaches a marked cell: `row` from
/// `Reach::row`, `low` and `high` its x extent, `cell` the grid's.
fn row_covers((first, marks): &(i64, Vec<bool>), cell: f64, low: f64, high: f64) -> bool {
    #[allow(clippy::cast_possible_truncation)]
    let key = |value: f64| (value / cell).floor() as i64;
    (key(low)..=key(high)).any(|x| {
        usize::try_from(x - first)
            .ok()
            .and_then(|at| marks.get(at))
            .copied()
            .unwrap_or(false)
    })
}

/// An apex as the space tool's search reads it: its query's index,
/// position and heading, kept together.
#[derive(Debug, Clone, Copy)]
struct Apex {
    index: u32,
    position: Vec3,
    heading: Vec3,
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
const SALT_LOBES: u64 = 0x6c6f_6265;
const SALT_SETTINGS: u64 = 0x7365_7474;
const SALT_BUMPS: u64 = 0x6275_6d70;

/// The outline of a `space@2` envelope and whether its points renew.
struct Outline {
    renew: bool,
    widest: f64,
    fullness: f64,
    lobes: f64,
    lobe_depth: f64,
    phases: [f64; 2],
    bumps: f64,
    bump_size: f64,
    seed: u64,
}

impl Outline {
    /// Radius of the envelope at relative height `h` toward `point`.
    ///
    /// Shape 1 is a superellipse in profile, widest at `widest` of the
    /// crown's depth: with $`u`$ the distance from there as a share of the
    /// part of the crown on that side and $`p`$ the `fullness`,
    ///
    /// ```math
    /// r(h) = R \left(1 - |u|^p\right)^{1/p}
    /// ```
    ///
    /// $`p = 2`$ at `widest` 0.5 is `space@1`'s ellipsoid; a larger $`p`$
    /// fills toward a cylinder with rounded ends, a smaller one draws in to
    /// points. Shapes 2 to 4 are `space@1`'s cone, paraboloid and cylinder.
    /// Lobes then scale the radius round the stem by
    /// $`1 + d\,n(\theta, h)`$, with $`n`$ two waves of `lobes` and
    /// `lobes + 1` crests round the azimuth $`\theta`$, drifting with height
    /// and phased by the seed, so each plant's crown bulges its own way.
    ///
    /// `theta` is the point's azimuth, `math::atan2(point.z, point.x)`, and
    /// `noise` its billows' noise, `value_noise(seed, point / bump_size)`;
    /// either is read only when the outline has lobes or billows.
    fn radius(&self, shape: f64, radius: f64, h: f64, theta: f64, noise: f64) -> f64 {
        if !(0.0..=1.0).contains(&h) {
            return -1.0;
        }
        let profile = self.profile(shape, radius, h);
        let billows = if self.bumps > 0.0 {
            1.0 + self.bumps * noise
        } else {
            1.0
        };
        if self.lobes <= 0.0 || self.lobe_depth <= 0.0 {
            return profile * billows;
        }
        let wave = 0.6 * math::cos(self.lobes * theta + self.phases[0] + 3.0 * h)
            + 0.4 * math::cos((self.lobes + 1.0) * theta + self.phases[1] - 5.0 * h);
        profile * (1.0 + self.lobe_depth * wave) * billows
    }

    /// The envelope's radius at relative height `h` in `[0, 1]` before
    /// lobes and billows.
    fn profile(&self, shape: f64, radius: f64, h: f64) -> f64 {
        #[allow(clippy::cast_possible_truncation)]
        if (2..=4).contains(&(shape.round() as i64)) {
            envelope_radius(shape, radius, h)
        } else {
            let side = if h < self.widest {
                self.widest
            } else {
                1.0 - self.widest
            };
            let u = ((h - self.widest) / side).abs().min(1.0);
            radius
                * math::pow(
                    (1.0 - math::pow(u, self.fullness)).max(0.0),
                    1.0 / self.fullness,
                )
        }
    }

    /// Bounds on the envelope's radius, before billows, for relative
    /// heights from `low` to `high` (within `[0, 1]`): a little under its
    /// least and a little over its most. Every profile rises to one height
    /// (`widest`, the base of a cone or paraboloid, the middle of an
    /// ellipsoid) and falls away from it, and lobes scale it by `1 ± d`.
    fn bounds(&self, shape: f64, radius: f64, low: f64, high: f64) -> (f64, f64) {
        #[allow(clippy::cast_possible_truncation)]
        let peak = match shape.round() as i64 {
            2..=4 => 0.0,
            1 => self.widest,
            _ => 0.5,
        };
        let least = self
            .profile(shape, radius, low)
            .min(self.profile(shape, radius, high));
        let most = self.profile(shape, radius, peak.clamp(low, high));
        let lobes = if self.lobes <= 0.0 || self.lobe_depth <= 0.0 {
            0.0
        } else {
            self.lobe_depth
        };
        (
            least * (1.0 - lobes) * (1.0 - 1e-9),
            most * (1.0 + lobes) * (1.0 + 1e-9),
        )
    }
}

/// Smooth value noise in [-1, 1] at `p` (lattice units): a value hashed
/// from `seed` and each corner of the unit cell round `p`, blended by
/// smoothstep weights, so it is continuous and the same on every machine.
fn value_noise(seed: u64, p: Vec3) -> f64 {
    let cell = [p.x.floor(), p.y.floor(), p.z.floor()];
    let fraction = [p.x - cell[0], p.y - cell[1], p.z - cell[2]];
    let smooth = fraction.map(|f| f * f * (3.0 - 2.0 * f));
    let mut total = 0.0;
    for corner in 0..8_u32 {
        let offset = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
        let mut weight = 1.0;
        let mut words = [seed, SALT_BUMPS, 0, 0, 0];
        for axis in 0..3 {
            let up = offset[axis] == 1;
            weight *= if up { smooth[axis] } else { 1.0 - smooth[axis] };
            // Lattice coordinates are small whole numbers; the cast keeps
            // their bits.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let coordinate = (cell[axis] as i64 + i64::from(up)) as u64;
            words[2 + axis] = coordinate;
        }
        total += weight * (2.0 * unit(hash_words(&words)) - 1.0);
    }
    total
}

/// The two lobe waves' phases for a plant grown from `seed`.
fn lobe_phases(seed: u64) -> [f64; 2] {
    let hash = hash_words(&[seed, SALT_LOBES]);
    [
        std::f64::consts::TAU * unit(hash),
        std::f64::consts::TAU * unit(crate::rng::mix64(hash)),
    ]
}

/// Space colonization (Runions, Lane and Prusinkiewicz 2007). Attraction
/// points sit one per cell of a jittered lattice inside the envelope. A point
/// within `kill` of the plant is consumed for good; otherwise it attracts the
/// nearest apex that declared `queries space` within `influence` and inside
/// its perception cone. Each apex gets the count and the mean direction.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn colonize(
    scene: &Scene,
    space_queries: &[bool],
    values: &[f64],
    version: u32,
    state: &mut ToolState,
    limits: &Limits,
    seed: u64,
) -> Result<Vec<(u32, Vec3)>, GrowthError> {
    let (first, later) = values.split_at(values.len().min(8));
    let [shape, base, height, radius, density, influence, kill, angle] = first[..] else {
        return Err(invalid("space", "wrong number of settings".into()));
    };
    let outline = match (version, later) {
        (1, []) => None,
        (2, &[renew, widest, fullness, lobes, lobe_depth, bumps, bump_size]) => {
            if !(widest > 0.0
                && widest < 1.0
                && fullness > 0.0
                && (0.0..1.0).contains(&lobe_depth)
                && (0.0..1.0).contains(&bumps)
                && bump_size > 0.0)
            {
                return Err(invalid(
                    "space",
                    format!(
                        "widest must lie between 0 and 1, fullness and bump_size be positive, \
                         and lobe_depth and bumps lie in [0, 1); found {widest}, {fullness}, \
                         {bump_size}, {lobe_depth} and {bumps}"
                    ),
                ));
            }
            Some(Outline {
                renew: renew > 0.0,
                widest,
                fullness,
                lobes: lobes.max(0.0).round(),
                lobe_depth,
                phases: lobe_phases(seed),
                bumps,
                bump_size,
                seed,
            })
        }
        _ => return Err(invalid("space", "wrong number of settings".into())),
    };
    let renew = outline.as_ref().is_some_and(|outline| outline.renew);
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

    let plant_points: Vec<(Vec3, Vec3)> = scene
        .segments
        .iter()
        .flat_map(|segment| [segment.start, segment.end])
        .chain(scene.queries.iter().map(|query| query.position))
        .map(|point| (point, point))
        .collect();
    // The plant's points in cells at least `kill` wide, so the 27 round a
    // point hold every part within `kill`; the apices in cells half the
    // influence wide, searched nearest first.
    let plant = Grid::new(kill.max(0.05), &plant_points);
    let apex_points: Vec<(Vec3, Apex)> = scene
        .queries
        .iter()
        .enumerate()
        .filter(|(index, _)| space_queries[*index])
        .map(|(index, query)| {
            let apex = Apex {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                position: query.position,
                heading: query.heading,
            };
            (query.position, apex)
        })
        .collect();
    let apices = Grid::new(0.5 * influence, &apex_points);
    let search = apices.search(influence);
    let reach = Reach::new(&apices, &search);
    let plant_reach = Reach::new(&plant, &plant.search(kill));
    // Passing over points out of reach pays where the plant fills little
    // of its envelope (a sapling under a canopy); in a full crown nearly
    // every point is in reach, and the test only costs.
    #[allow(clippy::cast_precision_loss)]
    let sparse = {
        let marked = reach.marked.iter().filter(|marked| **marked).count() as f64
            * apices.cell
            * apices.cell
            * apices.cell;
        let lattice_volume =
            ((x1 - x0 + 1) * (x1 - x0 + 1) * (y1 - y0 + 1)) as f64 * spacing * spacing * spacing;
        marked < 0.5 * lattice_volume
    };

    let cone = math::cos(math::radians(angle.clamp(0.0, 180.0)));
    let mut sums = vec![Vec3::ZERO; scene.queries.len()];
    let kill_sq = kill * kill;
    let influence_sq = influence * influence;

    let lattice = &mut state.points;
    lattice.cover([x0, y0, x0], [x1, y1, x1]);
    let noise_size = outline
        .as_ref()
        .filter(|outline| outline.bumps > 0.0)
        .map(|outline| outline.bump_size.to_bits());
    if noise_size.is_some() && lattice.noise_size != noise_size {
        for cell in &mut lattice.cells {
            cell.noise_drawn = false;
        }
        lattice.noise_size = noise_size;
    }
    let (low, size) = (lattice.low, lattice.size);
    // One layer of the lattice box: its points' pulls on the apices that
    // take them, in the order its points lie (z, then x).
    let layer_pulls = |y: i64, cells: &mut [LatticePoint]| -> Vec<(u32, Vec3)> {
        let mut pulls = Vec::new();
        let mut hint = None;
        // The envelope's bounds over the heights this layer's points take,
        // so most points fall inside or outside without its exact radius.
        #[allow(clippy::cast_precision_loss)]
        let heights = (
            (y as f64 * spacing - base) / (height - base),
            ((y + 1) as f64 * spacing - base) / (height - base),
        );
        let bounds = outline.as_ref().map(|outline| {
            if heights.1 < 0.0 || heights.0 > 1.0 {
                (-1.0, -1.0)
            } else {
                outline.bounds(shape, radius, heights.0.max(0.0), heights.1.min(1.0))
            }
        });
        for z in x0..=x1 {
            // Which of the row's points may be in reach of the plant or of
            // an apex; a point out of reach of both is neither consumed nor
            // taken this step.
            #[allow(clippy::cast_precision_loss)]
            let (row_low, row_high) = (
                Vec3::new(x0 as f64 * spacing, y as f64 * spacing, z as f64 * spacing),
                Vec3::new(
                    (x1 + 1) as f64 * spacing,
                    (y + 1) as f64 * spacing,
                    (z + 1) as f64 * spacing,
                ),
            );
            let rows = sparse.then(|| {
                (
                    reach.row(&apices, row_low, row_high),
                    plant_reach.row(&plant, row_low, row_high),
                )
            });
            for x in x0..=x1 {
                #[allow(clippy::cast_precision_loss)]
                let (left, right) = (x as f64 * spacing, (x + 1) as f64 * spacing);
                if let Some((apex_row, plant_row)) = &rows
                    && !row_covers(apex_row, apices.cell, left, right)
                    && !row_covers(plant_row, plant.cell, left, right)
                {
                    continue;
                }
                let at = usize::try_from((z - low[2]) * size[0] + (x - low[0])).unwrap_or(0);
                let cell = &mut cells[at];
                if !renew && cell.killed {
                    continue;
                }
                if !cell.drawn {
                    cell.point = attraction_point(seed, [x, y, z], spacing);
                    cell.theta = math::atan2(cell.point.z, cell.point.x);
                    cell.drawn = true;
                }
                let point = cell.point;
                let relative = (point.y - base) / (height - base);
                let radial = math::sqrt(point.x * point.x + point.z * point.z);
                match (&outline, bounds) {
                    (Some(outline), Some((least, most))) => {
                        if !(0.0..=1.0).contains(&relative) {
                            continue;
                        }
                        if outline.bumps > 0.0 && !cell.noise_drawn {
                            cell.noise =
                                value_noise(outline.seed, point * (1.0 / outline.bump_size));
                            cell.noise_drawn = true;
                        }
                        let billows = if outline.bumps > 0.0 {
                            1.0 + outline.bumps * cell.noise
                        } else {
                            1.0
                        };
                        if radial > most * billows {
                            continue;
                        }
                        if radial > least * billows
                            && radial
                                > outline.radius(shape, radius, relative, cell.theta, cell.noise)
                        {
                            continue;
                        }
                    }
                    _ => {
                        if radial > envelope_radius(shape, radius, relative) {
                            continue;
                        }
                    }
                }
                if plant.within(point, kill_sq) {
                    // `space@1` keeps a point consumed for good; `space@2`
                    // with `renew` frees it once the parts near it are shed.
                    if !renew {
                        cell.killed = true;
                    }
                    continue;
                }
                // No apex within reach of the point's cell: none takes it.
                let claimed = if reach.marked(apices.key(point)) {
                    claimant(&apices, &search, point, influence_sq, cone, hint)
                } else {
                    None
                };
                if let Some((distance_sq, apex)) = claimed {
                    hint = Some(apex);
                    pulls.push((
                        apex.index,
                        (point - apex.position) / math::sqrt(distance_sq),
                    ));
                }
            }
        }
        pulls
    };
    let layer_cells = usize::try_from(size[2] * size[0]).unwrap_or(1).max(1);
    let layers: Vec<(i64, &mut [LatticePoint])> = (low[1]..)
        .zip(lattice.cells.chunks_mut(layer_cells))
        .filter(|(y, _)| (y0..=y1).contains(y))
        .collect();
    // Many points share the work out; the pulls add up in the order the
    // points lie, layer by layer, however many threads drew them.
    let lease = Lease::take(
        if span((x0, x1)) * span((x0, x1)) * span((y0, y1)) > 100_000 {
            cores::per_task()
        } else {
            1
        },
    );
    let pulled = in_layers(layers, lease.threads(), &layer_pulls);
    drop(lease);
    for pulls in pulled {
        for (index, pull) in pulls {
            sums[index as usize] += pull;
            result[index as usize].0 += 1;
        }
    }
    for (entry, sum) in result.iter_mut().zip(sums) {
        entry.1 = sum.normalize_or(Vec3::ZERO);
    }
    Ok(result)
}

/// Each receiver's light, adding its directions' terms in their order:
/// weight times the neighbours' transmission times the group's factor (a
/// row of `factors` per direction). Where nothing but the plant shades the
/// sky (`clear`), a group's receivers add the same terms, so each group's
/// sum is taken once.
fn directions_light(
    directions: &[SkyDirection],
    factors: &[f64],
    receivers: &[(Vec3, f64)],
    group_of: &[u32],
    neighbours: &(dyn Fn(Vec3, &SkyDirection) -> f64 + Sync),
    clear: bool,
) -> Vec<f64> {
    let mut light = vec![0.0_f64; receivers.len()];
    let width = factors.len() / directions.len().max(1);
    if width == 0 {
        return light;
    }
    if clear {
        let mut sums = vec![0.0_f64; width];
        for (direction, row) in directions.iter().zip(factors.chunks(width)) {
            for (sum, factor) in sums.iter_mut().zip(row) {
                *sum += direction.weight * factor;
            }
        }
        for (value, group) in light.iter_mut().zip(group_of) {
            *value = sums[*group as usize];
        }
        return light;
    }
    let shine = |light: &mut [f64], receivers: &[(Vec3, f64)], group_of: &[u32]| {
        for ((value, (point, _)), group) in light.iter_mut().zip(receivers).zip(group_of) {
            for (j, direction) in directions.iter().enumerate() {
                let factor = factors[j * width + *group as usize];
                *value += direction.weight * neighbours(*point, direction) * factor;
            }
        }
    };
    let lease = Lease::take(if receivers.len() > 20_000 {
        cores::per_task()
    } else {
        1
    });
    in_chunks(&mut light, receivers, group_of, lease.threads(), &shine);
    light
}

/// Groups of light receivers that share a voxel and their own leaf area
/// (bit for bit): each group's voxel and area, and every receiver's
/// group. `flat` indexes the `voxels` voxels.
fn light_groups(
    cells: &[[usize; 3]],
    receivers: &[(Vec3, f64)],
    flat: &dyn Fn([usize; 3]) -> usize,
    voxels: usize,
) -> (Vec<([usize; 3], f64)>, Vec<u32>) {
    // The receivers by voxel: a counting sort.
    let mut starts = vec![0_u32; voxels + 1];
    for voxel in cells {
        starts[flat(*voxel) + 1] += 1;
    }
    for index in 1..starts.len() {
        starts[index] += starts[index - 1];
    }
    let mut fill = starts.clone();
    let mut order = vec![0_u32; cells.len()];
    for (receiver, voxel) in cells.iter().enumerate() {
        let slot = &mut fill[flat(*voxel)];
        order[*slot as usize] = u32::try_from(receiver).unwrap_or(u32::MAX);
        *slot += 1;
    }
    let mut groups = Vec::new();
    let mut group_of = vec![0_u32; cells.len()];
    let mut members: Vec<(u64, u32)> = Vec::new();
    for voxel in 0..voxels {
        let (from, to) = (starts[voxel] as usize, starts[voxel + 1] as usize);
        if from == to {
            continue;
        }
        members.clear();
        members.extend(
            order[from..to]
                .iter()
                .map(|receiver| (receivers[*receiver as usize].1.to_bits(), *receiver)),
        );
        members.sort_unstable();
        let mut last = None;
        for (area, receiver) in &members {
            if last != Some(*area) {
                groups.push((cells[*receiver as usize], f64::from_bits(*area)));
                last = Some(*area);
            }
            group_of[*receiver as usize] = u32::try_from(groups.len() - 1).unwrap_or(u32::MAX);
        }
    }
    (groups, group_of)
}

/// The light tool's work for one sky direction: its row of factors, and a
/// grid of scratch.
type Sweep<'a> = dyn Fn(&SkyDirection, &mut [f64], &mut [f64]) + Sync + 'a;

/// Run `work` for every direction on its row of `rows` (`width` values a
/// row), the directions spread over the idle cores when the work is
/// `large`, each with a scratch grid of `scratch` values. Each row is
/// worked on its own, so the result does not depend on the threads.
fn in_rows(
    directions: &[SkyDirection],
    rows: &mut [f64],
    width: usize,
    scratch: usize,
    large: bool,
    work: &Sweep<'_>,
) {
    if width == 0 {
        return;
    }
    let lease = Lease::take(if large { cores::per_task() } else { 1 });
    let threads = lease.threads();
    let tasks = directions.iter().zip(rows.chunks_mut(width));
    if threads <= 1 {
        let mut grid = vec![0.0_f64; scratch];
        for (direction, row) in tasks {
            work(direction, row, &mut grid);
        }
        return;
    }
    let queue = Mutex::new(tasks);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(directions.len()) {
            scope.spawn(|| {
                let mut grid = vec![0.0_f64; scratch];
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some((direction, row)) = next else {
                        break;
                    };
                    work(direction, row, &mut grid);
                }
            });
        }
    });
}

/// The light tool's work on a run of receivers: their light, the
/// receivers (position and own leaf area) and their groups.
type Shine<'a> = dyn Fn(&mut [f64], &[(Vec3, f64)], &[u32]) + Sync + 'a;

/// Run `work` over `light` with its `receivers` and their `groups`, in
/// chunks spread over up to `threads` threads. Each value is worked on its
/// own, so the result does not depend on the chunks.
fn in_chunks(
    light: &mut [f64],
    receivers: &[(Vec3, f64)],
    groups: &[u32],
    threads: usize,
    work: &Shine<'_>,
) {
    if threads <= 1 {
        work(light, receivers, groups);
        return;
    }
    let chunk = receivers.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for ((light, receivers), groups) in light
            .chunks_mut(chunk)
            .zip(receivers.chunks(chunk))
            .zip(groups.chunks(chunk))
        {
            scope.spawn(move || work(light, receivers, groups));
        }
    });
}

/// Run `work` on every layer, spread over up to `threads` threads, and
/// return each layer's result in the layers' order, so the result never
/// depends on scheduling.
fn in_layers<R: Send>(
    layers: Vec<(i64, &mut [LatticePoint])>,
    threads: usize,
    work: &(dyn Fn(i64, &mut [LatticePoint]) -> R + Sync),
) -> Vec<R> {
    let count = layers.len();
    if threads <= 1 || count < 2 {
        return layers
            .into_iter()
            .map(|(y, cells)| work(y, cells))
            .collect();
    }
    let queue = Mutex::new(layers.into_iter().enumerate());
    let done: Mutex<Vec<(usize, R)>> = Mutex::new(Vec::with_capacity(count));
    std::thread::scope(|scope| {
        for _ in 0..threads.min(count) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some((at, (y, cells))) = next else {
                        break;
                    };
                    let result = work(y, cells);
                    done.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((at, result));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(PoisonError::into_inner);
    done.sort_by_key(|(at, _)| *at);
    done.into_iter().map(|(_, result)| result).collect()
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
/// point most directly, then to the earlier bud. The cells of `apices` are
/// searched in the order of `search` (`Grid::search`), stopping where no
/// farther cell can hold an apex as near as the best so far; `hint`, the
/// apex that took the last point, is tried first, so that the best so far
/// starts near.
fn claimant(
    apices: &Grid<Apex>,
    search: &[([i64; 3], f64)],
    point: Vec3,
    influence_sq: f64,
    cone: f64,
    hint: Option<Apex>,
) -> Option<(f64, Apex)> {
    // The best so far: squared distance, facing and the apex.
    let mut best: Option<(f64, f64, Apex)> = None;
    let consider = |apex: &Apex, best: &mut Option<(f64, f64, Apex)>| {
        let offset = point - apex.position;
        let distance_sq = offset.length_squared();
        if distance_sq > influence_sq || distance_sq <= 0.0 {
            return;
        }
        // Farther than the best so far: not better, whichever way it faces.
        if best.is_some_and(|(distance, _, _)| distance_sq > distance) {
            return;
        }
        let facing = apex.heading.dot(offset) / math::sqrt(distance_sq);
        if facing < cone {
            return;
        }
        let better = best.is_none_or(|(distance, most_facing, earliest)| {
            distance_sq
                .total_cmp(&distance)
                .then(most_facing.total_cmp(&facing))
                .then(apex.index.cmp(&earliest.index))
                .is_lt()
        });
        if better {
            *best = Some((distance_sq, facing, *apex));
        }
    };
    if let Some(apex) = hint {
        consider(&apex, &mut best);
    }
    let [x, y, z] = apices.key(point);
    for ([dx, dy, dz], near) in search {
        let key = [x + dx, y + dy, z + dz];
        if let Some((distance, _, _)) = best {
            if *near > distance {
                break;
            }
            // A cell whose box lies farther than the best so far holds no
            // apex as near (a nanometre spared for rounding).
            if apices.gap_sq(key, point) > distance {
                continue;
            }
        }
        for apex in apices.cell_items(key) {
            consider(apex, &mut best);
        }
    }
    best.map(|(distance_sq, _, apex)| (distance_sq, apex))
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
        // No point is consumed: the plant stands below the envelope.
        assert!(state.points.cells.iter().all(|cell| !cell.killed));
    }

    #[test]
    fn a_draw_in_a_tool_setting_is_one_value_for_the_plants_life() {
        let program = Program::compile(
            "lsystem p 1; module A; tool pipe@1 { tip = uniform(0.002, 0.02) }; axiom A;",
        )
        .unwrap();
        let config = program.tool(ToolKind::Pipe).unwrap();
        let mut stack = Vec::new();
        let at = |step: u32, seed: u64, stack: &mut Vec<f64>| {
            let clock = Clock {
                step,
                t: f64::from(step),
                dt: 1.0,
            };
            settings(config, &[], clock, seed, stack)[1]
        };
        let first = at(0, 7, &mut stack);
        assert!((0.002..0.02).contains(&first));
        assert_eq!(first.to_bits(), at(40, 7, &mut stack).to_bits());
        assert_ne!(first.to_bits(), at(0, 8, &mut stack).to_bits());
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

    fn around(substrate: Option<&SubstrateField>) -> crate::conditions::Around<'_> {
        crate::conditions::Around {
            neighbourhood: &Neighbourhood::OPEN,
            host: None,
            substrate,
        }
    }

    #[test]
    fn light_2_adds_light_from_the_ground_and_the_rock_shades() {
        // A bud 0.2 m over open soil sees the whole sky and the soil's
        // reflection; one at the foot of a cleft's wall loses sky to it.
        let scene = Scene {
            queries: vec![
                bud(Vec3::new(0.0, 0.2, 0.0)),
                bud(Vec3::new(-0.2, 0.05, 0.0)),
            ],
            ..Scene::default()
        };
        let organ_area = vec![0.0; 18];
        let settings = [0.1, 0.5, 0.0, 0.5];
        let flat = light_below(
            &scene,
            &[true, true],
            &organ_area,
            &settings,
            &around(None),
            &Limits::default(),
        )
        .unwrap()
        .0;
        // At most the sky and all the soil's reflection; the lowest
        // directions leave the grid (reach) before they meet the soil.
        let reflected = 1.0 + crate::substrate::ALBEDO[1] * 7.0 / 9.0;
        assert!(
            flat[0] > 1.05 && flat[0] <= reflected + 1.0e-12,
            "open soil gives {}",
            flat[0]
        );
        let cleft = crate::substrate::Substrate::preset(crate::substrate::SubstratePreset::Cleft)
            .field()
            .unwrap();
        let walled = light_below(
            &scene,
            &[true, true],
            &organ_area,
            &settings,
            &around(Some(&cleft)),
            &Limits::default(),
        )
        .unwrap()
        .0;
        assert!(
            walled[1] < flat[1] - 0.2,
            "the wall's foot gets {}",
            walled[1]
        );
        // light@1 knows nothing of the substrate and never exceeds the sky.
        let old = light(
            &scene,
            &[true, true],
            &organ_area,
            &settings[..3],
            &around(Some(&cleft)),
            &Limits::default(),
        )
        .unwrap()
        .0;
        assert_eq!(old, vec![1.0, 1.0]);
    }

    #[test]
    fn space_3_counts_touching_tips_and_points_out_from_them() {
        let setup = setup(
            "lsystem p 1; module B queries space;
             tool space@3 { radius = 0.01 };
             axiom [ B ] [ f(0.015) B ] [ &(90) f(0.015) B ] [ f(1) B ];",
        );
        let output = tools(&setup, &Neighbourhood::OPEN);
        let read = |i: usize, field: EnvField| output.env[i].1[field as usize];
        // The first tip touches the two beside it; the far one touches none.
        assert_eq!(read(0, EnvField::Space), 2.0);
        assert_eq!(read(3, EnvField::Space), 0.0);
        assert_eq!(read(3, EnvField::Sy), 0.0);
        // The way out from the first tip leads away from both neighbours.
        let out = Vec3::new(
            read(0, EnvField::Sx),
            read(0, EnvField::Sy),
            read(0, EnvField::Sz),
        );
        let up = setup.scene.queries[1].position - setup.scene.queries[0].position;
        let side = setup.scene.queries[2].position - setup.scene.queries[0].position;
        assert!(out.dot(up) < 0.0 && out.dot(side) < 0.0);
        assert!((out.length() - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn substrate_1_reads_level_soil_without_a_substrate() {
        let setup = setup(
            "lsystem p 1; module B queries substrate;
             tool substrate@1 {};
             axiom f(0.03) B;",
        );
        let output = tools(&setup, &Neighbourhood::OPEN);
        let read = |field: EnvField| output.env[0].1[field as usize];
        assert!((read(EnvField::Sd) - 0.03).abs() < 1.0e-9);
        assert!((read(EnvField::Sny) - 1.0).abs() < 1.0e-9);
        assert_eq!(read(EnvField::Smat), f64::from(crate::substrate::SOIL));
    }

    #[test]
    fn a_module_reading_the_substrate_needs_its_tool() {
        let error =
            Program::compile("lsystem p 1; module B queries substrate; axiom B;").unwrap_err();
        assert!(error.message.contains("substrate@1"), "{}", error.message);
    }
}
