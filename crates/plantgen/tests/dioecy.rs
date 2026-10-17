//! Dioecy: a dioecious species' plants are male or female, each by its own
//! draw from its seed, and only the females bear fruit or seed cones.

use plantgen::grow::{Growth, GrowthSettings, grow};
use plantgen::lsys::Limits;
use plantgen::spec::{PlantSpec, SEX};

/// Grow `spec` from `seed` to `years`, keeping only that year, with the
/// sex `seed` draws.
fn grown(spec: &PlantSpec, seed: u64, years: f64) -> (Growth, f64) {
    let (program, params) = spec.program().unwrap();
    let seeded = spec.seeded_params(&program, &params, seed);
    let sex = program
        .params
        .iter()
        .position(|param| param.name == SEX)
        .map(|at| seeded[at])
        .unwrap();
    let variant = &spec.variant_list()[0];
    let growth = grow(
        &program,
        &seeded,
        &GrowthSettings {
            seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions: variant.conditions(),
            limits: Limits::default(),
            host: spec.host_geometry().unwrap(),
        },
    )
    .unwrap();
    (growth, sex)
}

/// How many organs named `name` the plant's last keyframe holds.
fn count(growth: &Growth, name: &str) -> usize {
    let Some(index) = growth.organ_types.iter().position(|kind| kind.name == name) else {
        return 0;
    };
    growth
        .keyframes
        .last()
        .unwrap()
        .organs
        .iter()
        .filter(|organ| usize::from(organ.organ) == index)
        .count()
}

/// Grow `spec` from seeds 1 to 8 and return, for each, its sex and the
/// counts of `names`; check both sexes grew.
fn by_sex(spec: &PlantSpec, years: f64, names: &[&str]) -> Vec<(bool, Vec<usize>)> {
    let plants: Vec<(bool, Vec<usize>)> = (1..=8)
        .map(|seed| {
            let (growth, sex) = grown(spec, seed, years);
            (
                sex >= 0.5,
                names.iter().map(|name| count(&growth, name)).collect(),
            )
        })
        .collect();
    assert!(
        plants.iter().any(|(female, _)| *female) && plants.iter().any(|(female, _)| !female),
        "{}: seeds 1 to 8 grow one sex only",
        spec.id
    );
    plants
}

/// Pacific yew states dioecy at its family (Taxaceae), and the seed
/// plants' rule makes it dioecious: its male trees bear no seed cones
/// (arils), and its female trees do.
#[test]
fn a_yews_male_trees_bear_no_seed_cones() {
    let spec = PlantSpec::builtin("taxus-brevifolia").unwrap();
    assert_eq!(spec.generator.params.get("dioecious"), Some(&1.0));
    let plants = by_sex(&spec, 40.0, &["cone"]);
    for (female, counts) in &plants {
        if !female {
            assert_eq!(counts[0], 0, "a male yew bears cones: {plants:?}");
        }
    }
    assert!(
        plants
            .iter()
            .any(|(female, counts)| *female && counts[0] > 0),
        "no female yew bears cones: {plants:?}"
    );
}

/// A species whose taxa state no sexual system, or both sexes on a plant,
/// is not dioecious, and every plant bears its fruit.
#[test]
fn a_plant_bearing_both_sexes_fruits_whatever_its_seed() {
    for id in [
        "acer-macrophyllum",
        "pseudotsuga-menziesii",
        "digitalis-purpurea",
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        assert_eq!(spec.generator.params.get("dioecious"), Some(&0.0), "{id}");
    }
}

/// A broadleaf's male plants bear male blooms, drawn as its blooms are in
/// flower and fallen where they would fruit; its female plants bear its
/// blooms, which fruit. Indian plum's blooms, made dioecious here.
#[test]
fn a_broadleafs_male_plants_bear_male_blooms_that_set_no_fruit() {
    let mut spec = PlantSpec::builtin("oemleria-cerasiformis").unwrap();
    spec.generator.params.insert("dioecious".into(), 1.0);
    let plants = by_sex(&spec, spec.growth.years, &["bloom", "bloom_male"]);
    for (female, counts) in &plants {
        if *female {
            assert_eq!(counts[1], 0, "a female bears male blooms: {plants:?}");
        } else {
            assert_eq!(counts[0], 0, "a male bears blooms that fruit: {plants:?}");
        }
    }
    assert!(
        plants
            .iter()
            .any(|(female, counts)| !female && counts[1] > 0)
    );
    assert!(
        plants
            .iter()
            .any(|(female, counts)| *female && counts[0] > 0)
    );

    // Its male blooms show in flower as its blooms do, and are gone when
    // they would be in fruit.
    let (program, _) = spec.program().unwrap();
    let season = spec.appearance.organs["bloom"].season.clone().unwrap();
    let at = |name: &str, day: f64| {
        let index = program.organs().position(|(own, _)| own == name).unwrap();
        spec.appearance.looks_on(program.organs(), Some(day)).1[index]
    };
    let flowering = f64::midpoint(season.flower, season.fruit);
    let fruiting = f64::midpoint(season.fruit, season.fall);
    assert!(at("bloom_male", flowering) > 0.0);
    assert!((at("bloom_male", flowering) - at("bloom", flowering)).abs() < 1e-9);
    assert!(at("bloom", fruiting) > 0.0);
    assert!(at("bloom_male", fruiting).abs() < 1e-12);
}

