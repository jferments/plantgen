//! The bark language's drawing record and the bark tiles drawn from it
//! (bark plan step 3; `plantgen-claude` `project-files/bark/LANGUAGE.md`).
//!
//! A [`BarkLook`] describes a species' bark as floras do: a list of
//! [`Stage`]s by stem diameter ("smooth with resin blisters when young,
//! becoming thick and deeply furrowed"), each a stack of layers in physical
//! units: the surface and its mottling, a fissure network with its ridge
//! profile, finer scales on the ridges, exfoliation (papery strips,
//! fibrous strips, flakes and patches whose exposed colour ages),
//! lenticels, warts and resin blisters. Fissure depth is a share of the
//! bark's thickness at the stem's diameter ([`Thickness`]), so furrows
//! deepen as a trunk grows.
//!
//! [`bake`] draws one stage at one diameter as a square tile that repeats
//! every `period` metres both round the stem and along it: height in
//! metres, linear albedo, roughness and occlusion, from which [`Tile`]
//! gives the glTF set (base colour, a tangent-space normal map with +Y up
//! the stem, occlusion-roughness-metallic) and a lit preview. Every cell,
//! strip and lenticel wraps round the tile's edges, so the tile has no
//! seam; every random number is a hash of the seed and what it is for, and
//! the transcendental functions go through `libm`, so the same record
//! draws the same bytes on every machine.
//!
//! The record is not yet read from species' specs: `plantc bark` draws it
//! from a file, so the language can be reviewed before it changes any
//! plant (the bark plan's order).

use serde::{Deserialize, Serialize};

use crate::rng::{hash_words, mix64, unit};

/// The record's schema.
pub const BARK_LOOK_SCHEMA: u32 = 1;

/// A species' bark, in the bark language's drawing terms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarkLook {
    pub schema: u32,
    /// The species' id.
    pub id: String,
    pub thickness: Thickness,
    /// The stages from the youngest, each until a stem diameter; the last
    /// has none.
    pub stages: Vec<Stage>,
    /// Where each value comes from, by its path in the record.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub evidence: std::collections::BTreeMap<String, String>,
}

/// How thick the bark is on a stem `d` metres across: `coefficient` × `d`
/// (the fire models' single bark thickness, FOFEM), or `power[0]` ×
/// `d`^`power[1]` (Rosell 2016), within `min_m` and `max_m`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thickness {
    #[serde(default)]
    pub coefficient: Option<f64>,
    #[serde(default)]
    pub power: Option<[f64; 2]>,
    #[serde(default)]
    pub min_m: f64,
    #[serde(default)]
    pub max_m: Option<f64>,
}

impl Thickness {
    /// Metres of bark on a stem `diameter` metres across.
    #[must_use]
    pub fn at(&self, diameter: f64) -> f64 {
        let d = diameter.max(0.0);
        let t = match (self.coefficient, self.power) {
            (Some(c), _) => c * d,
            (None, Some([a, b])) => a * libm::pow(d, b),
            (None, None) => 0.03 * d,
        };
        let t = t.max(self.min_m);
        self.max_m.map_or(t, |most| t.min(most))
    }
}

/// Where on a plant a stage shows first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum On {
    #[default]
    All,
    Base,
    Underside,
}

/// The four kinds of bark by how they grow (Shtein et al. 2023): whether
/// the outer layer keeps whole as the stem widens, and whether it has
/// layers that let pieces come away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    Stretched,
    Exfoliating,
    Furrowed,
    Peeling,
}

/// One stage of a bark's life.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: String,
    /// The stem diameter where the next stage takes over, metres.
    #[serde(default)]
    pub until_diameter_m: Option<f64>,
    #[serde(default)]
    pub on: On,
    pub architecture: Architecture,
    /// The outer surface's colour, sRGB.
    pub colour: [u8; 3],
    /// PBR roughness of the surface.
    #[serde(default = "matte")]
    pub roughness: f64,
    /// A waxy or chalky bloom: how far the surface whitens, 0 to 1.
    #[serde(default)]
    pub bloom: f64,
    #[serde(default)]
    pub mottle: Option<Mottle>,
    #[serde(default)]
    pub fissures: Option<Fissures>,
    #[serde(default)]
    pub scales: Option<Scales>,
    #[serde(default)]
    pub exfoliation: Option<Exfoliation>,
    #[serde(default)]
    pub lenticels: Option<Lenticels>,
    #[serde(default)]
    pub warts: Option<Bumps>,
    #[serde(default)]
    pub blisters: Option<Bumps>,
}

const fn matte() -> f64 {
    0.85
}

/// Soft patches of a different shade or colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mottle {
    /// Size of a patch, metres.
    pub scale_m: f64,
    /// How far a patch moves toward `colour` (or lighter and darker), 0 to 1.
    pub amount: f64,
    #[serde(default)]
    pub colour: Option<[u8; 3]>,
}

/// How the fissures run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    /// Parallel furrows up the stem.
    Longitudinal,
    /// Ridges that merge and split (anastomosing): oaks, ash, Douglas fir.
    Interlacing,
    /// A net of diamonds.
    Reticulate,
    /// Square or oblong blocks: checkered bark, plates.
    Tessellated,
}

/// The cross-section of a ridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Rounded,
    FlatTopped,
    Sharp,
}

/// Fissures between ridges or plates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fissures {
    pub network: Network,
    /// Ridge centre to ridge centre round the stem, metres.
    pub spacing_m: f64,
    /// A ridge or plate's length along the stem over its width.
    pub elongation: f64,
    /// The share of the surface the fissures' floors take.
    pub width_share: f64,
    /// The share of the bark's thickness the fissures cut through.
    pub depth_share: f64,
    pub profile: Profile,
    /// How far ridges bridge their fissures, 0 to 1 ("ridges connected at
    /// intervals by narrower cross ridges").
    #[serde(default)]
    pub cross_ridges: f64,
    /// The fissure floor's colour, sRGB; darker surface colour if none.
    #[serde(default)]
    pub floor_colour: Option<[u8; 3]>,
    /// The weathered ridge tops' colour, sRGB.
    #[serde(default)]
    pub ridge_top_colour: Option<[u8; 3]>,
    /// Fine grain on the ridges, as a share of the depth.
    #[serde(default = "grain")]
    pub grain: f64,
}

