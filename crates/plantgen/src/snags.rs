//! Standing dead trees: the snags of a natural stand. How many a stand
//! holds at each age and how thick, tall and decayed they are
//! ([`Snags`]), where they stand among the living trees ([`stand`]), and
//! how each is drawn ([`draw`]).
//!
//! The numbers are from the plots. Spies, Franklin and Thomas (1988,
//! Ecology 69: 1689-1702, table 4) counted every standing dead tree 10 cm
//! or more across on 196 natural Douglas-fir stands of western Oregon and
//! Washington, 40 to 900 years old: young stands (40-80 years) hold the
//! most, 171 a hectare, most of them thin, the stand's own crowding having
//! killed them, with thick ones left from the stand before; mature stands
//! (80-195 years) 121; old growth 60, the fewest but the thickest and
//! tallest. Trees there "typically die standing". Half to two thirds of
//! the snags stand over 5 m; the rest are broken off lower.
//!
//! How a snag looks follows the five decay classes the Forest Inventory
//! and Analysis crews use (Domke, Woodall and Smith 2011, Carbon Balance
//! and Management 6: 14, table 1): class 1 keeps all its limbs and
//! branches, all its bark and a pointed top; class 2 a few limbs and no
//! fine branches, its top perhaps broken; class 3 only limb stubs, its top
//! broken; class 4 few or no stubs; class 5 no limbs and under a fifth of
//! its bark, its sapwood gone. Its bark sloughs off as it rots: a snag
//! keeps 92, 66, 39 and 21% of it through classes 1 to 4 (their table 2).
//! Every rule here holds for any tree: a snag's species changes only its
//! bark.

use serde::{Deserialize, Serialize};

use crate::deadwood::{self, Stem};
use crate::development::Origin;
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng;
use crate::spec::Appearance;

/// The breast-height diameter that parts thin snags from thick ones in
/// Spies et al.'s (1988) table 4, metres.
pub const THICK_M: f64 = 0.5;

/// The thinnest snag the plots counted and the thickest drawn, metres:
/// 10 cm, and 1.5 m, about the widest old-growth Douglas-fir (as
/// [`deadwood::SIZE_CLASSES_M`]).
pub const DBH_RANGE_M: [f64; 2] = [0.10, 1.50];

/// Snags a hectare under and over [`THICK_M`] in young stands, 40-80 years
/// (Spies et al. 1988, table 4, all provinces).
pub const YOUNG_SNAGS: [f64; 2] = [145.0, 27.0];
/// ...in mature stands, 80-195 years...
pub const MATURE_SNAGS: [f64; 2] = [105.0, 16.0];
/// ...and in old growth, 200-900 years.
pub const OLD_SNAGS: [f64; 2] = [32.0, 27.0];

/// The share of thin and of thick snags that stand over 5 m, in young,
/// mature and old-growth stands (table 4: over 5 m tall 108, 73 and 30 a
/// hectare, of them over 50 cm 10, 8 and 14).
pub const OVER_5M: [[f64; 2]; 3] = [
    [98.0 / 145.0, 10.0 / 27.0],
    [65.0 / 105.0, 8.0 / 16.0],
    [16.0 / 32.0, 14.0 / 27.0],
];

/// The share of thick snags that stand over 15 m in young, mature and
/// old-growth stands (table 4: 2.8, 2.5 and 6.4 a hectare).
pub const OVER_15M: [f64; 3] = [2.8 / 27.0, 2.5 / 16.0, 6.4 / 27.0];

/// The mean breast-height diameter of all snags in young, mature and
/// old-growth stands, metres (table 4: 33, 32 and 55 cm).
pub const MEAN_DBH_M: [f64; 3] = [0.33, 0.32, 0.55];

/// How far thin snags crowd towards the thinnest: a diameter is drawn as
/// the smallest plus the class's span times a draw to this power, so the
/// thin snags average 26 cm. This model's choice; the thick ones' power
/// follows from it and [`MEAN_DBH_M`].
const THIN_POWER: f64 = 1.5;

/// The share of snags in decay classes 1 to 5 in young stands, by mass:
/// the snags' part of Spies et al.'s fig. 2 (all provinces), read off the
/// figure as 4, 8.5, 8, 12 and 4.5 Mg a hectare.
pub const YOUNG_DECAY: [f64; 5] = [0.11, 0.23, 0.22, 0.32, 0.12];
/// ...in mature stands (3.5, 7, 5.5, 5 and 1.5 Mg a hectare)...
pub const MATURE_DECAY: [f64; 5] = [0.16, 0.31, 0.24, 0.22, 0.07];
/// ...and in old growth (8.5, 24.5, 16, 7 and 3.5 Mg a hectare).
pub const OLD_DECAY: [f64; 5] = [0.14, 0.41, 0.27, 0.12, 0.06];

/// The share of its bark a snag of each decay class keeps (Domke et al.
/// 2011, table 2, classes 1-4); class 5 keeps "less than 20 percent"
/// (table 1), here 10%.
pub const BARK_KEPT: [f64; 5] = [0.92, 0.66, 0.39, 0.21, 0.1];

