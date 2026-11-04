//! Fill city tiles with plant communities, fast: the external contract.
//!
//! A fill never grows a plant per place. [`Fill::new`] picks a few hundred
//! *prototypes* once per set of communities (a species, a seed, a setting
//! and an age class each, every one exported with its tiers), and
//! [`Fill::tile`] lays out a tile as packed [`Instance`]s of them, plus a
//! ground cover raster and a far canopy raster. Any engine draws that with
//! GPU instancing.
//!
//! A tile's plan is a pure function of the fill's communities, the
//! [`Site`], the [`StandMap`], the seed and
//! [`GENERATOR_REVISION`](crate::GENERATOR_REVISION): the same on every
//! machine, thread count and order. Every draw is keyed by a stand cell's
//! place in the world, never by its place in the tile, so tiles laid side
//! by side meet without a seam and a caller can fill any tile alone.
//!
//! The stands thread owns what is computed (density, layers, closure,
//! allometry); this module holds the types and file forms callers build
//! on. Until then [`Fill::tile`] scatters each community's members at a
//! fixed density by layer: right in kind, not yet in number.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::{Error, Generator, Options, PartRecord, Plant, Settings, VariantRecord};
use crate::niche::SiteFactors;
use crate::rng;
use crate::site_field::SiteField;
use crate::spec::Environment;

/// The version of the instance file ([`Instance::to_bytes`]) and of the
/// tile JSON beside it.
pub const INSTANCE_SCHEMA: u32 = 1;

/// Bytes per packed instance.
pub const INSTANCE_BYTES: usize = 32;

/// The stand cell side when a [`StandMap`] does not choose one, metres.
pub const STAND_CELL_M: f64 = 40.0;

/// Where a tile sits in the world grid of tiles: east and north.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileId {
    pub x: i32,
    pub y: i32,
}

/// The layer of a stand a member fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Canopy,
    Subcanopy,
    Shrub,
    Herb,
    Ground,
    Climber,
}

/// One species of a community. How many of it a stand holds, and in
/// which layer, is not set here: the stand's development decides it from
/// the species' niche and the years since disturbance, so a closed forest
/// closes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    /// A built-in species id ([`crate::api::species`]).
    pub species: String,
    /// The layer it keeps to, where the community says; the stand's
    /// development places it otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
    /// The settings its prototypes are grown in; every one unless given.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<Environment>,
    /// Its weight against the other members, 1 unless given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abundance: Option<f64>,
}

impl Member {
    fn environments(&self) -> Vec<Environment> {
        if self.environments.is_empty() {
            Environment::ALL.to_vec()
        } else {
            self.environments.clone()
        }
    }
}

/// A plant community: its members and how they share a stand. Built from
/// JSON (`{"id": ..., "members": [{"species": ..., "layer": ...,
/// "abundance": ...}]}`, layer and abundance optional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Community {
    pub id: String,
    pub members: Vec<Member>,
}

impl Community {
    /// A community from its JSON.
    ///
    /// # Errors
    ///
    /// When the JSON does not parse, has no members, or names a species
    /// the library does not have.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let community: Self =
            serde_json::from_str(text).map_err(|error| Error(format!("community: {error}")))?;
        community.check()?;
        Ok(community)
    }

    fn check(&self) -> Result<(), Error> {
        if self.members.is_empty() {
            return Err(Error(format!("community {}: no members", self.id)));
        }
        let known = crate::api::species();
        for member in &self.members {
            if !known.iter().any(|id| id == &member.species) {
                return Err(Error(format!(
                    "community {}: no species {}",
                    self.id, member.species
                )));
            }
            if member
                .abundance
                .is_some_and(|abundance| !(abundance.is_finite() && abundance >= 0.0))
            {
                return Err(Error(format!(
                    "community {}: {} abundance must be 0 or more",
                    self.id, member.species
                )));
            }
        }
        Ok(())
    }
}

/// How a [`Fill`] chooses its prototypes. Start from
/// [`FillSettings::default`] and set what you choose.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FillSettings {
    /// Age (or size) classes per species: dominants to saplings.
    pub age_classes: u16,
    /// Distinct seeds per species, setting and class.
    pub seeds: u16,
    /// How each prototype is drawn.
    pub options: Options,
    /// Whether a tall prototype is also written in parts (its bole and
    /// its crown in bands, [`crate::api::crown_regions`]), so a caller can
    /// draw each part at the detail its own distance asks for.
    pub parts: bool,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            age_classes: 4,
            seeds: 2,
            options: Options::default(),
            parts: true,
        }
    }
}

/// What a prototype is: the key a caller caches it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrototypeKey {
    pub species: String,
    pub seed: u64,
    pub environment: Environment,
    /// 0 is the oldest class (dominants); higher classes are younger.
    pub age_class: u16,
}

/// One plant drawn once and placed many times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prototype {
    pub key: PrototypeKey,
    /// The age it is grown to, years.
    pub age_years: f64,
    pub layer: Option<Layer>,
}

/// The name of the file listing a fill's written prototypes.
pub const PROTOTYPES_FILE: &str = "prototypes.json";

/// A prototype as written: its place in [`Fill::prototypes`], its key,
/// and its files and size as [`Generator::write_variants`] lists a plant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrototypeFiles {
    pub index: u32,
    pub key: PrototypeKey,
    pub age_years: f64,
    pub layer: Option<Layer>,
    pub plant: VariantRecord,
    /// A tall prototype's parts, each with its box and a file per level;
    /// empty for a plant drawn whole.
    #[serde(default)]
    pub parts: Vec<PartRecord>,
}

