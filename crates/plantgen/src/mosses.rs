//! Mosses growing on surfaces: colonies of moss genera settling and
//! spreading over a site's faces year by year, and drawn as the cushions,
//! turfs and mats they grow into.
//!
//! A moss has no roots: it holds to bare rock, mortar, tiles, bark or
//! wood by rhizoids and takes its water from rain, dew and splash over its
//! whole surface, drying out and recovering again (Goffinet and Shaw,
//! *Bryophyte Biology*, 2009, pp. 193 and 254). So where it grows on a
//! building is set by how wet a face stays and how much light it gets,
//! not by soil: up-facing faces that catch rain (wall tops, ledges, the
//! joints between bricks, the steps of roof tiles), the damp foot of a
//! wall, faces turned from the sun; never the dry underside of eaves.
//!
//! Three stages, each a function of what came before:
//!
//! 1. [`kinds`]: each moss genus the library describes ([`MossKind`]),
//!    read from its rank files' traits (`colony_form`, `perichaetia`,
//!    leaf sizes, `hair_point_length_m`, `leaf_dry_posture`, capsules,
//!    `substrates`, `light`, `moisture`, `desiccation_tolerance`) by
//!    rules that hold for every moss, never per genus.
//! 2. [`grow`]: the habitat of every face in a region of a
//!    [`SiteMesh`] ([`Habitat`]: light, wetness, substrate), then spores
//!    landing and colonies establishing where a genus fits, and growing
//!    outward each year until they meet a neighbour or run out of
//!    surface. The result, a [`MossCover`], is a list of colonies, small
//!    enough for a host to keep and draw with its own systems.
//! 3. [`MossCover::draw`]: a mesh of the colonies. Far off, each is a
//!    shell in its colony's form (a dome, a level-topped turf, a low mat)
//!    coloured as its leaves and hair-points are; near a point of
//!    interest, the shell carries every shoot's tip with its leaves, and
//!    the capsules on their setae.
//!
//! Colonies stay on their surface: every point of a drawn colony is laid
//! on the nearest face of the site under it, and dropped where there is
//! none within reach (past a wall's corner, over a coping's edge), so no
//! moss floats.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::conditions::Envelope;
use crate::library::Library;
use crate::lsys::tools::closest_on_triangle;
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng::{hash_str, hash_words, unit};
use crate::site::SiteMesh;
use crate::site_field::{Tracer, frame, hemisphere, spread};
use crate::substrate::{BARK, BUILT, ROCK, SOIL, WOOD};

/// The version of a [`MossCover`] file.
pub const MOSS_COVER_VERSION: u32 = 1;

const GOLDEN: f64 = 2.399_963_229_728_653;

/// Pieces a small colony in a perfect place sheds a year that may take
/// root, before its habitat's chance; one of 1 cm radius sheds three
/// times as many. Walls and roofs fill in over decades rather than years.
/// Estimate.
const OFFSPRING_PER_YEAR: f64 = 2.0;

/// The two measured mosses [`MossKind::far_share`] runs between: (spore
/// diameter in metres, share of spores beyond 2 m) for *Bryum argenteum*
/// (spores 8-15 µm, over 80% beyond 2 m) and *Tortula truncata* (25-30 µm,
/// 30%).
pub const SPORE_FAR: ((f64, f64), (f64, f64)) = ((11.5e-6, 0.8), (27.5e-6, 0.3));

/// The mean distance beyond its rim a colony's pieces lodge: most fall
/// within a few centimetres. Estimate.
const OFFSPRING_THROW_M: f64 = 0.03;

/// How a colony's shoots grow together (trait `colony_form`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColonyForm {
    /// Shoots radiating from one point into a dome.
    Cushion,
    /// Upright shoots side by side under a level top.
    Turf,
    /// Shoots creeping over the surface and branching, pressed to it.
    Mat,
    /// Loose, interwoven ascending shoots.
    Weft,
}

/// How the leaves lie when the moss is dry (trait `leaf_dry_posture`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DryPosture {
    Appressed,
    Imbricate,
    Flexuose,
    Crisped,
    Twisted,
    Falcate,
}

/// A capsule's outline (trait `capsule_shape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleShape {
    /// Widest at its middle: ovoid, ellipsoid.
    #[default]
    Ovoid,
    /// Straight-sided from a short neck.
    Cylindric,
    /// Widening from a narrow neck toward the mouth: pyriform, clavate.
    Pyriform,
}

/// How a ripe capsule is held (trait `capsule_posture`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsulePosture {
    Erect,
    Inclined,
    Horizontal,
    Pendent,
}

/// A moss genus as it is drawn and grown: its traits turned into numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MossKind {
    pub genus: String,
    pub family: String,
    pub form: ColonyForm,
    /// Capsules on short side branches of creeping shoots, not at the
    /// tips of upright ones.
    pub pleurocarpous: bool,
    /// A grown colony's height above its surface: a cushion's or turf's
    /// height, a mat's depth.
    pub height_m: f64,
    pub leaf_length_m: f64,
    pub leaf_width_m: f64,
    /// The colourless awn beyond the green blade.
    pub hair_point_m: f64,
    /// Share of the blade, from its tip, with no chlorophyll.
    pub hyaline_share: f64,
    /// Linear RGB of the leaves, moist.
    pub colour: [f32; 3],
    pub dry_posture: DryPosture,
    /// How far a crisped leaf coils when dry, 0 to 1 (trait
    /// `leaf_dry_crisping`; 1 when left out): "crisped" 1, "somewhat
    /// crisped" or "contorted" 0.5.
    #[serde(default = "full_crisping")]
    pub crisping: f64,
    /// The blade's outline: its width (0 to 1 of the widest) at points
    /// from base (0) to tip (1), the library's profile for the genus's
    /// `leaf_shape` (kingdom rule `appearance.organs.leaf.shape.outline`);
    /// empty for lanceolate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<[f64; 2]>,
    /// How crowded the leaves are at the top of a shoot (trait
    /// `stem_foliation`): 0 evenly foliate, 0.5 comose, 1 rosulate.
    #[serde(default)]
    pub rosette: f64,
    /// Side branches curl up and inward when dry (trait
    /// `dry_branch_posture` curled).
    #[serde(default)]
    pub branches_curl_dry: bool,
    pub seta_m: f64,
    pub seta_arcuate: bool,
    pub seta_colour: [f32; 3],
    pub capsule_m: f64,
    pub capsule_colour: [f32; 3],
    /// The older leaves down a shoot, below its green tip.
    #[serde(default = "default_older_colour")]
    pub older_colour: [f32; 3],
    pub capsule_posture: CapsulePosture,
    /// The capsule lid's length against the capsule's (trait
    /// `operculum_form`: conic 0.1, long_conic 0.2, rostrate 0.3).
    #[serde(default = "default_lid")]
    pub lid_share: f64,
    /// The capsule's outline (trait `capsule_shape`).
    #[serde(default)]
    pub capsule_shape: CapsuleShape,
    /// Share of shoots bearing a capsule.
    pub capsule_share: f64,
    /// Spore diameter, metres (trait `spore_diameter_m`; the 20 µm that
    /// parts small, far-flying spores from large, local ones when left out,
    /// During 1979 in Glime vol. 1 p. 522).
    #[serde(default = "default_spore")]
    pub spore_m: f64,
    /// Metres a colony's edge moves out in a good year.
    pub spread_m: f64,
    /// Light classes it grows in: shade, part, sun.
    pub light: [bool; 3],
    /// Wetness classes it grows in: wet, mesic, dry, xeric.
    pub moisture: [bool; 4],
    /// Substrates it grows on, by material number (soil to built).
    pub substrates: [bool; 6],
    /// -1 acid surfaces only, 0 either, 1 basic only.
    pub reaction: i8,
    /// How well it lives through drying out, 0.2 (low) to 1 (high).
    pub tolerance: f64,
}

/// A spore size unstated in the traits: During's (1979) boundary between
/// numerous light spores that travel far and few large ones that stay
/// near (Glime, *Bryophyte Ecology* vol. 1, PDF p. 522).
fn default_spore() -> f64 {
    20.0e-6
}

/// A capsule lid unstated in the traits: a tall cone, between the short
/// cones of Bryum and the beaks of Grimmia (FNA vols. 27-28).
fn default_lid() -> f64 {
    0.2
}

/// Older leaves a moss genus gives no colour for: yellow-brown.
/// The library's lanceolate profile (kingdom rule
/// `appearance.organs.leaf.shape.outline`), for kinds read without one.
const LANCEOLATE: [[f64; 2]; 15] = [
    [0.0, 0.2],
    [0.02, 0.31],
    [0.05, 0.48],
    [0.1, 0.7],
    [0.2, 0.91],
    [0.3, 1.0],
    [0.4, 0.96],
    [0.5, 0.85],
    [0.6, 0.74],
    [0.7, 0.58],
    [0.8, 0.4],
    [0.9, 0.21],
    [0.95, 0.1],
    [0.98, 0.04],
    [1.0, 0.0],
];

fn full_crisping() -> f64 {
    1.0
}

fn default_older_colour() -> [f32; 3] {
    linear(DEFAULT_OLDER)
}

const DEFAULT_OLDER: [u8; 3] = [66, 54, 30];

/// The value a qualified state picks: the state stated as most usual,
/// the first of those that tie (in the map's order, so the same files
/// give the same pick).
fn usual(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Object(states) => {
            let weight = |word: &Value| match word.as_str() {
                Some("always") => 5,
                Some("usually") => 4,
                Some("often") => 3,
                Some("sometimes") => 2,
                Some("rarely") => 1,
                _ => 0,
            };
            let mut best: Option<(&String, i32)> = None;
            for (state, word) in states {
                let w = weight(word);
                if best.is_none_or(|(_, b)| w > b) {
                    best = Some((state, w));
                }
            }
            best.map(|(state, _)| state.clone())
        }
        _ => None,
    }
}

/// A number's typical value: the number, a range's `typical`, else the
/// middle of its `min` and `max`.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Object(range) => range.get("typical").and_then(Value::as_f64).or_else(|| {
            let low = range.get("min").and_then(Value::as_f64)?;
            let high = range.get("max").and_then(Value::as_f64)?;
            Some(0.5 * (low + high))
        }),
        _ => None,
    }
}

