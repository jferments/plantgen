//! Checks of grown plants against their characters (spec rework M8B).
//!
//! A species states what it is ([`crate::traits`]): a flora's ranges of
//! heights, leaf, head and spikelet sizes. Here those ranges are checked
//! against plants grown from the species' spec, with no rendering: several
//! seeds are grown to the spec's oldest keyframe, each size is measured on
//! every organ that has it as its character's `why` in `traits.json`
//! defines it, and the values of all the plants are judged together.
//!
//! **What is measured.** A plant's height, crown width and stem diameter
//! come from its graph; a culm's length is the axis that bears an
//! inflorescence, from its foot to the inflorescence. A leaf's blade is
//! its card less the card's petiole, at the size its light form draws it
//! (`crate::looks::Forms`); a leaf drawn as a structure of pieces (a grass
//! blade) is the pieces' lengths added up, leaving out pieces of a type a
//! production turns another organ into (thatch, the dead blade). Heads,
//! spikelets, divided leaves and clusters measure their parts from their
//! own layouts ([`crate::capitula::dimensions`],
//! [`crate::spikelets::dimensions`], [`crate::divided::dimensions`],
//! [`crate::clusters::Layout::length`]). Each organ is measured on a day
//! it is in the phase its character describes ([`crate::organs::phase`]):
//! flowers, heads, spikelets and inflorescences open, fruit and cones
//! ripe, and every organ without a season on the default day. An organ is
//! measured once per character, on the first of those days it qualifies.
//!
//! **What passes.** With the values sorted and $`q_{10}`$, $`q_{90}`$ their
//! 10th and 90th percentiles (nearest rank), and a range's `min` and
//! `max`:
//!
//! - an organ's size **passes** when $`[q_{10}, q_{90}]`$ lies within
//!   `[min, max]`, is a **warning** when part of it does not but no value
//!   passes the range's extremes, and an **error** past them: its `low`
//!   and `high` where it states them, else a quarter below `min` and above
//!   `max`. A single stated number passes within a quarter of it and is an
//!   error past half;
//! - a typical size (`height_typical_m`) is judged by the plants' median
//!   the same way;
//! - a greatest size (`height_max_m`, `crown_width_max_m`,
//!   `stem_diameter_max_m`) is an error if any plant passes it.
//!
//! These tolerances are the check's, not the plant's: floras' ranges are
//! not absolute, and a value a little outside one is a warning to look at,
//! not a wrong plant. A character no plant grew an organ for (a plant too
//! young to flower) is "not grown", never a failure. The spread of the
//! values is reported, not judged: a rule reads a range as its typical
//! value, so most plants do not yet vary within it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::graph::{GraphOrgan, PlantGraph};
use crate::grow::Growth;
use crate::library::Library;
use crate::looks::{Look, Season, Shape};
use crate::lsys::Program;
use crate::organs::{self, Phase};
use crate::spec::{Environment, PlantSpec};

/// How a size character is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judged {
    /// Every organ's value, by the population's middle (see the module).
    Organs,
    /// A plant's usual value, by the plants' median.
    Typical,
    /// The greatest any plant may reach.
    Greatest,
}

/// The size characters measured on grown plants, and how each is judged.
pub const SIZES: [(&str, Judged); 21] = [
    ("height_typical_m", Judged::Typical),
    ("height_max_m", Judged::Greatest),
    ("crown_width_max_m", Judged::Greatest),
    ("stem_diameter_max_m", Judged::Greatest),
    ("culm_length_m", Judged::Organs),
    ("leaf_length_m", Judged::Organs),
    ("leaf_width_m", Judged::Organs),
    ("petiole_length_m", Judged::Organs),
    ("inflorescence_length_m", Judged::Organs),
    ("flower_size_m", Judged::Organs),
    ("involucre_diameter_m", Judged::Organs),
    ("involucre_height_m", Judged::Organs),
    ("ray_length_m", Judged::Organs),
    ("pappus_length_m", Judged::Organs),
    ("spikelet_length_m", Judged::Organs),
    ("upper_glume_length_m", Judged::Organs),
    ("lower_glume_length_m", Judged::Organs),
    ("lemma_length_m", Judged::Organs),
    ("awn_length_m", Judged::Organs),
    ("fruit_size_m", Judged::Organs),
    ("cone_length_m", Judged::Organs),
];