#[derive(Serialize)]
struct PrototypeList<'a> {
    schema: u32,
    generator_revision: u32,
    prototypes: &'a [PrototypeFiles],
}

/// One tile's ground, as the fill reads it: a flat or height-field site,
/// or a tile of a measured site field ([`Site::from_field`]), which says
/// where buildings, boulders and sound pavement leave no room for plants,
/// how much of a cracked pavement is open, and the slope, sun and wetness
/// of each cell.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Site {
    pub tile: TileId,
    /// The tile's side, metres.
    pub tile_m: f64,
    /// Ground heights on a square grid over the tile, row by row from the
    /// south-west corner, `cols * cols` of them (at least 2 by 2).
    pub heights_m: Vec<f32>,
    pub cols: usize,
    /// The site factors where the site does not say otherwise: moisture
    /// and heat load as they are, elevation added to the ground height,
    /// slope taken from the ground. [`Site::with_factors`] sets them.
    pub factors: SiteFactors,
    /// The measured site this tile is part of, and where the tile's
    /// south-west corner lies in it; `None` for a height-field site.
    pub field: Option<FieldTile>,
}

/// A tile's place in a measured site field ([`crate::site_field`]).
///
/// The field's frame is the site mesh's: +X east, +Y up, and north toward
/// -Z, so a point `(east, north)` metres from the tile's south-west corner
/// is at `x = corner[0] + east`, `z = corner[1] - north`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldTile {
    pub field: std::sync::Arc<SiteField>,
    /// The tile's south-west corner, (x, z) in the field's frame.
    pub corner: [f64; 2],
}

impl FieldTile {
    /// The field's cell under `(east, north)`, if the field covers it.
    fn cell(&self, east: f64, north: f64) -> Option<u32> {
        self.field
            .cell_at(self.corner[0] + east, self.corner[1] - north)
    }
}

/// A cell's wetness index ([`crate::site_field::Ground::wetness`]) as a
/// niche's moisture, 0 dry to 1 saturated: level open ground, whose index
/// is about 7 on a 1 m grid, is the middling 0.5 a height-field site has;
/// a steep slope that sheds its water (index 2 or below) is dry, and a
/// hollow or a channel that gathers a hillside's (12 or above) saturated.
#[must_use]
pub fn moisture_from_wetness(wetness: f64) -> f64 {
    ((wetness - 2.0) / 10.0).clamp(0.0, 1.0)
}

/// A height-field site's factors until a caller says: middling moisture,
/// level ground's sunlight, heights above sea level as given.
pub const DEFAULT_FACTORS: SiteFactors = SiteFactors {
    moisture: 0.5,
    heat_load: 1.0,
    elevation_m: 0.0,
    slope_deg: 0.0,
};

impl Site {
    /// A flat tile at height 0.
    ///
    /// # Errors
    ///
    /// When the side is not above 0.
    pub fn flat(tile: TileId, tile_m: f64) -> Result<Self, Error> {
        Self::from_heightfield(tile, tile_m, vec![0.0; 4], 2)
    }

    /// A tile from a square height field.
    ///
    /// # Errors
    ///
    /// When the side is not above 0 or the heights are not `cols` by
    /// `cols`, at least 2 by 2, and finite.
    pub fn from_heightfield(
        tile: TileId,
        tile_m: f64,
        heights_m: Vec<f32>,
        cols: usize,
    ) -> Result<Self, Error> {
        if !(tile_m.is_finite() && tile_m > 0.0) {
            return Err(Error(format!("site: tile side {tile_m} m must be above 0")));
        }
        if cols < 2 || heights_m.len() != cols * cols {
            return Err(Error(format!(
                "site: {} heights is not {cols} by {cols} (at least 2 by 2)",
                heights_m.len()
            )));
        }
        if heights_m.iter().any(|height| !height.is_finite()) {
            return Err(Error("site: heights must be finite".into()));
        }
        Ok(Self {
            tile,
            tile_m,
            heights_m,
            cols,
            factors: DEFAULT_FACTORS,
            field: None,
        })
    }

    /// A tile of a measured site field: `tile_m` on a side, its
    /// south-west corner at `corner` (x, z) in the field's frame (north is
    /// -z). Its heights are the field's ground, sampled on the field's
    /// cells (where a cell has no ground, inside a building or a boulder,
    /// the nearest ground's height in its row, or 0 if the row has none);
    /// its plantable share, moisture, heat load and slope are the field's
    /// own, cell by cell. `factors` keeps the default moisture and heat
    /// load for points the field does not cover, and its elevation is the
    /// height above sea level of the field's y = 0.
    ///
    /// # Errors
    ///
    /// When the side is not above 0 or the corner is not finite.
    pub fn from_field(
        tile: TileId,
        tile_m: f64,
        field: std::sync::Arc<SiteField>,
        corner: [f64; 2],
    ) -> Result<Self, Error> {
        if !(tile_m.is_finite() && tile_m > 0.0) {
            return Err(Error(format!("site: tile side {tile_m} m must be above 0")));
        }
        if !(corner[0].is_finite() && corner[1].is_finite()) {
            return Err(Error("site: the tile's corner must be finite".into()));
        }
        let cell_m = field.settings.cell_m;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cols = ((tile_m / cell_m).round() as usize).clamp(1, 4096) + 1;
        #[allow(clippy::cast_precision_loss)]
        let step = tile_m / (cols - 1) as f64;
        let mut heights_m = Vec::with_capacity(cols * cols);
        for j in 0..cols {
            #[allow(clippy::cast_precision_loss)]
            let row: Vec<Option<f64>> = (0..cols)
                .map(|i| field.height_at(corner[0] + i as f64 * step, corner[1] - j as f64 * step))
                .collect();
            for i in 0..cols {
                let near = row[i].or_else(|| {
                    (1..cols).find_map(|d| {
                        let left = i.checked_sub(d).and_then(|k| row[k]);
                        left.or_else(|| row.get(i + d).copied().flatten())
                    })
                });
                #[allow(clippy::cast_possible_truncation)]
                heights_m.push(near.unwrap_or(0.0) as f32);
            }
        }
        let mut site = Self::from_heightfield(tile, tile_m, heights_m, cols)?;
        site.field = Some(FieldTile { field, corner });
        Ok(site)
    }

