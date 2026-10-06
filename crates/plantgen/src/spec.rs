//! Species specifications: a species as data.
//!
//! A [`PlantSpec`] names a plant program and overrides its parameters, says
//! how long to grow and which ages to keep, which growth environments and
//! seeds to build variants for, how the plant looks, and where every value
//! came from. Specs are JSON files; the built-in ones live in
//! `crates/after-plants/species/`.

use std::collections::BTreeMap;

pub use after_world::semantics::{FieldEvidence, Provenance, SourceRef};
use serde::{Deserialize, Serialize};

use crate::body::BodyLook;
use crate::looks::{self, Flare, Look, Moss, OrganLook, Ridges};
use crate::lsys::program::SymbolKind;
use crate::lsys::{Neighbourhood, OrganKind, Program, ProgramError, tools};

pub const SPEC_SCHEMA: u32 = 1;

/// Built-in plant programs, by name.
pub const PROGRAMS: [(&str, &str); 14] = [
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
];

/// Built-in species, by id: the catalogue of the south Puget Sound
/// lowlands, in its first five species' order and then by growth form.
pub const SPECIES: [(&str, &str); 100] = [
    // The first species: two conifers, a maple, a grass and a herb.
    (
        "pseudotsuga-menziesii",
        include_str!("../species/pseudotsuga-menziesii.json"),
    ),
    (
        "thuja-plicata",
        include_str!("../species/thuja-plicata.json"),
    ),
    (
        "acer-macrophyllum",
        include_str!("../species/acer-macrophyllum.json"),
    ),
    (
        "deschampsia-cespitosa",
        include_str!("../species/deschampsia-cespitosa.json"),
    ),
    (
        "chamaenerion-angustifolium",
        include_str!("../species/chamaenerion-angustifolium.json"),
    ),
    // Conifers.
    (
        "tsuga-heterophylla",
        include_str!("../species/tsuga-heterophylla.json"),
    ),
    (
        "picea-sitchensis",
        include_str!("../species/picea-sitchensis.json"),
    ),
    (
        "abies-grandis",
        include_str!("../species/abies-grandis.json"),
    ),
    (
        "pinus-contorta",
        include_str!("../species/pinus-contorta.json"),
    ),
    (
        "pinus-monticola",
        include_str!("../species/pinus-monticola.json"),
    ),
    (
        "pinus-ponderosa",
        include_str!("../species/pinus-ponderosa.json"),
    ),
    (
        "taxus-brevifolia",
        include_str!("../species/taxus-brevifolia.json"),
    ),
    (
        "juniperus-scopulorum",
        include_str!("../species/juniperus-scopulorum.json"),
    ),
    // Broadleaf trees.
    ("alnus-rubra", include_str!("../species/alnus-rubra.json")),
    (
        "populus-trichocarpa",
        include_str!("../species/populus-trichocarpa.json"),
    ),
    (
        "quercus-garryana",
        include_str!("../species/quercus-garryana.json"),
    ),
    (
        "arbutus-menziesii",
        include_str!("../species/arbutus-menziesii.json"),
    ),
    (
        "fraxinus-latifolia",
        include_str!("../species/fraxinus-latifolia.json"),
    ),
    (
        "populus-tremuloides",
        include_str!("../species/populus-tremuloides.json"),
    ),
    (
        "betula-papyrifera",
        include_str!("../species/betula-papyrifera.json"),
    ),
    (
        "frangula-purshiana",
        include_str!("../species/frangula-purshiana.json"),
    ),
    (
        "prunus-emarginata",
        include_str!("../species/prunus-emarginata.json"),
    ),
    (
        "cornus-nuttallii",
        include_str!("../species/cornus-nuttallii.json"),
    ),
    (
        "salix-lasiandra",
        include_str!("../species/salix-lasiandra.json"),
    ),
    ("malus-fusca", include_str!("../species/malus-fusca.json")),
    // Shrubs.
    (
        "gaultheria-shallon",
        include_str!("../species/gaultheria-shallon.json"),
    ),
    (
        "mahonia-nervosa",
        include_str!("../species/mahonia-nervosa.json"),
    ),
    (
        "mahonia-aquifolium",
        include_str!("../species/mahonia-aquifolium.json"),
    ),
    (
        "vaccinium-ovatum",
        include_str!("../species/vaccinium-ovatum.json"),
    ),
    (
        "vaccinium-parvifolium",
        include_str!("../species/vaccinium-parvifolium.json"),
    ),
    (
        "rubus-spectabilis",
        include_str!("../species/rubus-spectabilis.json"),
    ),
    (
        "rubus-parviflorus",
        include_str!("../species/rubus-parviflorus.json"),
    ),
    (
        "cornus-sericea",
        include_str!("../species/cornus-sericea.json"),
    ),
    (
        "holodiscus-discolor",
        include_str!("../species/holodiscus-discolor.json"),
    ),
    (
        "oemleria-cerasiformis",
        include_str!("../species/oemleria-cerasiformis.json"),
    ),
    (
        "symphoricarpos-albus",
        include_str!("../species/symphoricarpos-albus.json"),
    ),
    ("rosa-nutkana", include_str!("../species/rosa-nutkana.json")),
    (
        "sambucus-racemosa",
        include_str!("../species/sambucus-racemosa.json"),
    ),
    (
        "corylus-cornuta",
        include_str!("../species/corylus-cornuta.json"),
    ),
    (
        "amelanchier-alnifolia",
        include_str!("../species/amelanchier-alnifolia.json"),
    ),
    (
        "physocarpus-capitatus",
        include_str!("../species/physocarpus-capitatus.json"),
    ),
    (
        "spiraea-douglasii",
        include_str!("../species/spiraea-douglasii.json"),
    ),
    (
        "acer-circinatum",
        include_str!("../species/acer-circinatum.json"),
    ),
    (
        "rubus-armeniacus",
        include_str!("../species/rubus-armeniacus.json"),
    ),
    (
        "cytisus-scoparius",
        include_str!("../species/cytisus-scoparius.json"),
    ),
    (
        "ribes-sanguineum",
        include_str!("../species/ribes-sanguineum.json"),
    ),
    (
        "lonicera-involucrata",
        include_str!("../species/lonicera-involucrata.json"),
    ),
    // Grasses, sedges, rushes and cattails.
    (
        "festuca-roemeri",
        include_str!("../species/festuca-roemeri.json"),
    ),
    (
        "danthonia-californica",
        include_str!("../species/danthonia-californica.json"),
    ),
    (
        "elymus-glaucus",
        include_str!("../species/elymus-glaucus.json"),
    ),
    (
        "festuca-rubra",
        include_str!("../species/festuca-rubra.json"),
    ),
    (
        "phalaris-arundinacea",
        include_str!("../species/phalaris-arundinacea.json"),
    ),
    (
        "dactylis-glomerata",
        include_str!("../species/dactylis-glomerata.json"),
    ),
    (
        "holcus-lanatus",
        include_str!("../species/holcus-lanatus.json"),
    ),
    (
        "carex-obnupta",
        include_str!("../species/carex-obnupta.json"),
    ),
    (
        "carex-deweyana",
        include_str!("../species/carex-deweyana.json"),
    ),
    ("carex-inops", include_str!("../species/carex-inops.json")),
    (
        "juncus-effusus",
        include_str!("../species/juncus-effusus.json"),
    ),
    (
        "schoenoplectus-acutus",
        include_str!("../species/schoenoplectus-acutus.json"),
    ),
    (
        "glyceria-elata",
        include_str!("../species/glyceria-elata.json"),
    ),
    (
        "calamagrostis-nutkaensis",
        include_str!("../species/calamagrostis-nutkaensis.json"),
    ),
    (
        "koeleria-macrantha",
        include_str!("../species/koeleria-macrantha.json"),
    ),
    (
        "typha-latifolia",
        include_str!("../species/typha-latifolia.json"),
    ),
    // Ferns and horsetails.
    (
        "polystichum-munitum",
        include_str!("../species/polystichum-munitum.json"),
    ),
    (
        "pteridium-aquilinum",
        include_str!("../species/pteridium-aquilinum.json"),
    ),
    (
        "athyrium-filix-femina",
        include_str!("../species/athyrium-filix-femina.json"),
    ),
    (
        "struthiopteris-spicant",
        include_str!("../species/struthiopteris-spicant.json"),
    ),
    (
        "equisetum-telmateia",
        include_str!("../species/equisetum-telmateia.json"),
    ),
    (
        "dryopteris-expansa",
        include_str!("../species/dryopteris-expansa.json"),
    ),
    // Flowering herbs.
    (
        "camassia-quamash",
        include_str!("../species/camassia-quamash.json"),
    ),
    (
        "achillea-millefolium",
        include_str!("../species/achillea-millefolium.json"),
    ),
    (
        "leucanthemum-vulgare",
        include_str!("../species/leucanthemum-vulgare.json"),
    ),
    (
        "trillium-ovatum",
        include_str!("../species/trillium-ovatum.json"),
    ),
    (
        "achlys-triphylla",
        include_str!("../species/achlys-triphylla.json"),
    ),
    (
        "maianthemum-dilatatum",
        include_str!("../species/maianthemum-dilatatum.json"),
    ),
    (
        "tiarella-trifoliata",
        include_str!("../species/tiarella-trifoliata.json"),
    ),
    (
        "dicentra-formosa",
        include_str!("../species/dicentra-formosa.json"),
    ),
    (
        "oxalis-oregana",
        include_str!("../species/oxalis-oregana.json"),
    ),
    (
        "asarum-caudatum",
        include_str!("../species/asarum-caudatum.json"),
    ),
    (
        "tolmiea-menziesii",
        include_str!("../species/tolmiea-menziesii.json"),
    ),
    (
        "viola-glabella",
        include_str!("../species/viola-glabella.json"),
    ),
    (
        "lysichiton-americanus",
        include_str!("../species/lysichiton-americanus.json"),
    ),
    (
        "urtica-dioica",
        include_str!("../species/urtica-dioica.json"),
    ),
    (
        "heracleum-maximum",
        include_str!("../species/heracleum-maximum.json"),
    ),
    (
        "erythranthe-guttata",
        include_str!("../species/erythranthe-guttata.json"),
    ),
    (
        "oenanthe-sarmentosa",
        include_str!("../species/oenanthe-sarmentosa.json"),
    ),
    (
        "digitalis-purpurea",
        include_str!("../species/digitalis-purpurea.json"),
    ),
    (
        "jacobaea-vulgaris",
        include_str!("../species/jacobaea-vulgaris.json"),
    ),
    (
        "cirsium-vulgare",
        include_str!("../species/cirsium-vulgare.json"),
    ),
    (
        "hypochaeris-radicata",
        include_str!("../species/hypochaeris-radicata.json"),
    ),
    (
        "anaphalis-margaritacea",
        include_str!("../species/anaphalis-margaritacea.json"),
    ),
    (
        "solidago-lepida",
        include_str!("../species/solidago-lepida.json"),
    ),
    (
        "lupinus-polyphyllus",
        include_str!("../species/lupinus-polyphyllus.json"),
    ),
    (
        "eriophyllum-lanatum",
        include_str!("../species/eriophyllum-lanatum.json"),
    ),
    (
        "castilleja-hispida",
        include_str!("../species/castilleja-hispida.json"),
    ),
    (
        "sisyrinchium-idahoense",
        include_str!("../species/sisyrinchium-idahoense.json"),
    ),
    (
        "symphyotrichum-subspicatum",
        include_str!("../species/symphyotrichum-subspicatum.json"),
    ),
    ("iris-tenax", include_str!("../species/iris-tenax.json")),
    (
        "lilium-columbianum",
        include_str!("../species/lilium-columbianum.json"),
    ),
    (
        "aquilegia-formosa",
        include_str!("../species/aquilegia-formosa.json"),
    ),
];

