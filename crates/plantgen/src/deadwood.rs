//! Down wood: the logs on a forest floor. How many a stand holds at each
//! age and how far they have rotted ([`DownWood`]), where they lie among
//! the trees ([`lay`]), and how each is drawn ([`draw`]).
//!
//! The numbers are from the plots. Spies, Franklin and Thomas (1988,
//! Ecology 69: 1689-1702) measured every log over 10 cm at its large end
//! on 196 natural Douglas-fir stands of western Oregon and Washington, 40
//! to 900 years old: young stands (40-80 years) hold the most logs, most of
//! them small and well rotted, many carried over from the stand a fire
//! killed; mature stands (80-195 years) the fewest; old growth fewer than
//! young stands but more large ones, and less rotted. Logs covered 9.8, 6.5
//! and 9.3% of the ground ([`LOG_COVER`]). Logs are pieces more often than
//! whole trees (Maser and Trappe 1984, PNW-GTR-164, fig. 2).
//!
//! How a log looks follows the five decay classes of Maser et al. (1979),
//! as Spies and Cline (1988, PNW-GTR-229, table 1.1) give them for fallen
//! Douglas-fir: class I has its bark and twigs and lies raised on its
//! branches; II has lost its twigs and sags a little; III keeps traces of
//! bark, its wood faded and still hard, sagging to the ground; IV is bark
//! free, light to reddish brown, soft in small blocks, round to oval and
//! wholly on the ground; V is oval, red-brown to dark brown, soft and
//! powdery, and sinks into the forest floor (its height about 0.44 of its
//! width, Spies et al. 1988). Mosses and liverworts settle on a log's upper
//! surface once its sapwood sloughs off (class III to IV), and a class V
//! log is buried in litter and humus that cover them again (Maser and
//! Trappe 1984, p. 26). Every rule here holds for any tree: a log's
//! species changes only its bark.

use serde::{Deserialize, Serialize};

use crate::development::Origin;
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng;
use crate::spec::Appearance;

/// The large-end diameter classes Spies et al. (1988, table 3) count logs
/// in, metres: 10-30 cm, 30-60 cm and over 60 cm (here to 1.5 m, about the
/// widest old-growth Douglas-fir logs).
pub const SIZE_CLASSES_M: [[f64; 2]; 3] = [[0.10, 0.30], [0.30, 0.60], [0.60, 1.50]];

/// Logs a hectare in each of [`SIZE_CLASSES_M`] in young stands, 40-80
/// years (Spies et al. 1988, table 3, all provinces).
pub const YOUNG_LOGS: [f64; 3] = [370.0, 177.0, 53.0];
/// ...in mature stands, 80-195 years...
pub const MATURE_LOGS: [f64; 3] = [308.0, 112.0, 28.0];
/// ...and in old growth, 200-900 years.
pub const OLD_LOGS: [f64; 3] = [193.0, 164.0, 59.0];

/// The share of the ground logs cover, seen from above, in young, mature
/// and old-growth stands: 979, 649 and 933 m² a hectare (Spies et al.
/// 1988, table 3). Fallen trees of every decay class, the oldest only
/// mossy mounds, covered more, 24% (11-35%) of a 450-year stand at the H.
/// J. Andrews Forest (Maser and Trappe 1984, p. 7).
pub const LOG_COVER: [f64; 3] = [0.0979, 0.0649, 0.0933];

/// The share of log mass in decay classes I to V in young stands, read
/// from Spies et al. (1988, fig. 2, all provinces, logs alone): most of it
/// in classes III to V, much of it carried over from the stand before.
pub const YOUNG_DECAY: [f64; 5] = [0.01, 0.04, 0.31, 0.45, 0.19];
/// ...in mature stands...
pub const MATURE_DECAY: [f64; 5] = [0.02, 0.24, 0.27, 0.29, 0.17];
/// ...and in old growth, where more is fresh (Spies and Cline 1988: classes
/// IV and V hold 57% of a young stand's down wood, 26% of old growth's).
pub const OLD_DECAY: [f64; 5] = [0.02, 0.20, 0.42, 0.24, 0.11];
/// Just after the disturbance, before the plots begin: the trees it killed
/// have fallen lately, so more of the wood is in the first classes. Class I
/// turns to II within a few years and each class after lasts longer (Maser
/// and Trappe 1984, p. 7). This model's choice.
pub const FRESH_DECAY: [f64; 5] = [0.15, 0.30, 0.35, 0.15, 0.05];

/// The stand ages the plots' classes stand for, as shares of the pioneers'
/// lifespan (Douglas-fir's 500 years, as [`crate::development`] keeps its
/// stages): young at 60 years, mature at 160 and old growth from 400,
/// where Spies et al.'s fig. 4 has log mass rising to its peak.
pub const AGE_SHARES: [f64; 3] = [0.12, 0.32, 0.8];

/// The canopy height of the stands the plots measured: Douglas-fir's, the
/// low end of its tallest (`height_max_m`, 60 m). A community whose
/// tallest trees stand lower lays smaller logs ([`DownWood::scale`]).
pub const REFERENCE_CANOPY_M: f64 = 60.0;

