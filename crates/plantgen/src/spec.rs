//! Species specifications: a species as data.
//!
//! A [`PlantSpec`] names a plant program and overrides its parameters, says
//! how long to grow and which ages to keep, which growth environments and
//! seeds to build variants for, how the plant looks, and where every value
//! came from. Specs are JSON files; the built-in ones are the library's
//! ([`crate::library`]), filed by family and genus under `library/`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::body::BodyLook;
use crate::conditions::{ClassKey, Conditions};
pub use crate::evidence::{Evidence, FieldEvidence, Provenance, Source, SourceKind, SourceRef};
use crate::library::{Entry, Library};
use crate::looks::{self, Flare, Look, Moss, OrganLook, Ridges};
use crate::lsys::program::SymbolKind;
use crate::lsys::{Neighbourhood, OrganKind, Program, ProgramError, tools};

pub const SPEC_SCHEMA: u32 = 1;

/// Built-in plant programs, by name.
pub const PROGRAMS: [(&str, &str); 16] = [
    ("conifer", include_str!("../programs/conifer.lsys")),
    ("broadleaf", include_str!("../programs/broadleaf.lsys")),
    ("grass", include_str!("../programs/grass.lsys")),
    ("herb", include_str!("../programs/herb.lsys")),
    ("succulent", include_str!("../programs/succulent.lsys")),
    ("rosette", include_str!("../programs/rosette.lsys")),
    ("fern", include_str!("../programs/fern.lsys")),
    ("palm", include_str!("../programs/palm.lsys")),
    ("cane", include_str!("../programs/cane.lsys")),
    ("sedge", include_str!("../programs/sedge.lsys")),
    ("aquatic", include_str!("../programs/aquatic.lsys")),
    ("climber", include_str!("../programs/climber.lsys")),
    ("epiphyte", include_str!("../programs/epiphyte.lsys")),
    ("cushion", include_str!("../programs/cushion.lsys")),
    ("sapindaceae", include_str!("../programs/sapindaceae.lsys")),
    ("pinaceae", include_str!("../programs/pinaceae.lsys")),
];

/// A built-in program's source, by name.
#[must_use]
pub fn builtin_program(name: &str) -> Option<&'static str> {
    Library::builtin().program(name)
}

/// Every species of the built-in library ([`crate::library`]), by id
/// with its spec's JSON, sorted by family, genus and id.
pub fn all_species() -> impl Iterator<Item = (&'static str, &'static str)> {
    Library::builtin()
        .species()
        .iter()
        .map(|entry| (entry.id.as_str(), entry.source()))
}

/// A built-in species' spec, as JSON, by id.
#[must_use]
pub fn builtin_species(id: &str) -> Option<&'static str> {
    Library::builtin().entry(id).map(Entry::source)
}

/// How far an organ type's shading area may stray from the leaf area its
/// look draws, as a share of the drawn area, before `plantc atlas` points
/// it out. Every built-in species keeps within it.
pub const AREA_TOLERANCE: f64 = 0.25;

/// Shading area of each organ type 1 m long, m², in the order of
/// [`Program::organs`]: what the light and pipe tools count for it.
///
/// # Errors
///
/// Fails if an area is negative or not finite.
pub fn organ_areas(program: &Program, params: &[f64]) -> Result<Vec<f64>, SpecError> {
    let areas =
        tools::organ_areas(program, params).map_err(|error| SpecError(error.to_string()))?;
    Ok(program
        .symbols
        .iter()
        .zip(areas)
        .filter(|(symbol, _)| matches!(symbol.kind, SymbolKind::Organ { .. }))
        .map(|(_, area)| area)
        .collect())
}

