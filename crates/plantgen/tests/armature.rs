//! Stem armature in the broadleaf program (revision 27): thorns on a
//! share of the nodes, one beside each bud where they are shoots, falling
//! from older stems, and at the tips of shoots that end in one; and in the
//! herb program (revision 20), prickles at an armed herb's leafy nodes.

use plantgen::export;
use plantgen::library::Library;
use plantgen::package::Inputs;
use plantgen::quality;

/// A Nootka rose's thorn organs at `age`, grown with `params` over its
/// spec's, and their feet's distance from the end of their segment.
fn thorns(params: &[(&str, f64)], age: f64) -> Vec<f64> {
    thorns_of("rosa-nutkana", params, age)
}

/// A species' thorn organs, as [`thorns`] gives a Nootka rose's.
fn thorns_of(species: &str, params: &[(&str, f64)], age: f64) -> Vec<f64> {
    let library = Library::builtin();
    let mut spec = library.spec(species).unwrap();
    for (name, value) in params {
        spec.generator.params.insert((*name).to_string(), *value);
    }
    let inputs = Inputs::new(&spec, &quality::DRAFT).unwrap();
    let plant = export::grow(&inputs, 0, age).unwrap();
    let thorn = plant
        .organ_types
        .iter()
        .position(|organ| organ.name == "thorn")
        .unwrap();
    plant
        .graph
        .organs
        .iter()
        .filter(|organ| usize::from(organ.organ) == thorn)
        .map(|organ| {
            organ
                .segment
                .and_then(|segment| plant.graph.segments.get(segment as usize))
                .map_or(f64::INFINITY, |segment| {
                    (organ.position - segment.end).length()
                })
        })
        .collect()
}

#[test]
#[allow(clippy::cast_precision_loss)]
fn thorns_follow_their_share_pairing_and_years() {
    let every = thorns(&[], 5.0).len();
    assert!(every > 100, "{every}");
    let few = thorns(&[("thorn_share", 0.2)], 5.0).len();
    let share = few as f64 / every as f64;
    assert!((0.1..0.4).contains(&share), "{few} of {every}");
    let single = thorns(&[("thorn_pair", 0.0)], 5.0).len();
    let share = single as f64 / every as f64;
    assert!((0.35..0.65).contains(&share), "{single} of {every}");
    let kept = thorns(&[], 8.0).len();
    let young = thorns(&[("thorn_years", 2.0)], 8.0).len();
    assert!(young < kept * 3 / 4, "{young} of {kept}");
}

#[test]
fn a_shoot_that_ends_in_a_thorn_bears_it_at_its_tip() {
    let tips = thorns(&[("thorns", 0.0), ("thorn_tips", 1.0)], 5.0);
    assert!(tips.len() > 5, "{}", tips.len());
    assert!(tips.iter().all(|&apart| apart < 1.0e-6), "{tips:?}");
    assert!(thorns(&[("thorns", 0.0)], 5.0).is_empty());
}

#[test]
fn an_armed_herb_bears_prickles_at_its_leafy_nodes() {
    // An eggplant's stems are sparingly prickly (Flora of China): the
    // herb program sets two at each leafy node, which draw those between.
    let prickles = thorns_of("solanum-melongena", &[], 1.0);
    assert!(prickles.len() >= 10, "{}", prickles.len());
    assert!(thorns_of("solanum-melongena", &[("thorns", 0.0)], 1.0).is_empty());
}