    /// This site with `factors` as its own (see [`Site::factors`]).
    #[must_use]
    pub fn with_factors(mut self, factors: SiteFactors) -> Self {
        self.factors = factors;
        self
    }

    /// The share of plants that may stand at `(east, north)` metres from
    /// the tile's south-west corner, 0 to 1: 0 inside a building or on
    /// sound pavement, a crack's open share on broken pavement, 1 on soil.
    /// A height-field site is soil everywhere. On a field's tile it is the
    /// cell's open share ([`crate::site_field::Ground::open_share`]): 0
    /// where the cell has no ground (a building's or a boulder's
    /// footprint), on rock and on sound pavement, the share of its surface
    /// its cracks and holes open on broken pavement, 1 on soil; beyond the
    /// field, nothing.
    #[must_use]
    pub fn plantable_at(&self, east: f64, north: f64) -> f64 {
        let Some(tile) = &self.field else {
            return 1.0;
        };
        tile.cell(east, north).map_or(0.0, |cell| {
            let ground = &tile.field.ground;
            let index = cell as usize;
            if ground.height[index].is_none() {
                0.0
            } else {
                ground.open_share[index].clamp(0.0, 1.0)
            }
        })
    }

    /// The site factors at `(east, north)`, which set how well each
    /// species of a community suits the ground there
    /// ([`crate::niche::Niche::site_response`]). A height-field site has
    /// its [`Site::factors`], its elevation raised by the ground's height
    /// and its slope the ground's. On a field's tile, the cell's own
    /// ([`crate::site_field::SiteField::site_factors`]): its heat load
    /// relative to level ground, its slope, its elevation above the
    /// factors' datum, and its moisture from its wetness index
    /// ([`moisture_from_wetness`]); where the cell has no ground or the
    /// field does not reach, as on a height field.
    #[must_use]
    pub fn site_factors_at(&self, east: f64, north: f64) -> SiteFactors {
        if let Some(tile) = &self.field {
            if let Some(cell) = tile.cell(east, north) {
                let moisture = moisture_from_wetness(tile.field.ground.wetness[cell as usize]);
                if let Some(factors) =
                    tile.field
                        .site_factors(cell, self.factors.elevation_m, moisture)
                {
                    return factors;
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let step = self.tile_m / (self.cols - 1) as f64;
        let d = 0.5 * step;
        let rise_east = (self.height_at(east + d, north) - self.height_at(east - d, north)) / step;
        let rise_north = (self.height_at(east, north + d) - self.height_at(east, north - d)) / step;
        let slope = libm::atan(libm::sqrt(rise_east * rise_east + rise_north * rise_north));
        SiteFactors {
            elevation_m: self.factors.elevation_m + self.height_at(east, north),
            slope_deg: self.factors.slope_deg.max(slope.to_degrees()),
            ..self.factors
        }
    }

    /// The ground height at `(east, north)` metres from the tile's
    /// south-west corner, bilinear (on a field's tile, between the field's
    /// cells, [`crate::site_field::SiteField::height_at`], or the sampled
    /// heights where no cell round it has ground).
    #[must_use]
    pub fn height_at(&self, east: f64, north: f64) -> f64 {
        if let Some(tile) = &self.field {
            if let Some(height) = tile
                .field
                .height_at(tile.corner[0] + east, tile.corner[1] - north)
            {
                return height;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let last = (self.cols - 1) as f64;
        let u = (east / self.tile_m).clamp(0.0, 1.0) * last;
        let v = (north / self.tile_m).clamp(0.0, 1.0) * last;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (i, j) = (
            (u.floor() as usize).min(self.cols - 2),
            (v.floor() as usize).min(self.cols - 2),
        );
        #[allow(clippy::cast_precision_loss)]
        let (fu, fv) = (u - i as f64, v - j as f64);
        let at = |a: usize, b: usize| f64::from(self.heights_m[b * self.cols + a]);
        let south = at(i, j) * (1.0 - fu) + at(i + 1, j) * fu;
        let north_row = at(i, j + 1) * (1.0 - fu) + at(i + 1, j + 1) * fu;
        south * (1.0 - fv) + north_row * fv
    }
}

/// One stand cell: which community grows there and how long since it was
/// cleared, burnt or abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StandCell {
    /// Index into the fill's communities; `None` is bare (pavement,
    /// building, water).
    pub community: Option<u16>,
    pub years_since_disturbance: f32,
}

/// Which community grows where over a tile, cell by cell, row by row from
/// the south-west corner. One community over the whole tile is
/// [`StandMap::uniform`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StandMap {
    /// The cell side, metres; the tile side must be a whole number of
    /// cells.
    pub cell_m: f64,
    pub cols: usize,
    pub cells: Vec<StandCell>,
}

impl StandMap {
    /// Community `community` at `years` since disturbance over a whole
    /// tile of side `tile_m`, in cells of about [`STAND_CELL_M`].
    #[must_use]
    pub fn uniform(tile_m: f64, community: u16, years: f32) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cols = ((tile_m / STAND_CELL_M).round() as usize).max(1);
        #[allow(clippy::cast_precision_loss)]
        let cell_m = tile_m / cols as f64;
        Self {
            cell_m,
            cols,
            cells: vec![
                StandCell {
                    community: Some(community),
                    years_since_disturbance: years,
                };
                cols * cols
            ],
        }
    }

    fn check(&self, site: &Site, communities: usize) -> Result<(), Error> {
        #[allow(clippy::cast_precision_loss)]
        let covered_m = self.cell_m * self.cols as f64;
        if !(self.cell_m.is_finite() && self.cell_m > 0.0)
            || self.cells.len() != self.cols * self.cols
            || (covered_m - site.tile_m).abs() > 1e-6 * site.tile_m
        {
            return Err(Error(format!(
                "stands: {} cells of {} m do not cover a {} m tile",
                self.cells.len(),
                self.cell_m,
                site.tile_m
            )));
        }
        if let Some(cell) = self.cells.iter().find(|cell| {
            cell.community
                .is_some_and(|c| usize::from(c) >= communities)
        }) {
            return Err(Error(format!(
                "stands: community {:?} is not one of the fill's {communities}",
                cell.community
            )));
        }
        Ok(())
    }
}

/// One placed plant, packed into [`INSTANCE_BYTES`] by
/// [`Instance::to_bytes`]: u32 prototype, f32 x3 position, i16 x4 snorm
/// rotation quaternion (x, y, z, w), f16 scale, u16 age (tenths of a
/// year), i8 x2 tint (lightness, hue), u16 extra (a climber's reach clip
/// in decimetres, otherwise 0); little-endian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    pub prototype: u32,
    /// East, north, up, metres from the tile's south-west corner at
    /// height 0.
    pub position: [f32; 3],
    /// Unit quaternion (x, y, z, w), z up, turning the prototype about its
    /// foot: its yaw, and its lean where its crown stands off the stem
    /// toward room (a small tilt, as [`Instance::lean`] makes it).
    pub rotation: [f32; 4],
    pub scale: f32,
    pub age_years: f32,
    /// Lightness and hue shifts, -1 to 1.
    pub tint: [f32; 2],
    pub extra: u16,
}

impl Instance {
    /// The rotation of a plant turned `yaw` radians about the vertical
    /// (east toward north) and leant `lean` (east and north metres per
    /// metre up) about its foot.
    #[must_use]
    pub fn lean(yaw: f64, lean: [f64; 2]) -> [f32; 4] {
        let half = yaw / 2.0;
        let turn = [0.0, 0.0, half.sin(), half.cos()];
        let slope = lean[0].hypot(lean[1]);
        let tilt = if slope > 0.0 {
            // About the level axis square to the lean, by its angle.
            let angle = slope.atan() / 2.0;
            let axis = [-lean[1] / slope, lean[0] / slope];
            [
                axis[0] * angle.sin(),
                axis[1] * angle.sin(),
                0.0,
                angle.cos(),
            ]
        } else {
            [0.0, 0.0, 0.0, 1.0]
        };
        // The tilt after the turn.
        let [ax, ay, az, aw] = tilt;
        let [bx, by, bz, bw] = turn;
        #[allow(clippy::cast_possible_truncation)]
        [
            (aw * bx + ax * bw + ay * bz - az * by) as f32,
            (aw * by - ax * bz + ay * bw + az * bx) as f32,
            (aw * bz + ax * by - ay * bx + az * bw) as f32,
            (aw * bw - ax * bx - ay * by - az * bz) as f32,
        ]
    }