const fn grain() -> f64 {
    0.12
}

/// Small scales or flakes on the surface or the ridges, each lifted at its
/// lower edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scales {
    /// A scale's width round the stem, metres.
    pub size_m: f64,
    /// Its length along the stem over its width.
    pub aspect: f64,
    /// How far its edge stands proud, metres.
    pub depth_m: f64,
    /// How much scales differ in shade, 0 to 1.
    #[serde(default = "scale_variation")]
    pub variation: f64,
    /// Only on ridge tops (true) or everywhere.
    #[serde(default)]
    pub on_ridges: bool,
}

const fn scale_variation() -> f64 {
    0.12
}

/// How the outer bark comes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Thin sheets round the stem that curl at their edges (birch).
    PaperyStrips,
    /// Long strips up the stem (cedars).
    FibrousStrips,
    /// Pieces that fall and leave patches whose fresh colour ages
    /// (madrone, plane, pines' plates).
    Patches,
}

/// Exfoliation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exfoliation {
    pub mode: Mode,
    /// A piece's long side, metres.
    pub piece_m: f64,
    /// Its long side over its short side.
    pub aspect: f64,
    /// The share of the surface where a piece has come away.
    pub cover: f64,
    /// How far a loose edge lifts and curls, metres.
    #[serde(default)]
    pub curl_m: f64,
    /// Bark freshly exposed, sRGB.
    pub inner_colour: [u8; 3],
    /// The colours exposed bark passes through as it weathers, after
    /// `inner_colour`, toward the surface colour.
    #[serde(default)]
    pub aged: Vec<[u8; 3]>,
}

/// The shape of a lenticel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LenticelShape {
    Dot,
    Dash,
    Diamond,
}

/// Lenticels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lenticels {
    pub shape: LenticelShape,
    /// Long side round the stem (true) or along it.
    #[serde(default = "yes")]
    pub horizontal: bool,
    /// Length range when formed, metres.
    pub length_m: [f64; 2],
    /// Width over length.
    pub width_ratio: f64,
    pub per_m2: f64,
    /// Raised (positive) or sunken, metres.
    #[serde(default)]
    pub relief_m: f64,
    pub colour: [u8; 3],
    /// They widen round the stem as it grows (cherries, birches): length
    /// times the diameter over `formed_diameter_m`, at most 6.
    #[serde(default)]
    pub stretch: bool,
    #[serde(default = "formed")]
    pub formed_diameter_m: f64,
    /// Set in rows round the stem.
    #[serde(default)]
    pub rows: bool,
}

const fn yes() -> bool {
    true
}

const fn formed() -> f64 {
    0.02
}

/// Round bumps: corky warts or resin blisters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bumps {
    pub per_m2: f64,
    pub diameter_m: [f64; 2],
    pub height_m: f64,
    #[serde(default)]
    pub colour: Option<[u8; 3]>,
    /// Set in bands round the stem.
    #[serde(default)]
    pub rows: bool,
    /// PBR roughness on the bump; the surface's if none.
    #[serde(default)]
    pub roughness: Option<f64>,
}

/// A problem with a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarkLookError(pub String);

impl std::fmt::Display for BarkLookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl BarkLook {
    /// A record from its JSON, checked.
    ///
    /// # Errors
    ///
    /// When the JSON does not parse or a value is out of range.
    pub fn from_json(text: &str) -> Result<Self, BarkLookError> {
        let look: Self =
            serde_json::from_str(text).map_err(|error| BarkLookError(error.to_string()))?;
        look.check()?;
        Ok(look)
    }

    fn check(&self) -> Result<(), BarkLookError> {
        let fail = |what: String| Err(BarkLookError(format!("{}: {what}", self.id)));
        if self.schema != BARK_LOOK_SCHEMA {
            return fail(format!("schema {} is not {BARK_LOOK_SCHEMA}", self.schema));
        }
        if self.stages.is_empty() {
            return fail("no stages".into());
        }
        let mut last = 0.0;
        for (index, stage) in self.stages.iter().enumerate() {
            let final_stage = index + 1 == self.stages.len();
            match (stage.until_diameter_m, final_stage) {
                (Some(_), true) => return fail(format!("the last stage, {}, ends", stage.name)),
                (None, false) => return fail(format!("stage {} has no end", stage.name)),
                (Some(until), false) if until <= last => {
                    return fail(format!(
                        "stage {} ends before the one before it",
                        stage.name
                    ));
                }
                (Some(until), false) => last = until,
                (None, true) => {}
            }
            if let Some(fissures) = &stage.fissures
                && !(fissures.spacing_m > 0.0
                    && fissures.elongation > 0.0
                    && (0.0..1.0).contains(&fissures.width_share)
                    && (0.0..=1.0).contains(&fissures.depth_share))
            {
                return fail(format!("stage {}: fissures out of range", stage.name));
            }
            if let Some(scales) = &stage.scales
                && !(scales.size_m > 0.0 && scales.aspect > 0.0)
            {
                return fail(format!("stage {}: scales out of range", stage.name));
            }
            if let Some(peel) = &stage.exfoliation
                && !(peel.piece_m > 0.0 && peel.aspect >= 1.0 && (0.0..=1.0).contains(&peel.cover))
            {
                return fail(format!("stage {}: exfoliation out of range", stage.name));
            }
            if let Some(lenticels) = &stage.lenticels
                && !(lenticels.per_m2 > 0.0
                    && lenticels.length_m[0] > 0.0
                    && lenticels.length_m[1] >= lenticels.length_m[0])
            {
                return fail(format!("stage {}: lenticels out of range", stage.name));
            }
        }
        Ok(())
    }

