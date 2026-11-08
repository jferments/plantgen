//! The stable way in: grow a species, export it, package it, and make
//! many distinct plants of it, without knowing how the generator works
//! inside.
//!
//! Every other module of this crate is public so that `PlantLab`, `plantc`
//! and Project After can reach what they need, but those modules change
//! whenever the generator does. This one changes only with
//! [`API_VERSION`]: an item here keeps its name, arguments and meaning
//! until the version goes up, and the version goes up only when one of
//! them changes. What an item returns can still change in its bytes with
//! [`GENERATOR_REVISION`](crate::GENERATOR_REVISION), as every drawing
//! does.
//!
//! One [`Generator`] per species does all of it:
//!
//! ```no_run
//! use plantgen::api::{Generator, Options, Variants};
//!
//! # fn main() -> Result<(), plantgen::api::Error> {
//! // Yarrow from the built-in library, as its package draws it.
//! let yarrow = Generator::new("achillea-millefolium", &Options::default())?;
//!
//! // One plant: seed 1 in the open, at its oldest age, as glTF files.
//! let plant = yarrow.grow(1)?;
//! let files = yarrow.export(&plant, &Default::default())?;
//! plantgen::export::write(&files, "exports".as_ref())?;
//!
//! // Fifty distinct yarrows, each a GLB per level of detail, and a
//! // `variants.json` listing them.
//! let set = yarrow.write_variants(&Variants::count(50), "yarrows".as_ref())?;
//! println!("{} plants", set.plants.len());
//! # Ok(())
//! # }
//! ```
//!
//! The same species, [`GENERATOR_REVISION`](crate::GENERATOR_REVISION),
//! options and seed give the same bytes on every machine. Seed `n` is the
//! same plant whether 5 plants or 5,000 are made beside it, and the seeds
//! a species' spec lists (its `variants.seeds`, seed 1 always among them)
//! are the plants its package keeps.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::export;
use crate::library::Library;
use crate::package::{self, Inputs};
use crate::quality;

pub use crate::export::{
    Attributes, Budget, Compression, Export, Level, LevelPart, Plant, Region, Settings, Shade,
    crown_regions,
};
pub use crate::fill::{
    CLIMBER_SPACING_M, CanopyCell, Community, Cover, CoverKind, CrackPlant, FieldTile, Fill,
    FillSettings, Floor, FloorMoss, INSTANCE_BYTES, INSTANCE_SCHEMA, Instance, Layer, LayerCover,
    Log, Member, PROTOTYPES_FILE, Prototype, PrototypeFiles, PrototypeKey, PrototypePackage,
    RoofSoil, Roofs, STAND_AGES, STAND_CELL_M, STAND_SCALE, Site, StandCell, StandMap,
    TRIANGLE_PIXELS, TileId, TilePlan, TileStats, WallClip, moisture_from_wetness,
};
pub use crate::package::Measurements;
pub use crate::quality::Quality;
pub use crate::spec::{Environment, PlantSpec};

/// The version of this module's items. It goes up when one of them is
/// renamed, removed, or changes its arguments or meaning; items added
/// beside them leave it as it is. 2: [`Budget`] gained `tiers` and is
/// made with [`Budget::new`] or [`Budget::tiers`], so fields added later
/// break nothing; [`Variants`] draws tiers unless chosen. 3: [`Level`]
/// gained `cards`, the baked-card tier a level is drawn as. 4: [`Level`]
/// gained `parts`, its share in each region of
/// [`Generator::export_parts`]. 5: a [`Fill`] develops its stands at
/// [`FillSettings::stand_ages`] in place of age classes, keys a
/// [`PrototypeKey`] by its age (`age_tenths`) and is measured
/// ([`Fill::measure`]) before [`Fill::tile`]; [`FillSettings`] is no
/// longer `Copy` and [`TileStats`] is non-exhaustive; a plant grows past
/// its spec's years to its old ages. 6: [`Cover`] gained `floor` (each
/// share of a cell's forest floor, [`Floor`]) and is non-exhaustive, made
/// with [`Cover::of`]; [`TilePlan`] gained `logs` ([`Log`]) and
/// `floor_mosses`, the genera [`FloorMoss`] names, and `day`, the day of
/// the year it shows; [`TileStats`] gained `crowns_in_leaf`, and a
/// [`CanopyCell`]'s cover and colour follow the deciduous crowns' leaves
/// on that day.
pub const API_VERSION: u32 = 6;

