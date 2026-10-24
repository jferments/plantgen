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

/// A side view: cells in rows from the top, `true` where drawn. A
/// grown plant's side view is drawn from its leaves and wood; a
/// photograph's or a render's comes from its plant mask
/// ([`mask_from_png`]), so both are measured by the same steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    /// Cells across.
    pub width: usize,
    /// Rows, the top first.
    pub height: usize,
    /// `width * height` cells, row by row.
    pub cells: Vec<bool>,
}

impl Mask {
    /// An empty mask.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
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

    /// The sizes of its patches of drawn cells joined side to side.
    fn patches(&self) -> Vec<usize> {
        let mut seen = vec![false; self.cells.len()];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..self.cells.len() {
            if seen[start] || !self.cells[start] {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let mut size = 0;
            while let Some(at) = stack.pop() {
                size += 1;
                let (row, column) = (at / self.width, at % self.width);
                let mut visit = |next: usize| {
                    if !seen[next] && self.cells[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                };
                if row > 0 {
                    visit(at - self.width);
                }
                if row + 1 < self.height {
                    visit(at + self.width);
                }
                if column > 0 {
                    visit(at - 1);
                }
                if column + 1 < self.width {
                    visit(at + 1);
                }
            }
            sizes.push(size);
        }
        sizes
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
    side_of(graph, &leaves)
}

/// [`side_views`] of `graph`'s wood and `leaves`, each its card's middle,
/// area and normal.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn side_of(graph: &PlantGraph, leaves: &[(math::Vec3, f64, math::Vec3)]) -> Option<(f64, f64)> {
    let alive = |shed: Option<f64>| shed.is_none_or(|age| age > graph.age);
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
        let mut leafy = Mask::new(width, height);
        for (at, area, normal) in leaves {
            if at.y < foot {
                continue;
            }
            let shown = area * toward(*normal).abs();
            let (u, v) = to_grid(*at);
            leafy.disc(u, v, math::sqrt(shown / std::f64::consts::PI) / cell);
        }
        let mut drawn = Mask {
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
        let side = side_measures(&leafy, &drawn, reach);
        fits += side.outline_fit;
        throughs += side.see_through;
    }
    Some((fits / 2.0, throughs / 2.0))
}

/// Share of the outline's area a gap must cover to count as large.
pub const LARGE_GAP: f64 = 0.01;

/// Share of the widest row's width a mask's row must reach, counting up
/// from its foot, to belong to the crown: below it is the bare stem.
pub const CROWN_ROW: f64 = 0.25;

/// What a side view shows of a crown's outline and the sky through it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideMeasures {
    /// The outline's fit to the ellipse of the same area and second
    /// moments, their intersection over their union: 1 for an ellipse.
    pub outline_fit: f64,
    /// The share of the outline the sky shows through.
    pub see_through: f64,
    /// The outline's width over its height.
    pub width_to_height: f64,
    /// Where the outline is widest, as a share of its height from its
    /// foot (the middle of the widest rows).
    pub widest_at: f64,
    /// The outline's area over its convex hull's: 1 for a smooth dome,
    /// lower where the crown's edge is lobed or ragged.
    pub solidity: f64,
    /// Of the sky inside the outline, the share in gaps of at least
    /// [`LARGE_GAP`] of its area: near 0 where leaves are spread evenly,
    /// high where they clump round large holes.
    pub large_gaps: f64,
    /// Cells inside the outline.
    pub cells: usize,
}

/// The side measures of a view whose outline closes `leafy` by a square
/// of `reach` cells and fills its holes, with `drawn` (the leaves and
/// the wood) what covers the sky inside it.
// Cell counts are far below 2^52.
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn side_measures(leafy: &Mask, drawn: &Mask, reach: usize) -> SideMeasures {
    let outline = leafy.spread(reach, true).spread(reach, false).filled();
    let inside = outline.count().max(1) as f64;
    let covered = outline
        .cells
        .iter()
        .zip(&drawn.cells)
        .filter(|(out, cell)| **out && **cell)
        .count() as f64;
    // Each row's first and last cell inside the outline.
    let rows: Vec<Option<(usize, usize)>> = (0..outline.height)
        .map(|row| {
            let line = &outline.cells[row * outline.width..(row + 1) * outline.width];
            let first = line.iter().position(|&cell| cell)?;
            let last = line.iter().rposition(|&cell| cell)?;
            Some((first, last))
        })
        .collect();
    let top = rows.iter().position(Option::is_some);
    let foot = rows.iter().rposition(Option::is_some);
    let (mut width_to_height, mut widest_at) = (0.0, 0.0);
    if let (Some(top), Some(foot)) = (top, foot) {
        let left = rows.iter().flatten().map(|(a, _)| *a).min().unwrap_or(0);
        let right = rows.iter().flatten().map(|(_, b)| *b).max().unwrap_or(0);
        width_to_height = (right + 1 - left) as f64 / (foot + 1 - top) as f64;
        let span = |row: usize| rows[row].map_or(0, |(a, b)| b + 1 - a);
        let most = (top..=foot).map(span).max().unwrap_or(0);
        let widest: Vec<usize> = (top..=foot).filter(|&row| span(row) == most).collect();
        let middle = (widest[0] + widest[widest.len() - 1]) as f64 / 2.0;
        widest_at = (foot as f64 + 0.5 - middle) / (foot + 1 - top) as f64;
    }
    let hull = hull_area(&rows);
    let sky: Vec<bool> = outline
        .cells
        .iter()
        .zip(&drawn.cells)
        .map(|(out, cell)| *out && !*cell)
        .collect();
    let gaps = Mask {
        width: outline.width,
        height: outline.height,
        cells: sky,
    }
    .patches();
    let open: usize = gaps.iter().sum();
    let large: usize = gaps
        .iter()
        .filter(|&&size| size as f64 >= LARGE_GAP * inside)
        .sum();
    SideMeasures {
        outline_fit: outline.ellipse_fit(),
        see_through: 1.0 - covered / inside,
        width_to_height,
        widest_at,
        solidity: if hull > 0.0 {
            (inside / hull).min(1.0)
        } else {
            0.0
        },
        large_gaps: if open > 0 {
            large as f64 / open as f64
        } else {
            0.0
        },
        cells: outline.count(),
    }
}

/// The side measures of a plant mask, a photograph's or a render's: the
/// mask padded so its closing does not touch the border, cut to its
/// crown (the rows from the lowest that reaches [`CROWN_ROW`] of the
/// widest row up), closed by [`OUTLINE_CLOSING`] of the crown's width as
/// a grown plant's leaves are. What covers the sky inside it is `cover`
/// where given (the photograph's own pixels that are not sky, the same
/// size as the mask), else the mask itself: a mask's outline is reliable
/// where its holes may not be. `None` for an empty mask, or a cover of
/// another size.
// Cell counts are far below 2^52; the reach is a few cells.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[must_use]
pub fn measure_mask(mask: &Mask, cover: Option<&Mask>) -> Option<SideMeasures> {
    if cover.is_some_and(|cover| (cover.width, cover.height) != (mask.width, mask.height)) {
        return None;
    }
    let spans: Vec<usize> = (0..mask.height)
        .map(|row| {
            let line = &mask.cells[row * mask.width..(row + 1) * mask.width];
            match (
                line.iter().position(|&cell| cell),
                line.iter().rposition(|&cell| cell),
            ) {
                (Some(a), Some(b)) => b + 1 - a,
                _ => 0,
            }
        })
        .collect();
    let most = spans.iter().copied().max().filter(|&most| most > 0)?;
    let foot = spans
        .iter()
        .rposition(|&span| span as f64 >= CROWN_ROW * most as f64)?;
    let left = (0..mask.width)
        .find(|&column| (0..=foot).any(|row| mask.cells[row * mask.width + column]))?;
    let right = (0..mask.width)
        .rev()
        .find(|&column| (0..=foot).any(|row| mask.cells[row * mask.width + column]))?;
    let reach = ((OUTLINE_CLOSING * (right + 1 - left) as f64).round() as usize).max(1);
    let pad = reach + 2;
    let padded = |source: &Mask| {
        let mut crown = Mask::new(mask.width + 2 * pad, foot + 1 + 2 * pad);
        for row in 0..=foot {
            for column in 0..mask.width {
                crown.cells[(row + pad) * crown.width + column + pad] =
                    source.cells[row * mask.width + column];
            }
        }
        crown
    };
    let crown = padded(mask);
    let covered = padded(cover.unwrap_or(mask));
    Some(side_measures(&crown, &covered, reach))
}

/// A plant mask from a PNG: a cell is the plant's where its alpha is at
/// least half, or, in an image without alpha, where its brightness is.
///
/// # Errors
///
/// When the bytes are not a PNG the decoder reads.
pub fn mask_from_png(bytes: &[u8]) -> Result<Mask, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or("the PNG is too large to read")?;
    let mut buffer = vec![0; size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let mut mask = Mask::new(width, height);
    for row in 0..height {
        let line = &buffer[row * info.line_size..];
        for column in 0..width {
            let pixel = &line[column * channels..(column + 1) * channels];
            let on = match channels {
                2 | 4 => pixel[channels - 1] >= 128,
                _ => pixel.iter().map(|&v| usize::from(v)).sum::<usize>() >= 128 * channels,
            };
            mask.cells[row * width + column] = on;
        }
    }
    Ok(mask)
}

/// Twice the area of the convex hull of the rows' end cells, halved: the
/// hull of the outline's cells as unit squares.
// Cell indices are far below 2^52.
#[allow(clippy::cast_precision_loss)]
fn hull_area(rows: &[Option<(usize, usize)>]) -> f64 {
    let mut points: Vec<(f64, f64)> = Vec::new();
    for (row, span) in rows.iter().enumerate() {
        if let Some((a, b)) = span {
            let (y0, y1) = (row as f64, row as f64 + 1.0);
            let (x0, x1) = (*a as f64, *b as f64 + 1.0);
            points.extend([(x0, y0), (x0, y1), (x1, y0), (x1, y1)]);
        }
    }
    points.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
    points.dedup();
    if points.len() < 3 {
        return 0.0;
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut hull: Vec<(f64, f64)> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &(f64, f64)>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    let mut twice = 0.0;
    for (k, p) in hull.iter().enumerate() {
        let q = hull[(k + 1) % hull.len()];
        twice += p.0 * q.1 - q.0 * p.1;
    }
    twice.abs() / 2.0
}

/// The most times a leaf is drawn to fill its crown ([`filled`]).
pub const MOST_DRAWN: f64 = 4.0;

/// The least sky a crown is filled to ([`filled`]): a crown its
/// photographs show no sky through is filled to this.
pub const LEAST_SKY: f64 = 0.01;

/// The share of `graph`'s crown outline seen from the side that the sky
/// shows through, with its leaves drawn spread along their shoots
/// ([`crate::looks::OrganLook::spray`]) where they are drawn
/// (`crate::mesh::place`), at the area their cards draw; `None` with
/// fewer than [`SIDE_LEAVES`] of them.
#[must_use]
pub fn drawn_see_through(graph: &PlantGraph, looks: &[crate::looks::Look]) -> Option<f64> {
    side_of(graph, &drawn_cards(graph, looks, &card_areas(looks))).map(|(_, through)| through)
}

/// The leaf area a card one metre long draws with each look that spreads
/// its leaves along a shoot; `None` for the others.
fn card_areas(looks: &[crate::looks::Look]) -> Vec<Option<f64>> {
    looks
        .iter()
        .map(|look| {
            (look.spray > 0.0).then(|| {
                let templates = crate::templates::Templates::for_looks(std::slice::from_ref(look));
                templates
                    .templates
                    .first()
                    .map_or(0.0, |template| crate::templates::drawn_area(look, template))
            })
        })
        .collect()
}

/// Each living leaf drawn spread along a shoot, its card as drawn: its
/// middle, its area and its normal.
fn drawn_cards(
    graph: &PlantGraph,
    looks: &[crate::looks::Look],
    areas: &[Option<f64>],
) -> Vec<(math::Vec3, f64, math::Vec3)> {
    let alive = |shed: Option<f64>| shed.is_none_or(|age| age > graph.age);
    graph
        .organs
        .iter()
        .filter(|organ| alive(organ.shed) && organ.size > 0.0)
        .filter_map(|organ| {
            let index = usize::from(organ.organ);
            let area = (*areas.get(index)?)?;
            let (base, heading, left, length) = crate::mesh::place(organ, looks.get(index)?);
            Some((
                base + heading * (0.5 * length),
                area * length * length,
                heading.cross(left).normalize_or(math::Vec3::Y),
            ))
        })
        .collect()
}

/// `graph` with each leaf whose look fills its crown
/// ([`crate::looks::OrganLook::fill`]) drawn again, on shoots of their
/// own from its node, until the crown seen from the side shows about as
/// little sky as its photographs; `None` where no look fills or the crown
/// already shows no more. Drawn `n` times over, leaves strewn through a
/// crown leave about `sky^n` of it open, so `n` starts there and is
/// corrected twice by the drawn crown's own sky, up to [`MOST_DRAWN`].
/// Each copy keeps its leaf's segment, birth, shedding and light, so it
/// grows, falls and shades as its leaf, and the grown plant's leaf area,
/// light and size are not changed.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn filled(graph: &PlantGraph, looks: &[crate::looks::Look]) -> Option<PlantGraph> {
    let fill = |organ: &crate::graph::GraphOrgan| {
        looks
            .get(usize::from(organ.organ))
            .filter(|look| look.spray > 0.0)
            .and_then(|look| look.fill)
    };
    let goal = graph.organs.iter().find_map(fill)?.max(LEAST_SKY);
    let areas = card_areas(looks);
    let grown = drawn_cards(graph, looks, &areas);
    let (_, mut sky) = side_of(graph, &grown)?;
    let mut times: f64 = 1.0;
    let mut copies: Vec<(usize, crate::graph::GraphOrgan)> = Vec::new();
    for _ in 0..3 {
        if sky <= goal || sky >= 1.0 || times >= MOST_DRAWN {
            break;
        }
        times = (times * math::ln(goal) / math::ln(sky)).clamp(1.0, MOST_DRAWN);
        copies = graph
            .organs
            .iter()
            .enumerate()
            .filter(|(_, organ)| fill(organ).is_some())
            .flat_map(|(index, organ)| {
                let extra = times - 1.0;
                let more = usize::from(
                    crate::rng::unit(crate::rng::mix64(organ.id ^ 0x6669_6c6c)) < extra.fract(),
                );
                (0..extra.floor() as usize + more).map(move |copy| (index, copy_of(organ, copy)))
            })
            .collect();
        let again = PlantGraph {
            organs: copies.iter().map(|(_, organ)| organ.clone()).collect(),
            ..graph.clone()
        };
        let mut cards = grown.clone();
        cards.extend(drawn_cards(&again, looks, &areas));
        sky = side_of(graph, &cards)?.1;
    }
    if copies.is_empty() {
        return None;
    }
    let mut organs = Vec::with_capacity(graph.organs.len() + copies.len());
    let mut copies = copies.into_iter().peekable();
    for (index, organ) in graph.organs.iter().enumerate() {
        organs.push(organ.clone());
        while let Some((_, copy)) = copies.next_if(|(of, _)| *of == index) {
            organs.push(copy);
        }
    }
    Some(PlantGraph {
        organs,
        ..graph.clone()
    })
}

/// Copy `copy` of a leaf drawn to fill its crown ([`filled`]): an id of
/// its own and a point a millimetre on from its node, so it is spread
/// along a shoot of its own (`crate::mesh::place`), turned about the
/// vertical at random. Copies on their node's own shoot overlap it and
/// leave more sky (magnolia at 50 years from eye height: 14% against
/// 10%) for 8% fewer triangles.
#[allow(clippy::cast_precision_loss)]
fn copy_of(organ: &crate::graph::GraphOrgan, copy: usize) -> crate::graph::GraphOrgan {
    let id = crate::rng::hash_words(&[organ.id, copy as u64 + 1, 0x6669_6c6c]);
    let turn = std::f64::consts::TAU * crate::rng::unit(id);
    let along = organ.heading.normalize_or(math::Vec3::Y);
    crate::graph::GraphOrgan {
        id,
        position: organ.position + along * (0.001 * (copy as f64 + 1.0)),
        heading: organ.heading.rotate_about(math::Vec3::Y, turn),
        left: organ.left.rotate_about(math::Vec3::Y, turn),
        ..organ.clone()
    }
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

    /// A mask of an upright ellipse on a stem, with a square hole of
    /// `hole` cells a side in its middle, as a PNG whose alpha is the
    /// plant.
    #[allow(clippy::cast_precision_loss)]
    fn ellipse_png(hole: usize) -> Vec<u8> {
        let (width, height) = (200_usize, 260_usize);
        let mut rgba = vec![0_u8; width * height * 4];
        for row in 0..height {
            for column in 0..width {
                let (u, v) = ((column as f64 - 100.0) / 80.0, (row as f64 - 110.0) / 100.0);
                let stem = row >= 200 && (95..105).contains(&column);
                let gap = row.abs_diff(110) < hole / 2 && column.abs_diff(100) < hole / 2;
                if (u * u + v * v <= 1.0 || stem) && !gap {
                    rgba[(row * width + column) * 4 + 3] = 255;
                }
            }
        }
        crate::raster::encode_png(width, height, &rgba).expect("encodes")
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn a_mask_is_measured_as_a_side_view() {
        let mask = mask_from_png(&ellipse_png(0)).expect("reads");
        let side = measure_mask(&mask, None).expect("measures");
        assert!(side.outline_fit > 0.97, "{side:?}");
        assert!(side.solidity > 0.97, "{side:?}");
        assert!(side.see_through < 1e-9, "{side:?}");
        // The stem below the crown is left out: 160 wide by 200 tall.
        assert!((side.width_to_height - 0.8).abs() < 0.03, "{side:?}");
        assert!((side.widest_at - 0.5).abs() < 0.03, "{side:?}");
        let holed = mask_from_png(&ellipse_png(40)).expect("reads");
        // A mask whose outline holds no hole, with the hole in its cover.
        let outline_only = measure_mask(&mask, Some(&holed)).expect("measures");
        let holed = measure_mask(&holed, None).expect("measures");
        assert_eq!(outline_only, holed);
        assert!(
            (holed.see_through - 1600.0 / holed.cells as f64).abs() < 0.01,
            "{holed:?}"
        );
        assert!((holed.large_gaps - 1.0).abs() < 1e-9, "{holed:?}");
        assert!(measure_mask(&Mask::new(10, 10), None).is_none());
        assert!(measure_mask(&mask, Some(&Mask::new(10, 10))).is_none());
    }
}
