//! `Fill` as a caller outside the crate uses it: through `plantgen::api`
//! alone (this file names no other module), with the files it writes read
//! back by their documented forms. `docs/LIBRARY-API.md`, "Stands over a
//! whole map", is the contract these tests hold it to.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use plantgen::api::{
    API_VERSION, Community, Compression, Fill, FillSettings, INSTANCE_BYTES, INSTANCE_SCHEMA,
    Instance, PROTOTYPES_FILE, PrototypeFiles, Quality, Settings, Site, Snag, SnagShape, StandMap,
    TileId, TilePlan, read_prototypes,
};

const TILE_M: f64 = 20.0;

fn meadow() -> Community {
    Community::from_json(
        r#"{"id": "caller-meadow", "members": [
            {"species": "achillea-millefolium", "layer": "herb", "abundance": 0.1}
        ]}"#,
    )
    .expect("community")
}

fn settings(stand_ages: &[f64]) -> FillSettings {
    let mut settings = FillSettings::default();
    settings.stand_ages = stand_ages.to_vec();
    settings.options.quality = Quality::DRAFT;
    settings
}

fn measured(stand_ages: &[f64]) -> Fill {
    let fill = Fill::new(&[meadow()], &settings(stand_ages)).expect("fill");
    fill.measure(&[], 4, None).expect("measured");
    fill
}

static FILL: LazyLock<Fill> = LazyLock::new(|| measured(&[20.0]));

fn plan(fill: &Fill) -> TilePlan {
    let site = Site::flat(TileId { x: 5, y: -3 }, TILE_M).expect("site");
    fill.tile(&site, &StandMap::uniform(TILE_M, 0, 25.0), 11)
        .expect("tile")
}

fn export() -> Settings {
    Settings {
        compression: Compression::Quantize,
        ..Settings::default()
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("plantgen-fill-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every file under `dir`, by its path inside it, with its bytes.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).expect("folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path.strip_prefix(dir).unwrap().display().to_string();
                out.push((name, std::fs::read(&path).expect("file")));
            }
        }
    }
    out.sort();
    out
}

