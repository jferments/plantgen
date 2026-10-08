//! Bark patterns (plant roadmap P4): the fissures, plates, strips, scales,
//! peeling patches and lenticels of a species' bark, drawn on its wood
//! pixel by pixel.
//!
//! The wood mesh's furrows (`docs/developer/PLANTS.md` section 6.1) are as
//! fine as its rings and sides. A [`BarkPattern`] draws what is finer: a
//! shade and a relief at any point of a stem's surface, from where it lies
//! round the stem, `u` from 0 to 1 (the mesh's bark coordinate), along it,
//! `v` in metres, and the radius of the stem there, `r` in metres. The
//! point's place on the stem's cylinder,
//!
//! ```math
//! \mathbf c = (r\cos 2\pi u,\ r\sin 2\pi u,\ v),
//! ```
//!
//! is the same where a ring of the mesh starts again (`u` = 0 and 1), so
//! the pattern has no seam, and its cells keep their size in metres on a
//! twig and a trunk alike. In pattern space
//! $`\mathbf q = (c_x/s,\ c_y/s,\ c_z/(s e))`$, for the pattern's `scale`
//! $`s`$ and `elongation` $`e`$, the cells are the Voronoi cells of one
//! point jittered in each unit cube (Worley 1996). With $`F_1`$ and $`F_2`$
//! the distances to the nearest two points, a point lies on a plate or a
//! ridge where $`F_2 - F_1`$ passes the `fissure` width $`w`$:
//!
//! ```math
//! h = \operatorname{smoothstep}(0, w, F_2 - F_1),
//! \qquad
//! \text{shade} = \big(1 + k\,(h - \bar h)\big)\big(1 + \nu\,(\xi - \tfrac12)\big)
//! ```
//!
//! with `contrast` $`k`$, $`\bar h`$ the mean of $`h`$ over the pattern (so
//! the bark's mean colour is the wood's colour, which the far levels and
//! impostors show), `variation` $`\nu`$ and $`\xi`$ a draw of the cell. The
//! kinds ([`BarkKind`]) shape it:
//!
//! | Kind | Cells | Like |
//! | --- | --- | --- |
//! | furrowed | long ridges between wandering fissures | Douglas fir, oaks, ash |
//! | plated | broad plates between narrow dark fissures | ponderosa pine |
//! | fibrous | long strips | western redcedar, junipers |
//! | scaly | small scales, each darker at its foot | spruces |
//! | smooth | no cells: soft mottling | alder, beech, young maples |
//! | peeling | patches whose outer bark has peeled | paper birch, madrone |
//!
//! `lenticels` (per square metre) add horizontal dashes on any kind. Below
//! `smooth_below` (a radius, metres) the bark is smooth, as young bark is,
//! and the pattern grows in by twice that radius. Where a cell would cover
//! under a pixel or two the pattern fades to its mean (the caller gives the
//! pixel's `footprint` in metres), so far bark does not shimmer.
//!
//! `roughness` $`\rho`$ breaks the plates and ridges up with a finer grain
//! $`d`$: two octaves of value noise, four and nine times as fine as the
//! cells round the stem and $`\sqrt e`$ times longer along it than round
//! (less drawn out than the cells), each in $`[-\tfrac12, \tfrac12)`$, the
//! second at half weight, and each fading where its own grain would cover
//! under a pixel or two. The height becomes
//!
//! ```math
//! H = h\,(1 + \rho\,d),
//! ```
//!
//! which takes $`h`$'s place in the shade and the relief. The grain is
//! drawn apart from the cells and its mean is 0, so the bark's mean colour
//! stays the wood's.
//!
//! The relief tilts the surface's normal: the pattern stands `depth` times
//! a cell's width proud where $`H = 1`$, and [`sample`] returns its slope
//! round and along the stem from the gradient of $`F_2 - F_1`$,
//! $`\nabla_{\mathbf q}(F_2 - F_1) = \widehat{\mathbf q - \mathbf p_2} - \widehat{\mathbf q - \mathbf p_1}`$
//! for the nearest points $`\mathbf p_1`$, $`\mathbf p_2`$, and the grain's,
//! which value noise gives in closed form.
//!
//! The renderer's wood shader (`after-vegetation-render`, `draw.wgsl`
//! `bark_sample`) runs [`sample`] in f32, operation for operation, on the
//! numbers of [`BarkPattern::params`]; `crate::raster` runs it here for
//! previews and impostors.

