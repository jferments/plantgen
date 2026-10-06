//! `.afterplant` packages: reproducible bytes, keys that follow every
//! input, lossless objects and detected corruption.

use std::fs;
use std::path::{Path, PathBuf};

use after_plants::grow::{GrowthSettings, grow};
use after_plants::lsys::Limits;
use after_plants::package::{self, Inputs, PackageError};
use after_plants::quality::{self, Quality};
use after_plants::spec::{AllometryPoint, Environment, PlantSpec, builtin_program};

/// A young Douglas-fir: two seeds in the open, eight years.
fn small_spec() -> PlantSpec {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 8.0;
    spec.growth.keyframes = vec![8.0, 4.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1, 2];
    spec.allometry = vec![AllometryPoint {
        age: 6.0,
        environment: Environment::Open,
        height: 4.0,
        dbh: Some(0.05),
        tolerance: 0.9,
    }];
    spec
}

const TINY: Quality = Quality {
    name: "test",
    lods: quality::DRAFT.lods,
    impostor_views: 4,
    impostor_size: 16,
};

fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("after-plants-{name}-{}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    path
}

#[test]
fn packages_are_byte_identical_whatever_the_thread_count() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let one = package::build(&inputs, 1, &mut |_| {}).unwrap();
    let three = package::build(&inputs, 3, &mut |_| {}).unwrap();
    assert_eq!(one, three);
    assert_eq!(
        one.manifest_bytes().unwrap(),
        three.manifest_bytes().unwrap()
    );
    assert_eq!(one.manifest.key, inputs.key);

    // The package keeps the spec's ages in order, without the extra age
    // grown only to compare with the reference size.
    let manifest = &one.manifest;
    assert_eq!(manifest.variants.len(), 2);
    for variant in &manifest.variants {
        let ages: Vec<f64> = variant
            .keyframes
            .iter()
            .map(|keyframe| keyframe.age)
            .collect();
        assert_eq!(ages, vec![4.0, 8.0]);
        for keyframe in &variant.keyframes {
            assert_eq!(keyframe.lods.len(), 4);
            // Coarser levels never have more wood or more cards.
            for pair in keyframe.lods.windows(2) {
                assert!(pair[1].wood_triangles <= pair[0].wood_triangles);
                assert!(pair[1].cards <= pair[0].cards);
            }
            assert_eq!(keyframe.impostor.lod, package::IMPOSTOR_LOD);
        }
    }
    assert_eq!(manifest.validation.len(), 2);
    assert!(
        manifest
            .validation
            .iter()
            .all(|record| (record.age - 6.0).abs() < 1e-12)
    );
    // Every object the manifest names is in the package, and nothing else.
    assert_eq!(
        manifest.objects.keys().collect::<Vec<_>>(),
        one.objects.keys().collect::<Vec<_>>()
    );
}

#[test]
fn the_key_follows_every_input() {
    let spec = small_spec();
    let key = |spec: &PlantSpec, quality: &Quality| Inputs::new(spec, quality).unwrap().key;
    let base = key(&spec, &TINY);
    assert_eq!(base, key(&small_spec(), &TINY));
    assert_eq!(base.len(), 64);

    let mut changed = spec.clone();
    changed.generator.params.insert("whorl".into(), 4.0);
    assert_ne!(base, key(&changed, &TINY));
    let mut changed = spec.clone();
    changed.variants.seeds = vec![1, 3];
    assert_ne!(base, key(&changed, &TINY));
    let mut changed = spec.clone();
    changed.appearance.bark[0] = 0.2;
    assert_ne!(base, key(&changed, &TINY));
    assert_ne!(
        base,
        key(
            &spec,
            &Quality {
                impostor_size: 32,
                ..TINY
            }
        )
    );
    let source = format!("{}\n# a comment\n", builtin_program("conifer").unwrap());
    assert_ne!(
        base,
        Inputs::with_program(&spec, &source, &TINY).unwrap().key
    );
    assert!(Inputs::with_program(&spec, "lsystem broken", &TINY).is_err());
}

