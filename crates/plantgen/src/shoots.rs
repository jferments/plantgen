//! Needle shoots drawn as solids (plant roadmap P4).
//!
//! A [`Needles`] look's card paints a conifer shoot: a central axis and side
//! twigs set with needles in two flat ranks or all round, singly or in
//! fascicles (`crate::templates`). The solid (`draw`) is the same shoot
//! from the same layout, so the card is the solid seen face on.
//!
//! In the card's frame, lengths in units of its length, `x` across it
//! toward its left, `y` along it and `z` out of it, an axis leaves
//! $`\mathbf s`$ along the unit direction $`\mathbf d`$ in the card's
//! plane, and its needle $`i`$ leaves the axis at
//! $`\mathbf b = \mathbf s + a_i\,\mathbf d`$ along
//!
//! ```math
//! \mathbf u = \cos\tau\,\mathbf d + c_x\,\mathbf n + c_z\,\mathbf e_z,
//! \qquad \mathbf n = (-d_y, d_x, 0)
//! ```
//!
//! for its tilt $`\tau`$ from the axis, with
//! $`(c_x, c_z) = \sin\tau\,(\cos\varphi, \pm\sin\varphi)`$ for a needle at
//! azimuth $`\varphi`$ round a shoot needled all round (the needles of a
//! fascicle share a place and fan 0.25 rad apart in azimuth), and
//! $`(c_x, c_z) = (\pm\sin\tau, 0)`$ in two ranks. The painter draws each
//! needle's projection, from $`\mathbf b`$ to $`\mathbf b + \ell\,(\cos\tau\,\mathbf d + c_x\,\mathbf n)`$;
//! the solid draws it whole, a three-sided pyramid from a base
//! [`BASE`] times the needle's width $`w`$ across (the look's `width`
//! times its `needle`) to its tip $`\mathbf b + \ell\,\mathbf u`$, so it
//! covers what a needle $`w`$ wide all along would: flat, a third as thick
//! as wide, in two ranks; an even triangle all round. Each axis is a
//! three-sided prism in the look's accent colour, as thick as the painter
//! draws it, narrowing to three fifths at its tip. Shape coordinates
//! shrink by the card's border as the painter's do
//! (`templates::drawn_share`).
//!
//! The painter draws each needle as a line two of its finest widths
//! across (`templates::fine`), far wider than a needle, and so draws few
//! of them. The solid draws [`fill`] needles for each painted one, its
//! own and the rest set evenly along the axis before the next, each with
//! its own draws, so that at their true width they cover what the card
//! covers (the area the program shades with).
//!
//! The painter cannot show the side out of the card a needle all round
//! leaves on, so each organ draws its own: the sign of $`c_z`$ by a hash
//! of the organ and the needle. In two ranks each needle rises out of the
//! card's plane by up to [`LIFT`], lengthened so that its projection keeps
//! its place. Either way the card is unchanged and the shoot's four part
//! meshes differ (`crate::parts`).
//!
//! A needle takes 3 triangles and an axis 7; a shoot `coarse` steps
//! coarser fills in `coarse` fewer needles for each painted one, and once
//! it draws the painted ones alone, leaves out places along each axis.

use crate::blooms::{Part, shade};
use crate::graph::GraphOrgan;
use crate::looks::{Look, Needles, Shape};
use crate::math::{self, Vec3};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};
use crate::templates::{self, NeedleAxis};

/// A needle's base across, as a share of its width: its pyramid narrows
/// to its tip, so it covers what a needle of even width would.
pub const BASE: f64 = 2.0;

/// The most a needle in two ranks rises out of the card's plane, degrees.
pub const LIFT: f64 = 12.0;

/// The most needles the solid draws for each one its card paints.
pub const FILL: usize = 4;

/// Triangles a needle takes.
const NEEDLE_TRIANGLES: usize = 3;

/// Triangles an axis takes: a three-sided prism and its tip.
const AXIS_TRIANGLES: usize = 7;

/// How wide `s`'s needles are, in units of the card's length.
#[must_use]
pub fn needle_width(s: &Needles) -> f64 {
    s.width * s.needle
}

/// How many needles the solid draws for each one its card paints: the
/// painted line's width over the needle's, rounded, from 1 to [`FILL`].
#[must_use]
pub fn fill(s: &Needles) -> usize {
    let painted = 2.0 * templates::fine(&Shape::Needles(s.clone()));
    let ratio = painted / needle_width(s).max(1.0e-9);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fill = ratio.round().clamp(1.0, 64.0) as usize;
    fill.min(FILL)
}