/// The name of the file listing a set of variants ([`VariantSet`]).
pub const VARIANTS_FILE: &str = "variants.json";

/// Why something here failed, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<package::PackageError> for Error {
    fn from(error: package::PackageError) -> Self {
        Self(error.to_string())
    }
}

impl From<crate::spec::SpecError> for Error {
    fn from(error: crate::spec::SpecError) -> Self {
        Self(error.to_string())
    }
}

/// The ids of the built-in species, by family, genus and id: what
/// [`Generator::new`] takes.
#[must_use]
pub fn species() -> Vec<String> {
    Library::builtin()
        .species()
        .iter()
        .map(|entry| entry.id.clone())
        .collect()
}

/// How a [`Generator`] draws its species. Start from
/// [`Options::default`] and set what you choose.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Options {
    /// The detail of its levels and impostors: [`quality::STANDARD`], as
    /// packages are built, unless chosen.
    pub quality: Quality,
    /// The day of the year (1 to 365) its seasoned organs show: flowers
    /// open, fruit ripe or leaves gone. A package's day unless chosen.
    pub day: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            quality: quality::STANDARD,
            day: package::DEFAULT_DAY,
        }
    }
}

/// One species, ready to grow, export and package. Making one reads its
/// spec and draws its organs' textures once; every plant grown from it
/// after shares them, so a generator is worth keeping for many plants.
#[derive(Debug, Clone)]
pub struct Generator {
    inputs: Inputs,
    library: &'static Library,
}

impl Generator {
    /// The built-in species `id` (or the id it was renamed from).
    ///
    /// # Errors
    ///
    /// When the library has no such species, or its spec does not load.
    pub fn new(id: &str, options: &Options) -> Result<Self, Error> {
        let spec = Library::builtin().spec(id)?;
        Self::of(&spec, options)
    }

    /// A spec of your own, grown on the program its taxa name in the
    /// built-in library (a spec loaded with [`PlantSpec::from_json`]).
    ///
    /// # Errors
    ///
    /// When the spec is not valid or names no program the library has.
    pub fn of(spec: &PlantSpec, options: &Options) -> Result<Self, Error> {
        Self::of_in(spec, Library::builtin(), options)
    }

    /// [`Self::of`], with the program, a guest's host and its typical site
    /// from `library`, such as your own folder of species on top of the
    /// built-in ones ([`Library::from_dir`]).
    ///
    /// # Errors
    ///
    /// As [`Self::of`].
    pub fn of_in(
        spec: &PlantSpec,
        library: &'static Library,
        options: &Options,
    ) -> Result<Self, Error> {
        if !(1.0..=365.0).contains(&options.day) {
            return Err(Error(format!(
                "a day of the year is 1 to 365, found {}",
                options.day
            )));
        }
        let inputs = Inputs::on_day_in(spec, library, &options.quality, options.day)?;
        Ok(Self { inputs, library })
    }

    /// The species' spec, as its taxa and its own file give it.
    #[must_use]
    pub fn spec(&self) -> &PlantSpec {
        &self.inputs.spec
    }

