//! Growing conditions: the world a plant grows in, as Plantgen JSON.
//!
//! A [`Conditions`] document generalises the synthetic stand a variant
//! grows in ([`Neighbourhood`]) into sections, each optional and each with
//! its own `version`: `neighbours` (today's stand), `light`, `climate`,
//! `soil`, `disturbance` and `interactions`. Only `neighbours` changes how
//! a plant grows today, through `light@1`; the others are data that later
//! tools read, so they change nothing yet, except `substrate`, which
//! `substrate@1` and `light@2` read (G3). The four environments are named
//! presets of the document ([`Conditions::preset`]), and a preset expands
//! to exactly the numbers `Environment::neighbourhood` gives, so every
//! package keeps its bytes.
//!
//! The growth tools read the world through [`Surroundings`]: how much sky
//! light reaches a point past everything that is not the plant, whether a
//! point is taken, and the host's wood.
//!
//! Two documents are the same condition class when their resolved forms
//! ([`Conditions::resolve`]) write the same bytes; the class key
//! ([`Conditions::class`]) is the SHA-256 of those bytes, written with the
//! exact-number JSON writer packages use, so it is the same on every
//! machine.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::json;
use crate::lsys::tools::{Host, Neighbourhood};
use crate::math::Vec3;
use crate::spec::{Environment, HostSpec};
use crate::substrate::{Substrate, SubstrateField};

/// The document's schema.
pub const CONDITIONS_SCHEMA: u32 = 1;

/// The one version every section has so far.
pub const SECTION_VERSION: u32 = 1;

/// What a growing plant can sense of the world outside itself.
pub trait Surroundings {
    /// Share of overcast sky light along `direction` (a unit vector up
    /// toward the sky) that reaches `point` past everything that is not
    /// this plant, for a plant `height` metres tall.
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64;

    /// Whether `point` is taken by something else, such as another plant's
    /// crown in a stand. No released tool reads it yet.
    fn occupied(&self, _point: Vec3) -> bool {
        false
    }

    /// The host's wood, for `host@1`.
    fn host(&self) -> Option<&Host> {
        None
    }

    /// What the plant grows on, for `substrate@1` and `light@2`; `None`
    /// is level soil at height 0.
    fn substrate(&self) -> Option<&SubstrateField> {
        None
    }
}

impl Surroundings for Neighbourhood {
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        Neighbourhood::transmission(self, point, height, direction)
    }
}

/// A neighbourhood and a host's wood: what growth hands the tools.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    pub neighbourhood: &'a Neighbourhood,
    pub host: Option<&'a Host>,
    pub substrate: Option<&'a SubstrateField>,
}

impl Surroundings for Around<'_> {
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        self.neighbourhood.transmission(point, height, direction)
    }

    fn host(&self) -> Option<&Host> {
        self.host
    }

    fn substrate(&self) -> Option<&SubstrateField> {
        self.substrate
    }
}

/// The conditions a plant grows in. Every section is optional; a section
/// left out is not known, and the plant grows as it would without it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    pub schema: u32,
    /// A named stand that fills `neighbours` when that is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<Environment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighbours: Option<Neighbours>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub climate: Option<Climate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soil: Option<Soil>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disturbance: Option<Disturbance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactions: Option<Interactions>,
    /// What the plant grows on (G3): a preset or a distance-field grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substrate: Option<Substrate>,
}

/// The stand round the plant: today's [`Neighbourhood`], with the same
/// meanings, and room for explicit neighbour crowns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Neighbours {
    pub version: u32,
    /// Shade per metre of neighbouring canopy above a point, 1/m.
    pub density: f64,
    /// Top of the neighbouring canopy as a multiple of the plant's height.
    pub relative_height: f64,
    /// Lowest top of the neighbouring canopy, metres.
    pub canopy: f64,
    /// Distance between stems, metres.
    pub spacing: f64,
    /// Neighbours stand only on the −X side.
    pub one_sided: bool,
    /// Explicit neighbour crowns. Read from `Stand` (`PlantGen` PG4) on;
    /// until then they must be left empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelopes: Vec<Envelope>,
}

