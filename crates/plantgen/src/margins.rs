//! Leaf margins grown with their veins (spec rework L3; after Runions,
//! Tsiantis and Prusinkiewicz 2017, "A common developmental program can
//! produce diverse leaf shapes").
//!
//! A blade's teeth and lobes are laid down while it grows. Where auxin
//! converges on the margin (a convergence point) a vein starts and the
//! margin grows out along it, so each tooth or lobe has a vein at its tip,
//! points the way that vein runs, and is as large as the growth it shared
//! in after it formed. New points form wherever the margin has grown room
//! for one, the way primordia form where the old ones leave room
//! (Hofmeister's rule).
//!
//! One side of a blade is grown here along its length, `t` from the base
//! (0) to the tip (1). The blade grows from a primordium $`G`$ times
//! smaller over [`STEPS`] steps. Tissue matures from the tip down, as in
//! most leaves: at step $`n`$ of $`N`$ only the share
//! $`f(n) = 1 - 0.85\,n / N`$ of the blade nearest its base still grows,
//! at the rate $`r`$ a step, so a point at $`x \le f L`$ of a blade $`L`$
//! long moves out by $`r x`$ and one beyond it by $`r f L`$. Each step,
//! wherever two neighbouring points (or a point and an end of the toothed
//! zone) lie farther apart along the margin than the spacing $`d`$, a new
//! point forms between them, a little off the middle. A point's tooth
//! grows while its tissue does, so its size is the number of steps it
//! spent growing; the oldest teeth in the middle of the blade are the
//! largest, those near the tip stop early and those near the base form
//! late. The spacing $`d`$ is set so that the margin ends with the look's
//! number of teeth.
//!
//! A tooth points the way its vein runs into it ([`Margin::lean`]): a vein
//! meeting the margin square makes a symmetric tooth (dentate), one that
//! runs along it toward the tip a saw tooth (serrate). Its sinus is that
//! much nearer the tooth below it.
//!
//! The growth uses no random generator: every draw is a hash of the seed,
//! the step and the gap, so a look's margin is the same every time.

use crate::math;
use crate::rng::{combine, unit};

/// Steps of a blade's growth.
const STEPS: u32 = 64;
/// Relative growth a step of tissue that still grows: with the front
/// moving down, the blade grows about 500-fold.
const RATE: f64 = 0.17;
/// How far down the blade the maturation front moves.
const FRONT: f64 = 0.85;
/// How far off the middle of its gap a new point may form, a share of
/// the gap either way.
const JITTER: f64 = 0.18;
/// Samples of the outline for its arc length.
const SAMPLES: usize = 256;

/// One tooth or lobe of a margin: its tip's place along the blade, its
/// growth as a share of the largest tooth's (0 for one formed at the last
/// step, 1), the step it formed at and how far it leans toward the
/// blade's tip (0 square to the margin, 1 along it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Apex {
    pub t: f64,
    pub size: f64,
    pub born: u32,
    pub lean: f64,
}

/// One side of a blade's margin, grown (see the module): its teeth or
/// lobes from base to tip, within the zone they form in.
#[derive(Debug, Clone, PartialEq)]
pub struct Margin {
    pub zone: (f64, f64),
    pub apices: Vec<Apex>,
}

/// A point while the blade grows: where it is, blade lengths from the
/// base, the step it formed at and the steps it has grown since.
#[derive(Debug, Clone, Copy)]
struct Point {
    x: f64,
    born: u32,
    grown: u32,
}

/// Arc length along an outline: the length of the margin from the base
/// to each sample of `t`, for the half-width `half(t)` in blade lengths.
fn arc_table(half: &dyn Fn(f64) -> f64) -> Vec<f64> {
    let step = 1.0 / to_f64(SAMPLES);
    let mut table = Vec::with_capacity(SAMPLES + 1);
    table.push(0.0);
    let mut previous = half(0.0);
    let mut total = 0.0;
    for i in 1..=SAMPLES {
        let width = half(to_f64(i) * step);
        total += math::sqrt(step * step + (width - previous) * (width - previous));
        table.push(total);
        previous = width;
    }
    table
}

