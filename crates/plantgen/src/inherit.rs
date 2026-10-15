//! Specs inherited down the tree of life (growth plan G2).
//!
//! Beside its species, the library may hold a spec for any taxon above
//! them, a *rank file*:
//!
//! - `<family>/family.json` in a family's folder and
//!   `<family>/<genus>/genus.json` in a genus's;
//! - `_ranks/<rank>/<name>.json` for the ranks above families (orders,
//!   classes, phyla and unranked clades) and between a family and its
//!   genera (subfamilies, tribes), kept flat beside the families, each
//!   naming its parent, so a change in the classification is a one-field
//!   edit.
//!
//! A rank file is a spec's JSON with every key optional, plus `schema`,
//! `rank`, `name` (the taxon's; its file or folder is that name in lower
//! case, spaces as hyphens), `parent` and `forms`. `parent` names the rank
//! file it stands on as `<rank>/<name>`, such as `order/sapindales` or
//! `family/poaceae`; a genus stands on its family's file unless it names a
//! subfamily or tribe of that family. `forms` holds values for one growth
//! form only, by the form's name, laid over the file's other values. A
//! rank file never holds `id` or `taxon`: only a species' own `spec.json`
//! names a species.
//!
//! A species stands on a chain of rank files: its genus's, its family's,
//! and those they stand on, up to the top. Its effective spec is that
//! chain and its own `spec.json` merged from the top down by JSON Merge
//! Patch (RFC 7396), so the nearer file wins: objects merge key by key,
//! arrays and other values replace, and `null` deletes what the files
//! above set. Evidence travels with the values: each note of the
//! effective spec comes from one file of the chain, and a note is dropped
//! where a nearer file sets, replaces or deletes anything it describes. A
//! species with no rank file above it is its own `spec.json`, byte for
//! byte.
//!
//! A rank file or a species' spec may also state `traits`, facts from the
//! vocabulary in `traits.json` ([`crate::traits`]), and `rules` that turn
//! traits into spec values: a spec path with a formula ([`crate::formula`])
//! or a map from an enum trait's values. A formula reads traits and the
//! species' program parameters by name, a parameter as the files set it
//! (or a rule before it), else as the program's default; so a rule can
//! say what a leaf card of the species' own size means for its shoots.
//! A rule that sets a day of a look's season (`bud`, `flower`, `fruit`,
//! `ripe` or `fall`) applies only to looks with a season, and reads the
//! season's own `opening` and `ripening` by those names.
//! A trait stated as a range reads as its `typical` value, else the middle
//! of its `min` and `max`, and as `min(name)` and `max(name)` its bounds
//! (a number stated alone is both; months, the first and the last of the
//! run they make round the year), so a rule whose `rule` is a list of
//! formulas, one for each item, can set a range a look varies within:
//! ribs stated as 12 to 24 set a body's `[12, 24]`. A rule that reads one
//! range (or one parameter a rule before it spread), and nothing else
//! that varies, is also worked out at five points of it ([`SPREAD_AT`],
//! [`range_at`]): its lowest, its `min`, its `typical`, its `max` and its
//! highest (spec rework VARY); one reading two keeps its one value, as no
//! flora says how two ranges vary together, and a bound is one number.
//! Where its numbers differ, the effective spec records them beside the
//! value as its spread ([`spread_path`], [`Spread`]), from which each
//! organ drawn solid draws its look (`crate::looks::variety`), and each
//! plant a parameter that is a character of the whole plant in its own
//! units (`crate::spec::PlantSpec::seeded_params`); a parameter a formula
//! works out from a range, or one sized by a part's range, keeps its one
//! value, and a list a rule sets, and a body's look, vary within their own
//! ranges and record none. A trait
//! stated as qualified states reads as its most
//! usual state ([`FREQUENCIES`]), and a rule reading states that tie sets
//! nothing, unless it is a map that draws `between` them: a flora's "ovate
//! to lanceolate" means the forms between, so the map gives the mean of
//! its numbers for the states, and of lists as long as each other item by
//! item, or their one value where they agree, words too (a head disciform
//! or discoid is drawn discoid either way). A formula's whole number is
//! written as one, so a rule can set a count (`round(ray_florets)`).
//! Every rule lives in the taxon it holds for, its home (growth plan G2,
//! Joshi's rule that rules belong to taxa). Traits and rules merge down
//! the chain like values; then each rule sets its path, after the rules
//! that set the parameters it reads, unless a file at least as near as
//! the rule and what it reads set the path by hand, the species' program
//! has no such parameter, or the path is in the look of an organ
//! (`appearance.organs.<organ>`) the species does not draw. A rule inside
//! a look's `shape` names the card `template` it is for, or a list of
//! them, one of whose fields it sets ([`TEMPLATES`]; a count takes only a
//! whole number), and sets it only where the species draws the organ with
//! such a card: a family's rule for a head card's rays sets them where a
//! species draws its heads with one, and nothing where it draws them
//! another way. A rule inside a look's `inflorescence` sets one of its
//! keys ([`INFLORESCENCE`]) only where the species draws the organ as a
//! cluster, its look holding the key: a taxon says which organ is a
//! cluster with `"inflorescence": {}`, and the rules fill it in. A rule's
//! path may name `*` for the organ, as in
//! `appearance.organs.*.inflorescence.kind`, and then sets its path in
//! every look it may, whatever the species' program calls the organ. A
//! field that replaces others ([`REPLACES`]: a leaf's
//! outline replaces where it is widest, its tip and its base) sets nothing
//! where those are set at least as near, and else removes them. The
//! effective spec carries the values, not the traits or rules.
//!
//! Every file notes what it sets (per-value evidence): a note stays in the
//! effective spec until a nearer file replaces or deletes what it
//! describes, and a note on a subtree stays beside the nearer files' notes
//! on values inside it. A value a rule sets carries the rule's note: its
//! file's note on `rules.<path>`, else on all of its `rules`.
//!
//! The module reads nothing but JSON, so the build script compiles it too
//! and merges every built-in species' chain while compiling.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use serde_json::{Map, Value};

use crate::formula::Formula;

/// The schema number of a rank file.
pub const RANK_SCHEMA: u64 = 1;

/// The folder beside the families that holds the ranks kept flat:
/// `_ranks/<rank>/<name>.json`.
pub const RANKS: &str = "_ranks";

/// An unranked group, such as the eudicots: it may stand anywhere above
/// families.
pub const CLADE: &str = "clade";

/// The ranks a rank file stands at, highest first; and [`CLADE`].
pub const LEVELS: [&str; 13] = [
    "kingdom",
    "phylum",
    "subphylum",
    "class",
    "subclass",
    "superorder",
    "order",
    "suborder",
    "family",
    "subfamily",
    "tribe",
    "subtribe",
    "genus",
];

const FAMILY: usize = 8;
const GENUS: usize = 12;

/// How often a state of a trait stated as qualified states occurs, from
/// the most often: `{"opposite": "usually", "alternate": "rarely"}`.
pub const FREQUENCIES: [&str; 4] = ["usually", "often", "sometimes", "rarely"];

/// The bounds of a trait stated as a range: `min` and `max`, and where
/// known the rare extremes, `low` and `high`, and the usual value,
/// `typical`. A flora's "(3-)5-8" is `{"min": 5, "max": 8, "low": 3}`.
pub const RANGE: [&str; 5] = ["min", "max", "low", "high", "typical"];

/// The share of a range drawn below its `min` where it states a `low`,
/// and above its `max` where it states a `high`: its rare extremes.
pub const TAIL: f64 = 0.05;

/// The points a rule reading a range is worked out at, as shares of the
/// way through the range ([`range_at`]): its lowest, its `min`, its
/// `typical`, its `max` and its highest.
pub const SPREAD_AT: [f64; 5] = [0.0, TAIL, 0.5, 1.0 - TAIL, 1.0];

/// The value a range stated as `stated` takes `share` of the way through
/// it: half its values lie below its `typical` (else the middle of its
/// `min` and `max`) and half above, evenly within each half, and
/// [`TAIL`] of them below `min` down to `low` and above `max` up to
/// `high` where it states them. Nothing for a range without a `min` and a
/// `max`.
#[must_use]
pub fn range_at(stated: &Map<String, Value>, share: f64) -> Option<f64> {
    let bound = |name: &str| stated.get(name).and_then(Value::as_f64);
    let (min, max) = (bound("min")?, bound("max")?);
    let mut points = vec![(0.0, bound("low").unwrap_or(min))];
    if bound("low").is_some() {
        points.push((TAIL, min));
    }
    points.push((
        0.5,
        bound("typical").unwrap_or_else(|| f64::midpoint(min, max)),
    ));
    if bound("high").is_some() {
        points.push((1.0 - TAIL, max));
    }
    points.push((1.0, bound("high").unwrap_or(max)));
    Some(through(&points, share))
}

/// The value `share` of the way along `points`, each a share and its
/// value, straight between them.
fn through(points: &[(f64, f64)], share: f64) -> f64 {
    let share = share.clamp(0.0, 1.0);
    for pair in points.windows(2) {
        let ((from, low), (to, high)) = (pair[0], pair[1]);
        if share <= to {
            return if to > from {
                low + (high - low) * (share - from) / (to - from)
            } else {
                high
            };
        }
    }
    points.last().map_or(0.0, |point| point.1)
}

/// Where the effective spec records the spread of the value at `path`:
/// `appearance.spreads.<rest>` for a value in `appearance` (a look's
/// `appearance.organs.head.shape.rays` at
/// `appearance.spreads.organs.head.shape.rays`), `generator.spreads.<rest>`
/// for one in `generator`; nothing elsewhere.
#[must_use]
pub fn spread_path(path: &str) -> Option<String> {
    let (section, rest) = path.split_once('.')?;
    ["appearance", "generator"]
        .contains(&section)
        .then(|| format!("{section}.spreads.{rest}"))
}

/// The spread of a value a rule sets from a range: its values at the
/// points of [`SPREAD_AT`], whole numbers where the rule sets a count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub values: [f64; 5],
    pub whole: bool,
}

impl Spread {
    /// The spread the effective spec records as `value`: five numbers.
    #[must_use]
    pub fn of(value: &Value) -> Option<Self> {
        let items = value
            .as_array()
            .filter(|items| items.len() == SPREAD_AT.len())?;
        let mut values = [0.0; 5];
        for (slot, item) in values.iter_mut().zip(items) {
            *slot = item.as_f64().filter(|number| number.is_finite())?;
        }
        Some(Self {
            values,
            whole: items.iter().all(|item| item.is_i64() || item.is_u64()),
        })
    }

    /// Its value `share` of the way through it, straight between its
    /// points; the nearest whole number for a count.
    #[must_use]
    pub fn at(&self, share: f64) -> f64 {
        let points: Vec<(f64, f64)> = SPREAD_AT.into_iter().zip(self.values).collect();
        let value = through(&points, share);
        if self.whole { value.round() } else { value }
    }

    /// Every spread recorded within `spreads` (a section's `spreads`),
    /// each with its path there.
    #[must_use]
    pub fn all(spreads: &Map<String, Value>) -> Vec<(String, Self)> {
        fn walk(map: &Map<String, Value>, at: &str, found: &mut Vec<(String, Spread)>) {
            for (key, value) in map {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match value {
                    Value::Object(inside) => walk(inside, &path, found),
                    other => {
                        if let Some(spread) = Spread::of(other) {
                            found.push((path, spread));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(spreads, "", &mut found);
        found
    }
}

fn level(rank: &str) -> Option<usize> {
    LEVELS.iter().position(|level| *level == rank)
}

/// Values, traits and rules, and the evidence notes on them, by path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Part {
    /// A partial spec, without `evidence`, `traits` or `rules`.
    pub values: Map<String, Value>,
    /// Its `evidence`: each note's path and the note. A note on a trait
    /// sits at `traits.<key>`, one on a rule at `rules.<path>`.
    pub notes: Map<String, Value>,
    /// Its `traits` (growth plan G2.2): facts by key, from `traits.json`'s
    /// vocabulary; `null` deletes an inherited one.
    pub traits: Map<String, Value>,
    /// Its `rules`: each spec path a rule sets from traits, and the rule
    /// (`{"rule": formula}` or `{"map": {trait: {value: spec value}}}`, a
    /// map's value a spec value or a formula `{"rule": formula}`, with a
    /// `why`, and inside a look's `shape` the card `template` it is for);
    /// `null` deletes an inherited one.
    pub rules: Map<String, Value>,
}

/// The spec of a taxon above species.
#[derive(Debug, Clone, PartialEq)]
pub struct RankFile {
    /// Its path in the library tree, which names it:
    /// `_ranks/order/sapindales.json`, `sapindaceae/family.json` or
    /// `sapindaceae/acer/genus.json`.
    pub file: String,
    /// One of [`LEVELS`], or [`CLADE`].
    pub rank: String,
    /// The taxon's name as written: `Sapindaceae`, `core eudicots`.
    pub name: String,
    /// For a family's or a genus's file, the family's folder.
    pub family: Option<String>,
    /// The rank file it stands on, by path: the one its `parent` names,
    /// or for a genus that names none, its family's `family.json`, which
    /// may be missing.
    pub parent: Option<String>,
    /// Whether it named its parent.
    pub named_parent: bool,
    /// Its values for every growth form.
    pub shared: Part,
    /// Its values for one growth form, by the form's name, laid over
    /// `shared`.
    pub forms: BTreeMap<String, Part>,
}

impl RankFile {
    /// The rank file at `file`, its path in the library tree, from its
    /// text.
    ///
    /// # Errors
    ///
    /// Fails on a file out of place, invalid JSON, a `schema`, `rank` or
    /// `name` that does not fit its place, a `parent` that is not a rank
    /// file's, a key only a species sets, a rule that is not one, or an
    /// evidence note on a value, trait or rule the file does not set.
    pub fn parse(file: &str, text: &str) -> Result<Self, String> {
        let place = Place::of(file)?;
        let mut map = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err("a rank file is a JSON object".into()),
            Err(error) => return Err(format!("invalid rank file: {error}")),
        };
        if map.remove("schema").and_then(|schema| schema.as_u64()) != Some(RANK_SCHEMA) {
            return Err(format!("`schema` must be {RANK_SCHEMA}"));
        }
        let rank = take_string(&mut map, "rank")?;
        if rank != place.rank {
            return Err(format!(
                "its place makes it a {} file; it says `rank` {rank}",
                place.rank
            ));
        }
        let name = take_string(&mut map, "name")?;
        if file_name(&name) != place.name {
            return Err(format!(
                "its `name` {name} would be named `{}`, not `{}`",
                file_name(&name),
                place.name
            ));
        }
        let named = match map.remove("parent") {
            None => None,
            Some(Value::String(parent)) => Some(parent_file(&parent)?),
            Some(_) => return Err("`parent` is a string, `<rank>/<name>`".into()),
        };
        let below_family = level(&rank).is_some_and(|level| level > FAMILY);
        if below_family && rank != "genus" && named.is_none() {
            return Err(format!(
                "a {rank} names its `parent`: its family, or the rank of that family it sits in"
            ));
        }
        let named_parent = named.is_some();
        let parent = named.or_else(|| {
            place
                .family
                .as_ref()
                .filter(|_| rank == "genus")
                .map(|family| format!("{family}/family.json"))
        });
        let forms = match map.remove("forms") {
            None => BTreeMap::new(),
            Some(Value::Object(forms)) => forms
                .into_iter()
                .map(|(form, part)| match part {
                    Value::Object(part) => {
                        Part::read(part, &format!("forms.{form}")).map(|part| (form, part))
                    }
                    _ => Err(format!("`forms.{form}` is an object, a partial spec")),
                })
                .collect::<Result<_, String>>()?,
            Some(_) => return Err("`forms` is an object: growth form → partial spec".into()),
        };
        let shared = Part::read(map, "")?;
        Ok(Self {
            file: file.to_string(),
            rank,
            name,
            family: place.family,
            parent,
            named_parent,
            shared,
            forms,
        })
    }

    /// Its parts: its shared values, then each form's, with where each
    /// sits in the file (`""` or `forms.<growth form>`).
    pub fn parts(&self) -> impl Iterator<Item = (String, &Part)> {
        std::iter::once((String::new(), &self.shared)).chain(
            self.forms
                .iter()
                .map(|(form, part)| (format!("forms.{form}"), part)),
        )
    }
}

impl Part {
    /// The part of a rank file at `at` (`""` for its shared values,
    /// `forms.<growth form>` for a form's) from its keys.
    fn read(values: Map<String, Value>, at: &str) -> Result<Self, String> {
        let within = |key: &str| {
            if at.is_empty() {
                format!("`{key}`")
            } else {
                format!("`{at}.{key}`")
            }
        };
        for key in ["id", "taxon"] {
            if values.contains_key(key) {
                return Err(format!(
                    "a rank file holds no {}: only a species' own spec.json sets it",
                    within(key)
                ));
            }
        }
        if !at.is_empty() {
            for key in ["schema", "rank", "name", "parent", "forms", "growth_form"] {
                if values.contains_key(key) {
                    return Err(format!(
                        "{} belongs to the whole file, not to one form",
                        within(key)
                    ));
                }
            }
        }
        let part = Self::take(values).map_err(|error| {
            if at.is_empty() {
                error
            } else {
                format!("in `{at}`: {error}")
            }
        })?;
        if let Some(path) = part.notes.keys().find(|path| !part.sets(path)) {
            return Err(format!(
                "the evidence note on `{path}` in {} is on a value it does not set",
                if at.is_empty() { "the file" } else { at }
            ));
        }
        Ok(part)
    }

    /// A species' own spec as a part: its `evidence`, `traits` and
    /// `rules` taken out of its values, each rule's shape checked.
    ///
    /// # Errors
    ///
    /// Fails if the spec is not an object, or those keys are not well
    /// formed.
    pub fn of_spec(spec: &Value) -> Result<Self, String> {
        match spec {
            Value::Object(values) => Self::take(values.clone()),
            _ => Err("a spec is a JSON object".into()),
        }
    }

    /// A part from a spec's keys: its `evidence`, `traits` and `rules`
    /// taken out of its values, each rule checked.
    fn take(mut values: Map<String, Value>) -> Result<Self, String> {
        let mut object = |key: &str| match values.remove(key) {
            None => Ok(Map::new()),
            Some(Value::Object(map)) => Ok(map),
            Some(_) => Err(format!("`{key}` is an object")),
        };
        let notes = object("evidence")?;
        let traits = object("traits")?;
        let rules = object("rules")?;
        for (path, rule) in &rules {
            check_rule(path, rule).map_err(|error| format!("the rule for `{path}`: {error}"))?;
        }
        Ok(Self {
            values,
            notes,
            traits,
            rules,
        })
    }

    /// Whether the part sets the value, trait (`traits.<key>`) or rule
    /// (`rules.<path>`) at `path`, or any trait (`traits`) or rule
    /// (`rules`).
    fn sets(&self, path: &str) -> bool {
        if path == "traits" {
            !self.traits.is_empty()
        } else if path == "rules" {
            !self.rules.is_empty()
        } else if let Some(key) = path.strip_prefix("traits.") {
            self.traits.contains_key(key)
        } else if let Some(rule) = path.strip_prefix("rules.") {
            self.rules.contains_key(rule)
        } else {
            find(&self.values, path).is_some()
        }
    }
}

/// The spec paths no rule may set: what names the species, its growth
/// form (which picks the parts that apply) and its program (whose
/// parameters the rules are checked against), and the notes.
const UNRULED: [&str; 6] = [
    "id",
    "taxon",
    "schema",
    "growth_form",
    "generator.program",
    "evidence",
];

/// Checks one rule's shape: `null` (deleting an inherited rule), or an
/// object with a `rule` (a formula over traits) or a `map` (one trait's
/// values to spec values), and a `why`.
fn check_rule(path: &str, rule: &Value) -> Result<(), String> {
    if path.split('.').any(str::is_empty) {
        return Err("its path is keys joined by dots".into());
    }
    if path.contains('*') && (!path.starts_with(EVERY) || path[EVERY.len()..].contains('*')) {
        return Err(format!(
            "`*` stands only for the organ, as in `{EVERY}inflorescence.kind`"
        ));
    }
    if let Some(field) = inflorescence_field(path) {
        inflorescence_takes(field)?;
    }
    if UNRULED
        .iter()
        .any(|unruled| path == *unruled || path.starts_with(&format!("{unruled}.")))
    {
        return Err("no rule may set it".into());
    }
    let rule = match rule {
        Value::Null => return Ok(()),
        Value::Object(rule) => rule,
        _ => return Err("a rule is an object".into()),
    };
    if let Some(key) = rule.keys().find(|key| {
        ![
            "rule", "map", "colour", "scale", "between", "why", "template", "else",
        ]
        .contains(&key.as_str())
    }) {
        return Err(format!(
            "`{key}` is not a rule's: `rule`, `map` (and `between`, `else`) or `colour`, `why`, and in a look's `shape` its `template`"
        ));
    }
    if !rule.get("why").is_none_or(Value::is_string) {
        return Err("`why` is a line of text".into());
    }
    match rule.get("between") {
        None => {}
        Some(Value::Bool(true)) if rule.contains_key("map") => {}
        Some(Value::Bool(true)) => {
            return Err("only a `map` draws `between` the values of states that tie".into());
        }
        Some(_) => return Err("`between` is `true`, or left out".into()),
    }
    if rule.contains_key("else") && !rule.contains_key("map") {
        return Err("only a `map` has an `else`, its value where its trait is not stated or names no one most usual state".into());
    }
    if shape_field(path) == Some("template") {
        if path.starts_with(EVERY) {
            return Err("a rule choosing a card names its organ, not `*`".into());
        }
        if rule.contains_key("template") {
            return Err("a rule choosing a card's `template` names none for itself".into());
        }
        if !rule.contains_key("map") {
            return Err("a rule choosing a card's `template` is a `map` to template names".into());
        }
        return check_map(rule, &|value| match value.as_str() {
            Some(name) if TEMPLATES.iter().any(|(template, _)| *template == name) => Ok(()),
            _ => Err(format!("{value} is no card template")),
        });
    }
    match (shape_field(path), rule.get("template")) {
        (Some(field), Some(template)) => {
            for template in templates(template)? {
                template_field(template, field)?;
            }
        }
        (Some(_), None) => {
            return Err(
                "a rule inside a look's `shape` names the card `template` it is for".into(),
            );
        }
        (None, Some(_)) => {
            return Err("only a rule inside a look's `shape` names a card `template`".into());
        }
        (None, None) => {}
    }
    if rule.contains_key("scale") && !rule.contains_key("colour") {
        return Err("only a `colour` rule has a `scale`".into());
    }
    match (rule.get("rule"), rule.get("map"), rule.get("colour")) {
        (Some(Value::String(formula)), None, None) => Formula::parse(formula).map(|_| ()),
        (Some(Value::Array(formulas)), None, None) if !formulas.is_empty() => {
            formulas.iter().try_for_each(|formula| match formula {
                Value::String(formula) => Formula::parse(formula).map(|_| ()),
                _ => Err("a list `rule` is formulas, one for each item".into()),
            })
        }
        (None, Some(Value::Object(_)), None) => check_map(rule, &|_| Ok(())),
        (None, None, Some(_)) => check_colour(rule),
        _ => Err(
            "a rule has a `rule` (a formula, or a list of them), a `map` or a `colour`, one of them"
                .into(),
        ),
    }
}

/// Checks a `map` rule's shape: one trait and its table, whose values are
/// formulas (`{"rule": formula}`), maps on another trait (`{"map": ...}`,
/// with their own `else`) or plain values, as its `else` is, each plain
/// value passing `plain`.
fn check_map(
    rule: &Map<String, Value>,
    plain: &dyn Fn(&Value) -> Result<(), String>,
) -> Result<(), String> {
    let Some(Value::Object(map)) = rule.get("map") else {
        return Err("a `map` names one trait, then its values' spec values".into());
    };
    let table = match map.iter().next() {
        Some((_, Value::Object(table))) if map.len() == 1 => table,
        _ => return Err("a `map` names one trait, then its values' spec values".into()),
    };
    for value in table.values().chain(rule.get("else")) {
        if let Some(formula) = map_formula(value) {
            Formula::parse(formula)?;
        } else if let Some(colour) = map_colour(value) {
            check_colour(colour)?;
        } else if let Some(inner) = map_rule(value) {
            if let Some(key) = inner
                .keys()
                .find(|key| !["map", "else"].contains(&key.as_str()))
            {
                return Err(format!("`{key}` is not a map's: `map` and `else`"));
            }
            check_map(inner, plain)?;
        } else {
            plain(value)?;
        }
    }
    Ok(())
}

/// What a field of a card template takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// A number.
    Number,
    /// A whole number, 0 or more: rays, leaflets, clusters.
    Count,
    /// One of the template's words: a fruit's kind.
    Word,
    /// Points along a blade, `[u, w]` pairs: a leaf's outline.
    Outline,
}

/// The fields of a card that replace others where set, and those they
/// replace: an outline draws the blade that `widest`, `tip` and `base`
/// otherwise do (`crate::looks::Simple`).
pub const REPLACES: [(&str, &[&str]); 1] = [("outline", &["widest", "tip", "base"])];

/// The fields of a card that cannot stand beside `field`: those it
/// replaces, or the one that replaces it ([`REPLACES`]).
fn rivals(field: &str) -> Vec<&'static str> {
    REPLACES
        .iter()
        .flat_map(|(replacing, replaced)| {
            if *replacing == field {
                replaced.to_vec()
            } else if replaced.contains(&field) {
                vec![*replacing]
            } else {
                Vec::new()
            }
        })
        .collect()
}

/// What a rule may set inside a look's `inflorescence`
/// (`crate::looks::Inflorescence`): every key but its `branch` and its
/// `stalk` and `spathe_colour` colours. The looks module's tests hold this
/// to the key.
pub const INFLORESCENCE: [(&str, Field); 15] = {
    use Field::{Count, Number, Word};
    [
        ("kind", Word),
        ("flowers", Count),
        ("branches", Count),
        ("per_node", Count),
        ("divergence", Number),
        ("spread", Number),
        ("pedicel", Number),
        ("peduncle", Number),
        ("size", Number),
        ("taper", Number),
        ("droop", Number),
        ("spathe", Number),
        ("spathe_width", Number),
        ("spathe_hood", Number),
        ("spathe_cup", Number),
    ]
};

/// The start of a rule's path that sets something in every look it may:
/// `*` stands for the organ.
const EVERY: &str = "appearance.organs.*.";

/// The start of a rule's path inside a body's look (spec rework E6c).
const BODIES: &str = "appearance.bodies.";

/// Whether a rule at `at` may set it in `spec`: in a body's look, a rule
/// fills in what the look has, so the body, its ribs or a spine set of
/// the role it names must be there.
fn in_body(spec: &Map<String, Value>, at: &str) -> Result<(), String> {
    if let Some(inside) = at.strip_prefix(BODIES)
        && let Some((within, _)) = at.rsplit_once('.')
        && find(spec, within).is_none()
    {
        let body = inside.split('.').next().unwrap_or_default();
        return Err(format!(
            "its `{body}` body has no `{}`",
            &within[BODIES.len() + body.len() + 1..]
        ));
    }
    Ok(())
}

/// The organ `traits.json` says the trait `name` describes (`plant`,
/// `leaf`, `stem`, ...), if it is one. The build script reads this module
/// too, so it reads the vocabulary itself.
fn trait_organ(name: &str) -> Option<&'static str> {
    static ORGANS: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    ORGANS
        .get_or_init(|| {
            let vocabulary: Value =
                serde_json::from_str(include_str!("../traits.json")).unwrap_or_default();
            vocabulary
                .get("traits")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(key, about)| {
                    Some((key.clone(), about.get("organ")?.as_str()?.to_string()))
                })
                .collect()
        })
        .get(name)
        .map(String::as_str)
}

