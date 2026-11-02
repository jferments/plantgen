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
use plantgen::site::{Cut, SiteMesh, Surface, climbable};
use plantgen::spec::{Environment, PlantSpec};
use plantgen::substrate::{BUILT, SOIL, WOOD};

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
    cuboid(
        Vec3::new(-6.0, -2.0, -6.0),
        Vec3::new(6.0, 0.0, 6.0),
        SOIL,
        &mut parts,
    );
    cuboid(
        Vec3::new(-2.0, -0.3, -0.8),
        Vec3::new(2.0, 2.5, -0.5),
        BUILT,
        &mut parts,
    );
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
    let high = graph
        .organs
        .iter()
        .filter(|organ| organ.position.y > 0.3)
        .count();
    assert!(
        near * 2 > high,
        "{near} of {high} raised leaves by the wall"
    );

    // With nothing it can climb in reach, it stays low.
    let (open, _) = ivy_at(&site, Vec3::new(0.0, 0.0, 4.0), 6.0);
    assert!(open.height < 1.0, "free ivy reached {:.2} m", open.height);
}

/// The garden's ground and wall, with a window 1 m wide and 1.2 m high
/// on the wall's south face, 1 cm proud of it: a thin pane, kept clear
/// when `clear`.
fn wall_with_window(clear: bool) -> SiteMesh {
    let mut parts = (Vec::new(), Vec::new(), Vec::new());
    cuboid(
        Vec3::new(-6.0, -2.0, -6.0),
        Vec3::new(6.0, 0.0, 6.0),
        SOIL,
        &mut parts,
    );
    cuboid(
        Vec3::new(-2.0, -0.3, -0.8),
        Vec3::new(2.0, 2.5, -0.5),
        BUILT,
        &mut parts,
    );
    let mut surfaces: Vec<Surface> = parts.2.iter().map(|m| Surface::solid(*m)).collect();
    let first = u32::try_from(parts.0.len()).unwrap();
    parts.0.extend([
        Vec3::new(-0.3, 0.6, -0.49),
        Vec3::new(0.7, 0.6, -0.49),
        Vec3::new(0.7, 1.8, -0.49),
        Vec3::new(-0.3, 1.8, -0.49),
    ]);
    parts
        .1
        .extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
    let glass = Surface {
        thin_m: 0.02,
        clear,
        ..Surface::solid(BUILT)
    };
    surfaces.extend([glass, glass]);
    SiteMesh::with_surfaces(&parts.0, &parts.1, &surfaces).unwrap()
}

#[test]
fn ivy_grows_round_a_window_kept_clear() {
    let site = wall_with_window(true);
    let foot = Vec3::new(0.2, 0.0, -0.2);
    let (graph, _) = ivy_at(&site, foot, 6.0);
    assert!(graph.height > 1.5, "ivy reached only {:.2} m", graph.height);
    // In the plant's frame the pane spans x -0.5 to 0.5, y 0.6 to 1.8, at
    // z = -0.29. Its tips were cut back each year as they reached it, so
    // far fewer stems end on it than on a pane left to grow over, even
    // before what overhangs it is trimmed.
    let on_pane = |p: Vec3| {
        (-0.5..0.5).contains(&p.x) && (0.6..1.8).contains(&p.y) && (p.z + 0.29).abs() < 0.12
    };
    let on = graph.segments.iter().filter(|s| on_pane(s.end)).count();
    let (left, _) = ivy_at(&wall_with_window(false), foot, 6.0);
    let on_left = left.segments.iter().filter(|s| on_pane(s.end)).count();
    assert!(
        on * 3 < on_left,
        "{on} stems end on the pane kept clear, {on_left} on one left"
    );
    // Trimmed, nothing is left on it.
    let mut trimmed = graph.clone();
    site.cut_back(&mut trimmed, foot, 0.05);
    assert!(trimmed.segments.len() > graph.segments.len() / 2);
    assert!(
        trimmed
            .organs
            .iter()
            .all(|organ| !(on_pane(organ.position) && (organ.position.z + 0.29).abs() < 0.05))
    );
}