fn arc_at(table: &[f64], t: f64) -> f64 {
    let at = t.clamp(0.0, 1.0) * to_f64(SAMPLES);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (at.floor() as usize).min(SAMPLES - 1);
    let s = at - to_f64(i);
    table[i] + (table[i + 1] - table[i]) * s
}

fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// Grow one side's points for the spacing `spacing` (see the module),
/// with the zone's ends as points of the blade too, so that the points
/// end in the zone; their places along the final blade.
fn grow_points(zone: (f64, f64), table: &[f64], spacing: f64, seed: u64) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::new();
    let mut length = 1.0;
    let mut ends = zone;
    let share = zone.1 - zone.0;
    // Where a point of the blade lies on the final one.
    let place = |x: f64, ends: (f64, f64)| zone.0 + (x - ends.0) / (ends.1 - ends.0) * share;
    for step in 0..STEPS {
        let front = (1.0 - FRONT * f64::from(step) / f64::from(STEPS)) * length;
        for point in &mut points {
            if point.x <= front {
                point.grown += 1;
            }
            point.x += RATE * point.x.min(front);
        }
        ends.0 += RATE * ends.0.min(front);
        ends.1 += RATE * ends.1.min(front);
        length += RATE * front;
        // New points wherever the margin has room.
        let scale = (ends.1 - ends.0) / share;
        let mut gaps: Vec<f64> = Vec::with_capacity(points.len() + 2);
        gaps.push(ends.0);
        gaps.extend(points.iter().map(|point| point.x));
        gaps.push(ends.1);
        let mut born = Vec::new();
        for (gap, pair) in gaps.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let room = arc_at(table, place(b, ends)) - arc_at(table, place(a, ends));
            if room * scale > spacing {
                let draw = unit(combine(seed, u64::from(step) << 20 | gap as u64));
                born.push(Point {
                    x: a + (b - a) * (0.5 + JITTER * (2.0 * draw - 1.0)),
                    born: step,
                    grown: 0,
                });
            }
        }
        if !born.is_empty() {
            points.extend(born);
            points.sort_by(|p, q| p.x.total_cmp(&q.x));
        }
    }
    for point in &mut points {
        point.x = place(point.x, ends);
    }
    points
}

/// How a margin's teeth are drawn: how `round` they are (0 saw teeth to 1
/// rounded lobes), how far a leaning tooth's sinus moves toward the tooth
/// below it (`skew`, a share of the gap between their tips for a tooth
/// along the margin), and the size of the tooth that grew least, a share
/// of the largest's (`least`; a tooth's size runs from it to 1 with its
/// growth).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawn {
    pub round: f64,
    pub skew: f64,
    pub least: f64,
}

impl Margin {
    /// Grow one side's margin to `count` teeth or lobes in `zone` (from
    /// and to, along the blade) of a blade whose untoothed half-width is
    /// `half(t)` blade lengths, drawing from `seed`. The spacing is found
    /// by bisection; where no spacing gives exactly `count` (two gaps
    /// filling at one step), the nearest count is kept.
    #[must_use]
    pub fn grow(count: u32, zone: (f64, f64), half: &dyn Fn(f64) -> f64, seed: u64) -> Self {
        let mut margin = Self {
            zone,
            apices: Vec::new(),
        };
        if count == 0 || zone.1 <= zone.0 {
            return margin;
        }
        let table = arc_table(half);
        let target = count as usize;
        // Spacings from one that fits a point every few samples of the
        // grown zone to one that never fits; the zone grows less than the
        // blade, at most by the blade's growth.
        let zone_arc = arc_at(&table, zone.1) - arc_at(&table, zone.0);
        let growth = math::exp(RATE * f64::from(STEPS));
        let (mut low, mut high) = (
            math::ln(zone_arc / 400.0),
            math::ln(zone_arc * growth * 2.0),
        );
        let mut best: Option<(usize, Vec<Point>)> = None;
        for _ in 0..60 {
            let middle = f64::midpoint(low, high);
            let points = grow_points(zone, &table, math::exp(middle), seed);
            let found = points.len();
            let miss = found.abs_diff(target);
            if best
                .as_ref()
                .is_none_or(|(old, _)| miss < old.abs_diff(target))
            {
                best = Some((found, points));
            }
            match found.cmp(&target) {
                std::cmp::Ordering::Greater => low = middle,
                std::cmp::Ordering::Less => high = middle,
                std::cmp::Ordering::Equal => break,
            }
        }
        let Some((_, points)) = best else {
            return margin;
        };
        let most = points
            .iter()
            .map(|point| point.grown)
            .max()
            .unwrap_or(0)
            .max(1);
        margin.apices = points
            .iter()
            .map(|point| Apex {
                t: point.x,
                size: f64::from(point.grown) / f64::from(most),
                born: point.born,
                lean: 0.0,
            })
            .collect();
        margin
    }

