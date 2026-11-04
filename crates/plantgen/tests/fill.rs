//! The external contract of a stands fill (`plantgen::api::Fill`): plans
//! that are the same however they are made, tiles that meet, and the file
//! forms callers read.

use plantgen::api::{
    Community, Fill, FillSettings, INSTANCE_BYTES, Instance, Site, StandCell, StandMap, TileId,
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

fn fill() -> Fill {
    Fill::new(&[forest()], &FillSettings::default()).expect("fill")
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
    let meadow = Community::from_json(
        r#"{"id": "test-meadow", "members": [
            {"species": "achillea-millefolium", "layer": "herb", "environments": ["open"], "abundance": 1.6}
        ]}"#,
    )
    .expect("community");
    let fill = Fill::new(&[forest(), meadow], &FillSettings::default()).expect("fill");
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
    let mut bad = stands.clone();
    bad.cells[0].community = Some(2);
    assert!(fill.tile(&site, &bad, 1).is_err());
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
    assert_eq!(header["schema"], 1);
    assert_eq!(header["instances"], plan.instances.len());
    let first: [u8; INSTANCE_BYTES] = bytes[..INSTANCE_BYTES].try_into().expect("bytes");
    assert_eq!(
        Instance::from_bytes(&first).prototype,
        plan.instances[0].prototype
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn every_prototype_is_keyed_by_species_seed_setting_and_class() {
    let fill = fill();
    let settings = FillSettings::default();
    let per_species = 4 * usize::from(settings.age_classes) * usize::from(settings.seeds);
    assert_eq!(fill.prototypes().len(), 2 * per_species);
    let mut keys: Vec<_> = fill.prototypes().iter().map(|p| p.key.clone()).collect();
    keys.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    keys.dedup();
    assert_eq!(keys.len(), fill.prototypes().len());
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
    settings.age_classes = 2;
    settings.seeds = 1;
    settings.options.quality = plantgen::quality::DRAFT;
    let fill = Fill::new(&[meadow], &settings).expect("fill");
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
    let _ = std::fs::remove_dir_all(&dir);
}
