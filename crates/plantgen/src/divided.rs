//! Divided leaves (spec rework P4; LANGUAGE.md 5.2.1): a compound leaf's
//! leaflets laid out by one function, [`layout`], in its card's frame, `x`
//! across and `y` from the petiole's foot (0) to the leaf's tip (1). The
//! card (`crate::templates`) and the leaf's parts (`crate::organs::parts`)
//! both come from it.
//!
//! A **pinnate** leaf sets its leaflets in opposite pairs along a rachis
//! above its petiole, each `angle` degrees from it, and ends in a terminal
//! leaflet (odd-pinnate, an odd count) or in its top pair (even-pinnate,
//! an even count). Of $`P`$ pairs
//! on a rachis whose petiole is the share $`p`$ of it, the longest lateral
//! is $`L = \min(2.4 (1 - p) / (P + 1.5),\ 0.45)`$ long, pair $`j`$ sits at
//! $`u = (j + \tfrac12)/P`$ of the rachis above the petiole and is
//! $`L\,f(u)`$ long: $`f`$ rises from `lowest` at the foot to 1 at
//! `largest` and falls back to `lowest` at the top. The terminal leaflet is
//! `terminal_size` times $`L`$. A **trifoliolate** leaf, one pair and a
//! terminal leaflet, sets its pair at the petiole's tip and its terminal
//! leaflet on a short stalk, reaching the leaf's tip ([`trifoliolate`]).
//!
//! A **palmate** leaf sets them from its petiole's tip, fanned evenly over
//! `spread` degrees, the middle one longest and the outermost `lowest` of
//! it.
//!
//! In a **twice-divided** leaf (`pinnules` above 0) each of those
//! divisions bears `pinnules` leaflets the same way: a pinna along its own
//! rachis at `pinnule_angle`, a palmate division from the tip of a stalk
//! 0.4 of its length. In a **thrice-divided** one (`segments` above 0)
//! each pinnule is divided once more into `segments`, along its own
//! rachis at `segment_angle` or fanned from its stalk's tip. Divisions
//! after the first are set as `inner` says, or as `pattern` if it is
//! unset: a ternate-pinnate leaf is palmate of three, then pinnate. A
//! **dissected** blade (`confluent` above 0) has segments instead of
//! leaflets: their bases broaden and run together along a winged rachis,
//! the rachis of the last division.
//!
//! Where the leaf would leave its card it is shrunk about its petiole's
//! foot (pinnate) or tip (palmate) until it fits: the card is at most 1.6
//! times as wide as it is long.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::looks::{Compound, Pattern};
use crate::math;

/// A point or direction in the card's frame.
pub type Point = (f64, f64);

/// A stretch of a leaf's axes: its petiole and rachis (order 0), a
/// pinna's rachis or a division's stalk (order 1), or a pinnule's (order
/// 2), from its foot to its
/// end, with the half-width at its foot of the wing a dissected blade's
/// rachis bears (0 for none), and, as its leaflets have them, the division
/// it is (or is in) and, for a pinnule's, its place in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub from: Point,
    pub to: Point,
    pub order: u8,
    pub wing: f64,
    pub division: Option<u32>,
    pub within: Option<u32>,
}

/// One leaflet, or one segment of a dissected blade: where it leaves its
/// rachis, which way it points (a unit vector), its length, stalk
/// included, its division (from the foot, the terminal one first), in a
/// leaf divided twice or more its place in that division (its pinnule's
/// in a thrice-divided leaf) and, in a thrice-divided one, its place in
/// its pinnule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leaflet {
    pub base: Point,
    pub direction: Point,
    pub length: f64,
    pub division: u32,
    pub within: Option<u32>,
    pub segment: Option<u32>,
    /// One of the small leaflets interjected between a pinnate leaf's
    /// pairs, which are not counted among its leaflets.
    pub interjected: bool,
}

/// A compound leaf laid out: its form, its axes, its leaflets in the order
/// they are drawn (the first drawn over the rest) and how far either side
/// of its midline it reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub form: Form,
    pub axes: Vec<Axis>,
    pub leaflets: Vec<Leaflet>,
    pub reach: f64,
}

/// A [`Compound`]'s fields with every unset one at its default, and a
/// dissected blade's segments broadened (see the module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Form {
    pub pattern: Pattern,
    pub inner: Pattern,
    pub leaflets: u32,
    pub pinnules: u32,
    pub segments: u32,
    pub confluent: f64,
    pub angle: f64,
    pub pinnule_angle: f64,
    pub segment_angle: f64,
    pub spread: f64,
    pub largest: f64,
    pub lowest: f64,
    pub terminal_size: f64,
    pub interjected: f64,
    pub width: f64,
    pub widest: f64,
    pub tip: f64,
    pub base: f64,
    pub petiolule: f64,
    pub petiole: f64,
    pub blade_width: Option<f64>,
    /// The blade's half-width as a share of its widest at [`ALONG`], where
    /// the leaf is bounded: for a leaf divided as only pinnate leaves once
    /// were and drawn without an outline, none, so it keeps its old fit.
    pub profile: Option<[f64; ALONG.len()]>,
}

