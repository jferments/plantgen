//! A site field: what a whole tile of a site offers plants, measured once
//! and read cheaply (sites plan S4, for the city-tile stands).
//!
//! Growing a plant on a [`SiteMesh`] cuts its substrate from the mesh
//! round its foot ([`SiteMesh::substrate_round`]), which is right for the
//! plants a host sets one by one and far too slow for every plant of a
//! city tile. A [`SiteField`] is built once per tile from the same mesh,
//! material table and paved tiles, and stand placement reads it without
//! touching the mesh again. It has three parts:
//!
//! 1. **The ground grid** ([`Ground`]): square cells about a metre across
//!    over the tile and a margin round it, each with the ground a plant
//!    can stand on there (soil, else whatever faces up: a pavement, bare
//!    rock), its slope, aspect and heat load, its material, how deep its
//!    soil is, how wet its place in the terrain is, how much sky it sees,
//!    how far it is from a wall and from a surface kept clear, and whether
//!    it lies under a roof.
//! 2. **Surface patches** ([`Patch`]) on everything that is not ground:
//!    walls, roofs, overhangs, fences, poles, boulders and logs. Each
//!    belongs to a [`Host`] (one wall face, roof face, pole, fence bay or
//!    boulder) and has coordinates (u, v) on it, so a climber's cover can
//!    be laid across neighbouring patches of one host.
//! 3. **Crack sites** ([`CrackSite`]): the openings of the paved tiles, as
//!    [`Pavement::openings`] finds them, each naming its ground cell, so
//!    the yearly step ([`crate::succession`]) can run on them.
//!
//! The margin (20 m by default, a big crown's reach) lets neighbouring
//! tiles measure their shared edge alike when each is handed the mesh
//! round it. Cells and patches are numbered in a fixed order, so the same
//! inputs give the same field, byte for byte. The file is versioned
//! ([`SITE_FIELD_VERSION`]) and documented in `docs/sites/SITE-FIELD.md`.
//!
//! Every quantity here is geometry the mesh gives, or a named model
//! choice; nothing is a species' fact.

use serde::{Deserialize, Serialize};

use crate::lsys::tools::closest_on_triangle;
use crate::math::Vec3;
use crate::pavement::Pavement;
use crate::site::{SiteMesh, climbable};
use crate::substrate::{BARK, BUILT, ROCK, SOIL, WOOD};

/// The version of the site field file. It goes up when a field is
/// renamed, removed or changes its meaning; fields added beside the
/// others leave it as it is.
pub const SITE_FIELD_VERSION: u32 = 1;

/// How the field is measured.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldSettings {
    /// Edge of a ground cell, metres.
    pub cell_m: f64,
    /// How far past the tile the field reaches, metres.
    pub margin_m: f64,
    /// Edge of a surface patch, metres.
    pub patch_m: f64,
    /// Rays cast to measure how much sky a cell or patch sees.
    pub sky_rays: u32,
    /// How far a sky ray looks for something in its way, metres.
    pub sky_reach_m: f64,
    /// How far a cell looks for a wall or a surface kept clear, metres;
    /// farther ones count as this far.
    pub wall_reach_m: f64,
    /// The site's latitude, degrees (negative south), for heat load.
    pub latitude_deg: f64,
    /// Edge of a fence bay, metres: a long fence is one host per bay.
    pub fence_bay_m: f64,
}

impl Default for FieldSettings {
    fn default() -> Self {
        Self {
            cell_m: 1.0,
            margin_m: 20.0,
            patch_m: 0.75,
            sky_rays: 24,
            sky_reach_m: 60.0,
            wall_reach_m: 10.0,
            latitude_deg: 45.0,
            fence_bay_m: 2.5,
        }
    }
}

/// The ground grid, one value per cell in each list, row by row: cell
/// `index = row * columns + column`, its centre at
/// `origin + ((column + 0.5) * cell_m, (row + 0.5) * cell_m)` in X and Z.
/// A cell with no ground (`height` null) has nulls or zeros elsewhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ground {
    /// Height of the ground's surface at the cell's centre, metres.
    pub height: Vec<Option<f64>>,
    /// Slope, degrees from level.
    pub slope_deg: Vec<f64>,
    /// The compass bearing the slope faces, degrees (0 north, 90 east);
    /// 0 on level ground.
    pub aspect_deg: Vec<f64>,
    /// McCune and Keon's heat load index (2002, their equation 3): the
    /// sun a slope takes in over the year, by latitude, slope and aspect;
    /// about 0.5 to 1.1; level ground at 45 degrees 0.71.
    pub heat_load: Vec<f64>,
    /// The ground's material: 1 soil, 2 rock, 3 bark, 4 wood, 5 built
    /// (the substrate's numbers); a paved tile is 5.
    pub material: Vec<u8>,
    /// Depth of rootable soil from the surface, metres; null where it
    /// goes deeper than roots do. In a paved cell, the deepest soil in
    /// any of its openings.
    pub rooting_m: Vec<Option<f64>>,
    /// Share of a paved cell's surface open to soil (its cracks and
    /// holes); 1 off a pavement on soil, 0 on rock or a roof.
    pub open_share: Vec<f64>,
    /// The topographic wetness index, ln(a / tan β), of the cell's place
    /// in the grid's terrain: higher where water gathers.
    pub wetness: Vec<f64>,
    /// Share of the sky the ground sees, 0 to 1, weighted as light
    /// falls on a level surface (a sky view factor).
    pub sky: Vec<f64>,
    /// Metres to the nearest wall or other upright solid that is not
    /// ground, up to `wall_reach_m`.
    pub wall_m: Vec<f64>,
    /// Metres to the nearest surface kept clear (a window, a door), up
    /// to `wall_reach_m`.
    pub clear_m: Vec<f64>,
    /// Whether a roof or other cover stands over the ground.
    pub under_roof: Vec<bool>,
    /// The crack sites in each cell, indices into `cracks`.
    pub cracks: Vec<Vec<u32>>,
}

/// What kind of thing a host is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// An upright face of a building or other built solid.
    Wall,
    /// A face of a building that faces up: a roof, a ledge, a flat top.
    Roof,
    /// A face that faces down: the underside of eaves or a balcony.
    Overhang,
    /// A post, pole or trunk standing upright, narrow and tall.
    Pole,
    /// One bay of a fence or other low, long, thin upright.
    Fence,
    /// A wire, a mesh or another thin part not a fence's board.
    Thin,
    /// Rock standing out of the ground.
    Boulder,
    /// Wood or bark lying on the ground.
    Log,
    /// Anything else.
    Other,
}

/// One host of surface patches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub id: u32,
    pub kind: HostKind,
    /// Its commonest material by area.
    pub material: u8,
    /// Lowest point, metres: where `v` is 0.
    pub foot_m: f64,
    /// Low and high corners of its box.
    pub low: [f64; 3],
    pub high: [f64; 3],
    /// The direction `u` runs along it: along a wall or a roof's eaves,
    /// along a fence; round a pole or a boulder `u` is the arc length
    /// round its upright axis from north, and this is null.
    pub u_axis: Option<[f64; 3]>,
    /// For a pole or a boulder, its upright axis's (x, z).
    pub axis: Option<[f64; 2]>,
}

/// A patch of a host's surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub host: u32,
    /// Metres along the host (see [`Host::u_axis`]) and up from its foot
    /// (up its slope on a roof), at the patch's corner.
    pub u: f64,
    pub v: f64,
    /// Centre, by area.
    pub centre: [f64; 3],
    /// Unit normal, by area.
    pub normal: [f64; 3],
    /// Area, m².
    pub area: f64,
    /// Its commonest material by area.
    pub material: u8,
    /// Metres from the ground under its centre to its centre.
    pub height_m: f64,
    /// Share of the sky it sees round its normal, 0 to 1.
    pub sky: f64,
    /// The ground cell a climber on it roots in: under its foot, a step
    /// out from the face; null when there is no ground there.
    pub root_cell: Option<u32>,
    /// Whether most of it is kept clear.
    pub clear: bool,
    /// Whether a climber may hold to most of it.
    pub climbable: bool,
    /// A thin part's thickness, metres (a wire's, a slat's); 0 for a
    /// face of a solid.
    pub thin_m: f64,
    /// Soil lying on it where it faces up, metres, as the material table
    /// states (`soil_m`); 0 on an upright or clean face.
    pub soil_m: f64,
}

