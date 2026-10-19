//! The built-in species, the forest's and the Sonoran cacti: they grow to
//! their reference sizes, and their growth history stays consistent from
//! keyframe to keyframe.

use std::collections::HashMap;

use plantgen::graph::PlantGraph;
use plantgen::grow::{Growth, GrowthSettings, grow};
use plantgen::lsys::Limits;
use plantgen::package;
use plantgen::spec::{self, GrowthForm, PlantSpec, Variant};
use plantgen::templates::{self, Templates};

fn grow_variant(spec: &PlantSpec, variant: &Variant, years: f64, keyframes: Vec<f64>) -> Growth {
    let (program, params) = spec.program().unwrap();
    grow(
        &program,
        &spec.seeded_params(&program, &params, variant.seed),
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes,
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().unwrap(),
        },
    )
    .unwrap()
}

/// Space colonization narrowed in 32-bit, as a GPU narrows it (here in
/// Rust, on every step), grows the same plants as the full search, bit for
/// bit: maples, oak, Douglas-fir and umbrella thorn, in the open and inside
/// a stand.
#[test]
fn narrowed_bud_searches_grow_the_same_plants() {
    for id in [
        "acer-circinatum",
        "acer-macrophyllum",
        "quercus-garryana",
        "pseudotsuga-menziesii",
        "vachellia-tortilis",
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        for variant in spec.variant_list().iter().take(2) {
            let years = spec.growth.years.min(20.0);
            let full = grow_variant(&spec, variant, years, vec![years / 2.0, years]);
            let narrowed = plantgen::lsys::narrow::emulated(|| {
                grow_variant(&spec, variant, years, vec![years / 2.0, years])
            });
            assert_eq!(full.digest(), narrowed.digest(), "{id} {variant:?}");
        }
    }
}

