//! Motion exports (plant motion S2): every mesh skinned to the plant's
//! bones, so that at rest the skin draws the plant exactly where a static
//! export draws it; the parts riding their bones' joints; and the bones and
//! branches carried in `PLANTGEN_motion`.

use plantgen::export::{self, Built, Compression, Settings};
use plantgen::gltf::{self, Glb};
use plantgen::motion::Motion;
use plantgen::package::{self, Inputs, PackageError};
use plantgen::quality::{self, Quality};
use plantgen::spec::{Environment, PlantSpec};
use serde_json::{Map, Value};

/// A young Douglas-fir: one seed in the open, eight years.
fn small_spec() -> PlantSpec {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 8.0;
    spec.growth.keyframes = vec![8.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1];
    spec.allometry.clear();
    spec
}

const TINY: Quality = Quality {
    name: "test",
    lods: quality::DRAFT.lods,
    impostor_views: 4,
    impostor_size: 16,
};

/// A 4×4 matrix, column-major as glTF writes them.
type Matrix = [f64; 16];

const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn times(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum();
        }
    }
    out
}

fn apply(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|row| m[row] * p[0] + m[4 + row] * p[1] + m[8 + row] * p[2] + m[12 + row])
}

fn triple(value: &Value, default: f64) -> [f64; 3] {
    value.as_array().map_or([default; 3], |v| {
        std::array::from_fn(|i| v[i].as_f64().unwrap())
    })
}

/// A node's own transform.
#[allow(clippy::many_single_char_names)]
fn local(node: &Value) -> Matrix {
    let [tx, ty, tz] = triple(&node["translation"], 0.0);
    let [sx, sy, sz] = triple(&node["scale"], 1.0);
    let q = node["rotation"]
        .as_array()
        .map_or([0.0, 0.0, 0.0, 1.0], |v| {
            std::array::from_fn(|i| v[i].as_f64().unwrap())
        });
    let [x, y, z, w] = q;
    let r = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [
        r[0] * sx,
        r[1] * sx,
        r[2] * sx,
        0.0,
        r[3] * sy,
        r[4] * sy,
        r[5] * sy,
        0.0,
        r[6] * sz,
        r[7] * sz,
        r[8] * sz,
        0.0,
        tx,
        ty,
        tz,
        1.0,
    ]
}

/// Every node's transform in the scene.
fn globals(glb: &Glb<'_>) -> Vec<Matrix> {
    let nodes = glb.json["nodes"].as_array().unwrap();
    let mut out = vec![IDENTITY; nodes.len()];
    let mut stack: Vec<(usize, Matrix)> = glb.json["scenes"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|at| (usize::try_from(at.as_u64().unwrap()).unwrap(), IDENTITY))
        .collect();
    while let Some((index, parent)) = stack.pop() {
        let global = times(&parent, &local(&nodes[index]));
        out[index] = global;
        for child in nodes[index]["children"].as_array().into_iter().flatten() {
            stack.push((usize::try_from(child.as_u64().unwrap()).unwrap(), global));
        }
    }
    out
}

fn accessor(glb: &Glb<'_>, at: &Value) -> Vec<f64> {
    glb.accessor(usize::try_from(at.as_u64().unwrap()).unwrap())
        .unwrap()
        .0
}

/// The node named `name`, by index.
fn named(glb: &Glb<'_>, name: &str) -> Option<usize> {
    glb.json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|node| node["name"] == name)
}

