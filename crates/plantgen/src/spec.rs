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
use crate::substrate::{Substrate, SubstratePreset};

pub const SPEC_SCHEMA: u32 = 1;

/// The top-level keys of a spec that no evidence note covers: its schema
/// and id, its model tier (itself the statement of how far the spec is
/// fitted), and its evidence and provenance.
pub const UNNOTED: &[&str] = &["schema", "id", "tier", "evidence", "provenance"];

/// Built-in plant programs, by name.
pub const PROGRAMS: [(&str, &str); 89] = [
    ("pinidae", include_str!("../programs/pinidae.lsys")),
    ("broadleaf", include_str!("../programs/broadleaf.lsys")),
    ("grass", include_str!("../programs/grass.lsys")),
    ("herb", include_str!("../programs/herb.lsys")),
    ("column", include_str!("../programs/column.lsys")),
    ("cactaceae", include_str!("../programs/cactaceae.lsys")),
    ("rosette", include_str!("../programs/rosette.lsys")),
    (
        "polypodiidae",
        include_str!("../programs/polypodiidae.lsys"),
    ),
    ("palm", include_str!("../programs/palm.lsys")),
    ("cane", include_str!("../programs/cane.lsys")),
    ("sedge", include_str!("../programs/sedge.lsys")),
    (
        "nymphaeaceae",
        include_str!("../programs/nymphaeaceae.lsys"),
    ),
    ("climber", include_str!("../programs/climber.lsys")),
    ("epiphyte", include_str!("../programs/epiphyte.lsys")),
    ("azorella", include_str!("../programs/azorella.lsys")),
    ("sapindaceae", include_str!("../programs/sapindaceae.lsys")),
    (
        "asparagaceae",
        include_str!("../programs/asparagaceae.lsys"),
    ),
    ("aloe", include_str!("../programs/aloe.lsys")),
    ("hechtia", include_str!("../programs/hechtia.lsys")),
    ("cyperaceae", include_str!("../programs/cyperaceae.lsys")),
    ("typha", include_str!("../programs/typha.lsys")),
    ("fouquieria", include_str!("../programs/fouquieria.lsys")),
    (
        "phyllostachys",
        include_str!("../programs/phyllostachys.lsys"),
    ),
    ("tillandsia", include_str!("../programs/tillandsia.lsys")),
    (
        "phoradendron",
        include_str!("../programs/phoradendron.lsys"),
    ),
    ("apiaceae", include_str!("../programs/apiaceae.lsys")),
    ("araceae", include_str!("../programs/araceae.lsys")),
    ("arecaceae", include_str!("../programs/arecaceae.lsys")),
    (
        "aristolochiaceae",
        include_str!("../programs/aristolochiaceae.lsys"),
    ),
    ("asteraceae", include_str!("../programs/asteraceae.lsys")),
    (
        "berberidaceae",
        include_str!("../programs/berberidaceae.lsys"),
    ),
    ("camassia", include_str!("../programs/camassia.lsys")),
    ("carex", include_str!("../programs/carex.lsys")),
    ("cyatheaceae", include_str!("../programs/cyatheaceae.lsys")),
    ("cycadaceae", include_str!("../programs/cycadaceae.lsys")),
    ("zamiaceae", include_str!("../programs/zamiaceae.lsys")),
    (
        "marattiaceae",
        include_str!("../programs/marattiaceae.lsys"),
    ),
    (
        "lycopodiopsida",
        include_str!("../programs/lycopodiopsida.lsys"),
    ),
    ("isoetaceae", include_str!("../programs/isoetaceae.lsys")),
    (
        "ophioglossaceae",
        include_str!("../programs/ophioglossaceae.lsys"),
    ),
    ("ephedraceae", include_str!("../programs/ephedraceae.lsys")),
    (
        "equisetaceae",
        include_str!("../programs/equisetaceae.lsys"),
    ),
    ("fabaceae", include_str!("../programs/fabaceae.lsys")),
    ("hedera", include_str!("../programs/hedera.lsys")),
    ("iridaceae", include_str!("../programs/iridaceae.lsys")),
    ("juncaceae", include_str!("../programs/juncaceae.lsys")),
    ("liliaceae", include_str!("../programs/liliaceae.lsys")),
    ("maianthemum", include_str!("../programs/maianthemum.lsys")),
    (
        "melanthiaceae",
        include_str!("../programs/melanthiaceae.lsys"),
    ),
    ("onagraceae", include_str!("../programs/onagraceae.lsys")),
    ("orchidaceae", include_str!("../programs/orchidaceae.lsys")),
    (
        "orobanchaceae",
        include_str!("../programs/orobanchaceae.lsys"),
    ),
    ("oxalidaceae", include_str!("../programs/oxalidaceae.lsys")),
    (
        "papaveraceae",
        include_str!("../programs/papaveraceae.lsys"),
    ),
    ("phrymaceae", include_str!("../programs/phrymaceae.lsys")),
    (
        "plantaginaceae",
        include_str!("../programs/plantaginaceae.lsys"),
    ),
    ("poaceae", include_str!("../programs/poaceae.lsys")),
    (
        "ranunculaceae",
        include_str!("../programs/ranunculaceae.lsys"),
    ),
    (
        "saxifragaceae",
        include_str!("../programs/saxifragaceae.lsys"),
    ),
    (
        "schoenoplectus",
        include_str!("../programs/schoenoplectus.lsys"),
    ),
    ("pereskia", include_str!("../programs/pereskia.lsys")),
    (
        "schlumbergera",
        include_str!("../programs/schlumbergera.lsys"),
    ),
    ("solanaceae", include_str!("../programs/solanaceae.lsys")),
    ("urticaceae", include_str!("../programs/urticaceae.lsys")),
    ("violaceae", include_str!("../programs/violaceae.lsys")),
    ("vitaceae", include_str!("../programs/vitaceae.lsys")),
    ("acanthaceae", include_str!("../programs/acanthaceae.lsys")),
    ("apiales", include_str!("../programs/apiales.lsys")),
    ("asterales", include_str!("../programs/asterales.lsys")),
    ("betulaceae", include_str!("../programs/betulaceae.lsys")),
    (
        "bignoniaceae",
        include_str!("../programs/bignoniaceae.lsys"),
    ),
    (
        "caprifoliaceae",
        include_str!("../programs/caprifoliaceae.lsys"),
    ),
    ("cornaceae", include_str!("../programs/cornaceae.lsys")),
    ("ericaceae", include_str!("../programs/ericaceae.lsys")),
    ("fabales", include_str!("../programs/fabales.lsys")),
    ("fagaceae", include_str!("../programs/fagaceae.lsys")),
    (
        "grossulariaceae",
        include_str!("../programs/grossulariaceae.lsys"),
    ),
    ("malvaceae", include_str!("../programs/malvaceae.lsys")),
    ("moraceae", include_str!("../programs/moraceae.lsys")),
    (
        "nothofagaceae",
        include_str!("../programs/nothofagaceae.lsys"),
    ),
    ("oleaceae", include_str!("../programs/oleaceae.lsys")),
    (
        "ranunculales",
        include_str!("../programs/ranunculales.lsys"),
    ),
    ("rhamnaceae", include_str!("../programs/rhamnaceae.lsys")),
    (
        "rhizophoraceae",
        include_str!("../programs/rhizophoraceae.lsys"),
    ),
    ("rosaceae", include_str!("../programs/rosaceae.lsys")),
    ("salicaceae", include_str!("../programs/salicaceae.lsys")),
    (
        "simmondsiaceae",
        include_str!("../programs/simmondsiaceae.lsys"),
    ),
    ("viburnaceae", include_str!("../programs/viburnaceae.lsys")),
    (
        "zygophyllaceae",
        include_str!("../programs/zygophyllaceae.lsys"),
    ),
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

/// A built-in species' spec, as JSON, by id (an old id finds the species
/// renamed from it).
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
    /// Its name: in a library, the accepted name in the World Checklist of
    /// Vascular Plants (`Pinus contorta var. contorta`), which the taxon's
    /// id writes as an id (`pinus-contorta-var-contorta`).
    pub scientific_name: String,
    /// Its English name, where it has one; a species drafted from facts
    /// may have none ([`Taxon::name`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub common_name: String,
    /// USDA PLANTS symbol, for example `PSME`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usda_symbol: Option<String>,
    /// GBIF backbone taxon key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gbif_key: Option<u64>,
    /// The family of the accepted name in the World Checklist of Vascular
    /// Plants (WCVP), as WCVP writes it (`Pinaceae`); the family folder is
    /// its lower case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// The genus `scientific_name` is written in (`Pseudotsuga`); the genus
    /// folder is its lower case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genus: Option<String>,
    /// WCVP's `plant_name_id` of the accepted taxon: the species, or the
    /// subspecies or variety where the taxon is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plant_name_id: Option<u64>,
}