/// The shortest snag drawn, metres: breast height, below which a snag's
/// diameter is not measured (Spies et al. measured those over 1.4 m at
/// breast height). A shorter one is a stump.
pub const SHORTEST_M: f64 = 1.4;

/// The heights Spies et al.'s table 4 parts snags by, metres: over 5 m and
/// over 15 m, in stands whose tallest trees stand
/// [`deadwood::REFERENCE_CANOPY_M`] tall.
pub const HEIGHT_BOUNDS_M: [f64; 2] = [5.0, 15.0];

/// The share of class 2 snags whose tops are broken: "top may be broken"
/// (Domke et al. 2011, table 1), here half. Classes 3 to 5 have lost
/// theirs, class 1 keeps it.
pub const CLASS_2_BROKEN: f64 = 0.5;

/// Bare wood weathered grey on a snag's surface, sRGB: wood left out of
/// doors greys where its sapwood stands (Wood Handbook, FPL-GTR-190,
/// chapter 16, "Weathering"), on classes 1 to 3; once the sapwood has
/// sloughed off, classes 4 and 5 show their rotted heartwood, dark
/// reddish brown to dark brown ([`deadwood::WOOD_SRGB`]). The value is this
/// model's choice.
pub const WEATHERED_SRGB: [u8; 3] = [138, 132, 122];

/// The share of a snag's foot under moss in each decay class, fading out
/// by [`MOSS_REACH_M`] up its stem, over bark and bare wood alike: none
/// while it is fresh, more the more it has rotted, as mosses climb from
/// the forest floor onto soft wood; a rotten snag's broken top is mossy too. Seen on the old-growth
/// snags at Wind River (photos of the US Forest Service, 2024, imagekit
/// batch commons-snags-2026-11-08); the amounts are this model's choice.
pub const SNAG_MOSS: [f64; 5] = [0.0, 0.0, 0.15, 0.35, 0.5];

/// How far up a snag's stem moss reaches from the floor in each decay
/// class, metres: higher the softer the wood, as on Wind River's rotten
/// snags, mossy to their tops at 3 to 5 m (the same photos; this model's
/// choice).
pub const MOSS_REACH_M: [f64; 5] = [0.0, 0.0, 2.0, 4.0, 6.0];

/// The snags a stand holds: how many a hectare thin and thick, how tall
/// and thick they are, and how far rotted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snags {
    /// Snags a hectare under and over [`THICK_M`] (times
    /// [`Self::scale`]).
    pub per_ha: [f64; 2],
    /// The share of thin and of thick snags over 5 m, and of thick ones
    /// over 15 m (both times [`Self::scale`]).
    pub over_5m: [f64; 2],
    pub over_15m: f64,
    /// The mean breast-height diameter of all of them, metres, before
    /// [`Self::scale`].
    pub mean_dbh_m: f64,
    /// The share in each decay class, 1 to 5.
    pub decay: [f64; 5],
    /// Every snag's diameter and the heights that part them are scaled by
    /// this: the community's canopy height over
    /// [`deadwood::REFERENCE_CANOPY_M`], as its logs are.
    pub scale: f64,
}

impl Snags {
    /// No snags: a planted stand, or ground with no trees.
    pub const NONE: Self = Self {
        per_ha: [0.0; 2],
        over_5m: OVER_5M[0],
        over_15m: OVER_15M[0],
        mean_dbh_m: MEAN_DBH_M[0],
        decay: [0.0, 0.0, 1.0, 0.0, 0.0],
        scale: 1.0,
    };

