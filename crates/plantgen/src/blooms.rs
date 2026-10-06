//! Flowers, fruit and cones drawn as solids (plant forms milestone F3).
//!
//! An organ type whose look has a [`Form`] is drawn on its nearest
//! [`Form::levels`] levels of detail as geometry in the plant's wood mesh,
//! and its card is left out there; farther levels draw its card. The form
//! reads the look's own template parameters, so every species gains its
//! solids without new data:
//!
//! - A **flower** ([`Shape::Flower`], or a daisy head, [`Shape::Head`]) is
//!   a floral diagram: `n` petals (the rays of a head) evenly round the
//!   flower's axis $`\mathbf a`$ (the organ's heading), each a thin solid
//!   leaf (`crate::leaves`) from the rim of the centre outward, raised by
//!   the cup $`\gamma`$ from the plane square to the axis and curled back
//!   by the curl over its length; a domed centre (a head's disc) in the
//!   accent colour; a fused tube when [`FlowerForm::tube`] is set; and
//!   stamens, thin cones rising round the centre. With the flower's
//!   radius $`R`$ (half the organ's size) and the template's centre share
//!   $`c`$, petals start at $`r_0 = 0.6\,c R`$ and reach $`R`$, so a petal is
//!   $`\ell = R - r_0`$ long and $`\min(\ell,\ w\,\pi (r_0 + R) / n)`$ wide
//!   for the template's petal width $`w`$.
//! - A **fruit** or bud ([`Shape::Fruit`] with `cone` below a half) is an
//!   ellipsoid along the organ's heading, its length the organ's size and
//!   its width the template's aspect times that.
//! - A **cone** (`cone` of a half or more) is that ellipsoid, shrunk to a
//!   core, under scales packed on a golden-angle spiral (Vogel 1979), each
//!   a small wedge pointing toward the cone's tip.
//!
//! Each organ type's solids on a level are capped at [`TYPE_TRIANGLES`]:
//! a type with more organs than fit (a tree in full bloom) keeps its cards
//! on that level ([`fits`]).

use serde::{Deserialize, Serialize};

use crate::graph::{GraphOrgan, PlantGraph};
use crate::leaves::{self, Leaf, Section, SolidLeaf};
use crate::looks::{Look, Shape};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};

/// How an organ is drawn as a solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Form {
    /// A flower or a daisy head.
    Flower(FlowerForm),
    /// A fruit, a bud or a cone.
    Fruit(FruitForm),
    /// No solid: the card everywhere.
    Card,
}

/// A flower's shape beyond its template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowerForm {
    /// Degrees the petals rise from the plane square to the flower's axis:
    /// 0 a flat star, about 40 a bowl, 70 or more a cup or tube.
    pub cup: f64,
    /// Degrees each petal curls back over its length.
    pub curl: f64,
    /// Length of a fused tube before the petals, as a share of the
    /// flower's radius.
    pub tube: f64,
    /// Stamens round the centre.
    pub stamens: u32,
    /// Their length, as a share of the flower's radius.
    pub stamen_length: f64,
    /// Levels, from the nearest, drawn solid: 1 or 2.
    pub levels: u8,
}

impl Default for FlowerForm {
    fn default() -> Self {
        Self {
            cup: 30.0,
            curl: 12.0,
            tube: 0.0,
            stamens: 0,
            stamen_length: 0.35,
            levels: 1,
        }
    }
}

/// A fruit's shape beyond its template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FruitForm {
    /// Scales on a cone; 0 picks a count from the cone's size.
    pub scales: u32,
    /// Levels, from the nearest, drawn solid: 1 or 2.
    pub levels: u8,
}

impl Default for FruitForm {
    fn default() -> Self {
        Self {
            scales: 0,
            levels: 1,
        }
    }
}

/// Most triangles one organ type's solids take on a level.
pub const TYPE_TRIANGLES: usize = 200_000;

impl Form {
    /// The form a look of `shape` takes unless it sets its own: flowers
    /// and heads are floral diagrams, fruit, buds and cones solids;
    /// everything else is drawn as cards.
    #[must_use]
    pub fn default_for(shape: &Shape) -> Self {
        match shape {
            Shape::Flower(_) => Self::Flower(FlowerForm::default()),
            // A daisy's rays spread nearly flat round its disc.
            Shape::Head(_) => Self::Flower(FlowerForm {
                cup: 8.0,
                curl: 6.0,
                ..FlowerForm::default()
            }),
            Shape::Fruit(_) => Self::Fruit(FruitForm::default()),
            _ => Self::Card,
        }
    }