fn states(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Value::String(text) => vec![text.clone()],
        _ => Vec::new(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn colour(value: &Value) -> Option<[f32; 3]> {
    let items = value.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let mut out = [0.0_f32; 3];
    for (channel, item) in out.iter_mut().zip(items) {
        let byte = u8::try_from(item.as_u64()?.min(255)).ok()?;
        *channel = crate::texture::srgb_to_linear(byte) as f32;
    }
    Some(out)
}

/// Linear RGB of an sRGB colour given in bytes.
#[allow(clippy::cast_possible_truncation)]
fn linear(srgb: [u8; 3]) -> [f32; 3] {
    srgb.map(|byte| crate::texture::srgb_to_linear(byte) as f32)
}

impl MossKind {
    /// The kind a genus's traits (its own and those it inherits) describe.
    /// Where a trait is unstated, a rule for every moss fills it in: the
    /// leaf two fifths as wide as long; a cushion 1.2 cm high, a turf
    /// 0.8 cm, a mat 0.6 cm; capsules erect on straight setae, warm
    /// yellow-brown (setae are yellow, red or brown in most mosses,
    /// *Bryophyte Biology* p. 98); the light and wetness of shade, where
    /// most bryophytes grow (p. 276).
    ///
    /// # Errors
    ///
    /// Where `colony_form` is unstated, or a stated value is not one of
    /// the vocabulary's.
    #[allow(clippy::too_many_lines)]
    pub fn from_traits(
        genus: &str,
        family: &str,
        traits: &Map<String, Value>,
    ) -> Result<Self, String> {
        let get = |key: &str| traits.get(key).filter(|value| !value.is_null());
        let pick = |key: &str| get(key).and_then(usual);
        let num = |key: &str| get(key).and_then(number);
        let form: ColonyForm = match pick("colony_form") {
            Some(text) => serde_json::from_value(Value::String(text))
                .map_err(|error| format!("{genus}: colony_form: {error}"))?,
            None => return Err(format!("{genus}: no colony_form stated")),
        };
        let pleurocarpous = pick("perichaetia").as_deref() == Some("pleurocarpous");
        let leaf_length_m = num("leaf_length_m").unwrap_or(0.0015);
        let height_m = num("height_typical_m").unwrap_or(match form {
            ColonyForm::Cushion => 0.012,
            ColonyForm::Turf => 0.008,
            ColonyForm::Mat | ColonyForm::Weft => 0.006,
        });
        let dry_posture = match pick("leaf_dry_posture") {
            Some(text) => serde_json::from_value(Value::String(text))
                .map_err(|error| format!("{genus}: leaf_dry_posture: {error}"))?,
            None => DryPosture::Appressed,
        };
        let capsule_posture = match pick("capsule_posture") {
            Some(text) => serde_json::from_value(Value::String(text))
                .map_err(|error| format!("{genus}: capsule_posture: {error}"))?,
            None => CapsulePosture::Erect,
        };
        let light_states = get("light").map(states).unwrap_or_default();
        let light = if light_states.is_empty() {
            [true, true, false]
        } else {
            ["shade", "part", "sun"].map(|name| light_states.iter().any(|s| s == name))
        };
        let wet_states = get("moisture").map(states).unwrap_or_default();
        let moisture = if wet_states.is_empty() {
            [true, true, false, false]
        } else {
            ["wet", "mesic", "dry", "xeric"].map(|name| wet_states.iter().any(|s| s == name))
        };
        let substrate_states = get("substrates").map(states).unwrap_or_default();
        let mut substrates = [false; 6];
        for (material, name) in [
            (SOIL, "soil"),
            (ROCK, "rock"),
            (BARK, "bark"),
            (WOOD, "wood"),
            (BUILT, "built"),
        ] {
            substrates[usize::from(material)] =
                substrate_states.is_empty() || substrate_states.iter().any(|s| s == name);
        }
        let reaction = match pick("substrate_reaction").as_deref() {
            Some("acid") => -1,
            Some("basic") => 1,
            _ => 0,
        };
        let tolerance = match pick("desiccation_tolerance").as_deref() {
            Some("high") => 1.0,
            Some("low") => 0.2,
            _ => 0.6,
        };
        Ok(Self {
            genus: genus.to_owned(),
            family: family.to_owned(),
            form,
            pleurocarpous,
            height_m,
            leaf_length_m,
            leaf_width_m: num("leaf_width_m").unwrap_or(leaf_length_m * 0.4),
            hair_point_m: num("hair_point_length_m").unwrap_or(0.0),
            hyaline_share: num("leaf_hyaline_share").unwrap_or(0.0),
            colour: get("leaf_colour")
                .and_then(colour)
                .unwrap_or(linear([90, 120, 50])),
            dry_posture,
            crisping: num("leaf_dry_crisping").unwrap_or(1.0).clamp(0.0, 1.0),
            outline: Vec::new(),
            rosette: match pick("stem_foliation").as_deref() {
                Some("rosulate") => 1.0,
                Some("comose") => 0.5,
                _ => 0.0,
            },
            branches_curl_dry: pick("dry_branch_posture").as_deref() == Some("curled"),
            seta_m: num("seta_length_m").unwrap_or(leaf_length_m * 8.0),
            seta_arcuate: pick("seta_form").as_deref() == Some("arcuate"),
            seta_colour: get("seta_colour")
                .and_then(colour)
                .unwrap_or(linear([150, 110, 60])),
            capsule_m: num("capsule_length_m").unwrap_or(leaf_length_m),
            capsule_colour: get("capsule_colour")
                .and_then(colour)
                .unwrap_or(linear([120, 80, 45])),
            older_colour: get("older_leaf_colour")
                .and_then(colour)
                .unwrap_or_else(default_older_colour),
            capsule_posture,
            capsule_shape: match pick("capsule_shape") {
                Some(text) => serde_json::from_value(Value::String(text))
                    .map_err(|error| format!("{genus}: capsule_shape: {error}"))?,
                None => CapsuleShape::Ovoid,
            },
            lid_share: match pick("operculum_form").as_deref() {
                Some("conic") => 0.1,
                Some("rostrate") => 0.3,
                _ => default_lid(),
            },
            capsule_share: num("capsule_share").unwrap_or(0.2),
            spore_m: num("spore_diameter_m").unwrap_or_else(default_spore),
            spread_m: num("colony_spread_m_per_year").unwrap_or(match form {
                ColonyForm::Cushion => 0.0015,
                ColonyForm::Turf => 0.003,
                ColonyForm::Mat | ColonyForm::Weft => 0.01,
            }),
            light,
            moisture,
            substrates,
            reaction,
            tolerance,
        })
    }

    /// The blade's width at `s` along it (0 base, 1 tip), 0 to 1 of its
    /// widest, from [`MossKind::outline`] (lanceolate when empty).
    #[must_use]
    pub fn blade_width(&self, s: f64) -> f64 {
        let points: &[[f64; 2]] = if self.outline.len() >= 2 {
            &self.outline
        } else {
            &LANCEOLATE
        };
        let s = s.clamp(0.0, 1.0);
        for pair in points.windows(2) {
            let ([s0, w0], [s1, w1]) = (pair[0], pair[1]);
            if s <= s1 {
                let t = if s1 > s0 { (s - s0) / (s1 - s0) } else { 0.0 };
                return w0 + (w1 - w0) * t;
            }
        }
        points.last().map_or(0.0, |p| p[1])
    }

    /// Where along the blade it is widest, 0 (base) to 1 (tip).
    #[must_use]
    pub fn blade_widest(&self) -> f64 {
        let points: &[[f64; 2]] = if self.outline.len() >= 2 {
            &self.outline
        } else {
            &LANCEOLATE
        };
        points
            .iter()
            .max_by(|a, b| a[1].total_cmp(&b[1]))
            .map_or(0.3, |p| p[0])
            .clamp(0.15, 0.85)
    }

    /// Across a shoot's leaves from tip to tip, moist: twice a leaf's
    /// reach out from the stem at the angle it stands at.
    #[must_use]
    pub fn shoot_width_m(&self) -> f64 {
        (2.0 * self.leaf_length_m * math::sin(math::radians(self.moist_angle_deg()))
            + self.leaf_width_m * 0.5)
            .max(0.0005)
    }

    /// The angle a leaf stands from its stem when moist. Leaves that lie
    /// overlapping when dry (imbricate) stay close when moist too; the
    /// others spread (Pottiaceae, Grimmiaceae: erect-spreading to patent
    /// when wet; FNA vol. 27).
    fn moist_angle_deg(&self) -> f64 {
        match self.dry_posture {
            DryPosture::Imbricate => 22.0,
            DryPosture::Appressed | DryPosture::Flexuose => 40.0,
            DryPosture::Crisped | DryPosture::Twisted => 50.0,
            DryPosture::Falcate => 45.0,
        }
    }

    /// The angle a leaf stands from its stem when dry.
    fn dry_angle_deg(&self) -> f64 {
        match self.dry_posture {
            DryPosture::Imbricate => 14.0,
            DryPosture::Appressed => 7.0,
            // Close to the stem, their outer halves swept round it.
            DryPosture::Twisted => 20.0,
            DryPosture::Flexuose => 16.0,
            DryPosture::Crisped => 28.0,
            DryPosture::Falcate => 25.0,
        }
    }

    /// How well it fits `habitat`, 0 to 1.
    #[must_use]
    pub fn fit(&self, habitat: &Habitat) -> f64 {
        let light = class_fit(habitat.light, &[0.0, 0.3, 0.65, 1.0], &self.light);
        // Wetness classes from wet to xeric, so the bounds fall.
        let wet = class_fit(
            1.0 - habitat.wetness,
            &[0.0, 0.3, 0.55, 0.75, 1.0],
            &self.moisture,
        );
        let material = usize::from(habitat.material);
        let mut substrate = if self.substrates.get(material).copied().unwrap_or(false) {
            1.0
        } else {
            0.06
        };
        if habitat.soil && self.substrates[usize::from(SOIL)] {
            substrate = 1.0;
        }
        // Built surfaces (mortar, concrete, cement tiles) are basic, bark
        // and wood acid, rock and soil either, unless the surface says
        // otherwise (an asphalt shingle roof neutral).
        let reaction = if self.reaction != 0 && habitat.reaction == -self.reaction {
            0.35
        } else {
            1.0
        };
        // A dry face spares only the mosses that live through drying out.
        let drying = math::smoothstep(0.45, 0.15, habitat.wetness);
        let tolerance = 1.0 - drying * (1.0 - self.tolerance);
        light * wet * substrate * reaction * tolerance * habitat.hold
    }

    /// How many spores it sheds, against a moss whose capsules stand on a
    /// fiftieth of its shoots: as the square root of its share of shoots
    /// in fruit, from 0.3 (a moss that seldom fruits and spreads mostly by
    /// pieces of shoot) to 3. Estimate, for the regional spore rain.
    #[must_use]
    pub fn spore_output(&self) -> f64 {
        (self.capsule_share / 0.02).sqrt().clamp(0.3, 3.0)
    }

    /// The share of its spores that fly beyond a couple of metres and join
    /// the regional spore rain; the rest fall in or near the colony that
    /// shed them. Small spores travel far, large ones stay near (Glime,
    /// *Bryophyte Ecology* vol. 1, PDF pp. 520-522). Straight between two
    /// measured mosses, by spore diameter: *Bryum argenteum* (spores
    /// 8-15 µm, FNA vol. 28) sends over 80% beyond 2 m (Miles and Longton
    /// 1990, 1992), *Tortula truncata* (25-30 µm, FNA vol. 27) 30% (Roads
    /// and Longton 2003; PDF pp. 594-596). Held to 0.2-0.85.
    #[must_use]
    pub fn far_share(&self) -> f64 {
        let (small, large) = (SPORE_FAR.0.0, SPORE_FAR.1.0);
        let t = (self.spore_m - small) / (large - small);
        (SPORE_FAR.0.1 + (SPORE_FAR.1.1 - SPORE_FAR.0.1) * t).clamp(0.2, 0.85)
    }

    /// How well it grows in `habitat`, 0 to 1: its fit, times the share of
    /// the year the surface stays moist. A moss grows only while wet, and
    /// only through moist spells of a day or more, so even the mosses
    /// that live through drying grow slowly where the surface dries fast
    /// (*Bryophyte Biology* pp. 193, 272-273).
    #[must_use]
    pub fn vigour(&self, habitat: &Habitat) -> f64 {
        self.fit(habitat) * growing_time(habitat.wetness)
    }
}

/// Days with 0.01 inch (0.25 mm) of rain or more, January to December, at
/// Portland International Airport: NOAA's 1991-2020 normals (station
/// USW00024229, as tabled in Wikipedia's "Portland, Oregon", read
/// 2026-11-07). 157 a year: wet winters, three or four in July and August.
pub const PORTLAND_RAIN_DAYS: [f64; 12] = [
    18.7, 15.7, 17.8, 17.4, 13.2, 9.2, 3.7, 3.6, 6.7, 13.5, 18.3, 19.2,
];

/// How wet moss is drawn on `day` of the year (1 to 365), as
/// [`DrawOptions::hydration`]: the share of days with rain at that time of
/// year in `rain_days` (per month, January first), straight between the
/// middles of the months. Mosses take water only from rain, not from dew
/// (*Grimmia pulvinata* on walls, Proctor 2004 in Glime, *Bryophyte
/// Ecology* vol. 1 p. 1303), and a wet spell outlasts its rain, so a rainy
/// day's moss is drawn wet; the share of rain days is an estimate of how
/// often moss is wet, not a measured one. Portland: 0.6 in January, 0.12
/// in July.
#[must_use]
pub fn hydration_on(day: f64, rain_days: &[f64; 12]) -> f64 {
    const LENGTHS: [f64; 12] = [
        31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0,
    ];
    let mut middles = [0.0; 12];
    let mut start = 0.0;
    for (month, length) in LENGTHS.iter().enumerate() {
        middles[month] = start + 0.5 * length;
        start += length;
    }
    let share = |month: usize| rain_days[month] / LENGTHS[month];
    let day = (day - 0.5).rem_euclid(365.0);
    // The month whose middle is at or before `day`, December's before
    // the middle of January.
    let before = middles
        .iter()
        .rposition(|middle| *middle <= day)
        .unwrap_or(11);
    let after = (before + 1) % 12;
    let from = middles[before];
    let to = if after == 0 {
        middles[0] + 365.0
    } else {
        middles[after]
    };
    let day = if day < from { day + 365.0 } else { day };
    let t = ((day - from) / (to - from)).clamp(0.0, 1.0);
    (share(before) + (share(after) - share(before)) * t).clamp(0.0, 1.0)
}

/// The share of the year a surface of `wetness` stays moist long enough
/// for mosses to grow.
fn growing_time(wetness: f64) -> f64 {
    0.08 + 0.92 * math::pow(math::smoothstep(0.2, 0.75, wetness), 1.5)
}

impl MossKind {
    /// The kind as it grows in `light` ([`Habitat::light`], 0 deep shade
    /// to 1 full sun). Glime, *Bryophyte Ecology* vol. 1 (PDF pages of the
    /// mirror's copy): hair-points are lost in damp air or deep shade
    /// (*Schistidium*, p. 1257; *Racomitrium* 50-100% shorter, p. 976);
    /// leaves hold more chlorophyll, so look darker green, as light falls
    /// (p. 463); in deep shade shoots etiolate, taller, thinner and paler
    /// with smaller leaves (pp. 946, 959-960, 1441); in full sun leaves
    /// yellow with carotenoids (*Ceratodon* carotenoid to chlorophyll 0.55
    /// in high light against 0.35 in low, p. 1735) or redden (pp. 821-823).
    /// Part shade, where most genera are described, changes nothing.
    #[must_use]
    pub fn in_light(&self, light: f64) -> std::borrow::Cow<'_, MossKind> {
        let sun = math::smoothstep(0.65, 1.0, light);
        let shade = math::smoothstep(0.45, 0.15, light);
        let deep = math::smoothstep(0.2, 0.03, light);
        if sun <= 0.0 && shade <= 0.0 {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut kind = self.clone();
        kind.hair_point_m *= 1.0 - 0.6 * shade;
        kind.height_m *= 1.0 + 0.5 * deep;
        kind.leaf_length_m *= 1.0 - 0.2 * deep;
        kind.leaf_width_m *= 1.0 - 0.25 * deep;
        #[allow(clippy::cast_possible_truncation)]
        let (sun, darker, deep) = (sun as f32, (shade - deep).max(0.0) as f32, deep as f32);
        let tint = |c: [f32; 3]| -> [f32; 3] {
            // Carotenoids: more red and green against blue in the sun.
            let [r, g, b] = [
                c[0] * (1.0 + 0.3 * sun),
                c[1] * (1.0 + 0.06 * sun),
                c[2] * (1.0 - 0.4 * sun),
            ];
            // More chlorophyll in shade: red and blue fall against green.
            let [r, g, b] = [r * (1.0 - 0.15 * darker), g, b * (1.0 - 0.1 * darker)];
            // Etiolated: paler and yellower, a third of the way to twice as light.
            let pale = [r * 2.0, g * 1.8, b * 1.2];
            [
                r + (pale[0] - r) * 0.35 * deep,
                g + (pale[1] - g) * 0.35 * deep,
                b + (pale[2] - b) * 0.35 * deep,
            ]
        };
        kind.colour = tint(kind.colour);
        kind.older_colour = tint(kind.older_colour);
        std::borrow::Cow::Owned(kind)
    }

    /// The moist colour a colony shows from afar, for hosts that draw
    /// moss as a tint ([`MossCover::tint`]).
    #[must_use]
    pub fn far_colour(&self) -> [f32; 3] {
        shell_colour(self, 1.0, 0.5)
    }

    /// How much the hair-points and hyaline tips whiten a colony seen from
    /// afar: more when dry, as the awns of folded leaves stand out (FNA
    /// vol. 27, Grimmia pulvinata).
    fn whiteness(&self, hydration: f64) -> f64 {
        let awn = (self.hair_point_m / self.leaf_length_m).min(1.2);
        let dryness = 1.0 - hydration;
        (awn * (0.07 + 0.16 * dryness) + self.hyaline_share * (0.2 + 0.22 * dryness)).min(0.3)
    }
}

/// How far `value` lies in the classes `allowed` marks, the classes split
/// at `bounds` (rising), with soft edges a tenth wide. Neighbouring
/// classes a moss takes are one range, soft only at its ends: a moss of
/// wet and mesic places fits fully where they meet.
fn class_fit(value: f64, bounds: &[f64], allowed: &[bool]) -> f64 {
    let mut best: f64 = 0.0;
    let mut index = 0;
    while index < allowed.len() {
        if !allowed[index] {
            index += 1;
            continue;
        }
        let first = index;
        while index + 1 < allowed.len() && allowed[index + 1] {
            index += 1;
        }
        let (low, high) = (bounds[first], bounds[index + 1]);
        let below = if first == 0 {
            1.0
        } else {
            math::smoothstep(low - 0.08, low + 0.08, value)
        };
        let above = if index + 1 == allowed.len() {
            1.0
        } else {
            math::smoothstep(high + 0.08, high - 0.08, value)
        };
        best = best.max(below * above);
        index += 1;
    }
    best
}

/// Every moss genus in `library` whose files state a `colony_form`.
///
/// # Errors
///
/// On a genus whose traits fail [`MossKind::from_traits`].
pub fn kinds(library: &Library) -> Result<Vec<MossKind>, String> {
    let mut found = Vec::new();
    for rank in library.ranks() {
        if rank.rank != "genus" {
            continue;
        }
        let traits = library.rank_traits(&rank.file)?;
        if traits.get("body_plan").and_then(Value::as_str) != Some("leafy")
            || !traits.contains_key("colony_form")
        {
            continue;
        }
        let family = rank.file.split('/').next().unwrap_or_default();
        let family = family
            .get(..1)
            .map(|first| first.to_uppercase() + &family[1..])
            .unwrap_or_default();
        let mut kind = MossKind::from_traits(&rank.name, &family, &traits)?;
        if let Some(shape) = traits.get("leaf_shape").and_then(Value::as_str) {
            let rule = library.rank_rule(&rank.file, "appearance.organs.leaf.shape.outline")?;
            let points = rule
                .as_ref()
                .and_then(|rule| rule.pointer(&format!("/map/leaf_shape/{shape}")))
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    format!("{}: leaf_shape `{shape}` has no outline profile", rank.name)
                })?;
            kind.outline = points
                .iter()
                .filter_map(|point| {
                    let pair = point.as_array()?;
                    Some([pair.first()?.as_f64()?, pair.get(1)?.as_f64()?])
                })
                .collect();
        }
        found.push(kind);
    }
    Ok(found)
}

/// What a point of a surface offers a moss.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Habitat {
    /// Light, 0 (deep shade) to 1 (open and turned to the sun): the sky it
    /// sees and the midday sun on it.
    pub light: f64,
    /// How wet the surface stays, 0 (baked dry) to 1 (always damp): the
    /// rain it catches, splash and damp near the ground, less what the sun
    /// dries.
    pub wetness: f64,
    pub material: u8,
    /// How acid (−1) or basic (+1) the face is ([`crate::site::Surface::reaction`]).
    pub reaction: i8,
    /// Soil lies on it (a roof's gathered grit, a gutter).
    pub soil: bool,
    /// How well spores and water stay on it: 1 on a face turned up, less
    /// on an upright one.
    pub hold: f64,
}

