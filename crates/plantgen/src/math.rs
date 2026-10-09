//! Deterministic scalar functions and a small 3-vector type.
//!
//! Plant generation must give bit-identical results on every machine, because
//! servers and clients rebuild the same plants from the same specs. IEEE 754
//! guarantees correctly rounded `+ - * / sqrt`, but the standard library's
//! `sin`, `exp`, `powf` and friends call the platform's math library, whose
//! last bits differ between operating systems and CPUs. Every transcendental
//! function used by generation therefore goes through the pure-Rust `libm`
//! port pinned in `Cargo.toml`. Never call `f64::sin` and friends in this
//! crate's generation code, and never use fused multiply-add.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

pub const PI: f64 = std::f64::consts::PI;

#[must_use]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

#[must_use]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

#[must_use]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

#[must_use]
pub fn asin(x: f64) -> f64 {
    libm::asin(x.clamp(-1.0, 1.0))
}

#[must_use]
pub fn acos(x: f64) -> f64 {
    libm::acos(x.clamp(-1.0, 1.0))
}

#[must_use]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[must_use]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// Natural logarithm; non-positive input gives negative infinity or NaN like `f64::ln`.
#[must_use]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

#[must_use]
pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

#[must_use]
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// Correctly rounded by IEEE 754, so the standard implementation is portable.
#[must_use]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[must_use]
pub fn radians(degrees: f64) -> f64 {
    degrees * (PI / 180.0)
}

#[must_use]
pub fn degrees(radians: f64) -> f64 {
    radians * (180.0 / PI)
}

/// Hermite smoothstep between `edge0` and `edge1`; 0 below, 1 above.
#[must_use]
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let span = edge1 - edge0;
    if span == 0.0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / span).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[must_use]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// A double-precision vector in a plant's own frame: metres, +Y up.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[must_use]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    #[must_use]
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }

    #[must_use]
    pub fn length(self) -> f64 {
        sqrt(self.length_squared())
    }

    /// Unit vector, or `fallback` when the length is too small to normalise.
    #[must_use]
    pub fn normalize_or(self, fallback: Self) -> Self {
        let length = self.length();
        if length > 1e-12 && length.is_finite() {
            self / length
        } else {
            fallback
        }
    }

    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        (self - other).length()
    }

    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    #[must_use]
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    /// Rotate around a unit `axis` by `angle` radians (Rodrigues' formula).
    #[must_use]
    pub fn rotate_about(self, axis: Self, angle: f64) -> Self {
        self.turned(axis, sin(angle), cos(angle))
    }

    /// [`Vec3::rotate_about`] given the angle's sine `s` and cosine `c`.
    #[must_use]
    fn turned(self, axis: Self, s: f64, c: f64) -> Self {
        self * c + axis.cross(self) * s + axis * (axis.dot(self) * (1.0 - c))
    }

    /// Narrowed to 32 bits for meshes: plant-frame metres keep well under
    /// a millimetre.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn to_f32(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, scale: f64) -> Self {
        Self::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

impl Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, scale: f64) -> Self {
        Self::new(self.x / scale, self.y / scale, self.z / scale)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// An orthonormal turtle frame: heading `h`, left `l` and up `u`, with
/// `h × l = u` (the convention of *The Algorithmic Beauty of Plants*).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub h: Vec3,
    pub l: Vec3,
    pub u: Vec3,
}

impl Frame {
    /// Heading straight up the plant's +Y axis.
    pub const UPRIGHT: Self = Self {
        h: Vec3::Y,
        l: Vec3::X,
        u: Vec3::new(0.0, 0.0, -1.0),
    };

    /// Rotate the whole frame about a unit axis.
    #[must_use]
    pub fn rotated(self, axis: Vec3, angle: f64) -> Self {
        let (s, c) = (sin(angle), cos(angle));
        Self {
            h: self.h.turned(axis, s, c),
            l: self.l.turned(axis, s, c),
            u: self.u.turned(axis, s, c),
        }
        .orthonormalized()
    }

    /// Re-orthonormalise after many rotations so rounding cannot accumulate.
    #[must_use]
    pub fn orthonormalized(self) -> Self {
        let h = self.h.normalize_or(Vec3::Y);
        let l = (self.l - h * h.dot(self.l)).normalize_or(any_perpendicular(h));
        let u = h.cross(l);
        Self { h, l, u }
    }
}

/// A unit vector perpendicular to the unit vector `v`.
#[must_use]
pub fn any_perpendicular(v: Vec3) -> Vec3 {
    let helper = if v.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
    v.cross(helper).normalize_or(Vec3::Z)
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn frame_stays_right_handed_after_rotation() {
        let frame = Frame::UPRIGHT
            .rotated(Vec3::X, 0.7)
            .rotated(Vec3::new(0.3, 0.4, 0.5).normalize_or(Vec3::Y), 1.3);
        assert!((frame.h.cross(frame.l) - frame.u).length() < 1e-12);
        assert!((frame.h.length() - 1.0).abs() < 1e-12);
        assert!(frame.h.dot(frame.l).abs() < 1e-12);
    }

    #[test]
    fn deterministic_functions_match_the_mathematical_values() {
        assert!((sin(PI / 6.0) - 0.5).abs() < 1e-15);
        assert!((pow(2.0, 10.0) - 1024.0).abs() < 1e-12);
        assert!((exp(ln(7.0)) - 7.0).abs() < 1e-12);
        assert!((degrees(radians(33.0)) - 33.0).abs() < 1e-12);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
    }

    #[test]
    fn rotation_about_up_turns_heading_toward_left() {
        let rotated = Vec3::X.rotate_about(Vec3::Y, PI / 2.0);
        assert!((rotated - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-12);
    }
}