#[test]
fn written_packages_read_back_losslessly_and_detect_corruption() {
    let spec = small_spec();
    let inputs = Inputs::new(&spec, &TINY).unwrap();
    let built = package::build(&inputs, 2, &mut |_| {}).unwrap();
    let out = scratch("write");
    assert!(package::existing(&out, &spec.id, &inputs.key).is_none());
    let path = package::write(&built, &out).unwrap();
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        inputs.directory_name()
    );
    assert_eq!(
        package::existing(&out, &spec.id, &inputs.key),
        Some(path.clone())
    );
    // Writing again leaves the package alone; no temporary directory is left.
    assert_eq!(package::write(&built, &out).unwrap(), path);
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);

    let manifest = package::verify(&path).unwrap();
    assert_eq!(manifest, built.manifest);
    assert_eq!(
        fs::read(path.join(package::MANIFEST)).unwrap(),
        built.manifest_bytes().unwrap()
    );

    // Graphs and meshes decode and encode to the same bytes, and the graph
    // matches the grown plant to 32-bit precision.
    let variant = &manifest.variants[0];
    let growth = grow(
        &spec.program().unwrap().0,
        &spec.program().unwrap().1,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years: spec.growth.years,
            keyframes: vec![4.0, 8.0],
            neighbourhood: variant.neighbourhood,
            limits: Limits::default(),
            host: None,
        },
    )
    .unwrap();
    for (keyframe, grown) in variant.keyframes.iter().zip(&growth.keyframes) {
        let bytes = package::read_object(&path, &keyframe.graph).unwrap();
        let graph = package::decode_graph(&bytes).unwrap();
        assert_eq!(package::encode_graph(&graph), bytes);
        assert_eq!(graph.segments.len(), grown.segments.len());
        assert_eq!(graph.organs.len(), grown.organs.len());
        for (decoded, original) in graph.segments.iter().zip(&grown.segments) {
            assert_eq!(
                (decoded.id, decoded.parent, decoded.lateral),
                (original.id, original.parent, original.lateral)
            );
            assert!((decoded.end - original.end).length() < 1e-5);
            assert!((decoded.radius - original.radius).abs() <= original.radius * 1e-6);
            assert_eq!(decoded.shed.is_some(), original.shed.is_some());
        }
        for lod in &keyframe.lods {
            let bytes = package::read_object(&path, &lod.mesh).unwrap();
            let mesh = package::decode_mesh(&bytes).unwrap();
            assert_eq!(package::encode_mesh(&mesh), bytes);
            assert_eq!(mesh.cards.len(), lod.cards);
            assert_eq!(mesh.wood.triangle_count(), lod.wood_triangles);
        }
        for image in [&keyframe.impostor.albedo, &keyframe.impostor.normal_depth] {
            let bytes = package::read_object(&path, image).unwrap();
            assert_eq!(&bytes[1..4], b"PNG");
        }
    }

    // One flipped byte in any object fails verification.
    let (sha, _) = manifest
        .objects
        .iter()
        .find(|(_, object)| object.kind == "mesh")
        .unwrap();
    let object = path.join(package::OBJECTS).join(sha);
    let mut bytes = fs::read(&object).unwrap();
    bytes[20] ^= 1;
    fs::write(&object, bytes).unwrap();
    let error = package::verify(&path).unwrap_err();
    assert!(
        error.to_string().contains("does not match its hash"),
        "{error}"
    );
    fs::remove_dir_all(&out).unwrap();
}

#[test]
fn decoders_reject_damaged_objects() {
    let spec = small_spec();
    let inputs = Inputs::new(&spec, &TINY).unwrap();
    let built = package::build(&inputs, 2, &mut |_| {}).unwrap();
    let keyframe = &built.manifest.variants[0].keyframes[1];
    let graph = &built.objects[&keyframe.graph];
    let mesh = &built.objects[&keyframe.lods[0].mesh];
    let format_error =
        |result: Result<(), PackageError>| matches!(result, Err(PackageError::Format(_)));

    assert!(format_error(
        package::decode_graph(&graph[..graph.len() - 1]).map(|_| ())
    ));
    assert!(format_error(package::decode_graph(mesh).map(|_| ())));
    assert!(format_error(
        package::decode_mesh(&mesh[..mesh.len() - 4]).map(|_| ())
    ));
    assert!(format_error(package::decode_mesh(graph).map(|_| ())));

    // The first segment claiming itself as its parent.
    let mut cyclic = graph.clone();
    cyclic[32..36].copy_from_slice(&0_u32.to_le_bytes());
    let error = package::decode_graph(&cyclic).unwrap_err();
    assert!(error.to_string().contains("parent"), "{error}");

    // An index past the last vertex: the first index of the wood follows
    // its 57-byte vertices and padding.
    let vertices = u32::from_le_bytes(mesh[8..12].try_into().unwrap()) as usize;
    let first_index = (16 + vertices * 57).next_multiple_of(4);
    let mut pointing_out = mesh.clone();
    pointing_out[first_index..first_index + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let error = package::decode_mesh(&pointing_out).unwrap_err();
    assert!(
        error.to_string().contains("past the last vertex"),
        "{error}"
    );

    // A count the bytes cannot hold is refused before anything is
    // allocated for it.
    let mut huge = mesh.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(format_error(package::decode_mesh(&huge).map(|_| ())));
}