/// How fast a log narrows from its large end to its small, metres of
/// diameter a metre: Spies et al. (1988) estimated snags' tops with
/// 0.12 cm a decimetre.
pub const TAPER: f64 = 0.012;

/// The decay classes' height over width: round to class III, 0.8 in IV
/// (round to oval) and 0.44 in V (Spies et al. 1988, from 20 class V logs:
/// vertical diameter 0.439 of the horizontal).
pub const ROUNDNESS: [f64; 5] = [1.0, 1.0, 1.0, 0.8, 0.44];

/// How far a log of each decay class lies above (+) or into (-) the forest
/// floor, as a share of its diameter: class I raised on its branches, II
/// sagging slightly, III touching, IV settled into the duff and V sunk
/// (Maser et al. 1979's "portion of tree on ground"). The amounts are this
/// model's choice.
pub const LIFT: [f64; 5] = [0.35, 0.12, 0.0, -0.1, -0.2];

/// The share of a log's upper surface under moss in each decay class:
/// none while it keeps its bark whole, patches on class III, most of a
/// class IV log, less on a class V log, whose moss litter and humus bury
/// (Maser and Trappe 1984, p. 26: mosses and liverworts establish on the
/// surface once the sapwood sloughs off; Rambo and Muir 1998, The
/// Bryologist 101: 366-376, bryophyte cover rising with decay). The amounts
/// are this model's choice.
pub const MOSS_ON_LOGS: [f64; 5] = [0.0, 0.05, 0.35, 0.7, 0.5];

/// The colour of bare wood of decay classes III to V, sRGB: faded from its
/// original colour, light to reddish brown, red-brown to dark brown
/// (Spies and Cline 1988, table 1.1, after Maser et al. 1979). The values
/// are this model's reading of the words.
pub const WOOD_SRGB: [[u8; 3]; 3] = [[126, 110, 92], [124, 84, 56], [92, 58, 40]];

/// A broken end's fresh wood, sRGB: Douglas-fir's heartwood, orange-red
/// to reddish (Wood Handbook, FPL-GTR-190, 2010, p. 2-12).
pub const HEARTWOOD_SRGB: [u8; 3] = [176, 116, 78];

/// Forest-floor litter over a sinking log, sRGB: dark brown needles and
/// duff. This model's choice.
pub const LITTER_SRGB: [u8; 3] = [92, 70, 48];

/// The logs a stand holds: how many a hectare in each size class, the
/// ground they cover and how far rotted they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DownWood {
    /// Logs a hectare in each of [`SIZE_CLASSES_M`], before
    /// [`Self::scale`].
    pub per_ha: [f64; 3],
    /// The share of the ground they cover seen from above.
    pub cover: f64,
    /// The share of logs in each decay class, I to V.
    pub decay: [f64; 5],
    /// Every log's diameter and length are scaled by this: the community's
    /// canopy height over [`REFERENCE_CANOPY_M`], so a stand of alders lays
    /// smaller logs than a stand of Douglas-firs. Their cover goes as its
    /// square.
    pub scale: f64,
}

impl DownWood {
    /// No logs: a planted or kept stand, whose fallen wood is cleared.
    pub const NONE: Self = Self {
        per_ha: [0.0; 3],
        cover: 0.0,
        decay: [0.0, 0.0, 1.0, 0.0, 0.0],
        scale: 1.0,
    };

    /// The logs of a stand `age` years after its disturbance, whose
    /// pioneers commonly live `lifespan` years and whose tallest trees
    /// stand `canopy_m` tall: the plots' young, mature and old-growth
    /// values ([`YOUNG_LOGS`], [`LOG_COVER`], [`YOUNG_DECAY`] and on) at
    /// [`AGE_SHARES`] of the lifespan and straight between, the young
    /// stands' before them with fresher wood towards the disturbance
    /// ([`FRESH_DECAY`]), and old growth's after. A planted stand holds
    /// none ([`Self::NONE`]). A stand on cleared ground ([`Origin::Cleared`])
    /// holds none of the young stands' logs carried over from the stand
    /// before: its own trees' logs build up from none at its clearing to
    /// the mature stands' at their age, freshly fallen at first, and follow
    /// the plots after. A fill lays a cleared stand's logs only as its
    /// trees grow thick enough to make them ([`Self::times`]).
    #[must_use]
    pub fn at(age: f64, lifespan: f64, canopy_m: f64, origin: Origin) -> Self {
        if origin == Origin::Planted || !(age.is_finite() && lifespan > 0.0) {
            return Self::NONE;
        }
        let share = (age / lifespan).max(0.0);
        let logs = [YOUNG_LOGS, MATURE_LOGS, OLD_LOGS];
        let decays = [YOUNG_DECAY, MATURE_DECAY, OLD_DECAY];
        let blend3 =
            |a: [f64; 3], b: [f64; 3], t: f64| std::array::from_fn(|i| math::lerp(a[i], b[i], t));
        let blend5 =
            |a: [f64; 5], b: [f64; 5], t: f64| std::array::from_fn(|i| math::lerp(a[i], b[i], t));
        let (per_ha, cover, decay) = if origin == Origin::Cleared && share < AGE_SHARES[1] {
            let t = share / AGE_SHARES[1];
            (
                MATURE_LOGS.map(|logs| logs * t),
                LOG_COVER[1] * t,
                blend5(FRESH_DECAY, MATURE_DECAY, t),
            )
        } else if share <= AGE_SHARES[0] {
            (
                YOUNG_LOGS,
                LOG_COVER[0],
                blend5(FRESH_DECAY, YOUNG_DECAY, share / AGE_SHARES[0]),
            )
        } else if share >= AGE_SHARES[2] {
            (OLD_LOGS, LOG_COVER[2], OLD_DECAY)
        } else {
            let k = usize::from(share >= AGE_SHARES[1]);
            let t = (share - AGE_SHARES[k]) / (AGE_SHARES[k + 1] - AGE_SHARES[k]);
            (
                blend3(logs[k], logs[k + 1], t),
                math::lerp(LOG_COVER[k], LOG_COVER[k + 1], t),
                blend5(decays[k], decays[k + 1], t),
            )
        };
        let scale = if canopy_m > 0.0 {
            (canopy_m / REFERENCE_CANOPY_M).clamp(0.25, 1.2)
        } else {
            1.0
        };
        Self {
            per_ha,
            cover: cover * scale * scale,
            decay,
            scale,
        }
    }