/// How a region of a site is grown over.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MossSettings {
    pub years: u32,
    pub seed: u64,
    /// For the sun's path; north is -Z.
    pub latitude_deg: f64,
    /// The box grown over, in the site's frame.
    pub low: [f64; 3],
    pub high: [f64; 3],
    /// Spacing of the habitat samples on each face, metres.
    pub habitat_m: f64,
    /// Spores of all the kinds that land on a square metre in a year and
    /// could start a colony where one fits.
    pub arrivals_per_m2: f64,
    /// Grow on open soil too (by default only rock, built faces, bark,
    /// wood and soil lying on a roof or ledge).
    pub ground: bool,
}

impl MossSettings {
    /// The settings for a fill tile `tile_m` metres across whose
    /// south-west corner is (x, z) `corner` in the site's frame (north is
    /// -Z, so the tile runs from `corner[1] - tile_m` to `corner[1]` in z),
    /// grown for `years` from `seed`: the tile's columns at every height,
    /// otherwise the defaults. A fill grows a tile's moss with these
    /// beside its plants ([`crate::fill::Site::from_field`]'s corner).
    #[must_use]
    pub fn over_tile(corner: [f64; 2], tile_m: f64, years: u32, seed: u64) -> Self {
        Self {
            years,
            seed,
            low: [corner[0], -1.0e9, corner[1] - tile_m],
            high: [corner[0] + tile_m, 1.0e9, corner[1]],
            ..Self::default()
        }
    }
}

impl Default for MossSettings {
    fn default() -> Self {
        Self {
            years: 30,
            seed: 1,
            latitude_deg: 45.0,
            low: [-1.0e9; 3],
            high: [1.0e9; 3],
            habitat_m: 0.1,
            arrivals_per_m2: 40.0,
            ground: false,
        }
    }
}

/// One colony.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Colony {
    /// Index into [`MossCover::kinds`].
    pub kind: u16,
    /// The site triangle it started on.
    pub triangle: u32,
    pub centre: [f64; 3],
    pub normal: [f64; 3],
    pub radius_m: f64,
    /// Years since it started.
    pub age: f64,
    /// How well its place suits it, 0 to 1: a well-placed colony grows
    /// full and bright, a struggling one thin.
    pub vigour: f64,
    /// The light where it started ([`Habitat::light`]), which sets how it
    /// looks ([`MossKind::in_light`]); covers written before it read as
    /// part shade, which changes nothing.
    #[serde(default = "part_shade")]
    pub light: f64,
}

fn part_shade() -> f64 {
    0.5
}

/// The mosses grown over a region of a site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MossCover {
    pub version: u32,
    pub settings: MossSettings,
    pub kinds: Vec<MossKind>,
    pub colonies: Vec<Colony>,
    /// The tree crowns that shaded it ([`grow_under`]), in the site's
    /// frame: each envelope's `centre_m` is (east, north) = (x, −z) of the
    /// site's origin and its heights are the site's y.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canopy: Vec<Envelope>,
}

/// A face's habitat samples.
struct Face {
    triangle: u32,
    area: f64,
    corners: [Vec3; 3],
    normal: Vec3,
    /// The spacing of its samples.
    step: f64,
    samples: Vec<(Vec3, Habitat)>,
}

/// The share of the rain along a ray through a crown's foliage that the
/// crown holds back where the foliage stops all light, less where light
/// passes. Beech crowns hold back about 19% of the rain and run 11% down
/// their trunks, spruce 39% and 3% (Nihlgård 1970, in Glime, *Bryophyte
/// Ecology* vol. 1, ch. 8, PDF p. 1482); 0.35 of a crown that stops 85%
/// of the light gives 30%.
const RAIN_HELD: f64 = 0.35;

/// How much rain a crown sheds off its rim, as a share of open rain, on a
/// point below a crown whose foliage stops all light: crowns drip most
/// under their edges, where Tamm (1953, in Goffinet and Shaw, *Bryophyte
/// Biology* pp. 310, 426) found feather moss growing fastest. Estimate.
const RIM_DRIP: f64 = 0.3;

/// The extra rain the crowns of `canopy` drip on `p` below them: most
/// under each rim, falling off over three tenths of its radius either
/// side, scaled by how much light the crown's full depth stops.
fn crown_drip(canopy: &[Envelope], p: Vec3) -> f64 {
    canopy
        .iter()
        .filter(|crown| p.y < crown.base_m)
        .map(|crown| {
            let (dx, dz) = (p.x - crown.centre_m[0], p.z + crown.centre_m[1]);
            let from_rim =
                (math::sqrt(dx * dx + dz * dz) - crown.radius_m) / (0.3 * crown.radius_m.max(0.1));
            let stops = 1.0 - math::exp(-crown.density * (crown.top_m - crown.base_m).max(0.0));
            RIM_DRIP * stops * math::exp(-from_rim * from_rim)
        })
        .sum()
}

