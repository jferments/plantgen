//! Lace: a divided leaf drawn as geometry (lacy leaves, technique T5).
//!
//! A compound leaf whose look sets `lace` is drawn, on its nearest `lace`
//! levels of detail, as its rachises and each of its leaflets or segments
//! in three dimensions instead of on its card; farther levels draw the
//! card as before, so the template and its area still describe the leaf.
//! The geometry is [`crate::divided::layout`]'s, the card's own layout, so
//! the two agree where they meet.
//!
//! The layout lies in the card's plane, `x` across and `y` along in units
//! of the card's length, `z` out of its face. Each primary division (with
//! everything it bears) turns about the leaf's rachis by
//!
//! ```math
//! \theta_1 = s\,\lambda + k\,\rho + \sigma\,(2u - 1)
//! ```
//!
//! for its side $`s`$ (1 right, -1 left, 0 the terminal one), the lift
//! $`\lambda`$, its place $`k`$ from the foot, the roll $`\rho`$ and the
//! scatter $`\sigma`$ times a draw $`u`$ of its own: a flat leaf turns by
//! nothing, a loose spray's divisions rise and scatter, and a
//! bottlebrush's stand all round the rachis. Each leaflet of a division
//! then turns about the division's own rachis by its place times the
//! inner roll, and half the scatter. The leaf then droops over its length
//! as its card would ([`crate::bend::Bend`]'s droop).
//!
//! A leaflet is a ribbon along its midrib, as wide as the card draws it,
//! with a face on each side; a rachis is a three-sided tube in the look's
//! accent colour, thinner at each order; a dissected blade's wing a ribbon
//! along its rachis. The geometry is part of the plant's wood mesh, with
//! the organ's colour per vertex, its birth and shed ages, and wind level
//! 3.

use crate::divided::{self, Layout, Leaflet};
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::{Compound, Look, Shape};
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};

/// The most levels a leaf is drawn as lace on.
pub const MAX_LEVELS: u8 = 2;

/// A rachis' radius, as a share of the card's length, by order, at most;
/// and as a share of the half-width of the widest leaflet it bears.
const RACHIS: [f64; 3] = [0.004, 0.0025, 0.0015];
const RACHIS_SHARE: [f64; 3] = [0.5, 0.4, 0.35];

/// Whether `c` is drawn as lace at `level`.
#[must_use]
pub fn drawn_at(c: &Compound, level: usize) -> bool {
    c.lace
        .is_some_and(|levels| level < usize::from(levels.min(MAX_LEVELS)))
}

/// Whether each organ type of `looks` is drawn as lace at `level`.
#[must_use]
pub fn lace_types(looks: &[Look], level: usize) -> Vec<bool> {
    looks
        .iter()
        .map(|look| matches!(&look.shape, Shape::Compound(c) if drawn_at(c, level)))
        .collect()
}

/// Draw every organ of `graph` whose look is lace at `level` into `mesh`,
/// coloured by `colour` (the organ's card colour).
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    level: usize,
    colour: &dyn Fn(&Look, &GraphOrgan) -> [f32; 3],
    mesh: &mut Mesh,
) {
    let mut laid: Vec<Option<Layout>> = vec![None; looks.len()];
    for organ in &graph.organs {
        let index = usize::from(organ.organ);
        let Some(look) = looks.get(index) else {
            continue;
        };
        let Shape::Compound(c) = &look.shape else {
            continue;
        };
        if !drawn_at(c, level) || organ.size.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        {
            continue;
        }
        let layout = laid[index].get_or_insert_with(|| divided::layout(c));
        let first = mesh.positions.len();
        Lace::new(look, c, organ, colour(look, organ), level).draw(layout, mesh);
        mesh.bear(first, organ.segment.unwrap_or(crate::mesh::GROUNDED));
    }
}

