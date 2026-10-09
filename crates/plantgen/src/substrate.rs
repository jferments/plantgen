//! The substrate: what a plant grows on, as a signed distance field round
//! it (G3, plants that take the shape of what they grow on).
//!
//! A conditions document's `substrate` section ([`Substrate`]) gives the
//! ground, rocks, logs, trunks or walls near the plant as a grid of signed
//! distances, negative inside the solid, and a material per cell. Either
//! it names a preset (`flat`, `slope`, `boulders`, `cleft`, `log`, `trunk`,
//! `wall`), which [`Substrate::resolve`] expands into the grid, or it holds
//! the grid itself, as a host such as Project After cuts it from its world.
//!
//! The grid has one encoding: an `i8` per cell in units of
//! `distance_scale_m`, saturating at ±127 outside a narrow band round the
//! surface, base64 in the document. Growth decodes it once
//! ([`SubstrateField`]) and reads it by trilinear interpolation in `f64`,
//! the surface normal from the field's gradient, so the same document gives
//! the same numbers on every machine. The growth tools `substrate@1` and
//! `light@2` read it (`crate::lsys::tools`).

use serde::{Deserialize, Serialize};

use crate::math::{self, Vec3};
use crate::rng::{hash_words, unit};

/// The section's one version so far.
pub const SUBSTRATE_VERSION: u32 = 1;

/// Most cells a substrate grid may hold.
pub const MAX_SUBSTRATE_CELLS: u64 = 16_000_000;

/// Materials, by the number a cell holds.
pub const AIR: u8 = 0;
pub const SOIL: u8 = 1;
pub const ROCK: u8 = 2;
pub const BARK: u8 = 3;
pub const WOOD: u8 = 4;
pub const BUILT: u8 = 5;

/// The share of light each material reflects (its albedo): physics, the
/// same for every plant. Index by material number; air reflects nothing.
pub const ALBEDO: [f64; 6] = [0.0, 0.2, 0.3, 0.15, 0.25, 0.35];

/// The named substrates a document may ask for instead of a grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubstratePreset {
    /// Level soil at height 0.
    Flat,
    /// Soil rising 20° toward −Z (north).
    Slope,
    /// Level soil with half-buried rounded boulders, placed by `seed`.
    Boulders,
    /// The plant at the foot of a cleft 0.5 m wide between two rock blocks
    /// running along Z.
    Cleft,
    /// A log lying along X just north of the plant, half sunk in soil.
    Log,
    /// An upright trunk 0.3 m in radius just west of the plant.
    Trunk,
    /// The foot of a wall facing south (+Z), 0.3 m north of the plant.
    Wall,
}

impl SubstratePreset {
    pub const ALL: [Self; 7] = [
        Self::Flat,
        Self::Slope,
        Self::Boulders,
        Self::Cleft,
        Self::Log,
        Self::Trunk,
        Self::Wall,
    ];
}

/// Preset grids: 1.6 m either side of the plant, 0.4 m below it to 1.6 m
/// above, in 2.5 cm cells.
const PRESET_CELL: f64 = 0.025;
const PRESET_HALF: f64 = 1.6;
const PRESET_LOW: f64 = -0.4;
const PRESET_HIGH: f64 = 1.6;

/// The `substrate` section of a conditions document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Substrate {
    pub version: u32,
    /// A named substrate, expanded into the grid by [`Self::resolve`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<SubstratePreset>,
    /// Seed of a preset that places things (`boulders`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// The grid's corner: the low corner of its first cell, metres in the
    /// plant's frame (the plant stands at the origin; +Y up).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_m: Option<[f64; 3]>,
    /// Edge of a cell, metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_m: Option<f64>,
    /// Cells along X, Y and Z. Cell `(x, y, z)` is at index
    /// `(y · nz + z) · nx + x`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 3]>,
    /// Metres per unit of a cell's distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_scale_m: Option<f64>,
    /// Signed distance at each cell's centre, an `i8` per cell (negative
    /// inside the solid), base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<String>,
    /// Material of each cell, a `u8` per cell ([`AIR`] to [`BUILT`]),
    /// base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The way up, a unit vector: the normal where the field gives none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<[f64; 3]>,
}

