//! Measures of a grown plant's form (growth plan G1): the crown's
//! outline, its width against the stem, and the size of its leaves.
//!
//! These measure what a species' references (`allometry`) do not: a crown
//! that grows into the same ball whatever the species, a stem too thin for
//! its crown, leaves too small for the tree. Nothing here changes growth.
//!
//! The crown is the plant's living organs. Its height runs from the lowest
//! organ $`y_0`$ to the highest $`y_1`$, cut into [`BANDS`] equal bands; a
//! band's radius $`r_k`$ is the [`OUTLINE_SHARE`] quantile of its organs'
//! distances from the stem axis, so a few far organs do not set it. With
//! $`R = \max_k r_k`$ and the band's middle at $`h_k \in (0, 1)`$ up the
//! crown, the outline's departure from an ellipse of the same height and
//! width is
//!
//! ```math
//! d = \sqrt{\frac{1}{K} \sum_k \left(\frac{r_k}{R} - \sqrt{1 - (2 h_k - 1)^2}\right)^2}
//! ```
//!
//! over the $`K`$ bands that hold organs: 0 for an orb, about 0.3 for a
//! cone or a vase. Round the stem, the widest band's organs fall in
//! [`SECTORS`] sectors of azimuth; the sectors' radii (the same quantile)
//! vary by their coefficient of variation, the crown's lobing: 0 for a
//! crown round as a coin seen from above.
//!
//! **The crown from the side** (crown form research, 2026-10-12). The
//! living leaves, each a disc of its area turned to the view, and the wood
//! are drawn onto a grid of [`SIDE_CELL`] cells (finer for a crown under 4
//! m) seen along each horizontal axis, keeping the rows above the lowest
//! [`CROWN_FOOT`] of the leaves. The crown's outline is that drawing
//! closed by a square of [`OUTLINE_CLOSING`] of its width and with its
//! holes filled. Two measures follow, each the mean of the two views:
//!
//! - the **outline's fit** to the ellipse of the same area and second
//!   moments, their intersection over their union: 1 for an elliptical
//!   outline. Every broadleaf crown grown inside its envelope fits at 0.89
//!   to 0.94, photographs of open-grown Oregon white oaks at 0.82 to 0.84;
//! - the share of the outline the sky shows through (**see-through**).

use crate::graph::{OrganType, PlantGraph};
use crate::lsys::program::OrganKind;
use crate::math;

/// Horizontal bands the crown is cut into.
pub const BANDS: usize = 10;

/// Sectors of azimuth round the stem for the lobing.
pub const SECTORS: usize = 8;

/// Quantile of organ distances that makes a band's or sector's radius.
pub const OUTLINE_SHARE: f64 = 0.95;

/// A side view's cell, m, for a crown 4 m wide or more.
pub const SIDE_CELL: f64 = 0.04;

/// Share of the leaves, lowest first, below the crown a side view keeps.
pub const CROWN_FOOT: f64 = 0.02;

/// Half the side of the square that closes a side view's outline, as a
/// share of the crown's width.
pub const OUTLINE_CLOSING: f64 = 0.03;

/// Leaves a crown needs for its side views to be measured.
pub const SIDE_LEAVES: usize = 50;

/// The form of one grown plant at one age.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Form {
    /// Height of the plant, m.
    pub height: f64,
    /// Lowest and highest living organ, m.
    pub crown_low: f64,
    pub crown_high: f64,
    /// Widest band's radius, m ($`R`$).
    pub crown_radius: f64,
    /// Crown width over crown depth: $`2R / (y_1 - y_0)`$.
    pub width_to_depth: f64,
    /// Where the widest band lies, as a share of the crown's depth from
    /// its foot.
    pub widest_at: f64,
    /// Departure from an ellipse, $`d`$ (see the module).
    pub ellipse_departure: f64,
    /// Coefficient of variation of the sectors' radii round the widest
    /// band.
    pub lobing: f64,
    /// Diameter at breast height, m, where the plant has a stem there.
    pub dbh: Option<f64>,
    /// Crown width over dbh, where there is a dbh.
    pub width_to_dbh: Option<f64>,
    /// Median card length of the leaves (organs of kind `leaf`), m.
    pub leaf_length: Option<f64>,
    /// Living leaves.
    pub leaves: usize,
    /// The side outline's fit to its ellipse, from 0 to 1 (see the
    /// module), with at least [`SIDE_LEAVES`] leaves.
    pub outline_fit: Option<f64>,
    /// The share of the side outline the sky shows through.
    pub see_through: Option<f64>,
}

