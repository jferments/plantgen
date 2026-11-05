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
use crate::lsys::tools::SoilNeed;
use crate::niche::{self, Niche, SiteFactors};
use crate::package::Measurements;
use crate::rng;
use crate::site_field::SiteField;
use crate::spec::{Environment, PlantSpec};
use crate::stand_tile;

/// The version of the instance file ([`Instance::to_bytes`]) and of the
/// tile JSON beside it.
///
/// 3: a climber's `extra` is its [`WallClip`] (its wall's top and both
/// ends), no longer its wall's top alone in decimetres.
pub const INSTANCE_SCHEMA: u32 = 3;

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

/// A neighbouring stand whose canopy stands under this share of a cell's
/// own height opens an edge beside it, as a clearcut or a young stand
/// does beside an old one. Chen, Franklin and Spies (1992, Ecological
/// Applications 2: 387-396) measured old-growth Douglas-fir edges beside
/// clearcuts of 10-15 years; the share is this model's choice.
pub const EDGE_HEIGHT_SHARE: f64 = 0.5;

/// How far an edge tree's yaw strays either side of square to the
/// opening, radians: its crown is fullest toward the light, not ruled
/// along the edge. This model's choice.
const EDGE_YAW_SPREAD: f64 = 0.35;

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
    /// Whether a stand's first row of trees beside open ground grows as
    /// an edge ([`Environment::Edge`]), its crown free toward the
    /// opening. Open ground is a bare cell or a stand whose canopy stands
    /// under half as tall ([`EDGE_HEIGHT_SHARE`]).
    pub edges: bool,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            stand_ages: STAND_AGES.to_vec(),
            seeds: 2,
            options: Options::default(),
            parts: true,
            herbs: true,
            edges: true,
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
    /// Whether it is a climber grown at the foot of a wall
    /// ([`Generator::grow_ages_on_wall`]), not in its environment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_wall: bool,
    /// The height in whole metres of the closed canopy it came up beneath,
    /// for a shade-tolerant tree that established under its stand's
    /// crowns (ingrowth), grown with [`Generator::grow_ages_beneath`]; its
    /// `environment` is then [`Environment::Suppressed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beneath_m: Option<u16>,
}

impl PrototypeKey {
    /// The common name of its files: `<species>-<setting>-<seed>-<age>y`,
    /// as [`Generator::export`] names a plant; a climber's setting on a
    /// wall is `wall`, and a tree's come up beneath a canopy 60 m tall
    /// `beneath60m`.
    #[must_use]
    pub fn stem(&self) -> String {
        let age = if self.age_tenths.is_multiple_of(10) {
            format!("{}", self.age_tenths / 10)
        } else {
            format!("{}.{}", self.age_tenths / 10, self.age_tenths % 10)
        };
        let setting = if self.on_wall {
            "wall".to_string()
        } else if let Some(canopy) = self.beneath_m {
            format!("beneath{canopy}m")
        } else {
            self.environment.name().to_string()
        };
        format!("{}-{setting}-{}-{age}y", self.species, self.seed)
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

/// The package that holds one growth of a fill's prototypes
/// ([`Fill::packages`]): a species in one setting from one seed, kept at
/// each of its prototypes' ages. A host that draws packages (After)
/// builds it from `spec` as it builds any species' package
/// ([`crate::package::Inputs::new`], with motion or card tiers as it
/// likes), and draws prototype `prototypes[k].0` as the package's only
/// variant at keyframe `prototypes[k].1`: the same plant, of the same
/// size, as [`Fill::measure`] grows.
#[derive(Debug, Clone)]
pub struct PrototypePackage {
    /// The species' spec, with one variant (the growth's setting and
    /// seed) and a keyframe at each of its prototypes' ages, grown to the
    /// oldest of them.
    pub spec: PlantSpec,
    /// Each prototype it holds ([`Fill::prototypes`]) and its keyframe in
    /// the package, ages rising.
    pub prototypes: Vec<(u32, usize)>,
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
    /// Plants that came up by themselves in the tile's pavements
    /// ([`Site::with_colonists`]), drawn after its stands.
    pub colonists: Vec<CrackPlant>,
}

/// A plant that came up by itself in a crack or a hole of a pavement
/// ([`crate::succession`]), as a fill draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct CrackPlant {
    /// Its species' id, one of the fill's species.
    pub species: String,
    /// Metres from the tile's south-west corner.
    pub east: f64,
    pub north: f64,
    pub age_years: f64,
    /// Its height and crown radius, as its crack's soil has let it grow.
    pub height_m: f64,
    pub crown_m: f64,
}

/// A field tile's roof patches that hold soil, by the field cell under
/// them ([`Site::roofs`]).
#[derive(Debug, Clone, Default)]
pub struct Roofs {
    cells: HashMap<u32, u32>,
}

impl Roofs {
    /// The roof soil at `(east, north)` metres from `site`'s corner, if a
    /// roof patch holding soil lies over it.
    #[must_use]
    pub fn at(&self, site: &Site, east: f64, north: f64) -> Option<RoofSoil> {
        let tile = site.field.as_ref()?;
        let (x, z) = (tile.corner[0] + east, tile.corner[1] - north);
        let cell = tile.field.cell_at(x, z)?;
        let patch = &tile.field.patches[*self.cells.get(&cell)? as usize];
        let [nx, ny, nz] = patch.normal;
        // On the patch's plane.
        let height_m =
            patch.centre[1] - (nx * (x - patch.centre[0]) + nz * (z - patch.centre[2])) / ny;
        Some(RoofSoil {
            height_m,
            soil_m: patch.soil_m,
        })
    }
}

/// Soil gathered on a roof where a plant stands ([`Roofs::at`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoofSoil {
    /// The roof's height there, in the field's frame.
    pub height_m: f64,
    /// How deep its soil lies.
    pub soil_m: f64,
}

impl RoofSoil {
    /// How far the roof's soil carries a plant `crown_m` in crown radius
    /// and `height_m` tall whose roots ask `need`, 0 to 1, as
    /// [`Site::soil_ratio_at`] reads the ground: an even layer `soil_m`
    /// deep all round its foot.
    #[must_use]
    pub fn soil_ratio(&self, need: &SoilNeed, crown_m: f64, height_m: f64) -> f64 {
        need.ratio(crown_m, height_m, |reach, down| {
            Some(std::f64::consts::PI * reach * reach * down.min(self.soil_m))
        })
    }
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
            colonists: Vec::new(),
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

    /// This field's tile with the plants that come up by themselves in
    /// its pavements after `years` untended: [`crate::succession::run`]
    /// over `microsites` (the field's site mesh's,
    /// [`crate::succession::microsites`]) with the species of `pool` and
    /// the seed of `sources`, keeping those that stand inside the tile.
    /// Each is drawn as its species' prototype nearest its age, scaled to
    /// its height ([`Fill::tile`]), so the fill must hold every species of
    /// `pool` that comes up.
    ///
    /// # Errors
    ///
    /// When the site is not a field's tile.
    pub fn with_colonists(
        mut self,
        microsites: &[crate::succession::Microsite],
        pool: &[crate::succession::Colonist],
        sources: &[crate::succession::Source],
        settings: &crate::succession::Settings,
        years: u32,
    ) -> Result<Self, Error> {
        let Some(tile) = &self.field else {
            return Err(Error(
                "site: only a site field's tile has pavements for plants to come up in".into(),
            ));
        };
        let corner = tile.corner;
        let snapshots = crate::succession::run(microsites, pool, sources, settings, &[years]);
        let Some(snapshot) = snapshots.into_iter().find(|s| s.year == years) else {
            return Ok(self);
        };
        self.colonists = snapshot
            .plants
            .iter()
            .filter_map(|plant| {
                let site = &microsites[plant.microsite];
                let (east, north) = (site.x - corner[0], corner[1] - site.z);
                let inside =
                    (0.0..self.tile_m).contains(&east) && (0.0..self.tile_m).contains(&north);
                (inside && plant.height > 0.0).then(|| CrackPlant {
                    species: pool[plant.colonist].id.clone(),
                    east,
                    north,
                    age_years: plant.age,
                    height_m: plant.height,
                    crown_m: plant.crown,
                })
            })
            .collect();
        Ok(self)
    }

