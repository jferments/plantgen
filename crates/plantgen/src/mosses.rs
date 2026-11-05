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
    pub seta_m: f64,
    pub seta_arcuate: bool,
    pub seta_colour: [f32; 3],
    pub capsule_m: f64,
    pub capsule_colour: [f32; 3],
    pub capsule_posture: CapsulePosture,
    /// Share of shoots bearing a capsule.
    pub capsule_share: f64,
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
            seta_m: num("seta_length_m").unwrap_or(leaf_length_m * 8.0),
            seta_arcuate: pick("seta_form").as_deref() == Some("arcuate"),
            seta_colour: get("seta_colour")
                .and_then(colour)
                .unwrap_or(linear([150, 110, 60])),
            capsule_m: num("capsule_length_m").unwrap_or(leaf_length_m),
            capsule_colour: get("capsule_colour")
                .and_then(colour)
                .unwrap_or(linear([120, 80, 45])),
            capsule_posture,
            capsule_share: num("capsule_share").unwrap_or(0.2),
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
            DryPosture::Appressed | DryPosture::Twisted => 7.0,
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
        // and wood acid, rock and soil either.
        let surface_reaction: i8 = match habitat.material {
            BUILT => 1,
            BARK | WOOD => -1,
            _ => 0,
        };
        let reaction = if self.reaction != 0 && surface_reaction == -self.reaction {
            0.35
        } else {
            1.0
        };
        // A dry face spares only the mosses that live through drying out.
        let drying = math::smoothstep(0.45, 0.15, habitat.wetness);
        let tolerance = 1.0 - drying * (1.0 - self.tolerance);
        light * wet * substrate * reaction * tolerance * habitat.hold
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

/// The share of the year a surface of `wetness` stays moist long enough
/// for mosses to grow.
fn growing_time(wetness: f64) -> f64 {
    0.08 + 0.92 * math::pow(math::smoothstep(0.2, 0.75, wetness), 1.5)
}