/// The share of a range's span beyond which a value is an error when the
/// range states no extremes, and around a single stated number within
/// which the population passes.
pub const MARGIN: f64 = 0.25;

/// What one plant measured: each size character's values, one per organ
/// (or one for the plant).
pub type Measures = BTreeMap<&'static str, Vec<f64>>;

/// One grown plant's measures.
#[derive(Debug, Clone, PartialEq)]
pub struct Plant {
    pub seed: u64,
    pub age: f64,
    pub measures: Measures,
}

/// A check's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Pass,
    /// No plant grew an organ to measure.
    NotGrown,
    Warning,
    Error,
}

impl Verdict {
    /// Its word in a report.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::NotGrown => "not grown",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// A stated size, as a range: a single number is a range of one value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stated {
    pub min: f64,
    pub max: f64,
    /// The extremes, where the flora gives them ("(3–)5–8").
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub typical: Option<f64>,
}

impl Stated {
    /// `value` as a range, if it is a number or a range of numbers.
    #[must_use]
    pub fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Object(range) => {
                let get = |key: &str| range.get(key).and_then(Value::as_f64);
                let typical = get("typical");
                let min = get("min").or(typical).or_else(|| get("max"))?;
                let max = get("max").or(typical).unwrap_or(min);
                Some(Self {
                    min,
                    max,
                    low: get("low"),
                    high: get("high"),
                    typical,
                })
            }
            other => other.as_f64().map(|v| Self {
                min: v,
                max: v,
                low: None,
                high: None,
                typical: None,
            }),
        }
    }

    /// Whether it states one number, not a range.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn single(&self) -> bool {
        self.min == self.max && self.low.is_none() && self.high.is_none()
    }

    /// The band a population's middle must lie in to pass.
    #[must_use]
    pub fn band(&self) -> (f64, f64) {
        if self.single() {
            (self.min * (1.0 - MARGIN), self.max * (1.0 + MARGIN))
        } else {
            (self.min, self.max)
        }
    }

    /// The extremes no value may pass (see the module).
    #[must_use]
    pub fn extremes(&self) -> (f64, f64) {
        if self.single() {
            return (
                self.min * (1.0 - 2.0 * MARGIN),
                self.max * (1.0 + 2.0 * MARGIN),
            );
        }
        (
            self.low.unwrap_or(self.min * (1.0 - MARGIN)).min(self.min),
            self.high.unwrap_or(self.max * (1.0 + MARGIN)).max(self.max),
        )
    }

    /// The greatest the plant may reach: its `high`, else its `max`.
    #[must_use]
    pub fn greatest(&self) -> f64 {
        self.high.unwrap_or(self.max).max(self.max)
    }
}

/// The values measured for one character over every plant, summarised.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    /// Values in all.
    pub count: usize,
    /// Plants with at least one value.
    pub plants: usize,
    pub least: f64,
    pub q10: f64,
    pub median: f64,
    pub q90: f64,
    pub most: f64,
}

impl Spread {
    /// The summary of `values` (sorted in place) from `plants` plants, or
    /// `None` when there are none.
    #[must_use]
    pub fn of(values: &mut [f64], plants: usize) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        Some(Self {
            count: values.len(),
            plants,
            least: values[0],
            q10: rank(values, 0.1),
            median: rank(values, 0.5),
            q90: rank(values, 0.9),
            most: values[values.len() - 1],
        })
    }
}

/// The nearest-rank `share` quantile of sorted `values`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn rank(values: &[f64], share: f64) -> f64 {
    let index = ((values.len() - 1) as f64 * share).round() as usize;
    values[index.min(values.len() - 1)]
}

