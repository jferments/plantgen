//! Baked-card tiers (`plantgen::billboards`): each keeps within its
//! triangle and texel budgets, every card has a picture with something
//! in it, the cards stay inside the full plant's reach, a build gives
//! the same bytes twice, the pictures pack into one atlas, the far
//! impostor is one card with a picture from every way, a cholla's thin
//! joints are baked, an agave's long leaves are drawn whole, a yarrow's
//! corymbs are small impostors baked from its full model, a wild carrot's
//! umbels are baked flower by flower, a water lily's lone pads lie level,
//! and a plant with no pictures still packs into an atlas.
//! `cargo test --release -p plantgen --test billboards`.

use plantgen::billboards::{self, CORNERS, CardPlant, CardStats, CardTier, FAR, HEIGHTS, TIERS};
use plantgen::body::BodyLook;
use plantgen::graph::PlantGraph;
use plantgen::grow::{GrowthSettings, grow};
use plantgen::looks::Look;
use plantgen::lsys::Limits;
use plantgen::mesh::Mesh;
use plantgen::package::DEFAULT_DAY;
use plantgen::quality::STANDARD;
use plantgen::raster::{Material, Turn};
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
            // From any one height of eye; a turned card holds a picture
            // for each of three.
            assert!(
                stats.seen_texels <= tier.texels(height),
                "{id} {}: {} texels seen",
                tier.name,
                stats.seen_texels
            );
            assert!(
                stats.texels <= tier.texels(height) * HEIGHTS.len(),
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
            for (p, c) in plant.cards.positions.iter().zip(&plant.cards.colors) {
                let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
                // A card's outline may grow past its leaves by a little
                // where its corners are cut ([`CORNERS`]).
                assert!(
                    x.hypot(z) <= reach * 1.25 + height * 0.05,
                    "{id} {}: a card corner {:.3} m out, the plant reaches {reach:.3} m",
                    tier.name,
                    x.hypot(z)
                );
                // A card turned to the eye's height holds its parts as
                // seen from every height, from above too: as it stands
                // before it turns, a low clump's may reach below the
                // ground, where its pictures from the side are empty.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let picture = &plant.pictures[c[3] as usize];
                let low = if matches!(picture.turn, Turn::Heights { .. }) {
                    -height * 0.25
                } else {
                    -height * 0.1
                };
                assert!(
                    (low..=height * 1.25).contains(&y),
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

/// A tier with no pictures (a bare snag's, all wood) still packs into an
/// atlas an image can hold: After's `--plant-cards` build of
/// acer-macrophyllum stopped on "cannot encode PNG: Zero height not
/// allowed" (2026-10-27).
#[test]
fn a_plant_with_no_pictures_has_a_one_texel_atlas() {
    let plant = CardPlant {
        wood: Mesh::default(),
        cards: Mesh::default(),
        pictures: Vec::new(),
        stats: CardStats::default(),
    };
    let atlas = plant.atlas();
    assert_eq!((atlas.width, atlas.height), (1, 1));
    assert!(atlas.places.is_empty());
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

#[test]
fn a_yarrows_corymbs_are_drawn_solid() {
    // Pictured from nine ways, its corymbs blurred to white smudges close
    // up, where the low cards are the nearest version a weak machine
    // draws. Each is a solid dome over a shallow cone now, its colour the
    // corymb's from above: no picture turns to the eye for it.
    let (height, _, plants) = carded("achillea-millefolium");
    for (tier, plant) in TIERS.iter().zip(&plants) {
        assert!(
            !plant
                .pictures
                .iter()
                .any(|p| matches!(p.turn, Turn::Views { .. })),
            "{}: a head drawn as pictures",
            tier.name
        );
        assert!(
            plant.stats.wood_triangles >= billboards::HEAD_SIDES * 2,
            "{}: no solid head among {} wood triangles",
            tier.name,
            plant.stats.wood_triangles
        );
        assert!(plant.stats.cards <= tier.cards(height));
    }
    // Their tops are near white, as the photographs show them.
    let low = &plants[0];
    let whitest = low
        .wood
        .colors
        .iter()
        .map(|c| c[0].min(c[1]).min(c[2]))
        .fold(0.0_f32, f32::max);
    assert!(whitest > 0.35, "the whitest head colour is {whitest:.2}");
}

#[test]
fn a_sagebrushs_crown_cards_are_baked_from_each_height() {
    let (_, _, plants) = carded("artemisia-tridentata");
    for (tier, plant) in TIERS.iter().zip(&plants) {
        let mut turned = 0;
        for (i, p) in plant.pictures.iter().enumerate() {
            let Turn::Heights { first, count } = p.turn else {
                continue;
            };
            assert_eq!(count, HEIGHTS.len());
            if first != i {
                continue;
            }
            turned += 1;
            // Level, from 45 degrees above and from above, each the same
            // size about the same middle, so one outline serves all.
            let rises: Vec<f32> = (first..first + count)
                .map(|v| plant.pictures[v].seen_from[1])
                .collect();
            assert!(rises[0].abs() < 1e-3, "{}: {rises:?}", tier.name);
            assert!(
                (rises[1] - 0.5_f32.sqrt()).abs() < 1e-3,
                "{}: {rises:?}",
                tier.name
            );
            assert!(rises[2] > 0.999, "{}: {rises:?}", tier.name);
            for v in first..first + count {
                let q = &plant.pictures[v];
                assert_eq!((q.width, q.height), (p.width, p.height));
                assert_eq!(q.pivot, p.pivot);
            }
        }
        assert!(turned > 0, "{}: no crown card turns", tier.name);
    }
}

#[test]
fn a_wild_carrots_umbels_are_baked_flower_by_flower() {
    // From the second level, where its umbel is one flat card, the far
    // impostor hid half as much sky again as the carrot from 45 degrees.
    let (whole, far) = with_plant("daucus-carota", |drawn, looks, bodies, spec, templates| {
        let full = plantgen::mesh::build(
            drawn,
            looks,
            bodies,
            &spec.appearance,
            &STANDARD.lods[0].for_height(drawn.height),
            0,
        );
        let bark = templates.bark.map_or(Material::Opaque, Material::Bark);
        let whole = billboards::tone(
            &[(&full.wood, bark), (&full.card_mesh(), Material::Card)],
            templates,
            160,
        );
        let plant = billboards::impostor(
            drawn,
            looks,
            bodies,
            &spec.appearance,
            spec.growth_form,
            templates,
            &STANDARD.lods,
            &FAR,
        );
        let mut baked = templates.clone();
        baked.baked.clone_from(&plant.pictures);
        let far = billboards::tone(&[(&plant.cards, Material::Baked)], &baked, 160);
        (whole, far)
    });
    for (view, (whole, far)) in whole.cover.iter().zip(far.cover).enumerate() {
        let ratio = far / whole;
        assert!(
            (0.8..=1.2).contains(&ratio),
            "view {view}: the impostor hides {ratio:.2} of the sky the plant does"
        );
    }
}

#[test]
fn a_water_lilys_lone_pads_lie_on_the_water() {
    // A pad alone on its card stood on edge, as any plane through a lone
    // leaf's middle line was as good as another, and from 45 degrees the
    // low cards hid a third less sky than the plant.
    let (whole, low) = full_and_tier("nymphaea-odorata", &TIERS[0]);
    let ratio = low.cover[1] / whole.cover[1];
    assert!(
        (0.8..=1.2).contains(&ratio),
        "from 45 degrees the low cards hide {ratio:.2} of the sky the plant does"
    );
}

/// How `species` draws in full and on card tier `tier` ([`billboards::tone`]).
fn full_and_tier(species: &str, tier: &CardTier) -> (billboards::Drawn, billboards::Drawn) {
    with_plant(species, |drawn, looks, bodies, spec, templates| {
        let full = plantgen::mesh::build(
            drawn,
            looks,
            bodies,
            &spec.appearance,
            &STANDARD.lods[0].for_height(drawn.height),
            0,
        );
        let bark = templates.bark.map_or(Material::Opaque, Material::Bark);
        let whole = billboards::tone(
            &[(&full.wood, bark), (&full.card_mesh(), Material::Card)],
            templates,
            160,
        );
        let plant = billboards::build(
            drawn,
            looks,
            bodies,
            &spec.appearance,
            spec.growth_form,
            templates,
            &STANDARD.lods,
            tier,
        );
        let mut baked = templates.clone();
        baked.baked.clone_from(&plant.pictures);
        let carded = billboards::tone(
            &[(&plant.wood, bark), (&plant.cards, Material::Baked)],
            &baked,
            160,
        );
        (whole, carded)
    })
}

#[test]
fn a_carrots_card_tiers_keep_its_pale_stem_and_its_umbel() {
    // Its flowering stem, a ribbon on the minimal cards, took the dark
    // green of the leaves along it, and on both tiers its umbel lay level
    // with the leaves' clump: from the side the minimal cards drew half as
    // bright as the carrot, the low cards a quarter darker. Its umbel
    // drawn solid is narrower from the side than the lacy umbel of the
    // plant: the low cards 0.84 as bright.
    for tier in &TIERS {
        let (whole, carded) = full_and_tier("daucus-carota", tier);
        let ratio = carded.tone[0] / whole.tone[0];
        assert!(
            (0.8..=1.2).contains(&ratio),
            "{}: from the side the cards draw {ratio:.2} as bright as the plant",
            tier.name
        );
    }
}

#[test]
fn tubercled_bodies_keep_their_tone_on_the_cards() {
    // Smooth where the plant is all mounds, a living rock's and a
    // pincushion's coarse bodies took the sun their tubercles shade each
    // other from: the low cards drew 1.7 and 1.6 times as bright as the
    // plant from the side.
    for species in ["ariocarpus-fissuratus", "cochemiea-grahamii"] {
        let (whole, carded) = full_and_tier(species, &TIERS[0]);
        let ratio = carded.tone[0] / whole.tone[0];
        assert!(
            ratio <= 1.3,
            "{species}: from the side the cards draw {ratio:.2} as bright as the plant"
        );
    }
}

#[test]
fn a_peyotes_spines_and_wool_are_drawn_on_its_body() {
    // Baked onto cards of their own, its tufts of wool floated beside its
    // coarser body as white chips: 40 cards on the low tier where its
    // flowers need 9. Its body draws them in its skin.
    let (_, _, plants) = carded("lophophora-williamsii");
    for (tier, plant) in TIERS.iter().zip(&plants) {
        assert!(
            plant.stats.cards <= 10,
            "{}: {} cards",
            tier.name,
            plant.stats.cards
        );
    }
}
