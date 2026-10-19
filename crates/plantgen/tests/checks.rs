//! The checks (spec rework M8B and M8A) over the built-in species: every
//! species that states a size, a count or a kind a check reads grows
//! within that size's or count's extremes and draws the kinds it states,
//! seed after seed.

use std::sync::Mutex;

use plantgen::checks::{self, Verdict};
use plantgen::library::Library;
use plantgen::lsys::parser::extends_of;
use plantgen::spec::PlantSpec;

/// Checks known to fail, each with why, until its species or program is
/// changed: a species id, or `program:` and a program that grows every
/// species it names or that the program growing them extends. A known
/// failure that comes to pass fails the test
/// too, so the list only shrinks.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "achlys-triphylla",
        "leaf_arrangement",
        "Berberidaceae's leaves are alternate and vanilla leaf's rise one to a node from its rhizome, but the measure finds three or more leaves at most of its nodes (seen since B1 stated the family's arrangement)",
    ),
    (
        "amaranthus-retroflexus",
        "petiole_length_m",
        "the order's rules draw each leaf's petiole as its stated share of the card and shrink the uppermost leaves as a whole, so the small leaves under the panicle draw petioles of 8 to 9 mm against FNA's 1.5 to 5.5 cm for the stem leaves (inferred: FNA gives one petiole length for all)",
    ),
    (
        "ambrosia-dumosa",
        "fruit_size_m",
        "bursage first flowers with the family programs' blooms (revision 110): its bur, 4 to 6 mm with its hooked spines, is drawn as a capitulum's small achene, as no rule yet turns an involucre into a bur",
    ),
    (
        "ambrosia-dumosa",
        "involucre_diameter_m",
        "bursage first flowers with the family programs' blooms (revision 110): its small staminate heads are drawn with the capitulum's default involucre, wider than the 3 to 5 mm the records give",
    ),
    (
        "ambrosia-dumosa",
        "leaf_shape",
        "its dissected blade is drawn on a compound card of three pairs of lobed segments (spec rework E1), whose outline reads obovate or orbicular against the deltate, oblong or ovate outline Abrams and Ferris give the whole blade",
    ),
    (
        "anaphalis-margaritacea",
        "involucre_height_m",
        "its heads' involucre is drawn 3.5 to 4.1 mm tall against FNA's 5 to 7 mm",
    ),
    (
        "ascarina-lucida",
        "flower_symmetry",
        "Chloranthaceae's flowers are zygomorphic by reduction, their stamens to one side of the ovary (Floral Diagrams), but they have no perianth, and a flower card makes zygomorphy only from its petals or tepals, so it draws them regular",
    ),
    (
        "ascarina-lucida",
        "crown_see_through",
        "the broadleaf program spaces a crown's twigs by the cube root of its short shoots' leaf card, so a bush of 2.5 to 5 cm leaves shows half its outline as sky, where iNaturalist photographs show a dense bush leafy to its core",
    ),
    (
        "azorella-compacta",
        "height_typical_m",
        "it is grown 200 years, to a dome 0.6 m tall, but Reiche's mounds 1.5 m tall and more, and the ministry's 1 m, are of cushions centuries older (inferred: no source dates them)",
    ),
    (
        "azorella-compacta",
        "leaf_shape",
        "its rosette leaves are drawn by hand on a sprig card 0.4 as broad as long, so they read elliptic against the linear leaves Reiche gives",
    ),
    (
        "basella-alba",
        "juvenile_period_years",
        "the climber program first flowers in its second year (a shoot turns adult once the vine is adult_age old, counted from 0), so Malabar spinach, which flowers in its first, is drawn flowering at 2 years",
    ),
    (
        "basella-alba",
        "shoot_leaves",
        "the climber's shoots are drawn with 3 leaves against the 15 to 30 its twining stems bear; the climber program grows few leaves on a shoot (as on pipevine's)",
    ),
    (
        "capsicum-annuum",
        "leaf_length_m",
        "the smaller leaf of each geminate pair is drawn half the larger (Kilmer's ratio for belladonna) and the sympodium's leaves shrink to three quarters upward, so the least draw about 2 cm long, against the 5 to 10 cm Bailey gives the leaves of its varieties, which are not the smaller of a pair",
    ),
    (
        "capsicum-annuum",
        "leaf_width_m",
        "the smaller leaf of each geminate pair is drawn half the larger and the sympodium's leaves shrink to three quarters upward, so the least draw about 1.2 cm wide, against the 2 to 6.4 cm Bailey gives the leaves of its varieties",
    ),
    (
        "chloranthus-serratus",
        "flower_symmetry",
        "Chloranthaceae's flowers are zygomorphic by reduction, their stamens to one side of the ovary (Floral Diagrams), but they have no perianth, and a flower card makes zygomorphy only from its petals or tepals, so it draws them regular",
    ),
    (
        "cirsium-vulgare",
        "leaf_shape",
        "its pinnatifid blades are drawn on lobed cards whose outline, lobes and all, reads elliptic or obovate as well as oblanceolate, against the lanceolate to oblong blades Abrams and Ferris give",
    ),
    (
        "cladium-mariscus-subsp-jamaicense",
        "culm_length_m",
        "its panicles stand on lateral branches, and the measure stops at the first lateral axis below an inflorescence, so it reads a branch, not the culm",
    ),
    (
        "clinopodium-douglasii",
        "leaf_habit",
        "its trailing stems flower, and the herb program keeps a stem's leaves through winter only on a stem bearing leaves alone, so yerba buena's evergreen leaves are drawn as deciduous until a flowering stem can keep its leaves",
    ),
    (
        "cornus-sericea",
        "fruit_size_m",
        "its drupes are drawn about 1.6 cm across against the 6 to 8 mm Dendrology (2019) p. 196 gives (seen since the book's sizes joined the spec)",
    ),
    (
        "cyperus-papyrus",
        "culm_length_m",
        "its umbel's rays are lateral axes, and the measure stops at the first lateral axis below an inflorescence, so it reads a ray, not the culm",
    ),
    (
        "cyperus-papyrus",
        "leaf_shape",
        "its leaves are the brown sheaths at its culms' foot, drawn as cards tapering from the base, 0.15 as broad as long, so they read deltate or lanceolate against the linear blades Cyperaceae state; papyrus bears no blades",
    ),
    (
        "datura-stramonium",
        "leaf_shape",
        "its sinuate-lobed blades are drawn on lobed cards whose outline, lobes and all, reads elliptic or obovate, against the ovate blades Britton and Brown give",
    ),
    (
        "deschampsia-cespitosa",
        "awn_form",
        "it states straight or geniculate awns (FNA), which the Poales' rule draws between them, bent 30°, nearest a contorted awn's 40°",
    ),
    (
        "ferocactus-acanthodes-subsp-acanthodes",
        "stem_form",
        "Ferocactus states its stems globose, but Britton and Rose's 'globular to cylindric' allows a column, and its longest stem grows at least three times as long as wide",
    ),
    (
        "frangula-purshiana",
        "inflorescence",
        "Rhamnaceae's flowers are in cymes or panicles (Simpson 2019), but cascara's axillary clusters are drawn as an umbel",
    ),
    (
        "fuchsia-magellanica",
        "shoot_leaves",
        "a cut-back plant's shoots grow 3 to 5 ft a season (Bean) with some 15 to 25 whorls, but in the open at six years its shoots grow no more than a few vigour units, bearing about 8 leaves",
    ),
    (
        "hedera-helix",
        "fruit_size_m",
        "its fruit is drawn as one card 7 cm long, about 8.4 cm as measured, standing for a whole umbel of berries, against the 6 to 9 mm Dendrology (2019) p. 310 gives one drupe",
    ),
    (
        "hedyosmum-mexicanum",
        "flower_symmetry",
        "Chloranthaceae's flowers are zygomorphic by reduction, their stamens to one side of the ovary (Floral Diagrams), but they have no perianth, and a flower card makes zygomorphy only from its petals or tepals, so it draws them regular",
    ),
    (
        "heracleum-maximum",
        "leaf_length_m",
        "its stem leaves shrink upward in proportion, so the topmost draw about 15 cm against the 2 to 5 dm Abrams gives, which he measures on the main leaves",
    ),
    (
        "holodiscus-discolor",
        "flower_size_m",
        "its flowers are drawn about 5.8 mm across against the 3 to 3.5 mm Dendrology (2019) p. 318 gives",
    ),
    (
        "houttuynia-cordata",
        "inflorescence_bracts",
        "a cluster draws at most one bract at its foot, a spathe, so the four white petaloid bracts under fish mint's spike (Flora of China: involucral bracts 10 to 15 mm) are not drawn",
    ),
    (
        "larrea-tridentata",
        "leaf_division",
        "each leaf is two leaflets joined at their base, but its sprig cards draw each leaf as one blade",
    ),
    (
        "leucanthemum-vulgare",
        "leaf_shape",
        "its stem leaves are drawn oblanceolate, a basal leaf's outline with less width, but Abrams and Ferris give a shape (spatulate or obovate) only for the basal leaves; the stem leaves they call reduced, sessile and clasping",
    ),
    (
        "lonicera-involucrata",
        "fruit_size_m",
        "its berries are drawn about 1.6 cm long against the 7 to 8 mm Dendrology (2019) p. 390 gives",
    ),
    (
        "lupinus-latifolius",
        "leaf_length_m",
        "a rosette leaf's card holds its blade and its petiole, and no source read gives broadleaf lupine's petiole, so its card is as long as the 8 to 15 cm leaves FEIS gives and the palmate blade on it draws 6.5 to 9.6 cm, until its petiole is stated",
    ),
    (
        "lysichiton-americanus",
        "leaf_shape",
        "its spec draws the blade widest at 0.6 of its length and 0.45 as broad as long by hand, so it reads oblanceolate or obovate against Abrams's oblong-lanceolate",
    ),
    (
        "lysichiton-americanus",
        "units_per_inflorescence",
        "a cluster draws at most 200 units, so the spadix's 350 to 1350 flowers are drawn as 200",
    ),
    (
        "nicotiana-tabacum",
        "leaf_length_m",
        "its stem leaves shrink upward to a quarter of the longest (King's: smaller upward), so the topmost draw about 9 cm against the 1 to 2 ft King's gives, which it measures on the main leaves",
    ),
    (
        "nicotiana-tabacum",
        "leaf_width_m",
        "its stem leaves shrink upward to a quarter of the longest, so the topmost draw about 4 cm wide against the 6 to 8 in King's gives the main leaves",
    ),
    (
        "nymphaea-odorata",
        "shoot_leaves",
        "its rhizome is not drawn and each pad's petiole is, as a stem of its own, so each reads as a shoot of one leaf against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "olneya-tesota",
        "leaf_length_m",
        "a short shoot's leaf card draws a cluster of its small leaves, a branchlet's worth, 0.26 m long, so the crown keeps within the module limit, against Abrams's leaves 3 to 10 cm",
    ),
    (
        "oxalis-oregana",
        "leaf_arrangement",
        "Oxalidaceae's leaves are alternate and redwood sorrel's rise one to a node from its rhizome, but the measure finds three or more leaves at most of its nodes (seen since B1 stated the family's arrangement)",
    ),
    (
        "phyllostachys-aurea",
        "leaf_shape",
        "its sprig leaves are drawn lanceolate, 0.15 as broad as long, as a bamboo's are, against the linear blades Poaceae state for its grasses; the species states no shape of its own",
    ),
    (
        "parkinsonia-microphylla",
        "leaf_length_m",
        "a short shoot's leaf card draws a cluster of its small leaves, a branchlet's worth, so the crown keeps within the module limit, against Abrams's pinnae 1 to 3 cm",
    ),
    (
        "piper-nigrum",
        "fruit_size_m",
        "the climber draws a fruiting spike as one fruit card, about 10 cm long (Flora of China: spikes to as long as the leaves), so the measure reads the spike against the 3 to 4 mm drupes on it",
    ),
    (
        "piper-nigrum",
        "shoot_leaves",
        "a climbing shoot adds one or two nodes a season, each with one leaf, so it bears 1 to 2 leaves a year against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "populus-tremuloides",
        "juvenile_period_years",
        "aspen first flowers with the family programs' blooms (revision 110): its stated 2 to 3 years is adult_age, but broadleaf bears blooms only on short shoots, which a young aspen whose buds all grow out as long shoots first makes in its fourth year",
    ),
    (
        "portulaca-oleracea",
        "inflorescence_length_m",
        "its flowers sit 1 to 3 together at the stem tips, sessile, and the cluster is measured across its drawn flowers, 4 to 9 mm, against FNA's 8 to 15 mm, which includes the crowded leaves around them (inferred)",
    ),
    (
        "program:broadleaf",
        "crown_outline_fit",
        "its crowns still fill the envelope of the space tool (an ellipsoid), so from the side they fit an ellipse at about 0.90 to 0.95 against open-grown trees' 0.82 to 0.84; crown form step B grows the shape from light and space with no outline",
    ),
    (
        "program:equisetaceae",
        "leaf_form",
        "horsetails' leaves are small scales joined in a sheath at each node (Simpson 2019), but their whorled branches are drawn on blade cards",
    ),
    (
        "program:grass",
        "shoot_leaves",
        "a culm's own leaves are its 1 to 3 culm leaves and a tiller's blades stand at the ground on no stem, against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "program:herb",
        "phyllotaxis_pattern",
        "each basal leaf's roll is jittered ±30°, so a fan of two ranks (irises, blue-eyed grass) turns about 140° to 150° from one leaf to the next",
    ),
    (
        "program:herb",
        "shoot_leaves",
        "a stem grows all its nodes' leaves in its one year (2 on foamflower's, 36 on Columbia lily's, 45 on fireweed's), against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "program:pinidae",
        "cone_maturation_years",
        "a cone's look ripens it in its first season, so the three pines' cones, which take two to three, are drawn ripe in one",
    ),
    (
        "program:pinidae",
        "cone_retention_years",
        "western white and ponderosa pines' cones fall as they ripen, but drawn ripe in their first season they read held a year (the late cut fixed by the conifer lives, #96)",
    ),
    (
        "program:polypodiidae",
        "leaf_division",
        "a pinnatifid blade, simple and lobed nearly to its midrib (licorice fern's, Flora of North America vol. 2: blade pinnatifid), is drawn as free pinnae on its rachis, as the program has no wing joining them",
    ),
    (
        "program:rosette",
        "shoot_leaves",
        "a rosette's crown adds its year's leaves where they stand (29 to 38 on common sotol's), against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "program:sapindaceae",
        "crown_outline_fit",
        "a maple's crown fills its lobed envelope (space@2) closely, so from the side it fits an ellipse at about 0.91; crown form step B grows the shape from light and space with no outline",
    ),
    (
        "sarcandra-glabra",
        "flower_symmetry",
        "Chloranthaceae's flowers are zygomorphic by reduction, their stamens to one side of the ovary (Floral Diagrams), but they have no perianth, and a flower card makes zygomorphy only from its petals or tepals, so it draws them regular",
    ),
    (
        "sarcobatus-vermiculatus",
        "crown_width_max_m",
        "a shrub's crown is now sized from its flora's greatest width, but greasewood, grown 20 years, overshoots its usual height (below) and spreads with it, 1.6 to 2.2 m across against FNA's 1.8 m",
    ),
    (
        "sarcobatus-vermiculatus",
        "height_typical_m",
        "grown 20 years in the open it reaches 2.1 to 3.0 m, past the 1 to 2 m FNA gives as usual but within its 5 m at most: the shrubs' internode rule (a tenth of the usual height) is calibrated to their 12-year growth, and a rule cannot read the age a species is grown to",
    ),
    (
        "sassafras-albidum",
        "leaf_width_m",
        "its leaves are drawn on one card, the three-lobed form with broad rounded lobes as photographs show it, 16.9 cm wide, against FNA's 5 to 10 cm, which covers its narrower unlobed and mitten leaves too (inferred: FNA gives one width for all three forms)",
    ),
    (
        "sedum-divergens",
        "leaf_habit",
        "its leafy stems end in cymes, and the herb program keeps a stem's leaves through winter only on a stem bearing leaves alone, so spreading stonecrop's evergreen leaves are drawn as deciduous until a flowering stem can keep its leaves",
    ),
    (
        "solanum-melongena",
        "leaf_shape",
        "its sinuate-lobed blades are drawn on lobed cards whose outline, lobes and all, reads elliptic or obovate, against the ovate blades Bailey and Wikipedia give",
    ),
    (
        "tellima-grandiflora",
        "leaf_habit",
        "its leafy stems end in racemes, and the herb program keeps a stem's leaves through winter only on a stem bearing leaves alone, so fringecup's partly evergreen leaves are drawn as deciduous until a flowering stem can keep its leaves",
    ),
    (
        "tillandsia-usneoides",
        "leaf_shape",
        "its leaves are drawn as cards tapering from the base, an eighth as broad as long, so they read deltate or lanceolate against the linear leaves Bromeliaceae state",
    ),
    (
        "tillandsia-usneoides",
        "phyllotaxis_pattern",
        "Bromeliaceae's leaves are spiral (Simpson 2019), but Spanish moss's program sets its leaves in crossing pairs",
    ),
    (
        "vachellia-tortilis",
        "inflorescence_length_m",
        "umbrella thorn first flowers with the family programs' blooms (revision 110): its head, 5 to 10 mm, has no stated flower size, so its flowers are laid out at the default share and the head measures 1.5 to 2 mm; and its pods, 7 to 10 cm, are this cluster's unit, so it cannot grow as the heads do",
    ),
    (
        "washingtonia-filifera",
        "height_max_m",
        "its greatest height, 20 m, is a model estimate no source read gives, and its third seed already grew to 20.0 m; since palm revision 14 stands a year's newest spear on top of the year's trunk, the second seed's spear reaches 20.1 m, until a flora's greatest height replaces the estimate",
    ),
    (
        "whipplea-modesta",
        "shoot_leaves",
        "the angiosperms' default of 4 to 18 leaves a year's shoot is Critchfield's for woody shoots; a herb's trailing stem grows its 47 nodes in one year, so from #170's count of a year's growth it bears 94, until the measure or the default tells a lying herb's stem from a woody shoot",
    ),
];

