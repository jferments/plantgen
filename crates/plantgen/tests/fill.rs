//! The external contract of a stands fill (`plantgen::api::Fill`): plans
//! that are the same however they are made, tiles that meet, stands that
//! develop as their age says, and the file forms callers read.

use std::sync::LazyLock;

use plantgen::api::{
    Community, Environment, Fill, FillSettings, Generator, INSTANCE_BYTES, INSTANCE_SCHEMA,
    Instance, Options, PrototypeKey, Site, StandCell, StandMap, TileId,
};
use plantgen::package::{self, Inputs};
use plantgen::quality;

fn forest() -> Community {
    Community::from_json(
        r#"{"id": "test-fir-forest", "members": [
            {"species": "pseudotsuga-menziesii", "layer": "canopy"},
            {"species": "achillea-millefolium", "layer": "herb", "abundance": 0.7}
        ]}"#,
    )
    .expect("community")
}

fn meadow() -> Community {
    Community::from_json(
        r#"{"id": "test-meadow", "members": [
            {"species": "achillea-millefolium", "layer": "herb", "environments": ["open"], "abundance": 1.6}
        ]}"#,
    )
    .expect("community")
}

/// Two young stand ages, so the fill grows nothing older than 30 years:
/// one before the firs' crowns close (Douglas-fir's 500-year lifespan
/// closes them at 35) and one after.
fn settings() -> FillSettings {
    let mut settings = FillSettings::default();
    settings.stand_ages = vec![20.0, 40.0];
    settings
}

/// The forest (community 0) and the meadow (1), measured once for every
/// test.
static FILL: LazyLock<Fill> = LazyLock::new(|| {
    let fill = Fill::new(&[forest(), meadow()], &settings()).expect("fill");
    fill.measure(&[], 8, None).expect("measured");
    fill
});

fn fill() -> &'static Fill {
    &FILL
}

#[test]
fn a_tile_is_the_same_plan_whichever_thread_fills_it_and_when() {
    let fill = fill();
    let site = Site::flat(TileId { x: 3, y: -2 }, 80.0).expect("site");
    let stands = StandMap::uniform(80.0, 0, 120.0);
    let first = fill.tile(&site, &stands, 7).expect("tile");
    let again = std::thread::scope(|scope| {
        scope
            .spawn(|| fill.tile(&site, &stands, 7).expect("tile"))
            .join()
            .expect("thread")
    });
    assert_eq!(first, again);
    assert!(!first.instances.is_empty());
    let other = fill.tile(&site, &stands, 8).expect("tile");
    assert_ne!(
        first.instances, other.instances,
        "the seed chooses the plan"
    );
}

#[test]
fn a_tile_filled_whole_or_in_quarters_places_the_same_plants() {
    // Draws are keyed by a stand cell's place in the world, so an 80 m
    // tile of four 40 m cells places in each cell what a 40 m tile there
    // does.
    let fill = fill();
    let whole = fill
        .tile(
            &Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site"),
            &StandMap::uniform(80.0, 0, 120.0),
            7,
        )
        .expect("tile");
    let quarter = fill
        .tile(
            &Site::flat(TileId { x: 1, y: 1 }, 40.0).expect("site"),
            &StandMap::uniform(40.0, 0, 120.0),
            7,
        )
        .expect("tile");
    let in_corner: Vec<Instance> = whole
        .instances
        .iter()
        .filter(|i| i.position[0] >= 40.0 && i.position[1] >= 40.0)
        .map(|i| Instance {
            position: [i.position[0] - 40.0, i.position[1] - 40.0, i.position[2]],
            ..*i
        })
        .collect();
    assert_eq!(in_corner.len(), quarter.instances.len());
    for (a, b) in in_corner.iter().zip(&quarter.instances) {
        assert_eq!(a.prototype, b.prototype);
        for axis in 0..3 {
            assert!((a.position[axis] - b.position[axis]).abs() < 1e-3);
        }
    }
}

#[test]
fn a_stand_map_finer_than_a_stand_tile_reads_windows_of_one() {
    // A city's map changes community every few metres (a yard, a verge, a
    // bed), finer than a crown. Its cells read windows of one stand tile
    // laid out for their block of about 40 m, so a 40 m tile mapped in
    // 4 m cells of one stand places the very plants one 40 m cell does.
    let fill = fill();
    let site = Site::flat(TileId { x: 2, y: 5 }, 40.0).expect("site");
    let fine = |bare_column: Option<usize>| StandMap {
        cell_m: 4.0,
        cols: 10,
        cells: (0..100)
            .map(|at| StandCell {
                community: if Some(at % 10) == bare_column {
                    None
                } else {
                    Some(0)
                },
                years_since_disturbance: 120.0,
                moisture: None,
            })
            .collect(),
    };
    let places = |plan: &plantgen::api::TilePlan| {
        let mut at: Vec<(u32, i64, i64)> = plan
            .instances
            .iter()
            .map(|i| {
                (
                    i.prototype,
                    (f64::from(i.position[0]) * 1000.0).round() as i64,
                    (f64::from(i.position[1]) * 1000.0).round() as i64,
                )
            })
            .collect();
        at.sort_unstable();
        at
    };
    let whole = fill
        .tile(&site, &StandMap::uniform(40.0, 0, 120.0), 7)
        .expect("tile");
    let cells = fill.tile(&site, &fine(None), 7).expect("tile");
    assert!(whole.instances.len() > 10);
    assert_eq!(places(&whole), places(&cells));
    // A bare strip 4 m wide (a road through the stand) takes out the
    // plants whose stems stand in it and moves none of the rest; the
    // trees beside it face it as edges, standing where they stood.
    let road = fill.tile(&site, &fine(Some(4)), 7).expect("tile");
    let spot = |at: &(u32, i64, i64)| (at.1, at.2);
    let mut kept: Vec<(i64, i64)> = places(&cells)
        .iter()
        .filter(|at| !(16_000..20_000).contains(&at.1))
        .map(spot)
        .collect();
    let mut beside: Vec<(i64, i64)> = places(&road).iter().map(spot).collect();
    kept.sort_unstable();
    beside.sort_unstable();
    assert!(kept.len() < cells.instances.len());
    assert_eq!(kept, beside);
}

