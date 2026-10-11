//! `plantc export`: GLB files that draw each level exactly as the package
//! holds it, the same bytes every time, and the bark's shade kept apart or
//! multiplied in.

use plantgen::export::{self, Built, Shade};
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
        let exported = export::export(&inputs, &plant, Shade::Attribute, Some(&package)).unwrap();
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
    let error = export::export(&inputs, &plant, Shade::Attribute, Some(&package)).unwrap_err();
    assert!(error.to_string().contains("no keyframe"), "{error}");
    let mut plant = export::grow(&inputs, 0, 8.0).unwrap();
    plant.variant = 1;
    let error = export::export(&inputs, &plant, Shade::Attribute, Some(&package)).unwrap_err();
    assert!(error.to_string().contains("variant"), "{error}");
}

#[test]
fn exports_are_the_same_bytes_every_time() {
    let inputs = Inputs::new(&small_spec(), &TINY).unwrap();
    let first = export::export(
        &inputs,
        &export::grow(&inputs, 1, 8.0).unwrap(),
        Shade::Attribute,
        None,
    )
    .unwrap();
    let again = export::export(
        &inputs,
        &export::grow(&inputs, 1, 8.0).unwrap(),
        Shade::Attribute,
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
    let apart = export::export(&inputs, &plant, Shade::Attribute, None).unwrap();
    let baked = export::export(&inputs, &plant, Shade::Baked, None).unwrap();
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
    let exported = export::export(&inputs, &plant, Shade::Attribute, None).unwrap();
    let nearest = Glb::read(&exported.levels[0].glb).unwrap();
    let spines = mesh(&nearest, "spines").expect("spines on the nearest level");
    assert!(attribute(&nearest, spines, "POSITION").unwrap().len() > 3 * 100);
    let farthest = Glb::read(&exported.levels[TINY.lods.len() - 1].glb).unwrap();
    assert!(mesh(&farthest, "spines").is_none());
}
