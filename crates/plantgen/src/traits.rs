//! The trait vocabulary (growth plan G2.2 and the spec rework's M1): every
//! character a record, a rank file or a species' spec may state, from
//! `traits.json`, with its group, the organ it describes, its type and its
//! unit, values or range. Organs nest (a ray floret is part of a capitulum,
//! part of an inflorescence, part of a stem), and an organ with a `when`
//! exists only where the characters it names take the values it lists, so
//! a character applies only where its organ does: what a plant states
//! decides which of its characters are missing ([`Vocabulary::coverage`]).
//! A number or count may be stated as a range, an enum or bool as
//! qualified states. The vocabulary holds no rules: the rules that turn
//! traits into spec values live in the taxa they hold for
//! ([`crate::inherit`]).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::formula::Formula;
use crate::inherit::{FREQUENCIES, Part, Programs, RANGE};

/// Characters, each with the values that make an organ or a character
/// apply: it applies where any of them is stated in one of its values.
pub type When = BTreeMap<String, Vec<String>>;

/// One organ of the vocabulary.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Organ {
    /// The organ it is part of; `None` for the whole plant only.
    #[serde(default)]
    pub part_of: Option<String>,
    /// Where it exists, if not wherever the organ it is part of does.
    #[serde(default)]
    pub when: When,
    pub why: String,
}

/// One trait of the vocabulary.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trait {
    pub group: String,
    /// The organ it describes.
    pub organ: String,
    /// `number`, `count`, `bool`, `enum`, `enums`, `months`, `colour`,
    /// `text` or `ids`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub values: Option<Vec<String>>,
    #[serde(default)]
    pub range: Option<[f64; 2]>,
    /// Where it applies within its organ, if not everywhere.
    #[serde(default)]
    pub when: When,
    /// Whether many plants have nothing to state for it, so the audit
    /// never counts it missing.
    #[serde(default)]
    pub optional: bool,
    pub why: String,
}

/// The vocabulary: `traits.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub schema: u32,
    pub about: String,
    pub groups: Vec<String>,
    pub organs: BTreeMap<String, Organ>,
    pub traits: BTreeMap<String, Trait>,
}

const KINDS: [&str; 9] = [
    "number", "count", "bool", "enum", "enums", "months", "colour", "text", "ids",
];

/// Whether an organ or a character applies to a plant, by the characters
/// the plant states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    Yes,
    /// A character the plant states rules it out.
    No,
    /// It depends on a character the plant does not state.
    Undecided,
}

impl Applies {
    /// Whether both apply.
    #[must_use]
    pub const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Yes, Self::Yes) => Self::Yes,
            _ => Self::Undecided,
        }
    }
}

/// A plant's characters against the vocabulary, each list sorted by key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    /// The characters it states that apply, or may: what the audit counts
    /// as stated. Optional characters are left out.
    pub stated: Vec<String>,
    /// The characters that apply and it does not state.
    pub missing: Vec<String>,
    /// The characters it does not state that may apply, once the
    /// characters they depend on are stated.
    pub undecided: Vec<String>,
    /// The characters it states that another character it states rules
    /// out, which the library refuses.
    pub ruled_out: Vec<String>,
}

impl Coverage {
    /// The share of the characters that apply that it states, 0 to 1; 1
    /// when none apply.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn share(&self) -> f64 {
        let applying = self.stated.len() + self.missing.len();
        if applying == 0 {
            1.0
        } else {
            self.stated.len() as f64 / applying as f64
        }
    }
}

impl Trait {
    /// The states an enum or bool takes; none for other types.
    #[must_use]
    pub fn states(&self) -> Vec<String> {
        match self.kind.as_str() {
            "bool" => vec!["true".into(), "false".into()],
            _ => self.values.clone().unwrap_or_default(),
        }
    }

    /// Whether `value` is one plain value of the trait: a range or
    /// qualified states are not.
    fn fits(&self, value: &Value) -> bool {
        let in_range = |number: f64| {
            self.range
                .is_none_or(|[low, high]| (low..=high).contains(&number))
        };
        let listed = |name: &Value| {
            name.as_str()
                .is_some_and(|name| self.values.iter().flatten().any(|value| value == name))
        };
        match self.kind.as_str() {
            "number" => value.as_f64().is_some_and(in_range),
            "count" => value
                .as_u64()
                .and_then(|count| u32::try_from(count).ok())
                .is_some_and(|count| in_range(f64::from(count))),
            "bool" => value.is_boolean(),
            "enum" => listed(value),
            "enums" => value
                .as_array()
                .is_some_and(|items| !items.is_empty() && items.iter().all(listed)),
            "months" => value.as_array().is_some_and(|months| {
                !months.is_empty()
                    && months.iter().all(|month| {
                        month
                            .as_u64()
                            .is_some_and(|month| (1..=12).contains(&month))
                    })
            }),
            "colour" => value.as_array().is_some_and(|channels| {
                channels.len() == 3
                    && channels
                        .iter()
                        .all(|channel| channel.as_u64().is_some_and(|channel| channel <= 255))
            }),
            "text" => value.as_str().is_some_and(|text| !text.is_empty()),
            "ids" => value.as_array().is_some_and(|ids| {
                ids.iter().all(|id| {
                    id.as_str().is_some_and(|id| {
                        !id.is_empty()
                            && id.bytes().all(|byte| {
                                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                            })
                    })
                })
            }),
            _ => false,
        }
    }

