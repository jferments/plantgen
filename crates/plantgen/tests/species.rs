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
        &params,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes,
            neighbourhood: variant.neighbourhood,
            limits: Limits::default(),
            host: spec.host_geometry().unwrap(),
        },
    )
    .unwrap()
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
/// a viewer sees.
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
            let drawn = templates::drawn_area(look, template);
            if !spec::areas_agree(drawn, *shaded) {
                failures.push(format!(
                    "{id} `{}` draws {drawn:.3} m² on an organ 1 m long but shades with {shaded:.3} m²",
                    look.organ
                ));
            }
        }
        // Sun leaves draw less than their type's look and shade leaves
        // more, about evenly, so a crown with as many of each draws what
        // the light model shades with.
        for (index, look) in looks.iter().enumerate().take(shading.len()) {
            let forms = look.forms;
            let area = |index: usize, size: f64| {
                templates::drawn_area(&looks[index], &templates.templates[index]) * size * size
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
            let (Some(sun), Some(shade)) = (forms.sun, forms.shade) else {
                continue;
            };
            let (own, sun, shade) = (
                area(index, 1.0),
                area(sun, forms.sizes.0),
                area(shade, forms.sizes.1),
            );
            if !(sun < own && own < shade && spec::areas_agree(f64::midpoint(sun, shade), own)) {
                failures.push(format!(
                    "{id} `{}`: sun leaves draw {sun:.3} m², its own look {own:.3}, shade leaves {shade:.3}",
                    look.organ
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Grasses and herbs renew their shoots every year: at every keyframe
/// each stem, culm, leaf and flower grew that year, so nothing lives on
/// from the year before but a grass's thatch, the dead blades of last
/// year's tillers.
#[test]
fn grasses_and_herbs_renew_their_shoots_every_year() {
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        if !matches!(spec.growth_form, GrowthForm::Graminoid | GrowthForm::Forb) {
            continue;
        }
        checked += 1;
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
            for segment in &graph.segments {
                assert_eq!(
                    segment.born.to_bits(),
                    graph.age.to_bits(),
                    "{id}: a stem outlived its year"
                );
                assert_eq!(segment.shed, (graph.age < 4.0).then_some(graph.age + 1.0));
            }
            for organ in &graph.organs {
                assert_eq!(
                    organ.born.to_bits(),
                    graph.age.to_bits(),
                    "{id}: an organ outlived its year"
                );
            }
            if let Some(thatch) = thatch {
                let dead = graph
                    .organs
                    .iter()
                    .filter(|organ| usize::from(organ.organ) == thatch)
                    .count();
                // Thatch from the second year on, when the first tillers die.
                assert_eq!(
                    dead > 0,
                    graph.age >= 2.0,
                    "{id} at {}: {dead} thatch",
                    graph.age
                );
            }
        }
    }
    assert!(checked >= 2, "only {checked} grasses and herbs");
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
                    assert!((parent.order..=parent.order + 1).contains(&segment.order));
                    // The pipe model and annual rings: no branch is thicker
                    // than the wood it grows from. A fleshy body is not
                    // wood: a cactus pad can be wider than the pad it
                    // grows from.
                    assert!(
                        segment.body > 0 || segment.radius <= parent.radius * (1.0 + 1e-9),
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
    // berry to a long raceme of samaras.
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let fruiting = spec
            .appearance
            .organs
            .get("bloom")
            .is_some_and(|look| matches!(look.shape, plantgen::looks::Shape::Fruit(_)));
        if spec.generator.program != "broadleaf" || !fruiting {
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
        let sizes: Vec<f64> = growth.keyframes[0]
            .organs
            .iter()
            .filter(|organ| usize::from(organ.organ) == bloom)
            .map(|organ| organ.size)
            .collect();
        assert!(!sizes.is_empty(), "{id}: no fruit at {oldest} years");
        assert!(
            sizes.iter().all(|&size| size > 0.005 && size < 0.25),
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
    // default day each still bears its fruit.
    use plantgen::looks::{Shape, Stage};
    let mut checked = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let Some(bloom) = spec.appearance.organs.get("bloom") else {
            continue;
        };
        if spec.generator.program != "broadleaf" || !matches!(bloom.shape, Shape::Fruit(_)) {
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
            matches!(season.stage(day), Stage::Fruit { .. }),
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
        if spec.generator.program != "conifer" {
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
        checked, 11,
        "the ten conifers of the forest and the bald cypress"
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
                part.triangles() <= parts::SHOOT_TRIANGLES,
                "{id}: {}",
                part.triangles()
            );
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
        checked, 10,
        "the needled conifers, the bald cypress and the juniper's juvenile needles"
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
    let spec = PlantSpec::builtin("prosopis-velutina").unwrap();
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