/// Every built-in species, grown in each environment it has reference
/// sizes for, matches them within the spec's tolerance. This is the
/// acceptance check for a species' parameters: a change to the engine or a
/// program that breaks it needs the species retuned, not the reference
/// loosened.
#[test]
fn builtin_species_grow_to_their_reference_sizes() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        assert!(!spec.allometry.is_empty(), "{id} has no reference sizes");
        for variant in spec.variant_list() {
            let ages: Vec<f64> = spec
                .allometry
                .iter()
                .filter(|point| point.environment == variant.environment)
                .map(|point| point.age)
                .collect();
            if ages.is_empty() {
                continue;
            }
            let years = ages.iter().copied().fold(0.0, f64::max);
            let growth = grow_variant(&spec, &variant, years, ages.clone());
            let records = package::compare_allometry(&spec, &variant, &growth);
            assert_eq!(records.len(), ages.len());
            for record in records {
                checked += 1;
                if !record.within_tolerance {
                    failures.push(format!(
                        "{id} {} seed {} at {} years: {}",
                        variant.environment.name(),
                        variant.seed,
                        record.age,
                        record.describe()
                    ));
                }
            }
        }
    }
    assert!(checked >= 8, "only {checked} reference sizes were checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every organ type of every built-in species shades with about the leaf
/// area its look draws, so the light a plant grows by matches the foliage
/// a viewer sees; a needle card with its needles' own area, which its
/// painted lines overstate (`templates::shade_area`).
#[test]
fn builtin_species_shade_with_the_area_they_draw() {
    let mut failures = Vec::new();
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, params) = spec.program().unwrap();
        let looks = spec.appearance.looks(program.organs());
        let templates = Templates::for_looks(&looks);
        let shading = spec::organ_areas(&program, &params).unwrap();
        // One look per organ type, then its leaves' families' looks.
        assert!(shading.len() <= looks.len());
        for ((look, template), shaded) in looks.iter().zip(&templates.templates).zip(&shading) {
            let drawn = templates::shade_area(look, template);
            if !spec::areas_agree(drawn, *shaded) {
                failures.push(format!(
                    "{id} `{}` draws {drawn:.3} m² on an organ 1 m long but shades with {shaded:.3} m²",
                    look.organ
                ));
            }
        }
        // Leaves draw less the more sun they grew in, step by step, and
        // the sun's and the shade's about evenly about their type's look,
        // so a crown with as many of each draws what the light model
        // shades with.
        for (index, look) in looks.iter().enumerate().take(shading.len()) {
            let forms = look.forms;
            let area = |index: usize, size: f64| {
                templates::shade_area(&looks[index], &templates.templates[index]) * size * size
            };
            // Juvenile leaves draw about what the type's own look draws.
            if let Some((juvenile, _)) = forms.juvenile
                && !spec::areas_agree(area(juvenile, 1.0), area(index, 1.0))
            {
                failures.push(format!(
                    "{id} `{}`: juvenile leaves draw {:.3} m², its own look {:.3}",
                    look.organ,
                    area(juvenile, 1.0),
                    area(index, 1.0)
                ));
            }
            let Some(bands) = forms.bands else {
                continue;
            };
            let areas: Vec<f64> = bands
                .iter()
                .enumerate()
                .map(|(band, &at)| {
                    let size = plantgen::looks::Shape::size_in_light(
                        plantgen::looks::band_form(band),
                        forms.plasticity,
                    );
                    area(at, size)
                })
                .collect();
            let (shade, own, sun) = (areas[0], areas[bands.len() / 2], areas[bands.len() - 1]);
            if !(areas.windows(2).all(|pair| pair[1] < pair[0])
                && spec::areas_agree(f64::midpoint(sun, shade), own))
            {
                failures.push(format!(
                    "{id} `{}`: from shade to sun its leaves draw {areas:.3?} m²",
                    look.organ
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// No built-in species draws an organ floating free of the plant: each
/// stands on its wood, on the drawn end of the strap piece before it, or
/// on an organ it touches (`mesh::loose_organs`), at its oldest keyframe.
#[test]
fn builtin_species_draw_no_floating_organs() {
    const KNOWN: [&str; 0] = [];
    let species: Vec<&str> = spec::all_species().map(|(id, _)| id).collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = species
            .chunks(species.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    for &id in chunk {
                        let spec = PlantSpec::builtin(id).unwrap();
                        let years = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
                        let variant = &spec.variant_list()[0];
                        let growth = grow_variant(&spec, variant, years, vec![years]);
                        let looks = spec.appearance.looks(
                            growth
                                .organ_types
                                .iter()
                                .map(|organ| (organ.name.as_str(), organ.kind)),
                        );
                        let graph = &growth.keyframes[0];
                        let loose = plantgen::mesh::loose_organs(graph, &looks, 0.002);
                        if !loose.is_empty() && !KNOWN.contains(&id) {
                            let (organ, distance) =
                                loose.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                            failures.push(format!(
                                "{id}: {} organs float, a `{}` {:.1} mm from what bears it",
                                loose.len(),
                                growth.organ_types[usize::from(graph.organs[*organ].organ)].name,
                                distance * 1000.0
                            ));
                        }
                    }
                    failures
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Plants fitted to their place (PF1): a fern's fronds, a palm's or a
/// cycad's and a cactus's joints that sag to the ground lie on it
/// (`rest`), so no species of those programs, in any variant, ends wood
/// or an organ more than 0.05 m below the ground at any keyframe.
#[test]
fn fronds_and_pads_lie_on_the_ground() {
    const RESTING: [&str; 7] = [
        "polypodiidae",
        "palm",
        "arecaceae",
        "cyatheaceae",
        "cycadaceae",
        "zamiaceae",
        "marattiaceae",
    ];
    let mut checked = 0;
    let mut failures = Vec::new();
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let program = spec.generator.program.as_str();
        if !RESTING.contains(&program) && program != "cactaceae" {
            continue;
        }
        checked += 1;
        let oldest = *spec.growth.keyframes.last().unwrap();
        for variant in spec.variant_list() {
            let growth = grow_variant(&spec, &variant, oldest, spec.growth.keyframes.clone());
            for graph in &growth.keyframes {
                let lowest = graph
                    .segments
                    .iter()
                    .map(|segment| segment.end.y)
                    .chain(graph.organs.iter().map(|organ| organ.position.y))
                    .fold(f64::INFINITY, f64::min);
                if lowest < -0.05 {
                    failures.push(format!(
                        "{id} {:?} seed {} at {} years: {lowest:.3} m below the ground",
                        variant.environment, variant.seed, graph.age
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        checked, 49,
        "eleven ferns, seven palms, a tree fern, fifteen cycads and fifteen cacti"
    );
}

/// Nothing a package draws reaches below the ground its plant stands on
/// (the level plane of its foot), so a host can stand it on level ground
/// without leaving it out: no segment, organ, wood vertex or card of any
/// built-in species, at any keyframe of its first variant and any level,
/// ends more than 5 cm below it. Roots are left out: they are drawn to
/// meet the ground, and surface roots run half sunk in it.
#[test]
fn builtin_species_stand_above_the_ground() {
    // Wood grown into the ground: segments that end below it, which
    // growth has to stop (ground as an obstacle in growth, the M6
    // thread). A water lily's frame stands on the water's surface, with its
    // rhizome on the bottom below (`programs/nymphaeaceae.lsys`).
    const KNOWN: [&str; 1] = ["nymphaea-odorata"];
    const BED: f64 = 0.05;
    let species: Vec<&str> = spec::all_species().map(|(id, _)| id).collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = species
            .chunks(species.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    for &id in chunk {
                        let mut spec = PlantSpec::builtin(id).unwrap();
                        spec.appearance.roots = None;
                        let inputs = package::Inputs::new(&spec, &plantgen::quality::STANDARD).unwrap();
                        let years = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
                        let variant = &spec.variant_list()[0];
                        let growth = grow_variant(&spec, variant, years, spec.growth.keyframes.clone());
                        for graph in &growth.keyframes {
                            for (level, mesh) in package::keyframe_meshes(graph, &inputs, variant.seed).iter().enumerate() {
                                let below = plantgen::mesh::Below::drawn(graph, mesh);
                                if below.most() > BED && !KNOWN.contains(&id) {
                                    failures.push(format!(
                                        "{id}: at {} years, level {level}, {:.3} m below the ground ({below:?})",
                                        graph.age,
                                        below.most(),
                                    ));
                                }
                            }
                        }
                    }
                    failures
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Grasses and herbs renew their shoots every year: at every keyframe
/// each stem, culm, leaf and flower grew that year, so nothing lives on
/// from the year before but a grass's thatch, the dead blades of last
/// year's tillers, each born with its blade. An evergreen's leaves, and a
/// herb's stems bearing leaves only, live leaf_years, and its thatch comes
/// that much later.
#[test]
fn grasses_and_herbs_renew_their_shoots_every_year() {
    let mut checked = 0;
    let mut evergreen = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        if !matches!(spec.growth_form, GrowthForm::Graminoid | GrowthForm::Forb) {
            continue;
        }
        checked += 1;
        let years = spec
            .generator
            .params
            .get("leaf_years")
            .copied()
            .unwrap_or(1.0);
        if years > 1.0 {
            evergreen += 1;
        }
        let variant = spec.variant_list()[0];
        let ages = vec![1.0, 2.0, 3.0, 4.0];
        let growth = grow_variant(&spec, &variant, 4.0, ages);
        let thatch = growth
            .organ_types
            .iter()
            .position(|organ| organ.name == "thatch");
        for graph in &growth.keyframes {
            assert!(
                graph.height > 0.0 && !graph.organs.is_empty(),
                "{id} at {}",
                graph.age
            );
            if years > 1.0 {
                for segment in &graph.segments {
                    assert!(
                        graph.age - segment.born < years,
                        "{id}: a stem outlived its leaves"
                    );
                }
                for organ in &graph.organs {
                    let kept = if Some(usize::from(organ.organ)) == thatch {
                        years + 1.0
                    } else {
                        years
                    };
                    assert!(
                        graph.age - organ.born < kept,
                        "{id}: an organ outlived its years"
                    );
                }
            } else {
                for segment in &graph.segments {
                    assert_eq!(
                        segment.born.to_bits(),
                        graph.age.to_bits(),
                        "{id}: a stem outlived its year"
                    );
                    assert_eq!(segment.shed, (graph.age < 4.0).then_some(graph.age + 1.0));
                }
                for organ in &graph.organs {
                    let born = if Some(usize::from(organ.organ)) == thatch {
                        graph.age - 1.0
                    } else {
                        graph.age
                    };
                    assert_eq!(
                        organ.born.to_bits(),
                        born.to_bits(),
                        "{id}: an organ outlived its year"
                    );
                }
            }
            if let Some(thatch) = thatch {
                let dead = graph
                    .organs
                    .iter()
                    .filter(|organ| usize::from(organ.organ) == thatch)
                    .count();
                // Thatch once the first tillers' blades die: from the second
                // year on, an evergreen's a year later.
                assert_eq!(
                    dead > 0,
                    graph.age >= 1.0 + years,
                    "{id} at {}: {dead} thatch",
                    graph.age
                );
            }
        }
    }
    assert!(checked >= 2, "only {checked} grasses and herbs");
    assert!(
        evergreen >= 2,
        "only {evergreen} evergreen grasses and herbs"
    );
}

fn by_id(graph: &PlantGraph) -> HashMap<u64, usize> {
    graph
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| (segment.id, index))
        .collect()
}

/// Parts keep their identity, birth age and girth from one keyframe to the
/// next, and a part that disappears records when it was shed.
#[test]
fn growth_history_is_consistent_between_keyframes() {
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let palm = ["palm", "arecaceae"].contains(&spec.generator.program.as_str());
        let variant = spec.variant_list()[0];
        let ages = vec![4.0, 8.0, 12.0, 16.0];
        let growth = grow_variant(&spec, &variant, 16.0, ages.clone());
        assert_eq!(
            growth
                .keyframes
                .iter()
                .map(|graph| graph.age)
                .collect::<Vec<_>>(),
            ages
        );
        for graph in &growth.keyframes {
            assert!(graph.height > 0.0, "{id} at {} has no height", graph.age);
            for (index, segment) in graph.segments.iter().enumerate() {
                assert!(
                    segment.born <= graph.age,
                    "{id}: a segment is born after its keyframe"
                );
                assert!(segment.shed.is_none_or(|shed| shed > graph.age));
                assert!(segment.radius > 0.0 && segment.radius.is_finite());
                if let Some(parent) = segment.parent {
                    let parent = &graph.segments[parent as usize];
                    assert!(
                        (parent.order..=parent.order + 1).contains(&segment.order),
                        "{id} at {}: segment {index} of order {} grows from one of order {}",
                        graph.age,
                        segment.order,
                        parent.order
                    );
                    // The pipe model and annual rings: no branch is thicker
                    // than the wood it grows from. A fleshy body is not
                    // wood: a cactus pad can be wider than the pad it
                    // grows from. Nor is a palm's stem, which lays down no
                    // rings and may swell above its foot (a royal palm's
                    // belly).
                    assert!(
                        segment.body > 0 || palm || segment.radius <= parent.radius * (1.0 + 1e-9),
                        "{id} at {}: segment {index} is thicker than its parent",
                        graph.age
                    );
                    assert!(
                        parent
                            .parent
                            .is_none_or(|grandparent| (grandparent as usize) < index)
                    );
                }
            }
            for organ in &graph.organs {
                assert!(organ.born <= graph.age && organ.shed.is_none_or(|shed| shed > graph.age));
                assert!((0.0..=1.0).contains(&organ.light));
            }
        }
        for pair in growth.keyframes.windows(2) {
            let (young, old) = (&pair[0], &pair[1]);
            let later = by_id(old);
            for segment in &young.segments {
                if let Some(&index) = later.get(&segment.id) {
                    let same = &old.segments[index];
                    assert_eq!(same.born.to_bits(), segment.born.to_bits());
                    assert_eq!(same.shed, segment.shed);
                    assert!(same.radius >= segment.radius, "{id}: a segment got thinner");
                } else {
                    let shed = segment
                        .shed
                        .expect("a segment that disappears has a shed age");
                    assert!(shed > young.age && shed <= old.age, "{id}: shed at {shed}");
                }
            }
        }
    }
}

/// Every conifer bears cones once old enough and none as a sapling, each
/// cone within a span of its species' length (plant forms F3).
#[test]
fn woody_species_bear_their_fruit_when_old() {
    // Plant roadmap P4: every tree and shrub whose blooms are fruit (as in
    // mid-July) carries them at its oldest keyframe, at sizes from a small
    // capsule (white mountain-heather's, 2.5 to 3.5 mm) to a long raceme of
    // samaras, unless its stated months put it out of flower and fruit then.
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let fruiting = spec
            .appearance
            .organs
            .get("bloom")
            .is_some_and(|look| matches!(look.shape, plantgen::looks::Shape::Fruit(_)));
        let gone = spec
            .appearance
            .organs
            .get("bloom")
            .and_then(|look| look.season.as_ref())
            .is_some_and(|season| {
                matches!(
                    season.stage(plantgen::package::DEFAULT_DAY),
                    plantgen::looks::Stage::Gone
                )
            });
        if !grown_as_broadleaf(&spec) || !fruiting || (gone && !in_season_on_the_default_day(id)) {
            continue;
        }
        checked += 1;
        let variant = spec.variant_list()[0];
        let oldest = *spec.growth.keyframes.last().unwrap();
        let growth = grow_variant(&spec, &variant, oldest, vec![oldest]);
        let bloom = growth
            .organ_types
            .iter()
            .position(|organ| organ.name == "bloom")
            .unwrap_or_else(|| panic!("{id}: no bloom organ"));
        // A lone bloom is grown as wide as its flower and drawn in fruit
        // at its season's share of that.
        let in_fruit = spec
            .appearance
            .organs
            .get("bloom")
            .and_then(|look| look.season.as_ref())
            .map_or(1.0, |season| season.fruit_size);
        let sizes: Vec<f64> = growth.keyframes[0]
            .organs
            .iter()
            .filter(|organ| usize::from(organ.organ) == bloom)
            .map(|organ| organ.size * in_fruit)
            .collect();
        assert!(!sizes.is_empty(), "{id}: no fruit at {oldest} years");
        assert!(
            sizes.iter().all(|&size| size > 0.002 && size < 0.25),
            "{id}: fruit sizes {:?}",
            &sizes[..sizes.len().min(5)]
        );
    }
    assert!(checked >= 29, "{checked} fruiting trees and shrubs");
}

#[test]
fn fruiting_species_have_a_year() {
    // Plant roadmap P4: the bloom organ of every tree and shrub whose
    // blooms are fruit has a season, so its stage (bud, flower, unripe,
    // ripe, gone) follows the day a package is grown for, never one time
    // of year. Each stage comes round in the year, and on the forest's
    // default day each bears its flowers or its fruit where its stated
    // months put it in flower or fruit then.
    use plantgen::looks::{Shape, Stage};
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let Some(bloom) = spec.appearance.organs.get("bloom") else {
            continue;
        };
        if !grown_as_broadleaf(&spec) || !matches!(bloom.shape, Shape::Fruit(_)) {
            continue;
        }
        checked += 1;
        let season = bloom
            .season
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no season"));
        season.validate().unwrap();
        assert!(season.flower_look.is_some(), "{id}: no flower look");
        let day = plantgen::package::DEFAULT_DAY;
        assert!(
            matches!(season.stage(day), Stage::Flower | Stage::Fruit { .. })
                || matches!(season.stage(day), Stage::Gone) && !in_season_on_the_default_day(id),
            "{id}: {:?} on day {day}",
            season.stage(day)
        );
        let stages: Vec<Stage> = (1..=365).map(|day| season.stage(f64::from(day))).collect();
        for wanted in [Stage::Gone, Stage::Bud, Stage::Flower] {
            assert!(stages.contains(&wanted), "{id}: never {wanted:?}");
        }
        for share in [1.0, 0.0] {
            assert!(
                stages.contains(&Stage::Fruit { unripe: share }),
                "{id}: never fruit {share} unripe"
            );
        }
    }
    assert!(checked >= 29, "{checked} fruiting trees and shrubs");
}

#[test]
fn conifers_bear_cones_once_old() {
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        if spec.generator.program != "pinidae" {
            continue;
        }
        checked += 1;
        let variant = spec.variant_list()[0];
        let oldest = *spec.growth.keyframes.last().unwrap();
        let growth = grow_variant(&spec, &variant, oldest, vec![5.0, oldest]);
        let cone = growth
            .organ_types
            .iter()
            .position(|organ| organ.name == "cone")
            .unwrap_or_else(|| panic!("{id}: no cone organ"));
        let cones = |graph: &PlantGraph| {
            graph
                .organs
                .iter()
                .filter(|organ| usize::from(organ.organ) == cone)
                .map(|organ| organ.size)
                .collect::<Vec<f64>>()
        };
        assert!(
            cones(&growth.keyframes[0]).is_empty(),
            "{id}: cones on a sapling"
        );
        let old = cones(&growth.keyframes[1]);

        assert!(!old.is_empty(), "{id}: no cones at {oldest} years");
        assert!(
            old.iter().all(|&size| size > 0.005 && size < 0.3),
            "{id}: cone sizes {:?}",
            &old[..old.len().min(5)]
        );
    }
    assert_eq!(
        checked, 19,
        "the ten conifers of the forest, the bald cypress, Alaska and incense cedars, whitebark pine, mountain hemlock, subalpine fir, western larch, Pacific silver fir and common juniper"
    );
}

/// Conifer form (pinidae revision 5): open-grown conifers hold the leaf
/// area per crown area of the USDA Urban Tree Database's open-grown trees
/// of their species (`usfs-urban-tree-database-2016`, leaf area over the
/// area of the mean crown diameter, between its 10th and 90th percentiles
/// for the ages), and those that are not pines keep foliage on the inner
/// half of their lower branches at their oldest keyframe: a branch's
/// shoots die by light, not by age, and its side shoots and dormant buds
/// refoliate it.
#[test]
fn open_conifers_keep_their_lower_crowns() {
    // Species, age, the database's 10th and 90th percentiles, its trees.
    for (id, age, low, high) in [
        // Longview, WA: 8 trees 60 to 300 years old.
        ("pseudotsuga-menziesii", 60.0, 3.6, 16.5),
        // Longview, as Pinus contorta: 20 trees 20 to 35.
        ("pinus-contorta-var-contorta", 30.0, 3.6, 6.8),
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        let variant = *spec
            .variant_list()
            .iter()
            .find(|variant| variant.environment == spec::Environment::Open)
            .unwrap();
        let growth = grow_variant(&spec, &variant, age, vec![age]);
        let graph = &growth.keyframes[0];
        let radius = graph.crown_radius();
        let index = graph.leaf_area(&growth.organ_types) / (std::f64::consts::PI * radius * radius);
        assert!(
            (low..=high).contains(&index),
            "{id} at {age} years: {index:.2} m² of leaf per m² of crown, the database {low} to {high}"
        );
    }
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        if spec.generator.program != "pinidae" || id.starts_with("pinus-") {
            continue;
        }
        let Some(variant) = spec
            .variant_list()
            .into_iter()
            .find(|variant| variant.environment == spec::Environment::Open)
        else {
            continue;
        };
        checked += 1;
        let oldest = *spec.growth.keyframes.last().unwrap();
        let growth = grow_variant(&spec, &variant, oldest, vec![oldest]);
        let graph = &growth.keyframes[0];
        let radius = graph.crown_radius();
        let spray = growth
            .organ_types
            .iter()
            .position(|organ| organ.name == "spray")
            .unwrap_or_else(|| panic!("{id}: no spray organ"));
        let lower: Vec<_> = graph
            .organs
            .iter()
            .filter(|organ| {
                usize::from(organ.organ) == spray && organ.position.y < 0.5 * graph.height
            })
            .collect();
        let area = |organs: &[&plantgen::graph::GraphOrgan]| {
            organs
                .iter()
                .map(|organ| organ.size * organ.size)
                .sum::<f64>()
        };
        let inner: Vec<_> = lower
            .iter()
            .copied()
            .filter(|organ| organ.position.x.hypot(organ.position.z) < 0.5 * radius)
            .collect();
        let share = area(&inner) / area(&lower);
        assert!(
            share >= 0.12,
            "{id} at {oldest} years: {share:.2} of the lower crown's foliage on the inner half of its branches"
        );
    }
    assert_eq!(
        checked, 14,
        "the open-grown conifers that are not pines, Alaska and incense cedars, mountain hemlock, subalpine fir, western larch, Pacific silver fir and common juniper among them"
    );
}

/// Conifer form (pinidae revision 5): no conifer's wood or foliage passes
/// into the ground. A limb that sags to within `ground_gap` of it is shed
/// from there out, so in every variant at every keyframe no segment ends,
/// and no organ sits, more than 5 cm below the foot's plane.
#[test]
fn conifers_keep_above_the_ground() {
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        if spec.generator.program != "pinidae" {
            continue;
        }
        checked += 1;
        let oldest = *spec.growth.keyframes.last().unwrap();
        for variant in spec.variant_list() {
            let growth = grow_variant(&spec, &variant, oldest, spec.growth.keyframes.clone());
            for graph in &growth.keyframes {
                let lowest = graph
                    .segments
                    .iter()
                    .map(|segment| segment.end.y)
                    .chain(graph.organs.iter().map(|organ| organ.position.y))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    lowest >= -0.05,
                    "{id} {:?} seed {} at {} years: wood or foliage {lowest:.3} m below the ground",
                    variant.environment,
                    variant.seed,
                    graph.age
                );
            }
        }
    }
    assert_eq!(
        checked, 19,
        "the ten conifers of the forest, the bald cypress, Alaska and incense cedars, whitebark pine, mountain hemlock, subalpine fir, western larch, Pacific silver fir and common juniper"
    );
}

/// Plant roadmap P4: every species whose foliage is needles draws them
/// near the camera as a solid shoot, a part mesh within its cap whose span
/// is its needles' width, so it shows only where they are a pixel wide.
#[test]
fn needled_species_grow_solid_shoots() {
    use plantgen::looks::Shape;
    use plantgen::parts;
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, _) = spec.program().unwrap();
        let looks = spec.appearance.looks(program.organs());
        let meshes = parts::part_meshes(&looks);
        for (index, look) in looks.iter().enumerate() {
            let Shape::Needles(needles) = &look.shape else {
                continue;
            };
            checked += 1;
            let part = meshes
                .iter()
                .find(|part| part.template == index)
                .unwrap_or_else(|| panic!("{id}: no shoot"));
            assert!(
                part.span > 0.0 && part.span < 0.1,
                "{id}: span {}",
                part.span
            );
            assert!(
                (0.008..=0.15).contains(&needles.width),
                "{id}: needles {} as wide as long",
                needles.width
            );
        }
    }
    assert_eq!(
        checked, 16,
        "the needled conifers (whitebark pine, mountain hemlock, subalpine fir, western larch and Pacific silver fir among them), the bald cypress and the junipers' needles (Rocky Mountain juniper's juvenile ones, common juniper's)"
    );
}

/// Every built-in cluster (spec rework L1) draws near the camera as a part
/// mesh, whole, so none falls back to its card: in its own look and
/// through its year, its units in bud, flower and fruit.
#[test]
fn builtin_clusters_draw_as_part_meshes() {
    use plantgen::parts;
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, _) = spec.program().unwrap();
        // On their own look, and on every fifth day of the year, units
        // in their stages (spec rework L1b).
        let mut days = vec![None];
        if spec.appearance.has_seasons() {
            days.extend((1..=73).map(|day| Some(f64::from(day * 5))));
        }
        for day in days {
            let (looks, _) = spec.appearance.looks_on(program.organs(), day);
            let meshes = parts::part_meshes(&looks);
            for (index, look) in looks.iter().enumerate() {
                if look.inflorescence.is_none() {
                    continue;
                }
                // A male look copies its bloom's (dioecy), so it is checked
                // but not counted.
                if day.is_none() && !look.organ.ends_with(spec::MALE) {
                    checked += 1;
                }
                let part = meshes
                    .iter()
                    .find(|part| part.template == index)
                    .unwrap_or_else(|| {
                        panic!("{id} `{}` on day {day:?}: kept as cards", look.organ)
                    });
                assert!(part.triangles() > 0, "{id} on day {day:?}: {}", look.organ);
            }
        }
    }
    assert_eq!(
        checked, 166,
        "the 24 fruit clusters that moved in with L1a (broom's flowers stand alone since E4), the 15 clusters of flowers and heads of \
         L1c, the 18 inflorescences of grasses, sedges and rushes of P3, the orchid's spike \
         of P5, the skunk cabbage's spadix of E6b, and desert ironwood's racemes and the umbrella \
         thorn's heads from their taxa (visual forms Skill 4), and the six clusters of the core \
         expansion's first ten species, five of its second ten, five of its third and three of its fourth, the five of phase C's first nine (buckwheat's umbels, \
         bistort's spikes, sheep sorrel's panicles, candy flower's racemes and collomia's heads), the eight of its \
         second nine (the waterleaf's and three stonecrops' cymes, the bluebells' and phacelia's coils, and self-heal's \
         and hedge-nettle's spikes), the nine of its \
         third twelve (mock orange's and yerba de selva's racemes, the hydrangea's corymbs, poison oak's and \
         smooth sumac's panicles, fragrant sumac's spikes, and the cymes of Oregon boxwood and two spindles), the spikes \
         of the four Chloranthaceae, and sassafras's racemes, avocado's panicles, fish mint's spikes, \
         the umbels of spicebush, California bay, bay laurel and winter's bark, and nutmeg's cymes of \
         the magnoliids, the eight the species-facts records name (step 6): bur-sage's and \
         baneberry's racemes, ragwort's and foothill palo verde's corymbs, golden bamboo's \
         panicles, and the catkins of quaking aspen and two willows, and the umbels of fennel, \
         wild carrot and poison hemlock (lacy leaves), white clover\'s heads, the four \
         whose families now rank their clusters: aloe vera's racemes, black mangrove's cymes, \
         harsh paintbrush's spikes and Pacific bleeding heart's racemes, and lamb\'s ear\'s spike \
         (hair practice), the tomato's, eggplant's and tobacco's cymes and panicle (nightshades), \
         western columbine's cymes, now that its genus states them, and the racemes of fringecup, \
         blueblossom, spreading gooseberry and bush penstemon (phase A's first ten), and the \
         fourteen of the Mount Rainier subalpine batch, and the fifteen of the Saguaro desert \
         batch (beargrass's and banana yucca's flowering stalks are drawn whole, not as clusters)"
    );
}