/// Habitat at `p` on a face of normal `n`.
#[allow(clippy::too_many_arguments)]
fn habitat_at(
    site: &SiteMesh,
    tracer: &Tracer<'_>,
    sky_rays: &[Vec3],
    rain_rays: &[Vec3],
    suns: &[Vec3],
    canopy: &[Envelope],
    p: Vec3,
    n: Vec3,
    material: u8,
    reaction: i8,
    soil: bool,
) -> Habitat {
    let from = p + n * 0.004;
    // The share of light along `ray` that comes through the canopy's
    // crowns (Beer-Lambert, as plant light).
    let through = |ray: Vec3| {
        canopy
            .iter()
            .fold(1.0, |light, crown| light * crown.transmission(from, ray))
    };
    let sky = if canopy.is_empty() {
        tracer.sky_seen(from, n, sky_rays, 30.0)
    } else {
        let (tangent, bitangent) = frame(n);
        let open: f64 = sky_rays
            .iter()
            .map(|d| (tangent * d.x + n * d.y + bitangent * d.z).normalize_or(n))
            .filter(|ray| ray.y > -0.02 && !tracer.blocked(from, *ray, 30.0))
            .map(through)
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let share = open / sky_rays.len().max(1) as f64;
        share
    };
    // An upright face's rays turned to it reach at most half the sky.
    let sky_share = (sky / (0.5 + 0.5 * n.y.max(0.0))).min(1.0);
    let mut sun = 0.0;
    for s in suns {
        let facing = n.dot(*s);
        if facing > 0.0 && !tracer.blocked(from, *s, 40.0) {
            sun += facing * through(*s);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let sun = (sun / suns.len().max(1) as f64 / 0.8).min(1.0);
    let open_rays: Vec<Vec3> = rain_rays
        .iter()
        .copied()
        .filter(|ray| !tracer.blocked(from, *ray, 30.0))
        .collect();
    #[allow(clippy::cast_precision_loss)]
    let open_above = open_rays.len() as f64 / rain_rays.len().max(1) as f64;
    // Under a canopy, each crown a rain ray falls through holds back part
    // of it, the more the denser the foliage on its way, and sheds part
    // of what it catches off its rim (`crown_rain`).
    #[allow(clippy::cast_precision_loss)]
    let rained = if canopy.is_empty() {
        open_above
    } else {
        let through_rain: f64 = open_rays
            .iter()
            .map(|ray| 1.0 - RAIN_HELD * (1.0 - through(*ray)))
            .sum();
        through_rain / rain_rays.len().max(1) as f64 + crown_drip(canopy, p) * open_above
    };
    // Rain falls on faces turned up; wind drives a little onto walls.
    let rain = rained * (n.y.max(0.0) + 0.15 * (1.0 - n.y.abs()));
    let ground = site
        .footing(p.x, p.z, Some(p.y - 0.01))
        .map_or(p.y - 10.0, |(height, _)| height);
    let damp = math::exp(-(p.y - ground).max(0.0) / 0.3);
    let wetness =
        (0.42 + 0.38 * rain + 0.35 * damp - 0.45 * sun + 0.15 * (1.0 - sky_share)).clamp(0.0, 1.0);
    let light = (0.4 * sky_share + 0.6 * sun).clamp(0.0, 1.0);
    Habitat {
        light,
        wetness,
        material,
        reaction,
        soil,
        // A face that sees no sky is pressed against another (a brick's
        // back against its mortar) or shut in: nothing settles there.
        hold: if sky < 0.01 && open_above <= 0.0 {
            0.0
        } else {
            0.15 + 0.85 * n.y.max(0.0)
        },
    }
}

/// How far `p`, on a face of normal `n`, lies in a nook: the share of
/// short rays along the face, tilted a little off it, that meet another
/// face within 3 cm.
fn nook(tracer: &Tracer<'_>, p: Vec3, n: Vec3) -> f64 {
    let (t, b) = frame(n);
    let from = p + n * 0.002;
    let mut hits = 0;
    let count = 8;
    for k in 0..count {
        let angle = f64::from(k) / f64::from(count) * std::f64::consts::TAU;
        let ray = (t * math::cos(angle) + b * math::sin(angle) + n * 0.15).normalize_or(n);
        if tracer.blocked(from, ray, 0.03) {
            hits += 1;
        }
    }
    // An inner corner turns about three rays of eight; a joint or a slot
    // more.
    (f64::from(hits) / 3.0).min(1.0)
}

/// Points over a triangle at about `step` apart, its centre at least.
fn lattice(corners: [Vec3; 3], normal: Vec3, step: f64) -> Vec<Vec3> {
    let [a, b, c] = corners;
    let e1 = (b - a).normalize_or(Vec3::X);
    let e2 = normal.cross(e1).normalize_or(Vec3::Z);
    let to2 = |p: Vec3| [(p - a).dot(e1), (p - a).dot(e2)];
    let (pa, pb, pc) = (to2(a), to2(b), to2(c));
    let low = [pa[0].min(pb[0]).min(pc[0]), pa[1].min(pb[1]).min(pc[1])];
    let high = [pa[0].max(pb[0]).max(pc[0]), pa[1].max(pb[1]).max(pc[1])];
    let inside = |q: [f64; 2]| {
        let side = |p: [f64; 2], r: [f64; 2]| {
            (r[0] - p[0]) * (q[1] - p[1]) - (r[1] - p[1]) * (q[0] - p[0])
        };
        let (s1, s2, s3) = (side(pa, pb), side(pb, pc), side(pc, pa));
        (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0)
    };
    let mut points = vec![(a + b + c) * (1.0 / 3.0)];
    let mut u = low[0] + 0.5 * step;
    while u < high[0] {
        let mut v = low[1] + 0.5 * step;
        while v < high[1] {
            if inside([u, v]) {
                points.push(a + e1 * u + e2 * v);
            }
            v += step;
        }
        u += step;
    }
    points
}

/// The suns of a growing season's day at `latitude_deg`: morning, noon
/// and afternoon at the equinox, toward the equator.
fn suns(latitude_deg: f64) -> Vec<Vec3> {
    let toward = if latitude_deg >= 0.0 { 1.0 } else { -1.0 };
    let noon = math::radians((90.0 - latitude_deg.abs()).clamp(5.0, 89.0));
    let low = noon * 0.55;
    let at = |azimuth_deg: f64, elevation: f64| {
        let azimuth = math::radians(azimuth_deg);
        Vec3::new(
            math::sin(azimuth) * math::cos(elevation),
            math::sin(elevation),
            toward * math::cos(azimuth) * math::cos(elevation),
        )
    };
    vec![at(-60.0, low), at(0.0, noon), at(60.0, low)]
}

/// Grow mosses of `kinds` over the faces of `site` in the settings' box.
#[must_use]
pub fn grow(site: &SiteMesh, kinds: &[MossKind], settings: &MossSettings) -> MossCover {
    grow_under(site, kinds, settings, &[])
}

/// [`grow`] under the crowns of trees that stand over the site: each sky
/// and sun ray a face's habitat casts is dimmed by the foliage it passes
/// through, so a roof or wall a tree overhangs is shadier and, drying
/// less, wetter (OSU Extension: roof mosses do best on north slopes,
/// "especially where trees shade them"). The crowns are in the site's
/// frame ([`MossCover::canopy`]); a stand's crowns give them with
/// [`crate::conditions::Crown::envelope`]. Rain is not changed: a crown
/// catches some and drips the rest.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn grow_under(
    site: &SiteMesh,
    kinds: &[MossKind],
    settings: &MossSettings,
    canopy: &[Envelope],
) -> MossCover {
    let low = Vec3::new(settings.low[0], settings.low[1], settings.low[2]);
    let high = Vec3::new(settings.high[0], settings.high[1], settings.high[2]);
    let (bottom, top) = site.bounds();
    let (low, high) = (low.max(bottom), high.min(top));
    // The faces in the box a moss could hold to.
    let mut candidates = Vec::new();
    for index in 0..site.triangle_count() {
        let corners = site.corners(index);
        let centre = (corners[0] + corners[1] + corners[2]) * (1.0 / 3.0);
        if centre.x < low.x
            || centre.y < low.y
            || centre.z < low.z
            || centre.x > high.x
            || centre.y > high.y
            || centre.z > high.z
        {
            continue;
        }
        let surface = site.surface(index);
        let normal = site.normal(index);
        let open_soil = surface.material == SOIL && surface.soil_m <= 0.0;
        if surface.clear
            || (surface.thin() && surface.thin_m < 0.03)
            || normal.y < -0.3
            || (open_soil && !settings.ground)
        {
            continue;
        }
        candidates.push(index);
    }
    let tracer = Tracer::new(site, [low.x, low.z], [high.x, high.z], 30.0);
    let surfaces = Surfaces::new(site, low, high);
    let sky_rays = hemisphere(24);
    // Rain: rays within about 25 degrees of straight up.
    let rain_rays: Vec<Vec3> = hemisphere(48)
        .into_iter()
        .filter(|ray| ray.y > 0.9)
        .collect();
    let sun_rays = suns(settings.latitude_deg);
    let faces: Vec<Face> = spread(candidates.len(), &|at| {
        let index = candidates[at];
        let corners = site.corners(index);
        let normal = site.normal(index);
        let surface = site.surface(index);
        let area = 0.5
            * (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .length();
        let step = settings.habitat_m.max((area / 2000.0).sqrt());
        // Only the open part of a face is sampled: the mortar between
        // bricks, not behind them.
        let samples = lattice(corners, normal, step)
            .into_iter()
            .filter(|p| surfaces.open_at(*p, normal))
            .map(|p| {
                (
                    p,
                    habitat_at(
                        site,
                        &tracer,
                        &sky_rays,
                        &rain_rays,
                        &sun_rays,
                        canopy,
                        p,
                        normal,
                        surface.material,
                        surface.reaction(),
                        surface.soil_m > 0.0,
                    ),
                )
            })
            .collect();
        Face {
            triangle: u32::try_from(index).unwrap_or(u32::MAX),
            area,
            corners,
            normal,
            step,
            samples,
        }
    });
    // The habitat of the face's sample nearest `p`; where none lies near
    // (a mortar face open only in narrow joints), measured at `p` itself.
    let habitat_near = |face: &Face, p: Vec3| -> Habitat {
        face.samples
            .iter()
            .map(|(q, habitat)| (q.distance(p), habitat))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .filter(|(d, _)| *d <= face.step)
            .map_or_else(
                || {
                    let surface = site.surface(face.triangle as usize);
                    habitat_at(
                        site,
                        &tracer,
                        &sky_rays,
                        &rain_rays,
                        &sun_rays,
                        canopy,
                        p,
                        face.normal,
                        surface.material,
                        surface.reaction(),
                        surface.soil_m > 0.0,
                    )
                },
                |(_, habitat)| *habitat,
            )
    };
    // The habitat where a spore or a piece of shoot lodges at `p`.
    // Spores, grit and water lodge in nooks: a joint between bricks, the
    // step under a tile's edge, an inner corner.
    let lodged = |face: &Face, p: Vec3| -> Habitat {
        let mut habitat = habitat_near(face, p);
        let nook = nook(&tracer, p, face.normal);
        habitat.hold = (habitat.hold * (0.3 + 1.4 * nook)).min(1.0);
        habitat.wetness = (habitat.wetness + 0.2 * nook).min(1.0);
        habitat
    };
    let face_of: std::collections::HashMap<u32, usize> = faces
        .iter()
        .enumerate()
        .map(|(at, face)| (face.triangle, at))
        .collect();
    let mut colonies: Vec<Colony> = Vec::new();
    let mut grid = Grid::new(0.08);
    let salt = hash_words(&[settings.seed, hash_str("moss")]);
    for year in 0..settings.years {
        // Spores land, and some start colonies.
        for face in &faces {
            let expected = face.area * settings.arrivals_per_m2;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let draws = expected.ceil() as u64;
            for k in 0..draws {
                let h = hash_words(&[salt, u64::from(year), u64::from(face.triangle), k]);
                #[allow(clippy::cast_precision_loss)]
                if unit(h) > expected - k as f64 {
                    continue;
                }
                let (mut r1, mut r2) = (unit(mix(h, 1)), unit(mix(h, 2)));
                if r1 + r2 > 1.0 {
                    (r1, r2) = (1.0 - r1, 1.0 - r2);
                }
                let [a, b, c] = face.corners;
                let p = a + (b - a) * r1 + (c - a) * r2;
                // Nothing takes on a face pressed against another.
                if surfaces.laid(p, face.normal, 0.002).is_none()
                    || !surfaces.open_at(p, face.normal)
                {
                    continue;
                }
                let habitat = lodged(face, p);
                let fits: Vec<f64> = kinds.iter().map(|kind| kind.vigour(&habitat)).collect();
                // Which kind a spore is, by how many spores each sheds and
                // how many of them fly far: the spore rain is mostly of the
                // mosses that fruit freely on small spores.
                let rain: Vec<f64> = kinds
                    .iter()
                    .zip(&fits)
                    .map(|(kind, fit)| fit * kind.spore_output() * kind.far_share())
                    .collect();
                let total: f64 = rain.iter().sum();
                if total <= 1.0e-6 {
                    continue;
                }
                let mut pick = unit(mix(h, 3)) * total;
                let mut chosen = 0;
                for (index, share) in rain.iter().enumerate() {
                    if pick < *share {
                        chosen = index;
                        break;
                    }
                    pick -= share;
                    chosen = index;
                }
                let fit = fits[chosen];
                let light = habitat.light;
                if unit(mix(h, 4)) > fit * 0.5 {
                    continue;
                }
                let start = kinds[chosen].shoot_width_m() * 1.5;
                if grid.covered(&colonies, p, start) {
                    continue;
                }
                let lie = surfaces.lie(p, 0.03, face.normal);
                let id = colonies.len();
                colonies.push(Colony {
                    kind: u16::try_from(chosen).unwrap_or(0),
                    triangle: face.triangle,
                    centre: [p.x, p.y, p.z],
                    normal: [lie.x, lie.y, lie.z],
                    radius_m: start,
                    age: 0.0,
                    vigour: fit,
                    light,
                });
                grid.insert(id, p);
                grid.widest = grid.widest.max(start);
            }
        }
        // Colonies seed the surface about them: broken shoots, gemmae and
        // the many spores that fall close to the capsule that shed them
        // (most of a moss's realized dispersal is within a few metres of
        // the parent, Lönnell et al. 2012, PLoS ONE 7: e41987; on walls
        // the pieces lodge along a seam and spread into cushions that
        // join, BBS on Tortula muralis), so moss spreads in clumps and
        // fills the gaps between its colonies.
        let parents = colonies.len();
        for i in 0..parents {
            let colony = colonies[i];
            if colony.age < 2.0 {
                continue;
            }
            let kind = &kinds[usize::from(colony.kind)];
            let expected =
                colony.vigour.sqrt() * (0.5 + colony.radius_m / 0.01) * OFFSPRING_PER_YEAR;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let draws = expected.ceil() as u64;
            for k in 0..draws {
                let h = hash_words(&[salt, u64::from(year), hash_str("offspring"), i as u64, k]);
                #[allow(clippy::cast_precision_loss)]
                if unit(h) > expected - k as f64 {
                    continue;
                }
                let angle = unit(mix(h, 1)) * std::f64::consts::TAU;
                let throw = -(1.0 - unit(mix(h, 2))).max(1.0e-9).ln() * OFFSPRING_THROW_M;
                let normal = v3(colony.normal);
                let (t, b) = frame(normal);
                let q = v3(colony.centre)
                    + (t * math::cos(angle) + b * math::sin(angle)) * (colony.radius_m + throw);
                let Some((p, _, triangle)) = surfaces.laid_on(q, normal, 0.02) else {
                    continue;
                };
                let Some(face) = face_of.get(&triangle).map(|at| &faces[*at]) else {
                    continue;
                };
                if !surfaces.open_at(p, face.normal) {
                    continue;
                }
                let start = kind.shoot_width_m() * 1.5;
                if grid.covered(&colonies, p, start) {
                    continue;
                }
                // A piece of shoot needs no protonema to take, so it
                // takes more readily than a spore.
                let here = lodged(face, p);
                let fit = kind.vigour(&here);
                if unit(mix(h, 4)) > fit.sqrt() * 0.6 {
                    continue;
                }
                let lie = surfaces.lie(p, 0.03, face.normal);
                let id = colonies.len();
                colonies.push(Colony {
                    kind: colony.kind,
                    triangle: face.triangle,
                    centre: [p.x, p.y, p.z],
                    normal: [lie.x, lie.y, lie.z],
                    radius_m: start,
                    age: 0.0,
                    vigour: fit,
                    light: here.light,
                });
                grid.insert(id, p);
                grid.widest = grid.widest.max(start);
            }
        }
        // Each colony grows out, as far as its neighbours let it.
        let grown: Vec<f64> = (0..colonies.len())
            .map(|i| {
                let colony = colonies[i];
                let kind = &kinds[usize::from(colony.kind)];
                let mut radius = colony.radius_m + kind.spread_m * (0.3 + 0.7 * colony.vigour);
                // A cushion grows to a hemisphere at most, a little wider
                // than high (FNA vol. 27, Grimmia pulvinata: hemispherical
                // cushions); older ones meet and merge with neighbours.
                let most = match kind.form {
                    ColonyForm::Cushion => (kind.height_m * 1.3).max(0.01),
                    ColonyForm::Turf => 0.25,
                    ColonyForm::Mat | ColonyForm::Weft => 0.6,
                };
                radius = radius.min(most);
                let at = v3(colony.centre);
                // It grows only where there is surface to grow over: half
                // the rim it would reach must lie on faces turned as its
                // own, a step down a tile's or a brick's edge at most.
                if radius > colony.radius_m {
                    let normal = v3(colony.normal);
                    let (t, b) = frame(normal);
                    let on = (0..6)
                        .filter(|k| {
                            let a = f64::from(*k) / 6.0 * std::f64::consts::TAU;
                            let q = at + (t * math::cos(a) + b * math::sin(a)) * radius;
                            surfaces.laid(q, normal, 0.016).is_some()
                        })
                        .count();
                    if on < 3 {
                        radius = colony.radius_m;
                    }
                }
                for j in grid.near(at, radius + grid.widest) {
                    if j == i {
                        continue;
                    }
                    let other = colonies[j];
                    let gap = at.distance(v3(other.centre));
                    // Neighbours press together and merge a little; a mat's
                    // creeping shoots run in over its neighbour's edge.
                    let overlap = match kind.form {
                        ColonyForm::Mat | ColonyForm::Weft => 0.45,
                        _ => 0.75,
                    };
                    let room = gap - overlap * other.radius_m;
                    if room < radius {
                        radius = radius.min(room.max(colony.radius_m));
                    }
                }
                radius
            })
            .collect();
        for (colony, radius) in colonies.iter_mut().zip(grown) {
            colony.radius_m = radius;
            colony.age += 1.0;
            grid.widest = grid.widest.max(radius);
        }
        // A colony in a poor place may die out.
        let before = colonies.len();
        let mut kept = Vec::with_capacity(before);
        for (index, colony) in colonies.iter().enumerate() {
            let h = hash_words(&[salt, u64::from(year), hash_str("death"), index as u64]);
            if unit(h) < 0.04 * (1.0 - colony.vigour) {
                continue;
            }
            kept.push(*colony);
        }
        if kept.len() != before {
            colonies = kept;
            grid = Grid::new(0.08);
            for (id, colony) in colonies.iter().enumerate() {
                grid.insert(id, v3(colony.centre));
                grid.widest = grid.widest.max(colony.radius_m);
            }
        }
    }
    MossCover {
        version: MOSS_COVER_VERSION,
        settings: *settings,
        kinds: kinds.to_vec(),
        colonies,
        canopy: canopy.to_vec(),
    }
}

fn mix(hash: u64, salt: u64) -> u64 {
    hash_words(&[hash, salt])
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Colonies by the cube cell their centre lies in.
struct Grid {
    cell: f64,
    cells: std::collections::HashMap<(i64, i64, i64), Vec<usize>>,
    /// The widest colony's radius: no colony reaches farther from its
    /// centre, so a search need go no farther.
    widest: f64,
}

impl Grid {
    fn new(cell: f64) -> Self {
        Self {
            cell,
            cells: std::collections::HashMap::new(),
            widest: 0.0,
        }
    }

    fn key(&self, p: Vec3) -> (i64, i64, i64) {
        (
            math::floor_index(p.x / self.cell),
            math::floor_index(p.y / self.cell),
            math::floor_index(p.z / self.cell),
        )
    }

    fn insert(&mut self, id: usize, p: Vec3) {
        let key = self.key(p);
        self.cells.entry(key).or_default().push(id);
    }

    /// Colonies whose centres lie within about `reach` of `p`, in the
    /// order they were added.
    fn near(&self, p: Vec3, reach: f64) -> Vec<usize> {
        #[allow(clippy::cast_possible_truncation)]
        let n = (reach / self.cell).ceil() as i64;
        let (x, y, z) = self.key(p);
        let mut found = Vec::new();
        for i in -n..=n {
            for j in -n..=n {
                for k in -n..=n {
                    if let Some(ids) = self.cells.get(&(x + i, y + j, z + k)) {
                        found.extend_from_slice(ids);
                    }
                }
            }
        }
        found.sort_unstable();
        found
    }

    fn covered(&self, colonies: &[Colony], p: Vec3, gap: f64) -> bool {
        self.near(p, self.widest + gap).into_iter().any(|id| {
            let colony = &colonies[id];
            v3(colony.centre).distance(p) < colony.radius_m + gap
        })
    }
}

/// How finely colonies are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Every colony a low patch of a few tens of triangles, its rim and
    /// height only, for far views and low settings.
    Patch,
    /// Every colony a shell: its rim frayed into outlying tufts, its top
    /// in clumps of shoots, a few hundred triangles.
    Shell,
    /// Every colony with its shoots.
    Shoots,
    /// Shoots within twice the near sphere's radius, shells out to
    /// [`PATCH_BEYOND`] times it, patches farther.
    Near,
}

/// How many times the near sphere's radius out [`Detail::Near`] draws
/// shells before it draws patches.
pub const PATCH_BEYOND: f64 = 20.0;

/// How a [`MossCover`] is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawOptions {
    /// 0 dry (leaves folded, dull, hair-points showing) to 1 just rained
    /// on (leaves spread, bright).
    pub hydration: f64,
    pub detail: Detail,
    /// The centre and radius of the sphere drawn with shoots when
    /// `detail` is [`Detail::Near`].
    pub near: Option<(Vec3, f64)>,
    /// Draw capsules on their setae.
    pub capsules: bool,
}

impl Default for DrawOptions {
    fn default() -> Self {
        Self {
            hydration: 0.4,
            detail: Detail::Shell,
            near: None,
            capsules: true,
        }
    }
}

/// The site's triangles in a box, by the cube cell their bounds (padded by
/// [`Surfaces::PAD`]) touch, for laying points on the nearest face fast.
pub(crate) struct Surfaces<'a> {
    site: &'a SiteMesh,
    cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
    /// Every face, open, pressed or kept clear, for telling which is which.
    all: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
}

/// Whether `p`, on a face turned to `n`, lies within a millimetre behind a
/// face turned the other way (other than face `skip`).
fn pressed_at(
    site: &SiteMesh,
    cells: &std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
    p: Vec3,
    n: Vec3,
    skip: u32,
) -> bool {
    let probe = p + n * 5.0e-4;
    let key = |v: f64| math::floor_index(v / Surfaces::CELL);
    cells
        .get(&(key(probe.x), key(probe.y), key(probe.z)))
        .is_some_and(|others| {
            others.iter().any(|&other| {
                if other == skip || site.normal(other as usize).dot(n) > -0.5 {
                    return false;
                }
                let [a, b, c] = site.corners(other as usize);
                closest_on_triangle(probe, a, b, c).distance(probe) < 1.0e-3
            })
        })
}

impl<'a> Surfaces<'a> {
    const CELL: f64 = 0.2;
    /// The farthest a point is laid from: more than any reach asked.
    const PAD: f64 = 0.05;

