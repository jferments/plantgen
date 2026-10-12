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
//! `terminal_size` times $`L`$.
//!
//! A **palmate** leaf sets them from its petiole's tip, fanned evenly over
//! `spread` degrees, the middle one longest and the outermost `lowest` of
//! it.
//!
//! In a **twice-divided** leaf (`pinnules` above 0) each of those
//! divisions bears `pinnules` leaflets the same way: a pinna along its own
//! rachis at `pinnule_angle`, a palmate division from the tip of a stalk
//! 0.4 of its length. A **dissected** blade (`confluent` above 0) has
//! segments instead of leaflets: their bases broaden and run together
//! along a winged rachis.
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

/// A stretch of a leaf's axes: its petiole and rachis (order 0), or a
/// pinna's rachis or a division's stalk (order 1), from its foot to its
/// end, with the half-width at its foot of the wing a dissected blade's
/// rachis bears (0 for none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub from: Point,
    pub to: Point,
    pub order: u8,
    pub wing: f64,
}

/// One leaflet, or one segment of a dissected blade: where it leaves its
/// rachis, which way it points (a unit vector), its length, stalk
/// included, its division (from the foot, the terminal one first) and, in
/// a twice-divided leaf, its place in that division.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leaflet {
    pub base: Point,
    pub direction: Point,
    pub length: f64,
    pub division: u32,
    pub within: Option<u32>,
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
    pub leaflets: u32,
    pub pinnules: u32,
    pub confluent: f64,
    pub angle: f64,
    pub pinnule_angle: f64,
    pub spread: f64,
    pub largest: f64,
    pub lowest: f64,
    pub terminal_size: f64,
    pub width: f64,
    pub widest: f64,
    pub tip: f64,
    pub base: f64,
    pub petiolule: f64,
    pub petiole: f64,
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
        let once_pinnate = pattern == Pattern::Pinnate && pinnules == 0 && confluent <= 0.0;
        let profile = (c.outline.is_some() || !once_pinnate).then(|| {
            ALONG.map(|t| {
                crate::templates::outline_profile(c.outline.as_deref(), t, widest, blade_base, tip)
            })
        });
        Self {
            pattern,
            leaflets: c.leaflets,
            pinnules,
            confluent,
            angle,
            pinnule_angle: c.pinnule_angle.unwrap_or(angle),
            spread: c.spread.unwrap_or(180.0),
            largest: c.largest.unwrap_or(1.0),
            lowest: c.lowest.unwrap_or(0.82),
            terminal_size: c.terminal_size.unwrap_or(0.98),
            width: c.leaflet_width,
            widest,
            tip,
            base: blade_base,
            petiolule: c.petiolule.unwrap_or(0.0) * (1.0 - confluent),
            petiole: c.petiole,
            profile,
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
/// `(0, 1)`, its first `stalk` bare, each lateral `angle` radians from it:
/// where its rachis ends, and its leaflets, the terminal one first, then
/// its pairs from the foot up, right before left.
fn pinnate(form: &Form, count: u32, stalk: f64, angle: f64) -> (f64, Vec<Placed>) {
    let pairs = count / 2;
    let span = f64::from(pairs.max(1));
    let longest = longest(count, stalk);
    let (sin, cos) = (math::sin(angle), math::cos(angle));
    let terminal = count % 2 == 1;
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
    // Where the leaf is shrunk about if it does not fit.
    let (primary, about) = match form.pattern {
        Pattern::Pinnate => {
            let (end, group) = pinnate(
                &form,
                form.leaflets,
                form.petiole,
                math::radians(form.angle),
            );
            let wing = if form.pinnules == 0 {
                0.5 * form.confluent * 0.35 * form.width * longest(form.leaflets, form.petiole)
            } else {
                0.0
            };
            axes.push(Axis {
                from: (0.0, 0.0),
                to: (0.0, end),
                order: 0,
                wing,
            });
            (group, (0.0, 0.0))
        }
        Pattern::Palmate => {
            axes.push(Axis {
                from: (0.0, 0.0),
                to: (0.0, form.petiole + 0.01),
                order: 0,
                wing: 0.0,
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
    for (division, (base, direction, length)) in (0_u32..).zip(primary) {
        if form.pinnules == 0 {
            leaflets.push(Leaflet {
                base,
                direction,
                length,
                division,
                within: None,
            });
            continue;
        }
        let group = divide(&form, (base, direction, length), &mut axes);
        for (within, (b, d, l)) in (0_u32..).zip(group) {
            leaflets.push(Leaflet {
                base: frame(base, direction, length, b),
                direction: turn(direction, d),
                length: l * length,
                division,
                within: Some(within),
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

/// A division of a twice-divided leaf placed at `base`, pointing along
/// `direction` and `length` long: its axis added to `axes`, and its
/// leaflets in its own frame.
fn divide(form: &Form, (base, direction, length): Placed, axes: &mut Vec<Axis>) -> Vec<Placed> {
    match form.pattern {
        Pattern::Pinnate => {
            let (end, group) = pinnate(
                form,
                form.pinnules,
                PINNA_STALK,
                math::radians(form.pinnule_angle),
            );
            axes.push(Axis {
                from: base,
                to: frame(base, direction, length, (0.0, end)),
                order: 1,
                wing: 0.5
                    * form.confluent
                    * 0.5
                    * form.width
                    * length
                    * longest(form.pinnules, PINNA_STALK),
            });
            group
        }
        Pattern::Palmate => {
            axes.push(Axis {
                from: base,
                to: frame(base, direction, length, (0.0, DIVISION_STALK + 0.01)),
                order: 1,
                wing: 0.0,
            });
            palmate(
                form,
                form.pinnules,
                DIVISION_STALK,
                math::radians(form.spread),
            )
        }
    }
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

    fn compound(edit: impl FnOnce(&mut Compound)) -> Compound {
        let mut c = Compound::default();
        edit(&mut c);
        c
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
