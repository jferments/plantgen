//! The stable library API (`plantgen::api`, skill tree E3) and mass
//! variants (E4): any seed grows a plant, the seeds a spec lists grow its
//! package's plants bit for bit, seed n is the same plant however many are
//! made, and the plants of a set are distinct.

use plantgen::api::{self, Environment, Generator, Options, PlantSpec, Variants};
use plantgen::export;
use plantgen::package::Inputs;
use plantgen::quality::{self, Quality};

const TINY: Quality = Quality {
    name: "test",
    lods: quality::DRAFT.lods,
    impostor_views: 4,
    impostor_size: 16,
};

/// A young Douglas-fir: seeds 1 and 2 in the open, eight years.
fn small_spec() -> PlantSpec {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 8.0;
    spec.growth.keyframes = vec![8.0, 4.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![1, 2];
    spec.allometry.clear();
    spec
}

fn options() -> Options {
    let mut options = Options::default();
    options.quality = TINY;
    options
}

#[test]
fn the_api_lists_the_built_in_species() {
    let species = api::species();
    assert!(species.len() > 100, "{} species", species.len());
    assert!(species.iter().any(|id| id == "achillea-millefolium"));
    assert!(Generator::new("no-such-plant", &options()).is_err());
}

#[test]
fn a_listed_seed_grows_the_plant_its_package_keeps() {
    let spec = small_spec();
    let generator = Generator::of(&spec, &options()).unwrap();
    let inputs = Inputs::new(&spec, &TINY).unwrap();
    let packaged = export::export(
        &inputs,
        &export::grow(&inputs, 1, 8.0).unwrap(),
        &api::Settings::default(),
        None,
    )
    .unwrap();
    let plant = generator.grow_in(Environment::Open, 2, 8.0).unwrap();
    assert_eq!(plant.variant, 1);
    let through_api = generator.export(&plant, &api::Settings::default()).unwrap();
    assert_eq!(through_api, packaged);
}

#[test]
fn any_seed_grows_its_own_plant_the_same_every_time() {
    let generator = Generator::of(&small_spec(), &options()).unwrap();
    let grow = |seed| {
        let plant = generator.grow_in(Environment::Open, seed, 8.0).unwrap();
        generator.export(&plant, &api::Settings::default()).unwrap()
    };
    let seven = grow(7);
    assert_eq!(seven, grow(7));
    assert_eq!(seven.stem, "pseudotsuga-menziesii-open-7-8y");
    assert_ne!(seven.levels[0].glb, grow(8).levels[0].glb);
    assert_ne!(seven.levels[0].glb, grow(1).levels[0].glb);
    // Not a variant its package keeps.
    let plant = generator.grow_in(Environment::Open, 7, 8.0).unwrap();
    assert_eq!(plant.variant, small_spec().variant_list().len());
    // Past its old ages (doubling from 8 years to its 500-year lifespan,
    // the last 256).
    assert!(generator.grow_in(Environment::Open, 7, 300.0).is_err());
}

#[test]
fn a_plant_grows_on_past_its_years_and_one_growth_keeps_each_age() {
    let generator = Generator::of(&small_spec(), &options()).unwrap();
    let both = generator
        .grow_ages_in(Environment::Open, 7, &[4.0, 16.0])
        .unwrap();
    assert_eq!(both.len(), 2);
    assert!(both[1].size.height > both[0].size.height);
    // The younger is the plant grown to its age alone, size for size.
    let alone = generator.grow_in(Environment::Open, 7, 4.0).unwrap();
    assert_eq!(both[0].size, alone.size);
    assert!(
        generator
            .grow_ages_in(Environment::Open, 7, &[16.0, 4.0])
            .is_err()
    );
}

#[test]
fn a_set_of_variants_is_distinct_and_each_seed_keeps_its_plant() {
    let generator = Generator::of(&small_spec(), &options()).unwrap();
    let out = std::env::temp_dir().join(format!("plantgen-api-variants-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let mut four = Variants::count(4);
    four.age = Some(8.0);
    four.threads = 2;
    let set = generator.write_variants(&four, &out.join("four")).unwrap();
    assert_eq!(
        set.plants
            .iter()
            .map(|plant| plant.seed)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(set.duplicates.is_empty(), "{:?}", set.duplicates);
    assert_eq!(
        set.plants
            .iter()
            .map(|plant| plant.packaged)
            .collect::<Vec<_>>(),
        [true, true, false, false]
    );
    assert_eq!(set.generator_revision, plantgen::GENERATOR_REVISION);
    assert_eq!(set.api_version, api::API_VERSION);
    for plant in &set.plants {
        assert_eq!(plant.levels.len(), export::LEVELS);
        for level in &plant.levels {
            let bytes = std::fs::read(out.join("four").join(&level.file)).unwrap();
            assert_eq!(bytes.len(), level.bytes);
        }
        assert!(out.join("four").join(&plant.index).is_file());
    }
    let listed: api::VariantSet =
        serde_json::from_slice(&std::fs::read(out.join("four").join(api::VARIANTS_FILE)).unwrap())
            .unwrap();
    assert_eq!(listed.species, set.species);
    assert_eq!(listed.key, set.key);
    assert_eq!(
        listed
            .plants
            .iter()
            .map(|plant| &plant.fingerprint)
            .collect::<Vec<_>>(),
        set.plants
            .iter()
            .map(|plant| &plant.fingerprint)
            .collect::<Vec<_>>()
    );

    // Seeds 3 and 4 again, on one thread and from seed 3: the same plants.
    let mut two = four;
    two.count = 2;
    two.first_seed = 3;
    two.threads = 1;
    let again = generator.write_variants(&two, &out.join("two")).unwrap();
    assert_eq!(again.plants[..], set.plants[2..]);
    let _ = std::fs::remove_dir_all(&out);
}