/// An opening of a paved tile where a plant can come up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrackSite {
    /// The paved tile, in the order the site lists them.
    pub tile: u32,
    pub x: f64,
    pub z: f64,
    /// Height of the soil's surface, metres.
    pub y: f64,
    /// Depth of soil, metres.
    pub soil_depth_m: f64,
    /// Width of the opening, metres.
    pub width_m: f64,
    /// Metres the soil lies below the pavement's surface.
    pub sunk_m: f64,
    /// How heavily a low plant there is trodden or driven over, 0 to 1.
    pub wear: f64,
    /// Its ground cell.
    pub cell: Option<u32>,
}

/// A tile's site field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteField {
    pub version: u32,
    pub settings: FieldSettings,
    /// The tile the host asked for, X and Z, low then high; the grid
    /// covers it and `margin_m` round it.
    pub tile_low: [f64; 2],
    pub tile_high: [f64; 2],
    /// The grid's low corner, X and Z.
    pub origin: [f64; 2],
    pub columns: u32,
    pub rows: u32,
    pub ground: Ground,
    pub hosts: Vec<Host>,
    pub patches: Vec<Patch>,
    pub cracks: Vec<CrackSite>,
}

impl SiteField {
    /// The cell (column, row) holding (`x`, `z`), when the grid covers it.
    #[must_use]
    pub fn cell_at(&self, x: f64, z: f64) -> Option<u32> {
        cell_index(
            self.origin,
            self.settings.cell_m,
            self.columns,
            self.rows,
            x,
            z,
        )
    }

    /// The centre (x, z) of cell `index`.
    #[must_use]
    pub fn cell_centre(&self, index: u32) -> [f64; 2] {
        let (column, row) = (index % self.columns, index / self.columns);
        [
            self.origin[0] + (f64::from(column) + 0.5) * self.settings.cell_m,
            self.origin[1] + (f64::from(row) + 0.5) * self.settings.cell_m,
        ]
    }

    /// Height of the ground at (`x`, `z`), blended between the centres of
    /// the four cells round it by distance (bilinear). Where some of them
    /// have no ground (a wall's foot, a boulder), it blends the others;
    /// where none has, it is `None`.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn height_at(&self, x: f64, z: f64) -> Option<f64> {
        let cell = self.settings.cell_m;
        let fx = (x - self.origin[0]) / cell - 0.5;
        let fz = (z - self.origin[1]) / cell - 0.5;
        if !(fx.is_finite() && fz.is_finite()) {
            return None;
        }
        let (c0, r0) = (fx.floor(), fz.floor());
        let (tx, tz) = (fx - c0, fz - r0);
        let (mut sum, mut weight) = (0.0, 0.0);
        for (dc, dr, w) in [
            (0.0, 0.0, (1.0 - tx) * (1.0 - tz)),
            (1.0, 0.0, tx * (1.0 - tz)),
            (0.0, 1.0, (1.0 - tx) * tz),
            (1.0, 1.0, tx * tz),
        ] {
            let (c, r) = (c0 + dc, r0 + dr);
            if c < 0.0 || r < 0.0 || c >= f64::from(self.columns) || r >= f64::from(self.rows) {
                continue;
            }
            let index = r as usize * self.columns as usize + c as usize;
            if let Some(Some(h)) = self.ground.height.get(index) {
                sum += h * w;
                weight += w;
            }
        }
        // A point on a cell's centre takes that cell alone, even beside
        // cells with no ground.
        (weight > 1.0e-12).then(|| sum / weight)
    }

    /// Heat load of a cell relative to level ground at the field's
    /// latitude: 1 on level ground, above 1 facing the south-west (the
    /// north-west south of the equator). It is the scale a niche's
    /// `heat_load` range is written in ([`crate::niche::SiteFactors`]).
    #[must_use]
    pub fn relative_heat_load(&self, cell: u32) -> Option<f64> {
        let index = cell as usize;
        self.ground.height.get(index).copied().flatten()?;
        let level = heat_load_index(self.settings.latitude_deg, 0.0, 0.0);
        Some(self.ground.heat_load[index] / level)
    }

    /// What a species' niche reads of a cell
    /// ([`crate::niche::Niche::site_response`]): its heat load relative to
    /// level ground, its slope, and its elevation as its height above
    /// `datum_m` (the height above sea level of the field's y = 0). The
    /// field does not say how wet the ground is in a niche's terms, so the
    /// host gives `moisture`, 0 to 1; the cell's `wetness` index says
    /// where water gathers. `None` where the cell has no ground.
    #[must_use]
    pub fn site_factors(
        &self,
        cell: u32,
        datum_m: f64,
        moisture: f64,
    ) -> Option<crate::niche::SiteFactors> {
        let index = cell as usize;
        let height = self.ground.height.get(index).copied().flatten()?;
        Some(crate::niche::SiteFactors {
            moisture,
            heat_load: self.relative_heat_load(cell)?,
            elevation_m: datum_m + height,
            slope_deg: self.ground.slope_deg[index],
        })
    }

    /// The field as JSON.
    ///
    /// # Errors
    ///
    /// Never in practice; serialisation of plain numbers.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    /// A field read back from [`Self::to_json`]'s text.
    ///
    /// # Errors
    ///
    /// On text that does not parse, or a version this build does not read.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let field: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if field.version != SITE_FIELD_VERSION {
            return Err(format!(
                "site field version {} is not {SITE_FIELD_VERSION}",
                field.version
            ));
        }
        Ok(field)
    }
}

fn cell_index(origin: [f64; 2], cell: f64, columns: u32, rows: u32, x: f64, z: f64) -> Option<u32> {
    let c = ((x - origin[0]) / cell).floor();
    let r = ((z - origin[1]) / cell).floor();
    if c < 0.0 || r < 0.0 || c >= f64::from(columns) || r >= f64::from(rows) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(r as u32 * columns + c as u32)
}

/// Upward normals steeper than this (its Y) are ground or a roof; below
/// minus this, an overhang; between, upright.
const UPRIGHT_Y: f64 = 0.5;

/// Neighbouring faces of a building whose normals turn more than this
/// (cosine) are two hosts: about 25 degrees.
const FACE_TURN_COS: f64 = 0.9;

/// A part narrower than this across and three times as tall (at least a
/// metre) is a pole.
const POLE_WIDE_M: f64 = 0.8;

/// A wooden or bark part no taller than this and no thicker than
/// [`FENCE_THICK_M`] that runs longer than it is tall is a fence.
const FENCE_TALL_M: f64 = 2.5;
const FENCE_THICK_M: f64 = 0.35;

