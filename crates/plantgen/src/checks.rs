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
//! again), counted. A plant with leaves at its foot (a rosette's, a
//! tiller's) names its stem leaves `reduced`, `like_basal` or `none` by
//! their median length (`cauline_leaves`); where they are reduced, its
//! leaf sizes are measured on the leaves at its foot, as floras measure a
//! herb's basal leaves and call the stem's reduced. Heads,
//! spikelets, divided leaves and clusters measure their parts from their
//! own layouts ([`crate::capitula::dimensions`],
//! [`crate::spikelets::dimensions`], [`crate::divided::dimensions`],
//! [`crate::clusters::Layout::length`]). Each organ is measured on a day
//! it is in the phase its character describes ([`crate::organs::phase`]):
//! flowers, heads, spikelets and inflorescences open, fruit and cones
//! ripe, and every organ without a season on the default day. An organ is
//! measured once per character, on the first of those days it qualifies.
//! A male plant's cone (`<organ>_male`) is measured as its pollen cone,
//! and a seed borne outside a fruit (a cycad's) as a seed.
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
//! draw is one the species states, plainly or qualified: a cluster's kind,
//! its units and how far its axis bends over while its units are
//! flowers, a head's kind, awns, a spikelet's ranks, a divided leaf's
//! pattern and its end, what a leaf is made as and whether its blade is
//! one piece, a leaf's shape (its blade's outline read by the chart of
//! plane shapes, a divided leaf's leaflets', a frond's whole blade over
//! its pinnae's tips), a head's receptacle, where an awn leaves its
//! lemma and how it bends (each the nearest word its rule draws), a
//! cactus's stem, tubercles and main central spine, and a fruit's type;
//! where a drawing does not tell states apart (an ellipsoid fruit, a
//! berry's or a drupe's) it draws each of them. A cone's posture is its pitch down from its shoot, by the
//! plant's usual cone. How the leaves stand is measured
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
//! its ovary's place, its corolla's zygomorphy and so its symmetry, its
//! corolla's shape, whether a floral cup bears its whorls and whether it
//! is turned half round, as its layout has them (spec rework P5).
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
//!
//! **Leaf and cone lives (checks, 2026-10-13).** A plant is also kept
//! [`LIFE_YEARS`] before the end of growth (halfway on a shorter life),
//! where each organ carries the age it was shed at. A leaf shed by the
//! end lived from its birth to then (`leaf_life_years`, each leaf judged
//! as an organ's size is; a leaf drawn in pieces, a fern's frond, as its
//! structure); one that fell with its limb, or still stands at the end,
//! is not counted. A plant is evergreen (`leaf_habit`) where a leaf stands
//! on it into its second year, else deciduous; a grass blade turned to
//! thatch is a dead leaf, not one kept. A cone takes to ripen
//! (`cone_maturation_years`) the seasons the youngest cone drawn ripe has
//! stood, its first counted, one value for the plant, and stays once ripe
//! (`cone_retention_years`) its life less those.
//!
//! **The plant's year (development, 2026-10-14).** The months its flowers
//! are open in (`flowering_months`) and its fruit is ripe in
//! (`fruiting_months`) are read from its looks on every day of the year,
//! for each type of organ standing on it at the end ([`MONTHS`]). An organ
//! with a season shows flowers while it, or any of its units or florets,
//! is in flower, and ripe fruit once its fruit is half ripened; one
//! without shows its own look all year. A plant none of whose looks has a
//! season draws no year, and its months are not grown. The months drawn
//! pass when they are the months stated, warn when they differ only by a
//! month at either end, and are an error otherwise ([`Timing`]).

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
pub const SIZES: [(&str, Judged); 43] = [
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
    ("fertile_leaf_length_m", Judged::Organs),
    ("fertile_petiole_length_m", Judged::Organs),
    ("inflorescence_length_m", Judged::Organs),
    ("peduncle_length_m", Judged::Organs),
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
    ("cone_width_m", Judged::Organs),
    ("pollen_cone_length_m", Judged::Organs),
    ("pollen_cone_width_m", Judged::Organs),
    ("seed_length_m", Judged::Organs),
    ("seed_width_m", Judged::Organs),
    ("divergence_deg", Judged::Typical),
    ("juvenile_period_years", Judged::Typical),
    ("leaf_life_years", Judged::Organs),
    ("cone_maturation_years", Judged::Typical),
    ("cone_retention_years", Judged::Organs),
    ("crown_outline_fit", Judged::Typical),
    ("crown_see_through", Judged::Typical),
];