/// Where along a leaflet's blade its edges bound the leaf.
pub const ALONG: [f64; 8] = [0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 0.95, 0.99];

impl Form {
    #[must_use]
    pub fn of(c: &Compound) -> Self {
        let confluent = c.confluent.unwrap_or(0.0).clamp(0.0, 1.0);
        let angle = c.angle.unwrap_or(62.0);
        let base = c.base.unwrap_or(0.5);
        let widest = (c.widest.unwrap_or(0.45) * (1.0 - 0.6 * confluent)).max(0.15);
        let tip = c.tip.unwrap_or(0.45);
        let blade_base = base + (1.0 - base) * confluent;
        let pattern = c.pattern.unwrap_or_default();
        let pinnules = c.pinnules.unwrap_or(0);
        let segments = if pinnules > 0 {
            c.segments.unwrap_or(0)
        } else {
            0
        };
        let pinnule_angle = c.pinnule_angle.unwrap_or(angle);
        let once_pinnate = pattern == Pattern::Pinnate && pinnules == 0 && confluent <= 0.0;
        let profile = (c.outline.is_some() || !once_pinnate).then(|| {
            ALONG.map(|t| {
                crate::templates::outline_profile(c.outline.as_deref(), t, widest, blade_base, tip)
            })
        });
        Self {
            pattern,
            inner: c.inner.unwrap_or(pattern),
            leaflets: c.leaflets,
            pinnules,
            segments,
            confluent,
            angle,
            pinnule_angle,
            segment_angle: c.segment_angle.unwrap_or(pinnule_angle),
            spread: c.spread.unwrap_or_else(|| {
                open_enough(pattern, c.leaflets, pinnules, c.leaflet_width, |t| {
                    crate::templates::outline_profile(
                        c.outline.as_deref(),
                        t,
                        widest,
                        blade_base,
                        tip,
                    )
                })
            }),
            largest: c.largest.unwrap_or(1.0),
            lowest: c.lowest.unwrap_or(0.82),
            terminal_size: c.terminal_size.unwrap_or(0.98),
            interjected: c.interjected.unwrap_or(0.0).clamp(0.0, 0.8),
            width: c.leaflet_width,
            widest,
            tip,
            base: blade_base,
            petiolule: c.petiolule.unwrap_or(0.0) * (1.0 - confluent),
            petiole: c.petiole,
            blade_width: c.blade_width,
            profile,
        }
    }

    /// The longest lateral leaflet (or pinna) of a pinnate leaf, as a
    /// share of its rachis: as long as their spacing allows ([`longest`]),
    /// or, where the blade's width is set, no longer than reaches its edge
    /// at `angle` to the rachis.
    #[must_use]
    pub fn primary_longest(&self) -> f64 {
        let spaced = longest(self.leaflets, self.petiole);
        match self.blade_width {
            Some(width) if self.pattern == Pattern::Pinnate => {
                let reach = 0.5 * width * (1.0 - self.petiole);
                spaced.min((reach / math::sin(math::radians(self.angle)).max(0.2)).max(0.01))
            }
            _ => spaced,
        }
    }

    /// The length of a lateral leaflet at `u` of its rachis, as a share of
    /// the longest.
    fn size(&self, u: f64) -> f64 {
        let (largest, lowest) = (self.largest, self.lowest);
        if largest >= 1.0 || u <= largest {
            lowest + (1.0 - lowest) * (u / largest.max(1e-9)).min(1.0)
        } else {
            1.0 - (1.0 - lowest) * (u - largest) / (1.0 - largest)
        }
    }
}

/// How far a palmate leaf whose spread its look leaves unset opens,
/// degrees between its outermost leaflets: as far as its `count` leaflets,
/// each `width` as wide as long at its widest, where `profile` (its
/// half-width along it, 1 at the widest) is greatest, need to stand half
/// their width apart there, seen from the petiole's tip; at least 180, as
/// every such leaf opened before, and at most 340. Thirteen narrow
/// leaflets need more than a half circle, three broad ones less. A
/// twice-divided leaf's divisions, and a pinnate leaf, open 180.
#[must_use]
pub fn open_enough(
    pattern: Pattern,
    count: u32,
    pinnules: u32,
    width: f64,
    profile: impl Fn(f64) -> f64,
) -> f64 {
    if pattern != Pattern::Palmate || pinnules > 0 || count < 2 {
        return 180.0;
    }
    // Where it is widest: toward their feet all leaflets converge on the
    // petiole's tip.
    let widest = ALONG
        .iter()
        .copied()
        .fold((0.5, 0.0), |best: (f64, f64), t| {
            let w = profile(t);
            if w > best.1 { (t, w) } else { best }
        })
        .0;
    let angle = 2.0 * math::atan2(0.5 * width, widest);
    (f64::from(count - 1) * angle.to_degrees() * 1.5).clamp(180.0, 340.0)
}

