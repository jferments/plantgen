//! The external contract of a stands fill (`plantgen::api::Fill`): plans
//! that are the same however they are made, tiles that meet, stands that
//! develop as their age says, and the file forms callers read.

use std::sync::LazyLock;

use plantgen::api::{
    Community, Environment, Fill, FillSettings, INSTANCE_BYTES, INSTANCE_SCHEMA, Instance, Site,
    StandCell, StandMap, TileId,
};

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
fn one_tile_holds_many_communities_and_bare_cells() {
    let fill = fill();
    let site = Site::flat(TileId { x: 0, y: 0 }, 80.0).expect("site");
    let mut stands = StandMap::uniform(80.0, 0, 80.0);
    stands.cells[1].community = Some(1);
    stands.cells[2] = StandCell {
        community: None,
        years_since_disturbance: 0.0,
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
fn ground_that_takes_no_plant_holds_none_and_a_tile_needs_its_sizes() {
    let fill = fill();
    let site = Site::flat(TileId { x: 9, y: 1 }, 40.0).expect("site");
    let stands = StandMap::uniform(40.0, 1, 40.0);
    let plan = fill.tile(&site, &stands, 4).expect("tile");
    assert!(plan.stats.instances > 0);
    // Herbs left to the ground cover: none placed, the cover kept.
    let mut settings = settings();
    settings.herbs = false;
    let bare = Fill::new(&[meadow()], &settings).expect("fill");
    assert!(bare.tile(&site, &stands, 4).is_err(), "not measured yet");
    bare.measure(&[], 2, None).expect("measured");
    let plan = bare.tile(&site, &stands, 4).expect("tile");
    assert!(plan.instances.is_empty());
    assert!(plan.cover[0].fraction > 0.0, "{:?}", plan.cover);
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
    // 30 years), the yarrow at its oldest.
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
        [(Environment::Open, 150), (Environment::Interior, 300)],
        "{cohorts:?}"
    );
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