use serde::{Deserialize, Serialize};

use crate::spines::pcg;

/// How a species' bark breaks up (see the module).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarkKind {
    Furrowed,
    Plated,
    Fibrous,
    Scaly,
    Smooth,
    Peeling,
}

impl BarkKind {
    /// Its number in [`BarkParams::kind`] and the shader's; 0 is no
    /// pattern.
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::Furrowed => 1,
            Self::Plated => 2,
            Self::Fibrous => 3,
            Self::Scaly => 4,
            Self::Smooth => 5,
            Self::Peeling => 6,
        }
    }
}

/// A species' bark pattern (see the module).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarkPattern {
    pub kind: BarkKind,
    /// A cell's width round the stem, metres.
    pub scale: f64,
    /// A cell's length along the stem over its width.
    pub elongation: f64,
    /// The fissures' width, as a share of a cell.
    pub fissure: f64,
    /// How far a plate or ridge stands proud, as a share of a cell's
    /// width.
    pub depth: f64,
    /// How much darker the fissures are than the plates, 0 to 1.
    pub contrast: f64,
    /// How much the cells' brightness varies, 0 to 1.
    pub variation: f64,
    /// Peeling bark: the share of patches whose outer bark has peeled, and
    /// the colour under it.
    pub peeled: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner: Option<[f32; 3]>,
    /// Lenticels per square metre: horizontal dashes; 0 for none.
    pub lenticels: f64,
    /// The radius below which the bark is smooth, metres; the pattern
    /// grows in by twice it.
    pub smooth_below: f64,
    /// How far a finer grain raises and lowers the plates and ridges, as a
    /// share of their height, 0 to 1; 0 leaves them smooth.
    pub roughness: f64,
}

impl Default for BarkPattern {
    fn default() -> Self {
        Self {
            kind: BarkKind::Furrowed,
            scale: 0.05,
            elongation: 4.0,
            fissure: 0.25,
            depth: 0.3,
            contrast: 0.5,
            variation: 0.15,
            peeled: 0.0,
            inner: None,
            lenticels: 0.0,
            smooth_below: 0.02,
            roughness: 0.0,
        }
    }
}

impl BarkPattern {
    /// Check the pattern's ranges.
    ///
    /// # Errors
    ///
    /// Describes the first value out of range.
    pub fn validate(&self) -> Result<(), String> {
        let check = |name: &str, value: f64, low: f64, high: f64| {
            if (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "bark_pattern {name} must be between {low} and {high}, found {value}"
                ))
            }
        };
        check("scale", self.scale, 0.002, 0.5)?;
        check("elongation", self.elongation, 0.5, 40.0)?;
        check("fissure", self.fissure, 0.01, 0.6)?;
        check("depth", self.depth, 0.0, 1.0)?;
        check("contrast", self.contrast, 0.0, 1.0)?;
        check("variation", self.variation, 0.0, 1.0)?;
        check("peeled", self.peeled, 0.0, 1.0)?;
        check("lenticels", self.lenticels, 0.0, 5_000.0)?;
        check("smooth_below", self.smooth_below, 0.0, 1.0)?;
        check("roughness", self.roughness, 0.0, 1.0)?;
        if self
            .inner
            .is_some_and(|colour| colour.iter().any(|c| !(0.0..=1.0).contains(c)))
        {
            return Err("bark_pattern inner colour must be between 0 and 1".into());
        }
        Ok(())
    }

    /// The numbers [`sample`] and the shader read, for bark of colour
    /// `bark`: the peeled colour is the bark's paled if the pattern gives
    /// none.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn params(&self, bark: [f32; 3]) -> BarkParams {
        let mut params = BarkParams {
            kind: self.kind.number(),
            scale: self.scale as f32,
            elongation: self.elongation as f32,
            fissure: self.fissure as f32,
            depth: self.depth as f32,
            contrast: self.contrast as f32,
            variation: self.variation as f32,
            mean: 0.0,
            smooth_below: self.smooth_below as f32,
            lenticels: self.lenticels as f32,
            peeled: self.peeled as f32,
            inner: self
                .inner
                .unwrap_or_else(|| bark.map(|c| (c * 1.6 + 0.12).min(1.0))),
            roughness: self.roughness as f32,
        };
        params.mean = mean_height(&params);
        params
    }
}

