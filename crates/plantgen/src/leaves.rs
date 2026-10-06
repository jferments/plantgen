//! Solid leaves: thick, keeled or channelled leaves drawn as geometry.
//!
//! An organ type whose look carries a [`SolidLeaf`] is drawn, on its
//! nearest [`SolidLeaf::levels`] levels of detail, as a leaf swept along a
//! curved midrib instead of a card (plant forms milestone F2; the rosettes
//! of agaves, aloes, yuccas and sotols). Farther levels draw the organ's
//! card as before, so the template and its area still describe the leaf.
//!
//! The organ's card frame gives the leaf: its base $`\mathbf b`$, heading
//! $`\mathbf h`$, left $`\mathbf l`$ and face $`\mathbf n = \mathbf h \times
//! \mathbf l`$, its length $`\ell`$ (the organ's size) and its widest width
//! $`W`$ (the size times the look's aspect). At a share $`u \in [0, 1]`$ of
//! its length the midrib heads along $`\mathbf h`$ turned about
//! $`\mathbf l`$ by $`\alpha u`$ toward the leaf's back, $`\alpha`$ the
//! arch, and the leaf's half-width is $`\tfrac W2 w(u)`$ with
//!
//! ```math
//! w(u) = \begin{cases}
//!   \beta + (1 - \beta)\,\operatorname{smoothstep}(0, u_w, u) & u < u_w \\
//!   \left(1 - \frac{u - u_w}{1 - u_w}\right)^{\tau} & u \ge u_w
//! \end{cases}
//! ```
//!
//! for the base share $`\beta`$, the widest point $`u_w`$ and the taper
//! $`\tau`$. Across the leaf, at $`x \in [-1, 1]`$ of the half-width, the
//! section's centre lies $`f\,\tfrac W2 w(u)\,g(x)`$ along the face for the
//! fold $`f`$ (keeled $`g = |x|`$, channelled $`g = x^2`$, flat and
//! triangular $`g = 0`$), and the leaf is $`t\,W\,w(u)\,(1 - x^2)`$ thick
//! for the thickness share $`t`$, or $`t\,W\,w(u)\,(1 - |x|)`$ below a flat
//! top for a triangular section. The section turns about the midrib by
//! the twist times $`u`$. Marginal [`Teeth`] are small tetrahedra along
//! both edges, hooked toward the tip or the base, and a [`TipSpine`] a
//! three-sided cone. The leaf is part of the plant's wood mesh, with the
//! organ's colour per vertex, its birth and shed ages, and wind level 3.

use serde::{Deserialize, Serialize};

use crate::graph::PlantGraph;
use crate::looks::Look;
use crate::math::{self, Vec3};
use crate::mesh::{Mesh, Vertex};

/// How a solid leaf is shaped across.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// A lens, thickest along the midrib.
    #[default]
    Flat,
    /// A V: edges raised along the face, a keel below.
    Keeled,
    /// A U: a channel along the face.
    Channelled,
    /// Flat above, a ridge below, as an aloe's or a sotol's base.
    Triangular,
}

/// Teeth along both margins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Teeth {
    /// Distance between teeth along the margin, metres.
    pub spacing: f64,
    /// A tooth's length out from the margin, metres.
    pub length: f64,
    /// Degrees a tooth turns toward the tip (negative: toward the base).
    pub hook: f64,
    /// The share of the leaf, from its base, without teeth.
    pub start: f64,
    /// Linear RGB.
    pub colour: [f32; 3],
}

impl Default for Teeth {
    fn default() -> Self {
        Self {
            spacing: 0.02,
            length: 0.005,
            hook: 20.0,
            start: 0.1,
            colour: [0.25, 0.15, 0.08],
        }
    }
}

/// A spine at the leaf's tip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TipSpine {
    /// Length and base width, metres.
    pub length: f64,
    pub width: f64,
    /// Linear RGB.
    pub colour: [f32; 3],
}

impl Default for TipSpine {
    fn default() -> Self {
        Self {
            length: 0.03,
            width: 0.004,
            colour: [0.2, 0.12, 0.07],
        }
    }
}