/// Triangles a shoot of `s` takes, `coarse` steps coarser.
#[must_use]
pub fn triangles(s: &Needles, coarse: usize) -> usize {
    layout(s, 0, coarse)
        .iter()
        .map(|(_, needles)| AXIS_TRIANGLES + needles.len() * NEEDLE_TRIANGLES)
        .sum()
}

/// One needle in the card's frame: where it leaves its axis, its unit
/// direction and its length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShootNeedle {
    pub base: Vec3,
    pub direction: Vec3,
    pub length: f64,
    /// Its own draw in `[0, 1)`, which shades it as the painter does.
    pub jitter: f64,
    /// Whether the card paints it.
    pub painted: bool,
}

/// The needles of a shoot of `s` on the organ `id`, `coarse` steps
/// coarser, in the card's frame before its border (see the module),
/// axis by axis; with each axis.
pub(crate) fn layout(s: &Needles, id: u64, coarse: usize) -> Vec<(NeedleAxis, Vec<ShootNeedle>)> {
    let shape = Shape::Needles(s.clone());
    let fine = templates::fine(&shape);
    let half = s.aspect * 0.5;
    let per_place = templates::needles_per_place(s);
    let lift = math::radians(LIFT);
    let fill = fill(s);
    let filled = fill.saturating_sub(coarse).max(1);
    let step = i64::try_from(coarse.saturating_sub(fill - 1)).unwrap_or(0) + 1;
    templates::needle_axes(s)
        .into_iter()
        .enumerate()
        .map(|(index, axis)| {
            let d = Vec3::new(axis.direction.0, axis.direction.1, 0.0);
            let n = Vec3::new(-axis.direction.1, axis.direction.0, 0.0);
            let start = Vec3::new(axis.start.0, axis.start.1, 0.0);
            let mut needles = Vec::new();
            let count = templates::needle_count(s, &axis);
            let mut i = 0;
            while i < count {
                for (between, k) in
                    (0..filled).flat_map(|between| (0..per_place).map(move |k| (between, k)))
                {
                    let drawn =
                        templates::needle_at(s, index, &axis, i, k, (between, filled), half, fine);
                    if between > 0 && drawn.along > axis.length {
                        continue;
                    }
                    let key = mix64(
                        id ^ mix64(
                            (u64::try_from(index).unwrap_or(0) << 40)
                                ^ (u64::try_from(between).unwrap_or(0) << 32)
                                ^ (u64::try_from(i).unwrap_or(0) << 4)
                                ^ u64::try_from(k).unwrap_or(0),
                        ),
                    );
                    let flat = d * drawn.along_share + n * drawn.across;
                    let (direction, length) = if s.ranks == 0 {
                        let out = if unit(key) < 0.5 {
                            drawn.out
                        } else {
                            -drawn.out
                        };
                        (flat + Vec3::new(0.0, 0.0, out), drawn.length)
                    } else {
                        let rise = lift * unit(key);
                        (
                            flat * math::cos(rise) + Vec3::new(0.0, 0.0, math::sin(rise)),
                            drawn.length / math::cos(rise),
                        )
                    };
                    needles.push(ShootNeedle {
                        base: start + d * drawn.along,
                        direction: direction.normalize_or(d),
                        length,
                        jitter: drawn.jitter,
                        painted: between == 0,
                    });
                }
                i += step;
            }
            (axis, needles)
        })
        .collect()
}

