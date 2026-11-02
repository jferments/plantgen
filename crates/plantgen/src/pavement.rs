//! Pavement: a paved tile with its cracks, holes and the soil lying in
//! them (pavement-cracks plan K1, `PlantGen` side).
//!
//! A [`CrackTile`] is what a host hands over for one tile of road, lot or
//! sidewalk: the slab's rectangle, height and thickness, the gravel base
//! under it, its cracks as polylines with a width and a depth of fill at
//! each point, and its holes (potholes, broken-out patches) as polygons.
//! Nothing in it is a game's: the host makes the cracks from its own decay
//! level, and `PlantGen` grows what roots in them.
//!
//! [`Pavement`] rasterises a tile into cells (1 cm by default) and answers
//! what a site needs:
//!
//! - where a plant set on it stands ([`Pavement::footing`]): in a crack or a
//!   hole, on the soil lying in it; on sound pavement, on the slab, where
//!   nothing roots;
//! - what lies at a point ([`Pavement::classify`]): air, the slab
//!   ([`BUILT`]), the gravel base ([`ROCK`]), the soil in an opening or
//!   under the base ([`SOIL`]), with a signed distance to the surface, so
//!   a site's substrate grid ([`crate::site::SiteMesh::substrate_round`])
//!   carries the pavement as it carries the mesh. A grid coarser than a
//!   crack sees the crack as wide as one of its cells, so a stem can come
//!   up through it.
//! - how it looks ([`Pavement::drawn`]).
//!
//! The fill of a crack is measured up from the slab's bottom: a crack
//! whose fill is the slab's thickness is full to the brim. A hole's fill
//! is measured up from its own bottom.

use serde::{Deserialize, Serialize};

use crate::math::Vec3;
use crate::mesh::Mesh;
use crate::substrate::{AIR, BUILT, ROCK, SOIL};

/// The crack tile's schema.
pub const CRACK_TILE_VERSION: u32 = 1;

/// Share of a compacted gravel base a root can use, when the tile does not
/// say. Roots under pavement keep to the seam between the slab and the
/// base and to the voids among its stones, the zone of least compaction,
/// and stone-soil mixes as dense as a base do not stop them (Grabosky,
/// Bassuk and van Es 1996); a fifth, about the voids of a dense-graded
/// base, is this crate's estimate (soil depth design D1).
pub const BASE_ROOTABLE: f64 = 0.2;
/// Depth of compacted subgrade under a base when the tile does not say;
/// first estimate, to check.
pub const SUBGRADE_M: f64 = 0.3;
/// Share of compacted subgrade a root can use when the tile does not say:
/// none. Grabosky, Bassuk and van Es (1996) found that a clay loam
/// compacted to the peak density pavement specifications call for stopped
/// oak roots altogether. Native soil below counts no more, since roots
/// reach it only through the subgrade; a host whose old road's subgrade
/// has loosened says so in its tile.
pub const SUBGRADE_ROOTABLE: f64 = 0.0;

/// Most cells a tile may rasterise to.
pub const MAX_PAVEMENT_CELLS: usize = 16_000_000;

/// A paved tile with its cracks and holes, in a site's frame: X east, Y
/// up, Z south, metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrackTile {
    pub version: u32,
    /// The tile's corner of least X and Z.
    pub origin_m: [f64; 2],
    /// Its extent along X and Z.
    pub size_m: [f64; 2],
    /// Height of the pavement's surface.
    pub top_m: f64,
    /// The slab's thickness (asphalt 5 to 15 cm, a concrete slab 10 to 25).
    pub thickness_m: f64,
    /// Depth of the gravel base under the slab, above the soil.
    #[serde(default)]
    pub base_m: f64,
    /// Share of the base's volume a root can use (soil depth design D1);
    /// [`BASE_ROOTABLE`] when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_rootable: Option<f64>,
    /// Depth of the compacted subgrade under the base, above native soil;
    /// [`SUBGRADE_M`] when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgrade_m: Option<f64>,
    /// Share of the subgrade's volume a root can use;
    /// [`SUBGRADE_ROOTABLE`] when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subgrade_rootable: Option<f64>,
    /// The paving, for its colour: `asphalt`, `concrete`, `brick` or `gravel`.
    #[serde(default = "asphalt")]
    pub surface: String,
    /// Edge of a raster cell; 1 cm when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_m: Option<f64>,
    #[serde(default)]
    pub cracks: Vec<Crack>,
    #[serde(default)]
    pub holes: Vec<Hole>,
    /// What made it, for the record (a decay level, years since upkeep);
    /// growth does not read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_by: Option<serde_json::Value>,
}