    /// These logs, `share` as many (0 to 1), covering as much less.
    #[must_use]
    pub fn times(self, share: f64) -> Self {
        let share = share.clamp(0.0, 1.0);
        Self {
            per_ha: self.per_ha.map(|logs| logs * share),
            cover: self.cover * share,
            ..self
        }
    }

    /// Every log a hectare.
    #[must_use]
    pub fn per_ha_total(&self) -> f64 {
        self.per_ha.iter().sum()
    }
}

/// One log's shape, in its own frame: its axis along +x from its large end
/// at `-length_m / 2` to its small end at `+length_m / 2`, +y up, the
/// forest floor at y = 0 under its middle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub length_m: f64,
    /// Width at its large end and its small end, metres.
    pub butt_m: f64,
    pub top_m: f64,
    /// Its decay class, 1 to 5.
    pub decay: u8,
    /// Seeds its irregular surface, bark patches, moss and stubs.
    pub seed: u64,
}

impl Shape {
    /// The ground it covers seen from above, m².
    #[must_use]
    pub fn area_m2(&self) -> f64 {
        self.length_m * 0.5 * (self.butt_m + self.top_m)
    }

    fn class(&self) -> usize {
        usize::from(self.decay.clamp(1, 5) - 1)
    }
}

/// A tree's stem as logs keep clear of it: where it stands and its radius
/// at the foot, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stem {
    pub at: [f64; 2],
    pub radius_m: f64,
}

/// A log laid on a stand tile: the middle of its axis, the direction its
/// axis runs from its large end (radians anticlockwise from +x) and its
/// shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laid {
    pub at: [f64; 2],
    pub yaw: f64,
    pub shape: Shape,
}

impl Laid {
    /// Its two ends, large end first, in the tile's frame (not wrapped).
    #[must_use]
    pub fn ends(&self) -> [[f64; 2]; 2] {
        let half = 0.5 * self.shape.length_m;
        let (s, c) = (math::sin(self.yaw), math::cos(self.yaw));
        [
            [self.at[0] - c * half, self.at[1] - s * half],
            [self.at[0] + c * half, self.at[1] + s * half],
        ]
    }

    /// Whether `point` lies under it, on a tile of side `tile_m` that
    /// wraps.
    #[must_use]
    pub fn covers(&self, point: [f64; 2], tile_m: f64) -> bool {
        let [a, b] = self.ends();
        let reach = 0.5 * self.shape.butt_m;
        images(tile_m)
            .any(|shift| segment_point([a, b], [point[0] + shift[0], point[1] + shift[1]]) < reach)
    }
}

impl Laid {
    /// `point` moved off it, square to its axis, to just past its side,
    /// on a tile of side `tile_m` that wraps: where a shrub or a herb that
    /// stood where the log fell grows beside it. `None` where it does not
    /// lie under the log.
    #[must_use]
    pub fn moved_off(&self, point: [f64; 2], tile_m: f64) -> Option<[f64; 2]> {
        let [a, b] = self.ends();
        let reach = 0.5 * self.shape.butt_m;
        for shift in images(tile_m) {
            let p = [point[0] + shift[0], point[1] + shift[1]];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length2 = (dx * dx + dy * dy).max(1e-12);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2).clamp(0.0, 1.0);
            let near = [a[0] + t * dx, a[1] + t * dy];
            let (ox, oy) = (p[0] - near[0], p[1] - near[1]);
            let d = math::sqrt(ox * ox + oy * oy);
            if d >= reach {
                continue;
            }
            let (ux, uy) = if d > 1e-6 {
                (ox / d, oy / d)
            } else {
                let l = math::sqrt(length2);
                (-dy / l, dx / l)
            };
            let out = reach + 0.05;
            let moved = [near[0] + ux * out - shift[0], near[1] + uy * out - shift[1]];
            return Some([moved[0].rem_euclid(tile_m), moved[1].rem_euclid(tile_m)]);
        }
        None
    }
}