    /// The stage a stem `diameter` metres across wears (where it shows).
    #[must_use]
    pub fn stage_at(&self, diameter: f64) -> &Stage {
        self.stages
            .iter()
            .find(|stage| stage.until_diameter_m.is_none_or(|until| diameter < until))
            .unwrap_or_else(|| &self.stages[self.stages.len() - 1])
    }

    /// A diameter typical of each stage, for previews: the geometric mean
    /// of where it starts and ends (from 1 cm; the last runs to 2.5 times
    /// its start, at least 0.6 m).
    #[must_use]
    pub fn typical_diameters(&self) -> Vec<f64> {
        let mut from = 0.01_f64;
        self.stages
            .iter()
            .map(|stage| {
                let until = stage.until_diameter_m.unwrap_or((from * 2.5).max(0.6));
                let typical = libm::sqrt(from.max(0.01) * until);
                from = until;
                typical
            })
            .collect()
    }
}

/// One stage drawn as a square tile.
#[derive(Debug, Clone)]
pub struct Tile {
    /// Texels along each side.
    pub size: usize,
    /// Metres the tile covers before it repeats, round and along the stem.
    pub period_m: f64,
    /// Row by row from the top (the highest point along the stem): linear
    /// albedo.
    pub albedo: Vec<[f64; 3]>,
    /// Metres above the outer bark's top (0 or below; lifted edges above).
    pub height: Vec<f64>,
    pub roughness: Vec<f64>,
    pub occlusion: Vec<f64>,
    /// The bark's thickness the tile was drawn for, metres.
    pub thickness_m: f64,
}

/// A stream of draws for one purpose.
#[derive(Clone, Copy)]
struct Draws(u64);

impl Draws {
    fn unit(self, index: u64, what: u64) -> f64 {
        unit(mix64(hash_words(&[self.0, index, what])))
    }

    fn stream(self, name: u64) -> Self {
        Self(hash_words(&[self.0, name]))
    }
}

/// Streams, one for each layer.
const FISSURES: u64 = 1;
const WARP: u64 = 2;
const GRAIN: u64 = 3;
const SCALES: u64 = 4;
const PEEL: u64 = 5;
const LENTICELS: u64 = 6;
const WARTS: u64 = 7;
const BLISTERS: u64 = 8;
const MOTTLE: u64 = 9;
const BRIDGES: u64 = 10;

/// Whole cells along `period` of `size` metres each, at least `least`.
fn cells_in(period: f64, size: f64, least: u64) -> u64 {
    // A tile holds at most a few thousand cells a side.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = libm::round(period / size.max(1.0e-6)) as u64;
    n.clamp(least, 4096)
}

#[allow(clippy::cast_precision_loss)] // Cell counts and indices are small.
fn float(n: u64) -> f64 {
    n as f64
}

/// Index `i` wrapped into `0..n`.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)] // Small.
fn wrap(i: i64, n: u64) -> u64 {
    i.rem_euclid(n as i64) as u64
}

/// The nearest two feature points of a periodic grid of `nx` × `ny`
/// cells over a `period`-metre tile, one point a cell jittered by `jitter`,
/// with distances measured `stretch` times shorter along the stem.
struct Near {
    first: f64,
    second: f64,
    /// The nearest point's cell.
    id: u64,
    /// From the nearest point to the place, metres (round, along).
    offset: [f64; 2],
    /// From each of the nine points searched to the place.
    points: [[f64; 2]; 9],
    nearest: usize,
    stretch: f64,
}

impl Near {
    /// Distance in metres from the place to the nearest edge of its cell,
    /// measured straight across the edge on the bark, so a fissure is as
    /// wide where it crosses the stem as where it runs up it.
    #[allow(clippy::many_single_char_names)]
    fn edge(&self) -> f64 {
        let s = self.stretch;
        let o = self.points[self.nearest];
        let a = [o[0], o[1] / s];
        let mut least = f64::INFINITY;
        for (k, o) in self.points.iter().enumerate() {
            if k == self.nearest {
                continue;
            }
            let b = [o[0], o[1] / s];
            let d = [a[0] - b[0], a[1] - b[1]];
            let len = libm::hypot(d[0], d[1]).max(1.0e-12);
            let across = (b[0] * b[0] + b[1] * b[1] - a[0] * a[0] - a[1] * a[1]) / (2.0 * len);
            least = least.min(across / libm::hypot(d[0] / len, d[1] / len / s).max(1.0e-12));
        }
        least
    }
}

#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
fn nearest(
    draws: Draws,
    nx: u64,
    ny: u64,
    period: f64,
    jitter: f64,
    stretch: f64,
    x: f64,
    y: f64,
) -> Near {
    let (cw, ch) = (period / float(nx), period / float(ny));
    // The grid holds a few thousand cells a side.
    #[allow(clippy::cast_possible_truncation)]
    let (gx, gy) = (libm::floor(x / cw) as i64, libm::floor(y / ch) as i64);
    let mut near = Near {
        first: f64::INFINITY,
        second: f64::INFINITY,
        id: 0,
        offset: [0.0; 2],
        points: [[0.0; 2]; 9],
        nearest: 0,
        stretch,
    };
    let mut k = 0;
    for dy in -1..=1_i64 {
        for dx in -1..=1_i64 {
            let (i, j) = (gx + dx, gy + dy);
            let id = wrap(i, nx) * 65_537 + wrap(j, ny);
            #[allow(clippy::cast_precision_loss)]
            let px = (i as f64 + 0.5 + jitter * (draws.unit(id, 1) - 0.5)) * cw;
            #[allow(clippy::cast_precision_loss)]
            let py = (j as f64 + 0.5 + jitter * (draws.unit(id, 2) - 0.5)) * ch;
            let (ox, oy) = (x - px, y - py);
            let d = libm::hypot(ox, oy / stretch);
            near.points[k] = [ox, oy];
            if d < near.first {
                near.second = near.first;
                near.first = d;
                near.id = id;
                near.offset = [ox, oy];
                near.nearest = k;
            } else if d < near.second {
                near.second = d;
            }
            k += 1;
        }
    }
    near
}