    /// The packed form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; INSTANCE_BYTES] {
        let mut out = [0_u8; INSTANCE_BYTES];
        out[0..4].copy_from_slice(&self.prototype.to_le_bytes());
        for (i, value) in self.position.iter().enumerate() {
            out[4 + 4 * i..8 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in self.rotation.iter().enumerate() {
            out[16 + 2 * i..18 + 2 * i].copy_from_slice(&snorm16(*value).to_le_bytes());
        }
        out[24..26].copy_from_slice(&f16_bits(self.scale).to_le_bytes());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let age = (self.age_years * 10.0).round().clamp(0.0, 65_535.0) as u16;
        out[26..28].copy_from_slice(&age.to_le_bytes());
        out[28] = snorm8(self.tint[0]).to_le_bytes()[0];
        out[29] = snorm8(self.tint[1]).to_le_bytes()[0];
        out[30..32].copy_from_slice(&self.extra.to_le_bytes());
        out
    }

    /// Unpack [`Self::to_bytes`] (rotation, scale, age and tint to their
    /// packed precision).
    #[must_use]
    pub fn from_bytes(bytes: &[u8; INSTANCE_BYTES]) -> Self {
        let word = |at: usize| [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        let half = |at: usize| [bytes[at], bytes[at + 1]];
        let rotation =
            std::array::from_fn(|i| f32::from(i16::from_le_bytes(half(16 + 2 * i))) / 32_767.0);
        Self {
            prototype: u32::from_le_bytes(word(0)),
            position: std::array::from_fn(|i| f32::from_le_bytes(word(4 + 4 * i))),
            rotation,
            scale: f16_value(u16::from_le_bytes(half(24))),
            age_years: f32::from(u16::from_le_bytes(half(26))) / 10.0,
            tint: [
                f32::from(i8::from_le_bytes([bytes[28]])) / 127.0,
                f32::from(i8::from_le_bytes([bytes[29]])) / 127.0,
            ],
            extra: u16::from_le_bytes(half(30)),
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn snorm16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

#[allow(clippy::cast_possible_truncation)]
fn snorm8(value: f32) -> i8 {
    (value.clamp(-1.0, 1.0) * 127.0).round() as i8
}

/// IEEE half-precision bits, rounded to nearest; tiny values go to 0 and
/// large ones to the largest half.
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    #[allow(clippy::cast_possible_truncation)]
    let sign = ((bits >> 16) & 0x8000) as u16;
    let magnitude = value.abs();
    if magnitude.is_nan() {
        return 0x7e00;
    }
    if magnitude < 6.104e-5 {
        return sign;
    }
    if magnitude >= 65_504.0 {
        return sign | 0x7bff;
    }
    let exponent = ((bits >> 23) & 0xff).cast_signed() - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    // Round the 23-bit mantissa to 10 bits; a carry rolls into the
    // exponent, which the sum does by itself.
    let rounded = (exponent.cast_unsigned() << 10) + ((mantissa + 0x1000) >> 13);
    #[allow(clippy::cast_possible_truncation)]
    let rounded = rounded.min(0x7bff) as u16;
    sign | rounded
}

fn f16_value(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    if exponent == 0 {
        return sign * mantissa * 2.0_f32.powi(-24);
    }
    sign * (1.0 + mantissa / 1024.0) * 2.0_f32.powi(exponent - 15)
}

/// What covers the ground of a stand cell, for hosts that draw ground
/// cover with their own systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverKind {
    Bare,
    Litter,
    Herb,
    Grass,
    Moss,
}

/// One stand cell's ground cover.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cover {
    pub kind: CoverKind,
    /// 0 to 1.
    pub fraction: f32,
}

/// One stand cell of the far canopy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanopyCell {
    pub height_m: f32,
    /// Share of the cell under crowns, 0 to 1.
    pub cover: f32,
    /// Linear RGB of the canopy seen from above.
    pub colour: [f32; 3],
}

/// The stand-scale numbers a tile's plan can be checked against.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TileStats {
    pub instances: usize,
    pub stems_per_ha: f64,
}

/// A filled tile: every placed plant, the ground cover and the far
/// canopy, per stand cell of the [`StandMap`] it was filled from.
#[derive(Debug, Clone, PartialEq)]
pub struct TilePlan {
    pub tile: TileId,
    pub tile_m: f64,
    pub cell_m: f64,
    pub cols: usize,
    pub instances: Vec<Instance>,
    pub cover: Vec<Cover>,
    pub canopy: Vec<CanopyCell>,
    pub stats: TileStats,
}

#[derive(Serialize)]
struct TileHeader<'a> {
    schema: u32,
    generator_revision: u32,
    tile: TileId,
    tile_m: f64,
    cell_m: f64,
    cols: usize,
    instance_bytes: usize,
    instances: usize,
    instances_file: &'a str,
    cover: &'a [Cover],
    canopy: &'a [CanopyCell],
    stats: TileStats,
}