fn known(spec: &PlantSpec, character: &str) -> bool {
    KNOWN.iter().any(|(who, what, _)| {
        *what == character
            && (*who == spec.id
                || who
                    .strip_prefix("program:")
                    .is_some_and(|program| grows_on(spec, program)))
    })
}

/// Whether `spec`'s program is `program` or extends it.
fn grows_on(spec: &PlantSpec, program: &str) -> bool {
    let library = Library::builtin();
    let mut name = Some(spec.generator.program.clone());
    while let Some(current) = name {
        if current == program {
            return true;
        }
        name = library.program_text(&current).and_then(extends_of);
    }
    false
}

/// A report's errors: each character, and why it failed.
fn failures(report: &checks::Report) -> Vec<(&'static str, String)> {
    let findings = report
        .findings
        .iter()
        .filter(|finding| finding.verdict == Verdict::Error)
        .map(|finding| (finding.character, finding.reason.clone()));
    let kinds = report
        .kinds
        .iter()
        .filter(|agreement| agreement.verdict == Verdict::Error)
        .map(|agreement| {
            (
                agreement.character,
                format!("draws {:?}, states {:?}", agreement.drawn, agreement.stated),
            )
        });
    let timings = report
        .timings
        .iter()
        .filter(|timing| timing.verdict == Verdict::Error)
        .map(|timing| {
            (
                timing.character,
                format!(
                    "draws {}, states {}",
                    checks::months_text(&timing.drawn),
                    checks::months_text(&timing.stated)
                ),
            )
        });
    findings.chain(kinds).chain(timings).collect()
}