/// The count characters counted on grown plants (spec rework M8A), each
/// on every organ that holds what it counts and judged as an organ's size
/// is.
pub const COUNTS: [&str; 25] = [
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
    "pinnules",
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
pub const KINDS: [&str; 34] = [
    "inflorescence",
    "inflorescence_unit",
    "inflorescence_posture",
    "inflorescence_outline",
    "bract_form",
    "capitulum_kind",
    "awns",
    "scale_arrangement",
    "division_pattern",
    "terminal_leaflet",
    "leaf_division",
    "leaf_form",
    "leaf_shape",
    "receptacle_shape",
    "awn_insertion",
    "awn_form",
    "stem_form",
    "tubercles",
    "central_spine_form",
    "leaf_arrangement",
    "phyllotaxis_pattern",
    "perianth",
    "ovary_position",
    "flower_symmetry",
    "zygomorphy",
    "corolla_shape",
    "hypanthium",
    "resupinate",
    "fruit_type",
    "cone_posture",
    "glochids",
    "foliage_ranks",
    "leaf_habit",
    "cauline_leaves",
];

/// The months characters read from a grown plant's year (development,
/// 2026-10-14): the months its flowers are open in, and its fruit ripe in.
pub const MONTHS: [&str; 2] = ["flowering_months", "fruiting_months"];

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

/// A fertile flower's parts, which a sterile flower lacks: a cluster
/// whose flowers are all sterile (`sterile_flowers`, a mophead
/// hydrangea's) draws none, so its stated counts are not judged on it.
const FERTILE_PARTS: [&str; 3] = ["stamens", "staminodes", "carpels"];

/// `character` as `spec` states it in `traits`, or as a check holds it
/// where it states none ([`TREE_OUTLINE_FIT`]); none for a fertile
/// flower's parts ([`FERTILE_PARTS`]) where every flower is sterile.
#[must_use]
pub fn stated(spec: &PlantSpec, traits: &Map<String, Value>, character: &str) -> Option<Stated> {
    if FERTILE_PARTS.contains(&character)
        && traits.get("sterile_flowers").and_then(Value::as_str) == Some("all")
    {
        return None;
    }
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

    /// A span of years, which may be none (a cone shed as it ripens).
    fn years(&mut self, character: &'static str, organ: u64, part: u32, years: f64) {
        if years.is_finite() && years >= 0.0 {
            self.seen.entry((character, organ, part)).or_insert(years);
        }
    }

    fn kind(&mut self, character: &'static str, state: &str) {
        self.kinds
            .entry(character)
            .or_default()
            .insert(state.to_owned());
    }

    /// Another tally's values and kinds, but for the characters `skip`.
    fn absorb(&mut self, other: Tally, skip: &[&str]) {
        for (key, value) in other.seen {
            if !skip.contains(&key.0) {
                self.seen.entry(key).or_insert(value);
            }
        }
        for (character, states) in other.kinds {
            self.kinds.entry(character).or_default().extend(states);
        }
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
    /// The plant [`LIFE_YEARS`] earlier, whose organs carry the age they
    /// were shed at, if by the end of growth.
    pub before: Option<&'a PlantGraph>,
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

    /// Whether the organ is a male plant's own (`<organ>_male`): a
    /// cycad's pollen cone.
    fn male(&self, organ: &GraphOrgan) -> bool {
        self.growth
            .organ_types
            .get(usize::from(organ.organ))
            .is_some_and(|kind| kind.name.ends_with(crate::spec::MALE))
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
    // Each ripe cone's pitch down from its shoot.
    let mut cone_pitches = BTreeMap::new();
    // The fewest seasons any cone is drawn ripe in, its first counted.
    let mut ripening: Option<f64> = None;
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
            if grown.term(organ) == "cone" && in_stage(phase, Stage::Ripe) {
                let seasons = libm::round(graph.age - organ.born) + 1.0;
                ripening = Some(ripening.map_or(seasons, |least| least.min(seasons)));
                if let Some(pitch) = pitch_from_shoot(organ, &drawn) {
                    cone_pitches.entry(organ.id).or_insert(pitch);
                }
            }
        }
    }
    // A cone's posture on its shoot, by the plant's usual cone: each is
    // drawn pitched a little more or less than the next.
    if !cone_pitches.is_empty() {
        let mut pitches: Vec<f64> = cone_pitches.into_values().collect();
        pitches.sort_by(f64::total_cmp);
        let pitch = rank(&pitches, 0.5);
        tally.kind(
            "cone_posture",
            if pitch < 0.0 {
                "erect"
            } else if pitch < 75.0 {
                "spreading"
            } else if pitch < 105.0 {
                "deflexed"
            } else {
                "pendent"
            },
        );
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
    // The seasons a cone takes to ripen: the youngest drawn ripe, one
    // value for the plant.
    if let Some(seasons) = ripening {
        tally.add("cone_maturation_years", 0, 0, seasons);
    }
    lives(grown, ripening, &mut tally);
    leaf_habit(grown, &mut tally);
    year(grown, &mut tally);
    tally.measures()
}

/// The structure termed `leaf` that `structure` is or is within (a fern's
/// frond, round a pinna), if any.
fn leaf_of(grown: &Grown<'_>, graph: &PlantGraph, structure: u32) -> Option<u32> {
    let mut next = Some(structure);
    // Each structure stands within one listed before it, so the walk ends.
    while let Some(index) = next {
        let at = graph.structures.get(index as usize)?;
        let term = grown
            .growth
            .structure_types
            .get(usize::from(at.structure))?;
        if term.term == "leaf" {
            return Some(index);
        }
        next = at.within;
    }
    None
}

/// How long the plant's leaves and cones live, read on the plant
/// [`LIFE_YEARS`] before the end of growth (see the module): each one
/// shed by the end lived from its birth to then, a leaf drawn in pieces
/// (a fern's frond) as its structure did. One that fell with the wood
/// bearing it (a shed limb) did not die of age and is not counted, nor is
/// one still on the plant at the end. A cone's years on the tree once
/// ripe are its life less the seasons it took to ripen (`ripening`).
fn lives(grown: &Grown<'_>, ripening: Option<f64>, tally: &mut Tally) {
    let Some(before) = grown.before else {
        return;
    };
    let segments = &before.segments;
    // Whether something shed at `shed` fell with the segment bearing it.
    let with_wood = |segment: Option<usize>, shed: f64| {
        segment
            .and_then(|index| segments.get(index))
            .and_then(|wood| wood.shed)
            .is_some_and(|wood| (wood - shed).abs() < 1e-9)
    };
    // Each frond's pieces' last shed (none while one stays) and a piece's
    // segment.
    let mut fronds: BTreeMap<u32, (Option<f64>, Option<usize>)> = BTreeMap::new();
    for organ in &before.organs {
        if grown.later(organ) {
            continue;
        }
        let segment = organ.segment.map(|s| s as usize);
        match organ.within.and_then(|s| leaf_of(grown, before, s)) {
            Some(frond) => {
                let (shed, piece) = fronds.entry(frond).or_insert((organ.shed, segment));
                *shed = shed.zip(organ.shed).map(|(a, b)| a.max(b));
                if piece.is_none() {
                    *piece = segment;
                }
            }
            None if organ.within.is_none() => {
                let Some(shed) = organ.shed.filter(|&shed| !with_wood(segment, shed)) else {
                    continue;
                };
                match grown.term(organ) {
                    "leaf" => tally.years("leaf_life_years", organ.id, 0, shed - organ.born),
                    "cone" => {
                        if let Some(ripening) = ripening {
                            let held = libm::round(shed - organ.born - ripening).max(0.0);
                            tally.years("cone_retention_years", organ.id, 0, held);
                        }
                    }
                    _ => {}
                }
            }
            None => {}
        }
    }
    for (frond, (shed, piece)) in fronds {
        let Some(shed) = shed else {
            continue;
        };
        // The segment the frond stands on, below its own.
        let mut segment = piece;
        while let Some(index) = segment
            && let Some(at) = segments.get(index)
            && at.within.and_then(|s| leaf_of(grown, before, s)) == Some(frond)
        {
            segment = at.parent.map(|p| p as usize);
        }
        if !with_wood(segment, shed) {
            let at = &before.structures[frond as usize];
            tally.years("leaf_life_years", at.id, 1, shed - at.born);
        }
    }
}

/// Whether the plant keeps leaves through the winter (`leaf_habit`): an
/// evergreen's stand on it into their second year or later, a deciduous
/// plant's live out one season. A leaf drawn in pieces is alive while any
/// of its pieces is not yet a later phase (a blade's thatch).
fn leaf_habit(grown: &Grown<'_>, tally: &mut Tally) {
    let graph = grown.graph;
    let lone = graph
        .organs
        .iter()
        .filter(|organ| {
            organ.within.is_none() && !grown.later(organ) && grown.term(organ) == "leaf"
        })
        .map(|organ| organ.born);
    // A leaf structure lives while it holds a living piece: a grass blade
    // turned to thatch is last year's dead leaf, not a leaf kept.
    let living: BTreeSet<u32> = graph
        .organs
        .iter()
        .filter(|organ| !grown.later(organ))
        .filter_map(|organ| organ.within)
        .collect();
    let fronds = (0_u32..)
        .zip(&graph.structures)
        .filter(|(index, at)| {
            living.contains(index)
                && grown
                    .growth
                    .structure_types
                    .get(usize::from(at.structure))
                    .is_some_and(|kind| kind.term == "leaf")
        })
        .map(|(_, at)| at.born);
    let mut oldest: Option<f64> = None;
    for born in lone.chain(fronds) {
        let age = graph.age - born;
        oldest = Some(oldest.map_or(age, |most| most.max(age)));
    }
    if let Some(oldest) = oldest {
        tally.kind(
            "leaf_habit",
            if oldest >= 1.0 - 1e-9 {
                "evergreen"
            } else {
                // Leaves shed every year read as either: no drought is grown.
                "deciduous|drought_deciduous"
            },
        );
    }
}

/// The first day of each month in a year of 365 days, and the day after
/// the year's last.
const MONTH_STARTS: [u16; 13] = [1, 32, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366];

/// The month, 1 to 12, of day `day` of the year (1 to 365).
fn month_of(day: u16) -> u8 {
    let past = MONTH_STARTS[1..12]
        .iter()
        .take_while(|&&start| start <= day)
        .count();
    u8::try_from(past + 1).unwrap_or(12)
}

/// Ranks of a cluster's units, or a head's florets, from the first to
/// open to the last, at which their stages are read.
const UNIT_RANKS: u32 = 16;

/// The months the plant's flowers are open in and its fruit is ripe in
/// (`flowering_months`, `fruiting_months`), from its year as it is drawn:
/// each type of organ standing on it at the end, on every day of the
/// year. An organ with a season shows on a day its bud look in bud, its
/// flower look in flower (its own, unless that is its fruit), and its own
/// look in fruit, ripe once half ripened; a cluster's units, or a head's
/// florets, each in its own stage. One with none shows its own look every
/// day. A plant none of whose looks has a season draws no year, so its
/// months are not read. Flowers are flowers, heads, umbels, spikes,
/// panicles and spikelets; fruit is a fruit card that is no cone.
fn year(grown: &Grown<'_>, tally: &mut Tally) {
    if grown
        .spec
        .appearance
        .organs
        .values()
        .all(|look| look.season.is_none())
    {
        return;
    }
    let graph = grown.graph;
    let standing: BTreeSet<u16> = graph
        .organs
        .iter()
        .filter(|organ| organ.size > 0.0 && !organ.shed.is_some_and(|shed| shed <= graph.age))
        .map(|organ| organ.organ)
        .collect();
    for index in standing {
        let Some(kind) = grown.growth.organ_types.get(usize::from(index)) else {
            continue;
        };
        let Some(look) = grown.spec.appearance.organs.get(&kind.name) else {
            continue;
        };
        let cone = grown.terms.get(usize::from(index)) == Some(&"cone");
        let own_flower = flowery(&look.shape);
        let own_fruit = matches!(look.shape, Shape::Fruit(_)) && !cone;
        let ranks: Vec<f64> =
            if look.inflorescence.is_some() || matches!(look.shape, Shape::Head(_)) {
                (0..=UNIT_RANKS)
                    .map(|rank| f64::from(rank) / f64::from(UNIT_RANKS))
                    .collect()
            } else {
                vec![0.0]
            };
        for day in 1..=365_u16 {
            let (flowers, ripe) = match &look.season {
                None => (own_flower, own_fruit),
                Some(season) => ranks.iter().fold((false, false), |(flowers, ripe), &rank| {
                    match season.unit_stage(f64::from(day), rank) {
                        crate::looks::Stage::Flower => (
                            flowers
                                || season.flower_look.is_some()
                                || !matches!(look.shape, Shape::Fruit(_)),
                            ripe,
                        ),
                        crate::looks::Stage::Fruit { unripe } => {
                            (flowers || own_flower, ripe || (own_fruit && unripe < 0.5))
                        }
                        _ => (flowers, ripe),
                    }
                }),
            };
            let month = month_of(day).to_string();
            if flowers {
                tally.kind("flowering_months", &month);
            }
            if ripe {
                tally.kind("fruiting_months", &month);
            }
        }
    }
}

/// Whether a card draws flowers: a flower, a head, an umbel, a spike, a
/// panicle or a spikelet.
fn flowery(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Flower(_)
            | Shape::Head(_)
            | Shape::Umbel(_)
            | Shape::Spike(_)
            | Shape::Panicle(_)
            | Shape::Spikelet(_)
    )
}

/// Degrees `organ` is pitched down from the shoot that bears it, in the
/// plane of the shoot and the vertical: negative where it stands up from
/// the shoot, past 90 where it hangs back along it. None on a vertical
/// shoot, which has no side down.
fn pitch_from_shoot(organ: &GraphOrgan, graph: &PlantGraph) -> Option<f64> {
    let segment = graph.segments.get(organ.segment? as usize)?;
    let shoot = (segment.end - segment.start).normalize_or(Vec3::ZERO);
    // Down, square to the shoot.
    let down = (shoot * shoot.y - Vec3::Y).normalize_or(Vec3::ZERO);
    if shoot == Vec3::ZERO || down == Vec3::ZERO {
        return None;
    }
    let heading = organ.heading;
    Some(libm::atan2(heading.dot(down), heading.dot(shoot)).to_degrees())
}

/// Years before the end of growth at which a plant is kept to see how
/// long its leaves and cones live: past the longest a built-in species
/// states (a redcedar's lower foliage, 10.6 years).
pub const LIFE_YEARS: f64 = 20.0;

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
    // The longest stem standing on the ground: a stem's form is a grown
    // one's, not a young shoot's from its foot.
    let mut stem: Option<(f64, f64)> = None;
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
        cactus_kinds(look, jointed, tally);
        if axis.root && stem.is_none_or(|(length, _)| axis.length > length) {
            stem = Some((axis.length, axis.widest));
        }
    }
    if !jointed
        && let Some((length, widest)) = stem
        && widest > 0.0
    {
        let long = length / (2.0 * widest);
        let mut forms = Vec::new();
        if long <= COLUMN {
            forms.push("globose");
        }
        if long >= BARREL {
            forms.push("columnar");
        }
        tally.kind("stem_form", &forms.join("|"));
    }
}

/// How long a cactus's stem is for its width at most to be a barrel
/// (`globose`), and at least to be a column (`columnar`); between, it is
/// read as either.
const BARREL: f64 = 2.0;
const COLUMN: f64 = 3.0;