    /// The snags of a stand `age` years after its disturbance, whose
    /// pioneers commonly live `lifespan` years and whose tallest trees
    /// stand `canopy_m` tall: the plots' young, mature and old-growth
    /// values at [`deadwood::AGE_SHARES`] of the lifespan and straight
    /// between, as its logs ([`deadwood::DownWood::at`]): the young stands'
    /// before them with fresher wood towards the disturbance, old growth's
    /// after, none in a planted stand, and on cleared ground only its own
    /// trees', building up from none to the mature stands' at their age.
    #[must_use]
    pub fn at(age: f64, lifespan: f64, canopy_m: f64, origin: Origin) -> Self {
        if origin == Origin::Planted || !(age.is_finite() && lifespan > 0.0) {
            return Self::NONE;
        }
        let share = (age / lifespan).max(0.0);
        let shares = deadwood::AGE_SHARES;
        let counts = [YOUNG_SNAGS, MATURE_SNAGS, OLD_SNAGS];
        let decays = [YOUNG_DECAY, MATURE_DECAY, OLD_DECAY];
        let blend2 =
            |a: [f64; 2], b: [f64; 2], t: f64| std::array::from_fn(|i| math::lerp(a[i], b[i], t));
        let blend5 =
            |a: [f64; 5], b: [f64; 5], t: f64| std::array::from_fn(|i| math::lerp(a[i], b[i], t));
        // The stage the stand stands at (0 young, 1 mature, 2 old), and
        // how far towards the next.
        let (stage, t) = if share <= shares[0] {
            (0, 0.0)
        } else if share >= shares[2] {
            (2, 0.0)
        } else {
            let k = usize::from(share >= shares[1]);
            (k, (share - shares[k]) / (shares[k + 1] - shares[k]))
        };
        let next = (stage + 1).min(2);
        let mut snags = Self {
            per_ha: blend2(counts[stage], counts[next], t),
            over_5m: blend2(OVER_5M[stage], OVER_5M[next], t),
            over_15m: math::lerp(OVER_15M[stage], OVER_15M[next], t),
            mean_dbh_m: math::lerp(MEAN_DBH_M[stage], MEAN_DBH_M[next], t),
            decay: blend5(decays[stage], decays[next], t),
            scale: if canopy_m > 0.0 {
                (canopy_m / deadwood::REFERENCE_CANOPY_M).clamp(0.25, 1.2)
            } else {
                1.0
            },
        };
        if origin == Origin::Cleared && share < shares[1] {
            let t = share / shares[1];
            snags.per_ha = MATURE_SNAGS.map(|count| count * t);
            snags.over_5m = OVER_5M[1];
            snags.over_15m = OVER_15M[1];
            snags.mean_dbh_m = MEAN_DBH_M[1];
            snags.decay = blend5(deadwood::FRESH_DECAY, MATURE_DECAY, t);
        } else if share < shares[0] {
            snags.decay = blend5(deadwood::FRESH_DECAY, YOUNG_DECAY, share / shares[0]);
        }
        snags
    }

    /// These snags, `share` as many (0 to 1).
    #[must_use]
    pub fn times(self, share: f64) -> Self {
        let share = share.clamp(0.0, 1.0);
        Self {
            per_ha: self.per_ha.map(|count| count * share),
            ..self
        }
    }

    /// Every snag a hectare.
    #[must_use]
    pub fn per_ha_total(&self) -> f64 {
        self.per_ha.iter().sum()
    }

    /// The power a thick snag's diameter is drawn to (see [`THIN_POWER`]),
    /// so all of them average [`Self::mean_dbh_m`].
    fn thick_power(&self) -> f64 {
        let [low, high] = DBH_RANGE_M;
        let thin_mean = low + (THICK_M - low) / (1.0 + THIN_POWER);
        let [thin, thick] = self.per_ha;
        let thick_mean = if thick > 0.0 {
            (self.mean_dbh_m * (thin + thick) - thin_mean * thin) / thick
        } else {
            0.7
        };
        let excess = (thick_mean - THICK_M).clamp(0.05, high - THICK_M);
        ((high - THICK_M) / excess - 1.0).clamp(0.3, 8.0)
    }
}

/// One snag's shape, in its own frame: standing on +y from its foot at the
/// origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub height_m: f64,
    /// Its breast-height diameter, metres.
    pub dbh_m: f64,
    /// Its width where its top broke off, metres; 0 where it keeps its
    /// pointed top.
    pub top_m: f64,
    /// Its decay class, 1 to 5.
    pub decay: u8,
    /// Seeds its irregular surface, bark patches and limbs.
    pub seed: u64,
}

impl Shape {
    fn class(&self) -> usize {
        usize::from(self.decay.clamp(1, 5) - 1)
    }

    /// Its width `y` metres above its foot: tapering from its diameter at
    /// [`deadwood::TAPER`] to where it broke, or to a point at its top,
    /// flaring a little at its foot. The flare (a quarter more, fading
    /// over its own diameter) is this model's choice.
    #[must_use]
    pub fn width_at(&self, y: f64) -> f64 {
        let dbh = self.dbh_m.max(0.02);
        let height = self.height_m.max(0.1);
        let y = y.clamp(0.0, height);
        let stem = if self.top_m > 0.0 {
            (dbh - deadwood::TAPER * (y - 1.37)).max(self.top_m)
        } else {
            dbh * ((height - y) / (height - 1.37).max(0.1)).max(0.0)
        };
        stem * (1.0 + 0.25 * math::exp(-y / dbh))
    }
}

/// A snag stood on a stand tile: where its foot is, and its shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stood {
    pub at: [f64; 2],
    pub shape: Shape,
}

impl Stood {
    /// Its stem as logs and plants keep clear of it.
    #[must_use]
    pub fn stem(&self) -> Stem {
        Stem {
            at: self.at,
            radius_m: 0.5 * self.shape.width_at(0.0),
        }
    }