/// One leaf being drawn.
struct Lace<'a> {
    c: &'a Compound,
    base: Vec3,
    heading: Vec3,
    left: Vec3,
    face: Vec3,
    /// The card's length in metres, and its drawn shares across and along.
    length: f64,
    across: f64,
    along: f64,
    droop: f64,
    colour: [f32; 3],
    underside: [f32; 3],
    accent: [f32; 3],
    born: f64,
    shed: Option<f64>,
    id: u64,
    /// Stations along a leaflet, and whether it has a vertex on its midrib.
    stations: usize,
    midrib: bool,
}

/// A rotation about the line through `at` along `axis` by `angle` radians.
#[derive(Clone, Copy)]
struct Turn {
    at: Vec3,
    axis: Vec3,
    angle: f64,
}

impl Turn {
    fn apply(&self, p: Vec3) -> Vec3 {
        self.at + (p - self.at).rotate_about(self.axis, self.angle)
    }

    fn direction(&self, d: Vec3) -> Vec3 {
        d.rotate_about(self.axis, self.angle)
    }
}

fn mix(a: [f32; 3], b: [f32; 3], share: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * share)
}

fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn scaled(c: [f32; 3], factor: f32) -> [f32; 3] {
    c.map(|channel| (channel * factor).clamp(0.0, 1.0))
}

impl<'a> Lace<'a> {
    fn new(
        look: &Look,
        c: &'a Compound,
        organ: &GraphOrgan,
        colour: [f32; 3],
        level: usize,
    ) -> Self {
        let heading = organ.heading.normalize_or(Vec3::Y);
        let left = organ.left.normalize_or(Vec3::X);
        // The card's colour darkens the look's other colours with it.
        let darkening = if luminance(look.colour) > 1e-6 {
            (luminance(colour) / luminance(look.colour)).clamp(0.0, 2.0)
        } else {
            1.0
        };
        let (across, along) = crate::templates::drawn_share(&look.shape);
        Self {
            c,
            base: organ.position,
            heading,
            left,
            face: heading.cross(left),
            length: organ.size,
            across,
            along,
            droop: math::radians(look.bend.droop),
            colour,
            underside: look
                .underside
                .map_or_else(|| scaled(colour, 0.85), |under| scaled(under, darkening)),
            accent: scaled(look.accent, darkening),
            born: organ.born,
            shed: organ.shed,
            id: organ.id,
            stations: if level == 0 { 3 } else { 1 },
            midrib: level == 0,
        }
    }

    /// A point of the leaf's local frame (card units: `x` across, `y`
    /// along, `z` out of its face) in the world, drooping as its card
    /// would.
    fn world(&self, p: Vec3) -> Vec3 {
        let length = self.length * self.along;
        let v = p.y;
        let theta = self.droop * v;
        let (along, back) = if self.droop.abs() < 1.0e-9 {
            (v, 0.0)
        } else {
            (
                math::sin(theta) / self.droop,
                (1.0 - math::cos(theta)) / self.droop,
            )
        };
        let mid = self.base + self.heading * (length * along) - self.face * (length * back);
        let up = self.face * math::cos(theta) + self.heading * math::sin(theta);
        mid + self.left * (p.x * self.length * self.across) + up * (p.z * length)
    }

    /// A draw in `[-1, 1]` for the leaf's `what` of index `k`.
    fn draw_of(&self, what: u64, k: u64) -> f64 {
        2.0 * crate::rng::unit(crate::rng::hash_words(&[self.id, what, k])) - 1.0
    }

    /// How division `division` turns about the leaf's rachis: its side, its
    /// place, and its own scatter.
    fn primary(&self, layout: &Layout, division: u32) -> Option<Turn> {
        let lift = math::radians(self.c.lift.unwrap_or(0.0));
        let roll = math::radians(self.c.roll.unwrap_or(0.0));
        let scatter = math::radians(self.c.scatter.unwrap_or(0.0));
        let foot = layout
            .leaflets
            .iter()
            .find(|l| l.division == division)
            .map(|l| {
                layout
                    .axes
                    .iter()
                    .find(|a| a.order == 1 && a.division == Some(division))
                    .map_or((l.base, l.direction), |a| {
                        (a.from, (a.to.0 - a.from.0, a.to.1 - a.from.1))
                    })
            })?;
        let side = if foot.1.0 > 1e-9 {
            1.0
        } else if foot.1.0 < -1e-9 {
            -1.0
        } else {
            0.0
        };
        let angle = -side * lift
            + f64::from(division) * roll
            + scatter * self.draw_of(1, u64::from(division));
        Some(Turn {
            at: Vec3::new(0.0, foot.0.1, 0.0),
            axis: Vec3::Y,
            angle,
        })
    }

