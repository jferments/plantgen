//! Layouts (the crown-gap variants agreed with After): a plant grown among
//! neighbours like itself makes room for them as it would in a stand of
//! its like, and grows exactly what the middle plant of such a stand grows.

use plantgen::conditions::{Conditions, Layout, LayoutKind, Neighbours};
use plantgen::graph::PlantGraph;
use plantgen::grow::{Grower, Growth, GrowthSettings, grow};
use plantgen::lsys::{Limits, Program};
use plantgen::math::Vec3;
use plantgen::spec::{Environment, PlantSpec};
use plantgen::stand::{Member, grow_stand};

const YEARS: f64 = 15.0;

struct Maple {
    program: Program,
    params: Vec<f64>,
    settings: GrowthSettings,
}

/// Bigleaf maple's first open-grown variant, alone, for `YEARS`.
fn maple() -> Maple {
    let spec = PlantSpec::builtin("acer-macrophyllum").unwrap();
    let variant = spec
        .variant_list()
        .into_iter()
        .find(|variant| variant.environment == Environment::Open)
        .unwrap();
    let (program, params) = spec.program().unwrap();
    let params = spec.seeded_params(&program, &params, variant.seed);
    Maple {
        program,
        params,
        settings: GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years: YEARS,
            keyframes: vec![YEARS],
            conditions: Conditions::preset(Environment::Open),
            limits: Limits::default(),
            host: None,
        },
    }
}

/// The maple grown in `layout` with its neighbours `spacing_m` apart.
fn in_layout(maple: &Maple, kind: LayoutKind, spacing_m: f64) -> Growth {
    let mut neighbours = Neighbours::of(Environment::Open.neighbourhood());
    neighbours.layout = Some(Layout {
        kind,
        spacing_m,
        per_height: 0.0,
    });
    let settings = GrowthSettings {
        conditions: Conditions {
            neighbours: Some(neighbours),
            ..maple.settings.conditions.clone()
        },
        ..maple.settings.clone()
    };
    settings.conditions.validate().unwrap();
    grow(&maple.program, &maple.params, &settings).unwrap()
}

/// The mean place of its organs (leaves, mostly), east and north of its
/// foot.
fn organs_centre(graph: &PlantGraph) -> [f64; 2] {
    #[allow(clippy::cast_precision_loss)]
    let count = graph.organs.len().max(1) as f64;
    let sum = graph
        .organs
        .iter()
        .fold(Vec3::ZERO, |sum, organ| sum + organ.position);
    [sum.x / count, -sum.z / count]
}

/// How far its organs spread east-west and north-south: the root mean
/// square of their distance from its foot along each.
fn organs_spread(graph: &PlantGraph) -> [f64; 2] {
    #[allow(clippy::cast_precision_loss)]
    let count = graph.organs.len().max(1) as f64;
    let (x, z) = graph.organs.iter().fold((0.0, 0.0), |(x, z), organ| {
        (
            x + organ.position.x * organ.position.x,
            z + organ.position.z * organ.position.z,
        )
    });
    [(x / count).sqrt(), (z / count).sqrt()]
}

#[test]
fn an_interior_layout_grows_the_middle_plant_of_a_tile() {
    let maple = maple();
    // The crown grown alone, to space the neighbours so that the nearest
    // ring reaches the plant and the next lies out of reach.
    let mut grower = Grower::new(&maple.program, &maple.params, &maple.settings)
        .unwrap()
        .with_crown();
    while grower.step(None).unwrap() {}
    let crown = grower.crown().unwrap();
    let reach = 2.0 * crown.radius_m + 2.0 * crown.top_m;
    let spacing = 0.6 * reach;
    assert!(crown.reaches([spacing, 0.0], crown.radius_m));
    assert!(!crown.reaches([2.0 * spacing, 0.0], crown.radius_m));
    let laid_out = in_layout(&maple, LayoutKind::Interior, spacing);
    let tile = grow_stand(
        &[Member {
            program: &maple.program,
            params: &maple.params,
            settings: &maple.settings,
            at_m: [0.5 * spacing, 0.5 * spacing],
        }],
        Some([spacing, spacing]),
    )
    .unwrap();
    assert_eq!(laid_out.digest(), tile[0].digest());
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    assert_ne!(laid_out.digest(), alone.digest());
}

#[test]
fn an_edge_layout_leans_the_crown_away_from_its_neighbours() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    let edge = in_layout(&maple, LayoutKind::Edge, 3.0);
    let (alone, edge) = (&alone.keyframes[0], &edge.keyframes[0]);
    // Neighbours stand west (−X): the crown leans east.
    let lean = organs_centre(edge)[0] - organs_centre(alone)[0];
    assert!(lean > 0.2, "the edge tree leans {lean:.2} m east");
    assert!(edge.organs.len() < alone.organs.len());
}

#[test]
fn a_row_layout_narrows_the_crown_across_the_row() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    let row = in_layout(&maple, LayoutKind::Row, 3.0);
    let (alone, row) = (
        organs_spread(&alone.keyframes[0]),
        organs_spread(&row.keyframes[0]),
    );
    // Neighbours east and west: the crown spreads less that way than
    // alone, and less than along the row.
    assert!(
        row[0] < 0.9 * alone[0] && row[0] < row[1],
        "spread east-west {:.2} m (alone {:.2} m), north-south {:.2} m",
        row[0],
        alone[0],
        row[1]
    );
}

#[test]
fn a_layout_too_wide_to_reach_grows_the_plant_grown_alone() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    let apart = in_layout(&maple, LayoutKind::Interior, 100.0);
    assert_eq!(apart.digest(), alone.digest());
}