impl MossKind {
    /// The moist colour a colony shows from afar, for hosts that draw
    /// moss as a tint ([`MossCover::tint`]).
    #[must_use]
    pub fn far_colour(&self) -> [f32; 3] {
        blend(
            scaled(self.colour, CANOPY_SHADE),
            HYALINE,
            self.whiteness(0.5),
        )
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
/// at `bounds` (rising), with soft edges a tenth wide.
fn class_fit(value: f64, bounds: &[f64], allowed: &[bool]) -> f64 {
    let mut best: f64 = 0.0;
    for (index, &ok) in allowed.iter().enumerate() {
        if !ok {
            continue;
        }
        let (low, high) = (bounds[index], bounds[index + 1]);
        let below = if index == 0 {
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
        found.push(MossKind::from_traits(&rank.name, &family, &traits)?);
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
}

/// The mosses grown over a region of a site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MossCover {
    pub version: u32,
    pub settings: MossSettings,
    pub kinds: Vec<MossKind>,
    pub colonies: Vec<Colony>,
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

/// Habitat at `p` on a face of normal `n`.
#[allow(clippy::too_many_arguments)]
fn habitat_at(
    site: &SiteMesh,
    tracer: &Tracer<'_>,
    sky_rays: &[Vec3],
    rain_rays: &[Vec3],
    suns: &[Vec3],
    p: Vec3,
    n: Vec3,
    material: u8,
    soil: bool,
) -> Habitat {
    let from = p + n * 0.004;
    let sky = tracer.sky_seen(from, n, sky_rays, 30.0);
    // An upright face's rays turned to it reach at most half the sky.
    let sky_share = (sky / (0.5 + 0.5 * n.y.max(0.0))).min(1.0);
    let mut sun = 0.0;
    for s in suns {
        let facing = n.dot(*s);
        if facing > 0.0 && !tracer.blocked(from, *s, 40.0) {
            sun += facing;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let sun = (sun / suns.len().max(1) as f64 / 0.8).min(1.0);
    #[allow(clippy::cast_precision_loss)]
    let open_above = rain_rays
        .iter()
        .filter(|ray| !tracer.blocked(from, **ray, 30.0))
        .count() as f64
        / rain_rays.len().max(1) as f64;
    // Rain falls on faces turned up; wind drives a little onto walls.
    let rain = open_above * (n.y.max(0.0) + 0.15 * (1.0 - n.y.abs()));
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
#[allow(clippy::too_many_lines)]
pub fn grow(site: &SiteMesh, kinds: &[MossKind], settings: &MossSettings) -> MossCover {
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
                        p,
                        normal,
                        surface.material,
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
                        p,
                        face.normal,
                        surface.material,
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
                let total: f64 = fits.iter().sum();
                if total <= 1.0e-6 {
                    continue;
                }
                let mut pick = unit(mix(h, 3)) * total;
                let mut chosen = 0;
                for (index, fit) in fits.iter().enumerate() {
                    if pick < *fit {
                        chosen = index;
                        break;
                    }
                    pick -= fit;
                    chosen = index;
                }
                let fit = fits[chosen];
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
                let fit = kind.vigour(&lodged(face, p));
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
        let c = kind.colour;
        let grey = 0.3 * c[0] + 0.6 * c[1] + 0.1 * c[2];
        let wet = [c[0] * 0.8, c[1] * 0.86, c[2] * 0.8];
        let dry = blend(blend(c, [grey; 3], 0.45), [0.13, 0.11, 0.06], 0.25);
        let dry = scaled(dry, 1.08);
        let c = blend(dry, wet, self.options.hydration);
        blend([0.1, 0.08, 0.035], c, 0.45 + 0.55 * vigour)
    }

    /// The colour a shell shows from afar: the leaves, whitened by the
    /// hair-points and hyaline tips that show the more when it is dry
    /// (FNA vol. 27, Grimmia pulvinata: the awns entwine when dry), and
    /// darkened by the shade among its shoots, which a shell does not
    /// draw: a moss canopy reflects about half what its lit leaves do
    /// (canopy 0.05-0.11 in the visible, Lovelock and Robinson 2002,
    /// against about 0.13 for a leaf).
    fn shell_colour(&self, kind: &MossKind, vigour: f64) -> [f32; 3] {
        let leaf = scaled(self.leaf_colour(kind, vigour), CANOPY_SHADE);
        blend(leaf, HYALINE, kind.whiteness(self.options.hydration))
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
        // them: a cushion's or turf's sits a little below the tips, a
        // mat's well down, so its creeping stems lie on it.
        let mat = matches!(kind.form, ColonyForm::Mat | ColonyForm::Weft);
        let sink = match (shoots, mat) {
            (false, _) => 1.0,
            (true, false) => 0.8,
            (true, true) => 0.3,
        };
        // Among the shoots it shows their older, lower parts: a mat's in
        // shadow, a cushion's or turf's yellow-brown to dark brown below
        // the green tips (FNA vol. 27, Tortula; Ceratodon purpureus).
        let shell_colour = match (shoots, mat) {
            (false, _) => shell_colour,
            (true, true) => scaled(shell_colour, 0.6),
            (true, false) => blend(scaled(shell_colour, 0.4), [0.05, 0.035, 0.014], 0.6),
        };
        // Its foot, at the rim, darker still.
        let base_colour = blend([0.07, 0.05, 0.025], shell_colour, 0.35);
        let mut grid: Vec<Option<(Vec3, [f32; 3])>> = Vec::with_capacity(rings * sectors + 1);
        let tip = surfaces
            .lifted(centre, normal, reach, bridge)
            .map(|(p, n)| (p + n * (height * profile(0.0) * sink), shell_colour));
        grid.push(tip);
        for ring in 1..=rings {
            #[allow(clippy::cast_precision_loss)]
            let rho = ring as f64 / rings as f64;
            for sector in 0..sectors {
                #[allow(clippy::cast_precision_loss)]
                let angle = sector as f64 / sectors as f64 * std::f64::consts::TAU;
                let r = outline(angle) * rho;
                let q = centre + (t * math::cos(angle) + b * math::sin(angle)) * r;
                let laid = surfaces.lifted(q, normal, reach, bridge);
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
        // footprint, and the shoots' leafy tips crowd and overlap on it,
        // half as many again as would just touch.
        let surface = 1.0 + (height / radius.max(1.0e-6)).powi(2);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (area * surface * 1.5 / (width * width) * packing).ceil() as usize;
        let mature = math::smoothstep(1.0, 3.0, colony.age);
        let fertile_share = kind.capsule_share * mature * colony.vigour.clamp(0.3, 1.0);
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
            // its edge lean outward; a turf's stand upright.
            let lean = match kind.form {
                ColonyForm::Cushion => 0.85 * rho,
                _ => 0.12 * rho,
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
                // shoots they end lean out.
                self.sporophyte(kind, tip, (axis + n * 1.5).normalize_or(n), h);
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
        for node in 0..nodes {
            #[allow(clippy::cast_precision_loss)]
            let down = node as f64 * step;
            // Young leaves near the apex are smaller.
            #[allow(clippy::cast_precision_loss)]
            let size = (0.45 + 0.55 * (node as f64 / 3.0)).min(1.0);
            let base = tip - axis * down;
            #[allow(clippy::cast_precision_loss)]
            let turn = start + node as f64 * GOLDEN;
            let out = side.rotate_about(axis, turn);
            self.leaf(
                kind,
                colony,
                base,
                axis,
                out,
                size,
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
            DryPosture::Twisted => (0.1, dry * 0.9),
            DryPosture::Flexuose => (0.1 + dry * (unit(mix(seed, 2)) - 0.5) * 0.9, 0.0),
            DryPosture::Falcate => (0.7 + 0.25 * dry, 0.0),
            DryPosture::Appressed | DryPosture::Imbricate => (0.12 * wet, 0.0),
        };
        let length = kind.leaf_length_m * size;
        let width = kind.leaf_width_m * size;
        let lower = length * 0.38;
        let mid = base + dir * lower;
        // Falcate leaves curve back toward the stem's side away from the
        // light; the others outward and down from the stem.
        let bend_axis = across.rotate_about(dir, twist);
        let upper_dir = dir.rotate_about(bend_axis, bend).normalize_or(dir);
        let apex = mid + upper_dir * (length - lower);
        let flat = across.rotate_about(dir, twist * 0.5);
        let keel = dir.cross(flat).normalize_or(axis) * (width * 0.18);
        let green = scaled(
            self.leaf_colour(kind, colony.vigour),
            0.9 + 0.2 * unit(mix(seed, 5)),
        );
        let dark = scaled(green, 0.55);
        let tip_colour = blend(green, HYALINE, kind.hyaline_share * 1.6);
        let mid_colour = blend(green, HYALINE, (kind.hyaline_share - 0.4).max(0.0) * 1.5);
        let left = mid + flat * (width * 0.5);
        let right = mid - flat * (width * 0.5);
        let centre = mid + keel;
        self.tri([base, left, centre], [dark, mid_colour, mid_colour], true);
        self.tri([base, centre, right], [dark, mid_colour, mid_colour], true);
        self.tri(
            [left, apex, centre],
            [mid_colour, tip_colour, mid_colour],
            true,
        );
        self.tri(
            [centre, apex, right],
            [mid_colour, tip_colour, mid_colour],
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
                self.leaf(kind, colony, at, forward, out, 0.9, mix(h, 77 + node));
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
                        for leaf in 0..leaves {
                            #[allow(clippy::cast_precision_loss)]
                            let s = (leaf as f64 + 0.5) / leaves as f64;
                            let base = at + rise * (length * s);
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
        // An arcuate seta bends right over, its capsule bowed back among
        // the leaves, the more so when moist (FNA vol. 27, Grimmia).
        let total_bend = if kind.seta_arcuate {
            math::radians(110.0 + 40.0 * wet)
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
            let r = radius
                * math::sin(std::f64::consts::PI * (0.12 + 0.82 * s)).max(0.25)
                * if s < 0.2 { 0.6 + 2.0 * s } else { 1.0 };
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
            let lid_tip = last_centre + cap_dir * (length * 0.28);
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
            drawer.colony(kind, colony, seed, shoots, patch);
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
