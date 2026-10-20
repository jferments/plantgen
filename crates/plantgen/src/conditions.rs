//! Growing conditions: the world a plant grows in, as Plantgen JSON.
//!
//! A [`Conditions`] document generalises the synthetic stand a variant
//! grows in ([`Neighbourhood`]) into sections, each optional and each with
//! its own `version`: `neighbours` (today's stand, neighbours' crowns as
//! `envelopes`, and neighbours like the plant itself as a `layout`), `light`, `climate`, `soil`, `disturbance` and
//! `interactions`. Only `neighbours` changes how a plant grows today,
//! through the light tools and space colonization; the others are data that later
//! tools read, so they change nothing yet, except `substrate`, which
//! `substrate@1` and `light@2` read (G3), and a `light` section with a
//! `sun_and_sky` sky, which `light@3` reads. The four environments are named
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

use crate::graph::PlantGraph;
use crate::json;
use crate::lsys::tools::{Host, Neighbourhood};
use crate::math::{self, Vec3};
use crate::spec::{Environment, HostSpec};
use crate::substrate::{Substrate, SubstrateField};

/// The document's schema.
pub const CONDITIONS_SCHEMA: u32 = 1;

/// The one version every section has so far.
pub const SECTION_VERSION: u32 = 1;

/// What a growing plant can sense of the world outside itself. The light
/// tool asks from several threads at once.
pub trait Surroundings: Sync {
    /// Share of overcast sky light along `direction` (a unit vector up
    /// toward the sky) that reaches `point` past everything that is not
    /// this plant, for a plant `height` metres tall.
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64;

    /// Whether nothing but this plant shades the sky: `transmission` is 1
    /// everywhere.
    fn clear_sky(&self) -> bool {
        false
    }

    /// Whether `point` is taken by something else, such as another plant's
    /// crown in a stand. Space colonization draws no attraction point there.
    fn occupied(&self, _point: Vec3) -> bool {
        false
    }

    /// Whether any point may be taken (`occupied`).
    fn crowded(&self) -> bool {
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

    /// The latitude in degrees whose sun `light@3` adds to the sky; `None`
    /// under an overcast sky.
    fn sun_latitude(&self) -> Option<f64> {
        None
    }
}

impl Surroundings for Neighbourhood {
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        Neighbourhood::transmission(self, point, height, direction)
    }

    fn clear_sky(&self) -> bool {
        self.density <= 0.0
    }
}

/// A neighbourhood and a host's wood: what growth hands the tools.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    pub neighbourhood: &'a Neighbourhood,
    /// Neighbours' crowns, which shade the plant and take their room.
    pub envelopes: &'a [Envelope],
    pub host: Option<&'a Host>,
    pub substrate: Option<&'a SubstrateField>,
    /// The latitude of the sun, degrees, when the sky has one.
    pub sun: Option<f64>,
}

impl Surroundings for Around<'_> {
    fn transmission(&self, point: Vec3, height: f64, direction: Vec3) -> f64 {
        self.envelopes.iter().fold(
            self.neighbourhood.transmission(point, height, direction),
            |light, envelope| light * envelope.transmission(point, direction),
        )
    }

    fn clear_sky(&self) -> bool {
        self.neighbourhood.density <= 0.0 && self.envelopes.is_empty()
    }

    fn occupied(&self, point: Vec3) -> bool {
        self.envelopes
            .iter()
            .any(|envelope| envelope.contains(point))
    }

    fn crowded(&self) -> bool {
        !self.envelopes.is_empty()
    }

    fn host(&self) -> Option<&Host> {
        self.host
    }

    fn substrate(&self) -> Option<&SubstrateField> {
        self.substrate
    }

    fn sun_latitude(&self) -> Option<f64> {
        self.sun
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
    /// A stand of the plant's like whose stems stand this many times its
    /// height apart, at least `spacing`; 0: none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub like: f64,
    /// Neighbours' crowns, besides the stand above: each shades the light
    /// that passes through it and takes its room, so no attraction point
    /// of space colonization lies inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelopes: Vec<Envelope>,
    /// Neighbours like the plant itself, standing round it in a layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
}

