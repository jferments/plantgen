//! Where a species grows: the `niche` section of its record, `niche.json`
//! in its folder of the library.
//!
//! A [`Niche`] says which layers of a stand a species grows in and how
//! abundant it is there, and gives a range for each factor of its site
//! ([`SiteFactors`]): moisture, heat load and elevation as trapezoids, the
//! steepest ground it holds, how much shade it bears and, for species of
//! deep shade, how well it bears full sun. Since schema 2 it also gives a
//! range for each index of the climate it lives in ([`ClimateSummary`]):
//! the coldest and warmest months, the year's coldest night, its warmth as
//! degree-days above 0 and 5 °C, its moisture index and the share of its
//! rain that falls in summer, so a place grows only the species that could
//! live there. A host asks how well a place suits a species; where to put
//! plants is the host's own. Niches are ecology, not plant shape, so they
//! are kept out of the species specs and their packages: retuning a niche
//! moves plants without rebuilding a package. Every value carries an
//! evidence note.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::conditions::ClimateSummary;
use crate::evidence::{FieldEvidence, Provenance};
use crate::library::{self, LIBRARY};

/// Schema 2 added the climate ranges.
pub const NICHE_SCHEMA: u32 = 2;

/// The values every niche must give an evidence note for.
pub const EVIDENCE_FIELDS: [&str; 13] = [
    "layers",
    "moisture",
    "heat_load",
    "elevation_m",
    "max_slope_deg",
    "shade_tolerance",
    "coldest_c",
    "hard_freeze_c",
    "warmest_c",
    "degree_days_0",
    "degree_days_5",
    "moisture_index",
    "summer_rain",
];

/// Values a niche may leave out; each one it gives needs an evidence note.
pub const OPTIONAL_EVIDENCE_FIELDS: [&str; 1] = ["sun_tolerance"];

/// Share of the light scale shade tolerance moves the light a species
/// needs to grow fully: from full sun at tolerance 0 to a tenth of it at
/// tolerance 1.
const LIGHT_RANGE: f64 = 0.9;
/// How much less light than that it still grows in, fading to nothing.
const LIGHT_SPAN: f64 = 0.5;
/// Light above which a species that does not bear full sun starts to
/// fade, reaching its [`Niche::sun_tolerance`] in full sun.
const SUN_ONSET: f64 = 0.5;
/// Ground this much less steep than the steepest a species holds is not
/// thinned by slope at all.
const SLOPE_EASE: f64 = 0.75;

/// The layers of a stand that plants grow in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// Trees that reach the canopy.
    Canopy,
    /// Trees that live under it.
    Subcanopy,
    Shrub,
    /// Herbs and grasses.
    Herb,
}

impl Layer {
    /// Every layer, from the top down.
    pub const ALL: [Self; 4] = [Self::Canopy, Self::Subcanopy, Self::Shrub, Self::Herb];

    /// The layer's name, as niches spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Canopy => "canopy",
            Self::Subcanopy => "subcanopy",
            Self::Shrub => "shrub",
            Self::Herb => "herb",
        }
    }
}

/// The factors of a site a niche ranges over. Light is not one of them: it
/// changes as the stand grows ([`Niche::light_response`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteFactors {
    /// How wet the ground is, 0 (dry) to 1 (saturated).
    pub moisture: f64,
    /// Sunlight on the ground relative to level ground: above 1 facing
    /// the south-west (the north-west south of the equator), below 1
    /// facing away.
    pub heat_load: f64,
    /// Ground height above the geoid, metres.
    pub elevation_m: f64,
    pub slope_deg: f64,
}

/// A niche that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicheError(pub String);

impl fmt::Display for NicheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NicheError {}