#[must_use]
pub fn builtin_program(name: &str) -> Option<&'static str> {
    PROGRAMS
        .iter()
        .find(|(program, _)| *program == name)
        .map(|(_, source)| *source)
}

/// Built-in cacti of the Sonoran Desert (milestone F1): columns,
/// barrels, hedgehogs, a pincushion, chollas and prickly pears, grown by
/// the `succulent` program. No habitat places them yet, so they are not in
/// [`SPECIES`], the forest's catalogue; the desert milestone (F8) gives them
/// niches.
pub const SONORAN_SPECIES: [(&str, &str); 12] = [
    (
        "carnegiea-gigantea",
        include_str!("../species/carnegiea-gigantea.json"),
    ),
    (
        "stenocereus-thurberi",
        include_str!("../species/stenocereus-thurberi.json"),
    ),
    (
        "ferocactus-wislizeni",
        include_str!("../species/ferocactus-wislizeni.json"),
    ),
    (
        "ferocactus-cylindraceus",
        include_str!("../species/ferocactus-cylindraceus.json"),
    ),
    (
        "echinocereus-engelmannii",
        include_str!("../species/echinocereus-engelmannii.json"),
    ),
    (
        "mammillaria-grahamii",
        include_str!("../species/mammillaria-grahamii.json"),
    ),
    (
        "cylindropuntia-bigelovii",
        include_str!("../species/cylindropuntia-bigelovii.json"),
    ),
    (
        "cylindropuntia-fulgida",
        include_str!("../species/cylindropuntia-fulgida.json"),
    ),
    (
        "cylindropuntia-acanthocarpa",
        include_str!("../species/cylindropuntia-acanthocarpa.json"),
    ),
    (
        "opuntia-engelmannii",
        include_str!("../species/opuntia-engelmannii.json"),
    ),
    (
        "opuntia-basilaris",
        include_str!("../species/opuntia-basilaris.json"),
    ),
    (
        "opuntia-santa-rita",
        include_str!("../species/opuntia-santa-rita.json"),
    ),
];

