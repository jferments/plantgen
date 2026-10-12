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
//!
//! An organ with a season ([`crate::looks::Season`]) goes through
//! **phases**, from primordium to bud, open, fruit and shed ([`Phase`],
//! [`phase`]). Its phase is never stored: the day and the season decide
//! it, as they decide what its look draws. An organ continues through a
//! rule that rewrites it into another, as a flower into its fruit, so it
//! keeps one id all its life (`crate::lsys::CompiledRule::continuation`).
//!
//! What a drawn organ holds, a cluster's flowers, heads or spikelets, a
//! head's phyllaries and florets, a spikelet's glumes, florets and awns,
//! is never stored either: its layout gives it when asked ([`parts`]),
//! numbered as the layout lays it out, so a part has a name ([`name`]:
//! "inflorescence 12 › capitulum 7 › ray floret 3") and a plant a
//! census by term ([`census`]).

use crate::capitula;
use crate::clusters::Unit;
use crate::graph::{GraphOrgan, GraphStructure, PlantGraph};
use crate::looks::{Head, HeadKind, Look, Season, Shape, Spikelet, Stage};
use crate::lsys::{OrganKind, Program};
use crate::spec::Appearance;
use crate::spikelets::{self, Bract};
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

/// Each of `program`'s organ types' terms ([`terms`]) as `appearance`
/// draws them, whatever the day: a cluster a stage draws whole, as a
/// catkin in flower, is still an inflorescence.
#[must_use]
pub fn of_program(program: &Program, appearance: &Appearance) -> Vec<&'static str> {
    let kinds: Vec<OrganKind> = program.organs().map(|(_, kind)| kind).collect();
    let explicit: Vec<Option<&'static str>> = program.organ_terms().collect();
    terms(&kinds, &explicit, &appearance.looks(program.organs()))
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

/// Where an organ with a season is in its life on a day (organ phases,
/// spec rework M7). A head's florets each have one of their own, its
/// `open` (`crate::looks::Head`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Grown but not yet showing: before its season's bud day.
    Primordium,
    Bud,
    Open,
    /// Fruit, `unripe` of it still unripe, as its look draws it.
    Fruit {
        unripe: f64,
    },
    /// Past its season's fall, or shed from the plant.
    Shed,
}

/// Organ `organ`'s phase on day `day` of the plant's year `age` (years):
/// shed if it was shed by `age`, else what its `season`'s stage is that
/// day, telling the days before its bud from those after its fall; `None`
/// for an organ without a season, which has no phases (a leaf, a cone).
/// A season is one year's: a program that grows a step a year rewrites
/// its flowers each year, so each stands for that year's.
#[must_use]
pub fn phase(organ: &GraphOrgan, season: Option<&Season>, day: f64, age: f64) -> Option<Phase> {
    if organ.shed.is_some_and(|shed| shed <= age) {
        return Some(Phase::Shed);
    }
    let season = season?;
    Some(staged(season.stage(day), season, day))
}

/// The phase of something in `stage` of `season` on `day`.
fn staged(stage: Stage, season: &Season, day: f64) -> Phase {
    match stage {
        Stage::Gone if season.fallen(day) => Phase::Shed,
        Stage::Gone => Phase::Primordium,
        Stage::Bud => Phase::Bud,
        Stage::Flower => Phase::Open,
        Stage::Fruit { unripe } => Phase::Fruit { unripe },
    }
}

/// One part a drawn organ holds by its layout (spec rework M7B): a
/// cluster's unit, a head's phyllary or floret, a spikelet's glume,
/// floret or awn.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The vocabulary's organ it is.
    pub term: &'static str,
    /// Its phase, where it has one: a cluster's unit's in its own stage, a
    /// head's floret's as the head has opened, else its holder's.
    pub phase: Option<Phase>,
    /// What it holds.
    pub parts: Vec<Part>,
}

