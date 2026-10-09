//! What falls from a species: the `shed` section of its record,
//! `shed.json` in its folder of the library.
//!
//! A [`Shed`] holds the foliage a species sheds (needles, sprays of scale
//! leaves or leaves), the colours it falls in and dries to, how its leaves
//! curl and how fast it rots, what falls with it (cones, cone scales,
//! fruit, samaras, catkins, bark, twigs), and how its litter texture looks.
//! [`crate::litter`] draws a species' litter look from it. Being new data,
//! the section follows the full rule for evidence: every value has a note
//! ([`crate::evidence::check_coverage`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::evidence::{FieldEvidence, Provenance, check_coverage};
use crate::litter::ExtraKind;
use crate::looks::Shape;

/// The section's schema.
pub const SHED_SCHEMA: u32 = 1;

/// What falls from one species, and how its litter looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shed {
    pub schema: u32,
    /// The species' id, its folder's name.
    pub id: String,
    pub foliage: Foliage,
    /// How fast its litter rots, 0 to 1: how soon old pieces darken, break
    /// up and are eaten through.
    pub decay: f64,
    /// What falls with the foliage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<Piece>,
    pub look: LitterLook,
    /// A note per value, or per subtree of values with one basis.
    pub evidence: BTreeMap<String, FieldEvidence>,
    pub provenance: Provenance,
}

/// The foliage a species sheds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Foliage {
    pub kind: FoliageKind,
    /// Length, metres: a needle's, a spray's, or a leaf's from the base of
    /// its petiole to its tip.
    pub length_m: [f64; 2],
    /// Needles: their width, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_m: Option<f64>,
    /// Needles: how many a bundle holds (the pines' fascicles), 1 if single.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<u32>,
    /// Leaves: the shape a leaf seen whole needs where the crown's card is
    /// not true enough. The crown's own leaf when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// The colours it falls in, linear RGB, as they vary between pieces.
    pub fall_colours: Vec<[f64; 3]>,
    /// The colours it dries to on the ground.
    pub dry_colours: Vec<[f64; 3]>,
    /// How far its leaves cup and curl as they dry, 0 to 1.
    pub curl: f64,
    /// How much paler and greyer a leaf's underside is than its top, 0 to 1.
    pub pale_beneath: f64,
}

/// The kinds of foliage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoliageKind {
    /// Single needles, or bundles of them.
    Needles,
    /// Flat sprays of scale leaves, of the species' own spray look.
    Sprays,
    /// Leaves.
    Leaves,
}

/// Something that falls with the foliage: `per_m2` pieces a square metre
/// of litter, each `length_m` long and `aspect` as wide as long.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub kind: ExtraKind,
    pub length_m: [f64; 2],
    pub aspect: f64,
    /// Linear RGB.
    pub colour: [f64; 3],
    pub per_m2: f64,
}

/// The litter texture: its tile, its mean colour and its relief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LitterLook {
    /// Metres of ground the look covers before it repeats.
    pub tile_m: f32,
    /// Mean albedo, linear RGB reflectance: the drawing's own mean hue, at
    /// the brightness of the species' litter.
    pub mean: [f32; 3],
    /// Height of the highest texel over the lowest as drawn, metres.
    pub relief_m: f32,
}

impl Shed {
    /// Parse and check a section.
    ///
    /// # Errors
    ///
    /// Fails on malformed JSON or an invalid section.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let shed: Self =
            serde_json::from_str(text).map_err(|error| format!("invalid shed section: {error}"))?;
        shed.validate()?;
        Ok(shed)
    }

    /// Check the values, and that every value has an evidence note.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHED_SCHEMA {
            return Err(format!(
                "shed schema {} is not supported; expected {SHED_SCHEMA}",
                self.schema
            ));
        }
        self.foliage.validate()?;
        within("decay", self.decay, 0.0, 1.0)?;
        for (index, piece) in self.pieces.iter().enumerate() {
            let at = format!("pieces.{index}");
            range(&format!("{at}.length_m"), piece.length_m, 1.0)?;
            within(&format!("{at}.aspect"), piece.aspect, 0.01, 2.0)?;
            colour(&format!("{at}.colour"), piece.colour)?;
            within(&format!("{at}.per_m2"), piece.per_m2, 0.0, 1000.0)?;
        }
        within("look.tile_m", f64::from(self.look.tile_m), 0.1, 4.0)?;
        colour("look.mean", self.look.mean.map(f64::from))?;
        within("look.relief_m", f64::from(self.look.relief_m), 0.0, 0.2)?;
        if self.provenance.sources.is_empty() {
            return Err("provenance needs at least one source".to_string());
        }
        for (path, note) in &self.evidence {
            note.check()
                .map_err(|message| format!("the evidence note on `{path}`: {message}"))?;
        }
        let document = serde_json::to_value(self).map_err(|error| error.to_string())?;
        check_coverage(
            &document,
            &self.evidence,
            &["schema", "id", "evidence", "provenance"],
        )
    }
}

impl Foliage {
    fn validate(&self) -> Result<(), String> {
        range("foliage.length_m", self.length_m, 2.0)?;
        let needles = self.kind == FoliageKind::Needles;
        match (needles, self.width_m, self.bundle) {
            (true, Some(width), Some(bundle)) => {
                within("foliage.width_m", width, 0.0001, 0.05)?;
                within("foliage.bundle", f64::from(bundle), 1.0, 8.0)?;
            }
            (true, _, _) => return Err("needles need a width_m and a bundle".to_string()),
            (false, None, None) => {}
            (false, _, _) => {
                return Err("only needles have a width_m and a bundle".to_string());
            }
        }
        match (&self.shape, self.kind) {
            (Some(shape), FoliageKind::Leaves) => shape.validate()?,
            (Some(_), _) => return Err("only leaves have a shape of their own".to_string()),
            (None, _) => {}
        }
        for (what, colours) in [
            ("foliage.fall_colours", &self.fall_colours),
            ("foliage.dry_colours", &self.dry_colours),
        ] {
            if colours.is_empty() {
                return Err(format!("{what} needs at least one colour"));
            }
            for &value in colours {
                colour(what, value)?;
            }
        }
        within("foliage.curl", self.curl, 0.0, 1.0)?;
        within("foliage.pale_beneath", self.pale_beneath, 0.0, 1.0)
    }
}

fn within(what: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "shed {what} must be between {low} and {high}, found {value}"
        ))
    }
}

/// A `[shortest, longest]` length above 0 and at most `most` metres.
fn range(what: &str, [low, high]: [f64; 2], most: f64) -> Result<(), String> {
    if low > 0.0 && low <= high && high <= most {
        Ok(())
    } else {
        Err(format!(
            "shed {what} must run from above 0 up to at most {most} m, shortest first, found [{low}, {high}]"
        ))
    }
}

fn colour(what: &str, value: [f64; 3]) -> Result<(), String> {
    if value.iter().all(|channel| (0.0..=1.0).contains(channel)) {
        Ok(())
    } else {
        Err(format!(
            "shed {what} channels must be between 0 and 1, found {value:?}"
        ))
    }
}