/// Each vertex of the mesh node `name` where the file draws it at rest:
/// through its skin when it has one, else through the node's transform.
fn drawn(glb: &Glb<'_>, name: &str) -> Option<Vec<[f64; 3]>> {
    let index = named(glb, name)?;
    let node = &glb.json["nodes"][index];
    let global = globals(glb);
    let mesh = &glb.json["meshes"][usize::try_from(node["mesh"].as_u64().unwrap()).unwrap()];
    let mut out = Vec::new();
    for primitive in mesh["primitives"].as_array().unwrap() {
        let positions = accessor(glb, &primitive["attributes"]["POSITION"]);
        let positions = positions.as_chunks::<3>().0;
        if let Some(skin) = node["skin"].as_u64() {
            let skin = &glb.json["skins"][usize::try_from(skin).unwrap()];
            let joints: Vec<usize> = skin["joints"]
                .as_array()
                .unwrap()
                .iter()
                .map(|joint| usize::try_from(joint.as_u64().unwrap()).unwrap())
                .collect();
            let binds = accessor(glb, &skin["inverseBindMatrices"]);
            let binds: Vec<Matrix> = binds.as_chunks::<16>().0.to_vec();
            let joint = accessor(glb, &primitive["attributes"]["JOINTS_0"]);
            let weight = accessor(glb, &primitive["attributes"]["WEIGHTS_0"]);
            for (vertex, position) in positions.iter().enumerate() {
                assert!((weight[4 * vertex] - 1.0).abs() < 1.0e-9);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let j = joint[4 * vertex] as usize;
                let m = times(&global[joints[j]], &binds[j]);
                out.push(apply(&m, *position));
            }
        } else {
            out.extend(positions.iter().map(|p| apply(&global[index], *p)));
        }
    }
    Some(out)
}

fn bounds(points: &[[f64; 3]]) -> [f64; 6] {
    let mut out = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        for axis in 0..3 {
            out[axis] = out[axis].min(p[axis]);
            out[3 + axis] = out[3 + axis].max(p[axis]);
        }
    }
    out
}

fn near(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() <= tolerance)
}