/// The numbers a bark pattern is drawn from, as the shader reads them
/// (`after-vegetation-render` `pack::bark_template`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarkParams {
    /// [`BarkKind::number`]; 0 for none.
    pub kind: u32,
    pub scale: f32,
    pub elongation: f32,
    pub fissure: f32,
    pub depth: f32,
    pub contrast: f32,
    pub variation: f32,
    /// The mean of `h` over the pattern.
    pub mean: f32,
    pub smooth_below: f32,
    pub lenticels: f32,
    pub peeled: f32,
    pub inner: [f32; 3],
    pub roughness: f32,
}

/// What the pattern does at one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarkSample {
    /// What the wood's colour is multiplied by.
    pub shade: f32,
    /// How far the colour turns to the peeled colour, 0 to 1.
    pub inner: f32,
    /// The relief's slope round the stem and along it (metres per metre),
    /// which tilts the normal against them.
    pub slope: [f32; 2],
}

impl BarkSample {
    /// The bare wood: no change.
    pub const NONE: Self = Self {
        shade: 1.0,
        inner: 0.0,
        slope: [0.0, 0.0],
    };
}

const TAU: f32 = std::f32::consts::TAU;
/// Salts of the draws: the cells' points, a cell's brightness, whether a
/// cell has peeled, the mottling, the lenticels' points, and the grain's
/// two octaves.
const CELL_SALT: u32 = 0x68e3_1da4;
const SHADE_SALT: u32 = 0x5bd1_e995;
const PEEL_SALT: u32 = 0x2c1b_3c6d;
const MOTTLE_SALT: u32 = 0x297a_2d39;
const LENTICEL_SALT: u32 = 0x7feb_352d;
const GRAIN_SALT: u32 = 0x1b87_3593;
const FINE_SALT: u32 = 0xcc9e_2d51;

/// A draw in `[0, 1)` from a hash, as the shaders' `unit`.
#[allow(clippy::cast_precision_loss)]
fn unit(hash: u32) -> f32 {
    (hash >> 8) as f32 / 16_777_216.0
}

/// The hash of a lattice cell, as the shader's `bark_hash`.
#[allow(clippy::cast_sign_loss)]
fn cell_hash(cell: [i32; 3], salt: u32) -> u32 {
    pcg(cell[0] as u32 ^ pcg(cell[1] as u32 ^ pcg(cell[2] as u32 ^ salt)))
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The Voronoi cells round a point of pattern space: the distances to the
/// nearest two points, those points, and the nearest's hash.
#[derive(Debug, Clone, Copy)]
struct Cells {
    near: f32,
    second: f32,
    near_point: [f32; 3],
    second_point: [f32; 3],
    id: u32,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cells(q: [f32; 3], salt: u32) -> Cells {
    let base = q.map(|value| value.floor() as i32);
    let mut found = Cells {
        near: 1.0e9,
        second: 1.0e9,
        near_point: q,
        second_point: q,
        id: 0,
    };
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let cell = [base[0] + dx, base[1] + dy, base[2] + dz];
                let hash = cell_hash(cell, salt);
                let a = pcg(hash);
                let b = pcg(a);
                let point = [
                    cell[0] as f32 + 0.1 + 0.8 * unit(hash),
                    cell[1] as f32 + 0.1 + 0.8 * unit(a),
                    cell[2] as f32 + 0.1 + 0.8 * unit(b),
                ];
                let distance = length(sub(q, point));
                if distance < found.near {
                    found.second = found.near;
                    found.second_point = found.near_point;
                    found.near = distance;
                    found.near_point = point;
                    found.id = hash;
                } else if distance < found.second {
                    found.second = distance;
                    found.second_point = point;
                }
            }
        }
    }
    found
}

