//! A crown's outline grown as a front (crown form, round 3f): Duchemin,
//! Eloy, Badel and Moulia (2018), "Tree crowns grow into self-similar
//! shapes controlled by gravity and light sensing", *J. R. Soc. Interface*
//! 15: 20170976.
//!
//! The crown's surface, seen in a vertical section, is a closed curve of
//! points, each moving at the light it receives from an overcast sky
//! (weight $`(1 + 2\sin e)/3`$ at elevation $`e`$, times the cosine to its
//! outward normal $`\mathbf n`$) past the crown itself and the plant's
//! surroundings, $`\psi`$ as a share of the open sky's, along
//!
//! ```math
//! \mathbf U = \psi\,(\mathbf n + \alpha_g \mathbf v + \alpha_p \boldsymbol\ell)
//! ```
//!
//! with $`\mathbf v`$ up and $`\boldsymbol\ell`$ the mean direction of the
//! light it receives. The paper adds a little smoothing by curvature;
//! resampling the curve at even spacing each step stands in for it here.
//! The front has no length of its own, so it grows into one
//! outline whatever it starts from, set by the lineage's gravitropism
//! $`\alpha_g`$ and phototropism $`\alpha_p`$ alone: a downy birch's
//! (0.93, 0.27) narrow and pointed, a chestnut-leaved oak's (−1.0, 0.35)
//! broad and flat. Under a stand's or a neighbour's shade
//! ([`crate::conditions::Surroundings::transmission`]) less light comes
//! from the side, and the same lineage grows a narrower crown, wider toward
//! open sky. The plant grows its front in [`SECTIONS`] vertical sections
//! round its stem, each under the sky it sees, and keeps each half
//! section's radius, as a share of the crown's depth, at [`ROWS`] heights.

use crate::math::{self, Vec3};

/// Vertical sections a front is grown in, each giving two half sections:
/// the outline's radius is known every 45° round the stem.
pub const SECTIONS: usize = 4;

/// Heights, from the crown's foot to its top, at which a half section's
/// radius is kept.
pub const ROWS: usize = 32;

/// Points on the front.
const POINTS: usize = 72;

/// Directions of the sky's half circle a point is lit from.
const DIRECTIONS: usize = 37;

/// Steps the front grows, and the time each takes: long enough for the
/// outline to forget the small circle it starts from.
const STEPS: usize = 400;
const DT: f64 = 0.02;

/// A crown's outline: each half section's radius, as a share of the
/// crown's depth, from its foot to its top.
#[derive(Debug, Clone, PartialEq)]
pub struct Front {
    /// Half section `k` faces azimuth `k · 45°` round the stem
    /// (`math::atan2(z, x)`).
    halves: Vec<[f64; ROWS + 1]>,
}

impl Front {
    /// The front of a lineage with gravitropism `gravitropism` and
    /// phototropism `phototropism`, for a plant `height` metres tall whose
    /// sky along a direction from a point is `sky(point, direction)` (1 in
    /// the open). With `open`, every direction is lit alike and one section
    /// serves all.
    #[must_use]
    pub fn grow(
        gravitropism: f64,
        phototropism: f64,
        height: f64,
        open: bool,
        sky: &dyn Fn(Vec3, Vec3) -> f64,
    ) -> Self {
        let mut halves = vec![[0.0; ROWS + 1]; 2 * SECTIONS];
        if open {
            let outline = grow_section(gravitropism, phototropism, &|_, _| 1.0);
            let (right, left) = profiles(&outline);
            for half in &mut halves {
                for row in 0..=ROWS {
                    half[row] = f64::midpoint(right[row], left[row]);
                }
            }
            return Self { halves };
        }
        for k in 0..SECTIONS {
            #[allow(clippy::cast_precision_loss)]
            let azimuth = math::PI * k as f64 / SECTIONS as f64;
            let (ux, uz) = (math::cos(azimuth), math::sin(azimuth));
            let outline = grow_section(
                gravitropism,
                phototropism,
                &|(x, y): (f64, f64), (dx, dy): (f64, f64)| {
                    sky(
                        Vec3::new(x * ux * height, y * height, x * uz * height),
                        Vec3::new(dx * ux, dy, dx * uz),
                    )
                },
            );
            let (right, left) = profiles(&outline);
            halves[k] = right;
            halves[k + SECTIONS] = left;
        }
        Self { halves }
    }

    /// The outline's radius, as a share of the crown's depth, at relative
    /// height `h` in `[0, 1]` toward azimuth `theta`, blended between the
    /// half sections either side.
    #[must_use]
    pub fn radius(&self, h: f64, theta: f64) -> f64 {
        let count = self.halves.len();
        #[allow(clippy::cast_precision_loss)]
        let at = theta.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * count as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first = (at.floor() as usize) % count;
        let blend = at - at.floor();
        let second = (first + 1) % count;
        math::lerp(
            row_radius(&self.halves[first], h),
            row_radius(&self.halves[second], h),
            blend,
        )
    }