/// Where every part node (or instance) is drawn, sorted.
fn part_places(glb: &Glb<'_>) -> Vec<[f64; 3]> {
    let global = globals(glb);
    let nodes = glb.json["nodes"].as_array().unwrap();
    let mut out = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let name = node["name"].as_str().unwrap_or("");
        if node.get("mesh").is_none() || ["wood", "cards", "spines"].contains(&name) {
            continue;
        }
        match node.pointer("/extensions/EXT_mesh_gpu_instancing/attributes/TRANSLATION") {
            Some(at) => {
                let own = accessor(glb, at);
                // The instancing node carries no transform of its own.
                out.extend(
                    own.as_chunks::<3>()
                        .0
                        .iter()
                        .map(|t| apply(&global[index], *t)),
                );
            }
            None => out.push(apply(&global[index], [0.0; 3])),
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

#[test]
fn motion_exports_rest_as_static_ones_draw_the_plant() {
    let spec = small_spec();
    let still = Inputs::new(&spec, &TINY).unwrap();
    let moving = Inputs::new(&spec, &TINY)
        .unwrap()
        .with_motion(Motion::of(&Map::new()));
    let plant = export::grow(&moving, 0, 8.0).unwrap();
    for compression in [Compression::None, Compression::Quantize] {
        let settings = Settings {
            compression,
            ..Settings::default()
        };
        let a = export::export(&still, &plant, &settings, None).unwrap();
        let b = export::export(&moving, &plant, &settings, None).unwrap();
        let index: Value = serde_json::from_slice(&b.index).unwrap();
        assert_eq!(index["profile"], "motion");
        for (level, (plain, rigged)) in a.levels.iter().zip(&b.levels).enumerate() {
            let (plain_glb, glb) = (
                Glb::read(&plain.glb).unwrap(),
                Glb::read(&rigged.glb).unwrap(),
            );
            assert!(plain_glb.json.get("skins").is_none());
            assert!(plain_glb.json.get("extensions").is_none());
            assert_eq!(gltf::check(&rigged.glb), Ok(rigged.summary));
            let record = &glb.json["extensions"]["PLANTGEN_motion"];
            let bones = record["bones"].as_array().unwrap().len();
            assert!(bones > 10);
            assert_eq!(record["ground_joint"], bones);
            assert_eq!(record["bones"][0]["kind"], "trunk");
            let skins = glb.json["skins"].as_array().unwrap();
            assert_eq!(rigged.summary.joints, skins.len() * (bones + 1));
            assert_eq!(rigged.summary.triangles, plain.summary.triangles);
            // A quantized file's positions are within half a step of the
            // plant's, and each skin's matrices round again.
            let tolerance = rigged
                .position_step
                .map_or(2.0e-5, |step| f64::from(step) * 1.5);
            let (wood, rested) = (
                drawn(&plain_glb, "wood").unwrap(),
                drawn(&glb, "wood").unwrap(),
            );
            assert_eq!(wood.len(), rested.len());
            for (want, got) in wood.iter().zip(&rested) {
                assert!(
                    near(*want, *got, tolerance),
                    "level {level}: {want:?} {got:?}"
                );
            }
            let cards = drawn(&plain_glb, "cards");
            assert_eq!(cards.is_some(), named(&glb, "cards").is_some());
            if let (Some(cards), Some(rested)) = (cards, drawn(&glb, "cards")) {
                let (want, got) = (bounds(&cards), bounds(&rested));
                assert!(
                    (0..6).all(|at| (want[at] - got[at]).abs() <= tolerance),
                    "level {level}: {want:?} {got:?}"
                );
            }
            let (want, got) = (part_places(&plain_glb), part_places(&glb));
            assert_eq!(want.len(), got.len(), "level {level}");
            for (want, got) in want.iter().zip(&got) {
                assert!(near(*want, *got, 1.0e-4), "level {level}: {want:?} {got:?}");
            }
            if level == 0 {
                assert!(!got.is_empty(), "the nearest level places needle shoots");
            }
        }
    }
}

/// A motion package's own inputs, read back from its manifest, key as it
/// does, and an export of it is checked against it.
#[test]
fn a_motion_package_exports_as_it_holds_the_plant() {
    let spec = small_spec();
    let traits = plantgen::library::Library::builtin()
        .traits_of(&spec.id)
        .unwrap();
    let inputs = Inputs::new(&spec, &TINY)
        .unwrap()
        .with_motion(Motion::of(&traits));
    let built = package::build(&inputs, 2, &mut |_| {}).unwrap();
    let program = &built.objects[&built.manifest.program.object];
    let read = export::inputs_of(
        &built.manifest,
        program,
        &TINY,
        plantgen::library::Library::builtin(),
    )
    .unwrap();
    assert_eq!(read.key, inputs.key);
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
    let plant = export::grow(&read, 0, 8.0).unwrap();
    let exported = export::export(&read, &plant, &Settings::default(), Some(&package)).unwrap();
    assert!(exported.levels.iter().all(|level| level.summary.joints > 0));
}

/// A cactus's solid spines ride its body's bones, and rest where a static
/// export draws them.
#[test]
fn a_cactus_skins_its_spines_to_its_body() {
    let mut spec = PlantSpec::builtin("echinocereus-engelmannii").unwrap();
    spec.growth.years = 4.0;
    spec.growth.keyframes = vec![4.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1];
    spec.allometry.clear();
    let still = Inputs::new(&spec, &TINY).unwrap();
    let moving = Inputs::new(&spec, &TINY)
        .unwrap()
        .with_motion(Motion::of(&Map::new()));
    let plant = export::grow(&moving, 0, 4.0).unwrap();
    let a = export::export(&still, &plant, &Settings::default(), None).unwrap();
    let b = export::export(&moving, &plant, &Settings::default(), None).unwrap();
    let (plain, glb) = (
        Glb::read(&a.levels[0].glb).unwrap(),
        Glb::read(&b.levels[0].glb).unwrap(),
    );
    let (want, got) = (
        drawn(&plain, "spines").expect("spines on the nearest level"),
        drawn(&glb, "spines").unwrap(),
    );
    assert_eq!(want.len(), got.len());
    for (want, got) in want.iter().zip(&got) {
        assert!(near(*want, *got, 2.0e-5), "{want:?} {got:?}");
    }
    let node = &glb.json["nodes"][named(&glb, "spines").unwrap()];
    let mesh = &glb.json["meshes"][usize::try_from(node["mesh"].as_u64().unwrap()).unwrap()];
    let joints = accessor(&glb, &mesh["primitives"][0]["attributes"]["JOINTS_0"]);
    let ground = glb.json["extensions"]["PLANTGEN_motion"]["ground_joint"]
        .as_f64()
        .unwrap();
    #[allow(clippy::float_cmp)]
    let on_ground = joints
        .iter()
        .step_by(4)
        .filter(|&&joint| joint == ground)
        .count();
    assert_eq!(on_ground, 0, "every spine rides a bone");
}
