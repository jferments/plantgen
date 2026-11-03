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
        traffic: Vec::new(),
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
        traffic: Vec::new(),
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

// Plants that come up by themselves in a pavement (pavement-cracks plan
// K3): `plantgen::succession` on a paved lot in open ground.

mod succession {
    use plantgen::lsys::tools::SoilNeed;
    use plantgen::pavement::{Crack, CrackTile, Hole, Pavement, TrafficPath};
    use plantgen::site::SiteMesh;
    use plantgen::substrate::SOIL;
    use plantgen::succession::{Colonist, Settings, Size, Snapshot, microsites, run};

    /// A 10 m lot of asphalt in open ground: a hairline crack along z = -2,
    /// a 2 cm crack along z = 2 and a pothole 7 cm deep in soil at (0, 0);
    /// with `cracks_across`, cracks every half metre across a lane along z
    /// = 3.5 instead.
    fn lot(cracks_across: bool) -> SiteMesh {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        super::cuboid(
            plantgen::math::Vec3::new(-12.0, -2.0, -12.0),
            plantgen::math::Vec3::new(12.0, 0.0, 12.0),
            SOIL,
            &mut parts,
        );
        let mut site = SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap();
        let line = |z: f64, width: f64, fill: f64| Crack {
            points_m: vec![[-3.0, z], [3.0, z]],
            width_m: vec![width, width],
            fill_m: vec![fill, fill],
        };
        let (cracks, holes, traffic) = if cracks_across {
            let cracks = (0..13)
                .map(|i| {
                    let x = -3.0 + 0.5 * f64::from(i);
                    Crack {
                        points_m: vec![[x, 1.5], [x, 4.8]],
                        width_m: vec![0.01, 0.01],
                        fill_m: vec![0.05, 0.05],
                    }
                })
                .collect();
            let lane = TrafficPath {
                points_m: vec![[-5.0, 3.5], [5.0, 3.5]],
                spread_m: 0.254,
                load: 1.0,
                track_m: 0.75,
                body_m: 0.9,
            };
            (cracks, Vec::new(), vec![lane])
        } else {
            let outline: Vec<[f64; 2]> = (0..12)
                .map(|k| {
                    let t = std::f64::consts::TAU * f64::from(k) / 12.0;
                    [0.35 * t.cos(), 0.35 * t.sin()]
                })
                .collect();
            let hole = Hole {
                outline_m: outline,
                depth_m: 0.12,
                fill_m: 0.07,
            };
            (
                vec![line(-2.0, 0.002, 0.01), line(2.0, 0.02, 0.06)],
                vec![hole],
                Vec::new(),
            )
        };
        let tile = CrackTile {
            version: 1,
            origin_m: [-5.0, -5.0],
            size_m: [10.0, 10.0],
            top_m: 0.0,
            thickness_m: 0.08,
            base_m: 0.25,
            base_rootable: None,
            subgrade_m: None,
            subgrade_rootable: None,
            surface: "asphalt".into(),
            cell_m: None,
            cracks,
            holes,
            traffic,
            made_by: None,
        };
        site.add_pavement(Pavement::new(&tile).unwrap());
        site
    }

    /// A species for the yearly step: wind-borne seed, a short-lived seed
    /// bank, roots as `soil@1`'s defaults ask.
    fn colonist(
        id: &str,
        life: &str,
        woody: bool,
        trampling: &str,
        sizes: &[(f64, f64, f64)],
    ) -> Colonist {
        let mut traits = serde_json::Map::new();
        traits.insert("life_span".into(), life.into());
        traits.insert("seed_dispersal".into(), serde_json::json!(["wind"]));
        traits.insert("seedbank".into(), "short_term".into());
        traits.insert("trampling_tolerance".into(), trampling.into());
        let soil = SoilNeed {
            depth: if woody { 3.64 } else { 1.0 },
            spread: 2.25,
            demand: 0.6,
            per_height: if woody { 1.0 } else { 1.53 },
        };
        let sizes = sizes
            .iter()
            .map(|&(age, height, crown)| Size { age, height, crown })
            .collect();
        Colonist::new(id, &traits, woody, Some(soil), sizes)
    }

