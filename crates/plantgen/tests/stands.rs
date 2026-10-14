//! Stands (fitting plan PF5): plants grown together make room for one
//! another, a tile that wraps crowds its plants as its neighbours would,
//! and a stand grows the same whatever order its plants are listed in.

use plantgen::conditions::Conditions;
use plantgen::graph::PlantGraph;
use plantgen::grow::{Growth, GrowthSettings, grow};
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

/// Maples at `feet` (east, north), grown together.
fn stand(maple: &Maple, feet: &[[f64; 2]], tile_m: Option<[f64; 2]>) -> Vec<Growth> {
    let members: Vec<Member<'_>> = feet
        .iter()
        .map(|at_m| Member {
            program: &maple.program,
            params: &maple.params,
            settings: &maple.settings,
            at_m: *at_m,
        })
        .collect();
    grow_stand(&members, tile_m).unwrap()
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

#[test]
fn two_trees_grown_together_make_room_for_each_other() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    let alone = &alone.keyframes[0];
    // Two maples 5 m apart, east and west.
    let pair = stand(&maple, &[[0.0, 0.0], [5.0, 0.0]], None);
    let (west, east) = (&pair[0].keyframes[0], &pair[1].keyframes[0]);
    let centre = organs_centre(alone)[0];
    // Each leans its crown away from the other, as far as the other does
    // (they are the same seed, mirrored), and holds fewer leaves.
    let (west_lean, east_lean) = (
        organs_centre(west)[0] - centre,
        organs_centre(east)[0] - centre,
    );
    assert!(
        west_lean < -0.2 && east_lean > 0.2,
        "the west tree leans {west_lean:.2} m east, the east one {east_lean:.2} m"
    );
    assert!(west.organs.len() < alone.organs.len());
    assert!(east.organs.len() < alone.organs.len());
}

#[test]
fn a_tile_crowds_its_tree_and_grows_alike_however_listed() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    // One maple in a 6 m tile that wraps: its own crown, one tile over on
    // every side, stands 6 m off.
    let crowded = stand(&maple, &[[3.0, 3.0]], Some([6.0, 6.0]));
    let (alone, crowded) = (&alone.keyframes[0], &crowded[0].keyframes[0]);
    assert!(
        crowded.organs.len() * 4 < alone.organs.len() * 3,
        "{} leaves in the tile, {} alone",
        crowded.organs.len(),
        alone.organs.len()
    );
    // Listed in either order, three plants grow alike.
    let feet = [[1.0, 1.0], [5.0, 2.0], [3.0, 5.0]];
    let forward = stand(&maple, &feet, Some([7.0, 7.0]));
    let mut backward = stand(&maple, &[feet[2], feet[1], feet[0]], Some([7.0, 7.0]));
    backward.reverse();
    for (one, other) in forward.iter().zip(&backward) {
        assert_eq!(one.digest(), other.digest());
    }
}

#[test]
fn a_stand_of_one_grows_the_plant_grown_alone() {
    let maple = maple();
    let alone = grow(&maple.program, &maple.params, &maple.settings).unwrap();
    let one = stand(&maple, &[[0.0, 0.0]], None);
    assert_eq!(one[0].digest(), alone.digest());
}