/// Whether a shading area is within [`AREA_TOLERANCE`] of a drawn area.
#[must_use]
pub fn areas_agree(drawn: f64, shaded: f64) -> bool {
    (shaded - drawn).abs() <= AREA_TOLERANCE * drawn
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taxon {
    pub scientific_name: String,
    pub common_name: String,
    /// USDA PLANTS symbol, for example `PSME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usda_symbol: Option<String>,
    /// GBIF backbone taxon key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gbif_key: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthForm {
    /// One dominant stem to the top: most conifers.
    ExcurrentTree,
    /// The stem divides into a spreading crown: most broadleaf trees.
    DecurrentTree,
    ScaleLeavedTree,
    Shrub,
    Graminoid,
    Forb,
    Fern,
    Vine,
    /// A cactus or another plant whose fleshy stems store water and do
    /// the work of leaves: columns, barrels, globes, chollas and prickly
    /// pears.
    StemSucculent,
    /// A rosette of thick leaves on a short or tall stem: agaves, aloes,
    /// yuccas, sotols and desert bromeliads, and with forking stems the
    /// Joshua tree.
    RosetteSucculent,
    /// A crown of large fronds on an unbranched trunk that does not
    /// thicken: palms, cycads and tree ferns.
    Palm,
    /// A plant living on another's branches: Spanish moss.
    Epiphyte,
    /// A hard cushion of packed rosettes: llareta.
    Cushion,
}

/// Which plant program grows the species, and its parameter values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    /// Name of the program that grows it: a built-in one (see
    /// [`PROGRAMS`]) or a library folder's ([`Library`]).
    pub program: String,
    /// Parameter values that replace the program's defaults.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

/// How long to grow and which ages to keep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthPlan {
    /// Years from germination to the oldest keyframe.
    pub years: f64,
    /// Years per derivation step.
    pub step: f64,
    /// Ages to keep, in years. These become the package's age classes.
    pub keyframes: Vec<f64>,
}

/// The synthetic stand a variant grows in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    /// Open-grown: no neighbours, a full crown to the ground.
    Open,
    /// At a stand edge: neighbours on one side.
    Edge,
    /// Inside a closed stand of the same age.
    Interior,
    /// Under an older, taller canopy.
    Suppressed,
}

impl Environment {
    pub const ALL: [Self; 4] = [Self::Open, Self::Edge, Self::Interior, Self::Suppressed];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Edge => "edge",
            Self::Interior => "interior",
            Self::Suppressed => "suppressed",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|environment| environment.name() == name)
    }

    /// The default neighbourhood for this environment.
    #[must_use]
    pub fn neighbourhood(self) -> Neighbourhood {
        let stand = Neighbourhood {
            density: 0.12,
            relative_height: 1.0,
            canopy: 0.0,
            spacing: 5.0,
            one_sided: false,
        };
        match self {
            Self::Open => Neighbourhood::OPEN,
            Self::Edge => Neighbourhood {
                one_sided: true,
                ..stand
            },
            Self::Interior => stand,
            Self::Suppressed => Neighbourhood {
                density: 0.15,
                canopy: 30.0,
                ..stand
            },
        }
    }
}

/// Which variants the compiler builds: every environment with every seed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantPlan {
    pub environments: Vec<Environment>,
    pub seeds: Vec<u64>,
    /// Replacements for the default neighbourhoods.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub neighbourhoods: BTreeMap<Environment, Neighbourhood>,
    /// Growing conditions to build variants in as well, each grown with
    /// every seed and labelled by its preset ([`crate::conditions`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Conditions>,
}

/// One variant to grow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub environment: Environment,
    pub seed: u64,
    pub neighbourhood: Neighbourhood,
    /// The condition class of a variant grown in a conditions document;
    /// `None` for an environment's.
    #[serde(skip)]
    pub class: Option<ClassKey>,
}

impl Variant {
    /// The conditions it grows in: its environment, with its neighbourhood.
    #[must_use]
    pub fn conditions(&self) -> Conditions {
        Conditions::in_neighbourhood(self.environment, self.neighbourhood)
    }
}