    pub(crate) fn new(site: &'a SiteMesh, low: Vec3, high: Vec3) -> Self {
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> =
            std::collections::HashMap::new();
        let pad = Vec3::new(Self::PAD, Self::PAD, Self::PAD);
        let (low, high) = (low - pad, high + pad);
        for index in 0..site.triangle_count() {
            let [a, b, c] = site.corners(index);
            let (l, h) = (a.min(b).min(c) - pad, a.max(b).max(c) + pad);
            if l.x > high.x
                || l.y > high.y
                || l.z > high.z
                || h.x < low.x
                || h.y < low.y
                || h.z < low.z
            {
                continue;
            }
            let (l, h) = (l.max(low), h.min(high));
            let key = |v: f64| math::floor_index(v / Self::CELL);
            for x in key(l.x)..=key(h.x) {
                for y in key(l.y)..=key(h.y) {
                    for z in key(l.z)..=key(h.z) {
                        cells
                            .entry((x, y, z))
                            .or_default()
                            .push(u32::try_from(index).unwrap_or(u32::MAX));
                    }
                }
            }
        }
        // A face pressed flat against another (a tile's underside on the
        // deck, a brick's back on its mortar) is no surface to grow on:
        // nearly all of it lies a hair's breadth behind a face turned the
        // other way. A face only partly covered (the mortar showing in the
        // joints between bricks) stays.
        let all = cells.clone();
        let pressed: std::collections::BTreeSet<u32> = cells
            .values()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<u32>>()
            .into_iter()
            .filter(|&index| {
                let [a, b, c] = site.corners(index as usize);
                let n = site.normal(index as usize);
                let area = 0.5 * (b - a).cross(c - a).length();
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let m = ((area.sqrt() / 0.02).ceil() as usize).clamp(1, 48);
                // Scattered samples, at random within their cells, so as
                // not to fall in step with the courses of a wall; those
                // outside the box looked at do not count.
                let mut samples = 0_usize;
                let mut covered = 0_usize;
                for i in 0..m {
                    for j in 0..m - i {
                        let h = hash_words(&[u64::from(index), i as u64, j as u64]);
                        #[allow(clippy::cast_precision_loss)]
                        let (mut u, mut v) = (
                            (i as f64 + unit(h)) / m as f64,
                            (j as f64 + unit(mix(h, 1))) / m as f64,
                        );
                        if u + v > 1.0 {
                            (u, v) = (1.0 - u, 1.0 - v);
                        }
                        let p = a + (b - a) * u + (c - a) * v;
                        let key = |x: f64| math::floor_index(x / Self::CELL);
                        if !all.contains_key(&(key(p.x), key(p.y), key(p.z))) {
                            continue;
                        }
                        samples += 1;
                        if pressed_at(site, &all, p, n, index) {
                            covered += 1;
                        }
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                let share = covered as f64 / samples.max(1) as f64;
                share >= 0.95
            })
            .collect();
        // Nor is moss ever laid on what is kept clear.
        for list in cells.values_mut() {
            list.retain(|&index| !pressed.contains(&index) && !site.surface(index as usize).clear);
        }
        Self { site, cells, all }
    }

    /// Whether `p`, on a face turned to `n`, lies open to the air rather
    /// than pressed against another face (behind a brick, under a tile).
    pub(crate) fn open_at(&self, p: Vec3, n: Vec3) -> bool {
        !pressed_at(self.site, &self.all, p, n, u32::MAX)
    }

    /// `q` laid on a face within `reach` that faces about as `normal`
    /// does: the point there and the face's normal. Of the faces straight
    /// above or below `q` it takes the outermost, the one open to the air
    /// (a tile, not the deck under it); with none so, the nearest.
    pub(crate) fn laid(&self, q: Vec3, normal: Vec3, reach: f64) -> Option<(Vec3, Vec3)> {
        self.laid_on(q, normal, reach).map(|(p, n, _)| (p, n))
    }

    /// [`Surfaces::laid`], with the index of the face laid on.
    pub(crate) fn laid_on(&self, q: Vec3, normal: Vec3, reach: f64) -> Option<(Vec3, Vec3, u32)> {
        let key = |v: f64| math::floor_index(v / Self::CELL);
        let triangles = self.cells.get(&(key(q.x), key(q.y), key(q.z)))?;
        let reach = reach.min(Self::PAD);
        let mut outer: Option<(f64, Vec3, Vec3, u32)> = None;
        let mut nearest: Option<(f64, Vec3, Vec3, u32)> = None;
        for &index in triangles {
            let face_normal = self.site.normal(index as usize);
            if face_normal.dot(normal) < 0.75 {
                continue;
            }
            let [a, b, c] = self.site.corners(index as usize);
            let p = closest_on_triangle(q, a, b, c);
            let d = p.distance(q);
            if d > reach {
                continue;
            }
            let offset = p - q;
            let along = offset.dot(normal);
            if (offset - normal * along).length() < 1.0e-4 {
                if outer.is_none_or(|(e, _, _, _)| along > e) {
                    outer = Some((along, p, face_normal, index));
                }
            } else if nearest.is_none_or(|(e, _, _, _)| d < e) {
                nearest = Some((d, p, face_normal, index));
            }
        }
        outer.or(nearest).map(|(_, p, n, index)| (p, n, index))
    }

    /// `q` laid as [`Surfaces::laid`] does, then lifted to the highest of
    /// the surface within `bridge` round it: a colony grown over a narrow
    /// hollow (a mortar joint) bridges it, and one at a step (a tile's
    /// edge) runs a little way out over the lower side.
    pub(crate) fn lifted(
        &self,
        q: Vec3,
        normal: Vec3,
        reach: f64,
        bridge: f64,
    ) -> Option<(Vec3, Vec3)> {
        let (p, n) = self.laid(q, normal, reach)?;
        let (t, b) = frame(normal);
        let mut lift: f64 = 0.0;
        for side in [t, -t, b, -b] {
            if let Some((other, _)) = self.laid(q + side * bridge, normal, reach) {
                lift = lift.max((other - p).dot(normal));
            }
        }
        Some((p + normal * lift, n))
    }

    /// The way the surface about `q` mostly faces, within `radius`:
    /// `fallback` (the face `q` lies on) when it is turned within about
    /// 50 degrees of the faces' mean normal, each weighted by its area;
    /// otherwise the face turned most nearly as that mean. A spore lodged on the narrow step under
    /// a tile's edge grows out over the tile below, not up the step.
    pub(crate) fn lie(&self, q: Vec3, radius: f64, fallback: Vec3) -> Vec3 {
        let key = |v: f64| math::floor_index(v / Self::CELL);
        let Some(triangles) = self.cells.get(&(key(q.x), key(q.y), key(q.z))) else {
            return fallback;
        };
        let cap = 4.0 * std::f64::consts::PI * radius * radius;
        let mut near = Vec::new();
        let mut mean = Vec3::ZERO;
        for &index in triangles {
            let [a, b, c] = self.site.corners(index as usize);
            if closest_on_triangle(q, a, b, c).distance(q) > radius {
                continue;
            }
            let n = self.site.normal(index as usize);
            let area = ((b - a).cross(c - a).length() * 0.5).min(cap);
            mean = mean + n * area;
            near.push(n);
        }
        let mean = mean.normalize_or(fallback);
        // A face that is itself turned much as the surface round it is
        // kept: the top of a slab near its edge stays the top.
        if fallback.dot(mean) >= 0.6 {
            return fallback;
        }
        near.into_iter()
            .max_by(|x, y| x.dot(mean).total_cmp(&y.dot(mean)))
            .unwrap_or(fallback)
    }
}

/// Value noise on the plane, 0 to 1, one lattice cell per unit.
fn lattice_noise(seed: u64, x: f64, y: f64) -> f64 {
    let (ix, iy) = (math::floor_index(x), math::floor_index(y));
    #[allow(clippy::cast_precision_loss)]
    let (fx, fy) = (x - ix as f64, y - iy as f64);
    #[allow(clippy::cast_sign_loss)]
    let at = |i: i64, j: i64| unit(hash_words(&[seed, i as u64, j as u64]));
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * sx;
    let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// Value noise round a circle: smooth, about 1 on average.
fn ring_noise(seed: u64, angle: f64, lobes: u32) -> f64 {
    let mut total = 0.0;
    let mut weight = 0.0;
    for k in 1..=lobes {
        let phase = unit(mix(seed, u64::from(k))) * std::f64::consts::TAU;
        let amp = 1.0 / f64::from(k);
        total += amp * math::sin(f64::from(k + 1) * angle + phase);
        weight += amp;
    }
    total / weight.max(1.0e-9)
}

struct Drawer<'a> {
    surfaces: &'a Surfaces<'a>,
    mesh: Mesh,
    options: DrawOptions,
}

/// Hyaline awns and leaf tips, massed: a light grey, as dry Racomitrium
/// and Grimmia cushions look hoary rather than white.
const HYALINE: [f32; 3] = [0.5, 0.51, 0.49];

/// Hyaline tips and awns massed over a colony, seen from afar, with the
/// shade among them: a silvery grey about as bright as the green, so
/// silver moss reads greyer, not brighter, than its neighbours (photos of
/// Bryum argenteum: as bright as Kindbergia's, less saturated).
const HYALINE_MASSED: [f32; 3] = [0.2, 0.205, 0.195];

/// How far apart a cushion's or turf's shoots stand, in leaf lengths:
/// close enough that their leafy tips close into one velvety surface, as
/// they do in photos of Dicranoweisia, Tortula and Grimmia cushions
/// (imagekit batch `moss-pnw-2026-11-05`, 24 Washington photos a genus).
/// Fitted to those photos by eye (Jay, 2026-11-08: no published shoot
/// count was found): at 0.7 the tips stood apart as spiky tufts, at 0.5
/// they close.
const SHOOT_SPACING: f64 = 0.5;

/// How much darker a colony's shell is than its lit leaves: the shade
/// among the shoots that a shell does not draw.
const CANOPY_SHADE: f64 = 0.62;

fn scaled(c: [f32; 3], k: f64) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    let k = k as f32;
    [c[0] * k, c[1] * k, c[2] * k]
}

fn blend(a: [f32; 3], b: [f32; 3], t: f64) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    let t = t.clamp(0.0, 1.0) as f32;
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

impl Drawer<'_> {
    fn tri(&mut self, p: [Vec3; 3], c: [[f32; 3]; 3], two_sided: bool) {
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Y);
        let base = u32::try_from(self.mesh.positions.len()).unwrap_or(u32::MAX);
        for k in 0..3 {
            self.mesh.push(Vertex::plain(p[k], n, c[k]));
        }
        self.mesh.indices.extend([base, base + 1, base + 2]);
        if two_sided {
            let base = u32::try_from(self.mesh.positions.len()).unwrap_or(u32::MAX);
            for k in [0, 2, 1] {
                self.mesh.push(Vertex::plain(p[k], -n, c[k]));
            }
            self.mesh.indices.extend([base, base + 1, base + 2]);
        }
    }

    /// The leaf colour of `kind` at the moment's hydration. Wet leaves
    /// are darker and a deeper green, as water fills the spaces between
    /// them and their cells; drying, a moss reflects more across the
    /// visible and turns duller, yellower and paler (moss crusts reflect
    /// more dry than wet over 400-900 nm, Remote Sens. 12: 1158, 2020;
    /// woolly Racomitrium light grey dry, dark green moist). A struggling
    /// colony is yellower and browner.
    fn leaf_colour(&self, kind: &MossKind, vigour: f64) -> [f32; 3] {
        leaf_colour(kind, vigour, self.options.hydration)
    }

    /// The colour a shell shows from afar: the leaves, whitened by the
    /// hair-points and hyaline tips that show the more when it is dry
    /// (FNA vol. 27, Grimmia pulvinata: the awns entwine when dry), and
    /// darkened by the shade among its shoots, which a shell does not
    /// draw: a moss canopy reflects about half what its lit leaves do
    /// (canopy 0.05-0.11 in the visible, Lovelock and Robinson 2002,
    /// against about 0.13 for a leaf).
    fn shell_colour(&self, kind: &MossKind, vigour: f64) -> [f32; 3] {
        shell_colour(kind, vigour, self.options.hydration)
    }

    #[allow(clippy::too_many_lines)]
    #[allow(clippy::fn_params_excessive_bools)]
    fn colony(&mut self, kind: &MossKind, colony: &Colony, seed: u64, shoots: bool, patch: bool) {
        let centre = v3(colony.centre);
        let normal = v3(colony.normal).normalize_or(Vec3::Y);
        let (t, b) = frame(normal);
        let radius = colony.radius_m;
        // A small colony is as high as it is wide at most.
        let height = match kind.form {
            ColonyForm::Cushion => kind.height_m.min(radius * 0.95),
            ColonyForm::Turf => kind.height_m.min(radius * 0.8),
            ColonyForm::Mat | ColonyForm::Weft => kind.height_m.min(radius * 0.5),
        } * (0.6 + 0.4 * colony.vigour.clamp(0.0, 1.0));
        let lobes = match kind.form {
            ColonyForm::Cushion => 3,
            ColonyForm::Turf => 5,
            ColonyForm::Mat | ColonyForm::Weft => 7,
        };
        let roughness = match kind.form {
            ColonyForm::Cushion => 0.12,
            ColonyForm::Turf => 0.22,
            ColonyForm::Mat | ColonyForm::Weft => 0.35,
        };
        // A ragged rim: the colony's lobes, and finer the clumps of shoots
        // at its edge, some ahead of the rest.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let fine_lobes = ((std::f64::consts::TAU * radius / 0.006) as u32).clamp(6, 40);
        let outline = |angle: f64| {
            radius
                * (1.0
                    + roughness * ring_noise(seed, angle, lobes)
                    + 0.1 * ring_noise(mix(seed, 77), angle, fine_lobes))
        };
        let profile = |rho: f64| -> f64 {
            let rho = rho.clamp(0.0, 1.0);
            match kind.form {
                ColonyForm::Cushion => math::pow((1.0 - rho * rho).max(0.0), 0.45),
                ColonyForm::Turf => math::smoothstep(1.0, 0.7, rho) * (0.85 + 0.15 * (1.0 - rho)),
                ColonyForm::Mat | ColonyForm::Weft => math::smoothstep(1.0, 0.55, rho),
            }
        };
        // Far enough to follow a couple of steps down tiles' edges or a
        // brick's, as a colony on its own plane reaches over the courses
        // below it; only faces turned as the colony's are laid on, so it
        // does not reach round a corner.
        let reach = (height * 0.6).max(0.035);
        // Hollows narrower than this are bridged, steps overhung as far.
        // A mortar joint is 10 mm: a colony grown over it bridges it, its
        // shoots and the grit they hold filling it.
        let bridge = (height * 0.8)
            .clamp(0.003, 0.008)
            .max(radius * 0.25)
            .min(0.012);
        let surfaces = self.surfaces;
        // The shell.
        // Under a mat's shoots it is the shoots below the top ones, so it
        // is drawn fine, a few millimetres a cell, and mottled.
        let fine = shoots && matches!(kind.form, ColonyForm::Mat | ColonyForm::Weft);
        // The clumps a shell shows: a cushion's shoots crowd in tufts of a
        // few millimetres, a mat's shoots and branches lie in patches.
        let lump = match kind.form {
            ColonyForm::Cushion => (kind.shoot_width_m() * 3.0).max(0.004),
            ColonyForm::Turf => (kind.shoot_width_m() * 4.0).max(0.005),
            ColonyForm::Mat | ColonyForm::Weft => 0.009,
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (rings, sectors) = if patch {
            (
                if radius > 0.03 { 2 } else { 1 },
                ((std::f64::consts::TAU * radius / 0.012).ceil() as usize).clamp(6, 16),
            )
        } else if fine {
            (
                ((radius / 0.0025).ceil() as usize).clamp(6, 72),
                ((std::f64::consts::TAU * radius / 0.0025).ceil() as usize).clamp(10, 480),
            )
        } else {
            // Fine enough to show its clumps, about two cells a clump.
            // and no more than five rings for a large one, whose clumps
            // then show in its colours more than its shape.
            let cell = (lump * 0.55).max(0.004).max(radius / 5.0);
            (
                ((radius / cell).ceil() as usize).clamp(3, 5),
                ((std::f64::consts::TAU * radius / cell).ceil() as usize).clamp(8, 32),
            )
        };
        // Colonies differ a little, as clones and their places do.
        let shell_colour = scaled(
            self.shell_colour(kind, colony.vigour),
            0.86 + 0.28 * unit(mix(seed, 3)),
        );
        // Drawn with its shoots, the shell is only the dark depth among
        // them: a mat's well down, so its creeping stems lie on it.
        let mat = matches!(kind.form, ColonyForm::Mat | ColonyForm::Weft);
        // A cushion's or turf's sits three quarters of a leafy tip below
        // the tips, so the tips stand clear of it.
        let tip_length = kind.leaf_length_m * 2.2;
        let sink = match (shoots, mat) {
            (false, _) => 1.0,
            (true, false) => (1.0 - tip_length * 0.75 / height.max(1.0e-6)).clamp(0.3, 0.85),
            (true, true) => 0.3,
        };
        // Among the shoots it shows their older, lower parts: a mat's in
        // shadow, a cushion's or turf's yellow-brown to dark brown below
        // the green tips (FNA vol. 27, Tortula; Ceratodon purpureus).
        let shell_colour = match (shoots, mat) {
            (false, _) => shell_colour,
            (true, true) => scaled(shell_colour, 0.6),
            (true, false) => blend(
                scaled(shell_colour, 0.4),
                scaled(kind.older_colour, 0.7),
                0.6,
            ),
        };
        // Its foot, at the rim, darker still.
        let base_colour = blend([0.07, 0.05, 0.025], shell_colour, 0.35);
        let mut grid: Vec<Option<(Vec3, [f32; 3])>> = Vec::with_capacity(rings * sectors + 1);
        let tip = surfaces
            .lifted(centre, normal, reach, bridge)
            .map(|(p, n)| (p + n * (height * profile(0.0) * sink), shell_colour));
        grid.push(tip);
        // Where the surface steps (a tile's edge), the shell drapes over
        // the step down, falling no steeper than it runs out, rather than
        // dropping sheer; at a step up it runs in under the overlapping
        // edge the same way.
        let mut floor: Vec<Option<f64>> = vec![
            surfaces
                .lifted(centre, normal, reach, bridge)
                .map(|(p, _)| (p - centre).dot(normal));
            sectors
        ];
        for ring in 1..=rings {
            #[allow(clippy::cast_precision_loss)]
            let rho = ring as f64 / rings as f64;
            for sector in 0..sectors {
                #[allow(clippy::cast_precision_loss)]
                let angle = sector as f64 / sectors as f64 * std::f64::consts::TAU;
                let r = outline(angle) * rho;
                let q = centre + (t * math::cos(angle) + b * math::sin(angle)) * r;
                #[allow(clippy::cast_precision_loss)]
                let run = outline(angle) / rings as f64;
                let laid = surfaces.lifted(q, normal, reach, bridge).map(|(p, n)| {
                    let below = (p - centre).dot(normal);
                    let inner = floor[sector].unwrap_or(below);
                    let p = p + normal * (below.clamp(inner - run, inner + run) - below);
                    floor[sector] = Some((p - centre).dot(normal));
                    (p, n)
                });
                let point = laid.map(|(p, n)| {
                    let h = mix(mix(seed, ring as u64), sector as u64);
                    let speckle = if fine {
                        // Mottled at the shoots' own scale, by where the
                        // point lies rather than by the grid.
                        let n = lattice_noise(
                            seed,
                            (q - centre).dot(t) / 0.0016,
                            (q - centre).dot(b) / 0.0016,
                        );
                        0.4 + 1.2 * n * n
                    } else {
                        0.92 + 0.16 * unit(h)
                    };
                    // Clumps of shoots: the shell rises and falls by them,
                    // lit on their tops and shaded between.
                    let clump = if fine || patch {
                        0.5
                    } else {
                        let u = (q - centre).dot(t) / lump;
                        let v = (q - centre).dot(b) / lump;
                        0.65 * lattice_noise(mix(seed, 5), u, v)
                            + 0.35 * lattice_noise(mix(seed, 6), u * 2.3, v * 2.3)
                    };
                    let c = blend(
                        base_colour,
                        scaled(shell_colour, speckle * (0.62 + 0.76 * clump)),
                        profile(rho).powf(0.35),
                    );
                    let rise = height * profile(rho) * sink * (0.7 + 0.6 * clump);
                    (p + n * (rise + 0.0004), c)
                });
                grid.push(point);
            }
        }
        let at = |ring: usize, sector: usize| -> usize {
            if ring == 0 {
                0
            } else {
                1 + (ring - 1) * sectors + sector % sectors
            }
        };
        for ring in 0..rings {
            for sector in 0..sectors {
                let quad = if ring == 0 {
                    vec![[at(0, 0), at(1, sector), at(1, sector + 1)]]
                } else {
                    vec![
                        [
                            at(ring, sector),
                            at(ring + 1, sector),
                            at(ring + 1, sector + 1),
                        ],
                        [
                            at(ring, sector),
                            at(ring + 1, sector + 1),
                            at(ring, sector + 1),
                        ],
                    ]
                };
                for tri in quad {
                    if let (Some(a), Some(b2), Some(c)) = (grid[tri[0]], grid[tri[1]], grid[tri[2]])
                    {
                        // Wound so the normal faces out from the surface.
                        let n = (b2.0 - a.0).cross(c.0 - a.0);
                        if n.dot(normal) >= 0.0 {
                            self.tri([a.0, b2.0, c.0], [a.1, b2.1, c.1], false);
                        } else {
                            self.tri([a.0, c.0, b2.0], [a.1, c.1, b2.1], false);
                        }
                    }
                }
            }
        }
        // Outliers: small tufts beyond the rim, young colonies and pieces
        // of shoot lodged round the parent, the more the nearer it, so a
        // colony's edge frays into the surface rather than stopping on a
        // smooth line.
        if patch {
            return;
        }
        {
            let width = kind.shoot_width_m();
            let perimeter = std::f64::consts::TAU * radius;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = (perimeter / 0.015 * colony.vigour.clamp(0.2, 1.0)).ceil() as usize;
            for k in 0..count {
                let h = mix(seed, 90_000 + k as u64);
                let angle = unit(mix(h, 1)) * std::f64::consts::TAU;
                let out = unit(mix(h, 2));
                let r = outline(angle) * 0.95 + out * out * (0.006 + radius * 0.5);
                let q = centre + (t * math::cos(angle) + b * math::sin(angle)) * r;
                let Some((p, n)) = surfaces.laid(q, normal, reach) else {
                    continue;
                };
                let size = width * (1.0 + 2.5 * unit(mix(h, 3))) * (1.0 - 0.6 * out);
                let rise = (height * 0.5).min(size * 1.2) * (1.0 - 0.5 * out);
                let colour = scaled(shell_colour, 0.7 + 0.5 * unit(mix(h, 4)));
                if shoots && !mat {
                    self.shoot_tip(kind, colony, p + n * rise, n, h);
                } else {
                    self.tuft(p, n, size, rise, colour, blend(base_colour, colour, 0.4), h);
                }
            }
        }
        // Shoots and capsules.
        let width = kind.shoot_width_m();
        let area = std::f64::consts::PI * radius * radius;
        let packing = match kind.form {
            ColonyForm::Cushion | ColonyForm::Turf => 1.0,
            ColonyForm::Mat | ColonyForm::Weft => 0.0,
        };
        // A cushion's dome or a turf's top has more surface than its
        // footprint, and its shoots stand crowded on it (FNA vol. 27,
        // Tortula muralis: shoots crowded), their leaves interleaving with
        // their neighbours': a stem every half leaf length, so a cushion
        // of Tortula holds some hundred shoots a square centimetre. No
        // book found gives a count for these mosses (Glime vol. 1 gives
        // 1000 a square decimetre only for Rhytidiadelphus triquetrus,
        // Bates 1988); `SHOOT_SPACING` is fitted to photos instead (see
        // its note).
        let surface = 1.0 + (height / radius.max(1.0e-6)).powi(2);
        let spacing = (kind.leaf_length_m * SHOOT_SPACING)
            .min(width * SHOOT_SPACING / 0.7)
            .max(0.0004);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (area * surface / (spacing * spacing) * packing).ceil() as usize;
        let mature = math::smoothstep(1.0, 3.0, colony.age);
        // `capsule_share` is the share of shoots fertile at a stem every
        // 0.7 leaf length, so as many capsules stand on a square
        // centimetre however closely the shoots are drawn.
        let capsule_spacing = (kind.leaf_length_m * 0.7).min(width).max(0.0005);
        let fertile_share = (kind.capsule_share * (spacing / capsule_spacing).powi(2)).min(1.0)
            * mature
            * colony.vigour.clamp(0.3, 1.0);
        if matches!(kind.form, ColonyForm::Mat | ColonyForm::Weft) {
            if shoots {
                self.mat(
                    kind, colony, seed, &outline, &profile, height, reach, bridge,
                );
            }
            // Capsules come with the shoots: on a shell, a millimetre or
            // two across, they would cost more triangles than it does.
            if self.options.capsules && shoots {
                // Pleurocarps raise setae from short side branches all over
                // the mat (FNA vol. 28, Hypnaceae).
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let setae = (area / (width * width * 4.0) * fertile_share).round() as usize;
                for k in 0..setae {
                    let h = mix(seed, 50_000 + k as u64);
                    let rho = unit(mix(h, 1)).sqrt() * 0.92;
                    let angle = unit(mix(h, 2)) * std::f64::consts::TAU;
                    let q = centre
                        + (t * math::cos(angle) + b * math::sin(angle)) * (outline(angle) * rho);
                    if let Some((p, n)) = surfaces.laid(q, normal, reach) {
                        let foot = p + n * (height * profile(rho) * 0.9);
                        self.sporophyte(kind, foot, n, h);
                    }
                }
            }
            return;
        }
        if !shoots {
            return;
        }
        for k in 0..count {
            let h = mix(seed, k as u64 + 7);
            #[allow(clippy::cast_precision_loss)]
            let rho = ((k as f64 + 0.5) / count as f64).sqrt();
            #[allow(clippy::cast_precision_loss)]
            let angle = k as f64 * GOLDEN + (unit(mix(h, 9)) - 0.5) * 0.3;
            let r = outline(angle) * rho;
            let radial = t * math::cos(angle) + b * math::sin(angle);
            // Shoots stand irregularly, not in the spiral's rows.
            let q = centre
                + radial * r
                + (t * (unit(mix(h, 15)) - 0.5) + b * (unit(mix(h, 16)) - 0.5)) * (width * 0.9);
            let fertile = self.options.capsules && unit(mix(h, 11)) < fertile_share;
            let Some((p, n)) = surfaces.lifted(q, normal, reach, bridge) else {
                continue;
            };
            let rise = height * profile(rho);
            // Cushion shoots radiate from the colony's centre, so those at
            // its edge lean outward; a turf's stand upright but for those
            // on its edge, which lean out over its side.
            let lean = match kind.form {
                ColonyForm::Cushion => 0.85 * rho,
                _ => 0.12 * rho + 0.6 * math::smoothstep(0.7, 1.0, rho),
            };
            let tip =
                p + n * rise * (0.92 + 0.12 * unit(mix(h, 12))) + radial * (rise * lean * 0.5);
            let axis = (n * (1.0 - lean)
                + radial * lean
                + t * ((unit(mix(h, 13)) - 0.5) * 0.25)
                + b * ((unit(mix(h, 14)) - 0.5) * 0.25))
                .normalize_or(n);
            if shoots {
                self.shoot_tip(kind, colony, tip, axis, h);
            }
            if fertile {
                // Setae rise toward the light more steeply than the
                // shoots they end lean out, and on a wall turn upward
                // (setae are negatively gravitropic).
                let up = (Vec3::Y - n * Vec3::Y.dot(n)) * 0.8;
                self.sporophyte(kind, tip, (axis + n * 1.5 + up).normalize_or(n), h);
            }
        }
    }

    /// The top of an upright shoot, ending at `tip`: its leaves in a
    /// spiral down to where the colony's shell hides them.
    fn shoot_tip(&mut self, kind: &MossKind, colony: &Colony, tip: Vec3, axis: Vec3, seed: u64) {
        let length = kind.leaf_length_m * 2.2;
        let step = kind.leaf_length_m * 0.24;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let nodes = (length / step).ceil() as usize;
        let side = any_side(axis);
        let start = unit(mix(seed, 31)) * std::f64::consts::TAU;
        // A rosulate stem's top leaves stand crowded and full size, those
        // below the rosette smaller and further apart (FNA vol. 28,
        // Rosulabryum: stems usually strongly rosulate); comose, half so.
        const ROSETTE: usize = 6;
        let crowd = kind.rosette;
        let mut down = 0.0;
        for node in 0..nodes * 2 {
            if node > 0 {
                down += step
                    * if node <= ROSETTE {
                        1.0 - 0.5 * crowd
                    } else {
                        1.0 + 0.6 * crowd
                    };
            }
            if down >= length - 1.0e-12 {
                break;
            }
            // Young leaves near the apex are smaller.
            #[allow(clippy::cast_precision_loss)]
            let size = (0.45 + 0.55 * (node as f64 / 3.0)).min(1.0)
                * if node <= ROSETTE {
                    1.0 + 0.1 * crowd
                } else {
                    1.0 - 0.45 * crowd
                };
            let base = tip - axis * down;
            #[allow(clippy::cast_precision_loss)]
            let turn = start + node as f64 * GOLDEN;
            let out = side.rotate_about(axis, turn);
            #[allow(clippy::cast_precision_loss)]
            let older = math::smoothstep(2.0, nodes as f64, node as f64);
            self.leaf(
                kind,
                colony,
                base,
                axis,
                out,
                size,
                older,
                mix(seed, 100 + node as u64),
            );
        }
    }

    /// One leaf from `base` on a stem running along `axis`, turned out
    /// toward `out`.
    #[allow(clippy::too_many_arguments)]
    fn leaf(
        &mut self,
        kind: &MossKind,
        colony: &Colony,
        base: Vec3,
        axis: Vec3,
        out: Vec3,
        size: f64,
        older: f64,
        seed: u64,
    ) {
        let wet = self.options.hydration.clamp(0.0, 1.0);
        let angle = math::radians(math::lerp(
            kind.dry_angle_deg(),
            kind.moist_angle_deg(),
            wet,
        ));
        let jitter = (unit(mix(seed, 1)) - 0.5) * 0.25;
        let dir =
            (axis * math::cos(angle + jitter) + out * math::sin(angle + jitter)).normalize_or(axis);
        let across = axis.cross(out).normalize_or(any_side(dir));
        // The blade's outer part bends: outward a little when moist; dry,
        // curled about at random (crisped), round the stem (twisted),
        // wavy (flexuose) or to one side (falcate).
        let dry = 1.0 - wet;
        let (bend, twist) = match kind.dry_posture {
            DryPosture::Crisped => (
                0.15 + dry * (unit(mix(seed, 2)) - 0.3) * 1.6,
                dry * (unit(mix(seed, 3)) - 0.5) * 2.2,
            ),
            // Spirally twisted round the stem (FNA vol. 28, Rosulabryum
            // capillare): the outer blade sweeps sideways round the stem,
            // every leaf the same way, its face turned with it.
            DryPosture::Twisted => (0.1 + dry * 0.9, dry * std::f64::consts::FRAC_PI_2),
            DryPosture::Flexuose => (0.1 + dry * (unit(mix(seed, 2)) - 0.5) * 0.9, 0.0),
            DryPosture::Falcate => (0.7 + 0.25 * dry, 0.0),
            DryPosture::Appressed | DryPosture::Imbricate => (0.12 * wet, 0.0),
        };
        let length = kind.leaf_length_m * size;
        let width = kind.leaf_width_m * size;
        // The blade bends where it is widest (its outline, leaf_shape).
        let widest = kind.blade_widest();
        let lower = length * widest;
        let mid = base + dir * lower;
        // Falcate leaves curve back toward the stem's side away from the
        // light; the others outward and down from the stem.
        let bend_axis = across.rotate_about(dir, twist);
        let upper_dir = dir.rotate_about(bend_axis, bend).normalize_or(dir);
        let apex = mid + upper_dir * (length - lower);
        let flat = across.rotate_about(dir, twist * 0.5);
        let keel = dir.cross(flat).normalize_or(axis) * (width * 0.18);
        // Older leaves down the shoot turn to the genus's older colour.
        let green = scaled(
            blend(
                self.leaf_colour(kind, colony.vigour),
                kind.older_colour,
                older * 0.85,
            ),
            0.9 + 0.2 * unit(mix(seed, 5)),
        );
        let dark = scaled(green, 0.55);
        let tip_colour = blend(green, HYALINE, kind.hyaline_share * 1.6);
        let mid_colour = blend(green, HYALINE, (kind.hyaline_share - 0.4).max(0.0) * 1.5);
        if matches!(kind.dry_posture, DryPosture::Crisped) {
            self.crisped_blade(
                base,
                dir,
                across,
                length,
                width,
                dry,
                kind.crisping,
                &|s| kind.blade_width(s),
                [dark, mid_colour, tip_colour],
                seed,
            );
            return;
        }
        // Widest at `mid`; at the base and halfway on to the tip as wide
        // as the outline says there.
        let left = mid + flat * (width * 0.5);
        let right = mid - flat * (width * 0.5);
        let centre = mid + keel;
        let foot = width * 0.5 * kind.blade_width(0.0);
        if foot > width * 0.1 {
            let (foot_left, foot_right) = (base + flat * foot, base - flat * foot);
            self.tri(
                [foot_left, left, centre],
                [dark, mid_colour, mid_colour],
                true,
            );
            self.tri([base, foot_left, centre], [dark, dark, mid_colour], true);
            self.tri([base, centre, foot_right], [dark, mid_colour, dark], true);
            self.tri(
                [foot_right, centre, right],
                [dark, mid_colour, mid_colour],
                true,
            );
        } else {
            self.tri([base, left, centre], [dark, mid_colour, mid_colour], true);
            self.tri([base, centre, right], [dark, mid_colour, mid_colour], true);
        }
        let half = (widest + 1.0) * 0.5;
        let up = mid + upper_dir * (length * (half - widest));
        let up_half = width * 0.5 * kind.blade_width(half);
        let up_colour = blend(mid_colour, tip_colour, 0.5);
        let (up_left, up_right) = (up + flat * up_half, up - flat * up_half);
        self.tri(
            [left, up_left, centre],
            [mid_colour, up_colour, mid_colour],
            true,
        );
        self.tri(
            [centre, up_left, up],
            [mid_colour, up_colour, up_colour],
            true,
        );
        self.tri(
            [centre, up, up_right],
            [mid_colour, up_colour, up_colour],
            true,
        );
        self.tri(
            [centre, up_right, right],
            [mid_colour, up_colour, mid_colour],
            true,
        );
        self.tri(
            [up_left, apex, up_right],
            [up_colour, tip_colour, up_colour],
            true,
        );
        if kind.hair_point_m > 0.0 {
            // The awn, toothed and bent a little, as a thin sliver.
            let awn = kind.hair_point_m * size * (0.8 + 0.4 * unit(mix(seed, 6)));
            let awn_dir = upper_dir
                .rotate_about(bend_axis, 0.15 * dry)
                .normalize_or(upper_dir);
            let end = apex + awn_dir * awn;
            let half = flat * (width * 0.07);
            self.tri(
                [apex - half, end, apex + half],
                [HYALINE, HYALINE, HYALINE],
                true,
            );
        }
    }

    /// A crisped leaf's blade (trait `leaf_dry_posture: crisped`): dry, its
    /// outer part curls in toward the stem a little more at every piece
    /// and twists its own way, so the leaf coils (FNA vol. 27,
    /// Dicranoweisia cirrata: leaves crisped and incurved when dry;
    /// Ceratodon purpureus: contorted or somewhat crisped when dry);
    /// moist, it straightens and spreads. Lanceolate: widest about a third
    /// of the way up, tapering to the apex.
    #[allow(clippy::too_many_arguments)]
    fn crisped_blade(
        &mut self,
        base: Vec3,
        dir: Vec3,
        across: Vec3,
        length: f64,
        width: f64,
        dry: f64,
        crisping: f64,
        outline: &dyn Fn(f64) -> f64,
        [dark, mid_colour, tip_colour]: [[f32; 3]; 3],
        seed: u64,
    ) {
        const PIECES: u32 = 5;
        // Negative turns the blade toward the stem (incurved); a leaf the
        // flora calls only somewhat crisped coils part way.
        let dry = dry * crisping;
        let curl = 0.12 * (1.0 - dry) - dry * (1.8 + 1.4 * unit(mix(seed, 7)));
        let spin = dry * (unit(mix(seed, 3)) - 0.5) * 3.2;
        let lower = 0.3;
        let half_width = |s: f64| width * 0.5 * outline(s);
        let colour = |s: f64| {
            if s < lower {
                blend(dark, mid_colour, s / lower)
            } else {
                blend(mid_colour, tip_colour, (s - lower) / (1.0 - lower))
            }
        };
        // The midrib: straight to a third of the way up, then the pieces.
        let mut rib: Vec<(Vec3, Vec3, f64)> = vec![(base, across, 0.0)];
        let mut at = base + dir * (length * lower);
        rib.push((at, across, lower));
        let mut heading = dir;
        for piece in 1..=PIECES {
            let f = f64::from(piece) / f64::from(PIECES);
            let turn_axis = across.rotate_about(dir, spin * f);
            heading = heading
                .rotate_about(turn_axis, curl / f64::from(PIECES))
                .normalize_or(heading);
            at += heading * (length * (1.0 - lower) / f64::from(PIECES));
            let s = lower + (1.0 - lower) * f;
            let flat = turn_axis.rotate_about(heading, spin * f * 0.5);
            rib.push((at, flat, s));
        }
        for pair in rib.windows(2) {
            let (a, fa, sa) = pair[0];
            let (b, fb, sb) = pair[1];
            let (wa, wb) = (half_width(sa), half_width(sb));
            let (ca, cb) = (colour(sa), colour(sb));
            let (a_left, a_right) = (a + fa * wa, a - fa * wa);
            let (b_left, b_right) = (b + fb * wb, b - fb * wb);
            self.tri([a_left, b_left, a_right], [ca, cb, ca], true);
            if wb > 1.0e-9 {
                self.tri([a_right, b_left, b_right], [ca, cb, cb], true);
            }
        }
    }

    /// A pleurocarp mat's creeping shoots, branching pinnately, laid on the
    /// surface; their leaves curve to the side away from it. The shoots
    /// start all over the mat and creep outward, each curving its own
    /// way, so they cross and interweave (FNA vol. 28, Hypnum: plants
    /// in mats, stems creeping, regularly pinnate).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn mat(
        &mut self,
        kind: &MossKind,
        colony: &Colony,
        seed: u64,
        outline: &dyn Fn(f64) -> f64,
        profile: &dyn Fn(f64) -> f64,
        height: f64,
        reach: f64,
        bridge: f64,
    ) {
        let centre = v3(colony.centre);
        let normal = v3(colony.normal).normalize_or(Vec3::Y);
        let (t, b) = frame(normal);
        let radius = colony.radius_m;
        let branch = (kind.leaf_length_m * 4.0).clamp(0.003, 0.009);
        let spacing = branch * 0.4;
        let reach_of_stem = (radius * 0.9).clamp(0.008, 0.04);
        let area = std::f64::consts::PI * radius * radius;
        // Enough shoots to lie about four deep: a leafy shoot and its side
        // branches cover about twice the leafy shoot's own width along it.
        let leafy = 2.0 * kind.leaf_length_m * math::sin(math::radians(kind.moist_angle_deg()));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let stems = ((area * 4.0 / (reach_of_stem * leafy * 2.0)).ceil() as usize).max(3);
        // Leaves crowd the creeping stems, three or four to a leaf's
        // length, as on the upright shoots.
        let step = kind.leaf_length_m * 0.28;
        let surfaces = self.surfaces;
        for stem in 0..stems {
            let h = mix(seed, 9000 + stem as u64);
            // Where it starts, in the mat's own plane.
            let r0 = unit(mix(h, 1)).sqrt() * 0.95;
            let a0 = unit(h) * std::f64::consts::TAU;
            let mut u = r0 * math::cos(a0);
            let mut v = r0 * math::sin(a0);
            // Heading: outward from the centre, turned at random.
            let mut heading = a0 + (unit(mix(h, 4)) - 0.5) * 2.2;
            // The point drawn is the stem's middle, not its start, so the
            // mat's centre is crossed by stems as much as its edge: it
            // starts half its length back, where that lies within the mat.
            let back = reach_of_stem * 0.5 / radius.max(1.0e-6);
            let (bu, bv) = (u - math::cos(heading) * back, v - math::sin(heading) * back);
            if (bu * bu + bv * bv).sqrt() < 0.9 * outline(math::atan2(bv, bu)) / radius {
                (u, v) = (bu, bv);
            }
            let curl = (unit(mix(h, 2)) - 0.5) * 2.0 / reach_of_stem;
            let mut along = 0.0;
            let mut walked = 0.0;
            let mut node = 0_u64;
            while walked < reach_of_stem {
                let rho = (u * u + v * v).sqrt();
                let angle = math::atan2(v, u);
                if rho > outline(angle) / radius {
                    break;
                }
                let q = centre + (t * u + b * v) * radius;
                let Some((p, n)) = surfaces.lifted(q, normal, reach, bridge) else {
                    break;
                };
                let lift = height * profile(rho) * (0.45 + 0.4 * unit(mix(h, 900 + node)));
                let at = p + n * lift;
                let forward_flat = t * math::cos(heading) + b * math::sin(heading);
                let forward = (forward_flat - n * forward_flat.dot(n)).normalize_or(forward_flat);
                // Leaves on the main stem.
                let out = n.rotate_about(forward, (unit(mix(h, node)) - 0.5) * 2.0);
                self.leaf(kind, colony, at, forward, out, 0.9, 0.2, mix(h, 77 + node));
                along += step;
                if along >= spacing {
                    along = 0.0;
                    let sign = if node % 2 == 0 { 1.0 } else { -1.0 };
                    // Some side branches stay short, some grow on; a few
                    // are missing.
                    let grow = unit(mix(h, 500 + node));
                    if grow >= 0.12 {
                        let side_dir = forward
                            .rotate_about(n, sign * (0.65 + 0.45 * unit(mix(h, 700 + node))))
                            .normalize_or(forward);
                        let rise = (side_dir + n * 0.2).normalize_or(side_dir);
                        let length = branch * (0.4 + grow * grow) * (1.0 - 0.3 * rho.min(1.0));
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let leaves = (length / (kind.leaf_length_m * 0.25)).ceil() as usize;
                        // Branches that curl when dry bend up from the
                        // surface and in over the stem along their length,
                        // as far as a half turn when quite dry (BBS,
                        // Homalothecium sericeum: the crowded branches
                        // curl up when dry, straight again wet).
                        let curl = if kind.branches_curl_dry {
                            0.25 + 2.6 * (1.0 - self.options.hydration.clamp(0.0, 1.0))
                        } else {
                            0.0
                        };
                        let bend_axis = rise.cross(n).normalize_or(any_side(rise));
                        let mut base = at;
                        let mut previous = 0.0;
                        for leaf in 0..leaves {
                            #[allow(clippy::cast_precision_loss)]
                            let s = (leaf as f64 + 0.5) / leaves as f64;
                            let rise = rise.rotate_about(bend_axis, curl * s).normalize_or(rise);
                            base += rise * (length * (s - previous));
                            previous = s;
                            #[allow(clippy::cast_precision_loss)]
                            let out = n.rotate_about(
                                rise,
                                (leaf as f64 * GOLDEN) % std::f64::consts::TAU - 3.1,
                            );
                            let size = 0.75 * (1.0 - 0.45 * s);
                            self.leaf(
                                kind,
                                colony,
                                base,
                                rise,
                                out,
                                size,
                                0.0,
                                mix(h, 2000 + node * 64 + leaf as u64),
                            );
                        }
                    }
                }
                node += 1;
                walked += step;
                u += math::cos(heading) * step / radius.max(1.0e-6);
                v += math::sin(heading) * step / radius.max(1.0e-6);
                heading += (curl + (unit(mix(h, 3000 + node)) - 0.5) * 60.0) * step;
            }
        }
    }

    /// A seta from `foot` along `axis` and the capsule at its end.
    #[allow(clippy::too_many_lines)]
    fn sporophyte(&mut self, kind: &MossKind, foot: Vec3, axis: Vec3, seed: u64) {
        let wet = self.options.hydration;
        let side = any_side(axis).rotate_about(axis, unit(mix(seed, 41)) * std::f64::consts::TAU);
        let length = kind.seta_m * (0.75 + 0.5 * unit(mix(seed, 42)));
        // An arcuate seta bows over in an arc, its capsule among the
        // leaves (FNA vol. 27, Grimmia: seta arcuate to cygneous; G.
        // pulvinata: seta arcuate, 3-4 mm). It bends back furthest dry and
        // stands out when moist (Wikipedia, Grimmia pulvinata: capsules
        // bend back into the leaves when dry and stand out when moist).
        // Any genus whose seta_form is arcuate does the same.
        let total_bend = if kind.seta_arcuate {
            math::radians(math::lerp(150.0, 85.0, wet.clamp(0.0, 1.0)))
        } else {
            (unit(mix(seed, 43)) - 0.5) * 0.35
        };
        let bend_axis = axis.cross(side).normalize_or(any_side(axis));
        let segments = 6;
        let mut points = vec![foot];
        let mut dir = axis;
        let mut at = foot;
        for _ in 0..segments {
            dir = dir
                .rotate_about(bend_axis, total_bend / f64::from(segments))
                .normalize_or(dir);
            at = at + dir * (length / f64::from(segments));
            points.push(at);
        }
        let seta_radius = (kind.leaf_width_m * 0.12).clamp(0.00004, 0.00012);
        for pair in points.windows(2) {
            self.tube(pair[0], pair[1], seta_radius, seta_radius, kind.seta_colour);
        }
        // The capsule: turned from the seta's end as its posture says.
        let tilt = match kind.capsule_posture {
            CapsulePosture::Erect => 0.0,
            CapsulePosture::Inclined => 0.75,
            CapsulePosture::Horizontal => 1.5,
            CapsulePosture::Pendent => 2.5,
        };
        let cap_dir = dir.rotate_about(bend_axis, tilt).normalize_or(dir);
        let slender = match kind.capsule_posture {
            CapsulePosture::Erect | CapsulePosture::Inclined if !kind.seta_arcuate => 0.2,
            _ => 0.3,
        };
        let length = kind.capsule_m;
        let radius = length * slender;
        let rings = 6;
        let sides: usize = 8;
        let start = at;
        let mut previous: Option<Vec<Vec3>> = None;
        let mut last_centre = start;
        let across = any_side(cap_dir);
        for ring in 0..=rings {
            let s = f64::from(ring) / f64::from(rings);
            // A neck tapering into the seta, the urn, then the mouth.
            let neck = if s < 0.2 { 0.6 + 2.0 * s } else { 1.0 };
            let r = radius
                * neck
                * match kind.capsule_shape {
                    CapsuleShape::Ovoid => {
                        math::sin(std::f64::consts::PI * (0.12 + 0.82 * s)).max(0.25)
                    }
                    CapsuleShape::Cylindric => 0.82 + 0.18 * math::smoothstep(0.0, 0.3, s),
                    CapsuleShape::Pyriform => 0.45 + 0.55 * math::smoothstep(0.05, 0.75, s),
                };
            let c = start + cap_dir * (length * s);
            last_centre = c;
            let ring_points: Vec<Vec3> = (0..sides)
                .map(|k| {
                    #[allow(clippy::cast_precision_loss)]
                    let a = k as f64 / sides as f64 * std::f64::consts::TAU;
                    c + across.rotate_about(cap_dir, a) * r
                })
                .collect();
            if let Some(prev) = &previous {
                for k in 0..sides {
                    let k1 = (k + 1) % sides;
                    let shade = 0.85 + 0.3 * s;
                    let col = scaled(kind.capsule_colour, shade);
                    self.tri(
                        [prev[k], ring_points[k], ring_points[k1]],
                        [col, col, col],
                        false,
                    );
                    self.tri([prev[k], ring_points[k1], prev[k1]], [col, col, col], false);
                }
            }
            previous = Some(ring_points);
        }
        // The lid, a short cone.
        if let Some(prev) = previous {
            let lid_tip = last_centre + cap_dir * (length * kind.lid_share);
            let lid = blend(kind.capsule_colour, [0.35, 0.12, 0.05], 0.4);
            for k in 0..sides {
                let k1 = (k + 1) % sides;
                self.tri([prev[k], lid_tip, prev[k1]], [lid, lid, lid], false);
            }
        }
    }

    /// A small tuft as a low five-sided dome: `size` across its foot,
    /// `rise` high, lit `top` and darker `foot`.
    #[allow(clippy::too_many_arguments)]
    fn tuft(
        &mut self,
        p: Vec3,
        n: Vec3,
        size: f64,
        rise: f64,
        top: [f32; 3],
        foot: [f32; 3],
        seed: u64,
    ) {
        let (t, b) = frame(n);
        let turn = unit(seed) * std::f64::consts::TAU;
        let apex = p + n * (rise + 0.0004);
        let ring: Vec<Vec3> = (0..5u32)
            .map(|k| {
                let a = turn + f64::from(k) / 5.0 * std::f64::consts::TAU;
                let r = size * 0.5 * (0.75 + 0.5 * unit(mix(seed, u64::from(k) + 1)));
                p + (t * math::cos(a) + b * math::sin(a)) * r + n * 0.0003
            })
            .collect();
        for k in 0..5 {
            let (a, c) = (ring[k], ring[(k + 1) % 5]);
            if (c - a).cross(apex - a).dot(n) >= 0.0 {
                self.tri([a, c, apex], [foot, foot, top], false);
            } else {
                self.tri([c, a, apex], [foot, foot, top], false);
            }
        }
    }

    fn tube(&mut self, a: Vec3, b: Vec3, ra: f64, rb: f64, colour: [f32; 3]) {
        let axis = (b - a).normalize_or(Vec3::Y);
        let side = any_side(axis);
        let sides = 4;
        let ring = |c: Vec3, r: f64| -> Vec<Vec3> {
            (0..sides)
                .map(|k| {
                    c + side.rotate_about(
                        axis,
                        f64::from(k) / f64::from(sides) * std::f64::consts::TAU,
                    ) * r
                })
                .collect()
        };
        let (p, q) = (ring(a, ra), ring(b, rb));
        for k in 0..sides as usize {
            let k1 = (k + 1) % sides as usize;
            self.tri([p[k], q[k], q[k1]], [colour; 3], false);
            self.tri([p[k], q[k1], p[k1]], [colour; 3], false);
        }
    }
}

