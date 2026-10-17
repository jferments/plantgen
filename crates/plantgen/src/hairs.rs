//! Hairs on an organ's face, its indumentum ([`crate::looks::Hairs`]),
//! drawn the three ways a renderer sees them at different distances.
//!
//! - **On the card**, where a hair is far thinner than a texel: a soft
//!   cast of the face's colour toward the hairs' (a felt hides the face,
//!   a few hairs barely tint it; [`cast`]), and a sheen ([`sheen`]):
//!   hairs catch light where the
//!   face is seen edge on and glow where the sun is behind them, as a
//!   velvet does (asperity scattering; Koenderink and Pont, 2003). Both
//!   are the same over the face, so a renderer needs only the template's
//!   hairs (`crate::package::AtlasLayer::hairs`), no texels of their own.
//! - **Along the edge**, a fringe of hairs painted out from the outline
//!   ([`paint`]), pale where they catch the light and show the face's
//!   colour through them, which shows a hairy leaf's soft rim against what
//!   lies behind it. Floras say so for a ciliate edge; every hairy blade has
//!   some. The fringe is no part of the leaf's area, and a renderer leaves
//!   it out where the strands draw the edge's hairs ([`fringe_drawn`]).
//! - **Up close**, where a hair is longer than a couple of pixels, each
//!   hair a strand of its own ([`strand_mesh`]): a thin tapering solid
//!   standing from the face as the hairs' lie and curl set, drawn at least
//!   half a pixel wide, as a camera's blur draws a hair thinner than its
//!   pixel, and lit as a fibre is (Kajiya and Kay, 1989), along its length
//!   rather than across a face.

// Texel and strand counts are small whole numbers, and texel positions
// small signed ones: casts between them and to `f64` lose nothing.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::looks::Hairs;
use crate::math::{self, Vec3};
use crate::mesh::{Card, Mesh, PlantMesh, Vertex};
use crate::raster::Lighting;
use crate::rng::{mix64, unit};

/// How far a face's hairs move its colour toward theirs at most, under a
/// felt that hides it; a fringe's own texels are all hair. The cast goes
/// with the cube of their density ([`cast`]): a few clear hairs barely
/// tint a face, a felt or a wool whitens it.
pub const CAST: f64 = 0.45;

/// The hair weight of a face whose hairs hide `density` of it (see
/// [`CAST`]).
#[must_use]
pub fn cast(density: f64) -> f64 {
    let density = density.clamp(0.0, 1.0);
    CAST * density * density * density
}

/// How much brighter than the face a fringe's hair is: clear hairs show
/// the face's colour through them and catch the light.
pub const FRINGE_BRIGHTNESS: f32 = 1.5;

/// Strands one plant draws at most ([`strand_mesh`]), each nine
/// triangles: a close-up's budget.
pub const MAX_STRANDS: usize = 60_000;

/// A number in `[0, 1)` from two small integers and a salt.
fn hash(a: u64, b: u64, salt: u64) -> f64 {
    unit(mix64(mix64(a ^ salt.rotate_left(17)) ^ b.rotate_left(31)))
}