    /// `point` moved off its foot to just past its side, on a tile of side
    /// `tile_m` that wraps: where a shrub or a herb that stood where the
    /// snag stands grows beside it. `None` where it is clear of the snag.
    #[must_use]
    pub fn moved_off(&self, point: [f64; 2], tile_m: f64) -> Option<[f64; 2]> {
        let reach = 0.5 * self.shape.width_at(0.0);
        for shift in deadwood::images(tile_m) {
            let p = [point[0] + shift[0], point[1] + shift[1]];
            let (dx, dy) = (p[0] - self.at[0], p[1] - self.at[1]);
            let d = math::sqrt(dx * dx + dy * dy);
            if d >= reach {
                continue;
            }
            let (ux, uy) = if d > 1e-6 {
                (dx / d, dy / d)
            } else {
                (1.0, 0.0)
            };
            let out = reach + 0.05;
            let moved = [
                self.at[0] + ux * out - shift[0],
                self.at[1] + uy * out - shift[1],
            ];
            return Some([moved[0].rem_euclid(tile_m), moved[1].rem_euclid(tile_m)]);
        }
        None
    }
}

/// How many places a snag is tried at before it is left out.
const TRIES: u64 = 40;

/// The height of a living tree as thick as `dbh`, from the stand's trees
/// (`live`, each its breast-height diameter and height): the tallest tree
/// no thicker than it, so a snag thicker than every tree stands as tall as
/// the tallest, and one thinner than every tree in proportion to the
/// thinnest. 0 where there are no trees.
fn live_height(live: &[(f64, f64)], dbh: f64) -> f64 {
    let tallest_thinner = live
        .iter()
        .filter(|(thick, _)| *thick <= dbh)
        .map(|(_, height)| *height)
        .fold(0.0_f64, f64::max);
    if tallest_thinner > 0.0 {
        return tallest_thinner;
    }
    live.iter()
        .filter(|(thick, _)| *thick > 0.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(0.0, |(thick, height)| height * (dbh / thick).min(1.0))
}

/// Stand `snags` on a stand tile of side `tile_m` that wraps, among the
/// living trees (`live`, each its breast-height diameter and height) and
/// clear of their `stems` and of each other: each class's snags a hectare
/// (rounded by a draw), each one's diameter drawn within its class (more
/// of the thin), its decay class by [`Snags::decay`], and its height by
/// the plots' shares over 5 and 15 m, up to a living tree's as thick as it
/// (a class 1 snag, just dead, stands its full height). The thickest are
/// stood first; one that finds no room in [`TRIES`] places is left out.
#[must_use]
pub fn stand(
    snags: &Snags,
    stems: &[Stem],
    live: &[(f64, f64)],
    tile_m: f64,
    seed: u64,
) -> Vec<Stood> {
    if tile_m.is_nan() || tile_m <= 0.0 || snags.per_ha_total() <= 0.0 || live.is_empty() {
        return Vec::new();
    }
    let area_ha = tile_m * tile_m / 10_000.0;
    let draw = |salt: &[u64]| {
        let mut words = vec![seed];
        words.extend_from_slice(salt);
        rng::unit(rng::hash_words(&words))
    };
    let [low, high] = DBH_RANGE_M;
    let powers = [THIN_POWER, snags.thick_power()];
    let spans = [[low, THICK_M], [THICK_M, high]];
    let [five, fifteen] = HEIGHT_BOUNDS_M.map(|bound| bound * snags.scale);
    let shortest = SHORTEST_M.min(five);
    let mut shapes: Vec<Shape> = Vec::new();
    for (class, per_ha) in snags.per_ha.iter().enumerate() {
        let expected = per_ha * area_ha;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (expected.floor()
            + f64::from(u8::from(draw(&[1, class as u64]) < expected.fract())))
            as u64;
        let [from, to] = spans[class];
        for n in 0..count {
            let key = |k: u64| [k, class as u64, n];
            let dbh = (from + (to - from) * math::pow(draw(&key(2)), powers[class])) * snags.scale;
            let mut pick = draw(&key(3));
            let mut decay = 5_u8;
            for (k, share) in snags.decay.iter().enumerate() {
                if pick < *share {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        decay = k as u8 + 1;
                    }
                    break;
                }
                pick -= share;
            }
            let full = live_height(live, dbh).max(shortest);
            let height = if decay == 1 {
                full
            } else {
                // The plots' shares hold for all snags, class 1's too,
                // which all stand their full height: the others make up
                // the rest.
                let fresh = snags.decay[0].min(0.9);
                let rest = |share: f64| ((share - fresh) / (1.0 - fresh)).clamp(0.0, 1.0);
                let u = draw(&key(4));
                let (lower, upper) = if class == 1 && u < rest(snags.over_15m) {
                    (fifteen, full)
                } else if u < rest(snags.over_5m[class]) {
                    (five, if class == 1 { fifteen } else { full })
                } else {
                    (shortest, five)
                };
                let upper = upper.min(full);
                math::lerp(lower.min(upper), upper, draw(&key(5)))
            };
            let broken = decay >= 3 || (decay == 2 && draw(&key(6)) < CLASS_2_BROKEN);
            let top = if broken {
                (dbh - deadwood::TAPER * (height - 1.37)).max(0.3 * dbh)
            } else {
                0.0
            };
            shapes.push(Shape {
                height_m: height,
                dbh_m: dbh,
                top_m: top,
                decay,
                seed: rng::hash_words(&[seed, 7, class as u64, n]),
            });
        }
    }
    shapes.sort_by(|a, b| b.dbh_m.total_cmp(&a.dbh_m));
    let mut stood: Vec<Stood> = Vec::new();
    for (n, shape) in shapes.into_iter().enumerate() {
        for attempt in 0..TRIES {
            let at = [
                draw(&[8, n as u64, attempt]) * tile_m,
                draw(&[9, n as u64, attempt]) * tile_m,
            ];
            let snag = Stood { at, shape };
            let own = snag.stem();
            let apart = |other: &Stem| {
                deadwood::images(tile_m).all(|shift| {
                    let (dx, dy) = (
                        other.at[0] + shift[0] - at[0],
                        other.at[1] + shift[1] - at[1],
                    );
                    math::sqrt(dx * dx + dy * dy) >= own.radius_m + other.radius_m + 0.1
                })
            };
            if stems.iter().all(apart) && stood.iter().all(|other| apart(&other.stem())) {
                stood.push(snag);
                break;
            }
        }
    }
    stood
}

