//! Fleshy bodies: the swollen green stems of cacti and other succulents.
//!
//! A program draws a body with a symbol it declares with `body`, as it draws
//! wood with `F`, and gives each segment its radius with `!`; a species says
//! how each body type looks in `appearance.bodies` ([`BodyLook`]). Where
//! wood is a plain tube per axis ([`crate::mesh`]), a body is a smooth
//! surface:
//!
//! 1. Its axis follows a centripetal Catmull-Rom spline through the
//!    segments' ends (Yuksel, Schaefer and Keyser 2011), so an arm bent over
//!    a few segments curves instead of kinking, and its radius a monotone
//!    cubic through the segments' radii (Fritsch and Carlson 1980), which
//!    never swells past them.
//! 2. A dome closes its tip, and a rounded base its foot if the look asks:
//!    the radius times `sqrt(1 - x²)` over the last `dome` and the first
//!    `base` radii of its length.
//! 3. A cross-section is a circle, or an ellipse `flatten` times as thick as
//!    it is wide across the turtle's left vector: a pad. [`Ribs`] fold it
//!    into crests and valleys; [`Tubercles`] raise a bump under each
//!    areole.
//! 4. Areoles sit along each rib's crest at even spacing, neighbouring ribs
//!    offset by the golden section, or on a body without ribs on a
//!    golden-angle spiral that gives each the same share of the skin (Vogel
//!    1979, unrolled onto the body): the crossing spirals of a pincushion
//!    cactus come out of that. Areoles inside another body, as where an arm
//!    leaves the trunk, are left out, judged by that body's own shape: its
//!    dome, its rounded foot and a pad's thinness.
//! 5. Near, each areole is a [`Tuft`] that becomes solid spines when drawn
//!    ([`crate::spines`]); a little farther, two cards drawn from its spines:
//!    a star face on and a fan seen from the side; far away the spines only
//!    tint the skin and swell its outline by how far they stand out.
//!
//! Each level of detail draws less of this ([`DETAIL`]): ribs keep their
//! folds to the third level, tubercles to the second, spines are solid only
//! at the first.

use serde::{Deserialize, Serialize};

use crate::graph::PlantGraph;
use crate::math::{self, PI, Vec3, any_perpendicular};
use crate::mesh::{Card, PlantMesh, Vertex};
use crate::rng::{mix64, unit};
use crate::spines::{self, MAX_SPINE_SETS, MAX_TUFT_SPINES, Tuft};

/// The golden angle as a share of a turn, `1 - 1/φ`.
const GOLDEN_TURN: f64 = 0.381_966_011_250_105_1;

/// Most areoles one axis of a body may carry, and one plant.
const MAX_AXIS_AREOLES: usize = 60_000;
const MAX_PLANT_AREOLES: usize = 400_000;

/// How one body type of a program looks, as a species spec gives it.
/// Lengths are metres; colours linear RGB, 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BodyLook {
    /// The skin's colour.
    pub colour: [f32; 3],
    /// Colour at the bottom of a rib's valley and between tubercles; the
    /// skin's colour darkened if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valley: Option<[f32; 3]>,
    /// Thickness over width of a cross-section: 1 round, less a pad
    /// flattened across the turtle's left vector.
    pub flatten: f64,
    /// Length of the dome that closes the tip, in radii of the tip: 1 a
    /// half sphere, less a flatter top, more a pointed one.
    pub dome: f64,
    /// Length of a rounded base, in radii of the foot: 0 leaves the foot
    /// open, as a stem standing in the ground; a joint of a cholla or a
    /// prickly pear's pad is rounded at both ends.
    pub base: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ribs: Option<Ribs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tubercles: Option<Tubercles>,
    pub areoles: Areoles,
    /// Every areole's spines, in sets (radials, centrals, glochids): at
    /// most [`MAX_SPINE_SETS`] sets and [`MAX_TUFT_SPINES`] spines.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spines: Vec<SpineSet>,
    /// Spines turn this colour as they age.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub greying: Option<Greying>,
    /// Old stems turn to bark from the ground up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cork: Option<Cork>,
}

impl Default for BodyLook {
    fn default() -> Self {
        Self {
            colour: [0.09, 0.18, 0.07],
            valley: None,
            flatten: 1.0,
            dome: 1.0,
            base: 0.0,
            ribs: None,
            tubercles: None,
            areoles: Areoles::default(),
            spines: Vec::new(),
            greying: None,
            cork: None,
        }
    }
}

/// Ribs: the body's skin folded lengthwise into crests and valleys, as a
/// saguaro's pleats or a barrel cactus's ribs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ribs {
    /// Fewest and most ribs round one axis.
    pub count: [u32; 2],
    /// Distance between neighbouring crests round the axis where it is
    /// widest: a thicker stem has more ribs, within `count`.
    pub spacing: f64,
    /// How far a valley lies below the crests, as a share of the radius.
    pub depth: f64,
    /// 0 for rounded crests over folds pinched to a V (a saguaro's
    /// pleats), 1 for sharp crests over rounded valleys (a barrel
    /// cactus), between for a mix.
    pub sharpness: f64,
    /// How far the ribs spiral round the axis, degrees per metre.
    pub twist: f64,
}

impl Default for Ribs {
    fn default() -> Self {
        Self {
            count: [8, 24],
            spacing: 0.06,
            depth: 0.12,
            sharpness: 0.3,
            twist: 0.0,
        }
    }
}

/// A bump under each areole: the nipples of a pincushion cactus, or chins
/// along a barrel cactus's ribs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tubercles {
    /// How far a tubercle stands above the skin, as a share of the radius.
    pub height: f64,
    /// Radius of its foot, as a share of the areoles' spacing.
    pub width: f64,
    /// 1 rounded; more a pointed cone, less a flat-topped mound.
    pub point: f64,
    /// How many times longer than wide a tubercle is along the stem: a
    /// buckhorn cholla's 2 to 3.
    pub stretch: f64,
}

impl Default for Tubercles {
    fn default() -> Self {
        Self {
            height: 0.15,
            width: 0.6,
            point: 1.0,
            stretch: 1.0,
        }
    }
}

/// Where spines grow from, and the felt (wool) round them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Areoles {
    /// Distance between areoles along a rib's crest, or on a body without
    /// ribs the side of the square of skin each one has to itself; 0 for
    /// none.
    pub spacing: f64,
    /// Radius of each areole's felt.
    pub felt_radius: f64,
    /// The felt's colour.
    pub felt: [f32; 3],
}

impl Default for Areoles {
    fn default() -> Self {
        Self {
            spacing: 0.02,
            felt_radius: 0.003,
            felt: [0.55, 0.5, 0.42],
        }
    }
}

/// One set of every areole's spines: its radial spines, its centrals or
/// its glochids. Angles are degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpineSet {
    /// Fewest and most spines of the set on one areole.
    pub count: [u32; 2],
    /// Angle from the skin: 0 lies on it, 90 stands straight out.
    pub angle: f64,
    /// How far the angle varies, either way.
    pub angle_spread: f64,
    /// Direction the set points along the skin, from up the body (toward
    /// its tip): 0 up, 90 to one side, 180 down.
    pub aim: f64,
    /// Arc the set's spines spread over round the areole, centred on
    /// `aim`: 360 all round.
    pub arc: f64,
    /// Length, metres, and how far it varies, as a share either way.
    pub length: f64,
    pub length_spread: f64,
    /// How far each spine curves back toward the skin over its length.
    pub curve: f64,
    /// How far its last fifth turns back as a hook: a fishhook's 120 to
    /// 180.
    pub hook: f64,
    /// Width at the base, metres, and thickness over width: below 1 a
    /// flattened spine.
    pub width: f64,
    pub flat: f64,
    /// The spine's colour, and toward its tip, if that differs.
    pub colour: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<[f32; 3]>,
}

impl Default for SpineSet {
    fn default() -> Self {
        Self {
            count: [8, 12],
            angle: 20.0,
            angle_spread: 8.0,
            aim: 0.0,
            arc: 360.0,
            length: 0.012,
            length_spread: 0.2,
            curve: 0.0,
            hook: 0.0,
            width: 0.0006,
            flat: 1.0,
            colour: [0.75, 0.7, 0.6],
            tip: None,
        }
    }
}

/// Spines bleach or grey as they age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Greying {
    /// Their colour once old.
    pub colour: [f32; 3],
    /// Years an areole's spines take to turn to it.
    pub years: f64,
}

impl Default for Greying {
    fn default() -> Self {
        Self {
            colour: [0.33, 0.32, 0.3],
            years: 10.0,
        }
    }
}

/// Bark on an old stem: the corky base of an old saguaro, the woody trunk
/// of a cholla. It rises from the ground as the plant ages, and most
/// areoles under it lose their spines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cork {
    /// The bark's colour.
    pub colour: [f32; 3],
    /// Plant age at which it starts at the ground, years.
    pub start: f64,
    /// How fast it rises, metres a year, and how high it reaches.
    pub rate: f64,
    pub height: f64,
    /// Share of the areoles under it that keep their spines.
    pub spines: f64,
}

impl Default for Cork {
    fn default() -> Self {
        Self {
            colour: [0.2, 0.18, 0.14],
            start: 40.0,
            rate: 0.02,
            height: 1.5,
            spines: 0.2,
        }
    }
}

/// Check that `value` lies in `low..=high`.
fn within(what: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be between {low} and {high}, found {value}"
        ))
    }
}

fn colour_ok(what: &str, colour: [f32; 3]) -> Result<(), String> {
    for channel in colour {
        within(what, f64::from(channel), 0.0, 1.0)?;
    }
    Ok(())
}

impl BodyLook {
    /// A body of this colour with every other value its default: smooth,
    /// round, domed, areoles without spines.
    #[must_use]
    pub fn plain(colour: [f32; 3]) -> Self {
        Self {
            colour,
            ..Self::default()
        }
    }

    /// Colour at the bottom of a valley.
    #[must_use]
    pub fn valley_colour(&self) -> [f32; 3] {
        self.valley
            .unwrap_or_else(|| self.colour.map(|channel| channel * 0.62))
    }