fn asphalt() -> String {
    "asphalt".to_owned()
}

/// A crack: a polyline with its opening and fill at each point, varying
/// linearly between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crack {
    /// Points (X, Z), at least two.
    pub points_m: Vec<[f64; 2]>,
    /// Opening at each point, metres.
    pub width_m: Vec<f64>,
    /// Soil and grit lying in it at each point, up from the slab's bottom.
    pub fill_m: Vec<f64>,
}

/// A hole through the slab: a pothole or a broken-out patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hole {
    /// Its outline (X, Z), at least three points.
    pub outline_m: Vec<[f64; 2]>,
    /// Depth of its bottom below the surface.
    pub depth_m: f64,
    /// Soil lying in it, up from its bottom.
    pub fill_m: f64,
}

impl CrackTile {
    /// A tile from its JSON.
    ///
    /// # Errors
    ///
    /// On JSON that is not a crack tile, or one [`Self::validate`] refuses.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let tile: Self =
            serde_json::from_str(text).map_err(|error| format!("invalid crack tile: {error}"))?;
        tile.validate()?;
        Ok(tile)
    }

    /// Checks the schema, the sizes and every crack and hole.
    ///
    /// # Errors
    ///
    /// On the first value out of range.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != CRACK_TILE_VERSION {
            return Err(format!(
                "crack tile version {} is not {CRACK_TILE_VERSION}",
                self.version
            ));
        }
        let finite = |name: &str, v: f64, low: f64, high: f64| {
            if v.is_finite() && v >= low && v <= high {
                Ok(())
            } else {
                Err(format!(
                    "crack tile {name} {v} is not within {low} to {high}"
                ))
            }
        };
        for v in self.origin_m {
            finite("origin_m", v, -1.0e6, 1.0e6)?;
        }
        for v in self.size_m {
            finite("size_m", v, 0.05, 1000.0)?;
        }
        finite("top_m", self.top_m, -1.0e4, 1.0e4)?;
        finite("thickness_m", self.thickness_m, 0.005, 2.0)?;
        finite("base_m", self.base_m, 0.0, 5.0)?;
        for (name, value, high) in [
            ("base_rootable", self.base_rootable, 1.0),
            ("subgrade_m", self.subgrade_m, 10.0),
            ("subgrade_rootable", self.subgrade_rootable, 1.0),
        ] {
            if let Some(value) = value {
                finite(name, value, 0.0, high)?;
            }
        }
        if let Some(cell) = self.cell_m {
            finite("cell_m", cell, 0.002, 0.1)?;
        }
        if !matches!(
            self.surface.as_str(),
            "asphalt" | "concrete" | "brick" | "gravel"
        ) {
            return Err(format!(
                "crack tile surface `{}` is not asphalt, concrete, brick or gravel",
                self.surface
            ));
        }
        for (n, crack) in self.cracks.iter().enumerate() {
            let points = crack.points_m.len();
            if points < 2 || crack.width_m.len() != points || crack.fill_m.len() != points {
                return Err(format!(
                    "crack {n} needs two points or more and a width and fill at each"
                ));
            }
            for p in &crack.points_m {
                finite("crack point", p[0], -1.0e6, 1.0e6)?;
                finite("crack point", p[1], -1.0e6, 1.0e6)?;
            }
            for w in &crack.width_m {
                finite("crack width_m", *w, 0.0, 2.0)?;
            }
            for f in &crack.fill_m {
                finite("crack fill_m", *f, 0.0, 2.0)?;
            }
        }
        for (n, hole) in self.holes.iter().enumerate() {
            if hole.outline_m.len() < 3 {
                return Err(format!("hole {n} needs three points or more"));
            }
            for p in &hole.outline_m {
                finite("hole point", p[0], -1.0e6, 1.0e6)?;
                finite("hole point", p[1], -1.0e6, 1.0e6)?;
            }
            finite("hole depth_m", hole.depth_m, 0.0, 5.0)?;
            finite("hole fill_m", hole.fill_m, 0.0, hole.depth_m)?;
        }
        Ok(())
    }
}