/// How crowded a once-divided palmate leaf's leaflets are where they are
/// widest: the angle each spans there, seen from the petiole's tip, over
/// the angle between neighbours. Above 1 they overlap, and the leaf reads
/// as one scalloped blade, not as leaflets. `None` for other leaves.
#[must_use]
pub fn crowding(c: &Compound) -> Option<f64> {
    let form = Form::of(c);
    if form.pattern != Pattern::Palmate || form.pinnules > 0 || form.leaflets < 2 {
        return None;
    }
    let profile = |t: f64| {
        crate::templates::outline_profile(c.outline.as_deref(), t, form.widest, form.base, form.tip)
    };
    let widest = ALONG
        .iter()
        .copied()
        .fold((0.5, 0.0), |best: (f64, f64), t| {
            let w = profile(t);
            if w > best.1 { (t, w) } else { best }
        })
        .0;
    let span = 2.0 * math::atan2(0.5 * form.width, widest);
    let apart = form.spread.to_radians() / f64::from(form.leaflets - 1);
    Some(span / apart.max(1e-9))
}

/// The longest lateral leaflet of a pinnate group of `count` along a
/// rachis 1 long whose stalk is the share `stalk` of it.
#[must_use]
pub fn longest(count: u32, stalk: f64) -> f64 {
    let pairs = f64::from(count / 2);
    ((1.0 - stalk) * 2.4 / (pairs + 1.5)).min(0.45)
}

/// A leaflet's base, direction and length.
type Placed = (Point, Point, f64);

/// A pinnate group of `count` leaflets along a rachis from `(0, 0)` to
/// `(0, 1)`, its first `stalk` bare, each lateral `angle` radians from it
/// and the longest `longest` long:
/// where its rachis ends, and its leaflets, the terminal one first, then
/// its pairs from the foot up, right before left.
fn pinnate(form: &Form, count: u32, stalk: f64, angle: f64, longest: f64) -> (f64, Vec<Placed>) {
    let pairs = count / 2;
    let span = f64::from(pairs.max(1));
    let (sin, cos) = (math::sin(angle), math::cos(angle));
    let terminal = count % 2 == 1;
    if let Some((top, length, lateral)) = trifoliolate(form, count, stalk, longest) {
        let mut leaflets = vec![((0.0, top), (0.0, 1.0), length)];
        for side in [1.0, -1.0] {
            leaflets.push(((0.0, stalk), (side * sin, cos), lateral));
        }
        return (top + 0.02, leaflets);
    }
    let top = if terminal {
        1.0 - longest * form.terminal_size.max(1.0)
    } else {
        // The top pair's tips reach the end of the axis.
        let last = (span - 0.5) / span;
        let reach = longest * form.size(last) * cos;
        stalk + (1.0 - reach - stalk) / last
    };
    let mut leaflets = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    if terminal {
        leaflets.push(((0.0, top), (0.0, 1.0), longest * form.terminal_size));
    }
    let mut end = top + 0.02;
    for j in 0..pairs {
        let u = (f64::from(j) + 0.5) / span;
        let at = stalk + (top - stalk) * u;
        let length = longest * form.size(u);
        for side in [1.0, -1.0] {
            leaflets.push(((0.0, at), (side * sin, cos), length));
        }
        if !terminal {
            end = at + 0.015;
        }
    }
    (end, leaflets)
}

/// The terminal leaflet's stalk above a trifoliolate leaf's lateral pair,
/// a share of the terminal leaflet's length.
pub const TERMINAL_STALK: f64 = 0.15;

/// A trifoliolate leaf, divided once into a terminal leaflet and one
/// lateral pair, on a rachis from `(0, 0)` to `(0, 1)` whose first `stalk` is bare: where
/// its terminal leaflet leaves the rachis, its length and its laterals'.
/// The pair sits at the top of the stalk (a trifoliolate leaf's laterals
/// are at its petiole's tip, "lateral leaflets usually sessile and the
/// terminal one stalked", Abrams vol. 3 p. 50), and the terminal leaflet
/// stands on a short stalk, [`TERMINAL_STALK`] of its length, its tip at
/// the rachis's end. The laterals are `terminal_size` as long, but no
/// longer than `longest`. `None` for any other leaf or group: a pinna's
/// or pinnule's group of three keeps its pair halfway up.
#[must_use]
pub fn trifoliolate(form: &Form, count: u32, stalk: f64, longest: f64) -> Option<(f64, f64, f64)> {
    (count == 3 && form.pinnules == 0).then(|| {
        let length = (1.0 - stalk) / (1.0 + TERMINAL_STALK);
        let lateral = (length / form.terminal_size.max(0.3)).min(longest);
        (stalk + TERMINAL_STALK * length, length, lateral)
    })
}

/// A palmate group of `count` leaflets from `(0, stalk)`, fanned over
/// `spread` radians, the middle one `1 - stalk` long: the middle one
/// first, then outward, right before left.
fn palmate(form: &Form, count: u32, stalk: f64, spread: f64) -> Vec<Placed> {
    let n = f64::from(count.max(2) - 1);
    let mut order: Vec<u32> = (0..count).collect();
    order.sort_by(|a, b| {
        let off = |k: u32| (2.0 * f64::from(k) / n - 1.0).abs();
        off(*a).total_cmp(&off(*b)).then(b.cmp(a))
    });
    order
        .into_iter()
        .map(|k| {
            let v = if count > 1 {
                2.0 * f64::from(k) / n - 1.0
            } else {
                0.0
            };
            let theta = 0.5 * spread * v;
            let length = (1.0 - stalk) * (1.0 - (1.0 - form.lowest) * v.abs());
            ((0.0, stalk), (math::sin(theta), math::cos(theta)), length)
        })
        .collect()
}