/// A bound of the trait `name` as stated (`value`), read as `min(name)`
/// or `max(name)`: of a range, its `min` or `max`; of a number, the
/// number; of months, the first and the last of the run they make round
/// the year ([`months_run`]).
fn bound_of(name: &str, bound: &str, value: &Value) -> Result<Value, String> {
    match value {
        Value::Object(range) => range
            .get(bound)
            .filter(|bound| bound.is_number())
            .cloned()
            .ok_or_else(|| format!("`{name}` states no `{bound}`")),
        Value::Number(_) => Ok(value.clone()),
        Value::Array(months) => {
            let (first, last) =
                months_run(months).ok_or_else(|| format!("`{name}` names no months"))?;
            Ok(Value::from(if bound == "min" { first } else { last }))
        }
        _ => Err(format!("`{name}` is no number to read the {bound} of")),
    }
}

/// The first and the last of `months` (each 1 to 12) as one run round
/// the year: from the month after the longest gap between them to the
/// month before it, a month past 12 being one of the next year. November
/// to February is 11 and 14; April, May and September, 4 and 9 (the run
/// takes in its shorter gaps); every month, 1 and 12.
fn months_run(months: &[Value]) -> Option<(u64, u64)> {
    let months: Vec<u64> = months
        .iter()
        .filter_map(Value::as_u64)
        .filter(|month| (1..=12).contains(month))
        .collect::<BTreeSet<u64>>()
        .into_iter()
        .collect();
    let count = months.len();
    if count == 0 {
        return None;
    }
    // The month after the longest gap, the first such round the year.
    let mut after = 0;
    let mut longest = 0;
    for (index, &month) in months.iter().enumerate() {
        let next = months[(index + 1) % count];
        let gap = (next + 11 - month) % 12;
        if gap > longest {
            longest = gap;
            after = (index + 1) % count;
        }
    }
    let first = months[after];
    let last = months[(after + count - 1) % count];
    Some((first, if last < first { last + 12 } else { last }))
}

/// The stage looks of a look's season that a rule in its `shape` also
/// sets where they draw its template, unless they draw the whole cluster.
const STAGE_LOOKS: [&str; 2] = ["flower_look", "bud_look"];

/// The organ of a path inside `appearance.organs`.
fn organ_of(path: &str) -> &str {
    path.strip_prefix("appearance.organs.")
        .and_then(|look| look.split_once('.'))
        .map_or("", |(organ, _)| organ)
}

/// The key a rule's `path` sets inside a look's `inflorescence`
/// (`appearance.organs.<organ>.inflorescence.<key>`), if it sets one.
fn inflorescence_field(path: &str) -> Option<&str> {
    let (_, look) = path.strip_prefix("appearance.organs.")?.split_once('.')?;
    look.strip_prefix("inflorescence.")
}

/// The days of a look's season a rule may set (`crate::looks::Season`),
/// each a day of the year.
const SEASON_DAYS: [&str; 5] = ["bud", "flower", "fruit", "ripe", "fall"];

/// The day a rule's `path` sets in a look's season
/// (`appearance.organs.<organ>.season.<day>`), if it sets one.
#[must_use]
pub fn season_day(path: &str) -> Option<&str> {
    let (_, look) = path.strip_prefix("appearance.organs.")?.split_once('.')?;
    look.strip_prefix("season.")
        .filter(|day| SEASON_DAYS.contains(day))
}

/// The fields of a look's season a rule setting one of its days reads by
/// name, and what each is where the season does not set it: the days its
/// units take to open, and its fruit to ripen.
pub const SEASON_READS: [(&str, f64); 2] = [("opening", 0.0), ("ripening", 20.0)];

/// The value of `key`, one of [`SEASON_READS`], in the season whose day
/// `at` sets, as `spec` has it.
fn season_read(spec: &Map<String, Value>, at: &str, key: &str) -> Option<Value> {
    season_day(at)?;
    let (_, default) = SEASON_READS.iter().find(|(name, _)| *name == key)?;
    let season = format!("appearance.organs.{}.season.{key}", organ_of(at));
    Some(
        find(spec, &season)
            .cloned()
            .unwrap_or_else(|| Value::from(*default)),
    )
}

/// What the inflorescence's `key` takes.
fn inflorescence_takes(key: &str) -> Result<Field, String> {
    INFLORESCENCE
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, takes)| *takes)
        .ok_or_else(|| {
            format!(
                "a rule sets no `{key}` in an inflorescence: it sets {}",
                INFLORESCENCE
                    .iter()
                    .map(|(name, _)| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// The paths a rule at `path` sets in `spec`: its own, or where it names
/// every look (`*`), each look's that it may set, in the organs' order;
/// or why there are none.
fn every_look(
    spec: &Map<String, Value>,
    path: &str,
    rule: &Map<String, Value>,
) -> Result<Vec<String>, String> {
    let Some(rest) = path.strip_prefix(EVERY) else {
        return Ok(vec![path.to_string()]);
    };
    let looks = find(spec, "appearance.organs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut paths = Vec::new();
    for (organ, look) in &looks {
        let at = format!("appearance.organs.{organ}.{rest}");
        if inflorescence_field(&at).is_some()
            && !look.get("inflorescence").is_some_and(Value::is_object)
        {
            continue;
        }
        if season_day(&at).is_some() && !look.get("season").is_some_and(Value::is_object) {
            continue;
        }
        if let Some(field) = shape_field(&at).map(str::to_string) {
            let names = rule
                .get("template")
                .map(templates)
                .transpose()?
                .unwrap_or_default();
            let draws = |look: &Value| {
                look.get("shape")
                    .and_then(|shape| shape.get("template"))
                    .and_then(Value::as_str)
                    .is_some_and(|drawn| names.contains(&drawn))
            };
            // A rule on the fruit card is about fruit (a fruit's kind
            // from its type): a look with a season shows its fruit in its
            // own shape, and one with none is a fruit unless it is the
            // plant's buds. A season's bud and flower looks are no fruit.
            if names == ["fruit"] {
                if draws(look) && (look.get("season").is_some() || organ != "bud") {
                    paths.push(at);
                }
                continue;
            }
            if draws(look) {
                paths.push(at);
            }
            // A season's stage looks that draw the template take the rule
            // too (spec rework P5): broom's flowers are only its flower
            // look's. A look that draws the whole cluster (a dogwood's head
            // in its bracts) is not one flower, so it takes none.
            for stage in STAGE_LOOKS {
                if look
                    .get("season")
                    .and_then(|season| season.get(stage))
                    .is_some_and(|stage| {
                        draws(stage) && !stage.get("whole").is_some_and(|w| w == true)
                    })
                {
                    paths.push(format!(
                        "appearance.organs.{organ}.season.{stage}.shape.{field}"
                    ));
                }
            }
            continue;
        }
        paths.push(at);
    }
    if paths.is_empty() {
        return Err(if inflorescence_field(path).is_some() {
            "it draws no look as a cluster (with an `inflorescence`)".to_string()
        } else if season_day(path).is_some() {
            "it draws no look with a `season`".to_string()
        } else if shape_field(path).is_some() {
            let names = rule
                .get("template")
                .map(templates)
                .transpose()?
                .unwrap_or_default();
            format!("it draws no look with {} cards", cards(&names))
        } else {
            "it draws no looks".to_string()
        });
    }
    Ok(paths)
}

/// The fields of each card template a look's `shape` may name
/// (`crate::looks::Shape`), by the template's name: what a rule that names
/// the template may set inside a look's `shape`. The looks module's tests
/// hold this to the templates.
pub const TEMPLATES: [(&str, &[(&str, Field)]); 16] = {
    use Field::{Count, Number, Outline, Word};
    [
        (
            "needles",
            &[
                ("twigs", Count),
                ("needle", Number),
                ("twig_angle", Number),
                ("ranks", Count),
                ("aspect", Number),
                ("width", Number),
                ("bundle", Count),
            ],
        ),
        (
            "scales",
            &[
                ("branchlets", Count),
                ("angle", Number),
                ("cord", Number),
                ("aspect", Number),
            ],
        ),
        (
            "palmate",
            &[
                ("lobes", Count),
                ("depth", Number),
                ("lobe_width", Number),
                ("teeth", Count),
                ("spread", Number),
                ("petiole", Number),
            ],
        ),
        (
            "simple",
            &[
                ("width", Number),
                ("outline", Outline),
                ("widest", Number),
                ("tip", Number),
                ("base", Number),
                ("teeth", Count),
                ("tooth_depth", Number),
                ("petiole", Number),
            ],
        ),
        (
            "lobed",
            &[
                ("width", Number),
                ("lobes", Count),
                ("depth", Number),
                ("round", Number),
                ("petiole", Number),
            ],
        ),
        (
            "compound",
            &[
                ("leaflets", Count),
                ("leaflet_width", Number),
                ("teeth", Count),
                ("petiole", Number),
                ("pattern", Word),
                ("pinnules", Count),
                ("confluent", Number),
                ("angle", Number),
                ("pinnule_angle", Number),
                ("spread", Number),
                ("largest", Number),
                ("lowest", Number),
                ("terminal_size", Number),
                ("outline", Outline),
                ("widest", Number),
                ("tip", Number),
                ("base", Number),
                ("petiolule", Number),
            ],
        ),
        (
            "sprig",
            &[
                ("leaves", Count),
                ("angle", Number),
                ("stalk", Number),
                ("width", Number),
                ("outline", Outline),
                ("widest", Number),
                ("tip", Number),
                ("base", Number),
                ("teeth", Count),
                ("tooth_depth", Number),
                ("round", Number),
                ("petiole", Number),
            ],
        ),
        ("blade", &[("taper", Number), ("aspect", Number)]),
        (
            "frond",
            &[
                ("pinnae", Count),
                ("width", Number),
                ("divided", Number),
                ("stipe", Number),
            ],
        ),
        (
            "flower",
            &[
                ("petals", Count),
                ("petal_width", Number),
                ("point", Number),
                ("notch", Number),
                ("centre", Number),
                ("perianth", Word),
                ("sepals", Count),
                ("sepal_length", Number),
                ("sepals_fused", Number),
                ("corolla", Word),
                ("banner", Number),
                ("lip", Number),
                ("fused", Number),
                ("throat", Number),
                ("belly", Number),
                ("spur", Number),
                ("tube", Number),
                ("stamens", Count),
                ("stamen_length", Number),
                ("staminodes", Count),
                ("carpels", Count),
                ("ovary", Word),
                ("column", Number),
                ("turn", Number),
                ("open", Number),
            ],
        ),
        (
            "head",
            &[
                ("rays", Count),
                ("ray_width", Number),
                ("ray_tip", Number),
                ("disc", Number),
                ("kind", Word),
                ("florets", Count),
                ("phyllaries", Count),
                ("series", Count),
                ("involucre", Number),
                ("receptacle", Number),
                ("spread", Number),
                ("floret_length", Number),
                ("open", Number),
                ("pappus", Number),
            ],
        ),
        ("umbel", &[("clusters", Count), ("florets", Count)]),
        (
            "spike",
            &[
                ("florets", Count),
                ("aspect", Number),
                ("plume", Number),
                ("stalk", Number),
            ],
        ),
        (
            "panicle",
            &[
                ("whorls", Count),
                ("branches", Count),
                ("spread", Number),
                ("spikelets", Count),
                ("aspect", Number),
                ("stalk", Number),
            ],
        ),
        (
            "spikelet",
            &[
                ("florets", Count),
                ("glumes", Count),
                ("glume", Number),
                ("lower", Number),
                ("lemma", Number),
                ("aspect", Number),
                ("awn", Number),
                ("awn_from", Number),
                ("awned", Number),
                ("bent", Number),
                ("ranks", Count),
                ("gape", Number),
            ],
        ),
        (
            "fruit",
            &[
                ("aspect", Number),
                ("cone", Number),
                ("kind", Word),
                ("unripe", Number),
            ],
        ),
    ]
};

/// The field a rule's `path` sets inside a look's `shape`
/// (`appearance.organs.<organ>.shape.<field>`), if it sets one.
fn shape_field(path: &str) -> Option<&str> {
    let (_, look) = path.strip_prefix("appearance.organs.")?.split_once('.')?;
    look.strip_prefix("shape.")
}

/// The card templates a rule's `template` names: one, or a list of them.
fn templates(template: &Value) -> Result<Vec<&str>, String> {
    let names = match template {
        Value::String(name) => vec![name.as_str()],
        Value::Array(names) if !names.is_empty() => names
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<&str>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if names.is_empty() {
        return Err("a rule's `template` is a card template, or a list of them".into());
    }
    Ok(names)
}

/// The cards `templates` name, for a message: "`simple` or `sprig`".
#[must_use]
pub fn cards(templates: &[&str]) -> String {
    templates
        .iter()
        .map(|template| format!("`{template}`"))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// What `field` of the card `template` takes.
fn template_field(template: &str, field: &str) -> Result<Field, String> {
    let fields = TEMPLATES
        .iter()
        .find(|(name, _)| *name == template)
        .map(|(_, fields)| *fields)
        .ok_or_else(|| format!("`{template}` is no card template"))?;
    fields
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, takes)| *takes)
        .ok_or_else(|| format!("`{template}` cards have no `{field}`"))
}

/// What a rule for the card `template` (one or a list) may set at `path`
/// inside `appearance.organs`: something in the look of an organ the
/// species draws; inside its `shape` a field of the card, where the
/// species draws the organ with that card; and inside its `inflorescence`
/// a key of it, where the species draws the organ as a cluster (`Some` of
/// what the field or key takes).
fn look_field(
    spec: &Map<String, Value>,
    path: &str,
    template: Option<&Value>,
) -> Result<Option<Field>, String> {
    let (organ, look) = path.split_once('.').unwrap_or((path, ""));
    let mut drawn_look = find(spec, &format!("appearance.organs.{organ}"))
        .ok_or_else(|| format!("it draws no `{organ}` look"))?;
    let mut look = look;
    // A field of one of its season's stage looks.
    for stage in STAGE_LOOKS {
        if let Some(rest) = look
            .strip_prefix("season.")
            .and_then(|rest| rest.strip_prefix(stage))
            .and_then(|rest| rest.strip_prefix('.'))
        {
            drawn_look = find(spec, &format!("appearance.organs.{organ}.season.{stage}"))
                .ok_or_else(|| format!("its `{organ}` look has no `{stage}`"))?;
            look = rest;
        }
    }
    if let Some(day) = look.strip_prefix("season.")
        && SEASON_DAYS.contains(&day)
    {
        if !drawn_look.get("season").is_some_and(Value::is_object) {
            return Err(format!("its `{organ}` look has no `season`"));
        }
        return Ok(Some(Field::Number));
    }
    if let Some(key) = look.strip_prefix("inflorescence.") {
        if !drawn_look
            .get("inflorescence")
            .is_some_and(Value::is_object)
        {
            return Err(format!(
                "its `{organ}` look is no cluster: it has no `inflorescence`"
            ));
        }
        return inflorescence_takes(key).map(Some);
    }
    let drawn = drawn_look
        .get("shape")
        .and_then(|shape| shape.get("template"))
        .and_then(Value::as_str);
    // A rule choosing the card: a farther file's choice yields to it, a
    // nearer one's stands (`Ruled::apply`).
    if look == "shape.template" {
        return Ok(Some(Field::Word));
    }
    let (Some(field), Some(template)) = (look.strip_prefix("shape."), template) else {
        return Ok(None);
    };
    let templates = templates(template)?;
    match drawn {
        Some(drawn) if templates.contains(&drawn) => template_field(drawn, field).map(Some),
        Some(drawn) => Err(format!(
            "the rule is for {} cards and its `{organ}` look is drawn with `{drawn}`",
            cards(&templates)
        )),
        None => Err(format!("its `{organ}` look names no card template")),
    }
}

/// Where a rank file sits: the rank and name its place gives it.
struct Place {
    rank: String,
    name: String,
    family: Option<String>,
}

impl Place {
    fn of(file: &str) -> Result<Self, String> {
        let parts: Vec<&str> = file.split('/').collect();
        match parts.as_slice() {
            [RANKS, rank, name] => {
                let stem = name
                    .strip_suffix(".json")
                    .filter(|stem| is_name(stem))
                    .ok_or_else(|| {
                        format!(
                            "{file}: a rank file is named by its taxon, in lowercase letters, digits and hyphens, then `.json`"
                        )
                    })?;
                if *rank != CLADE
                    && !level(rank).is_some_and(|level| level != FAMILY && level != GENUS)
                {
                    return Err(format!(
                        "{file}: `{rank}` is not a rank kept in {RANKS}/; those are {CLADE} and the ranks of {} except family and genus",
                        LEVELS.join(", ")
                    ));
                }
                Ok(Self {
                    rank: (*rank).to_string(),
                    name: stem.to_string(),
                    family: None,
                })
            }
            [family, "family.json"] => Ok(Self {
                rank: "family".into(),
                name: (*family).to_string(),
                family: Some((*family).to_string()),
            }),
            [family, genus, "genus.json"] => Ok(Self {
                rank: "genus".into(),
                name: (*genus).to_string(),
                family: Some((*family).to_string()),
            }),
            _ => Err(format!(
                "{file} is not where a rank file goes: {RANKS}/<rank>/<name>.json, <family>/family.json or <family>/<genus>/genus.json"
            )),
        }
    }
}

/// A rank file's name: its taxon's in lower case, spaces as hyphens.
#[must_use]
pub fn file_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// 1 to 64 lowercase letters, digits and hyphens.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The path of the rank file a `parent` of `<rank>/<name>` names.
fn parent_file(parent: &str) -> Result<String, String> {
    let (rank, name) = parent
        .split_once('/')
        .filter(|(rank, name)| {
            is_name(name) && (*rank == CLADE || level(rank).is_some_and(|level| level != GENUS))
        })
        .ok_or_else(|| {
            format!(
                "`parent` {parent} is not `<rank>/<name>` of a rank file, such as `order/sapindales` or `family/poaceae`"
            )
        })?;
    Ok(if rank == "family" {
        format!("{name}/family.json")
    } else {
        format!("{RANKS}/{rank}/{name}.json")
    })
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Result<String, String> {
    match map.remove(key) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(format!("`{key}` is missing or not a string")),
    }
}

/// The value at `path` (keys joined by dots; an array's items by index).
#[must_use]
pub fn find<'a>(values: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = values.get(parts.next()?)?;
    for part in parts {
        value = match value {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => match part.parse::<usize>() {
                Ok(index) => items.get(index)?,
                Err(_) => by_role(items, part)?,
            },
            _ => return None,
        };
    }
    Some(value)
}

/// The item of a list of objects whose `role` is `role`: a body's spine
/// set, as a rule's path names it (`appearance.bodies.stem.spines.radial`).
fn by_role<'a>(items: &'a [Value], role: &str) -> Option<&'a Value> {
    items
        .iter()
        .find(|item| item.get("role").and_then(Value::as_str) == Some(role))
}

/// The rank files a species of the genus `genus` in the family `family`
/// stands on, nearest first: its genus's file, or its family's when the
/// genus has none, and those it stands on.
///
/// # Errors
///
/// As [`chain`].
pub fn above<'a>(
    ranks: &'a BTreeMap<String, RankFile>,
    family: &str,
    genus: &str,
) -> Result<Vec<&'a RankFile>, String> {
    let genus_file = format!("{family}/{genus}/genus.json");
    if ranks.contains_key(&genus_file) {
        chain(ranks, &genus_file)
    } else {
        chain(ranks, &format!("{family}/family.json"))
    }
}

