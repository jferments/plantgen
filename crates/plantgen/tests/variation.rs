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
    // A parameter set from one range records its spread, which no plant
    // draws from yet.
    assert_eq!(
        spreads(&spec.generator.spreads),
        ["params.leaf_length", "params.panicle_length"]
    );
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