    /// What one value of the trait is, in words.
    #[must_use]
    pub fn what(&self) -> String {
        match (self.kind.as_str(), &self.values, &self.range) {
            ("enum", Some(values), _) => format!("one of {}", values.join(", ")),
            ("enums", Some(values), _) => format!("a list of {}", values.join(", ")),
            (kind, _, Some([low, high])) => format!(
                "a {kind} from {low} to {high}{}",
                self.unit
                    .as_deref()
                    .map_or(String::new(), |unit| format!(" {unit}"))
            ),
            ("months", _, _) => "a list of months, 1 to 12".into(),
            ("colour", _, _) => "a colour, three numbers 0 to 255".into(),
            ("text", _, _) => "a line of text".into(),
            ("ids", _, _) => "a list of species ids".into(),
            (kind, _, _) => format!("a {kind}"),
        }
    }

    /// Checks a range of a number or count: a `min` and a `max`, and
    /// maybe the rare extremes `low` and `high` and the usual `typical`,
    /// each a value of the trait, in the order low, min, typical, max,
    /// high.
    fn check_range(&self, key: &str, range: &Map<String, Value>) -> Result<(), String> {
        if let Some(bound) = range.keys().find(|bound| !RANGE.contains(&bound.as_str())) {
            return Err(format!(
                "`{key}`: `{bound}` is not part of a range: `min` and `max`, and if known `low`, `high` and `typical`"
            ));
        }
        if !range.contains_key("min") || !range.contains_key("max") {
            return Err(format!("`{key}`: a range has a `min` and a `max`"));
        }
        for (bound, value) in range {
            if !self.fits(value) {
                return Err(format!(
                    "`{key}`: its `{bound}` is {}; {value} is not",
                    self.what()
                ));
            }
        }
        let mut last: Option<(&str, f64)> = None;
        for bound in ["low", "min", "typical", "max", "high"] {
            if let Some(value) = range.get(bound).and_then(Value::as_f64) {
                if let Some((before, previous)) = last
                    && value < previous
                {
                    return Err(format!(
                        "`{key}`: its `{bound}` {value} is below its `{before}` {previous}; a range runs low, min, typical, max, high"
                    ));
                }
                last = Some((bound, value));
            }
        }
        Ok(())
    }

    /// Checks qualified states of an enum or bool: at least one, each a
    /// state of the trait with how often it occurs, and at most one of
    /// them `usually`.
    fn check_qualified(&self, key: &str, qualified: &Map<String, Value>) -> Result<(), String> {
        if qualified.is_empty() {
            return Err(format!("`{key}`: qualified states name at least one state"));
        }
        let states = self.states();
        for (state, frequency) in qualified {
            if !states.contains(state) {
                return Err(format!(
                    "`{key}`: `{state}` is not one of its states, {}",
                    states.join(", ")
                ));
            }
            if !frequency
                .as_str()
                .is_some_and(|frequency| FREQUENCIES.contains(&frequency))
            {
                return Err(format!(
                    "`{key}`: how often it is {state} is one of {}; {frequency} is not",
                    FREQUENCIES.join(", ")
                ));
            }
        }
        let usually = qualified
            .values()
            .filter(|frequency| frequency.as_str() == Some(FREQUENCIES[0]))
            .count();
        if usually > 1 {
            return Err(format!(
                "`{key}`: at most one state is `{}`",
                FREQUENCIES[0]
            ));
        }
        Ok(())
    }
}