    /// How the leaflet `leaflet` turns about its division's rachis, once
    /// its division has turned by `first`.
    fn secondary(&self, layout: &Layout, leaflet: &Leaflet, first: Option<Turn>) -> Option<Turn> {
        let within = leaflet.within?;
        let axis = layout
            .axes
            .iter()
            .find(|a| a.order == 1 && a.division == Some(leaflet.division))?;
        let roll = math::radians(self.c.inner_roll.unwrap_or(0.0));
        let scatter = math::radians(self.c.scatter.unwrap_or(0.0)) * 0.5;
        let from = Vec3::new(axis.from.0, axis.from.1, 0.0);
        let to = Vec3::new(axis.to.0, axis.to.1, 0.0);
        let (from, to) = match first {
            Some(turn) => (turn.apply(from), turn.apply(to)),
            None => (from, to),
        };
        let angle = f64::from(within) * roll
            + scatter * self.draw_of(2, u64::from(leaflet.division) * 4096 + u64::from(within));
        (angle != 0.0).then(|| Turn {
            at: from,
            axis: (to - from).normalize_or(Vec3::Y),
            angle,
        })
    }

    fn vertex(&self, position: Vec3, normal: Vec3, colour: [f32; 3]) -> Vertex {
        Vertex {
            position,
            normal,
            uv: [0.0, 0.0],
            color: [colour[0], colour[1], colour[2], 1.0],
            born: self.born,
            shed: self.shed,
            level: 3,
            segment: crate::mesh::NO_SEGMENT,
        }
    }

    fn draw(&self, layout: &Layout, mesh: &mut Mesh) {
        let turns = |division: Option<u32>| division.and_then(|d| self.primary(layout, d));
        let form = &layout.form;
        // The rachises: each turned with the division it is or is in.
        for axis in &layout.axes {
            let first = turns(axis.division);
            let mut from = Vec3::new(axis.from.0, axis.from.1, 0.0);
            let mut to = Vec3::new(axis.to.0, axis.to.1, 0.0);
            let mut face = Vec3::Z;
            if let Some(turn) = first {
                from = turn.apply(from);
                to = turn.apply(to);
                face = turn.direction(face);
            }
            if let (Some(within), Some(division)) = (axis.within, axis.division) {
                let leaflet = Leaflet {
                    base: axis.from,
                    direction: (0.0, 1.0),
                    length: 0.0,
                    division,
                    within: Some(within),
                    segment: None,
                };
                if let Some(turn) = self.secondary(layout, &leaflet, first) {
                    from = turn.apply(from);
                    to = turn.apply(to);
                    face = turn.direction(face);
                }
            }
            let order = usize::from(axis.order.min(2));
            let borne = layout
                .leaflets
                .iter()
                .filter(|l| {
                    axis.division.is_none_or(|d| l.division == d)
                        && (axis.within.is_none() || l.within == axis.within)
                })
                .map(|l| 0.5 * self.c.leaflet_width * l.length * (1.0 - form.petiolule))
                .fold(0.0, f64::max);
            let radius = RACHIS[order].min(RACHIS_SHARE[order] * borne);
            self.tube(from, to, radius, order, mesh);
            if axis.wing > 0.0 {
                let wing = |t: f64| axis.wing * (1.0 - 0.7 * t);
                self.ribbon(from, to, face, &wing, self.colour, mesh);
            }
        }
        // The leaflets.
        for leaflet in &layout.leaflets {
            let first = turns(Some(leaflet.division));
            let second = self.secondary(layout, leaflet, first);
            let mut base = Vec3::new(leaflet.base.0, leaflet.base.1, 0.0);
            let mut direction = Vec3::new(leaflet.direction.0, leaflet.direction.1, 0.0);
            let mut face = Vec3::Z;
            for turn in [first, second].into_iter().flatten() {
                base = turn.apply(base);
                direction = turn.direction(direction);
                face = turn.direction(face);
            }
            let stalk = form.petiolule * leaflet.length;
            let foot = base + direction * stalk;
            let tip = base + direction * leaflet.length;
            if stalk > 0.0 {
                self.tube(base, foot, RACHIS[2], 2, mesh);
            }
            let blade = leaflet.length - stalk;
            let half = |t: f64| {
                0.5 * self.c.leaflet_width
                    * blade
                    * crate::templates::outline_profile(
                        self.c.outline.as_deref(),
                        t,
                        form.widest,
                        form.base,
                        form.tip,
                    )
            };
            self.ribbon(foot, tip, face, &half, self.colour, mesh);
        }
    }

