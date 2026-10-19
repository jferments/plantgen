//! Prickles drawn as solids (Rosaceae, 2026-10-19).
//!
//! A prickle grows from a stem's skin (a rose's, a bramble's), not from a
//! bud as a thorn does: it stands on the stem's surface, broad at its foot
//! along the stem and flattened across it, narrows quickly and curves back
//! toward the stem's base, a little where the flora calls it curved, a lot
//! where hooked. A [`PrickleForm`] draws each thorn organ the program
//! places at a node as one, standing out at the organ's heading; then the
//! prickles between the nodes on the internode below it, at the same lean
//! from the stem, each 55 to 100 % of the node's (floras: internodal
//! prickles "similar or smaller"); then the aciculi among them, needles a
//! third as long that stand nearly square to the stem.
//!
//! The centre line of a prickle `l` long that curves back by `k`
//! radians is an arc: at `s` from 0 to 1 it is
//! `f + (l / k) (sin(k s) o + (1 - cos(k s)) b)` from its foot `f`, for
//! the direction `o` it leaves the stem in and `b` toward the stem's base.
//! Its section is an ellipse, `width` along the stem and `thick` across it
//! at the foot, shrinking as `(1 - s)^2`.

use serde::{Deserialize, Serialize};

use crate::blooms::Part;
use crate::graph::{GraphOrgan, GraphSegment};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};

/// How a look's prickles stand between its nodes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PrickleForm {
    /// Prickles a metre of stem 1 cm across bears between its nodes,
    /// besides those at its nodes.
    pub between: f64,
    /// Aciculi (needle-like prickles, bristles) a metre of stem 1 cm
    /// across bears among them.
    pub bristles: f64,
    /// Whether its prickles stand only between the nodes, none at them (a
    /// flora's prickles "scattered" or "at the internodes": a catclaw's).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub between_only: bool,
}

impl PrickleForm {
    /// Check the values: up to 2000 prickles and 5000 aciculi a metre.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        crate::looks::within("form prickles between", self.between, 0.0, 2000.0)?;
        crate::looks::within("form prickles bristles", self.bristles, 0.0, 5000.0)
    }

    /// The prickles and aciculi one node's organ draws on `length` metres
    /// of internode `radius` thick: half of what it bears, as two organs
    /// stand at a node. A flora's density is of the stem's skin, so a
    /// stem bears them as its girth, from half as many on a thin twig to
    /// four times as many on a stout cane 4 cm across (a devil's club's).
    #[must_use]
    pub fn on(&self, length: f64, radius: f64) -> (f64, f64) {
        let girth = (radius / 0.005).clamp(0.5, 4.0);
        let along = length * girth * 0.5;
        (self.between * along, self.bristles * along)
    }
}

/// Faces round a node's prickle, one between the nodes, and an aciculus:
/// those between are many and small, so they take three.
const SIDES: u32 = 5;
const BETWEEN_SIDES: u32 = 3;
const NEEDLE_SIDES: u32 = 3;

/// Triangles a prickle takes: its two rings' band and the fan to its tip.
pub const PRICKLE_TRIANGLES: usize = 3 * SIDES as usize;
/// Triangles a prickle between the nodes takes.
pub const BETWEEN_TRIANGLES: usize = 3 * BETWEEN_SIDES as usize;
/// Triangles an aciculus takes: a fan from its foot's ring to its tip.
pub const NEEDLE_TRIANGLES: usize = NEEDLE_SIDES as usize;

/// Most triangles one organ type's prickles take on a level, twice
/// another type's (`crate::blooms::TYPE_TRIANGLES`): a densely prickly
/// rose bears tens of thousands, each a handful of triangles.
pub const BUDGET: usize = 2 * crate::blooms::TYPE_TRIANGLES;

/// One prickle's frame and size.
pub(crate) struct Prickle {
    /// Its foot, on the stem's surface.
    pub foot: Vec3,
    /// Whether it is drawn with a band below its tip, which carries its
    /// curve (a prickle), or as a fan straight to its tip (an aciculus).
    pub banded: bool,
    /// The direction it leaves the stem in.
    pub out: Vec3,
    /// Toward the stem's base, which it curves to.
    pub back: Vec3,
    pub length: f64,
    /// Across its foot along the stem, and across the stem.
    pub width: f64,
    pub thick: f64,
    /// Radians it turns toward the stem's base from foot to tip.
    pub curve: f64,
}