impl TilePlan {
    /// The prototypes the plan places, each once, in order: what to pass
    /// [`Fill::write_prototypes`] for this tile.
    #[must_use]
    pub fn prototypes_used(&self) -> Vec<u32> {
        let mut used: Vec<u32> = self.instances.iter().map(|i| i.prototype).collect();
        used.sort_unstable();
        used.dedup();
        used
    }

    /// The instances packed back to back.
    #[must_use]
    pub fn instance_bytes(&self) -> Vec<u8> {
        self.instances.iter().flat_map(Instance::to_bytes).collect()
    }

    /// Write `<x>_<y>.json` (header, cover, canopy, stats) and
    /// `<x>_<y>.bin` (the packed instances) into `dir`; the JSON's path.
    ///
    /// # Errors
    ///
    /// When a file cannot be written.
    pub fn write(&self, dir: &Path) -> Result<std::path::PathBuf, Error> {
        let stem = format!("{}_{}", self.tile.x, self.tile.y);
        let bin = format!("{stem}.bin");
        let header = TileHeader {
            schema: INSTANCE_SCHEMA,
            generator_revision: crate::GENERATOR_REVISION,
            tile: self.tile,
            tile_m: self.tile_m,
            cell_m: self.cell_m,
            cols: self.cols,
            instance_bytes: INSTANCE_BYTES,
            instances: self.instances.len(),
            instances_file: &bin,
            cover: &self.cover,
            canopy: &self.canopy,
            stats: self.stats,
        };
        let fail = |error: std::io::Error| Error(format!("{}: {error}", dir.display()));
        std::fs::create_dir_all(dir).map_err(fail)?;
        std::fs::write(dir.join(&bin), self.instance_bytes()).map_err(fail)?;
        let json = serde_json::to_string_pretty(&header).map_err(|e| Error(e.to_string()))?;
        let path = dir.join(format!("{stem}.json"));
        std::fs::write(&path, json).map_err(fail)?;
        Ok(path)
    }
}

/// A set of communities, their prototypes chosen once; fills any number
/// of tiles. It is `Send + Sync` and [`Fill::tile`] takes `&self`, so a
/// caller fills tiles on its own threads.
#[derive(Debug, Clone)]
pub struct Fill {
    communities: Vec<Community>,
    prototypes: Vec<Prototype>,
    settings: FillSettings,
}