/// `local`, a point of a frame whose `y` runs along `direction` from
/// `origin`, `scale` card units to its 1, in the card's frame.
fn frame(origin: Point, direction: Point, scale: f64, local: Point) -> Point {
    let across = (direction.1, -direction.0);
    (
        origin.0 + scale * (local.1 * direction.0 + local.0 * across.0),
        origin.1 + scale * (local.1 * direction.1 + local.0 * across.1),
    )
}

/// A direction of such a frame in the card's.
fn turn(direction: Point, local: Point) -> Point {
    frame((0.0, 0.0), direction, 1.0, local)
}

/// The share of a pinna that is bare stalk below its first leaflets.
const PINNA_STALK: f64 = 0.06;
/// The share of a palmate division that is stalk below its leaflets.
const DIVISION_STALK: f64 = 0.4;

/// `c` laid out (see the module).
#[must_use]
pub fn layout(c: &Compound) -> Layout {
    let form = Form::of(c);
    let mut axes = Vec::new();
    let once = form.pinnules == 0;
    // Where the leaf is shrunk about if it does not fit.
    let (primary, about) = match form.pattern {
        Pattern::Pinnate => {
            let (end, group) = pinnate(
                &form,
                form.leaflets,
                form.petiole,
                math::radians(form.angle),
                form.primary_longest(),
            );
            let wing = if once {
                0.5 * form.confluent * 0.35 * form.width * form.primary_longest()
            } else {
                0.0
            };
            axes.push(Axis {
                from: (0.0, 0.0),
                to: (0.0, end),
                order: 0,
                wing,
                division: None,
                within: None,
            });
            (group, (0.0, 0.0))
        }
        Pattern::Palmate => {
            axes.push(Axis {
                from: (0.0, 0.0),
                to: (0.0, form.petiole + 0.01),
                order: 0,
                wing: 0.0,
                division: None,
                within: None,
            });
            (
                palmate(
                    &form,
                    form.leaflets,
                    form.petiole,
                    math::radians(form.spread),
                ),
                (0.0, form.petiole),
            )
        }
    };
    let mut leaflets = Vec::new();
    for (division, placed) in (0_u32..).zip(primary) {
        if once {
            leaflets.push(leaflet(placed, division, None, None));
            continue;
        }
        let on = (division, None);
        for (within, pinnule) in (0_u32..).zip(divide(&form, 1, on, placed, &mut axes)) {
            if form.segments == 0 {
                leaflets.push(leaflet(pinnule, division, Some(within), None));
                continue;
            }
            let on = (division, Some(within));
            for (segment, placed) in (0_u32..).zip(divide(&form, 2, on, pinnule, &mut axes)) {
                leaflets.push(leaflet(placed, division, Some(within), Some(segment)));
            }
        }
    }
    if once && form.pattern == Pattern::Pinnate && form.interjected > 0.0 {
        let first = u32::try_from(leaflets.len()).unwrap_or(u32::MAX);
        for (k, placed) in (first..).zip(interjected(&form, &leaflets)) {
            leaflets.push(Leaflet {
                interjected: true,
                ..leaflet(placed, k, None, None)
            });
        }
    }
    let mut laid = Layout {
        form,
        axes,
        leaflets,
        reach: 0.0,
    };
    laid.fit(about);
    laid
}

/// The small leaflets interjected between a once-pinnate leaf's pairs: a
/// pair halfway between each two pairs, and between the top pair and the
/// terminal leaflet, each `interjected` as long as the shorter of the two
/// pairs, set at their angle. Drawn after the rest, so under them.
fn interjected(form: &Form, leaflets: &[Leaflet]) -> Vec<Placed> {
    // The pairs' places up the rachis, each with its length and angle.
    let mut pairs: Vec<(f64, f64, Point)> = leaflets
        .iter()
        .filter(|l| l.direction.0 > 0.0)
        .map(|l| (l.base.1, l.length, l.direction))
        .collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some(top) = leaflets.iter().find(|l| l.direction.0 == 0.0) {
        if let Some(&(_, length, direction)) = pairs.last() {
            pairs.push((top.base.1, length, direction));
        }
    }
    let mut placed = Vec::new();
    for pair in pairs.windows(2) {
        let (lower, upper) = (pair[0], pair[1]);
        let at = 0.5 * (lower.0 + upper.0);
        let length = form.interjected * lower.1.min(upper.1);
        for side in [1.0, -1.0] {
            placed.push(((0.0, at), (side * lower.2.0, lower.2.1), length));
        }
    }
    placed
}