/// Periodic value noise on `nx` × `ny` cells over the tile, 0 to 1.
fn noise(draws: Draws, nx: u64, ny: u64, period: f64, x: f64, y: f64) -> f64 {
    let (gx, gy) = (x / period * float(nx), y / period * float(ny));
    let (fx, fy) = (libm::floor(gx), libm::floor(gy));
    // Small grid indices.
    #[allow(clippy::cast_possible_truncation)]
    let (ix, iy) = (fx as i64, fy as i64);
    let at = |i: i64, j: i64| draws.unit(wrap(i, nx) * 65_537 + wrap(j, ny), 0);
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(gx - fx), smooth(gy - fy));
    let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * sx;
    let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// Three octaves of [`noise`], 0 to 1.
fn fbm(draws: Draws, nx: u64, ny: u64, period: f64, x: f64, y: f64) -> f64 {
    let mut sum = 0.0;
    let mut weight = 1.0;
    let mut total = 0.0;
    for octave in 0..3_u64 {
        sum += weight
            * noise(
                draws.stream(octave),
                nx << octave,
                ny << octave,
                period,
                x,
                y,
            );
        total += weight;
        weight *= 0.5;
    }
    sum / total
}

fn smoothstep(low: f64, high: f64, x: f64) -> f64 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn linear(colour: [u8; 3]) -> [f64; 3] {
    colour.map(crate::texture::srgb_to_linear)
}

fn blend(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn times(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|c| c * k)
}

/// What one texel holds while the layers are drawn.
#[derive(Clone, Copy)]
struct Texel {
    colour: [f64; 3],
    height: f64,
    roughness: f64,
    /// 0 in a fissure's floor, 1 on a ridge top.
    ridge: f64,
}

/// Draw `stage` of `look` on a stem `diameter` metres across as a tile
/// `size` texels a side repeating every `period` metres, from `seed`.
#[must_use]
pub fn bake(
    look: &BarkLook,
    stage: &Stage,
    diameter: f64,
    size: usize,
    period: f64,
    seed: u64,
) -> Tile {
    let draws = Draws(hash_words(&[seed, 0x6261_726b]));
    let thickness = look.thickness.at(diameter);
    let surface = linear(stage.colour);
    let texel_m = period / crate::texture::to_f64(size);
    let mut texels = Vec::with_capacity(size * size);
    for row in 0..size {
        for column in 0..size {
            let x = (crate::texture::to_f64(column) + 0.5) * texel_m;
            // Row 0 is the top of the tile, the highest point up the stem.
            let y = (crate::texture::to_f64(size - 1 - row) + 0.5) * texel_m;
            texels.push(draw(
                stage, draws, thickness, diameter, period, surface, x, y,
            ));
        }
    }
    let deepest = texels
        .iter()
        .map(|t| t.height)
        .fold(0.0_f64, f64::min)
        .min(-1.0e-6);
    let occlusion = texels
        .iter()
        .map(|t| 1.0 - 0.65 * libm::pow((t.height / deepest).clamp(0.0, 1.0), 0.8))
        .collect();
    Tile {
        size,
        period_m: period,
        albedo: texels.iter().map(|t| t.colour).collect(),
        height: texels.iter().map(|t| t.height).collect(),
        roughness: texels.iter().map(|t| t.roughness).collect(),
        occlusion,
        thickness_m: thickness,
    }
}