/// Where one species grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Niche {
    pub schema: u32,
    /// The species' id, as in its plant spec.
    pub species: String,
    /// The layers it grows in, and its abundance in each relative to the
    /// other species there where all fit the site equally.
    pub layers: BTreeMap<Layer, f64>,
    /// Trapezoids `[a, b, c, d]`: nothing below `a`, rising to full at
    /// `b`, full to `c`, falling to nothing at `d`.
    pub moisture: [f64; 4],
    pub heat_load: [f64; 4],
    /// Ground height above the geoid, metres.
    pub elevation_m: [f64; 4],
    /// The steepest ground it holds, degrees.
    pub max_slope_deg: f64,
    /// 0 (needs full sun) to 1 (grows under a closed canopy).
    pub shade_tolerance: f64,
    /// How well a species of deep shade grows in full sun, 0 (not at all)
    /// to 1 (fully); leaving it out means 1. Forest herbs such as redwood
    /// sorrel scorch and dry out in a clearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun_tolerance: Option<f64>,
    /// Climate trapezoids over the [`ClimateSummary`] of the place at the
    /// plant's own height: the coldest month's mean temperature, °C...
    pub coldest_c: [f64; 4],
    /// ...the year's coldest night, °C...
    pub hard_freeze_c: [f64; 4],
    /// ...the warmest month's mean temperature, °C...
    pub warmest_c: [f64; 4],
    /// ...degree-days above 0 °C and above 5 °C, K·days...
    pub degree_days_0: [f64; 4],
    pub degree_days_5: [f64; 4],
    /// ...Prentice's moisture index, 0 to 1...
    pub moisture_index: [f64; 4],
    /// ...and the share of the year's precipitation in its warmest three
    /// months, 0 to 1.
    pub summer_rain: [f64; 4],
    /// Evidence for each value in [`EVIDENCE_FIELDS`] and each value of
    /// [`OPTIONAL_EVIDENCE_FIELDS`] it gives.
    pub evidence: BTreeMap<String, FieldEvidence>,
    pub provenance: Provenance,
}

impl Niche {
    /// Parse and validate a niche.
    ///
    /// # Errors
    ///
    /// Fails on malformed JSON or any problem [`Niche::validate`] finds.
    pub fn from_json(text: &str) -> Result<Self, NicheError> {
        let niche: Self = serde_json::from_str(text)
            .map_err(|error| NicheError(format!("invalid niche: {error}")))?;
        niche.validate()?;
        Ok(niche)
    }

    /// The built-in niche of a species: the `niche.json` in its folder of
    /// the built-in library.
    ///
    /// # Errors
    ///
    /// Fails if there is none.
    pub fn builtin(species: &str) -> Result<Self, NicheError> {
        let text = library::species(species)
            .and_then(|found| found.niche)
            .ok_or_else(|| {
                let mut known: Vec<&str> = LIBRARY
                    .iter()
                    .filter(|species| species.niche.is_some())
                    .map(|species| species.id)
                    .collect();
                known.sort_unstable();
                NicheError(format!(
                    "no built-in niche for `{species}`; built-in niches: {}",
                    known.join(", ")
                ))
            })?;
        Self::from_json(text)
    }