#[test]
fn a_cells_own_moisture_sets_which_species_suit_it() {
    let fill = fill();
    let site = Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site");
    let mut stands = StandMap::uniform(80.0, 0, 40.0);
    // The south-west cell is a saturated bank, past Douglas-fir's niche
    // (gone by 0.8); the rest keep the site's middling moisture.
    stands.cells[0].moisture = Some(0.95);
    let plan = fill.tile(&site, &stands, 1).expect("tile");
    let firs_in = |west: bool, south: bool| {
        plan.instances
            .iter()
            .filter(|i| (i.position[0] < 40.0) == west && (i.position[1] < 40.0) == south)
            .filter(|i| {
                fill.prototypes()[i.prototype as usize].key.species == "pseudotsuga-menziesii"
            })
            .count()
    };
    assert_eq!(firs_in(true, true), 0, "no fir on the saturated bank");
    assert!(firs_in(false, true) > 0 && firs_in(true, false) > 0);
    // A moisture outside 0 to 1 is refused.
    stands.cells[0].moisture = Some(1.5);
    assert!(fill.tile(&site, &stands, 1).is_err());
}

#[test]
fn one_tile_holds_many_communities_and_bare_cells() {
    let fill = fill();
    let site = Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site");
    let mut stands = StandMap::uniform(80.0, 0, 80.0);
    stands.cells[1].community = Some(1);
    stands.cells[2] = StandCell {
        community: None,
        years_since_disturbance: 0.0,
        moisture: None,
    };
    let plan = fill.tile(&site, &stands, 1).expect("tile");
    assert!(
        plan.instances
            .iter()
            .all(|i| !(i.position[0] < 40.0 && i.position[1] >= 40.0)),
        "the bare cell stays bare"
    );
    // The meadow's cell holds only yarrow, grown in the open.
    let meadow_cell: Vec<&Instance> = plan
        .instances
        .iter()
        .filter(|i| i.position[0] >= 40.0 && i.position[1] < 40.0)
        .collect();
    assert!(!meadow_cell.is_empty());
    assert!(meadow_cell.iter().all(|i| {
        let key = &fill.prototypes()[i.prototype as usize].key;
        key.species == "achillea-millefolium" && key.environment == Environment::Open
    }));
    // The forest's far canopy wears its Douglas-firs' foliage, the
    // meadow's (no crowns) another green, the bare cell's none.
    let fir = Generator::new("pseudotsuga-menziesii", &Options::default())
        .expect("fir")
        .spec()
        .appearance
        .foliage;
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
    assert!(plan.canopy[0].cover > 0.5);
    assert!(
        close(plan.canopy[0].colour, fir),
        "{:?}",
        plan.canopy[0].colour
    );
    assert!(plan.canopy[1].cover < 1e-6 && !close(plan.canopy[1].colour, fir));
    assert!(close(plan.canopy[2].colour, [0.0; 3]));
    let mut bad = stands.clone();
    bad.cells[0].community = Some(2);
    assert!(fill.tile(&site, &bad, 1).is_err());
}

#[test]
fn a_young_stand_is_open_grown_and_closes_as_it_ages() {
    // Before its crowns close a stand's firs grow as in the open; after,
    // as in a closed stand, its canopy covering most of the ground
    // (western Oregon's stands past closure: 60-96%, McIntosh et al.
    // 2009).
    let fill = fill();
    let site = Site::flat(TileId { x: 5, y: 5 }, 80.0).expect("site");
    let fir_settings = |years: f32| {
        let plan = fill
            .tile(&site, &StandMap::uniform(80.0, 0, years), 3)
            .expect("tile");
        let mut settings: Vec<Environment> = plan
            .instances
            .iter()
            .map(|i| &fill.prototypes()[i.prototype as usize].key)
            .filter(|key| key.species == "pseudotsuga-menziesii")
            .map(|key| key.environment)
            .collect();
        settings.dedup();
        (settings, plan.stats)
    };
    let (young, _) = fir_settings(20.0);
    assert_eq!(young, [Environment::Open]);
    let (closed, stats) = fir_settings(40.0);
    assert_eq!(closed, [Environment::Interior]);
    assert!(
        (0.6..0.96).contains(&stats.canopy_cover),
        "canopy cover {}",
        stats.canopy_cover
    );
    assert!(stats.trees_per_ha > 100.0, "{stats:?}");
    assert!(stats.basal_area_m2_per_ha > 1.0, "{stats:?}");
}

#[test]
fn herbs_left_to_the_ground_cover_are_not_placed_and_a_tile_needs_its_sizes() {
    let fill = fill();
    let site = Site::flat(TileId { x: 9, y: 1 }, 40.0).expect("site");
    let plan = fill
        .tile(&site, &StandMap::uniform(40.0, 1, 40.0), 4)
        .expect("tile");
    assert!(plan.stats.instances > 0);
    // Herbs left to the ground cover: none placed, the cover kept.
    let mut settings = settings();
    settings.herbs = false;
    let stands = StandMap::uniform(40.0, 0, 40.0);
    let bare = Fill::new(&[meadow()], &settings).expect("fill");
    assert!(bare.tile(&site, &stands, 4).is_err(), "not measured yet");
    bare.measure(&[], 2, None).expect("measured");
    let plan = bare.tile(&site, &stands, 4).expect("tile");
    assert!(plan.instances.is_empty());
    assert!(plan.cover[0].fraction > 0.0, "{:?}", plan.cover);
}