/// Built-in rosette plants of the deserts of the American Southwest
/// (milestone F2), grown by the `rosette` program: an agave, a yucca, a
/// sotol, the Joshua tree, an aloe and a desert bromeliad. Like the
/// cacti, no habitat places them yet.
pub const ROSETTE_SPECIES: [(&str, &str); 6] = [
    (
        "agave-deserti",
        include_str!("../species/agave-deserti.json"),
    ),
    ("yucca-elata", include_str!("../species/yucca-elata.json")),
    (
        "dasylirion-wheeleri",
        include_str!("../species/dasylirion-wheeleri.json"),
    ),
    (
        "yucca-brevifolia",
        include_str!("../species/yucca-brevifolia.json"),
    ),
    ("aloe-vera", include_str!("../species/aloe-vera.json")),
    (
        "hechtia-montana",
        include_str!("../species/hechtia-montana.json"),
    ),
];

/// Palms, a cycad and a tree fern (plant forms F4): the California fan
/// palm of desert oases, the date palm, the cabbage palm, the royal palm,
/// the creeping saw palmetto, the sago cycad and the soft tree fern. No
/// habitat places them yet.
pub const PALM_SPECIES: [(&str, &str); 7] = [
    (
        "washingtonia-filifera",
        include_str!("../species/washingtonia-filifera.json"),
    ),
    (
        "phoenix-dactylifera",
        include_str!("../species/phoenix-dactylifera.json"),
    ),
    (
        "sabal-palmetto",
        include_str!("../species/sabal-palmetto.json"),
    ),
    (
        "roystonea-regia",
        include_str!("../species/roystonea-regia.json"),
    ),
    (
        "serenoa-repens",
        include_str!("../species/serenoa-repens.json"),
    ),
    (
        "cycas-revoluta",
        include_str!("../species/cycas-revoluta.json"),
    ),
    (
        "dicksonia-antarctica",
        include_str!("../species/dicksonia-antarctica.json"),
    ),
];