/// Draw the shoot of `s` on the card of `organ` (`crate::mesh::place`)
/// into `mesh`: needles in `colour`, the axes in `accent`, `coarse` steps
/// coarser.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    organ: &GraphOrgan,
    look: &Look,
    s: &Needles,
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    if length <= 0.0 {
        return;
    }
    let (base, heading, left) = crate::mesh::place(organ, look);
    let heading = heading.normalize_or(Vec3::Y);
    let left = (left - heading * left.dot(heading)).normalize_or(math::any_perpendicular(heading));
    let out = left.cross(heading);
    let (across, along) = templates::drawn_share(&look.shape);
    let to_world = |p: Vec3| {
        base + (left * (p.x * across) + heading * (p.y * along) + out * (p.z * across)) * length
    };
    let width = needle_width(s);
    // The axes as thick as the painter draws them.
    let fine = templates::fine(&look.shape);
    let leaf = part.leaf(base, heading, left, 0.0, 0.0, colour);
    let twig = shade(accent, 0.75);
    for (index, (axis, needles)) in layout(s, organ.id, part.coarse).into_iter().enumerate() {
        // The axis: a prism narrowing to its tip, which it closes.
        let d = Vec3::new(axis.direction.0, axis.direction.1, 0.0);
        let start = Vec3::new(axis.start.0, axis.start.1, 0.0);
        let thick = 2.0 * fine * if index == 0 { 1.3 } else { 1.0 };
        let near = ring(start, d, Vec3::Z, thick * 0.5, thick);
        let far = ring(
            start + d * axis.length,
            d,
            Vec3::Z,
            thick * 0.3,
            thick * 0.6,
        );
        let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for point in near.iter().chain(&far) {
            mesh.push(leaf.vertex(to_world(*point), twig));
        }
        for k in 0..3 {
            let (a0, a1) = (first + k, first + (k + 1) % 3);
            let (b0, b1) = (a0 + 3, a1 + 3);
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
        mesh.indices
            .extend_from_slice(&[first + 3, first + 4, first + 5]);
        for needle in needles {
            // Flat in two ranks, its width in the card's plane; round all
            // round, its width square to its axis too.
            let side = if s.ranks == 0 {
                needle.direction.cross(d)
            } else {
                Vec3::Z.cross(needle.direction)
            };
            let half = width * BASE * 0.5;
            let thick = if s.ranks == 0 {
                half * 3.0_f64.sqrt()
            } else {
                half * 2.0 / 3.0
            };
            #[allow(clippy::cast_possible_truncation)]
            let jitter = 0.1 * (needle.jitter as f32 - 0.5);
            let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for point in ring(needle.base, needle.direction, side, half, thick) {
                mesh.push(leaf.vertex(to_world(point), shade(colour, 0.78 + jitter)));
            }
            let tip = needle.base + needle.direction * needle.length;
            mesh.push(leaf.vertex(to_world(tip), shade(colour, 1.1 + jitter)));
            for k in 0..3 {
                mesh.indices
                    .extend_from_slice(&[first + k, first + (k + 1) % 3, first + 3]);
            }
        }
    }
}