    /// Whether level `level` may draw the form solid.
    #[must_use]
    pub fn solid_at(&self, level: usize) -> bool {
        let levels = match self {
            Self::Flower(form) => form.levels,
            Self::Fruit(form) => form.levels,
            Self::Card => 0,
        };
        level < usize::from(levels.min(2))
    }
}

/// About how many triangles one organ of `look` takes as a solid.
fn triangles_per_organ(look: &Look) -> usize {
    match (&look.form, &look.shape) {
        (Form::Flower(form), Shape::Flower(flower)) => {
            flower.petals as usize * 24 + DOME_TRIANGLES + form.stamens as usize * 3
        }
        (Form::Flower(form), Shape::Head(head)) => {
            head.rays as usize * 24 + DOME_TRIANGLES + form.stamens as usize * 3
        }
        (Form::Fruit(form), Shape::Fruit(fruit)) if fruit.cone >= 0.5 => {
            ELLIPSOID_TRIANGLES + scales(form, fruit.aspect) as usize * WEDGE_TRIANGLES
        }
        (Form::Fruit(_), _) => ELLIPSOID_TRIANGLES,
        _ => 0,
    }
}

/// Triangles a cone scale takes.
const WEDGE_TRIANGLES: usize = 4;

/// The scales on a cone of width over length `aspect`: the form's, or
/// more on a slender cone, from 16 to 40.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scales(form: &FruitForm, aspect: f64) -> u32 {
    if form.scales > 0 {
        form.scales
    } else {
        (12.0 / aspect.clamp(0.05, 2.0)).clamp(16.0, 40.0) as u32
    }
}

/// Whether the organs of type `index` in `graph` fit [`TYPE_TRIANGLES`]
/// drawn as solids.
#[must_use]
pub fn fits(graph: &PlantGraph, look: &Look, index: usize) -> bool {
    let organs = graph
        .organs
        .iter()
        .filter(|organ| usize::from(organ.organ) == index)
        .count();
    organs * triangles_per_organ(look) <= TYPE_TRIANGLES
}

/// Draw every organ of `graph` whose type is in `solid` (by index) and
/// whose look has a flower or fruit form into `mesh`, coloured by `colour`
/// (the organ's card colour).
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    colour: &dyn Fn(&Look, &GraphOrgan) -> [f32; 3],
    mesh: &mut Mesh,
) {
    for organ in &graph.organs {
        let index = usize::from(organ.organ);
        let Some(look) = looks.get(index) else {
            continue;
        };
        if !solid.get(index).copied().unwrap_or(false) || look.solid.is_some() {
            continue;
        }
        let first = mesh.positions.len();
        let first_index = mesh.indices.len();
        let painted = colour(look, organ);
        let axis = organ.heading.normalize_or(Vec3::Y);
        let side = organ.left.normalize_or(any_perpendicular(axis));
        let part = Part {
            born: organ.born,
            shed: organ.shed,
        };
        match (&look.form, &look.shape) {
            (Form::Flower(form), Shape::Flower(flower)) => {
                let petals = Petals {
                    count: flower.petals.max(1),
                    width: flower.petal_width,
                    centre: flower.centre,
                    pointed: flower.point,
                };
                draw_flower(
                    organ,
                    axis,
                    side,
                    form,
                    &petals,
                    painted,
                    look.accent,
                    part,
                    mesh,
                );
            }
            (Form::Flower(form), Shape::Head(head)) => {
                let petals = Petals {
                    count: head.rays.max(1),
                    width: head.ray_width,
                    centre: head.disc,
                    pointed: 0.3,
                };
                draw_flower(
                    organ,
                    axis,
                    side,
                    form,
                    &petals,
                    painted,
                    look.accent,
                    part,
                    mesh,
                );
            }
            (Form::Fruit(form), Shape::Fruit(fruit)) => {
                draw_fruit(
                    organ,
                    axis,
                    side,
                    form,
                    fruit.aspect,
                    fruit.cone,
                    painted,
                    part,
                    mesh,
                );
            }
            _ => continue,
        }
        leaves::smooth_normals(mesh, first, first_index);
    }
}

