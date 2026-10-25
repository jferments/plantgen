//! Baked-card tiers (`plantgen::billboards`): each keeps within its
//! triangle and texel budgets, every card has a picture with something
//! in it, the cards stay inside the full plant's reach, a build gives
//! the same bytes twice, the pictures pack into one atlas, the far
//! impostor is one card with a picture from every way, a cholla's thin
//! joints are baked, and an agave's long leaves are drawn whole.
//! `cargo test --release -p plantgen --test billboards`.

use plantgen::billboards::{self, CORNERS, CardPlant, FAR, TIERS};
use plantgen::body::BodyLook;
use plantgen::graph::PlantGraph;
use plantgen::grow::{GrowthSettings, grow};
use plantgen::looks::Look;
use plantgen::lsys::Limits;
use plantgen::package::DEFAULT_DAY;
use plantgen::quality::STANDARD;
use plantgen::spec::PlantSpec;
use plantgen::templates::Templates;

/// A fern, a grass and a forb: lace, blades on culms, and flowers.
const SPECIES: [&str; 3] = [
    "polystichum-munitum",
    "elymus-glaucus",
    "epilobium-angustifolium",
];

/// `f` of a species' oldest plant as drawn, its looks, bodies, spec and
/// templates.
fn with_plant<T>(
    id: &str,
    f: impl FnOnce(&PlantGraph, &[Look], &[BodyLook], &PlantSpec, &Templates) -> T,
) -> T {
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
    f(drawn, &looks, &bodies, &spec, &templates)
}

fn carded(id: &str) -> (f64, f64, Vec<CardPlant>) {
    with_plant(id, |drawn, looks, bodies, spec, templates| {
        let reach = drawn
            .segments
            .iter()
            .flat_map(|s| [s.start, s.end])
            .chain(drawn.organs.iter().map(|o| o.position))
            .map(|p| p.x.hypot(p.z))
            .fold(0.0, f64::max);
        let plants = TIERS
            .iter()
            .map(|tier| {
                billboards::build(
                    drawn,
                    looks,
                    bodies,
                    &spec.appearance,
                    spec.growth_form,
                    templates,
                    &STANDARD.lods,
                    tier,
                )
            })
            .collect();
        (drawn.height, reach, plants)
    })
}

#[test]
fn tiers_keep_their_budgets() {
    for id in SPECIES {
        let (height, _, plants) = carded(id);
        for (tier, plant) in TIERS.iter().zip(&plants) {
            let cards = tier.cards(height);
            let stats = &plant.stats;
            assert!(
                stats.cards <= cards,
                "{id} {}: {} cards of {cards}",
                tier.name,
                stats.cards
            );
            assert!(
                stats.card_triangles <= cards * (CORNERS - 2),
                "{id} {}: {} card triangles",
                tier.name,
                stats.card_triangles
            );
            // Tubes and ribbons take at most two thirds as many as the
            // cards may.
            assert!(
                stats.wood_triangles <= cards * (CORNERS - 2) * 2 / 3,
                "{id} {}: {} wood triangles",
                tier.name,
                stats.wood_triangles
            );
            assert!(
                stats.texels <= tier.texels(height),
                "{id} {}: {} texels",
                tier.name,
                stats.texels
            );
        }
    }
}

#[test]
fn every_card_shows_something() {
    for id in SPECIES {
        let (_, _, plants) = carded(id);
        for (tier, plant) in TIERS.iter().zip(&plants) {
            assert!(plant.stats.cards > 0, "{id} {}: no cards", tier.name);
            for (i, picture) in plant.pictures.iter().enumerate() {
                let covered = picture.color.iter().filter(|c| c[3] >= 0.5).count();
                assert!(covered > 0, "{id} {}: picture {i} is empty", tier.name);
            }
        }
    }
}

