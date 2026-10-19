//! Levels drawn to a budget (`plantc export --triangles --max-mib`): one
//! dial through a quality's levels sets every knob of a drawing at once,
//! so an export can search it for the most detail that fits a user's
//! triangles and bytes.
//!
//! The dial runs from 0, the quality's nearest level, through 1, 2 and 3,
//! its other levels, to [`DIAL_MAX`]. On a whole number up to 3 it draws
//! that level as a package does. Between two whole numbers it draws the
//! finer level's features (which organs are solids, part meshes, the
//! fleshy bodies' detail and the roots), and the lengths and counts of its
//! [`LodSpec`] go from the finer level's to the coarser's: the thinnest
//! wood kept, the spacing of tube rings and the size of leaf clusters
//! geometrically (linearly from 0), the most sides around a tube
//! geometrically and the bend that keeps a ring linearly. Past the
//! farthest level they keep going at the rate they went from the level
//! before it, so wood thins to its stems and leaves to a few big cards.
//! Between the nearest level and the next, solids give way to cards: over
//! the first half of the way part meshes hold fewer triangles and areoles
//! keep fewer of their spines, the longest, down to [`SOLID_SHARE`] (a
//! type whose coarsest part mesh no longer fits keeps its cards); over the
//! second half a growing share of the organs and areoles, picked by their
//! number's hash, are drawn as their cards instead. Fleshy bodies' detail
//! goes from one level's to the next the same way (`crate::body`), their
//! tubercles staying until the next level; and on the way from the level
//! that draws areoles as cards to the one where spines only tint the skin,
//! a growing share of the areoles' cards go over the second half. Going from a level that draws a card per
//! organ to one that clusters them, the first half of the way draws a
//! growing share of the bent cards flat, the smallest first
//! ([`Detail::flat`]; a bent card takes 16 triangles, a flat one 2), and
//! the clusters grow from nothing over the second half.
//!
//! Each step along the dial draws fewer triangles, mostly: a coarser
//! level's sticks can stand in for more wood than the thin twigs they
//! replace. A search ([`most_detail`]) takes the finest dial it finds that
//! fits.

use crate::mesh::LodSpec;
use crate::quality::Quality;

/// The coarsest dial: three levels past the farthest, where a tree keeps
/// its stems and thickest limbs and its leaves are a few cards.
pub const DIAL_MAX: f64 = 6.0;

/// The least share of its finest triangles a part mesh may
/// hold, and of its spines an areole keeps as solids, half a level along
/// the dial, approached geometrically from the nearest level.
pub const SOLID_SHARE: f64 = 1.0 / 16.0;

/// The level whose areoles are cards, the next tinting the skin
/// (`crate::body`'s detail).
const SPINE_CARDS_LEVEL: usize = 1;

/// How one level is drawn at a point of the dial.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detail {
    /// The point, 0 to [`DIAL_MAX`].
    pub dial: f64,
    /// The level whose features it draws: the dial's whole part, at most
    /// the farthest level.
    pub level: usize,
    /// How far it is from that level toward the next, 0 to 1; 0 past the
    /// farthest level.
    pub along: f64,
    /// Its lengths and counts, for a plant of the height it was given.
    pub lod: LodSpec,
    /// Share of their finest triangles the part meshes hold, and of their spines the
    /// areoles drawn solid keep; 1 on the nearest level, and 0 where
    /// neither is drawn.
    pub solid_detail: f64,
    /// Share of the organs with part meshes and of the areoles drawn
    /// solid, the rest as their cards; 1 on the nearest level, and 0
    /// where neither is drawn.
    pub solid_share: f64,
    /// Share of the areoles drawn as cards that keep them.
    pub spine_cards: f64,
    /// Share of the bent cards drawn flat, the smallest first; 1 where
    /// leaves are clustered, whose cards are all flat.
    pub flat: f64,
}

impl Detail {
    /// Level `level` of `quality` as a package draws it, for a plant
    /// `height` metres tall.
    #[must_use]
    pub fn level(quality: &Quality, height: f64, level: usize) -> Self {
        let level = level.min(quality.lods.len() - 1);
        #[allow(clippy::cast_precision_loss)]
        let dial = level as f64;
        Self {
            dial,
            level,
            along: 0.0,
            lod: quality.lods[level].for_height(height),
            solid_detail: if level == 0 { 1.0 } else { 0.0 },
            solid_share: if level == 0 { 1.0 } else { 0.0 },
            spine_cards: 1.0,
            flat: if quality.lods[level].cluster > 0.0 {
                1.0
            } else {
                0.0
            },
        }
    }

