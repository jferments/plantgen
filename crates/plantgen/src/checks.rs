//! Checks of grown plants against their characters (spec rework M8B and
//! M8A).
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
//! production turns another organ into (thatch, the dead blade). A leaf
//! drawn as a structure of its own axis and the leaflets on it (a fern's
//! frond) has its stipe, the axis below the lowest leaflet, its blade, the
//! axis above, its width, twice the farthest a leaflet reaches out from
//! the axis, and its leaflets (its pinnae, where they are drawn divided
//! again), counted. Heads,
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
//! values is reported, not judged: organs drawn solid vary within a range
//! a rule reads alone (spec rework VARY, each a variety of its look), but
//! a size set by hand, by a rule reading two ranges or by the plant's
//! parameters, or drawn only on a card, does not: a plant draws only
//! characters of the whole plant (the years to its first flowers).
//!
//! **Counts and kinds (M8A).** Counts are counted on each organ that
//! holds them, from what its layout draws ([`COUNTS`]: a head's rays,
//! florets, phyllaries and rows, a spikelet's florets and glumes, a
//! cluster's units and branches, a divided leaf's leaflets and pinnae
//! and the times it divides, the leaves of a whorl), and judged as an
//! organ's size is. Kinds ([`KINDS`]) pass when every state the plants
//! draw is one the species states, plainly or qualified: a cluster's kind
//! while its units are flowers, a head's kind, awns, a spikelet's ranks,
//! a divided leaf's pattern and its end. How the leaves stand is measured
//! on the graph: the leaves sharing a node, and the turn from each node
//! to the next up its axis, the leaf below carried up the bending axis by
//! the least rotation, the plant's median turn named by the nearest
//! pattern (distichous 180°, spiral 137.5°, tristichous 120°, decussate
//! half the gap between a node's leaves). A rosette's or tiller's leaves,
//! which stand on no segment, are measured round the vertical where it
//! stands about upright, one to a node in the order they were placed. A
//! year's growth of an axis bears the leaves of its own nodes, the most on
//! any one counted for the plant (`shoot_leaves`). A flower counts its
//! petals or tepals in a whorl, its stamens, staminodes and carpels and a
//! spiral's tepals, measures its spurs, and draws its perianth's kind,
//! its ovary's place and its corolla's zygomorphy, as its layout has them
//! (spec rework P5).
//!
//! **Crown form (crown form research, 2026-10-12).** A crown's outline
//! seen from the side is fitted to an ellipse (`crown_outline_fit`) and
//! the sky through it is measured (`crown_see_through`), as
//! [`crate::form`] defines them, each judged by the plants' median as a
//! typical size is. A tree (growth form `excurrent_tree` or
//! `decurrent_tree`) that states no fit is held to [`TREE_OUTLINE_FIT`],
//! so no tree's crown passes as a ball or an egg.
//!
//! **Juvenile and adult (spec rework P6).** A plant's juvenile period
//! (`juvenile_period_years`) is the birth of its first flower, fruit or
//! cone of any size, watched step by step as it grows up to the range's
//! upper extreme, and judged by the plants' median as a typical size is.
//! A flower or fruit on a shoot whose recorded adultness is below 0.5, a
//! juvenile shoot, is an error whatever the species states.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::graph::{GraphOrgan, PlantGraph};
use crate::grow::Growth;
use crate::library::Library;
use crate::looks::{Look, Season, Shape};
use crate::lsys::{OrganKind, Program};
use crate::math::Vec3;
use crate::organs::{self, Phase};
use crate::spec::{Environment, PlantSpec};

/// How a size character is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judged {
    /// Every organ's value, by the population's middle (see the module).
    Organs,
    /// A plant's usual value (the median of its organs' where it has
    /// several), by the plants' median.
    Typical,
    /// The greatest any plant may reach.
    Greatest,
}

/// The size characters measured on grown plants, and how each is judged.
pub const SIZES: [(&str, Judged); 32] = [
    ("height_typical_m", Judged::Typical),
    ("height_max_m", Judged::Greatest),
    ("crown_width_max_m", Judged::Greatest),
    ("stem_diameter_max_m", Judged::Greatest),
    ("culm_length_m", Judged::Organs),
    ("joint_length_m", Judged::Typical),
    ("joint_width_m", Judged::Typical),
    ("radial_spine_length_m", Judged::Typical),
    ("central_spine_length_m", Judged::Typical),
    ("areole_spacing_m", Judged::Typical),
    ("leaf_length_m", Judged::Organs),
    ("leaf_width_m", Judged::Organs),
    ("petiole_length_m", Judged::Organs),
    ("inflorescence_length_m", Judged::Organs),
    ("bract_length_m", Judged::Organs),
    ("flower_size_m", Judged::Organs),
    ("spur_length_m", Judged::Organs),
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
    ("divergence_deg", Judged::Typical),
    ("juvenile_period_years", Judged::Typical),
    ("crown_outline_fit", Judged::Typical),
    ("crown_see_through", Judged::Typical),
];

/// The count characters counted on grown plants (spec rework M8A), each
/// on every organ that holds what it counts and judged as an organ's size
/// is.
pub const COUNTS: [&str; 24] = [
    "ray_florets",
    "disc_florets",
    "ligulate_florets",
    "phyllaries",
    "phyllary_series",
    "florets_per_spikelet",
    "glumes",
    "units_per_inflorescence",
    "inflorescence_branches",
    "inflorescence_bracts",
    "leaflets",
    "pinnae",
    "division_order",
    "leaves_per_whorl",
    "shoot_leaves",
    "flower_merosity",
    "stamens",
    "staminodes",
    "carpels",
    "perianth_parts",
    "ribs",
    "radial_spines",
    "central_spines",
    "needles_per_fascicle",
];

/// The kinds a grown plant's drawing must agree with (spec rework M8A):
/// each an enum or bool character, compared with what the plant draws.
pub const KINDS: [&str; 14] = [
    "inflorescence",
    "bract_form",
    "capitulum_kind",
    "awns",
    "scale_arrangement",
    "division_pattern",
    "terminal_leaflet",
    "leaf_arrangement",
    "phyllotaxis_pattern",
    "perianth",
    "ovary_position",
    "zygomorphy",
    "glochids",
    "foliage_ranks",
];

/// The share of a range's span beyond which a value is an error when the
/// range states no extremes, and around a single stated number within
/// which the population passes.
pub const MARGIN: f64 = 0.25;

/// The outline fit a tree's crown is held to where its species states
/// none (see the module): photographs of open-grown Oregon white oaks fit
/// at 0.82 to 0.84, and every broadleaf crown grown inside its envelope at
/// 0.89 to 0.94 (crown form research, 2026-10-12), so a median above 0.87
/// is a warning and above 0.90 an error.
pub const TREE_OUTLINE_FIT: Stated = Stated {
    min: 0.6,
    max: 0.87,
    low: Some(0.4),
    high: Some(0.9),
    typical: None,
};