    fn pool() -> Vec<Colonist> {
        vec![
            colonist("grass", "annual", false, "tolerant", &[(1.0, 0.15, 0.08)]),
            colonist(
                "herb",
                "perennial",
                false,
                "intermediate",
                &[(1.0, 0.1, 0.08), (3.0, 0.35, 0.15)],
            ),
            colonist(
                "tree",
                "perennial",
                true,
                "intolerant",
                &[
                    (1.0, 0.6, 0.2),
                    (5.0, 3.0, 1.2),
                    (10.0, 7.0, 2.5),
                    (20.0, 14.0, 4.0),
                ],
            ),
        ]
    }

    fn settings(seed: u64, traffic: f64) -> Settings {
        Settings {
            seed,
            traffic,
            maintenance: 0.0,
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_plants() {
        let site = lot(false);
        let places = microsites(&site, 2);
        let pool = pool();
        let years = [0, 5, 10];
        let once = run(&places, &pool, &[], &settings(3, 0.0), &years);
        let again = run(&places, &pool, &[], &settings(3, 0.0), &years);
        assert_eq!(once, again);
        assert_eq!(once.len(), 3);
        assert!(once[0].plants.is_empty(), "year 0 is bare pavement");
        assert!(!once[2].plants.is_empty(), "nothing came up in 10 years");
        let other = run(&places, &pool, &[], &settings(4, 0.0), &years);
        assert_ne!(once, other, "another seed gives another site");
    }

    /// Plants of `colonist` (any, if `None`) at least `min_age` years old
    /// in the microsites `which` accepts, per microsite and run.
    fn density(
        places: &[plantgen::succession::Microsite],
        runs: &[Snapshot],
        colonist: Option<usize>,
        min_age: f64,
        which: impl Fn(&plantgen::succession::Microsite) -> bool,
    ) -> f64 {
        let sites = places.iter().filter(|p| which(p)).count().max(1);
        let plants: usize = runs
            .iter()
            .map(|snapshot| {
                snapshot
                    .plants
                    .iter()
                    .filter(|p| colonist.is_none_or(|c| p.colonist == c) && p.age >= min_age)
                    .filter(|p| which(&places[p.microsite]))
                    .count()
            })
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let per = plants as f64 / (sites * runs.len()) as f64;
        per
    }

    #[test]
    fn trees_take_hold_in_a_pothole_not_in_a_hairline_crack() {
        // Plan K3: a pothole with 7 cm of soil grows a young tree in ten
        // years; a hairline crack's soil carries none past its first year.
        // Over a compacted subgrade the pothole's tree stays a sapling
        // (soil depth design D1).
        let site = lot(false);
        let places = microsites(&site, 2);
        let pool = pool();
        // A tree of its kind across the lot seeds it.
        let parent = plantgen::succession::Source {
            colonist: 2,
            x: 0.0,
            z: 9.0,
            age: 20.0,
        };
        let runs: Vec<Snapshot> = (1..=6)
            .flat_map(|seed| run(&places, &pool, &[parent], &settings(seed, 0.0), &[10]))
            .collect();
        let hole = |p: &plantgen::succession::Microsite| p.x.hypot(p.z) < 0.4;
        let hairline = |p: &plantgen::succession::Microsite| (p.z + 2.0).abs() < 0.1;
        let trees_in_hole: Vec<_> = runs
            .iter()
            .map(|s| {
                s.plants
                    .iter()
                    .filter(|p| p.colonist == 2 && p.age >= 2.0 && hole(&places[p.microsite]))
                    .collect::<Vec<_>>()
            })
            .collect();
        let with_tree = trees_in_hole
            .iter()
            .filter(|trees| !trees.is_empty())
            .count();
        assert!(
            with_tree >= 3,
            "a tree in the pothole at year 10 in {with_tree} runs of 6"
        );
        for tree in trees_in_hole.iter().flatten() {
            let (open_ground, _) = pool[2].size_at(tree.age);
            assert!(
                tree.height < open_ground && tree.rs < 1.0,
                "a {}-year-old tree in the pothole is {:.2} m tall, {open_ground:.2} m in open ground, rs {:.2}",
                tree.age,
                tree.height,
                tree.rs
            );
        }
        let in_hairline = density(&places, &runs, Some(2), 2.0, hairline);
        assert!(
            in_hairline < 0.01,
            "trees in a hairline crack: {in_hairline:.3} a microsite"
        );
    }

    #[test]
    fn a_busy_lane_stays_barer_under_its_wheels_than_at_its_edges() {
        let site = lot(true);
        let places = microsites(&site, 2);
        let pool = pool();
        let runs: Vec<Snapshot> = (1..=4)
            .flat_map(|seed| run(&places, &pool, &[], &settings(seed, 1.0), &[10]))
            .collect();
        let wheels = |p: &plantgen::succession::Microsite| p.wear > 0.8;
        let edges = |p: &plantgen::succession::Microsite| p.wear_tall < 0.1;
        let under = density(&places, &runs, None, 1.0, wheels);
        let beside = density(&places, &runs, None, 1.0, edges);
        assert!(
            under < 0.5 * beside,
            "plants a microsite under the wheels {under:.3}, beside the lane {beside:.3}"
        );
        // Nothing taller than a car's clearance lasts anywhere in the lane.
        let in_lane = |p: &plantgen::succession::Microsite| p.wear_tall > 0.8;
        let trees = density(&places, &runs, Some(2), 2.0, in_lane);
        assert!(
            trees < 1.0e-9,
            "trees past their first year in the lane: {trees:.3}"
        );
    }
}

mod field {
    use super::{cuboid, garden, paved_lot, wire_fence};
    use plantgen::api::{self, FieldSettings, HostKind, SiteField, SiteInput, Surface};
    use plantgen::math::Vec3;
    use plantgen::site::SiteMesh;
    use plantgen::site_field::{build, heat_load_index};
    use plantgen::substrate::{ROCK, SOIL, WOOD};

    fn settings() -> FieldSettings {
        FieldSettings {
            cell_m: 0.5,
            margin_m: 0.0,
            sky_rays: 32,
            ..FieldSettings::default()
        }
    }

    fn field_of(site: &SiteMesh) -> SiteField {
        build(site, [-6.0, -6.0], [6.0, 6.0], &settings()).unwrap()
    }

    fn cell(field: &SiteField, x: f64, z: f64) -> usize {
        field.cell_at(x, z).unwrap() as usize
    }

    #[test]
    fn a_wall_is_four_upright_hosts_and_the_ground_beside_it_knows_it() {
        let field = field_of(&garden());
        let walls = field
            .hosts
            .iter()
            .filter(|h| h.kind == HostKind::Wall)
            .count();
        assert_eq!(walls, 4, "{:?}", field.hosts);
        assert!(field.hosts.iter().any(|h| h.kind == HostKind::Roof));
        let g = &field.ground;
        // Inside the wall's footprint nothing stands; beside it, the
        // wall is near and half the sky is gone.
        assert!(g.height[cell(&field, 0.0, -0.65)].is_none());
        let beside = cell(&field, 0.0, -0.25);
        let open = cell(&field, 0.0, 4.75);
        assert!(g.wall_m[beside] < 0.5, "{}", g.wall_m[beside]);
        assert!(g.wall_m[open] > 4.0);
        assert!(
            g.sky[beside] < g.sky[open] - 0.1,
            "{} {}",
            g.sky[beside],
            g.sky[open]
        );
        // The soil runs 2 m down to the ground block's floor.
        assert!((g.rooting_m[open].unwrap() - 2.0).abs() < 1.0e-6);
        // Every patch on the wall's south face roots in the ground south
        // of it, and none lies below the ground.
        for patch in &field.patches {
            assert!(patch.centre[1] > -0.03, "{patch:?}");
            if patch.normal[2] > 0.9 && patch.centre[1] < 2.4 {
                let root = patch.root_cell.expect("a root cell");
                assert!(field.cell_centre(root)[1] > -0.5);
                assert!(patch.climbable);
            }
        }
    }

    #[test]
    fn posts_and_wires_make_a_fence_in_bays_with_u_running_on() {
        let field = field_of(&wire_fence());
        let fences: Vec<_> = field
            .hosts
            .iter()
            .filter(|h| h.kind == HostKind::Fence)
            .collect();
        assert_eq!(fences.len(), 2, "{:?}", field.hosts);
        let us: Vec<f64> = field
            .patches
            .iter()
            .filter(|p| p.host == fences[1].id)
            .map(|p| p.u)
            .collect();
        // The second bay's patches carry on from 2.5 m along the fence.
        assert!(us.iter().all(|u| *u >= 2.25), "{us:?}");
        assert!(field.patches.iter().any(|p| p.thin_m > 0.0));
    }

    #[test]
    fn a_pole_and_a_sunk_boulder_are_found_and_patched_round() {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-6.0, -2.0, -6.0),
            Vec3::new(6.0, 0.0, 6.0),
            SOIL,
            &mut parts,
        );
        cuboid(
            Vec3::new(2.85, -0.5, -0.15),
            Vec3::new(3.15, 8.0, 0.15),
            WOOD,
            &mut parts,
        );
        cuboid(
            Vec3::new(-3.0, -0.4, -1.0),
            Vec3::new(-1.0, 0.6, 1.0),
            ROCK,
            &mut parts,
        );
        let site = SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap();
        let field = field_of(&site);
        let pole = field
            .hosts
            .iter()
            .find(|h| h.kind == HostKind::Pole)
            .unwrap();
        let rock = field
            .hosts
            .iter()
            .find(|h| h.kind == HostKind::Boulder)
            .unwrap();
        let round: Vec<_> = field.patches.iter().filter(|p| p.host == pole.id).collect();
        // Four sides of the pole, each patch facing out, none at its axis.
        for p in round.iter().filter(|p| p.normal[1].abs() < 0.5) {
            let out = (p.centre[0] - 3.0).hypot(p.centre[2]);
            assert!(out > 0.1, "{p:?}");
            assert!(p.sky > 0.2, "{p:?}");
        }
        assert!(round.iter().any(|p| p.height_m > 7.0));
        // The ground under the boulder is not ground.
        assert!(field.ground.height[cell(&field, -2.0, 0.0)].is_none());
        assert!(
            field
                .patches
                .iter()
                .any(|p| p.host == rock.id && p.normal[1] > 0.9)
        );
    }