/// Write a tile as a caller does: the plan, the prototypes it places,
/// the whole tile as one GLB and in 10 m cells.
fn write_tile(fill: &Fill, plan: &TilePlan, dir: &Path) -> Vec<PrototypeFiles> {
    plan.write(&dir.join("tiles")).expect("tile files");
    let prototypes = fill
        .write_prototypes(
            &plan.prototypes_used(),
            &export(),
            2,
            &dir.join("prototypes"),
        )
        .expect("prototypes");
    let glb = plan
        .glb(&prototypes, &dir.join("prototypes"), 3, false)
        .expect("tile glb");
    std::fs::write(dir.join("tiles").join("5_-3.glb"), glb).expect("write glb");
    for (name, bytes) in plan
        .glb_cells(&prototypes, &dir.join("prototypes"), 10.0, &[2, 3], false)
        .expect("cells")
    {
        let path = dir.join("tiles").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    prototypes
}

#[test]
fn the_api_version_is_the_one_the_docs_describe() {
    assert_eq!(API_VERSION, 7);
    assert_eq!(INSTANCE_SCHEMA, 4);
    assert_eq!(INSTANCE_BYTES, 32);
    // This file is a caller's: it reaches PlantGen through `api` alone.
    let source = include_str!("fill_api.rs");
    let crate_path = concat!("plant", "gen::");
    let reached = source.matches(crate_path).count();
    let through_api = source.matches(concat!("plant", "gen::api")).count();
    assert_eq!(
        reached, through_api,
        "a test here reaches past plantgen::api"
    );
}

#[test]
fn an_instance_decodes_from_its_documented_layout() {
    let plan = plan(&FILL);
    assert!(!plan.instances.is_empty());
    let bytes = plan.instance_bytes();
    assert_eq!(bytes.len(), plan.instances.len() * INSTANCE_BYTES);
    for (instance, packed) in plan
        .instances
        .iter()
        .zip(bytes.as_chunks::<INSTANCE_BYTES>().0)
    {
        // As the docs give it: u32 prototype, f32 x3 position, i16 x4
        // snorm rotation, f16 scale, u16 age in tenths, i8 x2 tint, u16
        // extra; little-endian, no padding.
        let u32_at = |at: usize| u32::from_le_bytes(packed[at..at + 4].try_into().unwrap());
        let f32_at = |at: usize| f32::from_le_bytes(packed[at..at + 4].try_into().unwrap());
        let i16_at = |at: usize| i16::from_le_bytes(packed[at..at + 2].try_into().unwrap());
        let u16_at = |at: usize| u16::from_le_bytes(packed[at..at + 2].try_into().unwrap());
        assert_eq!(u32_at(0), instance.prototype);
        for axis in 0..3 {
            assert_eq!(
                f32_at(4 + 4 * axis).to_bits(),
                instance.position[axis].to_bits()
            );
        }
        for k in 0..4 {
            let snorm = f32::from(i16_at(16 + 2 * k)) / 32_767.0;
            assert!((snorm - instance.rotation[k]).abs() < 1e-4, "rotation {k}");
        }
        let half = u16_at(24);
        let exponent = i32::from((half >> 10) & 0x1f);
        let mantissa = f32::from(half & 0x3ff);
        assert_eq!(half >> 15, 0, "a scale is positive");
        let scale = (1.0 + mantissa / 1024.0) * 2.0_f32.powi(exponent - 15);
        assert!((scale - instance.scale).abs() <= instance.scale * 1e-3);
        assert!((f32::from(u16_at(26)) / 10.0 - instance.age_years).abs() <= 0.05);
        for k in 0..2 {
            let tint = f32::from(i8::from_le_bytes([packed[28 + k]])) / 127.0;
            assert!((tint - instance.tint[k]).abs() <= 1.0 / 127.0);
        }
        assert_eq!(u16_at(30), instance.extra);
        assert_eq!(Instance::from_bytes(packed).to_bytes(), *packed);
    }
}

#[test]
fn a_written_tile_reads_back_and_writes_the_same_bytes() {
    let fill = &*FILL;
    let mut plan = plan(fill);
    // A meadow holds no snags: stand one, made as a caller would.
    assert!(plan.snags.is_empty());
    plan.snags.push(Snag::new(
        "pseudotsuga-menziesii",
        [3.25, 7.5, 0.0],
        SnagShape {
            height_m: 11.3,
            dbh_m: 0.42,
            top_m: 0.31,
            decay: 3,
            seed: 7,
        },
    ));
    let dir = folder("round-trip");
    let json = plan.write(&dir).expect("write");
    assert_eq!(json, dir.join("5_-3.json"));
    let header: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&json).unwrap()).expect("a JSON file");
    assert_eq!(header["schema"], INSTANCE_SCHEMA);
    assert_eq!(header["instance_bytes"], INSTANCE_BYTES);
    assert_eq!(header["instances_file"], "5_-3.bin");
    assert_eq!(header["tile"]["x"], 5);
    assert_eq!(header["instances"], plan.instances.len());
    assert_eq!(
        header["cover"].as_array().unwrap().len(),
        plan.cols * plan.cols
    );
    assert_eq!(
        header["canopy"].as_array().unwrap().len(),
        plan.cols * plan.cols
    );

    let read = TilePlan::read(&json).expect("read back");
    assert_eq!(read.tile, plan.tile);
    assert_eq!(read.cover, plan.cover);
    assert_eq!(read.canopy, plan.canopy);
    assert_eq!(read.stats, plan.stats);
    assert_eq!(read.logs, plan.logs);
    assert_eq!(read.snags, plan.snags);
    assert_eq!(read.instance_bytes(), plan.instance_bytes());
    // Written again, the files are the same bytes.
    let again = folder("round-trip-again");
    read.write(&again).expect("write again");
    assert_eq!(files(&dir), files(&again));

    // A tile of another schema is refused, not misread.
    let text = std::fs::read_to_string(&json).unwrap();
    let schema = format!("\"schema\": {INSTANCE_SCHEMA}");
    assert!(text.contains(&schema));
    std::fs::write(&json, text.replacen(&schema, "\"schema\": 3", 1)).unwrap();
    let refused = TilePlan::read(&json).expect_err("schema 3 is not read as 4");
    assert!(refused.0.contains("schema 3"), "{refused}");
    for dir in [dir, again] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_tile_read_from_its_files_draws_as_the_one_filled() {
    let fill = &*FILL;
    let plan = plan(fill);
    let dir = folder("draw");
    let written = write_tile(fill, &plan, &dir);
    // A later run with no Fill: the plan and prototypes from their files.
    let read = TilePlan::read(&dir.join("tiles").join("5_-3.json")).expect("tile");
    let (revision, listed) = read_prototypes(&dir.join("prototypes")).expect("prototypes");
    assert_eq!(listed, written);
    assert!(revision > 0);
    for instance in &read.instances {
        assert!(
            listed.iter().any(|p| p.index == instance.prototype),
            "prototype {} placed but not listed",
            instance.prototype
        );
    }
    let from_files = read
        .glb(&listed, &dir.join("prototypes"), 3, false)
        .expect("glb from files");
    let filled = std::fs::read(dir.join("tiles").join("5_-3.glb")).unwrap();
    assert!(
        from_files == filled,
        "the tile's GLB places each plant as its packed instance"
    );
    let cells: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("tiles/5_-3/cells.json")).unwrap())
            .expect("cells index");
    assert_eq!(cells["format"], "plantgen-tile-cells");
    assert_eq!(cells["schema"], 1);
    for cell in cells["cells"].as_array().unwrap() {
        for level in cell["levels"].as_array().unwrap() {
            let name = level["file"].as_str().unwrap();
            let glb = std::fs::read(dir.join("tiles").join(name)).expect("a listed cell file");
            assert_eq!(level["bytes"], glb.len());
            assert_eq!(&glb[..4], b"glTF");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_same_inputs_write_the_same_bytes() {
    let first = folder("same-a");
    let second = folder("same-b");
    let fill = measured(&[20.0]);
    write_tile(&fill, &plan(&fill), &first);
    // A fill made afresh from the same communities and settings.
    let fill = measured(&[20.0]);
    write_tile(&fill, &plan(&fill), &second);
    let (a, b) = (files(&first), files(&second));
    assert!(a.len() > 5, "{} files", a.len());
    assert_eq!(
        a.iter().map(|f| &f.0).collect::<Vec<_>>(),
        b.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    for ((name, x), (_, y)) in a.iter().zip(&b) {
        assert!(x == y, "{name} differs");
    }
    for dir in [first, second] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_prototype_is_the_same_bytes_under_its_key_in_any_fill() {
    // The second fill holds a lawn of plantain before the meadow, so its
    // yarrows' indices differ, but a key both hold draws the same plant.
    let one = &*FILL;
    let lawn = Community::from_json(
        r#"{"id": "caller-lawn", "members": [
            {"species": "plantago-major", "layer": "herb"}
        ]}"#,
    )
    .expect("community");
    let two = Fill::new(&[lawn, meadow()], &settings(&[20.0])).expect("fill");
    let (a, b) = (folder("key-a"), folder("key-b"));
    let written_a = one
        .write_prototypes(&[], &export(), 2, &a)
        .expect("prototypes");
    let written_b = two
        .write_prototypes(&[], &export(), 2, &b)
        .expect("prototypes");
    let mut shared = 0;
    for files_a in &written_a {
        let files_b = written_b
            .iter()
            .find(|f| f.key == files_a.key)
            .expect("every yarrow of the meadow in both");
        assert_ne!(files_a.index, files_b.index, "the lawn comes first");
        shared += 1;
        assert_eq!(
            files_a.plant.fingerprint, files_b.plant.fingerprint,
            "{:?}",
            files_a.key
        );
        for (level_a, level_b) in files_a.plant.levels.iter().zip(&files_b.plant.levels) {
            assert_eq!(level_a.file, level_b.file);
            let same = std::fs::read(a.join(&level_a.file)).unwrap()
                == std::fs::read(b.join(&level_b.file)).unwrap();
            assert!(same, "{} differs", level_a.file);
        }
    }
    assert!(shared > 0);
    for dir in [a, b] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn the_prototype_list_keeps_every_prototype_written_into_a_folder() {
    let fill = &*FILL;
    assert!(fill.prototypes().len() >= 2, "two seeds, two prototypes");
    let dir = folder("list");
    // Two tiles' worth, one call each, as a city writes them.
    let first = fill
        .write_prototypes(&[0], &export(), 1, &dir)
        .expect("first");
    let second = fill
        .write_prototypes(&[1], &export(), 1, &dir)
        .expect("second");
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1, "a call returns its own prototypes");
    let (_, listed) = read_prototypes(&dir).expect("list");
    let indices: Vec<u32> = listed.iter().map(|p| p.index).collect();
    assert_eq!(indices, vec![0, 1]);
    assert!(dir.join(PROTOTYPES_FILE).is_file());
    let _ = std::fs::remove_dir_all(dir);
}