    /// The drawing at `dial` (see the module) for a plant `height` metres
    /// tall.
    #[must_use]
    pub fn at(quality: &Quality, height: f64, dial: f64) -> Self {
        let dial = dial.clamp(0.0, DIAL_MAX);
        let last = quality.lods.len() - 1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = (dial.floor() as usize).min(last);
        #[allow(clippy::cast_precision_loss)]
        let along = dial - whole as f64;
        if along <= 0.0 {
            return Self::level(quality, height, whole);
        }
        let lods = quality.lods.map(|lod| lod.for_height(height));
        // From level `from` toward level `from + 1`, or on past the
        // farthest at the rate it came from the one before.
        let (from, to, t) = if whole < last {
            (whole, whole + 1, along)
        } else {
            (last - 1, last, 1.0 + along)
        };
        let (a, b) = (lods[from], lods[to]);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sides =
            |a: u32, b: u32| (geometric(f64::from(a), f64::from(b), t).round() as u32).max(3);
        let min_sides = sides(a.min_sides, b.min_sides);
        // Bent cards go flat before organs gather into clusters.
        let (cluster, flat) = if a.cluster <= 0.0 && b.cluster > 0.0 {
            if t < 0.5 {
                (0.0, 2.0 * t)
            } else {
                (b.cluster * (2.0 * t - 1.0), 1.0)
            }
        } else {
            let cluster = geometric(a.cluster, b.cluster, t);
            (cluster, if cluster > 0.0 { 1.0 } else { 0.0 })
        };
        let lod = LodSpec {
            min_radius: geometric(a.min_radius, b.min_radius, t),
            ring_edge: geometric(a.ring_edge, b.ring_edge, t),
            min_sides,
            max_sides: sides(a.max_sides, b.max_sides).max(min_sides),
            bend: (a.bend + (b.bend - a.bend) * t).clamp(0.0, MAX_BEND),
            cluster,
        };
        Self {
            dial,
            level: whole,
            along: if whole < last { along } else { 0.0 },
            lod,
            solid_detail: match whole {
                0 if along < 0.5 => crate::math::pow(SOLID_SHARE, 2.0 * along),
                0 => SOLID_SHARE,
                _ => 0.0,
            },
            solid_share: match whole {
                0 if along < 0.5 => 1.0,
                0 => 2.0 - 2.0 * along,
                _ => 0.0,
            },
            spine_cards: if whole == SPINE_CARDS_LEVEL && along >= 0.5 {
                2.0 - 2.0 * along
            } else {
                1.0
            },
            flat,
        }
    }
}

/// The most an axis may turn at a ring that is dropped, degrees, however
/// far the dial goes.
const MAX_BEND: f64 = 60.0;

/// `t` of the way from `a` to `b` (past `b` beyond 1): geometrically when
/// both are above 0, else linearly.
fn geometric(a: f64, b: f64, t: f64) -> f64 {
    if a > 0.0 && b > 0.0 {
        a * crate::math::pow(b / a, t)
    } else {
        (a + (b - a) * t).max(0.0)
    }
}

/// The finest dial from `low` to [`DIAL_MAX`] whose drawing fits its
/// budget, and whether it does there: [`DIAL_MAX`] and false when nothing
/// fits. `taken(dial)` is how much of its budget the drawing at `dial`
/// takes, 1 just fitting. It tries `low`, then each whole number past it,
/// then narrows the last step from over to within the budget, aiming
/// where the logarithm of what is taken, drawn straight between the two
/// ends, reaches [`AIM`], until a drawing takes at least [`FULL`] or the
/// step is [`STEP`] wide.
///
/// # Errors
///
/// When `taken` fails.
pub fn most_detail<E>(
    low: f64,
    mut taken: impl FnMut(f64) -> Result<f64, E>,
) -> Result<(f64, bool), E> {
    let low = low.clamp(0.0, DIAL_MAX);
    let first = taken(low)?;
    if first <= 1.0 {
        return Ok((low, true));
    }
    let mut over = (low, first);
    let mut under = None;
    let mut next = low.floor() + 1.0;
    while next <= DIAL_MAX {
        let share = taken(next)?;
        if share <= 1.0 {
            under = Some((next, share));
            break;
        }
        over = (next, share);
        next += 1.0;
    }
    let Some(mut under) = under else {
        return Ok((DIAL_MAX, false));
    };
    for _ in 0..NARROWINGS {
        let width = under.0 - over.0;
        if under.1 >= FULL || width <= STEP {
            break;
        }
        // Where ln(taken) reaches ln(AIM) on the line between the ends,
        // kept a tenth of the step from either.
        let (high, low) = (crate::math::ln(over.1), crate::math::ln(under.1.max(1e-9)));
        let t = ((high - crate::math::ln(AIM)) / (high - low)).clamp(0.1, 0.9);
        let dial = over.0 + width * if t.is_finite() { t } else { 0.5 };
        let share = taken(dial)?;
        if share <= 1.0 {
            under = (dial, share);
        } else {
            over = (dial, share);
        }
    }
    Ok((under.0, true))
}

/// A drawing taking at least this share of its budget is close enough.
pub const FULL: f64 = 0.95;

