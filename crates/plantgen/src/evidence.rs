//! Where a species' values come from: a note for each value saying how it
//! is known and which source it comes from, the sources themselves, and the
//! contributors behind the whole spec.
//!
//! A note names its source by a stable id: the name of a file
//! `library/sources/<id>.json` holding a [`Source`]. The tier lives in the
//! source, not on each note, so every value from one source can be listed,
//! and removed, in one step (`plantc sources cite ID`). These types write
//! the JSON Project After's evidence types wrote, plus the note's optional
//! `source`, written only when set, so a spec without one keeps its bytes.

use serde::{Deserialize, Serialize};

/// How a value is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Evidence {
    /// Taken directly from a measurement.
    Measured,
    /// Derived from source data.
    SourceInferred,
    /// Set by a person on purpose.
    Authored,
    /// From an explicit test fixture.
    Synthetic,
    /// Not known.
    Unknown,
}

/// The tiers a source can have, from the most reliable: 1 peer-reviewed
/// and institutional botany, 2 government and agency data, 3 curated
/// aggregations and observations, 4 inferred (from the genus, the family
/// or a model).
pub const TIERS: std::ops::RangeInclusive<u8> = 1..=4;

/// Whether `id` can name a source: 1 to 64 lowercase letters, digits and
/// hyphens, the rule species ids follow.
#[must_use]
pub fn is_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// How one value of authored data (a species, a niche) is known, and why.
/// A note may cover a subtree of values by naming the subtree's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldEvidence {
    pub evidence: Evidence,
    /// The note's source: the id of a [`Source`] in the library's
    /// `sources/` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub note: String,
}

impl FieldEvidence {
    /// Checks that the note's source, where it names one, is written as a
    /// source id. Whether the library holds that source is the library's
    /// check ([`crate::library::Library::source`]).
    ///
    /// # Errors
    ///
    /// Says what is wrong with the id.
    pub fn check(&self) -> Result<(), String> {
        match &self.source {
            Some(id) if !is_source_id(id) => Err(format!(
                "source `{id}` is not a source id: 1 to 64 lowercase letters, digits or hyphens"
            )),
            _ => Ok(()),
        }
    }
}

/// What kind of thing a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A dataset, used as published.
    Dataset,
    /// A text (a flora, a monograph, a paper), its facts in our own words.
    Text,
    /// Observations or occurrence records.
    Observations,
    /// A model's output.
    Model,
    /// Inferred from the genus, the family or by hand, with no outside
    /// source.
    Inferred,
}

/// A source values cite by id, one per file `library/sources/<id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The file's name without `.json`.
    pub id: String,
    pub title: String,
    pub authors: String,
    pub year: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// SPDX identifier or a short licence statement.
    pub licence: String,
    /// One of [`TIERS`].
    pub tier: u8,
    pub kind: SourceKind,
}

impl Source {
    /// Parse a source and check it.
    ///
    /// # Errors
    ///
    /// Fails on malformed JSON or an invalid source.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let source: Self =
            serde_json::from_str(text).map_err(|error| format!("invalid source: {error}"))?;
        source.validate()?;
        Ok(source)
    }

    /// Checks the id, the tier and that no text field is blank.
    ///
    /// # Errors
    ///
    /// Describes the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        if !is_source_id(&self.id) {
            return Err(format!(
                "source `{}`: the id must be 1 to 64 lowercase letters, digits or hyphens",
                self.id
            ));
        }
        if !TIERS.contains(&self.tier) {
            return Err(format!(
                "source `{}`: tier {} is not one of {} to {}",
                self.id,
                self.tier,
                TIERS.start(),
                TIERS.end()
            ));
        }
        for (name, text) in [
            ("title", &self.title),
            ("authors", &self.authors),
            ("licence", &self.licence),
        ] {
            if text.trim().is_empty() {
                return Err(format!("source `{}`: {name} is blank", self.id));
            }
        }
        Ok(())
    }
}

/// The path of every value in `document` outside its `skip` keys: object
/// keys joined by dots, array elements that are objects by their index
/// (`pieces.0.kind`). Any other array, a colour or a range, is one value;
/// `null` is no value.
#[must_use]
pub fn value_paths(document: &serde_json::Value, skip: &[&str]) -> Vec<String> {
    fn walk(value: &serde_json::Value, path: &str, paths: &mut Vec<String>) {
        let join = |key: &str| {
            if path.is_empty() {
                key.to_string()
            } else {
                format!("{path}.{key}")
            }
        };
        match value {
            serde_json::Value::Null => {}
            serde_json::Value::Object(map) => {
                for (key, inner) in map {
                    walk(inner, &join(key), paths);
                }
            }
            serde_json::Value::Array(items) if items.iter().any(serde_json::Value::is_object) => {
                for (index, inner) in items.iter().enumerate() {
                    walk(inner, &join(&index.to_string()), paths);
                }
            }
            _ => paths.push(path.to_string()),
        }
    }
    let mut paths = Vec::new();
    if let serde_json::Value::Object(map) = document {
        for (key, inner) in map {
            if !skip.contains(&key.as_str()) {
                walk(inner, key, &mut paths);
            }
        }
    }
    paths
}