/// Builds the field of `site` for the tile from `low` to `high` (X, Z),
/// with `settings`.
///
/// # Errors
///
/// On settings that are not positive and finite, a tile with no area, or
/// a grid of more than 25 million cells.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn build(
    site: &SiteMesh,
    low: [f64; 2],
    high: [f64; 2],
    settings: &FieldSettings,
) -> Result<SiteField, String> {
    let positive = [
        settings.cell_m,
        settings.patch_m,
        settings.sky_reach_m,
        settings.wall_reach_m,
        settings.fence_bay_m,
    ];
    if !positive.iter().all(|v| v.is_finite() && *v > 0.0)
        || !settings.margin_m.is_finite()
        || settings.margin_m < 0.0
        || !settings.latitude_deg.is_finite()
        || settings.latitude_deg.abs() > 90.0
    {
        return Err(format!("site field settings out of range: {settings:?}"));
    }
    if !(high[0] > low[0] && high[1] > low[1]) || !low.iter().chain(&high).all(|v| v.is_finite()) {
        return Err(format!("a site field's tile runs from {low:?} to {high:?}"));
    }
    let cell = settings.cell_m;
    let origin = [low[0] - settings.margin_m, low[1] - settings.margin_m];
    let columns = ((high[0] + settings.margin_m - origin[0]) / cell).ceil() as u32;
    let rows = ((high[1] + settings.margin_m - origin[1]) / cell).ceil() as u32;
    let count = u64::from(columns) * u64::from(rows);
    if count > 25_000_000 {
        return Err(format!("a site field of {count} cells is too large"));
    }
    let count = count as usize;
    let centre = |index: usize| -> (f64, f64) {
        let (c, r) = (index % columns as usize, index / columns as usize);
        (
            origin[0] + (c as f64 + 0.5) * cell,
            origin[1] + (r as f64 + 0.5) * cell,
        )
    };

    // Ground height and material.
    let mut height: Vec<Option<f64>> = vec![None; count];
    // The ground's surface wherever the mesh has one, inside solids too:
    // what a host's face sinks below.
    let mut terrain: Vec<Option<f64>> = vec![None; count];
    let mut material = vec![0_u8; count];
    let mut under_roof = vec![false; count];
    for index in 0..count {
        let (x, z) = centre(index);
        if let Some((y, stuff)) = site.footing(x, z, None) {
            terrain[index] = Some(y);
            let crossings = site.crossings(x, z);
            // Inside a closed solid (a boulder's sunk foot, a house's
            // footprint) the ground runs on where nothing stands: a ray
            // from there to the sky crosses the solids an odd number of
            // times (faces pressed together, a wall's top under its roof,
            // cancel out).
            let above: Vec<f64> = crossings
                .iter()
                .map(|&(h, _)| h)
                .filter(|&h| h > y + 1.0e-3)
                .collect();
            if above.len() % 2 == 1 && site.pavements().iter().all(|t| !t.covers(x, z)) {
                continue;
            }
            height[index] = Some(y);
            material[index] = stuff;
            under_roof[index] = above.iter().any(|&h| h > y + 0.05);
        }
    }
    // Slope and aspect from the neighbours' heights.
    let at = |c: i64, r: i64| -> Option<f64> {
        if c < 0 || r < 0 || c >= i64::from(columns) || r >= i64::from(rows) {
            return None;
        }
        height[(r as usize) * columns as usize + c as usize]
    };
    let mut slope_deg = vec![0.0; count];
    let mut aspect_deg = vec![0.0; count];
    let mut heat_load = vec![0.0; count];
    let mut tan_slope = vec![0.0; count];
    for index in 0..count {
        let Some(here) = height[index] else {
            continue;
        };
        let (c, r) = (
            (index % columns as usize) as i64,
            (index / columns as usize) as i64,
        );
        let gradient = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (b - a) / (2.0 * cell),
            (Some(a), None) => (here - a) / cell,
            (None, Some(b)) => (b - here) / cell,
            (None, None) => 0.0,
        };
        // Steps over a wall or a curb are not the ground's slope.
        let near = |v: Option<f64>| v.filter(|v| (v - here).abs() <= 2.0 * cell);
        let hx = gradient(near(at(c - 1, r)), near(at(c + 1, r)));
        let hz = gradient(near(at(c, r - 1)), near(at(c, r + 1)));
        let steep = hx.hypot(hz);
        tan_slope[index] = steep;
        slope_deg[index] = steep.atan().to_degrees();
        // Downhill is -gradient; north is -Z, east +X.
        let aspect = if steep < 1.0e-9 {
            0.0
        } else {
            (-hx).atan2(hz).to_degrees().rem_euclid(360.0)
        };
        aspect_deg[index] = aspect;
        heat_load[index] = heat_load_index(settings.latitude_deg, slope_deg[index], aspect);
    }

    // Wetness: D8 flow over the grid, steepest descent.
    let mut order: Vec<usize> = (0..count).filter(|i| height[*i].is_some()).collect();
    order.sort_by(|a, b| {
        height[*b]
            .unwrap_or(0.0)
            .total_cmp(&height[*a].unwrap_or(0.0))
            .then(a.cmp(b))
    });
    let mut area = vec![cell * cell; count];
    for &index in &order {
        let here = height[index].unwrap_or(0.0);
        let (c, r) = (
            (index % columns as usize) as i64,
            (index / columns as usize) as i64,
        );
        let mut best: Option<(f64, usize)> = None;
        for (dc, dr) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let Some(there) = at(c + dc, r + dr) else {
                continue;
            };
            let run = cell * f64::from(i32::try_from(dc * dc + dr * dr).unwrap_or(1)).sqrt();
            let drop = (here - there) / run;
            if drop > 0.0 && (here - there) <= 2.0 * cell && best.is_none_or(|(d, _)| drop > d) {
                best = Some((
                    drop,
                    ((r + dr) as usize) * columns as usize + (c + dc) as usize,
                ));
            }
        }
        if let Some((_, down)) = best {
            area[down] += area[index];
        }
    }
    let wetness: Vec<f64> = (0..count)
        .map(|index| {
            if height[index].is_none() {
                return 0.0;
            }
            let a = area[index] / cell;
            (a / tan_slope[index].max(1.0e-3)).ln()
        })
        .collect();

    // Rooting depth and open share.
    let mut rooting_m: Vec<Option<f64>> = vec![Some(0.0); count];
    let mut open_share = vec![0.0; count];
    for index in 0..count {
        let Some(y) = height[index] else {
            rooting_m[index] = None;
            continue;
        };
        let (x, z) = centre(index);
        if let Some(tile) = site.pavements().iter().find(|t| t.covers(x, z)) {
            let (deepest, open) = paved_cell(tile, x, z, cell);
            rooting_m[index] = Some(deepest);
            open_share[index] = open;
            continue;
        }
        if material[index] != SOIL {
            continue;
        }
        open_share[index] = 1.0;
        // The soil's bottom: the next crossing below its surface.
        rooting_m[index] = site
            .crossings(x, z)
            .iter()
            .find(|&&(below, _)| below < y - 1.0e-6)
            .map(|&(below, _)| y - below);
    }

    // Sky and distance to walls, over every core.
    let reach = settings.sky_reach_m.max(settings.wall_reach_m);
    let tracer = Tracer::new(
        site,
        origin,
        [
            origin[0] + f64::from(columns) * cell,
            origin[1] + f64::from(rows) * cell,
        ],
        reach,
    );
    let directions = hemisphere(settings.sky_rays);
    let measured = spread(count, &|index| {
        let y = height[index]?;
        let (x, z) = centre(index);
        // In a crack or a pothole, from the pavement's surface: the mesh
        // under a tile does not shade it.
        let top = site
            .pavements()
            .iter()
            .find(|t| t.covers(x, z))
            .map_or(y, |t| y.max(t.top()));
        let foot = Vec3::new(x, top + 0.05, z);
        let sky = tracer.sky_seen(foot, Vec3::Y, &directions, settings.sky_reach_m);
        let (wall, clear) = tracer.nearest_upright(foot, settings.wall_reach_m);
        Some((sky, wall, clear))
    });
    let mut sky = vec![0.0; count];
    let mut wall_m = vec![settings.wall_reach_m; count];
    let mut clear_m = vec![settings.wall_reach_m; count];
    for (index, found) in measured.into_iter().enumerate() {
        if let Some((s, w, c)) = found {
            sky[index] = s;
            wall_m[index] = w;
            clear_m[index] = c;
        }
    }

    // Crack sites.
    let mut cracks = Vec::new();
    let mut cell_cracks: Vec<Vec<u32>> = vec![Vec::new(); count];
    for (t, tile) in site.pavements().iter().enumerate() {
        for opening in tile.openings(crate::succession::PATCH_M) {
            let index = cell_index(origin, cell, columns, rows, opening.x, opening.z);
            if let Some(index) = index {
                cell_cracks[index as usize].push(u32::try_from(cracks.len()).unwrap_or(u32::MAX));
            }
            cracks.push(CrackSite {
                tile: u32::try_from(t).unwrap_or(u32::MAX),
                x: opening.x,
                z: opening.z,
                y: opening.soil_top,
                soil_depth_m: opening.soil_depth,
                width_m: opening.width,
                sunk_m: (tile.top() - opening.soil_top).max(0.0),
                wear: tile.wear(opening.x, opening.z, 0.0),
                cell: index,
            });
        }
    }

    // Hosts and patches.
    let (hosts, patches) = surfaces(
        site, &tracer, origin, cell, columns, rows, &height, &terrain, settings,
    );

    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let ground = Ground {
        height: height.iter().map(|h| h.map(round)).collect(),
        slope_deg: slope_deg
            .into_iter()
            .map(|v| (v * 10.0).round() / 10.0)
            .collect(),
        aspect_deg: aspect_deg.into_iter().map(|v| v.round()).collect(),
        heat_load: heat_load.into_iter().map(round).collect(),
        material,
        rooting_m: rooting_m.iter().map(|h| h.map(round)).collect(),
        open_share: open_share.into_iter().map(round).collect(),
        wetness: wetness
            .into_iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect(),
        sky: sky.into_iter().map(round).collect(),
        wall_m: wall_m
            .into_iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect(),
        clear_m: clear_m
            .into_iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect(),
        under_roof,
        cracks: cell_cracks,
    };
    Ok(SiteField {
        version: SITE_FIELD_VERSION,
        settings: *settings,
        tile_low: low,
        tile_high: high,
        origin,
        columns,
        rows,
        ground,
        hosts,
        patches,
        cracks,
    })
}