/// The rank file `file` and those it stands on, nearest first; none if
/// the library lacks `file`.
///
/// # Errors
///
/// Fails on a named parent the library lacks, parents that come back
/// round, a rank standing on one at or below it, a clade below a family,
/// or a file below a family that names its parent but does not reach its
/// own family through it.
pub fn chain<'a>(
    ranks: &'a BTreeMap<String, RankFile>,
    file: &str,
) -> Result<Vec<&'a RankFile>, String> {
    let mut found: Vec<&'a RankFile> = Vec::new();
    // The level of the lowest ranked file met so far, and whether a clade
    // has been met.
    let mut lowest: Option<usize> = None;
    let mut over_clade = false;
    let mut next = ranks.get(file);
    while let Some(rank) = next {
        if found.iter().any(|seen| seen.file == rank.file) {
            return Err(format!("{}: its parents come back round to it", rank.file));
        }
        if let Some(below) = found.last() {
            let fits = match level(&rank.rank) {
                Some(level) => {
                    lowest.is_none_or(|lowest| level < lowest) && !(over_clade && level >= FAMILY)
                }
                None => lowest.is_none_or(|lowest| lowest <= FAMILY),
            };
            if !fits {
                return Err(format!(
                    "{} ({}) cannot stand on {} ({})",
                    below.file, below.rank, rank.file, rank.rank
                ));
            }
        }
        match level(&rank.rank) {
            Some(level) => lowest = Some(level),
            None => over_clade = true,
        }
        found.push(rank);
        next = match &rank.parent {
            None => None,
            Some(parent) => match ranks.get(parent) {
                Some(next) => Some(next),
                None if rank.named_parent => {
                    return Err(format!(
                        "{}: its parent {parent} is not in the library",
                        rank.file
                    ));
                }
                None => None,
            },
        };
    }
    if let Some(first) = found.first()
        && first.named_parent
        && level(&first.rank).is_some_and(|level| level > FAMILY)
    {
        match found.iter().find(|rank| rank.rank == "family") {
            None => {
                return Err(format!("{}: its parents never reach a family", first.file));
            }
            Some(family) if first.family.is_some() && family.family != first.family => {
                return Err(format!(
                    "{}: its parents reach {}, not its own family's file",
                    first.file, family.file
                ));
            }
            Some(_) => {}
        }
    }
    Ok(found)
}

/// A program's parameters, by name, each with its default where the
/// program writes it as a number.
pub type Params = BTreeMap<String, Option<f64>>;

/// The organ types a program declares, its own and those of the programs
/// it extends, each with its name and kind (`organ <name> <kind> …;`).
pub type Organs = Vec<(String, String)>;

/// What the rules need of each program, by name: its parameters and its
/// organ types, its own and those of the programs it extends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Programs {
    params: BTreeMap<String, Params>,
    organs: BTreeMap<String, Organs>,
}

impl Programs {
    /// The program's parameters.
    #[must_use]
    pub fn get(&self, program: &str) -> Option<&Params> {
        self.params.get(program)
    }

    /// The program's organ types, each with its kind; none for a program
    /// not known.
    #[must_use]
    pub fn organs(&self, program: &str) -> &[(String, String)] {
        self.organs.get(program).map_or(&[], Vec::as_slice)
    }

    /// Each program's name and parameters, by name.
    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, String, Params> {
        self.params.iter()
    }

    /// Each program's parameters.
    pub fn values(&self) -> std::collections::btree_map::Values<'_, String, Params> {
        self.params.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.params.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
}

impl<'a> IntoIterator for &'a Programs {
    type Item = (&'a String, &'a Params);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Params>;

    fn into_iter(self) -> Self::IntoIter {
        self.params.iter()
    }
}

impl std::ops::Index<&str> for Programs {
    type Output = Params;

    fn index(&self, program: &str) -> &Params {
        &self.params[program]
    }
}

/// The parameters and organ types each program declares, with those of
/// the programs it extends, read from the programs' text (`name`,
/// `source`): a `param` statement at the start of a line names one and its
/// default, an `organ` statement an organ type and its kind, and the
/// header `lsystem <name> <revision> extends <parent>;` names the parent,
/// whose defaults the program's own replace. The library's tests hold this
/// to the compiled programs.
#[must_use]
pub fn program_params<'a>(sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Programs {
    let mut own: BTreeMap<&str, (Params, Organs, Option<String>)> = BTreeMap::new();
    for (name, source) in sources {
        let mut params = Params::new();
        let mut organs = Organs::new();
        let mut parent = None;
        let mut header = true;
        for line in source.lines() {
            let line = line.split('#').next().unwrap_or_default();
            let words: Vec<&str> = line
                .split(|c: char| c.is_whitespace() || c == ';' || c == '=')
                .filter(|word| !word.is_empty())
                .collect();
            if words.is_empty() {
                continue;
            }
            if header {
                header = false;
                if words[0] == "lsystem" && words.get(3) == Some(&"extends") {
                    parent = words.get(4).map(|parent| (*parent).to_string());
                }
            }
            if words[0] == "param"
                && let Some(param) = words.get(1)
            {
                let default = match words[2..] {
                    [value] => value.parse().ok(),
                    _ => None,
                };
                params.insert((*param).to_string(), default);
            }
            if words[0] == "organ"
                && let [_, organ, kind, ..] = words[..]
            {
                organs.push((organ.to_string(), kind.to_string()));
            }
        }
        own.insert(name, (params, organs, parent));
    }
    let mut programs = Programs::default();
    for name in own.keys() {
        let mut params = Params::new();
        let mut organs = Organs::new();
        let mut next = Some(*name);
        // As deep as `extends` chains may go, so a loop ends.
        for _ in 0..8 {
            let Some((own_params, own_organs, parent)) = next.and_then(|at| own.get(at)) else {
                break;
            };
            for (param, default) in own_params {
                params.entry(param.clone()).or_insert(*default);
            }
            // A program's organ types follow those of the program it
            // extends, in the compiled program's order.
            organs.splice(0..0, own_organs.iter().cloned());
            next = parent.as_deref();
        }
        programs.params.insert((*name).to_string(), params);
        programs.organs.insert((*name).to_string(), organs);
    }
    programs
}

/// A species' spec as it inherits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Inherited {
    /// The effective spec, its traits turned into values by its rules. It
    /// holds no traits or rules itself, nor their notes.
    pub spec: Value,
    /// The files it was merged from, nearest first: the species' own
    /// `spec.json`, then the rank files it stands on.
    pub chain: Vec<String>,
    /// Each value's path, such as `generator.params.whorl` (an array is
    /// one value), and the file that set it: its path in the tree, then
    /// `, forms.<growth form>` for a value from a form's part. A value a
    /// rule set reads `<rule's file>: rule on <name> from <where>, …`, each
    /// trait or parameter it read with the file (or rule, or program) it
    /// came from; `<rule's file>: rule` when it read nothing.
    pub origins: BTreeMap<String, String>,
    /// Each evidence note's path and the file it came from, written as in
    /// `origins`, the notes on traits (`traits.<key>`) and rules
    /// (`rules.<path>`) among them.
    pub notes: BTreeMap<String, String>,
    /// Each trait as it stands at the species: its value, the file that
    /// set it and that file's note on it.
    pub traits: BTreeMap<String, Stated>,
    /// Each trait the species has, as each file down the chain stated it,
    /// farthest first: every statement since the nearest that deleted it,
    /// the last one the value in `traits`.
    pub statements: BTreeMap<String, Vec<Stated>>,
    /// Each rule, by the path it sets: the file it is in, and `None` if it
    /// set the path, else why it did not.
    pub rules: BTreeMap<String, (String, Option<String>)>,
}

impl Inherited {
    /// Each trait's value as stated, by key.
    #[must_use]
    pub fn trait_values(&self) -> Map<String, Value> {
        self.traits
            .iter()
            .map(|(key, stated)| (key.clone(), stated.value.clone()))
            .collect()
    }
}

/// A trait as a species inherits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stated {
    /// Its value as stated: a value, a range or qualified states.
    pub value: Value,
    /// The file that set it, written as in [`Inherited::origins`].
    pub file: String,
    /// That file's evidence note on it: on `traits.<key>`, else on all of
    /// its `traits`.
    pub note: Option<Value>,
}

/// Where a value or note came from: the file's label and its layer's
/// place in the chain, counted from the top.
type Origin = (String, usize);

/// The effective spec of a species whose own `spec.json`, at `file` in the
/// tree, holds `own`, on the rank files `above` it, nearest first as
/// [`above`] gives them, its rules checked against `programs`.
///
/// The growth form whose parts apply is the nearest one set: the
/// species', else the nearest rank file's. Values, traits and rules merge
/// down the chain, the nearer winning; then each rule sets its path from
/// the traits and parameters it reads, after any rule that sets one of
/// those parameters, unless a file at least as near as the rule and what
/// it reads set the path by hand, the path is a parameter the species'
/// program lacks, or it is in a look the species does not draw or draws
/// with another card than the rule's. Rules that read each other's paths
/// in a loop set nothing.
///
/// # Errors
///
/// Fails if `own` is not a JSON object, or its `evidence`, `traits` or
/// `rules` not an object or not well formed.
pub fn inherit(
    file: &str,
    own: &Value,
    above: &[&RankFile],
    programs: &Programs,
) -> Result<Inherited, String> {
    let Value::Object(own) = own else {
        return Err(format!("{file}: a spec is a JSON object"));
    };
    let own = Part::take(own.clone()).map_err(|error| format!("{file}: {error}"))?;
    let layers = layers(file, &own, above);
    let mut spec = Map::new();
    let mut origins: BTreeMap<String, Origin> = BTreeMap::new();
    let mut notes: BTreeMap<String, (Origin, Value)> = BTreeMap::new();
    let mut traits: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    let mut trait_notes: BTreeMap<String, Option<Value>> = BTreeMap::new();
    let mut statements: BTreeMap<String, Vec<Stated>> = BTreeMap::new();
    let mut rules: Rules = BTreeMap::new();
    for (index, (label, part)) in layers.iter().enumerate() {
        let origin = (label.clone(), index);
        let mut touched = Vec::new();
        merge(
            &mut spec,
            &part.values,
            "",
            &origin,
            &mut origins,
            &mut touched,
        );
        for (key, value) in &part.traits {
            touched.push(format!("traits.{key}"));
            if value.is_null() {
                traits.remove(key);
                trait_notes.remove(key);
                statements.remove(key);
            } else {
                traits.insert(key.clone(), (value.clone(), index));
                let note = part
                    .notes
                    .get(&format!("traits.{key}"))
                    .or_else(|| part.notes.get("traits"));
                trait_notes.insert(key.clone(), note.cloned());
                statements.entry(key.clone()).or_default().push(Stated {
                    value: value.clone(),
                    file: label.clone(),
                    note: note.cloned(),
                });
            }
        }
        for (path, rule) in &part.rules {
            touched.push(format!("rules.{path}"));
            match rule {
                Value::Object(rule) => {
                    rules.insert(path.clone(), (rule.clone(), index));
                }
                _ => {
                    rules.remove(path);
                }
            }
        }
        // A note goes where what it describes is replaced or deleted; a
        // note on a subtree stays when a nearer file sets something inside
        // it, as that file notes what it sets (per-value evidence).
        notes.retain(|path, _| !touched.iter().any(|set| within(path, set)));
        for (path, note) in &part.notes {
            notes.insert(path.clone(), (origin.clone(), note.clone()));
        }
    }
    let outcomes = Ruled::run(
        &rules,
        &layers,
        &traits,
        programs,
        &mut spec,
        &mut origins,
        &mut notes,
    );
    let carried: Map<String, Value> = notes
        .iter()
        .filter(|(path, _)| !within(path, "traits") && !within(path, "rules"))
        .map(|(path, (_, note))| (path.clone(), note.clone()))
        .collect();
    if !carried.is_empty() {
        spec.insert("evidence".into(), Value::Object(carried));
    }
    Ok(Inherited {
        spec: Value::Object(spec),
        chain: std::iter::once(file.to_string())
            .chain(above.iter().map(|rank| rank.file.clone()))
            .collect(),
        origins: origins
            .into_iter()
            .map(|(path, (label, _))| (path, label))
            .collect(),
        notes: notes
            .into_iter()
            .map(|(path, ((label, _), _))| (path, label))
            .collect(),
        traits: traits
            .into_iter()
            .map(|(key, (value, index))| {
                let note = trait_notes.remove(&key).flatten();
                let file = layers[index].0.clone();
                (key, Stated { value, file, note })
            })
            .collect(),
        statements,
        rules: outcomes,
    })
}

/// Move each of the merged `generator.defaults` into `generator.params`
/// where no file set the parameter and the species' program leaves it
/// valueless, with its origin and note, and drop the rest: a default for a
/// growth form, stated high in the tree, fills what a lineage's own program
/// does not give and yields to what it does. A rule may still set the
/// parameter, as over any value a farther file set; and a rule may set a
/// default (`generator.defaults.<name>`), which goes in as it is set, in
/// place of a default `filled` earlier from a file no nearer than the
/// rule's: a form's rule sizing a part from the species' facts replaces
/// the size stated farther up for species that state none. A default
/// filled in stands farthest of all, so a rule setting the parameter
/// itself replaces it from any file.
fn fill_defaults(
    spec: &mut Map<String, Value>,
    programs: &Programs,
    origins: &mut BTreeMap<String, Origin>,
    notes: &mut BTreeMap<String, (Origin, Value)>,
    filled: &mut BTreeMap<String, Origin>,
) {
    let Some(Value::Object(generator)) = spec.get_mut("generator") else {
        return;
    };
    let Some(Value::Object(defaults)) = generator.remove("defaults") else {
        return;
    };
    // A default's spread (from a rule reading a range) goes with it.
    let mut spreads = match generator
        .get_mut("spreads")
        .and_then(Value::as_object_mut)
        .and_then(|spreads| spreads.remove("defaults"))
    {
        Some(Value::Object(spreads)) => spreads,
        _ => Map::new(),
    };
    let params = generator
        .get("program")
        .and_then(Value::as_str)
        .and_then(|program| programs.get(program));
    for (key, value) in defaults {
        let from = format!("generator.defaults.{key}");
        let to = format!("generator.params.{key}");
        let valueless = params.is_some_and(|params| params.get(&key) == Some(&None));
        let origin = origins.get(&from).cloned();
        let depth = origin.as_ref().map_or(0, |(_, depth)| *depth);
        // Unset, or still the default filled from a file no nearer.
        let unset = generator
            .get("params")
            .and_then(|params| params.get(&key))
            .is_none()
            || filled.get(&key).is_some_and(|(label, earlier)| {
                *earlier <= depth && origins.get(&to) == Some(&(label.clone(), 0))
            });
        if valueless && unset {
            if let Value::Object(params) = generator
                .entry("params")
                .or_insert_with(|| Value::Object(Map::new()))
            {
                params.insert(key.clone(), value);
            }
            // A default stands farthest of all, so any rule may set the
            // parameter over it.
            if let Some((label, depth)) = origin {
                origins.insert(to.clone(), (label.clone(), 0));
                filled.insert(key.clone(), (label, depth));
            }
            let note = notes
                .iter()
                .filter(|(path, _)| within(&from, path))
                .max_by_key(|(path, _)| path.len())
                .map(|(_, note)| note.clone());
            if let Some(note) = note {
                notes.insert(to, note);
            }
            if let Some(spread) = spreads.remove(&key) {
                let section = generator
                    .entry("spreads")
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Value::Object(section) = section {
                    if let Value::Object(params) = section
                        .entry("params")
                        .or_insert_with(|| Value::Object(Map::new()))
                    {
                        params.insert(key.clone(), spread);
                    }
                }
                let from = format!("generator.spreads.defaults.{key}");
                if let Some(origin) = origins.get(&from).cloned() {
                    origins.insert(format!("generator.spreads.params.{key}"), origin);
                }
                // And its evidence note, which the check asks of every value.
                if let Some(note) = notes.get(&from).cloned() {
                    notes.insert(format!("generator.spreads.params.{key}"), note);
                }
            }
        }
    }
    forget(origins, "generator.defaults");
    forget(origins, "generator.spreads.defaults");
    notes.retain(|path, _| {
        !within(path, "generator.defaults") && !within(path, "generator.spreads.defaults")
    });
}

/// The end of a male plant's organ's name (`bloom_male`), as
/// `crate::spec::MALE` writes it.
pub const MALE: &str = "_male";

/// Give each organ type the species' program declares that has no look of
/// its own its kind's look from the merged `appearance.defaults`, with its
/// origin and note, and drop the defaults (LANGUAGE.md 5.2): the rules then
/// reach every organ the plant draws. A look filled in stands farthest of
/// all, as a default parameter does, so any rule may set what it holds. A
/// male plant's organ takes none: it stands for the organ it is the male
/// of.
fn fill_looks(
    spec: &mut Map<String, Value>,
    programs: &Programs,
    origins: &mut BTreeMap<String, Origin>,
    notes: &mut BTreeMap<String, (Origin, Value)>,
) {
    let program = spec
        .get("generator")
        .and_then(|generator| generator.get("program"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(Value::Object(appearance)) = spec.get_mut("appearance") else {
        return;
    };
    let Some(Value::Object(defaults)) = appearance.remove("defaults") else {
        return;
    };
    let organs = program.as_deref().map(|program| programs.organs(program));
    for (organ, kind) in organs.unwrap_or_default() {
        let Some(look) = defaults.get(kind) else {
            continue;
        };
        if organ.ends_with(MALE) {
            continue;
        }
        let Value::Object(looks) = appearance
            .entry("organs")
            .or_insert_with(|| Value::Object(Map::new()))
        else {
            continue;
        };
        if looks.contains_key(organ) {
            continue;
        }
        looks.insert(organ.clone(), look.clone());
        let from = format!("appearance.defaults.{kind}");
        let to = format!("appearance.organs.{organ}");
        let moved = |path: &str| format!("{to}{}", &path[from.len()..]);
        let taken: Vec<(String, Origin)> = origins
            .iter()
            .filter(|(path, _)| within(path, &from))
            .map(|(path, (label, _))| (moved(path), (label.clone(), 0)))
            .collect();
        origins.extend(taken);
        // Its notes: those inside the kind's default, else the nearest on
        // the defaults holding it.
        let inside: Vec<(String, (Origin, Value))> = notes
            .iter()
            .filter(|(path, _)| within(path, &from))
            .map(|(path, note)| (moved(path), note.clone()))
            .collect();
        let holding = notes
            .iter()
            .filter(|(path, _)| {
                within(&from, path) && *path != &from && within(path, "appearance.defaults")
            })
            .max_by_key(|(path, _)| path.len())
            .map(|(_, note)| note.clone());
        if let Some(note) = holding {
            notes.insert(to.clone(), note);
        }
        notes.extend(inside);
    }
    forget(origins, "appearance.defaults");
    notes.retain(|path, _| !within(path, "appearance.defaults"));
}

/// The parts a species merges, from the top down, each with its file's
/// label: each rank file's shared part and its part for the species'
/// growth form, then the species' own. The growth form is the nearest one
/// set: the species', else the nearest rank file's.
fn layers<'a>(file: &str, own: &'a Part, above: &[&'a RankFile]) -> Vec<(String, &'a Part)> {
    let form = own
        .values
        .get("growth_form")
        .or_else(|| {
            above
                .iter()
                .find_map(|rank| rank.shared.values.get("growth_form"))
        })
        .and_then(Value::as_str);
    let mut layers: Vec<(String, &Part)> = Vec::new();
    for rank in above.iter().rev() {
        layers.push((rank.file.clone(), &rank.shared));
        if let Some((form, part)) = form.and_then(|form| Some((form, rank.forms.get(form)?))) {
            layers.push((format!("{}, forms.{form}", rank.file), part));
        }
    }
    layers.push((file.to_string(), own));
    layers
}

/// One rule of a species' chain, with what it reads from.
struct Ruled<'a> {
    /// The rule's path, as its file writes it.
    path: &'a str,
    /// The path it sets: its own, or for each look its `*` names, that
    /// look's.
    at: &'a str,
    rule: &'a Map<String, Value>,
    /// The rule's layer in the chain.
    index: usize,
    layers: &'a [(String, &'a Part)],
    traits: &'a BTreeMap<String, (Value, usize)>,
    /// The values at the points of [`SPREAD_AT`] of each parameter a rule
    /// before it set from one range, recorded as a spread or not, so a
    /// rule that reads it reads a range.
    ranged: &'a RefCell<BTreeMap<String, [f64; 5]>>,
}