/// A leaf drawn as geometry on the nearest levels of detail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SolidLeaf {
    pub section: Section,
    /// Thickness at the midrib as a share of the widest width.
    pub thickness: f64,
    /// Depth of a keel or channel as a share of the half-width.
    pub fold: f64,
    /// Where the leaf is widest, as a share of its length from the base.
    pub widest: f64,
    /// Width at the base as a share of the widest.
    pub base: f64,
    /// Power of the taper from the widest point to the tip: 1 straight,
    /// less a blunter tip, more a longer point.
    pub taper: f64,
    /// Degrees the midrib bends toward the leaf's back from base to tip.
    pub arch: f64,
    /// Degrees the section turns about the midrib from base to tip.
    pub twist: f64,
    /// Colour of the margins, the organ's colour if absent (linear RGB):
    /// an agave's pale edge, a yucca's fibres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub teeth: Option<Teeth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spine: Option<TipSpine>,
    /// Levels of detail, from the nearest, that draw the leaf solid: 1 or
    /// 2. The others draw its card.
    pub levels: u8,
}

impl Default for SolidLeaf {
    fn default() -> Self {
        Self {
            section: Section::Flat,
            thickness: 0.15,
            fold: 0.0,
            widest: 0.2,
            base: 0.8,
            taper: 1.0,
            arch: 0.0,
            twist: 0.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 2,
        }
    }
}

/// Stations along a leaf and samples across each half at most, by level.
const STATIONS: [usize; 2] = [14, 7];
const ACROSS: [usize; 2] = [4, 2];
/// The length between stations a level aims for, metres: a short leaf
/// takes fewer stations, down to [`MIN_STATIONS`].
const STATION_LENGTH: [f64; 2] = [0.025, 0.06];
const MIN_STATIONS: usize = 3;
/// Leaves narrower than this, metres, take one sample across each half.
const NARROW_M: f64 = 0.02;
/// Levels a solid leaf may cover.
pub const MAX_LEVELS: u8 = 2;

impl SolidLeaf {
    /// Whether level `level` draws this leaf solid.
    #[must_use]
    pub fn solid_at(&self, level: usize) -> bool {
        level < usize::from(self.levels.min(MAX_LEVELS))
    }

    /// The half-width share at `u` of the length.
    #[must_use]
    pub fn width_at(&self, u: f64) -> f64 {
        let widest = self.widest.clamp(1.0e-3, 0.999);
        if u < widest {
            math::lerp(self.base, 1.0, math::smoothstep(0.0, widest, u))
        } else {
            math::pow(
                (1.0 - (u - widest) / (1.0 - widest)).max(0.0),
                self.taper.max(0.05),
            )
        }
    }

    /// The section's centre offset and thickness factor at `x` across.
    fn across(&self, x: f64) -> (f64, f64, f64) {
        let shape = match self.section {
            Section::Keeled => x.abs(),
            Section::Channelled => x * x,
            Section::Flat | Section::Triangular => 0.0,
        };
        // (centre along the face, share above it, share below it)
        match self.section {
            Section::Triangular => (shape, 0.0, 1.0 - x.abs()),
            _ => (shape, 0.5 * (1.0 - x * x), 0.5 * (1.0 - x * x)),
        }
    }
}

/// Whether each organ type of `looks` is drawn solid at `level`.
#[must_use]
pub fn solid_types(looks: &[Look], level: usize) -> Vec<bool> {
    looks
        .iter()
        .map(|look| {
            look.solid
                .as_ref()
                .is_some_and(|solid| solid.solid_at(level))
        })
        .collect()
}

/// Draw every organ of `graph` whose look is solid at `level` into
/// `mesh`, coloured by `colour` (the organ's card colour).
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    level: usize,
    colour: &dyn Fn(&Look, &crate::graph::GraphOrgan) -> [f32; 3],
    mesh: &mut Mesh,
) {
    let detail = level.min(STATIONS.len() - 1);
    for organ in &graph.organs {
        let Some(look) = looks.get(usize::from(organ.organ)) else {
            continue;
        };
        let Some(solid) = look.solid.as_ref().filter(|solid| solid.solid_at(level)) else {
            continue;
        };
        let leaf = Leaf {
            base: organ.position,
            heading: organ.heading.normalize_or(Vec3::Y),
            left: organ.left.normalize_or(Vec3::X),
            length: organ.size,
            width: organ.size * look.shape.aspect(),
            colour: colour(look, organ),
            born: organ.born,
            shed: organ.shed,
        };
        let (stations, across) = detail_of(detail, leaf.length, leaf.width);
        leaf.draw(solid, stations, across, mesh);
    }
}