/// McCune and Keon's heat load index (2002, equation 3), for a slope at
/// `latitude_deg` of `slope_deg` facing `aspect_deg`. The aspect is
/// folded about the north-east to south-west line, so a south-west slope
/// is warmest; south of the equator it is mirrored north for south.
#[must_use]
pub fn heat_load_index(latitude_deg: f64, slope_deg: f64, aspect_deg: f64) -> f64 {
    let aspect = if latitude_deg < 0.0 {
        (180.0 - aspect_deg).rem_euclid(360.0)
    } else {
        aspect_deg
    };
    let folded = (180.0 - (aspect - 225.0).abs()).abs().to_radians();
    let l = latitude_deg.abs().to_radians();
    let s = slope_deg.to_radians();
    (-1.467 + 1.582 * l.cos() * s.cos()
        - 1.5 * folded.cos() * s.sin() * l.sin()
        - 0.262 * l.sin() * s.sin()
        + 0.607 * folded.sin() * s.sin())
    .exp()
}

/// The deepest soil in a paved cell's openings and the share of it open,
/// sampled on the tile's own raster.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn paved_cell(tile: &Pavement, x: f64, z: f64, cell: f64) -> (f64, f64) {
    let step = tile.cell().max(cell / 40.0);
    let n = ((cell / step).round() as i64).max(1);
    let (mut deepest, mut open, mut total) = (0.0_f64, 0_u32, 0_u32);
    for k in 0..n {
        for i in 0..n {
            let px = x - 0.5 * cell + (i as f64 + 0.5) * step;
            let pz = z - 0.5 * cell + (k as f64 + 0.5) * step;
            if !tile.covers(px, pz) {
                continue;
            }
            total += 1;
            let soil: f64 = tile
                .column(px, pz)
                .unwrap_or_default()
                .iter()
                .filter(|(top, _, share)| *share >= 1.0 && *top >= tile.top() - 0.5)
                .map(|(top, bottom, _)| (top - bottom.max(top - 5.0)).max(0.0))
                .fold(0.0, f64::max);
            if soil > 0.0 {
                open += 1;
                deepest = deepest.max(soil);
            }
        }
    }
    (deepest, f64::from(open) / f64::from(total.max(1)))
}

/// `count` directions over the hemisphere round +Y, cosine-weighted
/// (more near the zenith, as light falls on a level surface), from a
/// fixed spiral so the same count gives the same rays.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn hemisphere(count: u32) -> Vec<Vec3> {
    let count = count.max(1);
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    (0..count)
        .map(|k| {
            let t = (f64::from(k) + 0.5) / f64::from(count);
            let r = t.sqrt();
            let angle = golden * f64::from(k);
            Vec3::new(r * angle.cos(), (1.0 - t).sqrt(), r * angle.sin())
        })
        .collect()
}

/// The site's triangles by column cell over the ground a field's rays and
/// wall searches reach, with each column's top, so that a ray passes over
/// the columns it clears without testing their triangles. Built once per
/// field and shared by every thread.
pub(crate) struct Tracer<'a> {
    site: &'a SiteMesh,
    /// The lowest column's index, as (x, z).
    low: [i64; 2],
    size: [usize; 2],
    /// Each column's triangles: `items[start[i]..start[i + 1]]`.
    start: Vec<u32>,
    items: Vec<u32>,
    /// The highest point of any triangle in each column.
    top: Vec<f64>,
    /// Each column's faces a wall search finds: upright faces that are
    /// not soil, thin parts, and faces kept clear.
    wall_start: Vec<u32>,
    walls: Vec<u32>,
    min_y: f64,
    max_y: f64,
}

impl<'a> Tracer<'a> {
    /// Columns from `low` to `high` (x, z), and `reach` round them.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn new(site: &'a SiteMesh, low: [f64; 2], high: [f64; 2], reach: f64) -> Self {
        let column = SiteMesh::COLUMN_M;
        let index = |v: f64| (v / column).floor() as i64;
        let first = [index(low[0] - reach) - 1, index(low[1] - reach) - 1];
        let last = [index(high[0] + reach) + 1, index(high[1] + reach) + 1];
        let size = [
            (last[0] - first[0] + 1).max(1) as usize,
            (last[1] - first[1] + 1).max(1) as usize,
        ];
        let count = size[0] * size[1];
        let (mut start, mut items, mut top) = (
            Vec::with_capacity(count + 1),
            Vec::new(),
            vec![f64::NEG_INFINITY; count],
        );
        let (mut wall_start, mut walls) = (Vec::with_capacity(count + 1), Vec::new());
        start.push(0);
        wall_start.push(0);
        for k in 0..size[1] {
            for i in 0..size[0] {
                let x = (first[0] + i as i64) as f64 * column + 0.5 * column;
                let z = (first[1] + k as i64) as f64 * column + 0.5 * column;
                for &triangle in site.column_triangles(x, z) {
                    items.push(triangle);
                    let [a, b, c] = site.corners(triangle as usize);
                    let at = k * size[0] + i;
                    top[at] = top[at].max(a.y).max(b.y).max(c.y);
                    let surface = site.surface(triangle as usize);
                    let upright =
                        site.normal(triangle as usize).y.abs() < UPRIGHT_Y || surface.thin();
                    if surface.material != SOIL && (upright || surface.clear) {
                        walls.push(triangle);
                    }
                }
                start.push(u32::try_from(items.len()).unwrap_or(u32::MAX));
                wall_start.push(u32::try_from(walls.len()).unwrap_or(u32::MAX));
            }
        }
        let (bottom, summit) = site.bounds();
        Self {
            site,
            low: first,
            size,
            start,
            items,
            top,
            wall_start,
            walls,
            min_y: bottom.y,
            max_y: summit.y,
        }
    }

    /// A column's slot in the grid, if it lies in it.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn slot(&self, column: [i64; 2]) -> Option<usize> {
        let (i, k) = (column[0] - self.low[0], column[1] - self.low[1]);
        (i >= 0 && k >= 0 && (i as usize) < self.size[0] && (k as usize) < self.size[1])
            .then(|| k as usize * self.size[0] + i as usize)
    }

    /// A column's triangles and top. Outside the grid, the site's own
    /// index, with no top to pass over.
    #[allow(clippy::cast_precision_loss)]
    fn column(&self, column: [i64; 2]) -> (&[u32], f64) {
        match self.slot(column) {
            Some(at) => (
                &self.items[self.start[at] as usize..self.start[at + 1] as usize],
                self.top[at],
            ),
            None => {
                let size = SiteMesh::COLUMN_M;
                (
                    self.site.column_triangles(
                        (column[0] as f64 + 0.5) * size,
                        (column[1] as f64 + 0.5) * size,
                    ),
                    f64::INFINITY,
                )
            }
        }
    }

    /// A column's faces for a wall search.
    #[allow(clippy::cast_precision_loss)]
    fn wall_faces(&self, column: [i64; 2]) -> std::borrow::Cow<'_, [u32]> {
        match self.slot(column) {
            Some(at) => std::borrow::Cow::Borrowed(
                &self.walls[self.wall_start[at] as usize..self.wall_start[at + 1] as usize],
            ),
            None => {
                let size = SiteMesh::COLUMN_M;
                self.site
                    .column_triangles(
                        (column[0] as f64 + 0.5) * size,
                        (column[1] as f64 + 0.5) * size,
                    )
                    .iter()
                    .copied()
                    .filter(|&t| {
                        let surface = self.site.surface(t as usize);
                        let upright =
                            self.site.normal(t as usize).y.abs() < UPRIGHT_Y || surface.thin();
                        surface.material != SOIL && (upright || surface.clear)
                    })
                    .collect::<Vec<u32>>()
                    .into()
            }
        }
    }

    /// Whether the ray from `from` along unit `ray` meets a triangle of
    /// the site within `reach`. It walks the column cells the ray crosses
    /// in order (Amanatides and Woo 1987), each once, and tests a column's
    /// triangles only where the ray is not yet above the column's top.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn blocked(&self, from: Vec3, ray: Vec3, reach: f64) -> bool {
        let size = SiteMesh::COLUMN_M;
        let mut column = [
            (from.x / size).floor() as i64,
            (from.z / size).floor() as i64,
        ];
        // Distance along the ray to the next column edge, and between edges.
        #[allow(clippy::cast_precision_loss)]
        let edge = |p: f64, d: f64, at: i64| -> (f64, f64, i64) {
            if d > 1.0e-12 {
                (((at + 1) as f64 * size - p) / d, size / d, 1)
            } else if d < -1.0e-12 {
                ((at as f64 * size - p) / d, -size / d, -1)
            } else {
                (f64::INFINITY, f64::INFINITY, 0)
            }
        };
        let (mut next_x, step_x, dx) = edge(from.x, ray.x, column[0]);
        let (mut next_z, step_z, dz) = edge(from.z, ray.z, column[1]);
        let mut enter = 0.0_f64;
        loop {
            let leave = next_x.min(next_z).min(reach);
            let (y_enter, y_leave) = (from.y + ray.y * enter, from.y + ray.y * leave);
            if (ray.y > 0.0 && y_enter > self.max_y) || (ray.y < 0.0 && y_enter < self.min_y) {
                return false;
            }
            let (triangles, top) = self.column(column);
            if y_enter.min(y_leave) <= top + 1.0e-9 {
                for &index in triangles {
                    let [a, b, c] = self.site.corners(index as usize);
                    if let Some(hit) = ray_triangle(from, ray, a, b, c)
                        && hit > 1.0e-4
                        && hit <= reach
                    {
                        return true;
                    }
                }
            }
            if leave >= reach {
                return false;
            }
            enter = leave;
            if next_x < next_z {
                column[0] += dx;
                next_x += step_x;
            } else {
                column[1] += dz;
                next_z += step_z;
            }
        }
    }

    /// Share of `directions` (about +Y) turned to `normal` that leave
    /// `from` without meeting the site within `reach`.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn sky_seen(&self, from: Vec3, normal: Vec3, directions: &[Vec3], reach: f64) -> f64 {
        let (tangent, bitangent) = frame(normal);
        let open = directions
            .iter()
            .filter(|d| {
                let ray = (tangent * d.x + normal * d.y + bitangent * d.z).normalize_or(normal);
                // A ray into the ground below the horizon is not sky.
                ray.y > -0.02 && !self.blocked(from, ray, reach)
            })
            .count();
        open as f64 / directions.len().max(1) as f64
    }

    /// Distance from `foot` to the nearest upright face that is not soil,
    /// and to the nearest face kept clear, each at most `reach`. It
    /// searches the columns round `foot` ring by ring and stops once a
    /// ring lies farther than both.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn nearest_upright(&self, foot: Vec3, reach: f64) -> (f64, f64) {
        let size = SiteMesh::COLUMN_M;
        let n = (reach / size).ceil() as i64;
        let centre = [
            (foot.x / size).floor() as i64,
            (foot.z / size).floor() as i64,
        ];
        let (mut wall, mut clear) = (reach, reach);
        for ring in 0..=n {
            if ring >= 1 && (ring - 1) as f64 * size >= wall.max(clear) {
                break;
            }
            for k in -ring..=ring {
                for i in -ring..=ring {
                    if i.abs().max(k.abs()) != ring {
                        continue;
                    }
                    for &index in self.wall_faces([centre[0] + i, centre[1] + k]).iter() {
                        let surface = self.site.surface(index as usize);
                        let upright =
                            self.site.normal(index as usize).y.abs() < UPRIGHT_Y || surface.thin();
                        let [a, b, c] = self.site.corners(index as usize);
                        let d = (closest_on_triangle(foot, a, b, c) - foot).length();
                        if upright {
                            wall = wall.min(d);
                        }
                        if surface.clear {
                            clear = clear.min(d);
                        }
                    }
                }
            }
        }
        (wall, clear)
    }
}