/// Every layer of `stage` at `(x, y)` metres on the tile.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]
fn draw(
    stage: &Stage,
    draws: Draws,
    thickness: f64,
    diameter: f64,
    period: f64,
    surface: [f64; 3],
    x: f64,
    y: f64,
) -> Texel {
    let mut texel = Texel {
        colour: surface,
        height: 0.0,
        roughness: stage.roughness,
        ridge: 1.0,
    };
    // Fissures, ridges and plates.
    if let Some(fissures) = &stage.fissures {
        let depth = fissures.depth_share * thickness;
        let along = fissures.spacing_m * fissures.elongation;
        let nx = cells_in(period, fissures.spacing_m, 3);
        let ny = cells_in(period, along, 1);
        let (jitter, warp) = match fissures.network {
            Network::Longitudinal => (0.45, 0.25),
            Network::Interlacing => (0.85, 0.55),
            Network::Reticulate => (0.9, 0.2),
            Network::Tessellated => (0.6, 0.1),
        };
        // Ridges wander round the stem as they run up it.
        let wander = warp
            * fissures.spacing_m
            * (2.0
                * noise(
                    draws.stream(WARP),
                    nx.max(2) / 2,
                    ny.max(1) * 2,
                    period,
                    x,
                    y,
                )
                - 1.0);
        let stretch = fissures.elongation.max(0.05);
        let near = nearest(
            draws.stream(FISSURES),
            nx,
            ny,
            period,
            jitter,
            stretch,
            x + wander,
            y,
        );
        // The fissure, floor and walls, takes `width_share` of the spacing;
        // the ridge keeps the rest.
        let edge = near.edge();
        let half = 0.5 * period / float(nx);
        let w = fissures.width_share.max(0.01);
        // Plates are cells with fissures round them. Running ridges are the
        // other way round: each fissure is a long lens inside a cell, and
        // the ridges are the network between, splitting and joining.
        let lenses = matches!(
            fissures.network,
            Network::Longitudinal | Network::Interlacing
        );
        // 0 on the fissure's centre line, 1 where the ridge begins; and
        // 0 at the ridge's edge, 1 along its middle.
        let (mut wall, across) = if lenses {
            // Each lens tapers to a point toward its cell's ends.
            let reach = 0.5 * period / float(ny);
            let taper = libm::sqrt((1.0 - (near.offset[1] / reach).powi(2)).max(0.0));
            let w = (1.3 * w * taper).max(1.0e-3);
            let rim = half * (1.0 - w);
            (
                1.0 - ((edge - rim) / (w * half)).clamp(0.0, 1.0),
                (1.0 - edge / rim.max(1.0e-12)).clamp(0.0, 1.0),
            )
        } else {
            (
                (edge / (w * half)).clamp(0.0, 1.0),
                (edge / half).clamp(0.0, 1.0),
            )
        };
        // Cross ridges bridge a fissure here and there.
        if fissures.cross_ridges > 0.0 && wall < 1.0 {
            let bridge = noise(draws.stream(BRIDGES), nx * 2, ny * 6, period, x, y);
            let bridged = smoothstep(1.0 - 0.6 * fissures.cross_ridges, 1.0, bridge);
            wall = wall.max(0.75 * bridged);
        }
        // The floor is the inner third of the fissure.
        let rise = smoothstep(0.3, 1.0, wall);
        let mut h = match fissures.profile {
            Profile::Rounded => {
                rise * (0.75 + 0.25 * libm::sqrt((1.0 - (1.0 - across) * (1.0 - across)).max(0.0)))
            }
            Profile::FlatTopped => rise,
            Profile::Sharp => rise * (0.6 + 0.4 * across),
        };
        // Fine grain on the ridges, deeper on rougher bark.
        let grain = fbm(
            draws.stream(GRAIN),
            cells_in(period, fissures.spacing_m / 4.0, 4),
            cells_in(
                period,
                fissures.spacing_m * fissures.elongation.sqrt() / 4.0,
                4,
            ),
            period,
            x,
            y,
        ) - 0.5;
        h = (h * (1.0 + 2.0 * fissures.grain * grain)).clamp(0.0, 1.2);
        texel.height = (h - 1.0) * depth;
        texel.ridge = h.min(1.0);
        let cell = draws.stream(FISSURES).unit(near.id, 3);
        let floor = fissures
            .floor_colour
            .map_or_else(|| times(surface, 0.35), linear);
        let top = fissures.ridge_top_colour.map_or(surface, linear);
        let mut colour = blend(floor, surface, smoothstep(0.02, 0.45, h));
        colour = blend(colour, top, smoothstep(0.75, 1.0, h));
        texel.colour = times(colour, 1.0 + 0.16 * (cell - 0.5));
        texel.roughness = (stage.roughness + 0.1 * (1.0 - h)).min(1.0);
    } else {
        // Smooth bark still has a faint grain.
        let grain = fbm(draws.stream(GRAIN), 24, 12, period, x, y) - 0.5;
        texel.height = 0.0002 * grain;
    }
    // Scales on the surface or on the ridges, each lifted at its lower edge.
    if let Some(scales) = &stage.scales {
        let weight = if scales.on_ridges {
            smoothstep(0.35, 0.7, texel.ridge)
        } else {
            1.0
        };
        if weight > 0.0 {
            let stretch = scales.aspect.max(0.05);
            let nx = cells_in(period, scales.size_m, 3);
            let ny = cells_in(period, scales.size_m * stretch, 2);
            let near = nearest(draws.stream(SCALES), nx, ny, period, 0.8, stretch, x, y);
            let ch = period / float(ny);
            let s = (near.second - near.first) / (near.second + near.first).max(1.0e-12);
            // Highest at its lower edge, where it stands off the one below.
            let lift = (0.5 - near.offset[1] / ch).clamp(0.0, 1.0);
            let edge = smoothstep(0.0, 0.12, s);
            texel.height += weight * scales.depth_m * lift * edge;
            let shade = 1.0
                + scales.variation * (draws.stream(SCALES).unit(near.id, 4) - 0.5) * 2.0
                - 0.15 * (1.0 - edge);
            texel.colour = times(texel.colour, 1.0 + weight * (shade - 1.0));
        }
    }
    // Exfoliation.
    if let Some(peel) = &stage.exfoliation {
        exfoliate(peel, stage, draws, period, surface, x, y, &mut texel);
    }
    // Soft mottling.
    if let Some(mottle) = &stage.mottle {
        let n = cells_in(period, mottle.scale_m, 2);
        let m = fbm(draws.stream(MOTTLE), n, n, period, x, y);
        let t = mottle.amount * smoothstep(0.35, 0.75, m);
        texel.colour = match mottle.colour {
            Some(colour) => blend(texel.colour, linear(colour), t),
            None => times(texel.colour, 1.0 + 2.0 * mottle.amount * (m - 0.5)),
        };
    }
    // Lenticels, stretched round the stem as it grows.
    if let Some(lenticels) = &stage.lenticels {
        let stretch = if lenticels.stretch {
            (diameter / lenticels.formed_diameter_m).clamp(1.0, 6.0)
        } else {
            1.0
        };
        let pattern = Marks {
            per_m2: lenticels.per_m2,
            length: [
                lenticels.length_m[0] * stretch,
                lenticels.length_m[1] * stretch,
            ],
            width_ratio: lenticels.width_ratio,
            horizontal: lenticels.horizontal,
            rows: lenticels.rows,
            shape: lenticels.shape,
        };
        if let Some(inside) = pattern.at(draws.stream(LENTICELS), period, x, y) {
            texel.colour = blend(texel.colour, linear(lenticels.colour), inside);
            texel.height += lenticels.relief_m * inside;
        }
    }
    // Warts and blisters: domes.
    for (bumps, stream) in [(&stage.warts, WARTS), (&stage.blisters, BLISTERS)] {
        if let Some(bumps) = bumps {
            let pattern = Marks {
                per_m2: bumps.per_m2,
                length: bumps.diameter_m,
                width_ratio: 1.0,
                horizontal: true,
                rows: bumps.rows,
                shape: LenticelShape::Dot,
            };
            if let Some(inside) = pattern.at(draws.stream(stream), period, x, y) {
                let dome = libm::sqrt(inside.clamp(0.0, 1.0));
                texel.height += bumps.height_m * dome;
                if let Some(colour) = bumps.colour {
                    texel.colour = blend(texel.colour, linear(colour), inside);
                } else {
                    texel.colour = times(texel.colour, 1.0 + 0.12 * dome);
                }
                if let Some(roughness) = bumps.roughness {
                    texel.roughness += (roughness - texel.roughness) * inside;
                }
            }
        }
    }
    // A bloom whitens and dulls the surface (not the fissures' floors).
    if stage.bloom > 0.0 {
        let t = stage.bloom * texel.ridge;
        texel.colour = blend(texel.colour, [0.75, 0.75, 0.72], t);
        texel.roughness = texel.roughness.max(0.9 * t + texel.roughness * (1.0 - t));
    }
    texel
}