    /// Check the niche's values.
    ///
    /// # Errors
    ///
    /// Fails on an unknown schema, a malformed id, no layers, an abundance
    /// outside (0, 10], a range out of order or not finite, a slope outside
    /// (0, 90], a shade or sun tolerance outside [0, 1], a value without an
    /// evidence note, a note whose source is not written as a source id, or
    /// no provenance source. Whether a cited source exists is the
    /// library's check (`Library::check_citations`).
    pub fn validate(&self) -> Result<(), NicheError> {
        let species = &self.species;
        let fail = |message: String| Err(NicheError(format!("niche `{species}`: {message}")));
        if self.schema != NICHE_SCHEMA {
            return fail(format!(
                "schema {} is not supported; expected {NICHE_SCHEMA}",
                self.schema
            ));
        }
        let id_ok = !species.is_empty()
            && species.len() <= 64
            && species
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return fail("the species must be 1 to 64 lowercase letters, digits or hyphens".into());
        }
        if self.layers.is_empty() {
            return fail("it needs at least one layer".into());
        }
        for (layer, abundance) in &self.layers {
            if !(*abundance > 0.0 && *abundance <= 10.0) {
                return fail(format!(
                    "abundance in the {} layer must be above 0 and at most 10, found {abundance}",
                    layer.name()
                ));
            }
        }
        for (name, range) in [
            ("moisture", self.moisture),
            ("heat_load", self.heat_load),
            ("elevation_m", self.elevation_m),
            ("coldest_c", self.coldest_c),
            ("hard_freeze_c", self.hard_freeze_c),
            ("warmest_c", self.warmest_c),
            ("degree_days_0", self.degree_days_0),
            ("degree_days_5", self.degree_days_5),
            ("moisture_index", self.moisture_index),
            ("summer_rain", self.summer_rain),
        ] {
            if !range.iter().all(|value| value.is_finite()) || !range.is_sorted() {
                return fail(format!("{name} must be four finite values in order"));
            }
        }
        if !(self.max_slope_deg > 0.0 && self.max_slope_deg <= 90.0) {
            return fail("max_slope_deg must be above 0 and at most 90".into());
        }
        if !(0.0..=1.0).contains(&self.shade_tolerance) {
            return fail("shade_tolerance must be between 0 and 1".into());
        }
        if self
            .sun_tolerance
            .is_some_and(|tolerance| !(0.0..=1.0).contains(&tolerance))
        {
            return fail("sun_tolerance must be between 0 and 1".into());
        }
        let given = self.sun_tolerance.is_some().then_some("sun_tolerance");
        for field in EVIDENCE_FIELDS.into_iter().chain(given) {
            if self
                .evidence
                .get(field)
                .is_none_or(|evidence| evidence.note.trim().is_empty())
            {
                return fail(format!("{field} needs an evidence note"));
            }
        }
        if let Some(extra) = self
            .evidence
            .keys()
            .find(|key| !EVIDENCE_FIELDS.contains(&key.as_str()) && given != Some(key.as_str()))
        {
            return fail(format!("evidence for unknown value `{extra}`"));
        }
        for (field, note) in &self.evidence {
            if let Err(message) = note.check() {
                return fail(format!("the evidence note on `{field}`: {message}"));
            }
        }
        if self.provenance.sources.is_empty() {
            return fail("provenance needs at least one source".into());
        }
        Ok(())
    }

    /// Its abundance in a layer; 0 where it does not grow. A host's own
    /// layers convert: one that holds no plants has none.
    #[must_use]
    pub fn abundance(&self, layer: impl TryInto<Layer>) -> f64 {
        layer
            .try_into()
            .ok()
            .and_then(|layer| self.layers.get(&layer).copied())
            .unwrap_or(0.0)
    }

    /// How well the site suits it, 0 to 1: the product of its responses to
    /// moisture, heat load, elevation and slope. A host's own sites convert.
    #[must_use]
    pub fn site_response(&self, site: impl Into<SiteFactors>) -> f64 {
        let site = site.into();
        trapezoid(self.moisture, site.moisture)
            * trapezoid(self.heat_load, site.heat_load)
            * trapezoid(self.elevation_m, site.elevation_m)
            * (1.0
                - smoothstep(
                    SLOPE_EASE * self.max_slope_deg,
                    self.max_slope_deg,
                    site.slope_deg,
                ))
    }

    /// How well the climate suits it, 0 to 1: the product of its
    /// trapezoids over the climate's coldest month, coldest night,
    /// warmest month, degree-days above 0 and 5 °C, moisture index and
    /// summer rain. A host's own climate indices convert.
    #[must_use]
    pub fn climate_response(&self, climate: impl Into<ClimateSummary>) -> f64 {
        let climate = climate.into();
        trapezoid(self.coldest_c, climate.coldest_c)
            * trapezoid(self.hard_freeze_c, climate.hard_freeze_c)
            * trapezoid(self.warmest_c, climate.warmest_c)
            * trapezoid(self.degree_days_0, climate.degree_days_0)
            * trapezoid(self.degree_days_5, climate.degree_days_5)
            * trapezoid(self.moisture_index, climate.moisture_index)
            * trapezoid(self.summer_rain, climate.summer_rain)
    }

    /// How well it grows in `light` (1 is full sun), 0 to 1: rising to
    /// full at the light its shade tolerance needs and, for a species that
    /// does not bear full sun, falling from [`SUN_ONSET`] to its sun
    /// tolerance in full sun.
    #[must_use]
    pub fn light_response(&self, light: f64) -> f64 {
        let full = 1.0 - LIGHT_RANGE * self.shade_tolerance;
        let sun = self.sun_tolerance.unwrap_or(1.0);
        smoothstep((full - LIGHT_SPAN).max(0.0), full, light)
            * (1.0 - (1.0 - sun) * smoothstep(SUN_ONSET, 1.0, light))
    }
}