/// One character checked against the plants.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub character: &'static str,
    pub judged: Judged,
    pub stated: Stated,
    pub spread: Option<Spread>,
    pub verdict: Verdict,
    /// Why, in a few words, where it is not a pass.
    pub reason: String,
}

/// `values` of one character from `plants` plants, judged against
/// `stated` (see the module).
#[must_use]
pub fn judge(
    character: &'static str,
    judged: Judged,
    stated: Stated,
    values: &mut [f64],
    plants: usize,
) -> Finding {
    let finding = |spread, verdict, reason: String| Finding {
        character,
        judged,
        stated,
        spread,
        verdict,
        reason,
    };
    let Some(spread) = Spread::of(values, plants) else {
        return finding(None, Verdict::NotGrown, "no organ to measure".into());
    };
    let (band_low, band_high) = stated.band();
    let (far_low, far_high) = stated.extremes();
    let shown = length_text;
    match judged {
        Judged::Greatest => {
            let greatest = stated.greatest();
            if spread.most > greatest {
                finding(
                    Some(spread),
                    Verdict::Error,
                    format!(
                        "a plant reaches {}, past the greatest {}",
                        shown(spread.most),
                        shown(greatest)
                    ),
                )
            } else {
                finding(Some(spread), Verdict::Pass, String::new())
            }
        }
        Judged::Typical => {
            let m = spread.median;
            if (band_low..=band_high).contains(&m) {
                finding(Some(spread), Verdict::Pass, String::new())
            } else if (far_low..=far_high).contains(&m) {
                finding(
                    Some(spread),
                    Verdict::Warning,
                    format!("the median {} is outside the range", shown(m)),
                )
            } else {
                finding(
                    Some(spread),
                    Verdict::Error,
                    format!("the median {} is past the range's extremes", shown(m)),
                )
            }
        }
        Judged::Organs => {
            if spread.least < far_low || spread.most > far_high {
                let worst = if spread.least < far_low {
                    spread.least
                } else {
                    spread.most
                };
                finding(
                    Some(spread),
                    Verdict::Error,
                    format!("{} is past the range's extremes", shown(worst)),
                )
            } else if spread.q10 < band_low || spread.q90 > band_high {
                finding(
                    Some(spread),
                    Verdict::Warning,
                    "the middle of the values leaves the range".into(),
                )
            } else {
                finding(Some(spread), Verdict::Pass, String::new())
            }
        }
    }
}

/// The days organs are measured on: the default day, then each season's
/// day of full flower (all its units or florets open, before fruit) and of
/// ripe fruit (before the fall), in order, without repeats.
#[must_use]
pub fn days(seasons: &[Option<&Season>]) -> Vec<f64> {
    let mut days = vec![crate::package::DEFAULT_DAY];
    for season in seasons.iter().flatten() {
        let flower = (season.flower + season.opening).min(season.fruit - 1.0);
        let ripe = (season.ripe + season.ripening).min(season.fall - 1.0);
        for day in [flower.max(season.flower), ripe.max(season.ripe)] {
            if !days.contains(&day) {
                days.push(day);
            }
        }
    }
    days
}

/// What phase a character's organs must be in to be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Flowers, heads, spikelets, inflorescences: open.
    Open,
    /// Fruit and cones: ripe.
    Ripe,
}

fn in_stage(phase: Option<Phase>, stage: Stage) -> bool {
    match (phase, stage) {
        (None, _) | (Some(Phase::Open), Stage::Open) => true,
        (Some(Phase::Fruit { unripe }), Stage::Ripe) => unripe < 0.5,
        _ => false,
    }
}

/// Values of one plant, each kept once per organ (and part) it measures.
#[derive(Default)]
struct Tally {
    seen: BTreeMap<(&'static str, u64, u32), f64>,
}

impl Tally {
    fn add(&mut self, character: &'static str, organ: u64, part: u32, value: f64) {
        if value.is_finite() && value > 0.0 {
            self.seen.entry((character, organ, part)).or_insert(value);
        }
    }