/// A snag drawn at level of detail `level` (0 nearest to 3 farthest), in
/// its own frame ([`Shape`]), bark coloured as `appearance`'s where it
/// keeps bark (alpha minus its radius, as a plant's bark is drawn, so a
/// renderer lays its bark pattern on it), bare wood as plain colours. Its
/// foot reaches a little into the ground, so it stands on a slope; its
/// surface is irregular, more so the more it has rotted; a broken top is
/// splintered while its wood is hard and crumbled once it is soft; it
/// keeps limbs, stubs or none by its class.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn draw(shape: &Shape, appearance: Option<&Appearance>, level: usize) -> Mesh {
    let level = level.min(deadwood::SIDES.len() - 1);
    let class = shape.class();
    let sides = deadwood::SIDES[level];
    let height = shape.height_m.max(0.2);
    let foot = -0.3_f64.min(0.2 * height);
    let pointed = shape.top_m <= 0.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings = (((height - foot) / deadwood::RING_STEP_M[level]).ceil() as usize)
        .max(if level < 3 { 3 } else { 2 })
        + 1;
    let lumps = [0.02, 0.03, 0.05, 0.08, 0.1][class];
    // Rotting wood checks into fissures along the stem.
    let fissures = [0.0, 0.0, 0.05, 0.1, 0.14][class];
    let moss = deadwood::moss_colour(appearance);
    let mossy = SNAG_MOSS[class];
    let bark_colour = appearance.map_or([0.25, 0.17, 0.11], |a| a.bark);
    let ridges = appearance.and_then(|a| a.ridges.as_ref());
    let weathered = deadwood::linear(WEATHERED_SRGB);
    let wood = match class {
        0..=1 => weathered,
        2 => deadwood::mix(weathered, deadwood::linear(deadwood::WOOD_SRGB[0]), 0.5),
        3 => deadwood::linear(deadwood::WOOD_SRGB[1]),
        _ => deadwood::linear(deadwood::WOOD_SRGB[2]),
    };
    let seed = shape.seed;
    let kept = BARK_KEPT[class];
    // Bark sloughs from the top down: a little more is kept low on the
    // stem than high.
    let barked = |angle: f64, y: f64| {
        let keep = (kept * (1.25 - 0.5 * y / height)).clamp(0.0, 1.0);
        keep >= 1.0 || deadwood::wave(seed, 11, angle, y, 0.6) < keep
    };
    // A snag leans a little.
    let lean = Vec3::new(
        (rng::unit(rng::hash_words(&[seed, 51])) - 0.5) * 0.02,
        0.0,
        (rng::unit(rng::hash_words(&[seed, 52])) - 0.5) * 0.02,
    );
    let mut mesh = Mesh::default();
    let mut ring_starts = Vec::with_capacity(rings);
    let top_y = if pointed { height * 0.97 } else { height };
    for ring in 0..rings {
        #[allow(clippy::cast_precision_loss)]
        let t = ring as f64 / (rings - 1).max(1) as f64;
        let y = math::lerp(foot, top_y, t);
        let radius = 0.5 * shape.width_at(y.max(0.0));
        let centre = lean * y;
        ring_starts.push(u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX));
        for side in 0..=sides {
            let angle = 2.0 * math::PI * f64::from(side) / f64::from(sides);
            let bark = barked(angle, y);
            let lump = 1.0 + lumps * (2.0 * deadwood::wave(seed, 3, angle, y, 0.8 + radius) - 1.0)
                - fissures * math::pow(deadwood::wave(seed, 13, 7.0 * angle, y, 0.4), 4.0);
            let mut furrow = 0.0;
            if let (true, Some(ridges)) = (bark, ridges) {
                let count = (2.0 * math::PI * radius / ridges.spacing)
                    .round()
                    .min(f64::from(sides / 3));
                if count >= 3.0 {
                    let twisted = angle + math::radians(ridges.twist) * y;
                    furrow = math::pow(0.5 + 0.5 * math::cos(count * twisted), 3.0);
                }
            }
            let depth = ridges.map_or(0.0, |ridges| ridges.depth) * furrow;
            let out = radius * lump * (1.0 - depth);
            let (cos, sin) = (math::cos(angle), math::sin(angle));
            let position = centre + Vec3::new(cos * out, y, sin * out);
            let normal = Vec3::new(cos, 0.05, sin).normalize_or(Vec3::X);
            // Moss climbs from the floor in patches, over bark and wood.
            let low = (1.0 - y.max(0.0) / MOSS_REACH_M[class].max(1.0e-6)).clamp(0.0, 1.0);
            let covered = mossy * low * 1.6;
            let patch = deadwood::wave(seed, 7, 2.0 * angle, y, 0.5 + radius);
            let weight = if covered > 0.0 {
                math::smoothstep(1.0 - covered - 0.08, 1.0 - covered + 0.08, patch)
            } else {
                0.0
            };
            let colour = if bark && weight < 0.5 {
                let contrast = ridges.map_or(0.0, |ridges| f64::from(ridges.contrast));
                #[allow(clippy::cast_possible_truncation)]
                let dark = (1.0 - contrast * furrow) as f32;
                let tinted = bark_colour.map(|channel| channel * dark);
                #[allow(clippy::cast_possible_truncation)]
                let alpha = -(out.max(1.0e-4) as f32);
                [tinted[0], tinted[1], tinted[2], alpha]
            } else {
                let grain = 0.85 + 0.3 * deadwood::wave(seed, 5, 4.0 * angle, y, 0.15);
                #[allow(clippy::cast_possible_truncation)]
                let grained = wood.map(|channel| channel * grain as f32);
                let surface = deadwood::mix(grained, moss, weight);
                [surface[0], surface[1], surface[2], 1.0]
            };
            #[allow(clippy::cast_possible_truncation)]
            let uv = [(f64::from(side) / f64::from(sides)) as f32, y as f32];
            mesh.push(Vertex {
                position,
                normal,
                uv,
                color: colour,
                born: 0.0,
                shed: None,
                level: 0,
                segment: crate::mesh::NO_SEGMENT,
            });
        }
    }
    for ring in 0..rings - 1 {
        let (a, b) = (ring_starts[ring], ring_starts[ring + 1]);
        for side in 0..sides {
            let (p, q) = (a + side, b + side);
            mesh.indices.extend([p, p + 1, q, p + 1, q + 1, q]);
        }
    }
    // The top: a point where it keeps it, else broken off, splintered
    // while the wood is hard, crumbled once it is soft.
    let last = ring_starts[rings - 1];
    let radius = 0.5 * shape.width_at(top_y);
    if pointed {
        let tip = lean * height + Vec3::new(0.0, height, 0.0);
        let first = mesh.push(Vertex::plain(tip, Vec3::Y, bark_colour));
        for side in 0..sides {
            mesh.indices.extend([last + side, first, last + side + 1]);
        }
    } else {
        // Hard wood breaks into tall splinters, soft wood crumbles low: the
        // wall runs on up past the break to a jagged rim of bare wood, and
        // the break's end grain closes it a little below the rim's mean.
        let rough = [3.0, 3.0, 2.4, 1.0, 0.6][class];
        let splinter = |angle: f64| {
            radius * rough * math::pow(deadwood::wave(seed, 30, 5.0 * angle, 0.0, 1.0), 2.5)
        };
        let end_grain = deadwood::mix(wood.map(|c| c * 0.8), moss, (1.5 * mossy).min(0.8));
        let centre = lean * height;
        let mut rim = Vec::new();
        let mut mean = 0.0;
        for side in 0..=sides {
            let angle = 2.0 * math::PI * f64::from(side) / f64::from(sides);
            let at = mesh.positions[(last + side) as usize];
            let foot = Vec3::new(f64::from(at[0]), f64::from(at[1]), f64::from(at[2]));
            let up = splinter(angle);
            // A splinter narrows as it rises.
            let inward = 1.0 - 0.35 * (up / (radius * rough).max(1.0e-6));
            let out = foot - centre - Vec3::new(0.0, height, 0.0);
            let point = centre + Vec3::new(0.0, height + up, 0.0) + out * inward;
            let normal = Vec3::new(math::cos(angle), 0.1, math::sin(angle)).normalize_or(Vec3::X);
            let grain = 0.85 + 0.3 * deadwood::wave(seed, 5, 4.0 * angle, height, 0.15);
            #[allow(clippy::cast_possible_truncation)]
            let bare = wood.map(|channel| channel * grain as f32);
            let below = mesh.push(Vertex::plain(foot, normal, bare));
            mesh.push(Vertex::plain(point, normal, bare));
            if side > 0 {
                let (p, q) = (below - 2, below);
                mesh.indices.extend([p, q, p + 1, q, q + 1, p + 1]);
            }
            rim.push(point);
            mean += up / (f64::from(sides) + 1.0);
        }
        let tip = centre + Vec3::new(0.0, height + 0.4 * mean, 0.0);
        let first = mesh.push(Vertex::plain(tip, Vec3::Y, end_grain));
        for (side, point) in rim.into_iter().enumerate() {
            let normal =
                (Vec3::Y * 0.8 + (point - tip) * 0.2 / radius.max(0.01)).normalize_or(Vec3::Y);
            let idx = mesh.push(Vertex::plain(point, normal, end_grain));
            if side > 0 {
                mesh.indices.extend([first, idx, idx - 1]);
            }
        }
    }
    if level <= 1 {
        limbs(&mut mesh, shape, lean, (bark_colour, wood), level);
    }
    mesh
}