/// What the search aims for, a little within the budget so the drawing
/// aimed at fits.
const AIM: f64 = 0.97;

/// The narrowest step the search narrows to, and the most narrowings.
const STEP: f64 = 1.0 / 64.0;
const NARROWINGS: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality::STANDARD;

    #[test]
    fn whole_numbers_are_the_qualitys_levels() {
        for height in [0.4, 6.0, 30.0] {
            for level in 0..4 {
                #[allow(clippy::cast_precision_loss)]
                let detail = Detail::at(&STANDARD, height, level as f64);
                assert_eq!(detail, Detail::level(&STANDARD, height, level));
                assert_eq!(detail.lod, STANDARD.lods[level].for_height(height));
            }
        }
    }

    #[test]
    fn every_knob_coarsens_along_the_dial() {
        let mut before = Detail::at(&STANDARD, 20.0, 0.0);
        for step in 1..=96 {
            let detail = Detail::at(&STANDARD, 20.0, f64::from(step) / 16.0);
            let (a, b) = (before.lod, detail.lod);
            assert!(b.min_radius >= a.min_radius, "{step}: {b:?} after {a:?}");
            assert!(b.ring_edge >= a.ring_edge, "{step}: {b:?} after {a:?}");
            assert!(b.max_sides <= a.max_sides, "{step}: {b:?} after {a:?}");
            assert!(b.max_sides >= b.min_sides && b.min_sides >= 3);
            assert!(b.bend >= a.bend, "{step}: {b:?} after {a:?}");
            assert!(b.cluster >= a.cluster, "{step}: {b:?} after {a:?}");
            assert!(detail.flat >= before.flat, "{step}");
            assert!(detail.solid_detail <= before.solid_detail, "{step}");
            assert!(detail.solid_share <= before.solid_share, "{step}");
            if detail.level == before.level {
                assert!(detail.spine_cards <= before.spine_cards, "{step}");
            }
            assert!(detail.level >= before.level);
            before = detail;
        }
        assert_eq!(before.level, 3);
        assert!(before.lod.min_radius > 0.5 && before.lod.cluster > 10.0);
        assert_eq!(before.lod.max_sides, 3);
        // Solids grow coarser over the first half of the way to the next
        // level, then fewer, and stop there.
        let at = |dial| {
            let detail = Detail::at(&STANDARD, 20.0, dial);
            (detail.solid_detail, detail.solid_share)
        };
        let close =
            |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-12 && (b - d).abs() < 1e-12;
        assert!(close(at(0.0), (1.0, 1.0)));
        assert!(close(at(0.25), (0.25, 1.0)));
        assert!(close(at(0.5), (SOLID_SHARE, 1.0)));
        assert!(close(at(0.75), (SOLID_SHARE, 0.5)));
        assert!(close(at(1.0), (0.0, 0.0)));
    }

    #[test]
    fn bent_cards_go_flat_before_leaves_cluster() {
        // Standard's level 1 draws a card per organ, its level 2 clusters.
        let at = |dial| Detail::at(&STANDARD, 20.0, dial);
        assert!(at(1.0).flat.abs() < 1e-12 && at(1.0).lod.cluster.abs() < 1e-12);
        let quarter = at(1.25);
        assert!((quarter.flat - 0.5).abs() < 1e-12 && quarter.lod.cluster.abs() < 1e-12);
        let three = at(1.75);
        assert!((three.flat - 1.0).abs() < 1e-12);
        assert!((three.lod.cluster - 0.5 * STANDARD.lods[2].cluster).abs() < 1e-12);
        assert!((at(2.0).flat - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_search_finds_where_a_falling_count_fits() {
        // A count that falls along the dial, with a step at 1 as when part
        // meshes become cards.
        let count = |dial: f64| {
            if dial < 1.0 {
                1000.0 - 100.0 * dial
            } else {
                400.0 * crate::math::pow(0.3, dial - 1.0)
            }
        };
        let search = |most: f64| {
            let mut tries = 0;
            let found = most_detail(0.0, |dial| {
                tries += 1;
                Ok::<_, ()>(count(dial) / most)
            })
            .unwrap();
            (found, tries)
        };
        assert_eq!(search(2000.0), ((0.0, true), 1));
        // Before the step at 1, from over it.
        let ((dial, fits), tries) = search(950.0);
        assert!(fits && dial >= 0.5 && count(dial) <= 950.0, "{dial}");
        assert!(dial - 0.5 <= STEP || count(dial) >= FULL * 950.0, "{dial}");
        assert!(tries <= 2 + NARROWINGS, "{tries}");
        // Past it, close to the budget in few tries.
        let ((dial, fits), tries) = search(150.0);
        assert!(
            fits && count(dial) <= 150.0 && count(dial) >= FULL * 150.0,
            "{dial}"
        );
        assert!(tries <= 6, "{tries}");
        assert_eq!(search(0.01).0, (DIAL_MAX, false));
    }
}