    /// Check the values.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        colour_ok("colour", self.colour)?;
        if let Some(valley) = self.valley {
            colour_ok("valley", valley)?;
        }
        within("flatten", self.flatten, 0.05, 1.0)?;
        within("dome", self.dome, 0.05, 4.0)?;
        within("base", self.base, 0.0, 4.0)?;
        if let Some(ribs) = &self.ribs {
            let [low, high] = ribs.count;
            if !(3..=64).contains(&low) || high < low || high > 64 {
                return Err(format!(
                    "ribs count must be two numbers from 3 to 64, the first no larger, found {low} and {high}"
                ));
            }
            within("ribs spacing", ribs.spacing, 0.002, 1.0)?;
            within("ribs depth", ribs.depth, 0.0, 0.6)?;
            within("ribs sharpness", ribs.sharpness, 0.0, 1.0)?;
            within("ribs twist", ribs.twist, -720.0, 720.0)?;
        }
        if let Some(tubercles) = &self.tubercles {
            within("tubercles height", tubercles.height, 0.0, 2.0)?;
            within("tubercles width", tubercles.width, 0.2, 2.0)?;
            within("tubercles point", tubercles.point, 0.2, 5.0)?;
            within("tubercles stretch", tubercles.stretch, 0.3, 4.0)?;
        }
        let areoles = &self.areoles;
        if areoles.spacing != 0.0 {
            within("areoles spacing", areoles.spacing, 0.001, 0.5)?;
        }
        within("areoles felt_radius", areoles.felt_radius, 0.0, 0.05)?;
        colour_ok("areoles felt", areoles.felt)?;
        if self.spines.len() > MAX_SPINE_SETS {
            return Err(format!(
                "a body has at most {MAX_SPINE_SETS} spine sets, found {}",
                self.spines.len()
            ));
        }
        let mut most = 0;
        for set in &self.spines {
            let [low, high] = set.count;
            if high < low {
                return Err(format!(
                    "spines count must not fall, found {low} and {high}"
                ));
            }
            most += high;
            within("spines angle", set.angle, 0.0, 90.0)?;
            within("spines angle_spread", set.angle_spread, 0.0, 90.0)?;
            within("spines aim", set.aim, -360.0, 360.0)?;
            within("spines arc", set.arc, 0.0, 360.0)?;
            within("spines length", set.length, 0.0005, 0.5)?;
            within("spines length_spread", set.length_spread, 0.0, 0.9)?;
            within("spines curve", set.curve, -180.0, 180.0)?;
            within("spines hook", set.hook, 0.0, 270.0)?;
            within("spines width", set.width, 0.000_05, 0.01)?;
            within("spines flat", set.flat, 0.05, 1.0)?;
            colour_ok("spines colour", set.colour)?;
            if let Some(tip) = set.tip {
                colour_ok("spines tip", tip)?;
            }
        }
        if most > MAX_TUFT_SPINES {
            return Err(format!(
                "an areole carries at most {MAX_TUFT_SPINES} spines, but its sets allow {most}"
            ));
        }
        if let Some(greying) = &self.greying {
            colour_ok("greying colour", greying.colour)?;
            within("greying years", greying.years, 0.1, 1000.0)?;
        }
        if let Some(cork) = &self.cork {
            colour_ok("cork colour", cork.colour)?;
            within("cork start", cork.start, 0.0, 2000.0)?;
            within("cork rate", cork.rate, 0.0, 5.0)?;
            within("cork height", cork.height, 0.0, 50.0)?;
            within("cork spines", cork.spines, 0.0, 1.0)?;
        }
        Ok(())
    }

    /// Height of the cork line at plant age `age`, metres; 0 without cork.
    #[must_use]
    pub fn cork_line(&self, age: f64) -> f64 {
        self.cork.as_ref().map_or(0.0, |cork| {
            ((age - cork.start) * cork.rate).clamp(0.0, cork.height)
        })
    }

    /// The colour of the spines and felt seen from afar, and the share of
    /// the skin they hide: what the far levels tint the skin with.
    #[must_use]
    pub fn tint(&self) -> ([f32; 3], f64) {
        let spacing = self.areoles.spacing;
        if spacing <= 0.0 {
            return (self.colour, 0.0);
        }
        let felt = self.areoles.felt_radius;
        let felt_area = PI * felt * felt;
        let spine_area: f64 = self
            .spines
            .iter()
            .map(|set| f64::from(set.count[0] + set.count[1]) * 0.5 * set.length * set.width)
            .sum();
        let total = felt_area + spine_area;
        let colour = if total > 0.0 {
            let spines = spines::colour(self);
            #[allow(clippy::cast_possible_truncation)]
            let share = (spine_area / total) as f32;
            std::array::from_fn(|channel| {
                self.areoles.felt[channel] + (spines[channel] - self.areoles.felt[channel]) * share
            })
        } else {
            self.colour
        };
        (colour, spines::cover(self, spacing * spacing))
    }
}

/// How much of a body one level of detail draws.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Detail {
    /// Vertices round a ring per rib; 0 draws ribs only as shading.
    per_rib: u32,
    /// Most vertices round a ring.
    max_sides: u32,
    /// Vertices round a ring of a body without ribs or tubercles.
    smooth_sides: u32,
    /// Longest step between rings, as a share of the axis's mean radius.
    ring_step: f64,
    /// Most the axis may turn between rings, degrees, and its radius
    /// change, as a share.
    max_turn: f64,
    max_change: f64,
    /// Rings over the dome, and over a rounded base.
    cap_rings: u32,
    /// Vertices across a tubercle's foot; 0 leaves tubercles out.
    per_tubercle: f64,
    spines: Spines,
}

/// How a level draws the spines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spines {
    /// A tuft per areole, which a renderer draws as solid spines near the
    /// camera, and the cards of [`Spines::Cards`] beyond.
    Tufts,
    /// A star card and a fan card per areole.
    Cards,
    /// No spines: they tint the skin and swell the outline.
    Tint,
}

/// Most triangles a plant's skin may take at each level before it is
/// drawn coarser (see `build`): about what a tall conifer's wood takes at
/// LOD0.
const SKIN_TRIANGLES: [usize; 4] = [300_000, 120_000, usize::MAX, usize::MAX];
const MIN_PER_TUBERCLE: f64 = 1.5;
/// Most a plant over its budget stretches a level's ring step and the
/// turn and change of radius between rings, the fewest rings it leaves
/// over a dome or a rounded foot, and the fewest vertices round a smooth
/// ring.
const MAX_RING_STRETCH: f64 = 2.0;
const MIN_CAP_RINGS: u32 = 3;
const MIN_SMOOTH_SIDES: u32 = 12;

/// What each level of detail draws, LOD0 first.
const DETAIL: [Detail; 4] = [
    Detail {
        per_rib: 6,
        max_sides: 192,
        smooth_sides: 32,
        ring_step: 0.35,
        max_turn: 5.0,
        max_change: 0.04,
        cap_rings: 8,
        per_tubercle: 4.0,
        spines: Spines::Tufts,
    },
    Detail {
        per_rib: 4,
        max_sides: 96,
        smooth_sides: 20,
        ring_step: 0.7,
        max_turn: 10.0,
        max_change: 0.08,
        cap_rings: 5,
        per_tubercle: 2.5,
        spines: Spines::Cards,
    },
    Detail {
        per_rib: 2,
        max_sides: 48,
        smooth_sides: 12,
        ring_step: 1.4,
        max_turn: 20.0,
        max_change: 0.16,
        cap_rings: 3,
        per_tubercle: 0.0,
        spines: Spines::Tint,
    },
    Detail {
        per_rib: 0,
        max_sides: 12,
        smooth_sides: 8,
        ring_step: 2.5,
        max_turn: 35.0,
        max_change: 0.3,
        cap_rings: 2,
        per_tubercle: 0.0,
        spines: Spines::Tint,
    },
];

/// One body axis: a chain of segments of one body type, each continuing
/// the one before.
#[derive(Debug, Clone)]
struct Axis {
    /// Index of the body type among the program's bodies.
    body: usize,
    segments: Vec<usize>,
    /// The first segment's identity: places the axis's patterns.
    id: u64,
}

/// Every body axis of `graph`, parents before children.
fn axes(graph: &PlantGraph) -> Vec<Axis> {
    let continuations = graph.continuations();
    let mut axes = Vec::new();
    for (index, segment) in graph.segments.iter().enumerate() {
        if segment.body == 0 {
            continue;
        }
        let starts = match segment.parent {
            None => true,
            Some(parent) => {
                segment.lateral
                    || graph.segments[parent as usize].body != segment.body
                    || continuations[parent as usize] != u32::try_from(index).ok()
            }
        };
        if !starts {
            continue;
        }
        let mut chain = vec![index];
        let mut cursor = index;
        while let Some(next) = continuations[cursor] {
            let next = next as usize;
            if graph.segments[next].body != segment.body {
                break;
            }
            chain.push(next);
            cursor = next;
        }
        axes.push(Axis {
            body: usize::from(segment.body - 1),
            segments: chain,
            id: segment.id,
        });
    }
    axes
}

/// A point of an axis's spline.
#[derive(Debug, Clone, Copy)]
struct Station {
    /// Distance along the axis, metres.
    s: f64,
    point: Vec3,
    tangent: Vec3,
    /// Where `t` = 0 points round the axis: the width of a pad.
    across: Vec3,
    /// Radius before the dome and base round it.
    radius: f64,
    born: f64,
    shed: Option<f64>,
}

/// One end of a segment, as the spline passes through it.
#[derive(Debug, Clone, Copy)]
struct Knot {
    point: Vec3,
    radius: f64,
    left: Vec3,
    born: f64,
    shed: Option<f64>,
}