/// `work` on every index below `count`, over the machine's idle cores,
/// in the indices' order: no result depends on how many threads ran.
pub(crate) fn spread<R: Send>(count: usize, work: &(dyn Fn(usize) -> R + Sync)) -> Vec<R> {
    const CHUNK: usize = 256;
    let chunks: Vec<usize> = (0..count.div_ceil(CHUNK)).collect();
    let lease = crate::cores::Lease::take(crate::cores::available());
    crate::cores::map(chunks, lease.threads(), &|chunk: usize| {
        (chunk * CHUNK..((chunk + 1) * CHUNK).min(count))
            .map(work)
            .collect::<Vec<R>>()
    })
    .into_iter()
    .flatten()
    .collect()
}

pub(crate) fn frame(normal: Vec3) -> (Vec3, Vec3) {
    let helper = if normal.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent = helper.cross(normal).normalize_or(Vec3::X);
    (tangent, normal.cross(tangent))
}

/// Möller and Trumbore's ray and triangle test: the distance along the
/// ray to the hit, when there is one.
fn ray_triangle(from: Vec3, ray: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = ray.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1.0e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = from - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

/// A triangle that is not ground: not soil, and not the upward face a
/// ground cell stands on.
fn is_host_face(site: &SiteMesh, index: usize, ground: &dyn Fn(f64, f64) -> Option<f64>) -> bool {
    let surface = site.surface(index);
    if surface.material == SOIL {
        return false;
    }
    let [a, b, c] = site.corners(index);
    let centre = (a + b + c) * (1.0 / 3.0);
    // Under a paved tile's surface the tile stands in for the mesh.
    if site
        .pavements()
        .iter()
        .any(|t| t.covers(centre.x, centre.z) && centre.y <= t.top() + 0.02)
    {
        return false;
    }
    let normal = site.normal(index);
    // A face pressed against another solid (a wall's top under its roof,
    // a floor on the ground) is inside the site, not a surface.
    if normal.y.abs() > UPRIGHT_Y && !surface.thin() {
        let pressed = site
            .crossings(centre.x, centre.z)
            .iter()
            .any(|&(y, other)| {
                other != index
                    && (y - centre.y).abs() < 0.02
                    && site.normal(other).dot(normal) < 0.0
            });
        if pressed {
            return false;
        }
    }
    if normal.y > UPRIGHT_Y && !surface.thin() {
        // An upward face the ground grid already stands on (bare rock
        // terrain, a slab) is ground, not a host.
        if let Some(y) = ground(centre.x, centre.z)
            && (y - centre.y).abs() < 0.05
            && surface.material != BUILT
        {
            return false;
        }
    }
    true
}

/// Splits the site's non-ground faces into hosts and samples each into
/// patches.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_arguments
)]
fn surfaces(
    site: &SiteMesh,
    tracer: &Tracer<'_>,
    origin: [f64; 2],
    cell: f64,
    columns: u32,
    rows: u32,
    height: &[Option<f64>],
    terrain: &[Option<f64>],
    settings: &FieldSettings,
) -> (Vec<Host>, Vec<Patch>) {
    let ground = |x: f64, z: f64| -> Option<f64> {
        cell_index(origin, cell, columns, rows, x, z).and_then(|i| terrain[i as usize])
    };
    let total = site.triangle_count();
    let faces: Vec<usize> = (0..total)
        .filter(|&i| is_host_face(site, i, &ground))
        .collect();
    let mut is_face = vec![false; total];
    for &i in &faces {
        is_face[i] = true;
    }
    // Faces by edge, to join neighbours.
    let mut by_edge: std::collections::HashMap<(u32, u32), Vec<usize>> =
        std::collections::HashMap::new();
    for &i in &faces {
        let corners = site.triangle_corners(i);
        for k in 0..3 {
            let (p, q) = (corners[k], corners[(k + 1) % 3]);
            by_edge.entry((p.min(q), p.max(q))).or_default().push(i);
        }
    }
    let neighbours = |i: usize| -> Vec<usize> {
        let corners = site.triangle_corners(i);
        let mut out = Vec::new();
        for k in 0..3 {
            let (p, q) = (corners[k], corners[(k + 1) % 3]);
            for &j in by_edge.get(&(p.min(q), p.max(q))).into_iter().flatten() {
                if j != i {
                    out.push(j);
                }
            }
        }
        out
    };
    // Parts: faces joined by edges, of one solid-or-thin kind.
    let mut part = vec![usize::MAX; total];
    let mut parts: Vec<Vec<usize>> = Vec::new();
    for &start in &faces {
        if part[start] != usize::MAX {
            continue;
        }
        let id = parts.len();
        let thin = site.surface(start).thin();
        let mut members = vec![start];
        part[start] = id;
        let mut k = 0;
        while k < members.len() {
            for j in neighbours(members[k]) {
                if part[j] == usize::MAX && site.surface(j).thin() == thin {
                    part[j] = id;
                    members.push(j);
                }
            }
            k += 1;
        }
        members.sort_unstable();
        parts.push(members);
    }

    let parts = join_near(site, parts);
    let directions = hemisphere(settings.sky_rays.max(8) / 2);
    // Each part's hosts and patches, numbered from 0 within the part, on
    // every core; then numbered on in the parts' order.
    let built = spread(parts.len(), &|index| {
        let members = &parts[index];
        let mut hosts: Vec<Host> = Vec::new();
        let mut patches: Vec<Patch> = Vec::new();
        let (low, high) = members.iter().fold(
            (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(lo, hi), &i| {
                let [a, b, c] = site.corners(i);
                (lo.min(a).min(b).min(c), hi.max(a).max(b).max(c))
            },
        );
        let extent = high - low;
        let material = commonest_material(site, members);
        let thin = site.surface(members[0]).thin();
        let wide = extent.x.max(extent.z);
        let narrow = extent.x.min(extent.z);
        let kept_clear = members.iter().all(|&i| site.surface(i).clear);
        let all_thin = members.iter().all(|&i| site.surface(i).thin());
        let fence_like = matches!(material, WOOD | BARK) || thin;
        // Standing on the ground, not strung or set high (a wire between
        // poles).
        let middle = (low + high) * 0.5;
        let on_ground = ground(middle.x, middle.z).is_none_or(|y| low.y <= y + 0.5);
        let kind = if kept_clear {
            // A window or a door: part of its wall, its patches clear.
            HostKind::Wall
        } else if fence_like
            && on_ground
            && extent.y > 0.3
            && extent.y <= FENCE_TALL_M + 0.5
            && narrow <= FENCE_THICK_M
            && wide > extent.y
        {
            HostKind::Fence
        } else if !all_thin && wide <= POLE_WIDE_M && extent.y >= (3.0 * wide).max(1.0) {
            HostKind::Pole
        } else if all_thin {
            HostKind::Thin
        } else if material == ROCK {
            HostKind::Boulder
        } else if matches!(material, WOOD | BARK) && extent.y < wide * 0.5 {
            HostKind::Log
        } else {
            // A building: one host per face, split where it turns.
            HostKind::Wall
        };
        match kind {
            HostKind::Wall => {
                for face in split_faces(site, members, &neighbours, &is_face) {
                    let normal = area_normal(site, &face);
                    let kind = if normal.y > UPRIGHT_Y {
                        HostKind::Roof
                    } else if normal.y < -UPRIGHT_Y {
                        HostKind::Overhang
                    } else {
                        HostKind::Wall
                    };
                    planar_host(
                        site,
                        &face,
                        kind,
                        normal,
                        &mut hosts,
                        &mut patches,
                        settings,
                        &ground,
                    );
                }
            }
            HostKind::Fence | HostKind::Thin | HostKind::Log => {
                // One host along its whole length, then split in bays by
                // u, so u runs on unbroken from bay to bay.
                let along = if extent.x >= extent.z {
                    Vec3::X
                } else {
                    Vec3::Z
                };
                let normal = area_normal_abs(site, members);
                let first = patches.len();
                planar_host_along(
                    site,
                    members,
                    kind,
                    normal,
                    along,
                    &mut hosts,
                    &mut patches,
                    settings,
                    &ground,
                );
                if kind != HostKind::Log {
                    split_bays(&mut hosts, &mut patches, first, settings);
                }
            }
            _ => {
                round_host(
                    site,
                    members,
                    kind,
                    low,
                    high,
                    &mut hosts,
                    &mut patches,
                    settings,
                    &ground,
                );
            }
        }
        (hosts, patches)
    });
    let mut hosts: Vec<Host> = Vec::new();
    let mut patches: Vec<Patch> = Vec::new();
    for (part_hosts, part_patches) in built {
        let offset = u32::try_from(hosts.len()).unwrap_or(u32::MAX);
        hosts.extend(part_hosts.into_iter().map(|host| Host {
            id: host.id + offset,
            ..host
        }));
        patches.extend(part_patches.into_iter().map(|patch| Patch {
            host: patch.host + offset,
            ..patch
        }));
    }
    // Hosts left with no patch (all below ground, or hidden) go, and the
    // rest are numbered again in order.
    let mut used = vec![false; hosts.len()];
    for patch in &patches {
        used[patch.host as usize] = true;
    }
    let mut renumber = vec![u32::MAX; hosts.len()];
    let mut kept = Vec::new();
    for (old, host) in hosts.into_iter().enumerate() {
        if used[old] {
            renumber[old] = u32::try_from(kept.len()).unwrap_or(u32::MAX);
            kept.push(Host {
                id: renumber[old],
                ..host
            });
        }
    }
    let hosts = kept;
    for patch in &mut patches {
        patch.host = renumber[patch.host as usize];
    }
    // Sky, height and root cell of each patch.
    let skies = spread(patches.len(), &|index| {
        let patch = &patches[index];
        let c = Vec3::new(patch.centre[0], patch.centre[1], patch.centre[2]);
        let n = Vec3::new(patch.normal[0], patch.normal[1], patch.normal[2]);
        tracer.sky_seen(c + n * 0.05, n, &directions, settings.sky_reach_m)
    });
    for (patch, sky) in patches.iter_mut().zip(skies) {
        let c = Vec3::new(patch.centre[0], patch.centre[1], patch.centre[2]);
        let n = Vec3::new(patch.normal[0], patch.normal[1], patch.normal[2]);
        patch.sky = (sky * 1000.0).round() / 1000.0;
        let under = site.footing(c.x, c.z, Some(c.y + 1.0e-3)).map(|(y, _)| y);
        patch.height_m = under.map_or(0.0, |y| ((c.y - y).max(0.0) * 1000.0).round() / 1000.0);
        // A step out from the face, at the ground.
        let flat = Vec3::new(n.x, 0.0, n.z).normalize_or(Vec3::ZERO);
        let out = c + flat * (0.5 * cell);
        patch.root_cell = cell_index(origin, cell, columns, rows, out.x, out.z)
            .filter(|i| height[*i as usize].is_some());
    }
    (hosts, patches)
}

/// Parts of wood or bark, and thin parts, that stand within [`JOIN_M`] of
/// each other (a fence's pickets, rails and posts, its wires) are one part, so a fence is one host
/// split in bays, not a host per picket.
fn join_near(site: &SiteMesh, parts: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    // Wood and bark, and thin parts (wires, mesh) not kept clear.
    let woody = |members: &[usize]| {
        members.iter().all(|&i| {
            let surface = site.surface(i);
            !surface.clear && (matches!(surface.material, WOOD | BARK) || surface.thin())
        })
    };
    let boxes: Vec<(Vec3, Vec3)> = parts
        .iter()
        .map(|members| {
            members.iter().fold(
                (
                    Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                    Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                ),
                |(lo, hi), &i| {
                    let [a, b, c] = site.corners(i);
                    (lo.min(a).min(b).min(c), hi.max(a).max(b).max(c))
                },
            )
        })
        .collect();
    let mut group: Vec<usize> = (0..parts.len()).collect();
    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    let near = |a: &(Vec3, Vec3), b: &(Vec3, Vec3)| {
        a.0.x - JOIN_M <= b.1.x
            && b.0.x - JOIN_M <= a.1.x
            && a.0.y - JOIN_M <= b.1.y
            && b.0.y - JOIN_M <= a.1.y
            && a.0.z - JOIN_M <= b.1.z
            && b.0.z - JOIN_M <= a.1.z
    };
    // Only low parts: a fence's, not a utility pole with its wires.
    let wood: Vec<bool> = parts
        .iter()
        .zip(&boxes)
        .map(|(m, (lo, hi))| woody(m) && hi.y - lo.y <= FENCE_TALL_M + 0.5)
        .collect();
    for i in 0..parts.len() {
        if !wood[i] {
            continue;
        }
        for j in (i + 1)..parts.len() {
            if wood[j] && near(&boxes[i], &boxes[j]) {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                if a != b {
                    group[b.max(a)] = a.min(b);
                }
            }
        }
    }
    let mut joined: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, members) in parts.into_iter().enumerate() {
        let r = root(&mut group, i);
        joined.entry(r).or_default().extend(members);
    }
    joined
        .into_values()
        .map(|mut m| {
            m.sort_unstable();
            m
        })
        .collect()
}