impl Taxon {
    /// The name to show it by: its common name, or its scientific name
    /// where it has none.
    #[must_use]
    pub fn name(&self) -> &str {
        if self.common_name.is_empty() {
            &self.scientific_name
        } else {
            &self.common_name
        }
    }
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

impl GrowthForm {
    /// Every growth form, in the order they are declared.
    pub const ALL: [Self; 13] = [
        Self::ExcurrentTree,
        Self::DecurrentTree,
        Self::ScaleLeavedTree,
        Self::Shrub,
        Self::Graminoid,
        Self::Forb,
        Self::Fern,
        Self::Vine,
        Self::StemSucculent,
        Self::RosetteSucculent,
        Self::Palm,
        Self::Epiphyte,
        Self::Cushion,
    ];
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
    /// Values for parameters the program leaves valueless, which a rank
    /// file states for a growth form: inheritance moves each into
    /// `params` where no file set it and the program gives it no value,
    /// so an effective spec carries none ([`crate::inherit`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, f64>,
    /// The spreads of parameters rules pass a range of a character of the
    /// whole plant through to, by path (`params.<name>`;
    /// [`crate::inherit::Spread`]), from which each plant draws its own by
    /// its seed (spec rework VARY, [`PlantSpec::seeded_params`]).
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub spreads: serde_json::Map<String, serde_json::Value>,
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

/// How far apart the stems of a stand of the same age stand, as a share of
/// their height (the stand's relative spacing, Hart's index), so the stand
/// thins as its trees grow. Each neighbour is the plant's own crown
/// ([`crate::conditions::Layout`]), so the shade over a crown is what crowns
/// like it cast and lifts as it lifts. The value is the median of fully
/// stocked Douglas-fir stands in McArdle et al. (1961, Tables 1 and 2): the
/// square spacing of their trees per acre over the height of their dominants
/// is 0.102-0.147 across site indices 80-210 and ages 30-160, median 0.115,
/// and rises slowly with age. Grown in it, Douglas-fir is 33.5 m and 32.5 cm
/// across at 60 years where the tables' average tree at that height (site
/// index 140) is 33.2 m and 28.2 cm; at a quarter, the value fitted to
/// old-growth trunks before, it was 59 cm. Trees older than the tables, such
/// as 300-year Douglas-fir 150-180 cm across (FEIS), stand far wider apart
/// than a fully stocked stand, which this spacing does not model. Before it
/// a stand was shade of 0.12 a metre from the ground to the plant's top,
/// which deepened as the plant grew and left any tree only its top 8-10 m of
/// foliage: a 55 cm Douglas-fir with its crown from 51 m up.
pub const STAND_SPACING: f64 = 0.115;

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
        // A stand of the same age is a stand of the plant's like, its
        // crowns spaced by its height (STAND_SPACING).
        let stand = Neighbourhood {
            density: 0.0,
            relative_height: 1.0,
            canopy: 0.0,
            spacing: 0.0,
            one_sided: false,
            like: STAND_SPACING,
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
                relative_height: 1.0,
                canopy: 30.0,
                spacing: 5.0,
                one_sided: false,
                like: 0.0,
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
    /// The substrate preset (and its seed) of a variant grown in a
    /// conditions document that names one (G3); `None` grows on level soil.
    #[serde(skip)]
    pub substrate: Option<(SubstratePreset, u64)>,
}

/// Why a spec's variant cannot grow in `conditions`, if it cannot. A
/// variant keeps a substrate preset and its seed only
/// ([`Variant::conditions`]): a grid, a scaled place and neighbours' crowns
/// come from a host growing one plant in its own world
/// ([`crate::grow::grow_in`]).
fn unfit_for_a_variant(conditions: &Conditions) -> Option<&'static str> {
    let substrate = conditions.substrate.as_ref();
    if substrate.is_some_and(|substrate| substrate.preset.is_none()) {
        Some("a spec's substrate must name a preset")
    } else if substrate.is_some_and(|substrate| substrate.scale.is_some())
        || !conditions.envelopes().is_empty()
    {
        Some("a spec's conditions give no substrate scale and no envelopes")
    } else {
        None
    }
}

impl Variant {
    /// The conditions it grows in: its environment, with its neighbourhood.
    #[must_use]
    pub fn conditions(&self) -> Conditions {
        Conditions {
            substrate: self.substrate.map(|(preset, seed)| Substrate {
                seed: Some(seed),
                ..Substrate::preset(preset)
            }),
            ..Conditions::in_neighbourhood(self.environment, self.neighbourhood)
        }
    }
}

/// The radius below which wood keeps its twig colour when a species
/// states none ([`Appearance::twig_radius`]): 4 mm, a twig about a
/// centimetre thick, so a tree's twigs and finest branches keep it and
/// its limbs wear their bark.
pub const TWIG_RADIUS: f64 = 0.004;

/// How the plant looks. Colours are linear RGB in 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub bark: [f32; 3],
    /// Colour of the wood drawn within a leaf: a frond's petiole and
    /// rachis, a fern's stipe. It is drawn plain, with no bark pattern,
    /// moss or furrows. The bark's colour and bark if none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaf_axis: Option<[f32; 3]>,
    /// Young wood's colour, which it loses as it grows for the bark's or
    /// leaf axis's ([`YoungAxis`]): a fern's green crozier and young
    /// stipe. Young wood is its bark's colour where none is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub young_axis: Option<YoungAxis>,
    /// The hairs on the young stems and the leaf stalks
    /// (`crate::hairs`): they cast the young wood toward their colour,
    /// wearing off as it ages past [`crate::hairs::YOUNG_YEARS`], and
    /// up close stand from it as strands. None where none are stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stem_hairs: Option<crate::looks::Hairs>,
    /// Colour of the young wood where it differs from the bark: a
    /// fuchsia's red shoots, a dogwood's red twigs. The wood is in it its
    /// first year and turns to the bark's as it ages past
    /// [`crate::hairs::YOUNG_YEARS`]. The bark's where none is stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub young_wood: Option<[f32; 3]>,
    /// Colour of young twigs before their bark forms, where it differs
    /// from the bark's: wood thinner than `twig_radius` is this colour,
    /// turning to the bark's by three times that radius, as a paper
    /// birch's dark red twigs turn to its white bark. The bark's colour
    /// throughout if none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twig: Option<[f32; 3]>,
    /// The radius below which wood keeps its twig colour, metres
    /// (default [`TWIG_RADIUS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twig_radius: Option<f64>,
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
    /// The look of each organ kind (`leaf`, `flower`, …) for the organs
    /// the program draws that `organs` does not list, which a rank file
    /// states: inheritance gives each such organ its kind's, so an
    /// effective spec carries none ([`crate::inherit`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, OrganLook>,
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
    /// The bark in the bark language (`crate::bark_look`): its stages by
    /// stem diameter, each baked to a tile for export. Where none is
    /// stated, the bark pattern's ([`Appearance::bark_look`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bark_look: Option<crate::bark_look::BarkSpec>,
    /// How each fleshy body of the program looks, by body name: ribs,
    /// areoles, spines (see [`crate::body`]). Bodies not listed get the
    /// default look.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, BodyLook>,
    /// The spreads of look values rules set from ranges, by path
    /// (`organs.<organ>.<field>`; [`crate::inherit::Spread`]): each organ
    /// drawn solid is one of [`crate::looks::VARIETIES`] varieties of its
    /// look, each drawing its values from them ([`Look::of`]).
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub spreads: serde_json::Map<String, serde_json::Value>,
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