/// Slopes for a monotone cubic through `(x, y)` (Fritsch and Carlson
/// 1980): the mean of the neighbouring secants, zero at a turn, cut back
/// where they would overshoot.
#[allow(clippy::many_single_char_names)]
fn monotone_slopes(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = x.len();
    let secant: Vec<f64> = (0..n - 1)
        .map(|k| (y[k + 1] - y[k]) / (x[k + 1] - x[k]).max(1e-12))
        .collect();
    let mut slopes = vec![0.0; n];
    slopes[0] = secant[0];
    slopes[n - 1] = secant[n - 2];
    for k in 1..n - 1 {
        slopes[k] = if secant[k - 1] * secant[k] <= 0.0 {
            0.0
        } else {
            f64::midpoint(secant[k - 1], secant[k])
        };
    }
    for k in 0..n - 1 {
        if secant[k].abs() < 1e-15 {
            slopes[k] = 0.0;
            slopes[k + 1] = 0.0;
            continue;
        }
        let a = slopes[k] / secant[k];
        let b = slopes[k + 1] / secant[k];
        let size = a * a + b * b;
        if size > 9.0 {
            let tau = 3.0 / math::sqrt(size);
            slopes[k] = tau * a * secant[k];
            slopes[k + 1] = tau * b * secant[k];
        }
    }
    slopes
}

/// The cubic Hermite curve from `y0` to `y1` with slopes `m0` and `m1`
/// over a span `h` long, at `u` (0 to 1) of the way along.
fn hermite(y0: f64, y1: f64, m0: f64, m1: f64, h: f64, u: f64) -> f64 {
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * y0
        + (u3 - 2.0 * u2 + u) * h * m0
        + (3.0 * u2 - 2.0 * u3) * y1
        + (u3 - u2) * h * m1
}

/// The centripetal Catmull-Rom spline through `p[1]` and `p[2]`, with
/// `p[0]` and `p[3]` beside them, at `u` (0 to 1) of the way from `p[1]`
/// (Barry and Goldman's pyramid, knots at the square roots of the chords).
fn catmull_rom(p: [Vec3; 4], u: f64) -> Vec3 {
    let knot = |a: Vec3, b: Vec3| math::sqrt((b - a).length()).max(1e-9);
    let t0 = 0.0;
    let t1 = t0 + knot(p[0], p[1]);
    let t2 = t1 + knot(p[1], p[2]);
    let t3 = t2 + knot(p[2], p[3]);
    let t = t1 + (t2 - t1) * u;
    let blend = |a: Vec3, b: Vec3, ta: f64, tb: f64| {
        a * ((tb - t) / (tb - ta)) + b * ((t - ta) / (tb - ta))
    };
    let a1 = blend(p[0], p[1], t0, t1);
    let a2 = blend(p[1], p[2], t1, t2);
    let a3 = blend(p[2], p[3], t2, t3);
    let b1 = blend(a1, a2, t0, t2);
    let b2 = blend(a2, a3, t1, t3);
    blend(b1, b2, t1, t2)
}

/// An axis's spline, sampled finely, with its dome and base.
#[derive(Debug, Clone)]
struct Centreline {
    stations: Vec<Station>,
    length: f64,
    /// Lengths of the dome at the tip and the rounded base at the foot.
    dome: f64,
    base: f64,
}

impl Centreline {
    /// The spline of `axis`, its cross-sections turned by `phase` radians
    /// (round bodies) or set by the turtle's left vector (flattened ones).
    #[allow(clippy::too_many_lines)]
    fn new(graph: &PlantGraph, axis: &Axis, look: &BodyLook, phase: f64) -> Option<Self> {
        let mut knots: Vec<Knot> = axis
            .segments
            .iter()
            .map(|&index| {
                let segment = &graph.segments[index];
                Knot {
                    point: segment.start,
                    radius: segment.radius.max(1e-4),
                    left: segment.left,
                    born: segment.born,
                    shed: segment.shed,
                }
            })
            .collect();
        let last = &graph.segments[*axis.segments.last()?];
        knots.push(Knot {
            point: last.end,
            radius: last.radius.max(1e-4),
            left: last.left,
            born: last.born,
            shed: last.shed,
        });
        knots.dedup_by(|later, earlier| (later.point - earlier.point).length() < 1e-7);
        if knots.len() < 2 {
            return None;
        }
        let n = knots.len();
        let mut chord = vec![0.0; n];
        for k in 1..n {
            chord[k] = chord[k - 1] + (knots[k].point - knots[k - 1].point).length();
        }
        let radii: Vec<f64> = knots.iter().map(|knot| knot.radius).collect();
        let slopes = monotone_slopes(&chord, &radii);
        let point = |k: isize| -> Vec3 {
            // Reflected past the ends, so the spline leaves them straight.
            let last = n - 1;
            if k < 0 {
                knots[0].point * 2.0 - knots[1].point
            } else if k.cast_unsigned() > last {
                knots[last].point * 2.0 - knots[last - 1].point
            } else {
                knots[k.cast_unsigned()].point
            }
        };

        // Points, radii and left vectors along the spline.
        let mut samples: Vec<(Vec3, f64, Vec3, f64, Option<f64>)> = Vec::new();
        for k in 0..n - 1 {
            let span = chord[k + 1] - chord[k];
            let mean = f64::midpoint(knots[k].radius, knots[k + 1].radius);
            // A small whole number of pieces per span.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pieces = ((span / (0.2 * mean)).ceil() as u32).clamp(2, 48);
            #[allow(clippy::cast_possible_wrap)]
            let k_signed = k as isize;
            let ends = [
                point(k_signed - 1),
                point(k_signed),
                point(k_signed + 1),
                point(k_signed + 2),
            ];
            for piece in 0..pieces {
                let u = f64::from(piece) / f64::from(pieces);
                samples.push((
                    catmull_rom(ends, u),
                    hermite(radii[k], radii[k + 1], slopes[k], slopes[k + 1], span, u),
                    knots[k].left.lerp(knots[k + 1].left, u),
                    knots[k].born,
                    knots[k].shed,
                ));
            }
        }
        let end = knots[n - 1];
        samples.push((end.point, end.radius, end.left, end.born, end.shed));

        let count = samples.len();
        let mut stations = Vec::with_capacity(count);
        let mut s = 0.0;
        let mut across = Vec3::ZERO;
        for (index, &(point, radius, left, born, shed)) in samples.iter().enumerate() {
            if index > 0 {
                s += (point - samples[index - 1].0).length();
            }
            let before = samples[index.saturating_sub(1)].0;
            let after = samples[(index + 1).min(count - 1)].0;
            let tangent = (after - before).normalize_or(Vec3::Y);
            // Parallel transport, or the turtle's left vector for a pad.
            let carried = if index == 0 {
                any_perpendicular(tangent).rotate_about(tangent, phase)
            } else {
                (across - tangent * tangent.dot(across)).normalize_or(any_perpendicular(tangent))
            };
            across = if look.flatten < 1.0 {
                let wide = left - tangent * tangent.dot(left);
                if wide.length() > 1e-6 {
                    let wide = wide.normalize_or(carried);
                    if index > 0 && wide.dot(carried) < 0.0 {
                        -wide
                    } else {
                        wide
                    }
                } else {
                    carried
                }
            } else {
                carried
            };
            stations.push(Station {
                s,
                point,
                tangent,
                across,
                radius: radius.max(1e-4),
                born,
                shed,
            });
        }
        let length = s;
        if length <= 1e-6 {
            return None;
        }
        let mut dome = look.dome * stations[count - 1].radius;
        let mut base = look.base * stations[0].radius;
        if dome + base > 0.95 * length {
            let fit = 0.95 * length / (dome + base);
            dome *= fit;
            base *= fit;
        }
        Some(Self {
            stations,
            length,
            dome,
            base,
        })
    }