/// Draw `prickle` with `sides` faces.
pub(crate) fn draw(prickle: &Prickle, sides: u32, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
    let out = prickle.out.normalize_or(Vec3::Y);
    let back =
        (prickle.back - out * prickle.back.dot(out)).normalize_or_else(|| any_perpendicular(out));
    let curve = prickle.curve;
    // Its centre and heading at s along it.
    let at = |s: f64| {
        let turn = curve * s;
        let centre = if curve.abs() < 1e-4 {
            prickle.foot + out * (prickle.length * s)
        } else {
            let radius = prickle.length / curve;
            prickle.foot + (out * math::sin(turn) + back * (1.0 - math::cos(turn))) * radius
        };
        let heading = out * math::cos(turn) + back * math::sin(turn);
        let along = back * math::cos(turn) - out * math::sin(turn);
        (centre, heading, along)
    };
    let leaf = part.leaf(prickle.foot, out, back, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let rings: &[f64] = if prickle.banded { &[0.0, 0.4] } else { &[0.0] };
    for &s in rings {
        let (centre, heading, along) = at(s);
        let across = heading.cross(along);
        let shrink = (1.0 - s) * (1.0 - s);
        for k in 0..sides {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(sides);
            let radial = along * (math::cos(theta) * prickle.width * 0.5 * shrink)
                + across * (math::sin(theta) * prickle.thick * 0.5 * shrink);
            mesh.push(leaf.vertex(centre + radial, colour));
        }
    }
    let (tip, _, _) = at(1.0);
    mesh.push(leaf.vertex(tip, colour));
    let ring_count = u32::try_from(rings.len()).unwrap_or(1);
    for ring in 0..ring_count - 1 {
        for k in 0..sides {
            let a0 = first + ring * sides + k;
            let a1 = first + ring * sides + (k + 1) % sides;
            let (b0, b1) = (a0 + sides, a1 + sides);
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
    let last = first + (ring_count - 1) * sides;
    let tip = last + sides;
    for k in 0..sides {
        mesh.indices
            .extend_from_slice(&[last + k, last + (k + 1) % sides, tip]);
    }
}

/// Where a node's organ meets its stem: the stem's direction from base to
/// tip, the radial direction the organ stands out in, and its foot on the
/// surface. The organ's own point is on the stem's axis.
pub(crate) fn foot(organ: &GraphOrgan, segment: Option<&GraphSegment>) -> (Vec3, Vec3, Vec3) {
    let heading = organ.heading.normalize_or(Vec3::Y);
    let Some(segment) = segment else {
        return (any_perpendicular(heading), heading, organ.position);
    };
    let stem = (segment.end - segment.start).normalize_or_else(|| any_perpendicular(heading));
    let radial = (heading - stem * heading.dot(stem))
        .normalize_or_else(|| organ.left.normalize_or_else(|| any_perpendicular(stem)));
    // Sunk a little into the bark so no seam shows at its foot.
    (
        stem,
        radial,
        organ.position + radial * (segment.radius * 0.85),
    )
}

/// Draw a node organ's prickle (where `node`), then `between` prickles and `bristles`
/// aciculi on its internode `segment` (counts already thinned to the
/// budget), each placed by the organ's id: the same plant draws them in
/// the same places at every age.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_organ(
    organ: &GraphOrgan,
    segment: Option<&GraphSegment>,
    (width, curve): (f64, f64),
    (between, bristles): (f64, f64),
    node: bool,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let size = organ.size;
    let heading = organ.heading.normalize_or(Vec3::Y);
    let (stem, _, at) = foot(organ, segment);
    let thick = (width * 0.5).min(size * 0.2).max(size * 0.06);
    if node {
        draw(
            &Prickle {
                banded: true,
                foot: at,
                out: heading,
                back: -stem,
                length: size,
                width: size * width,
                thick,
                curve,
            },
            SIDES,
            colour,
            part,
            mesh,
        );
    }
    let Some(segment) = segment else {
        return;
    };
    // The lean from the stem the node's prickle has, for those between.
    let lean = heading.dot(stem).clamp(-1.0, 1.0);
    let square = (1.0 - lean * lean).sqrt();
    let one = any_perpendicular(stem);
    let other = stem.cross(one);
    let count = |expected: f64, salt: u64| {
        let whole = expected.floor();
        let extra = if unit(mix64(organ.id ^ salt)) < expected - whole {
            1.0
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (whole + extra) as u64;
        count
    };
    let place = |k: u64, salt: u64| {
        let seed = mix64(organ.id ^ salt ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let t = 0.06 + 0.88 * unit(seed);
        let theta = std::f64::consts::TAU * unit(mix64(seed ^ 1));
        let radial = one * math::cos(theta) + other * math::sin(theta);
        let centre = segment.start + (segment.end - segment.start) * t;
        let radius = segment.radius * (1.0 - 0.1 * t) * 0.85;
        (centre + radial * radius, radial, unit(mix64(seed ^ 2)))
    };
    for k in 0..count(between, 0x5052_4943) {
        let (foot, radial, scale) = place(k, 0x5052_4943);
        let length = size * (0.55 + 0.45 * scale);
        draw(
            &Prickle {
                banded: true,
                foot,
                out: stem * lean + radial * square,
                back: -stem,
                length,
                width: length * width,
                thick: thick * length / size,
                curve,
            },
            BETWEEN_SIDES,
            colour,
            part,
            mesh,
        );
    }
    for k in 0..count(bristles, 0x4143_4943) {
        let (foot, radial, scale) = place(k, 0x4143_4943);
        let length = size * (0.2 + 0.15 * scale);
        draw(
            &Prickle {
                banded: false,
                foot,
                out: (radial + stem * 0.3).normalize_or(radial),
                back: -stem,
                length,
                width: length * 0.14,
                thick: length * 0.14,
                curve: 0.0,
            },
            NEEDLE_SIDES,
            colour,
            part,
            mesh,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part() -> Part {
        Part {
            born: 0.0,
            shed: None,
            coarse: 0,
        }
    }

    #[test]
    fn a_hooked_prickle_turns_its_tip_toward_the_stem_s_base() {
        let mut straight = Mesh::default();
        let mut hooked = Mesh::default();
        for (mesh, curve) in [(&mut straight, 0.0), (&mut hooked, 1.4)] {
            draw(
                &Prickle {
                    banded: true,
                    foot: Vec3::ZERO,
                    out: Vec3::X,
                    back: -Vec3::Y,
                    length: 0.01,
                    width: 0.006,
                    thick: 0.002,
                    curve,
                },
                SIDES,
                [0.5; 3],
                part(),
                mesh,
            );
            assert_eq!(mesh.triangle_count(), PRICKLE_TRIANGLES);
        }
        let tip = |mesh: &Mesh| *mesh.positions.last().unwrap();
        assert!((tip(&straight)[0] - 0.01).abs() < 1e-6 && tip(&straight)[1].abs() < 1e-6);
        assert!(tip(&hooked)[1] < -0.004, "a hook ends well below its foot");
        assert!(tip(&hooked)[0] < 0.009, "and nearer the stem");
    }

    #[test]
    fn prickles_stand_on_the_stem_s_surface_and_between_its_nodes() {
        let segment = GraphSegment {
            start: Vec3::ZERO,
            end: Vec3::new(0.0, 0.1, 0.0),
            radius: 0.005,
            id: 0,
            parent: None,
            lateral: false,
            order: 0,
            born: 0.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
            young: 0.0,
        };
        let organ = GraphOrgan {
            id: 7,
            organ: 0,
            segment: Some(0),
            position: Vec3::new(0.0, 0.1, 0.0),
            heading: Vec3::X,
            left: Vec3::Z,
            size: 0.006,
            born: 0.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        };
        let mut mesh = Mesh::default();
        draw_organ(
            &organ,
            Some(&segment),
            (0.6, 0.5),
            (3.0, 4.0),
            true,
            [0.5; 3],
            part(),
            &mut mesh,
        );
        assert_eq!(
            mesh.triangle_count(),
            PRICKLE_TRIANGLES + 3 * BETWEEN_TRIANGLES + 4 * NEEDLE_TRIANGLES
        );
        // Prickles only between the nodes: none at the node.
        let mut between = Mesh::default();
        draw_organ(
            &organ,
            Some(&segment),
            (0.6, 0.5),
            (3.0, 4.0),
            false,
            [0.5; 3],
            part(),
            &mut between,
        );
        assert_eq!(
            between.triangle_count(),
            3 * BETWEEN_TRIANGLES + 4 * NEEDLE_TRIANGLES
        );
        let first = mesh.positions[0];
        assert!((f64::from(first[0]).hypot(f64::from(first[2])) - 0.005).abs() < 0.004);
        let reach = mesh
            .positions
            .iter()
            .map(|p| f64::from(p[0]).hypot(f64::from(p[2])))
            .fold(0.0, f64::max);
        assert!(
            reach > 0.009 && reach < 0.012,
            "out past the surface by a prickle: {reach}"
        );
    }
}