impl Fill {
    /// Choose the prototypes of `communities`: for each species, every
    /// setting, age class and seed. Nothing is grown yet; see
    /// [`Fill::grow_prototype`].
    ///
    /// # Errors
    ///
    /// When there are no communities, one does not check, or the settings
    /// ask for no classes or seeds.
    pub fn new(communities: &[Community], settings: &FillSettings) -> Result<Self, Error> {
        if communities.is_empty() {
            return Err(Error("fill: no communities".into()));
        }
        if settings.age_classes == 0 || settings.seeds == 0 {
            return Err(Error("fill: at least one age class and one seed".into()));
        }
        for community in communities {
            community.check()?;
        }
        let mut prototypes = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for member in communities.iter().flat_map(|c| &c.members) {
            if !seen.insert(member.species.clone()) {
                continue;
            }
            let generator = Generator::new(&member.species, &settings.options)?;
            let oldest = generator.default_age();
            for environment in member.environments() {
                for age_class in 0..settings.age_classes {
                    for seed in 1..=u64::from(settings.seeds) {
                        prototypes.push(Prototype {
                            key: PrototypeKey {
                                species: member.species.clone(),
                                seed,
                                environment,
                                age_class,
                            },
                            age_years: class_age(oldest, age_class, settings.age_classes),
                            layer: member.layer,
                        });
                    }
                }
            }
        }
        Ok(Self {
            communities: communities.to_vec(),
            prototypes,
            settings: *settings,
        })
    }

    #[must_use]
    pub fn communities(&self) -> &[Community] {
        &self.communities
    }

    /// Every prototype; an [`Instance::prototype`] is an index here.
    #[must_use]
    pub fn prototypes(&self) -> &[Prototype] {
        &self.prototypes
    }

    /// Grow prototype `index`, to export with its species'
    /// [`Generator::export`].
    ///
    /// # Errors
    ///
    /// When there is no such prototype or it does not grow.
    pub fn grow_prototype(&self, index: usize) -> Result<(Generator, Plant), Error> {
        let prototype = self
            .prototypes
            .get(index)
            .ok_or_else(|| Error(format!("fill: no prototype {index}")))?;
        let generator = Generator::new(&prototype.key.species, &self.settings.options)?;
        let plant = generator.grow_in(
            prototype.key.environment,
            prototype.key.seed,
            prototype.age_years,
        )?;
        Ok((generator, plant))
    }