/// How close two wooden parts stand to count as one, metres.
const JOIN_M: f64 = 0.12;

/// Splits the host whose patches start at `first` (the last host made)
/// into one host per bay of `fence_bay_m` along u.
#[allow(clippy::cast_possible_truncation)]
fn split_bays(
    hosts: &mut Vec<Host>,
    patches: &mut [Patch],
    first: usize,
    settings: &FieldSettings,
) {
    let Some(whole) = hosts.pop() else {
        return;
    };
    let half = 0.5 * settings.patch_m;
    let mut bays: std::collections::BTreeMap<i64, u32> = std::collections::BTreeMap::new();
    for patch in patches[first..].iter_mut() {
        let bay = (patch.u / settings.fence_bay_m).floor() as i64;
        let id = *bays.entry(bay).or_insert_with(|| {
            let id = u32::try_from(hosts.len()).unwrap_or(u32::MAX);
            hosts.push(Host {
                id,
                low: [f64::INFINITY; 3],
                high: [f64::NEG_INFINITY; 3],
                ..whole.clone()
            });
            id
        });
        patch.host = id;
        let host = &mut hosts[id as usize];
        for k in 0..3 {
            host.low[k] = host.low[k].min(round3(patch.centre[k] - half));
            host.high[k] = host.high[k].max(round3(patch.centre[k] + half));
        }
        host.foot_m = host.foot_m.min(whole.foot_m);
    }
    for host in hosts.iter_mut().skip(whole.id as usize) {
        host.low[1] = host.low[1].max(whole.low[1]);
        host.high[1] = host.high[1].min(whole.high[1]);
    }
}