/// A cactus body's kinds: what its stem is built as (`stem_form`), a
/// chain of joints, flattened into pads where the look draws its
/// cross-section thinner than 0.7 of its width, else cylindrical (a stem
/// of one body, its plant's longest standing on the ground, is a column
/// where it is at least [`COLUMN`] times as long as it is wide and a
/// barrel where at most [`BARREL`]); whether its areoles stand on
/// tubercles; and its main central spine's
/// form (`central_spine_form`; a central set's where none is the main
/// one): hooked where its last fifth turns back 90° or more (a
/// fishhook's 120° to 180°), curved where it bends back 15° or more over
/// its length, flattened where it is drawn 0.6 as thick as wide or less,
/// else straight. A twisted spine is not drawn.
fn cactus_kinds(look: &crate::body::BodyLook, jointed: bool, tally: &mut Tally) {
    use crate::body::SpineRole;
    if jointed {
        tally.kind(
            "stem_form",
            if look.flatten < 0.7 {
                "flat_joints"
            } else {
                "cylindric_joints"
            },
        );
    }
    let tubercles = look.tubercles.as_ref().is_some_and(|t| t.height > 0.0);
    tally.kind("tubercles", if tubercles { "true" } else { "false" });
    let main = look
        .spines
        .iter()
        .find(|set| set.role == Some(SpineRole::Main))
        .or_else(|| {
            look.spines
                .iter()
                .find(|set| set.role == Some(SpineRole::Central))
        });
    if let Some(set) = main {
        let mut forms = Vec::new();
        if set.curve >= 15.0 {
            forms.push("curved");
        }
        if set.flat <= 0.6 {
            forms.push("flattened");
        }
        if set.hook >= 90.0 {
            forms.push("hooked");
        }
        if forms.is_empty() {
            forms.push("straight");
        }
        tally.kind("central_spine_form", &forms.join("|"));
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
    // only those count: where a plant's stems bear leaves of their own,
    // only those, and not its rosette's at the ground. A plant whose stems
    // keep none (every tip stopped for some years, its leaves all on short
    // shoots) leaves them unmeasured: two short shoots at a pair of buds
    // crowd their leaves at one place, which would read as a whorl.
    let own = |segment: Option<usize>, organ: &GraphOrgan| {
        segment
            .and_then(|index| segments.get(index))
            .is_none_or(|at| (at.born - organ.born).abs() < 1e-9)
    };
    if holders
        .iter()
        .any(|(segment, _, _, organ, _)| segment.is_some() && own(*segment, organ))
    {
        holders.retain(|(segment, _, _, organ, _)| segment.is_some() && own(*segment, organ));
    } else {
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
    // The plant's usual leaves at a node, the count most of its leaves
    // stand in (a lily's whorls, not the few leaves scattered between
    // them), and each whorl's count.
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
        .max_by_key(|(leaves, nodes)| (**nodes * **leaves, std::cmp::Reverse(**leaves)))
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
/// `leaf` drawn in pieces. A plant with leaves at its foot names its stem
/// leaves (`cauline_leaves`) by their median length: reduced under
/// [`REDUCED`] of the basal leaves', else like them, and none where it has
/// none. Where they are reduced, its leaf sizes are those at its foot:
/// floras measure a herb's basal leaves and call the stem's reduced.
#[allow(clippy::too_many_lines)]
fn leaves(grown: &Grown<'_>, drawn: &PlantGraph, looks: &[Look], tally: &mut Tally) {
    let (mut ground, mut stem) = (Tally::default(), Tally::default());
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
                    let into = if organ.segment.is_none() {
                        &mut ground
                    } else {
                        &mut stem
                    };
                    leaf_sizes(&look.shape, size, organ.id, into);
                }
            }
            _ => {}
        }
    }
    let kind_of = |structure: u32| {
        drawn
            .structures
            .get(structure as usize)
            .map(|s| s.structure)
    };
    // Structure types whose leaves are drawn as leaflets (a palm's or a
    // cycad's frond): any other piece within one is what a dead frond
    // leaves, hanging or as a boot, not a blade.
    let feathered: BTreeSet<_> = leaflets.keys().filter_map(|&s| kind_of(s)).collect();
    for (structure, members) in &leaflets {
        let leafy = kind_of(*structure)
            .and_then(|kind| grown.growth.structure_types.get(usize::from(kind)))
            .is_some_and(|kind| kind.term == "leaf");
        // A dimorphic fern's fertile fronds bear its `fertile_` leaflets
        // and are measured against its fertile leaves' characters.
        let fertile = members.iter().any(|organ| {
            grown
                .growth
                .organ_types
                .get(usize::from(organ.organ))
                .is_some_and(|kind| kind.name.starts_with("fertile_"))
        });
        if leafy {
            let into = if at_ground(*structure, members, drawn) {
                &mut ground
            } else {
                &mut stem
            };
            frond_sizes(*structure, members, fertile, drawn, looks, into);
        }
    }
    for (structure, members) in pieces {
        let leafy = kind_of(structure)
            .and_then(|kind| grown.growth.structure_types.get(usize::from(kind)))
            .is_some_and(|kind| kind.term == "leaf");
        if !leafy || kind_of(structure).is_some_and(|kind| feathered.contains(&kind)) {
            continue;
        }
        let tally = if at_ground(structure, &members, drawn) {
            &mut ground
        } else {
            &mut stem
        };
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
        let mut outline = Vec::new();
        let mut cards = true;
        for piece in members {
            if let Some((look, size)) = leaf_look(piece, looks) {
                leaf_kinds(&look.shape, tally);
                if let Shape::Blade(card) = &look.shape {
                    for i in 0..=4 {
                        let y = f64::from(i) / 4.0;
                        let half = size * crate::templates::blade_half_width(card, y);
                        outline.push((length + size * y, half));
                    }
                } else {
                    cards = false;
                }
                length += size;
                width = width.max(size * look.shape.aspect());
            }
        }
        if cards && length > 0.0 {
            for point in &mut outline {
                point.0 /= length;
            }
            if let Some(words) = outline_shape(&outline, width / length) {
                tally.kind("leaf_shape", &words);
            }
        }
        tally.add("leaf_length_m", id, 0, length);
        tally.add("leaf_width_m", id, 0, width);
    }
    let sizes = ["leaf_length_m", "leaf_width_m", "petiole_length_m"];
    let lengths = |of: &Tally| {
        let mut lengths: Vec<f64> = of
            .seen
            .iter()
            .filter(|((character, _, _), _)| *character == "leaf_length_m")
            .map(|(_, length)| *length)
            .collect();
        lengths.sort_by(f64::total_cmp);
        lengths
    };
    let (at_foot, up_stems) = (lengths(&ground), lengths(&stem));
    let reduced = !at_foot.is_empty()
        && !up_stems.is_empty()
        && rank(&up_stems, 0.5) < REDUCED * rank(&at_foot, 0.5);
    if !at_foot.is_empty() {
        tally.kind(
            "cauline_leaves",
            if up_stems.is_empty() {
                "none"
            } else if reduced {
                "reduced"
            } else {
                "like_basal"
            },
        );
    }
    tally.absorb(ground, &[]);
    tally.absorb(stem, if reduced { &sizes } else { &[] });
}

/// Of the basal leaves' median length, the stem leaves' under which they
/// are reduced.
const REDUCED: f64 = 0.75;

/// Whether a leaf structure stands on the ground (a rosette's or a
/// tiller's leaf), not on a stem: the axis its first organ's segments,
/// followed out of the structure, stand on.
fn at_ground(structure: u32, members: &[&GraphOrgan], drawn: &PlantGraph) -> bool {
    let Some(first) = members.first() else {
        return false;
    };
    let mut segment = first.segment.map(|s| s as usize);
    while let Some(index) = segment
        && drawn.segments[index].within == Some(structure)
    {
        segment = drawn.segments[index].parent.map(|p| p as usize);
    }
    segment.is_none()
}

/// The share of its longest piece of blade a frond's shortest reaches once
/// it has unrolled: an unrolled frond's pieces are all alike, while one
/// still in its crozier, or still unrolling its tip, draws the pieces
/// beyond the share unrolled shorter, with smaller pinnae. (A frond lying
/// on the ground bends sharply where it meets it, but its pieces are
/// whole.)
const UNROLLED: f64 = 0.9;

/// A leaf drawn as a structure whose axis is its own segments and whose
/// leaflets stand on them (a fern's frond: its stipe and rachis, and its
/// pinnae): the stipe is the axis below the lowest leaflet, the blade the
/// rest, and the blade's width twice the farthest a leaflet reaches out
/// from the axis. A frond not yet unrolled (see [`UNROLLED`]) has no
/// outline or size yet. A frond whose stipe forks into arms (a pedate
/// blade) is measured as a fan ([`fan_sizes`]).
fn frond_sizes(
    structure: u32,
    leaflets: &[&GraphOrgan],
    fertile: bool,
    drawn: &PlantGraph,
    looks: &[Look],
    tally: &mut Tally,
) {
    let bearing: BTreeSet<u32> = leaflets.iter().filter_map(|organ| organ.segment).collect();
    let (mut stipe, mut blade) = (0.0, 0.0);
    let mut first = true;
    // How far up the rachis each of its segments starts.
    let mut starts: BTreeMap<u32, f64> = BTreeMap::new();
    // Its blade's shortest and longest segments.
    let (mut shortest, mut longest) = (f64::INFINITY, 0.0_f64);
    for (index, segment) in (0_u32..).zip(&drawn.segments) {
        if segment.within != Some(structure) {
            continue;
        }
        let length = (segment.end - segment.start).length();
        starts.insert(index, stipe + blade);
        // The rachis is drawn base first: bare pieces up to the first
        // that bears a leaflet are the stipe.
        if first && !bearing.contains(&index) {
            stipe += length;
        } else {
            first = false;
            blade += length;
            shortest = shortest.min(length);
            longest = longest.max(length);
        }
    }
    let mut width: f64 = 0.0;
    // The blade's outline: how far each leaflet reaches out, and where.
    let mut outline = Vec::with_capacity(leaflets.len());
    for organ in leaflets {
        let Some((_, size)) = leaf_look(organ, looks) else {
            continue;
        };
        let segment = organ.segment.and_then(|s| drawn.segments.get(s as usize));
        let axis = segment.map_or(organ.heading, |s| {
            (s.end - s.start).normalize_or(organ.heading)
        });
        let along = organ.heading.dot(axis).clamp(-1.0, 1.0);
        let reach = size * (1.0 - along * along).sqrt();
        width = width.max(2.0 * reach);
        if let (Some(index), Some(segment)) = (organ.segment, segment)
            && let Some(start) = starts.get(&index)
            && blade > 0.0
        {
            let up = start + (organ.position - segment.start).length() - stipe;
            outline.push((up / blade, reach));
        }
    }
    // Floras measure grown fronds: one still unrolling is neither its
    // outline nor its size yet.
    let unrolled = shortest >= UNROLLED * longest;
    let fan =
        forked(structure, &bearing, drawn).and_then(|fork| fan_sizes(fork, leaflets, drawn, looks));
    if let Some((length, across, _)) = fan {
        (blade, width) = (length, across);
    }
    let id = drawn.structures[structure as usize].id;
    if fertile {
        // A fertile frond's own sizes; its shape and counts are the
        // sterile fronds' to give.
        if unrolled {
            tally.add("fertile_leaf_length_m", id, 0, blade);
            tally.add("fertile_petiole_length_m", id, 0, stipe);
        }
        return;
    }
    let words = match &fan {
        Some((_, _, words)) => words.clone(),
        None if blade > 0.0 => outline_shape(&outline, width / blade),
        None => None,
    };
    if unrolled && let Some(words) = words {
        tally.kind("leaf_shape", &words);
    }
    if unrolled && blade > 0.0 {
        tally.add("leaf_length_m", id, 0, blade);
        tally.add("leaf_width_m", id, 0, width);
        tally.add("petiole_length_m", id, 0, stipe);
    }
    // Leaflets drawn as frond cards are the frond's pinnae, each divided
    // again; drawn whole, they are its leaflets.
    let again = leaflets
        .first()
        .and_then(|organ| leaf_look(organ, looks))
        .map_or(0, |(look, _)| division_order(&look.shape));
    let count = u32::try_from(leaflets.len()).unwrap_or(u32::MAX);
    tally.count(if again > 0 { "pinnae" } else { "leaflets" }, id, 0, count);
    tally.count("division_order", id, 0, 1 + again);
    tally.kind(
        "division_pattern",
        if fan.is_some() { "palmate" } else { "pinnate" },
    );
    tally.kind("leaf_division", "compound");
    tally.kind("leaf_form", "blade");
}

/// The top of a frond's bare stipe where its rachis forks into two or
/// more arms that bear its leaflets (a pedate blade, as a maidenhair's
/// "2 equal branches surmounting the stipe"), if it does.
fn forked(structure: u32, bearing: &BTreeSet<u32>, drawn: &PlantGraph) -> Option<u32> {
    let mut arms: BTreeMap<u32, u32> = BTreeMap::new();
    for (index, segment) in (0_u32..).zip(&drawn.segments) {
        if segment.within == Some(structure)
            && bearing.contains(&index)
            && let Some(parent) = segment.parent
            && !bearing.contains(&parent)
            && drawn
                .segments
                .get(parent as usize)
                .is_some_and(|p| p.within == Some(structure))
        {
            *arms.entry(parent).or_default() += 1;
        }
    }
    arms.into_iter()
        .find(|&(_, n)| n >= 2)
        .map(|(fork, _)| fork)
}