fn leaflet(
    (base, direction, length): Placed,
    division: u32,
    within: Option<u32>,
    segment: Option<u32>,
) -> Leaflet {
    Leaflet {
        base,
        direction,
        length,
        division,
        within,
        segment,
        interjected: false,
    }
}

/// A division of order `order` (1 a pinna, 2 a pinnule), `on` a division
/// and in a pinna's place in it, placed at `base`,
/// pointing along `direction` and `length` long: its axis added to
/// `axes`, and its leaflets or segments placed in the card's frame. Only
/// the last division's rachis bears a dissected blade's wing.
fn divide(
    form: &Form,
    order: u8,
    (division, within): (u32, Option<u32>),
    (base, direction, length): Placed,
    axes: &mut Vec<Axis>,
) -> Vec<Placed> {
    let (count, angle, last) = if order == 1 {
        (form.pinnules, form.pinnule_angle, form.segments == 0)
    } else {
        (form.segments, form.segment_angle, true)
    };
    let group = match form.inner {
        Pattern::Pinnate => {
            let (end, group) = pinnate(
                form,
                count,
                PINNA_STALK,
                math::radians(angle),
                longest(count, PINNA_STALK),
            );
            let wing = if last {
                0.5 * form.confluent * 0.5 * form.width * length * longest(count, PINNA_STALK)
            } else {
                0.0
            };
            axes.push(Axis {
                from: base,
                to: frame(base, direction, length, (0.0, end)),
                order,
                wing,
                division: Some(division),
                within,
            });
            group
        }
        Pattern::Palmate => {
            axes.push(Axis {
                from: base,
                to: frame(base, direction, length, (0.0, DIVISION_STALK + 0.01)),
                order,
                wing: 0.0,
                division: Some(division),
                within,
            });
            palmate(form, count, DIVISION_STALK, math::radians(form.spread))
        }
    };
    group
        .into_iter()
        .map(|(b, d, l)| {
            (
                frame(base, direction, length, b),
                turn(direction, d),
                l * length,
            )
        })
        .collect()
}

/// How far either side of its midline `c`'s layout reaches, remembered
/// per shape, since a card asks for it at every texel.
#[must_use]
pub fn reach(c: &Compound) -> f64 {
    static REACH: OnceLock<Mutex<BTreeMap<String, f64>>> = OnceLock::new();
    let key = format!("{c:?}");
    let reaches = REACH.get_or_init(Default::default);
    if let Some(&reach) = reaches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return reach;
    }
    let reach = layout(c).reach;
    reaches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, reach);
    reach
}

/// A divided leaf's sizes as its characters measure them (spec rework
/// M8B), each a share of its card's length: its blade from its lowest
/// leaflets to its farthest tip, its widest across, and its petiole below
/// the lowest leaflets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimensions {
    pub blade: f64,
    pub width: f64,
    pub petiole: f64,
}

/// `c`'s [`Dimensions`], from its layout.
#[must_use]
pub fn dimensions(c: &Compound) -> Dimensions {
    let laid = layout(c);
    let foot = laid
        .axes
        .iter()
        .map(|axis| axis.from.1)
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let first = laid
        .leaflets
        .iter()
        .map(|leaflet| leaflet.base.1)
        .fold(f64::INFINITY, f64::min);
    let points = laid.bounds();
    let top = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    if !first.is_finite() || !top.is_finite() {
        return Dimensions {
            blade: 0.0,
            width: 0.0,
            petiole: 0.0,
        };
    }
    Dimensions {
        blade: (top - first).max(0.0),
        width: (right - left).max(0.0),
        petiole: (first - foot).max(0.0),
    }
}

/// A compound leaf's counts as floras give them (spec rework M8A): its
/// leaflets, one count for a leaf divided once, one for each pinna of a
/// twice-divided leaf and one for each pinnule of a thrice-divided one;
/// its pinnules, one count for each pinna of a thrice-divided leaf and
/// none otherwise; its pinnae, 0 for a leaf divided once; and, where its
/// last division is pinnate, whether it ends in a single leaflet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counts {
    pub leaflets: Vec<u32>,
    pub pinnules: Vec<u32>,
    pub pinnae: u32,
    pub terminal: Option<bool>,
}

/// `c`'s [`Counts`], from the leaflets its layout draws.
#[must_use]
pub fn counts(c: &Compound) -> Counts {
    let laid = layout(c);
    let mut per_group: BTreeMap<(u32, Option<u32>), u32> = BTreeMap::new();
    let mut per_division: BTreeMap<u32, std::collections::BTreeSet<Option<u32>>> = BTreeMap::new();
    for leaflet in &laid.leaflets {
        let thrice = leaflet.segment.is_some();
        let group = if thrice { leaflet.within } else { None };
        *per_group.entry((leaflet.division, group)).or_default() += 1;
        per_division
            .entry(leaflet.division)
            .or_default()
            .insert(leaflet.within);
    }
    let once = laid.form.pinnules == 0;
    let thrice = laid.form.segments > 0;
    let leaflets = if once {
        let counted = laid.leaflets.iter().filter(|l| !l.interjected).count();
        vec![u32::try_from(counted).unwrap_or(u32::MAX)]
    } else {
        per_group.values().copied().collect()
    };
    let pinnules = if thrice {
        per_division
            .values()
            .map(|within| u32::try_from(within.len()).unwrap_or(u32::MAX))
            .collect()
    } else {
        Vec::new()
    };
    let pinnae = if once {
        0
    } else {
        u32::try_from(per_division.len()).unwrap_or(u32::MAX)
    };
    let last = if once {
        laid.form.pattern
    } else {
        laid.form.inner
    };
    // An odd count ends in a leaflet, an even one in a pair.
    let terminal =
        (last == Pattern::Pinnate).then(|| leaflets.first().is_some_and(|count| count % 2 == 1));
    Counts {
        leaflets,
        pinnules,
        pinnae,
        terminal,
    }
}