/// `character` as `spec` states it in `traits`, or as a check holds it
/// where it states none ([`TREE_OUTLINE_FIT`]).
#[must_use]
pub fn stated(spec: &PlantSpec, traits: &Map<String, Value>, character: &str) -> Option<Stated> {
    traits.get(character).and_then(Stated::of).or_else(|| {
        (character == "crown_outline_fit"
            && matches!(
                spec.growth_form,
                crate::spec::GrowthForm::ExcurrentTree | crate::spec::GrowthForm::DecurrentTree
            ))
        .then_some(TREE_OUTLINE_FIT)
    })
}

/// What one plant measured: each size character's values, one per organ
/// (or one for the plant).
pub type Measures = BTreeMap<&'static str, Vec<f64>>;

/// The states one plant draws for each kind character.
pub type Kinds = BTreeMap<&'static str, BTreeSet<String>>;

/// One grown plant's measures.
#[derive(Debug, Clone, PartialEq)]
pub struct Plant {
    pub seed: u64,
    pub age: f64,
    pub measures: Measures,
    pub kinds: Kinds,
    /// Its flowers and fruit on juvenile shoots (spec rework P6).
    pub juvenile_flowers: usize,
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
    let shown = |value: f64| value_text(character, value);
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
    kinds: Kinds,
}

impl Tally {
    fn add(&mut self, character: &'static str, organ: u64, part: u32, value: f64) {
        if value.is_finite() && value > 0.0 {
            self.seen.entry((character, organ, part)).or_insert(value);
        }
    }

    /// A count, which may be 0 (a head without rays).
    fn count(&mut self, character: &'static str, organ: u64, part: u32, count: u32) {
        self.seen
            .entry((character, organ, part))
            .or_insert(f64::from(count));
    }

    fn kind(&mut self, character: &'static str, state: &str) {
        self.kinds
            .entry(character)
            .or_default()
            .insert(state.to_owned());
    }