    /// What the species' roots ask of the soil (its program's `soil@1`
    /// settings with its first seed's parameters), or `None` when its
    /// program has no soil tool or does not compile.
    #[must_use]
    pub fn soil_need(&self) -> Option<crate::lsys::tools::SoilNeed> {
        let spec = &self.inputs.spec;
        let (program, params) = spec.program_in(self.library).ok()?;
        let seed = spec.variants.seeds.first().copied().unwrap_or(1);
        let params = spec.seeded_params(&program, &params, seed);
        crate::lsys::tools::SoilNeed::of_program(&program, &params)
    }

    /// The package key: a hash of the spec, its program, the quality, the
    /// day and [`GENERATOR_REVISION`](crate::GENERATOR_REVISION).
    #[must_use]
    pub fn key(&self) -> &str {
        &self.inputs.key
    }

    /// The age, in years, a plant is grown to unless one is chosen: the
    /// oldest its package keeps.
    #[must_use]
    pub fn default_age(&self) -> f64 {
        export::oldest_age(&self.inputs.spec)
    }

    /// The setting a plant grows in unless one is chosen: its typical
    /// site's.
    #[must_use]
    pub fn default_environment(&self) -> Environment {
        crate::drawing::typical_environment(&self.inputs.spec, self.library)
    }

    /// Plant `seed` in its typical setting at [`Self::default_age`].
    ///
    /// # Errors
    ///
    /// When the plant does not grow.
    pub fn grow(&self, seed: u64) -> Result<Plant, Error> {
        self.grow_in(self.default_environment(), seed, self.default_age())
    }

    /// Plant `seed` grown in `environment` and kept at `age` years. Any
    /// seed grows a plant; those the spec lists are its package's.
    ///
    /// # Errors
    ///
    /// When the age is not above 0 and within the years the spec grows
    /// or its old ages ([`PlantSpec::old_ages`]: doubling past its last
    /// keyframe up to its lifespan), or the plant does not grow.
    pub fn grow_in(&self, environment: Environment, seed: u64, age: f64) -> Result<Plant, Error> {
        Ok(export::grow_seed(&self.inputs, environment, seed, age)?)
    }

    /// Plant `seed` grown in `environment`, kept at each of `ages`
    /// (rising) from one growth: cheaper than [`Self::grow_in`] at each.
    ///
    /// # Errors
    ///
    /// When an age is not above 0 and within the years the spec grows or
    /// its old ages, the ages do not rise, or the plant does not grow.
    pub fn grow_ages_in(
        &self,
        environment: Environment,
        seed: u64,
        ages: &[f64],
    ) -> Result<Vec<Plant>, Error> {
        Ok(export::grow_seed_ages(
            &self.inputs,
            environment,
            seed,
            ages,
        )?)
    }

    /// Plant `seed` come up beneath a closed canopy `canopy_m` tall, as a
    /// stand's shade-tolerant ingrowth does, kept at each of `ages`
    /// (rising) from one growth ([`export::grow_beneath`]): its like's
    /// crowns around it as in [`Environment::Interior`], and the canopy's
    /// shade over it ([`crate::spec::UNDERSTOREY_DEPTH`]).
    ///
    /// # Errors
    ///
    /// As [`Self::grow_ages_in`], and when `canopy_m` is not above 0.
    pub fn grow_ages_beneath(
        &self,
        canopy_m: f64,
        seed: u64,
        ages: &[f64],
    ) -> Result<Vec<Plant>, Error> {
        Ok(export::grow_beneath(&self.inputs, canopy_m, seed, ages)?)
    }

    /// A climber's `seed` grown at the foot of a wall `wall_m` high and
    /// kept at each of `ages` (rising), from one growth
    /// ([`export::grow_on_wall`]): the wall's face is the plane z = -0.3
    /// of the plant's frame, facing +z.
    ///
    /// # Errors
    ///
    /// As [`Self::grow_ages_in`], and when `wall_m` is not above 0.
    pub fn grow_ages_on_wall(
        &self,
        seed: u64,
        ages: &[f64],
        wall_m: f64,
    ) -> Result<Vec<Plant>, Error> {
        Ok(export::grow_on_wall(&self.inputs, seed, ages, wall_m)?)
    }