    /// The rootable soil under `(east, north)`, metres deep: the field
    /// cell's [`crate::site_field::Ground::rooting_m`]; `None` where the
    /// soil goes deeper than roots do, on a height-field site, or where the
    /// cell has no ground or the field does not reach.
    #[must_use]
    pub fn rooting_at(&self, east: f64, north: f64) -> Option<f64> {
        let tile = self.field.as_ref()?;
        let cell = tile.cell(east, north)? as usize;
        tile.field.ground.height[cell]?;
        tile.field.ground.rooting_m[cell]
    }

    /// How far the soil round `(east, north)` carries a plant `crown_m`
    /// in crown radius and `height_m` tall whose roots ask `need` of it,
    /// 0 to 1 ([`SoilNeed::ratio`], as `soil@1` reads it each year). The
    /// rootable volume within a reach of the foot is summed over the
    /// field's cells whose centres lie within it: each cell's area down to
    /// its [`Site::rooting_at`] depth (all the way where nothing stops the
    /// roots), times its open share, so a tree in a pit in sound pavement
    /// has its pit's soil and no more, and nothing where a cell has no
    /// ground. A plant drawn there has its lengths times the square root
    /// of this, as `soil@1` grows them. 1 on a height-field site or a
    /// plant whose program asks nothing of the soil.
    #[must_use]
    pub fn soil_ratio_at(
        &self,
        east: f64,
        north: f64,
        need: &SoilNeed,
        crown_m: f64,
        height_m: f64,
    ) -> f64 {
        let Some(tile) = &self.field else {
            return 1.0;
        };
        let field = &tile.field;
        let cell_m = field.settings.cell_m;
        let (x, z) = (tile.corner[0] + east, tile.corner[1] - north);
        need.ratio(crown_m, height_m, |reach, down| {
            let rings = (reach / cell_m).ceil() + 1.0;
            let mut volume = 0.0;
            let mut seen = 0_usize;
            #[allow(clippy::cast_possible_truncation)]
            let span = rings as i64;
            for dz in -span..=span {
                for dx in -span..=span {
                    #[allow(clippy::cast_precision_loss)]
                    let (px, pz) = (x + dx as f64 * cell_m, z + dz as f64 * cell_m);
                    let Some(cell) = field.cell_at(px, pz) else {
                        continue;
                    };
                    let centre = field.cell_centre(cell);
                    if (centre[0] - x).hypot(centre[1] - z) > reach.max(0.5 * cell_m) {
                        continue;
                    }
                    seen += 1;
                    let index = cell as usize;
                    if field.ground.height[index].is_none() {
                        continue;
                    }
                    let depth = field.ground.rooting_m[index].map_or(down, |d| d.clamp(0.0, down));
                    volume +=
                        cell_m * cell_m * depth * field.ground.open_share[index].clamp(0.0, 1.0);
                }
            }
            // Beyond the field's edge, nothing is known: no limit.
            (seen > 0).then(|| {
                // The cells stand in for the disc they cover.
                #[allow(clippy::cast_precision_loss)]
                let covered = seen as f64 * cell_m * cell_m;
                let disc = std::f64::consts::PI * reach * reach;
                volume * (disc / covered).min(1.0)
            })
        })
    }