fn commonest_material(site: &SiteMesh, members: &[usize]) -> u8 {
    let mut areas = [0.0_f64; 6];
    for &i in members {
        let [a, b, c] = site.corners(i);
        let m = site.surface(i).material as usize;
        if m < areas.len() {
            areas[m] += 0.5 * (b - a).cross(c - a).length();
        }
    }
    let mut best = 0;
    for (m, area) in areas.iter().enumerate() {
        if *area > areas[best] {
            best = m;
        }
    }
    u8::try_from(best).unwrap_or(BUILT)
}

fn area_normal(site: &SiteMesh, members: &[usize]) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for &i in members {
        let [a, b, c] = site.corners(i);
        sum += (b - a).cross(c - a);
    }
    sum.normalize_or(Vec3::Y)
}

/// A thin part's normal, its faces' turned one way (a thin face may be
/// wound either way).
fn area_normal_abs(site: &SiteMesh, members: &[usize]) -> Vec3 {
    let first = site.normal(members[0]);
    let mut sum = Vec3::ZERO;
    for &i in members {
        let [a, b, c] = site.corners(i);
        let n = (b - a).cross(c - a);
        sum += if n.dot(first) < 0.0 { n * -1.0 } else { n };
    }
    sum.normalize_or(Vec3::Y)
}

/// A building's faces split where neighbouring faces turn more than
/// [`FACE_TURN_COS`] from the face they grow from.
fn split_faces(
    site: &SiteMesh,
    members: &[usize],
    neighbours: &dyn Fn(usize) -> Vec<usize>,
    is_face: &[bool],
) -> Vec<Vec<usize>> {
    let mut taken = std::collections::HashSet::new();
    let mut out = Vec::new();
    for &start in members {
        if !taken.insert(start) {
            continue;
        }
        let seed = site.normal(start);
        let mut face = vec![start];
        let mut k = 0;
        while k < face.len() {
            for j in neighbours(face[k]) {
                if is_face[j] && site.normal(j).dot(seed) >= FACE_TURN_COS && taken.insert(j) {
                    face.push(j);
                }
            }
            k += 1;
        }
        face.sort_unstable();
        out.push(face);
    }
    out
}

/// A flat host's patches: `u` along its horizontal direction, `v` up its
/// slope from its foot.
#[allow(clippy::too_many_arguments)]
fn planar_host(
    site: &SiteMesh,
    members: &[usize],
    kind: HostKind,
    normal: Vec3,
    hosts: &mut Vec<Host>,
    patches: &mut Vec<Patch>,
    settings: &FieldSettings,
    ground: &dyn Fn(f64, f64) -> Option<f64>,
) {
    let along = Vec3::Y.cross(normal);
    let along = if along.length() < 1.0e-6 {
        Vec3::X
    } else {
        along.normalize_or(Vec3::X)
    };
    planar_host_along(
        site, members, kind, normal, along, hosts, patches, settings, ground,
    );
}

#[allow(clippy::too_many_arguments)]
fn planar_host_along(
    site: &SiteMesh,
    members: &[usize],
    kind: HostKind,
    normal: Vec3,
    along: Vec3,
    hosts: &mut Vec<Host>,
    patches: &mut Vec<Patch>,
    settings: &FieldSettings,
    ground: &dyn Fn(f64, f64) -> Option<f64>,
) {
    // Up the face: world up less its part along the normal; on a level
    // face, across it.
    let up = Vec3::Y - normal * normal.y;
    let up = if up.length() < 1.0e-3 {
        normal.cross(along).normalize_or(Vec3::Z)
    } else {
        up.normalize_or(Vec3::Y)
    };
    let mut low = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut high = low * -1.0;
    for &i in members {
        for p in site.corners(i) {
            low = low.min(p);
            high = high.max(p);
        }
    }
    let foot = low.y;
    let base_point = Vec3::new(0.0, foot, 0.0);
    let u0 = members
        .iter()
        .flat_map(|&i| site.corners(i))
        .map(|p| p.dot(along))
        .fold(f64::INFINITY, f64::min);
    let v0 = members
        .iter()
        .flat_map(|&i| site.corners(i))
        .map(|p| (p - base_point).dot(up))
        .fold(f64::INFINITY, f64::min);
    let id = u32::try_from(hosts.len()).unwrap_or(u32::MAX);
    hosts.push(Host {
        id,
        kind,
        material: commonest_material(site, members),
        foot_m: round3(foot),
        low: [round3(low.x), round3(low.y), round3(low.z)],
        high: [round3(high.x), round3(high.y), round3(high.z)],
        u_axis: Some([round3(along.x), round3(along.y), round3(along.z)]),
        axis: None,
    });
    let coords = |p: Vec3| (p.dot(along) - u0, (p - base_point).dot(up) - v0);
    bin_patches(
        site,
        members,
        id,
        &coords,
        settings.patch_m,
        Some(normal),
        patches,
        settings,
        ground,
    );
}

/// A pole's or a boulder's patches: `u` the arc round its upright axis
/// from north, `v` up from its foot.
#[allow(clippy::too_many_arguments)]
fn round_host(
    site: &SiteMesh,
    members: &[usize],
    kind: HostKind,
    low: Vec3,
    high: Vec3,
    hosts: &mut Vec<Host>,
    patches: &mut Vec<Patch>,
    settings: &FieldSettings,
    ground: &dyn Fn(f64, f64) -> Option<f64>,
) {
    let axis = [0.5 * (low.x + high.x), 0.5 * (low.z + high.z)];
    let radius = (0.25 * ((high.x - low.x) + (high.z - low.z))).max(0.01);
    let id = u32::try_from(hosts.len()).unwrap_or(u32::MAX);
    hosts.push(Host {
        id,
        kind,
        material: commonest_material(site, members),
        foot_m: round3(low.y),
        low: [round3(low.x), round3(low.y), round3(low.z)],
        high: [round3(high.x), round3(high.y), round3(high.z)],
        u_axis: None,
        axis: Some([round3(axis[0]), round3(axis[1])]),
    });
    let coords = |p: Vec3| {
        // Bearing from north (-Z), clockwise to east (+X).
        let bearing = (p.x - axis[0])
            .atan2(-(p.z - axis[1]))
            .rem_euclid(std::f64::consts::TAU);
        (bearing * radius, p.y - low.y)
    };
    // Round a pole at least four patches, so each faces one way.
    let around = std::f64::consts::TAU * radius;
    let u_size = settings.patch_m.min(around / 4.0);
    bin_patches(
        site, members, id, &coords, u_size, None, patches, settings, ground,
    );
}