/// A rasterised crack tile.
#[derive(Debug, Clone)]
pub struct Pavement {
    origin: [f64; 2],
    cell: f64,
    nx: usize,
    nz: usize,
    top: f64,
    thickness: f64,
    base: f64,
    colour: [f32; 3],
    base_rootable: f64,
    subgrade: f64,
    subgrade_rootable: f64,
    /// Height of the soil in each open cell; NaN where the slab is sound.
    soil_top: Vec<f32>,
    /// Height of the bottom of each open cell's opening: the slab's bottom
    /// under a crack, a hole's own bottom; NaN where sound.
    floor: Vec<f32>,
    /// Share of each open cell the opening takes (a 4 mm crack in a 1 cm
    /// cell, 0.4); 0 where sound.
    share: Vec<f32>,
    /// Distance from each cell to the nearest open cell, in cells, and
    /// that cell's index.
    to_open: Vec<f32>,
    nearest_open: Vec<u32>,
    /// Distance from each cell to the nearest sound cell, in cells.
    to_sound: Vec<f32>,
}

/// Linear colours of pavings, weathered, and of soil in a crack.
fn paving_colour(surface: &str) -> [f32; 3] {
    match surface {
        "concrete" => [0.42, 0.41, 0.38],
        "brick" => [0.36, 0.15, 0.10],
        "gravel" => [0.30, 0.28, 0.25],
        _ => [0.12, 0.12, 0.12],
    }
}

const CRACK_SOIL: [f32; 3] = [0.10, 0.08, 0.05];
const CRACK_DEPTHS: [f32; 3] = [0.03, 0.03, 0.025];