/// Exfoliation on one texel.
#[allow(clippy::too_many_arguments)]
fn exfoliate(
    peel: &Exfoliation,
    stage: &Stage,
    draws: Draws,
    period: f64,
    surface: [f64; 3],
    x: f64,
    y: f64,
    texel: &mut Texel,
) {
    let draws = draws.stream(PEEL);
    let (round, along, jitter) = match peel.mode {
        Mode::PaperyStrips => (peel.piece_m, peel.piece_m / peel.aspect, 0.7),
        Mode::FibrousStrips => (peel.piece_m / peel.aspect, peel.piece_m, 0.5),
        Mode::Patches => (
            peel.piece_m / libm::sqrt(peel.aspect),
            peel.piece_m / libm::sqrt(peel.aspect) * peel.aspect,
            0.95,
        ),
    };
    let nx = cells_in(period, round, 2);
    // Fibrous strips may run the whole tile's height.
    let ny = cells_in(
        period,
        along,
        u64::from(peel.mode != Mode::FibrousStrips) + 1,
    );
    let stretch = (period / float(ny)) / (period / float(nx));
    // Pieces tear along ragged, rounded edges, not straight cuts.
    let (wx, wy) = if peel.mode == Mode::FibrousStrips {
        (0.0, 0.0)
    } else {
        let (rx, ry) = (period / float(nx), period / float(ny));
        let warp = |stream: u64| fbm(draws.stream(stream), nx * 2, ny * 2, period, x, y) - 0.5;
        (0.6 * rx * warp(WARP), 0.6 * ry * warp(GRAIN))
    };
    let near = nearest(draws, nx, ny, period, jitter, stretch, x + wx, y + wy);
    let s = (near.second - near.first) / (near.second + near.first).max(1.0e-12);
    let age = draws.unit(near.id, 5);
    let inner = linear(peel.inner_colour);
    // The ramp of colours exposed bark weathers through, ending at the
    // surface's own.
    let mut ramp = vec![inner];
    ramp.extend(peel.aged.iter().copied().map(linear));
    ramp.push(surface);
    match peel.mode {
        Mode::FibrousStrips => {
            // Each strip its own shade; dark gaps between strips; some
            // strips loose and lifted.
            let shade = 0.82 + 0.36 * age;
            let gap = smoothstep(0.0, 0.18, s);
            texel.colour = times(texel.colour, shade * (0.55 + 0.45 * gap));
            texel.height +=
                peel.curl_m * smoothstep(0.2, 0.8, s) * f64::from(u8::from(age < peel.cover));
            if age < peel.cover {
                let fresh = blend(inner, surface, age / peel.cover.max(1.0e-6));
                // Bright inner bark shows where a loose strip lifts.
                texel.colour = blend(texel.colour, fresh, 0.6 * (1.0 - gap));
            }
        }
        Mode::PaperyStrips | Mode::Patches => {
            if age < peel.cover {
                // Exposed: its colour by how long ago its piece came away.
                let t = age / peel.cover.max(1.0e-6);
                let colour = along_ramp(&ramp, t);
                let edge = smoothstep(0.0, 0.08, s);
                texel.colour = blend(texel.colour, colour, edge);
                texel.height -= 0.0003 * edge;
                texel.roughness = stage.roughness.min(texel.roughness);
            }
            // Loose edges curl up: a light lip with a shadow inside it.
            if peel.curl_m > 0.0 {
                let lip = 1.0 - smoothstep(0.0, 0.1, s);
                let side = draws.unit(near.id, 6) < 0.5;
                if age < peel.cover && lip > 0.0 {
                    texel.height += peel.curl_m * lip;
                    let k = if side { 1.12 } else { 0.7 };
                    texel.colour = times(texel.colour, 1.0 + (k - 1.0) * lip);
                }
            } else {
                let line = 1.0 - smoothstep(0.0, 0.05, s);
                texel.colour = times(texel.colour, 1.0 - 0.3 * line);
            }
        }
    }
}

/// The colour `t` (0 to 1) of the way along `ramp`.
fn along_ramp(ramp: &[[f64; 3]], t: f64) -> [f64; 3] {
    let steps = ramp.len().saturating_sub(1).max(1);
    let at = t.clamp(0.0, 1.0) * crate::texture::to_f64(steps);
    // Few steps.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (libm::floor(at) as usize).min(steps - 1);
    blend(
        ramp[i],
        ramp[(i + 1).min(ramp.len() - 1)],
        at - crate::texture::to_f64(i),
    )
}

/// Small marks scattered over the bark: lenticels, warts, blisters.
struct Marks {
    per_m2: f64,
    length: [f64; 2],
    width_ratio: f64,
    horizontal: bool,
    rows: bool,
    shape: LenticelShape,
}