/// Paint `hairs` onto a `size` by `size` template of a card `aspect` wide
/// for its length, in place: a fringe of hairs out from its edge into the
/// texels around it, each a
/// stroke a texel wide standing out from the outline and curving toward
/// the tip (FNA's "marginal hairs perpendicular to margin or curved toward
/// apex", Asarum). Texels within `border` of the template's sides and tip
/// stay clear. Returns the coverage the fringe added to each texel, which
/// the leaf's area leaves out.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn paint(
    size: usize,
    aspect: f64,
    border: usize,
    hairs: &Hairs,
    seed: u64,
    coverage: &mut [f32],
    brightness: &mut [f32],
    accent: &mut [f32],
) -> Vec<f32> {
    let texels = size * size;
    let mut fringe = vec![0.0_f32; texels];
    if !hairs.drawn() || size < 4 {
        return fringe;
    }
    let density = hairs.density.clamp(0.0, 1.0);
    let lie = hairs.lie.clamp(0.0, 1.0);
    let curl = hairs.curl.clamp(0.0, 1.0);
    // The fringe, from the edge texels: covered, beside one that is not.
    let covered = |x: isize, y: isize| -> f64 {
        if x < 0 || y < 0 || x >= size as isize || y >= size as isize {
            return 0.0;
        }
        f64::from(coverage[y as usize * size + x as usize])
    };
    let edge = hairs.edge.unwrap_or(density).clamp(0.0, 1.0);
    // Physical units: x across in card lengths (`aspect` wide), y along.
    let across = aspect / size as f64;
    let along = 1.0 / size as f64;
    // A stroke reaches out one and a half to three texels, longer for
    // longer hairs: a texel is a millimetre or so of a leaf's card.
    let reach = (hairs.length / 0.0015).clamp(0.6, 2.0);
    let mut strokes = Vec::new();
    for y in 1..size as isize - 1 {
        for x in 1..size as isize - 1 {
            if covered(x, y) < 0.5 {
                continue;
            }
            let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| covered(x + dx, y + dy) < 0.5);
            if !open {
                continue;
            }
            // About one edge texel in three bears a hair on a fully
            // fringed edge.
            if hash(x as u64, y as u64, seed ^ 0x4652_494e) >= edge * 0.4 {
                continue;
            }
            // The outward normal: down the slope of coverage round it.
            let mut gx = 0.0;
            let mut gy = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = covered(x + dx, y + dy);
                    gx += f64::from(dx as i32) * c;
                    gy += f64::from(dy as i32) * c;
                }
            }
            let (nx, ny) = (-gx / across, -gy / along);
            let norm = math::sqrt(nx * nx + ny * ny);
            if norm < 1e-9 {
                continue;
            }
            let (nx, ny) = (nx / norm, ny / norm);
            // Curved toward the tip, more for hairs that lie flat.
            let jitter = hash(x as u64, y as u64, seed ^ 0x4355_524c) - 0.5;
            let lean = (0.25 + 0.5 * (1.0 - lie)) + jitter * (0.4 + curl);
            let (dx, dy) = (nx, ny + lean.max(0.0) * (1.0 - ny.abs()));
            let length = (1.5 + 1.5 * hash(x as u64, y as u64, seed ^ 0x4c45_4e47)) * reach * along;
            let norm = math::sqrt(dx * dx + dy * dy);
            strokes.push((x, y, dx / norm, dy / norm, length));
        }
    }
    let low = border.max(1) as f64;
    let high_x = (size - border.max(1)) as f64 - 1.0;
    let high_y = (size - border.max(1)) as f64 - 1.0;
    for (x, y, dx, dy, length) in strokes {
        let steps = ((length / along) * 4.0).ceil().max(1.0) as usize;
        for step in 1..=steps {
            let t = length * step as f64 / steps as f64;
            let px = x as f64 + dx * t / across;
            let py = y as f64 + dy * t / along;
            if px < low || py < low || px > high_x || py > high_y {
                break;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let at = py.round() as usize * size + px.round() as usize;
            if coverage[at] >= 0.5 {
                continue;
            }
            fringe[at] += 1.0 - coverage[at];
            coverage[at] = 1.0;
            brightness[at] = FRINGE_BRIGHTNESS;
            accent[at] = 0.0;
        }
    }
    fringe
}