/// Fronds open and sag as they age (plant forms F4): on a fern, palms and
/// a tree fern, the leaflets of each year's fronds hang lower, on average,
/// than those of the fronds a year younger. (On a palm the younger fronds
/// also stand a little higher up the trunk; a year's growth is a small
/// part of a frond's length.)
#[test]
fn older_fronds_hang_lower() {
    for (id, organ) in [
        ("polystichum-munitum", "pinna"),
        ("phoenix-dactylifera", "leaflet"),
        ("washingtonia-filifera", "ray"),
        ("dicksonia-antarctica", "leaflet"),
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        let variant = spec.variant_list()[0];
        let oldest = *spec.growth.keyframes.last().unwrap();
        let growth = grow_variant(&spec, &variant, oldest, vec![oldest]);
        let index = growth
            .organ_types
            .iter()
            .position(|kind| kind.name == organ)
            .unwrap();
        let graph = &growth.keyframes[0];
        // Mean height of the leaflets of the fronds born each year.
        let mut by_year: Vec<(i64, f64, usize)> = Vec::new();
        for leaflet in graph
            .organs
            .iter()
            .filter(|o| usize::from(o.organ) == index)
        {
            #[allow(clippy::cast_possible_truncation)]
            let age = (graph.age - leaflet.born).round() as i64;
            match by_year.iter_mut().find(|(year, _, _)| *year == age) {
                Some((_, sum, count)) => {
                    *sum += leaflet.position.y;
                    *count += 1;
                }
                None => by_year.push((age, leaflet.position.y, 1)),
            }
        }
        by_year.sort_by_key(|row| row.0);
        #[allow(clippy::cast_precision_loss)]
        let means: Vec<(i64, f64)> = by_year
            .iter()
            .map(|&(year, sum, count)| (year, sum / count as f64))
            .collect();
        assert!(means.len() >= 2, "{id}: fronds of one age only: {means:?}");
        for pair in means.windows(2) {
            assert!(
                pair[1].1 < pair[0].1,
                "{id}: fronds {} years old hang no lower than {} years old: {means:?}",
                pair[1].0,
                pair[0].0
            );
        }
    }
}