    /// The station at distance `s`, interpolated.
    fn at(&self, s: f64) -> Station {
        let s = s.clamp(0.0, self.length);
        let upper = self
            .stations
            .partition_point(|station| station.s < s)
            .clamp(1, self.stations.len() - 1);
        let (a, b) = (&self.stations[upper - 1], &self.stations[upper]);
        let span = b.s - a.s;
        let u = if span > 0.0 {
            ((s - a.s) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let tangent = a.tangent.lerp(b.tangent, u).normalize_or(a.tangent);
        let across = a.across.lerp(b.across, u);
        let across =
            (across - tangent * tangent.dot(across)).normalize_or(any_perpendicular(tangent));
        Station {
            s,
            point: a.point.lerp(b.point, u),
            tangent,
            across,
            radius: a.radius + (b.radius - a.radius) * u,
            born: a.born,
            shed: a.shed,
        }
    }

    /// How much the dome and base round the radius at `s`: 1 along the
    /// body, falling to 0 at a closed end.
    fn cap(&self, s: f64) -> f64 {
        let mut cap = 1.0;
        if self.dome > 0.0 && s > self.length - self.dome {
            let x = ((s - (self.length - self.dome)) / self.dome).min(1.0);
            cap *= math::sqrt((1.0 - x * x).max(0.0));
        }
        if self.base > 0.0 && s < self.base {
            let x = ((self.base - s) / self.base).min(1.0);
            cap *= math::sqrt((1.0 - x * x).max(0.0));
        }
        cap
    }

    /// The body's radius at `s`, dome and base included: its ribs' crests.
    fn crest(&self, s: f64) -> f64 {
        self.at(s).radius * self.cap(s)
    }

    /// Distances along the axis to put rings at: the base's and the dome's
    /// rings by equal angles round their quarter circles, the rest as far
    /// apart as `step` allows while the axis turns less than `detail`'s
    /// most and its radius changes less. Candidates lie a quarter step
    /// apart; when one breaks a limit the ring goes on the last that kept
    /// it, so no two rings turn more than the most, however sharp an
    /// elbow.
    fn rings(&self, detail: &Detail, step: f64) -> Vec<f64> {
        let quarter = PI * 0.5;
        let caps = f64::from(detail.cap_rings.max(1));
        let mut out = Vec::new();
        let start = if self.base > 0.0 {
            for ring in 0..detail.cap_rings.max(1) {
                let angle = quarter * f64::from(ring) / caps;
                out.push(self.base - self.base * math::cos(angle));
            }
            self.base
        } else {
            0.0
        };
        let end = self.length - self.dome;
        out.push(start);
        if end > start + 1e-9 {
            let fine = (step * 0.25).max((end - start) / 20_000.0);
            let cos_turn = math::cos(math::radians(detail.max_turn));
            let breaks = |from: &Station, to: &Station| {
                to.tangent.dot(from.tangent) < cos_turn
                    || (to.radius - from.radius).abs() > detail.max_change * from.radius
            };
            let mut last = start;
            let mut last_station = self.at(start);
            // The last candidate since the ring that kept every limit.
            let mut kept: Option<Station> = None;
            let mut s = start + fine;
            while s < end - fine * 0.5 {
                let here = self.at(s);
                if breaks(&last_station, &here) {
                    if let Some(previous) = kept.take() {
                        out.push(previous.s);
                        last = previous.s;
                        last_station = previous;
                    }
                    if breaks(&last_station, &here) {
                        // One candidate step breaks it: the ring has to go
                        // here.
                        out.push(s);
                        last = s;
                        last_station = here;
                    } else {
                        kept = Some(here);
                    }
                } else if s - last >= step - 1e-12 {
                    out.push(s);
                    last = s;
                    last_station = here;
                    kept = None;
                } else {
                    kept = Some(here);
                }
                s += fine;
            }
            out.push(end);
        }
        for ring in 1..=detail.cap_rings.max(1) {
            let angle = quarter * f64::from(ring) / caps;
            out.push(end + self.dome * math::sin(angle));
        }
        out.dedup_by(|later, earlier| (*later - *earlier).abs() < 1e-9);
        out
    }

    /// The crest's length along the skin from the foot to each of
    /// `samples` distances, for spacing areoles along a crest.
    fn meridian(&self, samples: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(samples.len());
        let mut total = 0.0;
        let mut previous: Option<(f64, f64)> = None;
        for &s in samples {
            let radius = self.crest(s);
            if let Some((ps, pr)) = previous {
                let (ds, dr) = (s - ps, radius - pr);
                total += math::sqrt(ds * ds + dr * dr);
            }
            out.push(total);
            previous = Some((s, radius));
        }
        out
    }
}

/// Distances `step` apart from 0 to `length`, at most `most` of them.
fn even_samples(length: f64, step: f64, most: usize) -> Vec<f64> {
    let most = u32::try_from(most.max(2)).unwrap_or(u32::MAX);
    // A whole number of pieces, at most `most`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pieces = ((length / step.max(1e-9)).ceil() as u32).clamp(1, most - 1);
    (0..=pieces)
        .map(|piece| length * f64::from(piece) / f64::from(pieces))
        .collect()
}

/// Where `value` falls in the rising `table`, as a distance from `at`:
/// the inverse of a table of cumulative lengths or areas.
fn invert(table: &[f64], at: &[f64], value: f64) -> f64 {
    let upper = table
        .partition_point(|&entry| entry < value)
        .clamp(1, table.len() - 1);
    let (a, b) = (table[upper - 1], table[upper]);
    let u = if b > a {
        ((value - a) / (b - a)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    at[upper - 1] + (at[upper] - at[upper - 1]) * u
}

/// Perimeter of an ellipse with half axes `a` and `b` (Ramanujan's second
/// approximation).
fn perimeter(a: f64, b: f64) -> f64 {
    if a + b <= 0.0 {
        return 0.0;
    }
    let h = ((a - b) / (a + b)) * ((a - b) / (a + b));
    PI * (a + b) * (1.0 + 3.0 * h / (10.0 + math::sqrt(4.0 - 3.0 * h)))
}

/// The angle round a cross-section `flatten` thick at which a share `g`
/// of its perimeter lies, so areoles spread evenly round a pad's faces and
/// rims.
struct Around {
    /// Cumulative share of the perimeter at each of `angles`.
    shares: Vec<f64>,
    angles: Vec<f64>,
}

impl Around {
    fn new(flatten: f64) -> Self {
        const STEPS: u32 = 256;
        let mut angles = Vec::with_capacity(STEPS as usize + 1);
        let mut shares = Vec::with_capacity(STEPS as usize + 1);
        let mut total = 0.0;
        for step in 0..=STEPS {
            let t = 2.0 * PI * f64::from(step) / f64::from(STEPS);
            if step > 0 {
                let middle = t - PI / f64::from(STEPS);
                let (sin, cos) = (math::sin(middle), math::cos(middle));
                total += math::sqrt(sin * sin + flatten * flatten * cos * cos);
            }
            angles.push(t);
            shares.push(total);
        }
        for share in &mut shares {
            *share /= total.max(1e-12);
        }
        Self { shares, angles }
    }

    fn angle(&self, share: f64) -> f64 {
        invert(&self.shares, &self.angles, share.rem_euclid(1.0))
    }
}

/// Where an areole sits: its distance along the axis, its angle round it
/// and its place in the axis's sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Site {
    s: f64,
    t: f64,
    ordinal: u32,
}

/// The fold of a rib at `u` (0 at a crest, 0.5 in a valley): 1 on the
/// crest, 0 at the valley's bottom. `sharpness` blends a rounded crest over
/// a V-shaped fold into a sharp crest over a rounded valley.
fn rib_profile(u: f64, sharpness: f64) -> f64 {
    let (sin, cos) = (math::sin(PI * u).abs(), math::cos(PI * u).abs());
    (1.0 - sharpness) * cos + sharpness * (1.0 - sin)
}

/// `angle` wrapped into `[-π, π)`.
fn wrap(angle: f64) -> f64 {
    (angle + PI).rem_euclid(2.0 * PI) - PI
}

/// One body axis's surface: its centreline, cross-section and tubercles.
struct Surface<'a> {
    line: &'a Centreline,
    flatten: f64,
    /// Ribs round the axis, 0 for none, and whether the mesh folds them.
    ribs: u32,
    folded: bool,
    depth: f64,
    sharpness: f64,
    /// Turn of the first crest, radians, and the ribs' twist, radians per
    /// metre.
    phase: f64,
    twist: f64,
    /// Tubercle sites sorted by distance along the axis, and their foot's
    /// radius, height (share of the radius) and point.
    bumps: Vec<Site>,
    bump_width: f64,
    bump_height: f64,
    bump_point: f64,
    /// A tubercle's length along the axis over its width.
    bump_stretch: f64,
    /// Metres added to the radius: the spines' reach on far levels.
    swell: f64,
}

impl Surface<'_> {
    /// The rib fold at `(s, t)`: 1 on a crest, 0 in a valley; 1 without
    /// folded ribs.
    fn fold(&self, s: f64, t: f64) -> f64 {
        if !self.folded {
            return 1.0;
        }
        let ribs = f64::from(self.ribs);
        let u = ((t - self.phase - self.twist * s) * ribs / (2.0 * PI)).rem_euclid(1.0);
        rib_profile(u, self.sharpness)
    }

    /// The tubercles' sum at `(s, t)`, 0 between them and 1 on a peak.
    fn bump(&self, s: f64, t: f64, radius: f64) -> f64 {
        if self.bumps.is_empty() || self.bump_width <= 0.0 {
            return 0.0;
        }
        let width = self.bump_width;
        let length = width * self.bump_stretch;
        let first = self.bumps.partition_point(|site| site.s < s - length);
        let (sin, cos) = (math::sin(t), math::cos(t));
        let around = radius * math::sqrt(sin * sin + self.flatten * self.flatten * cos * cos);
        let mut sum: f64 = 0.0;
        for site in &self.bumps[first..] {
            let ds = site.s - s;
            if ds > length {
                break;
            }
            let arc = wrap(site.t - t) * around;
            let d2 = (ds * ds) / (length * length) + (arc * arc) / (width * width);
            if d2 < 1.0 {
                let q = (1.0 - d2) * (1.0 - d2);
                sum = sum.max(math::pow(q, self.bump_point));
            }
        }
        sum
    }

    /// Radius of the surface at `(s, t)` and how deep in a valley it lies
    /// (0 on a crest or a tubercle's peak, 1 at a valley's bottom).
    fn radius(&self, s: f64, t: f64) -> (f64, f64) {
        let crest = self.line.crest(s);
        let fold = self.fold(s, t);
        let mut radius = crest * (1.0 - self.depth * (1.0 - fold));
        let mut valley = if self.folded { 1.0 - fold } else { 0.0 };
        if !self.bumps.is_empty() {
            let bump = self.bump(s, t, crest);
            radius += crest * self.bump_height * bump;
            valley = valley.max(1.0 - bump);
        }
        let swell = self.swell * self.line.cap(s);
        (radius + swell, valley)
    }

    /// The surface point at `(s, t)`.
    fn point(&self, s: f64, t: f64) -> Vec3 {
        let station = self.line.at(s);
        let (radius, _) = self.radius(s, t);
        let side = station.tangent.cross(station.across);
        station.point
            + station.across * (radius * math::cos(t))
            + side * (radius * self.flatten * math::sin(t))
    }

    /// The point at `(s, t)`, its outward normal and the unit vector along
    /// the skin toward the tip, by differences of [`Surface::point`].
    fn frame(&self, s: f64, t: f64) -> (Vec3, Vec3, Vec3) {
        let length = self.line.length;
        let ds = (length * 1e-5).max(1e-6);
        let dt = 1e-4;
        let (low, high) = ((s - ds).max(0.0), (s + ds).min(length));
        let along = (self.point(high, t) - self.point(low, t)) / (high - low).max(1e-12);
        let round = (self.point(s, t + dt) - self.point(s, t - dt)) / (2.0 * dt);
        let position = self.point(s, t);
        let station = self.line.at(s);
        let fallback = if s > 0.5 * length {
            station.tangent
        } else {
            -station.tangent
        };
        let normal = round.cross(along).normalize_or(fallback);
        // A closed end: every direction round it meets at one point.
        let normal = if round.length() < 1e-9 {
            fallback
        } else {
            normal
        };
        let up = (along - normal * normal.dot(along)).normalize_or(station.tangent);
        (position, normal, up)
    }
}

/// An axis's body as its centreline's stations, each with the crest's
/// radius there (dome and base included) and its cross-section's frame:
/// an areole of another axis inside it is hidden. Unlike capsules round
/// the segments, a hull ends where the body does, at its dome's apex, so
/// the joints of a cholla keep their areoles up to where they meet.
#[derive(Debug, Clone)]
struct Hull {
    knots: Vec<HullKnot>,
    flatten: f64,
    /// Bounds of the body, to pass over points far from it.
    low: Vec3,
    high: Vec3,
}

#[derive(Debug, Clone, Copy)]
struct HullKnot {
    point: Vec3,
    across: Vec3,
    side: Vec3,
    radius: f64,
}

impl Hull {
    fn new(line: &Centreline, flatten: f64) -> Self {
        let knots: Vec<HullKnot> = line
            .stations
            .iter()
            .map(|station| HullKnot {
                point: station.point,
                across: station.across,
                side: station.tangent.cross(station.across),
                radius: station.radius * line.cap(station.s),
            })
            .collect();
        let widest = knots.iter().map(|knot| knot.radius).fold(0.0, f64::max);
        let grow = Vec3::new(widest, widest, widest);
        let (low, high) = knots.iter().fold(
            (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(low, high), knot| (low.min(knot.point), high.max(knot.point)),
        );
        Self {
            knots,
            flatten,
            low: low - grow,
            high: high + grow,
        }
    }

    /// Whether `point` lies inside the body: within the cross-section at
    /// the nearest point of its axis, and not past its foot or its apex.
    fn holds(&self, point: Vec3) -> bool {
        if point.x < self.low.x
            || point.y < self.low.y
            || point.z < self.low.z
            || point.x > self.high.x
            || point.y > self.high.y
            || point.z > self.high.z
        {
            return false;
        }
        let mut nearest: Option<(f64, usize, f64)> = None;
        for (index, pair) in self.knots.windows(2).enumerate() {
            let span = pair[1].point - pair[0].point;
            let length2 = span.length_squared();
            if length2 <= 0.0 {
                continue;
            }
            let raw = (point - pair[0].point).dot(span) / length2;
            let distance2 = (point - (pair[0].point + span * raw.clamp(0.0, 1.0))).length_squared();
            if nearest.is_none_or(|(best, _, _)| distance2 < best) {
                nearest = Some((distance2, index, raw));
            }
        }
        let Some((_, index, raw)) = nearest else {
            return false;
        };
        if (index == 0 && raw < 0.0) || (index + 2 == self.knots.len() && raw > 1.0) {
            return false;
        }
        let (a, b) = (self.knots[index], self.knots[index + 1]);
        let u = raw.clamp(0.0, 1.0);
        let radius = a.radius + (b.radius - a.radius) * u;
        if radius <= 1e-9 {
            return false;
        }
        let span = b.point - a.point;
        let offset = point - (a.point + span * u);
        let along = offset.dot(span) / span.length();
        let across = offset.dot(a.across.lerp(b.across, u));
        let side = offset.dot(a.side.lerp(b.side, u));
        let thick = radius * self.flatten;
        (across * across + along * along) / (radius * radius) + side * side / (thick * thick) < 1.0
    }
}

/// Areoles along each rib's crest, `spacing` apart along the skin, the
/// ribs offset from each other by the golden section. None where the
/// crests crowd closer than half the spacing, near the apex and a closed
/// base.
fn rib_sites(line: &Centreline, surface: &Surface<'_>, spacing: f64, id: u64) -> Vec<Site> {
    let samples = even_samples(line.length, spacing * 0.25, 40_000);
    let meridian = line.meridian(&samples);
    let total = meridian.last().copied().unwrap_or(0.0);
    let ribs = surface.ribs.max(1);
    let mut sites = Vec::new();
    for rib in 0..ribs {
        let offset = (f64::from(rib) * (1.0 - GOLDEN_TURN)).rem_euclid(1.0);
        let mut row = 0_u32;
        loop {
            let jitter =
                (unit(mix64(id ^ (u64::from(rib) << 32) ^ u64::from(row) ^ 0xA2E0)) - 0.5) * 0.3;
            let along = (f64::from(row) + offset + jitter) * spacing;
            row += 1;
            if along > total {
                break;
            }
            if along < 0.0 {
                continue;
            }
            let s = invert(&meridian, &samples, along);
            let crowd = 2.0 * PI * line.crest(s) / f64::from(ribs);
            if crowd < 0.45 * spacing {
                continue;
            }
            let t = surface.phase + surface.twist * s + 2.0 * PI * f64::from(rib) / f64::from(ribs);
            sites.push(Site {
                s,
                t,
                ordinal: u32::try_from(sites.len()).unwrap_or(u32::MAX),
            });
            if sites.len() >= MAX_AXIS_AREOLES {
                return sites;
            }
        }
    }
    sites
}

/// Areoles on a golden-angle spiral from the apex down, each with
/// `spacing²` of skin: equal steps of skin area along the axis and of the
/// golden angle round it, so neighbours fall into crossing spirals.
fn spiral_sites(line: &Centreline, flatten: f64, phase: f64, spacing: f64) -> Vec<Site> {
    let samples = even_samples(line.length, spacing * 0.25, 40_000);
    let mut area = Vec::with_capacity(samples.len());
    let mut total = 0.0;
    let mut previous: Option<(f64, f64)> = None;
    for &s in &samples {
        let radius = line.crest(s);
        if let Some((ps, pr)) = previous {
            let (ds, dr) = (s - ps, radius - pr);
            let middle = f64::midpoint(radius, pr);
            total += perimeter(middle, middle * flatten) * math::sqrt(ds * ds + dr * dr);
        }
        area.push(total);
        previous = Some((s, radius));
    }
    let share = spacing * spacing;
    // A whole number of areoles, bounded.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = ((total / share).floor() as usize).min(MAX_AXIS_AREOLES);
    let around = Around::new(flatten);
    let start = phase / (2.0 * PI);
    (0..count)
        .map(|index| {
            let ordinal = u32::try_from(index).unwrap_or(u32::MAX);
            let s = invert(&area, &samples, total - (f64::from(ordinal) + 0.5) * share);
            Site {
                s,
                t: around.angle(start + f64::from(ordinal) * GOLDEN_TURN),
                ordinal,
            }
        })
        .collect()
}

/// Mix `a` toward `b` by `t`.
fn mix(a: [f32; 3], b: [f32; 3], t: f64) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    let t = t.clamp(0.0, 1.0) as f32;
    std::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * t)
}