    /// Set how far each tooth leans toward the blade's tip, 0 to 1, from
    /// the heading of its vein: `leans` holds one per apex, in order. Each
    /// is eased toward its neighbours' (by 1, 2, 1), since a margin's teeth
    /// grow alike.
    pub fn lean(&mut self, leans: &[f64]) {
        let count = self.apices.len().min(leans.len());
        let at = |i: usize| leans[i].clamp(0.0, 1.0);
        for i in 0..count {
            let (before, after) = (at(i.saturating_sub(1)), at((i + 1).min(count - 1)));
            self.apices[i].lean = 0.25 * before + 0.5 * at(i) + 0.25 * after;
        }
    }

    /// How far the margin falls back from its outline at `t`, a share of
    /// the depth of its sinuses: 1 in a sinus, and at a tooth's tip less
    /// its size (see [`Drawn`]), so that the largest teeth reach the
    /// outline and smaller ones stand short of it. Between two tips the
    /// sinus lies nearer the lower one the more the teeth lean, less so
    /// the more round they are; `flank` gives how far a tooth falls back
    /// along its flank, from 0 at its tip to 1 at the sinus, for the share
    /// of the way there. The ends of the zone stand for teeth that reach
    /// the outline, and outside it the margin is its outline.
    #[must_use]
    pub fn fall(&self, t: f64, drawn: Drawn, flank: impl Fn(f64) -> f64) -> f64 {
        let (Some(first), Some(last)) = (self.apices.first(), self.apices.last()) else {
            return 0.0;
        };
        if t <= self.zone.0 || t >= self.zone.1 {
            return 0.0;
        }
        let above = self.apices.partition_point(|apex| apex.t <= t);
        let below = |i: usize| self.apices.get(i.wrapping_sub(1));
        let (low, high) = match (below(above), self.apices.get(above)) {
            (Some(low), Some(high)) => (*low, *high),
            (None, Some(_)) => (
                Apex {
                    t: self.zone.0,
                    size: 1.0,
                    ..*first
                },
                *first,
            ),
            (Some(_), None) => (
                *last,
                Apex {
                    t: self.zone.1,
                    size: 1.0,
                    ..*last
                },
            ),
            (None, None) => return 0.0,
        };
        let span = high.t - low.t;
        if span <= 1e-9 {
            return 0.0;
        }
        let lean = f64::midpoint(low.lean, high.lean);
        let sinus =
            (0.5 - drawn.skew * lean * (1.0 - drawn.round.clamp(0.0, 1.0))).clamp(0.04, 0.5);
        let u = (t - low.t) / span;
        let (tooth, share) = if u < sinus {
            (low, u / sinus)
        } else {
            (high, (1.0 - u) / (1.0 - sinus))
        };
        let size = drawn.least + (1.0 - drawn.least) * tooth.size;
        1.0 - size * (1.0 - flank(share.clamp(0.0, 1.0)))
    }
}

