//! The trait vocabulary (growth plan G2.2): every trait a record, a rank
//! file or a species' spec may state, from `traits.json`, with its group,
//! its type and its unit, values or range. It holds no rules: the rules
//! that turn traits into spec values live in the taxa they hold for
//! ([`crate::inherit`]).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::formula::Formula;
use crate::inherit::{Part, Programs};

/// One trait of the vocabulary.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trait {
    pub group: String,
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
    pub why: String,
}

/// The vocabulary: `traits.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub schema: u32,
    pub about: String,
    pub groups: Vec<String>,
    pub traits: BTreeMap<String, Trait>,
}

const KINDS: [&str; 9] = [
    "number", "count", "bool", "enum", "enums", "months", "colour", "text", "ids",
];

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

    /// Every trait is in a listed group, of a known type, with values for
    /// an enum and a range for a number or count, and nothing else.
    ///
    /// # Errors
    ///
    /// Names the first trait that is not.
    pub fn check(&self) -> Result<(), String> {
        for (key, item) in &self.traits {
            let fail = |message: &str| Err(format!("trait `{key}`: {message}"));
            if !self.groups.contains(&item.group) {
                return fail(&format!("group `{}` is not listed", item.group));
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
        }
        Ok(())
    }

    /// Whether `value` is a value of the trait `key`, or `null` (which
    /// deletes an inherited one).
    ///
    /// # Errors
    ///
    /// Says why not.
    pub fn check_value(&self, key: &str, value: &Value) -> Result<(), String> {
        let item = self
            .traits
            .get(key)
            .ok_or_else(|| format!("`{key}` is not a trait in traits.json"))?;
        if value.is_null() {
            return Ok(());
        }
        let in_range = |number: f64| {
            item.range
                .is_none_or(|[low, high]| (low..=high).contains(&number))
        };
        let listed = |name: &Value| {
            name.as_str()
                .is_some_and(|name| item.values.iter().flatten().any(|value| value == name))
        };
        let fits = match item.kind.as_str() {
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
        };
        if fits {
            return Ok(());
        }
        let what = match (item.kind.as_str(), &item.values, &item.range) {
            ("enum", Some(values), _) => format!("one of {}", values.join(", ")),
            ("enums", Some(values), _) => format!("a list of {}", values.join(", ")),
            (kind, _, Some([low, high])) => format!(
                "a {kind} from {low} to {high}{}",
                item.unit
                    .as_deref()
                    .map_or(String::new(), |unit| format!(" {unit}"))
            ),
            ("months", _, _) => "a list of months, 1 to 12".into(),
            ("colour", _, _) => "a colour, three numbers 0 to 255".into(),
            ("text", _, _) => "a line of text".into(),
            ("ids", _, _) => "a list of species ids".into(),
            (kind, _, _) => format!("a {kind}"),
        };
        Err(format!("`{key}` is {what}; {value} is not"))
    }

    /// Whether a formula may read the trait `key`: a number, a count or a
    /// bool (read as 1 or 0).
    #[must_use]
    pub fn numeric(&self, key: &str) -> bool {
        self.traits
            .get(key)
            .is_some_and(|item| matches!(item.kind.as_str(), "number" | "count" | "bool"))
    }

    /// Check a part's traits and rules: each trait in the vocabulary with
    /// a value that fits it; each rule reading traits a rule may read (a
    /// formula numbers, a map one enum or bool, by its values), and a
    /// parameter it sets declared by some program in `programs`.
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
                if !self.numeric(&name) {
                    return Err(format!(
                        "it reads `{name}`, which is not a number, count or bool trait (a `map` reads the others)"
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
            && !programs.values().any(|params| params.contains(param))
        {
            return Err(format!("no program has a parameter `{param}`"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Vocabulary;

    /// The built-in vocabulary is valid, holds RECORD.md's groups and the
    /// architecture group, and checks values by type.
    #[test]
    fn the_vocabulary_checks_values_by_type() {
        let vocabulary = Vocabulary::builtin();
        assert_eq!(vocabulary.schema, 1);
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
            ("leaflets", json!(2.5), "is a count from 0 to 2000"),
            ("flowering_months", json!([13]), "is a list of months"),
            ("bark_colour", json!([0.5, 0.4, 0.3]), "is a colour"),
            ("leaf_size", json!(1), "not a trait in traits.json"),
        ] {
            let error = vocabulary.check_value(key, &value).unwrap_err();
            assert!(error.contains(wanted), "{key}: {error}");
        }
        assert!(vocabulary.numeric("leaf_length_m"));
        assert!(vocabulary.numeric("short_shoots"));
        assert!(!vocabulary.numeric("leaf_type"));
    }
}
