//! Ranges become variation (spec rework VARY): a rule that reads one range
//! records its value's spread, and each organ drawn solid is one of its
//! look's varieties, which together draw every value across its spread.

use std::collections::BTreeSet;

use plantgen::inherit::Spread;
use plantgen::looks::{self, Look, Shape, VARIETIES};
use plantgen::spec::{self, PlantSpec};

/// The look of the organ type `organ` of `spec`'s program.
fn look(spec: &PlantSpec, organ: &str) -> Look {
    let (program, _) = spec.program().unwrap();
    spec.appearance
        .looks(program.organs())
        .into_iter()
        .find(|look| look.organ == organ)
        .unwrap()
}

#[test]
fn a_heads_varieties_draw_its_rays_across_their_range() {
    // Yarrow: 5 to 8 rays, rarely 3; 10 to 20 disc florets; 10 to 100
    // heads in a corymb.
    let spec = PlantSpec::builtin("achillea-millefolium").unwrap();
    let head = look(&spec, "flower");
    assert_eq!(head.varieties.len(), VARIETIES);
    let Shape::Head(own) = &head.shape else {
        panic!("yarrow's flower is drawn as a head");
    };
    assert_eq!(own.rays, 7, "the look itself keeps the typical value");
    let counts = |count: fn(&Look) -> u32| -> Vec<u32> {
        let mut counts: Vec<u32> = head.varieties.iter().map(count).collect();
        counts.sort_unstable();
        counts
    };
    let rays = |look: &Look| match &look.shape {
        Shape::Head(head) => head.rays,
        _ => 0,
    };
    let florets = |look: &Look| match &look.shape {
        Shape::Head(head) => head.florets,
        _ => 0,
    };
    let heads = |look: &Look| {
        look.inflorescence
            .as_ref()
            .map_or(0, |cluster| cluster.cluster.flowers)
    };
    assert_eq!(counts(rays), [5, 6, 7, 8]);
    let florets = counts(florets);
    assert!(
        florets[0] >= 10 && florets[3] <= 20 && florets[0] < florets[3],
        "{florets:?}"
    );
    let heads = counts(heads);
    assert!(
        heads[0] >= 10 && heads[3] <= 100 && heads[0] < heads[3],
        "{heads:?}"
    );
    // An organ is drawn as the variety its id picks, and each variety
    // draws organs.
    let picked: BTreeSet<usize> = (0..64).map(looks::variety_of).collect();
    assert_eq!(picked.len(), VARIETIES);
    for id in 0..64 {
        assert_eq!(head.of(id), &head.varieties[looks::variety_of(id)]);
    }
}

#[test]
fn a_rule_on_two_ranges_keeps_its_value() {
    // Orchardgrass: its spikelet's florets come from one range and vary;
    // its lemma, as long against the spikelet as the flora's two lengths
    // say, reads two, so it keeps its one share.
    let spec = PlantSpec::builtin("dactylis-glomerata").unwrap();
    let spreads = |section: &serde_json::Map<String, serde_json::Value>| -> Vec<String> {
        Spread::all(section)
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    };
    let shape = spreads(&spec.appearance.spreads);
    assert!(
        shape.contains(&"organs.panicle.shape.florets".to_string()),
        "{shape:?}"
    );
    assert!(
        !shape.contains(&"organs.panicle.shape.lemma".to_string()),
        "{shape:?}"
    );
    assert!(
        !shape.iter().any(|path| path.ends_with(".aspect")),
        "{shape:?}"
    );
    // Its leaf and panicle lengths, parameters set from one range each,
    // are sizes of parts, which vary part by part: no plant draws them.
    assert!((spec.generator.params["leaf_length"] - 0.275).abs() < 1e-12);
    assert!(
        spec.generator.spreads.is_empty(),
        "{:?}",
        spec.generator.spreads
    );
}