/// Every species a check reads, three plants each, has no size or count
/// past its ranges' extremes, draws no kind it does not state and flowers
/// and fruits within a month of the months it states, but the [`KNOWN`]
/// ones.
#[test]
fn stated_characters_hold_on_grown_plants() {
    let library = Library::builtin();
    let ids: Vec<String> = library
        .species()
        .iter()
        .map(|entry| entry.id.clone())
        .filter(|id| {
            let (Ok(spec), Ok(traits)) = (library.spec(id), library.traits_of(id)) else {
                return false;
            };
            checks::states_checked(&spec, &traits)
        })
        .collect();
    assert!(
        ids.len() >= 50,
        "only {} species state checked characters",
        ids.len()
    );
    let next = Mutex::new(ids.iter());
    let errors = Mutex::new(Vec::new());
    let failing_known = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let Some(id) = next.lock().unwrap().next() else {
                        break;
                    };
                    let traits = library.traits_of(id).unwrap();
                    let spec = PlantSpec::builtin(id).unwrap();
                    let seeds = checks::seeds_of(&spec, 3);
                    let report = checks::check(&spec, library, &traits, &seeds).unwrap();
                    let failed = failures(&report);
                    for (character, reason) in failed {
                        if known(&spec, character) {
                            failing_known
                                .lock()
                                .unwrap()
                                .push((spec.id.clone(), character));
                        } else {
                            errors
                                .lock()
                                .unwrap()
                                .push(format!("{} {character}: {reason}", spec.id));
                        }
                    }
                }
            });
        }
    });
    let mut errors = errors.into_inner().unwrap();
    errors.sort();
    assert!(errors.is_empty(), "checks failed:\n{}", errors.join("\n"));
    let failing_known = failing_known.into_inner().unwrap();
    for (who, character, why) in KNOWN {
        let still = failing_known.iter().any(|(id, c)| {
            c == character
                && (id == who
                    || who.strip_prefix("program:").is_some_and(|program| {
                        PlantSpec::builtin(id).is_ok_and(|spec| grows_on(&spec, program))
                    }))
        });
        assert!(
            still,
            "{who} {character} now holds ({why}): take it off the known list"
        );
    }
}

/// A plant is kept [`checks::LIFE_YEARS`] before the end of growth to see
/// its leaves and cones fall, so no species may state a leaf or a cone
/// that lives longer.
#[test]
fn leaves_and_cones_fall_within_the_years_kept() {
    let library = Library::builtin();
    for entry in library.species() {
        let traits = library.traits_of(&entry.id).unwrap();
        let life = |key: &str| {
            traits
                .get(key)
                .and_then(checks::Stated::of)
                .map_or(0.0, |stated| stated.extremes().1)
        };
        let longest = life("leaf_life_years")
            .max(life("cone_maturation_years") + life("cone_retention_years"));
        assert!(
            longest < checks::LIFE_YEARS,
            "{} states a leaf or a cone that lives up to {longest} years",
            entry.id
        );
    }
}