/// The share of the way along a tooth's flank from its tip (0) to its
/// sinus (1) to fall back by, for teeth as `round` as given: saw teeth
/// stay near the outline and fall steeply into the sinus, rounded lobes
/// swell and narrow evenly. The templates' toothed leaves draw with it.
#[must_use]
pub fn tooth_flank(share: f64, round: f64) -> f64 {
    let saw = math::pow(share, 1.5);
    if round <= 0.0 {
        return saw;
    }
    let lobe = 1.0 - math::pow(math::cos(0.5 * math::PI * share), 0.6);
    saw + (lobe - saw) * round
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ovate(t: f64) -> f64 {
        0.3 * math::sin(math::PI * t.clamp(0.0, 1.0))
    }

    #[test]
    fn a_margin_grows_the_teeth_its_look_asks_for() {
        for count in [1, 2, 3, 4, 6, 9, 14, 20, 30, 45, 60] {
            for seed in 0..6 {
                let margin = Margin::grow(count, (0.08, 0.94), &ovate, seed);
                let found = u32::try_from(margin.apices.len()).unwrap();
                assert!(
                    found.abs_diff(count) <= 1,
                    "{count} teeth, seed {seed}: {found}"
                );
                assert!(margin.apices.windows(2).all(|pair| pair[0].t < pair[1].t));
                assert!(
                    margin
                        .apices
                        .iter()
                        .all(|apex| (0.08..0.94).contains(&apex.t))
                );
                assert!(
                    margin
                        .apices
                        .iter()
                        .all(|apex| (0.0..=1.0).contains(&apex.size))
                );
            }
        }
        assert!(Margin::grow(0, (0.08, 0.94), &ovate, 1).apices.is_empty());
        assert_eq!(
            Margin::grow(9, (0.08, 0.94), &ovate, 3),
            Margin::grow(9, (0.08, 0.94), &ovate, 3)
        );
    }

    #[test]
    fn teeth_form_from_the_tip_down_and_the_middle_ones_grow_largest() {
        let margin = Margin::grow(12, (0.06, 0.94), &ovate, 7);
        let apices = &margin.apices;
        // The lowest third formed last.
        let third = apices.len() / 3;
        #[allow(clippy::cast_precision_loss)]
        let mean = |range: &[Apex]| {
            range.iter().map(|apex| f64::from(apex.born)).sum::<f64>() / range.len() as f64
        };
        assert!(mean(&apices[..third]) > mean(&apices[apices.len() - third..]));
        // The largest tooth is neither the lowest nor the highest.
        let largest = apices
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.size.total_cmp(&b.1.size))
            .unwrap()
            .0;
        assert!(largest > 0 && largest < apices.len() - 1, "{apices:?}");
        // Sizes differ.
        assert!(apices.iter().any(|apex| apex.size < 0.8));
    }

    const SAW: Drawn = Drawn {
        round: 0.0,
        skew: 0.45,
        least: 0.5,
    };

    #[test]
    fn the_margin_falls_back_between_tips_and_leans_with_its_veins() {
        let mut margin = Margin::grow(6, (0.1, 0.9), &ovate, 2);
        for apex in margin.apices.clone() {
            let fall = margin.fall(apex.t, SAW, |share| tooth_flank(share, 0.0));
            assert!((fall - (1.0 - SAW.least - (1.0 - SAW.least) * apex.size)).abs() < 1e-9);
        }
        assert!(margin.fall(0.05, SAW, |share| share).abs() < 1e-12);
        assert!(margin.fall(0.95, SAW, |share| share).abs() < 1e-12);
        let (a, b) = (margin.apices[2], margin.apices[3]);
        let deepest = |margin: &Margin| {
            (1..100)
                .map(|i| a.t + (b.t - a.t) * f64::from(i) / 100.0)
                .max_by(|p, q| {
                    margin
                        .fall(*p, SAW, |share| share)
                        .total_cmp(&margin.fall(*q, SAW, |share| share))
                })
                .unwrap()
        };
        // Square to the margin the sinus is midway; leaning, near the
        // lower tooth.
        let square = (deepest(&margin) - a.t) / (b.t - a.t);
        assert!((square - 0.5).abs() < 0.02, "{square}");
        let leans = vec![0.8; margin.apices.len()];
        margin.lean(&leans);
        let leaning = (deepest(&margin) - a.t) / (b.t - a.t);
        assert!(leaning < 0.15, "{leaning}");
        let depth = margin.fall(a.t + (b.t - a.t) * leaning, SAW, |share| share);
        assert!(depth > 0.97, "{depth}");
    }
}