/// The leaf colour of `kind` at `hydration` (see `Drawer::leaf_colour`).
fn leaf_colour(kind: &MossKind, vigour: f64, hydration: f64) -> [f32; 3] {
    let c = kind.colour;
    let grey = 0.3 * c[0] + 0.6 * c[1] + 0.1 * c[2];
    let wet = [c[0] * 0.8, c[1] * 0.86, c[2] * 0.8];
    let dry = blend(blend(c, [grey; 3], 0.45), [0.13, 0.11, 0.06], 0.25);
    let dry = scaled(dry, 1.08);
    let c = blend(dry, wet, hydration);
    blend([0.1, 0.08, 0.035], c, 0.45 + 0.55 * vigour)
}

/// The colour a colony shows from afar at `hydration` (see
/// `Drawer::shell_colour`).
fn shell_colour(kind: &MossKind, vigour: f64, hydration: f64) -> [f32; 3] {
    let leaf = scaled(leaf_colour(kind, vigour, hydration), CANOPY_SHADE);
    blend(leaf, HYALINE_MASSED, kind.whiteness(hydration))
}

/// A colony's ragged outline: its radius at `angle`, as its shell draws
/// it (the colony's lobes, and finer the clumps of shoots at its edge).
fn outline_radius(form: ColonyForm, radius: f64, seed: u64, angle: f64) -> f64 {
    let (lobes, roughness) = match form {
        ColonyForm::Cushion => (3, 0.12),
        ColonyForm::Turf => (5, 0.22),
        ColonyForm::Mat | ColonyForm::Weft => (7, 0.35),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fine_lobes = ((std::f64::consts::TAU * radius / 0.006) as u32).clamp(6, 40);
    radius
        * (1.0
            + roughness * ring_noise(seed, angle, lobes)
            + 0.1 * ring_noise(mix(seed, 77), angle, fine_lobes))
}

/// Moss as a tint over the site's own surfaces, with no triangles of its
/// own: the level below [`Detail::Patch`], for far views and the lowest
/// settings, where a host colours its roof's or wall's texture by the
/// moss over it ([`MossCover::tint`]).
pub struct Tint<'a> {
    cover: &'a MossCover,
    hydration: f64,
    cell: f64,
    cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>>,
}