    /// Grow and export the prototypes `which` (indices into
    /// [`Fill::prototypes`]; every one when empty) into `out`, on up to
    /// `threads` threads, and list them in [`PROTOTYPES_FILE`]. A
    /// prototype whose record (`<stem>.prototype.json`) is in `out` from
    /// the same revision is kept, not drawn again, so a city's tiles can
    /// ask for theirs as they come.
    ///
    /// # Errors
    ///
    /// When an index is not a prototype, one does not grow or export, or a
    /// file cannot be written.
    pub fn write_prototypes(
        &self,
        which: &[u32],
        settings: &Settings,
        threads: usize,
        out: &Path,
    ) -> Result<Vec<PrototypeFiles>, Error> {
        let mut wanted: Vec<u32> = if which.is_empty() {
            (0..u32::try_from(self.prototypes.len()).unwrap_or(u32::MAX)).collect()
        } else {
            which.to_vec()
        };
        wanted.sort_unstable();
        wanted.dedup();
        if let Some(bad) = wanted
            .iter()
            .find(|i| **i as usize >= self.prototypes.len())
        {
            return Err(Error(format!("fill: no prototype {bad}")));
        }
        std::fs::create_dir_all(out)
            .map_err(|error| Error(format!("cannot create {}: {error}", out.display())))?;
        let mut generators = std::collections::BTreeMap::new();
        for index in &wanted {
            let species = &self.prototypes[*index as usize].key.species;
            if !generators.contains_key(species) {
                generators.insert(
                    species.clone(),
                    Generator::new(species, &self.settings.options)?,
                );
            }
        }
        let next = std::sync::Mutex::new(0_usize);
        let done = std::sync::Mutex::new(vec![None; wanted.len()]);
        let failure = std::sync::Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..threads.clamp(1, wanted.len().max(1)) {
                scope.spawn(|| {
                    loop {
                        let slot = {
                            let mut next = next
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if *next >= wanted.len()
                                || failure
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .is_some()
                            {
                                return;
                            }
                            *next += 1;
                            *next - 1
                        };
                        let index = wanted[slot];
                        let prototype = &self.prototypes[index as usize];
                        let generator = &generators[&prototype.key.species];
                        match Self::write_prototype(
                            generator,
                            index,
                            prototype,
                            settings,
                            self.settings.parts,
                            out,
                        ) {
                            Ok(files) => {
                                done.lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)[slot] =
                                    Some(files);
                            }
                            Err(error) => {
                                failure
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .get_or_insert(error);
                                return;
                            }
                        }
                    }
                });
            }
        });
        if let Some(error) = failure
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return Err(error);
        }
        let written: Vec<PrototypeFiles> = done
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .into_iter()
            .flatten()
            .collect();
        let list = PrototypeList {
            schema: INSTANCE_SCHEMA,
            generator_revision: crate::GENERATOR_REVISION,
            prototypes: &written,
        };
        let path = out.join(PROTOTYPES_FILE);
        let text = serde_json::to_string_pretty(&list).map_err(|error| Error(error.to_string()))?;
        std::fs::write(&path, text + "\n")
            .map_err(|error| Error(format!("cannot write {}: {error}", path.display())))?;
        Ok(written)
    }

    fn write_prototype(
        generator: &Generator,
        index: u32,
        prototype: &Prototype,
        settings: &Settings,
        parts: bool,
        out: &Path,
    ) -> Result<PrototypeFiles, Error> {
        let key = &prototype.key;
        let record = out.join(format!(
            "{}-{}-{}-a{}{}.prototype.json",
            key.species,
            key.environment.name(),
            key.seed,
            key.age_class,
            if parts { "-parts" } else { "" }
        ));
        if let Some(kept) = std::fs::read_to_string(&record)
            .ok()
            .and_then(|text| serde_json::from_str::<KeptPrototype>(&text).ok())
            .filter(|kept| {
                kept.generator_revision == crate::GENERATOR_REVISION
                    && kept.files.key == *key
                    && (kept.files.age_years - prototype.age_years).abs() < 1e-9
                    && kept
                        .files
                        .plant
                        .levels
                        .iter()
                        .chain(kept.files.parts.iter().flat_map(|part| &part.levels))
                        .all(|level| out.join(&level.file).is_file())
            })
        {
            return Ok(PrototypeFiles {
                index,
                ..kept.files
            });
        }
        let (plant, parts) = generator.write_one_in(
            key.environment,
            key.seed,
            prototype.age_years,
            settings,
            out,
            parts,
        )?;
        let files = PrototypeFiles {
            index,
            key: key.clone(),
            age_years: prototype.age_years,
            layer: prototype.layer,
            plant,
            parts,
        };
        let kept = KeptPrototype {
            generator_revision: crate::GENERATOR_REVISION,
            files: files.clone(),
        };
        let text = serde_json::to_string_pretty(&kept).map_err(|error| Error(error.to_string()))?;
        std::fs::write(&record, text + "\n")
            .map_err(|error| Error(format!("cannot write {}: {error}", record.display())))?;
        Ok(files)
    }

    /// Fill one tile: its instances, ground cover and far canopy.
    ///
    /// # Errors
    ///
    /// When the stand map does not cover the site or names a community
    /// the fill does not have.
    pub fn tile(&self, site: &Site, stands: &StandMap, seed: u64) -> Result<TilePlan, Error> {
        stands.check(site, self.communities.len())?;
        let cell_area_ha = stands.cell_m * stands.cell_m / 10_000.0;
        let mut instances = Vec::new();
        let mut cover = Vec::with_capacity(stands.cells.len());
        let mut canopy = Vec::with_capacity(stands.cells.len());
        for (index, cell) in stands.cells.iter().enumerate() {
            let (col, row) = (index % stands.cols, index / stands.cols);
            let world = world_cell(site, stands, col, row);
            let Some(community) = cell.community.map(|c| &self.communities[usize::from(c)]) else {
                cover.push(Cover {
                    kind: CoverKind::Bare,
                    fraction: 0.0,
                });
                canopy.push(CanopyCell {
                    height_m: 0.0,
                    cover: 0.0,
                    colour: [0.0; 3],
                });
                continue;
            };
            let before = instances.len();
            for (m, member) in community.members.iter().enumerate() {
                let key = rng::hash_words(&[seed, world[0], world[1], m as u64]);
                let expected = stub_stems_per_ha(member.layer)
                    * member.abundance.unwrap_or(1.0)
                    * cell_area_ha;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let count = (expected + rng::unit(key)).floor() as u64;
                for n in 0..count {
                    let draw = |salt: u64| rng::unit(rng::hash_words(&[key, n, salt]));
                    #[allow(clippy::cast_precision_loss)]
                    let east = (col as f64 + draw(1)) * stands.cell_m;
                    #[allow(clippy::cast_precision_loss)]
                    let north = (row as f64 + draw(2)) * stands.cell_m;
                    let prototype = self.pick(member, draw(3), draw(4));
                    let yaw = draw(5) * std::f64::consts::TAU;
                    #[allow(clippy::cast_possible_truncation)]
                    instances.push(Instance {
                        prototype,
                        position: [
                            east as f32,
                            north as f32,
                            site.height_at(east, north) as f32,
                        ],
                        rotation: Instance::lean(yaw, [0.0, 0.0]),
                        scale: (0.85 + 0.3 * draw(6)) as f32,
                        age_years: self.prototypes[prototype as usize].age_years as f32,
                        tint: [
                            (draw(7) * 2.0 - 1.0) as f32 * 0.2,
                            (draw(8) * 2.0 - 1.0) as f32 * 0.1,
                        ],
                        extra: 0,
                    });
                }
            }
            #[allow(clippy::cast_precision_loss)]
            let placed = (instances.len() - before) as f64;
            #[allow(clippy::cast_possible_truncation)]
            let closure = (placed * 25.0 / (stands.cell_m * stands.cell_m)).min(1.0) as f32;
            cover.push(Cover {
                kind: CoverKind::Litter,
                fraction: closure,
            });
            canopy.push(CanopyCell {
                height_m: 0.0,
                cover: closure,
                colour: [0.08, 0.12, 0.06],
            });
        }
        #[allow(clippy::cast_precision_loss)]
        let stems_per_ha = instances.len() as f64 / (site.tile_m * site.tile_m / 10_000.0);
        Ok(TilePlan {
            tile: site.tile,
            tile_m: site.tile_m,
            cell_m: stands.cell_m,
            cols: stands.cols,
            stats: TileStats {
                instances: instances.len(),
                stems_per_ha,
            },
            instances,
            cover,
            canopy,
        })
    }

    /// The prototype a member's plant is drawn as: an interior plant of a
    /// class and seed by its draws.
    fn pick(&self, member: &Member, class_draw: f64, seed_draw: f64) -> u32 {
        let classes = f64::from(self.settings.age_classes);
        let seeds = f64::from(self.settings.seeds);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (class, seed) = (
            ((class_draw * classes) as u16).min(self.settings.age_classes - 1),
            ((seed_draw * seeds) as u64).min(u64::from(self.settings.seeds) - 1) + 1,
        );
        let environments = member.environments();
        let environment = if environments.contains(&Environment::Interior) {
            Environment::Interior
        } else {
            environments[0]
        };
        let index = self
            .prototypes
            .iter()
            .position(|p| {
                p.key.species == member.species
                    && p.key.environment == environment
                    && p.key.age_class == class
                    && p.key.seed == seed
            })
            .unwrap_or(0);
        u32::try_from(index).unwrap_or(u32::MAX)
    }
}