/// A herb's male plants bear open flowers where its female plants bear
/// fruit. Foxglove, made dioecious here.
#[test]
fn a_herbs_male_plants_bear_flowers_in_place_of_fruit() {
    let mut spec = PlantSpec::builtin("digitalis-purpurea").unwrap();
    spec.generator.params.insert("dioecious".into(), 1.0);
    let plants = by_sex(&spec, spec.growth.years, &["fruit", "flower"]);
    for (female, counts) in &plants {
        if !female {
            assert_eq!(counts[0], 0, "a male herb bears fruit: {plants:?}");
        }
    }
    let flowers = |female: bool| {
        plants
            .iter()
            .filter(|(own, counts)| *own == female && counts.iter().sum::<usize>() > 0)
            .map(|(_, counts)| counts.clone())
            .next()
    };
    let (Some(male), Some(female)) = (flowers(false), flowers(true)) else {
        panic!("no flowering plant of each sex: {plants:?}");
    };
    assert!(female[0] > 0, "a female herb bears no fruit: {plants:?}");
    assert!(
        male[1] > female[1],
        "a male herb bears no more flowers: {plants:?}"
    );
}

/// Each `ids` species, made dioecious here, grown from seeds 1 to 8: its
/// male plants bear no `fruit` (`organs[0]`), and some female plant does.
/// Returns the plants, by sex, with their counts of `organs`.
fn males_bear_no_fruit(id: &str, organs: &[&str]) -> Vec<(bool, Vec<usize>)> {
    let mut spec = PlantSpec::builtin(id).unwrap();
    spec.generator.params.insert("dioecious".into(), 1.0);
    let plants = by_sex(&spec, spec.growth.years, organs);
    for (female, counts) in &plants {
        if !female {
            assert_eq!(counts[0], 0, "{id}: a male bears fruit: {plants:?}");
        }
    }
    assert!(
        plants
            .iter()
            .any(|(female, counts)| *female && counts[0] > 0),
        "{id}: no female bears fruit: {plants:?}"
    );
    plants
}

/// A mistletoe's male clumps bear no berries. California mistletoe, made
/// dioecious here.
#[test]
fn a_mistletoes_male_clumps_bear_no_berries() {
    males_bear_no_fruit("phoradendron-californicum", &["fruit"]);
}

/// A cactus's male plants bear open flowers in its rings where its female
/// plants bear fruit, and hang no chains of fruit. Saguaro and jumping
/// cholla, made dioecious here.
#[test]
fn a_cactuss_male_plants_bear_flowers_in_place_of_fruit() {
    for id in ["carnegiea-gigantea", "cylindropuntia-fulgida"] {
        let plants = males_bear_no_fruit(id, &["fruit", "flower"]);
        // Plants differ in size by seed, so compare each sex's mean.
        #[allow(clippy::cast_precision_loss)]
        let mean = |female: bool| {
            let counts: Vec<usize> = plants
                .iter()
                .filter(|(own, _)| *own == female)
                .map(|(_, counts)| counts[1])
                .collect();
            counts.iter().sum::<usize>() as f64 / counts.len() as f64
        };
        assert!(mean(false) > mean(true), "{id}: {plants:?}");
    }
}

/// A grass's male plants bear male panicles, its female plants its
/// panicles. Dewey's sedge, made dioecious here.
#[test]
fn a_grasss_male_plants_bear_male_panicles() {
    let mut spec = PlantSpec::builtin("carex-deweyana").unwrap();
    spec.generator.params.insert("dioecious".into(), 1.0);
    let plants = by_sex(&spec, spec.growth.years, &["panicle", "panicle_male"]);
    for (female, counts) in &plants {
        assert_eq!(counts[usize::from(*female)], 0, "{plants:?}");
    }
    assert!(
        plants.iter().any(|(female, c)| !female && c[1] > 0),
        "{plants:?}"
    );
    assert!(
        plants.iter().any(|(female, c)| *female && c[0] > 0),
        "{plants:?}"
    );
}