/// A tree past its `life` stands as a snag (plant forms F5): the velvet
/// mesquite dies at 100 years; at 120 it holds no leaf, stands about as
/// tall as it was, and has lost dead branches.
#[test]
fn a_tree_past_its_life_stands_as_a_snag() {
    let spec = PlantSpec::builtin("neltuma-velutina").unwrap();
    let variant = spec.variant_list()[0];
    let growth = grow_variant(&spec, &variant, 120.0, vec![99.0, 120.0]);
    let leaf = growth
        .organ_types
        .iter()
        .position(|kind| kind.name == "leaf")
        .unwrap();
    let (alive, snag) = (&growth.keyframes[0], &growth.keyframes[1]);
    let leaves = |graph: &PlantGraph| {
        graph
            .organs
            .iter()
            .filter(|organ| usize::from(organ.organ) == leaf)
            .count()
    };
    assert!(leaves(alive) > 0);
    assert_eq!(leaves(snag), 0, "a snag holds no leaves");
    assert!(snag.height > alive.height * 0.6, "the trunk still stands");
    assert!(
        snag.segments.len() < alive.segments.len(),
        "dead branches fall: {} of {}",
        snag.segments.len(),
        alive.segments.len()
    );
}

/// A guest grows on its host (plant forms F7): every segment of a climber
/// or an epiphyte's colony stays near the wood of the host it is grown on.
#[test]
fn guests_grow_on_their_hosts() {
    for (id, near) in [
        ("hedera-helix", 0.6),
        ("vitis-californica", 4.0),
        ("tillandsia-usneoides", 2.5),
        ("phoradendron-californicum", 1.0),
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        let host = spec.host_geometry().unwrap().expect("a host");
        let variant = spec.variant_list()[0];
        let oldest = *spec.growth.keyframes.last().unwrap();
        let growth = grow_variant(&spec, &variant, oldest, vec![oldest]);
        let graph = &growth.keyframes[0];
        assert!(!graph.segments.is_empty(), "{id} grew nothing");
        let far = graph
            .segments
            .iter()
            .filter(|segment| host.nearest(segment.start, near).is_none())
            .count();
        // A vine's first stems cross open ground to reach its host.
        assert!(
            far * 20 <= graph.segments.len(),
            "{id}: {far} of {} segments farther than {near} m from the host",
            graph.segments.len()
        );
        let up = graph
            .segments
            .iter()
            .map(|segment| segment.end.y)
            .fold(0.0, f64::max);
        assert!(up > 3.0, "{id} stays near the ground ({up} m)");
    }
}