impl Marks {
    /// How far inside a mark `(x, y)` lies, 0 at its edge to 1 at its
    /// heart, or `None` outside every mark.
    #[allow(clippy::many_single_char_names)]
    fn at(&self, draws: Draws, period: f64, x: f64, y: f64) -> Option<f64> {
        let cell = 1.0 / libm::sqrt(self.per_m2.max(1.0e-6));
        let n = cells_in(period, cell, 1);
        let cw = period / float(n);
        // Rows: marks share a few heights per cell row, as in bands.
        let reach_cells = libm::ceil(self.length[1] / cw * 0.5) + 1.0;
        // A handful of cells.
        #[allow(clippy::cast_possible_truncation)]
        let reach = reach_cells.min(6.0) as i64;
        #[allow(clippy::cast_possible_truncation)]
        let (gx, gy) = (libm::floor(x / cw) as i64, libm::floor(y / cw) as i64);
        let mut best: Option<f64> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (i, j) = (gx + dx, gy + dy);
                let id = wrap(i, n) * 65_537 + wrap(j, n);
                #[allow(clippy::cast_precision_loss)]
                let px = (i as f64 + draws.unit(id, 1)) * cw;
                let row_offset = if self.rows {
                    0.5 + 0.15 * (draws.unit(wrap(j, n), 7) - 0.5)
                } else {
                    draws.unit(id, 2)
                };
                #[allow(clippy::cast_precision_loss)]
                let py = (j as f64 + row_offset) * cw;
                let length = self.length[0] + (self.length[1] - self.length[0]) * draws.unit(id, 3);
                let (half_long, half_short) =
                    (length / 2.0, (length * self.width_ratio / 2.0).max(1.0e-5));
                let (mut ox, mut oy) = (x - px, y - py);
                if !self.horizontal {
                    std::mem::swap(&mut ox, &mut oy);
                }
                let (a, b) = (ox / half_long, oy / half_short);
                let reach = match self.shape {
                    LenticelShape::Dot | LenticelShape::Dash => a * a + b * b,
                    LenticelShape::Diamond => (a.abs() + b.abs()) * (a.abs() + b.abs()),
                };
                if reach < 1.0 {
                    let inside = 1.0 - reach;
                    best = Some(best.map_or(inside, |b: f64| b.max(inside)));
                }
            }
        }
        best
    }
}

impl Tile {
    fn index(&self, x: usize, y: usize) -> usize {
        (y % self.size) * self.size + x % self.size
    }

    /// The surface's slope round the stem and up it at each texel, metres
    /// per metre: a central difference that wraps round the tile.
    #[must_use]
    pub fn slopes(&self) -> Vec<[f64; 2]> {
        let texel = self.period_m / crate::texture::to_f64(self.size);
        let n = self.size;
        let mut out = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let round = (self.height[self.index(x + 1, y)]
                    - self.height[self.index(x + n - 1, y)])
                    / (2.0 * texel);
                // Row 0 is the top, so up the stem is up the rows.
                let up = (self.height[self.index(x, y + n - 1)]
                    - self.height[self.index(x, y + 1)])
                    / (2.0 * texel);
                out.push([round, up]);
            }
        }
        out
    }

    /// The base colour map: sRGB, opaque.
    #[must_use]
    pub fn base_colour_rgba(&self) -> Vec<u8> {
        self.albedo
            .iter()
            .flat_map(|c| {
                let [r, g, b] = c.map(crate::texture::linear_to_srgb);
                [r, g, b, 255]
            })
            .collect()
    }

    /// The tangent-space normal map: x round the stem, +y up it, z out
    /// (glTF's convention), each in 0 to 255.
    #[must_use]
    pub fn normal_rgba(&self) -> Vec<u8> {
        self.slopes()
            .iter()
            .flat_map(|[round, up]| {
                let n = [-round, -up, 1.0];
                let length = libm::sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
                let [x, y, z] = n.map(|c| crate::texture::to_byte(0.5 + 0.5 * c / length));
                [x, y, z, 255]
            })
            .collect()
    }

    /// The occlusion-roughness-metallic map (glTF's channels).
    #[must_use]
    pub fn orm_rgba(&self) -> Vec<u8> {
        self.occlusion
            .iter()
            .zip(&self.roughness)
            .flat_map(|(o, r)| {
                [
                    crate::texture::to_byte(*o),
                    crate::texture::to_byte(*r),
                    0,
                    255,
                ]
            })
            .collect()
    }

    /// The height map: 255 at the highest texel, 0 at the lowest.
    #[must_use]
    pub fn height_rgba(&self) -> Vec<u8> {
        let (low, high) = self.relief();
        let span = (high - low).max(1.0e-9);
        self.height
            .iter()
            .flat_map(|h| {
                let v = crate::texture::to_byte((h - low) / span);
                [v, v, v, 255]
            })
            .collect()
    }

    /// The lowest and highest heights, metres.
    #[must_use]
    pub fn relief(&self) -> (f64, f64) {
        self.height
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), h| {
                (low.min(*h), high.max(*h))
            })
    }

    /// The tile's mean linear albedo.
    #[must_use]
    pub fn mean_albedo(&self) -> [f64; 3] {
        let n = crate::texture::to_f64(self.albedo.len().max(1));
        let sum = self.albedo.iter().fold([0.0; 3], |sum, c| {
            [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]]
        });
        sum.map(|s| s / n)
    }

    /// The tile lit flat by a low sun from the upper left, sRGB.
    #[must_use]
    pub fn lit_rgba(&self) -> Vec<u8> {
        let sun = normalized([-0.55, 0.55, 0.63]);
        self.slopes()
            .iter()
            .zip(&self.albedo)
            .zip(&self.occlusion)
            .flat_map(|((slope, albedo), occlusion)| {
                let n = normalized([-slope[0], -slope[1], 1.0]);
                let light = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]).max(0.0);
                let lit = 0.2 * occlusion + 0.95 * light * (0.4 + 0.6 * occlusion);
                let [r, g, b] = albedo.map(|c| crate::texture::linear_to_srgb(c * lit * 1.6));
                [r, g, b, 255]
            })
            .collect()
    }

    /// A trunk `diameter` metres across seen from the side, `width` × `height`
    /// pixels showing `span` metres of it, lit from the upper left, sRGB:
    /// the tile wrapped round it as a renderer wraps it.
    #[must_use]
    pub fn trunk_rgba(&self, diameter: f64, span: f64, width: usize, height: usize) -> Vec<u8> {
        let radius = diameter / 2.0;
        let slopes = self.slopes();
        let sun = normalized([-0.55, 0.45, 0.7]);
        let texel = self.period_m / crate::texture::to_f64(self.size);
        let (low, _) = self.relief();
        let mut out = Vec::with_capacity(width * height * 4);
        let metres_per_pixel = span / crate::texture::to_f64(height);
        let half_width = crate::texture::to_f64(width) / 2.0;
        for row in 0..height {
            for column in 0..width {
                let across = (crate::texture::to_f64(column) + 0.5 - half_width) * metres_per_pixel;
                if across.abs() >= radius {
                    out.extend_from_slice(&[235, 235, 230, 255]);
                    continue;
                }
                let angle = libm::asin(across / radius);
                let round = radius * (angle + core::f64::consts::FRAC_PI_2);
                let up = (crate::texture::to_f64(height - row) - 0.5) * metres_per_pixel;
                let tx = round / texel;
                let ty = up / texel;
                // Rows of the tile run down from its top.
                let size = crate::texture::to_f64(self.size);
                let fx = tx.rem_euclid(size);
                let fy = (size - ty.rem_euclid(size)).rem_euclid(size);
                // Inside the tile.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let at = self.index(fx as usize, fy as usize);
                let [sr, su] = slopes[at];
                let tangent = [libm::cos(angle), 0.0, -libm::sin(angle)];
                let normal = [libm::sin(angle), 0.0, libm::cos(angle)];
                let n = normalized([
                    normal[0] - tangent[0] * sr,
                    -su,
                    normal[2] - tangent[2] * sr,
                ]);
                let light = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]).max(0.0);
                let occlusion = self.occlusion[at];
                // Deeper texels sit a little in their own shadow.
                let depth = if low < 0.0 {
                    (self.height[at] / low).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let lit = 0.22 * occlusion + 0.95 * light * (1.0 - 0.5 * depth);
                let [r, g, b] =
                    self.albedo[at].map(|c| crate::texture::linear_to_srgb(c * lit * 1.6));
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        out
    }
}