#[test]
fn every_example_community_loads_and_makes_a_fill() {
    // docs/examples/communities: each file is a community of the
    // library's species, and together they make one fill, as a city's
    // stand map uses them.
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/communities");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("examples")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 8, "{paths:?}");
    let communities: Vec<Community> = paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).expect("read");
            let community = Community::from_json(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let stem = path.file_stem().and_then(|s| s.to_str()).expect("stem");
            if stem != "wind-river" {
                assert_eq!(community.id, stem, "{}", path.display());
            }
            community
        })
        .collect();
    let planted: Vec<&str> = communities
        .iter()
        .filter(|c| c.origin == plantgen::development::Origin::Planted)
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(
        planted,
        ["olympia-garden-bed", "olympia-lawn", "olympia-yard-trees"]
    );
    let mut settings = FillSettings::default();
    settings.stand_ages = vec![10.0, 80.0];
    settings.seeds = 1;
    let fill = Fill::new(&communities, &settings).expect("fill");
    assert!(!fill.prototypes().is_empty());
}

#[test]
fn a_tile_writes_its_header_and_packed_instances() {
    let fill = fill();
    let site = Site::flat(TileId { x: -1, y: 4 }, 40.0).expect("site");
    let plan = fill
        .tile(&site, &StandMap::uniform(40.0, 0, 50.0), 2)
        .expect("tile");
    let dir = std::env::temp_dir().join(format!("plantgen-fill-test-{}", std::process::id()));
    let json = plan.write(&dir).expect("write");
    let bytes = std::fs::read(dir.join("-1_4.bin")).expect("bin");
    assert_eq!(bytes.len(), plan.instances.len() * INSTANCE_BYTES);
    let header: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).expect("json")).expect("parse");
    assert_eq!(header["schema"], INSTANCE_SCHEMA);
    assert!(header["stats"]["canopy_cover"].is_number());
    assert_eq!(header["instances"], plan.instances.len());
    let first: [u8; INSTANCE_BYTES] = bytes[..INSTANCE_BYTES].try_into().expect("bytes");
    assert_eq!(
        Instance::from_bytes(&first).prototype,
        plan.instances[0].prototype
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn every_prototype_is_a_cohort_of_a_stand_in_each_seed() {
    let fill = fill();
    let settings = settings();
    let mut keys: Vec<_> = fill.prototypes().iter().map(|p| p.key.clone()).collect();
    keys.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    keys.dedup();
    assert_eq!(keys.len(), fill.prototypes().len());
    // Each cohort in as many seeds as the settings ask.
    let mut cohorts: Vec<_> = keys
        .iter()
        .map(|key| (key.species.clone(), key.environment, key.age_tenths))
        .collect();
    cohorts.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    cohorts.dedup();
    assert_eq!(
        keys.len(),
        cohorts.len() * usize::from(settings.seeds),
        "{keys:?}"
    );
    // The firs at the ages nearest the stands' (their keyframes 15 and
    // 30 years), the closed stand's also as an edge; the yarrow at its
    // oldest.
    for (index, prototype) in fill.prototypes().iter().enumerate() {
        assert!((f64::from(prototype.key.age_tenths) / 10.0 - prototype.age_years).abs() < 0.05);
        assert!(
            fill.size(index).is_some(),
            "{:?} is measured",
            prototype.key
        );
    }
    let mut firs: Vec<(Environment, u32)> = cohorts
        .iter()
        .filter(|c| c.0 == "pseudotsuga-menziesii")
        .map(|c| (c.1, c.2))
        .collect();
    firs.sort_by_key(|fir| fir.1);
    assert_eq!(
        firs,
        [
            (Environment::Open, 150),
            (Environment::Edge, 300),
            (Environment::Interior, 300)
        ],
        "{cohorts:?}"
    );
}

#[test]
fn shade_tolerant_conifers_that_came_up_under_the_canopy_grow_beneath_it() {
    // A 160-year stand of Douglas-fir, with western hemlock and bigleaf
    // maple that came up under it from its 100th year.
    let community = Community::from_json(
        r#"{"id": "test-old-forest", "members": [
            {"species": "pseudotsuga-menziesii"},
            {"species": "tsuga-heterophylla"},
            {"species": "acer-macrophyllum"}
        ]}"#,
    )
    .expect("community");
    let mut settings = FillSettings::default();
    settings.stand_ages = vec![160.0];
    settings.seeds = 1;
    settings.edges = false;
    let fill = Fill::new(&[community], &settings).expect("fill");
    let keys: Vec<&PrototypeKey> = fill.prototypes().iter().map(|p| &p.key).collect();
    let of = |species: &str| -> Vec<&PrototypeKey> {
        keys.iter()
            .copied()
            .filter(|key| key.species == species)
            .collect()
    };
    // The pioneers grow in their stand; the hemlocks that came up under
    // it beneath a canopy as tall as Douglas-fir commonly grows (60 m),
    // at every age to the 60 years since; the maples, whose program does
    // not grow them beneath a canopy yet, in the stand.
    assert!(
        of("pseudotsuga-menziesii")
            .iter()
            .all(|key| key.environment == Environment::Interior && key.beneath_m.is_none())
    );
    let beneath: Vec<u32> = of("tsuga-heterophylla")
        .iter()
        .filter(|key| key.beneath_m == Some(60))
        .map(|key| {
            assert_eq!(key.environment, Environment::Suppressed);
            key.age_tenths
        })
        .collect();
    assert_eq!(beneath, [50, 150, 300, 600], "{keys:?}");
    assert!(
        of("acer-macrophyllum")
            .iter()
            .all(|key| key.beneath_m.is_none() && key.environment == Environment::Interior),
        "{keys:?}"
    );
    let young = fill
        .prototypes()
        .iter()
        .position(|p| p.key.beneath_m.is_some() && p.key.age_tenths == 150)
        .expect("a 15-year hemlock beneath");
    assert_eq!(
        fill.prototypes()[young].key.stem(),
        "tsuga-heterophylla-beneath60m-1-15y"
    );
    // Beneath the canopy a 15-year hemlock is a sapling, under half the
    // height of one in a stand of its own age.
    let (_, plant) = fill.grow_prototype(young).expect("grown");
    let stand = Generator::new("tsuga-heterophylla", &Options::default())
        .expect("hemlock")
        .grow_in(Environment::Interior, 1, 15.0)
        .expect("grown");
    assert!(
        plant.size.height < 0.5 * stand.size.height,
        "{} m beneath, {} m in a stand",
        plant.size.height,
        stand.size.height
    );
}