/// An ivy past its juvenile period (spec rework P6) bears juvenile,
/// palmate leaves on its climbing shoots and adult, entire leaves and its
/// fruit on adult ones, and nothing flowers or fruits on a juvenile shoot.
#[test]
fn an_old_ivy_fruits_only_on_adult_shoots() {
    let spec = PlantSpec::builtin("hedera-helix").unwrap();
    let variant = spec.variant_list()[0];
    let oldest = *spec.growth.keyframes.last().unwrap();
    let growth = grow_variant(&spec, &variant, oldest, vec![9.0, oldest]);
    let (program, _) = spec.program().unwrap();
    let looks = spec.appearance.looks(program.organs());
    let kind = |organ: &plantgen::graph::GraphOrgan| {
        growth.organ_types[usize::from(organ.organ)].name.as_str()
    };
    // Before its juvenile period ends every shoot is juvenile.
    assert!(
        growth.keyframes[0]
            .organs
            .iter()
            .all(|organ| organ.adult == Some(0.0)),
        "an adult shoot before 10 years"
    );
    let graph = &growth.keyframes[1];
    let leaves = |adult: bool| {
        graph
            .organs
            .iter()
            .filter(|organ| {
                kind(organ) == "leaf" && organ.adult.is_some_and(|a| (a >= 0.5) == adult)
            })
            .collect::<Vec<_>>()
    };
    let (juvenile, adult) = (leaves(false), leaves(true));
    assert!(
        !adult.is_empty() && juvenile.len() > adult.len(),
        "{} juvenile and {} adult leaves",
        juvenile.len(),
        adult.len()
    );
    let leaf = growth
        .organ_types
        .iter()
        .position(|organ| organ.name == "leaf")
        .unwrap();
    let forms = looks[leaf].forms;
    let palmate = forms.juvenile.unwrap().0;
    assert!(
        juvenile
            .iter()
            .all(|organ| forms.of(leaf, organ).0 == palmate)
    );
    assert!(adult.iter().all(|organ| forms.of(leaf, organ).0 == leaf));
    assert!(matches!(
        looks[leaf].shape,
        plantgen::looks::Shape::Simple(_)
    ));
    assert!(matches!(
        looks[palmate].shape,
        plantgen::looks::Shape::Palmate(_)
    ));
    let fruit: Vec<_> = graph
        .organs
        .iter()
        .filter(|organ| kind(organ) == "fruit")
        .collect();
    assert!(!fruit.is_empty(), "no fruit at {oldest} years");
    assert!(fruit.iter().all(|organ| organ.adult == Some(1.0)));
}

