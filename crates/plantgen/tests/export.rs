//! `plantc export`: GLB files that draw each level exactly as the package
//! holds it, the same bytes every time, and the bark's shade kept apart or
//! multiplied in.

use plantgen::export::{self, Attributes, Built, Compression, Settings, Shade};
use plantgen::gltf::{self, Glb};
use plantgen::package::{self, Inputs, PackageError};
use plantgen::quality::{self, Quality};
use plantgen::spec::{Environment, PlantSpec};
use serde_json::Value;

/// A young Douglas-fir: two seeds in the open, eight years.
fn small_spec() -> PlantSpec {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 8.0;
    spec.growth.keyframes = vec![8.0, 4.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1, 2];
    spec.allometry.clear();
    spec
}

const TINY: Quality = Quality {
    name: "test",
    lods: quality::DRAFT.lods,
    impostor_views: 4,
    impostor_size: 16,
};

/// The mesh named `name` in a GLB file's document, and the file read.
fn mesh<'a>(glb: &'a Glb<'_>, name: &str) -> Option<&'a Value> {
    glb.json
        .get("meshes")?
        .as_array()?
        .iter()
        .find(|mesh| mesh.get("name").and_then(Value::as_str) == Some(name))
}

/// Attribute `name` of a mesh's first primitive, if it has one.
fn attribute(glb: &Glb<'_>, mesh: &Value, name: &str) -> Option<Vec<f64>> {
    let at = mesh.pointer(&format!("/primitives/0/attributes/{name}"))?;
    let index = usize::try_from(at.as_u64()?).ok()?;
    Some(glb.accessor(index).unwrap().0)
}

#[test]
fn every_level_is_drawn_as_its_package_holds_it() {
    let spec = small_spec();
    let inputs = Inputs::new(&spec, &TINY).unwrap();
    let built = package::build(&inputs, 2, &mut |_| {}).unwrap();
    let object = |hash: &str| {
        built
            .objects
            .get(hash)
            .cloned()
            .ok_or_else(|| PackageError::Format(format!("no object {hash}")))
    };
    let package = Built {
        manifest: &built.manifest,
        object: &object,
    };
    for (variant, age) in [(0, 8.0), (1, 4.0)] {
        let plant = export::grow(&inputs, variant, age).unwrap();
        let exported =
            export::export(&inputs, &plant, &Settings::default(), Some(&package)).unwrap();
        assert_eq!(exported.levels.len(), TINY.lods.len());
        for (level, file) in exported.levels.iter().enumerate() {
            assert_eq!(file.file, format!("{}-lod{level}.glb", exported.stem));
            assert!(file.summary.triangles > 0);
            assert_eq!(gltf::check(&file.glb), Ok(file.summary));
            let glb = Glb::read(&file.glb).unwrap();
            let extras = glb.json.pointer("/scenes/0/extras/plantgen").unwrap();
            assert_eq!(extras["package_key"], inputs.key.as_str());
            assert_eq!(extras["level"], level);
            assert_eq!(extras["age"], age);
        }
        // Needle shoots are parts on the nearest level alone.
        let parts = |level: usize| {
            Glb::read(&exported.levels[level].glb)
                .unwrap()
                .json
                .pointer("/nodes")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .any(|node| node.get("name").and_then(Value::as_str) == Some("parts"))
        };
        assert!(parts(0));
        assert!(!parts(1));
        let index: Value = serde_json::from_slice(&exported.index).unwrap();
        assert_eq!(index["files"].as_array().unwrap().len(), TINY.lods.len());
        assert_eq!(index["species"], spec.id.as_str());
    }

    // An age the package keeps no keyframe at, or another seed's plant, is
    // refused.
    let plant = export::grow(&inputs, 0, 6.0).unwrap();
    let error = export::export(&inputs, &plant, &Settings::default(), Some(&package)).unwrap_err();
    assert!(error.to_string().contains("no keyframe"), "{error}");
    let mut plant = export::grow(&inputs, 0, 8.0).unwrap();
    plant.variant = 1;
    let error = export::export(&inputs, &plant, &Settings::default(), Some(&package)).unwrap_err();
    assert!(error.to_string().contains("variant"), "{error}");
}

#[test]
fn exports_are_the_same_bytes_every_time() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let first = export::export(
        &inputs,
        &export::grow(&inputs, 1, 8.0).unwrap(),
        &Settings::default(),
        None,
    )
    .unwrap();
    let again = export::export(
        &inputs,
        &export::grow(&inputs, 1, 8.0).unwrap(),
        &Settings::default(),
        None,
    )
    .unwrap();
    assert_eq!(first, again);
    assert_eq!(first.stem, "pseudotsuga-menziesii-open-2-8y");
}

