//! What a plant's organs are and what they belong to (spec rework M7).
//!
//! A program's organ types say how each organ is drawn (`foliage`, `leaf`,
//! `flower`, `fruit` or `cone`), not what it is: a grass blade is drawn as
//! six `blade` organs and a `blade_tip`, a sedge's glomerule as a `leaf`.
//! Each organ type is therefore given a **term**, one of the vocabulary's
//! organs (`traits.json`), or [`PIECE`] for a drawn piece of something
//! larger:
//!
//! 1. its look's, where the species' look draws a group, which names it
//!    more exactly than its program can: an `inflorescence` is an
//!    inflorescence, a lone `head` a capitulum, a lone `spikelet` a
//!    spikelet; but a piece stays a piece;
//! 2. else the program's, where its declaration says `as term`;
//! 3. else its drawing kind's: `foliage` and `leaf` a leaf, `flower` a
//!    flower, `fruit` a fruit, `cone` a cone.
//!
//! A program also declares **structures** (`structure frond leaf;`), each
//! with a term, and opens one by drawing its name: every segment and organ
//! drawn from there to the end of the branch is within it
//! ([`crate::graph::GraphStructure`]). [`disagreements`] checks a grown
//! graph against the vocabulary's `part_of`.

use crate::graph::{GraphStructure, PlantGraph};
use crate::looks::{Look, Shape};
use crate::lsys::OrganKind;
use crate::traits::Vocabulary;

/// The term of a drawn piece of a structure, such as a piece of a grass
/// blade: not an organ of its own.
pub const PIECE: &str = "piece";

/// The vocabulary's organ named `name` as a term an organ type may be
/// given, or [`PIECE`]; `None` for the whole plant and for any other name.
#[must_use]
pub fn organ_term(name: &str) -> Option<&'static str> {
    if name == PIECE {
        return Some(PIECE);
    }
    structure_term(name)
}

/// The vocabulary's organ named `name` as a term a structure may be given;
/// `None` for the whole plant and for any other name.
#[must_use]
pub fn structure_term(name: &str) -> Option<&'static str> {
    let vocabulary = Vocabulary::builtin();
    vocabulary
        .organs
        .get_key_value(name)
        .filter(|(_, organ)| organ.part_of.is_some())
        .map(|(key, _)| key.as_str())
}

/// The terms a program may give, for its error messages: every organ of
/// the vocabulary but the whole plant.
#[must_use]
pub fn known_terms() -> Vec<&'static str> {
    Vocabulary::builtin()
        .organs
        .iter()
        .filter(|(_, organ)| organ.part_of.is_some())
        .map(|(name, _)| name.as_str())
        .collect()
}

/// What an organ drawn with `look` is when its program does not say: a
/// look that draws a group names it (see the module), else `None`.
#[must_use]
pub fn of_look(look: &Look) -> Option<&'static str> {
    if look.inflorescence.is_some() {
        return Some("inflorescence");
    }
    match look.shape {
        Shape::Head(_) => Some("capitulum"),
        Shape::Spikelet(_) => Some("spikelet"),
        _ => None,
    }
}

/// What an organ drawn as `kind` is when neither its program nor its look
/// says.
#[must_use]
pub fn of_kind(kind: OrganKind) -> &'static str {
    match kind {
        OrganKind::Foliage | OrganKind::Leaf => "leaf",
        OrganKind::Flower => "flower",
        OrganKind::Fruit => "fruit",
        OrganKind::Cone => "cone",
    }
}

/// Each organ type's term, in declaration order: its look's where the
/// look draws a group and the program does not call it a piece, else the
/// program's (`explicit`, `None` where it gives none), else its kind's.
/// `looks` holds one look per organ type first, as
/// [`crate::looks::resolve`] gives them.
#[must_use]
pub fn terms(
    kinds: &[OrganKind],
    explicit: &[Option<&'static str>],
    looks: &[Look],
) -> Vec<&'static str> {
    kinds
        .iter()
        .enumerate()
        .map(|(index, &kind)| {
            let program = explicit.get(index).copied().flatten();
            let look = looks.get(index).and_then(of_look);
            match (program, look) {
                (Some(PIECE), _) => PIECE,
                (_, Some(look)) => look,
                (Some(program), None) => program,
                (None, None) => of_kind(kind),
            }
        })
        .collect()
}

/// Whether something termed `term` may be within a structure termed
/// `structure`: a piece within anything, anything else within an organ it
/// is part of (`part_of`, followed up to the whole plant).
#[must_use]
pub fn may_be_within(term: &str, structure: &str) -> bool {
    if term == PIECE {
        return true;
    }
    let organs = &Vocabulary::builtin().organs;
    let mut next = organs.get(term).and_then(|organ| organ.part_of.as_deref());
    // The vocabulary's check rules out a part_of cycle; the bound is for
    // safety only.
    for _ in 0..organs.len() {
        match next {
            Some(name) if name == structure => return true,
            Some(name) => next = organs.get(name).and_then(|organ| organ.part_of.as_deref()),
            None => return false,
        }
    }
    false
}