/// Draw the bodies of `graph` at level of detail `level` into `out`: their
/// surfaces into the wood mesh, and their spines as tufts and cards. `looks`
/// holds one look per body type of the program (see
/// [`crate::spec::Appearance::body_looks`]); the spine cards' templates
/// follow the `first_template` organ templates, a star and a fan per body
/// type (see [`crate::templates::Templates::for_plant`]).
pub fn build(
    graph: &PlantGraph,
    looks: &[BodyLook],
    first_template: usize,
    level: usize,
    out: &mut PlantMesh,
) {
    if looks.is_empty() || !graph.segments.iter().any(|segment| segment.body > 0) {
        return;
    }
    let mut detail = DETAIL[level.min(DETAIL.len() - 1)];
    let axes = axes(graph);
    let lines: Vec<Option<(&BodyLook, f64, Centreline)>> = axes
        .iter()
        .map(|axis| {
            let look = looks.get(axis.body)?;
            let phase = unit(mix64(axis.id ^ 0xB0D7)) * 2.0 * PI;
            Centreline::new(graph, axis, look, phase).map(|line| (look, phase, line))
        })
        .collect();
    let hulls: Vec<Option<Hull>> = lines
        .iter()
        .map(|line| {
            line.as_ref()
                .map(|(look, _, line)| Hull::new(line, look.flatten))
        })
        .collect();
    let (axes, lines, hulls) = (&axes, &lines, &hulls);
    let bodies = |detail: Detail| {
        axes.iter()
            .zip(lines)
            .enumerate()
            .filter_map(move |(index, (axis, line))| {
                let (look, phase, line) = line.as_ref()?;
                Some(Body {
                    graph,
                    axis,
                    index,
                    look,
                    line,
                    detail,
                    phase: *phase,
                    hulls,
                    first_template,
                })
            })
    };
    // A plant of thousands of small tubercles, as a chain-fruit cholla, or
    // of many tall ribbed stems, as an organ pipe, would take a million
    // triangles at the near levels. Until its skin fits the level's
    // budget its tubercles get fewer vertices, its domes and feet fewer
    // rings, its rings lie farther apart and bend more between them, and
    // its smooth rings and then its ribs get fewer vertices round.
    let budget = SKIN_TRIANGLES[level.min(SKIN_TRIANGLES.len() - 1)];
    let tubercled = axes.iter().any(|axis| {
        looks
            .get(axis.body)
            .is_some_and(|look| look.tubercles.is_some() && look.areoles.spacing > 0.0)
    });
    let longest = detail.ring_step * MAX_RING_STRETCH;
    let turns = detail.max_turn * MAX_RING_STRETCH;
    let changes = detail.max_change * MAX_RING_STRETCH;
    // Each way in turn, until it is spent or stops paying.
    let mut way = u8::from(!tubercled);
    let mut last = usize::MAX;
    for _ in 0..12 {
        let skin: usize = bodies(detail).map(|body| body.triangles()).sum();
        if skin <= budget {
            break;
        }
        #[allow(clippy::cast_precision_loss)]
        if skin as f64 > 0.95 * last as f64 {
            way += 1;
        }
        last = skin;
        #[allow(clippy::cast_precision_loss)]
        let over = skin as f64 / budget as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let fewer = |count: u32, least: u32| ((f64::from(count) / over) as u32).max(least);
        match way {
            0 if detail.per_tubercle > MIN_PER_TUBERCLE => {
                detail.per_tubercle =
                    (detail.per_tubercle / math::sqrt(over)).max(MIN_PER_TUBERCLE);
            }
            0 | 1 if detail.cap_rings > MIN_CAP_RINGS => {
                way = 1;
                detail.cap_rings = fewer(detail.cap_rings, MIN_CAP_RINGS);
            }
            0..=2 if detail.ring_step < longest => {
                way = 2;
                detail.ring_step = (detail.ring_step * over).min(longest);
                detail.max_turn = (detail.max_turn * over).min(turns);
                detail.max_change = (detail.max_change * over).min(changes);
            }
            0..=3 if detail.smooth_sides > MIN_SMOOTH_SIDES => {
                way = 3;
                detail.smooth_sides = fewer(detail.smooth_sides, MIN_SMOOTH_SIDES);
            }
            0..=4 if detail.per_rib > 2 => {
                way = 4;
                detail.per_rib -= 2;
            }
            _ => break,
        }
    }
    let mut areoles = 0;
    for body in bodies(detail) {
        areoles += body.draw(out, MAX_PLANT_AREOLES.saturating_sub(areoles));
    }
}