/// The form of `graph`, or `None` with fewer than [`BANDS`] living
/// organs.
// Band and sector counts are small; ages and shares are in [0, 1].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[must_use]
pub fn measure(graph: &PlantGraph, types: &[OrganType]) -> Option<Form> {
    let organs: Vec<_> = graph
        .organs
        .iter()
        .filter(|organ| organ.shed.is_none())
        .collect();
    if organs.len() < BANDS {
        return None;
    }
    let low = organs
        .iter()
        .map(|organ| organ.position.y)
        .fold(f64::INFINITY, f64::min);
    let high = organs
        .iter()
        .map(|organ| organ.position.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let depth = (high - low).max(1e-6);
    let radial = |x: f64, z: f64| math::sqrt(x * x + z * z);
    let mut bands: Vec<Vec<f64>> = vec![Vec::new(); BANDS];
    for organ in &organs {
        let share = ((organ.position.y - low) / depth).clamp(0.0, 1.0);
        let band = ((share * BANDS as f64) as usize).min(BANDS - 1);
        bands[band].push(radial(organ.position.x, organ.position.z));
    }
    let radii: Vec<Option<f64>> = bands.iter_mut().map(|band| quantile(band)).collect();
    let (widest, radius) = radii
        .iter()
        .enumerate()
        .filter_map(|(k, r)| r.map(|r| (k, r)))
        .fold(
            (0, 0.0),
            |best, (k, r)| if r > best.1 { (k, r) } else { best },
        );
    let radius = radius.max(1e-6);
    let mut sum = 0.0;
    let mut count = 0.0;
    for (k, r) in radii.iter().enumerate() {
        if let Some(r) = r {
            let h = (k as f64 + 0.5) / BANDS as f64;
            let ellipse = math::sqrt((1.0 - (2.0 * h - 1.0) * (2.0 * h - 1.0)).max(0.0));
            sum += (r / radius - ellipse).powi(2);
            count += 1.0;
        }
    }
    let mut sectors: Vec<Vec<f64>> = vec![Vec::new(); SECTORS];
    for organ in &organs {
        let share = ((organ.position.y - low) / depth).clamp(0.0, 1.0);
        if ((share * BANDS as f64) as usize).min(BANDS - 1) != widest {
            continue;
        }
        let angle = math::atan2(organ.position.z, organ.position.x) + std::f64::consts::PI;
        let sector = ((angle / std::f64::consts::TAU * SECTORS as f64) as usize).min(SECTORS - 1);
        sectors[sector].push(radial(organ.position.x, organ.position.z));
    }
    let sector_radii: Vec<f64> = sectors
        .iter_mut()
        .map(|s| quantile(s).unwrap_or(0.0))
        .collect();
    let mean = sector_radii.iter().sum::<f64>() / SECTORS as f64;
    let variance = sector_radii.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / SECTORS as f64;
    let dbh = graph.diameter_at_breast_height();
    let mut leaf_sizes: Vec<f64> = organs
        .iter()
        .filter(|organ| {
            types
                .get(usize::from(organ.organ))
                .is_some_and(|kind| kind.kind == OrganKind::Leaf)
        })
        .map(|organ| organ.size.abs())
        .collect();
    leaf_sizes.sort_by(f64::total_cmp);
    let side = side_views(graph, types);
    Some(Form {
        height: graph.height,
        crown_low: low,
        crown_high: high,
        crown_radius: radius,
        width_to_depth: 2.0 * radius / depth,
        widest_at: (widest as f64 + 0.5) / BANDS as f64,
        ellipse_departure: math::sqrt(sum / count),
        lobing: if mean > 0.0 {
            math::sqrt(variance) / mean
        } else {
            0.0
        },
        dbh,
        width_to_dbh: dbh.filter(|d| *d > 0.0).map(|d| 2.0 * radius / d),
        leaf_length: leaf_sizes.get(leaf_sizes.len() / 2).copied(),
        leaves: leaf_sizes.len(),
        outline_fit: side.map(|(fit, _)| fit),
        see_through: side.map(|(_, through)| through),
    })
}

/// A side view: cells in rows from the top, `true` where drawn.
struct Grid {
    width: usize,
    height: usize,
    cells: Vec<bool>,
}

impl Grid {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    fn count(&self) -> usize {
        self.cells.iter().filter(|&&cell| cell).count()
    }

    /// Fill the disc of `radius` cells round `(u, v)`, at least its cell.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn disc(&mut self, u: f64, v: f64, radius: f64) {
        let reach = radius.max(0.5);
        let span = |low: f64, high: f64, cells: usize| {
            let first = low.floor().max(0.0) as usize;
            let last = (high.floor().max(0.0) as usize).min(cells - 1);
            first..=last
        };
        for row in span(v - reach, v + reach, self.height) {
            for column in span(u - reach, u + reach, self.width) {
                let (du, dv) = (column as f64 + 0.5 - u, row as f64 + 0.5 - v);
                if du * du + dv * dv <= reach * reach {
                    self.cells[row * self.width + column] = true;
                }
            }
        }
    }

    /// Grown by `reach` cells in each direction (a square), or shrunk.
    fn spread(&self, reach: usize, grow: bool) -> Self {
        let pass = |cells: &[bool], width: usize, height: usize, along_rows: bool| {
            let mut out = vec![false; cells.len()];
            let (lines, length) = if along_rows {
                (height, width)
            } else {
                (width, height)
            };
            for line in 0..lines {
                let at = |i: usize| {
                    if along_rows {
                        line * width + i
                    } else {
                        i * width + line
                    }
                };
                for i in 0..length {
                    let (a, b) = (i.saturating_sub(reach), (i + reach).min(length - 1));
                    let mut window = (a..=b).map(|j| cells[at(j)]);
                    out[at(i)] = if grow {
                        window.any(|cell| cell)
                    } else {
                        i >= reach && i + reach < length && window.all(|cell| cell)
                    };
                }
            }
            out
        };
        let rows = pass(&self.cells, self.width, self.height, true);
        Self {
            width: self.width,
            height: self.height,
            cells: pass(&rows, self.width, self.height, false),
        }
    }

    /// With every hole (a cell the border cannot reach) filled.
    fn filled(&self) -> Self {
        let mut outside = vec![false; self.cells.len()];
        let mut stack = Vec::new();
        for row in 0..self.height {
            for column in 0..self.width {
                if (row == 0 || column == 0 || row + 1 == self.height || column + 1 == self.width)
                    && !self.cells[row * self.width + column]
                {
                    stack.push(row * self.width + column);
                }
            }
        }
        while let Some(at) = stack.pop() {
            if outside[at] || self.cells[at] {
                continue;
            }
            outside[at] = true;
            let (row, column) = (at / self.width, at % self.width);
            if row > 0 {
                stack.push(at - self.width);
            }
            if row + 1 < self.height {
                stack.push(at + self.width);
            }
            if column > 0 {
                stack.push(at - 1);
            }
            if column + 1 < self.width {
                stack.push(at + 1);
            }
        }
        Self {
            width: self.width,
            height: self.height,
            cells: outside.iter().map(|&out| !out).collect(),
        }
    }

    /// Intersection over union with the ellipse of the same area and
    /// second moments.
    #[allow(clippy::cast_precision_loss)]
    fn ellipse_fit(&self) -> f64 {
        let (mut n, mut su, mut sv) = (0.0, 0.0, 0.0);
        for (at, _) in self.cells.iter().enumerate().filter(|(_, cell)| **cell) {
            n += 1.0;
            su += (at % self.width) as f64;
            sv += (at / self.width) as f64;
        }
        if n < 3.0 {
            return 0.0;
        }
        let (cu, cv) = (su / n, sv / n);
        let (mut uu, mut vv, mut uv) = (0.0, 0.0, 0.0);
        for (at, _) in self.cells.iter().enumerate().filter(|(_, cell)| **cell) {
            let (du, dv) = ((at % self.width) as f64 - cu, (at / self.width) as f64 - cv);
            uu += du * du;
            vv += dv * dv;
            uv += du * dv;
        }
        let (uu, vv, uv) = (uu / n, vv / n, uv / n);
        // The covariance's eigenvalues; the ellipse with these second
        // moments has semi-axes twice their square roots.
        let mean = f64::midpoint(uu, vv);
        let spread = math::sqrt((0.25 * (uu - vv) * (uu - vv) + uv * uv).max(0.0));
        let (major, minor) = (mean + spread, (mean - spread).max(1e-9));
        let angle = 0.5 * math::atan2(2.0 * uv, uu - vv);
        let (cos, sin) = (math::cos(angle), math::sin(angle));
        let (a2, b2) = (4.0 * major, 4.0 * minor);
        let (mut both, mut either) = (0.0, 0.0);
        for row in 0..self.height {
            for column in 0..self.width {
                let (du, dv) = (column as f64 - cu, row as f64 - cv);
                let (p, q) = (du * cos + dv * sin, -du * sin + dv * cos);
                let inside = p * p / a2 + q * q / b2 <= 1.0;
                let cell = self.cells[row * self.width + column];
                if inside && cell {
                    both += 1.0;
                }
                if inside || cell {
                    either += 1.0;
                }
            }
        }
        if either > 0.0 { both / either } else { 0.0 }
    }
}