/// How many times a log is tried at a new place before it is left out.
const TRIES: u64 = 40;

/// Lay `wood`'s logs on a stand tile of side `tile_m` that wraps, clear of
/// `stems` and of each other: each class's logs a hectare (rounded by a
/// draw), each log's large-end width drawn within its class (more of the
/// small), its decay class drawn by [`DownWood::decay`], its length drawn
/// and scaled with every other's so together they cover
/// [`DownWood::cover`] of the tile, its small end narrower by [`TAPER`].
/// The largest are laid first; a log that finds no room in [`TRIES`]
/// places is left out, so a crowded stand holds less than the plots'.
#[must_use]
pub fn lay(wood: &DownWood, stems: &[Stem], tile_m: f64, seed: u64) -> Vec<Laid> {
    if !(tile_m > 0.0) || wood.per_ha_total() <= 0.0 {
        return Vec::new();
    }
    let area_ha = tile_m * tile_m / 10_000.0;
    let draw = |salt: &[u64]| {
        let mut words = vec![seed];
        words.extend_from_slice(salt);
        rng::unit(rng::hash_words(&words))
    };
    // Each log's class, width and decay, and its share of the length.
    let mut logs: Vec<(f64, f64, u8, u64)> = Vec::new();
    for (class, per_ha) in wood.per_ha.iter().enumerate() {
        let expected = per_ha * area_ha;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (expected.floor()
            + f64::from(u8::from(draw(&[1, class as u64]) < expected.fract())))
            as u64;
        let [low, high] = SIZE_CLASSES_M[class];
        // Smaller logs are commoner within a class, and the widest few.
        let skew = [1.6, 1.3, 2.0][class];
        for n in 0..count {
            let key = [2, class as u64, n];
            let width = (low + (high - low) * math::pow(draw(&key), skew)) * wood.scale;
            let mut pick = draw(&[3, class as u64, n]);
            let mut decay = 5_u8;
            for (k, share) in wood.decay.iter().enumerate() {
                if pick < *share {
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        decay = k as u8 + 1;
                    }
                    break;
                }
                pick -= share;
            }
            let length_share = 0.3 + 1.4 * draw(&[4, class as u64, n]);
            logs.push((
                width,
                length_share,
                decay,
                rng::hash_words(&[seed, 5, class as u64, n]),
            ));
        }
    }
    // Lengths: shares scaled together to the cover, each within a metre
    // and the longest a stand's trees give.
    let longest = (0.6 * REFERENCE_CANOPY_M * wood.scale).min(0.45 * tile_m);
    let target = wood.cover * tile_m * tile_m;
    let shapes = |k: f64| -> Vec<Shape> {
        logs.iter()
            .map(|(width, share, decay, seed)| {
                let length = (k * share).clamp(1.0_f64.min(longest), longest);
                let top = (width - TAPER * length)
                    .max(0.45 * width)
                    .max(0.06_f64.min(*width));
                Shape {
                    length_m: length,
                    butt_m: *width,
                    top_m: top,
                    decay: *decay,
                    seed: *seed,
                }
            })
            .collect()
    };
    let mut k = 1.0;
    for _ in 0..6 {
        let area: f64 = shapes(k).iter().map(Shape::area_m2).sum();
        if area <= 0.0 {
            break;
        }
        k *= target / area;
    }
    let mut shapes = shapes(k);
    shapes.sort_by(|a, b| b.area_m2().total_cmp(&a.area_m2()));
    let mut laid: Vec<Laid> = Vec::new();
    for (n, shape) in shapes.into_iter().enumerate() {
        for attempt in 0..TRIES {
            let key = [6, n as u64, attempt];
            let at = [draw(&key) * tile_m, draw(&[7, n as u64, attempt]) * tile_m];
            let yaw = draw(&[8, n as u64, attempt]) * 2.0 * math::PI;
            let log = Laid { at, yaw, shape };
            let ends = log.ends();
            let reach = 0.5 * shape.butt_m;
            let clear_of_stems = stems.iter().all(|stem| {
                images(tile_m).all(|shift| {
                    segment_point(ends, [stem.at[0] + shift[0], stem.at[1] + shift[1]])
                        >= reach + stem.radius_m + 0.05
                })
            });
            if !clear_of_stems {
                continue;
            }
            let clear_of_logs = laid.iter().all(|other| {
                let theirs = other.ends();
                images(tile_m).all(|shift| {
                    let moved = theirs.map(|end| [end[0] + shift[0], end[1] + shift[1]]);
                    segment_segment(ends, moved) >= reach + 0.5 * other.shape.butt_m + 0.05
                })
            });
            if clear_of_logs {
                laid.push(log);
                break;
            }
        }
    }
    laid
}