/// One neighbour's crown: an upright cylinder of foliage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The crown's centre on the ground, metres east and north of the plant.
    pub centre_m: [f64; 2],
    pub base_m: f64,
    pub top_m: f64,
    pub radius_m: f64,
    /// Shade per metre of this crown, 1/m.
    pub density: f64,
}

impl Neighbours {
    /// The section holding `neighbourhood`'s numbers.
    #[must_use]
    pub fn of(neighbourhood: Neighbourhood) -> Self {
        Self {
            version: SECTION_VERSION,
            density: neighbourhood.density,
            relative_height: neighbourhood.relative_height,
            canopy: neighbourhood.canopy,
            spacing: neighbourhood.spacing,
            one_sided: neighbourhood.one_sided,
            envelopes: Vec::new(),
        }
    }

    /// Its stand as `light@1` reads it.
    #[must_use]
    pub fn neighbourhood(&self) -> Neighbourhood {
        Neighbourhood {
            density: self.density,
            relative_height: self.relative_height,
            canopy: self.canopy,
            spacing: self.spacing,
            one_sided: self.one_sided,
        }
    }
}

/// The light above the stand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Light {
    pub version: u32,
    pub latitude_deg: f64,
    pub sky: Sky,
}

/// The sky's light: today only `light@1`'s uniform overcast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sky {
    Overcast,
}

/// The climate a plant grows through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Climate {
    pub version: u32,
    /// One year per year of growth; a single year repeats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub years: Vec<ClimateYear>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<ClimateSummary>,
}

/// One year of growth's weather, as growth will read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateYear {
    /// Days of the growing season.
    pub season_days: f64,
    pub mean_c: f64,
    /// Precipitation less potential evapotranspiration, mm.
    pub water_balance_mm: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ClimateEvent>,
}

/// What can happen in a year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateEvent {
    Drought,
    LateFrost,
    Mast,
}

/// A climate's indices: the ten a niche ranges over seven of
/// ([`crate::niche::Niche::climate_response`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateSummary {
    /// Mean temperature of the warmest month, °C.
    pub warmest_c: f64,
    /// Mean temperature of the coldest month, °C.
    pub coldest_c: f64,
    /// Precipitation over the year, mm.
    pub annual_mm: f64,
    /// Potential evapotranspiration over the year, mm.
    pub pet_mm: f64,
    /// `annual_mm / pet_mm`.
    pub aridity: f64,
    /// Degree-days above 0 °C and above 5 °C over the year.
    pub degree_days_0: f64,
    pub degree_days_5: f64,
    /// Actual over potential evapotranspiration, 0 to 1.
    pub moisture_index: f64,
    /// Share of the year's precipitation in its warmest three months.
    pub summer_rain: f64,
    /// The year's coldest night, °C.
    pub hard_freeze_c: f64,
}

/// The ground the plant roots in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Soil {
    pub version: u32,
    pub depth_m: f64,
    pub water_capacity_mm: f64,
    pub nutrients: Nutrients,
    pub ph: f64,
}

/// How rich the soil is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nutrients {
    Poor,
    Moderate,
    Rich,
}

/// What breaks, burns or eats the plant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disturbance {
    pub version: u32,
    /// Years between fires; none when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire_return_years: Option<f64>,
    /// Share of limbs wind breaks, 0 to 1.
    #[serde(default)]
    pub wind_breakage: f64,
    #[serde(default)]
    pub snow_load_kpa: f64,
    #[serde(default)]
    pub browse: Browse,
}

/// Browsing animals: how high they reach and how hard they browse.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Browse {
    pub height_m: f64,
    /// 0 to 1.
    pub intensity: f64,
}

/// Other living things.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interactions {
    pub version: u32,
    /// The plant a climber, epiphyte or parasite grows on: a spec's `host`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<HostSpec>,
    /// Share of flowers set, 0 to 1.
    #[serde(default = "one")]
    pub pollination: f64,
    /// Share of leaf area eaten, 0 to 1.
    #[serde(default)]
    pub herbivory: f64,
}