/// The parts of `organ`, drawn with `look` (its look on a day, as
/// [`crate::looks::resolve_on`] gives it) and in `phase` that day, in
/// the order its layout lays them out: a cluster's units as
/// [`crate::clusters::layout`] places them on the organ, each with what
/// it holds; a head's phyllaries from the outermost row, its rays round
/// the rim, then its disc florets or straps from the centre out; a
/// spikelet's glumes from the lowest, then its florets from the lowest
/// up, each with its awn. Nothing for an organ its look draws whole: a
/// leaf, a lone flower or fruit, a cone, a cluster in bud as one bud.
#[must_use]
pub fn parts(organ: &GraphOrgan, look: &Look, phase: Option<Phase>) -> Vec<Part> {
    let Some(cluster) = &look.inflorescence else {
        return held(&look.shape, phase);
    };
    let unit = cluster.unit;
    cluster
        .laid_on(organ)
        .units
        .iter()
        .map(|p| match &cluster.opening {
            // Every unit in the organ's stage, drawn as its look is.
            None => Part {
                term: unit_term(unit, phase),
                phase,
                parts: held(&look.shape, phase),
            },
            Some(opening) => {
                let season = &opening.season;
                let own = staged(season.unit_stage(opening.day, p.rank), season, opening.day);
                Part {
                    term: unit_term(unit, Some(own)),
                    phase: Some(own),
                    parts: opening
                        .showing(p)
                        .map_or_else(Vec::new, |(unit, _)| held(&unit.shape, Some(own))),
                }
            }
        })
        .collect()
}

/// What a cluster's unit is in `phase`: a head or a spikelet in any; else
/// a flower in bud or open, a fruit in fruit, and otherwise what its look
/// draws.
fn unit_term(unit: Unit, phase: Option<Phase>) -> &'static str {
    match (unit, phase) {
        (Unit::Head, _) => "capitulum",
        (Unit::Spikelet { .. }, _) => "spikelet",
        (_, Some(Phase::Fruit { .. }))
        | (Unit::Fruit { .. }, None | Some(Phase::Primordium | Phase::Shed)) => "fruit",
        _ => "flower",
    }
}

/// The parts something drawn as `shape` in `phase` holds: a head's, a
/// spikelet's or a compound leaf's.
fn held(shape: &Shape, phase: Option<Phase>) -> Vec<Part> {
    match shape {
        Shape::Head(head) => in_head(head),
        Shape::Spikelet(spikelet) => in_spikelet(spikelet, phase),
        Shape::Compound(compound) => in_compound(compound, phase),
        _ => Vec::new(),
    }
}

/// A compound leaf's leaflets in the order its layout lays them out
/// (`crate::divided`): the terminal one, then its pairs from the foot up;
/// division by division in a twice-divided leaf. A dissected blade's
/// segments are not leaflets: it holds none.
fn in_compound(compound: &crate::looks::Compound, phase: Option<Phase>) -> Vec<Part> {
    let laid = crate::divided::layout(compound);
    if laid.form.confluent > 0.0 {
        return Vec::new();
    }
    laid.leaflets
        .iter()
        .map(|_| Part {
            term: "leaflet",
            phase,
            parts: Vec::new(),
        })
        .collect()
}

/// A head's parts: its florets each in the stage its `open` gives it,
/// its rays furled in bud, out from the moment it opens, fallen once
/// half its disc is in seed (`crate::capitula`).
fn in_head(head: &Head) -> Vec<Part> {
    let laid = capitula::layout(head, 0.0, 0.0);
    let part = |term, phase| Part {
        term,
        phase,
        parts: Vec::new(),
    };
    let ray = match head.open {
        open if open <= 0.0 => Phase::Bud,
        open if open < 1.5 => Phase::Open,
        _ => Phase::Shed,
    };
    let rays = if head.kind == HeadKind::Radiate {
        head.rays
    } else {
        0
    };
    let floret = if head.kind == HeadKind::Ligulate {
        "ligulate_floret"
    } else {
        "disc_floret"
    };
    laid.phyllaries
        .iter()
        .map(|_| part("phyllary", None))
        .chain((0..rays).map(|_| part("ray_floret", Some(ray))))
        .chain(laid.florets.iter().map(|f| {
            let phase = match f.stage {
                capitula::Stage::Bud => Phase::Bud,
                capitula::Stage::Open => Phase::Open,
                capitula::Stage::Seed => Phase::Fruit { unripe: 0.0 },
            };
            part(floret, Some(phase))
        }))
        .collect()
}