    /// A ribbon from `from` to `to` in the local frame, its face along
    /// `face`, `half(t)` wide either side at `t` of the way, with a face
    /// each side.
    #[allow(clippy::many_single_char_names)]
    fn ribbon(
        &self,
        from: Vec3,
        to: Vec3,
        face: Vec3,
        half: &dyn Fn(f64) -> f64,
        colour: [f32; 3],
        mesh: &mut Mesh,
    ) {
        let along = to - from;
        if along.length() <= 0.0 {
            return;
        }
        let side = face.cross(along).normalize_or(Vec3::X);
        let columns: &[f64] = if self.midrib {
            &[-1.0, 0.0, 1.0]
        } else {
            &[-1.0, 1.0]
        };
        let n = self.stations;
        for (upper, colour) in [(true, colour), (false, self.underside)] {
            let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            let mut points = Vec::with_capacity((n + 1) * columns.len());
            for i in 0..=n {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f64 / n as f64;
                let w = half(t);
                for &x in columns {
                    points.push(self.world(from + along * t + side * (x * w)));
                }
            }
            // The face's normal, from the world points.
            let du = points[columns.len() - 1] - points[0];
            let dv = points[n * columns.len()] - points[0];
            let mut normal = du.cross(dv).normalize_or(self.face);
            if !upper {
                normal = -normal;
            }
            let shade = |k: usize| {
                if self.midrib && k % columns.len() == 1 {
                    scaled(colour, 1.06)
                } else {
                    colour
                }
            };
            for (k, &p) in points.iter().enumerate() {
                mesh.push(self.vertex(p, normal, shade(k)));
            }
            let width = u32::try_from(columns.len()).unwrap_or(2);
            for i in 0..u32::try_from(n).unwrap_or(1) {
                for j in 0..width - 1 {
                    let a = first + i * width + j;
                    let (b, c, d) = (a + 1, a + width, a + width + 1);
                    let quad = if upper {
                        [a, b, d, a, d, c]
                    } else {
                        [a, d, b, a, c, d]
                    };
                    mesh.indices.extend_from_slice(&quad);
                }
            }
        }
    }