/// The light a face's hairs add, seen along `view` with its normal
/// `normal` turned
/// toward the eye: a sheen where the face is seen edge on, stronger for
/// hairs pressed flat (silky, sericeous), and a glow where the sun shines
/// through them toward the eye, stronger for hairs standing up (a backlit
/// halo), in `lighting`; `lit` is the sun's share that reaches the point.
#[must_use]
pub fn sheen(hairs: &Hairs, normal: Vec3, view: Vec3, lit: f64, lighting: &Lighting) -> [f64; 3] {
    let (sun, sun_colour, sky) = (lighting.sun, lighting.sun_color, lighting.sky);
    // It goes with the square of how much of the face the hairs cover: a
    // few hairs catch little light, a felt shines.
    let density = hairs.density.clamp(0.0, 1.0);
    let weight = density * density;
    let lie = hairs.lie.clamp(0.0, 1.0);
    let edge_on = 1.0 - normal.dot(view).abs().min(1.0);
    let graze = edge_on * edge_on * edge_on;
    let behind = (-view.dot(sun)).max(0.0);
    let glow = behind * behind * behind * behind;
    let sunlit = lit * (0.5 * (1.0 - 0.5 * lie) * graze + 0.9 * (0.4 + 0.6 * lie) * glow);
    let skylit = 0.25 * graze;
    std::array::from_fn(|channel| {
        f64::from(hairs.colour[channel])
            * weight
            * (f64::from(sun_colour[channel]) * sunlit + f64::from(sky[channel]) * skylit)
    })
}

/// Whether a card draws its edge's painted fringe at a point where a
/// pixel is `pixel` wide: only where its hairs are too short to draw as
/// strands ([`strand_mesh`], at least two pixels long).
#[must_use]
pub fn fringe_drawn(hairs: &Hairs, pixel: f64) -> bool {
    hairs.length < 2.0 * pixel
}

/// The light on a strand with unit tangent `tangent`, seen along `view`
/// toward the eye in `lighting`, `lit` the sun's share reaching it, in a
/// colour `albedo`: a fibre's diffuse light, strongest across its
/// length, its glint, and its glow when the sun shines through it.
#[must_use]
pub fn strand_light(
    albedo: [f64; 3],
    tangent: Vec3,
    view: Vec3,
    lit: f64,
    lighting: &Lighting,
) -> [f64; 3] {
    let (sun, sun_colour, sky, ground) = (
        lighting.sun,
        lighting.sun_color,
        lighting.sky,
        lighting.ground,
    );
    let across = |d: Vec3| math::sqrt((1.0 - tangent.dot(d).powi(2)).max(0.0));
    let diffuse = across(sun) * lit;
    let half = (sun + view).normalize_or(sun);
    let glint = across(half).powi(60) * lit;
    let behind = (-view.dot(sun)).max(0.0);
    let glow = behind.powi(4) * lit;
    std::array::from_fn(|channel| {
        albedo[channel]
            * (f64::from(sun_colour[channel]) * (0.6 * diffuse + 1.2 * glow)
                + 0.75 * f64::from(sky[channel])
                + 0.25 * f64::from(ground[channel]))
            + f64::from(sun_colour[channel]) * 0.2 * glint
    })
}

/// A bent card's point at `u` across (0 left, 1 right) and `v` along, and
/// its unit normal on the face side: as [`PlantMesh::card_mesh`] lays it.
fn card_point(card: &Card, u: f64, v: f64) -> (Vec3, Vec3) {
    let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    card.bend.point(
        vector(card.base),
        vector(card.heading),
        vector(card.left),
        f64::from(card.width) * 0.5,
        f64::from(card.length),
        2.0 * u - 1.0,
        v,
    )
}

/// One strand, before it is meshed: its root, its two directions (the
/// second for its far half), its length and its root's radius.
struct Strand {
    root: Vec3,
    first: Vec3,
    second: Vec3,
    length: f64,
    radius: f64,
}

/// An edge texel: its `u`, `v` and its outward normal across and along
/// the card, in card lengths.
type Edge = (f64, f64, f64, f64);