/// Trees and shrubs of the Sonoran Desert (plant forms F5): foothill palo
/// verde, velvet mesquite (which dies at 100 and stands as a snag), desert
/// ironwood, creosote bush, white bursage, brittlebush, jojoba, desert
/// willow and ocotillo. No habitat places them yet (milestone F8).
pub const DESERT_SPECIES: [(&str, &str); 9] = [
    (
        "parkinsonia-microphylla",
        include_str!("../species/parkinsonia-microphylla.json"),
    ),
    (
        "prosopis-velutina",
        include_str!("../species/prosopis-velutina.json"),
    ),
    (
        "olneya-tesota",
        include_str!("../species/olneya-tesota.json"),
    ),
    (
        "larrea-tridentata",
        include_str!("../species/larrea-tridentata.json"),
    ),
    (
        "ambrosia-dumosa",
        include_str!("../species/ambrosia-dumosa.json"),
    ),
    (
        "encelia-farinosa",
        include_str!("../species/encelia-farinosa.json"),
    ),
    (
        "simmondsia-chinensis",
        include_str!("../species/simmondsia-chinensis.json"),
    ),
    (
        "chilopsis-linearis",
        include_str!("../species/chilopsis-linearis.json"),
    ),
    (
        "fouquieria-splendens",
        include_str!("../species/fouquieria-splendens.json"),
    ),
];