/// The nine shifts of a tile that wraps: itself and its eight neighbours.
pub(crate) fn images(tile_m: f64) -> impl Iterator<Item = [f64; 2]> {
    (0..9).map(move |k| {
        #[allow(clippy::cast_precision_loss)]
        let shift = |d: i32| f64::from(d - 1) * tile_m;
        [shift(k % 3), shift(k / 3)]
    })
}

/// The distance from `point` to the segment `ends`.
fn segment_point(ends: [[f64; 2]; 2], point: [f64; 2]) -> f64 {
    let [a, b] = ends;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 {
        (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + t * dx - point[0], a[1] + t * dy - point[1]);
    math::sqrt(x * x + y * y)
}

/// The least distance between two segments: 0 where they cross.
fn segment_segment(p: [[f64; 2]; 2], q: [[f64; 2]; 2]) -> f64 {
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let (d1, d2) = (cross(q[0], q[1], p[0]), cross(q[0], q[1], p[1]));
    let (d3, d4) = (cross(p[0], p[1], q[0]), cross(p[0], p[1], q[1]));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    segment_point(q, p[0])
        .min(segment_point(q, p[1]))
        .min(segment_point(p, q[0]))
        .min(segment_point(p, q[1]))
}

/// Sides round a log, and the most metres between its rings, at each
/// level of detail (0 nearest to 3 farthest).
pub(crate) const SIDES: [u32; 4] = [16, 10, 6, 4];
pub(crate) const RING_STEP_M: [f64; 4] = [0.35, 0.9, 2.5, 1.0e9];

/// A value from 0 to 1 that varies smoothly round the log (`angle`) and
/// along it (`along`, metres) at `scale` metres, seeded by `seed`: two
/// waves round and along, wrapped so it joins up round the log.
pub(crate) fn wave(seed: u64, salt: u64, angle: f64, along: f64, scale: f64) -> f64 {
    let phase = |k: u64| rng::unit(rng::hash_words(&[seed, salt, k])) * 2.0 * math::PI;
    let a = math::sin(2.0 * angle + along / scale + phase(0));
    let b = math::sin(3.0 * angle - 1.7 * along / scale + phase(1));
    let c = math::sin(angle + 0.6 * along / scale + phase(2));
    (0.5 + 0.25 * a + 0.15 * b + 0.1 * c).clamp(0.0, 1.0)
}

pub(crate) fn linear(srgb: [u8; 3]) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    srgb.map(|byte| crate::texture::srgb_to_linear(byte) as f32)
}