fn normalized(v: [f64; 3]) -> [f64; 3] {
    let length = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).max(1.0e-12);
    v.map(|c| c / length)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> BarkLook {
        BarkLook::from_json(
            r#"{
                "schema": 1,
                "id": "test",
                "thickness": {"coefficient": 0.063},
                "stages": [
                    {"name": "young", "until_diameter_m": 0.3, "architecture": "stretched",
                     "colour": [120, 110, 100],
                     "lenticels": {"shape": "dash", "length_m": [0.003, 0.008],
                                   "width_ratio": 0.25, "per_m2": 900, "colour": [200, 180, 150],
                                   "stretch": true}},
                    {"name": "old", "architecture": "furrowed", "colour": [90, 70, 55],
                     "fissures": {"network": "interlacing", "spacing_m": 0.06, "elongation": 5,
                                  "width_share": 0.3, "depth_share": 0.8, "profile": "flat_topped",
                                  "cross_ridges": 0.4, "floor_colour": [140, 70, 45]}}
                ]
            }"#,
        )
        .expect("a record")
    }

    #[test]
    fn stages_by_diameter() {
        let look = look();
        assert_eq!(look.stage_at(0.1).name, "young");
        assert_eq!(look.stage_at(0.3).name, "old");
        assert_eq!(look.stage_at(2.0).name, "old");
    }

    #[test]
    fn furrows_deepen_with_thickness() {
        let look = look();
        let old = &look.stages[1];
        let thin = bake(&look, old, 0.4, 64, 0.5, 1);
        let thick = bake(&look, old, 1.2, 64, 0.5, 1);
        let (low_thin, _) = thin.relief();
        let (low_thick, _) = thick.relief();
        assert!(low_thick < low_thin * 2.5, "{low_thick} vs {low_thin}");
        // Deepest furrow within its thickness share.
        assert!(-low_thick <= 0.8 * 0.063 * 1.2 * 1.25);
    }

    #[test]
    fn tiles_wrap_without_a_seam() {
        // Every layer is periodic: a point and the same point a period on,
        // round the stem or along it, draw alike.
        let mut look = look();
        look.stages[1].scales = Some(Scales {
            size_m: 0.02,
            aspect: 1.5,
            depth_m: 0.002,
            variation: 0.1,
            on_ridges: true,
        });
        look.stages[1].exfoliation = Some(Exfoliation {
            mode: Mode::Patches,
            piece_m: 0.1,
            aspect: 2.0,
            cover: 0.4,
            curl_m: 0.001,
            inner_colour: [90, 160, 60],
            aged: vec![[200, 120, 40]],
        });
        look.stages[1].lenticels = look.stages[0].lenticels.clone();
        let stage = &look.stages[1];
        let draws = Draws(5);
        let period = 0.5;
        let surface = linear(stage.colour);
        for k in 0..40 {
            let (x, y) = (0.0123 * f64::from(k), 0.0371 * f64::from(k) % period);
            let at = |x, y| draw(stage, draws, 0.05, 0.8, period, surface, x, y);
            let (here, round, along) = (at(x, y), at(x + period, y), at(x, y + period));
            for other in [round, along] {
                assert!((here.height - other.height).abs() < 1.0e-9, "{k}");
                assert!((here.colour[0] - other.colour[0]).abs() < 1.0e-9, "{k}");
            }
        }
    }

    #[test]
    fn same_seed_same_bytes() {
        let look = look();
        let a = bake(&look, &look.stages[0], 0.1, 32, 0.25, 3).base_colour_rgba();
        let b = bake(&look, &look.stages[0], 0.1, 32, 0.25, 3).base_colour_rgba();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_unordered_stages() {
        let text = r#"{"schema": 1, "id": "x", "thickness": {},
            "stages": [{"name": "a", "until_diameter_m": 0.5, "architecture": "stretched", "colour": [1,2,3]},
                       {"name": "b", "until_diameter_m": 0.2, "architecture": "stretched", "colour": [1,2,3]},
                       {"name": "c", "architecture": "stretched", "colour": [1,2,3]}]}"#;
        assert!(BarkLook::from_json(text).is_err());
    }
}
