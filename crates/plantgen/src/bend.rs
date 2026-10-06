//! Bent leaf cards: a leaf drawn as a curved surface on the nearest levels
//! of detail (plant forms milestone F2).
//!
//! A card of a look with a [`Bend`] is drawn on the levels that keep one
//! card per organ as a grid of [`ACROSS`] by [`ALONG`] cells instead of one
//! quad, each corner moved off the card's plane; its template still cuts
//! the leaf's outline. With $`x \in [-1, 1]`$ across the card (from its
//! left edge to its right) and $`v \in [0, 1]`$ along it, $`w`$ its half
//! width and $`\ell`$ its length, the midrib bends toward the card's back
//! by $`\theta(v) = \delta v`$ for the droop $`\delta`$, so it reaches
//!
//! ```math
//! \mathbf m(v) = \ell\left(\frac{\sin \delta v}{\delta}\,\mathbf h
//!   - \frac{1 - \cos \delta v}{\delta}\,\mathbf n\right)
//! ```
//!
//! ($`\ell v \mathbf h`$ when $`\delta = 0`$) for the card's heading
//! $`\mathbf h`$ and face $`\mathbf n = \mathbf h \times \mathbf l`$, and a
//! point across it lies
//!
//! ```math
//! a = x\,w\cos\varphi, \qquad b = |x|\,w\sin\varphi + c\,w\,x^2
//! ```
//!
//! across and above the midrib, for the fold $`\varphi`$ (each half raised
//! from flat) and the cup $`c`$, turned about the midrib by the twist
//! $`\tau v`$ and carried by the midrib's frame: $`\mathbf h(v) = \mathbf h
//! \cos\theta - \mathbf n \sin\theta`$ and $`\mathbf n(v) = \mathbf n
//! \cos\theta + \mathbf h \sin\theta`$. The renderer draws the same
//! surface (`after-vegetation-render` `shaders/draw.wgsl`, `LEAVES`), so
//! the impostors, which this crate bakes from [`crate::mesh::PlantMesh::card_mesh`],
//! match what is drawn.

use serde::{Deserialize, Serialize};

use crate::looks::Shape;
use crate::math::{self, Vec3};

/// Cells across and along a bent card.
pub const ACROSS: u32 = 2;
pub const ALONG: u32 = 4;
/// Vertices a bent card draws: two triangles a cell.
pub const VERTICES: u32 = ACROSS * ALONG * 6;

/// How a leaf card bends, in degrees and shares of its half-width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bend {
    /// Degrees each half rises from flat along the midrib.
    pub fold: f64,
    /// How far the edges curl up, as a share of the half-width.
    pub cup: f64,
    /// Degrees the midrib bends toward the card's back from base to tip.
    pub droop: f64,
    /// Degrees the blade turns about its midrib from base to tip.
    pub twist: f64,
}

impl Default for Bend {
    fn default() -> Self {
        Self::FLAT
    }
}

impl Bend {
    pub const FLAT: Self = Self {
        fold: 0.0,
        cup: 0.0,
        droop: 0.0,
        twist: 0.0,
    };