/// A forked frond's blade, a fan of leaflets round the top of its stipe:
/// its length from there to its farthest leaflet tip (a flora's blade
/// from its foot to the tip of its longest pinna), its width across, and the shape words its outline reads as,
/// with cordate (and reniform, broader than long) where its leaflets
/// reach back past the stipe, as a reniform blade's lobes do.
fn fan_sizes(
    fork: u32,
    leaflets: &[&GraphOrgan],
    drawn: &PlantGraph,
    looks: &[Look],
) -> Option<(f64, f64, Option<String>)> {
    let top = drawn.segments.get(fork as usize)?.end;
    let tips: Vec<Vec3> = leaflets
        .iter()
        .filter_map(|organ| {
            leaf_look(organ, looks).map(|(_, size)| organ.position + organ.heading * size)
        })
        .collect();
    if tips.is_empty() {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let middle = tips.iter().fold(Vec3::ZERO, |sum, &tip| sum + (tip - top)) / tips.len() as f64;
    let forward = middle.normalize_or(Vec3::ZERO);
    // Across the fan: in its plane, square to its middle.
    let arm = drawn
        .segments
        .iter()
        .find(|segment| segment.parent == Some(fork))
        .map(|segment| segment.end - segment.start)?;
    let side = forward.cross(arm).cross(forward).normalize_or(Vec3::ZERO);
    if forward == Vec3::ZERO || side == Vec3::ZERO {
        return None;
    }
    let (mut length, mut low, mut high) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut back = false;
    for &tip in &tips {
        let (out, across) = ((tip - top).dot(forward), (tip - top).dot(side));
        length = length.max((tip - top).length());
        (low, high) = (low.min(across), high.max(across));
        back |= out < 0.0;
    }
    let width = high - low;
    if length <= 0.0 {
        return None;
    }
    let outline: Vec<(f64, f64)> = tips
        .iter()
        .map(|&tip| {
            (
                (tip - top).dot(forward).max(0.0) / length,
                (tip - top).dot(side).abs(),
            )
        })
        .collect();
    let words = outline_shape(&outline, width / length).map(|words| {
        let mut all: BTreeSet<&str> = words.split('|').collect();
        if back {
            all.insert("cordate");
            if width > length {
                all.insert("reniform");
            }
        }
        all.into_iter().collect::<Vec<_>>().join("|")
    });
    Some((length, width, words))
}

/// What a leaf drawn as `shape` is made as (`leaf_form`: a needle, a
/// scale or a flat blade) and whether its blade is one piece
/// (`leaf_division`): a divided leaf's leaflets and a frond's pinnae
/// stand apart, as separate leaflets; a dissected blade's segments run
/// together along its rachis (`confluent` half or more, 0.75 as the
/// angiosperms draw one), and lobes, a sprig's leaves, a grass blade,
/// needles and scales are each one piece.
fn leaf_kinds(shape: &Shape, tally: &mut Tally) {
    let (form, division) = match shape {
        Shape::Needles(_) => ("needle", "simple"),
        Shape::Scales(_) => ("scale", "simple"),
        Shape::Compound(compound) if compound.confluent.unwrap_or(0.0) < 0.5 => {
            ("blade", "compound")
        }
        Shape::Frond(_) => ("blade", "compound"),
        Shape::Compound(_)
        | Shape::Simple(_)
        | Shape::Lobed(_)
        | Shape::Palmate(_)
        | Shape::Sprig(_)
        | Shape::Blade(_) => ("blade", "simple"),
        _ => return,
    };
    tally.kind("leaf_form", form);
    tally.kind("leaf_division", division);
}

/// How many times a leaf drawn as `shape` divides (`division_order`): a
/// divided leaf once, twice where it bears pinnae, three times where
/// their pinnules bear segments; lobes once; a frond
/// card once, and again where its pinnae are cut at least halfway to
/// their midribs (its `divided` cuts them three quarters of the way at
/// 1); a leaf drawn whole 0.
fn division_order(shape: &Shape) -> u32 {
    match shape {
        Shape::Compound(compound) => {
            let form = crate::divided::Form::of(compound);
            1 + u32::from(form.pinnules > 0) + u32::from(form.segments > 0)
        }
        Shape::Frond(frond) => 1 + u32::from(frond.divided >= 0.5),
        Shape::Lobed(_) | Shape::Palmate(_) => 1,
        _ => 0,
    }
}

/// How far either side of the chart of plane shapes' borders a blade's
/// widest point reads as both sides' (a share of its length).
const SHAPE_BORDER: f64 = 0.03;

/// The shape a blade's outline reads as (`leaf_shape`), by the
/// Systematics Association's chart of plane shapes (1962), the chart the
/// kingdom's rule draws its outlines to: from its half-width at places
/// along it, `(u, w)` with `u` from its base (0) to its tip (1), and its
/// width over its length. A blade whose sides run within a tenth of its
/// widest for 0.6 of its length or more is linear (narrower than 1:6)
/// or oblong; any blade at least twelve times as long as it is wide is
/// linear too. Otherwise where it is widest (the middle of where it is
/// within 3% of it) names its kind: in its lowest fifth deltate, and
/// below its middle (0.45) lanceolate where narrower than 1:2 and ovate
/// where wider than 1:3; at its middle (0.45 to 0.55) elliptic; above
/// it oblanceolate and obovate by the same widths, and spatulate where
/// its lower third is under a third of its widest; and orbicular at 5:6
/// or wider. Read within [`SHAPE_BORDER`] of a border, it is both
/// sides'. The chart gives lanceolate 1:3 to 1:6 and ovate, elliptic and
/// obovate 2:3 to 1:2; between 1:2 and 1:3 a blade is named both ways.
fn outline_shape(outline: &[(f64, f64)], aspect: f64) -> Option<String> {
    let mut sorted = outline.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The outline's edge: the widest at each place along it.
    let mut edge: Vec<(f64, f64)> = Vec::with_capacity(sorted.len());
    for (u, w) in sorted {
        match edge.last_mut() {
            Some(last) if (last.0 - u).abs() < 1e-9 => last.1 = last.1.max(w),
            _ => edge.push((u, w)),
        }
    }
    let most = edge.iter().fold(0.0_f64, |most, p| most.max(p.1));
    if most <= 0.0 || aspect <= 0.0 || !aspect.is_finite() {
        return None;
    }
    let within = |share: f64| {
        edge.iter()
            .filter(move |p| p.1 >= share * most)
            .map(|p| p.0)
    };
    let (sum, count) = within(0.97).fold((0.0, 0.0), |(sum, count), u| (sum + u, count + 1.0));
    let widest = sum / count;
    let (low, high) = within(0.9).fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), u| {
        (low.min(u), high.max(u))
    });
    // The half-width at `u`, straight between the places read.
    let at = |u: f64| -> f64 {
        let after = edge.partition_point(|p| p.0 < u);
        match (
            after.checked_sub(1).and_then(|i| edge.get(i)),
            edge.get(after),
        ) {
            (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * (u - a.0) / (b.0 - a.0),
            (Some(a), _) => a.1,
            (None, Some(b)) => b.1,
            (None, None) => 0.0,
        }
    };
    let mut words = BTreeSet::new();
    if aspect <= 1.0 / 12.0 {
        words.insert("linear");
    }
    if high - low >= 0.6 {
        words.insert(if aspect < 1.0 / 6.0 {
            "linear"
        } else {
            "oblong"
        });
    } else {
        let (lance, ovate, round) = (aspect < 0.5, aspect > 1.0 / 3.0, aspect >= 5.0 / 6.0);
        if widest < 0.2 + SHAPE_BORDER {
            words.insert("deltate");
        }
        if widest < 0.45 + SHAPE_BORDER {
            words.extend(lance.then_some("lanceolate"));
            words.extend(ovate.then_some("ovate"));
        }
        if (0.45 - SHAPE_BORDER..=0.55 + SHAPE_BORDER).contains(&widest) {
            words.insert("elliptic");
        }
        if widest > 0.55 - SHAPE_BORDER {
            words.extend(lance.then_some("oblanceolate"));
            words.extend(ovate.then_some("obovate"));
            if at(0.3) < 0.3 * most {
                words.insert("spatulate");
            }
        }
        words.extend(round.then_some("orbicular"));
    }
    Some(words.into_iter().collect::<Vec<_>>().join("|"))
}

/// The shape words a leaf card's blade reads as: its outline's
/// ([`outline_shape`]), and where its basal lobes reach back past the
/// petiole (`Simple.sinus`), cordate, and reniform too when it is
/// broader than long (a cordate blade may be).
fn card_shape(shape: &Shape) -> Option<String> {
    let (outline, aspect) = card_outline(shape)?;
    let words = outline_shape(&outline, aspect)?;
    let Shape::Simple(simple) = shape else {
        return Some(words);
    };
    if simple.sinus <= 0.0 {
        return Some(words);
    }
    let mut all: BTreeSet<&str> = words.split('|').collect();
    all.insert("cordate");
    if aspect > 1.0 {
        all.insert("reniform");
    }
    Some(all.into_iter().collect::<Vec<_>>().join("|"))
}

/// Places a card's blade outline is read at, from its base to its tip.
const OUTLINE_PLACES: u32 = 100;

/// A card's half-width `profile` read at [`OUTLINE_PLACES`] along it.
fn sampled(profile: impl Fn(f64) -> f64) -> Vec<(f64, f64)> {
    (0..=OUTLINE_PLACES)
        .map(|i| {
            let u = f64::from(i) / f64::from(OUTLINE_PLACES);
            (u, profile(u))
        })
        .collect()
}

/// The outline a leaf card draws its blade with, for [`outline_shape`]:
/// a blade's, a lobed blade's over its lobes' tips, a sprig's leaves', a
/// grass blade card's, a frond card's over its pinnae's tips, and a
/// divided leaf's leaflets' (`leaf_shape` is a compound leaf's leaflets'),
/// but a dissected blade's whole, as its segments run together.
fn card_outline(shape: &Shape) -> Option<(Vec<(f64, f64)>, f64)> {
    use crate::templates::{blade_half_width, frond_pinna, lobed_envelope, outline_profile};
    Some(match shape {
        Shape::Simple(s) => (
            sampled(|u| outline_profile(s.outline.as_deref(), u, s.widest, s.base, s.tip)),
            s.width,
        ),
        Shape::Sprig(s) => (
            sampled(|u| outline_profile(s.outline.as_deref(), u, s.widest, s.base, s.tip)),
            s.width,
        ),
        Shape::Lobed(s) => (sampled(lobed_envelope), s.width),
        Shape::Compound(c) if c.confluent.unwrap_or(0.0) < 0.5 => {
            let form = crate::divided::Form::of(c);
            (
                sampled(|u| {
                    outline_profile(c.outline.as_deref(), u, form.widest, form.base, form.tip)
                }),
                form.width,
            )
        }
        Shape::Compound(c) => {
            // A dissected blade's edge runs over its segments' tips.
            let laid = crate::divided::layout(c);
            let tips: Vec<(f64, f64)> = laid
                .leaflets
                .iter()
                .map(|l| {
                    (
                        l.base.0 + l.length * l.direction.0,
                        l.base.1 + l.length * l.direction.1,
                    )
                })
                .collect();
            let foot = laid
                .leaflets
                .iter()
                .map(|l| l.base.1)
                .fold(f64::INFINITY, f64::min);
            let top = tips.iter().map(|t| t.1).fold(foot, f64::max);
            let length = top - foot;
            if !length.is_finite() || length <= 0.0 {
                return None;
            }
            let reach = tips.iter().fold(0.0_f64, |most, t| most.max(t.0.abs()));
            let points = tips
                .iter()
                .map(|&(x, y)| ((y - foot) / length, x.abs()))
                .collect();
            (points, 2.0 * reach / length)
        }
        Shape::Blade(b) => (sampled(|u| blade_half_width(b, u)), b.aspect),
        Shape::Frond(f) => (
            (0..f.pinnae).map(|k| frond_pinna(f, k, false)).collect(),
            f.width / (1.0 - f.stipe).max(1e-6),
        ),
        _ => return None,
    })
}