/// Two trees of the African savanna (plant forms F5): the umbrella thorn
/// acacia's flat crown and the baobab's bottle trunk.
pub const SAVANNA_SPECIES: [(&str, &str); 2] = [
    (
        "vachellia-tortilis",
        include_str!("../species/vachellia-tortilis.json"),
    ),
    (
        "adansonia-digitata",
        include_str!("../species/adansonia-digitata.json"),
    ),
];

/// Plants of swamps, coasts and open water (plant forms F6): red and
/// black mangrove, bald cypress, strangler fig, sawgrass, papyrus, a
/// bamboo and a water lily. No habitat places them yet.
pub const WETLAND_SPECIES: [(&str, &str); 8] = [
    (
        "rhizophora-mangle",
        include_str!("../species/rhizophora-mangle.json"),
    ),
    (
        "avicennia-germinans",
        include_str!("../species/avicennia-germinans.json"),
    ),
    (
        "taxodium-distichum",
        include_str!("../species/taxodium-distichum.json"),
    ),
    ("ficus-aurea", include_str!("../species/ficus-aurea.json")),
    (
        "cyperus-papyrus",
        include_str!("../species/cyperus-papyrus.json"),
    ),
    (
        "cladium-jamaicense",
        include_str!("../species/cladium-jamaicense.json"),
    ),
    (
        "phyllostachys-aurea",
        include_str!("../species/phyllostachys-aurea.json"),
    ),
    (
        "nymphaea-odorata",
        include_str!("../species/nymphaea-odorata.json"),
    ),
];

/// Plants that grow on a host (plant forms F7): each is grown on, and
/// stood under in a garden, its `host` model.
pub const GUEST_SPECIES: [(&str, &str); 4] = [
    ("hedera-helix", include_str!("../species/hedera-helix.json")),
    (
        "tillandsia-usneoides",
        include_str!("../species/tillandsia-usneoides.json"),
    ),
    (
        "phoradendron-californicum",
        include_str!("../species/phoradendron-californicum.json"),
    ),
    (
        "vitis-californica",
        include_str!("../species/vitis-californica.json"),
    ),
];

/// Cushions, tussocks and wind-pruned trees of the high Andes and the
/// Patagonian steppe (plant forms F7): llareta, neneo, coirón and a lenga
/// flagged by the wind.
pub const ALPINE_SPECIES: [(&str, &str); 4] = [
    (
        "azorella-compacta",
        include_str!("../species/azorella-compacta.json"),
    ),
    (
        "mulinum-spinosum",
        include_str!("../species/mulinum-spinosum.json"),
    ),
    (
        "festuca-gracillima",
        include_str!("../species/festuca-gracillima.json"),
    ),
    (
        "nothofagus-pumilio",
        include_str!("../species/nothofagus-pumilio.json"),
    ),
];

/// The gardens a world can grow beside the forest, by name: `sonoran`,
/// the cacti, the rosettes and the desert's trees and shrubs; `palms`, the
/// palms; `savanna`, the savanna's trees; `wetland`, the plants of swamps,
/// coasts and open water; `hosts`, plants that grow on a host; `alpine`,
/// cushions, tussocks and wind-pruned trees.
pub const GARDENS: [&str; 6] = ["sonoran", "palms", "savanna", "wetland", "hosts", "alpine"];

/// The species of the garden `name`, in planting order; `None` for an
/// unknown garden.
#[must_use]
pub fn garden(name: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match name {
        "sonoran" => Some(
            SONORAN_SPECIES
                .iter()
                .chain(ROSETTE_SPECIES.iter())
                .chain(DESERT_SPECIES.iter())
                .copied()
                .collect(),
        ),
        "palms" => Some(PALM_SPECIES.to_vec()),
        "savanna" => Some(SAVANNA_SPECIES.to_vec()),
        "wetland" => Some(WETLAND_SPECIES.to_vec()),
        "hosts" => Some(GUEST_SPECIES.to_vec()),
        "alpine" => Some(ALPINE_SPECIES.to_vec()),
        _ => None,
    }
}