    fn measures(self) -> (Measures, Kinds) {
        let mut measures = Measures::new();
        for ((character, _, _), value) in self.seen {
            measures.entry(character).or_default().push(value);
        }
        (measures, self.kinds)
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

    /// Whether the organ is drawn as a cluster on some days, so a look
    /// without one draws the whole cluster (a dogwood's head in its bracts).
    fn clustered(&self, organ: &GraphOrgan) -> bool {
        self.growth
            .organ_types
            .get(usize::from(organ.organ))
            .and_then(|kind| self.spec.appearance.organs.get(&kind.name))
            .is_some_and(|look| look.inflorescence.is_some())
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
pub fn measure(grown: &Grown<'_>) -> (Measures, Kinds) {
    let mut tally = Tally::default();
    let graph = grown.graph;
    // The plant.
    tally.add("height_typical_m", 0, 0, graph.height);
    tally.add("height_max_m", 0, 0, graph.height);
    if let Some(form) = crate::form::measure(graph, &grown.growth.organ_types) {
        tally.add("crown_width_max_m", 0, 0, 2.0 * form.crown_radius);
        if let Some(fit) = form.outline_fit {
            tally.add("crown_outline_fit", 0, 0, fit);
        }
        if let Some(through) = form.see_through {
            tally.add("crown_see_through", 0, 0, through);
        }
    }
    if let Some(dbh) = graph.diameter_at_breast_height() {
        tally.add("stem_diameter_max_m", 0, 0, dbh);
    }
    culms(grown, &mut tally);
    bodies(grown, &mut tally);
    let (looks, _) =
        crate::drawing::looks_of(grown.spec, grown.growth, crate::package::DEFAULT_DAY);
    arrangement(grown, &looks, &mut tally);
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
    // Each spray card's longest needles, its main axis's.
    let mut needle_lengths = Vec::new();
    for day in days(&seasons) {
        let (looks, sizes) = crate::drawing::looks_of(grown.spec, grown.growth, day);
        let drawn = crate::drawing::drawn(graph, &sizes);
        leaves(grown, &drawn, &looks, &mut tally);
        for organ in &drawn.organs {
            let Some(look) = looks
                .get(usize::from(organ.organ))
                .map(|look| look.of(organ.id))
            else {
                continue;
            };
            let phase = organs::phase(organ, grown.season(organ), day, graph.age);
            reproductive(grown, organ, look, phase, &mut tally);
            if let Shape::Needles(needles) = &look.shape {
                needle_counts(needles, organ.id, &mut tally);
                needle_lengths.push(organ.size * needles.needle);
            }
        }
    }
    // A card draws its needles a share of its own length, so they grow
    // with the spray as no shoot's needles do: the plant's usual needle
    // is judged, one value for the plant.
    if !needle_lengths.is_empty() {
        needle_lengths.sort_by(f64::total_cmp);
        tally.add(
            "leaf_length_m",
            0,
            0,
            needle_lengths[needle_lengths.len() / 2],
        );
    }
    tally.measures()
}

/// Areoles sampled on each body axis to count and measure their spines.
const AREOLES: u64 = 24;

/// A cactus's bodies (spec rework E6c): the ribs round each stem where it
/// is widest (none on one without ribs); each column's or trunk's
/// greatest diameter; each terminal joint's length and width (a pad's),
/// as floras measure them, judged by the usual joint, as the program
/// draws a branch's deeper joints shorter; its areoles' spacing, along a
/// rib's crest or between neighbours on a joint, as its look sets it; and
/// on sampled areoles of each, the spines of each role as they are drawn
/// full-grown, counted (none where no set has the role, a main central
/// among the centrals) and each one's length, judged by the usual spine:
/// the rules draw it a fifth below the longest a flora gives; and whether
/// they bear glochids.
fn bodies(grown: &Grown<'_>, tally: &mut Tally) {
    use crate::body::SpineRole;
    let looks = crate::drawing::bodies_of(grown.spec, grown.growth);
    if looks.is_empty() {
        return;
    }
    let param = |name: &str| {
        grown
            .spec
            .generator
            .params
            .get(name)
            .copied()
            .unwrap_or_default()
    };
    let jointed = param("joints") > 0.0;
    let trunk = param("trunk_joints") > 1.0;
    for axis in crate::body::measure(grown.graph, &looks) {
        let look = &looks[axis.body];
        tally.count("ribs", axis.id, 0, axis.ribs);
        tally.add("areole_spacing_m", axis.id, 0, look.areoles.spacing);
        if !jointed || (axis.root && trunk) {
            tally.add("stem_diameter_max_m", axis.id, 0, 2.0 * axis.widest);
        } else if axis.tip {
            tally.add("joint_length_m", axis.id, 0, axis.length);
            tally.add("joint_width_m", axis.id, 0, 2.0 * axis.widest);
        }
        let has = |roles: &[SpineRole]| {
            look.spines
                .iter()
                .any(|set| set.role.is_some_and(|role| roles.contains(&role)))
        };
        tally.kind(
            "glochids",
            if has(&[SpineRole::Glochid]) {
                "true"
            } else {
                "false"
            },
        );
        for (roles, count, length) in [
            (
                &[SpineRole::Radial][..],
                "radial_spines",
                "radial_spine_length_m",
            ),
            (
                &[SpineRole::Central, SpineRole::Main][..],
                "central_spines",
                "central_spine_length_m",
            ),
        ] {
            if !has(roles) {
                tally.count(count, axis.id, 0, 0);
                continue;
            }
            for areole in 0..AREOLES {
                // The low bits of a well-mixed hash.
                #[allow(clippy::cast_possible_truncation)]
                let seed = crate::rng::mix64(axis.id ^ areole.wrapping_mul(0x9E37)) as u32;
                // Areoles number far fewer than 2^32, spines on one
                // fewer than 2^8.
                #[allow(clippy::cast_possible_truncation)]
                let part = (areole as u32) << 8;
                let mut n = 0;
                for (set, lengths) in look
                    .spines
                    .iter()
                    .zip(crate::spines::areole_spines(look, seed))
                {
                    if set.role.is_some_and(|role| roles.contains(&role)) {
                        for long in lengths {
                            tally.add(length, axis.id, part + n, f64::from(long));
                            n += 1;
                        }
                    }
                }
                tally.count(count, axis.id, part, n);
            }
        }
    }
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

/// Where a node stands: on a segment of an axis (the axis's first
/// segment), or on the ground at a rosette, among the leaves placed there
/// together (its place, to a millimetre, and their birth).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Axis {
    Stem(usize),
    Ground(i64, i64, i64),
}

/// The leaves at one node of an axis.
struct Node {
    axis: Axis,
    /// How far up the axis it stands; a rosette's leaves in the order
    /// they were placed.
    along: f64,
    /// The axis's direction there.
    up: Vec3,
    /// Which way each leaf leaves the axis, square to it.
    around: Vec<Vec3>,
}

/// `v` turned by the least rotation that takes `from` to `to` (both unit
/// vectors), as the axis carries its frame from one node to the next.
fn carried(v: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let axis = from.cross(to);
    let sin = axis.length();
    if sin < 1e-9 {
        return v;
    }
    let k = axis * (1.0 / sin);
    let cos = from.dot(to).clamp(-1.0, 1.0);
    v * cos + k.cross(v) * sin + k * (k.dot(v) * (1.0 - cos))
}

/// How the plant's leaves stand at their nodes, measured on the graph:
/// the leaves at each node (`leaf_arrangement`, and `leaves_per_whorl` at
/// a whorl), and how far each node turns from the one below it round its
/// axis, the frame carried up the axis between them (`divergence_deg`,
/// the plant's median, and the `phyllotaxis_pattern` nearest it). Leaves
/// at the ground, a rosette's or a tiller's, stand one to a node, in the
/// order they were placed, each year's apart. On a stem, a node is the
/// leaves born with the internode below it: a short shoot's leaves,
/// crowded later where its bud stands, are no node of the stem. A stem's
/// leaves of its own on one year's growth of one axis are what a long
/// shoot bears in a year (`shoot_leaves`), the most on any one counted,
/// one value for the plant: its most vigorous shoot's.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines
)]
fn arrangement(grown: &Grown<'_>, looks: &[Look], tally: &mut Tally) {
    let graph = grown.graph;
    let segments = &graph.segments;
    // Each segment's axis and its end's distance up that axis.
    let mut axis = Vec::with_capacity(segments.len());
    let mut along_end: Vec<f64> = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let length = (segment.end - segment.start).length();
        match segment.parent.map(|p| p as usize) {
            Some(parent) if !segment.lateral && parent < index => {
                axis.push(axis[parent]);
                along_end.push(along_end[parent] + length);
            }
            _ => {
                axis.push(index);
                along_end.push(length);
            }
        }
    }
    // Where each leaf leaves its axis: a lone leaf where it is placed, a
    // leaf drawn in pieces where its first piece's structure leaves the
    // axis that bears it.
    let mut first_piece: BTreeMap<u32, (&GraphOrgan, usize)> = BTreeMap::new();
    let mut holders: Vec<(Option<usize>, Vec3, Vec3, &GraphOrgan, usize)> = Vec::new();
    for (order, organ) in graph.organs.iter().enumerate() {
        if grown.later(organ) {
            continue;
        }
        match organ.within {
            None if grown.term(organ) == "leaf" => {
                // A card of several leaves (a sprig, a spray) is not one
                // leaf at one node.
                let one = looks.get(usize::from(organ.organ)).is_some_and(|look| {
                    matches!(
                        look.shape,
                        Shape::Simple(_)
                            | Shape::Lobed(_)
                            | Shape::Compound(_)
                            | Shape::Blade(_)
                            | Shape::Palmate(_)
                    )
                });
                if one {
                    holders.push((
                        organ.segment.map(|s| s as usize),
                        organ.position,
                        organ.heading,
                        organ,
                        order,
                    ));
                }
            }
            Some(structure) => {
                first_piece.entry(structure).or_insert((organ, order));
            }
            None => {}
        }
    }
    for (structure, (piece, order)) in first_piece {
        let leafy = graph
            .structures
            .get(structure as usize)
            .and_then(|s| grown.growth.structure_types.get(usize::from(s.structure)))
            .is_some_and(|kind| kind.term == "leaf");
        if !leafy {
            continue;
        }
        let mut segment = piece.segment.map(|s| s as usize);
        let mut position = piece.position;
        while let Some(index) = segment
            && segments[index].within == Some(structure)
        {
            position = segments[index].start;
            segment = segments[index].parent.map(|p| p as usize);
        }
        holders.push((segment, position, piece.heading, piece, order));
    }
    // A shoot's own leaves are born with the internode below them. Leaves
    // born there later are a short shoot's, from a bud at the node, which
    // crowds each year's leaves at one place (with last year's until they
    // fall); floras state the arrangement of the long shoots' leaves, so
    // where a plant's stems bear leaves of their own, only those count.
    let own = |segment: Option<usize>, organ: &GraphOrgan| {
        segment
            .and_then(|index| segments.get(index))
            .is_none_or(|at| (at.born - organ.born).abs() < 1e-9)
    };
    if holders
        .iter()
        .any(|(segment, _, _, organ, _)| segment.is_some() && own(*segment, organ))
    {
        holders.retain(|(segment, _, _, organ, _)| own(*segment, organ));
    }
    // A step is a year, so an axis's segments born in one step are a
    // year's growth of it: the most leaves of their own any one bears are
    // what the plant's most vigorous long shoot bears in a year.
    let mut units: BTreeMap<(usize, i64), u32> = BTreeMap::new();
    for (segment, _, _, organ, _) in &holders {
        if let Some(index) = *segment
            && own(*segment, organ)
        {
            let year = libm::round(segments[index].born * 1e6) as i64;
            *units.entry((axis[index], year)).or_default() += 1;
        }
    }
    if let Some(most) = units.values().max() {
        tally.count("shoot_leaves", 0, 0, *most);
    }
    // Leaves sharing a segment and a place are one node.
    let mut nodes: BTreeMap<(Axis, [i64; 3], u64), Node> = BTreeMap::new();
    let mut headings: BTreeMap<(Axis, [i64; 3], u64), Vec<Vec3>> = BTreeMap::new();
    for (segment, position, heading, organ, order) in holders {
        let at = segment.and_then(|index| segments.get(index));
        let up = at.map_or(Vec3::Y, |at| (at.end - at.start).normalize_or(Vec3::Y));
        let place = [position.x, position.y, position.z].map(|x| libm::round(x * 1e5) as i64);
        let (key, along) = if let (Some(index), Some(at)) = (segment, at) {
            (
                (Axis::Stem(axis[index]), place, 0),
                along_end[index] - (at.end - at.start).length() + (position - at.start).dot(up),
            )
        } else {
            let ground = Axis::Ground(
                libm::round(position.x * 1e3) as i64,
                libm::round(position.z * 1e3) as i64,
                libm::round(organ.born * 1e6) as i64,
            );
            ((ground, place, organ.id), order as f64)
        };
        nodes.entry(key).or_insert_with(|| Node {
            axis: key.0,
            along,
            up,
            around: Vec::new(),
        });
        headings.entry(key).or_default().push(heading);
    }
    // A tiller or rosette has no segment to give its axis: its leaves are
    // measured round the vertical, and only where it stands about upright
    // (their mean heading within 20° of it), since round a leaning tiller
    // the turns cannot be told from the lean.
    let mut leaning: BTreeMap<Axis, Vec3> = BTreeMap::new();
    for (key, list) in &headings {
        if matches!(key.0, Axis::Ground(..)) {
            let sum = leaning.entry(key.0).or_insert(Vec3::ZERO);
            for heading in list {
                *sum += *heading;
            }
        }
    }
    let upright = crate::math::cos(crate::math::radians(20.0));
    for (key, node) in &mut nodes {
        if leaning
            .get(&node.axis)
            .is_some_and(|sum| sum.normalize_or(Vec3::X).dot(Vec3::Y) < upright)
        {
            continue;
        }
        for heading in &headings[key] {
            let out = *heading - node.up * heading.dot(node.up);
            if out.length() >= 1e-6 {
                node.around.push(out.normalize_or(Vec3::X));
            }
        }
    }
    nodes.retain(|_, node| !node.around.is_empty());
    // The plant's usual leaves at a node, and each whorl's count.
    let mut per_node: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, node) in nodes.values().enumerate() {
        let leaves = node.around.len();
        *per_node.entry(leaves).or_default() += 1;
        if leaves >= 3 {
            tally.count(
                "leaves_per_whorl",
                0,
                u32::try_from(index).unwrap_or(u32::MAX),
                u32::try_from(leaves).unwrap_or(u32::MAX),
            );
        }
    }
    let Some(usual) = per_node
        .iter()
        .max_by_key(|(leaves, nodes)| (**nodes, std::cmp::Reverse(**leaves)))
        .map(|(leaves, _)| *leaves)
    else {
        return;
    };
    tally.kind(
        "leaf_arrangement",
        match usual {
            1 => "alternate",
            2 => "opposite",
            _ => "whorled",
        },
    );
    // The turn from each node to the next up its axis, among nodes of the
    // usual count: to the nearest leaf of the next, so a pair crossing the
    // one below turns a quarter.
    let mut by_axis: BTreeMap<Axis, Vec<&Node>> = BTreeMap::new();
    for node in nodes.values().filter(|node| node.around.len() == usual) {
        by_axis.entry(node.axis).or_default().push(node);
    }
    let mut turns: Vec<f64> = Vec::new();
    for (_, mut members) in by_axis {
        members.sort_by(|a, b| a.along.total_cmp(&b.along));
        for pair in members.windows(2) {
            let (below, above) = (pair[0], pair[1]);
            if above.along - below.along <= 1e-9 {
                continue;
            }
            let from = carried(below.around[0], below.up, above.up);
            let from = (from - above.up * from.dot(above.up)).normalize_or(from);
            if let Some(turn) = above
                .around
                .iter()
                .map(|to| crate::math::degrees(crate::math::acos(from.dot(*to).clamp(-1.0, 1.0))))
                .reduce(f64::min)
            {
                turns.push(turn);
            }
        }
    }
    if turns.is_empty() {
        return;
    }
    turns.sort_by(f64::total_cmp);
    let median = turns[turns.len() / 2];
    tally.add("divergence_deg", 0, 0, median);
    // The pattern whose turn is nearest: a pair or whorl crossing the one
    // below turns half the gap between its leaves.
    let patterns: &[(&str, f64)] = if usual >= 2 {
        &[("decussate", 180.0 / usual as f64), ("superposed", 0.0)]
    } else {
        &[
            ("distichous", 180.0),
            ("spiral", 137.5),
            ("tristichous", 120.0),
        ]
    };
    if let Some((pattern, _)) = patterns
        .iter()
        .min_by(|a, b| (a.1 - median).abs().total_cmp(&(b.1 - median).abs()))
    {
        tally.kind("phyllotaxis_pattern", pattern);
    }
}