    /// The looks the cards of every plant it draws index
    /// ([`Generator::draw`]'s second part).
    #[must_use]
    pub fn looks(&self) -> &[crate::looks::Look] {
        &self.inputs.looks
    }

    /// `plant` drawn at level `level` of its quality's levels of detail
    /// (0 the finest; past the last, the last), on the generator's day,
    /// as a package draws it; and the looks its cards' templates index.
    /// For a renderer that draws many plants at once
    /// ([`crate::preview::render_on_site`]): it gathers each species'
    /// looks once and shifts its cards' templates past the looks before.
    #[must_use]
    pub fn draw(
        &self,
        plant: &Plant,
        level: usize,
    ) -> (crate::mesh::PlantMesh, &[crate::looks::Look]) {
        let inputs = &self.inputs;
        let lods = &inputs.quality.lods;
        let level = level.min(lods.len().saturating_sub(1));
        let staged = crate::looks::staged(&plant.graph, &inputs.sizes, &inputs.looks);
        let drawn = staged.as_ref().unwrap_or(&plant.graph);
        let (mesh, _) = package::level_mesh(
            drawn,
            inputs,
            plant.seed,
            &lods[level].for_height(plant.graph.height),
            (level, 0.0),
            None,
            false,
        );
        (mesh, &inputs.looks)
    }

    /// A grown plant as glTF binary files, one per level of detail, and
    /// the JSON index naming them; [`export::write`] writes them.
    ///
    /// # Errors
    ///
    /// When the plant cannot be drawn as the settings say.
    pub fn export(&self, plant: &Plant, settings: &Settings) -> Result<Export, Error> {
        Ok(export::export(&self.inputs, plant, settings, None)?)
    }

    /// [`Self::export`], each level also cut into `regions` (each level's
    /// [`Level::parts`], written beside it as `STEM-<region>-lodN.glb`), so
    /// a renderer can draw each part of a tall plant at the detail its own
    /// distance asks for. [`crown_regions`] gives a tree's: its bole and
    /// its crown in bands, each limb whole in the part where it leaves the
    /// stem.
    ///
    /// # Errors
    ///
    /// As [`Self::export`], and when the plant moves (a motion package's
    /// bones span the parts).
    pub fn export_parts(
        &self,
        plant: &Plant,
        settings: &Settings,
        regions: &[Region],
    ) -> Result<Export, Error> {
        Ok(export::export_parts(
            &self.inputs,
            plant,
            settings,
            regions,
        )?)
    }

    /// Build the species' package (every variant its spec lists, at every
    /// keyframe, with levels, impostors and textures) into `out`, unless
    /// it is there already; its directory.
    ///
    /// # Errors
    ///
    /// When a variant does not grow or the package cannot be written.
    pub fn package(&self, out: &Path) -> Result<PathBuf, Error> {
        if let Some(existing) = package::existing(out, &self.inputs.spec.id, &self.inputs.key) {
            return Ok(existing);
        }
        let built = package::build(&self.inputs, package::default_threads(), &mut |_| {})?;
        Ok(package::write(&built, out)?)
    }

    /// Grow, export and write each plant of `variants` into `out`, as
    /// many at once as `variants.threads`, each written as soon as it is
    /// drawn; then the list of them, [`VARIANTS_FILE`].
    ///
    /// # Errors
    ///
    /// When a plant does not grow or a file cannot be written; plants
    /// already written stay.
    pub fn write_variants(&self, variants: &Variants, out: &Path) -> Result<VariantSet, Error> {
        self.write_variants_reporting(variants, out, &|_| {})
    }

