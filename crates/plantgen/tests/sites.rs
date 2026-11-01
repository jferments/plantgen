//! Plants grown on a mesh someone hands over (sites plan S4): ivy set at
//! the foot of a brick wall in a site's mesh climbs the wall, its stems on
//! the wall's face rather than in it, where with nothing to climb it
//! sprawls low.

use std::sync::Arc;

use plantgen::conditions::Conditions;
use plantgen::graph::PlantGraph;
use plantgen::grow::{GrowthSettings, grow};
use plantgen::lsys::Limits;
use plantgen::lsys::tools::Host;
use plantgen::math::Vec3;
use plantgen::site::{Cut, SiteMesh, climbable};
use plantgen::spec::{Environment, PlantSpec};
use plantgen::substrate::{BUILT, SOIL};

/// A box from `low` to `high`, its faces wound outward.
fn cuboid(low: Vec3, high: Vec3, material: u8, out: &mut (Vec<Vec3>, Vec<[u32; 3]>, Vec<u8>)) {
    let first = u32::try_from(out.0.len()).unwrap();
    for i in 0..8 {
        out.0.push(Vec3::new(
            if i & 1 == 0 { low.x } else { high.x },
            if i & 2 == 0 { low.y } else { high.y },
            if i & 4 == 0 { low.z } else { high.z },
        ));
    }
    for [a, b, c, d] in [
        [0, 4, 6, 2],
        [1, 3, 7, 5],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 2, 3, 1],
        [4, 5, 7, 6],
    ] {
        out.1.push([first + a, first + b, first + c]);
        out.1.push([first + a, first + c, first + d]);
        out.2.extend([material, material]);
    }
}

/// Ground 12 m across, its top at 0, and a brick wall 4 m long, 2.5 m
/// high and 0.3 m thick whose south face stands at z = −0.5.
fn garden() -> SiteMesh {
    let mut parts = (Vec::new(), Vec::new(), Vec::new());
    cuboid(Vec3::new(-6.0, -2.0, -6.0), Vec3::new(6.0, 0.0, 6.0), SOIL, &mut parts);
    cuboid(Vec3::new(-2.0, -0.3, -0.8), Vec3::new(2.0, 2.5, -0.5), BUILT, &mut parts);
    SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap()
}

/// English ivy, its first variant, grown for `years` at `foot` in the
/// site: on the substrate cut round its foot, holding to the site's
/// climbable faces. Its graph and the place it grew in.
fn ivy_at(site: &SiteMesh, foot: Vec3, years: f64) -> (PlantGraph, Conditions) {
    let spec = PlantSpec::builtin("hedera-helix").unwrap();
    let (program, params) = spec.program().unwrap();
    let seed = spec.variants.seeds[0];
    let cut = Cut::for_plant(3.0, 2.0);
    let mut conditions = Conditions::preset(Environment::Open);
    conditions.substrate = Some(site.substrate_round(foot, cut).unwrap());
    let host: Option<Arc<Host>> = site.host_round(foot, cut, &climbable).map(Arc::new);
    let growth = grow(
        &program,
        &spec.seeded_params(&program, &params, seed),
        &GrowthSettings {
            seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions: conditions.clone(),
            limits: Limits::default(),
            host,
        },
    )
    .unwrap();
    (growth.keyframes[0].clone(), conditions)
}

#[test]
fn ivy_climbs_a_wall_in_the_mesh_on_its_face() {
    let site = garden();
    // 0.3 m south of the wall's face.
    let (graph, place) = ivy_at(&site, Vec3::new(0.0, 0.0, -0.2), 6.0);
    assert!(graph.height > 1.5, "ivy reached only {:.2} m", graph.height);
    let intrusion = place.intrusion(&graph).unwrap();
    #[allow(clippy::cast_precision_loss)]
    let share = intrusion.segments_in_solids as f64 / intrusion.segments.max(1) as f64;
    assert!(share < 0.05, "{}", intrusion.describe());
    // Its leaves on the wall's south face: most within 25 cm of it, in
    // the plant's frame at z = −0.3.
    let near = graph
        .organs
        .iter()
        .filter(|organ| organ.position.y > 0.3 && (organ.position.z + 0.3).abs() < 0.25)
        .count();
    let high = graph.organs.iter().filter(|organ| organ.position.y > 0.3).count();
    assert!(near * 2 > high, "{near} of {high} raised leaves by the wall");

    // With nothing it can climb in reach, it stays low.
    let (open, _) = ivy_at(&site, Vec3::new(0.0, 0.0, 4.0), 6.0);
    assert!(open.height < 1.0, "free ivy reached {:.2} m", open.height);
}