impl Substrate {
    /// A section naming a preset.
    #[must_use]
    pub fn preset(preset: SubstratePreset) -> Self {
        Self {
            version: SUBSTRATE_VERSION,
            preset: Some(preset),
            seed: None,
            origin_m: None,
            cell_m: None,
            size: None,
            distance_scale_m: None,
            distance: None,
            material: None,
            up: None,
        }
    }

    /// The section with its preset expanded into the grid, so sections that
    /// describe the same substrate resolve alike.
    #[must_use]
    pub fn resolve(&self) -> Self {
        match self.preset {
            Some(preset) => grid_of(preset, self.seed.unwrap_or(1)),
            None => self.clone(),
        }
    }

    /// Check the section: a preset, or a whole grid whose data fit its size.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != SUBSTRATE_VERSION {
            return Err(format!(
                "substrate version {} is not supported; expected {SUBSTRATE_VERSION}",
                self.version
            ));
        }
        let grid = [
            self.origin_m.is_some(),
            self.cell_m.is_some(),
            self.size.is_some(),
            self.distance_scale_m.is_some(),
            self.distance.is_some(),
            self.material.is_some(),
            self.up.is_some(),
        ];
        if self.preset.is_some() {
            if grid.iter().any(|given| *given) {
                return Err("substrate names a preset and also gives a grid".into());
            }
            return Ok(());
        }
        if self.seed.is_some() {
            return Err("substrate seed is only for a preset".into());
        }
        if !grid.iter().all(|given| *given) {
            return Err(
                "substrate needs a preset, or origin_m, cell_m, size, distance_scale_m, \
                 distance, material and up"
                    .into(),
            );
        }
        SubstrateField::decode(self).map(|_| ())
    }

    /// The decoded grid.
    ///
    /// # Errors
    ///
    /// Fails on an invalid section.
    pub fn field(&self) -> Result<SubstrateField, String> {
        SubstrateField::decode(&self.resolve())
    }
}

/// A decoded substrate: the grid growth reads.
#[derive(Debug, Clone, PartialEq)]
pub struct SubstrateField {
    origin: Vec3,
    cell: f64,
    size: [usize; 3],
    scale: f64,
    distance: Vec<i8>,
    material: Vec<u8>,
    up: Vec3,
}

impl SubstrateField {
    fn decode(section: &Substrate) -> Result<Self, String> {
        let (
            Some(origin),
            Some(cell),
            Some(size),
            Some(scale),
            Some(distance),
            Some(material),
            Some(up),
        ) = (
            section.origin_m,
            section.cell_m,
            section.size,
            section.distance_scale_m,
            &section.distance,
            &section.material,
            section.up,
        )
        else {
            return Err("substrate grid is incomplete".into());
        };
        if !origin.iter().all(|v| v.is_finite() && v.abs() <= 1.0e4) {
            return Err("substrate origin_m must be finite, within 10 km".into());
        }
        if !(cell.is_finite() && (0.001..=10.0).contains(&cell)) {
            return Err(format!(
                "substrate cell_m must be 0.001 to 10, found {cell}"
            ));
        }
        if !(scale.is_finite() && scale > 0.0 && scale <= cell * 16.0) {
            return Err(format!(
                "substrate distance_scale_m must be above 0 and at most 16 cells, found {scale}"
            ));
        }
        if size.contains(&0) || size.contains(&1) {
            return Err("substrate size needs at least 2 cells along each axis".into());
        }
        let cells = size.iter().map(|n| u64::from(*n)).product::<u64>();
        if cells > MAX_SUBSTRATE_CELLS {
            return Err(format!(
                "substrate has {cells} cells; the limit is {MAX_SUBSTRATE_CELLS}"
            ));
        }
        let up_vector = Vec3::new(up[0], up[1], up[2]);
        if !up.iter().all(|v| v.is_finite()) || (up_vector.length() - 1.0).abs() > 1.0e-6 {
            return Err("substrate up must be a unit vector".into());
        }
        let distance = base64_decode(distance).ok_or("substrate distance is not base64")?;
        let material = base64_decode(material).ok_or("substrate material is not base64")?;
        let count = usize::try_from(cells).map_err(|_| "substrate is too large")?;
        if distance.len() != count || material.len() != count {
            return Err(format!(
                "substrate grid of {count} cells has {} distances and {} materials",
                distance.len(),
                material.len()
            ));
        }
        if let Some(bad) = material.iter().find(|m| usize::from(**m) >= ALBEDO.len()) {
            return Err(format!("substrate material {bad} is not one of 0 to 5"));
        }
        #[allow(clippy::cast_possible_wrap)]
        let distance = distance.into_iter().map(|byte| byte as i8).collect();
        Ok(Self {
            origin: Vec3::new(origin[0], origin[1], origin[2]),
            cell,
            size: size.map(|n| n as usize),
            scale,
            distance,
            material,
            up: up_vector,
        })
    }

