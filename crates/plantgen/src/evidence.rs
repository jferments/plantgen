//! Where a species' values come from: a note for each value saying how it
//! is known, and the sources and contributors behind the whole spec.
//!
//! These types write exactly the JSON that Project After's own evidence
//! types (`after_world::semantics`) write, which `PlantGen` used until it
//! had its own, so every package's embedded spec keeps its bytes;
//! `after-plants`' tests compare the two. `FieldEvidence` adds two
//! optional fields, a note's source and that source's tier, written only
//! when set.

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

/// How one value of authored data (a species, a niche) is known, and why.
/// A note may cover a subtree of values by naming the subtree's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldEvidence {
    pub evidence: Evidence,
    /// The tier of the note's source, one of [`TIERS`]. Not the spec's
    /// model tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<u8>,
    /// The note's source: an index into `provenance.sources`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<usize>,
    pub note: String,
}

impl FieldEvidence {
    /// Checks the note's tier and source, where it gives them, against
    /// [`TIERS`] and the sources of `provenance`.
    ///
    /// # Errors
    ///
    /// Says which is out of range.
    pub fn check(&self, provenance: &Provenance) -> Result<(), String> {
        if let Some(tier) = self.tier.filter(|tier| !TIERS.contains(tier)) {
            return Err(format!(
                "tier {tier} is not one of {} to {}",
                TIERS.start(),
                TIERS.end()
            ));
        }
        let sources = provenance.sources.len();
        if let Some(source) = self.source.filter(|&source| source >= sources) {
            return Err(format!(
                "source {source} is out of range: provenance has {sources} sources, counted from 0"
            ));
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
    use super::{Evidence, FieldEvidence, Provenance, SourceRef};

    fn provenance(sources: usize) -> Provenance {
        Provenance {
            sources: (0..sources)
                .map(|index| SourceRef {
                    title: format!("source {index}"),
                    url: None,
                    licence: "CC0-1.0".into(),
                })
                .collect(),
            contributors: vec!["a contributor".into()],
            notes: None,
        }
    }

    #[test]
    fn tier_and_source_are_written_only_when_set() {
        let mut note = FieldEvidence {
            evidence: Evidence::Authored,
            tier: None,
            source: None,
            note: "Hand-set.".into(),
        };
        assert_eq!(
            serde_json::to_string(&note).unwrap(),
            r#"{"evidence":"Authored","note":"Hand-set."}"#
        );
        note.tier = Some(4);
        note.source = Some(1);
        let text = serde_json::to_string(&note).unwrap();
        assert_eq!(
            text,
            r#"{"evidence":"Authored","tier":4,"source":1,"note":"Hand-set."}"#
        );
        assert_eq!(serde_json::from_str::<FieldEvidence>(&text).unwrap(), note);
        assert!(serde_json::from_str::<FieldEvidence>(&text.replace("tier", "tear")).is_err());
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
            tier: None,
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

    #[test]
    fn tier_and_source_are_checked() {
        let mut note = FieldEvidence {
            evidence: Evidence::SourceInferred,
            tier: Some(1),
            source: Some(1),
            note: "From the flora.".into(),
        };
        assert_eq!(note.check(&provenance(2)), Ok(()));
        assert!(note.check(&provenance(1)).unwrap_err().contains("source 1"));
        note.source = None;
        assert_eq!(note.check(&provenance(0)), Ok(()));
        for tier in [0, 5] {
            note.tier = Some(tier);
            assert!(
                note.check(&provenance(2))
                    .unwrap_err()
                    .contains(&format!("tier {tier}"))
            );
        }
    }
}