/// The species a world growing the garden `name` needs: the garden's,
/// then each guest's host that is not among them (plant forms F7).
#[must_use]
pub fn garden_with_hosts(name: &str) -> Option<Vec<&'static str>> {
    let mut ids: Vec<&'static str> = garden(name)?.into_iter().map(|(id, _)| id).collect();
    for index in 0..ids.len() {
        let Ok(spec) = PlantSpec::builtin(ids[index]) else {
            continue;
        };
        if let Some(host) = spec.host
            && let Some((id, _)) = all_species().find(|(id, _)| *id == host.species)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    Some(ids)
}

/// Every garden's species, garden by garden in [`GARDENS`] order.
pub fn garden_species() -> impl Iterator<Item = (&'static str, &'static str)> {
    SONORAN_SPECIES
        .iter()
        .chain(ROSETTE_SPECIES.iter())
        .chain(DESERT_SPECIES.iter())
        .chain(PALM_SPECIES.iter())
        .chain(SAVANNA_SPECIES.iter())
        .chain(WETLAND_SPECIES.iter())
        .chain(GUEST_SPECIES.iter())
        .chain(ALPINE_SPECIES.iter())
        .copied()
}

/// Every built-in catalogue: the forest's, then the garden's.
pub fn all_species() -> impl Iterator<Item = (&'static str, &'static str)> {
    SPECIES.iter().copied().chain(garden_species())
}

#[must_use]
pub fn builtin_species(id: &str) -> Option<&'static str> {
    all_species()
        .find(|(species, _)| *species == id)
        .map(|(_, source)| source)
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
    /// Name of a built-in program (see [`PROGRAMS`]).
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
}

/// One variant to grow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub environment: Environment,
    pub seed: u64,
    pub neighbourhood: Neighbourhood,
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
    /// How each fleshy body of the program looks, by body name: ribs,
    /// areoles, spines (see [`crate::body`]). Bodies not listed get the
    /// default look.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, BodyLook>,
}

impl Appearance {
    /// The look of each of a program's organ types, in its declaration
    /// order (see [`Program::organs`]).
    #[must_use]
    pub fn looks<'a>(&self, organs: impl IntoIterator<Item = (&'a str, OrganKind)>) -> Vec<Look> {
        looks::resolve(organs, &self.organs, self.foliage, self.foliage_shade)
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
        let Some(host) = &self.host else {
            return Ok(None);
        };
        let spec = PlantSpec::builtin(&host.species)
            .map_err(|error| SpecError(format!("host {}: {error}", host.species)))?;
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
        let (program, params) = spec.program()?;
        let growth = crate::grow::grow(
            &program,
            &params,
            &crate::grow::GrowthSettings {
                seed: variant.seed,
                dt: spec.growth.step,
                years: host.age,
                keyframes: vec![host.age],
                neighbourhood: variant.neighbourhood,
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
        Ok(Some(std::sync::Arc::new(crate::lsys::tools::Host::new(
            capsules,
        ))))
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
        let spec: Self = serde_json::from_str(text)
            .map_err(|error| SpecError(format!("invalid spec: {error}")))?;
        spec.validate()?;
        Ok(spec)
    }

    /// A built-in species by id.
    ///
    /// # Errors
    ///
    /// Fails if there is no such species.
    pub fn builtin(id: &str) -> Result<Self, SpecError> {
        let text = builtin_species(id).ok_or_else(|| {
            let known: Vec<&str> = all_species().map(|(name, _)| name).collect();
            SpecError(format!(
                "no built-in species `{id}`; built-in species: {}",
                known.join(", ")
            ))
        })?;
        Self::from_json(text)
    }

    /// Check the spec's values.
    ///
    /// # Errors
    ///
    /// Describes the first problem found.
    pub fn validate(&self) -> Result<(), SpecError> {
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
        if let Err(message) = self.appearance.validate() {
            return fail(message);
        }
        for point in &self.allometry {
            if !(point.height > 0.0 && point.tolerance > 0.0 && point.age <= growth.years) {
                return fail(format!("allometry at age {} is invalid", point.age));
            }
            if !self.variants.environments.contains(&point.environment) {
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
        if builtin_program(&self.generator.program).is_none() {
            let known: Vec<&str> = PROGRAMS.iter().map(|(name, _)| *name).collect();
            return fail(format!(
                "unknown program `{}`; built-in programs: {}",
                self.generator.program,
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
        let source = builtin_program(&self.generator.program)
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
        Ok((program, params))
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
    }
}