pub(crate) fn mix(a: [f32; 3], b: [f32; 3], t: f64) -> [f32; 3] {
    #[allow(clippy::cast_possible_truncation)]
    let t = t.clamp(0.0, 1.0) as f32;
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The moss that grows on a log: the species' own bark moss where its
/// appearance states one, else the default bark moss's colour.
pub(crate) fn moss_colour(appearance: Option<&Appearance>) -> [f32; 3] {
    appearance
        .and_then(|appearance| appearance.moss.as_ref())
        .map_or_else(|| crate::looks::Moss::default().colour, |moss| moss.colour)
}

/// A log drawn at level of detail `level` (0 nearest to 3 farthest), in
/// its own frame ([`Shape`]), bark coloured as `appearance`'s where it
/// keeps bark (alpha minus its radius, as a plant's bark is drawn, so a
/// renderer lays its bark pattern on it), bare wood, moss and litter as
/// plain colours. Its surface is irregular, more so the more it has
/// rotted; its ends are broken off, splintered while its wood is hard,
/// crumbled once it is soft; a class I or II log keeps stubs of its
/// branches, class I's long enough to hold it off the ground.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn draw(shape: &Shape, appearance: Option<&Appearance>, level: usize) -> Mesh {
    let level = level.min(SIDES.len() - 1);
    let class = shape.class();
    let sides = SIDES[level];
    let length = shape.length_m.max(0.1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rings =
        ((length / RING_STEP_M[level]).ceil() as usize).max(if level < 3 { 2 } else { 1 }) + 1;
    let roundness = ROUNDNESS[class];
    let butt = shape.butt_m.max(0.02);
    let top = shape.top_m.clamp(0.01, butt);
    let lumps = [0.025, 0.03, 0.06, 0.1, 0.12][class];
    let bark_colour = appearance.map_or([0.25, 0.17, 0.11], |a| a.bark);
    let ridges = appearance.and_then(|a| a.ridges.as_ref());
    let wood = match class {
        0..=2 => linear(WOOD_SRGB[0]),
        3 => linear(WOOD_SRGB[1]),
        _ => linear(WOOD_SRGB[2]),
    };
    let moss = moss_colour(appearance);
    let litter = linear(LITTER_SRGB);
    let heart = linear(HEARTWOOD_SRGB);
    let seed = shape.seed;
    // The axis' height above the floor at `t` (0 large end, 1 small): the
    // log rests on its underside, raised or sunk by its class, a raised
    // log sagging between its ends.
    let lift = LIFT[class] * butt;
    let width_at = |t: f64| math::lerp(butt, top, t);
    let axis_y = |t: f64| {
        let half_height = 0.5 * width_at(t) * roundness;
        let sag = if lift > 0.0 {
            0.5 * lift * 4.0 * t * (1.0 - t)
        } else {
            0.0
        };
        half_height + lift - sag
    };
    // No log lies quite straight: its axis bows sideways a little.
    let bow = (rng::unit(rng::hash_words(&[seed, 50])) - 0.5) * 0.03 * length;
    let axis_z = |t: f64| bow * math::sin(math::PI * t);
    let mut mesh = Mesh::default();
    let mut ring_starts = Vec::with_capacity(rings);
    // Where the class III log keeps its bark, and whether a point of the
    // surface is bark at all.
    let barked = |angle: f64, along: f64| match class {
        0 | 1 => true,
        2 => wave(seed, 11, angle, along, 0.6) > 0.68,
        _ => false,
    };
    for ring in 0..rings {
        #[allow(clippy::cast_precision_loss)]
        let t = ring as f64 / (rings - 1).max(1) as f64;
        let x = (t - 0.5) * length;
        let along = t * length;
        let radius = 0.5 * width_at(t);
        let centre_y = axis_y(t);
        ring_starts.push(u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX));
        for side in 0..=sides {
            let angle = 2.0 * math::PI * f64::from(side) / f64::from(sides);
            let bark = barked(angle, along);
            let lump = 1.0 + lumps * (2.0 * wave(seed, 3, angle, along, 0.8 + radius) - 1.0);
            let mut furrow = 0.0;
            if let (true, Some(ridges)) = (bark, ridges) {
                let count = (2.0 * math::PI * radius / ridges.spacing)
                    .round()
                    .min(f64::from(sides / 3));
                if count >= 3.0 {
                    let twisted = angle + math::radians(ridges.twist) * along;
                    furrow = math::pow(0.5 + 0.5 * math::cos(count * twisted), 3.0);
                }
            }
            let depth = ridges.map_or(0.0, |ridges| ridges.depth) * furrow;
            let r = radius * lump * (1.0 - depth);
            // The cross-section: round, or wider than tall as it rots.
            let (c, s) = (math::cos(angle), math::sin(angle));
            let local = Vec3::new(0.0, s * r * roundness, c * r);
            let position = Vec3::new(x, centre_y, axis_z(t)) + local;
            let normal = Vec3::new(0.0, s / roundness.max(0.1), c).normalize_or(Vec3::Y);
            // Colour: bark, else bare wood under moss on its upper face and
            // litter over a sinking log.
            let facing = ((normal.y + 0.1) / 0.8).clamp(0.0, 1.0);
            let mossy = MOSS_ON_LOGS[class];
            let patch = wave(seed, 7, angle, along, 0.5 + radius);
            let moss_weight = if mossy > 0.0 {
                facing * math::smoothstep(1.0 - mossy - 0.08, 1.0 - mossy + 0.08, patch)
            } else {
                0.0
            };
            let colour = if bark {
                let contrast = ridges.map_or(0.0, |ridges| f64::from(ridges.contrast));
                #[allow(clippy::cast_possible_truncation)]
                let dark = (1.0 - contrast * furrow) as f32;
                let bark = bark_colour.map(|channel| channel * dark);
                let tinted = mix(bark, moss, moss_weight);
                if moss_weight > 0.5 {
                    [tinted[0], tinted[1], tinted[2], 1.0]
                } else {
                    #[allow(clippy::cast_possible_truncation)]
                    let alpha = -(r.max(1.0e-4) as f32);
                    [tinted[0], tinted[1], tinted[2], alpha]
                }
            } else {
                let grain = 0.85 + 0.3 * wave(seed, 5, 4.0 * angle, along, 0.15);
                #[allow(clippy::cast_possible_truncation)]
                let bare = wood.map(|channel| channel * grain as f32);
                let mut colour = mix(bare, moss, moss_weight);
                if class == 4 {
                    let buried =
                        facing * math::smoothstep(0.55, 0.7, wave(seed, 9, angle, along, 0.7));
                    colour = mix(colour, litter, buried);
                }
                [colour[0], colour[1], colour[2], 1.0]
            };
            #[allow(clippy::cast_possible_truncation)]
            let uv = [(f64::from(side) / f64::from(sides)) as f32, along as f32];
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
            mesh.indices.extend([p, q, p + 1, p + 1, q, q + 1]);
        }
    }
    // The ends: broken off, a splintered face while the wood is hard, a
    // crumbled dome once it is soft.
    for (end, t) in [(0_u64, 0.0), (1, 1.0)] {
        let x = (t - 0.5) * length;
        let outward = if end == 0 { -1.0 } else { 1.0 };
        let radius = 0.5 * width_at(t);
        let centre_y = axis_y(t);
        let rough = if class <= 2 { 0.5 } else { 0.25 };
        let tip = Vec3::new(
            x + outward * radius * rough * (0.3 + 0.7 * wave(seed, 20 + end, 0.0, 0.0, 1.0)),
            centre_y,
            axis_z(t),
        );
        let face = if class <= 1 { heart } else { wood };
        let first = mesh.push(Vertex::plain(tip, Vec3::new(outward, 0.0, 0.0), face));
        let start = ring_starts[if end == 0 { 0 } else { rings - 1 }];
        for side in 0..=sides {
            let angle = 2.0 * math::PI * f64::from(side) / f64::from(sides);
            let at = mesh.positions[(start + side) as usize];
            let rim = Vec3::new(f64::from(at[0]), f64::from(at[1]), f64::from(at[2]));
            // Splinters stand out of the face at random round the rim.
            let splinter =
                outward * radius * rough * 0.6 * wave(seed, 30 + end, 3.0 * angle, 0.0, 1.0);
            let point = rim + Vec3::new(splinter, 0.0, 0.0);
            let normal = (Vec3::new(outward * 0.8, 0.0, 0.0)
                + (point - tip) * 0.2 / radius.max(0.01))
            .normalize_or(Vec3::new(outward, 0.0, 0.0));
            let idx = mesh.push(Vertex::plain(point, normal, face));
            if side > 0 {
                if end == 0 {
                    mesh.indices.extend([first, idx - 1, idx]);
                } else {
                    mesh.indices.extend([first, idx, idx - 1]);
                }
            }
        }
    }
    // Branch stubs on classes I and II, on the nearer levels: class I's
    // long and some reaching down to the floor, class II's short.
    if class <= 1 && level <= 1 {
        let spacing = if class == 0 { 0.8 } else { 1.6 };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let stubs = (length / spacing).floor() as u64;
        for n in 0..stubs {
            let pick = |k: u64| rng::unit(rng::hash_words(&[seed, 40, n, k]));
            let t = (0.1 + 0.85 * pick(0)).min(1.0);
            let angle = pick(1) * 2.0 * math::PI;
            let radius = 0.5 * width_at(t);
            let base = Vec3::new((t - 0.5) * length, axis_y(t), axis_z(t));
            let (c, s) = (math::cos(angle), math::sin(angle));
            let out = Vec3::new(0.35 * (pick(2) - 0.5), s, c).normalize_or(Vec3::Y);
            let stub_r = radius * (0.14 + 0.14 * pick(3));
            let reach = if class == 0 {
                // Long to the sides, broken short above, and one pointing
                // down reaching only to the floor.
                let long = radius * (2.5 + 4.5 * pick(4));
                if s < -0.3 {
                    long.min(axis_y(t) / -s)
                } else if s > 0.3 {
                    radius * (1.2 + 1.8 * pick(4))
                } else {
                    long
                }
            } else {
                radius * (1.0 + pick(4))
            };
            stub(
                &mut mesh,
                base,
                out,
                stub_r,
                reach,
                (bark_colour, true),
                level,
            );
        }
    }
    mesh
}