    fn measures(self) -> Measures {
        let mut measures = Measures::new();
        for ((character, _, _), value) in self.seen {
            measures.entry(character).or_default().push(value);
        }
        measures
    }
}

/// Organ types a production turns another organ type into (organ phases,
/// spec rework M7): a blade's thatch, a flower's fruit. Their organs are
/// what an organ became, not a grown organ of their own term.
#[must_use]
pub fn later_types(program: &Program) -> Vec<bool> {
    let mut organ_of = vec![None; program.symbols.len()];
    let mut count = 0;
    for (index, symbol) in program.symbols.iter().enumerate() {
        if matches!(symbol.kind, crate::lsys::program::SymbolKind::Organ { .. }) {
            organ_of[index] = Some(count);
            count += 1;
        }
    }
    let mut later = vec![false; count];
    for (symbol, rules) in program.productions.iter().enumerate() {
        if organ_of.get(symbol).copied().flatten().is_none() {
            continue;
        }
        for rule in rules {
            let Some(item) = rule.continuation.and_then(|at| rule.successor.get(at)) else {
                continue;
            };
            let next = usize::from(item.symbol);
            if next != symbol
                && let Some(Some(organ)) = organ_of.get(next)
            {
                later[*organ] = true;
            }
        }
    }
    later
}

/// The plant's grown graph with what a measure needs to read it.
pub struct Grown<'a> {
    pub spec: &'a PlantSpec,
    pub growth: &'a Growth,
    pub graph: &'a PlantGraph,
    /// Each organ type's term ([`organs::of_program`]).
    pub terms: Vec<&'static str>,
    /// Each organ type, whether it is a later phase ([`later_types`]).
    pub later: Vec<bool>,
}

impl Grown<'_> {
    fn season(&self, organ: &GraphOrgan) -> Option<&Season> {
        let kind = self.growth.organ_types.get(usize::from(organ.organ))?;
        self.spec
            .appearance
            .organs
            .get(&kind.name)
            .and_then(|look| look.season.as_ref())
    }

    fn term(&self, organ: &GraphOrgan) -> &'static str {
        self.terms
            .get(usize::from(organ.organ))
            .copied()
            .unwrap_or(organs::PIECE)
    }

    fn later(&self, organ: &GraphOrgan) -> bool {
        self.later
            .get(usize::from(organ.organ))
            .copied()
            .unwrap_or(false)
    }
}

/// Measure one grown plant: every size character on every organ that has
/// it (see the module).
#[must_use]
pub fn measure(grown: &Grown<'_>) -> Measures {
    let mut tally = Tally::default();
    let graph = grown.graph;
    // The plant.
    tally.add("height_typical_m", 0, 0, graph.height);
    tally.add("height_max_m", 0, 0, graph.height);
    if let Some(form) = crate::form::measure(graph, &grown.growth.organ_types) {
        tally.add("crown_width_max_m", 0, 0, 2.0 * form.crown_radius);
    }
    if let Some(dbh) = graph.diameter_at_breast_height() {
        tally.add("stem_diameter_max_m", 0, 0, dbh);
    }
    culms(grown, &mut tally);
    let seasons: Vec<Option<&Season>> = grown
        .growth
        .organ_types
        .iter()
        .map(|kind| {
            grown
                .spec
                .appearance
                .organs
                .get(&kind.name)
                .and_then(|look| look.season.as_ref())
        })
        .collect();
    for day in days(&seasons) {
        let (looks, sizes) = crate::drawing::looks_of(grown.spec, grown.growth, day);
        let drawn = crate::drawing::drawn(graph, &sizes);
        leaves(grown, &drawn, &looks, &mut tally);
        for organ in &drawn.organs {
            let Some(look) = looks.get(usize::from(organ.organ)) else {
                continue;
            };
            let phase = organs::phase(organ, grown.season(organ), day, graph.age);
            reproductive(grown, organ, look, phase, &mut tally);
        }
    }
    tally.measures()
}