/// Neighbours like the plant itself in one of the crown-gap layouts agreed
/// with After (2026-10-13): after each step the plant's own crown
/// ([`Crown`]) stands at every neighbour's place, and from the next step on
/// shades the plant and takes its room as a neighbour's crown would. The
/// plant grows as the middle one of a stand of its like, each making room
/// for the others, without growing them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub kind: LayoutKind,
    /// Distance between neighbouring stems, metres: the least, when
    /// `per_height` sets it.
    pub spacing_m: f64,
    /// Distance between neighbouring stems as a share of the plant's
    /// height, so the stand thins as it grows; 0: `spacing_m` always.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub per_height: f64,
}

/// Where a layout's neighbours stand, on a square grid of its spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    /// At a stand's edge: neighbours on the −X (west) side, the crown free
    /// toward +X.
    Edge,
    /// Inside a stand: neighbours all round.
    Interior,
    /// In a row (a hedgerow or a street): neighbours east and west, free
    /// along Z.
    Row,
}

impl Layout {
    /// The most rings of neighbours round the plant a layout places.
    pub const RINGS: i32 = 3;

    /// The least distance between stems a layout takes, metres.
    pub const LEAST_SPACING_M: f64 = 0.25;

    /// The neighbours' crowns at `age_years` when each is `crown`, the
    /// plant's own, as envelopes in the plant's frame: every one within
    /// [`Self::RINGS`] rings that can shade the plant or reach its crown
    /// ([`Crown::reaches`]), in [`sort_envelopes`]' order. A stand spaced
    /// by height opens with age as old-growth stands do
    /// ([`crate::spec::stand_spacing`]).
    #[must_use]
    pub fn envelopes(&self, crown: &Crown, age_years: f64) -> Vec<Envelope> {
        let rings = -Self::RINGS..=Self::RINGS;
        let opening = crate::spec::stand_spacing(age_years) / crate::spec::STAND_SPACING;
        let spacing_m = self.spacing_m.max(self.per_height * opening * crown.top_m);
        let mut envelopes = Vec::new();
        for east in rings.clone() {
            for north in rings.clone() {
                let placed = match self.kind {
                    LayoutKind::Edge => east < 0,
                    LayoutKind::Interior => east != 0 || north != 0,
                    LayoutKind::Row => east != 0 && north == 0,
                };
                let centre = [f64::from(east) * spacing_m, f64::from(north) * spacing_m];
                if placed && crown.reaches(centre, crown.radius_m) {
                    envelopes.push(crown.envelope(centre));
                }
            }
        }
        sort_envelopes(&mut envelopes);
        envelopes
    }

    fn validate(&self) -> Result<(), String> {
        within(
            "layout spacing_m",
            self.spacing_m,
            Self::LEAST_SPACING_M,
            100.0,
        )?;
        within("layout per_height", self.per_height, 0.0, 10.0)
    }
}

/// Put neighbours' crowns in one order, whatever order they were found in:
/// the shade through them multiplies in it.
pub fn sort_envelopes(envelopes: &mut [Envelope]) {
    envelopes.sort_by(|a, b| {
        let key = |e: &Envelope| [e.centre_m[0], e.centre_m[1], e.base_m, e.top_m, e.radius_m];
        key(a)
            .iter()
            .zip(key(b).iter())
            .map(|(x, y)| x.total_cmp(y))
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.density.total_cmp(&b.density))
    });
}

/// One neighbour's crown: an upright cylinder of foliage. North is −Z in
/// the plant's frame and east +X.
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

/// A plant's crown as its neighbours see it: an upright cylinder round its
/// foot holding its leaf area, for plants that grow together in a stand
/// (`crate::stand`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crown {
    /// The height below which one twentieth of its leaves stand, and its
    /// highest leaf, metres.
    pub base_m: f64,
    pub top_m: f64,
    /// The distance from its axis within which nine tenths of its leaves
    /// stand, metres.
    pub radius_m: f64,
    /// Its leaves' one-sided area, m².
    pub leaf_area_m2: f64,
}

impl Crown {
    /// How far past a plant's crown a neighbour's crown can shade it: twice
    /// the neighbour's height, light from 27° above the horizon and higher.
    pub const SHADE_REACH: f64 = 2.0;

    /// The extinction coefficient of foliage with leaves at every angle
    /// (a spherical leaf angle distribution): the shade per metre of a
    /// crown is this times its leaf area per cubic metre.
    pub const EXTINCTION: f64 = 0.5;