/// Every grown leaf: a leaf organ standing alone, or a structure termed
/// `leaf` drawn in pieces.
fn leaves(grown: &Grown<'_>, drawn: &PlantGraph, looks: &[Look], tally: &mut Tally) {
    // Pieces of each leaf structure: its index and the organs within it.
    let mut pieces: BTreeMap<u32, Vec<&GraphOrgan>> = BTreeMap::new();
    // Leaflets each leaf structure holds: a fern's pinnae on its rachis.
    let mut leaflets: BTreeMap<u32, Vec<&GraphOrgan>> = BTreeMap::new();
    for organ in &drawn.organs {
        if grown.later(organ) {
            continue;
        }
        let term = grown.term(organ);
        match organ.within {
            Some(structure) if term == organs::PIECE => {
                pieces.entry(structure).or_default().push(organ);
            }
            Some(structure) if term == "leaflet" => {
                leaflets.entry(structure).or_default().push(organ);
            }
            None if term == "leaf" => {
                if let Some((look, size)) = leaf_look(organ, looks) {
                    leaf_sizes(&look.shape, size, organ.id, tally);
                }
            }
            _ => {}
        }
    }
    for (structure, members) in &leaflets {
        let leafy = drawn
            .structures
            .get(*structure as usize)
            .and_then(|s| grown.growth.structure_types.get(usize::from(s.structure)))
            .is_some_and(|kind| kind.term == "leaf");
        if leafy && !pieces.contains_key(structure) {
            frond_sizes(*structure, members, drawn, looks, tally);
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

/// A leaf drawn as a structure whose axis is its own segments and whose
/// leaflets stand on them (a fern's frond: its stipe and rachis, and its
/// pinnae): the stipe is the axis below the lowest leaflet, the blade the
/// rest, and the blade's width twice the farthest a leaflet reaches out
/// from the axis.
fn frond_sizes(
    structure: u32,
    leaflets: &[&GraphOrgan],
    drawn: &PlantGraph,
    looks: &[Look],
    tally: &mut Tally,
) {
    let bearing: BTreeSet<u32> = leaflets.iter().filter_map(|organ| organ.segment).collect();
    let (mut stipe, mut blade) = (0.0, 0.0);
    let mut first = true;
    for (index, segment) in (0_u32..).zip(&drawn.segments) {
        if segment.within != Some(structure) {
            continue;
        }
        let length = (segment.end - segment.start).length();
        // The rachis is drawn base first: bare pieces up to the first
        // that bears a leaflet are the stipe.
        if first && !bearing.contains(&index) {
            stipe += length;
        } else {
            first = false;
            blade += length;
        }
    }
    let mut width: f64 = 0.0;
    for organ in leaflets {
        let Some((_, size)) = leaf_look(organ, looks) else {
            continue;
        };
        let axis = organ
            .segment
            .and_then(|s| drawn.segments.get(s as usize))
            .map_or(organ.heading, |s| {
                (s.end - s.start).normalize_or(organ.heading)
            });
        let along = organ.heading.dot(axis).clamp(-1.0, 1.0);
        width = width.max(2.0 * size * (1.0 - along * along).sqrt());
    }
    let id = drawn.structures[structure as usize].id;
    tally.add("leaf_length_m", id, 0, blade);
    tally.add("leaf_width_m", id, 0, width);
    tally.add("petiole_length_m", id, 0, stipe);
    // Leaflets drawn as frond cards are the frond's pinnae, each divided
    // again; drawn whole, they are its leaflets.
    let again = leaflets
        .first()
        .and_then(|organ| leaf_look(organ, looks))
        .map_or(0, |(look, _)| division_order(&look.shape));
    let count = u32::try_from(leaflets.len()).unwrap_or(u32::MAX);
    tally.count(if again > 0 { "pinnae" } else { "leaflets" }, id, 0, count);
    tally.count("division_order", id, 0, 1 + again);
    tally.kind("division_pattern", "pinnate");
}

/// How many times a leaf drawn as `shape` divides (`division_order`): a
/// divided leaf once, or twice where it bears pinnae; lobes once; a frond
/// card once, and again where its pinnae are cut at least halfway to
/// their midribs (its `divided` cuts them three quarters of the way at
/// 1); a leaf drawn whole 0.
fn division_order(shape: &Shape) -> u32 {
    match shape {
        Shape::Compound(compound) => 1 + u32::from(compound.pinnules.unwrap_or(0) > 0),
        Shape::Frond(frond) => 1 + u32::from(frond.divided >= 0.5),
        Shape::Lobed(_) | Shape::Palmate(_) => 1,
        _ => 0,
    }
}

/// The look a leaf is drawn with in its light, and its drawn size.
fn leaf_look<'a>(organ: &GraphOrgan, looks: &'a [Look]) -> Option<(&'a Look, f64)> {
    let own = usize::from(organ.organ);
    let look = looks.get(own)?;
    let (index, share) = look.forms.of(own, organ);
    Some((looks.get(index).unwrap_or(look), organ.size * share))
}

/// A leaf card's blade, its width and its petiole, for a card `size` long,
/// and how many times it divides.
fn leaf_sizes(shape: &Shape, size: f64, id: u64, tally: &mut Tally) {
    let order = division_order(shape);
    if order > 0 {
        tally.count("division_order", id, 0, order);
    }
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
            let counts = crate::divided::counts(compound);
            for (part, leaflets) in (0_u32..).zip(&counts.leaflets) {
                tally.count("leaflets", id, part, *leaflets);
            }
            if counts.pinnae > 0 {
                tally.count("pinnae", id, 0, counts.pinnae);
            }
            if let Some(terminal) = counts.terminal {
                tally.kind("terminal_leaflet", if terminal { "true" } else { "false" });
            }
            let pattern = compound.pattern.unwrap_or_default();
            tally.kind(
                "division_pattern",
                match pattern {
                    crate::looks::Pattern::Pinnate => "pinnate",
                    crate::looks::Pattern::Palmate => "palmate",
                },
            );
            let d = crate::divided::dimensions(compound);
            (size * d.blade, size * d.width, size * d.petiole)
        }
        Shape::Blade(blade) => (size, size * blade.aspect, 0.0),
        // Palmate lobes radiate from the petiole's tip over the card;
        // sprays, sprigs and fronds are not one leaf's blade.
        Shape::Palmate(palmate) => {
            tally.kind("division_pattern", "palmate");
            let blade = size * (1.0 - palmate.petiole);
            (blade, size * shape.aspect(), size * palmate.petiole)
        }
        _ => return,
    };
    tally.add("leaf_length_m", id, 0, blade);
    tally.add("leaf_width_m", id, 0, width);
    tally.add("petiole_length_m", id, 0, petiole);
}