/// Each culm: the axis that bears an inflorescence, from its foot (where
/// it leaves its parent, or the ground) to the inflorescence.
fn culms(grown: &Grown<'_>, tally: &mut Tally) {
    let graph = grown.graph;
    for organ in &graph.organs {
        if grown.term(organ) != "inflorescence" || organ.shed.is_some() {
            continue;
        }
        let mut length = 0.0;
        let mut next = organ.segment;
        let mut steps = 0;
        while let Some(segment) = next.and_then(|index| graph.segments.get(index as usize)) {
            length += (segment.end - segment.start).length();
            steps += 1;
            if segment.lateral || steps > graph.segments.len() {
                break;
            }
            next = segment.parent;
        }
        tally.add("culm_length_m", organ.id, 0, length);
    }
}

/// Every grown leaf: a leaf organ standing alone, or a structure termed
/// `leaf` drawn in pieces.
fn leaves(grown: &Grown<'_>, drawn: &PlantGraph, looks: &[Look], tally: &mut Tally) {
    // Pieces of each leaf structure: its index and the organs within it.
    let mut pieces: BTreeMap<u32, Vec<&GraphOrgan>> = BTreeMap::new();
    for organ in &drawn.organs {
        if grown.later(organ) {
            continue;
        }
        let term = grown.term(organ);
        match organ.within {
            Some(structure) if term == organs::PIECE => {
                pieces.entry(structure).or_default().push(organ);
            }
            None if term == "leaf" => {
                if let Some((look, size)) = leaf_look(organ, looks) {
                    leaf_sizes(&look.shape, size, organ.id, tally);
                }
            }
            _ => {}
        }
    }
    for (structure, members) in pieces {
        let leafy = drawn
            .structures
            .get(structure as usize)
            .and_then(|s| grown.growth.structure_types.get(usize::from(s.structure)))
            .is_some_and(|kind| kind.term == "leaf");
        if !leafy {
            continue;
        }
        let id = drawn.structures[structure as usize].id;
        if let [one] = members.as_slice() {
            if let Some((look, size)) = leaf_look(one, looks) {
                leaf_sizes(&look.shape, size, id, tally);
            }
            continue;
        }
        // A blade in pieces, end to end.
        let mut length = 0.0;
        let mut width: f64 = 0.0;
        for piece in members {
            if let Some((look, size)) = leaf_look(piece, looks) {
                length += size;
                width = width.max(size * look.shape.aspect());
            }
        }
        tally.add("leaf_length_m", id, 0, length);
        tally.add("leaf_width_m", id, 0, width);
    }
}

/// The look a leaf is drawn with in its light, and its drawn size.
fn leaf_look<'a>(organ: &GraphOrgan, looks: &'a [Look]) -> Option<(&'a Look, f64)> {
    let own = usize::from(organ.organ);
    let look = looks.get(own)?;
    let (index, share) = look.forms.of(own, organ.light, organ.born);
    Some((looks.get(index).unwrap_or(look), organ.size * share))
}

/// A leaf card's blade, its width and its petiole, for a card `size` long.
fn leaf_sizes(shape: &Shape, size: f64, id: u64, tally: &mut Tally) {
    let (blade, width, petiole) = match shape {
        Shape::Simple(simple) => {
            let blade = size * (1.0 - simple.petiole);
            (blade, blade * simple.width, size * simple.petiole)
        }
        Shape::Lobed(lobed) => {
            let blade = size * (1.0 - lobed.petiole);
            (blade, blade * lobed.width, size * lobed.petiole)
        }
        Shape::Compound(compound) => {
            let d = crate::divided::dimensions(compound);
            (size * d.blade, size * d.width, size * d.petiole)
        }
        Shape::Blade(blade) => (size, size * blade.aspect, 0.0),
        // Palmate lobes radiate from the petiole's tip over the card;
        // sprays, sprigs and fronds are not one leaf's blade.
        Shape::Palmate(palmate) => {
            let blade = size * (1.0 - palmate.petiole);
            (blade, size * shape.aspect(), size * palmate.petiole)
        }
        _ => return,
    };
    tally.add("leaf_length_m", id, 0, blade);
    tally.add("leaf_width_m", id, 0, width);
    tally.add("petiole_length_m", id, 0, petiole);
}