impl MossCover {
    /// The cover as a [`Tint`] at `hydration` (0 dry to 1 just rained on).
    #[must_use]
    pub fn tint(&self, hydration: f64) -> Tint<'_> {
        let widest = self
            .colonies
            .iter()
            .map(|colony| colony.radius_m)
            .fold(0.0, f64::max);
        let cell = (widest * 1.4).max(0.05);
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<u32>> =
            std::collections::HashMap::new();
        for (index, colony) in self.colonies.iter().enumerate() {
            cells
                .entry(cell_of(v3(colony.centre), cell))
                .or_default()
                .push(u32::try_from(index).unwrap_or(u32::MAX));
        }
        Tint {
            cover: self,
            hydration: hydration.clamp(0.0, 1.0),
            cell,
            cells,
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn cell_of(p: Vec3, cell: f64) -> (i64, i64, i64) {
    (
        (p.x / cell).floor() as i64,
        (p.y / cell).floor() as i64,
        (p.z / cell).floor() as i64,
    )
}

impl Tint<'_> {
    /// The moss at `p` on a surface facing `normal`: its far colour
    /// (linear RGB) and the share of the surface it hides, 0 to 1. The
    /// share falls off over the outer fifth of the colony's ragged rim
    /// and is less where the colony struggles (its shoots stand thinner).
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn at(&self, p: Vec3, normal: Vec3) -> Option<([f32; 3], f64)> {
        let (cx, cy, cz) = cell_of(p, self.cell);
        let mut best: Option<([f32; 3], f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.cells.get(&(cx + dx, cy + dy, cz + dz)) else {
                        continue;
                    };
                    for &index in list {
                        let colony = &self.cover.colonies[index as usize];
                        let Some(kind) = self.cover.kinds.get(usize::from(colony.kind)) else {
                            continue;
                        };
                        let n = v3(colony.normal);
                        if n.dot(normal) < 0.3 {
                            continue;
                        }
                        let d = p - v3(colony.centre);
                        // Over a step or two of tiles below its plane.
                        if d.dot(n).abs() > (colony.radius_m * 0.6).max(0.035) {
                            continue;
                        }
                        let flat = d - n * d.dot(n);
                        let rho = flat.length();
                        if rho > colony.radius_m * 1.5 {
                            continue;
                        }
                        let (t, b) = frame(n);
                        let angle = flat.dot(b).atan2(flat.dot(t));
                        let seed = hash_words(&[
                            self.cover.settings.seed,
                            hash_str("colony"),
                            u64::from(index),
                        ]);
                        let edge = outline_radius(kind.form, colony.radius_m, seed, angle);
                        let share = math::smoothstep(edge, edge * 0.8, rho)
                            * (0.7 + 0.3 * colony.vigour.clamp(0.0, 1.0));
                        if share > best.map_or(0.0, |(_, s)| s) {
                            best = Some((
                                shell_colour(
                                    &kind.in_light(colony.light),
                                    colony.vigour,
                                    self.hydration,
                                ),
                                share,
                            ));
                        }
                    }
                }
            }
        }
        best
    }

    /// `mesh` (a site as drawn, its colours linear RGB) tinted by the
    /// moss: its triangles near moss split until no edge is longer than
    /// `step`, and every vertex's colour blended toward the moss over it.
    /// What a host does to its surface's texture, at `step` a texel.
    /// Only triangles reaching into the box `low` to `high` are split.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn paint(&self, mesh: &Mesh, step: f64, low: Vec3, high: Vec3) -> Mesh {
        let step = step.max(1.0e-4);
        let mut out = Mesh::default();
        let position = |i: u32| {
            let p = mesh.positions[i as usize];
            Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        };
        let mut stack: Vec<([Vec3; 3], [[f32; 4]; 3], Vec3)> = Vec::new();
        for first in (0..mesh.indices.len() / 3).map(|t| t * 3) {
            let tri = &mesh.indices[first..first + 3];
            let corners = [position(tri[0]), position(tri[1]), position(tri[2])];
            let colours = [
                mesh.colors[tri[0] as usize],
                mesh.colors[tri[1] as usize],
                mesh.colors[tri[2] as usize],
            ];
            let n = mesh.normals[tri[0] as usize];
            let normal = Vec3::new(f64::from(n[0]), f64::from(n[1]), f64::from(n[2]));
            stack.push((corners, colours, normal));
            while let Some((p, c, normal)) = stack.pop() {
                let edges = [
                    p[0].distance(p[1]),
                    p[1].distance(p[2]),
                    p[2].distance(p[0]),
                ];
                let longest = edges.iter().copied().fold(0.0, f64::max);
                let (l, h) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                let inside = l.x <= high.x
                    && l.y <= high.y
                    && l.z <= high.z
                    && h.x >= low.x
                    && h.y >= low.y
                    && h.z >= low.z;
                if longest > step && inside && self.near(p, longest) {
                    let k = (0..3)
                        .max_by(|a, b| edges[*a].total_cmp(&edges[*b]))
                        .unwrap_or(0);
                    let (a, b) = (k, (k + 1) % 3);
                    let m = (p[a] + p[b]) * 0.5;
                    let mc = [
                        f32::midpoint(c[a][0], c[b][0]),
                        f32::midpoint(c[a][1], c[b][1]),
                        f32::midpoint(c[a][2], c[b][2]),
                        c[a][3],
                    ];
                    let mut first = p;
                    let mut first_c = c;
                    first[b] = m;
                    first_c[b] = mc;
                    let mut second = p;
                    let mut second_c = c;
                    second[a] = m;
                    second_c[a] = mc;
                    stack.push((first, first_c, normal));
                    stack.push((second, second_c, normal));
                    continue;
                }
                let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
                for k in 0..3 {
                    let mut colour = [c[k][0], c[k][1], c[k][2]];
                    if let Some((moss, share)) = self.at(p[k], normal) {
                        colour = blend(colour, moss, share);
                    }
                    out.push(Vertex::plain(p[k], normal, colour));
                }
                out.indices.extend([base, base + 1, base + 2]);
            }
        }
        out
    }

    /// Whether any colony lies within `reach` of the triangle `p`.
    fn near(&self, p: [Vec3; 3], reach: f64) -> bool {
        let centre = (p[0] + p[1] + p[2]) * (1.0 / 3.0);
        let span = reach + self.cell;
        #[allow(clippy::cast_possible_truncation)]
        let cells = (span / self.cell).ceil() as i64;
        let (cx, cy, cz) = cell_of(centre, self.cell);
        if (2 * cells + 1).pow(3) > i64::try_from(self.cells.len()).unwrap_or(i64::MAX) {
            return self.cells.keys().any(|&(x, y, z)| {
                (x - cx).abs() <= cells && (y - cy).abs() <= cells && (z - cz).abs() <= cells
            });
        }
        for dx in -cells..=cells {
            for dy in -cells..=cells {
                for dz in -cells..=cells {
                    if self.cells.contains_key(&(cx + dx, cy + dy, cz + dz)) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn any_side(v: Vec3) -> Vec3 {
    math::any_perpendicular(v).normalize_or(Vec3::X)
}

impl MossCover {
    /// The colonies drawn as a mesh to render with the site
    /// (`crate::raster::Material::Opaque`).
    #[must_use]
    pub fn draw(&self, site: &SiteMesh, options: &DrawOptions) -> Mesh {
        let (low, high) = self.colonies.iter().fold(
            (
                Vec3::new(f64::MAX, f64::MAX, f64::MAX),
                Vec3::new(f64::MIN, f64::MIN, f64::MIN),
            ),
            |(low, high), colony| {
                let r = colony.radius_m * 1.5 + 0.05;
                let at = v3(colony.centre);
                (
                    low.min(at - Vec3::new(r, r, r)),
                    high.max(at + Vec3::new(r, r, r)),
                )
            },
        );
        if self.colonies.is_empty() {
            return Mesh::default();
        }
        let surfaces = Surfaces::new(site, low, high);
        let mut drawer = Drawer {
            surfaces: &surfaces,
            mesh: Mesh::default(),
            options: *options,
        };
        for (index, colony) in self.colonies.iter().enumerate() {
            let Some(kind) = self.kinds.get(usize::from(colony.kind)) else {
                continue;
            };
            let distance = options.near.map_or(0.0, |(at, _)| {
                v3(colony.centre).distance(at) - colony.radius_m
            });
            let patch = match options.detail {
                Detail::Patch => true,
                Detail::Near => options
                    .near
                    .is_some_and(|(_, reach)| distance > reach * PATCH_BEYOND),
                _ => false,
            };
            let shoots = match options.detail {
                Detail::Shell | Detail::Patch => false,
                Detail::Shoots => true,
                Detail::Near => options.near.is_some_and(|(at, reach)| {
                    v3(colony.centre).distance(at) < reach * 2.0 + colony.radius_m
                }),
            };
            let seed = hash_words(&[self.settings.seed, hash_str("colony"), index as u64]);
            drawer.colony(&kind.in_light(colony.light), colony, seed, shoots, patch);
        }
        drawer.mesh
    }

    /// Share of the grown surfaces' area each kind covers, and the
    /// colonies of each, for reports and tests.
    #[must_use]
    pub fn census(&self) -> Vec<(String, usize, f64)> {
        let mut out: Vec<(String, usize, f64)> = self
            .kinds
            .iter()
            .map(|kind| (kind.genus.clone(), 0, 0.0))
            .collect();
        for colony in &self.colonies {
            if let Some(row) = out.get_mut(usize::from(colony.kind)) {
                row.1 += 1;
                row.2 += std::f64::consts::PI * colony.radius_m * colony.radius_m;
            }
        }
        out
    }

    /// The colony whose centre lies nearest `at`, with its distance, for
    /// naming what a close view shows.
    #[must_use]
    pub fn nearest(&self, at: [f64; 3]) -> Option<(&Colony, f64)> {
        self.colonies
            .iter()
            .map(|colony| {
                let d = (v3(colony.centre) - v3(at)).length();
                (colony, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// A genus's showpiece: its widest well-grown colony (radius weighted
    /// by vigour, age three years at least, so it fruits), for review
    /// views that must show that genus and no other. `None` when the cover
    /// holds none of it.
    #[must_use]
    pub fn showpiece(&self, genus: &str) -> Option<&Colony> {
        let kind = self.kinds.iter().position(|kind| kind.genus == genus)?;
        let score = |colony: &Colony| {
            colony.radius_m * colony.vigour.clamp(0.0, 1.0) * math::smoothstep(1.0, 3.0, colony.age)
        };
        self.colonies
            .iter()
            .filter(|colony| usize::from(colony.kind) == kind)
            .max_by(|a, b| score(a).total_cmp(&score(b)))
    }

    /// The point a close view of `colony` looks at: its centre raised by
    /// half its kind's height off the face.
    #[must_use]
    pub fn view_point(&self, colony: &Colony) -> [f64; 3] {
        let height = self
            .kinds
            .get(usize::from(colony.kind))
            .map_or(0.0, |kind| kind.height_m);
        let at = v3(colony.centre) + v3(colony.normal) * (height * 0.5);
        [at.x, at.y, at.z]
    }

    /// The cover as JSON.
    ///
    /// # Errors
    ///
    /// When it cannot be written (never for a cover [`grow`] made).
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    /// A cover read from JSON.
    ///
    /// # Errors
    ///
    /// On text that is not a cover of this version.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let cover: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if cover.version != MOSS_COVER_VERSION {
            return Err(format!(
                "a moss cover of version {}, not {MOSS_COVER_VERSION}",
                cover.version
            ));
        }
        Ok(cover)
    }
}

#[cfg(test)]
mod tests {
    use super::class_fit;

    #[test]
    fn neighbouring_classes_fit_fully_where_they_meet() {
        let bounds = [0.0, 0.3, 0.55, 0.75, 1.0];
        // Wet and mesic: whole across their border, soft only past mesic.
        let wet_mesic = [true, true, false, false];
        for value in [0.0, 0.22, 0.3, 0.38, 0.46] {
            assert!((class_fit(value, &bounds, &wet_mesic) - 1.0).abs() < 1e-12);
        }
        assert!((class_fit(0.55, &bounds, &wet_mesic) - 0.5).abs() < 1e-12);
        assert!(class_fit(0.7, &bounds, &wet_mesic) < 1e-12);
        // Wet and dry with mesic between: half at each inner border.
        let wet_dry = [true, false, true, false];
        assert!((class_fit(0.3, &bounds, &wet_dry) - 0.5).abs() < 1e-12);
        assert!(class_fit(0.42, &bounds, &wet_dry) < 1e-12);
    }
}
