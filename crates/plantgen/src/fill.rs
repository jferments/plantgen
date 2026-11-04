//! Fill city tiles with plant communities, fast: the external contract.
//!
//! A fill never grows a plant per place. [`Fill::new`] develops each
//! community's stand at a few stand ages once ([`crate::development`]:
//! which cohorts of which species a stand of that age holds, and how much
//! of the ground each layer covers) and keeps each cohort as
//! *prototypes*: a species, a seed, a setting and an age each.
//! [`Fill::measure`] grows each species once per seed and setting, keeping
//! every age its prototypes need from that one growth, so the fill knows
//! how big each is. [`Fill::tile`] then fills each stand cell from a stand
//! tile ([`crate::stand_tile`]: crowns placed until each layer covers what
//! its stage covers) laid out once for its community, age and site, turned
//! and shifted by the cell's place in the world, and keeps the plants the
//! ground there takes ([`Site::plantable_at`]). A tile is packed
//! [`Instance`]s of the prototypes, plus a ground cover raster and a far
//! canopy raster; any engine draws that with GPU instancing.
//!
//! A tile's plan is a pure function of the fill's communities and
//! settings, the [`Site`], the [`StandMap`], the seed and
//! [`GENERATOR_REVISION`](crate::GENERATOR_REVISION): the same on every
//! machine, thread count and order. Every draw is keyed by a stand cell's
//! place in the world, never by its place in the tile, so tiles laid side
//! by side meet without a seam and a caller can fill any tile alone.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};

use crate::api::{Error, Generator, Options, PartRecord, Plant, Settings, VariantRecord};
use crate::development::{self, Candidate, Stage};
use crate::niche::{self, Niche, SiteFactors};
use crate::package::Measurements;
use crate::rng;
use crate::spec::Environment;
use crate::stand_tile;

/// The version of the instance file ([`Instance::to_bytes`]) and of the
/// tile JSON beside it.
pub const INSTANCE_SCHEMA: u32 = 2;

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

/// The stand ages a fill develops its stands at unless chosen, years
/// since disturbance: doubling from 10 to 640, so each stage of the sere
/// ([`crate::development::Stage`]) has one or two.
pub const STAND_AGES: [f64; 7] = [10.0, 20.0, 40.0, 80.0, 160.0, 320.0, 640.0];

/// The scale range of a developed stand's copies of a prototype. Each
/// prototype is its cohort's grown dominant, so its copies are that size
/// or smaller: Wind River's tallest Douglas-fir stands 1.24 times its
/// Douglas-firs' mean height (Shaw et al. 2004, Ecosystems 7: 427-439).
pub const STAND_SCALE: [f64; 2] = [0.7, 1.0];

/// Stand tiles laid out for each community, age and site, of which each
/// cell draws one by its place in the world: neighbouring cells of one
/// stand differ even before each is turned and shifted.
pub const LAYOUTS: u64 = 4;

/// How a [`Fill`] chooses its prototypes. Start from
/// [`FillSettings::default`] and set what you choose.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FillSettings {
    /// The ages, years since disturbance, each community's stand is
    /// developed at, rising ([`STAND_AGES`] unless chosen). A stand cell
    /// is drawn at the one nearest its own years, by ratio.
    pub stand_ages: Vec<f64>,
    /// Distinct seeds per species, setting and age.
    pub seeds: u16,
    /// How each prototype is drawn.
    pub options: Options,
    /// Whether a tall prototype is also written in parts (its bole and
    /// its crown in bands, [`crate::api::crown_regions`]), so a caller can
    /// draw each part at the detail its own distance asks for.
    pub parts: bool,
    /// Whether herbs are placed one by one. Without them a tile's herbs
    /// are only its ground cover raster ([`Cover`]), for a host that draws
    /// ground cover its own way: a hectare of herbs is tens of thousands
    /// of instances.
    pub herbs: bool,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            stand_ages: STAND_AGES.to_vec(),
            seeds: 2,
            options: Options::default(),
            parts: true,
            herbs: true,
        }
    }
}

/// What a prototype is: the key a caller caches it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrototypeKey {
    pub species: String,
    pub seed: u64,
    pub environment: Environment,
    /// The age it is grown to, tenths of a year (as an [`Instance`] packs
    /// its age): one of its cohorts' ages in a stand's development.
    pub age_tenths: u32,
}

impl PrototypeKey {
    /// The common name of its files: `<species>-<setting>-<seed>-<age>y`,
    /// as [`Generator::export`] names a plant.
    #[must_use]
    pub fn stem(&self) -> String {
        let age = if self.age_tenths.is_multiple_of(10) {
            format!("{}", self.age_tenths / 10)
        } else {
            format!("{}.{}", self.age_tenths / 10, self.age_tenths % 10)
        };
        format!(
            "{}-{}-{}-{age}y",
            self.species,
            self.environment.name(),
            self.seed
        )
    }
}

/// One plant drawn once and placed many times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prototype {
    pub key: PrototypeKey,
    /// The age it is grown to, years.
    pub age_years: f64,
    /// The layer its community keeps its species to, where it says.
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