/// The edge texels of a template with their outward normals, in the
/// card's `u`, `v` (each from 0 to 1) and in card lengths: where the fringe
/// starts, and up close where edge strands do.
fn edges(templates: &crate::templates::Templates, template: usize) -> Vec<Edge> {
    let Some(drawn) = templates.templates.get(template) else {
        return Vec::new();
    };
    let size = templates.size;
    let solid = |x: isize, y: isize| -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < size
            && (y as usize) < size
            && drawn.coverage[y as usize * size + x as usize] >= 0.5
            && drawn
                .fringe
                .get(y as usize * size + x as usize)
                .copied()
                .unwrap_or(0.0)
                <= 0.0
    };
    let mut out = Vec::new();
    for y in 0..size as isize {
        for x in 0..size as isize {
            if !solid(x, y) {
                continue;
            }
            let mut gx = 0.0;
            let mut gy = 0.0;
            let mut open = false;
            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    let s = solid(x + dx, y + dy);
                    if !s && (dx == 0 || dy == 0) {
                        open = true;
                    }
                    let c = if s { 1.0 } else { 0.0 };
                    gx += dx as f64 * c;
                    gy += dy as f64 * c;
                }
            }
            if !open {
                continue;
            }
            let (nx, ny) = (-gx / drawn.aspect, -gy);
            let norm = math::sqrt(nx * nx + ny * ny);
            if norm < 1e-9 {
                continue;
            }
            out.push((
                (x as f64 + 0.5) / size as f64,
                (y as f64 + 0.5) / size as f64,
                nx / norm,
                ny / norm,
            ));
        }
    }
    out
}