/// A spikelet's parts: its glumes, then its florets, the upper ones each
/// with an awn where its lemma has one (`crate::spikelets`); its florets
/// in the spikelet's `phase`.
fn in_spikelet(spikelet: &Spikelet, phase: Option<Phase>) -> Vec<Part> {
    let laid = spikelets::layout(spikelet);
    let lemmas = laid
        .scales
        .iter()
        .filter(|s| s.bract == Bract::Lemma)
        .count();
    // The awns are on the upper lemmas.
    let unawned = lemmas.saturating_sub(laid.awns.len());
    let mut floret = 0;
    laid.scales
        .iter()
        .map(|s| match s.bract {
            Bract::Glume => Part {
                term: "glume",
                phase: None,
                parts: Vec::new(),
            },
            Bract::Lemma => {
                floret += 1;
                let awn = Part {
                    term: "awn",
                    phase: None,
                    parts: Vec::new(),
                };
                Part {
                    term: "spikelet_floret",
                    phase,
                    parts: if floret > unawned {
                        vec![awn]
                    } else {
                        Vec::new()
                    },
                }
            }
        })
        .collect()
}

/// The name of the part at `path` of the organ `organ`, its term and its
/// number, whose parts are `parts` ([`parts`]): the organ's, then each
/// step's term and its number among the parts of that term beside it,
/// counted from 1, as "inflorescence 12 › capitulum 7 › ray floret 3";
/// `None` where the path leads to no part. `path` holds an index into
/// `parts`, then into that part's parts, and so on.
#[must_use]
pub fn name(organ: (&str, usize), parts: &[Part], path: &[usize]) -> Option<String> {
    let mut words = vec![format!("{} {}", organ.0.replace('_', " "), organ.1)];
    let mut here = parts;
    for &index in path {
        let part = here.get(index)?;
        let number = here[..index].iter().filter(|p| p.term == part.term).count() + 1;
        words.push(format!("{} {number}", part.term.replace('_', " ")));
        here = &part.parts;
    }
    Some(words.join(" › "))
}

/// How many of one kind of thing a plant holds on a day: a row of its
/// [`census`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tally {
    /// The vocabulary's organ they are, or [`PIECE`].
    pub term: String,
    pub count: u64,
    /// How many of the things that hold such a thing hold any: the leaves
    /// with leaflets, the spikelets with awns; 0 for what the plant holds
    /// on its axes.
    pub holders: u64,
    /// How many are in each phase: primordium, bud, open, fruit and shed
    /// ([`PHASES`]); those without phases are in none.
    pub phases: [u64; 5],
    /// The program's organ and structure types counted, each with how
    /// many; none for parts its layouts give.
    pub types: Vec<(String, u64)>,
    /// What they hold, summed over them all.
    pub parts: Vec<Tally>,
}

/// The phases a [`Tally`] counts, in its order.
pub const PHASES: [&str; 5] = ["primordium", "bud", "open", "fruit", "shed"];

fn slot(phase: Phase) -> usize {
    match phase {
        Phase::Primordium => 0,
        Phase::Bud => 1,
        Phase::Open => 2,
        Phase::Fruit { .. } => 3,
        Phase::Shed => 4,
    }
}

/// The tally of `term` among `tallies`, added if it is not there.
fn tally<'t>(tallies: &'t mut Vec<Tally>, term: &str) -> &'t mut Tally {
    let index = if let Some(index) = tallies.iter().position(|t| t.term == term) {
        index
    } else {
        tallies.push(Tally {
            term: term.to_string(),
            ..Tally::default()
        });
        tallies.len() - 1
    };
    &mut tallies[index]
}

/// The tally at `path`, a term at each depth.
fn tally_at<'t>(tallies: &'t mut Vec<Tally>, path: &[&str]) -> &'t mut Tally {
    let (first, rest) = path.split_first().expect("a path names something");
    let here = tally(tallies, first);
    if rest.is_empty() {
        here
    } else {
        tally_at(&mut here.parts, rest)
    }
}

impl Tally {
    fn add(&mut self, phase: Option<Phase>) {
        self.count += 1;
        if let Some(phase) = phase {
            self.phases[slot(phase)] += 1;
        }
    }

    fn add_type(&mut self, name: &str) {
        match self.types.iter_mut().find(|(type_, _)| type_ == name) {
            Some((_, count)) => *count += 1,
            None => self.types.push((name.to_string(), 1)),
        }
    }

    fn add_parts(&mut self, parts: &[Part]) {
        let mut held: Vec<&str> = Vec::new();
        for part in parts {
            let here = tally(&mut self.parts, part.term);
            here.add(part.phase);
            if !held.contains(&part.term) {
                held.push(part.term);
                here.holders += 1;
            }
            here.add_parts(&part.parts);
        }
    }
}