    /// [`Self::write_variants`], telling `report` of each plant as it is
    /// written, in the order they finish.
    ///
    /// # Errors
    ///
    /// As [`Self::write_variants`].
    pub fn write_variants_reporting(
        &self,
        variants: &Variants,
        out: &Path,
        report: &(dyn Fn(&VariantRecord) + Sync),
    ) -> Result<VariantSet, Error> {
        if variants.count == 0 {
            return Err(Error("a set of variants has at least one plant".into()));
        }
        let environment = variants
            .environment
            .unwrap_or_else(|| self.default_environment());
        let age = variants.age.unwrap_or_else(|| self.default_age());
        let seeds: Vec<u64> = (0..variants.count)
            .map(|index| variants.first_seed.saturating_add(index as u64))
            .collect();
        std::fs::create_dir_all(out)
            .map_err(|error| Error(format!("cannot create {}: {error}", out.display())))?;
        let threads = variants.threads.clamp(1, seeds.len());
        let next = Mutex::new(0_usize);
        let done: Mutex<Vec<Option<VariantRecord>>> = Mutex::new(vec![None; seeds.len()]);
        let failure: Mutex<Option<Error>> = Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    loop {
                        let index = {
                            let mut next = next
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let failed = failure
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .is_some();
                            if *next >= seeds.len() || failed {
                                return;
                            }
                            *next += 1;
                            *next - 1
                        };
                        match self.write_one(
                            environment,
                            seeds[index],
                            age,
                            &variants.settings,
                            out,
                        ) {
                            Ok(record) => {
                                report(&record);
                                done.lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)[index] =
                                    Some(record);
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
        let plants: Vec<VariantRecord> = done
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .into_iter()
            .flatten()
            .collect();
        let set = VariantSet::of(self, environment, age, plants);
        let path = out.join(VARIANTS_FILE);
        let text = serde_json::to_string_pretty(&set)
            .map_err(|error| Error(format!("cannot write {}: {error}", path.display())))?;
        std::fs::write(&path, text + "\n")
            .map_err(|error| Error(format!("cannot write {}: {error}", path.display())))?;
        Ok(set)
    }

    pub(crate) fn write_one(
        &self,
        environment: Environment,
        seed: u64,
        age: f64,
        settings: &Settings,
        out: &Path,
    ) -> Result<VariantRecord, Error> {
        self.write_one_in(environment, seed, age, settings, out, false)
            .map(|(record, _)| record)
    }

    /// [`Self::write_one`], and with `parts` each level cut into the
    /// plant's [`crown_regions`]: the regions and each one's files.
    pub(crate) fn write_one_in(
        &self,
        environment: Environment,
        seed: u64,
        age: f64,
        settings: &Settings,
        out: &Path,
        parts: bool,
    ) -> Result<(VariantRecord, Vec<PartRecord>), Error> {
        let plant = self.grow_in(environment, seed, age)?;
        self.write_plant(&plant, settings, out, parts)
    }

    /// Export `plant` into `out` as [`Self::write_one_in`] does.
    pub(crate) fn write_plant(
        &self,
        plant: &Plant,
        settings: &Settings,
        out: &Path,
        parts: bool,
    ) -> Result<(VariantRecord, Vec<PartRecord>), Error> {
        let seed = plant.seed;
        let regions = if parts {
            crown_regions(&plant.size)
        } else {
            Vec::new()
        };
        let exported = self.export_parts(plant, settings, &regions)?;
        let written = export::write(&exported, out)?;
        let part_records = regions
            .iter()
            .enumerate()
            .map(|(index, region)| PartRecord {
                region: region.clone(),
                levels: exported
                    .levels
                    .iter()
                    .enumerate()
                    .filter_map(|(level_index, level)| {
                        level.parts.get(index).map(|part| LevelFile {
                            level: level_index,
                            file: part.file.clone(),
                            bytes: part.glb.len(),
                            triangles: part.summary.triangles,
                        })
                    })
                    .collect(),
            })
            .collect();
        let name = |path: &PathBuf| {
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        };
        let record = VariantRecord {
            seed,
            stem: exported.stem.clone(),
            packaged: plant.variant < self.inputs.spec.variant_list().len(),
            size: plant.size.clone(),
            levels: exported
                .levels
                .iter()
                .enumerate()
                .map(|(level_index, level)| LevelFile {
                    level: level_index,
                    file: level.file.clone(),
                    bytes: level.glb.len(),
                    triangles: level.summary.triangles,
                })
                .collect(),
            index: written.last().map(name).unwrap_or_default(),
            fingerprint: fingerprint(&exported),
        };
        Ok((record, part_records))
    }
}

/// SHA-256 of every level's bytes, in order: two plants with the same
/// fingerprint are drawn alike at every level.
fn fingerprint(exported: &Export) -> String {
    let mut hasher = Sha256::new();
    for level in &exported.levels {
        hasher.update((level.glb.len() as u64).to_le_bytes());
        hasher.update(&level.glb);
    }
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// What [`Generator::write_variants`] makes. Start from
/// [`Variants::count`] and set what you choose.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Variants {
    /// How many plants.
    pub count: usize,
    /// The first plant's seed; the others follow it one by one. 1 unless
    /// chosen, so the first plant is the species' typical one.
    pub first_seed: u64,
    /// The setting they grow in; the species' typical one unless chosen.
    pub environment: Option<Environment>,
    /// Their age, in years; the oldest its package keeps unless chosen.
    pub age: Option<f64>,
    /// How each is written: uncompressed, its levels drawn as tiers (max,
    /// high, mid and low; [`Budget::tiers`]), unless chosen.
    pub settings: Settings,
    /// How many plants to grow at once; the result does not depend on it.
    pub threads: usize,
}

impl Variants {
    /// `count` plants from seed 1, in the typical setting at the oldest
    /// age, uncompressed, drawn as tiers, as many at once as there are
    /// cores.
    #[must_use]
    pub fn count(count: usize) -> Self {
        Self {
            count,
            first_seed: 1,
            environment: None,
            age: None,
            settings: Settings {
                budget: Budget::tiers(),
                ..Settings::default()
            },
            threads: package::default_threads(),
        }
    }
}

/// The list a set of variants is written with ([`VARIANTS_FILE`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantSet {
    pub species: String,
    pub generator_revision: u32,
    pub api_version: u32,
    pub key: String,
    pub environment: String,
    pub age: f64,
    pub day: f64,
    pub quality: String,
    pub plants: Vec<VariantRecord>,
    /// Seeds whose plant is drawn exactly as an earlier seed's, at every
    /// level: none, unless the species grows alike from every seed.
    pub duplicates: Vec<u64>,
}

impl VariantSet {
    fn of(
        generator: &Generator,
        environment: Environment,
        age: f64,
        plants: Vec<VariantRecord>,
    ) -> Self {
        let mut seen = std::collections::BTreeSet::new();
        let duplicates = plants
            .iter()
            .filter(|plant| !seen.insert(plant.fingerprint.clone()))
            .map(|plant| plant.seed)
            .collect();
        let inputs = &generator.inputs;
        Self {
            species: inputs.spec.id.clone(),
            generator_revision: crate::GENERATOR_REVISION,
            api_version: API_VERSION,
            key: inputs.key.clone(),
            environment: environment.name().to_owned(),
            age,
            day: inputs.day.unwrap_or(package::DEFAULT_DAY),
            quality: inputs.quality.name.to_owned(),
            plants,
            duplicates,
        }
    }
}

/// One plant of a set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantRecord {
    pub seed: u64,
    /// Its files' common name: `<species>-<environment>-<seed>-<age>y`.
    pub stem: String,
    /// Whether it is a variant the species' package keeps.
    pub packaged: bool,
    pub size: Measurements,
    pub levels: Vec<LevelFile>,
    /// The export's JSON index, naming every level.
    pub index: String,
    /// SHA-256 over every level's bytes, in order.
    pub fingerprint: String,
}

