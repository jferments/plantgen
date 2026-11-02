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
use crate::package::{self, Inputs, Measurements};
use crate::quality;

pub use crate::export::{Attributes, Budget, Compression, Export, Level, Plant, Settings, Shade};
pub use crate::quality::Quality;
pub use crate::spec::{Environment, PlantSpec};

/// The version of this module's items. It goes up when one of them is
/// renamed, removed, or changes its arguments or meaning; items added
/// beside them leave it as it is. 2: [`Budget`] gained `tiers` and is
/// made with [`Budget::new`] or [`Budget::tiers`], so fields added later
/// break nothing; [`Variants`] draws tiers unless chosen.
pub const API_VERSION: u32 = 2;

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
    /// When the age is not above 0 and within the years the spec grows,
    /// or the plant does not grow.
    pub fn grow_in(&self, environment: Environment, seed: u64, age: f64) -> Result<Plant, Error> {
        Ok(export::grow_seed(&self.inputs, environment, seed, age)?)
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

    fn write_one(
        &self,
        environment: Environment,
        seed: u64,
        age: f64,
        settings: &Settings,
        out: &Path,
    ) -> Result<VariantRecord, Error> {
        let plant = self.grow_in(environment, seed, age)?;
        let exported = self.export(&plant, settings)?;
        let written = export::write(&exported, out)?;
        let name = |path: &PathBuf| {
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        };
        Ok(VariantRecord {
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
        })
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

/// One level's GLB file, 0 the nearest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelFile {
    pub level: usize,
    pub file: String,
    pub bytes: usize,
    pub triangles: usize,
}