/// The crown's outline fit and see-through from the side (see the
/// module), the means of the views along x and along z, or `None` with
/// fewer than [`SIDE_LEAVES`] leaves.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn side_views(graph: &PlantGraph, types: &[OrganType]) -> Option<(f64, f64)> {
    let alive = |shed: Option<f64>| shed.is_none_or(|age| age > graph.age);
    // Each leaf: its card's middle, its area and its normal.
    let leaves: Vec<(math::Vec3, f64, math::Vec3)> = graph
        .organs
        .iter()
        .filter(|organ| alive(organ.shed) && organ.size > 0.0)
        .filter_map(|organ| {
            let kind = types.get(usize::from(organ.organ))?;
            (kind.kind == OrganKind::Leaf).then(|| {
                (
                    organ.position + organ.heading * (0.5 * organ.size),
                    kind.area * organ.size * organ.size,
                    organ.heading.cross(organ.left).normalize_or(math::Vec3::Y),
                )
            })
        })
        .collect();
    if leaves.len() < SIDE_LEAVES {
        return None;
    }
    let mut heights: Vec<f64> = leaves.iter().map(|(at, _, _)| at.y).collect();
    heights.sort_by(f64::total_cmp);
    let foot = heights[((heights.len() - 1) as f64 * CROWN_FOOT).round() as usize];
    let top = heights[heights.len() - 1];
    let (mut fits, mut throughs) = (0.0, 0.0);
    for along_x in [false, true] {
        // Across the view, and the view's own axis.
        let across = |v: math::Vec3| if along_x { v.z } else { v.x };
        let toward = |v: math::Vec3| if along_x { v.x } else { v.z };
        let (least, most) = leaves
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (at, _, _)| {
                (a.min(across(*at)), b.max(across(*at)))
            });
        let extent = (most - least).max(top - foot).max(1e-3);
        let cell = SIDE_CELL.min(extent / 100.0);
        let reach = ((OUTLINE_CLOSING * (most - least) / cell).round() as usize).max(1);
        let margin = (reach + 2) as f64 * cell;
        let left = least - margin;
        let width = ((most + margin - left) / cell).ceil() as usize + 1;
        let height = ((top + margin - foot) / cell).ceil() as usize + 1;
        let to_grid = |at: math::Vec3| ((across(at) - left) / cell, (top + margin - at.y) / cell);
        let mut leafy = Grid::new(width, height);
        for (at, area, normal) in &leaves {
            if at.y < foot {
                continue;
            }
            let shown = area * toward(*normal).abs();
            let (u, v) = to_grid(*at);
            leafy.disc(u, v, math::sqrt(shown / std::f64::consts::PI) / cell);
        }
        let mut drawn = Grid {
            width,
            height,
            cells: leafy.cells.clone(),
        };
        for segment in graph.segments.iter().filter(|segment| alive(segment.shed)) {
            let radius = segment.radius / cell;
            if radius < 0.25 {
                continue;
            }
            let steps = (segment.start.distance(segment.end) / cell * 2.0) as usize + 1;
            for step in 0..=steps {
                let at = segment.start.lerp(segment.end, step as f64 / steps as f64);
                if at.y >= foot {
                    let (u, v) = to_grid(at);
                    drawn.disc(u, v, radius);
                }
            }
        }
        let outline = leafy.spread(reach, true).spread(reach, false).filled();
        let inside = outline.count().max(1) as f64;
        let covered = outline
            .cells
            .iter()
            .zip(&drawn.cells)
            .filter(|(out, cell)| **out && **cell)
            .count() as f64;
        fits += outline.ellipse_fit();
        throughs += 1.0 - covered / inside;
    }
    Some((fits / 2.0, throughs / 2.0))
}