/// One region of a plant cut into parts ([`Generator::export_parts`]):
/// its box and its files, a level each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartRecord {
    pub region: Region,
    pub levels: Vec<LevelFile>,
}

/// One level's GLB file, 0 the nearest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelFile {
    pub level: usize,
    pub file: String,
    pub bytes: usize,
    pub triangles: usize,
}

/// A site a host hands over: a plain triangle mesh, a material per
/// triangle as an index into a table of [`Surface`]s, and the paved
/// tiles laid on it, with no files and no OBJ. Positions are metres,
/// +Y up and -Z north unless `z_up` (then +Z up and +Y north, as CAD and
/// GIS tools often have it); triangles wind counter-clockwise seen from
/// outside.
#[derive(Debug, Clone, Copy)]
pub struct SiteInput<'a> {
    pub positions: &'a [[f64; 3]],
    pub triangles: &'a [[u32; 3]],
    /// One per triangle: an index into `table`.
    pub materials: &'a [u32],
    pub table: &'a [Surface],
    pub pavements: &'a [CrackTile],
    pub z_up: bool,
}

/// The site mesh of `input`, as growth reads it.
///
/// # Errors
///
/// On a material index past the table, or as
/// [`crate::site::SiteMesh::with_surfaces`] and [`crate::pavement::Pavement::new`].
pub fn site_mesh(input: &SiteInput<'_>) -> Result<crate::site::SiteMesh, Error> {
    if input.materials.len() != input.triangles.len() {
        return Err(Error(format!(
            "a site of {} triangles has {} materials",
            input.triangles.len(),
            input.materials.len()
        )));
    }
    let surfaces = input
        .materials
        .iter()
        .map(|&m| {
            input.table.get(m as usize).copied().ok_or_else(|| {
                Error(format!(
                    "site material {m} is past the table of {}",
                    input.table.len()
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let positions: Vec<crate::math::Vec3> = input
        .positions
        .iter()
        .map(|&[x, y, z]| {
            if input.z_up {
                crate::math::Vec3::new(x, z, -y)
            } else {
                crate::math::Vec3::new(x, y, z)
            }
        })
        .collect();
    let mut mesh = crate::site::SiteMesh::with_surfaces(&positions, input.triangles, &surfaces)
        .map_err(Error)?;
    for tile in input.pavements {
        mesh.add_pavement(crate::pavement::Pavement::new(tile).map_err(Error)?);
    }
    Ok(mesh)
}

/// The site field ([`SiteField`]) of the tile from `low` to `high` (X and
/// Z, metres, in the frame growth uses: +Y up, -Z north) of the site
/// `input`, measured as `settings` say. Build it once per tile and keep
/// it: the same inputs give the same field, and its JSON
/// ([`SiteField::to_json`]) is versioned ([`SITE_FIELD_VERSION`]) and
/// documented in `docs/sites/SITE-FIELD.md`. Hand the mesh round the tile
/// too, as far as `settings.margin_m` reaches, so neighbouring tiles
/// measure their shared edge alike.
///
/// # Errors
///
/// As [`site_mesh`] and [`crate::site_field::build`].
pub fn site_field(
    input: &SiteInput<'_>,
    low: [f64; 2],
    high: [f64; 2],
    settings: &FieldSettings,
) -> Result<SiteField, Error> {
    let mesh = site_mesh(input)?;
    crate::site_field::build(&mesh, low, high, settings).map_err(Error)
}

/// A site material by its name: `soil`, `rock`, `bark`, `wood` or
/// `built`, for a [`Surface`].
///
/// # Errors
///
/// On any other name.
pub fn site_material(name: &str) -> Result<u8, Error> {
    crate::site::material_named(name).map_err(Error)
}

pub use crate::pavement::CrackTile;
pub use crate::site::Surface;
pub use crate::site_field::{
    CrackSite, FieldSettings, Ground, Host, HostKind, Patch, SITE_FIELD_VERSION, SiteField,
};