impl Vocabulary {
    /// The built-in vocabulary, `crates/plantgen/traits.json`.
    ///
    /// # Panics
    ///
    /// Panics if the file is not a valid vocabulary, which the tests rule
    /// out.
    #[must_use]
    pub fn builtin() -> &'static Self {
        static BUILTIN: OnceLock<Vocabulary> = OnceLock::new();
        BUILTIN.get_or_init(|| {
            let vocabulary: Self = serde_json::from_str(include_str!("../traits.json"))
                .unwrap_or_else(|error| panic!("traits.json: {error}"));
            vocabulary
                .check()
                .unwrap_or_else(|error| panic!("traits.json: {error}"));
            vocabulary
        })
    }

    /// Every organ but the whole plant is part of another, up to the whole
    /// plant; every trait is of a listed organ, in a listed group, of a
    /// known type, with values for an enum and a range for a number or
    /// count, and nothing else; every `when` reads an enum or bool
    /// character of the organ it is on or one that organ is part of, by
    /// that character's own values.
    ///
    /// # Errors
    ///
    /// Names the first organ or trait that fails.
    pub fn check(&self) -> Result<(), String> {
        let wholes: Vec<&String> = self
            .organs
            .iter()
            .filter(|(_, organ)| organ.part_of.is_none())
            .map(|(name, _)| name)
            .collect();
        if wholes.len() != 1 {
            return Err(format!(
                "one organ, the whole plant, is part of no other; these are: {}",
                wholes
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for (name, organ) in &self.organs {
            let mut seen = vec![name.as_str()];
            let mut next = organ.part_of.as_deref();
            while let Some(whole) = next {
                if seen.contains(&whole) {
                    return Err(format!(
                        "organ `{name}`: what it is part of comes back round to {whole}"
                    ));
                }
                let found = self.organs.get(whole).ok_or_else(|| {
                    format!("organ `{name}`: it is part of `{whole}`, which is not an organ")
                })?;
                seen.push(whole);
                next = found.part_of.as_deref();
            }
            self.check_when(&organ.when, organ.part_of.as_deref())
                .map_err(|error| format!("organ `{name}`: {error}"))?;
        }
        for (key, item) in &self.traits {
            let fail = |message: &str| Err(format!("trait `{key}`: {message}"));
            if !self.groups.contains(&item.group) {
                return fail(&format!("group `{}` is not listed", item.group));
            }
            if !self.organs.contains_key(&item.organ) {
                return fail(&format!("organ `{}` is not listed", item.organ));
            }
            if !KINDS.contains(&item.kind.as_str()) {
                return fail(&format!(
                    "type `{}` is not one of {}",
                    item.kind,
                    KINDS.join(", ")
                ));
            }
            let listed = matches!(item.kind.as_str(), "enum" | "enums");
            if listed != item.values.is_some() {
                return fail("an enum, and only an enum, lists its values");
            }
            let ranged = matches!(item.kind.as_str(), "number" | "count");
            if ranged != item.range.is_some() {
                return fail("a number or count, and only those, has a range");
            }
            if (item.kind == "number") != item.unit.is_some() {
                return fail("a number, and only a number, has a unit");
            }
            if item.when.contains_key(key) {
                return fail("its `when` reads itself");
            }
            self.check_when(&item.when, Some(&item.organ))
                .map_err(|error| format!("trait `{key}`: {error}"))?;
        }
        Ok(())
    }

    /// A `when` reads enum or bool characters of `organ` or an organ it
    /// is part of, each by values of its own.
    fn check_when(&self, when: &When, organ: Option<&str>) -> Result<(), String> {
        let mut holding = Vec::new();
        let mut next = organ;
        while let Some(name) = next {
            holding.push(name);
            next = self
                .organs
                .get(name)
                .and_then(|organ| organ.part_of.as_deref());
        }
        for (key, values) in when {
            let item = self
                .traits
                .get(key)
                .ok_or_else(|| format!("its `when` reads `{key}`, which is not a trait"))?;
            if !matches!(item.kind.as_str(), "enum" | "bool") {
                return Err(format!(
                    "its `when` reads `{key}`, a {}; a `when` reads an enum or bool",
                    item.kind
                ));
            }
            if !holding.contains(&item.organ.as_str()) {
                return Err(format!(
                    "its `when` reads `{key}`, a character of the {}, which holds no part of it",
                    item.organ
                ));
            }
            if values.is_empty() {
                return Err(format!("its `when` lists no values of `{key}`"));
            }
            let states = item.states();
            if let Some(value) = values.iter().find(|value| !states.contains(value)) {
                return Err(format!(
                    "its `when` lists `{value}`, which is not a value of `{key}`"
                ));
            }
        }
        Ok(())
    }

    /// Whether `value` is a value of the trait `key`, a range of its
    /// values (a number or count), qualified states (an enum or bool), or
    /// `null` (which deletes an inherited one).
    ///
    /// # Errors
    ///
    /// Says why not.
    pub fn check_value(&self, key: &str, value: &Value) -> Result<(), String> {
        let item = self
            .traits
            .get(key)
            .ok_or_else(|| format!("`{key}` is not a trait in traits.json"))?;
        match value {
            Value::Null => Ok(()),
            Value::Object(object) => match item.kind.as_str() {
                "number" | "count" => item.check_range(key, object),
                "enum" | "bool" => item.check_qualified(key, object),
                _ => Err(format!(
                    "`{key}` is {}, never an object: only a number or count may be a range, and only an enum or bool qualified",
                    item.what()
                )),
            },
            _ if item.fits(value) => Ok(()),
            _ => Err(format!("`{key}` is {}; {value} is not", item.what())),
        }
    }

    /// Whether a formula may read the trait `key`: a number, a count or a
    /// bool (read as 1 or 0).
    #[must_use]
    pub fn numeric(&self, key: &str) -> bool {
        self.traits
            .get(key)
            .is_some_and(|item| matches!(item.kind.as_str(), "number" | "count" | "bool"))
    }

    /// The organs from the whole plant down, each with its depth (0 for
    /// the whole plant): each organ, then the organs that are part of it,
    /// by name.
    #[must_use]
    pub fn organ_tree(&self) -> Vec<(usize, &str)> {
        fn walk<'a>(
            vocabulary: &'a Vocabulary,
            name: &'a str,
            depth: usize,
            tree: &mut Vec<(usize, &'a str)>,
        ) {
            tree.push((depth, name));
            for (part, organ) in &vocabulary.organs {
                if organ.part_of.as_deref() == Some(name) {
                    walk(vocabulary, part, depth + 1, tree);
                }
            }
        }
        let mut tree = Vec::new();
        for (name, organ) in &self.organs {
            if organ.part_of.is_none() {
                walk(self, name, 0, &mut tree);
            }
        }
        tree
    }

    /// Whether the organ `name` exists in a plant stating `traits`: its
    /// own `when` holds and so does that of every organ it is part of.
    #[must_use]
    pub fn organ_applies(&self, name: &str, traits: &Map<String, Value>) -> Applies {
        let Some(organ) = self.organs.get(name) else {
            return Applies::No;
        };
        let own = holds(&organ.when, traits);
        match &organ.part_of {
            None => own,
            Some(whole) => self.organ_applies(whole, traits).and(own),
        }
    }

    /// Whether the character `key` applies to a plant stating `traits`:
    /// its organ exists and its own `when` holds.
    #[must_use]
    pub fn applies(&self, key: &str, traits: &Map<String, Value>) -> Applies {
        self.traits.get(key).map_or(Applies::No, |item| {
            self.organ_applies(&item.organ, traits)
                .and(holds(&item.when, traits))
        })
    }

    /// A plant's characters, `traits` as it states them, against the
    /// vocabulary: those it states, those it misses, those that wait on
    /// characters it does not state, and any a stated one rules out.
    #[must_use]
    pub fn coverage(&self, traits: &Map<String, Value>) -> Coverage {
        let mut coverage = Coverage::default();
        for (key, item) in &self.traits {
            let stated = traits.get(key).is_some_and(|value| !value.is_null());
            let list = match (self.applies(key, traits), stated) {
                (Applies::No, true) => &mut coverage.ruled_out,
                (Applies::No, false) => continue,
                _ if item.optional => continue,
                (_, true) => &mut coverage.stated,
                (Applies::Yes, false) => &mut coverage.missing,
                (Applies::Undecided, false) => &mut coverage.undecided,
            };
            list.push(key.clone());
        }
        coverage
    }

    /// Why the character `key`, which a plant stating `traits` states,
    /// cannot apply to it: the first `when`, its own or one of the organs
    /// it is on, that the stated characters rule out.
    #[must_use]
    pub fn ruled_out_by(&self, key: &str, traits: &Map<String, Value>) -> Option<String> {
        let item = self.traits.get(key)?;
        let mut whens = vec![(None, &item.when)];
        let mut next = Some(item.organ.as_str());
        while let Some(name) = next {
            let organ = self.organs.get(name)?;
            whens.push((Some(name), &organ.when));
            next = organ.part_of.as_deref();
        }
        let (organ, when) = whens
            .into_iter()
            .find(|(_, when)| holds(when, traits) == Applies::No)?;
        let wanted = when
            .iter()
            .map(|(read, values)| format!("`{read}` is {}", or_list(values)))
            .collect::<Vec<_>>()
            .join(", or ");
        let found = when
            .keys()
            .map(|read| format!("`{read}` is {}", self.show(read, &traits[read])))
            .collect::<Vec<_>>()
            .join(" and ");
        Some(match organ {
            None => format!("`{key}` applies only where {wanted}; here {found}"),
            Some(organ) => format!(
                "`{key}` is a character of the {organ}, which exists only where {wanted}; here {found}"
            ),
        })
    }

    /// A value of the trait `key` in words: a range as `5 to 8 (low 3)`,
    /// qualified states as `usually opposite, rarely alternate`, a number
    /// with its unit, a list joined by commas.
    #[must_use]
    pub fn show(&self, key: &str, value: &Value) -> String {
        let item = self.traits.get(key);
        let unit = item
            .and_then(|item| item.unit.as_deref())
            .map_or(String::new(), |unit| format!(" {unit}"));
        let plain = |value: &Value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        match value {
            Value::Object(object) if object.contains_key("min") => {
                let extras: Vec<String> = ["low", "high", "typical"]
                    .iter()
                    .filter_map(|bound| Some(format!("{bound} {}", object.get(*bound)?)))
                    .collect();
                format!(
                    "{} to {}{unit}{}",
                    object["min"],
                    object.get("max").unwrap_or(&Value::Null),
                    if extras.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", extras.join(", "))
                    }
                )
            }
            Value::Object(object) => {
                let mut states: Vec<(usize, &String)> = object
                    .iter()
                    .map(|(state, frequency)| {
                        let rank = FREQUENCIES
                            .iter()
                            .position(|known| frequency.as_str() == Some(known))
                            .unwrap_or(FREQUENCIES.len());
                        (rank, state)
                    })
                    .collect();
                states.sort();
                states
                    .iter()
                    .map(|(rank, state)| match FREQUENCIES.get(*rank) {
                        Some(frequency) => format!("{frequency} {state}"),
                        None => (*state).clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            Value::Array(items) if item.is_none_or(|item| item.kind != "colour") => {
                items.iter().map(plain).collect::<Vec<_>>().join(", ")
            }
            Value::Number(_) => format!("{value}{unit}"),
            other => plain(other),
        }
    }

    /// Check a part's traits and rules: each trait in the vocabulary with
    /// a value that fits it; each rule reading what a rule may read (a
    /// formula number traits and parameters of `programs`, a map one enum
    /// or bool trait, by its values), and a parameter it sets declared by
    /// some program in `programs`.
    ///
    /// # Errors
    ///
    /// Names the first trait or rule that fails.
    pub fn check_part(&self, part: &Part, programs: &Programs) -> Result<(), String> {
        for (key, value) in &part.traits {
            self.check_value(key, value)?;
        }
        for (path, rule) in &part.rules {
            let Value::Object(rule) = rule else {
                continue;
            };
            self.check_rule(path, rule, programs)
                .map_err(|error| format!("the rule for `{path}`: {error}"))?;
        }
        Ok(())
    }

    fn check_rule(
        &self,
        path: &str,
        rule: &Map<String, Value>,
        programs: &Programs,
    ) -> Result<(), String> {
        if let Some(Value::String(formula)) = rule.get("rule") {
            for name in Formula::parse(formula)?.names() {
                let param = programs.values().any(|params| params.contains_key(&name));
                if param && self.traits.contains_key(&name) {
                    return Err(format!(
                        "it reads `{name}`, both a trait and a program's parameter"
                    ));
                }
                if !param && !self.numeric(&name) {
                    return Err(format!(
                        "it reads `{name}`, which is neither a number, count or bool trait (a `map` reads the others) nor a program's parameter"
                    ));
                }
            }
        }
        if let Some(Value::Object(map)) = rule.get("map")
            && let Some((key, Value::Object(table))) = map.iter().next()
        {
            let item = self
                .traits
                .get(key)
                .ok_or_else(|| format!("`{key}` is not a trait in traits.json"))?;
            for name in table.keys() {
                let known = match item.kind.as_str() {
                    "enum" => item.values.iter().flatten().any(|value| value == name),
                    "bool" => name == "true" || name == "false",
                    _ => {
                        return Err(format!(
                            "a map reads an enum or bool trait; `{key}` is a {}",
                            item.kind
                        ));
                    }
                };
                if !known {
                    return Err(format!("`{name}` is not a value of `{key}`"));
                }
            }
        }
        if let Some(param) = path.strip_prefix("generator.params.")
            && !programs.values().any(|params| params.contains_key(param))
        {
            return Err(format!("no program has a parameter `{param}`"));
        }
        Ok(())
    }
}

/// Whether `when` holds for a plant stating `traits`: yes where any
/// character it reads is stated in one of its values (each state of
/// qualified states counts), no where every one is stated in none of
/// them, else undecided. An empty `when` always holds.
fn holds(when: &When, traits: &Map<String, Value>) -> Applies {
    if when.is_empty() {
        return Applies::Yes;
    }
    let mut undecided = false;
    for (key, values) in when {
        match traits.get(key).filter(|value| !value.is_null()) {
            None => undecided = true,
            Some(value) => {
                if states_of(value).iter().any(|state| values.contains(state)) {
                    return Applies::Yes;
                }
            }
        }
    }
    if undecided {
        Applies::Undecided
    } else {
        Applies::No
    }
}

/// The states a value of an enum or bool trait names: its one state, or
/// each state it qualifies.
fn states_of(value: &Value) -> Vec<String> {
    match value {
        Value::String(state) => vec![state.clone()],
        Value::Bool(state) => vec![state.to_string()],
        Value::Object(qualified) => qualified.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `a`, `a or b`, `a, b or c`.
fn or_list(values: &[String]) -> String {
    match values {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{Applies, Vocabulary};

    /// A plant's traits from a JSON object.
    fn stating(traits: &Value) -> Map<String, Value> {
        traits.as_object().unwrap().clone()
    }

    /// The growth forms a trait names are the spec's, so the two lists
    /// cannot drift apart.
    #[test]
    fn growth_forms_are_the_specs() {
        use crate::spec::GrowthForm;
        // A form added to the spec fails this match until `ALL` has it.
        for form in GrowthForm::ALL {
            match form {
                GrowthForm::ExcurrentTree
                | GrowthForm::DecurrentTree
                | GrowthForm::ScaleLeavedTree
                | GrowthForm::Shrub
                | GrowthForm::Graminoid
                | GrowthForm::Forb
                | GrowthForm::Fern
                | GrowthForm::Vine
                | GrowthForm::StemSucculent
                | GrowthForm::RosetteSucculent
                | GrowthForm::Palm
                | GrowthForm::Epiphyte
                | GrowthForm::Cushion => {}
            }
        }
        let names: Vec<String> = GrowthForm::ALL
            .iter()
            .map(|form| {
                serde_json::to_value(form)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            Vocabulary::builtin().traits["growth_form"].values.as_ref(),
            Some(&names)
        );
    }

    /// Every flower cluster kind of the forms plan's L1 is an
    /// `inflorescence`. L1 is not in the code yet: once it is, this reads
    /// its kinds from there.
    #[test]
    fn inflorescences_hold_the_cluster_kinds() {
        const CLUSTERS: [&str; 10] = [
            "raceme",
            "spike",
            "catkin",
            "corymb",
            "umbel",
            "compound_umbel",
            "panicle",
            "cyme",
            "scorpioid",
            "head",
        ];
        let values = Vocabulary::builtin().traits["inflorescence"]
            .values
            .clone()
            .unwrap();
        for kind in CLUSTERS {
            assert!(values.iter().any(|value| value == kind), "{kind}");
        }
    }

    /// The built-in vocabulary is valid, holds RECORD.md's groups and the
    /// architecture group, and checks values by type.
    #[test]
    fn the_vocabulary_checks_values_by_type() {
        let vocabulary = Vocabulary::builtin();
        assert_eq!(vocabulary.schema, 2);
        assert!(vocabulary.traits.len() > 50);
        assert!(vocabulary.groups.contains(&"architecture".to_string()));
        for (key, value) in [
            ("leaf_length_m", json!(0.12)),
            ("leaf_arrangement", json!("opposite")),
            ("clonal", json!(true)),
            ("leaflets", json!(7)),
            ("flowering_months", json!([4, 5])),
            ("bark_colour", json!([120, 110, 100])),
            ("nurse_forms", json!(["shrub", "tree"])),
            ("hosts", json!(["acer-macrophyllum"])),
            ("habit_notes", json!("Many-stemmed from the base.")),
            ("architectural_model", json!("scarrone")),
            ("leaf_habit", json!(null)),
        ] {
            vocabulary.check_value(key, &value).unwrap();
        }
        for (key, value, wanted) in [
            ("leaf_length_m", json!(30), "from 0.0005 to 25 m"),
            (
                "leaf_arrangement",
                json!("spiral"),
                "is one of alternate, opposite",
            ),
            ("leaflets", json!(2.5), "is a count from 2 to 2000"),
            ("flowering_months", json!([13]), "is a list of months"),
            ("bark_colour", json!([0.5, 0.4, 0.3]), "is a colour"),
            ("leaf_size", json!(1), "not a trait in traits.json"),
        ] {
            let error = vocabulary.check_value(key, &value).unwrap_err();
            assert!(error.contains(wanted), "{key}: {error}");
        }
        assert!(vocabulary.numeric("leaf_length_m"));
        assert!(vocabulary.numeric("short_shoots"));
        assert!(!vocabulary.numeric("leaf_form"));
    }

    /// A number or count may be a range, an enum or bool qualified states;
    /// each is held to the trait's values.
    #[test]
    fn ranges_and_qualified_states_are_checked() {
        let vocabulary = Vocabulary::builtin();
        for (key, value) in [
            ("ray_florets", json!({"min": 5, "max": 8, "low": 3})),
            (
                "leaf_length_m",
                json!({"min": 0.035, "max": 0.35, "typical": 0.1, "high": 0.4}),
            ),
            ("units_per_inflorescence", json!({"min": 10, "max": 10})),
            (
                "leaf_arrangement",
                json!({"opposite": "usually", "alternate": "rarely"}),
            ),
            ("clonal", json!({"true": "often", "false": "often"})),
        ] {
            vocabulary.check_value(key, &value).unwrap();
        }
        for (key, value, wanted) in [
            (
                "ray_florets",
                json!({"min": 5}),
                "a range has a `min` and a `max`",
            ),
            (
                "ray_florets",
                json!({"min": 5, "max": 8, "most": 6}),
                "`most` is not part of a range",
            ),
            (
                "ray_florets",
                json!({"min": 5.5, "max": 8}),
                "its `min` is a count from 1 to 200; 5.5 is not",
            ),
            (
                "ray_florets",
                json!({"min": 5, "max": 8, "low": 6}),
                "its `min` 5 is below its `low` 6",
            ),
            (
                "leaf_length_m",
                json!({"min": 0.2, "max": 0.1}),
                "its `max` 0.1 is below its `min` 0.2",
            ),
            (
                "leaf_length_m",
                json!({"min": 0.1, "max": 0.2, "typical": 0.3}),
                "its `max` 0.2 is below its `typical` 0.3",
            ),
            (
                "leaf_arrangement",
                json!({"spiral": "usually"}),
                "`spiral` is not one of its states, alternate, opposite, whorled",
            ),
            (
                "leaf_arrangement",
                json!({"opposite": "mostly"}),
                "how often it is opposite is one of usually, often, sometimes, rarely",
            ),
            (
                "leaf_arrangement",
                json!({"opposite": "usually", "alternate": "usually"}),
                "at most one state is `usually`",
            ),
            ("leaf_arrangement", json!({}), "name at least one state"),
            (
                "flowering_months",
                json!({"min": 4, "max": 6}),
                "never an object",
            ),
        ] {
            let error = vocabulary.check_value(key, &value).unwrap_err();
            assert!(error.contains(wanted), "{key}: {error}");
        }
    }

    /// The organs form one tree from the whole plant, and what a plant
    /// states decides which organs, and so which characters, apply.
    #[test]
    fn what_a_plant_states_decides_what_applies() {
        let vocabulary = Vocabulary::builtin();
        let tree = vocabulary.organ_tree();
        assert_eq!(tree[0], (0, "plant"));
        assert_eq!(tree.len(), vocabulary.organs.len());
        let at = |organ: &str| tree.iter().position(|(_, name)| *name == organ).unwrap();
        assert!(at("capitulum") < at("ray_floret") && at("inflorescence") < at("capitulum"));

        // Nothing stated: the plant, its stems and leaves apply; flowers
        // and cones wait on how it reproduces.
        let nothing = Map::new();
        assert_eq!(vocabulary.organ_applies("leaf", &nothing), Applies::Yes);
        assert_eq!(
            vocabulary.organ_applies("inflorescence", &nothing),
            Applies::Undecided
        );
        let blank = vocabulary.coverage(&nothing);
        assert!(blank.stated.is_empty() && blank.ruled_out.is_empty());
        assert!(blank.missing.contains(&"reproduction".to_string()));
        assert!(blank.undecided.contains(&"ray_florets".to_string()));
        // Optional characters are never missing.
        assert!(!blank.missing.contains(&"habit_notes".to_string()));
        assert!(blank.share() < 0.01);

        // A radiate head: rays and disc florets apply, single flowers,
        // spikelets and cones do not.
        let daisy = stating(&json!({"reproduction": "flowers",
            "inflorescence_unit": "capitulum", "capitulum_kind": "radiate"}));
        for (organ, applies) in [
            ("ray_floret", Applies::Yes),
            ("disc_floret", Applies::Yes),
            ("ligulate_floret", Applies::No),
            ("flower", Applies::No),
            ("spikelet", Applies::No),
            ("cone", Applies::No),
            ("fruit", Applies::Yes),
            ("bark", Applies::Undecided),
        ] {
            assert_eq!(vocabulary.organ_applies(organ, &daisy), applies, "{organ}");
        }
        let covered = vocabulary.coverage(&daisy);
        assert_eq!(covered.stated.len(), 3);
        assert!(covered.missing.contains(&"ray_florets".to_string()));
        assert!(!covered.missing.contains(&"flower_colour".to_string()));
        assert!(!covered.undecided.contains(&"flower_colour".to_string()));
        // Qualified states count with each state they name.
        let mixed = stating(&json!({"reproduction": "flowers",
            "inflorescence_unit": {"capitulum": "usually", "flower": "rarely"},
            "capitulum_kind": {"discoid": "usually", "radiate": "sometimes"}}));
        assert_eq!(vocabulary.organ_applies("ray_floret", &mixed), Applies::Yes);
        assert_eq!(vocabulary.organ_applies("flower", &mixed), Applies::Yes);

        // A character's own `when`: any of its characters may open it.
        for (traits, applies) in [
            (
                json!({"leaf_division": "simple", "leaf_incision": "lobed"}),
                Applies::Yes,
            ),
            (json!({"leaf_division": "compound"}), Applies::Yes),
            (json!({"leaf_division": "simple"}), Applies::Undecided),
            (
                json!({"leaf_division": "simple", "leaf_incision": "none"}),
                Applies::No,
            ),
        ] {
            assert_eq!(
                vocabulary.applies("division_pattern", &stating(&traits)),
                applies,
                "{traits}"
            );
        }

        // A stated character that another rules out, and why.
        let herb = stating(&json!({"woodiness": "herbaceous", "bark_texture": "smooth",
            "leaf_arrangement": "opposite", "leaves_per_whorl": 4}));
        let ruled = vocabulary.coverage(&herb).ruled_out;
        assert_eq!(ruled, ["bark_texture", "leaves_per_whorl"]);
        assert_eq!(
            vocabulary.ruled_out_by("bark_texture", &herb).unwrap(),
            "`bark_texture` is a character of the bark, which exists only where `woodiness` is woody or semi; here `woodiness` is herbaceous"
        );
        assert_eq!(
            vocabulary.ruled_out_by("leaves_per_whorl", &herb).unwrap(),
            "`leaves_per_whorl` applies only where `leaf_arrangement` is whorled; here `leaf_arrangement` is opposite"
        );
    }

    /// Values in words, as `plantc traits` prints them.
    #[test]
    fn values_read_as_words() {
        let vocabulary = Vocabulary::builtin();
        for (key, value, shown) in [
            (
                "ray_florets",
                json!({"min": 5, "max": 8, "low": 3}),
                "5 to 8 (low 3)",
            ),
            (
                "spikelet_length_m",
                json!({"min": 0.005, "max": 0.009}),
                "0.005 to 0.009 m",
            ),
            (
                "leaf_arrangement",
                json!({"whorled": "rarely", "alternate": "often", "opposite": "often"}),
                "often alternate, often opposite, rarely whorled",
            ),
            ("leaf_length_m", json!(0.12), "0.12 m"),
            ("flowering_months", json!([4, 5]), "4, 5"),
            ("bark_colour", json!([120, 110, 100]), "[120,110,100]"),
            ("inflorescence_unit", json!("capitulum"), "capitulum"),
        ] {
            assert_eq!(vocabulary.show(key, &value), shown, "{key}");
        }
    }

    /// The vocabulary's own rules: one whole plant, organs part of organs,
    /// and conditions on the characters of the organs above.
    #[test]
    fn vocabularies_against_the_rules_are_refused() {
        let vocabulary = |organs: Value, traits: Value| -> Vocabulary {
            serde_json::from_value(json!({"schema": 2, "about": "test", "groups": ["form"],
                "organs": organs, "traits": traits}))
            .unwrap()
        };
        let plant = json!({"why": "the plant"});
        let woodiness = json!({"group": "form", "organ": "plant", "type": "enum",
            "values": ["woody", "herbaceous"], "why": "wood"});
        let refused = |organs: Value, traits: Value, wanted: &str| {
            let error = vocabulary(organs, traits).check().unwrap_err();
            assert!(error.contains(wanted), "{error}");
        };
        refused(
            json!({"plant": plant, "moss": {"why": "another whole"}}),
            json!({}),
            "one organ, the whole plant, is part of no other; these are: moss, plant",
        );
        refused(
            json!({"plant": plant, "stem": {"part_of": "twig", "why": "a stem"},
                "twig": {"part_of": "stem", "why": "a twig"}}),
            json!({}),
            "what it is part of comes back round",
        );
        refused(
            json!({"plant": plant, "stem": {"part_of": "trunk", "why": "a stem"}}),
            json!({}),
            "organ `stem`: it is part of `trunk`, which is not an organ",
        );
        refused(
            json!({"plant": plant, "bark": {"part_of": "plant",
                "when": {"woodiness": ["wooden"]}, "why": "bark"}}),
            json!({"woodiness": woodiness}),
            "organ `bark`: its `when` lists `wooden`, which is not a value of `woodiness`",
        );
        refused(
            json!({"plant": plant, "stem": {"part_of": "plant", "why": "a stem"},
                "leaf": {"part_of": "plant", "when": {"thorny": ["true"]}, "why": "a leaf"}}),
            json!({"thorny": {"group": "form", "organ": "stem", "type": "bool", "why": "thorns"}}),
            "organ `leaf`: its `when` reads `thorny`, a character of the stem, which holds no part of it",
        );
        refused(
            json!({"plant": plant}),
            json!({"height": {"group": "form", "organ": "plant", "type": "number",
                "unit": "m", "range": [0, 100], "why": "height"},
                "width": {"group": "form", "organ": "plant", "type": "number",
                "unit": "m", "range": [0, 100], "when": {"height": ["1"]}, "why": "width"}}),
            "trait `width`: its `when` reads `height`, a number; a `when` reads an enum or bool",
        );
        refused(
            json!({"plant": plant}),
            json!({"woodiness": {"group": "form", "organ": "root", "type": "bool", "why": "wood"}}),
            "trait `woodiness`: organ `root` is not listed",
        );
    }
}