    /// The flat ground at height 0 that stands in when a document has no
    /// substrate: soil, as far as the plant reaches.
    ///
    /// # Panics
    ///
    /// Never: the flat preset always decodes.
    #[must_use]
    pub fn flat() -> Self {
        Self::decode(&grid_of(SubstratePreset::Flat, 1)).expect("the flat preset decodes")
    }

    fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (y * self.size[2] + z) * self.size[0] + x
    }

    /// Grid coordinates of `point`, cell centres at whole numbers, clamped
    /// to the grid.
    fn coordinates(&self, point: Vec3) -> [f64; 3] {
        let local = (point - self.origin) / self.cell;
        #[allow(clippy::cast_precision_loss)]
        let clamp = |v: f64, n: usize| (v - 0.5).clamp(0.0, (n - 1) as f64);
        [
            clamp(local.x, self.size[0]),
            clamp(local.y, self.size[1]),
            clamp(local.z, self.size[2]),
        ]
    }

    /// The signed distance from `point` to the substrate's surface, metres,
    /// negative inside it: the grid read by trilinear interpolation, at
    /// the nearest point of the grid for a point outside it. It saturates
    /// at 127 units of `distance_scale_m` away from the surface.
    #[must_use]
    pub fn distance(&self, point: Vec3) -> f64 {
        let [gx, gy, gz] = self.coordinates(point);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let base = |g: f64, n: usize| (g.floor() as usize).min(n - 2);
        let (x0, y0, z0) = (
            base(gx, self.size[0]),
            base(gy, self.size[1]),
            base(gz, self.size[2]),
        );
        #[allow(clippy::cast_precision_loss)]
        let (fx, fy, fz) = (gx - x0 as f64, gy - y0 as f64, gz - z0 as f64);
        let at = |x: usize, y: usize, z: usize| f64::from(self.distance[self.index(x, y, z)]);
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        let plane = |y: usize| {
            let near = lerp(at(x0, y, z0), at(x0 + 1, y, z0), fx);
            let far = lerp(at(x0, y, z0 + 1), at(x0 + 1, y, z0 + 1), fx);
            lerp(near, far, fz)
        };
        lerp(plane(y0), plane(y0 + 1), fy) * self.scale
    }

    /// The unit normal of the surface nearest `point`, out of the solid:
    /// the field's gradient by central differences half a cell wide, or
    /// `up` where the field is flat (far from the surface).
    #[must_use]
    pub fn normal(&self, point: Vec3) -> Vec3 {
        let h = self.cell * 0.5;
        let along = |axis: Vec3| self.distance(point + axis * h) - self.distance(point - axis * h);
        let gradient = Vec3::new(
            along(Vec3::new(1.0, 0.0, 0.0)),
            along(Vec3::new(0.0, 1.0, 0.0)),
            along(Vec3::new(0.0, 0.0, 1.0)),
        );
        if gradient.length() < 1.0e-12 {
            self.up
        } else {
            gradient.normalize_or(self.up)
        }
    }

    /// The material of the cell holding `point`.
    #[must_use]
    pub fn material_at(&self, point: Vec3) -> u8 {
        let [gx, gy, gz] = self.coordinates(point);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let near = |g: f64| g.round() as usize;
        self.material[self.index(near(gx), near(gy), near(gz))]
    }

    /// Whether `point` is inside the solid.
    #[must_use]
    pub fn solid(&self, point: Vec3) -> bool {
        self.distance(point) < 0.0
    }

    /// The material of the surface nearest `point`: the cell a little
    /// inside the surface, below the point along the normal; [`AIR`] when
    /// the surface is beyond the field's narrow band.
    #[must_use]
    pub fn surface_material(&self, point: Vec3) -> u8 {
        let distance = self.distance(point);
        let normal = self.normal(point);
        let inside = point - normal * (distance + self.cell * 0.75);
        let material = self.material_at(inside);
        if material == AIR {
            self.material_at(point - normal * distance)
        } else {
            material
        }
    }

    /// The way up.
    #[must_use]
    pub fn up(&self) -> Vec3 {
        self.up
    }
}