/// Smooth value noise in `[0, 1)`, as the shader's `bark_noise`.
#[allow(clippy::cast_possible_truncation)]
fn noise(q: [f32; 3], salt: u32) -> f32 {
    let base = q.map(|value| value.floor() as i32);
    let f = [
        q[0] - q[0].floor(),
        q[1] - q[1].floor(),
        q[2] - q[2].floor(),
    ];
    let w = f.map(|t| t * t * (3.0 - 2.0 * t));
    let corner = |dx: i32, dy: i32, dz: i32| {
        unit(cell_hash([base[0] + dx, base[1] + dy, base[2] + dz], salt))
    };
    let xa = corner(0, 0, 0) + (corner(1, 0, 0) - corner(0, 0, 0)) * w[0];
    let xb = corner(0, 1, 0) + (corner(1, 1, 0) - corner(0, 1, 0)) * w[0];
    let xc = corner(0, 0, 1) + (corner(1, 0, 1) - corner(0, 0, 1)) * w[0];
    let xd = corner(0, 1, 1) + (corner(1, 1, 1) - corner(0, 1, 1)) * w[0];
    let ya = xa + (xb - xa) * w[1];
    let yb = xc + (xd - xc) * w[1];
    ya + (yb - ya) * w[2]
}

/// Smooth value noise in `[0, 1)` at `q` and its gradient, as the shader's
/// `bark_noise_gradient`: the trilinear blend of the cube's corners in
/// powers of the smoothstep weights, whose derivatives give the gradient.
/// Its names are the blend's terms (`kxy` weighs `w.x w.y`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::similar_names
)]
fn noise_gradient(q: [f32; 3], salt: u32) -> (f32, [f32; 3]) {
    let base = q.map(|value| value.floor() as i32);
    let f = [
        q[0] - q[0].floor(),
        q[1] - q[1].floor(),
        q[2] - q[2].floor(),
    ];
    let w = f.map(|t| t * t * (3.0 - 2.0 * t));
    let dw = f.map(|t| 6.0 * t * (1.0 - t));
    // The cube's corners, x fastest.
    let corner: [f32; 8] = std::array::from_fn(|k| {
        let k = k as i32;
        unit(cell_hash(
            [
                base[0] + (k & 1),
                base[1] + ((k >> 1) & 1),
                base[2] + ((k >> 2) & 1),
            ],
            salt,
        ))
    });
    let kx = corner[1] - corner[0];
    let ky = corner[2] - corner[0];
    let kz = corner[4] - corner[0];
    let kxy = corner[0] - corner[1] - corner[2] + corner[3];
    let kxz = corner[0] - corner[1] - corner[4] + corner[5];
    let kyz = corner[0] - corner[2] - corner[4] + corner[6];
    let kxyz = -corner[0] + corner[1] + corner[2] - corner[3] + corner[4] - corner[5] - corner[6]
        + corner[7];
    let value = corner[0]
        + kx * w[0]
        + ky * w[1]
        + kz * w[2]
        + kxy * w[0] * w[1]
        + kxz * w[0] * w[2]
        + kyz * w[1] * w[2]
        + kxyz * w[0] * w[1] * w[2];
    let gradient = [
        dw[0] * (kx + kxy * w[1] + kxz * w[2] + kxyz * w[1] * w[2]),
        dw[1] * (ky + kxy * w[0] + kyz * w[2] + kxyz * w[0] * w[2]),
        dw[2] * (kz + kxz * w[0] + kyz * w[1] + kxyz * w[0] * w[1]),
    ];
    (value, gradient)
}