/// A species' rules, by the path each sets: the rule and its layer.
type Rules = BTreeMap<String, (Map<String, Value>, usize)>;

/// Fail a rule on `generator.params.<name>` unless the species' program
/// has that parameter.
fn has_param<T>(
    at: &str,
    program: Option<&str>,
    params: Option<&BTreeMap<String, T>>,
) -> Result<(), String> {
    if let Some(param) = at.strip_prefix("generator.params.") {
        let program = program.ok_or("the species names no program")?;
        if !params.is_some_and(|params| params.contains_key(param)) {
            return Err(format!("program `{program}` has no parameter `{param}`"));
        }
    }
    Ok(())
}

impl Ruled<'_> {
    /// Fill in a species' defaults ([`fill_defaults`]), then run its rules
    /// in [`order`] on its merged `spec`, and return each one's file and,
    /// if it set nothing, why.
    fn run(
        rules: &Rules,
        layers: &[(String, &Part)],
        traits: &BTreeMap<String, (Value, usize)>,
        programs: &Programs,
        spec: &mut Map<String, Value>,
        origins: &mut BTreeMap<String, Origin>,
        notes: &mut BTreeMap<String, (Origin, Value)>,
    ) -> BTreeMap<String, (String, Option<String>)> {
        let mut filled = BTreeMap::new();
        fill_defaults(spec, programs, origins, notes, &mut filled);
        fill_looks(spec, programs, origins, notes);
        let program = spec
            .get("generator")
            .and_then(|generator| generator.get("program"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut outcomes = BTreeMap::new();
        let ranged = RefCell::new(BTreeMap::new());
        for (path, waits) in order(rules) {
            let (rule, index) = &rules[path];
            let outcome = if waits.is_empty() {
                let mut set = 0;
                let mut why = None;
                let looks = every_look(spec, path, rule);
                for at in looks.clone().unwrap_or_default() {
                    let one = Ruled {
                        path,
                        at: &at,
                        rule,
                        index: *index,
                        layers,
                        traits,
                        ranged: &ranged,
                    };
                    match one.apply(program.as_deref(), programs, spec, origins, notes) {
                        Ok(()) => set += 1,
                        Err(error) => {
                            why.get_or_insert(if path.starts_with(EVERY) {
                                format!("in its `{}` look, {error}", organ_of(&at))
                            } else {
                                error
                            });
                        }
                    }
                }
                // A rule's default goes in at once, for the rules reading it.
                if set > 0 && within(path, "generator.defaults") {
                    fill_defaults(spec, programs, origins, notes, &mut filled);
                }
                match looks {
                    Err(error) => Some(error),
                    Ok(_) if set > 0 => None,
                    Ok(_) => why,
                }
            } else {
                Some(format!(
                    "it waits on rules that read each other: {}",
                    waits.join(", ")
                ))
            };
            outcomes.insert(path.to_string(), (layers[*index].0.clone(), outcome));
        }
        // A spread a rule replaced leaves no empty objects behind.
        for section in ["appearance", "generator"] {
            if let Some(Value::Object(inside)) = spec.get_mut(section) {
                if let Some(Value::Object(spreads)) = inside.get_mut("spreads") {
                    prune(spreads);
                }
                if inside
                    .get("spreads")
                    .is_some_and(|spreads| spreads.as_object().is_some_and(Map::is_empty))
                {
                    inside.remove("spreads");
                }
            }
        }
        outcomes
    }

    /// Set the rule's path from the traits and parameters it reads, or say
    /// why not.
    #[allow(clippy::too_many_lines)]
    fn apply(
        &self,
        program: Option<&str>,
        programs: &Programs,
        spec: &mut Map<String, Value>,
        origins: &mut BTreeMap<String, Origin>,
        notes: &mut BTreeMap<String, (Origin, Value)>,
    ) -> Result<(), String> {
        let params = program.and_then(|program| programs.get(program));
        has_param(self.at, program, params)?;
        let field = match self.at.strip_prefix("appearance.organs.") {
            Some(look) => look_field(spec, look, self.rule.get("template"))?,
            None => None,
        };
        in_body(spec, self.at)?;
        let mut depth = self.index;
        let mut from = Vec::new();
        let mut inputs: BTreeMap<String, Value> = BTreeMap::new();
        // A trait as stated, or a parameter.
        let mut read = |key: &str| -> Result<Value, String> {
            let path = format!("generator.params.{key}");
            let value = if let Some((name, bound)) = crate::formula::bound(key)
                && let Some((value, at)) = self.traits.get(name)
            {
                depth = depth.max(*at);
                from.push(format!("{name} from {}", self.layers[*at].0));
                bound_of(name, bound, value)?
            } else if let Some((value, at)) = self.traits.get(key) {
                depth = depth.max(*at);
                from.push(format!("{key} from {}", self.layers[*at].0));
                value.clone()
            } else if let Some(value) = season_read(spec, self.at, key) {
                // A season's own opening or ripening, where it sets a day.
                value
            } else if let Some(value) = find(spec, &path) {
                // A parameter as the files, or a rule that ran before this
                // one, set it.
                if let Some((label, at)) = origins.get(&path) {
                    depth = depth.max(*at);
                    from.push(format!("{key} from {label}"));
                }
                value.clone()
            } else if let (Some(program), Some(Some(default))) =
                (program, params.and_then(|params| params.get(key)))
            {
                from.push(format!("{key} from program `{program}`"));
                Value::from(*default)
            } else {
                return Err(format!("no `{key}` to read"));
            };
            Ok(value)
        };
        // A map reads its trait, then only what the values it picks read;
        // one drawing `between` reads every state that ties. A formula
        // needs only the names on the way it takes (`a ? b : c` reads one
        // of `b` and `c`), so a name it cannot read fails it only if the
        // formula then has no number.
        let mut picks = Vec::new();
        let mut unread = Vec::new();
        let names = if let Some((key, table)) = map_of(self.rule) {
            match read(key) {
                Ok(stated) => {
                    let read = if self.rule.contains_key("between") {
                        readings(key, &stated).map(|(picks, _)| picks)
                    } else {
                        reading(key, &stated).map(|pick| vec![pick])
                    };
                    match read {
                        Ok(read) => {
                            picks = read;
                            picked_reads(key, table, &picks)?
                        }
                        // Its states tie for the most usual, or none says
                        // how often: a map with an `else` gives that.
                        Err(_) if self.rule.contains_key("else") => Vec::new(),
                        Err(why) => return Err(why),
                    }
                }
                // Its trait unstated, a map with an `else` gives that, or
                // what the map its `else` holds reads.
                Err(_) if self.rule.contains_key("else") => self
                    .rule
                    .get("else")
                    .and_then(|otherwise| map_rule(otherwise).or_else(|| map_colour(otherwise)))
                    .map(reads)
                    .unwrap_or_default(),
                Err(why) => return Err(why),
            }
        } else {
            reads(self.rule)
        };
        for name in names {
            if let Entry::Vacant(entry) = inputs.entry(name) {
                match read(entry.key()).and_then(|value| reading(entry.key(), &value)) {
                    Ok(value) => {
                        entry.insert(value);
                    }
                    Err(why) => unread.push(why),
                }
            }
        }
        if let Some((label, by)) = origins.get(self.at).or_else(|| self.listed(spec, origins))
            && *by >= depth
        {
            return Err(format!("set by hand in {label}"));
        }
        let mut rivals = self.rivals(field, spec, origins, depth)?;
        let typical = inputs.clone();
        let value = picked(self.rule, &picks, &mut inputs).map_err(|why| {
            // Its number failed for want of a name it could not read.
            match unread.first() {
                Some(first) if why == NO_NUMBER => first.clone(),
                _ => why,
            }
        })?;
        fits(field, &value)?;
        if let (Some(shape), Some(card)) = (self.at.strip_suffix(".template"), value.as_str())
            && shape_field(self.at) == Some("template")
        {
            rivals.extend(Self::unfit(shape, card, spec, origins, depth)?);
        }
        let spread = self.spread(&picks, &typical, &value);
        let set_at = put(spec, origins, notes, (self.at, value, spread), &rivals);
        let file = &self.layers[self.index].0;
        // A trait read at both its bounds is named once.
        from.dedup();
        let label = if from.is_empty() {
            format!("{file}: rule")
        } else {
            format!("{file}: rule on {}", from.join(", "))
        };
        let origin = (label, depth);
        // The rule's note: on its path, else on all of its file's rules.
        let part = self.layers[self.index].1;
        let note = part
            .notes
            .get(&format!("rules.{}", self.path))
            .or_else(|| part.notes.get("rules"));
        for at in set_at {
            if let Some(note) = note {
                notes.insert(at.clone(), (origin.clone(), note.clone()));
            }
            origins.insert(at, origin.clone());
        }
        Ok(())
    }

    /// The spread of `value`, the rule's value from `inputs`, where it
    /// reads one thing that varies, a range or a parameter a rule before it
    /// set from one: its value at each point of [`SPREAD_AT`] with that
    /// read there ([`range_at`]), the middle one `value` itself, kept for
    /// the rules that read the parameter it sets ([`Ruled::ranged`]).
    /// Nothing where it reads none (a bound, `min(name)`, is one number),
    /// or two (how two ranges vary together is not stated), or its values
    /// are not all numbers or all the same, or it sets a body's look, which
    /// varies within the ranges it states; nor, though kept, where it sets
    /// a parameter to other than a character of the whole plant in its own
    /// units.
    fn spread(
        &self,
        picks: &[Value],
        inputs: &BTreeMap<String, Value>,
        value: &Value,
    ) -> Option<Value> {
        if self.at.starts_with(BODIES) {
            return None;
        }
        let varying: Vec<(&String, [f64; 5])> = inputs
            .keys()
            .filter_map(|name| {
                let mut at = [0.0; 5];
                match self.traits.get(name) {
                    Some((Value::Object(stated), _)) if stated.values().all(Value::is_number) => {
                        for (slot, share) in at.iter_mut().zip(SPREAD_AT) {
                            *slot = range_at(stated, share)?;
                        }
                    }
                    Some(_) => return None,
                    None => at = *self.ranged.borrow().get(name)?,
                }
                Some((name, at))
            })
            .collect();
        let [(name, at)] = varying.as_slice() else {
            return None;
        };
        if !value.is_number() {
            return None;
        }
        let mut values = Vec::with_capacity(SPREAD_AT.len());
        for (point, at) in at.iter().enumerate() {
            if point == SPREAD_AT.len() / 2 {
                values.push(value.clone());
                continue;
            }
            let mut read = inputs.clone();
            read.insert((*name).clone(), Value::from(*at));
            values.push(picked(self.rule, picks, &mut read).ok()?);
        }
        // A parameter varies plant by plant only where it is a character of
        // the whole plant (`traits.json` names its organ `plant`: the years
        // to its first flowers), in its own units. A part's size (a leaf's,
        // a joint's) varies part by part, and drawn for a whole plant spans
        // all its parts span: a teddy-bear cholla drawn with its shortest
        // joints grew to 41% of its reference height. One a formula works
        // out from a range is the program's tuning at the typical value:
        // drawn by plant, spermatophyta's growth units grew a lenga beech to
        // half its height.
        let of_plant = || trait_organ(name).is_none_or(|organ| organ == "plant");
        let scale = value
            .as_f64()
            .zip(Some(at[SPREAD_AT.len() / 2]).filter(|middle| *middle != 0.0))
            .map(|(value, middle)| value / middle);
        let recorded = !self.at.starts_with("generator.")
            || of_plant()
                && scale.is_some_and(|scale| {
                    values.iter().zip(at).all(|(value, at)| {
                        value.as_f64().is_some_and(|value| {
                            (value - scale * at).abs() <= 1e-9 * (scale * at).abs().max(1e-9)
                        })
                    })
                });
        if !values.iter().all(Value::is_number) || values.iter().all(|other| other == value) {
            return None;
        }
        let spread = Value::Array(values);
        if let Some(param) = self.at.strip_prefix("generator.params.")
            && let Some(found) = Spread::of(&spread)
        {
            self.ranged
                .borrow_mut()
                .insert(param.to_string(), found.values);
        }
        recorded.then_some(spread)
    }

    /// Where the rule sets a field of a list's item by its role
    /// (`spines.radial.count`), the origin of the list, if the item has
    /// the field: a field written in a list a file sets is set by hand
    /// there.
    fn listed<'b>(
        &self,
        spec: &Map<String, Value>,
        origins: &'b BTreeMap<String, Origin>,
    ) -> Option<&'b Origin> {
        find(spec, self.at)?;
        let mut at = self.at;
        while let Some((outer, _)) = at.rsplit_once('.') {
            if find(spec, outer).is_some_and(Value::is_array) {
                return origins.get(outer);
            }
            at = outer;
        }
        None
    }

    /// The fields of the card at `shape` that a `card` chosen in its
    /// place lacks, which go with the card a farther file chose; fails if
    /// a file at least as near as the rule set one by hand.
    fn unfit(
        shape: &str,
        card: &str,
        spec: &Map<String, Value>,
        origins: &BTreeMap<String, Origin>,
        depth: usize,
    ) -> Result<Vec<String>, String> {
        let fields = find(spec, shape)
            .and_then(Value::as_object)
            .map(|fields| fields.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut found = Vec::new();
        for name in fields {
            if name == "template" || template_field(card, &name).is_ok() {
                continue;
            }
            let path = format!("{shape}.{name}");
            if let Some((label, by)) = origins.get(&path)
                && *by >= depth
            {
                return Err(format!(
                    "its card's `{name}`, which a `{card}` card lacks, is set by hand in {label}"
                ));
            }
            found.push(path);
        }
        Ok(found)
    }

    /// The fields of the rule's card in `spec` that cannot stand beside the
    /// one it sets ([`REPLACES`]), for it to replace; or why it sets
    /// nothing, where one was set at least as near as what it reads
    /// (`depth`).
    fn rivals(
        &self,
        field: Option<Field>,
        spec: &Map<String, Value>,
        origins: &BTreeMap<String, Origin>,
        depth: usize,
    ) -> Result<Vec<String>, String> {
        let (Some(_), Some((shape, name))) = (field, self.at.rsplit_once('.')) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for rival in rivals(name) {
            let path = format!("{shape}.{rival}");
            if find(spec, &path).is_none() {
                continue;
            }
            if let Some((label, by)) = origins.get(&path)
                && *by >= depth
            {
                return Err(format!("its card's `{rival}` is set by hand in {label}"));
            }
            found.push(path);
        }
        Ok(found)
    }
}

/// A rule's value from what it reads (`inputs`) and, for a map, the
/// states of its trait it `picks`: its value for the one state, or the
/// value between its values for states that tie ([`between`]).
fn picked(
    rule: &Map<String, Value>,
    picks: &[Value],
    inputs: &mut BTreeMap<String, Value>,
) -> Result<Value, String> {
    let Some((key, _)) = map_of(rule) else {
        return evaluate(rule, inputs);
    };
    if picks.is_empty() {
        return evaluate(rule, inputs);
    }
    let mut values = Vec::new();
    for pick in picks {
        inputs.insert(key.clone(), pick.clone());
        values.push(evaluate(rule, inputs)?);
    }
    match values.as_slice() {
        [one] => Ok(one.clone()),
        _ => between(&values).ok_or_else(|| {
            format!(
                "its values for `{key}` {} have nothing between them",
                picks.iter().map(state).collect::<Vec<_>>().join(" and ")
            )
        }),
    }
}

/// Whether a rule's `value` fits the card's `field` it sets, if it sets
/// one, or what the field takes.
fn fits(field: Option<Field>, value: &Value) -> Result<(), String> {
    let (fits, takes) = match field {
        None => (true, ""),
        Some(Field::Count) => (value.is_u64(), "a whole number, 0 or more"),
        Some(Field::Number) => (value.is_number(), "a number"),
        Some(Field::Word) => (value.is_string(), "a word"),
        Some(Field::Outline) => (
            value.as_array().is_some_and(|points| {
                points.iter().all(|point| {
                    point
                        .as_array()
                        .is_some_and(|pair| pair.len() == 2 && pair.iter().all(Value::is_number))
                })
            }),
            "`[u, w]` points",
        ),
    };
    if fits {
        Ok(())
    } else {
        Err(format!("it gives {value}, and the look takes {takes}"))
    }
}

/// The order rules run in, by path: a rule that reads a parameter another
/// rule sets runs after it. Rules caught in a loop of such reads come
/// last, each with the paths it waits on.
fn order(rules: &Rules) -> Vec<(&str, Vec<String>)> {
    // A rule reading a parameter waits on the rule setting it, and one on
    // a card's field on the rule choosing that organ's card.
    let waits = |path: &str, rule: &Map<String, Value>| -> Vec<String> {
        let cards = rules.keys().filter(|card| {
            shape_field(card) == Some("template")
                && shape_field(path).is_some_and(|field| field != "template")
                && (path.starts_with(EVERY) || organ_of(card) == organ_of(path))
        });
        reads(rule)
            .into_iter()
            .flat_map(|name| {
                [
                    format!("generator.params.{name}"),
                    format!("generator.defaults.{name}"),
                ]
            })
            .filter(|path| rules.contains_key(path))
            .chain(cards.cloned())
            .collect()
    };
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut order = Vec::new();
    loop {
        let ready: Vec<&str> = rules
            .iter()
            .filter(|(path, (rule, _))| {
                !done.contains(path.as_str())
                    && waits(path, rule)
                        .iter()
                        .all(|wait| done.contains(wait.as_str()))
            })
            .map(|(path, _)| path.as_str())
            .collect();
        if ready.is_empty() {
            break;
        }
        done.extend(ready.iter().copied());
        order.extend(ready.into_iter().map(|path| (path, Vec::new())));
    }
    for (path, (rule, _)) in rules {
        if !done.contains(path.as_str()) {
            let waiting = waits(path, rule)
                .into_iter()
                .filter(|wait| !done.contains(wait.as_str()))
                .collect();
            order.push((path.as_str(), waiting));
        }
    }
    order
}

/// The names a rule may read: its formula's (traits or parameters), or its
/// map's trait and what its values' formulas read.
fn reads(rule: &Map<String, Value>) -> Vec<String> {
    let names = |formula: &str| {
        Formula::parse(formula)
            .map(|formula| formula.names())
            .unwrap_or_default()
    };
    if let Some(colours) = colour_names(rule) {
        return colours.into_iter().map(str::to_string).collect();
    }
    match rule.get("rule") {
        Some(Value::String(formula)) => return names(formula).into_iter().collect(),
        Some(Value::Array(formulas)) => {
            let all: BTreeSet<String> = formulas
                .iter()
                .filter_map(Value::as_str)
                .flat_map(names)
                .collect();
            return all.into_iter().collect();
        }
        _ => {}
    }
    let Some((key, table)) = map_of(rule) else {
        return Vec::new();
    };
    let mut read = vec![key.clone()];
    let more: BTreeSet<String> = table
        .values()
        .filter_map(map_formula)
        .flat_map(names)
        .chain(
            table
                .values()
                .chain(rule.get("else"))
                .filter_map(|value| map_rule(value).or_else(|| map_colour(value)))
                .flat_map(reads),
        )
        .collect();
    read.extend(more.into_iter().filter(|name| name != key));
    read
}

/// What the formulas a map's `picks` give read, besides its own trait.
fn picked_reads(
    key: &str,
    table: &Map<String, Value>,
    picks: &[Value],
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for pick in picks {
        if let Some(inner) = table
            .get(&state(pick))
            .and_then(|value| map_rule(value).or_else(|| map_colour(value)))
        {
            names.extend(reads(inner).into_iter().filter(|name| name != key));
            continue;
        }
        let Some(formula) = table.get(&state(pick)).and_then(map_formula) else {
            continue;
        };
        names.extend(
            Formula::parse(formula)?
                .names()
                .into_iter()
                .filter(|name| name != key),
        );
    }
    Ok(names)
}

/// A `map` rule's trait and its table of values.
fn map_of(rule: &Map<String, Value>) -> Option<(&String, &Map<String, Value>)> {
    let (key, table) = rule.get("map")?.as_object()?.iter().next()?;
    Some((key, table.as_object()?))
}

/// A map's value that is a formula, `{"rule": formula}`, worked out only
/// when the map picks it.
fn map_formula(value: &Value) -> Option<&str> {
    match value {
        Value::Object(entry) if entry.len() == 1 => entry.get("rule")?.as_str(),
        _ => None,
    }
}

/// The colour characters a colour rule (`{"colour": name}` or
/// `{"colour": [names]}`) reads, in order: it sets the first of them the
/// species states (LANGUAGE.md 5.2).
fn colour_names(rule: &Map<String, Value>) -> Option<Vec<&str>> {
    match rule.get("colour")? {
        Value::String(name) => Some(vec![name.as_str()]),
        Value::Array(names) => names.iter().map(Value::as_str).collect(),
        _ => None,
    }
}

/// A map's value that is a colour rule, `{"colour": …}`.
fn map_colour(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(entry)
            if entry.contains_key("colour")
                && entry.keys().all(|key| key == "colour" || key == "scale") =>
        {
            Some(entry)
        }
        _ => None,
    }
}

fn check_colour(rule: &Map<String, Value>) -> Result<(), String> {
    match colour_names(rule) {
        Some(names) if !names.is_empty() => {}
        _ => {
            return Err(
                "a `colour` rule names a colour character, or a list of them, of which it takes the first stated"
                    .into(),
            );
        }
    }
    match rule.get("scale") {
        None => Ok(()),
        Some(scale) if colour_scale(scale).is_some() => Ok(()),
        Some(_) => Err("a colour's `scale` is three numbers 0 to 1, one for each channel".into()),
    }
}