/// A census of `graph` on day `day` by term (spec rework M7B): what the
/// plant holds on its axes, each with what it holds, through its
/// structures, down to what its organs' layouts give ([`parts`]), in the
/// order the graph first holds each. `organ_types` and `structure_types`
/// give each type's name and term; `looks` each organ type's look that
/// day and `seasons` its season. An organ before its bud day or shed
/// holds nothing.
#[must_use]
pub fn census(
    graph: &PlantGraph,
    organ_types: &[(&str, &str)],
    structure_types: &[(&str, &str)],
    looks: &[Look],
    seasons: &[Option<&Season>],
    day: f64,
) -> Vec<Tally> {
    let unknown = ("?", "?");
    let structure_type = |structure: &GraphStructure| {
        structure_types
            .get(usize::from(structure.structure))
            .copied()
            .unwrap_or(unknown)
    };
    // The terms of the structures something within `within` is within,
    // the outermost first.
    let path = |within: Option<u32>| {
        let mut terms = Vec::new();
        let mut next = within;
        while let Some(structure) = next.and_then(|index| graph.structures.get(index as usize)) {
            if terms.len() > graph.structures.len() {
                break;
            }
            terms.push(structure_type(structure).1);
            next = structure.within;
        }
        terms.reverse();
        terms
    };
    let mut tallies = Vec::new();
    // Which terms each structure holds.
    let mut held = std::collections::BTreeSet::new();
    for structure in &graph.structures {
        let (name, term) = structure_type(structure);
        let mut at = path(structure.within);
        at.push(term);
        let here = tally_at(&mut tallies, &at);
        here.add(None);
        here.add_type(name);
        held.insert((structure.within, term));
    }
    for organ in &graph.organs {
        let index = usize::from(organ.organ);
        let (name, term) = organ_types.get(index).copied().unwrap_or(unknown);
        let season = seasons.get(index).copied().flatten();
        let phase = phase(organ, season, day, graph.age);
        let mut at = path(organ.within);
        at.push(term);
        let here = tally_at(&mut tallies, &at);
        here.add(phase);
        here.add_type(name);
        held.insert((organ.within, term));
        if !matches!(phase, Some(Phase::Primordium | Phase::Shed))
            && let Some(look) = looks.get(index)
        {
            here.add_parts(&parts(organ, look, phase));
        }
    }
    for (within, term) in held {
        if within.is_some() {
            let mut at = path(within);
            at.push(term);
            tally_at(&mut tallies, &at).holders += 1;
        }
    }
    tallies
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
    fn an_organ_goes_from_bud_to_shed_by_the_day() {
        use crate::graph::GraphOrgan;
        use crate::math::Vec3;
        let season = Season {
            bud: 95.0,
            flower: 110.0,
            fruit: 135.0,
            ripe: 175.0,
            ripening: 20.0,
            fall: 245.0,
            opening: 0.0,
            bud_look: None,
            flower_look: None,
        };
        let mut organ = GraphOrgan {
            id: 0,
            organ: 0,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 1.0,
            born: 3.0,
            shed: None,
            light: 1.0,
            within: None,
        };
        let on = |organ: &GraphOrgan, day| phase(organ, Some(&season), day, 3.0);
        assert_eq!(on(&organ, 50.0), Some(Phase::Primordium));
        assert_eq!(on(&organ, 100.0), Some(Phase::Bud));
        assert_eq!(on(&organ, 120.0), Some(Phase::Open));
        assert_eq!(on(&organ, 185.0), Some(Phase::Fruit { unripe: 0.5 }));
        assert_eq!(on(&organ, 300.0), Some(Phase::Shed));
        // A leaf has no phases; an organ shed by then is shed.
        assert_eq!(phase(&organ, None, 120.0, 3.0), None);
        organ.shed = Some(3.0);
        assert_eq!(on(&organ, 120.0), Some(Phase::Shed));
        assert_eq!(phase(&organ, None, 120.0, 3.0), Some(Phase::Shed));
    }

    #[test]
    fn a_heads_parts_are_its_phyllaries_rays_and_florets() {
        let head = |open: f64, kind: HeadKind| {
            held(
                &Shape::Head(Head {
                    kind,
                    open,
                    ..Head::default()
                }),
                None,
            )
        };
        let count = |parts: &[Part], term: &str, phase: Option<Phase>| {
            parts
                .iter()
                .filter(|p| p.term == term && (phase.is_none() || p.phase == phase))
                .count()
        };
        // Half open: 16 phyllaries, 21 rays out, the outer florets open.
        let parts = head(0.6, HeadKind::Radiate);
        assert_eq!(parts.len(), 16 + 21 + 60);
        assert_eq!(count(&parts, "phyllary", None), 16);
        assert_eq!(count(&parts, "ray_floret", Some(Phase::Open)), 21);
        let open = count(&parts, "disc_floret", Some(Phase::Open));
        let buds = count(&parts, "disc_floret", Some(Phase::Bud));
        assert!(open > 0 && buds > 0 && open + buds == 60, "{open} {buds}");
        assert_eq!(
            name(("capitulum", 4), &parts, &[18]).as_deref(),
            Some("capitulum 4 › ray floret 3")
        );
        assert_eq!(
            name(("capitulum", 4), &parts, &[37]).as_deref(),
            Some("capitulum 4 › disc floret 1")
        );
        assert_eq!(name(("capitulum", 4), &parts, &[97]), None);
        assert_eq!(name(("capitulum", 4), &parts, &[0, 0]), None);
        // In bud the rays are furled; once half the disc is seed they have
        // fallen.
        let bud = head(0.0, HeadKind::Radiate);
        assert_eq!(count(&bud, "ray_floret", Some(Phase::Bud)), 21);
        assert_eq!(count(&bud, "disc_floret", Some(Phase::Bud)), 60);
        let seed = head(1.6, HeadKind::Radiate);
        assert_eq!(count(&seed, "ray_floret", Some(Phase::Shed)), 21);
        assert!(count(&seed, "disc_floret", Some(Phase::Fruit { unripe: 0.0 })) > 30);
        // A dandelion's head is straps alone; a thistle's, disc florets.
        let straps = head(1.0, HeadKind::Ligulate);
        assert_eq!(count(&straps, "ligulate_floret", Some(Phase::Open)), 60);
        assert_eq!(count(&straps, "ray_floret", None), 0);
        assert_eq!(count(&head(1.0, HeadKind::Discoid), "ray_floret", None), 0);
    }

    #[test]
    fn a_spikelets_parts_are_its_glumes_and_florets_with_their_awns() {
        let spikelet = Spikelet {
            florets: 4,
            awn: 0.5,
            awned: 0.5,
            ..Spikelet::default()
        };
        let parts = held(&Shape::Spikelet(spikelet.clone()), Some(Phase::Open));
        let terms: Vec<_> = parts.iter().map(|p| (p.term, p.parts.len())).collect();
        assert_eq!(
            terms,
            [
                ("glume", 0),
                ("glume", 0),
                ("spikelet_floret", 0),
                ("spikelet_floret", 0),
                ("spikelet_floret", 1),
                ("spikelet_floret", 1)
            ],
            "glumes, then florets from the lowest, the upper half awned"
        );
        assert!(parts[2..].iter().all(|p| p.phase == Some(Phase::Open)));
        assert_eq!(parts[0].phase, None);
        assert_eq!(
            name(("spikelet", 9), &parts, &[5, 0]).as_deref(),
            Some("spikelet 9 › spikelet floret 4 › awn 1")
        );
        // A sedge's spikelet has no glumes, and an awnless one no awns.
        let bare = held(
            &Shape::Spikelet(Spikelet {
                glumes: 0,
                ..Spikelet::default()
            }),
            None,
        );
        assert_eq!(bare.len(), 4);
        assert!(bare.iter().all(|p| p.parts.is_empty()));
    }

    #[test]
    fn every_part_is_a_vocabulary_organ_within_what_holds_it() {
        let units = [
            Unit::Flower,
            Unit::Head,
            Unit::Fruit {
                aspect: 1.0,
                unripe: 0.0,
            },
            Unit::Spikelet {
                aspect: 0.3,
                beyond: 0.0,
                across: 0.3,
            },
        ];
        let phases = [None, Some(Phase::Bud), Some(Phase::Fruit { unripe: 0.0 })];
        let mut pairs: Vec<(&str, &str)> = units
            .iter()
            .flat_map(|&unit| phases.map(|phase| (unit_term(unit, phase), "inflorescence")))
            .collect();
        let shapes = [
            Shape::Head(Head::default()),
            Shape::Head(Head {
                kind: HeadKind::Ligulate,
                ..Head::default()
            }),
            Shape::Spikelet(Spikelet {
                awn: 0.5,
                ..Spikelet::default()
            }),
        ];
        for (shape, holder) in shapes.iter().zip(["capitulum", "capitulum", "spikelet"]) {
            for part in held(shape, None) {
                pairs.push((part.term, holder));
                pairs.extend(part.parts.iter().map(|inner| (inner.term, part.term)));
            }
        }
        for (term, holder) in pairs {
            assert_eq!(organ_term(term), Some(term), "{term}");
            assert!(may_be_within(term, holder), "{term} in {holder}");
        }
    }

    #[test]
    fn a_census_counts_by_term_through_structures_and_layouts() {
        use crate::looks::resolve;
        use crate::lsys::OrganKind::{Flower, Leaf};
        use crate::math::Vec3;
        let organ = |organ: u16, within: Option<u32>, shed: Option<f64>| GraphOrgan {
            id: 7,
            organ,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 1.0,
            born: 2.0,
            shed,
            light: 1.0,
            within,
        };
        let frond = |within: Option<u32>| GraphStructure {
            id: 0,
            structure: 0,
            within,
            born: 2.0,
        };
        let organ_types = [("pinna", "leaflet"), ("head", "capitulum")];
        let mut looks = resolve(
            [("pinna", Leaf), ("head", Flower)],
            &std::collections::BTreeMap::new(),
            [0.0; 3],
            [0.0; 3],
        );
        looks[1].shape = Shape::Head(Head::default());
        let season = Season {
            bud: 95.0,
            flower: 110.0,
            fruit: 135.0,
            ripe: 175.0,
            ripening: 20.0,
            fall: 245.0,
            opening: 0.0,
            bud_look: None,
            flower_look: None,
        };
        // Two fronds, of three pinnae and of two; a head open, and one
        // shed, which holds nothing.
        let graph = PlantGraph {
            age: 2.0,
            height: 1.0,
            segments: Vec::new(),
            organs: vec![
                organ(0, Some(0), None),
                organ(0, Some(0), None),
                organ(0, Some(0), None),
                organ(0, Some(1), None),
                organ(0, Some(1), None),
                organ(1, None, None),
                organ(1, None, Some(1.5)),
            ],
            structures: vec![frond(None), frond(None)],
        };
        let tallies = census(
            &graph,
            &organ_types,
            &[("frond", "leaf")],
            &looks,
            &[None, Some(&season)],
            120.0,
        );
        let leaf = &tallies[0];
        assert_eq!(
            (leaf.term.as_str(), leaf.count, leaf.holders),
            ("leaf", 2, 0)
        );
        assert_eq!(leaf.types, [("frond".to_string(), 2)]);
        let leaflet = &leaf.parts[0];
        assert_eq!(
            (leaflet.term.as_str(), leaflet.count, leaflet.holders),
            ("leaflet", 5, 2)
        );
        assert_eq!(leaflet.phases, [0; 5], "a leaflet has no phases");
        let head = &tallies[1];
        assert_eq!((head.term.as_str(), head.count), ("capitulum", 2));
        assert_eq!(head.phases, [0, 0, 1, 0, 1]);
        let inside: Vec<_> = head
            .parts
            .iter()
            .map(|t| (t.term.as_str(), t.count, t.holders))
            .collect();
        assert_eq!(
            inside,
            [
                ("phyllary", 16, 1),
                ("ray_floret", 21, 1),
                ("disc_floret", 60, 1)
            ]
        );
    }

    #[test]
    fn an_organ_types_term_is_the_same_on_every_day() {
        // An alder's catkins are drawn whole in flower, one catkin each:
        // an inflorescence all the same.
        let spec = crate::spec::PlantSpec::builtin("alnus-rubra").unwrap();
        let (program, _) = spec.program_in(crate::library::Library::builtin()).unwrap();
        let bloom = program
            .organs()
            .position(|(name, _)| name == "bloom")
            .unwrap();
        let kinds: Vec<OrganKind> = program.organs().map(|(_, kind)| kind).collect();
        let explicit: Vec<_> = program.organ_terms().collect();
        let (in_flower, _) = spec.appearance.looks_on(program.organs(), Some(70.0));
        assert!(in_flower[bloom].inflorescence.is_none(), "drawn whole");
        assert_eq!(terms(&kinds, &explicit, &in_flower)[bloom], "flower");
        assert_eq!(
            of_program(&program, &spec.appearance)[bloom],
            "inflorescence"
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