/// The limbs a snag keeps by its class: class 1 all its branches, long and
/// drooping, through the upper half of the tree it was; class 2 a few,
/// shorter, with no fine branches; class 3 short stubs; class 4 a few
/// stubs or none; class 5 none (Domke et al. 2011, table 1). How long and
/// how far apart they are is this model's reading of the words: a living
/// crown's branches reach about a tenth of the tree's height.
fn limbs(
    mesh: &mut Mesh,
    shape: &Shape,
    lean: Vec3,
    (bark, wood): ([f32; 3], [f32; 3]),
    level: usize,
) {
    let class = shape.class();
    if class >= 4 {
        return;
    }
    let height = shape.height_m;
    // Where the limbs stand, as shares of its height, how far apart
    // (metres), how far they reach (metres) and whether they keep bark.
    let (from, spacing, reach, barked) = match class {
        0 => (0.45, 0.15, (0.06, 0.12), true),
        1 => (0.45, 0.8, (0.03, 0.06), true),
        2 => (0.3, 1.0, (0.008, 0.02), false),
        _ => (0.3, 3.0, (0.005, 0.012), false),
    };
    let spacing = spacing * if level == 0 { 1.0 } else { 2.0 };
    let span = height * (1.0 - from);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (span / spacing).floor() as u64;
    for n in 0..count {
        let pick = |k: u64| rng::unit(rng::hash_words(&[shape.seed, 40, n, k]));
        if class == 3 && pick(5) < 0.5 {
            continue;
        }
        let y = height * from + span * pick(0);
        let radius = 0.5 * shape.width_at(y);
        let angle = pick(1) * 2.0 * math::PI;
        let (c, s) = (math::cos(angle), math::sin(angle));
        // Dead branches droop; the lower, the more.
        let droop = -0.1 - 0.4 * pick(2) * (1.0 - (y / height));
        let out = Vec3::new(c, droop, s).normalize_or(Vec3::X);
        let stub_r = (radius * (0.1 + 0.1 * pick(3))).clamp(0.01, 0.12);
        // Branches shorten up the crown, as a crown narrows to its top.
        let up = ((y - height * from) / span.max(0.1)).clamp(0.0, 1.0);
        let length = height * math::lerp(reach.0, reach.1, pick(4)) * (1.2 - 0.8 * up);
        let base = lean * y + Vec3::new(c * radius * 0.9, y, s * radius * 0.9);
        deadwood::stub(
            mesh,
            base,
            out,
            stub_r,
            length.max(2.0 * stub_r),
            (if barked { bark } else { wood }, barked),
            level,
        );
        // Class 1 keeps its branchlets: a few short side branches off each
        // limb, nearer only.
        if class == 0 && level == 0 {
            let side = out.cross(Vec3::Y).normalize_or(Vec3::Z);
            for k in 0..3_u64 {
                let along = length * (0.3 + 0.2 * f64::from(u8::try_from(k).unwrap_or(0)));
                let turn = if k % 2 == 0 { 1.0 } else { -1.0 };
                let twig = (out + side * turn * 0.9 + Vec3::new(0.0, -0.2, 0.0)).normalize_or(out);
                deadwood::stub(
                    mesh,
                    base + out * along,
                    twig,
                    (0.4 * stub_r).max(0.005),
                    length * (0.25 + 0.2 * pick(10 + k)),
                    (bark, true),
                    level,
                );
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;

    fn live() -> Vec<(f64, f64)> {
        (0..60)
            .map(|k| {
                let dbh = 0.1 + 0.02 * f64::from(k);
                (dbh, 8.0 + 40.0 * dbh)
            })
            .collect()
    }

    #[test]
    fn snags_follow_the_plots_by_stand_age() {
        let young = Snags::at(60.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(young.per_ha, YOUNG_SNAGS);
        let mature = Snags::at(160.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(mature.per_ha, MATURE_SNAGS);
        let old = Snags::at(450.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(old.per_ha, OLD_SNAGS);
        assert_eq!(old.decay, OLD_DECAY);
        // Most in young stands, the fewest but thickest in old growth.
        assert!(young.per_ha_total() > mature.per_ha_total());
        assert!(mature.per_ha_total() > old.per_ha_total());
        assert!(old.per_ha[1] / old.per_ha_total() > 0.4);
        // None planted; a lot's own trees' only, fresh at first.
        assert_eq!(Snags::at(60.0, 500.0, 60.0, Origin::Planted), Snags::NONE);
        let lot = Snags::at(10.0, 500.0, 60.0, Origin::Cleared);
        assert!(lot.per_ha_total() < 0.2 * young.per_ha_total());
        assert!(lot.decay[0] + lot.decay[1] > young.decay[0] + young.decay[1]);
        for share in [YOUNG_DECAY, MATURE_DECAY, OLD_DECAY] {
            assert!((share.iter().sum::<f64>() - 1.0).abs() < 0.011);
        }
    }

    #[test]
    fn stood_snags_number_size_and_stand_as_the_plots_measured() {
        let tile = 200.0;
        let stems: Vec<Stem> = (0..1200)
            .map(|k| Stem {
                at: [f64::from(k % 40) * 5.0 + 1.0, f64::from(k / 40) * 6.6 + 2.0],
                radius_m: 0.25,
            })
            .collect();
        for (age, stage) in [(60.0, 0), (160.0, 1), (450.0, 2)] {
            let snags = Snags::at(age, 500.0, 60.0, Origin::Natural);
            let stood = stand(&snags, &stems, &live(), tile, 11);
            let n = stood.len() as f64;
            let per_ha = n / 4.0;
            assert!(
                (per_ha - snags.per_ha_total()).abs() < 0.12 * snags.per_ha_total(),
                "{per_ha} snags a hectare against {}",
                snags.per_ha_total()
            );
            let mean_dbh = stood.iter().map(|s| s.shape.dbh_m).sum::<f64>() / n;
            assert!(
                (mean_dbh - MEAN_DBH_M[stage]).abs() < 0.06,
                "mean dbh {mean_dbh} against {}",
                MEAN_DBH_M[stage]
            );
            let tall = stood.iter().filter(|s| s.shape.height_m > 5.0).count() as f64 / n;
            let measured = [108.0 / 171.0, 73.0 / 121.0, 30.0 / 60.0][stage];
            assert!(
                (tall - measured).abs() < 0.06,
                "{tall} over 5 m against {measured}"
            );
            for (k, share) in snags.decay.iter().enumerate() {
                let class = stood
                    .iter()
                    .filter(|s| usize::from(s.shape.decay) == k + 1)
                    .count() as f64
                    / n;
                assert!(
                    (class - share).abs() < 0.06,
                    "{class} in decay class {} against {share}",
                    k + 1
                );
            }
            for snag in &stood {
                assert!(snag.shape.height_m >= SHORTEST_M - 1e-9);
                assert!((1..=5).contains(&snag.shape.decay));
                match snag.shape.decay {
                    1 => assert!(snag.shape.top_m <= 0.0, "class 1 keeps its top"),
                    2 => {}
                    _ => assert!(snag.shape.top_m > 0.0, "class 3-5 have lost theirs"),
                }
                for stem in &stems {
                    let (dx, dy) = (stem.at[0] - snag.at[0], stem.at[1] - snag.at[1]);
                    assert!(dx.hypot(dy) > stem.radius_m, "a snag stands in a stem");
                }
            }
            assert_eq!(stood, stand(&snags, &stems, &live(), tile, 11));
        }
        // No trees, no snags.
        let snags = Snags::at(60.0, 500.0, 60.0, Origin::Natural);
        assert!(stand(&snags, &stems, &[], tile, 11).is_empty());
    }

    #[test]
    fn a_fresh_snag_keeps_its_bark_limbs_and_top_a_rotten_one_none() {
        let shape = |decay, top_m| Shape {
            height_m: 20.0,
            dbh_m: 0.5,
            top_m,
            decay,
            seed: 5,
        };
        let barked = |mesh: &Mesh| {
            mesh.colors.iter().filter(|c| c[3] < 0.0).count() as f64 / mesh.colors.len() as f64
        };
        let fresh = draw(&shape(1, 0.0), None, 0);
        let rotten = draw(&shape(5, 0.3), None, 0);
        assert!(barked(&fresh) > 0.8, "{}", barked(&fresh));
        assert!(barked(&rotten) < 0.2, "{}", barked(&rotten));
        // Limbs make a fresh snag's mesh far larger.
        assert!(
            fresh.triangle_count() > 2 * rotten.triangle_count(),
            "{} against {}",
            fresh.triangle_count(),
            rotten.triangle_count()
        );
        let top = |mesh: &Mesh| mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((f64::from(top(&fresh)) - 20.0).abs() < 0.2);
        assert!(f64::from(top(&rotten)) < 20.5);
        // Farther levels draw fewer triangles.
        let far = draw(&shape(3, 0.3), None, 3).triangle_count();
        let near = draw(&shape(3, 0.3), None, 0).triangle_count();
        assert!(far * 4 < near, "{far} against {near}");
    }
}