    /// The bark look species `id` draws: the one it states, else the one
    /// made from its bark pattern ([`crate::bark_look::from_pattern`]);
    /// none for a plant with neither, or a palm's ringed stem.
    ///
    /// # Errors
    ///
    /// When the stated look's stages do not make a record
    /// ([`crate::bark_look::BarkSpec::look`]).
    pub fn bark_look(
        &self,
        id: &str,
    ) -> Result<Option<crate::bark_look::BarkLook>, crate::bark_look::BarkLookError> {
        match (&self.bark_look, &self.bark_pattern) {
            (Some(stated), _) => stated.look(id, self.bark).map(Some),
            (None, Some(pattern)) => Ok(crate::bark_look::from_pattern(id, pattern, self.bark)),
            (None, None) => Ok(None),
        }
    }

    /// The look of each of a program's organ types, in its declaration
    /// order (see [`Program::organs`]).
    #[must_use]
    pub fn looks<'a>(&self, organs: impl IntoIterator<Item = (&'a str, OrganKind)>) -> Vec<Look> {
        let organs: Vec<(&str, OrganKind)> = organs.into_iter().collect();
        let own = self.with_male(&organs);
        let mut looks = looks::resolve(
            organs.iter().copied(),
            &own,
            self.foliage,
            self.foliage_shade,
        );
        self.vary(&mut looks, organs.len(), |drawn| {
            looks::resolve(
                organs.iter().copied(),
                drawn,
                self.foliage,
                self.foliage_shade,
            )
        });
        looks
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
        let organs: Vec<(&str, OrganKind)> = organs.into_iter().collect();
        let own = self.with_male(&organs);
        let (mut looks, mut sizes) = looks::resolve_on(
            organs.iter().copied(),
            &own,
            self.foliage,
            self.foliage_shade,
            day,
        );
        self.vary(&mut looks, organs.len(), |drawn| {
            looks::resolve_on(
                organs.iter().copied(),
                drawn,
                self.foliage,
                self.foliage_shade,
                day,
            )
            .0
        });
        if let Some(day) = day {
            self.stagger(&mut looks, &mut sizes, &organs, &own, day);
        }
        (looks, sizes)
    }

    /// Draw each of `looks` whose season is staggered ([`looks::Season::cohort`])
    /// as its cohorts are on day `day`: each variety the look its cohort
    /// shows then, and its size as a share of the largest's, which becomes
    /// the type's size; the look itself is the first cohort in flower, else
    /// the first showing, so a card drawn from it shows flowers while any
    /// are open.
    fn stagger(
        &self,
        looks: &mut [Look],
        sizes: &mut [f64],
        organs: &[(&str, OrganKind)],
        own: &BTreeMap<String, OrganLook>,
        day: f64,
    ) {
        let staggered = |name: &str| {
            own.get(name)
                .and_then(|look| look.season.as_ref())
                .is_some_and(|season| season.stagger > 0.0)
        };
        if !looks
            .iter()
            .take(organs.len())
            .any(|look| staggered(&look.organ))
        {
            return;
        }
        let cohorts: Vec<(Vec<Look>, Vec<f64>, Vec<looks::Stage>)> = (0..looks::VARIETIES)
            .map(|variety| {
                let mut drawn = looks::variety(own, &self.spreads, variety);
                let mut stages = Vec::new();
                for look in drawn.values_mut() {
                    if let Some(season) = look.season.as_mut()
                        && season.stagger > 0.0
                    {
                        *season = season.cohort(variety);
                    }
                }
                for (name, _) in organs {
                    stages.push(
                        drawn
                            .get(*name)
                            .and_then(|look| look.season.as_ref())
                            .map_or(looks::Stage::Gone, |season| season.stage(day)),
                    );
                }
                let (looks, sizes) = looks::resolve_on(
                    organs.iter().copied(),
                    &drawn,
                    self.foliage,
                    self.foliage_shade,
                    Some(day),
                );
                (looks, sizes, stages)
            })
            .collect();
        for (index, look) in looks.iter_mut().enumerate().take(organs.len()) {
            if !staggered(&look.organ) {
                continue;
            }
            let shares: Vec<f64> = cohorts
                .iter()
                .map(|(_, sizes, _)| sizes.get(index).copied().unwrap_or(0.0))
                .collect();
            let most = shares.iter().copied().fold(0.0, f64::max);
            let drawn: Vec<Look> = cohorts
                .iter()
                .filter_map(|(looks, _, _)| looks.get(index).cloned())
                .collect();
            let shown = cohorts
                .iter()
                .position(|(_, _, stages)| stages.get(index) == Some(&looks::Stage::Flower))
                .or_else(|| shares.iter().position(|&share| share > 0.0))
                .unwrap_or(0);
            if let Some(base) = drawn.get(shown) {
                *look = base.clone();
            }
            look.varieties = drawn;
            look.variety_sizes = shares
                .iter()
                .map(|&share| if most > 0.0 { share / most } else { 0.0 })
                .collect();
            if let Some(size) = sizes.get_mut(index) {
                *size = most;
            }
        }
    }

    /// Give each of the first `types` of `looks` whose values vary
    /// ([`Appearance::spreads`]) and that is drawn solid its varieties
    /// (spec rework VARY): the looks `resolve` gives each variety's looks
    /// by organ name ([`looks::variety`]), where any differs from it.
    fn vary(
        &self,
        looks: &mut [Look],
        types: usize,
        resolve: impl Fn(&BTreeMap<String, OrganLook>) -> Vec<Look>,
    ) {
        let Some(serde_json::Value::Object(varied)) = self.spreads.get("organs") else {
            return;
        };
        if !looks
            .iter()
            .take(types)
            .any(|look| varied.contains_key(&look.organ) && look.draws_solid())
        {
            return;
        }
        let varieties: Vec<Vec<Look>> = (0..looks::VARIETIES)
            .map(|variety| {
                let mut drawn = looks::variety(&self.organs, &self.spreads, variety);
                for (male, of) in
                    &self.males(looks.iter().take(types).map(|look| look.organ.as_str()))
                {
                    if let Some(look) = drawn.get(of) {
                        let look = male_look(look);
                        drawn.insert(male.clone(), look);
                    }
                }
                resolve(&drawn)
            })
            .collect();
        for (index, look) in looks.iter_mut().enumerate().take(types) {
            if varied.contains_key(&look.organ) && look.draws_solid() {
                let drawn: Vec<Look> = varieties
                    .iter()
                    .filter_map(|drawn| drawn.get(index).cloned())
                    .collect();
                // A look whose varieties all draw its typical values (its
                // spreads differ only in their rare extremes) stays one.
                if drawn.iter().any(|variety| variety != look) {
                    look.varieties = drawn;
                }
            }
        }
    }

    /// Its looks by organ name, with a look for each of `organs` that is a
    /// male plant's ([`MALE`]) and has none of its own: the look of the
    /// organ it stands for, as [`male_look`] draws it. A thorn or prickle
    /// whose look states no colour takes the bark's: the young stem's skin
    /// it grows from (Rosaceae, 2026-10-19).
    fn with_male(
        &self,
        organs: &[(&str, OrganKind)],
    ) -> std::borrow::Cow<'_, BTreeMap<String, OrganLook>> {
        let males = self.males(organs.iter().map(|(name, _)| *name));
        let bare: Vec<&String> = self
            .organs
            .iter()
            .filter(|(_, look)| {
                look.colour.is_none()
                    && matches!(
                        look.form,
                        Some(crate::blooms::Form::Thorn | crate::blooms::Form::Prickles(_))
                    )
            })
            .map(|(name, _)| name)
            .collect();
        if males.is_empty() && bare.is_empty() {
            return std::borrow::Cow::Borrowed(&self.organs);
        }
        let mut own = self.organs.clone();
        for name in bare {
            if let Some(look) = own.get_mut(name) {
                look.colour = Some(self.bark);
            }
        }
        for (male, of) in males {
            if let Some(look) = self.organs.get(&of) {
                own.insert(male, male_look(look));
            }
        }
        std::borrow::Cow::Owned(own)
    }

    /// The male plant's organs among `organs` that have no look of their
    /// own, each with the organ it stands for (`bloom` for `bloom_male`).
    fn males<'a>(&self, organs: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
        organs
            .into_iter()
            .filter(|name| !self.organs.contains_key(*name))
            .filter_map(|name| {
                let of = name.strip_suffix(MALE)?;
                Some((name.to_string(), of.to_string()))
            })
            .collect()
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
            .map(|name| self.bodies.get(name).cloned().unwrap_or_default().drawn())
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
        for (kind, look) in &self.defaults {
            if crate::lsys::OrganKind::from_name(kind).is_none() {
                return Err(format!(
                    "appearance.defaults.{kind}: no organ kind is called `{kind}`"
                ));
            }
            look.validate()
                .map_err(|message| format!("appearance.defaults.{kind}: {message}"))?;
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
        if let Some(look) = &self.bark_look {
            look.look("bark_look", self.bark)
                .map_err(|error| format!("appearance: {error}"))?;
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
        if let Some(young) = &self.young_axis {
            young
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

/// The colour of young wood. A piece of an axis is moved from its bark's
/// colour toward `colour` by its youth: the youth its program drew it
/// with (the L-system's `young`, as a fern draws its crozier), or, where
/// `years` is given, by how new it is, whichever is more.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YoungAxis {
    /// Linear RGB.
    pub colour: [f32; 3],
    /// Years new wood takes to lose the colour, more than 0. Without it
    /// only the program's youth colours wood.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub years: Option<f64>,
}

impl YoungAxis {
    /// How far a piece of wood drawn with youth `young` and `age` years
    /// old is moved toward the young colour, 0 to 1: the more of `young`
    /// and, with `years`, 1 when new falling smoothly to 0 at `years`.
    #[must_use]
    pub fn weight(&self, young: f32, age: f64) -> f64 {
        let new = self
            .years
            .map_or(0.0, |years| 1.0 - crate::math::smoothstep(0.0, years, age));
        f64::from(young.clamp(0.0, 1.0)).max(new)
    }

    /// Check its colour and years.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.colour.iter().any(|c| !(0.0..=1.0).contains(c)) {
            return Err("young_axis colour must be between 0 and 1".into());
        }
        if self
            .years
            .is_some_and(|years| !(years > 0.0 && years <= 10.0))
        {
            return Err("young_axis years must be more than 0 and at most 10".into());
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

/// The program parameter that holds each plant's own draw from 0 to 1 for
/// its sex ([`PlantSpec::seeded_params`]).
pub const SEX: &str = "sex";

/// The suffix of a program's organ that a dioecious species' male plants
/// bear in place of the organ it names (`bloom_male` for `bloom`): unless
/// the species gives it a look, it is drawn as that organ is on a male
/// plant ([`male_look`]).
pub const MALE: &str = "_male";

/// How a male plant draws an organ whose look is `look` on a plant that
/// fruits: as it is in bud and flower, and fallen where it would set
/// fruit. A look with no season is drawn as it is.
#[must_use]
pub fn male_look(look: &OrganLook) -> OrganLook {
    let mut male = look.clone();
    if let Some(season) = &mut male.season {
        season.fall = season.fruit;
        season.ripe = season.ripe.min(season.fruit);
    }
    male
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
            &spec.seeded_params(&program, &params, variant.seed),
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

    /// A built-in species by id (an old id finds the species renamed from
    /// it).
    ///
    /// # Errors
    ///
    /// Fails if there is no such species.
    pub fn builtin(id: &str) -> Result<Self, SpecError> {
        Library::builtin().spec(id)
    }

    /// Hold a spec, as written (`document`, its JSON: a species' merged
    /// spec, or a spec file), to per-value evidence: every value outside
    /// [`UNNOTED`] has a note on its own path or a subtree holding it, no
    /// note names a path the spec does not have or is blank, and every note
    /// cites a source `library` holds. A subtree's note stands for every
    /// value under it, so it may cover them only where they share its basis;
    /// review holds that, as the check cannot.
    ///
    /// Packages are built without the notes (they are not in a package's
    /// key), so this is the library's check, not [`PlantSpec::validate_in`]'s.
    ///
    /// # Errors
    ///
    /// Names the first value without a note, or the first note that is
    /// blank, names no value, or cites no source or an unknown one.
    pub fn check_evidence(document: &serde_json::Value, library: &Library) -> Result<(), String> {
        let notes: BTreeMap<String, FieldEvidence> = document
            .get("evidence")
            .map(|notes| serde_json::from_value(notes.clone()))
            .transpose()
            .map_err(|error| format!("evidence: {error}"))?
            .unwrap_or_default();
        crate::evidence::check_coverage(document, &notes, UNNOTED)?;
        for (path, note) in &notes {
            note.check()
                .map_err(|error| format!("the evidence note on `{path}`: {error}"))?;
            match note.source.as_deref() {
                None => return Err(format!("the evidence note on `{path}` cites no source")),
                Some(id) if library.source(id).is_none() => {
                    return Err(format!(
                        "the evidence note on `{path}` cites `{id}`, which is not in the library's sources/"
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
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
    #[allow(clippy::too_many_lines)]
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
            if let Some(problem) = unfit_for_a_variant(conditions) {
                return fail(format!("{at}: {problem}"));
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
        // A juvenile family without `until` draws its leaves only by the
        // adultness the program records (spec rework P6).
        if self.appearance.organs.values().any(|look| {
            look.families
                .as_ref()
                .and_then(|families| families.juvenile.as_ref())
                .is_some_and(|juvenile| juvenile.until.is_none())
        }) && !self
            .program_in(library)
            .is_ok_and(|(program, _)| program.draws_adult())
        {
            return fail(format!(
                "families juvenile needs until: program {} records no adult shoots",
                self.generator.program
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
        let drawn = self.drawn_areas(&program);
        let params = if drawn.is_empty() {
            program.resolve_params(&self.generator.params)?
        } else {
            let mut given = self.generator.params.clone();
            given.extend(drawn);
            program.resolve_params(&given)?
        };
        self.check_names(&program)?;
        Ok((program, params))
    }

    /// The shading area of each organ whose area is a program parameter
    /// this spec leaves unset: the area its look draws on an organ 1 m long
    /// (LANGUAGE.md 5.2, areas from looks), so the light model shades with
    /// what the plant shows, or for a needle card its needles' own area
    /// ([`crate::templates::shade_area`]); where organs share a parameter,
    /// the first's.
    fn drawn_areas(&self, program: &Program) -> BTreeMap<String, f64> {
        let names = program.organ_area_params();
        let unset: Vec<(usize, &str)> = names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                name.filter(|name| !self.generator.params.contains_key(*name))
                    .map(|name| (index, name))
            })
            .collect();
        if unset.is_empty() {
            return BTreeMap::new();
        }
        let looks = self.appearance.looks(program.organs());
        let templates = crate::templates::Templates::for_looks(&looks);
        let mut drawn = BTreeMap::new();
        for (index, name) in unset {
            if let (Some(look), Some(template)) = (looks.get(index), templates.templates.get(index))
            {
                drawn
                    .entry(name.to_string())
                    .or_insert_with(|| crate::templates::shade_area(look, template));
            }
        }
        drawn
    }

    /// The parameters the plant grown from `seed` grows with (spec rework
    /// VARY): `params`, the spec's as [`PlantSpec::program_in`] resolves
    /// them for `program`, with each that has a spread
    /// ([`Generator::spreads`]: a character of the whole plant a rule
    /// passes through) drawn from it, and the parameters that follow from
    /// it following it. The seeds a package grows ([`VariantPlan::seeds`])
    /// share each spread out as an organ's varieties do
    /// ([`crate::rng::dealt`]), so one seed grows the typical plant and
    /// three draw from the middles of a range's thirds; another seed draws
    /// by its hash. A seed grows the same plant on every machine and in
    /// every environment.
    ///
    /// A program's `sex` ([`SEX`]) is each plant's own draw from 0 to 1,
    /// dealt the same way: in a dioecious species the plants below a half
    /// are male, the rest female, so a package's seeds grow both, and the
    /// typical plant (one seed, or none) is female. A plant of a species
    /// that is not dioecious draws no sex and keeps its value.
    #[must_use]
    pub fn seeded_params(&self, program: &Program, params: &[f64], seed: u64) -> Vec<f64> {
        let share = |name: &str| match self.variants.seeds.iter().position(|own| *own == seed) {
            Some(index) => crate::rng::dealt(index, self.variants.seeds.len(), name),
            None => crate::rng::unit(crate::rng::mix64(seed ^ crate::rng::hash_str(name))),
        };
        let mut drawn = self.generator.params.clone();
        let mut any = false;
        if let Some(serde_json::Value::Object(spreads)) = self.generator.spreads.get("params") {
            for (name, spread) in crate::inherit::Spread::all(spreads) {
                if let Some(value) = drawn.get_mut(&name) {
                    *value = spread.at(share(&name));
                    any = true;
                }
            }
        }
        let dioecious = program
            .params
            .iter()
            .position(|param| param.name == "dioecious")
            .is_some_and(|at| params[at] >= 0.5);
        if dioecious && program.params.iter().any(|param| param.name == SEX) {
            drawn.insert(SEX.to_string(), share(SEX));
            any = true;
        }
        if !any {
            return params.to_vec();
        }
        // The areas its looks give stay as `params` has them.
        for name in program.organ_area_params().into_iter().flatten() {
            if !drawn.contains_key(name)
                && let Some(at) = program.params.iter().position(|param| param.name == name)
            {
                drawn.insert(name.to_string(), params[at]);
            }
        }
        program
            .resolve_params(&drawn)
            .unwrap_or_else(|_| params.to_vec())
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
                    substrate: None,
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
                    substrate: conditions.substrate.as_ref().and_then(|substrate| {
                        substrate
                            .preset
                            .map(|preset| (preset, substrate.seed.unwrap_or(1)))
                    }),
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
    fn young_wood_takes_the_more_of_its_youth_and_its_newness() {
        let drawn = YoungAxis {
            colour: [0.1, 0.2, 0.05],
            years: None,
        };
        // Without years only the program's youth counts.
        assert!((drawn.weight(0.6, 0.0) - 0.6).abs() < 1e-6);
        assert!(drawn.weight(0.0, 0.0).abs() < 1e-12);
        let timed = YoungAxis {
            years: Some(2.0),
            ..drawn
        };
        assert!((timed.weight(0.0, 0.0) - 1.0).abs() < 1e-12);
        assert!(timed.weight(0.0, 2.0).abs() < 1e-12);
        assert!((timed.weight(0.6, 2.0) - 0.6).abs() < 1e-6);
        assert!(drawn.validate().is_ok() && timed.validate().is_ok());
        assert!(
            YoungAxis {
                years: Some(0.0),
                ..drawn
            }
            .validate()
            .is_err()
        );
        assert!(
            YoungAxis {
                colour: [1.2, 0.0, 0.0],
                ..drawn
            }
            .validate()
            .is_err()
        );
    }

    /// Per-value evidence: every value has a note, and every note cites a
    /// source the library holds.
    #[test]
    fn every_value_needs_a_note_citing_a_source() {
        let library = Library::builtin();
        let good: serde_json::Value =
            serde_json::from_str(crate::library::source("pseudotsuga-menziesii")).unwrap();
        PlantSpec::check_evidence(&good, library).unwrap();
        let refused = |change: &dyn Fn(&mut serde_json::Value), wanted: &str| {
            let mut spec = good.clone();
            change(&mut spec);
            let error = PlantSpec::check_evidence(&spec, library).unwrap_err();
            assert!(error.contains(wanted), "{wanted}: {error}");
        };
        refused(
            &|spec| {
                spec["evidence"].as_object_mut().unwrap().remove("growth");
            },
            "has no evidence note",
        );
        refused(
            &|spec| {
                spec["evidence"]["taxon"]
                    .as_object_mut()
                    .unwrap()
                    .remove("source");
            },
            "the evidence note on `taxon` cites no source",
        );
        refused(
            &|spec| spec["evidence"]["taxon"]["source"] = "no-such-source".into(),
            "cites `no-such-source`",
        );
        refused(
            &|spec| spec["evidence"]["taxon"]["note"] = " ".into(),
            "blank",
        );
        refused(
            &|spec| {
                let note = spec["evidence"]["taxon"].clone();
                spec["evidence"]["colour"] = note;
            },
            "`colour`, which holds no value",
        );
        // A more specific note may stand beside its subtree's.
        let mut finer = good.clone();
        let mut note = finer["evidence"]["generator.params"].clone();
        note["note"] = "Measured.".into();
        finer["evidence"]["generator.params.whorl"] = note;
        PlantSpec::check_evidence(&finer, library).unwrap();
        // The schema, id, model tier and provenance need no note.
        for key in UNNOTED {
            assert!(good.get(*key).is_some(), "{key}");
        }
    }

    /// An organ whose area parameter the spec leaves unset shades with the
    /// area its look draws; one the spec sets keeps its own.
    #[test]
    fn an_unset_organ_area_is_what_its_look_draws() {
        let mut spec = PlantSpec::builtin("leucanthemum-vulgare").unwrap();
        let (program, own) = spec.program().unwrap();
        let at = |name: &str| {
            program
                .params
                .iter()
                .position(|param| param.name == name)
                .unwrap()
        };
        assert!((own[at("leaf_area")] - spec.generator.params["leaf_area"]).abs() < 1e-12);
        spec.generator.params.remove("leaf_area");
        let (program, params) = spec.program().unwrap();
        let looks = spec.appearance.looks(program.organs());
        let templates = crate::templates::Templates::for_looks(&looks);
        let leaf = program
            .organs()
            .position(|(name, _)| name == "leaf")
            .unwrap();
        let drawn = crate::templates::drawn_area(&looks[leaf], &templates.templates[leaf]);
        assert!((params[at("leaf_area")] - drawn).abs() < 1e-12);
        assert!(drawn > 0.0);
        // A seed's own parameters keep it.
        let seeded = spec.seeded_params(&program, &params, 7);
        assert!((seeded[at("leaf_area")] - drawn).abs() < 1e-12);
    }

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
        // Juvenile leaves without `until` need a program that records adult
        // shoots (spec rework P6).
        let mut spec = PlantSpec::builtin("juniperus-scopulorum").unwrap();
        spec.validate().unwrap();
        let juvenile = spec
            .appearance
            .organs
            .values_mut()
            .find_map(|look| look.families.as_mut()?.juvenile.as_mut())
            .unwrap();
        juvenile.until = None;
        assert!(
            spec.validate().unwrap_err().0.ends_with(
                ": families juvenile needs until: program pinidae records no adult shoots"
            )
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
