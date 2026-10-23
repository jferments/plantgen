//! Solid low-poly tiers (`plantgen::lowpoly`): each keeps its budget,
//! draws the signature organs a species is known by, and keeps leaves.
//! `cargo test --release -p plantgen --test lowpoly`.

use plantgen::grow::{GrowthSettings, grow};
use plantgen::lowpoly::{self, SolidPlant, TIERS};
use plantgen::lsys::Limits;
use plantgen::package::DEFAULT_DAY;
use plantgen::quality::STANDARD;
use plantgen::spec::PlantSpec;
use plantgen::templates::Templates;

fn solid(id: &str) -> Vec<SolidPlant> {
    let spec = PlantSpec::builtin(id).expect("a built-in species");
    let (program, params) = spec.program().expect("its program");
    let variant = spec.variant_list()[0];
    let age = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
    let growth = grow(
        &program,
        &params,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years: age,
            keyframes: vec![age],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().expect("its host"),
        },
    )
    .expect("it grows");
    let graph = growth.keyframes.last().expect("a keyframe");
    let day = spec.appearance.has_seasons().then_some(DEFAULT_DAY);
    let (looks, sizes) = spec.appearance.looks_on(program.organs(), day);
    let bodies = spec.appearance.body_looks(program.bodies());
    let named: Vec<(&str, &_)> = program.bodies().zip(&bodies).collect();
    let templates = Templates::for_plant(&looks, &named).with_bark(spec.appearance.bark_params());
    let staged = plantgen::looks::staged(graph, &sizes, &looks);
    let drawn = staged.as_ref().unwrap_or(graph);
    TIERS
        .iter()
        .map(|tier| {
            lowpoly::build(
                drawn,
                &looks,
                &bodies,
                &spec.appearance,
                &templates,
                &STANDARD.lods,
                tier,
            )
        })
        .collect()
}

#[test]
fn tiers_keep_their_budgets_flowers_and_leaves() {
    for id in ["digitalis-purpurea", "epilobium-angustifolium"] {
        for (tier, plant) in TIERS.iter().zip(solid(id)) {
            let stats = &plant.stats;
            assert!(
                stats.triangles() <= stats.budget,
                "{id} {}: {} triangles over {}",
                tier.name,
                stats.triangles(),
                stats.budget
            );
            assert!(stats.signature.0 > 0, "{id} {}: no flowers", tier.name);
            assert!(stats.foliage.0 > 0, "{id} {}: no leaves", tier.name);
            // Organs and wood are solid, opaque and untextured.
            assert!(plant.organs.colors.iter().all(|c| c[3] == 1.0));
            assert_eq!(plant.organs.indices.len() % 3, 0);
        }
    }
}

#[test]
fn a_fern_keeps_its_fronds() {
    for (tier, plant) in TIERS.iter().zip(solid("polystichum-munitum")) {
        let stats = &plant.stats;
        assert!(stats.triangles() <= stats.budget, "{}", tier.name);
        // Its pinnae drawn as fronds stand for most of them.
        assert!(
            stats.foliage.0 * 3 >= stats.foliage.1,
            "{}: {:?}",
            tier.name,
            stats.foliage
        );
    }
}

#[test]
fn succulents_keep_their_shape_within_budget() {
    // A prickly pear's pads drawn flat, its fruit kept; an agave's thick
    // leaves drawn as their cards.
    for (tier, plant) in TIERS.iter().zip(solid("opuntia-engelmannii")) {
        let stats = &plant.stats;
        assert!(
            stats.triangles() <= stats.budget,
            "{}: {stats:?}",
            tier.name
        );
        assert!(
            stats.wood_triangles > 0 && stats.signature.0 > 0,
            "{}: {stats:?}",
            tier.name
        );
    }
    for (tier, plant) in TIERS.iter().zip(solid("agave-deserti")) {
        let stats = &plant.stats;
        assert!(
            stats.triangles() <= stats.budget,
            "{}: {stats:?}",
            tier.name
        );
        assert!(stats.foliage.0 > 0, "{}: no leaves", tier.name);
    }
}