/// Ground, with a post-and-wire fence along z = 0 from x = -2 to 2: posts
/// 1.4 m high every 2 m and four wires (thin parts 4 mm thick).
fn wire_fence() -> SiteMesh {
    let mut parts = (Vec::new(), Vec::new(), Vec::new());
    cuboid(
        Vec3::new(-6.0, -2.0, -6.0),
        Vec3::new(6.0, 0.0, 6.0),
        SOIL,
        &mut parts,
    );
    for x in [-2.0, 0.0, 2.0] {
        cuboid(
            Vec3::new(x - 0.06, 0.0, -0.06),
            Vec3::new(x + 0.06, 1.4, 0.06),
            WOOD,
            &mut parts,
        );
    }
    let mut surfaces: Vec<Surface> = parts.2.iter().map(|m| Surface::solid(*m)).collect();
    let wire = Surface {
        thin_m: 0.004,
        ..Surface::solid(BUILT)
    };
    for y in [0.3, 0.6, 0.9, 1.2] {
        let first = u32::try_from(parts.0.len()).unwrap();
        parts.0.extend([
            Vec3::new(-2.0, y - 0.005, 0.065),
            Vec3::new(2.0, y - 0.005, 0.065),
            Vec3::new(2.0, y + 0.005, 0.065),
            Vec3::new(-2.0, y + 0.005, 0.065),
        ]);
        parts
            .1
            .extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
        surfaces.extend([wire, wire]);
    }
    SiteMesh::with_surfaces(&parts.0, &parts.1, &surfaces).unwrap()
}

#[test]
fn a_vine_climbs_a_wire_fence() {
    let site = wire_fence();
    // A metre from the nearest post, 15 cm in front of the wires.
    let foot = Vec3::new(1.0, 0.0, 0.2);
    let spec = PlantSpec::builtin("lonicera-ciliosa").unwrap();
    let (program, params) = spec.program().unwrap();
    let seed = spec.variants.seeds[0];
    let cut = Cut::for_plant(2.0, 1.5);
    let mut conditions = Conditions::preset(Environment::Open);
    conditions.substrate = Some(site.substrate_round(foot, cut).unwrap());
    let host = site.host_round(foot, cut, &climbable).map(Arc::new);
    assert!(host.is_some());
    let growth = grow(
        &program,
        &spec.seeded_params(&program, &params, seed),
        &GrowthSettings {
            seed,
            dt: spec.growth.step,
            years: 5.0,
            keyframes: vec![5.0],
            conditions,
            limits: Limits::default(),
            host,
        },
    )
    .unwrap();
    let graph = &growth.keyframes[0];
    // It climbs the wires: up past the lowest two, its raised leaves
    // mostly within 30 cm of the fence's plane (z = -0.2 in its frame).
    assert!(graph.height > 0.9, "reached only {:.2} m", graph.height);
    let raised: Vec<_> = graph.organs.iter().filter(|o| o.position.y > 0.4).collect();
    let near = raised
        .iter()
        .filter(|o| (o.position.z + 0.2).abs() < 0.3)
        .count();
    assert!(
        near * 2 > raised.len(),
        "{near} of {} raised leaves by the fence",
        raised.len()
    );
}

/// A slab of asphalt 8 cm thick laid over the garden's ground from
/// x = −1 to 1 and z = 1 to 3, with one crack 1 cm wide along z = 2 and
/// 4 cm of grit in it.
fn paved_garden() -> SiteMesh {
    use plantgen::pavement::{Crack, CrackTile, Pavement};
    let mut site = garden();
    let tile = CrackTile {
        version: 1,
        origin_m: [-1.0, 1.0],
        size_m: [2.0, 2.0],
        top_m: 0.0,
        thickness_m: 0.08,
        base_m: 0.2,
        base_rootable: None,
        subgrade_m: None,
        subgrade_rootable: None,
        surface: "asphalt".into(),
        cell_m: None,
        cracks: vec![Crack {
            points_m: vec![[-0.9, 2.0], [0.9, 2.0]],
            width_m: vec![0.01, 0.01],
            fill_m: vec![0.04, 0.04],
        }],
        holes: Vec::new(),
        made_by: None,
    };
    site.add_pavement(Pavement::new(&tile).unwrap());
    site
}

#[test]
fn a_grass_comes_up_through_a_crack_and_nothing_roots_on_sound_slab() {
    let site = paved_garden();
    // In the crack: on its grit, 4 cm under the surface.
    let (y, material) = site.footing(0.0, 2.003, None).unwrap();
    assert_eq!(material, SOIL);
    assert!((y + 0.04).abs() < 1.0e-3, "{y}");
    // On sound slab: the slab, where nothing roots; off the tile, the soil.
    assert_eq!(site.footing(0.0, 1.5, None), Some((0.0, BUILT)));
    assert_eq!(site.footing(0.0, 4.0, None).map(|(_, m)| m), Some(SOIL));
    let spec = PlantSpec::builtin("dactylis-glomerata").unwrap();
    let (program, params) = spec.program().unwrap();
    let seed = spec.variants.seeds[0];
    let foot = Vec3::new(0.0, y, 2.003);
    let cut = Cut::for_plant(1.0, 0.5);
    let mut conditions = Conditions::preset(Environment::Open);
    conditions.substrate = Some(site.substrate_round(foot, cut).unwrap());
    let growth = grow(
        &program,
        &spec.seeded_params(&program, &params, seed),
        &GrowthSettings {
            seed,
            dt: spec.growth.step,
            years: 2.0,
            keyframes: vec![2.0],
            conditions: conditions.clone(),
            limits: Limits::default(),
            host: None,
        },
    )
    .unwrap();
    let graph = &growth.keyframes[0];
    assert!(
        graph.height > 0.3,
        "the grass reached {:.2} m",
        graph.height
    );
    // Its stems come up through the crack, not through the slab. The
    // tuft's blades spring from a crown wider than a 1 cm crack, so their
    // bases lie in the slab, but never deeper than the crack's grit (a
    // tuft held to its crack's line waits on rooting space, plan K4).
    let intrusion = conditions.intrusion(graph).unwrap();
    assert_eq!(intrusion.segments_in_solids, 0, "{}", intrusion.describe());
    assert!(intrusion.deepest_m <= 0.045, "{}", intrusion.describe());
}