#[test]
fn each_plant_draws_its_parameters_from_their_spreads() {
    // Red alder comes of age at 3 to 10 years. Its package grows one
    // plant, the typical one; any other seed draws its own age by its hash,
    // the same on every machine.
    let spec = PlantSpec::builtin("alnus-rubra").unwrap();
    let (program, params) = spec.program().unwrap();
    let at = |program: &plantgen::lsys::Program| {
        program
            .params
            .iter()
            .position(|param| param.name == "adult_age")
            .unwrap()
    };
    assert_eq!(spec.variants.seeds, [1]);
    assert_eq!(spec.seeded_params(&program, &params, 1), params);
    let ages: Vec<f64> = (2..=9)
        .map(|seed| spec.seeded_params(&program, &params, seed)[at(&program)])
        .collect();
    let distinct: BTreeSet<u64> = ages.iter().map(|age| age.to_bits()).collect();
    assert!(distinct.len() >= 6, "{ages:?}");
    assert!(
        ages.iter().all(|age| (3.0..=10.0).contains(age)),
        "{ages:?}"
    );
    assert_eq!(
        spec.seeded_params(&program, &params, 4)[at(&program)].to_bits(),
        ages[2].to_bits()
    );
    // Creosote bush comes of age at 4 to 13 years, typically 8.5. Its
    // package grows three plants, which come of age at the middles of the
    // range's thirds.
    let spec = PlantSpec::builtin("larrea-tridentata").unwrap();
    let (program, params) = spec.program().unwrap();
    let mut ages: Vec<f64> = spec
        .variants
        .seeds
        .iter()
        .map(|seed| spec.seeded_params(&program, &params, *seed)[at(&program)])
        .collect();
    ages.sort_by(f64::total_cmp);
    assert_eq!(ages.len(), 3);
    for (age, middle) in ages.iter().zip([5.5, 8.5, 11.5]) {
        assert!((age - middle).abs() < 1e-9, "{ages:?}");
    }
    // A part's size a parameter takes varies part by part, so no plant
    // draws it: a teddy-bear cholla's joints. Nor one a formula works out
    // from a range, the program's tuning at the typical value: lenga
    // beech's growth units, from the leaves a vigorous shoot bears.
    for (id, param) in [
        ("cylindropuntia-bigelovii", "joint_length"),
        ("nothofagus-pumilio", "unit_nodes"),
    ] {
        let spec = PlantSpec::builtin(id).unwrap();
        assert!(spec.generator.params.contains_key(param), "{id}");
        assert!(
            spec.generator.spreads.is_empty(),
            "{id}: {:?}",
            spec.generator.spreads
        );
    }
    // Redwood sorrel states no range a parameter is set from.
    let spec = PlantSpec::builtin("oxalis-oregana").unwrap();
    let (program, params) = spec.program().unwrap();
    assert_eq!(spec.seeded_params(&program, &params, 3), params);
}

#[test]
fn only_looks_drawn_solid_from_ranges_vary() {
    let mut count = 0;
    for (id, _) in spec::all_species() {
        let spec = PlantSpec::builtin(id).unwrap();
        let (program, _) = spec.program().unwrap();
        let organs = spec
            .appearance
            .spreads
            .get("organs")
            .and_then(serde_json::Value::as_object);
        // The looks on no day, then on a day in each month: a stage's look
        // (a bloom's flowers) is drawn, and varies, only on its days.
        let mut days = vec![spec.appearance.looks(program.organs())];
        for month in 0..12 {
            let day = 15.0 + 30.0 * f64::from(month);
            days.push(spec.appearance.looks_on(program.organs(), Some(day)).0);
        }
        let mut varying = BTreeSet::new();
        for (day, looks) in days.iter().enumerate() {
            for look in looks {
                let spread = organs.and_then(|organs| organs.get(&look.organ));
                if look.varieties.is_empty() {
                    if day == 0 {
                        typical_in_every_variety(id, look, spread);
                    }
                    continue;
                }
                varying.insert(look.organ.clone());
                assert!(
                    spread.is_some() && look.draws_solid(),
                    "{id} `{}`",
                    look.organ
                );
                assert_eq!(look.varieties.len(), VARIETIES, "{id} `{}`", look.organ);
                let typical = Look {
                    varieties: Vec::new(),
                    ..look.clone()
                };
                assert!(
                    look.varieties.iter().any(|variety| variety != &typical),
                    "{id} `{}`: its varieties are all the look itself",
                    look.organ
                );
            }
        }
        count += varying.len();
        // Every spread holds five values, its middle the value it spreads.
        for (section, spreads) in [
            ("appearance", &spec.appearance.spreads),
            ("generator", &spec.generator.spreads),
        ] {
            let effective = serde_json::to_value(&spec).unwrap();
            for (path, spread) in Spread::all(spreads) {
                let at = format!("/{section}/{}", path.replace('.', "/"));
                let value = effective.pointer(&at).and_then(serde_json::Value::as_f64);
                assert_eq!(value, Some(spread.values[2]), "{id} {section}.{path}");
            }
        }
    }
    assert!(count >= 30, "only {count} looks vary");
}

/// A look drawn solid with spreads (`spread`, its organ's) but no
/// varieties draws each spread's typical value in every variety; a
/// spread in its season's stage looks varies only on the stage's days.
fn typical_in_every_variety(id: &str, look: &Look, spread: Option<&serde_json::Value>) {
    let spread = spread
        .filter(|_| look.draws_solid())
        .and_then(serde_json::Value::as_object);
    for (path, spread) in spread.map(Spread::all).unwrap_or_default() {
        if path.starts_with("season.") {
            continue;
        }
        let path = format!("organs.{}.{path}", look.organ);
        for variety in 0..VARIETIES {
            let share = looks::variety_share(variety, &path);
            let at = spread.at(share);
            assert!((at - spread.values[2]).abs() < 1e-9, "{id} {path}: {at}");
        }
    }
}