    /// The crown of leaves at `points` (each with its area, m²), relative
    /// to the plant's foot; `None` when none has any area.
    #[must_use]
    pub fn of(points: impl Iterator<Item = (Vec3, f64)>) -> Option<Self> {
        let leaves: Vec<(Vec3, f64)> = points.filter(|(_, area)| *area > 0.0).collect();
        if leaves.is_empty() {
            return None;
        }
        let quantile = |mut values: Vec<f64>, share: f64| {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let at = ((values.len() - 1) as f64 * share).round() as usize;
            *values.select_nth_unstable_by(at, f64::total_cmp).1
        };
        let radial = leaves
            .iter()
            .map(|(point, _)| math::sqrt(point.x * point.x + point.z * point.z))
            .collect();
        let heights: Vec<f64> = leaves.iter().map(|(point, _)| point.y).collect();
        let top = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let base = quantile(heights, 0.05).max(0.0).min(top);
        Some(Self {
            base_m: base,
            top_m: top.max(base + 0.01),
            radius_m: quantile(radial, 0.9).max(0.05),
            leaf_area_m2: leaves.iter().map(|(_, area)| area).sum(),
        })
    }

    /// Whether this crown, standing `centre_m` (east, north) from a plant
    /// whose crown reaches `reach_m` from its axis, can shade that plant or
    /// reach its crown ([`Self::SHADE_REACH`]).
    #[must_use]
    pub fn reaches(&self, centre_m: [f64; 2], reach_m: f64) -> bool {
        let apart = math::sqrt(centre_m[0] * centre_m[0] + centre_m[1] * centre_m[1]);
        apart - self.radius_m - reach_m < Self::SHADE_REACH * self.top_m
    }

    /// The crown as an envelope centred `centre_m` (east, north) from the
    /// plant it shades, its density from its leaf area.
    #[must_use]
    pub fn envelope(&self, centre_m: [f64; 2]) -> Envelope {
        let volume = math::PI * self.radius_m * self.radius_m * (self.top_m - self.base_m);
        Envelope {
            centre_m,
            base_m: self.base_m,
            top_m: self.top_m,
            radius_m: self.radius_m,
            density: (Self::EXTINCTION * self.leaf_area_m2 / volume).min(10.0),
        }
    }
}

impl Envelope {
    /// Where its axis stands in the plant's frame, `(x, z)`.
    fn axis(&self) -> (f64, f64) {
        (self.centre_m[0], -self.centre_m[1])
    }

    /// Whether `point` is inside the crown.
    #[must_use]
    pub fn contains(&self, point: Vec3) -> bool {
        let (x, z) = self.axis();
        let (dx, dz) = (point.x - x, point.z - z);
        (self.base_m..=self.top_m).contains(&point.y)
            && dx * dx + dz * dz <= self.radius_m * self.radius_m
    }

    /// The share of light along `direction` (a unit vector) that reaches
    /// `point` through the crown: `exp(−density · ℓ)`, where `ℓ` is the
    /// length of the ray from `point` inside it.
    #[must_use]
    pub fn transmission(&self, point: Vec3, direction: Vec3) -> f64 {
        let (x, z) = self.axis();
        // The ray's span inside the crown's height, as distances along it.
        let (mut low, mut high) = if direction.y.abs() < 1e-12 {
            if (self.base_m..=self.top_m).contains(&point.y) {
                (0.0, f64::INFINITY)
            } else {
                return 1.0;
            }
        } else {
            let a = (self.base_m - point.y) / direction.y;
            let b = (self.top_m - point.y) / direction.y;
            (a.min(b).max(0.0), a.max(b))
        };
        // And inside its circle, seen from above.
        let (px, pz) = (point.x - x, point.z - z);
        let flat = direction.x * direction.x + direction.z * direction.z;
        let radius_sq = self.radius_m * self.radius_m;
        if flat < 1e-12 {
            if px * px + pz * pz > radius_sq {
                return 1.0;
            }
        } else {
            let half = (px * direction.x + pz * direction.z) / flat;
            let reach = half * half - (px * px + pz * pz - radius_sq) / flat;
            if reach < 0.0 {
                return 1.0;
            }
            let root = math::sqrt(reach);
            low = low.max(-half - root);
            high = high.min(-half + root);
        }
        if high <= low {
            return 1.0;
        }
        math::exp(-self.density * (high - low))
    }

