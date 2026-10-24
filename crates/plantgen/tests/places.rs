//! Plants grown on in a place (fitting plan PF3, PF4): a tree's crown
//! makes room for a wall at a tree's scale and shies from a neighbour's
//! crown, and grown on from an age it is the library's plant until then.

use plantgen::conditions::{Conditions, Intrusion};
use plantgen::graph::PlantGraph;
use plantgen::grow::{Growth, GrowthSettings, grow_in};
use plantgen::lsys::Limits;
use plantgen::math::Vec3;
use plantgen::spec::{Environment, PlantSpec};

/// Bigleaf maple, its first open-grown variant, for `years`, keeping
/// `keyframes`, grown on in `place` from `from_age` years.
fn maple(place: &str, from_age: f64, years: f64, keyframes: Vec<f64>) -> Growth {
    let spec = PlantSpec::builtin("acer-macrophyllum").unwrap();
    let variant = spec
        .variant_list()
        .into_iter()
        .find(|variant| variant.environment == Environment::Open)
        .unwrap();
    let (program, params) = spec.program().unwrap();
    grow_in(
        &program,
        &spec.seeded_params(&program, &params, variant.seed),
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes,
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: None,
        },
        &Conditions::from_json(place).unwrap(),
        from_age,
        None,
    )
    .unwrap()
}

const OPEN: &str = r#"{"schema": 1}"#;
/// A wall 15 m high whose face stands 1.5 m north of the trunk.
const WALL: &str = r#"{"schema": 1, "substrate": {"version": 1, "preset": "wall", "scale": 5}}"#;
/// A neighbour's crown 4.2 m across, its axis 3.6 m east, from 1 m to 15 m up.
const NEIGHBOUR: &str = r#"{"schema": 1, "neighbours": {"version": 1, "density": 0,
    "relative_height": 0, "canopy": 0, "spacing": 0, "one_sided": false,
    "envelopes": [{"centre_m": [3.6, 0], "base_m": 1, "top_m": 15, "radius_m": 2.1,
                   "density": 0.5}]}}"#;

fn intrusion(place: &str, graph: &PlantGraph) -> Intrusion {
    Conditions::from_json(place)
        .unwrap()
        .intrusion(graph)
        .unwrap()
}

/// The mean place of its organs (leaves, mostly).
fn organs_centre(graph: &PlantGraph) -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let count = graph.organs.len().max(1) as f64;
    graph
        .organs
        .iter()
        .fold(Vec3::ZERO, |sum, organ| sum + organ.position)
        / count
}

#[test]
fn a_crown_makes_room_for_a_wall_and_shies_from_a_neighbour() {
    let open = maple(OPEN, 0.0, 15.0, vec![15.0]);
    let open = &open.keyframes[0];

    // Grown by the wall, its shoots find no buds' goals inside it: of the
    // parts the open-grown crown puts there, fewer than half go in, and
    // none deeper than a year's shoot.
    let walled = maple(WALL, 0.0, 15.0, vec![15.0]);
    let (blind, seen) = (intrusion(WALL, open), intrusion(WALL, &walled.keyframes[0]));
    assert!(
        blind.segments_in_solids > 40 && blind.deepest_m > 0.8,
        "the open crown fills the wall's room: {}",
        blind.describe()
    );
    assert!(
        seen.segments_in_solids * 2 < blind.segments_in_solids && seen.deepest_m < 0.6,
        "by the wall: {}; open: {}",
        seen.describe(),
        blind.describe()
    );

    // A neighbour's crown to the east shades it and takes its room: its
    // leaves stand farther west, and fewer of them in the neighbour. A
    // bigleaf maple stands on several stems divided at 1 m (`stem_clump`),
    // which stay where they part, so its leaves move about 0.15 m at 15
    // years where a single stem's moved 0.3 m and more.
    let shaded = maple(NEIGHBOUR, 0.0, 15.0, vec![15.0]);
    let shaded = &shaded.keyframes[0];
    let (west, centre) = (organs_centre(shaded).x, organs_centre(open).x);
    assert!(
        west < centre - 0.1,
        "leaves' centre {west:.2} m, open {centre:.2} m"
    );
    let (inside, open_inside) = (
        intrusion(NEIGHBOUR, shaded).organs_in_crowns,
        intrusion(NEIGHBOUR, open).organs_in_crowns,
    );
    assert!(
        inside * 2 < open_inside,
        "{inside} leaves in the neighbour, {open_inside} open-grown"
    );
}

/// The shape of `graph`: without what it sheds later (two growths that
/// part at an age shed their parts at different ages after it) or the
/// light its organs read (at the age it moves, they read the place's).
fn shape(graph: &PlantGraph) -> PlantGraph {
    let mut graph = graph.clone();
    for segment in &mut graph.segments {
        segment.shed = None;
    }
    for organ in &mut graph.organs {
        organ.shed = None;
        organ.light = 0.0;
    }
    graph
}

#[test]
fn grown_on_from_an_age_it_is_the_library_s_plant_until_then() {
    let open = maple(OPEN, 0.0, 12.0, vec![8.0, 12.0]);
    let moved = maple(NEIGHBOUR, 8.0, 12.0, vec![8.0, 12.0]);
    assert_eq!(shape(&moved.keyframes[0]), shape(&open.keyframes[0]));
    assert_ne!(shape(&moved.keyframes[1]), shape(&open.keyframes[1]));
}