/// A conifer's needle spray (spec rework E6d): the needles to a bundle,
/// one where they stand in two ranks, as the card draws a bundle only
/// all round a shoot, and how they stand.
fn needle_counts(needles: &crate::looks::Needles, id: u64, tally: &mut Tally) {
    let two_ranked = needles.ranks == 2;
    let bundle = if two_ranked { 1 } else { needles.bundle.max(1) };
    tally.count("needles_per_fascicle", id, 0, bundle);
    tally.kind(
        "foliage_ranks",
        if two_ranked {
            "two_ranked"
        } else {
            "all_round"
        },
    );
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
        // The `inflorescence` character groups a shoot's flowers: a
        // cluster of fruit, drawn on the days its units are fruit, is
        // what the flowers became and may be drawn otherwise.
        if in_stage(phase, Stage::Open)
            && !matches!(cluster.unit, crate::clusters::Unit::Fruit { .. })
        {
            // A spike in a spathe is a spadix (PO:0030120).
            let kind = if cluster.cluster.kind == crate::looks::ClusterKind::Spike
                && !layout.sheets.is_empty()
            {
                "spadix"
            } else {
                cluster.cluster.kind.name()
            };
            tally.kind("inflorescence", kind);
        }
        if in_stage(phase, Stage::Open) {
            tally.add(
                "inflorescence_length_m",
                organ.id,
                0,
                organ.size * layout.length(cluster.unit),
            );
            tally.count(
                "units_per_inflorescence",
                organ.id,
                0,
                u32::try_from(layout.units.len()).unwrap_or(u32::MAX),
            );
            tally.count(
                "inflorescence_bracts",
                organ.id,
                0,
                u32::try_from(layout.sheets.len()).unwrap_or(u32::MAX),
            );
            for (index, sheet) in (0_u32..).zip(&layout.sheets) {
                tally.add("bract_length_m", organ.id, index, organ.size * sheet.length);
                tally.kind("bract_form", "spathe");
            }
            if matches!(
                cluster.cluster.kind,
                crate::looks::ClusterKind::Panicle | crate::looks::ClusterKind::CompoundUmbel
            ) {
                tally.count(
                    "inflorescence_branches",
                    organ.id,
                    0,
                    cluster.cluster.branches,
                );
            }
        }
        for (index, unit) in layout.units.iter().enumerate() {
            let part = u32::try_from(index + 1).unwrap_or(u32::MAX);
            // A unit in flower: the look's own shape, or its flower look
            // where its units open by the day.
            let shown = match &cluster.opening {
                Some(opening) => opening
                    .shown(unit)
                    .filter(|(look, _)| *look == 1)
                    .and_then(|(look, _)| opening.looks[look].as_ref())
                    .map(|stage| &stage.shape),
                None => in_stage(phase, Stage::Open).then_some(&look.shape),
            };
            if let Some(Shape::Flower(flower)) = shown {
                flower_parts(flower, organ.size * unit.length, (organ.id, part), tally);
            }
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
    if let Shape::Flower(flower) = &look.shape
        && in_stage(phase, Stage::Open)
        && !grown.clustered(organ)
    {
        flower_parts(flower, organ.size, (organ.id, 0), tally);
    }
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
                head_counts(head, (organ, part), tally);
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
            tally.count("florets_per_spikelet", organ, part, spikelet.florets.max(1));
            tally.count("glumes", organ, part, spikelet.glumes.min(2));
            let awned = spikelet.awn > 0.0 && spikelet.awned > 0.0;
            tally.kind("awns", if awned { "true" } else { "false" });
            tally.kind(
                "scale_arrangement",
                if spikelet.ranks == 2 {
                    "distichous"
                } else {
                    "spiral"
                },
            );
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

/// A flower `width` across, its parts as its layout draws them (spec
/// rework P5): the petals or tepals in a whorl (a corolla's one whorl of
/// petals, half of two whorls of tepals; a spiral has none), its
/// stamens, staminodes and carpels, a spiral's tepals, its spurs' length,
/// and the kind of its perianth, its ovary's place and how its petals
/// differ by place.
fn flower_parts(
    flower: &crate::looks::Flower,
    width: f64,
    (organ, part): (u64, u32),
    tally: &mut Tally,
) {
    use crate::looks::{Corolla, Ovary, Perianth};
    tally.count("stamens", organ, part, flower.stamens);
    tally.count("staminodes", organ, part, flower.staminodes);
    tally.count("carpels", organ, part, flower.carpels);
    let floral = crate::flowers::layout(
        flower,
        &crate::blooms::FlowerForm::default(),
        0.5 * width,
        0.0,
    );
    let spur = floral
        .spurs
        .iter()
        .map(|rod| rod.length)
        .fold(0.0, f64::max);
    tally.add("spur_length_m", organ, part, spur);
    let perianth = match flower.perianth {
        Perianth::Corolla => {
            tally.count("flower_merosity", organ, part, flower.petals);
            "sepals_and_petals"
        }
        Perianth::Tepals => {
            tally.add(
                "flower_merosity",
                organ,
                part,
                f64::from(flower.petals) / 2.0,
            );
            "tepals"
        }
        Perianth::Spiral => {
            tally.count("perianth_parts", organ, part, flower.petals);
            "spiral"
        }
        Perianth::Absent => "absent",
    };
    tally.kind("perianth", perianth);
    tally.kind(
        "ovary_position",
        match flower.ovary {
            Ovary::Superior => "superior",
            Ovary::Half => "half_inferior",
            Ovary::Inferior => "inferior",
        },
    );
    tally.kind(
        "zygomorphy",
        match flower.corolla {
            Corolla::Regular | Corolla::Unequal => "other",
            Corolla::Papilionaceous => "papilionaceous",
            Corolla::Bilabiate => "bilabiate",
            Corolla::Labellate => "labellate",
        },
    );
}

/// A head's florets and phyllaries, as its layout draws them, and its
/// kind.
fn head_counts(head: &crate::looks::Head, (organ, part): (u64, u32), tally: &mut Tally) {
    use crate::looks::HeadKind;
    let florets = head.florets.max(1);
    match head.kind {
        HeadKind::Radiate => {
            tally.count("ray_florets", organ, part, head.rays);
            tally.count("disc_florets", organ, part, florets);
        }
        HeadKind::Discoid => tally.count("disc_florets", organ, part, florets),
        HeadKind::Ligulate => tally.count("ligulate_florets", organ, part, florets),
    }
    tally.count("phyllaries", organ, part, head.phyllaries);
    if head.phyllaries > 0 {
        tally.count(
            "phyllary_series",
            organ,
            part,
            head.series.clamp(1, head.phyllaries),
        );
    }
    tally.kind(
        "capitulum_kind",
        match head.kind {
            // A radiate head drawn without rays is a disc alone.
            HeadKind::Radiate if head.rays > 0 => "radiate",
            HeadKind::Radiate | HeadKind::Discoid => "discoid",
            HeadKind::Ligulate => "liguliflorous",
        },
    );
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
    first_flower_by: Option<f64>,
) -> Result<Plant, String> {
    let years = spec.growth.years;
    let (program, _) = spec
        .program_in(library)
        .map_err(|error| error.to_string())?;
    let reproductive: Vec<bool> = program.organs().map(|(_, kind)| bears_seed(kind)).collect();
    let mut first = FirstFlower {
        reproductive: &reproductive,
        until: first_flower_by.unwrap_or(-1.0),
        born: None,
    };
    let growth = crate::drawing::grow_variant_watched(
        spec,
        library,
        None,
        environment,
        seed,
        vec![years],
        years,
        Some(&mut first),
    )?;
    let graph = growth.keyframes.last().ok_or("nothing grew")?;
    let grown = Grown {
        spec,
        growth: &growth,
        graph,
        terms: organs::of_program(&program, &spec.appearance),
        later: later_types(&program),
    };
    let (mut measures, kinds) = measure(&grown);
    if let Some(born) = first.born {
        measures.insert("juvenile_period_years", vec![born]);
    }
    let juvenile_flowers = graph
        .organs
        .iter()
        .filter(|organ| {
            organ.adult.is_some_and(|adult| adult < 0.5)
                && growth
                    .organ_types
                    .get(usize::from(organ.organ))
                    .is_some_and(|kind| matches!(kind.kind, OrganKind::Flower | OrganKind::Fruit))
        })
        .count();
    Ok(Plant {
        seed,
        age: graph.age,
        measures,
        kinds,
        juvenile_flowers,
    })
}

/// Whether organs of `kind` are a plant's reproduction: flowers, fruit
/// and cones.
fn bears_seed(kind: OrganKind) -> bool {
    matches!(kind, OrganKind::Flower | OrganKind::Fruit | OrganKind::Cone)
}

/// Watches a plant grow for the birth of its first flower, fruit or cone
/// (spec rework P6), step by step up to age `until`.
struct FirstFlower<'a> {
    /// Each organ type, whether it is reproductive.
    reproductive: &'a [bool],
    until: f64,
    born: Option<f64>,
}

impl crate::grow::Watch for FirstFlower<'_> {
    fn wants_frame(&mut self, _step: u32) -> bool {
        self.born.is_none() && self.until >= 0.0
    }

    fn step(&mut self, progress: crate::grow::Progress<'_>) -> bool {
        // The age it is first seen at: an organ an interpretation draws
        // (a cactus's ring of flowers) is as old as the module it draws.
        if let Some(graph) = progress.frame.and_then(|frame| frame.keyframes.last())
            && graph.organs.iter().any(|organ| {
                organ.size > 0.0
                    && self
                        .reproductive
                        .get(usize::from(organ.organ))
                        .copied()
                        .unwrap_or(false)
            })
        {
            self.born = Some(graph.age);
        }
        if progress.age > self.until {
            self.until = -1.0;
        }
        true
    }
}