    #[test]
    fn cracks_belong_to_their_cells_and_paved_cells_root_only_in_them() {
        let field = field_of(&paved_lot());
        assert!(!field.cracks.is_empty());
        for (index, site) in field.cracks.iter().enumerate() {
            let cell = site.cell.unwrap() as usize;
            assert!(field.ground.cracks[cell].contains(&u32::try_from(index).unwrap()));
        }
        let g = &field.ground;
        let crack = cell(&field, 0.0, 2.0);
        let slab = cell(&field, 0.0, -1.0);
        assert!(g.rooting_m[crack].unwrap() > 0.03);
        assert!(g.rooting_m[slab].unwrap() < 1.0e-9);
        assert!(g.open_share[crack] > 0.0 && g.open_share[crack] < 0.2);
    }

    #[test]
    fn a_south_west_slope_is_warmer_than_a_north_east_one() {
        let level = heat_load_index(45.0, 0.0, 0.0);
        assert!((level - 0.706).abs() < 0.01, "{level}");
        assert!(heat_load_index(45.0, 30.0, 225.0) > level);
        assert!(heat_load_index(45.0, 30.0, 45.0) < level);
        // South of the equator it turns round.
        assert!(heat_load_index(-45.0, 30.0, 315.0) > heat_load_index(-45.0, 30.0, 135.0));
    }