fn one() -> f64 {
    1.0
}

/// A condition class: the SHA-256 of a resolved document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassKey(pub [u8; 32]);

impl fmt::Display for ClassKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Conditions {
    /// The conditions of a named stand, nothing else known.
    #[must_use]
    pub fn preset(environment: Environment) -> Self {
        Self {
            schema: CONDITIONS_SCHEMA,
            preset: Some(environment),
            neighbours: None,
            light: None,
            climate: None,
            soil: None,
            disturbance: None,
            interactions: None,
            substrate: None,
        }
    }

    /// A named stand with its neighbours replaced by `neighbourhood`.
    #[must_use]
    pub fn in_neighbourhood(environment: Environment, neighbourhood: Neighbourhood) -> Self {
        Self {
            neighbours: Some(Neighbours::of(neighbourhood)),
            ..Self::preset(environment)
        }
    }

    /// The stand `light@1` reads: the `neighbours` section, else the
    /// preset's, else open ground.
    #[must_use]
    pub fn neighbourhood(&self) -> Neighbourhood {
        match (&self.neighbours, self.preset) {
            (Some(neighbours), _) => neighbours.neighbourhood(),
            (None, Some(preset)) => preset.neighbourhood(),
            (None, None) => Neighbourhood::OPEN,
        }
    }

    /// The document with its preset expanded into `neighbours` and left
    /// out, and a substrate preset expanded into its grid, so documents
    /// that grow alike resolve alike.
    #[must_use]
    pub fn resolve(&self) -> Self {
        Self {
            preset: None,
            neighbours: Some(Neighbours::of(self.neighbourhood())),
            substrate: self.substrate.as_ref().map(Substrate::resolve),
            ..self.clone()
        }
    }

    /// The document's condition class.
    ///
    /// # Panics
    ///
    /// Panics if the resolved document has a number JSON cannot write,
    /// which a validated one has not.
    #[must_use]
    pub fn class(&self) -> ClassKey {
        let bytes = json::to_vec(&self.resolve()).expect("a validated document writes");
        ClassKey(Sha256::digest(bytes).into())
    }

    /// Parse and check a document.
    ///
    /// # Errors
    ///
    /// Fails on malformed JSON or an invalid document.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let conditions: Self =
            serde_json::from_str(text).map_err(|error| format!("invalid conditions: {error}"))?;
        conditions.validate()?;
        Ok(conditions)
    }

    /// Check the schema, every section's version and its values.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONDITIONS_SCHEMA {
            return Err(format!(
                "conditions schema {} is not supported; expected {CONDITIONS_SCHEMA}",
                self.schema
            ));
        }
        if let Some(neighbours) = &self.neighbours {
            neighbours.validate()?;
        }
        if let Some(light) = &self.light {
            version("light", light.version)?;
            within("light latitude_deg", light.latitude_deg, -90.0, 90.0)?;
        }
        if let Some(climate) = &self.climate {
            climate.validate()?;
        }
        if let Some(soil) = &self.soil {
            version("soil", soil.version)?;
            within("soil depth_m", soil.depth_m, 0.0, 50.0)?;
            within(
                "soil water_capacity_mm",
                soil.water_capacity_mm,
                0.0,
                2000.0,
            )?;
            within("soil ph", soil.ph, 2.0, 12.0)?;
        }
        if let Some(disturbance) = &self.disturbance {
            disturbance.validate()?;
        }
        if let Some(interactions) = &self.interactions {
            version("interactions", interactions.version)?;
            within(
                "interactions pollination",
                interactions.pollination,
                0.0,
                1.0,
            )?;
            within("interactions herbivory", interactions.herbivory, 0.0, 1.0)?;
        }
        if let Some(substrate) = &self.substrate {
            substrate.validate()?;
        }
        Ok(())
    }
}