    /// The least and most radius over relative heights `low` to `high` and
    /// every azimuth, with the rows either side, so every radius between
    /// lies within them.
    #[must_use]
    pub fn bounds(&self, low: f64, high: f64) -> (f64, f64) {
        #[allow(clippy::cast_precision_loss)]
        let rows = ROWS as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (first, last) = (
            ((low.clamp(0.0, 1.0) * rows).floor() as usize).min(ROWS),
            ((high.clamp(0.0, 1.0) * rows).ceil() as usize).min(ROWS),
        );
        let mut narrowest = f64::INFINITY;
        let mut broadest: f64 = 0.0;
        for half in &self.halves {
            for value in &half[first..=last] {
                broadest = broadest.max(*value);
            }
            narrowest = narrowest
                .min(row_radius(half, low))
                .min(row_radius(half, high));
        }
        (narrowest.min(broadest), broadest)
    }

    /// How far this front falls short of `open`, the same lineage's
    /// open-grown one, at each height and azimuth: the share of its radius
    /// this one reaches there (1 where the open one has none), at most 1:
    /// shade never grows a crown past its open-grown outline.
    #[must_use]
    pub fn shaded_by(&self, open: &Self) -> Self {
        let halves = self
            .halves
            .iter()
            .zip(&open.halves)
            .map(|(here, there)| {
                let mut share = [1.0; ROWS + 1];
                for row in 0..=ROWS {
                    if there[row] > 1e-6 {
                        share[row] = (here[row] / there[row]).min(1.0);
                    }
                }
                share
            })
            .collect();
        Self { halves }
    }

    /// The widest the outline reaches, as a share of the crown's depth.
    #[must_use]
    pub fn widest(&self) -> f64 {
        self.halves
            .iter()
            .flat_map(|half| half.iter())
            .fold(0.0, |most: f64, value| most.max(*value))
    }
}

/// A half section's radius at relative height `h`, between its rows.
fn row_radius(half: &[f64; ROWS + 1], h: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let at = h.clamp(0.0, 1.0) * ROWS as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let row = (at.floor() as usize).min(ROWS - 1);
    #[allow(clippy::cast_precision_loss)]
    math::lerp(half[row], half[row + 1], at - row as f64)
}

/// Grow one section's front, its points counter-clockwise in the section's
/// plane (`x` out along its azimuth, `y` up), lit along each direction by
/// `sky((x, y), direction)` with points scaled so the front's top is 1.
fn grow_section(
    gravitropism: f64,
    phototropism: f64,
    sky: &dyn Fn((f64, f64), (f64, f64)) -> f64,
) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = (0..POINTS)
        .map(|k| {
            #[allow(clippy::cast_precision_loss)]
            let angle = std::f64::consts::TAU * k as f64 / POINTS as f64;
            [0.3 * math::cos(angle), 1.0 + 0.3 * math::sin(angle)]
        })
        .collect();
    // Each direction of the sky's half circle and its overcast weight,
    // (1 + 2 sin e) / 3 at elevation e, as a share of the whole sky's.
    let directions: Vec<([f64; 2], f64)> = (0..DIRECTIONS)
        .map(|j| {
            #[allow(clippy::cast_precision_loss)]
            let elevation = math::PI * (j as f64 + 0.5) / DIRECTIONS as f64;
            let sine = math::sin(elevation);
            ([math::cos(elevation), sine], (1.0 + 2.0 * sine) / 3.0)
        })
        .collect();
    let whole: f64 = directions.iter().map(|(_, weight)| weight).sum();
    let mut moves = vec![[0.0; 2]; POINTS];
    for _ in 0..STEPS {
        let top = points
            .iter()
            .fold(0.0_f64, |top, p| top.max(p[1]))
            .max(1e-9);
        let span = points.iter().fold(0.0_f64, |most, p| most.max(p[1]))
            - points
                .iter()
                .fold(f64::INFINITY, |least, p| least.min(p[1]));
        let lift = 1e-4 * span.max(1e-6);
        for i in 0..POINTS {
            let before = points[(i + POINTS - 1) % POINTS];
            let here = points[i];
            let after = points[(i + 1) % POINTS];
            let tangent = [after[0] - before[0], after[1] - before[1]];
            let length = math::sqrt(tangent[0] * tangent[0] + tangent[1] * tangent[1]).max(1e-12);
            // Outward for a counter-clockwise curve.
            let normal = [tangent[1] / length, -tangent[0] / length];
            let origin = [here[0] + normal[0] * lift, here[1] + normal[1] * lift];
            let mut light = 0.0;
            let mut mean = [0.0; 2];
            for (direction, overcast) in &directions {
                let facing = direction[0] * normal[0] + direction[1] * normal[1];
                if facing <= 0.0 || blocked(&points, origin, *direction) {
                    continue;
                }
                let weight = overcast * facing / whole
                    * sky(
                        (origin[0] / top, origin[1] / top),
                        (direction[0], direction[1]),
                    );
                light += weight;
                mean[0] += weight * direction[0];
                mean[1] += weight * direction[1];
            }
            let norm = math::sqrt(mean[0] * mean[0] + mean[1] * mean[1]);
            let mean = if norm > 0.0 {
                [mean[0] / norm, mean[1] / norm]
            } else {
                [0.0; 2]
            };
            let heading = [
                normal[0] + phototropism * mean[0],
                normal[1] + gravitropism + phototropism * mean[1],
            ];
            moves[i] = [light * heading[0], light * heading[1]];
        }
        for (point, step) in points.iter_mut().zip(&moves) {
            point[0] += DT * step[0];
            point[1] = (point[1] + DT * step[1]).max(0.0);
        }
        points = resample(&points);
    }
    points
}