/// The look a leaf is drawn with in its light, and its drawn size.
fn leaf_look<'a>(organ: &GraphOrgan, looks: &'a [Look]) -> Option<(&'a Look, f64)> {
    let own = usize::from(organ.organ);
    let look = looks.get(own)?;
    let (index, share) = look.forms.of(own, organ);
    Some((looks.get(index).unwrap_or(look), organ.size * share))
}

/// A leaf card's blade, its width and its petiole, for a card `size` long,
/// how many times it divides, and what it is made as.
fn leaf_sizes(shape: &Shape, size: f64, id: u64, tally: &mut Tally) {
    leaf_kinds(shape, tally);
    if let Some(words) = card_shape(shape) {
        tally.kind("leaf_shape", &words);
    }
    let order = division_order(shape);
    if order > 0 {
        tally.count("division_order", id, 0, order);
    }
    let (blade, width, petiole) = match shape {
        Shape::Simple(simple) => {
            let junction = crate::templates::junction(simple);
            let blade = size * (1.0 - junction);
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
            for (part, pinnules) in (0_u32..).zip(&counts.pinnules) {
                tally.count("pinnules", id, part, *pinnules);
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
    let term = match grown.term(organ) {
        "cone" if grown.male(organ) => POLLEN_CONE,
        term => term,
    };
    if let Some(cluster) = &look.inflorescence {
        let layout = cluster.laid_on(organ);
        cluster_kinds(cluster, &layout, look, phase, tally);
        if in_stage(phase, Stage::Open) {
            // Along its axis: a nodding or hanging cluster held straight.
            let straight = cluster.laid_straight_on(organ);
            tally.add(
                "inflorescence_length_m",
                organ.id,
                0,
                organ.size * straight.length(cluster.unit),
            );
            tally.add(
                "peduncle_length_m",
                organ.id,
                0,
                organ.size * straight.peduncle(),
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
                    .map(|stage| (&stage.shape, stage.size)),
                None => in_stage(phase, Stage::Open).then_some((&look.shape, 1.0)),
            };
            if let Some((Shape::Flower(flower), size)) = shown {
                let width = organ.size * unit.length * size;
                flower_parts(flower, flower_cup(look), width, (organ.id, part), tally);
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
    // A flower drawn closed with no season is its bud (a herb's `bud`,
    // its flower closed): not yet the flower its characters describe.
    if phase.is_none() && matches!(&look.shape, Shape::Flower(flower) if flower.open <= 0.0) {
        return;
    }
    unit_sizes(&look.shape, organ.size, term, phase, (organ.id, 0), tally);
    if in_stage(phase, Stage::Open) && !grown.clustered(organ) {
        // A flower, head or spikelet borne alone.
        let unit = match &look.shape {
            Shape::Flower(flower) => {
                flower_parts(flower, flower_cup(look), organ.size, (organ.id, 0), tally);
                "flower"
            }
            Shape::Head(_) => "capitulum",
            Shape::Spikelet(_) => "spikelet",
            _ => return,
        };
        tally.kind("inflorescence_unit", unit);
    }
}

/// A cluster's kind, its units and how far its axis bends over, while
/// its units are flowers (heads, spikelets). The `inflorescence`
/// character groups a shoot's flowers: a cluster of fruit, drawn on the
/// days its units are fruit, is what the flowers became and may be drawn
/// otherwise. Its units in flower are the look's own while it is open,
/// or, where they open by the day, its flower look while any shows it (a
/// look whose own is its fruit, as a lupine's).
fn cluster_kinds(
    cluster: &crate::clusters::ClusterLook,
    layout: &crate::clusters::Layout,
    look: &Look,
    phase: Option<Phase>,
    tally: &mut Tally,
) {
    use crate::looks::ClusterKind;
    let opened = cluster
        .opening
        .as_ref()
        .filter(|opening| {
            layout
                .units
                .iter()
                .any(|unit| matches!(opening.shown(unit), Some((1, _))))
        })
        .and_then(|opening| opening.looks[1].as_ref())
        .map(|stage| &stage.shape);
    let flowers = if in_stage(phase, Stage::Open)
        && !matches!(cluster.unit, crate::clusters::Unit::Fruit { .. })
    {
        Some(&look.shape)
    } else {
        opened
    };
    let Some(flowers) = flowers else {
        return;
    };
    // A spike in a spathe is a spadix (PO:0030120). Units up an axis that
    // all stand at its one node (a raceme of a pair, as twinberry's) are
    // drawn as a few-flowered cyme's or an umbel's would be.
    let one_node = matches!(
        cluster.cluster.kind,
        ClusterKind::Raceme | ClusterKind::Spike | ClusterKind::Catkin | ClusterKind::Corymb
    ) && cluster.cluster.branches == 0
        && (2..=cluster.cluster.per_node.clamp(1, 8) as usize).contains(&layout.units.len());
    let kind = if cluster.cluster.kind == ClusterKind::Spike && !layout.sheets.is_empty() {
        "spadix".to_owned()
    } else if one_node {
        format!("{}|cyme|umbel", cluster.cluster.kind.name())
    } else {
        cluster.cluster.kind.name().to_owned()
    };
    tally.kind("inflorescence", &kind);
    tally.kind(
        "inflorescence_unit",
        match flowers {
            Shape::Head(_) => "capitulum",
            Shape::Spikelet(_) => "spikelet",
            _ => "flower",
        },
    );
    // A nodding one bends over a quarter turn by its tip, a hanging one
    // half a turn.
    let droop = cluster.cluster.droop;
    tally.kind(
        "inflorescence_posture",
        if droop < 45.0 {
            "erect"
        } else if droop < 135.0 {
            "nodding"
        } else {
            "pendent"
        },
    );
    // A corymb's units level is flat-topped; on a dome, rounded; on most
    // of a sphere, a ball.
    if cluster.cluster.kind == ClusterKind::Corymb {
        let dome = cluster.cluster.dome;
        tally.kind(
            "inflorescence_outline",
            if dome < 30.0 {
                "flat_topped"
            } else if dome < 135.0 {
                "rounded"
            } else {
                "globose"
            },
        );
    }
}

/// One flower, head, spikelet, fruit or cone drawn with `shape`, `length`
/// long (a flower's or head's width).
/// The term a male plant's cone is measured under: its pollen cone, which
/// floras give apart from the seed cone.
const POLLEN_CONE: &str = "pollen_cone";

fn unit_sizes(
    shape: &Shape,
    length: f64,
    term: &str,
    phase: Option<Phase>,
    (organ, part): (u64, u32),
    tally: &mut Tally,
) {
    match shape {
        Shape::Flower(flower) if in_stage(phase, Stage::Open) => {
            // Across its sepals where they outreach its petals.
            let reach = if flower.sepals > 0 {
                flower.sepal_length.max(1.0)
            } else {
                1.0
            };
            tally.add("flower_size_m", organ, part, length * reach);
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
                tally.kind("fruit_type", "cypsela");
            }
        }
        Shape::Spikelet(spikelet) if in_stage(phase, Stage::Open) => {
            tally.count("florets_per_spikelet", organ, part, spikelet.florets.max(1));
            tally.count("glumes", organ, part, spikelet.glumes.min(2));
            let awned = spikelet.awn > 0.0 && spikelet.awned > 0.0;
            tally.kind("awns", if awned { "true" } else { "false" });
            if awned {
                tally.kind(
                    "awn_insertion",
                    &nearest(&AWN_INSERTIONS, spikelet.awn_from),
                );
                tally.kind("awn_form", &nearest(&AWN_FORMS, spikelet.bent));
            }
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
        // A flower drawn as a fruit with no season is its bud (a herb's
        // `bud`), never a fruit.
        Shape::Fruit(_) if term == "flower" && phase.is_none() => {}
        // A bulbil drawn as a fruit is a small bulb in a leaf's axil, never
        // a fruit.
        Shape::Fruit(_) if term == "bulbil" => {}
        Shape::Fruit(fruit) if in_stage(phase, Stage::Ripe) => {
            if term == POLLEN_CONE {
                // A male plant's cone (a cycad's) is its pollen cone.
                tally.add("pollen_cone_length_m", organ, part, length);
                tally.add(
                    "pollen_cone_width_m",
                    organ,
                    part,
                    length * fruit.aspect.clamp(0.05, 2.0),
                );
            } else if term == "seed" {
                // A seed borne outside a fruit (a cycad's), its fleshy
                // coat and all.
                tally.add("seed_length_m", organ, part, length);
                tally.add(
                    "seed_width_m",
                    organ,
                    part,
                    length * fruit.aspect.clamp(0.05, 2.0),
                );
            } else if term == "cone" {
                tally.add("cone_length_m", organ, part, length);
                tally.add(
                    "cone_width_m",
                    organ,
                    part,
                    length * fruit.aspect.clamp(0.05, 2.0),
                );
            } else {
                tally.add("fruit_size_m", organ, part, length);
                // A fruit card drawn as a cone's scales (an alder's woody
                // catkin) shows no one fruit's kind.
                if fruit.cone < 0.5 {
                    tally.kind("fruit_type", fruit_types(fruit.kind));
                }
            }
        }
        _ => {}
    }
}

/// The fruit types a fruit drawn as `kind` may be (`fruit_type`), joined
/// by `|` where the drawing does not tell them apart: an ellipsoid is a
/// berry or a drupe, or a dry one-seeded fruit (an achene, a cypsela, a
/// grain or a nut); one crowned by its calyx a berry or a pome, as a
/// fleshy fruit from an inferior ovary is; drupelets an aggregate or a
/// multiple fruit; a rose hip the achenes in its cup, an aggregate; a
/// wing a samara, or the pair of them a maple's schizocarp splits into;
/// a nut in a cup or a husk a nut; a pod a legume or another pod; a
/// ribbed fruit a capsule, a follicle or a schizocarp of ribbed mericarps;
/// a cactus fruit with scales or areoles on its skin a berry.
fn fruit_types(kind: crate::looks::FruitKind) -> &'static str {
    use crate::looks::FruitKind;
    match kind {
        FruitKind::Berry => "berry|drupe|achene|cypsela|caryopsis|nut",
        FruitKind::Crowned => "berry|pome",
        FruitKind::Pome => "pome",
        FruitKind::Drupelets => "aggregate|multiple",
        FruitKind::Hip => "aggregate|achene",
        FruitKind::Samara => "samara|schizocarp",
        FruitKind::Acorn | FruitKind::Husked => "nut",
        FruitKind::Pod => "legume|pod",
        FruitKind::Capsule => "capsule|follicle|schizocarp",
        FruitKind::Scaly | FruitKind::Areolate | FruitKind::Spiny => "berry",
        FruitKind::Follicles | FruitKind::Samaras => "aggregate",
        FruitKind::Berries => "berry|aggregate",
    }
}

/// A flower `width` across, its parts as its layout draws them (spec
/// rework P5): the petals or tepals in a whorl (a corolla's one whorl of
/// petals, half of two whorls of tepals; a spiral has none), its
/// stamens, staminodes and carpels, a spiral's tepals, its spurs' length,
/// and the kind of its perianth, its ovary's place, how its petals
/// differ by place and so its symmetry, its corolla's shape, whether a
/// floral cup bears its whorls and whether it is turned half round.
fn flower_parts(
    flower: &crate::looks::Flower,
    cup: f64,
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
            tally.count(
                "flower_merosity",
                organ,
                part,
                flower.petals / flower.petal_whorls.max(1),
            );
            "sepals_and_petals"
        }
        // Tepals stated by count stand in whorls of the merosity; else
        // in two whorls.
        Perianth::Tepals if flower.whorl > 0 => {
            tally.count("flower_merosity", organ, part, flower.whorl);
            tally.count("perianth_parts", organ, part, flower.petals);
            "tepals"
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
    // A zygomorphic corolla's kind; a regular flower has none, so a genus
    // whose family states a kind its own flowers lack (mimosoid Vachellia
    // under the pea family's papilionaceous) is not judged by it.
    if flower.is_zygomorphic() {
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
    tally.kind(
        "flower_symmetry",
        if flower.is_zygomorphic() {
            "zygomorphic"
        } else {
            "actinomorphic"
        },
    );
    if matches!(flower.perianth, Perianth::Corolla | Perianth::Tepals) {
        tally.kind("corolla_shape", &corolla_shape(flower, cup));
    }
    tally.kind(
        "hypanthium",
        if flower.tube > 0.0 || flower.floral_tube > 0.0 {
            "true"
        } else {
            "false"
        },
    );
    // A resupinate flower is drawn turned half round.
    tally.kind(
        "resupinate",
        if libm::fabs(flower.turn) >= 90.0 {
            "true"
        } else {
            "false"
        },
    );
}

/// Each corolla shape as the angiosperms draw it (`_ranks/clade/
/// angiosperms.json`): the share of each petal joined in a tube, the
/// tube's mouth over a petal's length, and how much it swells.
const COROLLA_SHAPES: [(&str, [f64; 3]); 7] = [
    ("rotate", [0.0, 0.3, 0.0]),
    ("cupped", [0.0, 0.3, 0.0]),
    ("campanulate", [0.45, 0.45, 0.0]),
    ("funnelform", [0.6, 0.4, 0.0]),
    ("tubular", [0.75, 0.2, 0.0]),
    ("salverform", [0.8, 0.12, 0.0]),
    ("urceolate", [0.85, 0.1, 0.25]),
];

/// A receptacle's dome for each shape word, its height over the disc's
/// radius, as the Asteraceae's rule draws it (`asteraceae/family.json`).
const RECEPTACLES: [(&str, f64); 4] = [
    ("flat", 0.0),
    ("convex", 0.3),
    ("hemispheric", 1.0),
    ("conic", 2.0),
];

/// Where an awn leaves its lemma, a share of the lemma below its tip, and
/// how far it bends at its knee, in degrees, for each word, as the
/// Poales' rules draw them (`_ranks/order/poales.json`).
const AWN_INSERTIONS: [(&str, f64); 4] = [
    ("apical", 0.0),
    ("subapical", 0.15),
    ("dorsal", 0.5),
    ("basal", 0.8),
];
const AWN_FORMS: [(&str, f64); 5] = [
    ("straight", 0.0),
    ("flexuous", 15.0),
    ("contorted", 40.0),
    ("geniculate", 60.0),
    ("hooked", 110.0),
];

/// The word of `table` whose value a drawn `value` is nearest, or each of
/// the nearest where it lies halfway between two (as a rule draws two
/// words that tie).
fn nearest(table: &[(&str, f64)], value: f64) -> String {
    let least = table
        .iter()
        .map(|(_, at)| (at - value).abs())
        .fold(f64::INFINITY, f64::min);
    table
        .iter()
        .filter(|(_, at)| (at - value).abs() <= least + 1e-9)
        .map(|(word, _)| *word)
        .collect::<Vec<_>>()
        .join("|")
}

/// How steeply free tepals rise for each shape they make
/// without a tube, in degrees from the flower's plane, as the Liliaceae's
/// rule draws them (`liliaceae/family.json`): a bell, a funnel, and a
/// tube or urn of tepals held close.
const FREE_CUPS: [(&str, f64); 4] = [
    ("campanulate", 60.0),
    ("funnelform", 65.0),
    ("tubular", 75.0),
    ("urceolate", 75.0),
];

/// How steeply the look's flower form raises its petals, in degrees; 0
/// where it draws no flower form.
fn flower_cup(look: &Look) -> f64 {
    match &look.form {
        crate::blooms::Form::Flower(form) => form.cup,
        _ => 0.0,
    }
}

/// The corolla shape a flower draws: the nearest of [`COROLLA_SHAPES`] to
/// its tube, or each of the nearest where they are drawn alike (a flat
/// and a cupped corolla, both free petals). Free tepals raised steeply (a
/// lily's) draw a bell or a funnel with no tube: the nearest of
/// [`FREE_CUPS`] to their rise. A corolla's free petals are judged by
/// their tube alone, as before: a columbine's raised blades over its
/// spurs are not its corolla's shape.
fn corolla_shape(flower: &crate::looks::Flower, cup: f64) -> String {
    if flower.fused <= 0.0
        && flower.perianth == crate::looks::Perianth::Tepals
        && cup >= crate::flowers::BELL
    {
        let off = |rise: f64| libm::fabs(rise - cup);
        let nearest = FREE_CUPS
            .iter()
            .map(|(_, rise)| off(*rise))
            .fold(f64::INFINITY, f64::min);
        return FREE_CUPS
            .iter()
            .filter(|(_, rise)| off(*rise) <= nearest + 1e-12)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join("|");
    }
    let drawn = [flower.fused, flower.throat, flower.belly];
    let distance = |shape: &[f64; 3]| -> f64 {
        shape
            .iter()
            .zip(&drawn)
            .map(|(a, b)| (a - b) * (a - b))
            .sum()
    };
    let nearest = COROLLA_SHAPES
        .iter()
        .map(|(_, shape)| distance(shape))
        .fold(f64::INFINITY, f64::min);
    COROLLA_SHAPES
        .iter()
        .filter(|(_, shape)| distance(shape) <= nearest + 1e-12)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join("|")
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
    tally.kind("receptacle_shape", &nearest(&RECEPTACLES, head.receptacle));
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
    // The plant is also kept LIFE_YEARS before the end, or halfway on a
    // shorter life, to see how long its leaves and cones live.
    let before = libm::round((years - LIFE_YEARS).max(years / 2.0));
    let keyframes = if before > 0.0 && before < years {
        vec![before, years]
    } else {
        vec![years]
    };
    let growth = crate::drawing::grow_variant_watched(
        spec,
        library,
        None,
        environment,
        seed,
        keyframes,
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
        before: (growth.keyframes.len() > 1).then(|| &growth.keyframes[0]),
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
    pub timings: Vec<Timing>,
    /// Size characters the species states that nothing here measures yet.
    pub unmeasured: Vec<String>,
    /// Flowers and fruit on juvenile shoots, over all the plants: each an
    /// error (spec rework P6).
    pub juvenile_flowers: usize,
}

/// A kind character judged against what the plants draw: a pass when
/// every state drawn is one the species states (plainly, or qualified,
/// however rarely), an error otherwise. A state drawn that could be any
/// of several, as an ellipsoid fruit could be a berry or a drupe, is
/// their names joined by `|`, and passes when the species states one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreement {
    pub character: &'static str,
    pub stated: Vec<String>,
    pub drawn: BTreeSet<String>,
    pub verdict: Verdict,
}

/// A months character judged against the months the plants draw it in
/// ([`MONTHS`]): a pass when they are the months stated, a warning when
/// they differ only by a month at either end (each month drawn within a
/// month of one stated, and each stated within a month of one drawn), an
/// error otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    pub character: &'static str,
    pub stated: BTreeSet<u8>,
    pub drawn: BTreeSet<u8>,
    pub verdict: Verdict,
}

impl Timing {
    /// `drawn` judged against `stated`.
    #[must_use]
    pub fn judge(character: &'static str, stated: BTreeSet<u8>, drawn: BTreeSet<u8>) -> Self {
        let verdict = if drawn.is_empty() {
            Verdict::NotGrown
        } else if drawn == stated {
            Verdict::Pass
        } else if drawn.iter().all(|&month| near(month, &stated))
            && stated.iter().all(|&month| near(month, &drawn))
        {
            Verdict::Warning
        } else {
            Verdict::Error
        };
        Self {
            character,
            stated,
            drawn,
            verdict,
        }
    }

    /// Whether it draws a month more than a month from every month stated
    /// (else an error is for stated months it leaves undrawn).
    #[must_use]
    pub fn strays(&self) -> bool {
        self.drawn.iter().any(|&month| !near(month, &self.stated))
    }
}

/// Whether `month` is within a month of one of `months`, round the year.
fn near(month: u8, months: &BTreeSet<u8>) -> bool {
    months.iter().any(|&other| {
        let apart = month.abs_diff(other);
        apart.min(12 - apart) <= 1
    })
}

/// The months of a stated months character, 1 to 12.
#[must_use]
pub fn months_of(value: &Value) -> BTreeSet<u8> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .filter_map(|month| u8::try_from(month).ok())
        .filter(|month| (1..=12).contains(month))
        .collect()
}

/// Months for a report, each run of them from its first to its last
/// (a run may go on past December): "Apr–Jun", "Nov–Feb", "Apr, Aug–Sep",
/// "all year".
#[must_use]
pub fn months_text(months: &BTreeSet<u8>) -> String {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if months.len() >= 12 {
        return "all year".into();
    }
    let has = |month: u8| months.contains(&((month + 11) % 12 + 1));
    // Runs start at a month whose month before is missing.
    let mut runs = Vec::new();
    for start in 1..=12_u8 {
        if !has(start) || has(start + 11) {
            continue;
        }
        let mut end = start;
        while has(end + 1) {
            end += 1;
        }
        let name = |month: u8| NAMES[usize::from((month - 1) % 12)];
        runs.push(if end == start {
            name(start).to_string()
        } else {
            format!("{}–{}", name(start), name(end))
        });
    }
    runs.join(", ")
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
            .chain(self.timings.iter().map(|timing| timing.verdict))
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
/// [`COUNTS`], [`KINDS`] or [`MONTHS`] checks, or is held to one where it
/// states none ([`stated`]).
#[must_use]
pub fn states_checked(spec: &PlantSpec, traits: &Map<String, Value>) -> bool {
    SIZES
        .iter()
        .map(|(key, _)| *key)
        .chain(COUNTS)
        .chain(KINDS)
        .chain(MONTHS)
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
    let vocabulary = crate::traits::Vocabulary::builtin();
    let kinds = KINDS
        .iter()
        // A state inherited for flowers of another kind, such as a
        // family's two-lipped corolla on a species with regular flowers,
        // does not apply to the species.
        .filter(|&&character| vocabulary.applies(character, traits) != crate::traits::Applies::No)
        .filter_map(|&character| {
            let stated = states_of(traits.get(character)?);
            let drawn: BTreeSet<String> = plants
                .iter()
                .filter_map(|plant| plant.kinds.get(character))
                .flatten()
                .cloned()
                .collect();
            // A drawn state may be any of several (`berry|drupe`), where
            // the drawing does not tell them apart.
            let verdict = if drawn.is_empty() {
                Verdict::NotGrown
            } else if drawn
                .iter()
                .all(|state| state.split('|').any(|one| stated.iter().any(|s| s == one)))
            {
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
    let timings = MONTHS
        .iter()
        .filter_map(|&character| {
            let stated = months_of(traits.get(character)?);
            let drawn = plants
                .iter()
                .filter_map(|plant| plant.kinds.get(character))
                .flatten()
                .filter_map(|month| month.parse().ok())
                .collect();
            Some(Timing::judge(character, stated, drawn))
        })
        .collect();
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
        timings,
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
        let unit = if (value - 1.0).abs() < 1e-9 {
            "year"
        } else {
            "years"
        };
        format!("{} {unit}", value_text(years, value))
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
        let unit = if stated.single() && (stated.min - 1.0).abs() < 1e-9 {
            "year"
        } else {
            "years"
        };
        return format!("{} {unit}", range_text(bare, stated));
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
        flower_parts(&cactus, 0.0, 0.1, (1, 0), &mut tally);
        let pea = Flower {
            corolla: Corolla::Papilionaceous,
            stamens: 10,
            carpels: 1,
            ..Flower::default()
        };
        flower_parts(&pea, 0.0, 0.02, (2, 0), &mut tally);
        let (measures, kinds) = tally.measures();
        assert_eq!(measures["stamens"], [200.0, 10.0]);
        assert_eq!(measures["carpels"], [12.0, 1.0]);
        assert_eq!(measures["perianth_parts"], [24.0]);
        let states = |key: &str| kinds[key].iter().cloned().collect::<Vec<_>>();
        assert_eq!(states("perianth"), ["sepals_and_petals", "spiral"]);
        assert_eq!(states("ovary_position"), ["inferior", "superior"]);
        // The cactus's regular flower has no zygomorphy to draw.
        assert_eq!(states("zygomorphy"), ["papilionaceous"]);
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
        flower_parts(&lily, 0.0, 0.1, (1, 0), &mut tally);
        // A columbine spurs every petal; a pea's keel draws no spur.
        let columbine = Flower {
            spur: 1.5,
            ..Flower::default()
        };
        flower_parts(&columbine, 0.0, 0.04, (2, 0), &mut tally);
        let pea = Flower {
            corolla: Corolla::Papilionaceous,
            spur: 1.0,
            ..Flower::default()
        };
        flower_parts(&pea, 0.0, 0.02, (3, 0), &mut tally);
        let water_lily = Flower {
            perianth: Perianth::Spiral,
            petals: 24,
            ..Flower::default()
        };
        flower_parts(&water_lily, 0.0, 0.1, (4, 0), &mut tally);
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
    fn a_flower_draws_its_symmetry_corolla_cup_and_turn() {
        use crate::looks::{Corolla, Flower, Perianth};
        let raised = |flower: &Flower, cup: f64| {
            let mut tally = Tally::default();
            flower_parts(flower, cup, 0.02, (1, 0), &mut tally);
            let (_, kinds) = tally.measures();
            [
                "flower_symmetry",
                "corolla_shape",
                "hypanthium",
                "resupinate",
            ]
            .map(|character| {
                kinds
                    .get(character)
                    .map(|states| states.iter().cloned().collect::<Vec<_>>().join(","))
                    .unwrap_or_default()
            })
        };
        let kinds = |flower: &Flower| raised(flower, 0.0);
        assert_eq!(
            kinds(&Flower::default()),
            ["actinomorphic", "rotate|cupped", "false", "false"]
        );
        let rose = Flower {
            tube: 0.4,
            ..Flower::default()
        };
        assert_eq!(kinds(&rose)[2], "true");
        let huckleberry = Flower {
            fused: 0.85,
            throat: 0.1,
            belly: 0.25,
            ..Flower::default()
        };
        assert_eq!(kinds(&huckleberry)[1], "urceolate");
        // Usually a bell, sometimes a funnel: drawn between them, nearer
        // the bell.
        let harebell = Flower {
            fused: 0.5,
            throat: 0.43,
            ..Flower::default()
        };
        assert_eq!(kinds(&harebell)[1], "campanulate");
        // Free tepals raised steeply: a lily's bell and funnel, no tube.
        let lily = Flower {
            perianth: Perianth::Tepals,
            petals: 6,
            ..Flower::default()
        };
        assert_eq!(raised(&lily, 40.0)[1], "rotate|cupped");
        assert_eq!(raised(&lily, 60.0)[1], "campanulate");
        assert_eq!(raised(&lily, 65.0)[1], "funnelform");
        let orchid = Flower {
            perianth: Perianth::Tepals,
            petals: 6,
            corolla: Corolla::Labellate,
            turn: 180.0,
            ..Flower::default()
        };
        assert_eq!(
            kinds(&orchid),
            ["zygomorphic", "rotate|cupped", "false", "true"]
        );
        let water_lily = Flower {
            perianth: Perianth::Spiral,
            petals: 24,
            ..Flower::default()
        };
        assert_eq!(kinds(&water_lily)[1], "");
    }

    #[test]
    fn a_leaf_is_a_blade_needle_or_scale_in_one_piece_or_leaflets() {
        use crate::looks::{Compound, Frond, Needles, Scales, Simple};
        let yarrow = Compound {
            confluent: Some(0.75),
            ..Compound::default()
        };
        let kinds = [
            Shape::Simple(Simple::default()),
            Shape::Compound(Compound::default()),
            Shape::Compound(yarrow),
            Shape::Frond(Frond::default()),
            Shape::Needles(Needles::default()),
            Shape::Scales(Scales::default()),
        ]
        .map(|shape| {
            let mut tally = Tally::default();
            leaf_kinds(&shape, &mut tally);
            let (_, kinds) = tally.measures();
            let one = |character: &str| kinds[character].iter().next().cloned().unwrap();
            format!("{} {}", one("leaf_form"), one("leaf_division"))
        });
        assert_eq!(
            kinds,
            [
                "blade simple",
                "blade compound",
                "blade simple",
                "blade compound",
                "needle simple",
                "scale simple",
            ]
        );
    }

    #[test]
    fn the_broadest_blade_a_width_rule_sets_is_one_its_card_draws() {
        use crate::looks::{Compound, Simple, Sprig};
        let kingdom: Value =
            serde_json::from_str(include_str!("../library/_ranks/kingdom/plantae.json")).unwrap();
        // A formula's bounds: `clamp(ratio, low, high)`.
        let highest = |rule: &Value| -> Vec<f64> {
            let text = rule.to_string();
            text.split("clamp(")
                .skip(1)
                .map(|clamp| {
                    let inner = &clamp[..clamp.find(')').unwrap()];
                    inner.rsplit(',').next().unwrap().trim().parse().unwrap()
                })
                .collect()
        };
        for organ in ["leaf", "basal"] {
            let rule = &kingdom["rules"][format!("appearance.organs.{organ}.shape.width")];
            let highs = highest(rule);
            assert!(!highs.is_empty(), "{organ}");
            for width in highs {
                // A wild ginger's kidney-shaped blade, 1.83 times as broad
                // as long, draws as broad as the card allows.
                assert!(width >= 1.5, "{organ} {width}");
                Shape::Simple(Simple {
                    width,
                    ..Simple::default()
                })
                .validate()
                .unwrap();
                Shape::Sprig(Sprig {
                    width,
                    ..Sprig::default()
                })
                .validate()
                .unwrap();
            }
        }
        for organ in ["leaf", "basal"] {
            let rule = &kingdom["rules"][format!("appearance.organs.{organ}.shape.leaflet_width")];
            for leaflet_width in highest(rule) {
                Shape::Compound(Compound {
                    leaflet_width,
                    ..Compound::default()
                })
                .validate()
                .unwrap();
            }
        }
    }

    #[test]
    fn each_outline_the_kingdom_draws_reads_as_its_shape_word() {
        use crate::looks::Simple;
        let kingdom: Value =
            serde_json::from_str(include_str!("../library/_ranks/kingdom/plantae.json")).unwrap();
        let rules = &kingdom["rules"];
        let widths = &rules["appearance.organs.leaf.shape.width"]["map"]["leaf_shape"];
        let outlines = &rules["appearance.organs.leaf.shape.outline"]["map"]["leaf_shape"];
        let phylum: Value =
            serde_json::from_str(include_str!("../library/_ranks/phylum/tracheophyta.json"))
                .unwrap();
        let sinuses = &phylum["rules"]["appearance.organs.leaf.shape.sinus"]["map"]["leaf_shape"];
        // A word's width, or the width its formula falls back to where a
        // flora measures no blade: `first(measured, word's)`.
        let width = |value: &Value| {
            value.as_f64().unwrap_or_else(|| {
                let rule = value["rule"].as_str().unwrap();
                rule.trim_end_matches(')')
                    .rsplit(", ")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
        };
        let read: Vec<(String, String)> = outlines
            .as_object()
            .unwrap()
            .iter()
            .map(|(word, points)| {
                let simple = Simple {
                    width: width(&widths[word]),
                    outline: Some(serde_json::from_value(points.clone()).unwrap()),
                    sinus: sinuses[word].as_f64().unwrap_or(0.0),
                    ..Simple::default()
                };
                (word.clone(), card_shape(&Shape::Simple(simple)).unwrap())
            })
            .collect();
        for (word, words) in &read {
            assert!(
                words.split('|').any(|one| one == word),
                "{word} reads {words}"
            );
        }
        let plain: Vec<&str> = read
            .iter()
            .filter(|(word, words)| word == words)
            .map(|(word, _)| word.as_str())
            .collect();
        assert_eq!(
            plain,
            [
                "elliptic",
                "lanceolate",
                "linear",
                "oblanceolate",
                "oblong",
                "obovate",
                "ovate"
            ]
        );
    }

    #[test]
    fn a_blade_stated_toothed_draws_teeth() {
        let kingdom: Value =
            serde_json::from_str(include_str!("../library/_ranks/kingdom/plantae.json")).unwrap();
        let rules = &kingdom["rules"];
        let margin = |organ: &str, field: &str| {
            rules[format!("appearance.organs.{organ}.shape.{field}")]["map"]["leaf_margin"].clone()
        };
        // A rosette's leaves follow the species' margin as its leaves do.
        for field in ["teeth", "tooth_depth", "round"] {
            assert_eq!(margin("leaf", field), margin("basal", field), "{field}");
        }
        let (teeth, depth, round) = (
            margin("leaf", "teeth"),
            margin("leaf", "tooth_depth"),
            margin("leaf", "round"),
        );
        assert_eq!(teeth["entire"], 0);
        for word in ["serrate", "dentate", "crenate", "spiny"] {
            assert!(teeth[word].as_u64().unwrap() > 0, "{word}");
            assert!(depth[word].as_f64().unwrap() > 0.0, "{word}");
        }
        // Serrate saw teeth, dentate blunter, crenate scallops.
        let round = |word: &str| round[word].as_f64().unwrap();
        assert!(round("serrate") < round("dentate") && round("dentate") < round("crenate"));
        // Chloranthaceae state their leaves serrate and draw no teeth of
        // their own: the rule draws them.
        let spec: Value =
            serde_json::from_str(crate::library::species("sarcandra-glabra").unwrap().source)
                .unwrap();
        let shape = &spec["appearance"]["organs"]["leaf"]["shape"];
        assert_eq!(shape["teeth"], teeth["serrate"], "{shape}");
        assert_eq!(shape["round"], 0.0, "{shape}");
    }

    #[test]
    fn heads_and_awns_read_the_words_their_rules_draw() {
        let map = |file: &str, path: &str, word: &str| -> Vec<(String, f64)> {
            let rank: Value = serde_json::from_str(file).unwrap();
            rank["rules"][path]["map"][word]
                .as_object()
                .unwrap()
                .iter()
                .map(|(word, value)| (word.clone(), value.as_f64().unwrap()))
                .collect()
        };
        let table = |pairs: &[(&str, f64)]| -> Vec<(String, f64)> {
            let mut pairs: Vec<(String, f64)> = pairs
                .iter()
                .map(|(word, at)| ((*word).to_owned(), *at))
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            pairs
        };
        let asteraceae = include_str!("../library/asteraceae/family.json");
        let poales = include_str!("../library/_ranks/order/poales.json");
        assert_eq!(
            map(
                asteraceae,
                "appearance.organs.*.shape.receptacle",
                "receptacle_shape"
            ),
            table(&RECEPTACLES)
        );
        assert_eq!(
            map(
                poales,
                "appearance.organs.*.shape.awn_from",
                "awn_insertion"
            ),
            table(&AWN_INSERTIONS)
        );
        assert_eq!(
            map(poales, "appearance.organs.*.shape.bent", "awn_form"),
            table(&AWN_FORMS)
        );
        // Halfway between two words, as a rule draws a tie, reads as both.
        assert_eq!(nearest(&RECEPTACLES, 0.15), "flat|convex");
        assert_eq!(nearest(&AWN_FORMS, 60.0), "geniculate");
    }

    #[test]
    fn a_cactus_draws_its_stem_tubercles_and_main_spine() {
        use crate::body::{BodyLook, SpineRole, SpineSet, Tubercles};
        let read = |look: &BodyLook, jointed: bool| {
            let mut tally = Tally::default();
            cactus_kinds(look, jointed, &mut tally);
            let (_, kinds) = tally.measures();
            kinds
                .iter()
                .map(|(character, states)| {
                    let states: Vec<&str> = states.iter().map(String::as_str).collect();
                    format!("{character} {}", states.join(","))
                })
                .collect::<Vec<_>>()
        };
        let fishhook = BodyLook {
            tubercles: Some(Tubercles::default()),
            spines: vec![
                SpineSet {
                    role: Some(SpineRole::Central),
                    ..SpineSet::default()
                },
                SpineSet {
                    role: Some(SpineRole::Main),
                    hook: 150.0,
                    flat: 0.35,
                    ..SpineSet::default()
                },
            ],
            ..BodyLook::default()
        };
        assert_eq!(
            read(&fishhook, false),
            ["central_spine_form flattened|hooked", "tubercles true"]
        );
        let pad = BodyLook {
            flatten: 0.25,
            ..BodyLook::default()
        };
        assert_eq!(
            read(&pad, true),
            ["stem_form flat_joints", "tubercles false"]
        );
    }

    #[test]
    fn a_blade_widest_at_its_base_is_deltate_and_a_long_strip_linear() {
        use crate::looks::{Blade, Frond};
        // A frond whose lowest pinnae reach out farthest, as broad as long.
        let triangle: Vec<(f64, f64)> = (0..10)
            .map(|k| {
                let u = f64::from(k) / 10.0;
                (u, 0.5 * (1.0 - u))
            })
            .collect();
        assert_eq!(
            outline_shape(&triangle, 1.0).unwrap(),
            "deltate|orbicular|ovate"
        );
        let read = |shape: Shape| {
            let (outline, aspect) = card_outline(&shape).unwrap();
            outline_shape(&outline, aspect).unwrap()
        };
        let strip = Blade {
            taper: 0.0,
            aspect: 0.1,
            foot: 0.0,
        };
        assert_eq!(read(Shape::Blade(strip)), "linear");
        let tip = Blade {
            taper: 1.0,
            aspect: 0.05,
            foot: 0.0,
        };
        assert_eq!(read(Shape::Blade(tip)), "deltate|lanceolate|linear");
        // A frond card's blade is widest below its middle, a third as
        // broad as it is long.
        assert_eq!(read(Shape::Frond(Frond::default())), "lanceolate|ovate");
    }

    #[test]
    fn a_cone_is_pitched_from_its_shoot_toward_the_ground() {
        use crate::graph::GraphSegment;
        let shoot = GraphSegment {
            id: 1,
            parent: None,
            lateral: true,
            order: 1,
            start: Vec3::ZERO,
            end: Vec3::X,
            radius: 0.01,
            born: 1.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
            young: 0.0,
        };
        let cone = |heading: Vec3| GraphOrgan {
            id: 2,
            organ: 0,
            segment: Some(0),
            position: Vec3::X,
            heading,
            left: Vec3::Z,
            size: 0.1,
            born: 1.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        };
        let mut graph = PlantGraph {
            age: 1.0,
            height: 1.0,
            segments: vec![shoot],
            organs: Vec::new(),
            structures: Vec::new(),
        };
        let pitch = |graph: &PlantGraph, heading: Vec3| {
            pitch_from_shoot(&cone(heading.normalize_or(Vec3::Y)), graph).unwrap()
        };
        // On a level shoot: a fir's cone stands up, a spruce's hangs.
        assert!((pitch(&graph, Vec3::Y) + 90.0).abs() < 1e-9);
        assert!((pitch(&graph, Vec3::new(1.0, -1.0, 0.0)) - 45.0).abs() < 1e-9);
        assert!((pitch(&graph, Vec3::new(-0.5, -1.0, 0.0)) - 116.565).abs() < 1e-3);
        // Rolled off the vertical plane, it is pitched by its share in it.
        assert!((pitch(&graph, Vec3::new(0.0, -1.0, 1.0)) - 90.0).abs() < 1e-9);
        // A vertical shoot has no side down.
        graph.segments[0].end = Vec3::Y;
        assert!(pitch_from_shoot(&cone(Vec3::X), &graph).is_none());
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
    fn leaves_and_cones_live_from_their_birth_to_their_fall() {
        let library = Library::builtin();
        let grow = |id: &str| {
            let spec = library.spec(id).unwrap();
            let environment = crate::drawing::typical_environment(&spec, library);
            grow_and_measure(&spec, library, environment, spec.variants.seeds[0], None).unwrap()
        };
        // A redcedar's sprays live in whole years, longer low in the crown;
        // its cones are drawn ripe in their first season.
        let cedar = grow("thuja-plicata");
        let lives = &cedar.measures["leaf_life_years"];
        assert!(lives.iter().all(|&life| life >= 1.0 && life.fract() == 0.0));
        let (least, most) = lives.iter().fold((f64::MAX, 0.0_f64), |(a, b), &life| {
            (a.min(life), b.max(life))
        });
        assert!(most > least, "{least} to {most}");
        assert_eq!(cedar.measures["cone_maturation_years"], [1.0]);
        assert!(cedar.measures.contains_key("cone_retention_years"));
        let habit: Vec<&str> = cedar.kinds["leaf_habit"]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(habit, ["evergreen"]);
        // A fern's fronds, drawn in pieces, each live as a whole.
        let fern = grow("blechnum-spicant");
        let fronds = &fern.measures["leaf_life_years"];
        assert!(!fronds.is_empty() && fronds.iter().all(|&life| (life - fronds[0]).abs() < 1e-9));
    }

    #[test]
    fn a_plants_year_gives_the_months_it_flowers_and_fruits_in() {
        let library = Library::builtin();
        let grow = |id: &str| {
            let spec = library.spec(id).unwrap();
            let environment = crate::drawing::typical_environment(&spec, library);
            grow_and_measure(&spec, library, environment, spec.variants.seeds[0], None).unwrap()
        };
        let months = |plant: &Plant, character: &str| -> Vec<u8> {
            plant
                .kinds
                .get(character)
                .into_iter()
                .flatten()
                .map(|month| month.parse().unwrap())
                .collect::<BTreeSet<u8>>()
                .into_iter()
                .collect()
        };
        // A flowering currant, stated to flower in March and April and be
        // ripe from July to September: its racemes open their first flower
        // on day 60 and their last 21 days later, the last going to fruit
        // on day 121; its berries are half ripe from day 182 until they
        // fall on day 274.
        let currant = grow("ribes-sanguineum");
        assert_eq!(months(&currant, "flowering_months"), [3, 4]);
        assert_eq!(months(&currant, "fruiting_months"), [7, 8, 9]);
        // A plant whose looks have no season draws no year.
        let yarrow = grow("achillea-millefolium");
        assert!(months(&yarrow, "flowering_months").is_empty());
    }

    #[test]
    fn months_are_judged_to_within_a_month_at_either_end() {
        let set = |months: &[u8]| months.iter().copied().collect::<BTreeSet<u8>>();
        let judge = |stated: &[u8], drawn: &[u8]| {
            Timing::judge("flowering_months", set(stated), set(drawn)).verdict
        };
        assert_eq!(judge(&[4, 5, 6], &[4, 5, 6]), Verdict::Pass);
        assert_eq!(judge(&[4, 5, 6], &[3, 4, 5, 6, 7]), Verdict::Warning);
        assert_eq!(judge(&[4, 5, 6], &[5]), Verdict::Warning);
        assert_eq!(judge(&[4, 5, 6], &[4, 5, 6, 7, 8]), Verdict::Error);
        assert_eq!(
            judge(&[4, 5, 6], &(1..=12).collect::<Vec<_>>()),
            Verdict::Error
        );
        assert_eq!(judge(&[12, 1], &[11, 12, 1, 2]), Verdict::Warning);
        assert_eq!(judge(&[4, 5], &[]), Verdict::NotGrown);
        assert_eq!(month_of(1), 1);
        assert_eq!(month_of(31), 1);
        assert_eq!(month_of(32), 2);
        assert_eq!(month_of(335), 12);
        assert_eq!(month_of(365), 12);
        assert_eq!(months_text(&set(&[4, 5, 6])), "Apr–Jun");
        assert_eq!(months_text(&set(&[11, 12, 1, 2])), "Nov–Feb");
        assert_eq!(months_text(&set(&[4, 8, 9])), "Apr, Aug–Sep");
        assert_eq!(months_text(&set(&[4])), "Apr");
        assert_eq!(months_text(&(1..=12).collect()), "all year");
        assert_eq!(months_of(&json!([4, 5, 13, "x"])), set(&[4, 5]));
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