impl Neighbours {
    fn validate(&self) -> Result<(), String> {
        version("neighbours", self.version)?;
        within("neighbours density", self.density, 0.0, 10.0)?;
        within(
            "neighbours relative_height",
            self.relative_height,
            0.0,
            10.0,
        )?;
        within("neighbours canopy", self.canopy, 0.0, 150.0)?;
        within("neighbours spacing", self.spacing, 0.0, 100.0)?;
        if self.envelopes.is_empty() {
            Ok(())
        } else {
            Err("neighbours envelopes are read from Stand on; leave them empty".to_string())
        }
    }
}

impl Climate {
    fn validate(&self) -> Result<(), String> {
        version("climate", self.version)?;
        for year in &self.years {
            within("climate season_days", year.season_days, 0.0, 366.0)?;
            within("climate mean_c", year.mean_c, -60.0, 60.0)?;
            within(
                "climate water_balance_mm",
                year.water_balance_mm,
                -10000.0,
                10000.0,
            )?;
        }
        let Some(summary) = &self.summary else {
            return Ok(());
        };
        for (what, value, low, high) in [
            ("warmest_c", summary.warmest_c, -60.0, 60.0),
            ("coldest_c", summary.coldest_c, -80.0, 50.0),
            ("annual_mm", summary.annual_mm, 0.0, 15000.0),
            ("pet_mm", summary.pet_mm, 0.0, 5000.0),
            ("aridity", summary.aridity, 0.0, 100.0),
            ("degree_days_0", summary.degree_days_0, 0.0, 20000.0),
            ("degree_days_5", summary.degree_days_5, 0.0, 20000.0),
            ("moisture_index", summary.moisture_index, 0.0, 1.0),
            ("summer_rain", summary.summer_rain, 0.0, 1.0),
            ("hard_freeze_c", summary.hard_freeze_c, -90.0, 40.0),
        ] {
            within(&format!("climate summary {what}"), value, low, high)?;
        }
        Ok(())
    }
}

impl Disturbance {
    fn validate(&self) -> Result<(), String> {
        version("disturbance", self.version)?;
        if let Some(years) = self.fire_return_years {
            within("disturbance fire_return_years", years, 1.0, 10000.0)?;
        }
        within("disturbance wind_breakage", self.wind_breakage, 0.0, 1.0)?;
        within("disturbance snow_load_kpa", self.snow_load_kpa, 0.0, 50.0)?;
        within(
            "disturbance browse height_m",
            self.browse.height_m,
            0.0,
            10.0,
        )?;
        within(
            "disturbance browse intensity",
            self.browse.intensity,
            0.0,
            1.0,
        )
    }
}

impl Surroundings for Conditions {
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        self.neighbourhood().transmission(point, height, direction)
    }
}

fn version(section: &str, found: u32) -> Result<(), String> {
    if found == SECTION_VERSION {
        Ok(())
    } else {
        Err(format!(
            "{section} version {found} is not known; this PlantGen reads version {SECTION_VERSION}"
        ))
    }
}

