//! Plant motion S2: motion packages hold every keyframe's bones and
//! branches and every level's binding to them, static packages keep their
//! key, and every built-in species' drawn plant is bound to its bones.

use plantgen::drawing::bodies_of;
use plantgen::grow::{Growth, GrowthSettings, grow};
use plantgen::lsys::Limits;
use plantgen::mesh;
use plantgen::motion::{self, Motion};
use plantgen::package::{self, Inputs};
use plantgen::quality::{self, Quality};
use plantgen::spec::{self, Environment, PlantSpec, Variant};
use serde_json::{Map, Value};

fn grow_variant(spec: &PlantSpec, variant: &Variant, years: f64) -> Growth {
    let (program, params) = spec.program().unwrap();
    grow(
        &program,
        &spec.seeded_params(&program, &params, variant.seed),
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().unwrap(),
        },
    )
    .unwrap()
}

/// A young Douglas-fir: one seed in the open, eight years.
fn small_spec() -> PlantSpec {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 8.0;
    spec.growth.keyframes = vec![8.0, 4.0];
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

/// A motion package keys apart from the static one, which keeps its key
/// and holds no motion objects; it holds one `mechanics` object per
/// keyframe and one `binding` per level, an entry per vertex and card.
#[test]
fn motion_packages_hold_bones_branches_and_bindings() {
    let spec = small_spec();
    let still = Inputs::new(&spec, &TINY).unwrap();
    let static_key = still.key.clone();
    assert_eq!(Inputs::new(&spec, &TINY).unwrap().key, static_key);
    let moving = Inputs::new(&spec, &TINY)
        .unwrap()
        .with_motion(Motion::of(&Map::new()));
    assert_ne!(moving.key, static_key);

    let built = package::build(&still, 2, &mut |_| {}).unwrap();
    assert_eq!(built.manifest.mechanics_revision, None);
    for keyframe in &built.manifest.variants[0].keyframes {
        assert!(keyframe.mechanics.is_none());
        assert!(keyframe.lods.iter().all(|lod| lod.binding.is_none()));
    }
    let manifest = String::from_utf8(built.manifest_bytes().unwrap()).unwrap();
    assert!(!manifest.contains("mechanics") && !manifest.contains("binding"));

    let built = package::build(&moving, 2, &mut |_| {}).unwrap();
    assert_eq!(
        built.manifest.mechanics_revision,
        Some(motion::MECHANICS_REVISION)
    );
    for keyframe in &built.manifest.variants[0].keyframes {
        let record: Value =
            serde_json::from_slice(&built.objects[keyframe.mechanics.as_ref().unwrap()]).unwrap();
        let bones = record["bones"].as_array().unwrap();
        assert!(!bones.is_empty());
        assert_eq!(bones[0]["kind"], "trunk");
        assert!(bones[0]["reach"].as_f64().unwrap() > 0.0);
        assert!(record["anchorage"]["moment"].as_f64().unwrap() > 0.0);
        assert!(record["perches"].is_array() && record["crossings"]["pairs"].is_u64());
        assert!(
            record["material"]["assumed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|key| key == "wood_modulus_green_mpa")
        );
        for lod in &keyframe.lods {
            let bound =
                motion::decode_binding(&built.objects[lod.binding.as_ref().unwrap()]).unwrap();
            assert_eq!(bound.wood.len(), lod.wood_vertices);
            assert_eq!(bound.cards.len(), lod.cards);
            let count = u32::try_from(bones.len()).unwrap();
            assert!(bound.wood.iter().chain(&bound.cards).all(|&packed| {
                let bone = motion::unpack(packed).0;
                bone < count || bone == motion::GROUND
            }));
        }
    }
}

/// Every built-in species, at its oldest keyframe, on its nearest and its
/// third level: every wood vertex and card is borne by a segment bound to a
/// bone, or by the ground (roots, a rosette's leaves); every branch heads
/// its own chain of bones; and the rig and its record are found in under
/// seven seconds of the thread's own CPU time (two until pinidae 9's full
/// crowns: Sitka spruce and western hemlock carry about 230,000 segments;
/// three until conifers shaded with their needles' own area, generator
/// revision 82: Sitka spruce, western hemlock and Douglas-fir now carry
/// 320,000 to 350,000 segments, and the needle-leaved conifers take 4.0 to
/// 5.5 s).
/// Wall-clock time counted every other species' thread and the CI VM's
/// other jobs too, so it crept past the limit as species were added
/// (western redcedar 3.5 s on 2026-10-16) with no rig getting slower.
#[test]
fn builtin_species_bind_every_vertex_to_a_bone() {
    let species: Vec<&str> = spec::all_species().map(|(id, _)| id).collect();
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = species
            .chunks(species.len().div_ceil(threads))
            .map(|chunk| {
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    for &id in chunk {
                        failures.extend(check_species(id));
                    }
                    failures
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn check_species(id: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let spec = PlantSpec::builtin(id).unwrap();
    let years = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
    let variant = &spec.variant_list()[0];
    let growth = grow_variant(&spec, variant, years);
    let graph = &growth.keyframes[0];
    let looks = spec.appearance.looks(
        growth
            .organ_types
            .iter()
            .map(|organ| (organ.name.as_str(), organ.kind)),
    );
    let bodies = bodies_of(&spec, &growth);
    let motion = Motion::of(&Map::new());
    let started = std::time::Instant::now();
    let cpu_started = thread_cpu_seconds();
    let found = motion.of_graph(graph, &growth.organ_types);
    let record = motion::record(&found, &motion, graph, &growth.organ_types);
    let took = match (cpu_started, thread_cpu_seconds()) {
        (Some(start), Some(end)) => end - start,
        _ => started.elapsed().as_secs_f64(),
    };
    if took > 7.0 {
        failures.push(format!("{id}: its rig and record took {took:.1} s"));
    }
    if record.anchorage.moment.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        || record
            .bones
            .iter()
            .any(|bone| bone.reach.is_nan() || bone.reach < 0.0)
    {
        failures.push(format!("{id}: no anchorage, or a bone with no reach"));
    }
    for branch in &found.branches {
        if branch.chain.first() != Some(&branch.bone) {
            failures.push(format!("{id}: a branch's chain does not start at its bone"));
        }
    }
    for level in [0, 2] {
        let lod = quality::DRAFT.lods[level].for_height(graph.height);
        let plant = mesh::build(graph, &looks, &bodies, &spec.appearance, &lod, level);
        let bound = motion::bind(&plant, graph, &found);
        let unborne_above = (0..plant.wood.vertex_count())
            .filter(|&at| plant.wood.bearer(at) == mesh::NO_SEGMENT)
            .count();
        let unborne_cards = plant
            .cards
            .iter()
            .filter(|card| card.segment == mesh::NO_SEGMENT)
            .count();
        if unborne_above > 0 || unborne_cards > 0 || bound.unborne > 0 {
            failures.push(format!(
                "{id} level {level}: {unborne_above} wood vertices and {unborne_cards} cards \
                 are borne by nothing, {} bound to no bone",
                bound.unborne
            ));
        }
        let wood_bound = (0..plant.wood.vertex_count())
            .filter_map(|at| plant.wood.segment(at))
            .filter(|&segment| found.binding[segment as usize].is_none())
            .count();
        if wood_bound > 0 {
            failures.push(format!(
                "{id} level {level}: {wood_bound} wood vertices are on segments no bone binds"
            ));
        }
        assert_eq!(bound.wood.len(), plant.wood.vertex_count());
    }
    failures
}

/// The calling thread's user and system CPU time in seconds, from
/// /proc/thread-self/stat (in clock ticks, 100 a second on Linux), or None
/// where there is no such file and the caller falls back to wall-clock time.
fn thread_cpu_seconds() -> Option<f64> {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").ok()?;
    let mut fields = stat
        .get(stat.rfind(')')? + 2..)?
        .split_whitespace()
        .skip(11);
    let user: u32 = fields.next()?.parse().ok()?;
    let system: u32 = fields.next()?.parse().ok()?;
    Some((f64::from(user) + f64::from(system)) / 100.0)
}