/// An organ's flowers, heads, spikelets, fruit and cones, alone or as a
/// cluster's units, measured in their stage.
fn reproductive(
    grown: &Grown<'_>,
    organ: &GraphOrgan,
    look: &Look,
    phase: Option<Phase>,
    tally: &mut Tally,
) {
    let term = grown.term(organ);
    if let Some(cluster) = &look.inflorescence {
        let layout = cluster.laid_on(organ);
        if in_stage(phase, Stage::Open) {
            tally.add(
                "inflorescence_length_m",
                organ.id,
                0,
                organ.size * layout.length(cluster.unit),
            );
        }
        for (index, unit) in layout.units.iter().enumerate() {
            let part = u32::try_from(index + 1).unwrap_or(u32::MAX);
            unit_sizes(
                &look.shape,
                organ.size * unit.length,
                term,
                phase,
                (organ.id, part),
                tally,
            );
        }
        return;
    }
    unit_sizes(&look.shape, organ.size, term, phase, (organ.id, 0), tally);
}

/// One flower, head, spikelet, fruit or cone drawn with `shape`, `length`
/// long (a flower's or head's width).
fn unit_sizes(
    shape: &Shape,
    length: f64,
    term: &str,
    phase: Option<Phase>,
    (organ, part): (u64, u32),
    tally: &mut Tally,
) {
    match shape {
        Shape::Flower(_) if in_stage(phase, Stage::Open) => {
            tally.add("flower_size_m", organ, part, length);
        }
        Shape::Head(head) => {
            let d = crate::capitula::dimensions(head);
            if in_stage(phase, Stage::Open) {
                tally.add(
                    "involucre_diameter_m",
                    organ,
                    part,
                    length * d.involucre_diameter,
                );
                tally.add(
                    "involucre_height_m",
                    organ,
                    part,
                    length * d.involucre_height,
                );
                tally.add("ray_length_m", organ, part, length * d.ray_length);
                tally.add("pappus_length_m", organ, part, length * d.pappus_length);
            }
            // A head's fruit are its cypselae, ripe once most of its
            // florets are in seed (a head's phase is its drawn opening
            // where its organ has none).
            let ripe = match phase {
                None => head.open >= 1.5,
                Some(_) => in_stage(phase, Stage::Ripe),
            };
            if ripe {
                tally.add("fruit_size_m", organ, part, length * d.cypsela);
            }
        }
        Shape::Spikelet(spikelet) if in_stage(phase, Stage::Open) => {
            let d = crate::spikelets::dimensions(spikelet);
            tally.add("spikelet_length_m", organ, part, length * d.length);
            tally.add("upper_glume_length_m", organ, part, length * d.upper_glume);
            tally.add("lower_glume_length_m", organ, part, length * d.lower_glume);
            tally.add("lemma_length_m", organ, part, length * d.lemma);
            tally.add("awn_length_m", organ, part, length * d.awn);
        }
        Shape::Fruit(_) if in_stage(phase, Stage::Ripe) => {
            let character = if term == "cone" {
                "cone_length_m"
            } else {
                "fruit_size_m"
            };
            tally.add(character, organ, part, length);
        }
        _ => {}
    }
}

/// Grow `spec` from `seed` in `environment` to its oldest keyframe and
/// measure it.
///
/// # Errors
///
/// When the program does not load or growth fails.
pub fn grow_and_measure(
    spec: &PlantSpec,
    library: &Library,
    environment: Environment,
    seed: u64,
) -> Result<Plant, String> {
    let years = spec.growth.years;
    let growth =
        crate::drawing::grow_variant(spec, library, None, environment, seed, vec![years], years)?;
    let (program, _) = spec
        .program_in(library)
        .map_err(|error| error.to_string())?;
    let graph = growth.keyframes.last().ok_or("nothing grew")?;
    let grown = Grown {
        spec,
        growth: &growth,
        graph,
        terms: organs::of_program(&program, &spec.appearance),
        later: later_types(&program),
    };
    Ok(Plant {
        seed,
        age: graph.age,
        measures: measure(&grown),
    })
}