#[test]
fn cards_stay_within_the_plant() {
    for id in SPECIES {
        let (height, reach, plants) = carded(id);
        for (tier, plant) in TIERS.iter().zip(&plants) {
            for p in &plant.cards.positions {
                let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                // A card's outline may grow past its leaves by a little
                // where its corners are cut ([`CORNERS`]).
                assert!(
                    x.hypot(z) <= reach * 1.25 + height * 0.05,
                    "{id} {}: a card corner {:.3} m out, the plant reaches {reach:.3} m",
                    tier.name,
                    x.hypot(z)
                );
                assert!(
                    (-height * 0.1..=height * 1.25).contains(&y),
                    "{id} {}: a card corner at {y:.3} m, the plant is {height:.3} m",
                    tier.name
                );
            }
        }
    }
}

#[test]
fn builds_are_deterministic() {
    let (_, _, first) = carded(SPECIES[1]);
    let (_, _, second) = carded(SPECIES[1]);
    assert_eq!(first, second);
}

#[test]
fn pictures_pack_into_one_atlas() {
    let (_, _, plants) = carded(SPECIES[2]);
    for plant in &plants {
        let atlas = plant.atlas();
        assert!(atlas.width.is_power_of_two(), "{} wide", atlas.width);
        let rects: Vec<(usize, usize, usize, usize)> = plant
            .pictures
            .iter()
            .zip(&atlas.places)
            .map(|(p, &(x, y))| (x, y, x + p.width, y + p.height))
            .collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(
                a.2 <= atlas.width && a.3 <= atlas.height,
                "picture {i} spills"
            );
            for b in &rects[i + 1..] {
                let apart = a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1;
                assert!(apart, "pictures overlap: {a:?} {b:?}");
            }
        }
        assert!(atlas.fill(&plant.pictures) > 0.5);
    }
}

#[test]
fn the_far_impostor_is_one_card_seen_from_every_way() {
    let plant = with_plant(SPECIES[2], |drawn, looks, bodies, spec, templates| {
        billboards::impostor(
            drawn,
            looks,
            bodies,
            &spec.appearance,
            spec.growth_form,
            templates,
            &STANDARD.lods,
            &FAR,
        )
    });
    assert_eq!(plant.stats.cards, 1);
    assert!(plant.stats.card_triangles <= CORNERS - 2);
    assert_eq!(plant.pictures.len(), FAR.around * 2 + 1);
    for (i, picture) in plant.pictures.iter().enumerate() {
        let covered = picture.color.iter().filter(|c| c[3] >= 0.5).count();
        assert!(covered > 0, "view {i} is empty");
    }
}

#[test]
fn a_chollas_thin_joints_are_baked() {
    // Drawn whole at the coarsest level, its joints take about 24,000
    // triangles.
    let (_, _, plants) = carded("cylindropuntia-fulgida");
    for (tier, plant) in TIERS.iter().zip(&plants) {
        assert!(
            plant.stats.wood_triangles < 6_000,
            "{}: {} wood triangles",
            tier.name,
            plant.stats.wood_triangles
        );
        assert!(plant.stats.wood_triangles > 0, "{}: no trunk", tier.name);
        assert!(plant.stats.cards > 0, "{}: no cards", tier.name);
    }
}

#[test]
fn an_agaves_long_leaves_are_drawn_whole() {
    let (height, _, plants) = carded("agave-deserti");
    for (tier, plant) in TIERS.iter().zip(&plants) {
        let leaves = plant.stats.leaf_triangles;
        assert!(leaves > 0, "{}: no leaf drawn whole", tier.name);
        assert_eq!(leaves % billboards::WHOLE_LEAF_TRIANGLES, 0);
        // Each blade takes a card's worth of triangles from the cards.
        assert!(
            plant.stats.card_triangles + leaves <= tier.cards(height) * (CORNERS - 2),
            "{}: {} card and {leaves} leaf triangles",
            tier.name,
            plant.stats.card_triangles
        );
    }
}