#[test]
fn baked_shade_is_the_attribute_multiplied_in() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let plant = export::grow(&inputs, 0, 8.0).unwrap();
    let apart = export::export(&inputs, &plant, &Settings::default(), None).unwrap();
    let baked = Settings {
        shade: Shade::Baked,
        ..Settings::default()
    };
    let baked = export::export(&inputs, &plant, &baked, None).unwrap();
    for (apart, baked) in apart.levels.iter().zip(&baked.levels) {
        let (apart, baked) = (
            Glb::read(&apart.glb).unwrap(),
            Glb::read(&baked.glb).unwrap(),
        );
        let (wood, baked_wood) = (mesh(&apart, "wood").unwrap(), mesh(&baked, "wood").unwrap());
        let shades = attribute(&apart, wood, "_PLANTGEN_SHADE").unwrap();
        assert!(attribute(&baked, baked_wood, "_PLANTGEN_SHADE").is_none());
        // The tree's own crown shades some of its wood.
        assert!(shades.iter().all(|&shade| (0.2..=1.0).contains(&shade)));
        assert!(shades.iter().any(|&shade| shade < 1.0));
        let colours = attribute(&apart, wood, "COLOR_0").unwrap();
        let baked_colours = attribute(&baked, baked_wood, "COLOR_0").unwrap();
        assert_eq!(colours.len(), 3 * shades.len());
        for (vertex, &shade) in shades.iter().enumerate() {
            for channel in 0..3 {
                let at = 3 * vertex + channel;
                assert!((colours[at] * shade - baked_colours[at]).abs() < 1e-6);
            }
        }
        assert_eq!(
            attribute(&apart, wood, "POSITION"),
            attribute(&baked, baked_wood, "POSITION")
        );
    }
}

#[test]
fn a_cactus_exports_its_spines_as_solids_on_the_nearest_level() {
    let mut spec = PlantSpec::builtin("echinocereus-engelmannii").unwrap();
    spec.growth.years = 4.0;
    spec.growth.keyframes = vec![4.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1];
    spec.allometry.clear();
    let inputs = Inputs::new(&spec, &TINY).unwrap();
    let plant = export::grow(&inputs, 0, 4.0).unwrap();
    let exported = export::export(&inputs, &plant, &Settings::default(), None).unwrap();
    let nearest = Glb::read(&exported.levels[0].glb).unwrap();
    let spines = mesh(&nearest, "spines").expect("spines on the nearest level");
    assert!(attribute(&nearest, spines, "POSITION").unwrap().len() > 3 * 100);
    let farthest = Glb::read(&exported.levels[TINY.lods.len() - 1].glb).unwrap();
    assert!(mesh(&farthest, "spines").is_none());
}

/// The node named `name` in a GLB file's document.
fn node<'a>(glb: &'a Glb<'_>, name: &str) -> &'a Value {
    glb.json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node.get("name").and_then(Value::as_str) == Some(name))
        .unwrap()
}

/// The values of the accessor `at` names.
fn values(glb: &Glb<'_>, at: &Value) -> Vec<f64> {
    glb.accessor(usize::try_from(at.as_u64().unwrap()).unwrap())
        .unwrap()
        .0
}

/// A number the file writes in single precision, as written.
fn single(value: &Value) -> f64 {
    #[allow(clippy::cast_possible_truncation)]
    f64::from(value.as_f64().unwrap() as f32)
}

/// Each value of `got` is within `tolerance` of `want`'s.
fn close(got: &[f64], want: &[f64], tolerance: f64) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(got, want)| (got - want).abs() <= tolerance)
}

/// Plain and quantized exports of the same young Douglas-fir.
fn plain_and_quantized() -> (export::Export, export::Export) {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let plant = export::grow(&inputs, 0, 8.0).unwrap();
    let plain = export::export(&inputs, &plant, &Settings::default(), None).unwrap();
    let settings = Settings {
        compression: Compression::Quantize,
        ..Settings::default()
    };
    let quantized = export::export(&inputs, &plant, &settings, None).unwrap();
    (plain, quantized)
}