/// A species' report: each stated size character judged against plants
/// grown from several seeds.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub species: String,
    pub environment: Environment,
    pub seeds: Vec<u64>,
    pub age: f64,
    pub findings: Vec<Finding>,
    /// Size characters the species states that nothing here measures yet.
    pub unmeasured: Vec<String>,
}

impl Report {
    /// The worst verdict, `Pass` when there are no findings.
    #[must_use]
    pub fn worst(&self) -> Verdict {
        self.findings
            .iter()
            .map(|finding| finding.verdict)
            .max()
            .unwrap_or(Verdict::Pass)
    }
}

/// `n` seeds for a species: its variants' own first, then the numbers
/// after the largest of them.
#[must_use]
pub fn seeds_of(spec: &PlantSpec, n: usize) -> Vec<u64> {
    let mut seeds: Vec<u64> = spec.variants.seeds.iter().copied().take(n).collect();
    let mut next = seeds.iter().copied().max().unwrap_or(0) + 1;
    while seeds.len() < n {
        seeds.push(next);
        next += 1;
    }
    seeds
}

/// Whether `traits` states any character [`SIZES`] measures.
#[must_use]
pub fn states_sizes(traits: &Map<String, Value>) -> bool {
    SIZES.iter().any(|(key, _)| traits.contains_key(*key))
}

/// Check `spec`'s stated sizes (`traits`, as it inherits them) against
/// plants grown from `seeds` in its typical environment.
///
/// # Errors
///
/// When a plant does not grow.
pub fn check(
    spec: &PlantSpec,
    library: &Library,
    traits: &Map<String, Value>,
    seeds: &[u64],
) -> Result<Report, String> {
    let environment = crate::drawing::typical_environment(spec, library);
    let plants = seeds
        .iter()
        .map(|&seed| grow_and_measure(spec, library, environment, seed))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(judge_plants(spec, environment, traits, &plants))
}

/// Judge `plants` of `spec` against its stated sizes.
#[must_use]
pub fn judge_plants(
    spec: &PlantSpec,
    environment: Environment,
    traits: &Map<String, Value>,
    plants: &[Plant],
) -> Report {
    let mut findings = Vec::new();
    for (character, judged) in SIZES {
        let Some(stated) = traits.get(character).and_then(Stated::of) else {
            continue;
        };
        let mut values = Vec::new();
        let mut with = 0;
        for plant in plants {
            if let Some(found) = plant.measures.get(character) {
                with += 1;
                if judged == Judged::Typical {
                    values.push(found[0]);
                } else {
                    values.extend_from_slice(found);
                }
            }
        }
        findings.push(judge(character, judged, stated, &mut values, with));
    }
    let vocabulary = crate::traits::Vocabulary::builtin();
    let unmeasured = traits
        .keys()
        .filter(|key| {
            vocabulary
                .traits
                .get(key.as_str())
                .is_some_and(|t| t.group == "size" || t.unit.as_deref() == Some("m"))
        })
        .filter(|key| !SIZES.iter().any(|(size, _)| size == key))
        .filter(|key| !matches!(key.as_str(), "elevation_min_m" | "elevation_max_m"))
        .cloned()
        .collect();
    Report {
        species: spec.id.clone(),
        environment,
        seeds: plants.iter().map(|plant| plant.seed).collect(),
        age: plants.first().map_or(0.0, |plant| plant.age),
        findings,
        unmeasured,
    }
}

/// A length for a report: millimetres below a centimetre, centimetres
/// below a metre, else metres.
#[must_use]
pub fn length_text(metres: f64) -> String {
    if metres < 0.01 {
        format!("{:.1} mm", metres * 1000.0)
    } else if metres < 1.0 {
        format!("{:.1} cm", metres * 100.0)
    } else {
        format!("{metres:.2} m")
    }
}