impl Pavement {
    /// Rasterises `tile`.
    ///
    /// # Errors
    ///
    /// When the tile is invalid or makes more than [`MAX_PAVEMENT_CELLS`].
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::too_many_lines,
        clippy::many_single_char_names
    )]
    pub fn new(tile: &CrackTile) -> Result<Self, String> {
        tile.validate()?;
        let cell = tile.cell_m.unwrap_or(0.01);
        let nx = (tile.size_m[0] / cell).ceil().max(1.0) as usize;
        let nz = (tile.size_m[1] / cell).ceil().max(1.0) as usize;
        if nx * nz > MAX_PAVEMENT_CELLS {
            return Err(format!(
                "a crack tile of {} cells is larger than {MAX_PAVEMENT_CELLS}",
                nx * nz
            ));
        }
        let total = nx * nz;
        let mut soil_top = vec![f32::NAN; total];
        let mut floor = vec![f32::NAN; total];
        let mut share = vec![0.0_f32; total];
        let [ox, oz] = tile.origin_m;
        let bottom = tile.top_m - tile.thickness_m;
        let centre =
            |i: usize, k: usize| (ox + (i as f64 + 0.5) * cell, oz + (k as f64 + 0.5) * cell);
        let to_cell = |v: f64, low: f64, n: usize| -> usize {
            (((v - low) / cell).floor().max(0.0) as usize).min(n - 1)
        };
        let mut open = |at: usize, top: f64, low: f64, part: f64| {
            let top = top.min(tile.top_m) as f32;
            if soil_top[at].is_nan() || top > soil_top[at] {
                soil_top[at] = top;
            }
            let low = low as f32;
            if floor[at].is_nan() || low < floor[at] {
                floor[at] = low;
            }
            share[at] = share[at].max(part.clamp(0.0, 1.0) as f32);
        };
        for crack in &tile.cracks {
            for s in 0..crack.points_m.len() - 1 {
                let (a, b) = (crack.points_m[s], crack.points_m[s + 1]);
                let (wa, wb) = (crack.width_m[s], crack.width_m[s + 1]);
                let (fa, fb) = (crack.fill_m[s], crack.fill_m[s + 1]);
                if wa.max(wb) <= 0.0 {
                    continue;
                }
                let reach = wa.max(wb) * 0.5 + cell;
                let (i0, i1) = (
                    to_cell(a[0].min(b[0]) - reach, ox, nx),
                    to_cell(a[0].max(b[0]) + reach, ox, nx),
                );
                let (k0, k1) = (
                    to_cell(a[1].min(b[1]) - reach, oz, nz),
                    to_cell(a[1].max(b[1]) + reach, oz, nz),
                );
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let length2 = (dx * dx + dz * dz).max(1.0e-18);
                for k in k0..=k1 {
                    for i in i0..=i1 {
                        let (x, z) = centre(i, k);
                        let t = (((x - a[0]) * dx + (z - a[1]) * dz) / length2).clamp(0.0, 1.0);
                        let (px, pz) = (a[0] + dx * t - x, a[1] + dz * t - z);
                        let gap = (px * px + pz * pz).sqrt();
                        let width = wa + (wb - wa) * t;
                        if width <= 0.0 {
                            continue;
                        }
                        // A crack narrower than a cell opens the cells its
                        // line passes through, for the share it takes.
                        if gap <= (width * 0.5).max(cell * 0.5) + 1.0e-9 {
                            let fill = fa + (fb - fa) * t;
                            open(k * nx + i, bottom + fill, bottom, width / cell);
                        }
                    }
                }
            }
        }
        for hole in &tile.holes {
            let (low_x, high_x) = hole
                .outline_m
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
                    (l.min(p[0]), h.max(p[0]))
                });
            let (low_z, high_z) = hole
                .outline_m
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
                    (l.min(p[1]), h.max(p[1]))
                });
            let soil = tile.top_m - hole.depth_m + hole.fill_m;
            for k in to_cell(low_z, oz, nz)..=to_cell(high_z, oz, nz) {
                for i in to_cell(low_x, ox, nx)..=to_cell(high_x, ox, nx) {
                    let (x, z) = centre(i, k);
                    if inside_polygon(&hole.outline_m, x, z) {
                        open(k * nx + i, soil, tile.top_m - hole.depth_m, 1.0);
                    }
                }
            }
        }
        let is_open: Vec<bool> = soil_top.iter().map(|t| !t.is_nan()).collect();
        let (to_open, nearest_open) = distance_transform(&is_open, nx, nz);
        let is_sound: Vec<bool> = is_open.iter().map(|o| !o).collect();
        let (to_sound, _) = distance_transform(&is_sound, nx, nz);
        Ok(Self {
            origin: tile.origin_m,
            cell,
            nx,
            nz,
            top: tile.top_m,
            thickness: tile.thickness_m,
            base: tile.base_m,
            colour: paving_colour(&tile.surface),
            base_rootable: tile.base_rootable.unwrap_or(BASE_ROOTABLE),
            subgrade: tile.subgrade_m.unwrap_or(SUBGRADE_M),
            subgrade_rootable: tile.subgrade_rootable.unwrap_or(SUBGRADE_ROOTABLE),
            soil_top,
            floor,
            share,
            to_open,
            nearest_open,
            to_sound,
        })
    }

    /// Height of the pavement's surface.
    #[must_use]
    pub const fn top(&self) -> f64 {
        self.top
    }

    /// Edge of a raster cell, metres.
    #[must_use]
    pub const fn cell(&self) -> f64 {
        self.cell
    }

    /// The low and high (X, Z) corners of the tile.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn extent(&self) -> ([f64; 2], [f64; 2]) {
        (
            self.origin,
            [
                self.origin[0] + self.nx as f64 * self.cell,
                self.origin[1] + self.nz as f64 * self.cell,
            ],
        )
    }

    /// Whether (`x`, `z`) lies on the tile.
    #[must_use]
    pub fn covers(&self, x: f64, z: f64) -> bool {
        let (low, high) = self.extent();
        x >= low[0] && x < high[0] && z >= low[1] && z < high[1]
    }

    /// Share of the tile's area open (cracks and holes), weighted by how
    /// much of each cell the opening takes.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn open_share(&self) -> f64 {
        self.share.iter().map(|s| f64::from(*s)).sum::<f64>() / self.share.len() as f64
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn cell_of(&self, x: f64, z: f64) -> Option<usize> {
        if !self.covers(x, z) {
            return None;
        }
        let i = (((x - self.origin[0]) / self.cell) as usize).min(self.nx - 1);
        let k = (((z - self.origin[1]) / self.cell) as usize).min(self.nz - 1);
        Some(k * self.nx + i)
    }

    #[allow(clippy::cast_precision_loss)]
    fn centre_of(&self, at: usize) -> (f64, f64) {
        (
            self.origin[0] + ((at % self.nx) as f64 + 0.5) * self.cell,
            self.origin[1] + ((at / self.nx) as f64 + 0.5) * self.cell,
        )
    }

    /// The opening nearest (`x`, `z`) within `within` metres: its centre
    /// and the height of the soil in it.
    #[must_use]
    pub fn nearest_open(&self, x: f64, z: f64, within: f64) -> Option<(f64, f64, f64)> {
        let at = self.cell_of(x, z)?;
        if f64::from(self.to_open[at]) * self.cell > within {
            return None;
        }
        let near = self.nearest_open[at] as usize;
        let (cx, cz) = self.centre_of(near);
        Some((cx, cz, f64::from(self.soil_top[near])))
    }

    /// Where a plant set at (`x`, `z`) stands, and on what: on the soil in
    /// an opening there, else on the slab ([`BUILT`]), where nothing roots.
    /// `None` off the tile.
    #[must_use]
    pub fn footing(&self, x: f64, z: f64) -> Option<(f64, u8)> {
        let at = self.cell_of(x, z)?;
        let soil = self.soil_top[at];
        Some(if soil.is_nan() {
            (self.top, BUILT)
        } else {
            (f64::from(soil), SOIL)
        })
    }

    /// The rootable share of the ground under (`x`, `z`) by height (soil
    /// depth design D1): layers from the top down as (top, bottom, share),
    /// the last reaching down for ever. The slab counts nothing, the soil
    /// in an opening counts for the share of the cell the opening takes,
    /// the base and the compacted subgrade count their rootable shares,
    /// and native soil below counts no more than the least of those, since
    /// roots reach it only through them. `None` off the tile.
    #[must_use]
    pub fn column(&self, x: f64, z: f64) -> Option<Vec<(f64, f64, f64)>> {
        let at = self.cell_of(x, z)?;
        let slab_bottom = self.top - self.thickness;
        let base_bottom = slab_bottom - self.base;
        let subgrade_bottom = base_bottom - self.subgrade;
        let mut layers = Vec::with_capacity(4);
        let mut from = slab_bottom;
        if !self.soil_top[at].is_nan() {
            let soil = f64::from(self.soil_top[at]);
            let low = f64::from(self.floor[at]);
            if soil > low {
                layers.push((soil, low, f64::from(self.share[at])));
            }
            from = from.min(low);
        }
        if from > base_bottom {
            layers.push((from, base_bottom, self.base_rootable));
        }
        let from = from.min(base_bottom);
        if from > subgrade_bottom {
            layers.push((from, subgrade_bottom, self.subgrade_rootable));
        }
        // Roots reach the ground below only through what they passed, so
        // native soil under the subgrade counts no more than the least
        // rootable layer above it.
        let passed = self.base_rootable.min(self.subgrade_rootable);
        let passed = if self.base > 0.0 || self.subgrade > 0.0 {
            passed
        } else {
            1.0
        };
        layers.push((from.min(subgrade_bottom), f64::NEG_INFINITY, passed));
        Some(layers)
    }

    /// What lies at `p` and its signed distance to the pavement's surface
    /// (below 0 inside), as a substrate grid of cells `grid_m` wide sees it:
    /// an opening within half a grid cell counts as under the grid cell's
    /// centre, so a crack narrower than the grid is one cell wide. `None`
    /// off the tile.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn classify(&self, p: Vec3, grid_m: f64) -> Option<(f64, u8)> {
        let at = self.cell_of(p.x, p.z)?;
        let to_open = f64::from(self.to_open[at]) * self.cell;
        let to_sound = f64::from(self.to_sound[at]) * self.cell;
        let widen = grid_m * 0.5;
        let open_here = to_open <= widen;
        if open_here {
            let soil = f64::from(self.soil_top[self.nearest_open[at] as usize]);
            if p.y < soil {
                return Some((-(soil - p.y).max(1.0e-6), SOIL));
            }
            // The walls stand where the sound slab begins, at least half a
            // grid cell off.
            let wall = (to_sound - to_open).max(widen);
            let gap = if p.y < self.top {
                (p.y - soil).min(wall)
            } else {
                (p.y - soil).min(wall.hypot(p.y - self.top))
            };
            return Some((gap.max(1.0e-6), AIR));
        }
        if p.y >= self.top {
            return Some(((p.y - self.top).max(1.0e-6), AIR));
        }
        let material = if p.y >= self.top - self.thickness {
            BUILT
        } else if p.y >= self.top - self.thickness - self.base {
            ROCK
        } else {
            SOIL
        };
        let depth = self.top - p.y;
        // Inside: the surface above, or a crack's wall beside, whichever is
        // nearer, the wall only where the crack is open at this height.
        let soil = f64::from(self.soil_top[self.nearest_open[at] as usize]);
        let wall = if self.to_open[at].is_finite() && p.y > soil {
            to_open - widen
        } else {
            f64::INFINITY
        };
        Some((-depth.min(wall).max(1.0e-6), material))
    }

    /// The tile within `low` to `high`, moved by `offset`, as a mesh to
    /// draw: the slab's surface, the soil in its openings and its sides,
    /// flat-shaded. Runs of like cells merge into rectangles.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn drawn(&self, low: Vec3, high: Vec3, offset: Vec3) -> Mesh {
        let mut mesh = Mesh::default();
        let (lo, hi) = self.extent();
        if lo[0] > high.x || lo[1] > high.z || hi[0] < low.x || hi[1] < low.z {
            return mesh;
        }
        let mut quad = |corners: [Vec3; 4], normal: Vec3, colour: [f32; 3]| {
            let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for p in corners {
                mesh.positions.push((p + offset).to_f32());
                mesh.normals.push(normal.to_f32());
                mesh.uvs.push([0.0, 0.0]);
                mesh.colors.push([colour[0], colour[1], colour[2], 1.0]);
                mesh.births.push(0.0);
                mesh.sheds.push(f32::INFINITY);
                mesh.levels.push(0);
            }
            mesh.indices
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        };
        // Merge each row's runs, then stack runs of the same span and kind.
        let kind = |at: usize| -> Option<u32> {
            let t = self.soil_top[at];
            // Sound cells share one kind; open cells by their soil height
            // in millimetres.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            if t.is_nan() {
                Some(u32::MAX)
            } else {
                Some(((f64::from(t) - self.top + 10.0) * 1000.0) as u32)
            }
        };
        let mut done = vec![false; self.nx * self.nz];
        for k in 0..self.nz {
            let mut i = 0;
            while i < self.nx {
                let at = k * self.nx + i;
                if done[at] {
                    i += 1;
                    continue;
                }
                let what = kind(at);
                let mut j = i + 1;
                while j < self.nx && !done[k * self.nx + j] && kind(k * self.nx + j) == what {
                    j += 1;
                }
                let mut m = k + 1;
                'rows: while m < self.nz {
                    for c in i..j {
                        let here = m * self.nx + c;
                        if done[here] || kind(here) != what {
                            break 'rows;
                        }
                    }
                    m += 1;
                }
                for r in k..m {
                    for c in i..j {
                        done[r * self.nx + c] = true;
                    }
                }
                let x0 = self.origin[0] + i as f64 * self.cell;
                let x1 = self.origin[0] + j as f64 * self.cell;
                let z0 = self.origin[1] + k as f64 * self.cell;
                let z1 = self.origin[1] + m as f64 * self.cell;
                let (y, colour) = if what == Some(u32::MAX) {
                    (self.top, self.colour)
                } else {
                    (f64::from(self.soil_top[at]), CRACK_SOIL)
                };
                quad(
                    [
                        Vec3::new(x0, y, z0),
                        Vec3::new(x0, y, z1),
                        Vec3::new(x1, y, z1),
                        Vec3::new(x1, y, z0),
                    ],
                    Vec3::new(0.0, 1.0, 0.0),
                    colour,
                );
                i = j;
            }
        }
        // A floor under the slab, so a crack seen at a slant shows its dark
        // depths and not what lies beyond the tile.
        let floor = self.top - self.thickness - 1.0e-3;
        quad(
            [
                Vec3::new(lo[0], floor, lo[1]),
                Vec3::new(lo[0], floor, hi[1]),
                Vec3::new(hi[0], floor, hi[1]),
                Vec3::new(hi[0], floor, lo[1]),
            ],
            Vec3::new(0.0, 1.0, 0.0),
            CRACK_DEPTHS,
        );
        // The tile's sides, down through the base.
        let y0 = self.top - self.thickness - self.base;
        let y1 = self.top;
        let side = paving_side(self.colour);
        quad(
            [
                Vec3::new(lo[0], y0, lo[1]),
                Vec3::new(hi[0], y0, lo[1]),
                Vec3::new(hi[0], y1, lo[1]),
                Vec3::new(lo[0], y1, lo[1]),
            ],
            Vec3::new(0.0, 0.0, -1.0),
            side,
        );
        quad(
            [
                Vec3::new(hi[0], y0, hi[1]),
                Vec3::new(lo[0], y0, hi[1]),
                Vec3::new(lo[0], y1, hi[1]),
                Vec3::new(hi[0], y1, hi[1]),
            ],
            Vec3::new(0.0, 0.0, 1.0),
            side,
        );
        quad(
            [
                Vec3::new(lo[0], y0, hi[1]),
                Vec3::new(lo[0], y0, lo[1]),
                Vec3::new(lo[0], y1, lo[1]),
                Vec3::new(lo[0], y1, hi[1]),
            ],
            Vec3::new(-1.0, 0.0, 0.0),
            side,
        );
        quad(
            [
                Vec3::new(hi[0], y0, lo[1]),
                Vec3::new(hi[0], y0, hi[1]),
                Vec3::new(hi[0], y1, hi[1]),
                Vec3::new(hi[0], y1, lo[1]),
            ],
            Vec3::new(1.0, 0.0, 0.0),
            side,
        );
        mesh
    }
}

