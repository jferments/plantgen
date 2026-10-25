//! Baked-card tiers (`plantgen::billboards`): each keeps within its
//! triangle and texel budgets, every card has a picture with something
//! in it, the cards stay inside the full plant's reach, and a build gives
//! the same bytes twice.
//! `cargo test --release -p plantgen --test billboards`.

use plantgen::billboards::{self, CORNERS, CardPlant, TIERS};
use plantgen::grow::{GrowthSettings, grow};
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

fn carded(id: &str) -> (f64, f64, Vec<CardPlant>) {
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
                &looks,
                &bodies,
                &spec.appearance,
                &templates,
                &STANDARD.lods,
                tier,
            )
        })
        .collect();
    (drawn.height, reach, plants)
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