    #[test]
    fn a_host_builds_the_same_field_from_plain_arrays_and_reads_it_back() {
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
            plantgen::substrate::BUILT,
            &mut parts,
        );
        let positions: Vec<[f64; 3]> = parts.0.iter().map(|p| [p.x, p.y, p.z]).collect();
        let table = [
            Surface::solid(api::site_material("soil").unwrap()),
            Surface::solid(api::site_material("built").unwrap()),
        ];
        let materials: Vec<u32> = parts.2.iter().map(|m| u32::from(*m != SOIL)).collect();
        let input = SiteInput {
            positions: &positions,
            triangles: &parts.1,
            materials: &materials,
            table: &table,
            pavements: &[],
            z_up: false,
        };
        let from_api = api::site_field(&input, [-6.0, -6.0], [6.0, 6.0], &settings()).unwrap();
        assert_eq!(from_api, field_of(&garden()));
        let text = from_api.to_json().unwrap();
        assert_eq!(SiteField::from_json(&text).unwrap(), from_api);
        assert_eq!(from_api.version, api::SITE_FIELD_VERSION);
        // A table too short is refused, not read past.
        let short = SiteInput {
            table: &table[..1],
            ..input
        };
        assert!(api::site_field(&short, [-6.0, -6.0], [6.0, 6.0], &settings()).is_err());
    }
}