/// The grid of a preset.
fn grid_of(preset: SubstratePreset, seed: u64) -> Substrate {
    let cell = PRESET_CELL;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = |extent: f64| (extent / cell).round() as u32;
    let size = [
        count(2.0 * PRESET_HALF),
        count(PRESET_HIGH - PRESET_LOW),
        count(2.0 * PRESET_HALF),
    ];
    let origin = Vec3::new(-PRESET_HALF, PRESET_LOW, -PRESET_HALF);
    let scale = cell / 16.0;
    let shapes = shapes_of(preset, seed);
    let [nx, ny, nz] = size.map(|n| n as usize);
    let mut distance = Vec::with_capacity(nx * ny * nz);
    let mut material = Vec::with_capacity(nx * ny * nz);
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                #[allow(clippy::cast_precision_loss)]
                let centre =
                    origin + Vec3::new(x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5) * cell;
                let (d, m) = shapes
                    .iter()
                    .map(|shape| (shape.distance(centre), shape.material))
                    .fold((f64::INFINITY, AIR), |best, next| {
                        if next.0 < best.0 { next } else { best }
                    });
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let units = (d / scale).round().clamp(-127.0, 127.0) as i8;
                #[allow(clippy::cast_sign_loss)]
                distance.push(units as u8);
                material.push(if d < 0.0 { m } else { AIR });
            }
        }
    }
    Substrate {
        version: SUBSTRATE_VERSION,
        preset: None,
        seed: None,
        origin_m: Some([origin.x, origin.y, origin.z]),
        cell_m: Some(cell),
        size: Some(size),
        distance_scale_m: Some(scale),
        distance: Some(base64_encode(&distance)),
        material: Some(base64_encode(&material)),
        up: Some([0.0, 1.0, 0.0]),
    }
}

/// A solid of a preset: its signed distance function and material.
struct Shape {
    kind: ShapeKind,
    material: u8,
}

enum ShapeKind {
    /// Below the plane through the origin with this unit normal.
    HalfSpace(Vec3),
    Sphere(Vec3, f64),
    /// An axis-aligned box: centre and half extents.
    Cuboid(Vec3, Vec3),
    /// An infinite cylinder: a point on its axis, the axis, its radius.
    Cylinder(Vec3, Vec3, f64),
}

impl Shape {
    fn distance(&self, p: Vec3) -> f64 {
        match self.kind {
            ShapeKind::HalfSpace(normal) => p.dot(normal),
            ShapeKind::Sphere(centre, radius) => (p - centre).length() - radius,
            ShapeKind::Cuboid(centre, half) => {
                let q = Vec3::new(
                    (p.x - centre.x).abs() - half.x,
                    (p.y - centre.y).abs() - half.y,
                    (p.z - centre.z).abs() - half.z,
                );
                q.max(Vec3::ZERO).length() + q.x.max(q.y).max(q.z).min(0.0)
            }
            ShapeKind::Cylinder(point, axis, radius) => {
                let offset = p - point;
                (offset - axis * offset.dot(axis)).length() - radius
            }
        }
    }
}

