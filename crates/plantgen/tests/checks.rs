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
        "berberis-aquifolium",
        "flower_merosity",
        "its six petals stand in two whorls of three, and a corolla draws one whorl, so its spec draws six there",
    ),
    (
        "berberis-nervosa",
        "flower_merosity",
        "its six petals stand in two whorls of three, and a corolla draws one whorl, so its spec draws six there",
    ),
    (
        "dicentra-formosa",
        "flower_merosity",
        "its four petals stand in two pairs, outer and inner, and a corolla draws one whorl, so its spec draws four there",
    ),
    (
        "digitalis-purpurea",
        "corolla_shape",
        "its bell is joined 0.8 of its length round a wide mouth, its lobes short (Flora of New Zealand), but the angiosperms draw every bell joined 0.45 and a funnel 0.6, so its corolla is nearest a funnel; a longer bell needs the rule's shapes to allow one",
    ),
    (
        "encelia-farinosa",
        "shoot_leaves",
        "its most vigorous shoots grow one to three of the four internodes a year they may, two nodes each, so they bear 2 to 6 leaves against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
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
        "jacobaea-vulgaris",
        "leaf_width_m",
        "its stem leaves shrink upward in proportion, so the topmost, 4 to 5 cm long, draw 1.5 cm wide against the 2 to 6 cm Abrams and Ferris give for the blades, which they measure on the main leaves",
    ),
    (
        "larrea-tridentata",
        "leaf_division",
        "each leaf is two leaflets joined at their base, but its sprig cards draw each leaf as one blade",
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
        "oxalis-oregana",
        "leaf_arrangement",
        "Oxalidaceae's leaves are alternate and redwood sorrel's rise one to a node from its rhizome, but the measure finds three or more leaves at most of its nodes (seen since B1 stated the family's arrangement)",
    ),
    (
        "parkinsonia-microphylla",
        "flower_symmetry",
        "its zygomorphy is `other`, which the angiosperms draw as a corolla of like petals, so its flowers are drawn radial",
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
        "tolmiea-menziesii",
        "flower_symmetry",
        "its zygomorphy is `other`, which the angiosperms draw as a corolla of like petals, so its flowers are drawn radial",
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

/// Every species a check reads, three plants each, has no size or count
/// past its ranges' extremes and draws no kind it does not state, but the
/// [`KNOWN`] ones.
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
                    let failed = report
                        .findings
                        .iter()
                        .filter(|finding| finding.verdict == Verdict::Error)
                        .map(|finding| (finding.character, finding.reason.clone()))
                        .chain(
                            report
                                .kinds
                                .iter()
                                .filter(|agreement| agreement.verdict == Verdict::Error)
                                .map(|agreement| {
                                    (
                                        agreement.character,
                                        format!(
                                            "draws {:?}, states {:?}",
                                            agreement.drawn, agreement.stated
                                        ),
                                    )
                                }),
                        );
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