/// A trapezoid response to `value`, from `range = [a, b, c, d]`: 0 below
/// `a`, rising smoothly to 1 at `b`, 1 to `c`, falling smoothly to 0 at
/// `d`.
#[must_use]
pub fn trapezoid(range: [f64; 4], value: f64) -> f64 {
    let [rise, full, fade, gone] = range;
    smoothstep(rise, full, value) * (1.0 - smoothstep(fade, gone, value))
}

/// 0 below `low`, 1 above `high`, smooth (3t² − 2t³) between; a step at
/// `low` when the two meet.
fn smoothstep(low: f64, high: f64, x: f64) -> f64 {
    if high <= low {
        return if x < low { 0.0 } else { 1.0 };
    }
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[allow(clippy::float_cmp)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Every niche of the built-in library.
    fn builtin_niches() -> Vec<Niche> {
        LIBRARY
            .iter()
            .filter(|species| species.niche.is_some())
            .map(|species| Niche::builtin(species.id).unwrap())
            .collect()
    }

    #[test]
    fn built_in_niches_parse_and_name_their_species() {
        let niches = builtin_niches();
        assert!(!niches.is_empty());
        for niche in &niches {
            assert!(
                library::species(&niche.species).is_some(),
                "{}",
                niche.species
            );
        }
    }

    #[test]
    fn trapezoids_rise_hold_and_fall() {
        let range = [0.2, 0.4, 0.6, 0.8];
        assert_eq!(trapezoid(range, 0.1), 0.0);
        assert!((trapezoid(range, 0.3) - 0.5).abs() < 1e-12);
        assert_eq!(trapezoid(range, 0.5), 1.0);
        assert!((trapezoid(range, 0.7) - 0.5).abs() < 1e-12);
        assert_eq!(trapezoid(range, 0.9), 0.0);
        // Equal bounds are a step.
        assert_eq!(trapezoid([0.5, 0.5, 1.0, 1.0], 0.49), 0.0);
        assert_eq!(trapezoid([0.5, 0.5, 1.0, 1.0], 0.5), 1.0);
    }

    #[test]
    fn layers_are_spelled_as_niches_spell_them() {
        for layer in Layer::ALL {
            assert_eq!(
                serde_json::to_string(&layer).unwrap(),
                format!("\"{}\"", layer.name())
            );
        }
        assert!(Layer::ALL.is_sorted());
    }

    #[test]
    fn shade_tolerant_species_grow_in_less_light() {
        let cedar = Niche::builtin("thuja-plicata").unwrap();
        let fir = Niche::builtin("pseudotsuga-menziesii").unwrap();
        let grass = Niche::builtin("deschampsia-cespitosa").unwrap();
        // Under a closed canopy only the cedar comes up.
        assert!(cedar.light_response(0.15) > 0.5);
        assert_eq!(fir.light_response(0.15), 0.0);
        assert_eq!(grass.light_response(0.15), 0.0);
        // In the open everything does.
        for niche in [&cedar, &fir, &grass] {
            assert_eq!(niche.light_response(1.0), 1.0);
        }
    }

    #[test]
    fn deep_shade_herbs_fade_in_full_sun() {
        let sorrel = Niche::builtin("oxalis-oregana").unwrap();
        let sword_fern = Niche::builtin("polystichum-munitum").unwrap();
        let grass = Niche::builtin("deschampsia-cespitosa").unwrap();
        // Redwood sorrel grows fully in forest shade and in half light,
        // but only to its sun tolerance in a clearing.
        assert_eq!(sorrel.light_response(0.3), 1.0);
        assert_eq!(sorrel.light_response(0.5), 1.0);
        let tolerance = sorrel.sun_tolerance.unwrap();
        assert!(tolerance < 0.5);
        assert!((sorrel.light_response(1.0) - tolerance).abs() < 1e-12);
        let fading: Vec<f64> = (10..=20)
            .map(|tenth| sorrel.light_response(f64::from(tenth) / 20.0))
            .collect();
        assert!(fading.is_sorted_by(|a, b| a >= b), "{fading:?}");
        // Sword fern bears the sun better, and a grass of open ground fully.
        assert!(sword_fern.light_response(1.0) > sorrel.light_response(1.0));
        assert_eq!(grass.sun_tolerance, None);
        assert_eq!(grass.light_response(1.0), 1.0);
        // Only species of shade fade in the sun.
        for niche in builtin_niches() {
            if niche.sun_tolerance.is_some() {
                assert!(niche.shade_tolerance >= 0.7, "{}", niche.species);
            }
        }
    }

    #[test]
    fn responses_take_the_factors_and_the_summary() {
        let fir = Niche::builtin("pseudotsuga-menziesii").unwrap();
        let middle = |range: [f64; 4]| (range[1] + range[2]) / 2.0;
        let site = SiteFactors {
            moisture: middle(fir.moisture),
            heat_load: middle(fir.heat_load),
            elevation_m: middle(fir.elevation_m),
            slope_deg: 0.0,
        };
        assert_eq!(fir.site_response(site), 1.0);
        let too_steep = SiteFactors {
            slope_deg: fir.max_slope_deg,
            ..site
        };
        assert_eq!(fir.site_response(too_steep), 0.0);
        let climate = ClimateSummary {
            warmest_c: middle(fir.warmest_c),
            coldest_c: middle(fir.coldest_c),
            annual_mm: 1_300.0,
            pet_mm: 700.0,
            aridity: 1.9,
            degree_days_0: middle(fir.degree_days_0),
            degree_days_5: middle(fir.degree_days_5),
            moisture_index: middle(fir.moisture_index),
            summer_rain: middle(fir.summer_rain),
            hard_freeze_c: middle(fir.hard_freeze_c),
        };
        assert_eq!(fir.climate_response(climate), 1.0);
        let frozen = ClimateSummary {
            warmest_c: fir.warmest_c[0] - 1.0,
            ..climate
        };
        assert_eq!(fir.climate_response(frozen), 0.0);
        assert!(fir.abundance(Layer::Canopy) > 0.0);
        assert_eq!(fir.abundance(Layer::Herb), 0.0);
    }

    #[test]
    fn invalid_niches_are_refused() {
        let good = Niche::builtin("thuja-plicata").unwrap();
        let refused = |change: &dyn Fn(&mut Niche), expect: &str| {
            let mut niche = good.clone();
            change(&mut niche);
            let error = niche.validate().unwrap_err();
            assert!(error.0.contains(expect), "{error} lacks {expect}");
        };
        refused(&|n| n.schema = 1, "schema");
        refused(&|n| n.species = "Thuja".into(), "lowercase");
        refused(&|n| n.layers.clear(), "layer");
        refused(&|n| _ = n.layers.insert(Layer::Herb, 0.0), "abundance");
        refused(&|n| n.moisture = [0.5, 0.4, 0.6, 0.7], "moisture");
        refused(&|n| n.elevation_m[3] = f64::NAN, "elevation_m");
        refused(&|n| n.max_slope_deg = 0.0, "max_slope_deg");
        refused(&|n| n.shade_tolerance = 1.5, "shade_tolerance");
        refused(&|n| n.sun_tolerance = Some(-0.1), "sun_tolerance");
        // A sun tolerance needs its own evidence note.
        refused(
            &|n| n.sun_tolerance = Some(0.5),
            "sun_tolerance needs an evidence note",
        );
        refused(&|n| _ = n.evidence.remove("heat_load"), "heat_load");
        refused(&|n| n.warmest_c = [10.0, 9.0, 20.0, 25.0], "warmest_c");
        refused(&|n| n.summer_rain[0] = f64::INFINITY, "summer_rain");
        refused(&|n| _ = n.evidence.remove("hard_freeze_c"), "hard_freeze_c");
        refused(
            &|n| {
                let note = n.evidence["layers"].clone();
                n.evidence.insert("sun_tolerance".into(), note);
            },
            "sun_tolerance",
        );
        refused(
            &|n| {
                let note = n.evidence["layers"].clone();
                n.evidence.insert("colour".into(), note);
            },
            "colour",
        );
        refused(
            &|n| n.evidence.get_mut("moisture").unwrap().source = Some("Not An Id".into()),
            "`moisture`",
        );
        refused(&|n| n.provenance.sources.clear(), "source");
        // The ground holds no plants: no niche names it.
        let text = serde_json::to_string(&good)
            .unwrap()
            .replace("\"canopy\"", "\"ground\"");
        assert!(Niche::from_json(&text).is_err());
        assert!(Niche::from_json("{}").is_err());
        assert!(Niche::builtin("no-such-species").is_err());
    }
}