    fn validate(&self) -> Result<(), String> {
        for value in self.centre_m {
            within("envelope centre_m", value, -1000.0, 1000.0)?;
        }
        within("envelope base_m", self.base_m, 0.0, 150.0)?;
        within("envelope top_m", self.top_m, self.base_m, 150.0)?;
        within("envelope radius_m", self.radius_m, 0.0, 100.0)?;
        within("envelope density", self.density, 0.0, 10.0)?;
        if self.top_m > self.base_m && self.radius_m > 0.0 {
            Ok(())
        } else {
            Err("an envelope needs a top above its base and a radius above 0".into())
        }
    }
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
            like: neighbourhood.like,
            envelopes: Vec::new(),
            layout: None,
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
            like: self.like,
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

impl Light {
    /// The latitude whose sun the sky carries, if it carries one.
    #[must_use]
    pub fn sun_latitude(&self) -> Option<f64> {
        (self.sky == Sky::SunAndSky).then_some(self.latitude_deg)
    }
}

/// The sky's light: `light@1`'s uniform overcast, or the overcast sky with
/// the direct sun over the year at `latitude_deg`, which `light@3` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sky {
    Overcast,
    SunAndSky,
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

/// How much of a grown plant stands where its place leaves no room: in
/// its substrate's solids or in its neighbours' crowns.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Intrusion {
    /// Segments standing, and those whose end is inside a solid or a crown.
    pub segments: usize,
    pub segments_in_solids: usize,
    pub segments_in_crowns: usize,
    /// Organs standing, and those inside a solid or a crown.
    pub organs: usize,
    pub organs_in_solids: usize,
    pub organs_in_crowns: usize,
    /// How far the deepest of them is inside a solid, metres.
    pub deepest_m: f64,
}

impl Intrusion {
    /// One line: what stands in the solids and the crowns.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{} of {} segments and {} of {} organs in its solids (deepest {:.2} m), \
             {} segments and {} organs in neighbours' crowns",
            self.segments_in_solids,
            self.segments,
            self.organs_in_solids,
            self.organs,
            self.deepest_m,
            self.segments_in_crowns,
            self.organs_in_crowns
        )
    }
}

impl Conditions {
    /// What of `graph` stands in this place's solids and neighbours'
    /// crowns (segments by their ends, organs by their positions), leaving
    /// out what is shed by its age.
    ///
    /// # Errors
    ///
    /// When the substrate is not a valid grid.
    pub fn intrusion(&self, graph: &PlantGraph) -> Result<Intrusion, String> {
        let field = self.substrate.as_ref().map(Substrate::field).transpose()?;
        let standing = |shed: Option<f64>| shed.is_none_or(|shed| shed > graph.age);
        let mut intrusion = Intrusion::default();
        let mut check = |point: Vec3, solid: &mut usize, crown: &mut usize| {
            if let Some(field) = &field {
                let distance = field.distance(point);
                if distance < 0.0 {
                    *solid += 1;
                    intrusion.deepest_m = intrusion.deepest_m.max(-distance);
                }
            }
            if self
                .envelopes()
                .iter()
                .any(|envelope| envelope.contains(point))
            {
                *crown += 1;
            }
        };
        let (mut segments, mut segment_solids, mut segment_crowns) = (0, 0, 0);
        for segment in graph
            .segments
            .iter()
            .filter(|segment| standing(segment.shed))
        {
            segments += 1;
            check(segment.end, &mut segment_solids, &mut segment_crowns);
        }
        let (mut organs, mut organ_solids, mut organ_crowns) = (0, 0, 0);
        for organ in graph.organs.iter().filter(|organ| standing(organ.shed)) {
            organs += 1;
            check(organ.position, &mut organ_solids, &mut organ_crowns);
        }
        Ok(Intrusion {
            segments,
            segments_in_solids: segment_solids,
            segments_in_crowns: segment_crowns,
            organs,
            organs_in_solids: organ_solids,
            organs_in_crowns: organ_crowns,
            ..intrusion
        })
    }

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