/// Stations along and samples across each half of a leaf `length` by
/// `width` metres at detail level `detail` (0 or 1).
#[must_use]
pub fn detail_of(detail: usize, length: f64, width: f64) -> (usize, usize) {
    let detail = detail.min(STATIONS.len() - 1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let wanted = (length / STATION_LENGTH[detail]).ceil().max(0.0) as usize;
    let across = if width < NARROW_M { 1 } else { ACROSS[detail] };
    (wanted.clamp(MIN_STATIONS, STATIONS[detail]), across)
}

/// One leaf to draw.
struct Leaf {
    base: Vec3,
    heading: Vec3,
    left: Vec3,
    length: f64,
    width: f64,
    colour: [f32; 3],
    born: f64,
    shed: Option<f64>,
}

impl Leaf {
    fn vertex(&self, position: Vec3, colour: [f32; 3]) -> Vertex {
        Vertex {
            position,
            normal: Vec3::Y,
            uv: [0.0, 0.0],
            color: [colour[0], colour[1], colour[2], 1.0],
            born: self.born,
            shed: self.shed,
            level: 3,
        }
    }

    fn draw(&self, solid: &SolidLeaf, stations: usize, across: usize, mesh: &mut Mesh) {
        if !(self.length > 0.0 && self.width > 0.0) {
            return;
        }
        let first = mesh.positions.len();
        let first_index = mesh.indices.len();
        let margin = solid.margin.unwrap_or(self.colour);
        let arch = math::radians(solid.arch);
        let twist = math::radians(solid.twist);
        let step = self.length / stations as f64;
        // The midrib: a point and a frame (tangent, across, face) per
        // station, the last at the tip.
        let mut point = self.base;
        let mut frames = Vec::with_capacity(stations + 1);
        for i in 0..=stations {
            let u = i as f64 / stations as f64;
            let tangent = self.heading.rotate_about(self.left, arch * u);
            let side = self.left.rotate_about(tangent, twist * u);
            let face = tangent.cross(side);
            frames.push((point, tangent, side, face, u));
            point += tangent * step;
        }
        // Rings: the top surface from the left edge to the right, then the
        // bottom back, the edges shared.
        let loop_points = 4 * across;
        let half = self.width * 0.5;
        let thick = solid.thickness * self.width;
        for &(centre, _, side, face, u) in &frames[..stations] {
            let w = solid.width_at(u);
            let ring = |x: f64, top: bool| {
                let (offset, above, below) = solid.across(x);
                let lateral = side * (x * half * w);
                let lift = solid.fold * half * w * offset;
                let depth = if top {
                    thick * w * above
                } else {
                    -thick * w * below
                };
                centre + lateral + face * (lift + depth)
            };
            for j in 0..=2 * across {
                let x = -1.0 + j as f64 / across as f64;
                let colour = if j == 0 || j == 2 * across {
                    margin
                } else {
                    self.colour
                };
                mesh.push(self.vertex(ring(x, true), colour));
            }
            for j in (1..2 * across).rev() {
                let x = -1.0 + j as f64 / across as f64;
                mesh.push(self.vertex(ring(x, false), shade(self.colour, 0.85)));
            }
        }
        let tip_frame = frames[stations];
        let tip = mesh.push(self.vertex(tip_frame.0, self.colour));
        let index = |ring: usize, k: usize| {
            u32::try_from(first + ring * loop_points + k % loop_points).unwrap_or(0)
        };
        for ring in 0..stations - 1 {
            for k in 0..loop_points {
                let (a, b) = (index(ring, k), index(ring, k + 1));
                let (c, d) = (index(ring + 1, k), index(ring + 1, k + 1));
                mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        for k in 0..loop_points {
            let (a, b) = (index(stations - 1, k), index(stations - 1, k + 1));
            mesh.indices.extend_from_slice(&[a, tip, b]);
        }
        // The base, closed by a fan round its centre.
        let centre = mesh.push(self.vertex(self.base, self.colour));
        for k in 0..loop_points {
            mesh.indices
                .extend_from_slice(&[index(0, k), index(0, k + 1), centre]);
        }
        if let Some(teeth) = &solid.teeth {
            self.teeth(solid, teeth, &frames, mesh);
        }
        if let Some(spine) = &solid.spine {
            let (point, tangent, side, face, _) = tip_frame;
            cone(
                self,
                point,
                tangent,
                side,
                face,
                spine.length,
                spine.width * 0.5,
                spine.colour,
                mesh,
            );
        }
        smooth_normals(mesh, first, first_index);
    }

    /// Teeth along both margins from `teeth.start` of the length to the
    /// tip's last station.
    fn teeth(
        &self,
        solid: &SolidLeaf,
        teeth: &Teeth,
        frames: &[(Vec3, Vec3, Vec3, Vec3, f64)],
        mesh: &mut Mesh,
    ) {
        let spacing = teeth.spacing.max(1.0e-3);
        let stations = frames.len() - 1;
        let mut along = teeth.start.max(0.0) * self.length;
        let half = self.width * 0.5;
        let hook = math::radians(teeth.hook);
        while along < self.length * (1.0 - 0.5 / stations as f64) {
            let u = along / self.length;
            let at = (u * stations as f64).min(stations as f64 - 1.0);
            let i = at.floor() as usize;
            let s = at - i as f64;
            let (p0, t0, l0, n0, _) = frames[i];
            let (p1, t1, l1, n1, _) = frames[i + 1];
            let centre = p0.lerp(p1, s);
            let tangent = t0.lerp(t1, s).normalize_or(t0);
            let side = l0.lerp(l1, s).normalize_or(l0);
            let face = n0.lerp(n1, s).normalize_or(n0);
            let w = solid.width_at(u);
            let (offset, _, _) = solid.across(1.0);
            for sign in [-1.0, 1.0] {
                let edge =
                    centre + side * (sign * half * w) + face * (solid.fold * half * w * offset);
                let out = (side * sign).rotate_about(face, sign * hook);
                let reach = teeth.length * w.max(0.3);
                let apex = edge + out * reach;
                let a = edge - tangent * (reach * 0.6);
                let b = edge + tangent * (reach * 0.6);
                let lift = face * (reach * 0.25);
                let corners = [a, b, apex, edge + lift];
                let base = mesh.positions.len();
                for corner in corners {
                    mesh.push(self.vertex(corner, teeth.colour));
                }
                let v = |k: usize| u32::try_from(base + k).unwrap_or(0);
                mesh.indices.extend_from_slice(&[
                    v(0),
                    v(2),
                    v(1),
                    v(0),
                    v(1),
                    v(3),
                    v(1),
                    v(2),
                    v(3),
                    v(2),
                    v(0),
                    v(3),
                ]);
            }
            along += spacing;
        }
    }
}

/// A three-sided cone from `base` along `tangent`.
#[allow(clippy::too_many_arguments)]
fn cone(
    leaf: &Leaf,
    base: Vec3,
    tangent: Vec3,
    side: Vec3,
    face: Vec3,
    length: f64,
    radius: f64,
    colour: [f32; 3],
    mesh: &mut Mesh,
) {
    let first = mesh.positions.len();
    for k in 0..3 {
        let angle = math::radians(120.0 * f64::from(k));
        let around = side * math::cos(angle) + face * math::sin(angle);
        mesh.push(leaf.vertex(base + around * radius, colour));
    }
    mesh.push(leaf.vertex(base + tangent * length, colour));
    let v = |k: usize| u32::try_from(first + k).unwrap_or(0);
    for k in 0..3 {
        mesh.indices
            .extend_from_slice(&[v(k), v((k + 1) % 3), v(3)]);
    }
}

fn shade(colour: [f32; 3], factor: f32) -> [f32; 3] {
    colour.map(|channel| (channel * factor).clamp(0.0, 1.0))
}

/// Normals of the vertices from `first`, as the area-weighted sum of the
/// faces from `first_index` that use them.
fn smooth_normals(mesh: &mut Mesh, first: usize, first_index: usize) {
    let count = mesh.positions.len() - first;
    let mut sums = vec![Vec3::ZERO; count];
    let position = |index: u32| {
        let p = mesh.positions[index as usize];
        Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    };
    for triangle in mesh.indices[first_index..].chunks_exact(3) {
        let (a, b, c) = (
            position(triangle[0]),
            position(triangle[1]),
            position(triangle[2]),
        );
        let normal = (b - a).cross(c - a);
        for &index in triangle {
            if let Some(sum) = (index as usize)
                .checked_sub(first)
                .and_then(|k| sums.get_mut(k))
            {
                *sum += normal;
            }
        }
    }
    for (k, sum) in sums.into_iter().enumerate() {
        mesh.normals[first + k] = sum.normalize_or(Vec3::Y).to_f32();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> Leaf {
        Leaf {
            base: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            length: 0.5,
            width: 0.1,
            colour: [0.3, 0.4, 0.3],
            born: 1.0,
            shed: Some(9.0),
        }
    }

    #[test]
    fn a_leaf_narrows_to_its_tip_from_its_widest_point() {
        let solid = SolidLeaf {
            widest: 0.25,
            base: 0.6,
            ..SolidLeaf::default()
        };
        assert!((solid.width_at(0.0) - 0.6).abs() < 1e-12);
        assert!((solid.width_at(0.25) - 1.0).abs() < 1e-12);
        assert!(solid.width_at(1.0).abs() < 1e-12);
        let mut last = 1.0;
        for k in 0..=75 {
            let w = solid.width_at(0.25 + 0.01 * f64::from(k));
            assert!(w <= last + 1e-12, "the taper never widens");
            last = w;
        }
    }

    #[test]
    fn a_solid_leaf_is_a_closed_sweep_with_its_ages_and_teeth() {
        let solid = SolidLeaf {
            section: Section::Keeled,
            fold: 0.3,
            arch: 30.0,
            teeth: Some(Teeth::default()),
            spine: Some(TipSpine::default()),
            ..SolidLeaf::default()
        };
        let mut plain = Mesh::default();
        leaf().draw(
            &SolidLeaf {
                teeth: None,
                spine: None,
                ..solid.clone()
            },
            14,
            4,
            &mut plain,
        );
        // 13 bands of 16 quads, 16 tip and 16 base triangles.
        assert_eq!(plain.triangle_count(), 13 * 16 * 2 + 16 + 16);
        let mut mesh = Mesh::default();
        leaf().draw(&solid, 14, 4, &mut mesh);
        assert!(
            mesh.triangle_count() > plain.triangle_count() + 3,
            "teeth and a spine"
        );
        assert!(mesh.births.iter().all(|&b| (b - 1.0).abs() < 1e-6));
        assert!(mesh.sheds.iter().all(|&s| (s - 9.0).abs() < 1e-6));
        assert!(mesh.levels.iter().all(|&l| l == 3));
        for normal in &mesh.normals {
            let length = normal.iter().map(|c| c * c).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 1e-4);
        }
        // The face is heading × left = Y × X = -Z, so the arch bends the
        // tip toward +Z, the leaf's back.
        let tip = mesh.positions[13 * 16];
        assert!(tip[2] > 0.05, "{tip:?}");
        assert!(tip[1] < 0.5 && tip[1] > 0.3);
    }

    #[test]
    fn small_leaves_take_fewer_stations() {
        assert_eq!(detail_of(0, 0.5, 0.1), (14, 4));
        assert_eq!(detail_of(0, 0.2, 0.01), (8, 1));
        assert_eq!(detail_of(0, 0.02, 0.01), (3, 1));
        assert_eq!(detail_of(1, 0.5, 0.1), (7, 2));
        assert_eq!(detail_of(5, 0.1, 0.05), (3, 2));
    }

    #[test]
    fn a_level_draws_its_leaves_solid_only_within_the_look_s_levels() {
        let solid = SolidLeaf {
            levels: 1,
            ..SolidLeaf::default()
        };
        assert!(solid.solid_at(0) && !solid.solid_at(1));
        let solid = SolidLeaf {
            levels: 9,
            ..SolidLeaf::default()
        };
        assert!(
            solid.solid_at(1) && !solid.solid_at(2),
            "at most two levels"
        );
    }
}