/// One octave of the grain at the stem's point `c`, `frequency` times as
/// fine as the cells, at `weight`: its share of `d` and of `d`'s slope
/// round and along the stem (per metre), faded for a pixel `footprint`
/// metres across; as the shader's `bark_octave`. Its names follow the
/// module's formulas.
#[allow(clippy::many_single_char_names)]
fn octave(
    params: &BarkParams,
    c: [f32; 3],
    (sine, cosine): (f32, f32),
    footprint: f32,
    frequency: f32,
    weight: f32,
    salt: u32,
) -> [f32; 3] {
    let fade = 1.0 - smoothstep(0.3, 0.8, footprint * frequency / params.scale);
    if fade <= 0.0 {
        return [0.0; 3];
    }
    let k = frequency / params.scale;
    let stretch = params.elongation.max(1.0e-3).sqrt();
    let g = [c[0] * k, c[1] * k, c[2] * k / stretch];
    let (n, dn) = noise_gradient(g, salt);
    let a = weight * fade;
    [
        a * (n - 0.5),
        a * k * (-sine * dn[0] + cosine * dn[1]),
        a * k / stretch * dn[2],
    ]
}

/// The grain `d` at the stem's point `c` and its slope round and along the
/// stem (see the module).
fn grain(params: &BarkParams, c: [f32; 3], round: (f32, f32), footprint: f32) -> [f32; 3] {
    let coarse = octave(params, c, round, footprint, 4.0, 1.0, GRAIN_SALT);
    let fine = octave(params, c, round, footprint, 9.0, 0.5, FINE_SALT);
    [
        coarse[0] + fine[0],
        coarse[1] + fine[1],
        coarse[2] + fine[2],
    ]
}

/// The height `h` of a cell pattern at `q`, its gradient in pattern space,
/// and the cells.
fn height(params: &BarkParams, q: [f32; 3]) -> (f32, [f32; 3], Cells) {
    let found = cells(q, CELL_SALT);
    let edge = found.second - found.near;
    let w = params.fissure.max(1.0e-4);
    let t = (edge / w).clamp(0.0, 1.0);
    let mut h = t * t * (3.0 - 2.0 * t);
    let to_near = sub(q, found.near_point);
    let to_second = sub(q, found.second_point);
    let (near_length, second_length) = (length(to_near).max(1.0e-6), length(to_second).max(1.0e-6));
    // d h / d edge, inside the fissure only.
    let slope = if t > 0.0 && t < 1.0 {
        6.0 * t * (1.0 - t) / w
    } else {
        0.0
    };
    let mut gradient =
        [0, 1, 2].map(|i| slope * (to_second[i] / second_length - to_near[i] / near_length));
    if params.kind == BarkKind::Scaly.number() {
        // A scale stands proud at its foot and lies flat into the one
        // above: lower within the cell toward its top.
        let rise = (0.5 - to_near[2]).clamp(0.0, 1.0);
        let factor = 0.65 + 0.35 * rise;
        let inside = if 0.5 - to_near[2] > 0.0 && 0.5 - to_near[2] < 1.0 {
            -0.35 * h
        } else {
            0.0
        };
        gradient = gradient.map(|g| g * factor);
        gradient[2] += inside;
        h *= factor;
    }
    (h, gradient, found)
}

/// The cell pattern's height at `q` as the kind draws it: peeling bark is
/// flat but where a patch has peeled, which its fissure rims. With the
/// gradient, the nearest cell's hash, and whether it has peeled.
fn surface(params: &BarkParams, q: [f32; 3]) -> (f32, [f32; 3], u32, bool) {
    let (h, gradient, found) = height(params, q);
    if params.kind != BarkKind::Peeling.number() {
        return (h, gradient, found.id, false);
    }
    if unit(pcg(found.id ^ PEEL_SALT)) < params.peeled {
        (h, gradient, found.id, true)
    } else {
        (1.0, [0.0; 3], found.id, false)
    }
}