    /// This document laid over `base`, as a place a plant grows on in: each
    /// section it gives replaces `base`'s, and each it leaves out keeps
    /// `base`'s. A preset or a `neighbours` section replaces both of
    /// `base`'s, so a place that names no stand keeps the plant's own.
    #[must_use]
    pub fn over(&self, base: &Self) -> Self {
        let (preset, neighbours) = if self.preset.is_some() || self.neighbours.is_some() {
            (self.preset, self.neighbours.clone())
        } else {
            (base.preset, base.neighbours.clone())
        };
        Self {
            schema: CONDITIONS_SCHEMA,
            preset,
            neighbours,
            light: self.light.clone().or_else(|| base.light.clone()),
            climate: self.climate.clone().or_else(|| base.climate.clone()),
            soil: self.soil.clone().or_else(|| base.soil.clone()),
            disturbance: self
                .disturbance
                .clone()
                .or_else(|| base.disturbance.clone()),
            interactions: self
                .interactions
                .clone()
                .or_else(|| base.interactions.clone()),
            substrate: self.substrate.clone().or_else(|| base.substrate.clone()),
        }
    }

    /// The neighbours' crowns.
    #[must_use]
    pub fn envelopes(&self) -> &[Envelope] {
        self.neighbours
            .as_ref()
            .map_or(&[], |neighbours| &neighbours.envelopes)
    }

    /// Its neighbours like itself, if any.
    #[must_use]
    pub fn layout(&self) -> Option<Layout> {
        self.neighbours
            .as_ref()
            .and_then(|neighbours| neighbours.layout)
            .or_else(|| {
                let neighbourhood = self.neighbourhood();
                (neighbourhood.like > 0.0).then(|| Layout {
                    kind: if neighbourhood.one_sided {
                        LayoutKind::Edge
                    } else {
                        LayoutKind::Interior
                    },
                    spacing_m: neighbourhood.spacing.max(Layout::LEAST_SPACING_M),
                    per_height: neighbourhood.like,
                })
            })
    }