    /// Whether the card is drawn flat.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.fold == 0.0 && self.cup == 0.0 && self.droop == 0.0 && self.twist == 0.0
    }

    /// The bend a look of `shape` takes unless it sets its own: leaves
    /// fold a little along the midrib, cup and droop; grass blades and
    /// fronds arch; needle shoots, scale sprays, flowers and fruit stay
    /// flat (their solids come with later milestones).
    #[must_use]
    pub fn default_for(shape: &Shape) -> Self {
        match shape {
            Shape::Simple(_) | Shape::Lobed(_) | Shape::Palmate(_) | Shape::Compound(_) => Self {
                fold: 12.0,
                cup: 0.12,
                droop: 18.0,
                twist: 0.0,
            },
            Shape::Sprig(_) => Self {
                fold: 6.0,
                cup: 0.05,
                droop: 12.0,
                twist: 0.0,
            },
            Shape::Blade(_) => Self {
                fold: 10.0,
                cup: 0.0,
                droop: 20.0,
                twist: 8.0,
            },
            Shape::Frond(_) => Self {
                fold: 4.0,
                cup: 0.08,
                droop: 25.0,
                twist: 0.0,
            },
            _ => Self::FLAT,
        }
    }

    /// The point at `x` across and `v` along a card on `base` with heading
    /// `h`, left `l`, half-width `half` and length `length`, and its unit
    /// normal on the face side.
    #[must_use]
    pub fn point(
        &self,
        base: Vec3,
        h: Vec3,
        l: Vec3,
        half: f64,
        length: f64,
        x: f64,
        v: f64,
    ) -> (Vec3, Vec3) {
        let at = |x: f64, v: f64| self.position(base, h, l, half, length, x, v);
        let p = at(x, v);
        let eps = 1.0e-3;
        let du = at((x + eps).min(1.0), v) - at((x - eps).max(-1.0), v);
        let dv = at(x, (v + eps).min(1.0)) - at(x, (v - eps).max(0.0));
        let face = h.cross(l);
        let normal = dv.cross(du).normalize_or(face);
        // dv × du points like h × (-l)... keep it on the face side.
        let normal = if normal.dot(face) < 0.0 {
            -normal
        } else {
            normal
        };
        (p, normal)
    }

    #[allow(clippy::too_many_arguments)]
    fn position(
        &self,
        base: Vec3,
        h: Vec3,
        l: Vec3,
        half: f64,
        length: f64,
        x: f64,
        v: f64,
    ) -> Vec3 {
        let n = h.cross(l);
        let droop = math::radians(self.droop);
        let theta = droop * v;
        let (along, back) = if droop.abs() < 1.0e-9 {
            (v, 0.0)
        } else {
            (math::sin(theta) / droop, (1.0 - math::cos(theta)) / droop)
        };
        let mid = base + h * (length * along) - n * (length * back);
        let hv = h * math::cos(theta) - n * math::sin(theta);
        let nv = n * math::cos(theta) + h * math::sin(theta);
        let fold = math::radians(self.fold);
        let across = x * half * math::cos(fold);
        let lift = x.abs() * half * math::sin(fold) + self.cup * half * x * x;
        let twist = math::radians(self.twist) * v;
        let side = l.rotate_about(hv, twist);
        let up = nv.rotate_about(hv, twist);
        mid + side * across + up * lift
    }

    /// The bend as the renderer's template record holds it: fold, droop
    /// and twist in radians, and the cup.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn packed(&self) -> [f32; 4] {
        [
            math::radians(self.fold) as f32,
            self.cup as f32,
            math::radians(self.droop) as f32,
            math::radians(self.twist) as f32,
        ]
    }
}

/// The grid point of corner `k` (0 to [`VERTICES`] - 1) of a bent card:
/// `x` across in `[-1, 1]` and `v` along in `[0, 1]`, as the renderer
/// numbers them.
#[must_use]
pub fn corner(k: u32) -> (f64, f64) {
    let cell = k / 6;
    let (cu, cv) = quad_corner(k % 6);
    let column = f64::from(cell % ACROSS);
    let row = f64::from(cell / ACROSS);
    let u = (column + cu) / f64::from(ACROSS);
    let v = (row + cv) / f64::from(ALONG);
    (2.0 * u - 1.0, v)
}

/// Corner `k` of a quad drawn as two triangles, as the renderer's
/// `corner`: (0,0) (1,1) (1,0) (0,0) (0,1) (1,1).
fn quad_corner(k: u32) -> (f64, f64) {
    (f64::from((0x26 >> k) & 1), f64::from((0x32 >> k) & 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_bend_is_the_card() {
        let (p, n) = Bend::FLAT.point(Vec3::ZERO, Vec3::Y, Vec3::X, 0.1, 1.0, 1.0, 1.0);
        assert!((p - Vec3::new(0.1, 1.0, 0.0)).length() < 1e-9);
        assert!((n - Vec3::Y.cross(Vec3::X)).length() < 1e-6);
    }

    #[test]
    fn a_drooping_leaf_keeps_its_length_and_bends_back() {
        let bend = Bend {
            droop: 90.0,
            ..Bend::FLAT
        };
        let (tip, _) = bend.point(Vec3::ZERO, Vec3::Y, Vec3::X, 0.1, 1.0, 0.0, 1.0);
        // A quarter circle of length 1: radius 2/pi, ending back along -n.
        let r = 2.0 / std::f64::consts::PI;
        assert!((tip.y - r).abs() < 1e-9);
        let back = -(Vec3::Y.cross(Vec3::X));
        assert!((tip.dot(back) - r).abs() < 1e-9);
    }

    #[test]
    fn folding_raises_both_halves_toward_the_face() {
        let bend = Bend {
            fold: 30.0,
            ..Bend::FLAT
        };
        let face = Vec3::Y.cross(Vec3::X);
        for x in [-1.0, 1.0] {
            let (edge, _) = bend.point(Vec3::ZERO, Vec3::Y, Vec3::X, 0.1, 1.0, x, 0.5);
            assert!((edge.dot(face) - 0.05).abs() < 1e-9);
        }
    }

    #[test]
    fn the_grid_covers_the_card_in_cells() {
        let corners: Vec<(f64, f64)> = (0..VERTICES).map(corner).collect();
        assert!(corners.iter().any(|&(x, v)| x == -1.0 && v == 0.0));
        assert!(corners.iter().any(|&(x, v)| x == 1.0 && v == 1.0));
        assert!(
            corners
                .iter()
                .all(|&(x, v)| (-1.0..=1.0).contains(&x) && (0.0..=1.0).contains(&v))
        );
        assert_eq!(VERTICES, 48);
    }
}