#[test]
fn quantized_files_draw_the_same_plant_in_fewer_bytes() {
    let (plain, quantized) = plain_and_quantized();
    for (level, (plain, quantized)) in plain.levels.iter().zip(&quantized.levels).enumerate() {
        assert_eq!(plain.summary.triangles, quantized.summary.triangles);
        assert_eq!(plain.summary.instances, quantized.summary.instances);
        assert!(quantized.glb.len() < plain.glb.len());
        assert_eq!(plain.position_step, None);
        let step = f64::from(quantized.position_step.unwrap());
        // Eight years of a Douglas-fir is a metre or two: steps of tens of
        // micrometres.
        assert!(step > 0.0 && step < 1.0e-4, "{step}");
        let (a, b) = (
            Glb::read(&plain.glb).unwrap(),
            Glb::read(&quantized.glb).unwrap(),
        );
        assert!(a.json.get("extensionsRequired").is_none());
        let required = b.json["extensionsRequired"].as_array().unwrap();
        assert!(required.contains(&Value::from("KHR_mesh_quantization")));
        #[allow(clippy::cast_possible_truncation)]
        let recorded = b
            .json
            .pointer("/scenes/0/extras/plantgen/position_step")
            .and_then(Value::as_f64)
            .map(|step| step as f32);
        assert_eq!(recorded, quantized.position_step);
        for name in ["wood", "cards"] {
            let (Some(plain_mesh), Some(quantized_mesh)) = (mesh(&a, name), mesh(&b, name)) else {
                assert!(mesh(&a, name).is_none() && mesh(&b, name).is_none());
                continue;
            };
            let grid = node(&b, name);
            let origin: Vec<f64> = grid["translation"]
                .as_array()
                .unwrap()
                .iter()
                .map(single)
                .collect();
            let scale = single(&grid["scale"][0]);
            assert!(scale <= step);
            let primitives = |mesh: &Value| mesh["primitives"].as_array().unwrap().clone();
            let (plain_primitives, quantized_primitives) =
                (primitives(plain_mesh), primitives(quantized_mesh));
            assert_eq!(plain_primitives.len(), quantized_primitives.len());
            for (p, q) in plain_primitives.iter().zip(&quantized_primitives) {
                assert_eq!(values(&a, &p["indices"]), values(&b, &q["indices"]));
                let stored = values(&b, &q["attributes"]["POSITION"]);
                let placed: Vec<f64> = stored
                    .iter()
                    .enumerate()
                    .map(|(at, &value)| origin[at % 3] + scale * value)
                    .collect();
                let want = values(&a, &p["attributes"]["POSITION"]);
                assert!(
                    close(&placed, &want, scale * 0.5 + 1.0e-7),
                    "{name} {level}"
                );
                for (attribute, tolerance) in [
                    ("NORMAL", 0.5 / 127.0 + 1.0e-6),
                    ("COLOR_0", 0.5 / 65535.0 + 1.0e-7),
                    ("TEXCOORD_0", 0.5 / 65535.0 + 1.0e-7),
                    ("_PLANTGEN_BIRTH", 0.0),
                    ("_PLANTGEN_SHED", 0.0),
                    ("_PLANTGEN_WIND", 0.0),
                    ("_PLANTGEN_RADIUS", 0.0),
                    ("_PLANTGEN_SHADE", 0.0),
                ] {
                    let (Some(p), Some(q)) = (
                        p["attributes"].get(attribute),
                        q["attributes"].get(attribute),
                    ) else {
                        assert_eq!(
                            p["attributes"].get(attribute).is_some(),
                            q["attributes"].get(attribute).is_some(),
                            "{name}'s {attribute}"
                        );
                        continue;
                    };
                    assert!(
                        close(&values(&b, q), &values(&a, p), tolerance),
                        "{name}'s {attribute} on level {level}"
                    );
                }
            }
        }
    }
}

#[test]
fn quantized_files_draw_parts_as_instances() {
    let (plain, quantized) = plain_and_quantized();
    // The nearest level's needle shoots: a node each in plain files, a node
    // per part mesh drawing them as instances in quantized ones.
    let (a, b) = (
        Glb::read(&plain.levels[0].glb).unwrap(),
        Glb::read(&quantized.levels[0].glb).unwrap(),
    );
    let shoots = |glb: &Glb<'_>| {
        let nodes = glb.json["nodes"].as_array().unwrap();
        node(glb, "parts")["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|at| nodes[usize::try_from(at.as_u64().unwrap()).unwrap()]["mesh"].clone())
            .collect::<Vec<_>>()
    };
    let (each, instanced) = (shoots(&a), shoots(&b));
    let mut meshes = each.clone();
    meshes.sort_by_key(Value::as_u64);
    meshes.dedup();
    assert!(
        each.len() > 4 * meshes.len(),
        "{} {}",
        each.len(),
        meshes.len()
    );
    assert_eq!(instanced, meshes);
    let required = b.json["extensionsRequired"].as_array().unwrap();
    assert!(required.contains(&Value::from("EXT_mesh_gpu_instancing")));
    assert!(quantized.levels[0].summary.nodes < plain.levels[0].summary.nodes / 4);
    let index: Value = serde_json::from_slice(&quantized.index).unwrap();
    assert_eq!(index["compression"], "quantize");
    assert_eq!(
        index["files"][0]["instances"],
        plain.levels[0].summary.instances
    );
}