    /// The document with its preset expanded into `neighbours` and left
    /// out, and a substrate preset expanded into its grid, so documents
    /// that grow alike resolve alike.
    #[must_use]
    pub fn resolve(&self) -> Self {
        Self {
            preset: None,
            neighbours: Some(Neighbours {
                envelopes: self.envelopes().to_vec(),
                layout: self.layout(),
                ..Neighbours::of(self.neighbourhood())
            }),
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
        within("neighbours like", self.like, 0.0, 10.0)?;
        if let Some(layout) = &self.layout {
            layout.validate()?;
        }
        self.envelopes.iter().try_for_each(Envelope::validate)
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
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
    use super::{Around, Conditions, Crown, Envelope, Layout, LayoutKind, Surroundings};
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
        // {"schema":1,"neighbours":{"version":1,"density":0.0,
        // "relative_height":1.0,"canopy":0.0,"spacing":0.0,"one_sided":false,
        // "like":0.115,"layout":{"kind":"interior","spacing_m":0.25,
        // "per_height":0.115}}}.
        assert_eq!(
            interior.class().to_string(),
            "0a66def241296c41e8a15f1a9356fdb8222a1efbc06cfa7a5c63db339e36adcb"
        );
    }

    #[test]
    fn a_sunny_sky_names_its_latitude() {
        let sunny = Conditions::from_json(
            r#"{"schema": 1, "light": {"version": 1, "latitude_deg": 45, "sky": "sun_and_sky"}}"#,
        )
        .unwrap();
        assert_eq!(sunny.light.as_ref().unwrap().sun_latitude(), Some(45.0));
        let overcast = Conditions::from_json(
            r#"{"schema": 1, "light": {"version": 1, "latitude_deg": 45, "sky": "overcast"}}"#,
        )
        .unwrap();
        assert_eq!(overcast.light.as_ref().unwrap().sun_latitude(), None);
        assert_ne!(sunny.class(), overcast.class());
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
                "envelopes": [{"centre_m": [3, 0], "base_m": 10, "top_m": 10,
                               "radius_m": 2, "density": 0.5}]}}"#,
            "an envelope needs a top above its base",
        );
        refused(
            r#"{"schema": 1, "substrate": {"version": 1, "preset": "wall", "scale": 80}}"#,
            "substrate scale must be 0.1 to 50, found 80",
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

    fn crown() -> Envelope {
        Envelope {
            centre_m: [4.0, 0.0],
            base_m: 2.0,
            top_m: 10.0,
            radius_m: 2.0,
            density: 0.5,
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn a_neighbour_s_crown_shades_what_its_light_passes_through() {
        let crown = crown();
        let up = Vec3::new(0.0, 1.0, 0.0);
        // Straight up through its axis: 8 m of crown.
        let under = Vec3::new(4.0, 0.0, 0.0);
        assert!((crown.transmission(under, up) - (-0.5_f64 * 8.0).exp()).abs() < 1e-12);
        // From inside it, only the crown above the point.
        let within = Vec3::new(4.0, 6.0, 0.0);
        assert!((crown.transmission(within, up) - (-2.0_f64).exp()).abs() < 1e-12);
        assert!(crown.contains(within) && !crown.contains(under));
        // Beside it, above it, or looking away from it: no shade.
        assert_eq!(crown.transmission(Vec3::new(0.0, 5.0, 0.0), up), 1.0);
        assert_eq!(crown.transmission(Vec3::new(4.0, 11.0, 0.0), up), 1.0);
        let west = Vec3::new(-1.0, 1.0, 0.0).normalize_or(up);
        assert_eq!(crown.transmission(Vec3::new(0.0, 5.0, 0.0), west), 1.0);
        // Slanting east through it at 45°, along a chord of its circle:
        // the ray crosses x = 2 to 6 between heights 7 and 11, inside the
        // crown up to 10 m, 3 m of rise, so 3√2 m of crown.
        let east = Vec3::new(1.0, 1.0, 0.0).normalize_or(up);
        let shade = crown.transmission(Vec3::new(0.0, 5.0, 0.0), east);
        assert!(
            (shade - (-0.5 * 3.0 * 2.0_f64.sqrt()).exp()).abs() < 1e-9,
            "{shade}"
        );
        // North is −Z.
        let north = Envelope {
            centre_m: [0.0, 4.0],
            ..crown
        };
        assert!(north.contains(Vec3::new(0.0, 5.0, -4.0)));
        assert!(!north.contains(Vec3::new(0.0, 5.0, 4.0)));
    }

    #[test]
    fn crowns_round_a_plant_shade_it_and_take_their_room() {
        let crowns = [crown()];
        let around = Around {
            neighbourhood: &Neighbourhood::OPEN,
            envelopes: &crowns,
            host: None,
            substrate: None,
            sun: None,
        };
        let up = Vec3::new(0.0, 1.0, 0.0);
        assert!(!around.clear_sky() && around.crowded());
        assert!(around.transmission(Vec3::new(4.5, 1.0, 0.0), 1.0, up) < 0.1);
        assert!(around.occupied(Vec3::new(4.5, 3.0, 0.0)));
        assert!(!around.occupied(Vec3::new(0.0, 3.0, 0.0)));
        let open = Around {
            envelopes: &[],
            ..around
        };
        assert!(open.clear_sky() && !open.crowded());
    }

    #[test]
    fn a_place_laid_over_a_plant_s_conditions_keeps_what_it_leaves_out() {
        let edge = Conditions::from_json(
            r#"{"schema": 1, "preset": "edge",
                "light": {"version": 1, "latitude_deg": 47, "sky": "overcast"}}"#,
        )
        .unwrap();
        let wall = Conditions::from_json(
            r#"{"schema": 1, "substrate": {"version": 1, "preset": "wall", "scale": 10}}"#,
        )
        .unwrap();
        let placed = wall.over(&edge);
        assert_eq!(placed.preset, Some(Environment::Edge));
        assert_eq!(placed.light, edge.light);
        assert_eq!(placed.substrate, wall.substrate);
        // A stand of its own replaces the plant's.
        let crowded = Conditions::from_json(
            r#"{"schema": 1, "neighbours": {"version": 1, "density": 0,
                "relative_height": 0, "canopy": 0, "spacing": 0, "one_sided": false,
                "envelopes": [{"centre_m": [4, 0], "base_m": 2, "top_m": 10,
                               "radius_m": 2, "density": 0.5}]}}"#,
        )
        .unwrap()
        .over(&edge);
        assert_eq!(crowded.preset, None);
        assert_eq!(crowded.envelopes(), &[crown()]);
        assert_eq!(crowded.neighbourhood(), Neighbourhood::OPEN);
        // Its crowns are part of its class.
        let fewer = Conditions {
            neighbours: crowded.neighbours.clone().map(|mut neighbours| {
                neighbours.envelopes.clear();
                neighbours
            }),
            ..crowded.clone()
        };
        assert_ne!(fewer.class(), crowded.class());
    }

    #[test]
    fn a_layout_places_its_neighbours_round_the_plant() {
        let crown = Crown {
            base_m: 1.0,
            top_m: 10.0,
            radius_m: 3.0,
            leaf_area_m2: 200.0,
        };
        let placed = |kind, spacing_m| -> Vec<[f64; 2]> {
            Layout {
                kind,
                spacing_m,
                per_height: 0.0,
            }
            .envelopes(&crown, 0.0)
            .iter()
            .map(|envelope| envelope.centre_m)
            .collect()
        };
        // It reaches neighbours whose stems stand within 26 m: at 5 m apart,
        // all three rings.
        assert_eq!(placed(LayoutKind::Interior, 5.0).len(), 48);
        assert_eq!(placed(LayoutKind::Edge, 5.0).len(), 21);
        assert!(
            placed(LayoutKind::Edge, 5.0)
                .iter()
                .all(|[east, _]| *east < 0.0)
        );
        assert_eq!(
            placed(LayoutKind::Row, 5.0),
            [
                [-15.0, 0.0],
                [-10.0, 0.0],
                [-5.0, 0.0],
                [5.0, 0.0],
                [10.0, 0.0],
                [15.0, 0.0]
            ]
        );
        // At 20 m only the nearest four reach; at 30 m none does.
        assert_eq!(
            placed(LayoutKind::Interior, 20.0),
            [[-20.0, 0.0], [0.0, -20.0], [0.0, 20.0], [20.0, 0.0]]
        );
        assert!(placed(LayoutKind::Interior, 30.0).is_empty());
    }

    #[test]
    fn a_stand_of_the_plants_like_thins_as_it_grows() {
        let layout = Conditions::preset(Environment::Interior).layout().unwrap();
        assert_eq!(layout.kind, LayoutKind::Interior);
        assert_eq!(
            Conditions::preset(Environment::Edge).layout().unwrap().kind,
            LayoutKind::Edge
        );
        assert!(Conditions::preset(Environment::Open).layout().is_none());
        // The nearest neighbours stand STAND_SPACING of the plant's height away.
        let nearest_at = |top_m: f64, age_years: f64| {
            let crown = Crown {
                base_m: 0.3 * top_m,
                top_m,
                radius_m: 0.15 * top_m,
                leaf_area_m2: 100.0,
            };
            layout
                .envelopes(&crown, age_years)
                .iter()
                .map(|envelope| envelope.centre_m[0].hypot(envelope.centre_m[1]))
                .fold(f64::INFINITY, f64::min)
        };
        let nearest = |top_m: f64| nearest_at(top_m, 60.0);
        assert!((nearest(8.0) - 0.92).abs() < 1e-9);
        assert!((nearest(40.0) - 4.6).abs() < 1e-9);
        // Never nearer than the least spacing.
        assert!((nearest(0.4) - Layout::LEAST_SPACING_M).abs() < 1e-9);
        // Past the yield tables' ages it opens as old growth does.
        assert!((nearest_at(40.0, 160.0) - 4.6).abs() < 1e-9);
        assert!((nearest_at(40.0, 450.0) - 0.204 * 40.0).abs() < 1e-9);
        assert!((nearest_at(40.0, 850.0) - 0.204 * 40.0).abs() < 1e-9);
    }

    #[test]
    fn a_layout_is_read_checked_and_part_of_the_class() {
        let text = |spacing: &str| {
            format!(
                r#"{{"schema": 1, "neighbours": {{"version": 1, "density": 0,
                "relative_height": 0, "canopy": 0, "spacing": 0, "one_sided": false,
                "layout": {{"kind": "row", "spacing_m": {spacing}}}}}}}"#
            )
        };
        let row = Conditions::from_json(&text("6")).unwrap();
        assert_eq!(
            row.layout(),
            Some(Layout {
                kind: LayoutKind::Row,
                spacing_m: 6.0,
                per_height: 0.0
            })
        );
        assert_eq!(row.resolve().layout(), row.layout());
        assert!(Conditions::from_json(&text("0.1")).is_err());
        let open = Conditions {
            neighbours: row.neighbours.clone().map(|mut neighbours| {
                neighbours.layout = None;
                neighbours
            }),
            ..row.clone()
        };
        assert_ne!(open.class(), row.class());
        assert_eq!(open.class(), Conditions::preset(Environment::Open).class());
    }
}