/// A colour rule's `scale`, three numbers 0 to 1 its channels are
/// multiplied by (a shade's darker colour from a lit one).
fn colour_scale(scale: &Value) -> Option<[f64; 3]> {
    let channels: Vec<f64> = scale
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<_>>()?;
    let channels: [f64; 3] = channels.try_into().ok()?;
    channels
        .iter()
        .all(|channel| (0.0..=1.0).contains(channel))
        .then_some(channels)
}

/// Every colour character a rule reads, in its `colour` or in a map's
/// values (so the vocabulary can hold them to colour characters).
#[must_use]
pub fn colour_reads(rule: &Map<String, Value>) -> Vec<String> {
    let mut names: Vec<String> = colour_names(rule)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect();
    if let Some((_, table)) = map_of(rule) {
        for value in table.values().chain(rule.get("else")) {
            if let Some(colour) = map_colour(value) {
                names.extend(colour_reads(colour));
            } else if let Some(inner) = map_rule(value) {
                names.extend(colour_reads(inner));
            }
        }
    }
    names
}

/// The colour of the first of `names` the species states, its channels
/// (0 to 255) on the looks' 0 to 1 scale, each times its `scale`.
fn colour_of(
    names: &[&str],
    scale: [f64; 3],
    inputs: &BTreeMap<String, Value>,
) -> Result<Value, String> {
    for name in names {
        if let Some(Value::Array(channels)) = inputs.get(*name) {
            return channels
                .iter()
                .zip(scale)
                .map(|(channel, by)| {
                    channel
                        .as_f64()
                        .and_then(|c| written(plain(c / 255.0 * by)))
                })
                .collect::<Option<Vec<_>>>()
                .map(Value::Array)
                .ok_or_else(|| format!("`{name}` is no colour"));
        }
    }
    Err(format!(
        "no {} to read",
        names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" or ")
    ))
}

/// A map's value (or `else`) that is a map on another trait, `{"map":
/// ...}` with its own `else`, worked out only when the outer map picks it.
fn map_rule(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(entry) if entry.contains_key("map") => Some(entry),
        _ => None,
    }
}

/// The state a map looks up for a trait's value: a word as itself, a
/// count as its nearest whole number (a range is read at its middle, so 2
/// to 3 pinnate reads 3), any other value as JSON writes it.
fn state(value: &Value) -> String {
    match value {
        Value::String(name) => name.clone(),
        Value::Number(number) => number
            .as_f64()
            .map_or_else(|| number.to_string(), |count| format!("{}", count.round())),
        other => other.to_string(),
    }
}

/// The one value a rule reads from a trait as stated: of a range, its
/// `typical`, else the middle of its `min` and `max`; of qualified states,
/// the most usual (a bool's as a bool); else the value itself.
fn reading(key: &str, value: &Value) -> Result<Value, String> {
    match readings(key, value)? {
        (mut one, _) if one.len() == 1 => Ok(one.remove(0)),
        (tied, frequency) => Err(format!(
            "`{key}` has no one most usual state: {} are each {}",
            tied.iter().map(state).collect::<Vec<_>>().join(" and "),
            frequency.unwrap_or_default()
        )),
    }
}

/// The values a rule may read from a trait as stated, as [`reading`], but
/// of qualified states every one that ties for the most usual, with how
/// often they occur.
fn readings(key: &str, value: &Value) -> Result<(Vec<Value>, Option<&'static str>), String> {
    let Value::Object(stated) = value else {
        return Ok((vec![value.clone()], None));
    };
    if stated.values().all(Value::is_number) {
        let bound = |name: &str| stated.get(name).and_then(Value::as_f64);
        return bound("typical")
            .or_else(|| Some(f64::midpoint(bound("min")?, bound("max")?)))
            .and_then(serde_json::Number::from_f64)
            .map(|number| (vec![Value::Number(number)], None))
            .ok_or_else(|| format!("`{key}` is a range without a `min` and a `max`"));
    }
    let rank = |frequency: &Value| {
        FREQUENCIES
            .iter()
            .position(|known| frequency.as_str() == Some(known))
    };
    let best = stated
        .values()
        .filter_map(rank)
        .min()
        .ok_or_else(|| format!("`{key}` names no state with how often it occurs"))?;
    let most = stated
        .iter()
        .filter(|(_, frequency)| rank(frequency) == Some(best))
        .map(|(state, _)| match state.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(state.clone()),
        })
        .collect();
    Ok((most, Some(FREQUENCIES[best])))
}