/// The garden's ground paved 10 m across, a crack along z = 2.
fn paved_lot() -> SiteMesh {
    paved_lot_on(None, None)
}

/// The same, on a subgrade that deep and that rootable.
fn paved_lot_on(subgrade_m: Option<f64>, subgrade_rootable: Option<f64>) -> SiteMesh {
    use plantgen::pavement::{Crack, CrackTile, Pavement};
    let mut site = garden();
    let tile = CrackTile {
        version: 1,
        origin_m: [-5.0, -3.0],
        size_m: [10.0, 8.0],
        top_m: 0.0,
        thickness_m: 0.08,
        base_m: 0.2,
        base_rootable: None,
        subgrade_m,
        subgrade_rootable,
        surface: "asphalt".into(),
        cell_m: None,
        cracks: vec![Crack {
            points_m: vec![[-4.0, 2.0], [4.0, 2.0]],
            width_m: vec![0.01, 0.01],
            fill_m: vec![0.04, 0.04],
        }],
        holes: Vec::new(),
        made_by: None,
    };
    site.add_pavement(Pavement::new(&tile).unwrap());
    site
}

/// Grows `species` at `foot` on `site` for `years`, its roots held to the
/// soil the site gives them when `rooted`, and gives its height and its
/// organs' count.
fn grown_at(site: &SiteMesh, species: &str, foot: Vec3, years: f64, rooted: bool) -> (f64, usize) {
    let spec = PlantSpec::builtin(species).unwrap();
    let (program, params) = spec.program().unwrap();
    let seed = spec.variants.seeds[0];
    let cut = Cut::for_plant(2.0, 1.0);
    let mut conditions = Conditions::preset(Environment::Open);
    conditions.substrate = Some(site.substrate_round(foot, cut).unwrap());
    if rooted {
        conditions.rooting = site.rooting(foot);
        assert!(conditions.rooting.is_some(), "no rooting table at {foot:?}");
    }
    let growth = grow(
        &program,
        &spec.seeded_params(&program, &params, seed),
        &GrowthSettings {
            seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions,
            limits: Limits::default(),
            host: None,
        },
    )
    .unwrap();
    let graph = &growth.keyframes[0];
    (graph.height, graph.organs.len())
}

#[test]
fn plants_rooted_in_a_crack_stay_smaller_than_in_open_ground() {
    // Soil depth design D1: a crack holds a few litres of grit, and under
    // it the base and the compacted subgrade let roots use little of their
    // volume, so a plant in it grows as one short of water does.
    let site = paved_lot();
    let (y, _) = site.footing(0.0, 2.003, None).unwrap();
    let crack = Vec3::new(0.0, y, 2.003);
    let rooting = site.rooting(crack).unwrap();
    // The crack's grit and the base: far less than open ground's.
    let near = rooting.volume(0.2, 0.4);
    let open = std::f64::consts::PI * 0.2 * 0.2 * 0.4;
    assert!(near < 0.25 * open, "{near} m³ against {open}");
    assert!(site.rooting(Vec3::new(0.0, 0.0, 5.5)).is_none());
    // A grass in the street's crack; a tree in one in a lot laid on deep
    // compacted fill, through which its roots reach little.
    let lot = paved_lot_on(Some(2.0), Some(0.01));
    for (species, years, site) in [
        ("dactylis-glomerata", 3.0, &site),
        ("alnus-rubra", 4.0, &lot),
    ] {
        let (free, free_organs) = grown_at(site, species, crack, years, false);
        let (held, held_organs) = grown_at(site, species, crack, years, true);
        eprintln!(
            "{species}: {free:.2} m, {free_organs} organs free; {held:.2} m, {held_organs} organs in its crack"
        );
        assert!(
            held < 0.8 * free,
            "{species}: {held:.2} m in the crack, {free:.2} m free"
        );
        assert!(held_organs < free_organs, "{species}");
    }
}