/// When a part appears and is shed.
#[derive(Clone, Copy)]
struct Part {
    born: f64,
    shed: Option<f64>,
}

impl Part {
    fn leaf(
        self,
        base: Vec3,
        heading: Vec3,
        left: Vec3,
        length: f64,
        width: f64,
        colour: [f32; 3],
    ) -> Leaf {
        Leaf {
            base,
            heading,
            left,
            length,
            width,
            colour,
            born: self.born,
            shed: self.shed,
        }
    }
}

/// A flower's petals from its template.
struct Petals {
    count: u32,
    width: f64,
    centre: f64,
    pointed: f64,
}

const DOME_TRIANGLES: usize = 2 * 8 * 3;
// Two fans at the poles and a band between each pair of the six inner rings.
const ELLIPSOID_TRIANGLES: usize = 2 * 10 * (7 - 1);

#[allow(clippy::too_many_arguments)]
fn draw_flower(
    organ: &GraphOrgan,
    axis: Vec3,
    side: Vec3,
    form: &FlowerForm,
    petals: &Petals,
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let radius = organ.size * 0.5;
    if radius <= 0.0 {
        return;
    }
    let centre_radius = (petals.centre * radius).clamp(0.0, radius * 0.9);
    let start = 0.6 * centre_radius;
    let tube = form.tube.max(0.0) * radius;
    let base = organ.position + axis * tube;
    let length = (radius - start).max(radius * 0.1);
    let n = f64::from(petals.count);
    let width =
        (petals.width * std::f64::consts::PI * (start + radius) / n).clamp(length * 0.08, length);
    let cup = math::radians(form.cup);
    let other = axis.cross(side);
    let turn = unit(mix64(organ.id)) * std::f64::consts::TAU / n;
    let petal = SolidLeaf {
        section: Section::Flat,
        thickness: 0.05,
        fold: 0.0,
        widest: 0.55,
        base: 0.35,
        taper: if petals.pointed >= 0.5 { 1.3 } else { 0.45 },
        arch: form.curl,
        twist: 0.0,
        margin: None,
        teeth: None,
        spine: None,
        levels: 1,
    };
    for k in 0..petals.count {
        let angle = turn + std::f64::consts::TAU * f64::from(k) / n;
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let heading = (radial * math::cos(cup) + axis * math::sin(cup)).normalize_or(radial);
        // The petal's face toward the flower's axis, so its curl bends it
        // outward and back.
        let mut left = axis.cross(radial).normalize_or(side);
        if heading.cross(left).dot(axis) < 0.0 {
            left = -left;
        }
        let leaf = part.leaf(base + radial * start, heading, left, length, width, colour);
        let (stations, across) = leaves::detail_of(0, length, width);
        leaf.draw(&petal, stations.min(6), across.min(2), mesh);
    }
    if tube > 0.0 {
        frustum(
            organ.position,
            axis,
            side,
            tube,
            start * 0.6,
            start,
            colour,
            part,
            mesh,
        );
    }
    if centre_radius > 0.0 {
        dome(
            base,
            axis,
            side,
            centre_radius,
            centre_radius * 0.5,
            accent,
            part,
            mesh,
        );
    }
    let stamen = form.stamen_length * radius;
    for k in 0..form.stamens {
        let angle =
            std::f64::consts::TAU * f64::from(k) / f64::from(form.stamens.max(1)) + turn * 0.5;
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let direction = (axis + radial * 0.35).normalize_or(axis);
        let leaf = part.leaf(
            base + radial * (centre_radius * 0.5),
            direction,
            axis.cross(radial).normalize_or(side),
            stamen,
            stamen * 0.06,
            accent,
        );
        leaves::cone(
            &leaf,
            leaf.base,
            direction,
            leaf.left,
            direction.cross(leaf.left),
            stamen,
            stamen * 0.03,
            accent,
            mesh,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_fruit(
    organ: &GraphOrgan,
    axis: Vec3,
    side: Vec3,
    form: &FruitForm,
    aspect: f64,
    cone: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    if length <= 0.0 {
        return;
    }
    let half_length = length * 0.5;
    let half_width = length * aspect.clamp(0.05, 2.0) * 0.5;
    let centre = organ.position + axis * half_length;
    if cone < 0.5 {
        ellipsoid(
            centre,
            axis,
            side,
            half_length,
            half_width,
            colour,
            part,
            mesh,
        );
        return;
    }
    // A cone: a core and spiral scales over it, pointing toward the tip.
    let core = 0.6;
    ellipsoid(
        centre,
        axis,
        side,
        half_length * 0.95,
        half_width * core,
        shade(colour, 0.8),
        part,
        mesh,
    );
    let count = scales(form, aspect);
    let other = axis.cross(side);
    let golden = math::radians(137.507_764);
    for i in 0..count {
        let t = (f64::from(i) + 0.5) / f64::from(count);
        let z = -1.0 + 2.0 * t;
        let ring = math::sqrt((1.0 - z * z).max(0.0));
        let angle = golden * f64::from(i);
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let on_core =
            centre + axis * (z * half_length * 0.95) + radial * (ring * half_width * core);
        let reach = half_width * (1.0 - core) * 1.6 * (0.4 + 0.6 * ring);
        let tip = on_core + (radial * 0.8 + axis * 0.6).normalize_or(radial) * reach;
        let across = axis.cross(radial).normalize_or(side) * (reach * 0.7);
        wedge(
            on_core - across,
            on_core + across,
            tip,
            radial * (reach * 0.25),
            colour,
            part,
            mesh,
        );
    }
}

/// An ellipsoid of half-length `a` along `axis` and half-width `b`.
#[allow(clippy::too_many_arguments)]
fn ellipsoid(
    centre: Vec3,
    axis: Vec3,
    side: Vec3,
    a: f64,
    b: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 10;
    const RINGS: u32 = 7;
    let other = axis.cross(side);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let leaf = part.leaf(centre, axis, side, 0.0, 0.0, colour);
    mesh.push(leaf.vertex(centre - axis * a, colour));
    for r in 1..RINGS {
        let phi = std::f64::consts::PI * f64::from(r) / f64::from(RINGS);
        let along = -math::cos(phi) * a;
        let out = math::sin(phi) * b;
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(centre + axis * along + radial * out, colour));
        }
    }
    let top = mesh.push(leaf.vertex(centre + axis * a, colour));
    let at = |r: u32, k: u32| first + 1 + (r - 1) * AROUND + k % AROUND;
    for k in 0..AROUND {
        mesh.indices
            .extend_from_slice(&[first, at(1, k + 1), at(1, k)]);
        mesh.indices
            .extend_from_slice(&[top, at(RINGS - 1, k), at(RINGS - 1, k + 1)]);
    }
    for r in 1..RINGS - 1 {
        for k in 0..AROUND {
            let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
}

/// A dome over `base` along `axis`: `radius` wide, `height` tall.
#[allow(clippy::too_many_arguments)]
fn dome(
    base: Vec3,
    axis: Vec3,
    side: Vec3,
    radius: f64,
    height: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 8;
    const RINGS: u32 = 3;
    let other = axis.cross(side);
    let leaf = part.leaf(base, axis, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for r in 0..RINGS {
        let phi = std::f64::consts::FRAC_PI_2 * f64::from(r) / f64::from(RINGS);
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(
                base + radial * (radius * math::cos(phi)) + axis * (height * math::sin(phi)),
                colour,
            ));
        }
    }
    let top = mesh.push(leaf.vertex(base + axis * height, colour));
    let at = |r: u32, k: u32| first + r * AROUND + k % AROUND;
    for r in 0..RINGS - 1 {
        for k in 0..AROUND {
            let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
    for k in 0..AROUND {
        mesh.indices
            .extend_from_slice(&[at(RINGS - 1, k), at(RINGS - 1, k + 1), top]);
    }
}

/// An open frustum along `axis` from `base`, `length` long, from radius
/// `r0` to `r1`.
#[allow(clippy::too_many_arguments)]
fn frustum(
    base: Vec3,
    axis: Vec3,
    side: Vec3,
    length: f64,
    r0: f64,
    r1: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 8;
    let other = axis.cross(side);
    let leaf = part.leaf(base, axis, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for (along, radius) in [(0.0, r0), (length, r1)] {
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(base + axis * along + radial * radius, colour));
        }
    }
    for k in 0..AROUND {
        let (a0, a1) = (first + k, first + (k + 1) % AROUND);
        let (b0, b1) = (a0 + AROUND, a1 + AROUND);
        mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
    }
}

/// A scale: a wedge on the edge `a`–`b` reaching to `tip`, `lift` thick.
fn wedge(a: Vec3, b: Vec3, tip: Vec3, lift: Vec3, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
    let leaf = part.leaf(a, Vec3::Y, Vec3::X, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let middle = (a + b) * 0.5 + lift;
    for corner in [a, b, tip, middle] {
        mesh.push(leaf.vertex(corner, shade(colour, 1.0)));
    }
    let v = |k: u32| first + k;
    mesh.indices.extend_from_slice(&[
        v(0),
        v(1),
        v(2),
        v(1),
        v(3),
        v(2),
        v(3),
        v(0),
        v(2),
        v(0),
        v(3),
        v(1),
    ]);
}

fn shade(colour: [f32; 3], factor: f32) -> [f32; 3] {
    colour.map(|channel| (channel * factor).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::looks::{Flower, Fruit};

    fn organ(size: f64) -> GraphOrgan {
        GraphOrgan {
            id: 7,
            organ: 0,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size,
            born: 2.0,
            shed: Some(3.0),
            light: 1.0,
        }
    }

    #[test]
    fn a_flower_has_its_petals_centre_and_stamens_within_its_size() {
        let mut mesh = Mesh::default();
        let form = FlowerForm {
            stamens: 6,
            ..FlowerForm::default()
        };
        let petals = Petals {
            count: 5,
            width: 1.0,
            centre: 0.2,
            pointed: 0.0,
        };
        let part = Part {
            born: 2.0,
            shed: Some(3.0),
        };
        draw_flower(
            &organ(0.05),
            Vec3::Y,
            Vec3::X,
            &form,
            &petals,
            [0.9, 0.9, 0.9],
            [0.9, 0.7, 0.1],
            part,
            &mut mesh,
        );
        assert!(mesh.triangle_count() > 5 * 8, "petals, a dome and stamens");
        for position in &mesh.positions {
            let p = Vec3::new(
                f64::from(position[0]),
                f64::from(position[1]),
                f64::from(position[2]),
            );
            assert!(p.length() <= 0.05, "within the flower's diameter: {p:?}");
            assert!(p.y >= -0.01, "the flower opens along its axis: {p:?}");
        }
        assert!(mesh.births.iter().all(|&b| (b - 2.0).abs() < 1e-6));
    }

    #[test]
    fn a_fruit_is_an_ellipsoid_along_its_stalk_and_a_cone_has_scales() {
        let part = Part {
            born: 0.0,
            shed: None,
        };
        let mut berry = Mesh::default();
        draw_fruit(
            &organ(0.02),
            Vec3::Y,
            Vec3::X,
            &FruitForm::default(),
            0.8,
            0.0,
            [0.3, 0.1, 0.4],
            part,
            &mut berry,
        );
        assert_eq!(berry.triangle_count(), ELLIPSOID_TRIANGLES);
        let top = berry
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!(
            (top - 0.02).abs() < 1e-6,
            "the fruit reaches its length along the stalk"
        );
        let mut cone = Mesh::default();
        draw_fruit(
            &organ(0.08),
            Vec3::Y,
            Vec3::X,
            &FruitForm::default(),
            0.45,
            1.0,
            [0.4, 0.25, 0.1],
            part,
            &mut cone,
        );
        assert!(cone.triangle_count() >= ELLIPSOID_TRIANGLES + 20 * 4);
    }

    #[test]
    fn forms_default_by_shape_and_level() {
        assert!(matches!(
            Form::default_for(&Shape::Flower(Flower::default())),
            Form::Flower(_)
        ));
        assert!(matches!(
            Form::default_for(&Shape::Fruit(Fruit::default())),
            Form::Fruit(_)
        ));
        let form = Form::default_for(&Shape::Fruit(Fruit::default()));
        assert!(form.solid_at(0) && !form.solid_at(1));
        assert!(!Form::Card.solid_at(0));
    }
}