/// A tapering stub of branch from `base` along `direction`, `reach`
/// metres long, `radius` thick at its foot, coloured `colour`: barked
/// (its alpha the bark's, as a plant's bark is drawn) when the flag is
/// set, else bare wood.
pub(crate) fn stub(
    mesh: &mut Mesh,
    base: Vec3,
    direction: Vec3,
    radius: f64,
    reach: f64,
    (colour, barked): ([f32; 3], bool),
    level: usize,
) {
    let sides = if level == 0 { 6 } else { 4 };
    let side_a = if direction.x.abs() < 0.9 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let u = direction.cross(side_a).normalize_or(Vec3::Z);
    let v = direction.cross(u);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for (k, (along, r)) in [(0.0, radius), (reach, radius * 0.35)]
        .into_iter()
        .enumerate()
    {
        for side in 0..=sides {
            let angle = 2.0 * math::PI * f64::from(side) / f64::from(sides);
            let out = u * math::cos(angle) + v * math::sin(angle);
            #[allow(clippy::cast_possible_truncation)]
            let alpha = if barked { -(r.max(1.0e-4) as f32) } else { 1.0 };
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let uv = [
                (f64::from(side) / f64::from(sides)) as f32,
                (k as f64 * reach) as f32,
            ];
            mesh.push(Vertex {
                position: base + direction * along + out * r,
                normal: out,
                uv,
                color: [colour[0], colour[1], colour[2], alpha],
                born: 0.0,
                shed: None,
                level: 1,
                segment: crate::mesh::NO_SEGMENT,
            });
        }
    }
    let row = sides + 1;
    for side in 0..sides {
        let (p, q) = (first + side, first + row + side);
        mesh.indices.extend([p, p + 1, q, p + 1, q + 1, q]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_wood_follows_the_plots_by_stand_age() {
        let young = DownWood::at(60.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(young.per_ha, YOUNG_LOGS);
        assert!((young.cover - 0.0979).abs() < 1e-9);
        let old = DownWood::at(450.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(old.per_ha, OLD_LOGS);
        let mature = DownWood::at(160.0, 500.0, 60.0, Origin::Natural);
        assert_eq!(mature.per_ha, MATURE_LOGS);
        // Fewest in mature stands, most rotted in young ones.
        assert!(mature.cover < young.cover && mature.cover < old.cover);
        let rotted = |w: &DownWood| w.decay[3] + w.decay[4];
        assert!(rotted(&young) > rotted(&mature) && rotted(&mature) > rotted(&old));
        // Fresher wood just after the disturbance.
        let fresh = DownWood::at(5.0, 500.0, 60.0, Origin::Natural);
        assert!(fresh.decay[0] + fresh.decay[1] > young.decay[0] + young.decay[1]);
        // A kept yard holds none; a stand of short trees smaller logs.
        assert_eq!(
            DownWood::at(60.0, 500.0, 60.0, Origin::Planted),
            DownWood::NONE
        );
        let alders = DownWood::at(30.0, 100.0, 15.0, Origin::Natural);
        assert!((alders.scale - 0.25).abs() < 1e-9);
        assert!(alders.cover < 0.01);
    }

    #[test]
    fn cleared_ground_holds_only_the_logs_its_own_trees_drop() {
        // A lot has no forest's logs on it: none when it is cleared, its
        // own trees' building up to the mature stands', the plots' after.
        assert_eq!(
            DownWood::at(0.0, 500.0, 60.0, Origin::Cleared).per_ha_total(),
            0.0
        );
        let lot = DownWood::at(10.0, 500.0, 60.0, Origin::Cleared);
        let forest = DownWood::at(10.0, 500.0, 60.0, Origin::Natural);
        assert!(lot.per_ha_total() < 0.1 * forest.per_ha_total(), "{lot:?}");
        assert!(lot.cover < 0.1 * forest.cover);
        let young = DownWood::at(60.0, 500.0, 60.0, Origin::Cleared);
        assert!(young.per_ha_total() > lot.per_ha_total());
        assert!(
            young.per_ha_total() < DownWood::at(60.0, 500.0, 60.0, Origin::Natural).per_ha_total()
        );
        // Its own trees' wood, fresh at first.
        assert!(lot.decay[0] + lot.decay[1] > forest.decay[0] + forest.decay[1]);
        for age in [160.0, 300.0, 450.0] {
            assert_eq!(
                DownWood::at(age, 500.0, 60.0, Origin::Cleared),
                DownWood::at(age, 500.0, 60.0, Origin::Natural)
            );
        }
    }

    #[test]
    fn laid_logs_cover_what_the_plots_measured_and_keep_clear_of_stems() {
        let wood = DownWood::at(60.0, 500.0, 60.0, Origin::Natural);
        let tile = 60.0;
        let stems: Vec<Stem> = (0..40)
            .map(|k| Stem {
                at: [f64::from(k % 8) * 7.5 + 2.0, f64::from(k / 8) * 12.0 + 3.0],
                radius_m: 0.3,
            })
            .collect();
        let logs = lay(&wood, &stems, tile, 7);
        let per_ha = logs.len() as f64 / (tile * tile / 10_000.0);
        assert!(
            (per_ha - wood.per_ha_total()).abs() < 0.15 * wood.per_ha_total(),
            "{per_ha} logs/ha"
        );
        let cover: f64 = logs.iter().map(|log| log.shape.area_m2()).sum::<f64>() / (tile * tile);
        assert!(
            (cover - wood.cover).abs() < 0.25 * wood.cover,
            "cover {cover}"
        );
        for log in &logs {
            for stem in &stems {
                assert!(!log.covers(stem.at, tile), "a log lies through a stem");
            }
            assert!(log.shape.top_m <= log.shape.butt_m);
            assert!((1..=5).contains(&log.shape.decay));
        }
        // The same seed lays the same logs.
        assert_eq!(logs, lay(&wood, &stems, tile, 7));
    }

    #[test]
    fn a_rotten_log_is_oval_sunk_and_bare_a_sound_one_round_raised_and_barked() {
        let shape = |decay| Shape {
            length_m: 6.0,
            butt_m: 0.6,
            top_m: 0.55,
            decay,
            seed: 3,
        };
        for decay in 1..=5 {
            let mesh = draw(&shape(decay), None, 0);
            assert!(mesh.triangle_count() > 100);
            let (low, high) = mesh
                .positions
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    (lo.min(p[1]), hi.max(p[1]))
                });
            let barked = mesh.colors.iter().filter(|c| c[3] < 0.0).count();
            let class = usize::from(decay - 1);
            match decay {
                1 => assert!(low > 0.05 || barked > 0, "class I lies on its stubs"),
                5 => {
                    assert!(low < -0.02, "class V sinks: {low}");
                    assert!(f64::from(high) < 0.6 * 0.6, "class V is oval: {high}");
                }
                _ => {}
            }
            if class >= 3 {
                assert_eq!(barked, 0, "class {decay} has no bark");
            } else if class <= 1 {
                assert!(
                    barked > mesh.colors.len() / 2,
                    "class {decay} keeps its bark"
                );
            }
        }
        // Farther levels draw fewer triangles.
        let near = draw(&shape(3), None, 0).triangle_count();
        let far = draw(&shape(3), None, 3).triangle_count();
        assert!(far * 4 < near, "{far} against {near}");
    }
}