/// The mean of `h` over the pattern, from a fixed spread of points.
fn mean_height(params: &BarkParams) -> f32 {
    if params.kind == 0 || params.kind == BarkKind::Smooth.number() {
        return 1.0;
    }
    let count = 2048_u32;
    let mut sum = 0.0_f64;
    for n in 0..count {
        let a = pcg(n ^ 0x1234_5679);
        let b = pcg(a);
        let c = pcg(b);
        let q = [unit(a) * 16.0, unit(b) * 16.0, unit(c) * 16.0];
        sum += f64::from(surface(params, q).0);
    }
    #[allow(clippy::cast_possible_truncation)]
    let mean = (sum / f64::from(count)) as f32;
    mean
}

/// The bark pattern `params` at `u` round a stem of radius `r` and `v`
/// metres along it, for a pixel `footprint` metres across (see the
/// module). The shader's `bark_sample` mirrors it. Its names follow the
/// module's formulas.
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn sample(params: &BarkParams, u: f32, v: f32, r: f32, footprint: f32) -> BarkSample {
    if params.kind == 0 || r <= 0.0 || params.scale <= 0.0 {
        return BarkSample::NONE;
    }
    let young = if params.smooth_below > 0.0 {
        smoothstep(params.smooth_below, 2.0 * params.smooth_below, r)
    } else {
        1.0
    };
    let fade = 1.0 - smoothstep(0.3, 0.8, footprint / params.scale);
    let amount = young * fade;
    if amount <= 0.0 {
        return BarkSample::NONE;
    }
    let theta = TAU * u;
    let (sine, cosine) = theta.sin_cos();
    let c = [r * cosine, r * sine, v];
    let s = params.scale;
    let along = s * params.elongation.max(1.0e-3);
    let q = [c[0] / s, c[1] / s, c[2] / along];
    let (mut shade, inner, gradient, grain_slope) = if params.kind == BarkKind::Smooth.number() {
        let mottle = noise(q, MOTTLE_SALT);
        (
            1.0 + params.variation * (mottle - 0.5),
            0.0,
            [0.0_f32; 3],
            [0.0_f32; 2],
        )
    } else {
        let (h, dh, id, peeled) = surface(params, q);
        let cell = unit(pcg(id ^ SHADE_SALT));
        // The grain: H = h (1 + rho d), sloping as h does times the rise
        // and as d does times h rho.
        let (height, gradient, grain_slope) = if params.roughness > 0.0 {
            let d = grain(params, c, (sine, cosine), footprint);
            let rise = 1.0 + params.roughness * d[0];
            (
                h * rise,
                dh.map(|g| g * rise),
                [h * params.roughness * d[1], h * params.roughness * d[2]],
            )
        } else {
            (h, dh, [0.0; 2])
        };
        (
            (1.0 + params.contrast * (height - params.mean))
                * (1.0 + params.variation * (cell - 0.5)),
            if peeled { h } else { 0.0 },
            gradient,
            grain_slope,
        )
    };
    if params.lenticels > 0.0 {
        // One dash a cell of the lenticels' spacing, lying round the stem.
        let spacing = 1.0 / params.lenticels.sqrt();
        let found = cells(
            [c[0] / spacing, c[1] / spacing, c[2] / spacing],
            LENTICEL_SALT,
        );
        let offset = sub(
            [c[0] / spacing, c[1] / spacing, c[2] / spacing],
            found.near_point,
        );
        let round = -sine * offset[0] + cosine * offset[1];
        let reach = (round / 0.22) * (round / 0.22) + (offset[2] / 0.035) * (offset[2] / 0.035);
        shade *= 1.0 - 0.45 * (1.0 - smoothstep(0.6, 1.0, reach));
    }
    // The relief, `depth` cells proud, sloping round and along the stem.
    let relief = params.depth * s;
    let round_q = [-sine / s, cosine / s, 0.0];
    let slope = [
        relief * (gradient[0] * round_q[0] + gradient[1] * round_q[1]) + relief * grain_slope[0],
        relief * gradient[2] / along + relief * grain_slope[1],
    ];
    BarkSample {
        shade: 1.0 + (shade - 1.0) * amount,
        inner: inner * amount,
        slope: [slope[0] * amount, slope[1] * amount],
    }
}