fn paving_side(colour: [f32; 3]) -> [f32; 3] {
    colour.map(|c| c * 0.8)
}

/// Whether (`x`, `z`) lies inside `polygon` (even-odd rule).
fn inside_polygon(polygon: &[[f64; 2]], x: f64, z: f64) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > z) != (b[1] > z) && x < (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Distance in cells from each cell to the nearest `seed` cell, and that
/// cell's index (`u32::MAX`, at infinite distance, with no seed): an
/// eight-neighbour vector propagation in two passes (Danielsson 1980),
/// within a fraction of a cell of the exact distance.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn distance_transform(seed: &[bool], nx: usize, nz: usize) -> (Vec<f32>, Vec<u32>) {
    let total = nx * nz;
    let far = i32::MAX / 4;
    let mut offset = vec![(far, far); total];
    for (at, &s) in seed.iter().enumerate() {
        if s {
            offset[at] = (0, 0);
        }
    }
    let length = |(dx, dz): (i32, i32)| -> i64 {
        i64::from(dx) * i64::from(dx) + i64::from(dz) * i64::from(dz)
    };
    let (w, h) = (nx as i32, nz as i32);
    let relax = |offset: &mut Vec<(i32, i32)>, i: i32, k: i32, di: i32, dk: i32| {
        let (ni, nk) = (i + di, k + dk);
        if ni < 0 || nk < 0 || ni >= w || nk >= h {
            return;
        }
        let from = offset[(nk * w + ni) as usize];
        if from.0 >= far {
            return;
        }
        let candidate = (from.0 + di, from.1 + dk);
        let at = (k * w + i) as usize;
        if length(candidate) < length(offset[at]) {
            offset[at] = candidate;
        }
    };
    for k in 0..h {
        for i in 0..w {
            for (di, dk) in [(-1, 0), (-1, -1), (0, -1), (1, -1)] {
                relax(&mut offset, i, k, di, dk);
            }
        }
        for i in (0..w).rev() {
            relax(&mut offset, i, k, 1, 0);
        }
    }
    for k in (0..h).rev() {
        for i in (0..w).rev() {
            for (di, dk) in [(1, 0), (1, 1), (0, 1), (-1, 1)] {
                relax(&mut offset, i, k, di, dk);
            }
        }
        for i in 0..w {
            relax(&mut offset, i, k, -1, 0);
        }
    }
    let mut distance = vec![f32::INFINITY; total];
    let mut nearest = vec![u32::MAX; total];
    for k in 0..h {
        for i in 0..w {
            let at = (k * w + i) as usize;
            let (dx, dz) = offset[at];
            if dx >= far {
                continue;
            }
            distance[at] = (length((dx, dz)) as f64).sqrt() as f32;
            nearest[at] = u32::try_from((k + dz) * w + (i + dx)).unwrap_or(u32::MAX);
        }
    }
    (distance, nearest)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn tile() -> CrackTile {
        CrackTile {
            version: 1,
            origin_m: [0.0, 0.0],
            size_m: [1.0, 1.0],
            top_m: 0.0,
            thickness_m: 0.08,
            base_m: 0.2,
            base_rootable: None,
            subgrade_m: None,
            subgrade_rootable: None,
            surface: "asphalt".into(),
            cell_m: None,
            cracks: vec![Crack {
                points_m: vec![[0.1, 0.5], [0.9, 0.5]],
                width_m: vec![0.004, 0.02],
                fill_m: vec![0.02, 0.06],
            }],
            holes: vec![Hole {
                outline_m: vec![[0.6, 0.7], [0.8, 0.7], [0.8, 0.9], [0.6, 0.9]],
                depth_m: 0.15,
                fill_m: 0.1,
            }],
            made_by: None,
        }
    }

    #[test]
    fn a_plant_stands_in_a_crack_and_not_on_sound_pavement() {
        let pavement = Pavement::new(&tile()).unwrap();
        let (y, what) = pavement.footing(0.5, 0.505).unwrap();
        assert_eq!(what, SOIL);
        // Fill halfway along: 4 cm up from the slab's bottom at -0.08.
        assert!((y - (-0.04)).abs() < 0.002, "{y}");
        assert_eq!(pavement.footing(0.5, 0.2), Some((0.0, BUILT)));
        assert_eq!(pavement.footing(2.0, 0.2), None);
        let (_, what) = pavement.footing(0.7, 0.8).unwrap();
        assert_eq!(what, SOIL);
        let (x, z, soil) = pavement.nearest_open(0.5, 0.53, 0.05).unwrap();
        assert!((z - 0.5).abs() < 0.02 && (x - 0.5).abs() < 0.02, "{x} {z}");
        assert!(soil < 0.0);
        assert!(pavement.nearest_open(0.3, 0.2, 0.05).is_none());
    }

    #[test]
    fn layers_lie_under_the_surface_and_a_crack_opens_through_them() {
        let pavement = Pavement::new(&tile()).unwrap();
        let at = |x: f64, y: f64, z: f64| pavement.classify(Vec3::new(x, y, z), 0.025).unwrap();
        assert_eq!(at(0.3, 0.05, 0.2).1, AIR);
        assert!((at(0.3, 0.05, 0.2).0 - 0.05).abs() < 1.0e-9);
        assert_eq!(at(0.3, -0.04, 0.2).1, BUILT);
        assert_eq!(at(0.3, -0.15, 0.2).1, ROCK);
        assert_eq!(at(0.3, -0.4, 0.2).1, SOIL);
        // In the crack: air above its soil, soil below.
        assert_eq!(at(0.5, -0.03, 0.5).1, AIR);
        assert!(at(0.5, -0.03, 0.5).0 > 0.0);
        assert_eq!(at(0.5, -0.06, 0.5).1, SOIL);
        // A grid of 2.5 cm cells sees a 4 mm crack one cell wide.
        assert_eq!(at(0.15, -0.01, 0.51).1, AIR);
        assert_eq!(at(0.15, -0.01, 0.55).1, BUILT);
        assert!(pavement.open_share() > 0.04 && pavement.open_share() < 0.06);
    }

    #[test]
    fn distances_find_the_nearest_seed() {
        let mut seed = vec![false; 25];
        seed[0] = true;
        let (d, n) = distance_transform(&seed, 5, 5);
        assert_eq!(n[24], 0);
        assert!((d[24] - 32.0_f32.sqrt()).abs() < 1.0e-5);
        assert_eq!(d[0], 0.0);
    }

    #[test]
    fn the_tile_draws_as_merged_rectangles() {
        let pavement = Pavement::new(&tile()).unwrap();
        let mesh = pavement.drawn(
            Vec3::new(-10.0, -10.0, -10.0),
            Vec3::new(10.0, 10.0, 10.0),
            Vec3::new(0.0, 0.0, 0.0),
        );
        // Far fewer quads than the 10 000 cells.
        assert!(mesh.indices.len() / 6 < 1500, "{}", mesh.indices.len() / 6);
        assert!(!mesh.indices.is_empty());
    }

    #[test]
    fn a_tile_must_be_whole() {
        let mut bad = tile();
        bad.cracks[0].fill_m.pop();
        assert!(bad.validate().is_err());
        let mut bad = tile();
        bad.surface = "marble".into();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn roots_find_nothing_in_the_slab_and_part_of_the_base() {
        let pavement = Pavement::new(&tile()).unwrap();
        let sound = pavement.column(0.3, 0.2).unwrap();
        let near = |a: f64, b: f64| (a - b).abs() < 1.0e-6;
        assert_eq!(sound.len(), 3);
        assert!(near(sound[0].0, -0.08) && near(sound[0].1, -0.28));
        assert!(near(sound[0].2, BASE_ROOTABLE));
        assert!(near(sound[1].1, -0.28 - SUBGRADE_M));
        assert!(sound[2].1.is_infinite() && near(sound[2].2, SUBGRADE_ROOTABLE));
        // The hole's soil, 5 to 15 cm down, then the base below its bottom.
        let hole = pavement.column(0.7, 0.8).unwrap();
        assert!(near(hole[0].0, -0.05) && near(hole[0].1, -0.15) && near(hole[0].2, 1.0));
        assert!(near(hole[1].0, -0.15) && near(hole[1].1, -0.28));
        assert!(pavement.column(2.0, 0.5).is_none());
    }
}
