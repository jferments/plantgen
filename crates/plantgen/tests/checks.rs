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
        "aquilegia-formosa",
        "fruit_type",
        "its fruit look sets no fruit kind, so its follicles are drawn as the ellipsoid of a berry or drupe",
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
        "dicentra-formosa",
        "fruit_type",
        "its fruit look sets no fruit kind, so its capsules are drawn as the ellipsoid of a berry or drupe",
    ),
    (
        "digitalis-purpurea",
        "corolla_shape",
        "its bell is joined 0.8 of its length round a wide mouth, its lobes short (Flora of New Zealand), but the angiosperms draw every bell joined 0.45 and a funnel 0.6, so its corolla is nearest a funnel; a longer bell needs the rule's shapes to allow one",
    ),
    (
        "digitalis-purpurea",
        "fruit_type",
        "its fruit look sets no fruit kind, so its capsules are drawn as the ellipsoid of a berry or drupe",
    ),
    (
        "encelia-farinosa",
        "shoot_leaves",
        "its most vigorous shoots grow one to three of the four internodes a year they may, two nodes each, so they bear 2 to 6 leaves against the 4 to 18 the angiosperms state, from maples' long shoots, until each lineage states its own",
    ),
    (
        "larrea-tridentata",
        "leaf_division",
        "each leaf is two leaflets joined at their base, but its sprig cards draw each leaf as one blade",
    ),
    (
        "lupinus-polyphyllus",
        "fruit_type",
        "its look in fruit sets no fruit kind, so its pods are drawn as the ellipsoid of a berry or drupe",
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
        "program:broadleaf",
        "crown_outline_fit",
        "its crowns still fill the envelope of the space tool (an ellipsoid), so from the side they fit an ellipse at about 0.90 to 0.95 against open-grown trees' 0.82 to 0.84; crown form step B grows the shape from light and space with no outline",
    ),
    (
        "program:grass",
        "phyllotaxis_pattern",
        "each blade's roll is jittered ±40° (a culm leaf's ±30°), so the turn from one to the next has a median of about 146°, nearer the golden angle than half a turn",
    ),
    (
        "program:grass",
        "leaf_width_m",
        "since the Poaceae rule sets blade cards from Hitchcock's widths, the median blade measured sits inside the flora's range, but the narrowest run up to half its least width (smaller blades of young tillers, inferred, not traced)",
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
        "tiarella-trifoliata",
        "fruit_type",
        "its fruit look sets no fruit kind, so its capsules are drawn as the ellipsoid of a berry or drupe",
    ),
    (
        "tolmiea-menziesii",
        "fruit_type",
        "its look in fruit sets no fruit kind, so its capsules are drawn as the ellipsoid of a berry or drupe",
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