#[allow(clippy::cast_precision_loss)]
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> Vec<BarkPattern> {
        [
            BarkKind::Furrowed,
            BarkKind::Plated,
            BarkKind::Fibrous,
            BarkKind::Scaly,
            BarkKind::Smooth,
            BarkKind::Peeling,
        ]
        .into_iter()
        .flat_map(|kind| {
            [0.0, 0.6].map(|roughness| BarkPattern {
                kind,
                peeled: if kind == BarkKind::Peeling { 0.4 } else { 0.0 },
                lenticels: if kind == BarkKind::Smooth { 400.0 } else { 0.0 },
                roughness,
                ..BarkPattern::default()
            })
        })
        .collect()
    }

    #[test]
    fn the_pattern_has_no_seam_where_a_ring_starts_again() {
        for pattern in kinds() {
            let params = pattern.params([0.4, 0.3, 0.2]);
            for k in 0..50_u16 {
                let v = f32::from(k) * 0.037;
                let (a, b) = (
                    sample(&params, 0.0, v, 0.3, 0.0),
                    sample(&params, 1.0, v, 0.3, 0.0),
                );
                assert!((a.shade - b.shade).abs() < 1e-3, "{pattern:?} {a:?} {b:?}");
                assert!((a.inner - b.inner).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn the_mean_shade_is_the_woods_colour() {
        // Far levels and impostors draw the wood's colour alone, so near bark
        // must average to it.
        for pattern in kinds() {
            let params = pattern.params([0.4, 0.3, 0.2]);
            let mut sum = 0.0_f64;
            let count = 4000_u32;
            for n in 0..count {
                let a = pcg(n ^ 0x0bad_cafe);
                let b = pcg(a);
                let shade = sample(&params, unit(a), unit(b) * 3.0, 0.4, 0.0).shade;
                sum += f64::from(shade);
            }
            let mean = sum / f64::from(count);
            assert!(
                (mean - 1.0).abs() < 0.04,
                "{:?}: mean shade {mean}",
                pattern.kind
            );
        }
    }

    #[test]
    fn young_and_far_bark_is_plain() {
        let params = BarkPattern::default().params([0.4, 0.3, 0.2]);
        // A twig below `smooth_below`, and a trunk whose cells are smaller
        // than a pixel.
        assert_eq!(sample(&params, 0.3, 1.0, 0.015, 0.0), BarkSample::NONE);
        assert_eq!(sample(&params, 0.3, 1.0, 0.5, 0.2), BarkSample::NONE);
        let near = sample(&params, 0.3, 1.0, 0.5, 0.0);
        assert!(near != BarkSample::NONE);
    }

    #[test]
    fn fissures_are_darker_and_slope() {
        // Along a line round a trunk, the darkest points lie in fissures,
        // where the relief slopes, and plates are flat.
        let params = BarkPattern {
            kind: BarkKind::Plated,
            fissure: 0.15,
            ..BarkPattern::default()
        }
        .params([0.4, 0.3, 0.2]);
        let samples: Vec<BarkSample> = (0..2000_u16)
            .map(|k| sample(&params, f32::from(k) / 2000.0, 0.7, 0.4, 0.0))
            .collect();
        let dark = samples
            .iter()
            .filter(|s| s.shade < 1.0 - 0.3 * params.contrast)
            .collect::<Vec<_>>();
        let light = samples.iter().filter(|s| s.shade > 1.0).collect::<Vec<_>>();
        assert!(!dark.is_empty() && !light.is_empty());
        let steep = |set: &[&BarkSample]| {
            set.iter()
                .filter(|s| s.slope[0].abs() + s.slope[1].abs() > 0.05)
                .count() as f64
                / set.len() as f64
        };
        assert!(
            steep(&dark) > steep(&light),
            "{} {}",
            steep(&dark),
            steep(&light)
        );
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn the_slope_is_the_reliefs_gradient() {
        // Finite differences of the relief round and along the stem agree
        // with the slope away from the cells' edges, smooth or rough.
        for roughness in [0.0, 0.5] {
            let params = BarkPattern {
                kind: BarkKind::Furrowed,
                fissure: 0.4,
                roughness,
                ..BarkPattern::default()
            }
            .params([0.4, 0.3, 0.2]);
            let r = 0.3_f32;
            let relief = |u: f32, v: f32| {
                let s = params.scale;
                let theta = TAU * u;
                let c = [r * theta.cos(), r * theta.sin(), v];
                let q = [c[0] / s, c[1] / s, v / (s * params.elongation)];
                let d = grain(&params, c, (theta.sin(), theta.cos()), 0.0)[0];
                params.depth * s * height(&params, q).0 * (1.0 + params.roughness * d)
            };
            let mut checked = 0;
            for k in 0..400_u16 {
                let (u, v) = (f32::from(k) / 400.0, 0.3 + f32::from(k % 37) * 0.011);
                let got = sample(&params, u, v, r, 0.0).slope;
                let du = 1.0e-4_f32;
                let dv = 1.0e-4_f32;
                let round = (relief(u + du, v) - relief(u - du, v)) / (2.0 * du * TAU * r);
                let along = (relief(u, v + dv) - relief(u, v - dv)) / (2.0 * dv);
                if (round - got[0]).abs() < 0.05 + 0.1 * round.abs()
                    && (along - got[1]).abs() < 0.05 + 0.1 * along.abs()
                {
                    checked += 1;
                }
            }
            // Points by a cell's edge, where F1 and F2 swap, may differ.
            assert!(checked > 340, "roughness {roughness}: {checked}");
        }
    }

    #[test]
    fn roughness_breaks_up_the_plates_and_its_grain_fades_first() {
        let smooth = BarkPattern {
            kind: BarkKind::Plated,
            fissure: 0.15,
            ..BarkPattern::default()
        };
        let rough = BarkPattern {
            roughness: 0.6,
            ..smooth.clone()
        };
        let (smooth, rough) = (
            smooth.params([0.4, 0.3, 0.2]),
            rough.params([0.4, 0.3, 0.2]),
        );
        // Points round a trunk on the smooth pattern's plates, where it is
        // flat: the rough pattern slopes there.
        let plates: Vec<f32> = (0..2000_u16)
            .map(|k| f32::from(k) / 2000.0)
            .filter(|&u| sample(&smooth, u, 0.7, 0.4, 0.0).slope == [0.0, 0.0])
            .collect();
        assert!(plates.len() > 500, "{}", plates.len());
        let tilted = plates
            .iter()
            .filter(|&&u| {
                let slope = sample(&rough, u, 0.7, 0.4, 0.0).slope;
                slope[0].abs() + slope[1].abs() > 0.01
            })
            .count();
        assert!(
            tilted * 10 > plates.len() * 9,
            "{tilted} of {}",
            plates.len()
        );
        // Where the grain would cover under a pixel but the cells would
        // not, rough bark is drawn as smooth bark is.
        let footprint = 0.25 * smooth.scale;
        for k in 0..200_u16 {
            let u = f32::from(k) / 200.0;
            assert_eq!(
                sample(&rough, u, 0.7, 0.4, footprint),
                sample(&smooth, u, 0.7, 0.4, footprint)
            );
        }
    }

    #[test]
    fn patterns_validate_and_read_from_json() {
        for pattern in kinds() {
            pattern.validate().unwrap();
        }
        let read: BarkPattern =
            serde_json::from_str(r#"{"kind": "plated", "scale": 0.12, "contrast": 0.7}"#).unwrap();
        assert_eq!(read.kind, BarkKind::Plated);
        assert!(
            BarkPattern {
                fissure: 0.9,
                ..BarkPattern::default()
            }
            .validate()
            .is_err()
        );
    }
}