/// Every hair on the bent cards of `plant` (the levels that keep a card
/// per organ) whose look has hairs, as a strand at least two pixels long
/// where `pixel` gives a pixel's width at a point: a tapering three-sided
/// solid of nine triangles, its vertex normals its tangent (for
/// [`strand_light`]), in its hairs' colour. At most [`MAX_STRANDS`], an
/// even share of each card's kept where there would be more.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn strand_mesh(
    plant: &PlantMesh,
    templates: &crate::templates::Templates,
    pixel: &dyn Fn(Vec3) -> f64,
) -> Mesh {
    let bent = plant.bent_cards();
    let mut edge_lists: Vec<Option<Vec<Edge>>> = vec![None; templates.templates.len()];
    // Count first, to share the budget out.
    let wanted: Vec<(usize, f64, f64)> = plant.cards[..bent]
        .iter()
        .map(|card| {
            let Some(hairs) = templates
                .templates
                .get(usize::from(card.template))
                .and_then(|template| template.hairs.as_ref())
            else {
                return (0, 0.0, 0.0);
            };
            let template = &templates.templates[usize::from(card.template)];
            let area = f64::from(card.length) * f64::from(card.length) * template.area();
            let spacing = (hairs.length * 0.9).max(1e-4);
            let face = hairs.density * area / (spacing * spacing);
            let perimeter = 3.5 * math::sqrt(area);
            let edge = hairs.edge.unwrap_or(hairs.density) * perimeter / (spacing * 0.7);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            ((face + edge).round() as usize, face, edge)
        })
        .collect();
    let total: usize = wanted.iter().map(|(n, _, _)| n).sum();
    let share = if total > MAX_STRANDS {
        MAX_STRANDS as f64 / total as f64
    } else {
        1.0
    };
    let mut mesh = Mesh::default();
    let mut strands = Vec::new();
    for (index, card) in plant.cards[..bent].iter().enumerate() {
        let (count, face, edge) = wanted[index];
        if count == 0 {
            continue;
        }
        let template = usize::from(card.template);
        let drawn = &templates.templates[template];
        let Some(hairs) = drawn.hairs.as_ref() else {
            continue;
        };
        // Too short to see: none drawn one by one.
        let (middle, _) = card_point(card, 0.5, 0.5);
        if fringe_drawn(hairs, pixel(middle)) {
            continue;
        }
        let seed = mix64(index as u64 ^ 0x5354_5241_4e44);
        let lie = hairs.lie.clamp(0.0, 1.0);
        let curl = hairs.curl.clamp(0.0, 1.0);
        strands.clear();
        let length_of = |k: u64| hairs.length * (0.6 + 0.8 * hash(k, 1, seed));
        let turn = |k: u64, salt: u64| hash(k, salt, seed) - 0.5;
        // Hairs on the face, at random points the template covers.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let on_face = (face * share).round() as u64;
        let mut tries = 0_u64;
        let mut placed = 0_u64;
        while placed < on_face && tries < on_face * 4 + 16 {
            tries += 1;
            let (u, v) = (hash(tries, 2, seed), hash(tries, 3, seed));
            #[allow(clippy::cast_possible_truncation)]
            let texel = templates.sample(template, u as f32, v as f32);
            if texel.coverage < 0.5 || texel.fringe {
                continue;
            }
            placed += 1;
            let (root, normal) = card_point(card, u, v);
            let (ahead, _) = card_point(card, u, (v + 0.01).min(1.0));
            let (behind, _) = card_point(card, u, (v - 0.01).max(0.0));
            let tipward = (ahead - behind).normalize_or(math::any_perpendicular(normal));
            let side = normal
                .cross(tipward)
                .normalize_or(math::any_perpendicular(tipward));
            let rise = math::radians((lie * 80.0 + turn(tries, 4) * 24.0).clamp(3.0, 88.0));
            let swing = math::radians(turn(tries, 5) * (40.0 + curl * 120.0));
            let first = (tipward * (math::cos(rise) * math::cos(swing))
                + side * (math::cos(rise) * math::sin(swing))
                + normal * math::sin(rise))
            .normalize_or(normal);
            // Soft, long hairs bow over; curled ones turn on.
            let bow = (1.0 - lie) * 0.3 + curl * 0.8;
            let second = (first
                + side * (turn(tries, 6) * 2.0 * curl)
                + (tipward - normal * 0.5) * (bow * 0.5))
                .normalize_or(first);
            strands.push(Strand {
                root: root + normal * 1e-5,
                first,
                second,
                length: length_of(tries),
                radius: 0.0,
            });
        }
        // Hairs along the edge, standing out of it in the face's plane.
        if edge > 0.0 {
            let list = edge_lists[template].get_or_insert_with(|| edges(templates, template));
            if !list.is_empty() {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let on_edge = (edge * share).round() as u64;
                for k in 0..on_edge {
                    let pick = hash(k, 7, seed) * list.len() as f64;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let (u, v, nx, ny) = list[(pick as usize).min(list.len() - 1)];
                    let (root, normal) = card_point(card, u, v);
                    let (ahead, _) = card_point(card, u, (v + 0.01).min(1.0));
                    let (behind, _) = card_point(card, u, (v - 0.01).max(0.0));
                    let (right, _) = card_point(card, (u + 0.01).min(1.0), v);
                    let (left, _) = card_point(card, (u - 0.01).max(0.0), v);
                    let tipward = (ahead - behind).normalize_or(math::any_perpendicular(normal));
                    let rightward = (right - left).normalize_or(math::any_perpendicular(tipward));
                    let out = (rightward * nx + tipward * ny).normalize_or(rightward);
                    let lean = 0.25 + 0.5 * (1.0 - lie) + turn(k, 8) * (0.4 + curl);
                    let first =
                        (out + tipward * lean.max(0.0) + normal * (0.15 * lie)).normalize_or(out);
                    let second =
                        (first + tipward * (0.3 * (1.0 - lie)) + normal * turn(k, 9) * curl)
                            .normalize_or(first);
                    strands.push(Strand {
                        root,
                        first,
                        second,
                        length: length_of(k + 1_000_003),
                        radius: 0.0,
                    });
                }
            }
        }
        // A hair is clear: it shows the face's green through it.
        let colour: [f32; 3] =
            std::array::from_fn(|c| 0.45 * hairs.colour[c] + 0.55 * card.color[c].max(0.0));
        let lifetime = (
            f64::from(card.born),
            card.shed.is_finite().then_some(f64::from(card.shed)),
        );
        for strand in &mut strands {
            // Real hairs are a fiftieth of their length thick or less;
            // narrower than half a pixel, a camera blurs them to that.
            strand.radius = (strand.length * 0.012).max(0.5 * pixel(strand.root));
            push_strand(&mut mesh, strand, colour, lifetime, card.segment);
        }
    }
    mesh
}

