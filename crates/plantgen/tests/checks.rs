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
        "cornus-sericea",
        "inflorescence",
        "Cornaceae's flowers are in cymes (Simpson 2019), and red-osier dogwood's flat-topped clusters are cymes, but its look draws them as an umbel",
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
        "cytisus-scoparius",
        "leaf_shape",
        "its leaflets are drawn by hand on a sprig card widest at the middle, so they read elliptic against the obovate leaflets Abrams gives",
    ),
    (
        "deschampsia-cespitosa",
        "awn_form",
        "it states straight or geniculate awns (FNA), which the Poales' rule draws between them, bent 30°, nearest a contorted awn's 40°",
    ),
    (
        "digitalis-purpurea",
        "corolla_shape",
        "its bell is joined 0.8 of its length round a wide mouth, its lobes short (Flora of New Zealand), but the angiosperms draw every bell joined 0.45 and a funnel 0.6, so its corolla is nearest a funnel; a longer bell needs the rule's shapes to allow one",
    ),
    (
        "echinocereus-engelmannii",
        "central_spine_form",
        "its main central is drawn flattened and straight (its lower central, by eye from photographs), against the curved and twisted centrals Britton and Rose describe; a twisted spine is not drawn",
    ),
    (
        "encelia-farinosa",
        "shoot_leaves",
        "its most vigorous shoots grow one to three of the four internodes a year they may, two nodes each, so they bear 2 to 6 leaves against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "equisetum-telmateia",
        "leaf_form",
        "horsetails' leaves are small scales joined in a sheath at each node (Simpson 2019), but its whorled branches are drawn on blade cards",
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
        "hedera-helix",
        "fruit_size_m",
        "its fruit is drawn as one card 7 cm long, about 8.4 cm as measured, standing for a whole umbel of berries, against the 6 to 9 mm Dendrology (2019) p. 310 gives one drupe",
    ),
    (
        "heracleum-maximum",
        "inflorescence_bracts",
        "a cluster draws its bracts only as a spathe, so the umbel's involucre of 5 to 10 narrow bracts, which fall early, is not drawn",
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
        "holodiscus-discolor",
        "inflorescence_length_m",
        "its panicles are drawn about 7.5 cm long against the 10 to 25 cm Dendrology (2019) p. 318 gives",
    ),
    (
        "jacobaea-vulgaris",
        "leaf_width_m",
        "its stem leaves shrink upward in proportion, so the topmost, 4 to 5 cm long, draw 1.5 cm wide against the 2 to 6 cm Abrams and Ferris give for the blades, which they measure on the main leaves",
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
        "program:broadleaf",
        "crown_outline_fit",
        "its crowns still fill the envelope of the space tool (an ellipsoid), so from the side they fit an ellipse at about 0.90 to 0.95 against open-grown trees' 0.82 to 0.84; crown form step B grows the shape from light and space with no outline",
    ),
    (
        "program:grass",
        "leaf_length_m",
        "a culm's leaves are drawn 0.35 to 0.6 as long as its tiller's blades, so where a flora's blades span less than about three-fold (California oatgrass, Roemer's fescue, reed canarygrass) the shortest culm leaves fall below its least",
    ),
    (
        "program:grass",
        "leaf_width_m",
        "since the Poaceae rule sets blade cards from Hitchcock's widths, the median blade measured sits inside the flora's range, but the narrowest run up to half its least width (smaller blades of young tillers, inferred, not traced)",
    ),
    (
        "program:grass",
        "phyllotaxis_pattern",
        "a tiller's blades are measured round the vertical, so where a tiller leans out about as far as its blades stand off it (tufted hairgrass, coirón, Roemer's and red fescue: blades 10° to 15° off, tillers leaning up to 22°) the lean turns them all one way and the turns read as a spiral or three ranks (inferred, not traced)",
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
        "program:pinidae",
        "leaf_life_years",
        "Sitka spruce's needles live 8 to 10 years at the median, but some fall at 5, against the 7 to 10 it states (seen since the conifer lives, #96; not traced)",
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
        "typha-latifolia",
        "fruit_type",
        "Typhaceae's fruit is a tiny achene on a hairy stalk (Simpson 2019), but its fruiting spike is drawn with the berry card",
    ),
    (
        "viola-glabella",
        "zygomorphy",
        "a violet's two upper, two side and one spurred lower petal are no named kind (`other`), and it is drawn two-lipped by hand, the nearest corolla that spurs one lower petal",
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