#[test]
fn each_prototype_grows_in_its_package_as_the_fill_measured_it() {
    // Hemlocks that came up beneath a 160-year Douglas-fir stand, and the
    // meadow's yarrows: a host that draws packages builds one per growth,
    // and each keyframe is the prototype as the fill measured it.
    let community = Community::from_json(
        r#"{"id": "test-old-forest", "members": [
            {"species": "pseudotsuga-menziesii"},
            {"species": "tsuga-heterophylla"}
        ]}"#,
    )
    .expect("community");
    let mut settings = FillSettings::default();
    settings.stand_ages = vec![160.0];
    settings.seeds = 1;
    settings.edges = false;
    let old = Fill::new(&[community], &settings).expect("fill");
    let beneath: Vec<u32> = (0..old.prototypes().len())
        .filter(|&i| {
            let key = &old.prototypes()[i].key;
            key.beneath_m.is_some() && key.age_tenths <= 150
        })
        .map(|i| u32::try_from(i).expect("index"))
        .collect();
    assert_eq!(beneath.len(), 2, "{:?}", old.prototypes());
    let yarrows: Vec<u32> = (0..fill().prototypes().len())
        .filter(|&i| fill().prototypes()[i].key.species == "achillea-millefolium")
        .map(|i| u32::try_from(i).expect("index"))
        .collect();
    assert!(!yarrows.is_empty());
    old.measure(&beneath, 4, None).expect("measured");
    for (fill, which) in [(&old, &beneath), (fill(), &yarrows)] {
        let packages = fill.packages(which).expect("packages");
        let held: usize = packages.iter().map(|p| p.prototypes.len()).sum();
        assert_eq!(held, which.len());
        for package in &packages {
            assert_eq!(package.spec.variant_list().len(), 1);
            let inputs = Inputs::new(&package.spec, &quality::DRAFT).expect("inputs");
            let built = package::build(&inputs, 4, &mut |_| {}).expect("built");
            let keyframes = &built.manifest.variants[0].keyframes;
            for &(index, keyframe) in &package.prototypes {
                let prototype = &fill.prototypes()[index as usize];
                assert!(
                    (keyframes[keyframe].age - prototype.age_years).abs() < 1e-9,
                    "{}",
                    prototype.key.stem()
                );
                // The same plant: its counts equal, its sizes to rounding.
                let (kept, measured) = (
                    &keyframes[keyframe].size,
                    fill.size(index as usize).expect("measured"),
                );
                let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(1.0);
                assert!(
                    kept.segments == measured.segments
                        && kept.organs == measured.organs
                        && near(kept.height, measured.height)
                        && near(kept.crown_radius, measured.crown_radius)
                        && near(kept.leaf_area, measured.leaf_area),
                    "{}: {kept:?} in its package, {measured:?} measured",
                    prototype.key.stem()
                );
            }
        }
    }
}

/// The way a placed plant faces: its east turned by its rotation, as an
/// angle from east toward north.
fn facing(instance: &Instance) -> f64 {
    let [x, y, z, w] = instance.rotation.map(f64::from);
    // East (1, 0, 0) turned by the quaternion.
    let east = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z)];
    east[1].atan2(east[0])
}

fn bare() -> StandCell {
    StandCell {
        community: None,
        years_since_disturbance: 0.0,
        moisture: None,
    }
}

fn is_edge(fill: &Fill, instance: &Instance) -> bool {
    fill.prototypes()[instance.prototype as usize]
        .key
        .environment
        == Environment::Edge
}

#[test]
fn a_stands_first_row_beside_a_road_grows_as_an_edge_facing_it() {
    // A closed 40-year stand in three cells of an 80 m tile, a road in
    // the south-east cell.
    let fill = fill();
    let site = Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site");
    let mut stands = StandMap::uniform(80.0, 0, 40.0);
    stands.cells[1] = bare();
    let plan = fill.tile(&site, &stands, 3).expect("tile");
    let edges: Vec<&Instance> = plan.instances.iter().filter(|i| is_edge(fill, i)).collect();
    assert!(!edges.is_empty());
    assert!(plan.stats.edge_trees_per_ha > 0.0);
    for edge in &edges {
        let [x, y, _] = edge.position;
        // Within the crown's width the stand laid it out with, as grown
        // inside the stand.
        let key = PrototypeKey {
            environment: Environment::Interior,
            ..fill.prototypes()[edge.prototype as usize].key.clone()
        };
        let interior = fill
            .prototypes()
            .iter()
            .position(|p| p.key == key)
            .expect("its interior twin");
        let grown = fill.size(interior).expect("measured");
        let width = 2.0 * grown.crown_radius * f64::from(edge.scale) + 1e-2;
        // West of the road it faces east; north of it, south.
        let (gap, toward) = if y < 40.0 {
            (40.0 - f64::from(x), 0.0)
        } else {
            (f64::from(y) - 40.0, -std::f64::consts::FRAC_PI_2)
        };
        assert!(
            x < 40.0 || y >= 40.0,
            "{:?} stands in the road",
            edge.position
        );
        assert!(
            gap >= 0.0 && gap < width,
            "{:?} is not in the first row",
            edge.position
        );
        assert!(
            (facing(edge) - toward).abs() < 0.5,
            "{:?} faces {}",
            edge.position,
            facing(edge)
        );
    }
    // The cell with no opening beside it, and every shrub and herb, stays
    // as it was.
    assert!(
        plan.instances
            .iter()
            .filter(|i| i.position[0] < 40.0 && i.position[1] >= 40.0)
            .all(|i| !is_edge(fill, i))
    );
    // A stand of the same age beside it is no opening.
    let mut young = stands.clone();
    young.cells[1] = StandCell {
        community: Some(0),
        years_since_disturbance: 40.0,
        moisture: None,
    };
    let closed = fill.tile(&site, &young, 3).expect("tile");
    assert!(closed.instances.iter().all(|i| !is_edge(fill, i)));
}