/// An axis's grid at one level.
struct Layout {
    /// Ribs round the axis, and whether the level folds them.
    ribs: u32,
    folded: bool,
    /// Width of a tubercle's foot; 0 for none at this level.
    bump_width: f64,
    /// Vertices round a ring, and the longest step between rings.
    sides: u32,
    step: f64,
}

/// One axis being drawn.
struct Body<'a> {
    graph: &'a PlantGraph,
    axis: &'a Axis,
    /// The axis's index, to tell its own hull from the others'.
    index: usize,
    look: &'a BodyLook,
    line: &'a Centreline,
    detail: Detail,
    phase: f64,
    hulls: &'a [Option<Hull>],
    first_template: usize,
}

impl Body<'_> {
    /// The axis's grid at this level: its ribs, the vertices round a ring
    /// and the longest step between rings.
    fn layout(&self) -> Layout {
        let (look, line, detail) = (self.look, self.line, self.detail);
        let widest = line
            .stations
            .iter()
            .map(|station| station.radius)
            .fold(0.0, f64::max);
        let mean = line
            .stations
            .iter()
            .map(|station| station.radius)
            .sum::<f64>()
            / f64::from(u32::try_from(line.stations.len()).unwrap_or(1).max(1));

        // Ribs round this axis, and whether this level folds them.
        let ribs = look.ribs.as_ref().map_or(0, |ribs| {
            // A small whole number of ribs.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let fitted = (2.0 * PI * widest / ribs.spacing).round() as u32;
            fitted.clamp(ribs.count[0], ribs.count[1]).max(3)
        });
        let per_rib = if ribs > 0 && detail.per_rib > 0 {
            let per = detail.per_rib.min(detail.max_sides / ribs);
            if per >= 2 { per - per % 2 } else { 0 }
        } else {
            0
        };
        let folded = per_rib >= 2;
        let spacing = look.areoles.spacing;
        let tubercles = look
            .tubercles
            .as_ref()
            .filter(|_| detail.per_tubercle > 0.0 && spacing > 0.0);
        let bump_width = tubercles.map_or(0.0, |tubercles| tubercles.width * spacing);

        let sides = if folded {
            ribs * per_rib
        } else {
            let mut sides = f64::from(detail.smooth_sides);
            if look.flatten < 0.6 {
                sides *= 1.5;
            }
            if bump_width > 0.0 {
                sides = sides.max(2.0 * PI * widest / (bump_width / detail.per_tubercle));
            }
            // A small whole number of sides.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let sides = sides.ceil() as u32;
            sides.clamp(3, detail.max_sides)
        };
        let mut step = detail.ring_step * mean.max(1e-3);
        if bump_width > 0.0 {
            let stretch = tubercles.map_or(1.0, |tubercles| tubercles.stretch);
            step = step.min(bump_width * stretch / detail.per_tubercle);
        }
        Layout {
            ribs,
            folded,
            bump_width,
            sides,
            step,
        }
    }

    /// The triangles of the axis's skin at this level.
    fn triangles(&self) -> usize {
        let layout = self.layout();
        let rings = self.line.rings(&self.detail, layout.step).len();
        2 * layout.sides as usize * rings.saturating_sub(1)
    }

    /// Draw the axis into `out`, with at most `budget` areoles; the number
    /// of areoles drawn.
    fn draw(&self, out: &mut PlantMesh, budget: usize) -> usize {
        let (look, line, detail) = (self.look, self.line, self.detail);
        let Layout {
            ribs,
            folded,
            bump_width,
            sides,
            step,
        } = self.layout();
        let spacing = look.areoles.spacing;
        let tubercles = look
            .tubercles
            .as_ref()
            .filter(|_| detail.per_tubercle > 0.0 && spacing > 0.0);

        let (ribs_depth, sharpness, twist) = look.ribs.as_ref().map_or((0.0, 0.0, 0.0), |ribs| {
            (ribs.depth, ribs.sharpness, math::radians(ribs.twist))
        });
        let mut surface = Surface {
            line,
            flatten: look.flatten,
            ribs,
            folded,
            depth: if folded { ribs_depth } else { 0.0 },
            sharpness,
            phase: self.phase,
            twist,
            bumps: Vec::new(),
            bump_width,
            bump_height: tubercles.map_or(0.0, |tubercles| tubercles.height),
            bump_point: tubercles.map_or(1.0, |tubercles| tubercles.point),
            bump_stretch: tubercles.map_or(1.0, |tubercles| tubercles.stretch),
            swell: 0.0,
        };

        // Areoles: placed on the plain surface, those inside another
        // axis's body left out.
        let wants_sites = spacing > 0.0 && (detail.spines != Spines::Tint || bump_width > 0.0);
        let mut sites = if wants_sites {
            if ribs > 0 {
                rib_sites(line, &surface, spacing, self.axis.id)
            } else {
                spiral_sites(line, look.flatten, self.phase, spacing)
            }
        } else {
            Vec::new()
        };
        sites.truncate(budget);
        if !sites.is_empty() {
            let plain = surface.shallow();
            sites.retain(|site| {
                let point = plain.point(site.s, site.t);
                !self.hulls.iter().enumerate().any(|(index, hull)| {
                    index != self.index && hull.as_ref().is_some_and(|hull| hull.holds(point))
                })
            });
        }
        // Up the axis, so the tufts a renderer culls together (see
        // `after-vegetation-render`'s chunks) lie in bands round it.
        sites.sort_by(|a, b| a.s.total_cmp(&b.s));
        if bump_width > 0.0 {
            surface.bumps = sites.clone();
        }

        // Far levels: the spines tint the skin and swell the outline.
        let (tint, cover) = look.tint();
        let tinted = detail.spines == Spines::Tint && cover > 0.0;
        if tinted {
            surface.swell = f64::from(spines::reach(look).out) * cover;
        }

        self.mesh(
            &surface,
            sides,
            step,
            (tint, if tinted { cover } else { 0.0 }),
            out,
        );

        if detail.spines != Spines::Tint {
            self.spines(&surface, &sites, out);
        }
        sites.len()
    }

    /// The surface as a grid of rings, `sides` vertices round each.
    fn mesh(
        &self,
        surface: &Surface<'_>,
        sides: u32,
        step: f64,
        (tint, cover): ([f32; 3], f64),
        out: &mut PlantMesh,
    ) {
        let look = self.look;
        let rings = self.line.rings(&self.detail, step);
        let valley = look.valley_colour();
        // Ribs too fine for this level still shade the skin.
        let skin = if look.ribs.is_some() && !surface.folded {
            mix(look.colour, valley, 0.35)
        } else {
            look.colour
        };
        let cork = look.cork.as_ref();
        let cork_line = look.cork_line(self.graph.age);
        let wobble_phase = unit(mix64(self.axis.id ^ 0xC0E4)) * 2.0 * PI;
        let wood = &mut out.wood;
        let mut previous: Option<u32> = None;
        for &s in &rings {
            let station = self.line.at(s);
            let first = u32::try_from(wood.positions.len()).unwrap_or(u32::MAX);
            for side in 0..=sides {
                let t = self.phase
                    + surface.twist * s
                    + 2.0 * PI * f64::from(side % sides) / f64::from(sides);
                let (position, normal, _) = surface.frame(s, t);
                let (_, deep) = surface.radius(s, t);
                let mut colour = mix(skin, valley, deep);
                if let Some(cork) = cork
                    && cork_line > 0.0
                {
                    let wobble = 0.12
                        * math::sin(3.0 * t + wobble_phase)
                        * math::cos(2.0 * t + 5.0 * s + wobble_phase);
                    let band = 0.06 + 0.05 * cork_line;
                    let weight =
                        math::smoothstep(cork_line + band, cork_line - band, position.y + wobble);
                    colour = mix(colour, cork.colour, weight);
                }
                if cover > 0.0 {
                    colour = mix(colour, tint, cover);
                }
                #[allow(clippy::cast_possible_truncation)]
                let uv = [(f64::from(side) / f64::from(sides)) as f32, s as f32];
                wood.push(Vertex {
                    position,
                    normal,
                    uv,
                    color: [colour[0], colour[1], colour[2], 1.0],
                    born: station.born,
                    shed: station.shed,
                    level: 0,
                });
            }
            if let Some(below) = previous {
                for side in 0..sides {
                    let a = below + side;
                    let b = first + side;
                    // Counter-clockwise seen from outside.
                    wood.indices
                        .extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
                }
            }
            previous = Some(first);
        }
    }

    /// A tuft per areole on levels that draw them, and its star and fan
    /// cards.
    fn spines(&self, surface: &Surface<'_>, sites: &[Site], out: &mut PlantMesh) {
        let look = self.look;
        let line = self.line;
        let age = self.graph.age;
        let cork_line = look.cork_line(age);
        let reach = spines::reach(look);
        let colour = spines::colour(look);
        let star = self.first_template + 2 * self.axis.body;
        let templates = u8::try_from(star).ok().zip(u8::try_from(star + 1).ok());
        let body = u8::try_from(self.axis.body).unwrap_or(u8::MAX);
        for site in sites {
            let (position, normal, up) = surface.frame(site.s, site.t);
            let seed64 = mix64(self.axis.id ^ mix64(u64::from(site.ordinal) + 1));
            if cork_line > 0.0 && position.y < cork_line {
                let keep = look.cork.as_ref().map_or(1.0, |cork| cork.spines);
                if unit(mix64(seed64 ^ 0xC0)) >= keep {
                    continue;
                }
            }
            // Young areoles on the dome carry short spines.
            let dome_start = line.length - line.dome;
            let scale = if line.dome > 0.0 && site.s > dome_start {
                let x = ((site.s - dome_start) / line.dome).min(1.0);
                1.0 - 0.45 * x * x
            } else {
                1.0
            };
            let station = line.at(site.s);
            let grey = look.greying.as_ref().map_or(0.0, |greying| {
                ((age - station.born) / greying.years).clamp(0.0, 1.0)
            });
            // The low 32 bits of the seed.
            #[allow(clippy::cast_possible_truncation)]
            let seed = seed64 as u32;
            #[allow(clippy::cast_possible_truncation)]
            let tuft = Tuft {
                position: position.to_f32(),
                normal: normal.to_f32(),
                up: up.to_f32(),
                scale: scale as f32,
                grey: grey as f32,
                seed,
                body,
                born: station.born as f32,
                shed: station.shed.map_or(f32::INFINITY, |shed| shed as f32),
            };
            // A tuft is pushed only with its two cards, so on a level that
            // draws both the last cards are each tuft's star and fan in
            // order, which a renderer hides as it draws the tufts solid.
            let Some((star, fan)) = templates else {
                continue;
            };
            if self.detail.spines == Spines::Tufts {
                out.tufts.push(tuft);
            }
            let card_colour = match &look.greying {
                Some(greying) => mix(colour, greying.colour, grey),
                None => colour,
            };
            let across = f64::from(reach.across) * scale;
            let side = normal.cross(up);
            #[allow(clippy::cast_possible_truncation)]
            let (wide, tall) = ((2.0 * across) as f32, (f64::from(reach.out) * scale) as f32);
            let lift = f64::from(reach.out) * scale * 0.25;
            out.cards.push(Card {
                base: (position + normal * lift - up * across).to_f32(),
                heading: up.to_f32(),
                left: side.to_f32(),
                length: wide,
                width: wide,
                color: card_colour,
                template: star,
                born: tuft.born,
                shed: tuft.shed,
            });
            // Fans stand along the axis and across it in turn, so the
            // outline bristles from every side.
            let flank = if site.ordinal % 2 == 0 { up } else { side };
            out.cards.push(Card {
                base: position.to_f32(),
                heading: normal.to_f32(),
                left: flank.to_f32(),
                length: tall,
                width: wide,
                color: card_colour,
                template: fan,
                born: tuft.born,
                shed: tuft.shed,
            });
        }
    }
}