/// A strand's solid: a ring of three at its root, a narrower ring at its
/// middle, and its tip; nine triangles.
fn push_strand(
    mesh: &mut Mesh,
    strand: &Strand,
    colour: [f32; 3],
    (born, shed): (f64, Option<f64>),
    segment: u32,
) {
    let half = strand.length * 0.5;
    let middle = strand.root + strand.first * half;
    let tip = middle + strand.second * half;
    let ring = |centre: Vec3, along: Vec3, radius: f64| -> [Vec3; 3] {
        let a = math::any_perpendicular(along);
        let b = along.cross(a);
        std::array::from_fn(|k| {
            let angle = 2.0 * math::PI * k as f64 / 3.0;
            centre + (a * math::cos(angle) + b * math::sin(angle)) * radius
        })
    };
    let roots = ring(strand.root, strand.first, strand.radius);
    let middles = ring(
        middle,
        (strand.first + strand.second).normalize_or(strand.first),
        strand.radius * 0.7,
    );
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    let mut push = |position: Vec3, tangent: Vec3| {
        mesh.push(Vertex {
            position,
            normal: tangent,
            uv: [0.0, 0.0],
            color: [colour[0], colour[1], colour[2], 1.0],
            born,
            shed,
            level: 3,
            segment,
        });
    };
    for p in roots {
        push(p, strand.first);
    }
    for p in middles {
        push(p, strand.second);
    }
    push(tip, strand.second);
    for k in 0..3_u32 {
        let next = (k + 1) % 3;
        mesh.indices.extend_from_slice(&[
            first + k,
            first + next,
            first + 3 + next,
            first + k,
            first + 3 + next,
            first + 3 + k,
            first + 3 + k,
            first + 3 + next,
            first + 6,
        ]);
    }
}