#[test]
fn a_stand_without_edges_grows_none() {
    let mut settings = settings();
    settings.edges = false;
    let fill = Fill::new(&[forest()], &settings).expect("fill");
    assert!(
        fill.prototypes()
            .iter()
            .all(|p| p.key.environment != Environment::Edge)
    );
}

#[test]
fn an_edge_on_a_tiles_side_grows_as_it_would_in_one_tile() {
    // The road of the tile above, seen from the north-east quarter alone:
    // given the cell past its south side, its first row is the whole
    // tile's.
    let fill = fill();
    let mut stands = StandMap::uniform(80.0, 0, 40.0);
    stands.cells[1] = bare();
    let whole = fill
        .tile(
            &Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site"),
            &stands,
            7,
        )
        .expect("tile");
    let forest = StandMap::uniform(40.0, 0, 40.0).cells[0];
    let quarter_site = Site::flat(TileId { x: 1, y: 1 }, 40.0).expect("site");
    let quarter_stands = StandMap::uniform(40.0, 0, 40.0);
    let quarter = fill
        .tile_among(
            &quarter_site,
            &quarter_stands,
            &|col, row| {
                Some(if (col, row) == (0, -1) {
                    bare()
                } else {
                    forest
                })
            },
            7,
        )
        .expect("tile");
    let in_corner: Vec<Instance> = whole
        .instances
        .iter()
        .filter(|i| i.position[0] >= 40.0 && i.position[1] >= 40.0)
        .map(|i| Instance {
            position: [i.position[0] - 40.0, i.position[1] - 40.0, i.position[2]],
            ..*i
        })
        .collect();
    assert!(in_corner.iter().any(|i| is_edge(fill, i)));
    assert_eq!(in_corner.len(), quarter.instances.len());
    for (a, b) in in_corner.iter().zip(&quarter.instances) {
        assert_eq!(a.prototype, b.prototype);
        assert_eq!(a.rotation.map(f32::to_bits), b.rotation.map(f32::to_bits));
    }
    // Not told what lies past its side, the quarter grows no edge there.
    let alone = fill.tile(&quarter_site, &quarter_stands, 7).expect("tile");
    assert!(alone.instances.iter().all(|i| !is_edge(fill, i)));
}

#[test]
fn a_tall_plants_parts_draw_each_level_between_them() {
    // A tree cut into its bole and crown bands: each level's parts draw
    // every triangle of the level once, so a caller can draw each part at
    // the detail its own distance asks for and the tree stays whole.
    let mut options = plantgen::api::Options::default();
    options.quality = plantgen::quality::DRAFT;
    let fir = plantgen::api::Generator::new("pseudotsuga-menziesii", &options).expect("fir");
    let plant = fir
        .grow_in(plantgen::api::Environment::Interior, 1, 40.0)
        .expect("it grows");
    let regions = plantgen::api::crown_regions(&plant.size);
    assert!(regions.len() >= 2, "{regions:?}");
    assert_eq!(regions[0].name, "bole");
    let settings = plantgen::api::Settings {
        budget: plantgen::api::Budget::tiers(),
        ..Default::default()
    };
    let exported = fir
        .export_parts(&plant, &settings, &regions)
        .expect("it exports");
    for level in &exported.levels {
        assert_eq!(level.parts.len(), regions.len());
        let parts: usize = level.parts.iter().map(|part| part.summary.triangles).sum();
        assert_eq!(parts, level.summary.triangles, "{}", level.file);
    }
    assert!(exported.levels[0].parts[0].file.ends_with("-bole-lod0.glb"));
    // A yarrow is seen whole from wherever it is near.
    let yarrow = plantgen::api::Generator::new("achillea-millefolium", &options).expect("yarrow");
    let small = yarrow.grow(1).expect("it grows");
    assert!(plantgen::api::crown_regions(&small.size).is_empty());
}