/// A stated range for a report: "(low–)min–max(–high)".
#[must_use]
pub fn range_text(stated: &Stated) -> String {
    let mut text = String::new();
    if let Some(low) = stated.low {
        let _ = write!(text, "({}–)", length_text(low));
    }
    if stated.single() {
        text.push_str(&length_text(stated.min));
    } else {
        let _ = write!(
            text,
            "{}–{}",
            length_text(stated.min),
            length_text(stated.max)
        );
    }
    if let Some(high) = stated.high {
        let _ = write!(text, "(–{})", length_text(high));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stated(value: &Value) -> Stated {
        Stated::of(value).unwrap()
    }

    #[test]
    fn a_population_within_its_range_passes_and_one_past_its_extremes_fails() {
        let range = stated(&json!({"min": 0.1, "max": 0.45}));
        let mut inside: Vec<f64> = (0..20).map(|i| 0.12 + 0.01 * f64::from(i)).collect();
        let pass = judge("leaf_length_m", Judged::Organs, range, &mut inside, 3);
        assert_eq!(pass.verdict, Verdict::Pass, "{pass:?}");
        // A tail below the range, within a quarter of it: a warning.
        let mut tail: Vec<f64> = (0..20).map(|i| 0.08 + 0.005 * f64::from(i)).collect();
        let warn = judge("leaf_length_m", Judged::Organs, range, &mut tail, 3);
        assert_eq!(warn.verdict, Verdict::Warning, "{warn:?}");
        // One leaf at 4 cm, past a quarter below 10 cm.
        let mut far = vec![0.04, 0.2, 0.3];
        let error = judge("leaf_length_m", Judged::Organs, range, &mut far, 1);
        assert_eq!(error.verdict, Verdict::Error, "{error:?}");
        // Stated extremes are the extremes: "(3–)5–8 mm" allows 3.5 mm.
        let flora = stated(&json!({"min": 0.005, "max": 0.008, "low": 0.003}));
        let mut small = vec![0.0035, 0.006, 0.006, 0.006, 0.006, 0.006, 0.006, 0.006];
        let fine = judge("lemma_length_m", Judged::Organs, flora, &mut small, 1);
        assert_eq!(fine.verdict, Verdict::Pass, "{fine:?}");
    }

    #[test]
    fn a_typical_height_is_judged_by_the_median_and_a_greatest_by_every_plant() {
        let typical = stated(&json!({"min": 0.06, "max": 0.65}));
        let mut heights = vec![0.6, 0.62, 0.7];
        let pass = judge(
            "height_typical_m",
            Judged::Typical,
            typical,
            &mut heights,
            3,
        );
        assert_eq!(pass.verdict, Verdict::Pass);
        let greatest = stated(&json!(30.0));
        let mut trees = vec![25.0, 31.0];
        let over = judge("height_max_m", Judged::Greatest, greatest, &mut trees, 2);
        assert_eq!(over.verdict, Verdict::Error);
        let mut none: Vec<f64> = Vec::new();
        let missing = judge("fruit_size_m", Judged::Organs, typical, &mut none, 0);
        assert_eq!(missing.verdict, Verdict::NotGrown);
    }

    #[test]
    fn a_single_number_passes_within_a_quarter() {
        let single = stated(&json!(0.01));
        let mut near = vec![0.009, 0.011, 0.012];
        assert_eq!(
            judge("ray_length_m", Judged::Organs, single, &mut near, 1).verdict,
            Verdict::Pass
        );
        let mut off = vec![0.013, 0.013, 0.013];
        assert_eq!(
            judge("ray_length_m", Judged::Organs, single, &mut off, 1).verdict,
            Verdict::Warning
        );
        let mut far = vec![0.02];
        assert_eq!(
            judge("ray_length_m", Judged::Organs, single, &mut far, 1).verdict,
            Verdict::Error
        );
    }

    #[test]
    fn seeds_start_with_the_variants_own() {
        let spec = Library::builtin().spec("achillea-millefolium").unwrap();
        assert_eq!(seeds_of(&spec, 3), vec![1, 2, 3]);
    }
}