/// One tile's ground, as the fill reads it. The sites thread grows this
/// into the agreed site field (slope, soil depth, cracks, surface
/// patches); a flat or height-field site is the simple case.
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
        })
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
    /// A height-field site is soil everywhere.
    #[must_use]
    pub fn plantable_at(&self, east: f64, north: f64) -> f64 {
        let _ = (east, north);
        1.0
    }

    /// The site factors at `(east, north)`, which set how well each
    /// species of a community suits the ground there
    /// ([`crate::niche::Niche::site_response`]). A height-field site has
    /// its [`Site::factors`], its elevation raised by the ground's height
    /// and its slope the ground's.
    #[must_use]
    pub fn site_factors_at(&self, east: f64, north: f64) -> SiteFactors {
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
    /// south-west corner, bilinear.
    #[must_use]
    pub fn height_at(&self, east: f64, north: f64) -> f64 {
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

/// The stand-scale numbers a tile's plan can be checked against forest
/// plots, over the whole tile (bare cells too).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TileStats {
    pub instances: usize,
    /// Every placed plant, herbs too.
    pub stems_per_ha: f64,
    /// Plants 5 cm or more through at breast height (1.3 m), as plots
    /// count trees.
    pub trees_per_ha: f64,
    /// Their stems' cross-section at breast height, m² a hectare.
    pub basal_area_m2_per_ha: f64,
    /// Share of the ground under tree crowns seen from above, 0 to 1.
    pub canopy_cover: f64,
    /// One-sided leaf area over ground area.
    pub leaf_area_index: f64,
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

/// A set of communities, their stands developed and prototypes chosen
/// once; fills any number of tiles. It is `Send + Sync` and every method
/// takes `&self`, so a caller fills tiles on its own threads; clones share
/// what has been measured and laid out.
#[derive(Debug, Clone)]
pub struct Fill {
    communities: Vec<Community>,
    prototypes: Vec<Prototype>,
    settings: FillSettings,
    /// Each species of the communities, in the order first named.
    species: Vec<Species>,
    /// Each community's members that stand, in order: the candidates of
    /// its development.
    stands: Vec<Vec<StandMember>>,
    /// Each prototype's index by its key.
    index: HashMap<PrototypeKey, u32>,
    /// Each prototype's size, once measured or written.
    sizes: Arc<Vec<OnceLock<Measurements>>>,
    /// The stand tiles laid out so far.
    laid: Arc<Mutex<HashMap<LayoutKey, Arc<Layout>>>>,
}

#[derive(Debug, Clone)]
struct Species {
    id: String,
    generator: Generator,
    niche: Option<Niche>,
    /// The seeds of its prototypes: its package's own first.
    seeds: Vec<u64>,
    /// The light it needs from above to stand
    /// ([`development::light_needed`]).
    light_needed: f64,
}

impl Species {
    fn new(id: &str, settings: &FillSettings) -> Result<Self, Error> {
        let generator = Generator::new(id, &settings.options)?;
        let niche = Niche::builtin(id).ok();
        let mut seeds = Vec::new();
        for seed in generator.spec().variants.seeds.iter().copied().chain(1..) {
            if seeds.len() >= usize::from(settings.seeds) {
                break;
            }
            if !seeds.contains(&seed) {
                seeds.push(seed);
            }
        }
        let light_needed = niche.as_ref().map_or(0.0, development::light_needed);
        Ok(Self {
            id: id.to_string(),
            generator,
            niche,
            seeds,
            light_needed,
        })
    }
}

/// One member of a community as its stand's development reads it.
#[derive(Debug, Clone)]
struct StandMember {
    species: usize,
    abundance: f64,
    environments: Vec<Environment>,
    layer: Option<Layer>,
    /// Its candidate on a site that suits it fully.
    candidate: Candidate,
}

impl StandMember {
    fn new(member: &Member, species: &Species, at: usize, community: &str) -> Result<Self, Error> {
        let spec = species.generator.spec();
        let niche_layers = species
            .niche
            .as_ref()
            .map(|niche| niche::Layer::ALL.map(|l| niche.layers.get(&l).copied().unwrap_or(0.0)));
        // A layer the community gives keeps the species to it, at its
        // niche's abundance there (or its most anywhere).
        let layers = match (member.layer, niche_layers) {
            (Some(layer), niche_layers) => {
                let slot = layer_slot(layer);
                let own = niche_layers.map_or(0.0, |layers| layers[slot]);
                let most =
                    niche_layers.map_or(0.0, |layers| layers.into_iter().fold(0.0, f64::max));
                let mut layers = [0.0; 4];
                layers[slot] = if own > 0.0 {
                    own
                } else if most > 0.0 {
                    most
                } else {
                    1.0
                };
                layers
            }
            (None, Some(layers)) => layers,
            (None, None) => {
                return Err(Error(format!(
                    "community {community}: {} has no niche, so give the layer it keeps to",
                    member.species
                )));
            }
        };
        let mut ages = spec.growth.keyframes.clone();
        ages.extend(spec.old_ages());
        ages.retain(|age| *age > 0.0);
        ages.sort_by(f64::total_cmp);
        ages.dedup();
        Ok(Self {
            species: at,
            abundance: member.abundance.unwrap_or(1.0),
            environments: member.environments(),
            layer: member.layer,
            candidate: Candidate {
                shade_tolerance: species
                    .niche
                    .as_ref()
                    .map_or(0.5, |niche| niche.shade_tolerance),
                layers,
                lifespan: spec.growth.lifespan.unwrap_or(spec.growth.years),
                ages,
                suitability: 1.0,
            },
        })
    }

    /// The setting a cohort of it is grown in: a tree in a closed stand
    /// grows as in one ([`Environment::Interior`]), before its stand's
    /// crowns close as in the open; a shrub or herb as in the open. A
    /// member kept to other settings takes the first of them.
    fn environment(&self, stage: Stage) -> Environment {
        let preferred = if self.candidate.tree() && stage != Stage::Establishment {
            Environment::Interior
        } else {
            Environment::Open
        };
        if self.environments.contains(&preferred) {
            preferred
        } else {
            self.environments[0]
        }
    }
}

/// The slot of a layer in [`niche::Layer::ALL`]'s order; ground cover is
/// herbs'.
fn layer_slot(layer: Layer) -> usize {
    match layer {
        Layer::Canopy => 0,
        Layer::Subcanopy => 1,
        Layer::Shrub => 2,
        Layer::Herb | Layer::Ground | Layer::Climber => 3,
    }
}

/// A stand tile's key: its community, stand age, how well the site suits
/// each member (in quarters), cell side, seed and layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    community: usize,
    age: usize,
    suits: Vec<u8>,
    cell_bits: u64,
    seed: u64,
    layout: u64,
}

/// A stand tile laid out: its plants and what its crowns cover.
#[derive(Debug)]
struct Layout {
    plants: Vec<Laid>,
    /// Share of the cell under tree crowns (canopy and subcanopy) seen
    /// from above.
    canopy_cover: f64,
    /// Mean height of its canopy layer, metres.
    canopy_height: f64,
    /// Share of the ground under herbs.
    herb_cover: f64,
}

#[derive(Debug, Clone, Copy)]
struct Laid {
    prototype: u32,
    at: [f64; 2],
    yaw: f64,
    scale: f64,
    lean: [f64; 2],
    layer: niche::Layer,
    /// Its crown's area seen from above, m².
    crown_m2: f64,
}

#[derive(Serialize, Deserialize)]
struct KeptSize {
    generator_revision: u32,
    key: PrototypeKey,
    age_years: f64,
    size: Measurements,
}

impl Fill {
    /// Develop each community's stand at each of the settings' stand ages
    /// and choose the prototypes of their cohorts, each in as many seeds
    /// as the settings ask. Nothing is grown yet; see [`Fill::measure`].
    /// A climber is not placed yet: it needs a host to climb.
    ///
    /// # Errors
    ///
    /// When there are no communities, one does not check or names a
    /// species with no niche and no layer, or the settings ask for no
    /// stand ages or seeds, or ages not above 0 and rising.
    pub fn new(communities: &[Community], settings: &FillSettings) -> Result<Self, Error> {
        if communities.is_empty() {
            return Err(Error("fill: no communities".into()));
        }
        if settings.stand_ages.is_empty() || settings.seeds == 0 {
            return Err(Error("fill: at least one stand age and one seed".into()));
        }
        if settings
            .stand_ages
            .iter()
            .any(|age| !(age.is_finite() && *age > 0.0))
            || settings
                .stand_ages
                .windows(2)
                .any(|pair| pair[1] <= pair[0])
        {
            return Err(Error(format!(
                "fill: stand ages must be above 0 and rise, found {:?}",
                settings.stand_ages
            )));
        }
        let mut species: Vec<Species> = Vec::new();
        let mut stands = Vec::new();
        for community in communities {
            community.check()?;
            let mut members = Vec::new();
            for member in &community.members {
                if member.layer == Some(Layer::Climber) {
                    continue;
                }
                let at = if let Some(at) = species.iter().position(|s| s.id == member.species) {
                    at
                } else {
                    species.push(Species::new(&member.species, settings)?);
                    species.len() - 1
                };
                members.push(StandMember::new(member, &species[at], at, &community.id)?);
            }
            stands.push(members);
        }
        let mut fill = Self {
            communities: communities.to_vec(),
            prototypes: Vec::new(),
            settings: settings.clone(),
            species,
            stands,
            index: HashMap::new(),
            sizes: Arc::new(Vec::new()),
            laid: Arc::new(Mutex::new(HashMap::new())),
        };
        for community in 0..fill.communities.len() {
            for age in &fill.settings.stand_ages {
                let stand = development::develop(&fill.candidates(community, None), *age);
                for cohort in &stand.cohorts {
                    let member = &fill.stands[community][cohort.candidate];
                    for key in fill.cohort_keys(member, cohort, stand.stage) {
                        if fill.index.contains_key(&key) {
                            continue;
                        }
                        let index = u32::try_from(fill.prototypes.len())
                            .map_err(|_| Error("fill: too many prototypes".into()))?;
                        fill.index.insert(key.clone(), index);
                        fill.prototypes.push(Prototype {
                            key,
                            age_years: cohort.age,
                            layer: member.layer,
                        });
                    }
                }
            }
        }
        fill.sizes = Arc::new(
            (0..fill.prototypes.len())
                .map(|_| OnceLock::new())
                .collect(),
        );
        Ok(fill)
    }

    /// The candidates of community `community`'s development, on a site
    /// suiting each member `suits` quarters (fully when not given).
    fn candidates(&self, community: usize, suits: Option<&[u8]>) -> Vec<Candidate> {
        self.stands[community]
            .iter()
            .enumerate()
            .map(|(at, member)| Candidate {
                suitability: suits.map_or(1.0, |suits| f64::from(suits[at]) / 4.0),
                ..member.candidate.clone()
            })
            .collect()
    }

    /// The keys of a cohort's prototypes, one per seed.
    fn cohort_keys(
        &self,
        member: &StandMember,
        cohort: &development::Cohort,
        stage: Stage,
    ) -> Vec<PrototypeKey> {
        let species = &self.species[member.species];
        let environment = member.environment(stage);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let age_tenths = (cohort.age * 10.0).round() as u32;
        species
            .seeds
            .iter()
            .map(|seed| PrototypeKey {
                species: species.id.clone(),
                seed: *seed,
                environment,
                age_tenths,
            })
            .collect()
    }

    #[must_use]
    pub fn communities(&self) -> &[Community] {
        &self.communities
    }

    #[must_use]
    pub fn settings(&self) -> &FillSettings {
        &self.settings
    }

    /// Every prototype; an [`Instance::prototype`] is an index here.
    #[must_use]
    pub fn prototypes(&self) -> &[Prototype] {
        &self.prototypes
    }

    /// Prototype `index`'s size, once [`Fill::measure`] (or
    /// [`Fill::write_prototypes`] or [`Fill::grow_prototype`]) has grown
    /// it.
    #[must_use]
    pub fn size(&self, index: usize) -> Option<&Measurements> {
        self.sizes.get(index).and_then(OnceLock::get)
    }

    fn species_of(&self, prototype: &Prototype) -> &Species {
        self.species
            .iter()
            .find(|species| species.id == prototype.key.species)
            .unwrap_or(&self.species[0])
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
        let generator = self.species_of(prototype).generator.clone();
        let plant = generator.grow_in(
            prototype.key.environment,
            prototype.key.seed,
            prototype.age_years,
        )?;
        let _ = self.sizes[index].set(plant.size.clone());
        Ok((generator, plant))
    }

    /// The prototypes `which` names (every one when empty), sorted.
    fn wanted(&self, which: &[u32]) -> Result<Vec<u32>, Error> {
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
        Ok(wanted)
    }

    /// `indices` in growths: one per species, setting and seed, its ages
    /// rising; the oldest growths first, so the longest start soonest.
    fn growths(&self, indices: &[u32]) -> Vec<Vec<u32>> {
        let mut groups: BTreeMap<(&str, Environment, u64), Vec<u32>> = BTreeMap::new();
        for &index in indices {
            let key = &self.prototypes[index as usize].key;
            groups
                .entry((key.species.as_str(), key.environment, key.seed))
                .or_default()
                .push(index);
        }
        let mut growths: Vec<Vec<u32>> = groups.into_values().collect();
        for growth in &mut growths {
            growth.sort_by(|a, b| {
                self.prototypes[*a as usize]
                    .age_years
                    .total_cmp(&self.prototypes[*b as usize].age_years)
            });
        }
        let oldest = |growth: &Vec<u32>| {
            growth
                .last()
                .map_or(0.0, |index| self.prototypes[*index as usize].age_years)
        };
        growths.sort_by(|a, b| oldest(b).total_cmp(&oldest(a)));
        growths
    }

    /// Grow the prototypes `which` (indices into [`Fill::prototypes`];
    /// every one when empty) that are not measured yet, on up to `threads`
    /// threads, so [`Fill::tile`] can lay their stands out: one growth per
    /// species, setting and seed keeps every age its prototypes need. With
    /// `cache`, a size measured or written into it before at the same
    /// revision is read, not grown again, and each new one is written
    /// there (`<stem>.size.json`), so a city's runs measure each plant
    /// once. How many prototypes it grew.
    ///
    /// # Errors
    ///
    /// When an index is not a prototype, a plant does not grow, or a size
    /// cannot be written.
    pub fn measure(
        &self,
        which: &[u32],
        threads: usize,
        cache: Option<&Path>,
    ) -> Result<usize, Error> {
        let mut unknown = Vec::new();
        for index in self.wanted(which)? {
            if self.size(index as usize).is_some() {
                continue;
            }
            if let Some(size) = cache.and_then(|dir| self.cached_size(index, dir)) {
                let _ = self.sizes[index as usize].set(size);
                continue;
            }
            unknown.push(index);
        }
        if let Some(dir) = cache {
            std::fs::create_dir_all(dir)
                .map_err(|error| Error(format!("cannot create {}: {error}", dir.display())))?;
        }
        let growths = self.growths(&unknown);
        run_jobs(growths.len(), threads, |job| {
            let indices = &growths[job];
            let plants = self.grow_together(indices)?;
            for (index, plant) in indices.iter().zip(plants) {
                if let Some(dir) = cache {
                    let prototype = &self.prototypes[*index as usize];
                    let kept = KeptSize {
                        generator_revision: crate::GENERATOR_REVISION,
                        key: prototype.key.clone(),
                        age_years: prototype.age_years,
                        size: plant.size.clone(),
                    };
                    let path = dir.join(format!("{}.size.json", prototype.key.stem()));
                    let text = serde_json::to_string_pretty(&kept)
                        .map_err(|error| Error(error.to_string()))?;
                    std::fs::write(&path, text + "\n").map_err(|error| {
                        Error(format!("cannot write {}: {error}", path.display()))
                    })?;
                }
                let _ = self.sizes[*index as usize].set(plant.size);
            }
            Ok(())
        })?;
        Ok(unknown.len())
    }

    /// The prototypes `indices` of one species, setting and seed, ages
    /// rising, grown together.
    fn grow_together(&self, indices: &[u32]) -> Result<Vec<Plant>, Error> {
        let Some(first) = indices.first() else {
            return Ok(Vec::new());
        };
        let prototype = &self.prototypes[*first as usize];
        let ages: Vec<f64> = indices
            .iter()
            .map(|index| self.prototypes[*index as usize].age_years)
            .collect();
        self.species_of(prototype).generator.grow_ages_in(
            prototype.key.environment,
            prototype.key.seed,
            &ages,
        )
    }

    /// Prototype `index`'s size as a measure or a write left it in `dir`
    /// at this revision.
    fn cached_size(&self, index: u32, dir: &Path) -> Option<Measurements> {
        let prototype = &self.prototypes[index as usize];
        let stem = prototype.key.stem();
        let same = |key: &PrototypeKey, age: f64, revision: u32| {
            revision == crate::GENERATOR_REVISION
                && *key == prototype.key
                && (age - prototype.age_years).abs() < 1e-9
        };
        let read = |name: String| std::fs::read_to_string(dir.join(name)).ok();
        if let Some(kept) = read(format!("{stem}.size.json"))
            .and_then(|text| serde_json::from_str::<KeptSize>(&text).ok())
            .filter(|kept| same(&kept.key, kept.age_years, kept.generator_revision))
        {
            return Some(kept.size);
        }
        ["", "-parts"].into_iter().find_map(|parts| {
            read(format!("{stem}{parts}.prototype.json"))
                .and_then(|text| serde_json::from_str::<KeptPrototype>(&text).ok())
                .filter(|kept| {
                    same(
                        &kept.files.key,
                        kept.files.age_years,
                        kept.generator_revision,
                    )
                })
                .map(|kept| kept.files.plant.size)
        })
    }

    /// Grow and export the prototypes `which` (indices into
    /// [`Fill::prototypes`]; every one when empty) into `out`, on up to
    /// `threads` threads, and list them in [`PROTOTYPES_FILE`]; each is
    /// measured as it is written. A prototype whose record
    /// (`<stem>.prototype.json`) is in `out` from the same revision is
    /// kept, not drawn again, so a city's tiles can ask for theirs as they
    /// come.
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
        let wanted = self.wanted(which)?;
        std::fs::create_dir_all(out)
            .map_err(|error| Error(format!("cannot create {}: {error}", out.display())))?;
        let parts = self.settings.parts;
        let growths = self.growths(&wanted);
        let written = run_jobs(growths.len(), threads, |job| {
            let indices = &growths[job];
            let mut files = Vec::with_capacity(indices.len());
            let mut fresh = Vec::new();
            for &index in indices {
                match self.kept_prototype(index, parts, out) {
                    Some(kept) => files.push(kept),
                    None => fresh.push(index),
                }
            }
            let plants = self.grow_together(&fresh)?;
            for (index, plant) in fresh.into_iter().zip(plants) {
                let prototype = &self.prototypes[index as usize];
                let generator = &self.species_of(prototype).generator;
                let (record, part_records) = generator.write_plant(&plant, settings, out, parts)?;
                let written = PrototypeFiles {
                    index,
                    key: prototype.key.clone(),
                    age_years: prototype.age_years,
                    layer: prototype.layer,
                    plant: record,
                    parts: part_records,
                };
                let kept = KeptPrototype {
                    generator_revision: crate::GENERATOR_REVISION,
                    files: written.clone(),
                };
                let record = out.join(record_name(&prototype.key, parts));
                let text = serde_json::to_string_pretty(&kept)
                    .map_err(|error| Error(error.to_string()))?;
                std::fs::write(&record, text + "\n").map_err(|error| {
                    Error(format!("cannot write {}: {error}", record.display()))
                })?;
                files.push(written);
            }
            for written in &files {
                let _ = self.sizes[written.index as usize].set(written.plant.size.clone());
            }
            Ok(files)
        })?;
        let mut written: Vec<PrototypeFiles> = written.into_iter().flatten().collect();
        written.sort_by_key(|files| files.index);
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

    /// Prototype `index` as a write left it in `out` at this revision,
    /// every file still there.
    fn kept_prototype(&self, index: u32, parts: bool, out: &Path) -> Option<PrototypeFiles> {
        let prototype = &self.prototypes[index as usize];
        let record = out.join(record_name(&prototype.key, parts));
        std::fs::read_to_string(&record)
            .ok()
            .and_then(|text| serde_json::from_str::<KeptPrototype>(&text).ok())
            .filter(|kept| {
                kept.generator_revision == crate::GENERATOR_REVISION
                    && kept.files.key == prototype.key
                    && (kept.files.age_years - prototype.age_years).abs() < 1e-9
                    && kept
                        .files
                        .plant
                        .levels
                        .iter()
                        .chain(kept.files.parts.iter().flat_map(|part| &part.levels))
                        .all(|level| out.join(&level.file).is_file())
            })
            .map(|kept| PrototypeFiles {
                index,
                ..kept.files
            })
    }

    /// Fill one tile: its instances, ground cover and far canopy. Each
    /// stand cell is drawn from its community's stand at the stand age
    /// nearest its years since disturbance, on the site factors at its
    /// middle ([`Site::site_factors_at`]), and keeps each plant with the
    /// share [`Site::plantable_at`] gives where it stands.
    ///
    /// # Errors
    ///
    /// When the stand map does not cover the site or names a community
    /// the fill does not have, or a prototype a stand needs has not been
    /// measured ([`Fill::measure`]).
    #[allow(clippy::too_many_lines)]
    pub fn tile(&self, site: &Site, stands: &StandMap, seed: u64) -> Result<TilePlan, Error> {
        stands.check(site, self.communities.len())?;
        let cell_m = stands.cell_m;
        let mut instances = Vec::new();
        let mut cover = Vec::with_capacity(stands.cells.len());
        let mut canopy = Vec::with_capacity(stands.cells.len());
        let (mut trees, mut basal, mut leaf_m2, mut crowns) = (0_usize, 0.0, 0.0, 0.0);
        for (index, cell) in stands.cells.iter().enumerate() {
            let (col, row) = (index % stands.cols, index / stands.cols);
            let world = world_cell(site, stands, col, row);
            let Some(community) = cell.community.map(usize::from) else {
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
            #[allow(clippy::cast_precision_loss)]
            let corner = [col as f64 * cell_m, row as f64 * cell_m];
            let factors = site.site_factors_at(corner[0] + 0.5 * cell_m, corner[1] + 0.5 * cell_m);
            let suits = self.stands[community]
                .iter()
                .map(|member| {
                    let suits = self.species[member.species]
                        .niche
                        .as_ref()
                        .map_or(1.0, |niche| niche.site_response(factors));
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let quarters = (suits.clamp(0.0, 1.0) * 4.0).round() as u8;
                    quarters
                })
                .collect();
            let draw = |salt: u64| rng::hash_words(&[seed, world[0], world[1], salt]);
            let layout = self.layout(LayoutKey {
                community,
                age: nearest_age(
                    &self.settings.stand_ages,
                    f64::from(cell.years_since_disturbance),
                ),
                suits,
                cell_bits: cell_m.to_bits(),
                seed,
                layout: draw(1) % LAYOUTS,
            })?;
            // The cell's own turn and shift of the wrapping stand tile.
            let turn = draw(2) % 4;
            let shift = [rng::unit(draw(3)) * cell_m, rng::unit(draw(4)) * cell_m];
            let keep = draw(5);
            let (mut tree_crowns, mut kept_tree_crowns) = (0.0, 0.0);
            let (mut herbs, mut kept_herbs) = (0_usize, 0_usize);
            for (n, plant) in layout.plants.iter().enumerate() {
                let tree = matches!(plant.layer, niche::Layer::Canopy | niche::Layer::Subcanopy);
                let herb = plant.layer == niche::Layer::Herb;
                if tree {
                    tree_crowns += plant.crown_m2;
                }
                herbs += usize::from(herb);
                let at = turned(
                    [
                        wrap(plant.at[0] + shift[0], cell_m),
                        wrap(plant.at[1] + shift[1], cell_m),
                    ],
                    turn,
                    cell_m,
                );
                let (east, north) = (corner[0] + at[0], corner[1] + at[1]);
                let pick = |salt: u64| rng::unit(rng::hash_words(&[keep, n as u64, salt]));
                if pick(1) >= site.plantable_at(east, north) {
                    continue;
                }
                if tree {
                    kept_tree_crowns += plant.crown_m2;
                }
                kept_herbs += usize::from(herb);
                let lean = turned_vector(plant.lean, turn);
                #[allow(clippy::cast_precision_loss)]
                let yaw = plant.yaw + turn as f64 * std::f64::consts::FRAC_PI_2;
                let prototype = &self.prototypes[plant.prototype as usize];
                if let Some(size) = self.size(plant.prototype as usize) {
                    let dbh = size.dbh.unwrap_or(0.0) * plant.scale;
                    if dbh >= 0.05 {
                        trees += 1;
                        basal += std::f64::consts::PI * 0.25 * dbh * dbh;
                    }
                    leaf_m2 += size.leaf_area * plant.scale * plant.scale;
                }
                #[allow(clippy::cast_possible_truncation)]
                instances.push(Instance {
                    prototype: plant.prototype,
                    position: [
                        east as f32,
                        north as f32,
                        site.height_at(east, north) as f32,
                    ],
                    rotation: Instance::lean(yaw, lean),
                    scale: plant.scale as f32,
                    age_years: prototype.age_years as f32,
                    tint: [
                        (pick(2) * 2.0 - 1.0) as f32 * 0.2,
                        (pick(3) * 2.0 - 1.0) as f32 * 0.1,
                    ],
                    extra: 0,
                });
            }
            let kept_share = |kept: f64, all: f64| if all > 0.0 { kept / all } else { 1.0 };
            let canopy_cover = layout.canopy_cover * kept_share(kept_tree_crowns, tree_crowns);
            #[allow(clippy::cast_precision_loss)]
            let herb_cover = layout.herb_cover * kept_share(kept_herbs as f64, herbs as f64);
            crowns += canopy_cover * cell_m * cell_m;
            #[allow(clippy::cast_possible_truncation)]
            cover.push(if herb_cover > 0.0 {
                Cover {
                    kind: CoverKind::Herb,
                    fraction: herb_cover as f32,
                }
            } else if canopy_cover > 0.0 {
                Cover {
                    kind: CoverKind::Litter,
                    fraction: canopy_cover as f32,
                }
            } else {
                Cover {
                    kind: CoverKind::Bare,
                    fraction: 0.0,
                }
            });
            #[allow(clippy::cast_possible_truncation)]
            canopy.push(CanopyCell {
                height_m: layout.canopy_height as f32,
                cover: canopy_cover as f32,
                colour: CANOPY_COLOUR,
            });
        }
        let area_m2 = site.tile_m * site.tile_m;
        let area_ha = area_m2 / 10_000.0;
        #[allow(clippy::cast_precision_loss)]
        let stats = TileStats {
            instances: instances.len(),
            stems_per_ha: instances.len() as f64 / area_ha,
            trees_per_ha: trees as f64 / area_ha,
            basal_area_m2_per_ha: basal / area_ha,
            canopy_cover: crowns / area_m2,
            leaf_area_index: leaf_m2 / area_m2,
        };
        Ok(TilePlan {
            tile: site.tile,
            tile_m: site.tile_m,
            cell_m,
            cols: stands.cols,
            stats,
            instances,
            cover,
            canopy,
        })
    }

    /// The stand tile `key` names, laid out once.
    fn layout(&self, key: LayoutKey) -> Result<Arc<Layout>, Error> {
        if let Some(found) = self
            .laid
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(found));
        }
        // Laid out outside the lock: two threads that meet here make the
        // same tile, and the first kept serves both.
        let made = Arc::new(self.lay_out(&key)?);
        Ok(Arc::clone(
            self.laid
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key)
                .or_insert(made),
        ))
    }

    /// Lay out the stand tile `key` names: its community's stand developed
    /// at its age on its site, each cohort a member in each of its seeds,
    /// in the layer its measured height puts it.
    #[allow(clippy::too_many_lines)]
    fn lay_out(&self, key: &LayoutKey) -> Result<Layout, Error> {
        let candidates = self.candidates(key.community, Some(&key.suits));
        let stand = development::develop(&candidates, self.settings.stand_ages[key.age]);
        let mut chosen = Vec::new();
        for cohort in &stand.cohorts {
            let member = &self.stands[key.community][cohort.candidate];
            let keys = self.cohort_keys(member, cohort, stand.stage);
            #[allow(clippy::cast_precision_loss)]
            let share = cohort.weight * member.abundance / keys.len().max(1) as f64;
            for prototype_key in keys {
                let index = self.index.get(&prototype_key).copied().ok_or_else(|| {
                    Error(format!(
                        "fill: no prototype {}; the stand needs one Fill::new did not choose",
                        prototype_key.stem()
                    ))
                })?;
                let size = self.size(index as usize).ok_or_else(|| {
                    Error(format!(
                        "fill: prototype {index} ({}) is not measured yet; call Fill::measure first",
                        prototype_key.stem()
                    ))
                })?;
                chosen.push((index, cohort.candidate, share, size, member.species));
            }
        }
        let top = chosen
            .iter()
            .filter(|(_, candidate, ..)| candidates[*candidate].tree())
            .map(|(.., size, _)| size.height)
            .fold(0.0_f64, f64::max);
        let mut has_herbs = false;
        let members: Vec<stand_tile::Member> = chosen
            .iter()
            .map(
                |(index, candidate, share, size, species)| stand_tile::Member {
                    id: *index,
                    layer: development::layer_of(&candidates[*candidate], size.height, top),
                    share: *share,
                    height_m: size.height,
                    crown_base_m: size.crown_base.unwrap_or(0.0),
                    crown_radius_m: size.crown_radius,
                    light_needed: self.species[*species].light_needed,
                },
            )
            .filter(|member| {
                let herb = member.layer == niche::Layer::Herb;
                has_herbs |= herb;
                self.settings.herbs || !herb
            })
            .collect();
        let tile_m = f64::from_bits(key.cell_bits);
        let rules = stand_tile::Rules {
            tile_m,
            cover: stand.cover,
            scale: STAND_SCALE,
            ..stand_tile::Rules::default()
        };
        let placed = stand_tile::lay_out(
            &members,
            &rules,
            rng::hash_words(&[key.seed, key.layout, key.community as u64, key.age as u64]),
        );
        let in_layers = |layers: &[niche::Layer]| -> Vec<stand_tile::Placed> {
            placed
                .iter()
                .filter(|plant| layers.contains(&members[plant.member].layer))
                .copied()
                .collect()
        };
        let canopy_cover = stand_tile::cover(
            &members,
            &in_layers(&[niche::Layer::Canopy, niche::Layer::Subcanopy]),
            tile_m,
        );
        let herb_cover = if !has_herbs {
            0.0
        } else if self.settings.herbs {
            stand_tile::cover(&members, &in_layers(&[niche::Layer::Herb]), tile_m)
        } else {
            stand.cover[3]
        };
        let (mut heights, mut count) = (0.0, 0.0);
        let plants = placed
            .iter()
            .map(|plant| {
                let member = &members[plant.member];
                if member.layer == niche::Layer::Canopy {
                    heights += member.height_m * plant.scale;
                    count += 1.0;
                }
                // Leant so its crown's middle stands where the layout put
                // its crown.
                let middle = f64::midpoint(member.crown_base_m, member.height_m) * plant.scale;
                let shift = plant.crown_offset(tile_m);
                let lean = if middle > 0.5 {
                    [shift[0] / middle, shift[1] / middle]
                } else {
                    [0.0, 0.0]
                };
                let radius = member.crown_radius_m * plant.scale;
                Laid {
                    prototype: member.id,
                    at: plant.at_m,
                    yaw: plant.yaw_deg.to_radians(),
                    scale: plant.scale,
                    lean,
                    layer: member.layer,
                    crown_m2: std::f64::consts::PI * radius * radius,
                }
            })
            .collect();
        Ok(Layout {
            plants,
            canopy_cover,
            canopy_height: if count > 0.0 { heights / count } else { 0.0 },
            herb_cover,
        })
    }
}