    /// The patches of the field's roofs that hold soil (`soil_m` above
    /// 0, facing up, not kept clear) by the field cell under them, the
    /// highest where two overlap; none on a height-field site.
    #[must_use]
    pub fn roofs(&self) -> Roofs {
        let mut cells = HashMap::new();
        let Some(tile) = &self.field else {
            return Roofs { cells };
        };
        let field = &tile.field;
        let cell_m = field.settings.cell_m;
        for (index, patch) in field.patches.iter().enumerate() {
            let roof = field
                .hosts
                .get(patch.host as usize)
                .is_some_and(|host| host.kind == crate::site_field::HostKind::Roof);
            if !roof || patch.soil_m <= 0.0 || patch.clear || patch.normal[1] < 0.3 {
                continue;
            }
            // The cells whose centres lie under the patch, seen from above.
            let half = 0.5 * (patch.area * patch.normal[1]).sqrt();
            let rings = (half / cell_m).ceil();
            #[allow(clippy::cast_possible_truncation)]
            let span = rings as i64;
            for dz in -span..=span {
                for dx in -span..=span {
                    #[allow(clippy::cast_precision_loss)]
                    let (x, z) = (
                        patch.centre[0] + dx as f64 * cell_m,
                        patch.centre[2] + dz as f64 * cell_m,
                    );
                    let Some(cell) = field.cell_at(x, z) else {
                        continue;
                    };
                    let centre = field.cell_centre(cell);
                    if (centre[0] - patch.centre[0]).abs() > half
                        || (centre[1] - patch.centre[2]).abs() > half
                    {
                        continue;
                    }
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    cells
                        .entry(cell)
                        .and_modify(|kept: &mut u32| {
                            if field.patches[*kept as usize].centre[1] < patch.centre[1] {
                                *kept = index;
                            }
                        })
                        .or_insert(index);
                }
            }
        }
        Roofs { cells }
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
        if let Some(tile) = &self.field
            && let Some(cell) = tile.cell(east, north)
        {
            let moisture = moisture_from_wetness(tile.field.ground.wetness[cell as usize]);
            if let Some(factors) = tile
                .field
                .site_factors(cell, self.factors.elevation_m, moisture)
            {
                return factors;
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
        if let Some(tile) = &self.field
            && let Some(height) = tile
                .field
                .height_at(tile.corner[0] + east, tile.corner[1] - north)
        {
            return height;
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
/// year), i8 x2 tint (lightness, hue), u16 extra (a climber's cut back to
/// its wall, [`WallClip`], otherwise 0); little-endian.
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

/// Where a renderer cuts a climber back to the wall it stands at, packed
/// into its instance's [`Instance::extra`]. Its prototype grew on a wall
/// wider and taller than most it is set at ([`crate::export::grow_on_wall`]),
/// so drawn whole it would hang in the air past the wall's ends and top.
/// In the prototype's frame (growth's: +y up, its wall along x): nothing
/// above `top_m`, nor further than `back_m` along -x or `ahead_m` along +x
/// from its foot, is drawn. Packed as bits 0-5 the top in tenths of a
/// metre (1 to 63, 63 for no top), bits 6-10 and 11-15 back and ahead in
/// fifths of a metre (0 to 31, 31 for no end); each is rounded down, so
/// the cut falls on the wall, never past it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallClip {
    pub top_m: f32,
    pub back_m: f32,
    pub ahead_m: f32,
}

impl WallClip {
    const TOP_STEP_M: f32 = 0.1;
    const SIDE_STEP_M: f32 = 0.2;

    /// The packed form, never 0 (an instance that is not clipped).
    #[must_use]
    pub fn to_extra(self) -> u16 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let code = |metres: f32, step: f32, most: u16| {
            if metres.is_finite() {
                ((metres / step + 1.0e-3).floor().max(0.0) as u16).min(most)
            } else {
                most
            }
        };
        code(self.top_m, Self::TOP_STEP_M, 63).max(1)
            | code(self.back_m, Self::SIDE_STEP_M, 31) << 6
            | code(self.ahead_m, Self::SIDE_STEP_M, 31) << 11
    }

    /// [`Self::to_extra`] read back: none for 0; an end at its largest
    /// code is infinite.
    #[must_use]
    pub fn from_extra(extra: u16) -> Option<Self> {
        if extra == 0 {
            return None;
        }
        let read = |code: u16, step: f32, most: u16| {
            if code >= most {
                f32::INFINITY
            } else {
                f32::from(code) * step
            }
        };
        Some(Self {
            top_m: read(extra & 63, Self::TOP_STEP_M, 63),
            back_m: read(extra >> 6 & 31, Self::SIDE_STEP_M, 31),
            ahead_m: read(extra >> 11, Self::SIDE_STEP_M, 31),
        })
    }

    /// Whether `point`, in the prototype's frame, is on the wall's side
    /// of every cut.
    #[must_use]
    /// Behind its wall's face (z = -0.3) is inside the host it is set
    /// at, and is cut too.
    pub fn keeps(&self, point: [f32; 3]) -> bool {
        point[1] <= self.top_m
            && point[0] >= -self.back_m
            && point[0] <= self.ahead_m
            && point[2] >= -0.35
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
    /// Linear RGB of the canopy seen from above: its canopy and
    /// subcanopy trees' foliage in full light (each species'
    /// `appearance.foliage`), weighed by their crowns' area, so a maple
    /// stand reads lighter than a fir one from afar; a dark conifer green
    /// where the cell has no tree crowns, black where it has no stand.
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
    /// Trees grown as an edge beside open ground
    /// ([`FillSettings::edges`]), per hectare.
    #[serde(default)]
    pub edge_trees_per_ha: f64,
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

    /// The tile as one GLB that any glTF viewer opens: each prototype the
    /// tile places, at `level` (0 nearest to 3 farthest), copied once
    /// from its files in `dir` (as [`Fill::write_prototypes`] listed them
    /// in `prototypes`), and a node per prototype mesh drawing every plant
    /// that uses it with `EXT_mesh_gpu_instancing`. The root node sits at
    /// the tile's south-west corner (+X east, +Y up, -Z north). With
    /// `meshopt`, its arrays are compressed with `EXT_meshopt_compression`.
    ///
    /// # Errors
    ///
    /// When a placed prototype is not in `prototypes`, has no such level,
    /// or its file cannot be read or holds what instances cannot carry (a
    /// rig, a node placed by a matrix or scaled unevenly).
    pub fn glb(
        &self,
        prototypes: &[PrototypeFiles],
        dir: &Path,
        level: usize,
        meshopt: bool,
    ) -> Result<Vec<u8>, Error> {
        crate::tile_scene::tile_glb(self, prototypes, dir, level, meshopt)
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
    /// Each community's climbers, drawn at the feet of walls and fences.
    climbers: Vec<Vec<Climber>>,
    /// Each prototype's index by its key.
    index: HashMap<PrototypeKey, u32>,
    /// Each prototype's twin grown as a stand edge, for a tree grown in a
    /// closed stand when the settings draw edges.
    edge_twins: Vec<Option<u32>>,
    /// Each prototype's size, once measured or written.
    sizes: Arc<Vec<OnceLock<Measurements>>>,
    /// The stand tiles laid out so far.
    laid: Arc<Mutex<HashMap<LayoutKey, Arc<Layout>>>>,
}

/// A climber of a community ([`Layer::Climber`]): its species, its
/// abundance, and its prototypes (one per seed) grown on a wall.
#[derive(Debug, Clone)]
struct Climber {
    species: usize,
    abundance: f64,
    prototypes: Vec<u32>,
}

/// How far apart a climber of abundance 1 stands along the foot of a wall
/// or a fence where the ground at its foot is soil, metres. Hand-set: ivy
/// and other climbers set or come up every few metres along an old wall.
pub const CLIMBER_SPACING_M: f64 = 3.0;

/// How far above the ground a wall's or a fence's foot may stand for a
/// climber to root at it, metres: a footing or a sill, not a gable or a
/// window.
pub const CLIMBER_FOOT_M: f64 = 0.3;

/// The least height a wall or a fence stands above the ground at a
/// climber's foot for one to climb it, metres: not a curb.
pub const CLIMBER_LEAST_HOST_M: f64 = 0.5;

/// Each host's run of wall or fence along its `u` axis, metres
/// (`u_axis` · (x, z) at its two ends), for a host with an axis: its own
/// extent joined with every host it runs on into, of its kind, along the
/// same line (axes within 8°, faces within 0.3 m), its foot within 0.3 m
/// and its extent within 0.25 m of the other's: a fence's bays one run,
/// a house's wall one.
fn wall_runs(field: &SiteField) -> Vec<Option<[f64; 2]>> {
    use crate::site_field::Host;
    let along = |host: &Host, axis: [f64; 3]| {
        let mut range = [f64::INFINITY, f64::NEG_INFINITY];
        for x in [host.low[0], host.high[0]] {
            for z in [host.low[2], host.high[2]] {
                let s = axis[0] * x + axis[2] * z;
                range = [range[0].min(s), range[1].max(s)];
            }
        }
        range
    };
    let across = |host: &Host, axis: [f64; 3]| {
        let centre = [
            (host.low[0] + host.high[0]) / 2.0,
            (host.low[2] + host.high[2]) / 2.0,
        ];
        axis[0] * centre[1] - axis[2] * centre[0]
    };
    let hosts = &field.hosts;
    let mut root: Vec<usize> = (0..hosts.len()).collect();
    fn find(root: &mut [usize], mut at: usize) -> usize {
        while root[at] != at {
            root[at] = root[root[at]];
            at = root[at];
        }
        at
    }
    for (i, a) in hosts.iter().enumerate() {
        let Some(axis) = a.u_axis else { continue };
        for (j, b) in hosts.iter().enumerate().skip(i + 1) {
            let Some(other) = b.u_axis else { continue };
            if a.kind != b.kind
                || (axis[0] * other[0] + axis[2] * other[2]).abs() < 0.99
                || (across(a, axis) - across(b, axis)).abs() > 0.3
                || (a.foot_m - b.foot_m).abs() > 0.3
            {
                continue;
            }
            let (one, two) = (along(a, axis), along(b, axis));
            if one[0].max(two[0]) - one[1].min(two[1]) <= 0.25 {
                let (ra, rb) = (find(&mut root, i), find(&mut root, j));
                root[ra] = rb;
            }
        }
    }
    let groups: Vec<usize> = (0..hosts.len()).map(|at| find(&mut root, at)).collect();
    hosts
        .iter()
        .enumerate()
        .map(|(i, host)| {
            let axis = host.u_axis?;
            Some(
                hosts
                    .iter()
                    .zip(&groups)
                    .filter(|(_, group)| **group == groups[i])
                    .map(|(other, _)| along(other, axis))
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |run, range| {
                        [run[0].min(range[0]), run[1].max(range[1])]
                    }),
            )
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Species {
    id: String,
    /// Its spec, as its taxa and its own file give it.
    spec: crate::spec::PlantSpec,
    options: Options,
    /// Made the first time one of its prototypes is grown, drawn or
    /// written ([`Self::generator`]): making one draws every organ's look
    /// and part, which a fill whose sizes are all kept never reads.
    generator: OnceLock<Generator>,
    niche: Option<Niche>,
    /// The seeds of its prototypes: its package's own first.
    seeds: Vec<u64>,
    /// The light it needs from above to stand
    /// ([`development::light_needed`]).
    light_needed: f64,
    /// What its roots ask of the soil, where its program says
    /// ([`Site::soil_ratio_at`]).
    soil: Option<SoilNeed>,
    /// How tall its crowns stand in a stand's canopy, metres: the low end
    /// of the tallest it commonly grows (`height_max_m`), where its traits
    /// state it.
    canopy_m: Option<f64>,
    /// Whether its program grows it beneath a canopy as a shade-suppressed
    /// tree grows ([`BENEATH_PROGRAMS`]).
    grows_beneath: bool,
}

/// The programs whose trees grow beneath a closed canopy as
/// shade-suppressed trees do, slowly and keeping their limbs (`pinidae`
/// from its revision 16), so a stand's shade-tolerant ingrowth of them
/// grows beneath its canopy ([`PrototypeKey::beneath_m`]). A broadleaf's
/// program has no such growth yet: grown there, a bigleaf maple sheds
/// every leaf, so a broadleaf's ingrowth grows as in a stand.
pub const BENEATH_PROGRAMS: [&str; 1] = ["pinidae"];

impl Species {
    fn new(id: &str, settings: &FillSettings) -> Result<Self, Error> {
        let library = crate::library::Library::builtin();
        let spec = library.spec(id)?;
        let niche = Niche::builtin(id).ok();
        let mut seeds = Vec::new();
        for seed in spec.variants.seeds.iter().copied().chain(1..) {
            if seeds.len() >= usize::from(settings.seeds) {
                break;
            }
            if !seeds.contains(&seed) {
                seeds.push(seed);
            }
        }
        let light_needed = niche.as_ref().map_or(0.0, development::light_needed);
        // As `Generator::soil_need` reads it, without making the generator.
        let soil = spec.program_in(library).ok().and_then(|(program, params)| {
            let seed = spec.variants.seeds.first().copied().unwrap_or(1);
            let params = spec.seeded_params(&program, &params, seed);
            SoilNeed::of_program(&program, &params)
        });
        let canopy_m = library
            .traits_of(id)
            .ok()
            .and_then(|traits| traits.get("height_max_m").and_then(crate::traits::least))
            .filter(|height| *height > 0.0);
        let grows_beneath = BENEATH_PROGRAMS.contains(&spec.generator.program.as_str());
        Ok(Self {
            id: id.to_string(),
            spec,
            options: settings.options,
            generator: OnceLock::new(),
            niche,
            seeds,
            light_needed,
            soil,
            canopy_m,
            grows_beneath,
        })
    }

    /// Its generator, made the first time it is asked for. Two threads
    /// that ask at once may each make one; the first kept serves both.
    fn generator(&self) -> Result<&Generator, Error> {
        if let Some(made) = self.generator.get() {
            return Ok(made);
        }
        let made = Generator::new(&self.id, &self.options)?;
        Ok(self.generator.get_or_init(|| made))
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
        let spec = &species.spec;
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
    /// member kept to other settings takes the first of them. A
    /// shade-tolerant tree that came up beneath the canopy grows beneath
    /// it instead ([`Fill::cohort_keys`]).
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
    /// Its tree crowns' colour seen from above, linear RGB: each canopy
    /// and subcanopy tree's species' foliage in full light, weighed by
    /// its crown's area ([`CANOPY_COLOUR`] where it has none).
    canopy_colour: [f32; 3],
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
    /// as the settings ask, and each climber's on a wall. Nothing is grown
    /// yet; see [`Fill::measure`].
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
        // Every species once, in the order the communities first name
        // them, read on every core: reading a species' program draws its
        // organs' looks to measure their areas.
        let mut ids: Vec<&str> = Vec::new();
        for community in communities {
            community.check()?;
            for member in &community.members {
                if !ids.contains(&member.species.as_str()) {
                    ids.push(&member.species);
                }
            }
        }
        let species = crate::cores::map(ids, crate::cores::available(), &|id| {
            Species::new(id, settings)
        })
        .into_iter()
        .collect::<Result<Vec<Species>, Error>>()?;
        let mut stands = Vec::new();
        let mut climbers = Vec::new();
        for community in communities {
            let mut members = Vec::new();
            let mut climbing = Vec::new();
            for member in &community.members {
                let at = species
                    .iter()
                    .position(|s| s.id == member.species)
                    .unwrap_or_default();
                if member.layer == Some(Layer::Climber) {
                    climbing.push(Climber {
                        species: at,
                        abundance: member.abundance.unwrap_or(1.0).max(0.0),
                        prototypes: Vec::new(),
                    });
                    continue;
                }
                members.push(StandMember::new(member, &species[at], at, &community.id)?);
            }
            stands.push(members);
            climbers.push(climbing);
        }
        let mut fill = Self {
            communities: communities.to_vec(),
            prototypes: Vec::new(),
            settings: settings.clone(),
            species,
            stands,
            climbers,
            index: HashMap::new(),
            edge_twins: Vec::new(),
            sizes: Arc::new(Vec::new()),
            laid: Arc::new(Mutex::new(HashMap::new())),
        };
        for community in 0..fill.communities.len() {
            for age in fill.settings.stand_ages.clone() {
                let stand = development::develop(&fill.candidates(community, None), age);
                let canopy = fill.canopy_m(community);
                for cohort in &stand.cohorts {
                    let member = &fill.stands[community][cohort.candidate];
                    let (layer, edges) = (
                        member.layer,
                        fill.settings.edges
                            && member.candidate.tree()
                            && member.environments.contains(&Environment::Edge),
                    );
                    for key in fill.cohort_keys(member, cohort, stand.stage, canopy) {
                        let interior = key.environment == Environment::Interior;
                        let edge_key = PrototypeKey {
                            environment: Environment::Edge,
                            ..key.clone()
                        };
                        let index = fill.add_prototype(key, cohort.age, layer)?;
                        if edges && interior {
                            let twin = fill.add_prototype(edge_key, cohort.age, layer)?;
                            fill.edge_twins.resize(fill.prototypes.len(), None);
                            fill.edge_twins[index as usize] = Some(twin);
                        }
                    }
                }
            }
        }
        fill.add_climber_prototypes()?;
        fill.edge_twins.resize(fill.prototypes.len(), None);
        fill.sizes = Arc::new(
            (0..fill.prototypes.len())
                .map(|_| OnceLock::new())
                .collect(),
        );
        Ok(fill)
    }

    /// The index of the prototype `key`, added if it is new.
    fn add_prototype(
        &mut self,
        key: PrototypeKey,
        age_years: f64,
        layer: Option<Layer>,
    ) -> Result<u32, Error> {
        if let Some(index) = self.index.get(&key) {
            return Ok(*index);
        }
        let index = u32::try_from(self.prototypes.len())
            .map_err(|_| Error("fill: too many prototypes".into()))?;
        self.index.insert(key.clone(), index);
        self.prototypes.push(Prototype {
            key,
            age_years,
            layer,
        });
        Ok(index)
    }

    /// Each climber's prototypes: one per seed, grown on a wall
    /// ([`crate::export::WALL_M`]) to the first age its spec measures it
    /// that high, or to its oldest.
    fn add_climber_prototypes(&mut self) -> Result<(), Error> {
        for community in 0..self.climbers.len() {
            for climber in 0..self.climbers[community].len() {
                let species = &self.species[self.climbers[community][climber].species];
                let oldest = crate::export::oldest_age(&species.spec);
                let age = species
                    .spec
                    .allometry
                    .iter()
                    .filter(|point| point.height >= crate::export::WALL_M && point.age > 0.0)
                    .map(|point| point.age)
                    .fold(oldest, f64::min);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let age_tenths = (age * 10.0).round() as u32;
                let mut indices = Vec::new();
                for seed in species.seeds.clone() {
                    let key = PrototypeKey {
                        species: species.id.clone(),
                        seed,
                        environment: Environment::Open,
                        age_tenths,
                        on_wall: true,
                        beneath_m: None,
                    };
                    let index = if let Some(index) = self.index.get(&key) {
                        *index
                    } else {
                        let index = u32::try_from(self.prototypes.len())
                            .map_err(|_| Error("fill: too many prototypes".into()))?;
                        self.index.insert(key.clone(), index);
                        self.prototypes.push(Prototype {
                            key,
                            age_years: age,
                            layer: Some(Layer::Climber),
                        });
                        index
                    };
                    indices.push(index);
                }
                self.climbers[community][climber].prototypes = indices;
            }
        }
        Ok(())
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

    /// How tall the canopy of `community`'s stands stands, whole metres:
    /// as tall as the tallest of its trees commonly grows
    /// ([`Species::canopy_m`]), whatever the site and the stand's age, so a
    /// prototype's key does not hang on them. None where none of its
    /// trees states it.
    fn canopy_m(&self, community: usize) -> Option<u16> {
        let tallest = self.stands[community]
            .iter()
            .filter(|member| member.candidate.tree())
            .filter_map(|member| self.species[member.species].canopy_m)
            .fold(0.0_f64, f64::max);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (tallest >= 1.0).then(|| tallest.round().min(f64::from(u16::MAX)) as u16)
    }

    /// The keys of a cohort's prototypes, one per seed. A shade-tolerant
    /// tree that came up under its stand's crowns (ingrowth, not its
    /// pioneers) grows beneath a canopy `canopy` metres tall
    /// ([`PrototypeKey::beneath_m`]) where its program grows it there
    /// ([`BENEATH_PROGRAMS`]) and its member is not kept from
    /// [`Environment::Suppressed`]: a western hemlock that came up 120
    /// years ago beneath Wind River's 60 m canopy is 19.7 m tall, not the
    /// 43.6 m of a stand of hemlocks its age. The share of it a gap
    /// released ([`development::Cohort::released`]) grows as in a stand, at
    /// the age its size matches.
    fn cohort_keys(
        &self,
        member: &StandMember,
        cohort: &development::Cohort,
        stage: Stage,
        canopy: Option<u16>,
    ) -> Vec<PrototypeKey> {
        let species = &self.species[member.species];
        let beneath_m = canopy.filter(|_| {
            species.grows_beneath
                && member.candidate.tree()
                && !cohort.pioneer
                && !cohort.released
                && member.environments.contains(&Environment::Suppressed)
        });
        let environment = if beneath_m.is_some() {
            Environment::Suppressed
        } else {
            member.environment(stage)
        };
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
                on_wall: false,
                beneath_m,
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
        let generator = self.species_of(prototype).generator()?.clone();
        let plant = Self::grow_ages(&generator, &prototype.key, &[prototype.age_years])?
            .pop()
            .ok_or_else(|| Error(format!("fill: {} kept no plant", prototype.key.stem())))?;
        let _ = self.sizes[index].set(plant.size.clone());
        Ok((generator, plant))
    }

    /// A prototype `key` of `generator`'s species grown and kept at each
    /// of `ages` (rising): at the foot of a wall, beneath its stand's
    /// canopy, or in its environment.
    fn grow_ages(
        generator: &Generator,
        key: &PrototypeKey,
        ages: &[f64],
    ) -> Result<Vec<Plant>, Error> {
        Ok(if key.on_wall {
            generator.grow_ages_on_wall(key.seed, ages, crate::export::WALL_M)?
        } else if let Some(canopy) = key.beneath_m {
            generator.grow_ages_beneath(f64::from(canopy), key.seed, ages)?
        } else {
            generator.grow_ages_in(key.environment, key.seed, ages)?
        })
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

    /// `indices` in growths: one per species, setting (and canopy over
    /// it) and seed, its ages rising; the oldest growths first, so the
    /// longest start soonest.
    fn growths(&self, indices: &[u32]) -> Vec<Vec<u32>> {
        type Growth<'a> = (&'a str, Environment, bool, Option<u16>, u64);
        let mut groups: BTreeMap<Growth, Vec<u32>> = BTreeMap::new();
        for &index in indices {
            let key = &self.prototypes[index as usize].key;
            groups
                .entry((
                    key.species.as_str(),
                    key.environment,
                    key.on_wall,
                    key.beneath_m,
                    key.seed,
                ))
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

    /// The packages that hold the prototypes `which` (indices into
    /// [`Fill::prototypes`]; every one when empty): one per species,
    /// setting (and canopy over it) and seed, as [`Fill::measure`] grows
    /// them, the oldest growths first. A tree come up beneath a canopy
    /// grows as [`Generator::grow_ages_beneath`] grows it, its variant's
    /// neighbourhood the canopy's; an old plant past its spec's years
    /// grows on to its age, as a grown-old spec does
    /// ([`PlantSpec::grown_old`]). A climber grown at the foot of a wall
    /// has no package yet (a spec states no wall to grow on), so the
    /// prototypes of one are left out.
    ///
    /// # Errors
    ///
    /// When an index is not a prototype.
    pub fn packages(&self, which: &[u32]) -> Result<Vec<PrototypePackage>, Error> {
        let wanted = self.wanted(which)?;
        let mut packages = Vec::new();
        for growth in self.growths(&wanted) {
            let first = &self.prototypes[growth[0] as usize];
            if first.key.on_wall {
                continue;
            }
            let ages: Vec<f64> = growth
                .iter()
                .map(|index| self.prototypes[*index as usize].age_years)
                .collect();
            let mut spec = self.species_of(first).spec.clone();
            let key = &first.key;
            // Its reference sizes are its setting's: none beneath a
            // canopy, where no yield table measured it.
            if let Some(canopy) = key.beneath_m {
                let beneath = spec.neighbourhood_beneath(f64::from(canopy));
                spec.variants
                    .neighbourhoods
                    .insert(key.environment, beneath);
                spec.allometry.clear();
            } else {
                spec.allometry
                    .retain(|point| point.environment == key.environment);
            }
            spec.variants.environments = vec![key.environment];
            // Its parameters dealt as among the species' own seeds.
            spec.variants.dealt_among = std::mem::take(&mut spec.variants.seeds);
            spec.variants.seeds = vec![key.seed];
            spec.variants.conditions.clear();
            if let Some(&oldest) = ages.last() {
                spec.growth.years = spec.growth.years.max(oldest);
            }
            spec.growth.keyframes = ages;
            packages.push(PrototypePackage {
                spec,
                prototypes: growth
                    .iter()
                    .enumerate()
                    .map(|(keyframe, index)| (*index, keyframe))
                    .collect(),
            });
        }
        Ok(packages)
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
        Self::grow_ages(
            self.species_of(prototype).generator()?,
            &prototype.key,
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
                let generator = self.species_of(prototype).generator()?;
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
    /// share [`Site::plantable_at`] gives where it stands. A stand's first
    /// row of trees beside open ground grows as an edge
    /// ([`FillSettings::edges`]); the cells past the map's sides are not
    /// known here, so a tree there is grown as inside its stand (see
    /// [`Fill::tile_among`]).
    ///
    /// # Errors
    ///
    /// When the stand map does not cover the site or names a community
    /// the fill does not have, or a prototype a stand needs has not been
    /// measured ([`Fill::measure`]).
    pub fn tile(&self, site: &Site, stands: &StandMap, seed: u64) -> Result<TilePlan, Error> {
        self.tile_among(site, stands, &|_, _| None, seed)
    }

    /// [`Fill::tile`], with `outside(col, row)` giving the stand cells
    /// just past the map's sides (`col` -1 or `cols`, `row` -1 or the
    /// rows; never a corner), as the neighbouring tiles' maps hold them,
    /// or `None` where not known. With them, a stand's edge beside a
    /// neighbouring tile's opening grows as it would were the two one
    /// tile: a tile filled whole or in quarters places the same plants.
    ///
    /// A tree is grown as an edge when its stem stands within its crown's
    /// width of a side of its cell that opens: no crown of its stand
    /// stands between it and the light. It is turned to face the opening,
    /// the side [`crate::conditions::LayoutKind::Edge`] grows free.
    ///
    /// # Errors
    ///
    /// As [`Fill::tile`]; and when a cell `outside` gives names a
    /// community the fill does not have.
    #[allow(clippy::too_many_lines)]
    pub fn tile_among(
        &self,
        site: &Site,
        stands: &StandMap,
        outside: &dyn Fn(i64, i64) -> Option<StandCell>,
        seed: u64,
    ) -> Result<TilePlan, Error> {
        stands.check(site, self.communities.len())?;
        let cell_m = stands.cell_m;
        // A cell finer than a stand tile reads its window of one laid out
        // for its block of cells ([`cells_per_layout`]).
        let per = cells_per_layout(cell_m);
        #[allow(clippy::cast_precision_loss)]
        let layout_m = cell_m * per as f64;
        let rows = stands.cells.len() / stands.cols.max(1);
        let (cols_i, rows_i) = (
            i64::try_from(stands.cols).unwrap_or(i64::MAX),
            i64::try_from(rows).unwrap_or(i64::MAX),
        );
        // The cell at (col, row), in the map or past its sides.
        let cell_at = |col: i64, row: i64| -> Option<StandCell> {
            if (0..cols_i).contains(&col) && (0..rows_i).contains(&row) {
                usize::try_from(row * cols_i + col)
                    .ok()
                    .map(|at| stands.cells[at])
            } else {
                outside(col, row)
            }
        };
        // Each cell's stand tile: its community and age, how the site
        // suits each member at the cell's middle (at its block's middle,
        // within the tile, for a cell finer than a stand tile, so the
        // cells of a block share one), and which of the block's layouts.
        let key_of = |index: usize| -> Option<LayoutKey> {
            let cell = stands.cells[index];
            let community = usize::from(cell.community?);
            let (col, row) = (index % stands.cols, index / stands.cols);
            let (block, window) = layout_block(world_cell(site, stands, col, row), per, cell_m);
            #[allow(clippy::cast_precision_loss)]
            let corner = [col as f64 * cell_m, row as f64 * cell_m];
            let middle = if per == 1 {
                [corner[0] + 0.5 * cell_m, corner[1] + 0.5 * cell_m]
            } else {
                [
                    (corner[0] - window[0] + 0.5 * layout_m).clamp(0.0, site.tile_m),
                    (corner[1] - window[1] + 0.5 * layout_m).clamp(0.0, site.tile_m),
                ]
            };
            let factors = site.site_factors_at(middle[0], middle[1]);
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
            Some(LayoutKey {
                community,
                age: nearest_age(
                    &self.settings.stand_ages,
                    f64::from(cell.years_since_disturbance),
                ),
                suits,
                cell_bits: layout_m.to_bits(),
                seed,
                layout: rng::hash_words(&[seed, block[0], block[1], 1]) % LAYOUTS,
            })
        };
        let keys: Vec<Option<LayoutKey>> = (0..stands.cells.len()).map(key_of).collect();
        // Every stand tile the cells need, and each community and age's
        // tile on a site that suits it fully (its edges' height), laid out
        // on every core before the cells read them.
        let mut needed: Vec<LayoutKey> = Vec::new();
        for key in keys.iter().flatten() {
            let whole = LayoutKey {
                suits: vec![4; key.suits.len()],
                layout: 0,
                ..key.clone()
            };
            for key in [key.clone(), whole] {
                if !needed.contains(&key) {
                    needed.push(key);
                }
            }
        }
        self.lay_out_all(needed)?;
        let mut edge_trees = 0_usize;
        let mut instances = Vec::new();
        let mut cover = Vec::with_capacity(stands.cells.len());
        let mut canopy = Vec::with_capacity(stands.cells.len());
        let (mut trees, mut basal, mut leaf_m2, mut crowns) = (0_usize, 0.0, 0.0, 0.0);
        let roofs = site.roofs();
        for (index, cell) in stands.cells.iter().enumerate() {
            let (col, row) = (index % stands.cols, index / stands.cols);
            let world = world_cell(site, stands, col, row);
            let (Some(community), Some(key)) = (cell.community.map(usize::from), &keys[index])
            else {
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
            let (block, window) = layout_block(world, per, cell_m);
            let draw = |salt: u64| rng::hash_words(&[seed, block[0], block[1], salt]);
            let age = key.age;
            let layout = self.layout(key.clone())?;
            // Which of its sides, west, east, south and north, open onto
            // ground its canopy overtops.
            let mut opens = [false; 4];
            if self.settings.edges {
                let own = self.stand_height(community, age, layout_m, seed)?;
                if own > 0.0 {
                    #[allow(clippy::cast_possible_wrap)]
                    let (col, row) = (col as i64, row as i64);
                    let beside = [
                        (col - 1, row),
                        (col + 1, row),
                        (col, row - 1),
                        (col, row + 1),
                    ];
                    for (side, (c, r)) in beside.into_iter().enumerate() {
                        opens[side] = match cell_at(c, r) {
                            None => false,
                            Some(StandCell {
                                community: None, ..
                            }) => true,
                            Some(StandCell {
                                community: Some(other),
                                years_since_disturbance,
                            }) => {
                                let other = usize::from(other);
                                if other >= self.communities.len() {
                                    return Err(Error(format!(
                                        "stands: community {other} beside the tile is not one of the fill's {}",
                                        self.communities.len()
                                    )));
                                }
                                let age = nearest_age(
                                    &self.settings.stand_ages,
                                    f64::from(years_since_disturbance),
                                );
                                self.stand_height(other, age, layout_m, seed)?
                                    < EDGE_HEIGHT_SHARE * own
                            }
                        };
                    }
                }
            }
            // The block's own turn and shift of the wrapping stand tile.
            let turn = draw(2) % 4;
            let shift = [rng::unit(draw(3)) * layout_m, rng::unit(draw(4)) * layout_m];
            let keep = draw(5);
            let (mut tree_crowns, mut kept_tree_crowns) = (0.0, 0.0);
            let (mut herbs, mut kept_herbs) = (0_usize, 0_usize);
            for (n, plant) in layout.plants.iter().enumerate() {
                let at = turned(
                    [
                        wrap(plant.at[0] + shift[0], layout_m),
                        wrap(plant.at[1] + shift[1], layout_m),
                    ],
                    turn,
                    layout_m,
                );
                // Only the plants whose stems stand in this cell's window.
                let at = [at[0] - window[0], at[1] - window[1]];
                if !(0.0..cell_m).contains(&at[0]) || !(0.0..cell_m).contains(&at[1]) {
                    continue;
                }
                let tree = matches!(plant.layer, niche::Layer::Canopy | niche::Layer::Subcanopy);
                let herb = plant.layer == niche::Layer::Herb;
                if tree {
                    tree_crowns += plant.crown_m2;
                }
                herbs += usize::from(herb);
                let (east, north) = (corner[0] + at[0], corner[1] + at[1]);
                let pick = |salt: u64| rng::unit(rng::hash_words(&[keep, n as u64, salt]));
                let plantable = site.plantable_at(east, north);
                // A herb over a building stands on its roof where soil has
                // gathered there.
                let roof = if herb && plantable <= 0.0 {
                    roofs.at(site, east, north)
                } else {
                    None
                };
                if roof.is_none() && pick(1) >= plantable {
                    continue;
                }
                let prototype = &self.prototypes[plant.prototype as usize];
                let size = self.size(plant.prototype as usize);
                // A plant on shallow soil is drawn as `soil@1` grows it:
                // its lengths times the square root of its soil's supply.
                let scale = match (size, &self.species_of(prototype).soil) {
                    (Some(size), Some(need)) => {
                        let (crown_m, height_m) =
                            (size.crown_radius * plant.scale, size.height * plant.scale);
                        let ratio = match roof {
                            Some(roof) => roof.soil_ratio(need, crown_m, height_m),
                            None => site.soil_ratio_at(east, north, need, crown_m, height_m),
                        };
                        plant.scale * ratio.sqrt()
                    }
                    _ => plant.scale,
                };
                let up = roof.map_or_else(|| site.height_at(east, north), |roof| roof.height_m);
                let crown_m2 = plant.crown_m2 * (scale / plant.scale).powi(2);
                if tree {
                    kept_tree_crowns += crown_m2;
                }
                kept_herbs += usize::from(herb);
                let lean = turned_vector(plant.lean, turn);
                #[allow(clippy::cast_precision_loss)]
                let mut yaw = plant.yaw + turn as f64 * std::f64::consts::FRAC_PI_2;
                let mut index = plant.prototype;
                // A tree of the first row beside an opening grows as an
                // edge, facing it: the nearest open side within its
                // crown's width.
                if let Some(twin) = self.edge_twins[index as usize].filter(|_| tree) {
                    let width = 2.0 * (plant.crown_m2 / std::f64::consts::PI).sqrt();
                    let gaps = [at[0], cell_m - at[0], at[1], cell_m - at[1]];
                    if let Some(side) = (0..4)
                        .filter(|side| opens[*side] && gaps[*side] < width)
                        .min_by(|a, b| gaps[*a].total_cmp(&gaps[*b]))
                    {
                        const FACING: [f64; 4] = [
                            std::f64::consts::PI,
                            0.0,
                            -std::f64::consts::FRAC_PI_2,
                            std::f64::consts::FRAC_PI_2,
                        ];
                        index = twin;
                        yaw = FACING[side] + (pick(4) - 0.5) * 2.0 * EDGE_YAW_SPREAD;
                        edge_trees += 1;
                    }
                }
                // The twin's own size, at the plant's (soil) scale.
                let (prototype, measured) = if index == plant.prototype {
                    (prototype, size)
                } else {
                    (&self.prototypes[index as usize], self.size(index as usize))
                };
                if let Some(size) = measured {
                    let dbh = size.dbh.unwrap_or(0.0) * scale;
                    if dbh >= 0.05 {
                        trees += 1;
                        basal += std::f64::consts::PI * 0.25 * dbh * dbh;
                    }
                    leaf_m2 += size.leaf_area * scale * scale;
                }
                #[allow(clippy::cast_possible_truncation)]
                instances.push(Instance {
                    prototype: index,
                    position: [east as f32, north as f32, up as f32],
                    rotation: Instance::lean(yaw, lean),
                    scale: scale as f32,
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
                colour: layout.canopy_colour,
            });
        }
        // Plants that came up by themselves in the pavements, in the order
        // the site keeps them, each its species' prototype nearest its
        // age, scaled to its height.
        for (n, plant) in site.colonists.iter().enumerate() {
            let index = self.colonist_prototype(plant)?;
            let size = self.size(index).ok_or_else(|| {
                Error(format!(
                    "fill: prototype {index} ({}) has not been measured",
                    self.prototypes[index].key.stem()
                ))
            })?;
            let scale = if size.height > 0.0 {
                plant.height_m / size.height
            } else {
                1.0
            };
            let draw = |salt: u64| rng::unit(rng::hash_words(&[seed, 0x6372_6163, n as u64, salt]));
            let dbh = size.dbh.unwrap_or(0.0) * scale;
            if dbh >= 0.05 {
                trees += 1;
                basal += std::f64::consts::PI * 0.25 * dbh * dbh;
            }
            leaf_m2 += size.leaf_area * scale * scale;
            #[allow(clippy::cast_possible_truncation)]
            instances.push(Instance {
                prototype: u32::try_from(index).unwrap_or(u32::MAX),
                position: [
                    plant.east as f32,
                    plant.north as f32,
                    site.height_at(plant.east, plant.north) as f32,
                ],
                rotation: Instance::lean(draw(1) * std::f64::consts::TAU, [0.0, 0.0]),
                scale: scale as f32,
                age_years: self.prototypes[index].age_years as f32,
                tint: [
                    (draw(2) * 2.0 - 1.0) as f32 * 0.2,
                    (draw(3) * 2.0 - 1.0) as f32 * 0.1,
                ],
                extra: 0,
            });
        }
        for climbing in self.climbers_at(site, stands, seed) {
            if let Some(size) = self.size(climbing.prototype as usize) {
                leaf_m2 += size.leaf_area;
            }
            instances.push(climbing);
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
            edge_trees_per_ha: edge_trees as f64 / area_ha,
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

    /// The mean height of community `community`'s canopy at stand age
    /// `age` (an index into the settings' stand ages) on a site that
    /// suits all its members: what an edge's neighbours are compared by,
    /// the same wherever the cell stands.
    fn stand_height(
        &self,
        community: usize,
        age: usize,
        cell_m: f64,
        seed: u64,
    ) -> Result<f64, Error> {
        Ok(self
            .layout(LayoutKey {
                community,
                age,
                suits: vec![4; self.stands[community].len()],
                cell_bits: cell_m.to_bits(),
                seed,
                layout: 0,
            })?
            .canopy_height)
    }

    /// The climbers of a field's tile: at the foot of each climbable
    /// patch of a wall or a fence that is not kept clear, each climber of
    /// the community of the stand cell there stands with the chance its
    /// abundance gives (at abundance 1, one per [`CLIMBER_SPACING_M`] of
    /// foot) times the open share of the ground cell it roots in, so none
    /// stands on sound pavement, nor where the host's foot stands above
    /// the ground ([`CLIMBER_FOOT_M`]) or its top is under
    /// [`CLIMBER_LEAST_HOST_M`]. Each is one of its prototypes grown on a
    /// wall, its wall's face turned onto the patch's, and cut back to the
    /// host's top and to both ends of its run of wall ([`WallClip`] in
    /// `extra`).
    fn climbers_at(&self, site: &Site, stands: &StandMap, seed: u64) -> Vec<Instance> {
        let mut placed = Vec::new();
        let Some(tile) = &site.field else {
            return placed;
        };
        if self.climbers.iter().all(Vec::is_empty) {
            return placed;
        }
        let field = &tile.field;
        let patch_m = field.settings.patch_m;
        let runs = wall_runs(field);
        for (number, patch) in field.patches.iter().enumerate() {
            let Some(host) = field.hosts.get(patch.host as usize) else {
                continue;
            };
            let (Some(axis), Some(run)) = (host.u_axis, runs[patch.host as usize]) else {
                continue;
            };
            let level = patch.normal[0].hypot(patch.normal[2]);
            if !matches!(
                host.kind,
                crate::site_field::HostKind::Wall | crate::site_field::HostKind::Fence
            ) || !patch.climbable
                || patch.clear
                || patch.v > 1.0e-6
                || level < 0.7
            {
                continue;
            }
            let Some(root) = patch.root_cell.map(|cell| cell as usize) else {
                continue;
            };
            let Some(ground) = field.ground.height.get(root).copied().flatten() else {
                continue;
            };
            let open = field.ground.open_share[root].clamp(0.0, 1.0);
            // A climber roots at the foot of its host: not under a gable
            // or a window whose foot stands above the ground, nor at a
            // curb too low to climb.
            if open <= 0.0
                || host.foot_m - ground > CLIMBER_FOOT_M
                || host.high[1] - ground < CLIMBER_LEAST_HOST_M
            {
                continue;
            }
            // Its foot 0.3 m out from the face, as it grew at its wall.
            let out = [patch.normal[0] / level, patch.normal[2] / level];
            let (x, z) = (
                patch.centre[0] + 0.3 * out[0],
                patch.centre[2] + 0.3 * out[1],
            );
            let (east, north) = (x - tile.corner[0], tile.corner[1] - z);
            if !(0.0..site.tile_m).contains(&east) || !(0.0..site.tile_m).contains(&north) {
                continue;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cell =
                (north / stands.cell_m) as usize * stands.cols + (east / stands.cell_m) as usize;
            let Some(community) = stands
                .cells
                .get(cell)
                .and_then(|cell| cell.community)
                .map(usize::from)
            else {
                continue;
            };
            let Some(climbers) = self.climbers.get(community) else {
                continue;
            };
            for (which, climber) in climbers.iter().enumerate() {
                let draw = |salt: u64| {
                    rng::unit(rng::hash_words(&[
                        seed,
                        0x636c_696d,
                        number as u64,
                        which as u64,
                        salt,
                    ]))
                };
                let chance = (patch_m / CLIMBER_SPACING_M * climber.abundance).min(1.0) * open;
                if climber.prototypes.is_empty() || draw(1) >= chance {
                    continue;
                }
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let pick = ((draw(2) * climber.prototypes.len() as f64) as usize)
                    .min(climber.prototypes.len() - 1);
                let prototype = climber.prototypes[pick];
                // The prototype's face looks south (+z); turn it onto the
                // patch's, east toward north.
                let yaw = (-out[1]).atan2(out[0]) + std::f64::consts::FRAC_PI_2;
                // The prototype's +x along the field, and how far the
                // wall runs each way from its foot.
                let along = axis[0] * yaw.cos() - axis[2] * yaw.sin();
                let foot = axis[0] * x + axis[2] * z;
                let (before, after) = ((foot - run[0]).max(0.0), (run[1] - foot).max(0.0));
                let (back, ahead) = if along >= 0.0 {
                    (before, after)
                } else {
                    (after, before)
                };
                #[allow(clippy::cast_possible_truncation)]
                let clip = WallClip {
                    top_m: (host.high[1] - ground) as f32,
                    back_m: back as f32,
                    ahead_m: ahead as f32,
                }
                .to_extra();
                #[allow(clippy::cast_possible_truncation)]
                placed.push(Instance {
                    prototype,
                    position: [east as f32, north as f32, ground as f32],
                    rotation: Instance::lean(yaw, [0.0, 0.0]),
                    scale: 1.0,
                    age_years: self.prototypes[prototype as usize].age_years as f32,
                    tint: [
                        (draw(3) * 2.0 - 1.0) as f32 * 0.2,
                        (draw(4) * 2.0 - 1.0) as f32 * 0.1,
                    ],
                    extra: clip,
                });
            }
        }
        placed
    }

    /// The prototype a plant that came up in a crack is drawn as: its
    /// species' nearest its age (by their ratio), grown in the open where
    /// one is.
    fn colonist_prototype(&self, plant: &CrackPlant) -> Result<usize, Error> {
        let age = plant.age_years.max(0.1);
        self.prototypes
            .iter()
            .enumerate()
            .filter(|(_, prototype)| {
                prototype.key.species == plant.species && !prototype.key.on_wall
            })
            .min_by(|(_, a), (_, b)| {
                let off = |p: &Prototype| {
                    let open = if p.key.environment == Environment::Open { 0.0 } else { 1.0 };
                    (open, (p.age_years.max(0.1) / age).ln().abs())
                };
                off(a).partial_cmp(&off(b)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(index, _)| index)
            .ok_or_else(|| {
                Error(format!(
                    "fill: {} came up in a crack but the fill has no prototype of it; add the crack pool to the fill as a community",
                    plant.species
                ))
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

    /// Lay out every stand tile of `keys` not laid out yet, on every core.
    fn lay_out_all(&self, keys: Vec<LayoutKey>) -> Result<(), Error> {
        let missing: Vec<LayoutKey> = {
            let laid = self.laid.lock().unwrap_or_else(PoisonError::into_inner);
            keys.into_iter()
                .filter(|key| !laid.contains_key(key))
                .collect()
        };
        let made = crate::cores::map(missing, crate::cores::available(), &|key| {
            self.lay_out(&key).map(|layout| (key, Arc::new(layout)))
        });
        let mut laid = self.laid.lock().unwrap_or_else(PoisonError::into_inner);
        for made in made {
            let (key, layout) = made?;
            laid.entry(key).or_insert(layout);
        }
        Ok(())
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
            let keys = self.cohort_keys(member, cohort, stand.stage, self.canopy_m(key.community));
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
        let (members, foliage): (Vec<stand_tile::Member>, Vec<[f32; 3]>) = chosen
            .iter()
            .map(|(index, candidate, share, size, species)| {
                let member = stand_tile::Member {
                    id: *index,
                    layer: development::layer_of(&candidates[*candidate], size.height, top),
                    share: *share,
                    height_m: size.height,
                    crown_base_m: size.crown_base.unwrap_or(0.0),
                    crown_radius_m: size.crown_radius,
                    light_needed: self.species[*species].light_needed,
                };
                (member, self.species[*species].spec.appearance.foliage)
            })
            .filter(|(member, _)| {
                let herb = member.layer == niche::Layer::Herb;
                has_herbs |= herb;
                self.settings.herbs || !herb
            })
            .unzip();
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
        let (mut colour, mut crowns) = ([0.0_f64; 3], 0.0);
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
                if matches!(member.layer, niche::Layer::Canopy | niche::Layer::Subcanopy) {
                    let area = radius * radius;
                    for (sum, part) in colour.iter_mut().zip(foliage[plant.member]) {
                        *sum += area * f64::from(part);
                    }
                    crowns += area;
                }
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
            #[allow(clippy::cast_possible_truncation)]
            canopy_colour: if crowns > 0.0 {
                colour.map(|sum| (sum / crowns) as f32)
            } else {
                CANOPY_COLOUR
            },
            herb_cover,
        })
    }
}

/// The far canopy's colour seen from above where a stand cell has no
/// tree crowns, linear RGB: a dark conifer green.
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

/// How many stand cells a side one stand tile is laid out for: one for a
/// cell of [`STAND_CELL_M`] or more, else as many as make about that. A
/// finer cell (a city's yards, verges and beds, a few metres each) reads
/// its window of the stand tile its block shares, so a crown wider than the
/// cell is laid out whole and the cells of one community and age in a
/// block continue one pattern, with no seam between them.
#[must_use]
pub fn cells_per_layout(cell_m: f64) -> u64 {
    if !(cell_m > 0.0) || cell_m >= STAND_CELL_M {
        return 1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let per = (STAND_CELL_M / cell_m).round().max(1.0) as u64;
    per
}

/// The block of `per` cells a side that world cell `world` lies in, as two
/// words that key the block's draws, and the cell's window in the block's
/// stand tile, metres east and north of its corner.
fn layout_block(world: [u64; 2], per: u64, cell_m: f64) -> ([u64; 2], [f64; 2]) {
    let per = i64::try_from(per.max(1)).unwrap_or(1);
    let split = |word: u64| {
        let at = word.cast_signed();
        #[allow(clippy::cast_precision_loss)]
        let offset = at.rem_euclid(per) as f64 * cell_m;
        (at.div_euclid(per).cast_unsigned(), offset)
    };
    let ((east, east_m), (north, north_m)) = (split(world[0]), split(world[1]));
    ([east, north], [east_m, north_m])
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
    fn a_wall_clip_packs_into_an_instances_extra_rounded_onto_the_wall() {
        let clip = WallClip {
            top_m: 2.87,
            back_m: 0.35,
            ahead_m: 9.0,
        };
        let extra = clip.to_extra();
        assert_ne!(extra, 0);
        let back = WallClip::from_extra(extra).unwrap();
        assert!((back.top_m - 2.8).abs() < 1e-4, "{back:?}");
        assert!((back.back_m - 0.2).abs() < 1e-4, "{back:?}");
        assert!(back.ahead_m.is_infinite(), "{back:?}");
        assert!(back.keeps([1.0, 1.0, 0.0]) && back.keeps([-0.2, 2.8, -0.3]));
        assert!(!back.keeps([-0.3, 1.0, 0.0]) && !back.keeps([0.0, 2.9, 0.0]));
        // Behind its wall's face is inside the host.
        assert!(!back.keeps([0.0, 1.0, -0.5]));
        // Nothing to its top or ends is still a cut, never 0.
        let none = WallClip {
            top_m: 0.0,
            back_m: 0.0,
            ahead_m: 0.0,
        };
        assert_ne!(none.to_extra(), 0);
        assert_eq!(WallClip::from_extra(0), None);
    }

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
    #[allow(clippy::float_cmp)]
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
            on_wall: false,
            beneath_m: None,
        };
        assert_eq!(key.stem(), "x-interior-2-30.5y");
        let climber = PrototypeKey {
            on_wall: true,
            ..key.clone()
        };
        assert_eq!(climber.stem(), "x-wall-2-30.5y");
        let beneath = PrototypeKey {
            environment: Environment::Suppressed,
            beneath_m: Some(60),
            ..key.clone()
        };
        assert_eq!(beneath.stem(), "x-beneath60m-2-30.5y");
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