#[cfg(test)]
// Tests check exact results: copied and cleared values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::looks::{Look, Shape, Simple};
    use crate::templates::Templates;

    fn leaf(hairs: Option<Hairs>) -> Look {
        let mut look = crate::looks::resolve(
            [("leaf", crate::lsys::OrganKind::Leaf)],
            &std::collections::BTreeMap::new(),
            [0.1, 0.2, 0.05],
            [0.03, 0.06, 0.02],
        )
        .remove(0);
        look.shape = Shape::Simple(Simple::default());
        look.hairs = hairs;
        look
    }

    fn villous() -> Hairs {
        Hairs {
            density: 0.6,
            length: 0.0025,
            lie: 0.5,
            curl: 0.45,
            ..Hairs::default()
        }
    }

    #[test]
    fn hairs_tint_the_face_and_fringe_the_edge_but_add_no_leaf() {
        let bare = Templates::for_looks(&[leaf(None)]);
        let hairy = Templates::for_looks(&[leaf(Some(villous()))]);
        let (bare_one, hairy_one) = (&bare.templates[0], &hairy.templates[0]);
        assert!(bare_one.fringe.is_empty() && bare_one.hair() == 0.0);
        // The fringe covers texels the bare blade leaves open, and only
        // those: the blade's own texels are the same.
        let fringed = hairy_one.fringe.iter().filter(|&&f| f > 0.0).count();
        assert!(fringed > 20, "{fringed} fringe texels");
        for at in 0..bare_one.coverage.len() {
            if bare_one.coverage[at] >= 0.5 {
                assert_eq!(hairy_one.coverage[at], bare_one.coverage[at]);
                assert_eq!(hairy_one.fringe[at], 0.0);
            } else if hairy_one.fringe[at] > 0.0 {
                assert_eq!(hairy_one.brightness[at], FRINGE_BRIGHTNESS);
            }
        }
        assert!((hairy_one.area() - bare_one.area()).abs() < 1e-9);
        let weight = hairy_one.hair();
        assert!(weight > 0.05 && f64::from(weight) <= CAST);
        // A hairy face is paler: its colour moves toward the hairs'.
        let middle = hairy.sample(0, 0.5, 0.5);
        assert!(middle.coverage >= 0.5 && !middle.fringe && middle.hair == weight);
        let tinted = hairy.albedo(0, [0.1, 0.2, 0.05], middle);
        let plain = bare.albedo(0, [0.1, 0.2, 0.05], bare.sample(0, 0.5, 0.5));
        assert!(tinted[0] > plain[0] && tinted[2] > plain[2]);
    }

    #[test]
    fn a_hairy_face_shines_edge_on_and_glows_against_the_sun() {
        let hairs = villous();
        let sun = Vec3::new(0.0, 1.0, 0.0);
        let up = Vec3::new(0.0, 1.0, 0.0);
        let lighting = Lighting {
            sun,
            sun_color: [1.0; 3],
            sky: [0.3; 3],
            ground: [0.1; 3],
        };
        let face_on = sheen(&hairs, up, up, 1.0, &lighting);
        let edge_on = sheen(
            &hairs,
            up,
            Vec3::new(1.0, 0.05, 0.0).normalize_or(up),
            1.0,
            &lighting,
        );
        let backlit = sheen(
            &hairs,
            up,
            Vec3::new(0.3, -1.0, 0.0).normalize_or(up),
            1.0,
            &lighting,
        );
        assert!(face_on[1] < 1e-3);
        assert!(edge_on[1] > 0.05 && backlit[1] > 0.05);
        let bare = Hairs {
            density: 0.0,
            ..villous()
        };
        assert_eq!(sheen(&bare, up, up, 1.0, &lighting), [0.0; 3]);
    }

    #[test]
    fn strands_stand_up_close_and_not_far_away() {
        use crate::mesh::Card;
        let templates = Templates::for_looks(&[leaf(Some(villous()))]);
        let card = Card {
            base: [0.0; 3],
            heading: [0.0, 0.0, 1.0],
            left: [1.0, 0.0, 0.0],
            length: 0.1,
            width: 0.06,
            color: [0.1, 0.2, 0.05],
            template: 0,
            born: 0.0,
            shed: f32::INFINITY,
            bend: crate::bend::Bend {
                cup: 0.1,
                ..crate::bend::Bend::FLAT
            },
            segment: crate::mesh::NO_SEGMENT,
        };
        let plant = PlantMesh {
            cards: vec![card],
            ..PlantMesh::default()
        };
        assert_eq!(plant.bent_cards(), 1);
        let near = strand_mesh(&plant, &templates, &|_| 0.0001);
        let far = strand_mesh(&plant, &templates, &|_| 0.01);
        assert!(far.indices.is_empty());
        let strands = near.indices.len() / 27;
        assert!(strands > 100, "{strands} strands");
        assert_eq!(near.indices.len() % 27, 0);
        assert!(strands <= MAX_STRANDS);
        // Every hair stands on the face, the side `heading × left` points
        // to (up), within its length of the card, which cups a few
        // millimetres.
        let reach = villous().length * 1.4 + 0.004;
        for p in &near.positions {
            let y = f64::from(p[1]);
            assert!(y <= reach && y >= -1e-3, "{p:?}");
        }
    }

    #[test]
    fn a_species_stating_its_hairs_draws_them() {
        let spec = crate::spec::PlantSpec::builtin("asarum-canadense").unwrap();
        let hairs = spec.appearance.organs["leaf"].hairs.as_ref().unwrap();
        // Strigose (FNA: appressed-hirsute), and ciliate.
        assert!((hairs.density - 0.45).abs() < 1e-9 && hairs.lie < 0.1);
        assert_eq!(hairs.edge, Some(0.8));
    }
}