#[test]
fn growth_and_wind_attributes_can_be_left_out() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let plant = export::grow(&inputs, 0, 8.0).unwrap();
    for (attributes, kept) in [
        (
            Attributes::All,
            &[
                "_PLANTGEN_BIRTH",
                "_PLANTGEN_SHED",
                "_PLANTGEN_WIND",
                "_PLANTGEN_RADIUS",
            ][..],
        ),
        (
            Attributes::Wind,
            &["_PLANTGEN_WIND", "_PLANTGEN_RADIUS"][..],
        ),
        (Attributes::None, &[][..]),
    ] {
        for compression in [Compression::None, Compression::Quantize] {
            let settings = Settings {
                compression,
                attributes,
                ..Settings::default()
            };
            let exported = export::export(&inputs, &plant, &settings, None).unwrap();
            let glb = Glb::read(&exported.levels[0].glb).unwrap();
            let wood = mesh(&glb, "wood").unwrap();
            for name in [
                "_PLANTGEN_BIRTH",
                "_PLANTGEN_SHED",
                "_PLANTGEN_WIND",
                "_PLANTGEN_RADIUS",
            ] {
                assert_eq!(
                    attribute(&glb, wood, name).is_some(),
                    kept.contains(&name),
                    "{name} with {}",
                    attributes.name()
                );
            }
            // The bark's shade is the colour's, not a growth attribute.
            assert!(attribute(&glb, wood, "_PLANTGEN_SHADE").is_some());
            assert_eq!(
                glb.json["scenes"][0]["extras"]["plantgen"]["attributes"],
                attributes.name()
            );
        }
    }
}

#[test]
fn meshopt_files_hold_the_quantized_arrays_compressed() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let plant = export::grow(&inputs, 0, 8.0).unwrap();
    let export = |compression| {
        let settings = Settings {
            compression,
            ..Settings::default()
        };
        export::export(&inputs, &plant, &settings, None).unwrap()
    };
    let (quantized, compressed) = (export(Compression::Quantize), export(Compression::Meshopt));
    for (quantized, compressed) in quantized.levels.iter().zip(&compressed.levels) {
        assert_eq!(quantized.summary, compressed.summary);
        assert_eq!(quantized.position_step, compressed.position_step);
        assert!(compressed.glb.len() * 3 < quantized.glb.len() * 2);
        let (a, b) = (
            Glb::read(&quantized.glb).unwrap(),
            Glb::read(&compressed.glb).unwrap(),
        );
        let required = b.json["extensionsRequired"].as_array().unwrap();
        assert!(required.contains(&Value::from("EXT_meshopt_compression")));
        assert_eq!(
            b.json
                .pointer("/buffers/1/extensions/EXT_meshopt_compression/fallback"),
            Some(&Value::Bool(true))
        );
        // Every array reads back as the quantized file holds it; triangles
        // at most turned.
        let indices: Vec<usize> = a.json["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|mesh| mesh["primitives"].as_array().unwrap().clone())
            .map(|primitive| usize::try_from(primitive["indices"].as_u64().unwrap()).unwrap())
            .collect();
        for accessor in 0..a.json["accessors"].as_array().unwrap().len() {
            let (want, got) = (
                a.accessor(accessor).unwrap().0,
                b.accessor(accessor).unwrap().0,
            );
            if !indices.contains(&accessor) {
                assert_eq!(want, got, "accessor {accessor}");
                continue;
            }
            assert_eq!(want.len(), got.len());
            for (want, got) in want.as_chunks::<3>().0.iter().zip(got.as_chunks::<3>().0) {
                let turns = [
                    [want[0], want[1], want[2]],
                    [want[1], want[2], want[0]],
                    [want[2], want[0], want[1]],
                ];
                assert!(turns.contains(&[got[0], got[1], got[2]]));
            }
        }
    }
    let index: Value = serde_json::from_slice(&compressed.index).unwrap();
    assert_eq!(index["compression"], "meshopt");
}