/// The juvenile period a species states sets the first year of flowers or
/// cones where its program counts it by the plant's age (`adult_age`, spec
/// rework P6), and nothing elsewhere; a value set by hand stays.
#[test]
fn the_juvenile_period_sets_adult_age() {
    let adult_age = |id: &str| {
        let spec = PlantSpec::builtin(id).unwrap();
        spec.generator.params.get("adult_age").copied()
    };
    // Douglas-fir: 7 to 30 years, read as their middle.
    assert_eq!(adult_age("pseudotsuga-menziesii"), Some(18.5));
    assert_eq!(adult_age("hedera-helix"), Some(10.0));
    // Pacific madrone states none and keeps its own.
    assert_eq!(adult_age("arbutus-menziesii"), Some(8.0));
    // Joshua tree's rosette program counts a crown's age, not the plant's.
    let joshua = PlantSpec::builtin("yucca-brevifolia").unwrap();
    assert_eq!(adult_age("yucca-brevifolia"), None);
    assert_eq!(joshua.generator.params.get("flower_age"), Some(&25.0));
}

/// Open-grown broadleaf crowns hold the leaf area per crown area of the
/// USDA Urban Tree Database's open-grown trees of their species, or of
/// their genus where it has none (`usfs-urban-tree-database-2016`, leaf
/// area over the area of the mean crown diameter, between its 10th and
/// 90th percentiles at the age), and an old oak's inner leaves grow in
/// shade (crown calibration): short shoots live while they have light.
#[test]
fn open_crowns_hold_the_leaf_area_of_open_grown_trees() {
    // Species, age, the database's 10th and 90th percentiles, its trees.
    for (id, age, low, high) in [
        // Longview, WA: 15 trees 60 to 200 years old.
        ("acer-macrophyllum", 120.0, 4.4, 8.7),
        // Longview: 13 trees 35 to 60.
        ("fraxinus-latifolia", 50.0, 2.1, 17.2),
        // Longview, as Populus balsamifera subsp. trichocarpa: 12 trees 35 to 60.
        ("populus-tristis", 40.0, 4.2, 14.8),
        // Sacramento's valley oaks (Quercus lobata), white oaks as it is:
        // 18 trees 60 to 200.
        ("quercus-garryana", 80.0, 1.9, 5.3),
        // Longview's European white birches: 9 trees 35 to 60.
        ("betula-papyrifera", 40.0, 1.8, 11.1),
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        let variant = *spec
            .variant_list()
            .iter()
            .find(|variant| variant.environment == spec::Environment::Open)
            .unwrap();
        let growth = grow_variant(&spec, &variant, age, vec![age]);
        let graph = &growth.keyframes[0];
        let radius = graph.crown_radius();
        let index = graph.leaf_area(&growth.organ_types) / (std::f64::consts::PI * radius * radius);
        assert!(
            (low..=high).contains(&index),
            "{id} at {age} years: {index:.2} m² of leaf per m² of crown, the database {low} to {high}"
        );
        if id == "quercus-garryana" {
            let leaf = |organ: &&plantgen::graph::GraphOrgan| {
                growth.organ_types[usize::from(organ.organ)].name == "leaf"
            };
            let leaves: Vec<_> = graph.organs.iter().filter(leaf).collect();
            let shaded = leaves
                .iter()
                .filter(|organ| plantgen::looks::light_form(organ.light) < -0.25)
                .count();
            assert!(
                shaded * 10 >= leaves.len(),
                "{shaded} of {} leaves in shade",
                leaves.len()
            );
        }
    }
}