/// The [`OUTLINE_SHARE`] quantile of `values` (sorted in place), or `None`
/// when empty.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn quantile(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * OUTLINE_SHARE).round() as usize;
    values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphOrgan;
    use crate::math::Vec3;

    fn crown(profile: impl Fn(f64) -> f64) -> PlantGraph {
        let mut organs = Vec::new();
        for i in 0..200_u32 {
            let h = (f64::from(i) + 0.5) / 200.0;
            for j in 0..16_u32 {
                let angle = f64::from(j) / 16.0 * std::f64::consts::TAU;
                let r = profile(h);
                organs.push(GraphOrgan {
                    id: u64::from(i * 16 + j),
                    organ: 0,
                    segment: None,
                    position: Vec3 {
                        x: r * math::cos(angle),
                        y: 2.0 + 10.0 * h,
                        z: r * math::sin(angle),
                    },
                    heading: Vec3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    left: Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    size: 0.2,
                    born: 0.0,
                    shed: None,
                    light: 1.0,
                    within: None,
                    adult: None,
                });
            }
        }
        PlantGraph {
            age: 10.0,
            height: 12.0,
            segments: Vec::new(),
            organs,
            structures: Vec::new(),
        }
    }

    /// Leaves filling the solid of revolution `profile` (radius at a share
    /// of the height), one in each cell of 0.25 m, each turned half way to
    /// the side.
    fn solid(profile: impl Fn(f64) -> f64) -> PlantGraph {
        let mut graph = crown(|_| 0.0);
        graph.organs.clear();
        let (heading, left) = (Vec3::new(0.0, 0.7, 0.7), Vec3::new(0.7, 0.0, -0.7));
        for i in 0..40_i32 {
            let h = (f64::from(i) + 0.5) / 40.0;
            for x in -24..=24_i32 {
                for z in -24..=24_i32 {
                    let (x, z) = (f64::from(x) * 0.25, f64::from(z) * 0.25);
                    if x * x + z * z > profile(h).powi(2) {
                        continue;
                    }
                    let id = graph.organs.len() as u64;
                    // Scattered within its cell, the same for every run.
                    let jitter = |salt: u64| {
                        (crate::rng::unit(crate::rng::hash_words(&[id, salt])) - 0.5) * 0.25
                    };
                    graph.organs.push(GraphOrgan {
                        id,
                        position: Vec3::new(
                            x + jitter(0),
                            2.0 + 10.0 * h + jitter(1),
                            z + jitter(2),
                        ),
                        heading,
                        left,
                        ..graph_organ()
                    });
                }
            }
        }
        graph
    }

    fn graph_organ() -> GraphOrgan {
        GraphOrgan {
            id: 0,
            organ: 0,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size: 0.2,
            born: 0.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        }
    }

    #[test]
    fn from_the_side_an_orb_fits_its_ellipse_and_a_vase_does_not() {
        let types = [OrganType {
            name: "leaf".into(),
            kind: OrganKind::Leaf,
            area: 0.3,
            term: None,
        }];
        let orb = measure(
            &solid(|h| 5.0 * math::sqrt(1.0 - (2.0 * h - 1.0).powi(2))),
            &types,
        )
        .unwrap();
        let vase = measure(&solid(|h| 0.5 + 5.0 * h), &types).unwrap();
        assert!(orb.outline_fit.unwrap() > 0.92, "{orb:?}");
        assert!(vase.outline_fit.unwrap() < 0.85, "{vase:?}");
        assert!(orb.see_through.unwrap() < 0.1, "{orb:?}");
    }

    #[test]
    fn an_orb_departs_little_from_an_ellipse_and_a_cone_much() {
        let types = [OrganType {
            name: "leaf".into(),
            kind: OrganKind::Leaf,
            area: 0.3,
            term: None,
        }];
        let orb = measure(
            &crown(|h| 5.0 * math::sqrt(1.0 - (2.0 * h - 1.0).powi(2))),
            &types,
        )
        .unwrap();
        let cone = measure(&crown(|h| 5.0 * (1.0 - h)), &types).unwrap();
        assert!(orb.ellipse_departure < 0.08, "{orb:?}");
        assert!(cone.ellipse_departure > 0.2, "{cone:?}");
        assert!(cone.widest_at < 0.15);
        assert!(orb.lobing < 0.01);
        assert_eq!(orb.leaves, 3200);
        assert!((orb.leaf_length.unwrap() - 0.2).abs() < 1e-12);
    }
}