impl Layout {
    /// The points that bound the leaf: its axes' ends, and each leaflet's
    /// base, tip and the edges of its blade where it is widest.
    fn bounds(&self) -> Vec<Point> {
        let mut points = Vec::with_capacity(self.axes.len() * 2 + self.leaflets.len() * 4);
        for axis in &self.axes {
            points.push(axis.from);
            points.push(axis.to);
        }
        let form = &self.form;
        for leaflet in &self.leaflets {
            let (b, d, l) = (leaflet.base, leaflet.direction, leaflet.length);
            let stalk = form.petiolule * l;
            let widest = stalk + (l - stalk) * form.widest;
            let half = 0.5 * form.width * (l - stalk);
            points.push(b);
            points.push((b.0 + l * d.0, b.1 + l * d.1));
            let mut edge = |along: f64, half: f64| {
                for side in [1.0, -1.0] {
                    points.push((
                        b.0 + along * d.0 + side * half * d.1,
                        b.1 + along * d.1 - side * half * d.0,
                    ));
                }
            };
            edge(widest, half);
            if let Some(profile) = form.profile {
                for (t, share) in ALONG.iter().zip(profile) {
                    edge(stalk + (l - stalk) * t, half * share);
                }
            }
        }
        points
    }

    /// Shrink the leaf about `about` until it stays inside its card, and
    /// note its reach.
    fn fit(&mut self, about: Point) {
        let points = self.bounds();
        let top = points.iter().fold(f64::MIN, |m, p| m.max(p.1));
        let bottom = points.iter().fold(f64::MAX, |m, p| m.min(p.1));
        let side = points.iter().fold(0.0, |m: f64, p| m.max(p.0.abs()));
        let mut scale: f64 = 1.0;
        if top > 1.0 {
            scale = scale.min((1.0 - about.1) / (top - about.1));
        }
        if bottom < 0.01 && about.1 > bottom {
            scale = scale.min((about.1 - 0.01).max(0.0) / (about.1 - bottom));
        }
        if side > 0.785 {
            scale = scale.min(0.785 / side);
        }
        if scale < 1.0 {
            let at = |p: Point| {
                (
                    about.0 + scale * (p.0 - about.0),
                    about.1 + scale * (p.1 - about.1),
                )
            };
            for axis in &mut self.axes {
                // The petiole still stands on the card's foot.
                if axis.from != (0.0, 0.0) {
                    axis.from = at(axis.from);
                }
                axis.to = at(axis.to);
                axis.wing *= scale;
            }
            for leaflet in &mut self.leaflets {
                leaflet.base = at(leaflet.base);
                leaflet.length *= scale;
            }
        }
        self.reach = side * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A palmate leaf whose spread is unset opens as far as its leaflets
    /// need to stand apart: thirteen narrow ones nearly all the way round,
    /// three broad ones the half circle every such leaf opened before.
    #[test]
    fn a_palmate_leaf_opens_as_far_as_its_leaflets_need() {
        let narrow = Form::of(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 13;
            c.leaflet_width = 0.2;
            c.widest = Some(0.7);
        }));
        assert!(
            narrow.spread > 280.0 && narrow.spread <= 340.0,
            "{}",
            narrow.spread
        );
        let few = Form::of(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 3;
        }));
        assert_eq!(few.spread, 180.0);
        let set = Form::of(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 13;
            c.spread = Some(200.0);
        }));
        assert_eq!(set.spread, 200.0);
        // Neighbours no longer overlap where they are widest.
        let laid = layout(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 13;
            c.leaflet_width = 0.2;
            c.widest = Some(0.7);
        }));
        let gap = narrow.spread.to_radians() / 12.0;
        let half = 2.0 * math::atan2(0.5 * 0.2, 0.7);
        assert!(gap > half, "{gap} {half}");
        assert_eq!(laid.leaflets.len(), 13);
    }

    fn compound(edit: impl FnOnce(&mut Compound)) -> Compound {
        let mut c = Compound::default();
        edit(&mut c);
        c
    }

    #[test]
    fn counts_come_from_the_leaflets_laid_out() {
        let odd = counts(&compound(|c| c.leaflets = 7));
        assert_eq!(odd.leaflets, vec![7]);
        assert_eq!(odd.pinnae, 0);
        assert_eq!(odd.terminal, Some(true));
        let twice = counts(&compound(|c| {
            c.leaflets = 4;
            c.pinnules = Some(6);
        }));
        assert_eq!(twice.pinnae, 4);
        assert_eq!(twice.leaflets, vec![6; 4]);
        assert_eq!(twice.terminal, Some(false));
        let palmate = counts(&compound(|c| {
            c.leaflets = 5;
            c.pattern = Some(Pattern::Palmate);
        }));
        assert_eq!(palmate.leaflets, vec![5]);
        assert_eq!(palmate.terminal, None);
    }

    #[test]
    fn an_odd_pinnate_leaf_is_laid_out_as_it_always_was() {
        // Before P4 the card laid its leaflets out in place; these are its
        // numbers for the default leaf.
        let laid = layout(&Compound::default());
        let leaflet = (2.4 * 0.85 / 4.5_f64).min(0.45);
        let top = 1.0 - leaflet;
        assert_eq!(laid.leaflets.len(), 7);
        assert_eq!(laid.leaflets[0].base, (0.0, top));
        assert!((laid.leaflets[0].length - 0.98 * leaflet).abs() < 1e-12);
        let lowest = laid.leaflets[1];
        assert!((lowest.base.1 - (0.15 + (top - 0.15) * 0.5 / 3.0)).abs() < 1e-12);
        assert!((lowest.length - leaflet * (0.82 + 0.18 * 0.5 / 3.0)).abs() < 1e-12);
        let angle = math::radians(62.0);
        assert!((lowest.direction.0 - math::sin(angle)).abs() < 1e-12);
        assert!(laid.leaflets[2].direction.0 < 0.0);
        assert_eq!(laid.axes.len(), 1);
        assert!((laid.axes[0].to.1 - (top + 0.02)).abs() < 1e-12);
        assert!(laid.leaflets.iter().all(|l| l.within.is_none()));
    }

    #[test]
    fn a_trifoliolate_leaf_sets_its_pair_at_the_petiole_tip() {
        let laid = layout(&compound(|c| {
            c.leaflets = 3;
            c.petiole = 0.25;
        }));
        assert_eq!(laid.leaflets.len(), 3);
        let terminal = laid.leaflets[0];
        for lateral in &laid.leaflets[1..] {
            assert!((lateral.base.1 - 0.25).abs() < 1e-12, "{lateral:?}");
        }
        // The terminal leaflet stands on a short stalk and reaches the tip.
        let stalk = terminal.base.1 - 0.25;
        assert!((stalk - TERMINAL_STALK * terminal.length).abs() < 1e-12);
        assert!((terminal.base.1 + terminal.length - 1.0).abs() < 1e-9);
        assert!(
            (dimensions(&compound(|c| {
                c.leaflets = 3;
                c.petiole = 0.25;
            }))
            .petiole
                - 0.25)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn an_even_pinnate_leaf_ends_in_a_pair() {
        let laid = layout(&compound(|c| {
            c.leaflets = 8;
        }));
        assert_eq!(laid.leaflets.len(), 8);
        let top = laid.leaflets.iter().map(|l| l.base.1).fold(0.0, f64::max);
        // The rachis ends just past its top pair, whose tips reach the
        // card's tip.
        assert!(laid.axes[0].to.1 > top && laid.axes[0].to.1 < top + 0.02);
        let reach = laid
            .leaflets
            .iter()
            .map(|l| l.base.1 + l.length * l.direction.1)
            .fold(0.0, f64::max);
        assert!(reach > 0.95 && reach <= 1.0 + 1e-9, "{reach}");
    }

    #[test]
    fn leaflets_are_longest_where_the_leaf_says() {
        let laid = layout(&compound(|c| {
            c.leaflets = 11;
            c.largest = Some(0.5);
            c.lowest = Some(0.4);
        }));
        let laterals: Vec<f64> = laid.leaflets[1..]
            .iter()
            .step_by(2)
            .map(|l| l.length)
            .collect();
        let longest = laterals.iter().copied().fold(0.0, f64::max);
        let at = laterals
            .iter()
            .position(|&l| (l - longest).abs() < 1e-12)
            .unwrap();
        assert!(at > 0 && at < laterals.len() - 1, "{laterals:?}");
        assert!(laterals[0] < 0.6 * longest && laterals[laterals.len() - 1] < 0.6 * longest);
    }

    #[test]
    fn a_palmate_leaf_fans_from_its_petiole_and_fits_its_card() {
        let laid = layout(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 9;
            c.spread = Some(300.0);
            c.petiole = 0.45;
            c.leaflet_width = 0.25;
            c.lowest = Some(0.7);
        }));
        assert_eq!(laid.leaflets.len(), 9);
        let junction = laid.leaflets[0].base;
        assert!(laid.leaflets.iter().all(|l| l.base == junction));
        // The middle one first, pointing along the petiole, and longest.
        assert!(laid.leaflets[0].direction.0.abs() < 1e-12);
        assert!(
            laid.leaflets
                .iter()
                .all(|l| l.length <= laid.leaflets[0].length + 1e-12)
        );
        // Leaflets reach back past the junction, inside the card.
        for leaflet in &laid.leaflets {
            let tip = leaflet.base.1 + leaflet.length * leaflet.direction.1;
            assert!((0.0..=1.0 + 1e-9).contains(&tip), "{leaflet:?}");
        }
        assert!(laid.leaflets.iter().any(|l| l.direction.1 < -0.5));
        assert!(laid.reach <= 0.785 + 1e-9);
    }

    #[test]
    fn a_twice_divided_leaf_bears_leaflets_on_each_division() {
        // A mesquite's: one pair of pinnae, each with 15 pairs of leaflets.
        let laid = layout(&compound(|c| {
            c.leaflets = 2;
            c.pinnules = Some(30);
            c.angle = Some(35.0);
            c.pinnule_angle = Some(75.0);
            c.leaflet_width = 0.3;
        }));
        assert_eq!(laid.leaflets.len(), 60);
        assert_eq!(laid.axes.len(), 3);
        for division in 0..2 {
            let on: Vec<_> = laid
                .leaflets
                .iter()
                .filter(|l| l.division == division)
                .collect();
            assert_eq!(on.len(), 30);
            assert!(
                on.iter()
                    .enumerate()
                    .all(|(i, l)| l.within == u32::try_from(i).ok())
            );
        }
        // Each pinna's leaflets stand along it, inside the card.
        let points = laid.bounds();
        assert!(
            points
                .iter()
                .all(|p| p.1 <= 1.0 + 1e-9 && p.0.abs() <= laid.reach + 1e-9)
        );
        // A biternate leaf: three divisions of three.
        let laid = layout(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 3;
            c.pinnules = Some(3);
            c.spread = Some(140.0);
        }));
        assert_eq!(laid.leaflets.len(), 9);
        assert_eq!(laid.axes.len(), 4);
    }

    #[test]
    fn a_thrice_divided_leaf_bears_segments_on_each_pinnule() {
        // A tripinnate leaf: 7 pinnae of 5 pinnules of 3 segments.
        let c = compound(|c| {
            c.leaflets = 7;
            c.pinnules = Some(5);
            c.segments = Some(3);
            c.leaflet_width = 0.2;
        });
        let laid = layout(&c);
        assert_eq!(laid.leaflets.len(), 7 * 5 * 3);
        assert_eq!(laid.axes.len(), 1 + 7 + 7 * 5);
        assert_eq!(laid.axes.iter().filter(|a| a.order == 2).count(), 35);
        assert!(laid.leaflets.iter().all(|l| l.segment.is_some()));
        // Each segment is shorter than the pinnule it divides.
        let twice = layout(&compound(|c| {
            c.leaflets = 7;
            c.pinnules = Some(5);
            c.leaflet_width = 0.2;
        }));
        let longest = |laid: &Layout| laid.leaflets.iter().map(|l| l.length).fold(0.0, f64::max);
        assert!(longest(&laid) < 0.6 * longest(&twice));
        let points = laid.bounds();
        assert!(
            points
                .iter()
                .all(|p| p.1 <= 1.0 + 1e-9 && p.0.abs() <= laid.reach + 1e-9)
        );
        let counted = counts(&c);
        assert_eq!(counted.pinnae, 7);
        assert_eq!(counted.pinnules, vec![5; 7]);
        assert_eq!(counted.leaflets, vec![3; 35]);
        assert_eq!(counted.terminal, Some(true));
        // Segments without pinnules are not a third division.
        let none = Form::of(&compound(|c| c.segments = Some(3)));
        assert_eq!(none.segments, 0);
    }

    #[test]
    fn a_ternate_pinnate_leaf_is_pinnate_after_its_first_division() {
        let c = compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.inner = Some(Pattern::Pinnate);
            c.leaflets = 3;
            c.pinnules = Some(7);
        });
        let laid = layout(&c);
        assert_eq!(laid.leaflets.len(), 21);
        // Three pinnae fanned from the petiole's tip, each a rachis with a
        // terminal leaflet and three pairs.
        let pinnae: Vec<_> = laid.axes.iter().filter(|a| a.order == 1).collect();
        assert_eq!(pinnae.len(), 3);
        assert!(pinnae.iter().all(|a| a.from == pinnae[0].from));
        let counted = counts(&c);
        assert_eq!(counted.leaflets, vec![7; 3]);
        assert_eq!(counted.terminal, Some(true));
        // Unset, the inner pattern is the first's: biternate.
        let biternate = Form::of(&compound(|c| {
            c.pattern = Some(Pattern::Palmate);
            c.leaflets = 3;
            c.pinnules = Some(3);
        }));
        assert_eq!(biternate.inner, Pattern::Palmate);
    }

    #[test]
    fn a_dissected_blade_has_a_winged_rachis_and_broad_based_segments() {
        let c = compound(|c| {
            c.leaflets = 21;
            c.pinnules = Some(7);
            c.confluent = Some(0.8);
        });
        let laid = layout(&c);
        assert!(laid.axes.iter().skip(1).all(|a| a.wing > 0.0));
        let form = Form::of(&c);
        assert!(form.base > 0.85 && form.widest < 0.3 && form.petiolule == 0.0);
    }
}