/// A tree grown in a steady wind leans its crown downwind (plant forms
/// F7): the lenga's leaves lie mostly on the lee side (+X) of its stem.
#[test]
fn a_tree_in_a_steady_wind_flags_downwind() {
    let spec = PlantSpec::builtin("nothofagus-pumilio").unwrap();
    let variant = spec.variant_list()[0];
    let growth = grow_variant(&spec, &variant, 60.0, vec![60.0]);
    let graph = &growth.keyframes[0];
    let lee = graph
        .organs
        .iter()
        .filter(|organ| organ.position.x > 0.0)
        .count();
    assert!(
        lee * 10 >= graph.organs.len() * 7,
        "{lee} of {} organs downwind",
        graph.organs.len()
    );
}

/// Whether `spec` is grown by `broadleaf` or by a program that extends it
/// (growth plan G1: the maples' `sapindaceae`).
/// Whether the species' stated months (`flowering_months`,
/// `fruiting_months`) put it in flower or fruit in the month of the
/// forest's default day; where it states none, its looks are hand-built
/// for that day.
fn in_season_on_the_default_day(id: &str) -> bool {
    let traits = plantgen::library::Library::builtin().traits_of(id).unwrap();
    let stated: Vec<_> = plantgen::checks::MONTHS
        .iter()
        .filter_map(|name| traits.get(*name))
        .collect();
    stated.is_empty()
        || stated
            .iter()
            .any(|months| plantgen::checks::months_of(months).contains(&7))
}