#[derive(Serialize, Deserialize)]
struct KeptPrototype {
    generator_revision: u32,
    files: PrototypeFiles,
}

/// The stub's density until the stand's development sets it: mature
/// stems per hectare by layer.
fn stub_stems_per_ha(layer: Option<Layer>) -> f64 {
    match layer {
        Some(Layer::Canopy) | None => 300.0,
        Some(Layer::Subcanopy) => 200.0,
        Some(Layer::Shrub) => 1_000.0,
        Some(Layer::Herb) => 3_000.0,
        Some(Layer::Ground) => 5_000.0,
        Some(Layer::Climber) => 100.0,
    }
}

/// The age of class `class` of `classes`: the oldest at class 0, each
/// younger class a step nearer a tenth of it.
fn class_age(oldest: f64, class: u16, classes: u16) -> f64 {
    if classes <= 1 {
        return oldest;
    }
    let share = 1.0 - 0.9 * f64::from(class) / f64::from(classes - 1);
    oldest * share
}

/// A stand cell's place in the world grid of cells, as two words that key
/// its draws.
fn world_cell(site: &Site, stands: &StandMap, col: usize, row: usize) -> [u64; 2] {
    let cols = i64::try_from(stands.cols).unwrap_or(i64::MAX);
    let at = |tile: i32, cell: usize| {
        (i64::from(tile) * cols + i64::try_from(cell).unwrap_or(0)).cast_unsigned()
    };
    [at(site.tile.x, col), at(site.tile.y, row)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn an_instance_packs_into_32_bytes_and_back() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let instance = Instance {
            prototype: 7,
            position: [12.5, -3.25, 101.0],
            rotation: [0.0, 0.0, half, half],
            scale: 1.125,
            age_years: 63.4,
            tint: [0.5, -0.25],
            extra: 42,
        };
        let back = Instance::from_bytes(&instance.to_bytes());
        assert_eq!(back.prototype, 7);
        assert_eq!(back.position, instance.position);
        assert!((back.rotation[2] - half).abs() < 1e-4);
        assert!((back.scale - 1.125).abs() < 1e-3);
        assert!((back.age_years - 63.4).abs() < 0.05);
        assert!((back.tint[0] - 0.5).abs() < 0.01 && (back.tint[1] + 0.25).abs() < 0.01);
        assert_eq!(back.extra, 42);
    }

    #[test]
    fn halves_round_trip_the_scales_a_fill_uses() {
        for scale in [0.1_f32, 0.5, 0.85, 1.0, 1.15, 2.0, 7.5] {
            assert!((f16_value(f16_bits(scale)) - scale).abs() < scale * 1e-3);
        }
    }

    #[test]
    fn class_ages_run_from_the_oldest_to_a_tenth() {
        assert!((class_age(100.0, 0, 4) - 100.0).abs() < 1e-9);
        assert!((class_age(100.0, 3, 4) - 10.0).abs() < 1e-9);
    }

    /// Turns `v` by the quaternion `q` (x, y, z, w).
    #[allow(clippy::many_single_char_names)]
    fn turned(q: [f32; 4], v: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = q.map(f64::from);
        let t = [
            2.0 * (y * v[2] - z * v[1]),
            2.0 * (z * v[0] - x * v[2]),
            2.0 * (x * v[1] - y * v[0]),
        ];
        [
            v[0] + w * t[0] + (y * t[2] - z * t[1]),
            v[1] + w * t[1] + (z * t[0] - x * t[2]),
            v[2] + w * t[2] + (x * t[1] - y * t[0]),
        ]
    }

    #[test]
    fn a_lean_tilts_the_plants_up_axis_toward_it_after_its_yaw() {
        let yaw = 0.7;
        let plain = Instance::lean(yaw, [0.0, 0.0]);
        let half = yaw / 2.0;
        #[allow(clippy::cast_possible_truncation)]
        let expected = [0.0, 0.0, half.sin() as f32, half.cos() as f32];
        for (a, b) in plain.iter().zip(expected) {
            assert!((a - b).abs() < 1e-6);
        }
        for lean in [[0.1, 0.0], [0.0, -0.25], [0.3, 0.4]] {
            let up = turned(Instance::lean(yaw, lean), [0.0, 0.0, 1.0]);
            // The top moves `lean` metres east and north per metre up.
            assert!((up[0] / up[2] - lean[0]).abs() < 1e-5, "{lean:?}: {up:?}");
            assert!((up[1] / up[2] - lean[1]).abs() < 1e-5, "{lean:?}: {up:?}");
        }
        // The yaw still turns the plant about its own axis: east turns by `yaw`.
        let east = turned(plain, [1.0, 0.0, 0.0]);
        assert!((east[1].atan2(east[0]) - yaw).abs() < 1e-5);
    }
}