/// Where a grown graph disagrees with the vocabulary, one line each: a
/// piece within no structure, or an organ or a structure within a
/// structure that is not something its term is part of (a leaflet belongs
/// in a leaf, not in an inflorescence). Something within no structure is
/// part of the axis it stands on, which any organ may be. `organ_types`
/// and `structure_types` give each type's name and term.
#[must_use]
pub fn disagreements(
    graph: &PlantGraph,
    organ_types: &[(&str, &str)],
    structure_types: &[(&str, &str)],
) -> Vec<String> {
    let mut found = Vec::new();
    let within_term = |within: Option<u32>| {
        within
            .and_then(|index| graph.structures.get(index as usize))
            .and_then(|structure: &GraphStructure| {
                structure_types.get(usize::from(structure.structure))
            })
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut check = |what: &str, (name, term): (&str, &str), within: Option<u32>| {
        let problem = match within_term(within) {
            None if within.is_some() => Some(format!(
                "{what} `{name}` is within a structure the graph does not hold"
            )),
            None if term == PIECE => Some(format!(
                "{what} `{name}` is a piece, but is drawn within no structure"
            )),
            None => None,
            Some(&(outer, outer_term)) => (!may_be_within(term, outer_term)).then(|| {
                format!(
                    "{what} `{name}` is {}, but is drawn within structure `{outer}`, {}",
                    article(term),
                    article(outer_term)
                )
            }),
        };
        if let Some(problem) = problem
            && seen.insert(problem.clone())
        {
            found.push(problem);
        }
    };
    for organ in &graph.organs {
        if let Some(&kind) = organ_types.get(usize::from(organ.organ)) {
            check("organ", kind, organ.within);
        }
    }
    for structure in &graph.structures {
        if let Some(&kind) = structure_types.get(usize::from(structure.structure)) {
            check("structure", kind, structure.within);
        }
    }
    found
}

/// A term with its article: "a leaf", "an inflorescence".
fn article(term: &str) -> String {
    let an = term.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {}", if an { "an" } else { "a" }, term.replace('_', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_the_vocabularys_organs_and_piece() {
        assert_eq!(organ_term("leaflet"), Some("leaflet"));
        assert_eq!(organ_term("piece"), Some(PIECE));
        assert_eq!(structure_term("piece"), None);
        assert_eq!(
            organ_term("plant"),
            None,
            "the whole plant is no organ's term"
        );
        assert_eq!(organ_term("frond"), None);
        assert_eq!(organ_term("spine"), Some("spine"), "a thorn or a prickle");
        assert!(known_terms().contains(&"spikelet"));
    }

    #[test]
    fn an_organ_belongs_within_what_it_is_part_of() {
        assert!(may_be_within("leaflet", "leaf"));
        assert!(may_be_within("ray_floret", "capitulum"));
        assert!(
            may_be_within("ray_floret", "inflorescence"),
            "through the head"
        );
        assert!(may_be_within(PIECE, "inflorescence"));
        assert!(!may_be_within("leaflet", "inflorescence"));
        assert!(!may_be_within("leaf", "leaf"));
    }

    #[test]
    fn a_graph_is_checked_against_part_of() {
        use crate::graph::GraphOrgan;
        use crate::math::Vec3;
        let organ = |organ: u16, within: Option<u32>| GraphOrgan {
            id: 0,
            organ,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 1.0,
            born: 0.0,
            shed: None,
            light: 1.0,
            within,
        };
        let structure = |structure: u16, within: Option<u32>| GraphStructure {
            id: 0,
            structure,
            within,
            born: 0.0,
        };
        let organ_types = [
            ("pinna", "leaflet"),
            ("scrap", PIECE),
            ("head", "capitulum"),
        ];
        let structure_types = [("frond", "leaf"), ("cluster", "inflorescence")];
        let mut graph = PlantGraph {
            age: 1.0,
            height: 1.0,
            segments: Vec::new(),
            organs: vec![
                organ(0, Some(0)),
                organ(1, Some(1)),
                organ(2, Some(1)),
                organ(2, None),
            ],
            structures: vec![structure(0, None), structure(1, None)],
        };
        assert!(disagreements(&graph, &organ_types, &structure_types).is_empty());
        // A leaflet in an inflorescence, a piece within nothing, a frond
        // within a frond; each said once however often it occurs.
        graph
            .organs
            .extend([organ(0, Some(1)), organ(0, Some(1)), organ(1, None)]);
        graph.structures.push(structure(0, Some(0)));
        assert_eq!(
            disagreements(&graph, &organ_types, &structure_types),
            [
                "organ `pinna` is a leaflet, but is drawn within structure `cluster`, an \
                 inflorescence",
                "organ `scrap` is a piece, but is drawn within no structure",
                "structure `frond` is a leaf, but is drawn within structure `frond`, a leaf",
            ]
        );
    }

    #[test]
    fn terms_come_from_the_look_then_the_program_then_the_kind() {
        use crate::looks::{Head, Shape, resolve};
        use crate::lsys::OrganKind::{Flower, Fruit, Leaf};
        let organs = [
            ("pinna", Leaf),
            ("bloom", Flower),
            ("bud", Fruit),
            ("scrap", Fruit),
            ("tuft", Leaf),
        ];
        let kinds = organs.map(|(_, kind)| kind);
        let mut looks = resolve(
            organs,
            &std::collections::BTreeMap::new(),
            [0.0; 3],
            [0.0; 3],
        );
        // A species draws its buds and its scraps as heads.
        for look in &mut looks[2..4] {
            look.shape = Shape::Head(Head::default());
        }
        assert_eq!(
            terms(
                &kinds,
                &[Some("leaflet"), None, Some("flower"), Some(PIECE), None],
                &looks
            ),
            ["leaflet", "flower", "capitulum", PIECE, "leaf"]
        );
    }
}