/// The value between `values`, the values of states that tie: their one
/// value where they agree (a disciform and a discoid head are both drawn
/// discoid); else of numbers, their mean; of lists as long as each other,
/// each item's value between theirs (outlines on the same `u` give the
/// outline between them). Nothing for words that differ, or for lists of
/// different lengths.
fn between(values: &[Value]) -> Option<Value> {
    if let Some(first) = values.first()
        && values.iter().all(|value| value == first)
    {
        return Some(first.clone());
    }
    let count = f64::from(u32::try_from(values.len()).ok()?);
    if let Some(numbers) = values
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<f64>>>()
    {
        return written(plain(numbers.iter().sum::<f64>() / count));
    }
    let lists = values
        .iter()
        .map(Value::as_array)
        .collect::<Option<Vec<&Vec<Value>>>>()?;
    let length = lists.first()?.len();
    if lists.iter().any(|list| list.len() != length) {
        return None;
    }
    (0..length)
        .map(|item| {
            between(
                &lists
                    .iter()
                    .map(|list| list[item].clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Option<Vec<Value>>>()
        .map(Value::Array)
}

/// Why a formula has no number.
const NO_NUMBER: &str = "what it reads gives it no number";

/// A rule's value from what it reads, by name: its formula's number (a
/// bool trait reads as 1 or 0), or its map's value for the trait's, worked
/// out if that value is a formula.
fn evaluate(rule: &Map<String, Value>, inputs: &BTreeMap<String, Value>) -> Result<Value, String> {
    let calculate = |formula: &str| {
        let number = |key: &str| {
            inputs
                .get(key)
                .and_then(|value| value.as_f64().or_else(|| value.as_bool().map(f64::from)))
        };
        Formula::parse(formula)?
            .eval(&number)
            .map(plain)
            .and_then(written)
            .ok_or_else(|| NO_NUMBER.to_string())
    };
    if let Some(names) = colour_names(rule) {
        let scale = rule.get("scale").and_then(colour_scale).unwrap_or([1.0; 3]);
        return colour_of(&names, scale, inputs);
    }
    match rule.get("rule") {
        Some(Value::String(formula)) => return calculate(formula),
        Some(Value::Array(formulas)) => {
            return formulas
                .iter()
                .map(|formula| calculate(formula.as_str().unwrap_or_default()))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array);
        }
        _ => {}
    }
    if let Some((key, table)) = map_of(rule) {
        let Some(value) = inputs.get(key) else {
            return match rule.get("else") {
                Some(otherwise) => match map_rule(otherwise).or_else(|| map_colour(otherwise)) {
                    Some(inner) => evaluate(inner, inputs),
                    None => Ok(otherwise.clone()),
                },
                None => Err(format!("no `{key}` to read")),
            };
        };
        let name = state(value);
        let picked = table
            .get(&name)
            .ok_or_else(|| format!("its map holds nothing for `{key}` {name}"))?;
        if let Some(inner) = map_rule(picked).or_else(|| map_colour(picked)) {
            return evaluate(inner, inputs);
        }
        return match map_formula(picked) {
            Some(formula) => calculate(formula),
            None => Ok(picked.clone()),
        };
    }
    Err("it is no rule".into())
}

/// A rule's number kept to 6 significant digits: as plain as a value set
/// by hand, and it reads back as written once merged.
fn plain(value: f64) -> f64 {
    format!("{value:.5e}").parse().unwrap_or(value)
}

/// A rule's number as JSON writes it: a whole number as one, so a rule can
/// set a count (`round(ray_florets)`), else as a decimal; nothing for a
/// number that is not finite.
fn written(value: f64) -> Option<Value> {
    if value.fract() == 0.0
        && let Ok(whole) = format!("{value:.0}").parse::<i64>()
    {
        return Some(Value::from(whole));
    }
    serde_json::Number::from_f64(value).map(Value::Number)
}

/// Remove the value at `path` (keys joined by dots), if there is one.
fn remove(target: &mut Map<String, Value>, path: &str) {
    match path.split_once('.') {
        None => {
            target.remove(path);
        }
        Some((key, rest)) => {
            if let Some(Value::Object(inside)) = target.get_mut(key) {
                remove(inside, rest);
            }
        }
    }
}

/// Set the value at `path` (keys joined by dots), making the objects on
/// the way.
fn set(target: &mut Map<String, Value>, path: &str, value: Value) {
    match path.split_once('.') {
        None => {
            target.insert(path.to_string(), value);
        }
        Some((key, rest)) => {
            let entry = target
                .entry(key.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            // A list's item by its role (`spines.radial.count`).
            if let Value::Array(items) = entry {
                let (role, rest) = rest.split_once('.').unwrap_or((rest, ""));
                if let Some(Value::Object(inside)) = items
                    .iter_mut()
                    .find(|item| item.get("role").and_then(Value::as_str) == Some(role))
                    && !rest.is_empty()
                {
                    set(inside, rest, value);
                }
                return;
            }
            if !entry.is_object() {
                *entry = Value::Object(Map::new());
            }
            if let Value::Object(inside) = entry {
                set(inside, rest, value);
            }
        }
    }
}

/// The effective spec of a species, as the library stores it: `None` when
/// no rank file stands above it and it states no traits or rules, so it is
/// its own text; else the merged spec as pretty JSON.
///
/// # Errors
///
/// As [`inherit`], and on an own text that is not JSON.
pub fn effective_text(
    file: &str,
    own: &str,
    above: &[&RankFile],
    programs: &Programs,
) -> Result<Option<String>, String> {
    if above.is_empty() && !own.contains("\"traits\"") && !own.contains("\"rules\"") {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(own).map_err(|error| format!("{file}: invalid JSON: {error}"))?;
    if above.is_empty() && value.get("traits").is_none() && value.get("rules").is_none() {
        return Ok(None);
    }
    let inherited = inherit(file, &value, above, programs)?;
    let text = serde_json::to_string_pretty(&inherited.spec)
        .map_err(|error| format!("{file}: {error}"))?;
    // serde_json reads some numbers of 16 or more digits back a bit off
    // from how it wrote them: refuse a merged spec that does not read back
    // as merged, rather than change its package unseen.
    let back: Value =
        serde_json::from_str(&text).map_err(|error| format!("{file}: invalid JSON: {error}"))?;
    if let Some(path) = difference(&inherited.spec, &back, "") {
        return Err(format!(
            "{file}: `{path}` does not read back as written once merged; give it at most 15 significant digits"
        ));
    }
    Ok(Some(text))
}

/// The first path where two values differ, if they do.
fn difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
            a.iter().find_map(|(key, inside)| {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match b.get(key) {
                    Some(other) => difference(inside, other, &path),
                    None => Some(path),
                }
            })
        }
        _ => (a != b).then(|| at.to_string()),
    }
}

/// Lay `patch` over `target` by RFC 7396.
pub fn patch(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    merge(
        target,
        patch,
        "",
        &(String::new(), 0),
        &mut BTreeMap::new(),
        &mut Vec::new(),
    );
}

/// `values` with every null left out, at any depth.
#[must_use]
pub fn without_nulls(values: &Map<String, Value>) -> Map<String, Value> {
    values
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| match value {
            Value::Object(inside) => (key.clone(), Value::Object(without_nulls(inside))),
            _ => (key.clone(), value.clone()),
        })
        .collect()
}

/// Merge `layer` into `target` by RFC 7396, noting `origin` as the origin
/// of each value it sets and every path it sets, replaces or deletes in
/// `touched`.
fn merge(
    target: &mut Map<String, Value>,
    layer: &Map<String, Value>,
    at: &str,
    origin: &Origin,
    origins: &mut BTreeMap<String, Origin>,
    touched: &mut Vec<String>,
) {
    for (key, value) in layer {
        let path = if at.is_empty() {
            key.clone()
        } else {
            format!("{at}.{key}")
        };
        match value {
            Value::Null => {
                target.remove(key);
                forget(origins, &path);
                touched.push(path);
            }
            Value::Object(inside) => {
                let entry = target.entry(key.clone()).or_insert(Value::Null);
                if !entry.is_object() {
                    forget(origins, &path);
                    *entry = Value::Object(Map::new());
                    touched.push(path.clone());
                }
                if let Value::Object(entry) = entry {
                    merge(entry, inside, &path, origin, origins, touched);
                }
            }
            _ => {
                forget(origins, &path);
                target.insert(key.clone(), value.clone());
                origins.insert(path.clone(), origin.clone());
                touched.push(path);
            }
        }
    }
}

/// Set `value` at `at` in `spec`, and beside it its `spread`, if any
/// ([`spread_path`]), in place of what was at either and of `rivals` and
/// their spreads, whose origins and notes go with them; the paths set.
fn put(
    spec: &mut Map<String, Value>,
    origins: &mut BTreeMap<String, Origin>,
    notes: &mut BTreeMap<String, (Origin, Value)>,
    (at, value, spread): (&str, Value, Option<Value>),
    rivals: &[String],
) -> Vec<String> {
    let spread_at = spread_path(at);
    let replaced = rivals
        .iter()
        .flat_map(|rival| std::iter::once(rival.clone()).chain(spread_path(rival)))
        .chain(std::iter::once(at.to_string()))
        .chain(spread_at.clone());
    for path in replaced {
        remove(spec, &path);
        forget(origins, &path);
        notes.retain(|noted, _| !within(noted, &path));
    }
    set(spec, at, value);
    let mut set_at = vec![at.to_string()];
    if let (Some(spread_at), Some(spread)) = (spread_at, spread) {
        set(spec, &spread_at, spread);
        set_at.push(spread_at);
    }
    set_at
}

/// Remove every object within `map` left empty, from the inside out.
fn prune(map: &mut Map<String, Value>) {
    map.retain(|_, value| match value {
        Value::Object(inside) => {
            prune(inside);
            !inside.is_empty()
        }
        _ => true,
    });
}

/// Forget the origins of the value at `path` and of everything inside it.
fn forget<V>(origins: &mut BTreeMap<String, V>, path: &str) {
    origins.remove(path);
    // The paths inside `path` sort together: from `path.` up to `path/`.
    let inside: Vec<String> = origins
        .range(format!("{path}.")..format!("{path}/"))
        .map(|(inside, _)| inside.clone())
        .collect();
    for inside in inside {
        origins.remove(&inside);
    }
}

/// Whether `path` is `outer` or lies inside it.
fn within(path: &str, outer: &str) -> bool {
    path == outer
        || (path.len() > outer.len()
            && path.starts_with(outer)
            && path.as_bytes()[outer.len()] == b'.')
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        Programs, RankFile, Stated, above, chain, effective_text, file_name, inherit,
        program_params,
    };

    /// Three programs: one with every parameter the tests set, one that
    /// extends it, one without `space_density`.
    fn programs() -> Programs {
        program_params([
            (
                "broadleaf",
                "# A test.\nlsystem broadleaf 1;\nparam a = 1;\nparam b = 1;\nparam c = 1;\nparam d = 1;\nparam alternate = 0;\nparam space_density = 5;",
            ),
            (
                "sapindaceae",
                "lsystem sapindaceae 1 extends broadleaf;\nparam fork = 1;\nparam d = 3;\nparam e = d * 2;",
            ),
            (
                "shrub",
                "lsystem shrub 1;\nparam a = 1;  # the first\nparam b = 1;",
            ),
            (
                "herb",
                "lsystem herb 1;\nparam leaves_per_node = 1;\nparam divergence = 137.5;",
            ),
        ])
    }

    fn ranks(files: &[(&str, Value)]) -> BTreeMap<String, RankFile> {
        files
            .iter()
            .map(|(file, value)| {
                (
                    (*file).to_string(),
                    RankFile::parse(file, &value.to_string())
                        .unwrap_or_else(|error| panic!("{file}: {error}")),
                )
            })
            .collect()
    }

    /// RFC 7396's own examples (its appendix A), each patch laid by a
    /// rank file over a species' value.
    #[test]
    fn values_merge_as_json_merge_patch() {
        let cases = [
            (json!({"a": "b"}), json!({"a": "c"}), json!({"a": "c"})),
            (
                json!({"a": "b"}),
                json!({"b": "c"}),
                json!({"a": "b", "b": "c"}),
            ),
            (json!({"a": "b"}), json!({"a": null}), json!({})),
            (
                json!({"a": "b", "b": "c"}),
                json!({"a": null}),
                json!({"b": "c"}),
            ),
            (json!({"a": ["b"]}), json!({"a": "c"}), json!({"a": "c"})),
            (json!({"a": "c"}), json!({"a": ["b"]}), json!({"a": ["b"]})),
            (
                json!({"a": {"b": "c"}}),
                json!({"a": {"b": "d", "c": null}}),
                json!({"a": {"b": "d"}}),
            ),
            (
                json!({"a": [{"b": "c"}]}),
                json!({"a": [1]}),
                json!({"a": [1]}),
            ),
            (
                json!({}),
                json!({"a": {"bb": {"ccc": null}}}),
                json!({"a": {"bb": {}}}),
            ),
        ];
        for (target, patch, wanted) in cases {
            // The family file holds the target, the species the patch.
            let mut family = json!({"schema": 1, "rank": "family", "name": "Testaceae"});
            family["generator"] = target.clone();
            let files = ranks(&[("testaceae/family.json", family)]);
            let chain = above(&files, "testaceae", "testa").unwrap();
            let own = json!({"generator": patch});
            let inherited = inherit(
                "testaceae/testa/testa-one/spec.json",
                &own,
                &chain,
                &programs(),
            )
            .unwrap();
            assert_eq!(
                inherited.spec,
                json!({"generator": wanted}),
                "{target} + {patch}"
            );
        }
    }

    /// A clade, an order, a family with a part per form, and a genus.
    fn sapindaceae() -> BTreeMap<String, RankFile> {
        ranks(&[
            (
                "_ranks/clade/eudicots.json",
                json!({"schema": 1, "rank": "clade", "name": "eudicots",
                    "tier": "procedural_proxy",
                    "generator": {"program": "broadleaf", "params": {"a": 1.0, "b": 1.0}},
                    "evidence": {"generator.params": {"evidence": "Authored", "note": "Clade."}}}),
            ),
            (
                "_ranks/order/sapindales.json",
                json!({"schema": 1, "rank": "order", "name": "Sapindales",
                    "parent": "clade/eudicots",
                    "generator": {"params": {"c": 1.0}},
                    "evidence": {"generator.params.c": {"evidence": "Authored", "note": "Order."}}}),
            ),
            (
                "sapindaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Sapindaceae",
                "parent": "order/sapindales",
                "generator": {"params": {"b": 2.0}},
                "forms": {
                    "decurrent_tree": {"generator": {"program": "sapindaceae", "params": {"b": 3.0}}},
                    "shrub": {"generator": {"program": "shrub"}}
                }}),
            ),
            (
                "sapindaceae/acer/genus.json",
                json!({"schema": 1, "rank": "genus", "name": "Acer",
                    "growth_form": "decurrent_tree",
                    "generator": {"params": {"a": null}}}),
            ),
        ])
    }

    /// The chain from the top down, each file's values over those above,
    /// a form's part over its file's shared values; each value's origin
    /// and each note from the file that set what it describes.
    #[test]
    fn a_species_inherits_down_its_chain() {
        let files = sapindaceae();
        let chain = above(&files, "sapindaceae", "acer").unwrap();
        let names: Vec<&str> = chain.iter().map(|rank| rank.file.as_str()).collect();
        assert_eq!(
            names,
            [
                "sapindaceae/acer/genus.json",
                "sapindaceae/family.json",
                "_ranks/order/sapindales.json",
                "_ranks/clade/eudicots.json"
            ]
        );
        let file = "sapindaceae/acer/acer-test/spec.json";
        let own = json!({"id": "acer-test", "taxon": {"x": 1},
            "generator": {"params": {"d": 4.0}},
            "evidence": {"generator.params.d": {"evidence": "Authored", "note": "Own."}}});
        let inherited = inherit(file, &own, &chain, &programs()).unwrap();
        assert_eq!(
            inherited.spec,
            json!({
                "id": "acer-test",
                "taxon": {"x": 1},
                "tier": "procedural_proxy",
                "growth_form": "decurrent_tree",
                "generator": {"program": "sapindaceae", "params": {"b": 3.0, "c": 1.0, "d": 4.0}},
                "evidence": {
                    "generator.params": {"evidence": "Authored", "note": "Clade."},
                    "generator.params.c": {"evidence": "Authored", "note": "Order."},
                    "generator.params.d": {"evidence": "Authored", "note": "Own."}
                }
            })
        );
        let origin = |path: &str| inherited.origins[path].as_str();
        assert_eq!(origin("tier"), "_ranks/clade/eudicots.json");
        assert_eq!(origin("generator.params.c"), "_ranks/order/sapindales.json");
        assert_eq!(
            origin("generator.params.b"),
            "sapindaceae/family.json, forms.decurrent_tree"
        );
        assert_eq!(
            origin("generator.program"),
            "sapindaceae/family.json, forms.decurrent_tree"
        );
        assert_eq!(origin("growth_form"), "sapindaceae/acer/genus.json");
        assert_eq!(origin("generator.params.d"), file);
        assert!(!inherited.origins.contains_key("generator.params.a"));
        // The clade's note on all the params stays beside the nearer
        // files' notes on the params they set; none of them replaces the
        // params whole.
        assert_eq!(
            inherited.notes,
            BTreeMap::from([
                (
                    "generator.params".to_string(),
                    "_ranks/clade/eudicots.json".to_string()
                ),
                (
                    "generator.params.c".to_string(),
                    "_ranks/order/sapindales.json".to_string()
                ),
                ("generator.params.d".to_string(), file.to_string()),
            ])
        );
        assert_eq!(inherited.chain.len(), 5);
        assert_eq!(inherited.chain[0], file);
    }

    /// A shrub of the same family takes the shrub's part; with no genus
    /// file, it stands on the family's.
    #[test]
    fn each_growth_form_takes_its_own_part() {
        let files = sapindaceae();
        let shrub = json!({"id": "dodonaea-test", "growth_form": "shrub"});
        let chain = above(&files, "sapindaceae", "dodonaea").unwrap();
        assert_eq!(chain[0].file, "sapindaceae/family.json");
        let inherited = inherit(
            "sapindaceae/dodonaea/dodonaea-test/spec.json",
            &shrub,
            &chain,
            &programs(),
        )
        .unwrap();
        assert_eq!(inherited.spec["generator"]["program"], "shrub");
        assert_eq!(
            inherited.spec["generator"]["params"],
            json!({"a": 1.0, "b": 2.0, "c": 1.0})
        );
    }

    /// A species with no rank file above it is its own text; one under
    /// a family file that sets nothing keeps its values.
    #[test]
    fn a_species_without_rank_files_is_its_own_spec() {
        let own = r#"{"id": "testa-one", "generator": {"params": {"x": 0.1}},
            "evidence": {"generator": {"evidence": "Authored", "note": "Own."}}}"#;
        assert_eq!(
            effective_text("f/g/testa-one/spec.json", own, &[], &programs()),
            Ok(None)
        );
        let files = ranks(&[(
            "testaceae/family.json",
            json!({"schema": 1, "rank": "family", "name": "Testaceae"}),
        )]);
        let chain = above(&files, "testaceae", "testa").unwrap();
        let text = effective_text("f/g/testa-one/spec.json", own, &chain, &programs())
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::from_str::<Value>(own).unwrap()
        );
        // A number that would not read back as merged is refused, not
        // changed.
        let long = r#"{"id": "testa-one", "generator": {"params": {"x": 0.99634467830471407}}}"#;
        let error =
            effective_text("f/g/testa-one/spec.json", long, &chain, &programs()).unwrap_err();
        assert!(
            error.contains("`generator.params.x` does not read back"),
            "{error}"
        );
    }

    /// `file` holding `value` is refused, saying `wanted`.
    fn refused(file: &str, value: &Value, wanted: &str) {
        let error = RankFile::parse(file, &value.to_string()).unwrap_err();
        assert!(error.contains(wanted), "{file}: {error}");
    }

    /// A family file for Testaceae with `extra`'s keys.
    fn family(extra: Value) -> Value {
        let mut value = json!({"schema": 1, "rank": "family", "name": "Testaceae"});
        if let Value::Object(extra) = extra {
            for (key, item) in extra {
                value[key] = item;
            }
        }
        value
    }

    #[test]
    fn rank_files_against_the_rules_are_refused() {
        refused(
            "testaceae/x.json",
            &family(json!({})),
            "not where a rank file goes",
        );
        refused(
            "_ranks/family/testaceae.json",
            &family(json!({})),
            "not a rank kept",
        );
        refused(
            "_ranks/order/Test.json",
            &family(json!({})),
            "named by its taxon",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"schema": 2})),
            "`schema` must be 1",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rank": "order"})),
            "it says `rank` order",
        );
        refused(
            "otheraceae/family.json",
            &family(json!({})),
            "would be named `testaceae`",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"parent": "orders"})),
            "is not `<rank>/<name>`",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"parent": "genus/testa"})),
            "is not `<rank>/<name>`",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"id": "testaceae"})),
            "holds no `id`",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"forms": {"shrub": {"growth_form": "shrub"}}})),
            "`forms.shrub.growth_form` belongs to the whole file",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"forms": {"shrub": {"taxon": {}}}})),
            "holds no `forms.shrub.taxon`",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"tier": "calibrated",
                "evidence": {"generator": {"evidence": "Authored", "note": "Nothing."}}})),
            "the evidence note on `generator` in the file is on a value it does not set",
        );
        refused(
            "_ranks/tribe/testeae.json",
            &json!({"schema": 1, "rank": "tribe", "name": "Testeae"}),
            "a tribe names its `parent`",
        );
        assert_eq!(file_name("core eudicots"), "core-eudicots");
        RankFile::parse(
            "_ranks/clade/core-eudicots.json",
            &json!({"schema": 1, "rank": "clade", "name": "core eudicots"}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn rules_and_trait_notes_against_their_shape_are_refused() {
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"id": {"rule": "1"}}})),
            "the rule for `id`: no rule may set it",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {"rule": "uniform(0, 1)"}}})),
            "`uniform` is not a function",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {"map": {"x": 1}}}})),
            "a `map` names one trait",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {
                "map": {"x": {"y": {"rule": "uniform(0, 1)"}}}}}})),
            "`uniform` is not a function",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {"rule": "1", "map": {}}}})),
            "one of them",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"traits": {"clonal": true},
                "evidence": {"traits.leaf_length_m": {"evidence": "Authored", "note": "None."}}})),
            "the evidence note on `traits.leaf_length_m` in the file is on a value it does not set",
        );
        // A rule inside a look's shape names a card that has its field.
        let shape =
            |rule: Value| family(json!({"rules": {"appearance.organs.flower.shape.rays": rule}}));
        refused(
            "testaceae/family.json",
            &shape(json!({"rule": "5"})),
            "a rule inside a look's `shape` names the card `template` it is for",
        );
        refused(
            "testaceae/family.json",
            &shape(json!({"rule": "5", "template": "umbel"})),
            "`umbel` cards have no `rays`",
        );
        refused(
            "testaceae/family.json",
            &shape(json!({"rule": "5", "template": "daisy"})),
            "`daisy` is no card template",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {"rule": "1", "template": "head"}}})),
            "only a rule inside a look's `shape` names a card `template`",
        );
        // A list of cards names cards that each have the field.
        refused(
            "testaceae/family.json",
            &shape(json!({"rule": "5", "template": ["head", "umbel"]})),
            "`umbel` cards have no `rays`",
        );
        refused(
            "testaceae/family.json",
            &shape(json!({"rule": "5", "template": []})),
            "a rule's `template` is a card template, or a list of them",
        );
        // Only a map draws between the values of tied states.
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {"rule": "1", "between": true}}})),
            "only a `map` draws `between` the values of states that tie",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"rules": {"generator.params.a": {
                "map": {"x": {"y": 1}}, "between": false}}})),
            "`between` is `true`, or left out",
        );
    }

    #[test]
    fn chains_against_the_rules_are_refused() {
        let file = |rank: &str, name: &str, parent: Option<&str>| {
            let mut value = json!({"schema": 1, "rank": rank, "name": name});
            if let Some(parent) = parent {
                value["parent"] = parent.into();
            }
            value
        };
        let refused = |files: &[(&str, Value)], start: &str, wanted: &str| {
            let error = chain(&ranks(files), start).unwrap_err();
            assert!(error.contains(wanted), "{error}");
        };
        refused(
            &[(
                "testaceae/family.json",
                file("family", "Testaceae", Some("order/tests")),
            )],
            "testaceae/family.json",
            "its parent _ranks/order/tests.json is not in the library",
        );
        refused(
            &[
                ("_ranks/clade/a.json", file("clade", "a", Some("clade/b"))),
                ("_ranks/clade/b.json", file("clade", "b", Some("clade/a"))),
            ],
            "_ranks/clade/a.json",
            "come back round",
        );
        refused(
            &[
                ("_ranks/class/c.json", file("class", "c", Some("order/o"))),
                ("_ranks/order/o.json", file("order", "o", None)),
            ],
            "_ranks/class/c.json",
            "_ranks/class/c.json (class) cannot stand on _ranks/order/o.json (order)",
        );
        refused(
            &[
                ("_ranks/order/o.json", file("order", "o", Some("clade/k"))),
                ("_ranks/clade/k.json", file("clade", "k", Some("order/p"))),
                ("_ranks/order/p.json", file("order", "p", None)),
            ],
            "_ranks/order/o.json",
            "_ranks/clade/k.json (clade) cannot stand on _ranks/order/p.json (order)",
        );
        refused(
            &[
                ("_ranks/tribe/t.json", file("tribe", "t", Some("clade/k"))),
                ("_ranks/clade/k.json", file("clade", "k", None)),
            ],
            "_ranks/tribe/t.json",
            "_ranks/tribe/t.json (tribe) cannot stand on _ranks/clade/k.json (clade)",
        );
        refused(
            &[
                ("_ranks/tribe/t.json", file("tribe", "t", Some("order/o"))),
                ("_ranks/order/o.json", file("order", "o", None)),
            ],
            "_ranks/tribe/t.json",
            "never reach a family",
        );
        refused(
            &[
                (
                    "testaceae/testa/genus.json",
                    file("genus", "Testa", Some("tribe/t")),
                ),
                (
                    "_ranks/tribe/t.json",
                    file("tribe", "t", Some("family/otheraceae")),
                ),
                ("otheraceae/family.json", file("family", "Otheraceae", None)),
            ],
            "testaceae/testa/genus.json",
            "not its own family's file",
        );
        // A genus in a tribe of its family reaches the family through it;
        // a genus naming no parent stands on its family's file, if any.
        let files = ranks(&[
            (
                "testaceae/testa/genus.json",
                file("genus", "Testa", Some("tribe/t")),
            ),
            (
                "_ranks/tribe/t.json",
                file("tribe", "t", Some("family/testaceae")),
            ),
            (
                "testaceae/family.json",
                file("family", "Testaceae", Some("clade/k")),
            ),
            ("_ranks/clade/k.json", file("clade", "k", None)),
            ("testaceae/other/genus.json", file("genus", "Other", None)),
        ]);
        assert_eq!(
            chain(&files, "testaceae/testa/genus.json").unwrap().len(),
            4
        );
        assert_eq!(above(&files, "testaceae", "other").unwrap().len(), 3);
        assert_eq!(above(&files, "testaceae", "third").unwrap().len(), 2);
        assert!(above(&files, "nothingaceae", "third").unwrap().is_empty());
    }

    /// Rules on traits: Corner's rules at the seed plants, a family's
    /// leaf traits and a map from its leaf arrangement, a genus that sets
    /// the shoot spacing by hand and one that deletes the rule.
    fn corner() -> BTreeMap<String, RankFile> {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        ranks(&[
            (
                "_ranks/clade/spermatophyta.json",
                json!({"schema": 1, "rank": "clade", "name": "Spermatophyta",
                    "rules": {"generator.params.space_density": {
                        "rule": "clamp(8 * pow(0.15 / leaf_length_m, 0.75), 5, 60)",
                        "why": "Corner's rules"}},
                    "evidence": {"rules.generator.params.space_density": note("Corner.")}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/spermatophyta",
                    "traits": {"leaf_length_m": 0.15, "leaf_arrangement": "opposite"},
                    "evidence": {"traits.leaf_length_m": note("Family.")},
                    "generator": {"program": "broadleaf"},
                    "rules": {"generator.params.alternate": {
                        "map": {"leaf_arrangement": {"alternate": 1, "opposite": 0}}}}}),
            ),
            (
                "testaceae/manual/genus.json",
                json!({"schema": 1, "rank": "genus", "name": "Manual",
                    "generator": {"params": {"space_density": 20}}}),
            ),
            (
                "testaceae/unruled/genus.json",
                json!({"schema": 1, "rank": "genus", "name": "Unruled",
                    "rules": {"generator.params.space_density": null}}),
            ),
        ])
    }

    /// The species `<genus>-one` of Testaceae with its own spec `own`.
    fn grow(genus: &str, own: &Value) -> super::Inherited {
        let files = corner();
        let chain = above(&files, "testaceae", genus).unwrap();
        inherit(
            &format!("testaceae/{genus}/{genus}-one/spec.json"),
            own,
            &chain,
            &programs(),
        )
        .unwrap()
    }

    /// Corner's rules homed at a clade, read from a family's trait: each
    /// species below gets the rule's value unless a file at least as near
    /// set it by hand; a map turns an enum into a value.
    #[test]
    fn traits_set_values_through_rules_homed_in_taxa() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let clade = "_ranks/clade/spermatophyta.json";
        let family = "testaceae/family.json";

        // The family's leaf length through the clade's rule.
        let plain = grow("plain", &json!({"id": "plain-one"}));
        assert_eq!(plain.spec["generator"]["params"]["space_density"], 8.0);
        assert_eq!(plain.spec["generator"]["params"]["alternate"], 0);
        assert_eq!(
            plain.origins["generator.params.space_density"],
            format!("{clade}: rule on leaf_length_m from {family}")
        );
        assert_eq!(
            plain.rules["generator.params.space_density"],
            (clade.to_string(), None)
        );
        // The rule's note goes with its value; traits and rules stay out
        // of the spec and its notes.
        assert_eq!(
            plain.spec["evidence"],
            json!({"generator.params.space_density": note("Corner.")})
        );
        assert!(plain.spec.get("traits").is_none() && plain.spec.get("rules").is_none());
        assert_eq!(plain.notes["traits.leaf_length_m"], family);
        assert_eq!(
            plain.traits["leaf_arrangement"],
            Stated {
                value: json!("opposite"),
                file: family.to_string(),
                note: None
            }
        );
        assert_eq!(plain.traits["leaf_length_m"].note, Some(note("Family.")));

        // A genus that sets the value by hand is nearer than the trait.
        let manual = grow("manual", &json!({"id": "manual-one"}));
        assert_eq!(manual.spec["generator"]["params"]["space_density"], 20);
        assert_eq!(
            manual.rules["generator.params.space_density"].1.as_deref(),
            Some("set by hand in testaceae/manual/genus.json")
        );
        // A species' own leaf length is nearer than the genus's hand.
        let small = grow(
            "manual",
            &json!({"id": "manual-two", "traits": {"leaf_length_m": 0.05}}),
        );
        // 8 × 3^0.75, to 6 significant digits.
        assert_eq!(small.spec["generator"]["params"]["space_density"], 18.2361);
    }

    /// A rule sets a body's look (spec rework E6c): a list of formulas
    /// sets a range from a trait's bounds, a spine set is named by its
    /// role, a field written in the species' list is set by hand, and a
    /// look without ribs or a set of the role gets nothing.
    /// The angiosperms' rules set the days of each look's season from the
    /// months it flowers and its fruit is ripe in, reading the season's own
    /// opening and ripening, and share out the months it does both; a look
    /// with no season takes none.
    #[test]
    fn rules_set_a_seasons_days_from_its_months() {
        let angiosperms: Value =
            serde_json::from_str(include_str!("../library/_ranks/clade/angiosperms.json")).unwrap();
        let rules: serde_json::Map<String, Value> = angiosperms["rules"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.contains(".season."))
            .map(|(path, rule)| (path.clone(), rule.clone()))
            .collect();
        assert_eq!(rules.len(), 5);
        let files = ranks(&[(
            "testaceae/family.json",
            family(
                json!({"traits": {"reproduction": "flowers"}, "rules": rules,
                "evidence": {"rules": {"evidence": "Authored", "note": "Seasons."}}}),
            ),
        )]);
        let grow = |flowering: Value, fruiting: Value, season: Value| {
            let chain = above(&files, "testaceae", "testus").unwrap();
            inherit(
                "testaceae/testus/testus-one/spec.json",
                &json!({"id": "testus-one",
                    "traits": {"flowering_months": flowering, "fruiting_months": fruiting},
                    "appearance": {"organs": {
                        "bloom": {"shape": {"template": "fruit"}, "season": season},
                        "leaf": {"shape": {"template": "simple"}}}}}),
                &chain,
                &programs(),
            )
            .unwrap()
        };
        let days = |grown: &super::Inherited| {
            ["bud", "flower", "fruit", "ripe", "fall"]
                .map(|day| grown.spec["appearance"]["organs"]["bloom"]["season"][day].clone())
        };
        // Flowering in March and April, its units opening over 21 days;
        // ripe in July and August, ripening over 30.
        let spring = grow(
            json!([3, 4]),
            json!([7, 8]),
            json!({"opening": 21, "ripening": 30}),
        );
        assert_eq!(days(&spring), [45, 60, 100, 167, 244].map(|day| json!(day)));
        assert!(
            spring.spec["appearance"]["organs"]["leaf"]
                .get("season")
                .is_none()
        );
        // Flowering in November and December, ripe in February and March of
        // the next year, ripening over the default 20 days.
        let winter = grow(json!([11, 12]), json!([2, 3]), json!({}));
        assert_eq!(
            days(&winter),
            [290, 305, 366, 387, 456].map(|day| json!(day))
        );
        // Flowering from May to August, ripe from July to November: the
        // last flowers go to fruit on August 1, in the middle of July and
        // August, and the fruit ripens from then.
        let shared = grow(
            json!([5, 6, 7, 8]),
            json!([7, 8, 9, 10, 11]),
            json!({"ripening": 30}),
        );
        assert_eq!(
            days(&shared),
            [106, 121, 213, 213, 335].map(|day| json!(day))
        );
        // Flowering from January, its buds in the December before and its
        // year counted on from there.
        let hazel = grow(json!([1, 2, 3]), json!([8, 9, 10, 11]), json!({}));
        assert_eq!(
            days(&hazel),
            [351, 366, 456, 568, 700].map(|day| json!(day))
        );
        // With no season, the rules set nothing.
        let chain = above(&files, "testaceae", "testus").unwrap();
        let bare = inherit(
            "testaceae/testus/testus-one/spec.json",
            &json!({"id": "testus-one",
                "traits": {"flowering_months": [4], "fruiting_months": [6]},
                "appearance": {"organs": {"leaf": {"shape": {"template": "simple"}}}}}),
            &chain,
            &programs(),
        )
        .unwrap();
        assert_eq!(
            bare.rules["appearance.organs.*.season.flower"].1.as_deref(),
            Some("it draws no look with a `season`")
        );
    }

    #[test]
    fn rules_fill_in_bodies_by_role_from_bounds() {
        let files = ranks(&[(
            "cactaceae/family.json",
            json!({"schema": 1, "rank": "family", "name": "Cactaceae",
                "rules": {
                    "appearance.bodies.stem.ribs.count": {"rule": ["min(ribs)", "max(ribs)"]},
                    "appearance.bodies.stem.spines.radial.count":
                        {"rule": ["min(radial_spines)", "max(radial_spines)"]},
                    "appearance.bodies.stem.spines.radial.length":
                        {"rule": "max(radial_spine_length_m) / 1.25"},
                    "appearance.bodies.stem.spines.central.length":
                        {"rule": "max(radial_spine_length_m)"}},
                "evidence": {"rules": {"evidence": "Authored", "note": "Cacti."}}}),
        )]);
        let grow = |own: &Value| {
            let chain = above(&files, "cactaceae", "testcactus").unwrap();
            inherit(
                "cactaceae/testcactus/testcactus-one/spec.json",
                own,
                &chain,
                &programs(),
            )
            .unwrap()
        };
        let traits = json!({"ribs": {"min": 12, "max": 20},
            "radial_spines": {"min": 8, "max": 12, "typical": 10},
            "radial_spine_length_m": {"min": 0.01, "max": 0.02}});
        let ribbed = grow(&json!({"id": "testcactus-one", "traits": traits,
            "appearance": {"bodies": {"stem": {"ribs": {"spacing": 0.05},
                "spines": [{"role": "central", "count": [1, 1], "length": 0.03},
                           {"role": "radial", "angle": 10}]}}}}));
        let stem = &ribbed.spec["appearance"]["bodies"]["stem"];
        assert_eq!(stem["ribs"], json!({"spacing": 0.05, "count": [12, 20]}));
        assert_eq!(
            stem["spines"],
            json!([{"role": "central", "count": [1, 1], "length": 0.03},
                   {"role": "radial", "angle": 10, "count": [8, 12], "length": 0.016}])
        );
        assert_eq!(
            ribbed.rules["appearance.bodies.stem.spines.central.length"]
                .1
                .as_deref(),
            Some("set by hand in cactaceae/testcactus/testcactus-one/spec.json")
        );
        // No ribs and no radial set: those rules set nothing.
        let bare = grow(&json!({"id": "testcactus-one", "traits": traits,
            "appearance": {"bodies": {"stem": {"spines": [{"role": "glochid"}]}}}}));
        assert!(
            bare.spec["appearance"]["bodies"]["stem"]
                .get("ribs")
                .is_none()
        );
        assert_eq!(
            bare.rules["appearance.bodies.stem.ribs.count"].1.as_deref(),
            Some("its `stem` body has no `ribs`")
        );
        assert_eq!(
            bare.rules["appearance.bodies.stem.spines.radial.count"]
                .1
                .as_deref(),
            Some("its `stem` body has no `spines.radial`")
        );
        // A range without the bound a rule reads gives it nothing.
        let unbounded = grow(&json!({"id": "testcactus-one",
            "traits": {"ribs": {"typical": 14}},
            "appearance": {"bodies": {"stem": {"ribs": {}}}}}));
        assert_eq!(
            unbounded.rules["appearance.bodies.stem.ribs.count"]
                .1
                .as_deref(),
            Some("`ribs` states no `max`")
        );
    }

    /// A rule reads a range as its typical value, else its middle, and
    /// qualified states as the most usual one; states that tie give it
    /// nothing to read.
    #[test]
    fn rules_read_ranges_and_qualified_states() {
        let density = |own: &Value| {
            let grown = grow("plain", own);
            (
                grown.spec["generator"]["params"]["space_density"].clone(),
                grown.spec["generator"]["params"]["alternate"].clone(),
                grown.rules["generator.params.alternate"].1.clone(),
            )
        };
        // 0.1 to 0.2 m reads as 0.15 m, so the density is 8.
        let middle = density(&json!({"id": "plain-one", "traits": {
            "leaf_length_m": {"min": 0.1, "max": 0.2, "high": 0.5},
            "leaf_arrangement": {"opposite": "usually", "alternate": "rarely"}}}));
        assert_eq!(middle, (json!(8), json!(0), None));
        let typical = density(&json!({"id": "plain-two", "traits": {
            "leaf_length_m": {"min": 0.1, "max": 0.4, "typical": 0.15},
            "leaf_arrangement": {"alternate": "often", "opposite": "sometimes"}}}));
        assert_eq!(typical, (json!(8), json!(1), None));
        let tied = density(&json!({"id": "plain-three", "traits": {
            "leaf_arrangement": {"alternate": "often", "opposite": "often"}}}));
        assert_eq!(tied.1, Value::Null);
        assert_eq!(
            tied.2.as_deref(),
            Some(
                "`leaf_arrangement` has no one most usual state: alternate and opposite are each often"
            )
        );
        // A bool's states read as the bool.
        assert_eq!(
            super::reading("clonal", &json!({"true": "usually", "false": "rarely"})),
            Ok(json!(true))
        );
    }

    /// A deleted rule, a program without the parameter or a chain without
    /// the trait sets nothing; a species' own traits and rules resolve with
    /// no rank file above it.
    #[test]
    fn rules_set_nothing_they_cannot() {
        // A genus may delete a rule; a program without the parameter, or a
        // chain without the trait, sets nothing.
        let unruled = grow("unruled", &json!({"id": "unruled-one"}));
        assert!(!unruled.rules.contains_key("generator.params.space_density"));
        assert!(
            unruled.spec["generator"]["params"]
                .get("space_density")
                .is_none()
        );
        let shrub = grow(
            "plain",
            &json!({"id": "plain-two", "generator": {"program": "shrub"}}),
        );
        assert_eq!(
            shrub.rules["generator.params.space_density"].1.as_deref(),
            Some("program `shrub` has no parameter `space_density`")
        );
        let bare = grow(
            "plain",
            &json!({"id": "plain-three", "traits": {"leaf_length_m": null}}),
        );
        assert_eq!(
            bare.rules["generator.params.space_density"].1.as_deref(),
            Some("no `leaf_length_m` to read")
        );
        // A species' own traits and rules resolve with no rank file above.
        let alone = effective_text(
            "f/g/testa-one/spec.json",
            r#"{"generator": {"program": "broadleaf"}, "traits": {"leaflets": 5},
                "rules": {"generator.params.a": {"rule": "leaflets * 2"}}}"#,
            &[],
            &programs(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&alone).unwrap()["generator"]["params"]["a"],
            10.0
        );
    }

    /// A colour rule sets the first of its colour characters a species
    /// states, on the looks' 0 to 1 scale, as a rule or as a map's value,
    /// and its `scale` darkens it.
    #[test]
    fn rules_set_colours_from_colour_characters() {
        let family = "testaceae/family.json";
        let files = ranks(&[(
            family,
            json!({"schema": 1, "rank": "family", "name": "Testaceae",
                "generator": {"program": "broadleaf"},
                "rules": {
                    "appearance.organs.flower.colour": {
                        "map": {"capitulum_kind": {
                            "radiate": {"colour": ["ray_colour", "flower_colour"]}}},
                        "else": {"colour": "flower_colour"}},
                    "appearance.foliage_shade": {
                        "colour": "leaf_colour", "scale": [0.5, 0.5, 1]},
                    "appearance.bark": {"colour": "bark_colour"}}}),
        )]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let grown = |traits: Value| {
            let own = json!({"id": "plain-one", "traits": traits,
                "appearance": {"organs": {"flower": {"shape": {"template": "flower"}}}}});
            inherit(
                "testaceae/plain/plain-one/spec.json",
                &own,
                &chain,
                &programs(),
            )
            .unwrap()
        };
        let colour = |grown: &super::Inherited| {
            grown.spec["appearance"]["organs"]["flower"]["colour"].clone()
        };

        // A radiate head takes its rays' colour, and the foliage's shade
        // is the leaves' colour scaled; an unstated bark sets nothing.
        let rayed = grown(json!({"capitulum_kind": "radiate",
            "ray_colour": [255, 255, 255], "flower_colour": [255, 0, 0],
            "leaf_colour": [102, 204, 51]}));
        assert_eq!(colour(&rayed), json!([1, 1, 1]));
        assert_eq!(
            rayed.spec["appearance"]["foliage_shade"],
            json!([0.2, 0.4, 0.2])
        );
        assert!(rayed.spec["appearance"].get("bark").is_none());
        assert_eq!(
            rayed.rules["appearance.bark"].1.as_deref(),
            Some("no `bark_colour` to read")
        );
        // Without the rays' colour, the flower's; without a head kind, the
        // `else`'s.
        let unrayed = grown(json!({"capitulum_kind": "radiate", "flower_colour": [255, 0, 0]}));
        assert_eq!(colour(&unrayed), json!([1, 0, 0]));
        let plain = grown(json!({"flower_colour": [0, 0, 255]}));
        assert_eq!(colour(&plain), json!([0, 0, 1]));

        // A colour rule names colour characters, a `scale` three shares,
        // and only a colour rule has one.
        for (rule, wanted) in [
            (
                json!({"colour": []}),
                "a `colour` rule names a colour character",
            ),
            (
                json!({"colour": "leaf_colour", "scale": [2, 1, 1]}),
                "three numbers 0 to 1",
            ),
            (
                json!({"rule": "1", "scale": [1, 1, 1]}),
                "only a `colour` rule has a `scale`",
            ),
            (json!({"rule": "1", "colour": "leaf_colour"}), "one of them"),
        ] {
            let error = super::check_rule("appearance.foliage", &rule).unwrap_err();
            assert!(error.contains(wanted), "{rule}: {error}");
        }
    }

    /// A family's rules for its heads set a card's fields where the species
    /// draws the organ with that card: a head card's rays, an umbel card's
    /// clusters. A count takes a whole number, which a formula writes as
    /// one.
    #[test]
    fn rules_set_looks_their_templates_have() {
        let family = "testaceae/family.json";
        let files = ranks(&[(
            family,
            json!({"schema": 1, "rank": "family", "name": "Testaceae",
                "generator": {"program": "broadleaf"},
                "rules": {
                    "appearance.organs.flower.shape.rays": {
                        "template": "head", "rule": "round(ray_florets)"},
                    "appearance.organs.flower.shape.disc": {"template": "head",
                        "rule": "involucre_diameter_m / (involucre_diameter_m + 2 * ray_length_m)"},
                    "appearance.organs.flower.shape.clusters": {
                        "template": "umbel", "rule": "round(units_per_inflorescence)"},
                    "appearance.organs.flower.colour": {
                        "map": {"capitulum_kind": {"radiate": [1, 1, 1]}}}}}),
        )]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let species = "testaceae/plain/plain-one/spec.json";
        let grown = |template: &str, traits: &Value| {
            let own = json!({"id": "plain-one", "traits": traits,
                "appearance": {"organs": {"flower": {"shape": {"template": template}}}}});
            inherit(species, &own, &chain, &programs()).unwrap()
        };
        let traits = json!({"ray_florets": {"min": 13, "max": 34},
            "involucre_diameter_m": 0.016, "ray_length_m": 0.016,
            "units_per_inflorescence": {"min": 10, "max": 100},
            "capitulum_kind": "radiate"});
        let outcome = |grown: &super::Inherited, field: &str| {
            grown.rules[&format!("appearance.organs.flower.shape.{field}")]
                .1
                .clone()
        };

        // A head card takes the rays, 13 to 34 read as 23.5 and rounded,
        // and its disc's share of the head; it has no clusters.
        let head = grown("head", &traits);
        let shape = &head.spec["appearance"]["organs"]["flower"]["shape"];
        assert_eq!(shape["rays"], json!(24));
        assert_eq!(shape["disc"], json!(0.333_333));
        assert!(shape.get("clusters").is_none());
        assert_eq!(
            outcome(&head, "clusters").as_deref(),
            Some("the rule is for `umbel` cards and its `flower` look is drawn with `head`")
        );
        assert_eq!(
            head.spec["appearance"]["organs"]["flower"]["colour"],
            json!([1, 1, 1])
        );
        // An umbel card takes the clusters and no rays.
        let umbel = grown("umbel", &traits);
        let shape = &umbel.spec["appearance"]["organs"]["flower"]["shape"];
        assert_eq!(shape["clusters"], json!(55));
        assert!(shape.get("rays").is_none());
        assert_eq!(
            outcome(&umbel, "rays").as_deref(),
            Some("the rule is for `head` cards and its `flower` look is drawn with `umbel`")
        );

        // A species that draws no flower sets none of it.
        let bare = inherit(
            species,
            &json!({"id": "plain-two", "traits": traits}),
            &chain,
            &programs(),
        )
        .unwrap();
        assert!(bare.spec.get("appearance").is_none());
        assert_eq!(
            outcome(&bare, "rays").as_deref(),
            Some("it draws no `flower` look")
        );
        assert_eq!(
            bare.rules["appearance.organs.flower.colour"].1.as_deref(),
            Some("it draws no `flower` look")
        );

        // A count takes no fraction, and a look set by hand stands.
        let halved = inherit(
            species,
            &json!({"id": "plain-three", "traits": traits,
                "appearance": {"organs": {"flower": {"shape": {"template": "head", "disc": 0.3}}}},
                "rules": {"appearance.organs.flower.shape.rays": {
                    "template": "head", "rule": "ray_florets / 2"}}}),
            &chain,
            &programs(),
        )
        .unwrap();
        assert_eq!(
            outcome(&halved, "rays").as_deref(),
            Some("it gives 11.75, and the look takes a whole number, 0 or more")
        );
        assert_eq!(
            outcome(&halved, "disc").as_deref(),
            Some(format!("set by hand in {species}").as_str())
        );
        assert_eq!(
            halved.spec["appearance"]["organs"]["flower"]["shape"]["disc"],
            json!(0.3)
        );

        // Whole numbers are written as whole numbers, the rest as decimals.
        assert_eq!(super::written(24.0), Some(json!(24)));
        assert_eq!(super::written(-0.0), Some(json!(0)));
        assert_eq!(super::written(0.5), Some(json!(0.5)));
        assert_eq!(super::written(1e20), Some(json!(1e20)));
        assert_eq!(super::written(f64::INFINITY), None);
    }

    /// A rule whose path names `*` for the organ sets its path in every
    /// look it may; one inside a look's `inflorescence` sets it only where
    /// the look is a cluster.
    #[test]
    fn rules_fill_in_clusters_in_every_look() {
        let file = "testaceae/family.json";
        let files = ranks(&[(
            file,
            json!({"schema": 1, "rank": "family", "name": "Testaceae",
                "generator": {"program": "broadleaf"},
                "rules": {
                    "appearance.organs.*.inflorescence.kind": {
                        "map": {"inflorescence": {"raceme": "raceme", "spadix": "spike"}}},
                    "appearance.organs.*.inflorescence.flowers": {
                        "rule": "clamp(round(units_per_inflorescence), 1, 200)"},
                    "appearance.organs.*.shape.rays": {
                        "template": "head", "rule": "round(ray_florets)"}}}),
        )]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let species = "testaceae/plain/plain-one/spec.json";
        let grown = |looks: Value, traits: Value| {
            let own = json!({"id": "plain-one", "traits": traits,
                "appearance": {"organs": looks}});
            inherit(species, &own, &chain, &programs()).unwrap()
        };
        let outcome = |grown: &super::Inherited, path: &str| grown.rules[path].1.clone();
        let traits = json!({"inflorescence": "spadix",
            "units_per_inflorescence": {"min": 20, "max": 40}, "ray_florets": 8});

        // The bloom is a cluster and takes its kind and count; the flower,
        // a head card, takes its rays; the leaf takes neither.
        let both = grown(
            json!({"bloom": {"shape": {"template": "fruit"}, "inflorescence": {}},
                "flower": {"shape": {"template": "head"}},
                "leaf": {"shape": {"template": "simple"}}}),
            traits.clone(),
        );
        let looks = &both.spec["appearance"]["organs"];
        assert_eq!(
            looks["bloom"]["inflorescence"],
            json!({"kind": "spike", "flowers": 30})
        );
        assert!(looks["flower"].get("inflorescence").is_none());
        assert_eq!(looks["flower"]["shape"]["rays"], json!(8));
        assert_eq!(looks["leaf"], json!({"shape": {"template": "simple"}}));
        for path in [
            "appearance.organs.*.inflorescence.kind",
            "appearance.organs.*.inflorescence.flowers",
            "appearance.organs.*.shape.rays",
        ] {
            assert_eq!(outcome(&both, path), None, "{path}");
        }
        assert_eq!(
            both.origins["appearance.organs.bloom.inflorescence.kind"],
            format!("{file}: rule on inflorescence from {species}")
        );

        // Without a cluster or a head card, they set nothing, and say why.
        let neither = grown(
            json!({"leaf": {"shape": {"template": "simple"}}}),
            traits.clone(),
        );
        assert_eq!(
            outcome(&neither, "appearance.organs.*.inflorescence.kind").as_deref(),
            Some("it draws no look as a cluster (with an `inflorescence`)")
        );
        assert_eq!(
            outcome(&neither, "appearance.organs.*.shape.rays").as_deref(),
            Some("it draws no look with `head` cards")
        );

        // A kind set by hand stands; a state the map leaves out sets none.
        let by_hand = grown(
            json!({"bloom": {"shape": {"template": "fruit"},
                "inflorescence": {"kind": "umbel"}}}),
            traits.clone(),
        );
        assert_eq!(
            by_hand.spec["appearance"]["organs"]["bloom"]["inflorescence"],
            json!({"kind": "umbel", "flowers": 30})
        );
        assert_eq!(
            outcome(&by_hand, "appearance.organs.*.inflorescence.kind").as_deref(),
            Some(format!("in its `bloom` look, set by hand in {species}").as_str())
        );
        let solitary = grown(
            json!({"bloom": {"shape": {"template": "fruit"}, "inflorescence": {}}}),
            json!({"inflorescence": "solitary", "units_per_inflorescence": 1}),
        );
        assert_eq!(
            outcome(&solitary, "appearance.organs.*.inflorescence.kind").as_deref(),
            Some("in its `bloom` look, its map holds nothing for `inflorescence` solitary")
        );
    }

    /// A rule on the fruit card sets only the looks that show fruit: an
    /// organ with a season (its own shape is its fruit) or with none that
    /// is not the plant's buds, and none of a season's stage looks.
    #[test]
    fn a_rule_on_the_fruit_card_sets_only_fruit() {
        let file = "testaceae/family.json";
        let files = ranks(&[(
            file,
            json!({"schema": 1, "rank": "family", "name": "Testaceae",
                "generator": {"program": "broadleaf"},
                "rules": {"appearance.organs.*.shape.kind": {
                    "template": "fruit", "map": {"fruit_type": {"capsule": "capsule"}}}}}),
        )]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let species = "testaceae/plain/plain-one/spec.json";
        let own = json!({"id": "plain-one", "traits": {"fruit_type": "capsule"},
            "appearance": {"organs": {
                "flower": {"shape": {"template": "fruit"}, "season": {
                    "bud_look": {"shape": {"template": "fruit"}},
                    "flower_look": {"shape": {"template": "flower"}}}},
                "bud": {"shape": {"template": "fruit"}},
                "fruit": {"shape": {"template": "fruit"}}}}});
        let grown = inherit(species, &own, &chain, &programs()).unwrap();
        let looks = &grown.spec["appearance"]["organs"];
        assert_eq!(looks["flower"]["shape"]["kind"], json!("capsule"));
        assert_eq!(looks["fruit"]["shape"]["kind"], json!("capsule"));
        assert!(
            looks["flower"]["season"]["bud_look"]["shape"]
                .get("kind")
                .is_none()
        );
        assert!(looks["bud"]["shape"].get("kind").is_none());
    }

    /// A rule inside one look's `inflorescence` waits for that look to be
    /// a cluster; `*` names only the organ, and a rule sets only an
    /// inflorescence's own keys.
    #[test]
    fn a_rule_sets_an_inflorescence_only_in_a_cluster() {
        let file = "testaceae/family.json";
        let species = "testaceae/plain/plain-one/spec.json";
        let traits = json!({"inflorescence": "spadix",
            "units_per_inflorescence": {"min": 20, "max": 40}, "ray_florets": 8});
        let lone_files = ranks(&[(
            file,
            json!({"schema": 1, "rank": "family", "name": "Testaceae",
                "generator": {"program": "broadleaf"},
                "rules": {"appearance.organs.flower.inflorescence.flowers": {"rule": "5"}}}),
        )]);
        let lone = inherit(
            species,
            &json!({"id": "plain-one", "traits": traits,
                "appearance": {"organs": {"flower": {"shape": {"template": "head"}}}}}),
            &above(&lone_files, "testaceae", "plain").unwrap(),
            &programs(),
        )
        .unwrap();
        assert_eq!(
            lone.rules["appearance.organs.flower.inflorescence.flowers"]
                .1
                .as_deref(),
            Some("its `flower` look is no cluster: it has no `inflorescence`")
        );

        // `*` stands only for the organ; a rule sets an inflorescence's
        // keys, not its branch or colour.
        refused(
            file,
            &family(json!({"rules": {"appearance.*.flower.colour": {"rule": "1"}}})),
            "`*` stands only for the organ",
        );
        refused(
            file,
            &family(
                json!({"rules": {"appearance.organs.*.inflorescence.stalk": {
                "rule": "1"}}}),
            ),
            "a rule sets no `stalk` in an inflorescence",
        );
    }

    fn narrow() -> Value {
        json!([[0, 0], [0.5, 1], [1, 0]])
    }

    fn broad() -> Value {
        json!([[0, 1], [0.5, 1], [1, 0]])
    }

    /// The species `<genus>-one` of a family whose rules give a simple or
    /// sprig leaf an outline and a width from its `leaf_shape`, drawing
    /// between them where states tie, with its leaf drawn with `card` and
    /// its `leaf_shape` `shape`; the genus Handed sets the leaf's `widest`
    /// and `tip` by hand.
    fn shaped(genus: &str, card: &Value, shape: &Value) -> super::Inherited {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let files = ranks(&[
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "generator": {"program": "broadleaf"},
                    "rules": {
                        "appearance.organs.leaf.shape.outline": {
                            "template": ["simple", "sprig"], "between": true,
                            "map": {"leaf_shape": {"narrow": narrow(), "broad": broad(), "odd": "x"}}},
                        "appearance.organs.leaf.shape.width": {
                            "template": ["simple", "sprig"], "between": true,
                            "map": {"leaf_shape": {"narrow": 0.2, "broad": 0.5}}}},
                    "evidence": {"rules": note("Shapes.")}}),
            ),
            (
                "testaceae/handed/genus.json",
                json!({"schema": 1, "rank": "genus", "name": "Handed",
                    "appearance": {"organs": {"leaf": {"shape": {
                        "template": "simple", "widest": 0.3, "tip": 0.2}}}},
                    "evidence": {"appearance.organs.leaf.shape.widest": note("Widest."),
                        "appearance.organs.leaf.shape.tip": note("Tip.")}}),
            ),
        ]);
        let chain = above(&files, "testaceae", genus).unwrap();
        let own = json!({"id": format!("{genus}-one"), "traits": {"leaf_shape": shape},
            "appearance": {"organs": {"leaf": {"shape": card}}}});
        inherit(
            &format!("testaceae/{genus}/{genus}-one/spec.json"),
            &own,
            &chain,
            &programs(),
        )
        .unwrap()
    }

    /// What the rule on the leaf's `field` did: `None` where it set it.
    fn leaf_outcome(grown: &super::Inherited, field: &str) -> Option<String> {
        grown.rules[&format!("appearance.organs.leaf.shape.{field}")]
            .1
            .clone()
    }

    /// A rule for several cards sets an outline on any of them; where the
    /// states it reads tie, it draws between their values.
    #[test]
    fn rules_draw_outlines_and_between_tied_words() {
        let simple = json!({"template": "simple"});
        let leaf = |grown: &super::Inherited| grown.spec["appearance"]["organs"]["leaf"].clone();

        // One word: its outline and width, on a simple card or a sprig.
        let narrowed = shaped("plain", &simple, &json!("narrow"));
        assert_eq!(leaf(&narrowed)["shape"]["outline"], narrow());
        assert_eq!(leaf(&narrowed)["shape"]["width"], json!(0.2));
        let sprig = shaped("plain", &json!({"template": "sprig"}), &json!("broad"));
        assert_eq!(leaf(&sprig)["shape"]["outline"], broad());
        let compound = shaped("plain", &json!({"template": "compound"}), &json!("broad"));
        assert_eq!(
            leaf_outcome(&compound, "outline").as_deref(),
            Some(
                "the rule is for `simple` or `sprig` cards and its `leaf` look is drawn with `compound`"
            )
        );

        // Two words that tie: between their outlines and their widths.
        let tied = json!({"narrow": "often", "broad": "often"});
        let between = shaped("plain", &simple, &tied);
        assert_eq!(
            leaf(&between)["shape"]["outline"],
            json!([[0, 0.5], [0.5, 1], [1, 0]])
        );
        assert_eq!(leaf(&between)["shape"]["width"], json!(0.35));
        // A word the map does not hold, or values with nothing between
        // them, set nothing.
        let unknown = shaped(
            "plain",
            &simple,
            &json!({"narrow": "often", "round": "often"}),
        );
        assert_eq!(
            leaf_outcome(&unknown, "outline").as_deref(),
            Some("its map holds nothing for `leaf_shape` round")
        );
        let odd = shaped(
            "plain",
            &simple,
            &json!({"narrow": "often", "odd": "often"}),
        );
        assert_eq!(
            leaf_outcome(&odd, "outline").as_deref(),
            Some("its values for `leaf_shape` narrow and odd have nothing between them")
        );
        assert_eq!(super::between(&[json!([1, 2]), json!([1])]), None);
        assert_eq!(super::between(&[json!("a"), json!("b")]), None);
        // Words that agree give their word.
        assert_eq!(
            super::between(&[json!("discoid"), json!("discoid")]),
            Some(json!("discoid"))
        );
    }

    /// An outline takes the place of the `widest`, `tip` and `base` set
    /// farther off than the word it reads, while those set at least as
    /// near keep it out.
    #[test]
    fn outlines_replace_the_blade_s_shape_set_farther_off() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let leaf = |grown: &super::Inherited| grown.spec["appearance"]["organs"]["leaf"].clone();
        // A genus's `widest` and `tip` give way to the species' word: the
        // outline replaces them, and their notes go with them.
        let replaced = shaped("handed", &json!({}), &json!("narrow"));
        let shape = &leaf(&replaced)["shape"];
        assert_eq!(shape["outline"], narrow());
        assert!(shape.get("widest").is_none() && shape.get("tip").is_none());
        assert!(
            !replaced
                .origins
                .contains_key("appearance.organs.leaf.shape.widest")
        );
        assert_eq!(
            replaced.spec["evidence"],
            json!({"appearance.organs.leaf.shape.outline": note("Shapes."),
                "appearance.organs.leaf.shape.width": note("Shapes.")})
        );
        // The species' own `widest` stands, and keeps the outline out.
        let kept = shaped(
            "plain",
            &json!({"template": "simple", "widest": 0.6}),
            &json!("narrow"),
        );
        assert!(leaf(&kept)["shape"].get("outline").is_none());
        assert_eq!(leaf(&kept)["shape"]["widest"], json!(0.6));
        assert_eq!(
            leaf_outcome(&kept, "outline").as_deref(),
            Some("its card's `widest` is set by hand in testaceae/plain/plain-one/spec.json")
        );
        assert_eq!(leaf(&kept)["shape"]["width"], json!(0.2));
    }

    /// A program's parameters are its own and those of the programs it
    /// extends, with its own defaults where it gives them again; a default
    /// that is not a number is none.
    #[test]
    fn programs_inherit_their_parents_parameters() {
        let programs = programs();
        assert_eq!(programs["shrub"].keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(programs["sapindaceae"]["fork"], Some(1.0));
        assert_eq!(programs["sapindaceae"]["space_density"], Some(5.0));
        assert_eq!(programs["sapindaceae"]["d"], Some(3.0));
        assert_eq!(programs["broadleaf"]["d"], Some(1.0));
        assert_eq!(programs["sapindaceae"]["e"], None);
        assert!(!programs["broadleaf"].contains_key("fork"));
    }

    /// A rule may read the species' program parameters: as a file set
    /// them, as a rule before it set them, or as the program's default.
    /// It is as near as the nearest file it reads; rules that read each
    /// other in a loop set nothing.
    #[test]
    fn rules_read_parameters() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let clade = "_ranks/clade/spermatophyta.json";
        let files = ranks(&[
            (
                clade,
                json!({"schema": 1, "rank": "clade", "name": "Spermatophyta",
                    "rules": {
                        "generator.params.space_density": {"rule": "1 / pow(b, 3)"},
                        "generator.params.b": {"rule": "2 * a"},
                        "generator.params.c": {"rule": "d + 1"},
                        "generator.params.alternate": {"rule": "d"}},
                    "evidence": {"rules": note("Rules.")}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/spermatophyta",
                    "generator": {"program": "broadleaf", "params": {"a": 0.25, "alternate": 1}}}),
            ),
        ]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let species = "testaceae/plain/plain-one/spec.json";
        let grown = |own: &Value| inherit(species, own, &chain, &programs()).unwrap();

        let plain = grown(&json!({"id": "plain-one"}));
        let params = &plain.spec["generator"]["params"];
        // `b` from the family's `a`, then the density from `b`.
        assert_eq!(params["b"], 0.5);
        assert_eq!(params["space_density"], 8.0);
        assert_eq!(
            plain.origins["generator.params.space_density"],
            format!("{clade}: rule on b from {clade}: rule on a from testaceae/family.json")
        );
        // `c` from the program's default `d`.
        assert_eq!(params["c"], 2.0);
        assert_eq!(
            plain.origins["generator.params.c"],
            format!("{clade}: rule on d from program `broadleaf`")
        );
        // The family's `alternate` is as near as the rule, which reads only
        // the program, so the hand-set value stands.
        assert_eq!(params["alternate"], 1);
        // The clade notes all of its rules at once, and each value they
        // set carries that note.
        assert_eq!(
            plain.spec["evidence"],
            json!({"generator.params.b": note("Rules."),
                "generator.params.c": note("Rules."),
                "generator.params.space_density": note("Rules.")})
        );

        // A species' own `a` is nearer than anything above it.
        let own = grown(&json!({"id": "plain-two",
            "generator": {"params": {"a": 1, "space_density": 3}}}));
        assert_eq!(own.spec["generator"]["params"]["b"], 2.0);
        assert_eq!(
            own.rules["generator.params.space_density"].1.as_deref(),
            Some(format!("set by hand in {species}").as_str())
        );
        // A program's own default.
        let sapindaceae = grown(&json!({"id": "plain-three",
            "generator": {"program": "sapindaceae"}}));
        assert_eq!(sapindaceae.spec["generator"]["params"]["c"], 4.0);

        // Rules that read each other in a loop set nothing.
        let looped = grown(&json!({"id": "plain-four", "rules": {
            "generator.params.a": {"rule": "space_density"}}}));
        assert_eq!(
            looped.rules["generator.params.a"].1.as_deref(),
            Some("it waits on rules that read each other: generator.params.space_density")
        );
        assert_eq!(
            looped.rules["generator.params.b"].1.as_deref(),
            Some("it waits on rules that read each other: generator.params.a")
        );
        assert_eq!(looped.spec["generator"]["params"]["a"], 0.25);
        assert!(looped.spec["generator"]["params"].get("b").is_none());
        // A parameter no file sets and the program gives no number for.
        let unread = grown(
            &json!({"id": "plain-five", "generator": {"program": "sapindaceae"},
            "rules": {"generator.params.fork": {"rule": "e"}}}),
        );
        assert_eq!(
            unread.rules["generator.params.fork"].1.as_deref(),
            Some("no `e` to read")
        );
    }

    /// A map's value may be a formula, worked out only when the map picks
    /// it: whorled leaves take the taxon's count of them, and a species
    /// with alternate leaves needs no count. A rule whose formula reads a
    /// parameter another rule sets runs after it.
    #[test]
    fn a_map_value_may_be_a_formula_read_only_when_picked() {
        let phylum = "_ranks/phylum/tracheophyta.json";
        let files = ranks(&[
            (
                phylum,
                json!({"schema": 1, "rank": "phylum", "name": "Tracheophyta",
                    "rules": {
                        "generator.params.leaves_per_node": {"map": {"leaf_arrangement": {
                            "alternate": 1, "opposite": 2,
                            "whorled": {"rule": "leaves_per_whorl"}}}},
                        "generator.params.divergence": {"map": {"phyllotaxis_pattern": {
                            "spiral": 137.5, "distichous": 180,
                            "decussate": {"rule": "180 / leaves_per_node"}}}}}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "phylum/tracheophyta", "generator": {"program": "herb"}}),
            ),
        ]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let species = "testaceae/plain/plain-one/spec.json";
        let grown = |traits: Value| {
            inherit(
                species,
                &json!({"id": "plain-one", "traits": traits}),
                &chain,
                &programs(),
            )
            .unwrap()
        };
        let arrangement = |grown: &super::Inherited| {
            let params = &grown.spec["generator"]["params"];
            (
                params["leaves_per_node"].clone(),
                params["divergence"].clone(),
            )
        };

        // Whorls of six cross the whorl below.
        let whorled = grown(json!({"leaf_arrangement": "whorled", "leaves_per_whorl": 6,
            "phyllotaxis_pattern": "decussate"}));
        assert_eq!(arrangement(&whorled), (json!(6), json!(30)));
        assert_eq!(
            whorled.origins["generator.params.leaves_per_node"],
            format!(
                "{phylum}: rule on leaf_arrangement from {species}, leaves_per_whorl from {species}"
            )
        );
        // Pairs cross at a right angle; single leaves need no count.
        let opposite = grown(json!({"leaf_arrangement": "opposite",
            "phyllotaxis_pattern": "decussate"}));
        assert_eq!(arrangement(&opposite), (json!(2), json!(90)));
        let alternate = grown(json!({"leaf_arrangement": "alternate",
            "phyllotaxis_pattern": "distichous"}));
        assert_eq!(arrangement(&alternate), (json!(1), json!(180)));
        assert_eq!(alternate.rules["generator.params.leaves_per_node"].1, None);

        // Whorled leaves with no count to read set nothing.
        let uncounted = grown(json!({"leaf_arrangement": "whorled"}));
        assert_eq!(
            uncounted.rules["generator.params.leaves_per_node"]
                .1
                .as_deref(),
            Some("no `leaves_per_whorl` to read")
        );
        assert_eq!(
            uncounted.rules["generator.params.divergence"].1.as_deref(),
            Some("no `phyllotaxis_pattern` to read")
        );
    }

    #[test]
    fn a_map_on_a_count_reads_its_nearest_whole_number() {
        let files = ranks(&[
            (
                "_ranks/phylum/tracheophyta.json",
                json!({"schema": 1, "rank": "phylum", "name": "Tracheophyta"}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "phylum/tracheophyta", "generator": {"program": "herb"},
                    "rules": {"generator.params.leaves_per_node": {
                        "map": {"leaflets": {"1": 1, "2": 2, "3": 3}}, "else": 5}}}),
            ),
        ]);
        let chain = above(&files, "testaceae", "plain").unwrap();
        let per_node = |traits: Value| {
            inherit(
                "testaceae/plain/plain-one/spec.json",
                &json!({"id": "plain-one", "traits": traits}),
                &chain,
                &programs(),
            )
            .unwrap()
            .spec["generator"]["params"]["leaves_per_node"]
                .clone()
        };
        assert_eq!(per_node(json!({"leaflets": 1})), json!(1));
        // 2 to 3 reads at its middle, 2.5, the nearest whole number 3.
        assert_eq!(
            per_node(json!({"leaflets": {"min": 2, "max": 3}})),
            json!(3)
        );
        assert_eq!(
            per_node(json!({"leaflets": {"min": 1, "max": 3, "typical": 2}})),
            json!(2)
        );
        assert_eq!(per_node(json!({})), json!(5));
    }

    /// A clade's defaults for a growth form fill the parameters a program
    /// leaves valueless, and yield to a lineage program's own values and
    /// to any file's.
    #[test]
    fn defaults_fill_only_what_the_program_leaves_valueless() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let programs = program_params([
            (
                "open",
                "lsystem open 1;\nparam a;\nparam b;\nparam c;\nparam d = 4;\nparam e;\nparam f;",
            ),
            (
                "lineage",
                "lsystem lineage 1 extends open;\nparam b = 2;\nparam e = 5;",
            ),
        ]);
        let files = ranks(&[
            (
                "_ranks/clade/top.json",
                json!({"schema": 1, "rank": "clade", "name": "Top",
                    "forms": {"shrub": {
                        "generator": {"program": "open",
                            "defaults": {"a": 10.0, "b": 20.0, "c": 30.0, "d": 40.0}},
                        "rules": {"generator.defaults.e": {"rule": "h * 2"},
                            "generator.params.f": {"rule": "e + 1"}},
                        "evidence": {"generator": note("Shrubs."), "rules": note("Sizes.")}}}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/top"}),
            ),
            (
                "linaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Linaceae",
                    "parent": "clade/top", "generator": {"program": "lineage"},
                    "evidence": {"generator.program": note("Its own.")}}),
            ),
        ]);
        let grow = |family: &str, own: Value| {
            let chain = above(&files, family, "genus").unwrap();
            inherit(
                &format!("{family}/genus/one/spec.json"),
                &own,
                &chain,
                &programs,
            )
            .unwrap()
        };
        let open = grow(
            "testaceae",
            json!({"id": "one", "growth_form": "shrub", "traits": {"h": 2},
                "generator": {"params": {"c": 3.0}}}),
        );
        assert_eq!(
            open.spec["generator"],
            json!({"program": "open",
                "params": {"a": 10.0, "b": 20.0, "c": 3.0, "e": 4, "f": 5}})
        );
        assert_eq!(
            open.origins["generator.params.a"],
            "_ranks/clade/top.json, forms.shrub"
        );
        assert!(open.notes.contains_key("generator.params.a"));
        assert!(!open.notes.keys().any(|path| path.contains("defaults")));
        let lineage = grow(
            "linaceae",
            json!({"id": "one", "growth_form": "shrub", "traits": {"h": 2}}),
        );
        assert_eq!(
            lineage.spec["generator"],
            json!({"program": "lineage", "params": {"a": 10.0, "c": 30.0, "f": 6}})
        );
        let tree = grow("testaceae", json!({"id": "one", "growth_form": "tree"}));
        assert_eq!(tree.spec.get("generator"), None);
    }

    /// A form's rule sizing a part from the species' facts replaces the
    /// size a farther file states for species that state none, and a rule
    /// setting the parameter itself replaces any default, from any file.
    #[test]
    fn rules_replace_the_defaults_they_size() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let programs = program_params([("open", "lsystem open 1;\nparam a;\nparam b;")]);
        let files = ranks(&[
            (
                "_ranks/clade/far.json",
                json!({"schema": 1, "rank": "clade", "name": "Far",
                    "rules": {"generator.params.b": {"rule": "juvenile * 2"}},
                    "forms": {"forb": {"generator": {"defaults": {"a": 1.0}},
                        "evidence": {"generator": note("Far sizes.")}}},
                    "evidence": {"rules": note("Ages.")}}),
            ),
            (
                "_ranks/clade/near.json",
                json!({"schema": 1, "rank": "clade", "name": "Near", "parent": "clade/far",
                    "forms": {"forb": {
                        "generator": {"program": "open", "defaults": {"b": 7.0}},
                        "rules": {"generator.defaults.a": {"rule": "leaf * 3"}},
                        "evidence": {"generator": note("Forbs."), "rules": note("Sizes.")}}}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/near"}),
            ),
        ]);
        let grow = |own: Value| {
            let chain = above(&files, "testaceae", "genus").unwrap();
            inherit("testaceae/genus/one/spec.json", &own, &chain, &programs)
                .unwrap()
                .spec["generator"]["params"]
                .clone()
        };
        assert_eq!(
            grow(json!({"id": "one", "growth_form": "forb"})),
            json!({"a": 1.0, "b": 7.0})
        );
        assert_eq!(
            grow(json!({"id": "one", "growth_form": "forb",
                "traits": {"leaf": 2, "juvenile": 4}})),
            json!({"a": 6, "b": 8})
        );
        // A default a rule sets from a range carries its spread, and the
        // spread's note, into the parameter it fills.
        let chain = above(&files, "testaceae", "genus").unwrap();
        let own = json!({"id": "one", "growth_form": "forb",
            "traits": {"leaf": {"min": 1.0, "max": 3.0}}});
        let open = inherit("testaceae/genus/one/spec.json", &own, &chain, &programs).unwrap();
        let generator = &open.spec["generator"];
        assert_eq!(generator["params"]["a"], json!(6));
        assert!(
            generator["spreads"]["params"]["a"].is_array(),
            "{generator}"
        );
        assert!(
            generator["spreads"].get("defaults").is_none(),
            "{generator}"
        );
        assert!(
            open.notes.contains_key("generator.spreads.params.a"),
            "{:?}",
            open.notes.keys().collect::<Vec<_>>()
        );
    }

    /// A map with an `else` gives it where its trait is unstated and where
    /// the trait's states tie for the most usual, as a family that is often
    /// rosulate and often not; a state stated as usual picks its value.
    #[test]
    fn a_map_gives_its_else_where_states_tie() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let programs = program_params([("open", "lsystem open 1;\nparam basal;")]);
        let family = |name: &str, rosette: Value| {
            let mut file = json!({"schema": 1, "rank": "family", "name": name,
                "parent": "clade/top"});
            if !rosette.is_null() {
                file["traits"] = json!({"rosette": rosette});
            }
            (format!("{}/family.json", name.to_lowercase()), file)
        };
        let mut files = vec![(
            "_ranks/clade/top.json".to_string(),
            json!({"schema": 1, "rank": "clade", "name": "Top",
                "forms": {"forb": {
                    "generator": {"program": "open"},
                    "rules": {"generator.defaults.basal": {
                        "map": {"rosette": {"basal": 5, "none": 0}}, "else": 1}},
                    "evidence": {"generator": note("Forbs."), "rules": note("Rosettes.")}}}}),
        )];
        files.push(family(
            "Tiedaceae",
            json!({"basal": "often", "none": "often"}),
        ));
        files.push(family(
            "Rosulaceae",
            json!({"basal": "usually", "none": "rarely"}),
        ));
        files.push(family("Plainaceae", Value::Null));
        let files = ranks(
            &files
                .iter()
                .map(|(path, value)| (path.as_str(), value.clone()))
                .collect::<Vec<_>>(),
        );
        let basal = |family: &str| {
            let chain = above(&files, family, "genus").unwrap();
            let grown = inherit(
                &format!("{family}/genus/one/spec.json"),
                &json!({"id": "one", "growth_form": "forb"}),
                &chain,
                &programs,
            )
            .unwrap();
            grown.spec["generator"]["params"]["basal"].clone()
        };
        assert_eq!(basal("tiedaceae"), json!(1));
        assert_eq!(basal("rosulaceae"), json!(5));
        assert_eq!(basal("plainaceae"), json!(1));
    }

    /// A rule may choose an organ's card from characters, through maps
    /// on further characters with their own `else`: it fills a look that
    /// names no card, replaces a farther file's card and the fields that
    /// card alone takes, stands back for a nearer file's, and the rules on
    /// the card's fields wait for it.
    #[test]
    fn rules_choose_cards_from_characters() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let card = json!({"map": {"leaf_division": {
            "compound": "compound",
            "simple": {"map": {"leaf_incision": {
                "none": "simple",
                "lobed": {"map": {"division_pattern": {"palmate": "palmate", "pinnate": "lobed"}}}}},
                "else": "simple"}}}});
        let files = ranks(&[
            (
                "_ranks/clade/top.json",
                json!({"schema": 1, "rank": "clade", "name": "Top",
                    "generator": {"program": "broadleaf"},
                    "appearance": {"organs": {"leaf": {"shape": {"template": "simple", "tooth_depth": 0.1}}}},
                    "rules": {
                        "appearance.organs.leaf.shape.template": card,
                        "appearance.organs.leaf.shape.leaflets": {
                            "template": "compound", "rule": "leaflets"}},
                    "evidence": {"rules": note("Cards."),
                        "appearance": note("A plain leaf.")}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/top"}),
            ),
        ]);
        let grow = |own: Value| {
            let chain = above(&files, "testaceae", "genus").unwrap();
            inherit("testaceae/genus/one/spec.json", &own, &chain, &programs()).unwrap()
        };
        let shape =
            |grown: &super::Inherited| grown.spec["appearance"]["organs"]["leaf"]["shape"].clone();
        let compound = grow(json!({"id": "one",
            "traits": {"leaf_division": "compound", "leaflets": 7}}));
        assert_eq!(
            shape(&compound),
            json!({"template": "compound", "leaflets": 7})
        );
        let palmate = grow(json!({"id": "one", "traits": {"leaf_division": "simple",
            "leaf_incision": "lobed", "division_pattern": "palmate"}}));
        assert_eq!(shape(&palmate)["template"], "palmate");
        let unsaid = grow(json!({"id": "one", "traits": {"leaf_division": "simple"}}));
        assert_eq!(
            shape(&unsaid),
            json!({"template": "simple", "tooth_depth": 0.1})
        );
        let handed = grow(json!({"id": "one", "traits": {"leaf_division": "compound"},
            "appearance": {"organs": {"leaf": {"shape": {"template": "simple"}}}}}));
        assert_eq!(shape(&handed)["template"], "simple");
        assert!(
            handed.rules["appearance.organs.leaf.shape.template"]
                .1
                .as_deref()
                .is_some_and(|why| why.starts_with("set by hand"))
        );
        let bare = grow(json!({"id": "one"}));
        assert_eq!(shape(&bare)["template"], "simple");
    }

    /// A map's `else` may map another trait: a regular flower draws its
    /// petals alike in a family whose zygomorphic flowers are two-lipped,
    /// and a flower of unstated symmetry draws as its zygomorphy says.
    #[test]
    fn an_else_may_map_another_trait() {
        let note = |text: &str| json!({"evidence": "Authored", "note": text});
        let lips = json!({"map": {"zygomorphy": {"bilabiate": 2, "other": 1}}});
        let files = ranks(&[
            (
                "_ranks/clade/top.json",
                json!({"schema": 1, "rank": "clade", "name": "Top",
                    "generator": {"program": "broadleaf"},
                    "rules": {"generator.params.a": {
                        "map": {"flower_symmetry": {"actinomorphic": 0, "zygomorphic": lips}},
                        "else": lips}},
                    "evidence": {"rules": note("Lips.")}}),
            ),
            (
                "testaceae/family.json",
                json!({"schema": 1, "rank": "family", "name": "Testaceae",
                    "parent": "clade/top", "traits": {"zygomorphy": "bilabiate"},
                    "evidence": {"traits": note("Two lips.")}}),
            ),
        ]);
        let a = |traits: Value| {
            let chain = above(&files, "testaceae", "genus").unwrap();
            let own = json!({"id": "one", "traits": traits});
            inherit("testaceae/genus/one/spec.json", &own, &chain, &programs())
                .unwrap()
                .spec["generator"]["params"]["a"]
                .clone()
        };
        assert_eq!(a(json!({"flower_symmetry": "actinomorphic"})), json!(0));
        assert_eq!(a(json!({"flower_symmetry": "zygomorphic"})), json!(2));
        assert_eq!(a(json!({})), json!(2));
        assert_eq!(
            a(json!({"flower_symmetry": "zygomorphic", "zygomorphy": "other"})),
            json!(1)
        );
        assert!(
            super::check_rule(
                "generator.params.a",
                &json!({"map": {"x": {"y": 1}}, "else": {"map": {"z": {"w": 1}}, "why": "no"}})
            )
            .unwrap_err()
            .contains("not a map's")
        );
    }

    /// A rule choosing a card names its organ and maps to cards.
    #[test]
    fn card_rules_against_their_shape_are_refused() {
        let at = "appearance.organs.leaf.shape.template";
        let refused = |path: &str, rule: Value, wanted: &str| {
            let error = super::check_rule(path, &rule).unwrap_err();
            assert!(error.contains(wanted), "{error}");
        };
        refused(
            at,
            json!({"map": {"leaf_division": {"simple": "oak"}}}),
            "no card template",
        );
        refused(at, json!({"rule": "1"}), "is a `map`");
        refused(
            at,
            json!({"template": "simple", "map": {"x": {}}}),
            "names none",
        );
        refused(
            "appearance.organs.*.shape.template",
            json!({"map": {"x": {"a": "simple"}}}),
            "names its organ",
        );
        refused(
            "generator.params.a",
            json!({"rule": "1", "else": 2}),
            "only a `map`",
        );
        refused(
            at,
            json!({"map": {"leaf_division": {"simple": {"map": {"x": {"a": "simple"}}, "why": "no"}}}}),
            "not a map's",
        );
    }
}