/// Which of eight compass sectors a normal faces, or 8 for up and 9 for
/// down.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn facing(normal: Vec3) -> u8 {
    if normal.y > 0.7 {
        return 8;
    }
    if normal.y < -0.7 {
        return 9;
    }
    let bearing = normal.x.atan2(-normal.z).rem_euclid(std::f64::consts::TAU);
    ((bearing / (std::f64::consts::TAU / 8.0) + 0.5).floor() as u8) % 8
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Samples a host's faces into square bins of `patch_m` in (u, v): each
/// face is cut into small pieces whose areas, centres and normals go to
/// the bin their centre falls in.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn bin_patches(
    site: &SiteMesh,
    members: &[usize],
    host: u32,
    coords: &dyn Fn(Vec3) -> (f64, f64),
    u_size: f64,
    side_of: Option<Vec3>,
    patches: &mut Vec<Patch>,
    settings: &FieldSettings,
    ground: &dyn Fn(f64, f64) -> Option<f64>,
) {
    #[derive(Default)]
    struct Bin {
        area: f64,
        centre: Vec3,
        normal: Vec3,
        materials: [f64; 6],
        clear: f64,
        climb: f64,
        thin: f64,
        soil: f64,
    }
    let size = settings.patch_m;
    let mut bins: std::collections::BTreeMap<(i64, i64, u8), Bin> =
        std::collections::BTreeMap::new();
    let first = site.normal(members[0]);
    for &i in members {
        let [a, b, c] = site.corners(i);
        let surface = site.surface(i);
        let mut normal = site.normal(i);
        if surface.thin() && normal.dot(first) < 0.0 {
            normal = normal * -1.0;
        }
        let longest = (b - a).length().max((c - b).length()).max((a - c).length());
        let n = ((longest / (0.25 * size)).ceil() as usize).clamp(1, 400);
        let whole = 0.5 * (b - a).cross(c - a).length();
        let piece = whole / (n * n) as f64;
        // Centres of the n² sub-triangles of a regular split.
        for row in 0..n {
            for col in 0..(n - row) {
                let mut add = |s: f64, t: f64| {
                    let p = a + (b - a) * (s / n as f64) + (c - a) * (t / n as f64);
                    // Below the ground (a wall's footing, a boulder's
                    // buried part) is no surface.
                    if ground(p.x, p.z).is_some_and(|y| p.y < y - 0.02) {
                        return;
                    }
                    let (u, v) = coords(p);
                    // The two sides of a board or a fence are patches of
                    // their own; round a pole or a boulder, so are faces
                    // turned different ways.
                    let side = match side_of {
                        Some(n) => u8::from(normal.dot(n) < 0.0),
                        None => facing(normal),
                    };
                    let key = ((u / u_size).floor() as i64, (v / size).floor() as i64, side);
                    let bin = bins.entry(key).or_default();
                    bin.area += piece;
                    bin.centre += p * piece;
                    bin.normal += normal * piece;
                    bin.materials[(surface.material as usize).min(5)] += piece;
                    if surface.clear {
                        bin.clear += piece;
                    }
                    if surface.climb && climbable(surface.material) && !surface.clear {
                        bin.climb += piece;
                    }
                    bin.thin = bin.thin.max(surface.thin_m);
                    if normal.y > UPRIGHT_Y {
                        bin.soil = bin.soil.max(surface.soil_m);
                    }
                };
                let (r, c) = (row as f64, col as f64);
                add(c + 1.0 / 3.0, r + 1.0 / 3.0);
                if col + row + 1 < n {
                    add(c + 2.0 / 3.0, r + 2.0 / 3.0);
                }
            }
        }
    }
    for ((iu, iv, _), bin) in bins {
        if bin.area < 1.0e-6 {
            continue;
        }
        let centre = bin.centre * (1.0 / bin.area);
        let normal = bin.normal.normalize_or(first);
        let mut material = 0;
        for (m, area) in bin.materials.iter().enumerate() {
            if *area > bin.materials[material] {
                material = m;
            }
        }
        patches.push(Patch {
            host,
            u: round3(iu as f64 * u_size),
            v: round3(iv as f64 * size),
            centre: [round3(centre.x), round3(centre.y), round3(centre.z)],
            normal: [round3(normal.x), round3(normal.y), round3(normal.z)],
            area: round3(bin.area),
            material: u8::try_from(material).unwrap_or(BUILT),
            height_m: 0.0,
            sky: 0.0,
            root_cell: None,
            clear: bin.clear > 0.5 * bin.area,
            climbable: bin.climb > 0.5 * bin.area,
            thin_m: round3(bin.thin),
            soil_m: round3(bin.soil),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::Surface;

    /// Scattered triangles of every size and lean, some long and thin
    /// across many columns (wires, rails), some upright, some kept clear.
    fn scatter() -> SiteMesh {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            #[allow(clippy::cast_precision_loss)]
            let unit = (seed >> 11) as f64 / (1_u64 << 53) as f64;
            unit
        };
        let (mut positions, mut triangles, mut surfaces) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..300_u32 {
            let base = Vec3::new(next() * 30.0 - 15.0, next() * 6.0, next() * 30.0 - 15.0);
            let long = if k % 7 == 0 { 12.0 } else { 2.0 };
            let a = base;
            let b = base
                + Vec3::new(
                    next() * long - long / 2.0,
                    next() * 3.0 - 1.5,
                    next() * long - long / 2.0,
                );
            let c = base + Vec3::new(next() * 2.0 - 1.0, next() * 3.0 - 1.5, next() * 2.0 - 1.0);
            positions.extend([a, b, c]);
            triangles.push([3 * k, 3 * k + 1, 3 * k + 2]);
            let mut surface = Surface::solid(if k % 3 == 0 { SOIL } else { BUILT });
            surface.clear = k % 11 == 0;
            surfaces.push(surface);
        }
        SiteMesh::with_surfaces(&positions, &triangles, &surfaces).unwrap()
    }

    #[test]
    fn the_tracer_finds_what_testing_every_triangle_finds() {
        let site = scatter();
        let tracer = Tracer::new(&site, [-8.0, -8.0], [8.0, 8.0], 12.0);
        let directions = hemisphere(64);
        let everything = |from: Vec3, ray: Vec3, reach: f64| {
            (0..site.triangle_count()).any(|i| {
                let [a, b, c] = site.corners(i);
                ray_triangle(from, ray, a, b, c).is_some_and(|hit| hit > 1.0e-4 && hit <= reach)
            })
        };
        let mut blocked = 0;
        for (k, from) in [
            Vec3::new(0.3, 1.0, 0.7),
            Vec3::new(-7.5, 0.2, 6.1),
            Vec3::new(5.0, 3.0, -5.0),
            Vec3::new(0.0, 0.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            for (d, direction) in directions.iter().enumerate() {
                // Rays up, level and a little down, and some along an axis.
                let ray = match (k + d) % 4 {
                    0 => *direction,
                    1 => Vec3::new(direction.x, -0.01, direction.z).normalize_or(Vec3::X),
                    2 => Vec3::new(direction.x, 0.0, 0.0).normalize_or(Vec3::X),
                    _ => Vec3::new(direction.x, 0.05, direction.z).normalize_or(Vec3::X),
                };
                let found = tracer.blocked(from, ray, 12.0);
                assert_eq!(
                    found,
                    everything(from, ray, 12.0),
                    "from {from:?} along {ray:?}"
                );
                blocked += usize::from(found);
            }
        }
        // Both answers are tested.
        assert!(blocked > 20 && blocked < 4 * 64 - 20, "{blocked} blocked");
    }

    #[test]
    fn the_ring_search_finds_the_nearest_wall_and_window() {
        let site = scatter();
        let tracer = Tracer::new(&site, [-8.0, -8.0], [8.0, 8.0], 10.0);
        for foot in [
            Vec3::new(0.3, 1.0, 0.7),
            Vec3::new(-7.5, 0.2, 6.1),
            Vec3::new(5.0, 3.0, -5.0),
        ] {
            let (mut wall, mut clear) = (10.0_f64, 10.0_f64);
            for i in 0..site.triangle_count() {
                let surface = site.surface(i);
                let upright = site.normal(i).y.abs() < UPRIGHT_Y || surface.thin();
                if surface.material == SOIL {
                    continue;
                }
                let [a, b, c] = site.corners(i);
                let d = (closest_on_triangle(foot, a, b, c) - foot).length();
                if upright {
                    wall = wall.min(d);
                }
                if surface.clear {
                    clear = clear.min(d);
                }
            }
            assert_eq!(
                tracer.nearest_upright(foot, 10.0),
                (wall, clear),
                "at {foot:?}"
            );
        }
    }
}
