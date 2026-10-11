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
//! A trait stated as a range reads as its `typical` value, else the middle
//! of its `min` and `max`; one stated as qualified states reads as its most
//! usual state ([`FREQUENCIES`]), and a rule reading states that tie sets
//! nothing.
//! Every rule lives in the taxon it holds for, its home (growth plan G2,
//! Joshi's rule that rules belong to taxa). Traits and rules merge down
//! the chain like values; then each rule sets its path, after the rules
//! that set the parameters it reads, unless a file at least as near as
//! the rule and what it reads set the path by hand, or the species'
//! program has no such parameter. The effective spec carries the values,
//! not the traits or rules.
//!
//! Every file notes what it sets (per-value evidence): a note stays in the
//! effective spec until a nearer file replaces or deletes what it
//! describes, and a note on a subtree stays beside the nearer files' notes
//! on values inside it.
//!
//! The module reads nothing but JSON, so the build script compiles it too
//! and merges every built-in species' chain while compiling.

use std::collections::{BTreeMap, BTreeSet};

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
    /// (`{"rule": formula}` or `{"map": {trait: {value: spec value}}}`,
    /// with a `why`); `null` deletes an inherited one.
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
    if let Some(key) = rule
        .keys()
        .find(|key| !["rule", "map", "why"].contains(&key.as_str()))
    {
        return Err(format!(
            "`{key}` is not a rule's: `rule` or `map`, and `why`"
        ));
    }
    if !rule.get("why").is_none_or(Value::is_string) {
        return Err("`why` is a line of text".into());
    }
    match (rule.get("rule"), rule.get("map")) {
        (Some(Value::String(formula)), None) => Formula::parse(formula).map(|_| ()),
        (None, Some(Value::Object(map))) => match map.iter().next() {
            Some((_, Value::Object(_))) if map.len() == 1 => Ok(()),
            _ => Err("a `map` names one trait, then its values' spec values".into()),
        },
        _ => Err("a rule has a `rule` (a formula) or a `map`, not both".into()),
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
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
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

/// The parameters of each program, by name: its own and those of the
/// programs it extends.
pub type Programs = BTreeMap<String, Params>;

/// The parameters each program declares, with those of the programs it
/// extends, read from the programs' text (`name`, `source`): a `param`
/// statement at the start of a line names one and its default, and the
/// header `lsystem <name> <revision> extends <parent>;` names the parent,
/// whose defaults the program's own replace. The library's tests hold this
/// to the compiled programs.
#[must_use]
pub fn program_params<'a>(sources: impl IntoIterator<Item = (&'a str, &'a str)>) -> Programs {
    let mut own: BTreeMap<&str, (Params, Option<String>)> = BTreeMap::new();
    for (name, source) in sources {
        let mut params = Params::new();
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
        }
        own.insert(name, (params, parent));
    }
    own.keys()
        .map(|name| {
            let mut params = Params::new();
            let mut next = Some(*name);
            // As deep as `extends` chains may go, so a loop ends.
            for _ in 0..8 {
                let Some((own_params, parent)) = next.and_then(|at| own.get(at)) else {
                    break;
                };
                for (param, default) in own_params {
                    params.entry(param.clone()).or_insert(*default);
                }
                next = parent.as_deref();
            }
            ((*name).to_string(), params)
        })
        .collect()
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
/// it reads set the path by hand, or the path is a parameter the species'
/// program lacks. Rules that read each other's paths in a loop set
/// nothing.
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
            } else {
                traits.insert(key.clone(), (value.clone(), index));
                let note = part
                    .notes
                    .get(&format!("traits.{key}"))
                    .or_else(|| part.notes.get("traits"));
                trait_notes.insert(key.clone(), note.cloned());
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
        rules: outcomes,
    })
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
    path: &'a str,
    rule: &'a Map<String, Value>,
    /// The rule's layer in the chain.
    index: usize,
    layers: &'a [(String, &'a Part)],
    traits: &'a BTreeMap<String, (Value, usize)>,
}

/// A species' rules, by the path each sets: the rule and its layer.
type Rules = BTreeMap<String, (Map<String, Value>, usize)>;