/// How the plant looks. Colours are linear RGB in 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub bark: [f32; 3],
    /// Colour of organs in full light, unless their look gives one.
    pub foliage: [f32; 3],
    /// Colour of organs in deep shade; shaded organs blend toward it.
    pub foliage_shade: [f32; 3],
    /// How strongly organ colour varies between organs, 0 to 1.
    #[serde(default)]
    pub variation: f32,
    /// How each organ of the program looks, by organ name. Organs not
    /// listed get their kind's default look (see [`crate::looks`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub organs: BTreeMap<String, OrganLook>,
    /// Furrowed bark on thick wood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ridges: Option<Ridges>,
    /// A flared or buttressed stem base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flare: Option<Flare>,
    /// Moss on thick wood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moss: Option<Moss>,
    /// A swollen trunk (plant forms F5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottle: Option<crate::looks::Bottle>,
    /// Roots above the ground (plant forms F6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<crate::roots::Roots>,
    /// The bark's fissures, plates, strips, scales or lenticels, drawn
    /// pixel by pixel on the wood (plant roadmap P4, `crate::bark`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bark_pattern: Option<crate::bark::BarkPattern>,
    /// How each fleshy body of the program looks, by body name: ribs,
    /// areoles, spines (see [`crate::body`]). Bodies not listed get the
    /// default look.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, BodyLook>,
}

impl Appearance {
    /// The numbers its bark pattern is drawn from, if it has one
    /// ([`crate::bark::BarkPattern::params`]).
    #[must_use]
    pub fn bark_params(&self) -> Option<crate::bark::BarkParams> {
        self.bark_pattern
            .as_ref()
            .map(|pattern| pattern.params(self.bark))
    }