/// Whether a ray from `origin` along `direction` meets the front.
fn blocked(points: &[[f64; 2]], origin: [f64; 2], direction: [f64; 2]) -> bool {
    let count = points.len();
    (0..count).any(|k| {
        let a = points[k];
        let b = points[(k + 1) % count];
        let edge = [b[0] - a[0], b[1] - a[1]];
        let denominator = direction[0] * edge[1] - direction[1] * edge[0];
        if denominator.abs() < 1e-15 {
            return false;
        }
        let offset = [a[0] - origin[0], a[1] - origin[1]];
        let t = (offset[0] * edge[1] - offset[1] * edge[0]) / denominator;
        let u = (offset[0] * direction[1] - offset[1] * direction[0]) / denominator;
        t > 1e-9 && (0.0..=1.0).contains(&u)
    })
}

/// The closed curve through `points` resampled at [`POINTS`] points evenly
/// spaced along it, from the first.
fn resample(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let count = points.len();
    let mut along = Vec::with_capacity(count + 1);
    along.push(0.0);
    for k in 0..count {
        let a = points[k];
        let b = points[(k + 1) % count];
        let step = math::sqrt((b[0] - a[0]) * (b[0] - a[0]) + (b[1] - a[1]) * (b[1] - a[1]));
        along.push(along[k] + step);
    }
    let total = along[count];
    let mut out = Vec::with_capacity(POINTS);
    let mut edge = 0;
    for k in 0..POINTS {
        #[allow(clippy::cast_precision_loss)]
        let target = total * k as f64 / POINTS as f64;
        while edge + 1 < count && along[edge + 1] < target {
            edge += 1;
        }
        let a = points[edge];
        let b = points[(edge + 1) % count];
        let span = along[edge + 1] - along[edge];
        let t = if span > 0.0 {
            (target - along[edge]) / span
        } else {
            0.0
        };
        out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
    }
    out
}

/// The outline's radius out along the section's azimuth (`x` > 0) and
/// back along the opposite one, as shares of its depth, at [`ROWS`] + 1
/// heights from its lowest point to its highest: where a level line
/// crosses it farthest from the stem on each side.
fn profiles(points: &[[f64; 2]]) -> ([f64; ROWS + 1], [f64; ROWS + 1]) {
    let low = points
        .iter()
        .fold(f64::INFINITY, |least, p| least.min(p[1]));
    let high = points
        .iter()
        .fold(f64::NEG_INFINITY, |most, p| most.max(p[1]));
    let depth = (high - low).max(1e-12);
    let mut right = [0.0; ROWS + 1];
    let mut left = [0.0; ROWS + 1];
    let count = points.len();
    for row in 0..=ROWS {
        #[allow(clippy::cast_precision_loss)]
        let level =
            (low + depth * row as f64 / ROWS as f64).clamp(low + 1e-9 * depth, high - 1e-9 * depth);
        let (mut out, mut back) = (0.0_f64, 0.0_f64);
        for k in 0..count {
            let a = points[k];
            let b = points[(k + 1) % count];
            if (a[1] - level) * (b[1] - level) > 0.0 || (a[1] - b[1]).abs() < 1e-12 {
                continue;
            }
            let x = a[0] + (level - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            out = out.max(x);
            back = back.max(-x);
        }
        right[row] = out / depth;
        left[row] = back / depth;
    }
    (right, left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_birch_grows_narrow_and_an_oak_broad() {
        let birch = Front::grow(0.93, 0.27, 10.0, true, &|_, _| 1.0);
        let oak = Front::grow(-1.0, 0.35, 10.0, true, &|_, _| 1.0);
        // As wide as deep: about 0.24 and 1.05 (the prototype's 0.23 and
        // 1.11 at a finer resolution).
        assert!((0.2..0.28).contains(&birch.widest()), "{}", birch.widest());
        assert!((0.95..1.2).contains(&oak.widest()), "{}", oak.widest());
        // Under a sky closed below 50° on every side, as in a stand, the
        // birch grows narrower.
        let stand = Front::grow(0.93, 0.27, 10.0, false, &|_, direction: Vec3| {
            if direction.y < math::sin(math::radians(50.0)) {
                0.15
            } else {
                1.0
            }
        });
        assert!(stand.widest() < 0.8 * birch.widest(), "{}", stand.widest());
    }
}