impl Ruled<'_> {
    /// Run a species' rules in [`order`] on its merged `spec`, and return
    /// each one's file and, if it set nothing, why.
    fn run(
        rules: &Rules,
        layers: &[(String, &Part)],
        traits: &BTreeMap<String, (Value, usize)>,
        programs: &Programs,
        spec: &mut Map<String, Value>,
        origins: &mut BTreeMap<String, Origin>,
        notes: &mut BTreeMap<String, (Origin, Value)>,
    ) -> BTreeMap<String, (String, Option<String>)> {
        let program = spec
            .get("generator")
            .and_then(|generator| generator.get("program"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut outcomes = BTreeMap::new();
        for (path, waits) in order(rules) {
            let (rule, index) = &rules[path];
            let outcome = if waits.is_empty() {
                let one = Ruled {
                    path,
                    rule,
                    index: *index,
                    layers,
                    traits,
                };
                one.apply(program.as_deref(), programs, spec, origins, notes)
                    .err()
            } else {
                Some(format!(
                    "it waits on rules that read each other: {}",
                    waits.join(", ")
                ))
            };
            outcomes.insert(path.to_string(), (layers[*index].0.clone(), outcome));
        }
        outcomes
    }

    /// Set the rule's path from the traits and parameters it reads, or say
    /// why not.
    fn apply(
        &self,
        program: Option<&str>,
        programs: &Programs,
        spec: &mut Map<String, Value>,
        origins: &mut BTreeMap<String, Origin>,
        notes: &mut BTreeMap<String, (Origin, Value)>,
    ) -> Result<(), String> {
        let params = program.and_then(|program| programs.get(program));
        if let Some(param) = self.path.strip_prefix("generator.params.") {
            let program = program.ok_or("the species names no program")?;
            if !params.is_some_and(|params| params.contains_key(param)) {
                return Err(format!("program `{program}` has no parameter `{param}`"));
            }
        }
        let mut depth = self.index;
        let mut from = Vec::new();
        let mut inputs = BTreeMap::new();
        for key in reads(self.rule) {
            let path = format!("generator.params.{key}");
            let value = if let Some((value, at)) = self.traits.get(&key) {
                depth = depth.max(*at);
                from.push(format!("{key} from {}", self.layers[*at].0));
                reading(&key, value)?
            } else if let Some(value) = find(spec, &path) {
                // A parameter as the files, or a rule that ran before this
                // one, set it.
                if let Some((label, at)) = origins.get(&path) {
                    depth = depth.max(*at);
                    from.push(format!("{key} from {label}"));
                }
                value.clone()
            } else if let (Some(program), Some(Some(default))) =
                (program, params.and_then(|params| params.get(&key)))
            {
                from.push(format!("{key} from program `{program}`"));
                Value::from(*default)
            } else {
                return Err(format!("no `{key}` to read"));
            };
            inputs.insert(key, value);
        }
        if let Some((label, by)) = origins.get(self.path)
            && *by >= depth
        {
            return Err(format!("set by hand in {label}"));
        }
        let value = evaluate(self.rule, &inputs)?;
        set(spec, self.path, value);
        forget(origins, self.path);
        let file = &self.layers[self.index].0;
        let label = if from.is_empty() {
            format!("{file}: rule")
        } else {
            format!("{file}: rule on {}", from.join(", "))
        };
        let origin = (label, depth);
        origins.insert(self.path.to_string(), origin.clone());
        notes.retain(|path, _| !within(path, self.path));
        if let Some((_, note)) = notes.get(&format!("rules.{}", self.path)).cloned() {
            notes.insert(self.path.to_string(), (origin, note));
        }
        Ok(())
    }
}

/// The order rules run in, by path: a rule that reads a parameter another
/// rule sets runs after it. Rules caught in a loop of such reads come
/// last, each with the paths it waits on.
fn order(rules: &Rules) -> Vec<(&str, Vec<String>)> {
    let waits = |rule: &Map<String, Value>| -> Vec<String> {
        reads(rule)
            .into_iter()
            .map(|name| format!("generator.params.{name}"))
            .filter(|path| rules.contains_key(path))
            .collect()
    };
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut order = Vec::new();
    loop {
        let ready: Vec<&str> = rules
            .iter()
            .filter(|(path, (rule, _))| {
                !done.contains(path.as_str())
                    && waits(rule).iter().all(|wait| done.contains(wait.as_str()))
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
            let waiting = waits(rule)
                .into_iter()
                .filter(|wait| !done.contains(wait.as_str()))
                .collect();
            order.push((path.as_str(), waiting));
        }
    }
    order
}

/// The names a rule reads: its formula's (traits or parameters), or its
/// map's trait.
fn reads(rule: &Map<String, Value>) -> Vec<String> {
    match (rule.get("rule"), rule.get("map")) {
        (Some(Value::String(formula)), _) => Formula::parse(formula)
            .map(|formula| formula.names().into_iter().collect())
            .unwrap_or_default(),
        (_, Some(Value::Object(map))) => map.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// The one value a rule reads from a trait as stated: of a range, its
/// `typical`, else the middle of its `min` and `max`; of qualified states,
/// the most usual (a bool's as a bool); else the value itself.
fn reading(key: &str, value: &Value) -> Result<Value, String> {
    let Value::Object(stated) = value else {
        return Ok(value.clone());
    };
    if stated.values().all(Value::is_number) {
        let bound = |name: &str| stated.get(name).and_then(Value::as_f64);
        return bound("typical")
            .or_else(|| Some(f64::midpoint(bound("min")?, bound("max")?)))
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
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
    let most: Vec<&String> = stated
        .iter()
        .filter(|(_, frequency)| rank(frequency) == Some(best))
        .map(|(state, _)| state)
        .collect();
    match most.as_slice() {
        [state] => Ok(match state.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String((*state).clone()),
        }),
        _ => Err(format!(
            "`{key}` has no one most usual state: {} are each {}",
            most.iter()
                .map(|state| state.as_str())
                .collect::<Vec<_>>()
                .join(" and "),
            FREQUENCIES[best]
        )),
    }
}

/// A rule's value from what it reads, by name: its formula's number (a
/// bool trait reads as 1 or 0), or its map's value for the trait's.
fn evaluate(rule: &Map<String, Value>, inputs: &BTreeMap<String, Value>) -> Result<Value, String> {
    if let Some(Value::String(formula)) = rule.get("rule") {
        let number = |key: &str| {
            inputs
                .get(key)
                .and_then(|value| value.as_f64().or_else(|| value.as_bool().map(f64::from)))
        };
        return Formula::parse(formula)?
            .eval(&number)
            .map(plain)
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "what it reads gives it no number".to_string());
    }
    if let Some(Value::Object(map)) = rule.get("map")
        && let Some((key, Value::Object(table))) = map.iter().next()
    {
        let value = inputs
            .get(key)
            .ok_or_else(|| format!("no `{key}` to read"))?;
        let name = match value {
            Value::String(name) => name.clone(),
            other => other.to_string(),
        };
        return table
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("its map holds nothing for `{key}` {name}"));
    }
    Err("it is no rule".into())
}

/// A rule's number kept to 6 significant digits: as plain as a value set
/// by hand, and it reads back as written once merged.
fn plain(value: f64) -> f64 {
    format!("{value:.5e}").parse().unwrap_or(value)
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
            &family(json!({"rules": {"generator.params.a": {"rule": "1", "map": {}}}})),
            "not both",
        );
        refused(
            "testaceae/family.json",
            &family(json!({"traits": {"clonal": true},
                "evidence": {"traits.leaf_length_m": {"evidence": "Authored", "note": "None."}}})),
            "the evidence note on `traits.leaf_length_m` in the file is on a value it does not set",
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
        assert_eq!(middle, (json!(8.0), json!(0), None));
        let typical = density(&json!({"id": "plain-two", "traits": {
            "leaf_length_m": {"min": 0.1, "max": 0.4, "typical": 0.15},
            "leaf_arrangement": {"alternate": "often", "opposite": "sometimes"}}}));
        assert_eq!(typical, (json!(8.0), json!(1), None));
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
}