/// The corners of a triangle round `at` square to the unit `axis`,
/// counterclockwise seen from its tip, centred on `at`: `2 half` across
/// along `side` (made square to `axis`) and `thick` the other way.
fn ring(at: Vec3, axis: Vec3, side: Vec3, half: f64, thick: f64) -> [Vec3; 3] {
    let side = (side - axis * side.dot(axis)).normalize_or(math::any_perpendicular(axis));
    let other = axis.cross(side);
    [
        at + side * half - other * (thick / 3.0),
        at + other * (thick * 2.0 / 3.0),
        at - side * half - other * (thick / 3.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn organ(id: u64, size: f64) -> GraphOrgan {
        GraphOrgan {
            id,
            organ: 0,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size,
            born: 0.0,
            shed: None,
            light: 1.0,
        }
    }

    fn look(s: &Needles) -> Look {
        let shape = Shape::Needles(s.clone());
        Look {
            organ: "spray".to_string(),
            form: crate::blooms::Form::default_for(&shape),
            bend: crate::bend::Bend::FLAT,
            shape,
            colour: [0.2, 0.4, 0.2],
            shade: [0.1, 0.2, 0.1],
            accent: [0.3, 0.2, 0.1],
            face_up: 0.0,
            solid: None,
            forms: crate::looks::Forms::default(),
        }
    }

    fn sprays() -> Vec<Needles> {
        vec![
            Needles::default(),
            Needles {
                ranks: 0,
                twigs: 3,
                needle: 0.09,
                aspect: 0.55,
                ..Needles::default()
            },
            Needles {
                ranks: 0,
                twigs: 2,
                needle: 0.28,
                aspect: 0.8,
                bundle: 3,
                width: 0.01,
                ..Needles::default()
            },
        ]
    }

    #[test]
    fn the_card_is_the_solid_seen_face_on() {
        // Each needle's projection on the card's plane is the painter's
        // needle: from its place on its axis, along its share and across.
        let fine = |s: &Needles| templates::fine(&Shape::Needles(s.clone()));
        for s in sprays() {
            let half = s.aspect * 0.5;
            for (index, (axis, needles)) in layout(&s, 99, 0).iter().enumerate() {
                let d = Vec3::new(axis.direction.0, axis.direction.1, 0.0);
                let n = Vec3::new(-axis.direction.1, axis.direction.0, 0.0);
                let per_place = templates::needles_per_place(&s);
                let painted: Vec<&ShootNeedle> = needles.iter().filter(|n| n.painted).collect();
                assert_eq!(
                    painted.len(),
                    usize::try_from(templates::needle_count(&s, axis)).unwrap() * per_place
                );
                assert!(needles.len() >= painted.len() * fill(&s) - 2 * fill(&s) * per_place);
                for (j, needle) in painted.iter().enumerate() {
                    let (i, k) = (j / per_place, j % per_place);
                    let painted = templates::needle_at(
                        &s,
                        index,
                        axis,
                        i64::try_from(i).unwrap(),
                        k,
                        (0, 1),
                        half,
                        fine(&s),
                    );
                    let tip = needle.base + needle.direction * needle.length;
                    let flat = Vec3::new(tip.x, tip.y, 0.0);
                    let start = Vec3::new(axis.start.0, axis.start.1, 0.0);
                    let painted_tip = start
                        + d * (painted.along + painted.along_share * painted.length)
                        + n * (painted.across * painted.length);
                    assert!(
                        (flat - painted_tip).length() < 1e-9,
                        "{flat:?} {painted_tip:?}"
                    );
                    assert!(tip.x.abs() <= half + 1e-9 && tip.y <= 1.0 + 1e-9);
                }
            }
        }
    }

    #[test]
    fn organs_differ_out_of_the_card_alone() {
        for s in sprays() {
            let (a, b) = (layout(&s, 1, 0), layout(&s, 2, 0));
            let tips = |shoot: &Vec<(NeedleAxis, Vec<ShootNeedle>)>| -> Vec<Vec3> {
                shoot
                    .iter()
                    .flat_map(|(_, needles)| {
                        needles.iter().map(|n| n.base + n.direction * n.length)
                    })
                    .collect()
            };
            let (ta, tb) = (tips(&a), tips(&b));
            assert!(ta.iter().zip(&tb).any(|(p, q)| (p.z - q.z).abs() > 1e-6));
            for (p, q) in ta.iter().zip(&tb) {
                assert!((p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_shoot_takes_the_triangles_it_counts_and_coarser_takes_fewer() {
        for s in sprays() {
            let look = look(&s);
            for coarse in 0..3 {
                let mut mesh = Mesh::default();
                let part = Part {
                    born: 0.0,
                    shed: None,
                    coarse,
                };
                draw(
                    &organ(5, 0.3),
                    &look,
                    &s,
                    look.colour,
                    look.accent,
                    part,
                    &mut mesh,
                );
                assert_eq!(mesh.triangle_count(), triangles(&s, coarse));
                assert!(
                    mesh.indices
                        .iter()
                        .all(|&i| (i as usize) < mesh.positions.len())
                );
                // Within the card, a little out of its plane.
                for p in &mesh.positions {
                    let (x, y) = (f64::from(p[0]) / 0.3, f64::from(p[1]) / 0.3);
                    assert!(
                        x.abs() <= s.aspect * 0.5 + 0.02 && (-0.02..=1.0).contains(&y),
                        "{p:?}"
                    );
                }
            }
            assert!(triangles(&s, 1) < triangles(&s, 0));
        }
    }

    #[test]
    fn faces_point_outward() {
        // Every face of a needle or an axis turns away from the inside of
        // its piece: its normal leans away from the centroid of the piece's
        // vertices (a needle's four, an axis's six), which lies inside it.
        let needles = Needles::default();
        let look = look(&needles);
        let mut mesh = Mesh::default();
        let part = Part {
            born: 0.0,
            shed: None,
            coarse: 0,
        };
        draw(
            &organ(3, 1.0),
            &look,
            &needles,
            look.colour,
            look.accent,
            part,
            &mut mesh,
        );
        let point = |index: u32| {
            let q = mesh.positions[index as usize];
            Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2]))
        };
        let mut checked = 0;
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            let (pa, pb, pc) = (point(a), point(b), point(c));
            let normal = (pb - pa).cross(pc - pa);
            if normal.length() < 1e-12 {
                continue;
            }
            // A piece's triangles use its vertices alone, which follow one
            // another.
            let (first, last) = (a.min(b).min(c), a.max(b).max(c));
            let count = f64::from(last - first + 1);
            let middle =
                (first..=last).fold(Vec3::ZERO, |sum, index| sum + point(index)) * (1.0 / count);
            let centre = (pa + pb + pc) * (1.0 / 3.0);
            assert!(
                normal.dot(centre - middle) >= -1e-12,
                "inward face {:?}",
                [a, b, c]
            );
            checked += 1;
        }
        assert!(checked > 100);
    }
}