/// A species' report: each stated size character judged against plants
/// grown from several seeds.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub species: String,
    pub environment: Environment,
    pub seeds: Vec<u64>,
    pub age: f64,
    /// Sizes, then counts.
    pub findings: Vec<Finding>,
    pub kinds: Vec<Agreement>,
    /// Size characters the species states that nothing here measures yet.
    pub unmeasured: Vec<String>,
    /// Flowers and fruit on juvenile shoots, over all the plants: each an
    /// error (spec rework P6).
    pub juvenile_flowers: usize,
}

/// A kind character judged against what the plants draw: a pass when
/// every state drawn is one the species states (plainly, or qualified,
/// however rarely), an error otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    pub character: &'static str,
    pub stated: Vec<String>,
    pub drawn: BTreeSet<String>,
    pub verdict: Verdict,
}

/// The states a kind character's value names: a plain state, or each of
/// its qualified states.
#[must_use]
pub fn states_of(value: &Value) -> Vec<String> {
    match value {
        Value::String(state) => vec![state.clone()],
        Value::Bool(state) => vec![state.to_string()],
        Value::Object(qualified) => qualified.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

impl Report {
    /// The worst verdict, `Pass` when there are no findings.
    #[must_use]
    pub fn worst(&self) -> Verdict {
        self.findings
            .iter()
            .map(|finding| finding.verdict)
            .chain(self.kinds.iter().map(|agreement| agreement.verdict))
            .chain((self.juvenile_flowers > 0).then_some(Verdict::Error))
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

/// Whether `spec`, with its `traits`, states any character [`SIZES`],
/// [`COUNTS`] or [`KINDS`] checks, or is held to one where it states none
/// ([`stated`]).
#[must_use]
pub fn states_checked(spec: &PlantSpec, traits: &Map<String, Value>) -> bool {
    SIZES
        .iter()
        .map(|(key, _)| *key)
        .chain(COUNTS)
        .chain(KINDS)
        .any(|key| traits.contains_key(key) || stated(spec, traits, key).is_some())
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
    // A plant is watched for its first flowers up to the stated juvenile
    // period's upper extreme: one later is an error however much later.
    let first_flower_by = traits
        .get("juvenile_period_years")
        .and_then(Stated::of)
        .map(|stated| stated.extremes().1);
    let plants = seeds
        .iter()
        .map(|&seed| grow_and_measure(spec, library, environment, seed, first_flower_by))
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
    let counts = COUNTS.iter().map(|&count| (count, Judged::Organs));
    for (character, judged) in SIZES.into_iter().chain(counts) {
        let Some(stated) = stated(spec, traits, character) else {
            continue;
        };
        let mut values = Vec::new();
        let mut with = 0;
        for plant in plants {
            if let Some(found) = plant.measures.get(character) {
                with += 1;
                if judged == Judged::Typical {
                    // The plant's usual value: its one value, or the
                    // median of its organs' (a cactus's joints, spines).
                    let mut found = found.clone();
                    found.sort_by(f64::total_cmp);
                    values.push(rank(&found, 0.5));
                } else {
                    values.extend_from_slice(found);
                }
            }
        }
        findings.push(judge(character, judged, stated, &mut values, with));
    }
    let kinds = KINDS
        .iter()
        .filter_map(|&character| {
            let stated = states_of(traits.get(character)?);
            let drawn: BTreeSet<String> = plants
                .iter()
                .filter_map(|plant| plant.kinds.get(character))
                .flatten()
                .cloned()
                .collect();
            let verdict = if drawn.is_empty() {
                Verdict::NotGrown
            } else if drawn.iter().all(|state| stated.contains(state)) {
                Verdict::Pass
            } else {
                Verdict::Error
            };
            Some(Agreement {
                character,
                stated,
                drawn,
                verdict,
            })
        })
        .collect();
    let vocabulary = crate::traits::Vocabulary::builtin();
    let unmeasured = traits
        .keys()
        .filter(|key| {
            vocabulary.traits.get(key.as_str()).is_some_and(|t| {
                t.group == "size" || t.unit.as_deref() == Some("m") || t.kind == "count"
            })
        })
        .filter(|key| !SIZES.iter().any(|(size, _)| size == key))
        .filter(|key| !COUNTS.contains(&key.as_str()))
        .filter(|key| !matches!(key.as_str(), "elevation_min_m" | "elevation_max_m"))
        .cloned()
        .collect();
    Report {
        species: spec.id.clone(),
        environment,
        seeds: plants.iter().map(|plant| plant.seed).collect(),
        age: plants.first().map_or(0.0, |plant| plant.age),
        findings,
        kinds,
        unmeasured,
        juvenile_flowers: plants.iter().map(|plant| plant.juvenile_flowers).sum(),
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

/// A value of `character` for a report: a length (a character in metres)
/// or a count.
#[must_use]
pub fn value_text(character: &str, value: f64) -> String {
    if character.ends_with("_m") {
        length_text(value)
    } else if character.starts_with("crown_") {
        format!("{value:.2}")
    } else if character.ends_with("_deg") {
        format!("{value:.0}°")
    } else if let Some(years) = character.strip_suffix("_years") {
        format!("{} years", value_text(years, value))
    } else if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// A stated range of `character` for a report: "(low–)min–max(–high)".
#[must_use]
pub fn range_text(character: &str, stated: &Stated) -> String {
    // Years are named once, after the range.
    if let Some(bare) = character.strip_suffix("_years") {
        return format!("{} years", range_text(bare, stated));
    }
    let length_text = |value: f64| value_text(character, value);
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
    fn a_flower_counts_its_parts_and_draws_its_kinds() {
        use crate::looks::{Corolla, Flower, Ovary, Perianth};
        let mut tally = Tally::default();
        let cactus = Flower {
            perianth: Perianth::Spiral,
            petals: 24,
            stamens: 200,
            carpels: 12,
            ovary: Ovary::Inferior,
            ..Flower::default()
        };
        flower_parts(&cactus, 0.1, (1, 0), &mut tally);
        let pea = Flower {
            corolla: Corolla::Papilionaceous,
            stamens: 10,
            carpels: 1,
            ..Flower::default()
        };
        flower_parts(&pea, 0.02, (2, 0), &mut tally);
        let (measures, kinds) = tally.measures();
        assert_eq!(measures["stamens"], [200.0, 10.0]);
        assert_eq!(measures["carpels"], [12.0, 1.0]);
        assert_eq!(measures["perianth_parts"], [24.0]);
        let states = |key: &str| kinds[key].iter().cloned().collect::<Vec<_>>();
        assert_eq!(states("perianth"), ["sepals_and_petals", "spiral"]);
        assert_eq!(states("ovary_position"), ["inferior", "superior"]);
        assert_eq!(states("zygomorphy"), ["other", "papilionaceous"]);
    }

    #[test]
    fn a_whorl_holds_a_corollas_petals_or_half_the_tepals() {
        use crate::looks::{Corolla, Flower, Perianth};
        let mut tally = Tally::default();
        let lily = Flower {
            perianth: Perianth::Tepals,
            petals: 6,
            ..Flower::default()
        };
        flower_parts(&lily, 0.1, (1, 0), &mut tally);
        // A columbine spurs every petal; a pea's keel draws no spur.
        let columbine = Flower {
            spur: 1.5,
            ..Flower::default()
        };
        flower_parts(&columbine, 0.04, (2, 0), &mut tally);
        let pea = Flower {
            corolla: Corolla::Papilionaceous,
            spur: 1.0,
            ..Flower::default()
        };
        flower_parts(&pea, 0.02, (3, 0), &mut tally);
        let water_lily = Flower {
            perianth: Perianth::Spiral,
            petals: 24,
            ..Flower::default()
        };
        flower_parts(&water_lily, 0.1, (4, 0), &mut tally);
        let (measures, _) = tally.measures();
        assert_eq!(measures["flower_merosity"], [3.0, 5.0, 5.0]);
        let [spur] = measures["spur_length_m"][..] else {
            panic!("{:?}", measures["spur_length_m"]);
        };
        assert!((spur - 0.03).abs() < 1e-12, "{spur}");
    }

    #[test]
    fn a_leaf_divides_once_or_again() {
        use crate::looks::{Compound, Frond, Lobed};
        let once = Compound::default();
        let twice = Compound {
            pinnules: Some(5),
            ..Compound::default()
        };
        let bracken = Frond {
            divided: 1.0,
            ..Frond::default()
        };
        let orders = [
            Shape::Compound(once),
            Shape::Compound(twice),
            Shape::Frond(Frond::default()),
            Shape::Frond(bracken),
            Shape::Lobed(Lobed::default()),
        ]
        .map(|shape| division_order(&shape));
        assert_eq!(orders, [1, 2, 1, 2, 1]);
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
    fn a_short_shoots_crowded_leaves_are_no_whorl() {
        // A broadleaf's lateral buds leaf out as short shoots, crowding
        // each year's leaves where the bud stands: thimbleberry's
        // alternate leaves and Oregon ash's opposite pairs read as whorls
        // when those count as the stem's node.
        let library = Library::builtin();
        for (id, arrangement) in [
            ("rubus-parviflorus", "alternate"),
            ("fraxinus-latifolia", "opposite"),
        ] {
            let spec = library.spec(id).unwrap();
            let environment = crate::drawing::typical_environment(&spec, library);
            let plant = grow_and_measure(&spec, library, environment, spec.variants.seeds[0], None)
                .unwrap();
            let states: Vec<&str> = plant.kinds["leaf_arrangement"]
                .iter()
                .map(String::as_str)
                .collect();
            assert_eq!(states, [arrangement], "{id}");
        }
    }

    #[test]
    fn seeds_start_with_the_variants_own() {
        let spec = Library::builtin().spec("achillea-millefolium").unwrap();
        assert_eq!(seeds_of(&spec, 3), vec![1, 2, 3]);
    }

    #[test]
    fn kinds_read_plain_and_qualified_states() {
        assert_eq!(states_of(&json!("panicle")), vec!["panicle"]);
        assert_eq!(states_of(&json!(true)), vec!["true"]);
        assert_eq!(
            states_of(&json!({"opposite": "usually", "alternate": "rarely"})),
            vec!["alternate", "opposite"]
        );
        assert!(states_of(&json!(3)).is_empty());
    }

    #[test]
    fn counts_and_turns_read_as_numbers() {
        assert_eq!(value_text("ray_florets", 8.0), "8");
        assert_eq!(value_text("leaflets", 6.5), "6.5");
        assert_eq!(value_text("divergence_deg", 137.5), "138°");
        assert_eq!(value_text("lemma_length_m", 0.004), "4.0 mm");
        let ten = stated(&json!({"min": 5, "max": 8, "low": 3}));
        assert_eq!(range_text("ray_florets", &ten), "(3–)5–8");
        let mut rays = vec![5.0, 6.0, 7.0, 8.0, 8.0];
        let pass = judge("ray_florets", Judged::Organs, ten, &mut rays, 1);
        assert_eq!(pass.verdict, Verdict::Pass, "{pass:?}");
        let mut two = vec![2.0, 6.0];
        let error = judge("ray_florets", Judged::Organs, ten, &mut two, 1);
        assert_eq!(error.verdict, Verdict::Error, "{error:?}");
    }

    #[test]
    fn a_leaf_carried_up_a_bending_axis_keeps_its_side() {
        // An axis bending from upright to leaning along +X carries a leaf
        // pointing along -Z unchanged, and one along +X down below it.
        let up = Vec3::Y;
        let leaning = Vec3::new(1.0, 1.0, 0.0).normalize_or(Vec3::Y);
        let side = carried(Vec3::new(0.0, 0.0, -1.0), up, leaning);
        assert!(
            (side - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-9,
            "{side:?}"
        );
        let out = carried(Vec3::X, up, leaning);
        assert!(out.dot(leaning).abs() < 1e-9, "{out:?}");
        assert!(out.y < 0.0, "{out:?}");
    }
}