/// The far canopy's colour seen from above, linear RGB: a dark conifer
/// green until each species gives its own.
const CANOPY_COLOUR: [f32; 3] = [0.08, 0.12, 0.06];

/// The record a written prototype keeps beside its files.
fn record_name(key: &PrototypeKey, parts: bool) -> String {
    format!(
        "{}{}.prototype.json",
        key.stem(),
        if parts { "-parts" } else { "" }
    )
}

/// Run `count` jobs on up to `threads` threads, each job once; their
/// results in order, or the first failure.
fn run_jobs<T: Send>(
    count: usize,
    threads: usize,
    job: impl Fn(usize) -> Result<T, Error> + Sync,
) -> Result<Vec<T>, Error> {
    let next = Mutex::new(0_usize);
    let done: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    let failure = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, count.max(1)) {
            scope.spawn(|| {
                loop {
                    let slot = {
                        let mut next = next.lock().unwrap_or_else(PoisonError::into_inner);
                        if *next >= count
                            || failure
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .is_some()
                        {
                            return;
                        }
                        *next += 1;
                        *next - 1
                    };
                    match job(slot) {
                        Ok(result) => {
                            done.lock().unwrap_or_else(PoisonError::into_inner)[slot] =
                                Some(result);
                        }
                        Err(error) => {
                            failure
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .get_or_insert(error);
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().unwrap_or_else(PoisonError::into_inner) {
        return Err(error);
    }
    Ok(done
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .flatten()
        .collect())
}

#[derive(Serialize, Deserialize)]
struct KeptPrototype {
    generator_revision: u32,
    files: PrototypeFiles,
}

/// The index of the stand age in `ages` nearest `years`, by ratio; the
/// youngest for a stand not yet a year old.
fn nearest_age(ages: &[f64], years: f64) -> usize {
    if years.is_nan() || years < 1.0 {
        return 0;
    }
    (0..ages.len())
        .min_by(|a, b| {
            libm::fabs(libm::log(ages[*a] / years))
                .total_cmp(&libm::fabs(libm::log(ages[*b] / years)))
        })
        .unwrap_or(0)
}

/// `value` wrapped into `[0, side)`.
fn wrap(value: f64, side: f64) -> f64 {
    let wrapped = value.rem_euclid(side);
    if wrapped >= side { 0.0 } else { wrapped }
}

/// A point of a square of `side` turned `turns` quarter turns
/// anticlockwise about its middle.
fn turned(at: [f64; 2], turns: u64, side: f64) -> [f64; 2] {
    let [x, y] = at;
    let [x, y] = match turns % 4 {
        0 => [x, y],
        1 => [side - y, x],
        2 => [side - x, side - y],
        _ => [y, side - x],
    };
    [wrap(x, side), wrap(y, side)]
}

/// A vector turned `turns` quarter turns anticlockwise.
fn turned_vector(v: [f64; 2], turns: u64) -> [f64; 2] {
    match turns % 4 {
        0 => v,
        1 => [-v[1], v[0]],
        2 => [-v[0], -v[1]],
        _ => [v[1], -v[0]],
    }
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
    fn a_stand_cell_takes_the_stand_age_nearest_by_ratio() {
        let ages = STAND_AGES;
        assert_eq!(nearest_age(&ages, 0.0), 0);
        assert_eq!(nearest_age(&ages, 27.0), 1);
        assert_eq!(nearest_age(&ages, 30.0), 2);
        assert_eq!(nearest_age(&ages, 500.0), 6);
        assert_eq!(nearest_age(&ages, 5_000.0), 6);
    }

    #[test]
    fn a_quarter_turn_keeps_a_point_in_its_square_and_turns_its_lean_alike() {
        let side = 40.0;
        for turns in 0..4 {
            for at in [[0.0, 0.0], [10.0, 3.0], [39.99, 20.0]] {
                let point = super::turned(at, turns, side);
                assert!(point.iter().all(|v| (0.0..side).contains(v)), "{point:?}");
            }
        }
        // A point east of the middle turns north of it, as east turns north.
        let point = super::turned([30.0, 20.0], 1, side);
        assert!((point[0] - 20.0).abs() < 1e-9 && (point[1] - 30.0).abs() < 1e-9);
        assert_eq!(turned_vector([1.0, 0.0], 1), [-0.0, 1.0]);
        assert_eq!(wrap(-1e-20, side), 0.0);
    }

    #[test]
    fn a_prototype_stem_names_its_age_in_years() {
        let key = PrototypeKey {
            species: "x".into(),
            seed: 2,
            environment: Environment::Interior,
            age_tenths: 305,
        };
        assert_eq!(key.stem(), "x-interior-2-30.5y");
        let whole = PrototypeKey {
            age_tenths: 4800,
            ..key
        };
        assert_eq!(whole.stem(), "x-interior-2-480y");
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