    /// A three-sided tube of `radius` card lengths from `from` to `to` in
    /// the local frame, a rachis of order `order`.
    fn tube(&self, from: Vec3, to: Vec3, radius: f64, order: usize, mesh: &mut Mesh) {
        // The petiole and rachis in the accent colour, toward the leaf's
        // own as they divide: a pinna's rachis is green.
        let colour = match order {
            0 => mix(self.accent, self.colour, 0.4),
            _ => scaled(self.colour, 0.95),
        };
        let along = (to - from).normalize_or(Vec3::Y);
        if (to - from).length() <= 0.0 {
            return;
        }
        let a = along.cross(Vec3::Z).normalize_or(Vec3::X);
        let b = along.cross(a);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for end in [from, to] {
            for k in 0..3 {
                let angle = f64::from(k) * std::f64::consts::TAU / 3.0;
                let out = a * math::cos(angle) + b * math::sin(angle);
                let p = self.world(end + out * radius);
                let normal = (p - self.world(end)).normalize_or(self.face);
                mesh.push(self.vertex(p, normal, colour));
            }
        }
        for k in 0..3 {
            let (a, b) = (first + k, first + (k + 1) % 3);
            let (c, d) = (a + 3, b + 3);
            mesh.indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::looks::Pattern;

    fn compound(edit: impl FnOnce(&mut Compound)) -> Compound {
        let mut c = Compound {
            leaflets: 7,
            pinnules: Some(5),
            segments: Some(3),
            leaflet_width: 0.2,
            lace: Some(2),
            ..Compound::default()
        };
        edit(&mut c);
        c
    }

    /// `c` drawn flat (no droop) from the origin up `y`, 0.2 m long.
    fn drawn(c: &Compound) -> Mesh {
        let (across, along) = crate::templates::drawn_share(&Shape::Compound(c.clone()));
        let lace = Lace {
            c,
            base: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            face: Vec3::Y.cross(Vec3::X),
            length: 0.2,
            across,
            along,
            droop: 0.0,
            colour: [0.2, 0.4, 0.2],
            underside: [0.2, 0.3, 0.2],
            accent: [0.3, 0.4, 0.2],
            born: 0.0,
            shed: None,
            id: 7,
            stations: 3,
            midrib: true,
        };
        let mut mesh = Mesh::default();
        lace.draw(&divided::layout(c), &mut mesh);
        mesh
    }

    #[test]
    fn lace_is_drawn_on_its_nearest_levels_only() {
        let c = compound(|_| {});
        assert!(drawn_at(&c, 0) && drawn_at(&c, 1) && !drawn_at(&c, 2));
        assert!(!drawn_at(&compound(|c| c.lace = None), 0));
    }

    #[test]
    fn a_flat_leaf_lies_in_its_cards_plane_and_a_lifted_one_rises() {
        let flat = drawn(&compound(|_| {}));
        // 105 segments and 43 rachises.
        assert!(flat.triangle_count() > 105 * 8, "{}", flat.triangle_count());
        let height = |mesh: &Mesh| {
            mesh.positions
                .iter()
                .map(|p| f64::from(p[2]).abs())
                .fold(0.0, f64::max)
        };
        // Only the rachises' tubes leave the plane.
        assert!(height(&flat) < 0.2 * 0.01, "{}", height(&flat));
        let lifted = drawn(&compound(|c| c.lift = Some(40.0)));
        assert!(height(&lifted) > 0.2 * 0.05, "{}", height(&lifted));
        // A bottlebrush's divisions stand all round its rachis, below the
        // card's plane as well as above it.
        let round = drawn(&compound(|c| {
            c.roll = Some(137.5);
            c.pattern = Some(Pattern::Pinnate);
        }));
        let below = round.positions.iter().any(|p| p[2] < -0.005);
        let above = round.positions.iter().any(|p| p[2] > 0.005);
        assert!(below && above);
    }

    #[test]
    fn its_leaflets_reach_where_the_card_draws_them() {
        let c = compound(|_| {});
        let mesh = drawn(&c);
        let laid = divided::layout(&c);
        let (across, along) = crate::templates::drawn_share(&Shape::Compound(c.clone()));
        let tip = laid
            .leaflets
            .iter()
            .map(|l| l.base.1 + l.length * l.direction.1)
            .fold(0.0, f64::max);
        let top = mesh
            .positions
            .iter()
            .map(|p| f64::from(p[1]))
            .fold(0.0, f64::max);
        assert!((top - tip * 0.2 * along).abs() < 0.2 * 0.02, "{top} {tip}");
        let reach = mesh
            .positions
            .iter()
            .map(|p| f64::from(p[0]).abs())
            .fold(0.0, f64::max);
        assert!(
            (reach - laid.reach * 0.2 * across).abs() < 0.2 * 0.05,
            "{reach}"
        );
    }
}