    /// The look of each of a program's organ types, in its declaration
    /// order (see [`Program::organs`]).
    #[must_use]
    pub fn looks<'a>(&self, organs: impl IntoIterator<Item = (&'a str, OrganKind)>) -> Vec<Look> {
        looks::resolve(organs, &self.organs, self.foliage, self.foliage_shade)
    }

    /// The looks of [`Appearance::looks`] on day `day` of the year, with
    /// each organ type's size then as a share of its size in fruit (0 when
    /// gone); see [`looks::resolve_on`].
    #[must_use]
    pub fn looks_on<'a>(
        &self,
        organs: impl IntoIterator<Item = (&'a str, OrganKind)>,
        day: Option<f64>,
    ) -> (Vec<Look>, Vec<f64>) {
        looks::resolve_on(organs, &self.organs, self.foliage, self.foliage_shade, day)
    }

    /// Whether any organ has a season ([`looks::Season`]): then the day a
    /// package is grown for changes it.
    #[must_use]
    pub fn has_seasons(&self) -> bool {
        self.organs.values().any(|look| look.season.is_some())
    }

    /// The look of each of a program's body types, in its declaration
    /// order (see [`Program::bodies`]).
    #[must_use]
    pub fn body_looks<'a>(&self, bodies: impl IntoIterator<Item = &'a str>) -> Vec<BodyLook> {
        bodies
            .into_iter()
            .map(|name| self.bodies.get(name).cloned().unwrap_or_default())
            .collect()
    }

    /// Check colours, looks and wood settings.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        let colours = [self.bark, self.foliage, self.foliage_shade];
        if colours.iter().flatten().any(|c| !(0.0..=1.0).contains(c))
            || !(0.0..=1.0).contains(&self.variation)
        {
            return Err("appearance colours and variation must be between 0 and 1".into());
        }
        for (organ, look) in &self.organs {
            look.validate()
                .map_err(|message| format!("appearance.organs.{organ}: {message}"))?;
        }
        if let Some(ridges) = &self.ridges {
            ridges
                .validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        if let Some(flare) = &self.flare {
            flare
                .validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        if let Some(moss) = &self.moss {
            moss.validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        if let Some(pattern) = &self.bark_pattern {
            pattern
                .validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        if let Some(bottle) = &self.bottle {
            bottle
                .validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        if let Some(roots) = &self.roots {
            roots
                .validate()
                .map_err(|message| format!("appearance: {message}"))?;
        }
        for (body, look) in &self.bodies {
            look.validate()
                .map_err(|message| format!("appearance.bodies.{body}: {message}"))?;
        }
        Ok(())
    }
}

/// A reference size at an age, used by `plantc check` to compare the grown
/// plant with measurements or yield tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllometryPoint {
    pub age: f64,
    pub environment: Environment,
    /// Total height, metres.
    pub height: f64,
    /// Stem diameter at 1.3 m, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbh: Option<f64>,
    /// How far the measured value may be from the grown one, as a fraction.
    pub tolerance: f64,
}

/// How much a spec has been checked against reality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    /// Plausible shape from a generic program; not fitted to data.
    ProceduralProxy,
    /// Parameters fitted to reference sizes and photographs.
    Calibrated,
    /// Fitted to measured plants (scans, inventories).
    Measured,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlantSpec {
    pub schema: u32,
    /// Stable species id: lowercase letters, digits and hyphens.
    pub id: String,
    pub taxon: Taxon,
    pub growth_form: GrowthForm,
    pub generator: Generator,
    pub growth: GrowthPlan,
    pub variants: VariantPlan,
    pub appearance: Appearance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allometry: Vec<AllometryPoint>,
    pub tier: ModelTier,
    /// Evidence per field path, for example `"generator.params.whorl"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, FieldEvidence>,
    pub provenance: Provenance,
    /// The plant a climber, epiphyte or parasite grows on (plant forms F7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostSpec>,
}

/// A guest's host: a built-in species grown in `environment` from `seed`
/// to `age`, one of its keyframes. The guest is grown on that model, and a
/// garden stands that model under it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSpec {
    pub species: String,
    pub age: f64,
    pub environment: Environment,
    pub seed: u64,
}

impl PlantSpec {
    /// The host's wood for `host@1`, grown from the host's spec; `None`
    /// without a host.
    ///
    /// # Errors
    ///
    /// Fails on an unknown host, a host without that variant or keyframe,
    /// or a host that will not grow.
    pub fn host_geometry(
        &self,
    ) -> Result<Option<std::sync::Arc<crate::lsys::tools::Host>>, SpecError> {
        self.host_geometry_in(Library::builtin())
    }

    /// [`PlantSpec::host_geometry`] with the host and its program from
    /// `library`.
    ///
    /// # Errors
    ///
    /// As [`PlantSpec::host_geometry`].
    pub fn host_geometry_in(
        &self,
        library: &Library,
    ) -> Result<Option<std::sync::Arc<crate::lsys::tools::Host>>, SpecError> {
        match self.host_in(library)? {
            Some((spec, source)) => self.host_wood(&spec, &source).map(Some),
            None => Ok(None),
        }
    }

    /// The host's spec and its program's source, from `library`: what a
    /// guest's package hashes and grows its host from. `None` without a
    /// host.
    ///
    /// # Errors
    ///
    /// Fails if the library lacks the host or its program, or the host's
    /// spec is invalid.
    pub fn host_in(&self, library: &Library) -> Result<Option<(PlantSpec, String)>, SpecError> {
        let Some(host) = &self.host else {
            return Ok(None);
        };
        let spec = library
            .spec(&host.species)
            .map_err(|error| SpecError(format!("host {}: {error}", host.species)))?;
        let source = library
            .program(&spec.generator.program)
            .ok_or_else(|| {
                SpecError(format!(
                    "host {}: unknown program `{}`",
                    host.species, spec.generator.program
                ))
            })?
            .to_string();
        Ok(Some((spec, source)))
    }

    /// The wood of this guest's host, grown from the host's `spec` by its
    /// program's `source` (see [`PlantSpec::host_in`]).
    ///
    /// # Errors
    ///
    /// Fails without a host, on a host without the variant or keyframe, or
    /// a host that will not grow.
    pub fn host_wood(
        &self,
        spec: &PlantSpec,
        source: &str,
    ) -> Result<std::sync::Arc<crate::lsys::tools::Host>, SpecError> {
        let host = self
            .host
            .as_ref()
            .ok_or_else(|| SpecError(format!("species `{}` has no host", self.id)))?;
        if spec.host.is_some() {
            return Err(SpecError(format!(
                "host {} has a host of its own",
                host.species
            )));
        }
        if !spec.growth.keyframes.contains(&host.age) {
            return Err(SpecError(format!(
                "host {} has no keyframe at {} years",
                host.species, host.age
            )));
        }
        let variant = spec
            .variant_list()
            .into_iter()
            .find(|variant| variant.environment == host.environment && variant.seed == host.seed)
            .ok_or_else(|| SpecError(format!("host {} has no such variant", host.species)))?;
        let (program, params) = spec.program_from(source)?;
        let growth = crate::grow::grow(
            &program,
            &params,
            &crate::grow::GrowthSettings {
                seed: variant.seed,
                dt: spec.growth.step,
                years: host.age,
                keyframes: vec![host.age],
                conditions: variant.conditions(),
                limits: crate::lsys::Limits::default(),
                host: None,
            },
        )
        .map_err(|error| SpecError(format!("host {}: {error}", host.species)))?;
        let graph = &growth.keyframes[0];
        let capsules = graph
            .segments
            .iter()
            .filter(|segment| segment.body == 0)
            .map(|segment| (segment.start, segment.end, segment.radius))
            .collect();
        Ok(std::sync::Arc::new(crate::lsys::tools::Host::new(capsules)))
    }
}

/// A spec that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError(pub String);

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpecError {}

impl From<ProgramError> for SpecError {
    fn from(error: ProgramError) -> Self {
        Self(error.to_string())
    }
}

impl PlantSpec {
    /// Parse and validate a spec.
    ///
    /// # Errors
    ///
    /// Fails on malformed JSON or any problem [`PlantSpec::validate`] finds.
    pub fn from_json(text: &str) -> Result<Self, SpecError> {
        Self::from_json_in(text, Library::builtin())
    }

    /// Parse a spec and check it against `library`, whose programs and
    /// species it may name.
    ///
    /// # Errors
    ///
    /// Fails if the JSON is malformed or the spec is invalid.
    pub fn from_json_in(text: &str, library: &Library) -> Result<Self, SpecError> {
        let spec: Self = serde_json::from_str(text)
            .map_err(|error| SpecError(format!("invalid spec: {error}")))?;
        spec.validate_in(library)?;
        Ok(spec)
    }

    /// A built-in species by id.
    ///
    /// # Errors
    ///
    /// Fails if there is no such species.
    pub fn builtin(id: &str) -> Result<Self, SpecError> {
        Library::builtin().spec(id)
    }

    /// Check the spec's values against the built-in library.
    ///
    /// # Errors
    ///
    /// Describes the first problem found.
    pub fn validate(&self) -> Result<(), SpecError> {
        self.validate_in(Library::builtin())
    }

    /// Check the spec's values, with `library`'s programs.
    ///
    /// # Errors
    ///
    /// Describes the first problem found.
    pub fn validate_in(&self, library: &Library) -> Result<(), SpecError> {
        let fail = |message: String| Err(SpecError(format!("species `{}`: {message}", self.id)));
        if self.schema != SPEC_SCHEMA {
            return fail(format!(
                "schema {} is not supported; expected {SPEC_SCHEMA}",
                self.schema
            ));
        }
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return fail("the id must be 1 to 64 lowercase letters, digits or hyphens".into());
        }
        let growth = &self.growth;
        if !(growth.step > 0.0 && growth.step <= 10.0) {
            return fail(format!(
                "growth.step must be above 0 and at most 10 years, found {}",
                growth.step
            ));
        }
        if !(growth.years > 0.0 && growth.years <= 2_000.0) {
            return fail(format!(
                "growth.years must be above 0 and at most 2000, found {}",
                growth.years
            ));
        }
        if growth.keyframes.is_empty()
            || growth
                .keyframes
                .iter()
                .any(|age| !(*age >= 0.0 && *age <= growth.years))
        {
            return fail("growth.keyframes must list ages between 0 and growth.years".into());
        }
        if self.variants.environments.is_empty() || self.variants.seeds.is_empty() {
            return fail("variants need at least one environment and one seed".into());
        }
        // Each variant is named by its environment and seed, so a
        // conditions document's preset labels its variants and may not be
        // an environment's or another document's.
        let mut labels = self.variants.environments.clone();
        for (index, conditions) in self.variants.conditions.iter().enumerate() {
            let at = format!("variants.conditions[{index}]");
            if let Err(message) = conditions.validate() {
                return fail(format!("{at}: {message}"));
            }
            let Some(preset) = conditions.preset else {
                return fail(format!(
                    "{at} needs a preset, which names the variants grown in it"
                ));
            };
            if labels.contains(&preset) {
                return fail(format!(
                    "{at} is labelled {}, which another variant of the spec already is",
                    preset.name()
                ));
            }
            labels.push(preset);
            let host = conditions
                .interactions
                .as_ref()
                .and_then(|interactions| interactions.host.as_ref());
            if host.is_some() && host != self.host.as_ref() {
                return fail(format!(
                    "{at}: interactions.host must be the spec's own host"
                ));
            }
        }
        if let Err(message) = self.appearance.validate() {
            return fail(message);
        }
        for point in &self.allometry {
            if !(point.height > 0.0 && point.tolerance > 0.0 && point.age <= growth.years) {
                return fail(format!("allometry at age {} is invalid", point.age));
            }
            if !labels.contains(&point.environment) {
                return fail(format!(
                    "allometry at age {} is for the {} environment, which no variant grows in",
                    point.age,
                    point.environment.name()
                ));
            }
        }
        if self.provenance.sources.is_empty() {
            return fail("provenance needs at least one source".into());
        }
        for (path, note) in &self.evidence {
            if let Err(message) = note.check() {
                return fail(format!("the evidence note on `{path}`: {message}"));
            }
            if let Some(id) = note
                .source
                .as_deref()
                .filter(|id| library.source(id).is_none())
            {
                return fail(format!(
                    "the evidence note on `{path}` cites `{id}`, which is not in the library's sources/"
                ));
            }
        }
        if library.program(&self.generator.program).is_none() {
            let known: Vec<&str> = library.programs().collect();
            return fail(format!(
                "unknown program `{}`; {}programs: {}",
                self.generator.program,
                if library.root().is_none() {
                    "built-in "
                } else {
                    ""
                },
                known.join(", ")
            ));
        }
        Ok(())
    }

    /// Compile the spec's program and resolve its parameters.
    ///
    /// # Errors
    ///
    /// Fails if the program does not compile or a parameter is unknown.
    pub fn program(&self) -> Result<(Program, Vec<f64>), SpecError> {
        self.program_in(Library::builtin())
    }

    /// Compile the spec's program from `library` and resolve its
    /// parameters.
    ///
    /// # Errors
    ///
    /// Fails if the library lacks the program, it does not compile or a
    /// parameter is unknown.
    pub fn program_in(&self, library: &Library) -> Result<(Program, Vec<f64>), SpecError> {
        let source = library
            .program(&self.generator.program)
            .ok_or_else(|| SpecError(format!("unknown program `{}`", self.generator.program)))?;
        self.program_from(source)
    }

    /// Compile `source` in place of the built-in program, with this spec's
    /// parameters: for trying out a changed program before it is built in.
    ///
    /// # Errors
    ///
    /// Fails if the program does not compile or a parameter is unknown.
    pub fn program_from(&self, source: &str) -> Result<(Program, Vec<f64>), SpecError> {
        let program = Program::compile(source)?;
        let params = program.resolve_params(&self.generator.params)?;
        self.check_names(&program)?;
        Ok((program, params))
    }

    /// Check the spec against the program that grows it: every organ type
    /// and body its appearance gives a look is one the program declares.
    ///
    /// # Errors
    ///
    /// Names the first look the program has no use for.
    pub fn check_names(&self, program: &Program) -> Result<(), SpecError> {
        for organ in self.appearance.organs.keys() {
            if !program.organs().any(|(name, _)| name == organ) {
                let declared: Vec<&str> = program.organs().map(|(name, _)| name).collect();
                return Err(SpecError(format!(
                    "species `{}`: appearance.organs names `{organ}`, which program `{}` does not declare; it declares {}",
                    self.id,
                    program.name,
                    if declared.is_empty() {
                        "no organs".to_string()
                    } else {
                        declared.join(", ")
                    }
                )));
            }
        }
        for body in self.appearance.bodies.keys() {
            if !program.bodies().any(|name| name == body) {
                let declared: Vec<&str> = program.bodies().collect();
                return Err(SpecError(format!(
                    "species `{}`: appearance.bodies names `{body}`, which program `{}` does not declare; it declares {}",
                    self.id,
                    program.name,
                    if declared.is_empty() {
                        "no bodies".to_string()
                    } else {
                        declared.join(", ")
                    }
                )));
            }
        }
        Ok(())
    }

    /// Every variant the plan asks for, environments first.
    #[must_use]
    pub fn variant_list(&self) -> Vec<Variant> {
        let mut list = Vec::new();
        for environment in &self.variants.environments {
            for seed in &self.variants.seeds {
                list.push(Variant {
                    environment: *environment,
                    seed: *seed,
                    neighbourhood: self
                        .variants
                        .neighbourhoods
                        .get(environment)
                        .copied()
                        .unwrap_or_else(|| environment.neighbourhood()),
                    class: None,
                });
            }
        }
        for conditions in &self.variants.conditions {
            let Some(environment) = conditions.preset else {
                continue;
            };
            for seed in &self.variants.seeds {
                list.push(Variant {
                    environment,
                    seed: *seed,
                    neighbourhood: conditions.neighbourhood(),
                    class: Some(conditions.class()),
                });
            }
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_species_parse_validate_and_compile() {
        for (id, _) in all_species() {
            let spec = PlantSpec::builtin(id).unwrap();
            assert_eq!(spec.id, id);
            spec.program().unwrap();
            assert!(!spec.variant_list().is_empty());
        }
    }

    #[test]
    fn invalid_specs_are_rejected_with_reasons() {
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        spec.id = "Douglas Fir".into();
        assert!(spec.validate().unwrap_err().0.contains("lowercase"));
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        spec.growth.keyframes.push(10_000.0);
        assert!(spec.validate().is_err());
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        spec.allometry[0].environment = Environment::Suppressed;
        assert!(
            spec.validate()
                .unwrap_err()
                .0
                .contains("no variant grows in")
        );
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        spec.generator
            .params
            .insert("no_such_parameter".into(), 1.0);
        assert!(spec.program().unwrap_err().0.contains("no such parameter"));
        let text = builtin_species("pseudotsuga-menziesii")
            .unwrap()
            .replace("\"tier\"", "\"tear\"");
        assert!(PlantSpec::from_json(&text).is_err());
        // A note's source is a source id the library holds.
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        let note = spec.evidence.get_mut("allometry").unwrap();
        note.source = Some("Not An Id".into());
        assert!(spec.validate().unwrap_err().0.contains("not a source id"));
        let note = spec.evidence.get_mut("allometry").unwrap();
        note.source = Some("no-such-source".into());
        assert!(
            spec.validate()
                .unwrap_err()
                .0
                .contains("the evidence note on `allometry` cites `no-such-source`")
        );
    }

    #[test]
    fn conditions_documents_add_variants_named_by_their_preset() {
        let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
        spec.variants.environments = vec![Environment::Open];
        spec.variants.seeds = vec![1, 2];
        spec.allometry
            .retain(|point| point.environment == Environment::Open);
        let document = Conditions::preset(Environment::Suppressed);
        spec.variants.conditions = vec![document.clone()];
        spec.validate().unwrap();
        let variants = spec.variant_list();
        assert_eq!(variants.len(), 4);
        assert!(variants[..2].iter().all(|variant| variant.class.is_none()));
        for variant in &variants[2..] {
            assert_eq!(variant.environment, Environment::Suppressed);
            assert_eq!(
                variant.neighbourhood,
                Environment::Suppressed.neighbourhood()
            );
            assert_eq!(variant.class, Some(document.class()));
        }

        let refused = |change: &dyn Fn(&mut PlantSpec), expected: &str| {
            let mut changed = spec.clone();
            change(&mut changed);
            let message = changed.validate().unwrap_err().0;
            assert!(message.contains(expected), "{message}");
        };
        refused(
            &|spec| spec.variants.conditions[0].preset = None,
            "variants.conditions[0] needs a preset",
        );
        refused(
            &|spec| spec.variants.conditions[0].preset = Some(Environment::Open),
            "variants.conditions[0] is labelled open, which another variant",
        );
        refused(
            &|spec| spec.variants.conditions[0].schema = 2,
            "variants.conditions[0]: conditions schema 2",
        );
        refused(
            &|spec| {
                spec.variants.conditions[0].interactions = Some(crate::conditions::Interactions {
                    version: 1,
                    host: Some(HostSpec {
                        species: "acer-macrophyllum".into(),
                        age: 30.0,
                        environment: Environment::Open,
                        seed: 1,
                    }),
                    pollination: 1.0,
                    herbivory: 0.0,
                });
            },
            "interactions.host must be the spec's own host",
        );
        // A neighbourhood given in a spec names all five values and no
        // others.
        let text = builtin_species("pseudotsuga-menziesii").unwrap().replace(
            "\"seeds\"",
            "\"neighbourhoods\": {\"open\": {\"density\": 0, \"relative_height\": 0, \"canopy\": 0, \"spacing\": 0, \"one_sided\": false, \"shade\": 1}}, \"seeds\"",
        );
        assert!(
            PlantSpec::from_json(&text)
                .unwrap_err()
                .0
                .contains("unknown field `shade`")
        );
    }

    /// Values the built-ins never needed checking for, refused in a spec
    /// from a file: each message names the value and its range.
    #[test]
    fn looks_out_of_range_or_on_the_wrong_shape_are_refused() {
        fn spray(spec: &mut PlantSpec) -> &mut crate::looks::OrganLook {
            spec.appearance.organs.get_mut("spray").unwrap()
        }
        let refused = |change: &dyn Fn(&mut PlantSpec), expected: &str| {
            let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
            change(&mut spec);
            let message = spec.validate().unwrap_err().0;
            assert!(message.contains(expected), "{message}");
        };
        refused(
            &|spec| {
                spray(spec).bend = Some(crate::bend::Bend {
                    fold: 120.0,
                    ..crate::bend::Bend::FLAT
                });
            },
            "bend fold must be between -90 and 90, found 120",
        );
        refused(
            &|spec| {
                spray(spec).solid = Some(crate::leaves::SolidLeaf {
                    taper: 0.0,
                    ..crate::leaves::SolidLeaf::default()
                });
            },
            "solid taper must be between 0.05 and 10, found 0",
        );
        refused(
            &|spec| {
                spray(spec).solid = Some(crate::leaves::SolidLeaf {
                    levels: 3,
                    ..crate::leaves::SolidLeaf::default()
                });
            },
            "solid levels must be between 0 and 2, found 3",
        );
        refused(
            &|spec| {
                spray(spec).form = Some(crate::blooms::Form::Flower(
                    crate::blooms::FlowerForm::default(),
                ));
            },
            "appearance.organs.spray: form flower draws only on a flower or a head, found needles",
        );
        refused(
            &|spec| {
                spec.appearance.bottle = Some(crate::looks::Bottle {
                    height: 0.0,
                    ..crate::looks::Bottle::default()
                });
            },
            "bottle height must be between 0.1 and 50, found 0",
        );
        refused(
            &|spec| {
                spec.appearance.roots = Some(crate::roots::Roots {
                    prop: 1000,
                    ..crate::roots::Roots::default()
                });
            },
            "roots prop must be between 0 and 256, found 1000",
        );
    }
}