#[test]
fn a_tile_as_one_gltf_file_draws_every_plant_as_an_instance() {
    use plantgen::api::{Compression, Settings};
    let meadow = Community::from_json(
        r#"{"id": "test-meadow", "members": [
            {"species": "achillea-millefolium", "layer": "herb", "abundance": 0.1}
        ]}"#,
    )
    .expect("community");
    let mut settings = FillSettings::default();
    settings.stand_ages = vec![20.0];
    settings.seeds = 1;
    settings.options.quality = plantgen::quality::DRAFT;
    let fill = Fill::new(&[meadow], &settings).expect("fill");
    fill.measure(&[], 2, None).expect("measured");
    let site = Site::flat(TileId { x: 1, y: 2 }, 20.0).expect("site");
    let plan = fill
        .tile(&site, &StandMap::uniform(20.0, 0, 30.0), 4)
        .expect("tile");
    assert!(!plan.instances.is_empty());
    let dir = std::env::temp_dir().join(format!("plantgen-tile-glb-{}", std::process::id()));
    let export = Settings {
        compression: Compression::Meshopt,
        ..Settings::default()
    };
    let prototypes = fill
        .write_prototypes(&plan.prototypes_used(), &export, 2, &dir)
        .expect("prototypes");
    for meshopt in [false, true] {
        let glb = plan.glb(&prototypes, &dir, 3, meshopt).expect("tile glb");
        let summary = plantgen::gltf::check(&glb).expect("a valid file");
        // Each mesh node of a prototype draws once per plant placing it.
        let mut expected = 0;
        let mut triangles = 0;
        for record in &prototypes {
            let placed = plan
                .instances
                .iter()
                .filter(|i| i.prototype == record.index)
                .count();
            let low = &record.plant.levels[3];
            let one = plantgen::gltf::check(&std::fs::read(dir.join(&low.file)).unwrap())
                .expect("prototype file");
            expected += placed * one.instances;
            triangles += placed * low.triangles;
        }
        assert!(summary.instances >= plan.instances.len());
        assert_eq!(summary.instances, expected, "meshopt {meshopt}");
        assert_eq!(summary.triangles, triangles, "meshopt {meshopt}");
    }
    // Cut into 10 m cells at levels 2 and 3: every plant is in one cell,
    // a cell's files draw what the whole tile's do there, and the coarser
    // level is enough from farther.
    let whole = plantgen::gltf::check(&plan.glb(&prototypes, &dir, 3, true).unwrap()).unwrap();
    let files = plan
        .glb_cells(&prototypes, &dir, 10.0, &[2, 3], true)
        .expect("tile cells");
    let (index_name, index) = files.last().expect("an index");
    assert_eq!(index_name, "1_2/cells.json");
    let index: serde_json::Value = serde_json::from_slice(index).expect("json");
    let cells = index["cells"].as_array().expect("cells");
    assert_eq!(files.len(), cells.len() * 2 + 1);
    let placed: u64 = cells.iter().map(|c| c["instances"].as_u64().unwrap()).sum();
    assert_eq!(usize::try_from(placed).unwrap(), plan.instances.len());
    let mut drawn = 0;
    for (name, glb) in &files[..files.len() - 1] {
        let summary = plantgen::gltf::check(glb).expect("a valid cell file");
        if name.ends_with("-lod3.glb") {
            drawn += summary.instances;
        }
    }
    assert_eq!(drawn, whole.instances);
    for cell in cells {
        let levels = cell["levels"].as_array().unwrap();
        let from = |i: usize| levels[i]["from_m"].as_f64().unwrap();
        assert!(from(0) < from(1), "{cell}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fill over a measured site: plants on shallow soil stay small, and
/// plants that came up by themselves in a pavement's cracks are drawn.
mod on_a_site_field {
    use std::sync::Arc;

    use std::sync::LazyLock;

    use super::{StandMap, TileId, fill};
    use plantgen::api::{Community, CrackPlant, FieldSettings, Fill, FillSettings, Site, WallClip};
    use plantgen::math::Vec3;
    use plantgen::pavement::{Crack, CrackTile, Hole, Pavement};
    use plantgen::site::{SiteMesh, Surface};
    use plantgen::site_field::build;
    use plantgen::substrate::{BUILT, ROCK, SOIL};
    use plantgen::succession::{Colonist, Dispersal, Settings, Size, Source, microsites};

    type Parts = (Vec<Vec3>, Vec<[u32; 3]>, Vec<u8>);

    /// A box from `low` to `high`, its faces wound outward.
    fn cuboid(low: Vec3, high: Vec3, material: u8, out: &mut Parts) {
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

    /// Ground 40 m across, its top at 0: `soil_m` of soil over rock.
    fn ground(soil_m: f64) -> SiteMesh {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-20.0, -soil_m, -20.0),
            Vec3::new(20.0, 0.0, 20.0),
            SOIL,
            &mut parts,
        );
        cuboid(
            Vec3::new(-20.0, -soil_m - 3.0, -20.0),
            Vec3::new(20.0, -soil_m, 20.0),
            ROCK,
            &mut parts,
        );
        SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap()
    }

    fn settings() -> FieldSettings {
        FieldSettings {
            cell_m: 1.0,
            margin_m: 0.0,
            sky_rays: 16,
            ..FieldSettings::default()
        }
    }

    /// The fill's tile over all of `mesh`: 40 m, its south-west corner at
    /// (-20, 20), north being -z.
    fn tile_of(mesh: &SiteMesh) -> Site {
        let field = build(mesh, [-20.0, -20.0], [20.0, 20.0], &settings()).unwrap();
        Site::from_field(TileId { x: 0, y: 0 }, 40.0, Arc::new(field), [-20.0, 20.0]).unwrap()
    }

    #[test]
    fn trees_on_shallow_soil_are_drawn_smaller_and_cover_less() {
        let fill = fill();
        let stands = StandMap::uniform(40.0, 0, 40.0);
        let deep = fill.tile(&tile_of(&ground(4.0)), &stands, 5).unwrap();
        let shallow = fill.tile(&tile_of(&ground(0.03)), &stands, 5).unwrap();
        // The same plants stand in the same places, only smaller.
        assert_eq!(deep.instances.len(), shallow.instances.len());
        let mut smaller = 0;
        for (d, s) in deep.instances.iter().zip(&shallow.instances) {
            assert_eq!(
                (d.prototype, d.position[0], d.position[1]),
                (s.prototype, s.position[0], s.position[1])
            );
            assert!(s.scale <= d.scale + 1.0e-6, "{} > {}", s.scale, d.scale);
            smaller += usize::from(s.scale < 0.9 * d.scale);
        }
        assert!(smaller > 0, "no plant was held back by 3 cm of soil");
        // The firs too: 3 cm of soil holds a conifer back as it does a
        // herb.
        assert!(
            shallow.stats.basal_area_m2_per_ha < 0.9 * deep.stats.basal_area_m2_per_ha,
            "{} {}",
            shallow.stats.basal_area_m2_per_ha,
            deep.stats.basal_area_m2_per_ha
        );
        assert!(shallow.stats.canopy_cover < deep.stats.canopy_cover);
    }

    /// Ground 40 m across under a slab with one 2 cm crack across it, east
    /// to west at z = 2, and a pothole 7 cm deep in soil at (0, 6).
    fn cracked_lot() -> SiteMesh {
        let mut site = ground(2.0);
        let tile = CrackTile {
            version: 1,
            origin_m: [-20.0, -20.0],
            size_m: [40.0, 40.0],
            top_m: 0.0,
            thickness_m: 0.08,
            base_m: 0.2,
            base_rootable: None,
            subgrade_m: None,
            subgrade_rootable: None,
            surface: "asphalt".into(),
            cell_m: None,
            cracks: vec![Crack {
                points_m: vec![[-15.0, 2.0], [15.0, 2.0]],
                width_m: vec![0.02, 0.02],
                fill_m: vec![0.06, 0.06],
            }],
            holes: vec![Hole {
                outline_m: (0..12)
                    .map(|k| {
                        let t = std::f64::consts::TAU * f64::from(k) / 12.0;
                        [0.35 * t.cos(), 6.0 + 0.35 * t.sin()]
                    })
                    .collect(),
                depth_m: 0.12,
                fill_m: 0.07,
            }],
            traffic: Vec::new(),
            made_by: None,
        };
        site.add_pavement(Pavement::new(&tile).unwrap());
        site
    }

    #[test]
    fn plants_that_came_up_in_a_crack_are_drawn_after_the_stands() {
        let fill = fill();
        let mesh = cracked_lot();
        let traits = plantgen::library::Library::builtin()
            .traits_of("achillea-millefolium")
            .unwrap();
        let soil = plantgen::api::Generator::new(
            "achillea-millefolium",
            &plantgen::api::Options::default(),
        )
        .unwrap()
        .soil_need();
        let mut yarrow = Colonist::new(
            "achillea-millefolium",
            &traits,
            false,
            soil,
            vec![
                Size {
                    age: 1.0,
                    height: 0.15,
                    crown: 0.08,
                },
                Size {
                    age: 3.0,
                    height: 0.4,
                    crown: 0.15,
                },
                Size {
                    age: 8.0,
                    height: 0.6,
                    crown: 0.2,
                },
            ],
        );
        yarrow.dispersal = vec![Dispersal::Wind];
        let sites = microsites(&mesh, 2);
        let settings = Settings {
            seed: 3,
            traffic: 0.0,
            maintenance: 0.0,
        };
        // A yarrow at the lot's edge seeds it.
        let parent = Source {
            colonist: 0,
            x: 0.0,
            z: 9.0,
            age: 5.0,
        };
        let site = tile_of(&mesh)
            .with_colonists(&sites, &[yarrow], &[parent], &settings, 8)
            .unwrap();
        assert!(!site.colonists.is_empty(), "nothing came up in 8 years");
        // The slab leaves no room for the stand; only the crack's plants.
        let mut stands = StandMap::uniform(40.0, 1, 40.0);
        stands.cells[0].community = None;
        let plan = fill.tile(&site, &stands, 5).unwrap();
        assert_eq!(plan.instances.len(), site.colonists.len());
        for (instance, plant) in plan.instances.iter().zip(&site.colonists) {
            // North 18 m from the corner at z = 20 is the crack at z = 2,
            // north 14 m the pothole at z = 6.
            let north = f64::from(instance.position[1]);
            assert!(
                (north - 18.0).abs() < 0.2 || (north - 14.0).abs() < 0.5,
                "{plant:?} at {:?}",
                instance.position
            );
            assert!((plant.north - north).abs() < 1.0e-3, "{plant:?}");
            assert!(plant.height_m > 0.0 && instance.scale > 0.0);
        }
        // The same site fills the same way.
        assert_eq!(
            fill.tile(&site, &stands, 5).unwrap().instances,
            plan.instances
        );
        // A colonist the fill holds no prototype of is refused.
        let mut stranger = site.clone();
        stranger.colonists.push(CrackPlant {
            species: "taraxacum-sect-taraxacum".into(),
            east: 1.0,
            north: 18.0,
            age_years: 2.0,
            height_m: 0.2,
            crown_m: 0.1,
        });
        let error = fill.tile(&stranger, &stands, 5).unwrap_err().to_string();
        assert!(error.contains("crack pool"), "{error}");
    }

    /// Soil 1 m deep over rock, 40 m across, and a brick shed 10 m by 10 m
    /// and 3 m high from x = -10 to 0 and z = -5 to 5, 4 cm of soil
    /// gathered on its flat roof; a curb and a raised sign board apart.
    fn yard() -> SiteMesh {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-20.0, -1.0, -20.0),
            Vec3::new(20.0, 0.0, 20.0),
            SOIL,
            &mut parts,
        );
        cuboid(
            Vec3::new(-20.0, -4.0, -20.0),
            Vec3::new(20.0, -1.0, 20.0),
            ROCK,
            &mut parts,
        );
        cuboid(
            Vec3::new(-10.0, -0.3, -5.0),
            Vec3::new(0.0, 3.0, 5.0),
            BUILT,
            &mut parts,
        );
        let shed = parts.2.len() - 12;
        // A curb too low to climb, and a sign board whose foot stands
        // 2 m above the ground.
        cuboid(
            Vec3::new(5.0, -0.1, 10.0),
            Vec3::new(15.0, 0.2, 10.2),
            BUILT,
            &mut parts,
        );
        cuboid(
            Vec3::new(5.0, 2.0, -15.0),
            Vec3::new(15.0, 4.0, -14.7),
            BUILT,
            &mut parts,
        );
        let mut surfaces: Vec<Surface> = parts.2.iter().map(|m| Surface::solid(*m)).collect();
        // The shed's top: its fourth face, triangles 6 and 7 of its 12.
        let top = shed + 6;
        for surface in &mut surfaces[top..top + 2] {
            surface.soil_m = 0.04;
        }
        SiteMesh::with_surfaces(&parts.0, &parts.1, &surfaces).unwrap()
    }

    /// A yard's herbs and the ivy on its walls, its climbers grown on a
    /// wall.
    static YARD_FILL: LazyLock<Fill> = LazyLock::new(|| {
        let yard = Community::from_json(
            r#"{"id": "test-yard", "members": [
                {"species": "achillea-millefolium", "layer": "herb", "environments": ["open"], "abundance": 1.6},
                {"species": "hedera-helix", "layer": "climber"}
            ]}"#,
        )
        .unwrap();
        let mut settings = FillSettings::default();
        settings.stand_ages = vec![20.0];
        settings.seeds = 1;
        let fill = Fill::new(&[yard], &settings).unwrap();
        fill.measure(&[], 8, None).unwrap();
        fill
    });

    /// `v` turned by the quaternion `q` (x, y, z, w).
    #[allow(clippy::many_single_char_names)]
    fn turned(q: [f32; 4], v: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = q.map(f64::from);
        let t = [
            2.0 * (y * v[2] - z * v[1]),
            2.0 * (z * v[0] - x * v[2]),
            2.0 * (x * v[1] - y * v[0]),
        ];
        [
            v[0] + w * t[0] + (y * t[2] - z * t[1]),
            v[1] + w * t[1] + (z * t[0] - x * t[2]),
            v[2] + w * t[2] + (x * t[1] - y * t[0]),
        ]
    }

    #[test]
    fn ivy_stands_at_the_foot_of_walls_and_herbs_on_a_roof_with_soil() {
        let fill = &*YARD_FILL;
        let site = tile_of(&yard());
        let stands = StandMap::uniform(40.0, 0, 20.0);
        let plan = fill.tile(&site, &stands, 3).unwrap();
        assert_eq!(fill.tile(&site, &stands, 3).unwrap(), plan);
        let shed =
            |east: f32, north: f32| (10.0..=20.0).contains(&east) && (15.0..=25.0).contains(&north);
        let (mut ivy, mut on_roof, mut on_ground) = (0, Vec::new(), Vec::new());
        for instance in &plan.instances {
            let prototype = &fill.prototypes()[instance.prototype as usize];
            let [east, north, up] = instance.position;
            if prototype.key.on_wall {
                ivy += 1;
                assert_eq!(prototype.key.species, "hedera-helix");
                let clip = WallClip::from_extra(instance.extra).expect("a climber is cut");
                assert!((clip.top_m - 3.0).abs() < 0.01, "cut at the 3 m wall's top");
                // 0.3 m out from one of the shed's faces, its own face
                // (the prototype's south) turned onto the wall's.
                let x = f64::from(east) - 20.0;
                let z = 20.0 - f64::from(north);
                let faces = [
                    ((x + 10.3).abs(), [-1.0, 0.0]),
                    ((x - 0.3).abs(), [1.0, 0.0]),
                    ((z + 5.3).abs(), [0.0, 1.0]),
                    ((z - 5.3).abs(), [0.0, -1.0]),
                ];
                let (gap, out) = faces
                    .iter()
                    .copied()
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap();
                assert!(
                    gap < 0.3,
                    "ivy {gap:.2} m off the 0.3 m line at ({x:.2}, {z:.2})"
                );
                let facing = turned(instance.rotation, [0.0, -1.0, 0.0]);
                assert!(
                    (facing[0] - out[0]).abs() < 1e-3 && (facing[1] - out[1]).abs() < 1e-3,
                    "faces {facing:?}, the wall {out:?}"
                );
                // Cut at both ends of its wall, never past them: the
                // prototype's +x along the field (x, z), and how far the
                // shed's wall runs that way from the foot.
                let ahead = turned(instance.rotation, [1.0, 0.0, 0.0]);
                let ahead = [ahead[0], -ahead[1]];
                let to_end = |d: [f64; 2]| {
                    if d[0] > 0.5 {
                        -x
                    } else if d[0] < -0.5 {
                        x + 10.0
                    } else if d[1] > 0.5 {
                        5.0 - z
                    } else {
                        z + 5.0
                    }
                };
                for (cut, end) in [
                    (clip.ahead_m, to_end(ahead)),
                    (clip.back_m, to_end([-ahead[0], -ahead[1]])),
                ] {
                    if cut.is_finite() {
                        let cut = f64::from(cut);
                        assert!(
                            cut <= end + 1e-3 && cut >= end - 0.2 - 1e-3,
                            "cut {cut:.2} m out, the wall ends {end:.2} m out"
                        );
                    } else {
                        assert!(end >= 6.2 - 1e-3, "uncut, the wall ends {end:.2} m out");
                    }
                }
                assert!(up.abs() < 0.05);
            } else if shed(east, north) {
                assert!((up - 3.0).abs() < 0.05, "a herb inside the shed at {up} m");
                on_roof.push(instance.scale);
            } else {
                on_ground.push(instance.scale);
            }
        }
        // 40 m of wall foot, one ivy every 3 m or so.
        assert!((6..=24).contains(&ivy), "{ivy} ivies");
        assert!(!on_roof.is_empty() && !on_ground.is_empty());
        // 4 cm of roof soil holds a herb back.
        #[allow(clippy::cast_precision_loss)]
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(
            mean(&on_roof) < 0.8 * mean(&on_ground),
            "{} {}",
            mean(&on_roof),
            mean(&on_ground)
        );
    }
}