impl Surface<'_> {
    /// This surface without tubercles or swelling: where areoles are placed
    /// before the tubercles raised under them exist.
    fn shallow(&self) -> Surface<'_> {
        Surface {
            line: self.line,
            flatten: self.flatten,
            ribs: self.ribs,
            folded: self.folded,
            depth: self.depth,
            sharpness: self.sharpness,
            phase: self.phase,
            twist: self.twist,
            bumps: Vec::new(),
            bump_width: 0.0,
            bump_height: 0.0,
            bump_point: 1.0,
            bump_stretch: 1.0,
            swell: 0.0,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::graph::GraphSegment;

    fn segment(
        id: u64,
        parent: Option<u32>,
        lateral: bool,
        start: Vec3,
        end: Vec3,
        radius: f64,
    ) -> GraphSegment {
        GraphSegment {
            id,
            parent,
            lateral,
            order: u16::from(lateral),
            start,
            end,
            radius,
            born: 0.0,
            shed: None,
            body: 1,
            left: Vec3::X,
        }
    }

    /// A column `height` tall of `pieces` segments of radius `radius`.
    fn column(height: f64, radius: f64, pieces: u32) -> PlantGraph {
        let step = height / f64::from(pieces);
        let segments = (0..pieces)
            .map(|piece| {
                let y = f64::from(piece) * step;
                segment(
                    u64::from(piece) + 1,
                    piece.checked_sub(1),
                    false,
                    Vec3::new(0.0, y, 0.0),
                    Vec3::new(0.0, y + step, 0.0),
                    radius,
                )
            })
            .collect();
        PlantGraph {
            age: 20.0,
            height,
            segments,
            organs: Vec::new(),
        }
    }

    fn ribbed() -> BodyLook {
        BodyLook {
            ribs: Some(Ribs {
                count: [12, 12],
                depth: 0.2,
                ..Ribs::default()
            }),
            spines: vec![SpineSet::default()],
            ..BodyLook::default()
        }
    }

    fn mesh(graph: &PlantGraph, look: &BodyLook, level: usize) -> PlantMesh {
        let mut out = PlantMesh::default();
        build(graph, std::slice::from_ref(look), 0, level, &mut out);
        out
    }

    fn radial(position: [f32; 3]) -> f64 {
        f64::from(position[0]).hypot(f64::from(position[2]))
    }

    #[test]
    fn looks_parse_with_defaults_and_reject_unknown_fields() {
        let look: BodyLook = serde_json::from_str(
            r#"{ "colour": [0.1, 0.2, 0.1], "ribs": { "count": [10, 20] },
                 "spines": [ { "count": [1, 1], "hook": 150 } ] }"#,
        )
        .unwrap();
        assert_eq!(look.ribs.as_ref().unwrap().depth, Ribs::default().depth);
        assert_eq!(look.spines[0].hook, 150.0);
        assert_eq!(look.spines[0].length, SpineSet::default().length);
        look.validate().unwrap();
        assert!(serde_json::from_str::<BodyLook>(r#"{ "ribz": {} }"#).is_err());
        let text = serde_json::to_string(&look).unwrap();
        assert_eq!(serde_json::from_str::<BodyLook>(&text).unwrap(), look);
        let mut bad = look.clone();
        bad.ribs.as_mut().unwrap().count = [12, 8];
        assert!(bad.validate().unwrap_err().contains("ribs count"));
        let mut bad = look;
        bad.spines = vec![
            SpineSet {
                count: [40, 40],
                ..SpineSet::default()
            };
            2
        ];
        assert!(bad.validate().unwrap_err().contains("at most"));
    }

    #[test]
    fn a_ribbed_column_folds_into_crests_and_valleys_and_closes_its_dome() {
        let graph = column(2.0, 0.2, 4);
        let look = ribbed();
        let out = mesh(&graph, &look, 0);
        let wood = &out.wood;
        assert!(wood.vertex_count() > 0);
        // Along the column, every vertex lies between a valley's radius
        // and a crest's.
        let mut widest: f64 = 0.0;
        let mut narrowest = f64::MAX;
        for position in &wood.positions {
            if position[1] > 0.2 && position[1] < 1.5 {
                let r = radial(*position);
                widest = widest.max(r);
                narrowest = narrowest.min(r);
            }
        }
        assert!((widest - 0.2).abs() < 2e-3, "crests at {widest}");
        assert!((narrowest - 0.16).abs() < 2e-3, "valleys at {narrowest}");
        // The dome closes at the top of the column.
        let top = wood
            .positions
            .iter()
            .map(|position| f64::from(position[1]))
            .fold(0.0, f64::max);
        assert!((top - 2.0).abs() < 1e-3, "top at {top}");
        // Normals face out of the column, and up on the dome.
        for (position, normal) in wood.positions.iter().zip(&wood.normals) {
            let out = f64::from(normal[0]) * f64::from(position[0])
                + f64::from(normal[2]) * f64::from(position[2]);
            assert!(out > -1e-6 || normal[1] > 0.5, "{position:?} {normal:?}");
        }
        // Twelve ribs of six sides each.
        assert_eq!(wood.vertex_count() % (12 * 6 + 1), 0);
    }

    #[test]
    fn areoles_sit_on_the_crests_and_the_spines_follow_the_level() {
        let graph = column(2.0, 0.2, 4);
        let look = ribbed();
        let near = mesh(&graph, &look, 0);
        // About one areole per spacing along each crest.
        let expected = 12.0 * 2.0 / look.areoles.spacing;
        #[allow(clippy::cast_precision_loss)]
        let count = near.tufts.len() as f64;
        assert!(
            count > expected * 0.8 && count < expected * 1.2,
            "{count} areoles, about {expected} expected"
        );
        for tuft in &near.tufts {
            if tuft.position[1] > 0.1 && tuft.position[1] < 1.6 {
                assert!((radial(tuft.position) - 0.2).abs() < 2e-3, "{tuft:?}");
            }
        }
        // Two cards per areole near; no tufts a level farther; no spines
        // at all beyond, where they tint the skin instead.
        assert_eq!(near.cards.len(), 2 * near.tufts.len());
        // Each tuft's star and fan follow in its order, as a renderer
        // hiding them reads them, and the tufts run up the axis.
        let first = near.cards.len() - 2 * near.tufts.len();
        for (index, tuft) in near.tufts.iter().enumerate() {
            let star = &near.cards[first + 2 * index];
            let fan = &near.cards[first + 2 * index + 1];
            assert_eq!(fan.base, tuft.position);
            assert_eq!(star.template + 1, fan.template);
            assert_eq!((star.born, fan.born), (tuft.born, tuft.born));
        }
        assert!(
            near.tufts
                .windows(2)
                .all(|pair| pair[0].position[1] <= pair[1].position[1] + 1e-4)
        );
        let middle = mesh(&graph, &look, 1);
        assert!(middle.tufts.is_empty());
        assert_eq!(middle.cards.len(), near.cards.len());
        let far = mesh(&graph, &look, 2);
        assert!(far.tufts.is_empty() && far.cards.is_empty());
        assert!(far.wood.vertex_count() < middle.wood.vertex_count());
        assert!(middle.wood.vertex_count() < near.wood.vertex_count());
        let farthest = mesh(&graph, &look, 3);
        assert!(farthest.wood.vertex_count() < far.wood.vertex_count());
        assert_ne!(far.wood.colors[0], near.wood.colors[0]);
    }

    #[test]
    fn a_spiral_spaces_areoles_evenly_over_the_skin() {
        let graph = column(0.3, 0.06, 3);
        let look = BodyLook {
            areoles: Areoles {
                spacing: 0.01,
                ..Areoles::default()
            },
            ..BodyLook::default()
        };
        let out = mesh(&graph, &look, 0);
        let points: Vec<Vec3> = out
            .tufts
            .iter()
            .map(|tuft| {
                Vec3::new(
                    f64::from(tuft.position[0]),
                    f64::from(tuft.position[1]),
                    f64::from(tuft.position[2]),
                )
            })
            .collect();
        assert!(points.len() > 200, "{} areoles", points.len());
        let mut nearest = Vec::new();
        for (i, a) in points.iter().enumerate() {
            let d = points
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, b)| (*a - *b).length())
                .fold(f64::MAX, f64::min);
            nearest.push(d);
        }
        // Along the side, no two areoles crowd and none stands alone.
        for (point, d) in points.iter().zip(&nearest) {
            if point.y > 0.03 && point.y < 0.2 {
                assert!(*d > 0.5 * 0.01 && *d < 1.6 * 0.01, "{point:?}: {d}");
            }
        }
    }

    #[test]
    fn a_pad_is_flat_across_its_left_vector() {
        let mut graph = column(0.3, 0.1, 3);
        for segment in &mut graph.segments {
            segment.left = Vec3::X;
        }
        let look = BodyLook {
            flatten: 0.2,
            base: 0.8,
            ..BodyLook::default()
        };
        let out = mesh(&graph, &look, 0);
        let (mut x, mut z) = (0.0_f64, 0.0_f64);
        for position in &out.wood.positions {
            x = x.max(f64::from(position[0]).abs());
            z = z.max(f64::from(position[2]).abs());
        }
        assert!((x - 0.1).abs() < 2e-3, "{x}");
        assert!((z - 0.02).abs() < 2e-3, "{z}");
        // The rounded base closes at the foot.
        let lowest = out
            .wood
            .positions
            .iter()
            .filter(|position| position[1].abs() < 1e-6)
            .map(|position| radial(*position))
            .fold(0.0, f64::max);
        assert!(lowest < 1e-6, "{lowest}");
    }

    #[test]
    fn an_arm_curves_smoothly_and_loses_its_areoles_inside_the_trunk() {
        let mut graph = column(3.0, 0.25, 6);
        // An arm leaving the trunk's axis at 1.5 m: out level, then up.
        let points = [
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.4, 1.55, 0.0),
            Vec3::new(0.7, 1.7, 0.0),
            Vec3::new(0.85, 2.0, 0.0),
            Vec3::new(0.9, 2.4, 0.0),
            Vec3::new(0.9, 2.8, 0.0),
        ];
        for (index, pair) in points.windows(2).enumerate() {
            let parent = if index == 0 {
                Some(2)
            } else {
                Some(u32::try_from(graph.segments.len() - 1).unwrap())
            };
            graph.segments.push(segment(
                100 + index as u64,
                parent,
                index == 0,
                pair[0],
                pair[1],
                0.15,
            ));
        }
        let look = ribbed();
        let out = mesh(&graph, &look, 0);
        // No areole of the arm lies inside the trunk, nor of the trunk
        // inside the arm.
        for tuft in &out.tufts {
            let p = tuft.position;
            // Below the trunk's dome.
            let inside_trunk = radial(p) < 0.24 && p[1] < 2.7;
            assert!(!inside_trunk, "{p:?}");
            let arm = Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            let near_arm_root = (arm - Vec3::new(0.2, 1.51, 0.0)).length() < 0.1;
            assert!(!near_arm_root, "{p:?}");
        }
        assert_eq!(axes(&graph).len(), 2);
        // The arm's rings turn by less than the level's most.
        let line = Centreline::new(&graph, &axes(&graph)[1], &look, 0.0).unwrap();
        let rings = line.rings(&DETAIL[0], 0.05);
        for pair in rings.windows(2) {
            let (a, b) = (line.at(pair[0]).tangent, line.at(pair[1]).tangent);
            assert!(a.dot(b) > math::cos(math::radians(6.0)), "{pair:?}");
        }
    }

    /// A cholla's joint 12 cm long and 3 cm thick and two daughters
    /// leaving it 0.3 radii under its apex at 50 degrees to either side.
    fn joints(sides: &[f64]) -> PlantGraph {
        let radius = 0.03;
        let mut graph = column(0.12, radius, 3);
        for (k, &side) in sides.iter().enumerate() {
            let start = Vec3::new(0.0, 0.12 - 0.3 * radius, 0.0);
            let angle = math::radians(50.0);
            let heading = Vec3::new(side * math::sin(angle), math::cos(angle), 0.0);
            let first = u32::try_from(graph.segments.len()).unwrap();
            for piece in 0..3_u32 {
                let from = start + heading * (0.04 * f64::from(piece));
                graph.segments.push(segment(
                    100 * (k as u64 + 1) + u64::from(piece),
                    Some(if piece == 0 { 2 } else { first + piece - 1 }),
                    piece == 0,
                    from,
                    from + heading * 0.04,
                    radius,
                ));
            }
        }
        graph
    }

    #[test]
    fn joints_keep_their_areoles_up_to_where_they_meet() {
        let look = BodyLook {
            base: 0.8,
            areoles: Areoles {
                spacing: 0.01,
                ..Areoles::default()
            },
            spines: vec![SpineSet::default()],
            ..BodyLook::default()
        };
        let alone = mesh(&joints(&[]), &look, 0).tufts.len();
        let both = mesh(&joints(&[1.0, -1.0]), &look, 0).tufts;
        let start = Vec3::new(0.0, 0.12 - 0.3 * 0.03, 0.0);
        let dome = Vec3::new(0.0, 0.12 - 0.03, 0.0);
        for tuft in &both {
            let p = Vec3::new(
                f64::from(tuft.position[0]),
                f64::from(tuft.position[1]),
                f64::from(tuft.position[2]),
            );
            // Inside the joint: within its radius below its dome, or in
            // the dome's half sphere.
            let off_joint = p.x.hypot(p.z);
            let in_joint = if p.y < dome.y {
                off_joint < 0.95 * 0.03
            } else {
                (p - dome).length() < 0.95 * 0.03
            };
            // Inside a daughter, along it past its rounded foot.
            let in_daughter = [1.0, -1.0].iter().any(|&side: &f64| {
                let angle = math::radians(50.0);
                let heading = Vec3::new(side * math::sin(angle), math::cos(angle), 0.0);
                let distance = (p - start).dot(heading);
                (0.03..0.09).contains(&distance)
                    && (p - start - heading * distance).length() < 0.95 * 0.03
            });
            assert!(!in_joint || !in_daughter, "{p:?}");
            let near_joint_axis = off_joint < 0.5 * 0.03 && (0.03..dome.y).contains(&p.y);
            assert!(!near_joint_axis, "{p:?}");
        }
        // Three joints' worth of areoles, but for the daughters' feet sunk
        // in the joint and each other.
        let kept = f64::from(u32::try_from(both.len()).unwrap())
            / f64::from(u32::try_from(3 * alone).unwrap());
        assert!(kept > 0.8, "kept {} of {}", both.len(), 3 * alone);
    }

    #[test]
    fn a_hull_ends_at_its_apex_and_keeps_a_pad_thin() {
        // A domed column 0.2 m tall and 0.05 m in radius.
        let graph = column(0.2, 0.05, 4);
        let axis = &axes(&graph)[0];
        let round = BodyLook::default();
        let line = Centreline::new(&graph, axis, &round, 0.0).unwrap();
        let hull = Hull::new(&line, 1.0);
        assert!(hull.holds(Vec3::new(0.04, 0.1, 0.0)));
        assert!(!hull.holds(Vec3::new(0.06, 0.1, 0.0)));
        assert!(hull.holds(Vec3::new(0.0, 0.19, 0.0)));
        // The dome is narrower than the column near its top, and nothing
        // past the apex or under the foot is inside, as a capsule round
        // the segments would have it.
        assert!(!hull.holds(Vec3::new(0.04, 0.195, 0.0)));
        assert!(!hull.holds(Vec3::new(0.0, 0.215, 0.0)));
        assert!(!hull.holds(Vec3::new(0.0, -0.01, 0.0)));
        // A pad a fifth as thick as it is wide: 1 cm either side of its
        // middle, 5 cm across it.
        let pad = BodyLook {
            flatten: 0.2,
            ..BodyLook::default()
        };
        let line = Centreline::new(&graph, axis, &pad, 0.0).unwrap();
        let hull = Hull::new(&line, pad.flatten);
        let station = line.at(0.1);
        let side = station.tangent.cross(station.across);
        assert!(hull.holds(station.point + station.across * 0.045));
        assert!(hull.holds(station.point + side * 0.008));
        assert!(!hull.holds(station.point + side * 0.015));
    }

    #[test]
    fn tubercles_raise_bumps_under_their_areoles() {
        let graph = column(0.2, 0.05, 2);
        let look = BodyLook {
            tubercles: Some(Tubercles {
                height: 0.3,
                ..Tubercles::default()
            }),
            areoles: Areoles {
                spacing: 0.012,
                ..Areoles::default()
            },
            ..BodyLook::default()
        };
        let out = mesh(&graph, &look, 0);
        let mut highest: f64 = 0.0;
        for tuft in &out.tufts {
            if tuft.position[1] > 0.03 && tuft.position[1] < 0.12 {
                highest = highest.max(radial(tuft.position));
                // Each areole sits on its tubercle's peak.
                assert!(radial(tuft.position) > 0.05 * 1.25, "{tuft:?}");
            }
        }
        assert!((highest - 0.065).abs() < 2e-3, "{highest}");
        let far = mesh(&graph, &look, 2);
        let widest = far
            .wood
            .positions
            .iter()
            .map(|position| radial(*position))
            .fold(0.0, f64::max);
        assert!(widest < 0.055, "far levels draw no tubercles: {widest}");
    }

    #[test]
    fn cork_rises_from_the_ground_and_takes_the_spines() {
        let mut graph = column(2.0, 0.2, 4);
        graph.age = 100.0;
        let look = BodyLook {
            cork: Some(Cork {
                spines: 0.0,
                ..Cork::default()
            }),
            ..ribbed()
        };
        assert!((look.cork_line(100.0) - 1.2).abs() < 1e-9);
        let out = mesh(&graph, &look, 0);
        assert!(out.tufts.iter().all(|tuft| tuft.position[1] > 1.0));
        let low = out
            .wood
            .positions
            .iter()
            .zip(&out.wood.colors)
            .find(|(position, _)| position[1] < 0.1)
            .unwrap()
            .1;
        let cork = Cork::default().colour;
        assert!((low[0] - cork[0]).abs() < 0.02, "{low:?}");
    }

    #[test]
    fn the_same_body_is_drawn_the_same_every_time() {
        let graph = column(1.0, 0.1, 3);
        let look = ribbed();
        assert_eq!(mesh(&graph, &look, 0), mesh(&graph, &look, 0));
        // Plants without bodies draw nothing here.
        let mut wood = graph.clone();
        for segment in &mut wood.segments {
            segment.body = 0;
        }
        assert_eq!(mesh(&wood, &look, 0), PlantMesh::default());
    }
}
