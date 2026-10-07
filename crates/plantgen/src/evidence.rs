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