fn within(what: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be between {low} and {high}, found {value}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Conditions, Surroundings};
    use crate::lsys::tools::Neighbourhood;
    use crate::math::Vec3;
    use crate::spec::Environment;

    /// Each preset is exactly today's stand, so every package keeps its
    /// bytes.
    #[test]
    fn presets_are_todays_neighbourhoods() {
        for environment in Environment::ALL {
            let conditions = Conditions::preset(environment);
            assert_eq!(conditions.neighbourhood(), environment.neighbourhood());
            conditions.validate().unwrap();
            let point = Vec3::new(1.5, 2.0, -0.5);
            let direction = Vec3::new(0.3, 0.9, 0.3);
            assert_eq!(
                conditions.transmission(point, 12.0, direction).to_bits(),
                environment
                    .neighbourhood()
                    .transmission(point, 12.0, direction)
                    .to_bits()
            );
        }
        assert_eq!(
            Conditions {
                preset: None,
                ..Conditions::preset(Environment::Open)
            }
            .neighbourhood(),
            Neighbourhood::OPEN
        );
    }

    #[test]
    fn documents_that_grow_alike_share_a_class() {
        let interior = Conditions::preset(Environment::Interior);
        let spelled_out =
            Conditions::in_neighbourhood(Environment::Edge, Environment::Interior.neighbourhood());
        assert_eq!(interior.class(), spelled_out.class());
        assert_ne!(
            interior.class(),
            Conditions::preset(Environment::Edge).class()
        );
        let text = r#"{"schema": 1, "preset": "interior",
            "soil": {"version": 1, "depth_m": 1.2, "water_capacity_mm": 180,
                     "nutrients": "moderate", "ph": 5.6}}"#;
        let with_soil = Conditions::from_json(text).unwrap();
        assert_ne!(with_soil.class(), interior.class());
        assert_eq!(
            with_soil.class(),
            Conditions::from_json(text).unwrap().class()
        );
        // The key is the same on every run and machine: the SHA-256 of
        // {"schema":1,"neighbours":{"version":1,"density":0.12,
        // "relative_height":1.0,"canopy":0.0,"spacing":5.0,"one_sided":false}}.
        assert_eq!(
            interior.class().to_string(),
            "2ffbeb9dd5ed42be0d179e8bb53e76065b7baf7d9404b91c371402947dd706aa"
        );
    }

    #[test]
    fn invalid_documents_are_refused() {
        let refused = |text: &str, expected: &str| {
            let message = Conditions::from_json(text).unwrap_err();
            assert!(message.contains(expected), "{message}");
        };
        refused(r#"{"schema": 2}"#, "conditions schema 2 is not supported");
        refused(r#"{"schema": 1, "preset": "meadow"}"#, "invalid conditions");
        refused(r#"{"schema": 1, "colour": 3}"#, "unknown field `colour`");
        refused(
            r#"{"schema": 1, "light": {"version": 2, "latitude_deg": 47, "sky": "overcast"}}"#,
            "light version 2 is not known",
        );
        refused(
            r#"{"schema": 1, "light": {"version": 1, "latitude_deg": 97, "sky": "overcast"}}"#,
            "light latitude_deg must be between -90 and 90, found 97",
        );
        refused(
            r#"{"schema": 1, "neighbours": {"version": 1, "density": 0.1, "relative_height": 1,
                "canopy": 0, "spacing": 5, "one_sided": false,
                "envelopes": [{"centre_m": [3, 0], "base_m": 2, "top_m": 10,
                               "radius_m": 2, "density": 0.5}]}}"#,
            "envelopes are read from Stand on",
        );
        refused(
            r#"{"schema": 1, "interactions": {"version": 1, "herbivory": 1.5}}"#,
            "interactions herbivory must be between 0 and 1, found 1.5",
        );
    }

    #[test]
    fn a_substrate_preset_resolves_into_its_grid() {
        let text = r#"{"schema": 1, "preset": "open",
            "substrate": {"version": 1, "preset": "boulders", "seed": 3}}"#;
        let document = Conditions::from_json(text).unwrap();
        let resolved = document.resolve();
        let grid = resolved.substrate.as_ref().unwrap();
        assert!(grid.preset.is_none() && grid.distance.is_some());
        // The grid stands for the preset: spelling it out keeps the class.
        let spelled_out = Conditions {
            substrate: Some(grid.clone()),
            ..document.clone()
        };
        assert_eq!(spelled_out.class(), document.class());
        assert_ne!(
            document.class(),
            Conditions::preset(Environment::Open).class()
        );
        let other_seed =
            Conditions::from_json(&text.replace("\"seed\": 3", "\"seed\": 4")).unwrap();
        assert_ne!(other_seed.class(), document.class());
        assert!(
            Conditions::from_json(
                r#"{"schema": 1, "substrate": {"version": 2, "preset": "flat"}}"#
            )
            .unwrap_err()
            .contains("substrate version 2")
        );
    }
}