fn shapes_of(preset: SubstratePreset, seed: u64) -> Vec<Shape> {
    let soil = |normal: Vec3| Shape {
        kind: ShapeKind::HalfSpace(normal),
        material: SOIL,
    };
    let level = soil(Vec3::new(0.0, 1.0, 0.0));
    match preset {
        SubstratePreset::Flat => vec![level],
        SubstratePreset::Slope => {
            let rise = 20.0_f64.to_radians();
            vec![soil(Vec3::new(0.0, math::cos(rise), math::sin(rise)))]
        }
        SubstratePreset::Boulders => {
            let mut shapes = vec![level];
            for index in 0..6_u64 {
                let draw = |what: u64| unit(hash_words(&[0x626f_756c, seed, index, what]));
                let radius = 0.25 + 0.35 * draw(0);
                let angle = math::PI * 2.0 * draw(1);
                // Clear of the plant's foot, within the grid.
                let reach = radius + 0.12 + 0.6 * draw(2);
                // The centre from 0.3 radii above the ground to 0.3 below it.
                let lift = radius * (0.3 - 0.6 * draw(3));
                let centre = Vec3::new(reach * math::cos(angle), lift, reach * math::sin(angle));
                shapes.push(Shape {
                    kind: ShapeKind::Sphere(centre, radius),
                    material: ROCK,
                });
            }
            shapes
        }
        SubstratePreset::Cleft => vec![
            level,
            Shape {
                kind: ShapeKind::Cuboid(Vec3::new(-0.95, 0.45, 0.0), Vec3::new(0.7, 0.45, 2.0)),
                material: ROCK,
            },
            Shape {
                kind: ShapeKind::Cuboid(Vec3::new(0.95, 0.3, 0.0), Vec3::new(0.7, 0.3, 2.0)),
                material: ROCK,
            },
        ],
        SubstratePreset::Log => vec![
            level,
            Shape {
                kind: ShapeKind::Cylinder(
                    Vec3::new(0.0, 0.12, -0.4),
                    Vec3::new(1.0, 0.0, 0.0),
                    0.25,
                ),
                material: WOOD,
            },
        ],
        SubstratePreset::Trunk => vec![
            level,
            Shape {
                kind: ShapeKind::Cylinder(
                    Vec3::new(-0.35, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    0.3,
                ),
                material: BARK,
            },
        ],
        SubstratePreset::Wall => vec![
            level,
            Shape {
                kind: ShapeKind::Cuboid(Vec3::new(0.0, 1.0, -1.3), Vec3::new(3.0, 2.0, 1.0)),
                material: BUILT,
            },
        ],
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(BASE64[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decode standard base64 with padding; `None` on anything else.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let value = |c: u8| BASE64.iter().position(|b| *b == c).map(|v| v as u32);
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0_u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = if i >= 4 - pad { 0 } else { value(*c)? };
            n = (n << 6) | v;
        }
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&decoded[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn base64_round_trips() {
        for length in 0..10 {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 37 + 200) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(base64_decode(&text).as_deref(), Some(&bytes[..]));
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert!(base64_decode("TWE").is_none());
        assert!(base64_decode("T=E=").is_none());
    }

    #[test]
    fn every_preset_decodes_and_the_plant_stands_in_air() {
        for preset in SubstratePreset::ALL {
            let section = Substrate::preset(preset);
            section.validate().unwrap();
            let field = section.field().unwrap();
            let foot = Vec3::new(0.0, 0.01, 0.0);
            assert!(
                !field.solid(foot),
                "{preset:?}: the foot is inside the solid"
            );
            assert!(
                field.solid(Vec3::new(0.0, -0.1, 0.0)),
                "{preset:?}: no ground"
            );
        }
    }

    #[test]
    fn flat_ground_reads_true() {
        let field = SubstrateField::flat();
        let p = Vec3::new(0.3, 0.05, -0.2);
        assert!((field.distance(p) - 0.05).abs() < 1.0e-9);
        let n = field.normal(p);
        assert!((n.y - 1.0).abs() < 1.0e-9);
        assert_eq!(field.surface_material(p), SOIL);
        // Far above the band it saturates at 127 units.
        let far = field.distance(Vec3::new(0.0, 1.0, 0.0));
        assert!((far - 127.0 * PRESET_CELL / 16.0).abs() < 1.0e-12);
    }

    #[test]
    fn rock_and_wall_read_their_materials() {
        let cleft = Substrate::preset(SubstratePreset::Cleft).field().unwrap();
        // Beside the west block's face, at half its height.
        let p = Vec3::new(-0.22, 0.4, 0.0);
        assert!((cleft.distance(p) - 0.03).abs() < 0.005);
        assert!(cleft.normal(p).x > 0.99);
        assert_eq!(cleft.surface_material(p), ROCK);
        let wall = Substrate::preset(SubstratePreset::Wall).field().unwrap();
        let q = Vec3::new(0.0, 0.8, -0.27);
        assert_eq!(wall.surface_material(q), BUILT);
        assert!(wall.normal(q).z > 0.99);
    }

    #[test]
    fn grids_are_checked() {
        let mut grid = Substrate::preset(SubstratePreset::Flat).resolve();
        grid.validate().unwrap();
        grid.size = Some([3, 3, 3]);
        assert!(grid.validate().unwrap_err().contains("cells has"));
        let mut both = Substrate::preset(SubstratePreset::Flat);
        both.cell_m = Some(0.1);
        assert!(both.validate().unwrap_err().contains("also gives a grid"));
    }
}