/// Hold `notes` to the rule for authored data: every value of `document`
/// (outside `skip`) has a note on its own path or on a subtree holding it
/// (`foliage` covers `foliage.length_m`), no note names a path the document
/// does not have, and no note is blank.
///
/// # Errors
///
/// Names the first value without a note, or the first note without a value.
pub fn check_coverage(
    document: &serde_json::Value,
    notes: &std::collections::BTreeMap<String, FieldEvidence>,
    skip: &[&str],
) -> Result<(), String> {
    let paths = value_paths(document, skip);
    let covers = |key: &str, path: &str| {
        path == key
            || path
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    for (key, note) in notes {
        if note.note.trim().is_empty() {
            return Err(format!("the evidence note on `{key}` is blank"));
        }
        if !paths.iter().any(|path| covers(key, path)) {
            return Err(format!(
                "an evidence note names `{key}`, which holds no value"
            ));
        }
    }
    match paths
        .iter()
        .find(|path| !notes.keys().any(|key| covers(key, path)))
    {
        Some(path) => Err(format!("`{path}` has no evidence note")),
        None => Ok(()),
    }
}

/// A source authored data draws on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// SPDX identifier or a short licence statement.
    pub licence: String,
}

/// Where authored data came from and who made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub sources: Vec<SourceRef>,
    pub contributors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{Evidence, FieldEvidence, Source, SourceKind};

    #[test]
    fn a_source_is_written_only_when_set() {
        let mut note = FieldEvidence {
            evidence: Evidence::Authored,
            source: None,
            note: "Hand-set.".into(),
        };
        assert_eq!(
            serde_json::to_string(&note).unwrap(),
            r#"{"evidence":"Authored","note":"Hand-set."}"#
        );
        note.source = Some("fna-v4".into());
        let text = serde_json::to_string(&note).unwrap();
        assert_eq!(
            text,
            r#"{"evidence":"Authored","source":"fna-v4","note":"Hand-set."}"#
        );
        assert_eq!(serde_json::from_str::<FieldEvidence>(&text).unwrap(), note);
        assert!(serde_json::from_str::<FieldEvidence>(&text.replace("source", "sauce")).is_err());
        // A note names its source by id, never by a position or a tier.
        assert!(
            serde_json::from_str::<FieldEvidence>(
                r#"{"evidence":"Authored","source":1,"note":"Hand-set."}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<FieldEvidence>(
                r#"{"evidence":"Authored","tier":4,"note":"Hand-set."}"#
            )
            .is_err()
        );
    }

    #[test]
    fn a_note_s_source_is_written_as_an_id() {
        let mut note = FieldEvidence {
            evidence: Evidence::SourceInferred,
            source: Some("wcvp-v16".into()),
            note: "From the checklist.".into(),
        };
        assert_eq!(note.check(), Ok(()));
        note.source = None;
        assert_eq!(note.check(), Ok(()));
        for bad in ["", "WCVP", "wcvp v16", &"a".repeat(65)] {
            note.source = Some(bad.to_string());
            assert!(
                note.check().unwrap_err().contains("not a source id"),
                "{bad}"
            );
        }
    }

    #[test]
    fn sources_are_checked() {
        let text = r#"{"id": "fna-v4", "title": "Flora of North America, vol. 4",
            "authors": "FNA Editorial Committee", "year": 2003,
            "licence": "copyrighted text; facts only", "tier": 1, "kind": "text"}"#;
        let source = Source::from_json(text).unwrap();
        assert_eq!(source.kind, SourceKind::Text);
        assert!(
            Source::from_json(&text.replace("\"tier\": 1", "\"tier\": 5"))
                .unwrap_err()
                .contains("tier 5")
        );
        assert!(
            Source::from_json(&text.replace("FNA Editorial Committee", " "))
                .unwrap_err()
                .contains("authors is blank")
        );
        assert!(Source::from_json(&text.replace("fna-v4", "FNA")).is_err());
        assert!(Source::from_json(&text.replace("\"text\"", "\"rumour\"")).is_err());
        assert!(Source::from_json(&text.replace("\"year\"", "\"yeer\"")).is_err());
    }

    #[test]
    fn notes_cover_every_value_and_name_only_values() {
        use std::collections::BTreeMap;
        let document = serde_json::json!({
            "id": "abies-grandis",
            "foliage": {"length_m": [0.02, 0.05], "curl": 0.0, "shape": null},
            "pieces": [{"kind": "twig", "per_m2": 3.0}],
            "decay": 0.3,
        });
        assert_eq!(
            super::value_paths(&document, &["id"]),
            [
                "decay",
                "foliage.curl",
                "foliage.length_m",
                "pieces.0.kind",
                "pieces.0.per_m2"
            ]
        );
        let note = |text: &str| FieldEvidence {
            evidence: Evidence::Authored,
            source: None,
            note: text.into(),
        };
        let mut notes: BTreeMap<String, FieldEvidence> = ["foliage", "pieces", "decay"]
            .into_iter()
            .map(|key| (key.to_string(), note("Hand-set.")))
            .collect();
        assert_eq!(super::check_coverage(&document, &notes, &["id"]), Ok(()));
        // A more specific note may stand beside its subtree's.
        notes.insert("foliage.curl".into(), note("Measured on photographs."));
        assert_eq!(super::check_coverage(&document, &notes, &["id"]), Ok(()));
        notes.remove("decay");
        assert_eq!(
            super::check_coverage(&document, &notes, &["id"]),
            Err("`decay` has no evidence note".into())
        );
        notes.insert("decay".into(), note(" "));
        assert!(
            super::check_coverage(&document, &notes, &["id"])
                .unwrap_err()
                .contains("blank")
        );
        notes.insert("decay".into(), note("Hand-set."));
        notes.insert("foli".into(), note("Hand-set."));
        assert_eq!(
            super::check_coverage(&document, &notes, &["id"]),
            Err("an evidence note names `foli`, which holds no value".into())
        );
    }
}