fn grown_as_broadleaf(spec: &PlantSpec) -> bool {
    let library = plantgen::library::Library::builtin();
    let mut name = spec.generator.program.clone();
    for _ in 0..plantgen::lsys::chain::MAX_DEPTH {
        if name == "broadleaf" {
            return true;
        }
        match library
            .program_text(&name)
            .and_then(plantgen::lsys::parser::extends_of)
        {
            Some(parent) => name = parent,
            None => return false,
        }
    }
    false
}

/// A herb whose leaves and flowers rise on separate stalks: with
/// `flowering_share` a third of an adult cohort's stems end in a raceme and
/// the rest bear leaves only, and with `scape` 1 a flowering stem bears no
/// leaves, so vanilla leaf grown on a stem a leaf draws a leaf per leafy
/// stem and a raceme per flowering one.
#[test]
fn a_herb_may_flower_on_some_stems_and_leaf_on_the_rest() {
    let mut spec = PlantSpec::builtin("achlys-triphylla").unwrap();
    let variant = spec.variant_list()[0];
    let count = |spec: &PlantSpec| {
        let growth = grow_variant(spec, &variant, 6.0, vec![6.0]);
        let index = |name: &str| {
            growth
                .organ_types
                .iter()
                .position(|organ| organ.name == name)
                .unwrap()
        };
        let (leaf, flower) = (index("leaf"), index("flower"));
        let organs = &growth.keyframes[0].organs;
        let of = |kind: usize| {
            organs
                .iter()
                .filter(|organ| usize::from(organ.organ) == kind)
                .count()
        };
        (of(leaf), of(flower))
    };
    let params = &mut spec.generator.params;
    params.insert("stems".into(), 18.0);
    params.insert("leaves_per_node".into(), 1.0);
    let (leaves, flowers) = count(&spec);
    assert_eq!(leaves, flowers, "every stem flowers and bears its leaf");
    let params = &mut spec.generator.params;
    params.insert("flowering_share".into(), 1.0 / 3.0);
    params.insert("scape".into(), 1.0);
    let (leaves_apart, flowers_apart) = count(&spec);
    assert_eq!(
        leaves_apart + flowers_apart,
        leaves,
        "a stem is one or the other"
    );
    assert_eq!(flowers_apart, leaves / 3, "every third stem flowers");
}
