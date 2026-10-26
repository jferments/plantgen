//! A small deterministic software rasterizer for previews and impostors.
//!
//! Baking must give the same bytes on every machine, so this renderer uses
//! only IEEE arithmetic and `libm`: no GPU, no platform math library, no
//! threads. It draws [`Mesh`]es with a depth buffer, alpha-tested organ
//! cards, a sun with a shadow map, and a sky/ground ambient term.

use std::cell::RefCell;

use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::templates::Templates;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Vertical field of view in degrees.
    Perspective { fov_y: f64 },
    /// Half the visible height, in metres.
    Orthographic { half_height: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub projection: Projection,
}

impl Camera {
    fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let forward = (self.target - self.eye).normalize_or(Vec3::new(0.0, 0.0, -1.0));
        let right = forward
            .cross(self.up)
            .normalize_or(any_perpendicular(forward));
        let up = right.cross(forward);
        (right, up, forward)
    }
}

/// Sun and ambient light, linear RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// Unit vector pointing toward the sun.
    pub sun: Vec3,
    pub sun_color: [f32; 3],
    pub sky: [f32; 3],
    pub ground: [f32; 3],
}

impl Lighting {
    /// Mid-morning sun from the south-east under a clear sky, for
    /// previews. The sky gives a level surface 0.165 of its light, as a
    /// clear sky does (Erbs, Klein and Duffie 1982, Solar Energy 28: 293-302,
    /// eq. 1: the diffuse share of global irradiance at a clearness index
    /// above 0.8); at a third of it, as before, crowns' shaded sides and
    /// insides read nearly as bright as their sunlit faces, where photos
    /// show them dark.
    #[must_use]
    pub fn daylight() -> Self {
        Self {
            sun: Vec3::new(0.55, 0.7, 0.45).normalize_or(Vec3::Y),
            sun_color: [1.0, 0.95, 0.85],
            sky: [0.111, 0.132, 0.167],
            ground: [0.16, 0.15, 0.12],
        }
    }

    /// Even light from above with no shadows, for impostor albedo.
    #[must_use]
    pub fn flat() -> Self {
        Self {
            sun: Vec3::Y,
            sun_color: [0.0, 0.0, 0.0],
            sky: [1.0, 1.0, 1.0],
            ground: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Material {
    /// Opaque and one-sided.
    Opaque,
    /// Two-sided, cut out and coloured by the organ template named in the
    /// vertex alpha, and lit through from behind.
    Card,
    /// A plant's wood: opaque and one-sided, with this bark pattern drawn
    /// where its vertices are bark (alpha minus the stem's radius,
    /// `crate::mesh::Mesh::colors`; `crate::bark`).
    Bark(crate::bark::BarkParams),
    /// Hair strands (`crate::hairs::strand_mesh`): two-sided, their
    /// vertex normals their tangents, lit as fibres.
    Hair,
    /// The round heads of glandular hairs (`crate::hairs::Strands::heads`):
    /// drops of resin, lit as clear beads are, a bright glint where they
    /// mirror the sun and a glow where it shines through them.
    Gland,
    /// Drops of secretion on glandular heads (`crate::hairs::Strands::drops`):
    /// clear liquid, drawn after everything else so what is already drawn
    /// behind shows through, tinted by the vertex colour as much as the
    /// vertex alpha (its clarity) says; the sky mirrored at the rim, a
    /// sharp glint where the sun is, and the sun's light gathered on the
    /// far side. Casts no shadow.
    Drop,
    /// A baked card (`crate::billboards`): two-sided, cut out, coloured
    /// and lit by the [`Texture`] of [`Templates::baked`] whose index is in
    /// the vertex alpha, its normals mirrored through the card seen from
    /// behind.
    Baked,
}

/// A baked picture of part of a plant: linear colour with coverage in
/// alpha, and world-space normals, `width × height` texels from the
/// bottom-left, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub color: Vec<[f32; 4]>,
    pub normal: Vec<[f32; 3]>,
    /// How much of the sun reaches each texel through the whole plant, by
    /// direction: the four first spherical-harmonic coefficients (bands 0
    /// and 1, in the order 1, y, z, x) of its visibility, so it shades as
    /// it did in the plant under any sun ([`Self::sunlit`]). Empty: the
    /// shadow map shades it.
    pub sun: Vec<[f32; 4]>,
    /// How the card it is drawn on turns to the eye.
    pub turn: Turn,
    /// The point that card turns about.
    pub pivot: [f32; 3],
    /// How far, root mean square, what the card shows spreads along its
    /// right, up and normal as baked: turned, it is as wide as that seen
    /// from the eye.
    pub extent: [f32; 3],
    /// The way toward the eye it was baked from, for [`Turn::Views`]
    /// and [`Turn::Heights`].
    pub seen_from: [f32; 3],
}

/// How a baked card turns to face the eye, as a vertex shader turns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Turn {
    /// It stays where it was baked.
    #[default]
    Fixed,
    /// It faces the eye, as `SpeedTree`'s leaf cards do.
    Facing,
    /// It faces the eye and shows, of the `count` pictures from `first`
    /// on, the one baked from nearest the eye ([`Texture::seen_from`]):
    /// an impostor.
    Views { first: usize, count: usize },
    /// It faces the eye and shows, of the `count` pictures from `first`
    /// on, the one baked from the height nearest the eye's: a crown's
    /// card, alike from every side but not from above. Each picture's
    /// right is the card's own, level; its up is the world's up laid on
    /// it, but for one baked from straight above, which keeps its frame
    /// on the ground, up the way back in along the card's level normal.
    Heights { first: usize, count: usize },
}

/// The first four real spherical harmonics at the unit vector `d`, in
/// the order of [`Texture::sun`].
#[must_use]
pub fn harmonics(d: Vec3) -> [f64; 4] {
    // 1 / (2 sqrt(pi)) and sqrt(3) / (2 sqrt(pi)).
    let (y0, y1) = (0.282_094_791_773_878_1, 0.488_602_511_902_919_9);
    [y0, y1 * d.y, y1 * d.z, y1 * d.x]
}

impl Texture {
    /// How much of a sun along `toward` reaches the leaf at `(u, v)`, 0 to
    /// 1, or `None` when [`Self::sun`] is empty or no leaf is near.
    #[must_use]
    pub fn sunlit(&self, u: f64, v: f64, toward: Vec3) -> Option<f64> {
        if self.sun.is_empty() || self.width == 0 {
            return None;
        }
        // Between the four nearest texels, as the colour is read, but only
        // from those a leaf covers: an empty texel holds no sun.
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (
            u.clamp(0.0, 1.0) * self.width as f64 - 0.5,
            v.clamp(0.0, 1.0) * self.height as f64 - 0.5,
        );
        let (x0, y0) = (math::floor_index(x), math::floor_index(y));
        #[allow(clippy::cast_precision_loss)]
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let h = harmonics(toward);
        let (mut sum, mut weight) = (0.0, 0.0);
        for (dx, dy, w) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            #[allow(
                clippy::cast_possible_wrap,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let (tx, ty) = (
                (x0 + dx).clamp(0, self.width as i64 - 1) as usize,
                (y0 + dy).clamp(0, self.height as i64 - 1) as usize,
            );
            let at = ty * self.width + tx;
            let w = w * f64::from(self.color[at][3]);
            if w <= 0.0 {
                continue;
            }
            let c = self.sun[at];
            sum += w * (0..4).map(|i| f64::from(c[i]) * h[i]).sum::<f64>();
            weight += w;
        }
        (weight > 0.0).then(|| (sum / weight).clamp(0.0, 1.0))
    }

    /// Colour, coverage and normal at `(u, v)` in `[0, 1]²`, read between
    /// the four nearest texels; nothing outside.
    #[must_use]
    pub fn sample(&self, u: f64, v: f64) -> ([f64; 4], Vec3) {
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) || self.width == 0 {
            return ([0.0; 4], Vec3::Y);
        }
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (u * self.width as f64 - 0.5, v * self.height as f64 - 0.5);
        let (x0, y0) = (math::floor_index(x), math::floor_index(y));
        #[allow(clippy::cast_precision_loss)]
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let mut color = [0.0; 4];
        let mut normal = Vec3::ZERO;
        for (dx, dy, w) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            #[allow(
                clippy::cast_possible_wrap,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let (tx, ty) = (
                (x0 + dx).clamp(0, self.width as i64 - 1) as usize,
                (y0 + dy).clamp(0, self.height as i64 - 1) as usize,
            );
            let at = ty * self.width + tx;
            for (sum, value) in color.iter_mut().zip(self.color[at]) {
                *sum += w * f64::from(value);
            }
            normal = normal + vec3(self.normal[at]) * w;
        }
        (color, normal)
    }
}

impl Material {
    fn one_sided(self) -> bool {
        matches!(self, Self::Opaque | Self::Bark(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    pub width: usize,
    pub height: usize,
    /// Samples per pixel along each axis.
    pub supersample: usize,
    pub shadows: bool,
    /// The region the sun's shadow map covers; every mesh's bounds when
    /// `None`. A large ground plane would otherwise spread the map thin.
    pub shadow_bounds: Option<(Vec3, Vec3)>,
    /// Texels along each side of the shadow map ([`SHADOW_TEXELS`] for a
    /// whole plant).
    pub shadow_texels: usize,
}

/// Texels along each side of a whole plant's shadow map.
pub const SHADOW_TEXELS: usize = 2048;

/// A rendered image. Colour is linear RGB with coverage in alpha; normals are
/// in world space; depth is distance along the view direction (infinite
/// where nothing was drawn).
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub color: Vec<[f32; 4]>,
    pub normal: Vec<[f32; 3]>,
    pub depth: Vec<f32>,
    /// Pixels' worth of leaf and baked cards drawn, hidden or cut out or
    /// not: what a GPU shades for them, to set against the pixels they
    /// cover (overdraw).
    pub card_fragments: f64,
}

struct Target {
    width: usize,
    height: usize,
    color: Vec<[f32; 4]>,
    normal: Vec<[f32; 3]>,
    depth: Vec<f64>,
    card_fragments: usize,
}

/// What `meshes` hide from a light far off along a direction: their
/// depth seen from it.
pub struct ShadowMap {
    camera: Camera,
    basis: (Vec3, Vec3, Vec3),
    viewport: Viewport,
    size: usize,
    depth: Vec<f64>,
}

impl ShadowMap {
    /// The map of `meshes` from `toward` (a unit vector toward the light)
    /// over `bounds`, `size` texels square.
    #[must_use]
    pub fn new(
        meshes: &[(&Mesh, Material)],
        templates: &Templates,
        toward: Vec3,
        (low, high): (Vec3, Vec3),
        size: usize,
    ) -> Self {
        let center = (low + high) * 0.5;
        let radius = ((high - low).length() * 0.5).max(0.1);
        let camera = Camera {
            eye: center + toward * (radius * 3.0),
            target: center,
            up: any_perpendicular(toward),
            projection: Projection::Orthographic {
                half_height: radius,
            },
        };
        let size = size.max(1);
        let mut target = Target::new(size, size);
        for (mesh, material) in meshes {
            if *material == Material::Drop {
                continue;
            }
            draw(
                &mut target,
                mesh,
                *material,
                &camera,
                None,
                templates,
                None,
                false,
            );
        }
        Self {
            basis: camera.basis(),
            viewport: Viewport::new(&camera, size, size),
            camera,
            size,
            depth: target.depth,
        }
    }
}

/// Axis-aligned bounds of every vertex.
#[must_use]
pub fn bounds(meshes: &[(&Mesh, Material)]) -> Option<(Vec3, Vec3)> {
    let mut points = meshes
        .iter()
        .flat_map(|(mesh, _)| mesh.positions.iter())
        .map(|p| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])));
    let first = points.next()?;
    Some(points.fold((first, first), |(low, high), point| {
        (low.min(point), high.max(point))
    }))
}

/// Render meshes.
#[must_use]
pub fn render(
    meshes: &[(&Mesh, Material)],
    camera: &Camera,
    lighting: &Lighting,
    templates: &Templates,
    options: &RenderOptions,
) -> Image {
    render_as(meshes, camera, lighting, templates, options, false)
}

/// [`render`]'s depth and normals alone, as it draws them: the colour is
/// left black. Leaf cards, bark and opaque parts skip their shading;
/// other materials are drawn in full.
#[must_use]
pub fn render_normals(
    meshes: &[(&Mesh, Material)],
    camera: &Camera,
    lighting: &Lighting,
    templates: &Templates,
    options: &RenderOptions,
) -> Image {
    render_as(meshes, camera, lighting, templates, options, true)
}

fn render_as(
    meshes: &[(&Mesh, Material)],
    camera: &Camera,
    lighting: &Lighting,
    templates: &Templates,
    options: &RenderOptions,
    normals_only: bool,
) -> Image {
    let scale = options.supersample.max(1);
    let shadow = if options.shadows && lighting.sun_color.iter().any(|c| *c > 0.0) {
        options
            .shadow_bounds
            .or_else(|| bounds(meshes))
            .map(|bounds| {
                ShadowMap::new(
                    meshes,
                    templates,
                    lighting.sun,
                    bounds,
                    options.shadow_texels,
                )
            })
    } else {
        None
    };
    let mut target = Target::new(options.width * scale, options.height * scale);
    for (mesh, material) in meshes {
        draw(
            &mut target,
            mesh,
            *material,
            camera,
            Some(lighting),
            templates,
            shadow.as_ref(),
            normals_only,
        );
    }
    target.downsample(scale)
}

impl Target {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            color: vec![[0.0; 4]; width * height],
            normal: vec![[0.0; 3]; width * height],
            depth: vec![f64::INFINITY; width * height],
            card_fragments: 0,
        }
    }

    fn downsample(self, scale: usize) -> Image {
        let width = self.width / scale;
        let height = self.height / scale;
        let mut image = Image {
            width,
            height,
            color: vec![[0.0; 4]; width * height],
            normal: vec![[0.0; 3]; width * height],
            depth: vec![f32::INFINITY; width * height],
            #[allow(clippy::cast_precision_loss)]
            card_fragments: self.card_fragments as f64 / (scale * scale) as f64,
        };
        #[allow(clippy::cast_precision_loss)]
        let samples = (scale * scale) as f32;
        for y in 0..height {
            for x in 0..width {
                let mut color = [0.0_f32; 4];
                let mut normal = [0.0_f32; 3];
                let mut depth = f64::INFINITY;
                for sy in 0..scale {
                    for sx in 0..scale {
                        let at = (y * scale + sy) * self.width + x * scale + sx;
                        let sample = self.color[at];
                        // Premultiply so uncovered samples add nothing.
                        for channel in 0..3 {
                            color[channel] += sample[channel] * sample[3];
                        }
                        color[3] += sample[3];
                        for (sum, value) in normal.iter_mut().zip(self.normal[at]) {
                            *sum += value;
                        }
                        depth = depth.min(self.depth[at]);
                    }
                }
                let at = y * width + x;
                if color[3] > 0.0 {
                    for channel in 0..3 {
                        color[channel] /= color[3];
                    }
                }
                color[3] /= samples;
                let length =
                    (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
                if length > 0.0 {
                    for channel in &mut normal {
                        *channel /= length;
                    }
                }
                image.color[at] = color;
                image.normal[at] = normal;
                #[allow(clippy::cast_possible_truncation)]
                {
                    image.depth[at] = depth as f32;
                }
            }
        }
        image
    }
}

fn vec3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// Triangles are clipped this far (metres) in front of a perspective camera.
const NEAR: f64 = 0.05;

/// A world point in view space: along the camera's right, up and forward
/// axes, measured from the eye.
fn to_view(camera: &Camera, basis: (Vec3, Vec3, Vec3), point: Vec3) -> Vec3 {
    let relative = point - camera.eye;
    Vec3::new(
        relative.dot(basis.0),
        relative.dot(basis.1),
        relative.dot(basis.2),
    )
}

/// Maps view-space points to pixels.
#[derive(Debug, Clone, Copy)]
struct Viewport {
    width: f64,
    height: f64,
    /// Half the visible width and height: at unit depth for a perspective
    /// camera, in metres for an orthographic one.
    half_x: f64,
    half_y: f64,
    perspective: bool,
}

impl Viewport {
    fn new(camera: &Camera, width: usize, height: usize) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = (width as f64, height as f64);
        let (half_y, perspective) = match camera.projection {
            Projection::Perspective { fov_y } => (math::tan(math::radians(fov_y) * 0.5), true),
            Projection::Orthographic { half_height } => (half_height, false),
        };
        Self {
            width,
            height,
            half_x: half_y * width / height,
            half_y,
            perspective,
        }
    }

    /// Pixel position of a view-space point. A perspective point must be
    /// in front of the eye.
    fn screen(&self, view: Vec3) -> (f64, f64) {
        let (nx, ny) = if self.perspective {
            (
                view.x / (view.z * self.half_x),
                view.y / (view.z * self.half_y),
            )
        } else {
            (view.x / self.half_x, view.y / self.half_y)
        };
        (
            f64::midpoint(nx, 1.0) * self.width,
            (1.0 - ny) * 0.5 * self.height,
        )
    }
}

/// One corner of a triangle on screen: pixel position, view depth, and
/// barycentric weights on the unclipped triangle.
#[derive(Debug, Clone, Copy)]
struct Corner {
    x: f64,
    y: f64,
    z: f64,
    weights: [f64; 3],
}

const UNIT_WEIGHTS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Clip a triangle's view-space corners to `z >= NEAR`, keeping their
/// winding. The result has zero, three or four corners, each with its
/// barycentric weights on the original triangle. Orthographic triangles
/// are not clipped.
fn clip_near(view: [Vec3; 3], perspective: bool) -> ([(Vec3, [f64; 3]); 4], usize) {
    let mut out = [(Vec3::ZERO, [0.0; 3]); 4];
    if !perspective || view.iter().all(|p| p.z >= NEAR) {
        for corner in 0..3 {
            out[corner] = (view[corner], UNIT_WEIGHTS[corner]);
        }
        return (out, 3);
    }
    let mut count = 0;
    for from in 0..3 {
        let to = (from + 1) % 3;
        let (p, q) = (view[from], view[to]);
        if p.z >= NEAR {
            out[count] = (p, UNIT_WEIGHTS[from]);
            count += 1;
        }
        if (p.z >= NEAR) != (q.z >= NEAR) {
            let s = (NEAR - p.z) / (q.z - p.z);
            let mut point = p.lerp(q, s);
            point.z = NEAR;
            let mut weights = [0.0; 3];
            weights[from] = 1.0 - s;
            weights[to] = s;
            out[count] = (point, weights);
            count += 1;
        }
    }
    (out, count)
}

/// How far (pixels) outside a triangle's box a pixel centre is still
/// tested against it ([`rasterize`]): far more than the rounding of its
/// edge tests, so no pixel the tests take in is passed over.
const BOX_SLACK: f64 = 1e-3;

/// Call `visit(pixel, weights, depth)` for every pixel centre inside a
/// screen triangle, with perspective-correct weights on the original
/// triangle and the view depth there.
fn rasterize(
    width: usize,
    height: usize,
    corners: [Corner; 3],
    perspective: bool,
    visit: &mut impl FnMut(usize, [f64; 3], f64),
) {
    let [a, b, c] = corners;
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.is_nan() || area.abs() < 1e-12 || width == 0 || height == 0 {
        return;
    }
    let (low_x, high_x) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
    let (low_y, high_y) = (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y));
    #[allow(clippy::cast_precision_loss)]
    let (right, bottom) = (width as f64, height as f64);
    // Only pixels whose centres fall within the triangle's box, give or
    // take [`BOX_SLACK`], can be inside it: most triangles of a far plant
    // are a pixel or less across, and the whole pixels round their box
    // held about ten times as many as they covered.
    let first = |low: f64| (low - 0.5 - BOX_SLACK).ceil().max(0.0);
    let (x_first, x_last) = (first(low_x), (high_x - 0.5 + BOX_SLACK).floor());
    let (y_first, y_last) = (first(low_y), (high_y - 0.5 + BOX_SLACK).floor());
    if x_first > x_last.min(right - 1.0) || y_first > y_last.min(bottom - 1.0) {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, x1, y0, y1) = (
        x_first as usize,
        x_last.min(right - 1.0) as usize,
        y_first as usize,
        y_last.min(bottom - 1.0) as usize,
    );
    let inverse_area = 1.0 / area;
    for y in y0..=y1 {
        for x in x0..=x1 {
            #[allow(clippy::cast_precision_loss)]
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) * inverse_area;
            let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) * inverse_area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            // Perspective-correct weights interpolate 1/z linearly.
            let (k0, k1, k2) = if perspective {
                let (q0, q1, q2) = (w0 / a.z, w1 / b.z, w2 / c.z);
                let sum = q0 + q1 + q2;
                (q0 / sum, q1 / sum, q2 / sum)
            } else {
                (w0, w1, w2)
            };
            let depth = k0 * a.z + k1 * b.z + k2 * c.z;
            let weights =
                [0, 1, 2].map(|i| k0 * a.weights[i] + k1 * b.weights[i] + k2 * c.weights[i]);
            visit(y * width + x, weights, depth);
        }
    }
}

/// What a mesh's pixels need to be shaded.
struct Surface<'a> {
    mesh: &'a Mesh,
    material: Material,
    templates: &'a Templates,
    lighting: Option<&'a Lighting>,
    shadow: Option<&'a ShadowMap>,
    /// Only depth and normals are wanted ([`render_normals`]).
    normals_only: bool,
    eye: Vec3,
    forward: Vec3,
    perspective: bool,
    /// A pixel's width at unit depth (perspective) or in metres.
    pixel: f64,
}

impl Surface<'_> {
    /// [`Self::fragment`]'s depth and normal of a leaf card, bark or an
    /// opaque part, worked out as it works them out, without its colour.
    fn normal_only(
        &self,
        target: &mut Target,
        at: usize,
        corners: [usize; 3],
        k: [f64; 3],
        depth: f64,
    ) {
        let mesh = self.mesh;
        let bark = self.bark(corners, k, depth);
        let normals = corners.map(|i| vec3(mesh.normals[i]));
        let positions = corners.map(|i| vec3(mesh.positions[i]));
        let mut normal =
            (normals[0] * k[0] + normals[1] * k[1] + normals[2] * k[2]).normalize_or(Vec3::Y);
        let world = positions[0] * k[0] + positions[1] * k[1] + positions[2] * k[2];
        let view = if self.perspective {
            (self.eye - world).normalize_or(-self.forward)
        } else {
            -self.forward
        };
        if self.material == Material::Card && self.card_texel(corners, k, depth).is_none() {
            return;
        }
        target.depth[at] = depth;
        if let Some((sample, round, along)) = bark {
            normal =
                (normal - round * f64::from(sample.slope[0]) - along * f64::from(sample.slope[1]))
                    .normalize_or(normal);
        }
        if self.material == Material::Card && normal.dot(view) < 0.0 {
            normal = -normal;
        }
        target.normal[at] = normal.to_f32();
    }

    /// The template and texel a leaf card shows at a pixel of triangle
    /// `corners` at weights `k`, `depth` from the eye; `None` where its
    /// outline cuts the pixel away.
    fn card_texel(
        &self,
        corners: [usize; 3],
        k: [f64; 3],
        depth: f64,
    ) -> Option<(usize, crate::templates::Texel)> {
        let mesh = self.mesh;
        let mix = |values: [f64; 3]| k[0] * values[0] + k[1] * values[1] + k[2] * values[2];
        let u = mix(corners.map(|i| f64::from(mesh.uvs[i][0])));
        let v = mix(corners.map(|i| f64::from(mesh.uvs[i][1])));
        // The template, and the card's face, are packed in alpha.
        let (template, _, _) = crate::templates::unpack_face(mesh.colors[corners[0]][3]);
        #[allow(clippy::cast_possible_truncation)]
        let (u, v) = (u as f32, v as f32);
        // Cut where the coverage, read between texels, is below one
        // half, so the card's edge follows its outline.
        if self.templates.coverage(template, u, v) < 0.5 {
            return None;
        }
        let texel = self.templates.sample(template, u, v);
        // Up close the edge's hairs are strands of their own.
        if texel.fringe
            && let Some(hairs) = self
                .templates
                .templates
                .get(template)
                .and_then(|drawn| drawn.hairs.as_ref())
        {
            let footprint = if self.perspective {
                self.pixel * depth
            } else {
                self.pixel
            };
            if !crate::hairs::fringe_drawn(hairs, footprint) {
                return None;
            }
        }
        Some((template, texel))
    }

    /// The bark pattern at a pixel of triangle `corners` at weights `k`,
    /// `depth` from the eye, and the triangle's unit directions round and
    /// along its stem; `None` off bark.
    fn bark(
        &self,
        corners: [usize; 3],
        k: [f64; 3],
        depth: f64,
    ) -> Option<(crate::bark::BarkSample, Vec3, Vec3)> {
        let Material::Bark(params) = self.material else {
            return None;
        };
        let mesh = self.mesh;
        if corners.iter().any(|&i| mesh.colors[i][3] >= 0.0) {
            return None;
        }
        let mix = |values: [f64; 3]| k[0] * values[0] + k[1] * values[1] + k[2] * values[2];
        let radius = -mix(corners.map(|i| f64::from(mesh.colors[i][3])));
        let u = mix(corners.map(|i| f64::from(mesh.uvs[i][0])));
        let v = mix(corners.map(|i| f64::from(mesh.uvs[i][1])));
        let footprint = if self.perspective {
            self.pixel * depth
        } else {
            self.pixel
        };
        #[allow(clippy::cast_possible_truncation)]
        let sample =
            crate::bark::sample(&params, u as f32, v as f32, radius as f32, footprint as f32);
        // The directions in which u and v grow across the triangle.
        let [p0, p1, p2] = corners.map(|i| vec3(mesh.positions[i]));
        let [t0, t1, t2] = corners.map(|i| mesh.uvs[i].map(f64::from));
        let (e1, e2) = (p1 - p0, p2 - p0);
        let (du1, dv1, du2, dv2) = (t1[0] - t0[0], t1[1] - t0[1], t2[0] - t0[0], t2[1] - t0[1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            return Some((sample, Vec3::ZERO, Vec3::ZERO));
        }
        let round = ((e1 * dv2 - e2 * dv1) * (1.0 / det)).normalize_or(Vec3::ZERO);
        let along = ((e2 * du1 - e1 * du2) * (1.0 / det)).normalize_or(Vec3::ZERO);
        Some((sample, round, along))
    }

    /// Depth-test, cut out and shade one pixel of the triangle with vertex
    /// indices `corners` at barycentric weights `k`.
    #[allow(clippy::too_many_lines)]
    fn fragment(
        &self,
        target: &mut Target,
        at: usize,
        corners: [usize; 3],
        k: [f64; 3],
        depth: f64,
    ) {
        if depth >= target.depth[at] {
            return;
        }
        // A shadow map keeps only depth: of a leaf card, only whether its
        // outline cuts the pixel away.
        if self.lighting.is_none() && self.material != Material::Baked {
            if self.material == Material::Card && self.card_texel(corners, k, depth).is_none() {
                return;
            }
            target.depth[at] = depth;
            return;
        }
        if self.normals_only
            && self.lighting.is_some()
            && matches!(
                self.material,
                Material::Card | Material::Opaque | Material::Bark(_)
            )
        {
            self.normal_only(target, at, corners, k, depth);
            return;
        }
        let mesh = self.mesh;
        let mix = |values: [f64; 3]| k[0] * values[0] + k[1] * values[1] + k[2] * values[2];
        let mut albedo =
            [0, 1, 2].map(|channel| mix(corners.map(|i| f64::from(mesh.colors[i][channel]))));
        let mut hair = None;
        let bark = self.bark(corners, k, depth);
        if let (Some((sample, _, _)), Material::Bark(params)) = (bark, self.material) {
            for (channel, value) in albedo.iter_mut().enumerate() {
                let shaded = *value * f64::from(sample.shade);
                *value =
                    shaded + (f64::from(params.inner[channel]) - shaded) * f64::from(sample.inner);
            }
        }
        let normals = corners.map(|i| vec3(mesh.normals[i]));
        let positions = corners.map(|i| vec3(mesh.positions[i]));
        let mut normal =
            (normals[0] * k[0] + normals[1] * k[1] + normals[2] * k[2]).normalize_or(Vec3::Y);
        let world = positions[0] * k[0] + positions[1] * k[1] + positions[2] * k[2];
        let view = if self.perspective {
            (self.eye - world).normalize_or(-self.forward)
        } else {
            -self.forward
        };
        if self.material == Material::Card {
            let Some((template, texel)) = self.card_texel(corners, k, depth) else {
                return;
            };
            // The template, and the card's face, are packed in alpha.
            let alpha = mesh.colors[corners[0]][3];
            let face = crate::templates::card_face(alpha, normal.dot(view) >= 0.0);
            albedo = self.templates.paint(template, albedo, texel, face);
            hair = Some((template, face.lower));
        }
        let mut baked_normal = None;
        let mut baked_sun = None;
        if self.material == Material::Baked {
            let u = mix(corners.map(|i| f64::from(mesh.uvs[i][0])));
            let v = mix(corners.map(|i| f64::from(mesh.uvs[i][1])));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = mesh.colors[corners[0]][3].max(0.0).round() as usize;
            let Some(texture) = self.templates.baked.get(index) else {
                return;
            };
            let (texel, drawn) = texture.sample(u, v);
            if texel[3] < 0.5 {
                return;
            }
            albedo = [texel[0], texel[1], texel[2]];
            if let Some(lighting) = self.lighting {
                baked_sun = texture.sunlit(u, v, lighting.sun);
            }
            // Seen from behind, the picture's normals mirror through the
            // card.
            let drawn = drawn.normalize_or(normal);
            baked_normal = Some(if normal.dot(view) >= 0.0 {
                drawn
            } else {
                drawn - normal * (2.0 * drawn.dot(normal))
            });
        }
        target.depth[at] = depth;
        let Some(lighting) = self.lighting else {
            return;
        };
        if let Some((sample, round, along)) = bark {
            // The relief tilts the normal against its slope.
            normal =
                (normal - round * f64::from(sample.slope[0]) - along * f64::from(sample.slope[1]))
                    .normalize_or(normal);
        }
        if self.material == Material::Hair {
            // A strand's normal is its tangent.
            let lit = self
                .shadow
                .map_or(1.0, |map| map.visibility(world, lighting.sun, lighting.sun));
            let light = crate::hairs::strand_light(albedo, normal, view, lit, lighting);
            #[allow(clippy::cast_possible_truncation)]
            {
                target.color[at] = [light[0] as f32, light[1] as f32, light[2] as f32, 1.0];
            }
            target.normal[at] = view.to_f32();
            return;
        }
        if self.material == Material::Drop {
            if normal.dot(view) <= 0.0 {
                return;
            }
            let clarity = f64::from(mesh.colors[corners[0]][3]).clamp(0.0, 1.0);
            let lit = self
                .shadow
                .map_or(1.0, |map| map.visibility(world, normal, lighting.sun));
            let behind = target.color[at];
            let behind = if behind[3] > 0.0 {
                [0, 1, 2].map(|c| f64::from(behind[c]))
            } else {
                lighting.sky.map(f64::from)
            };
            let light =
                crate::hairs::drop_light(albedo, behind, clarity, normal, view, lit, lighting);
            #[allow(clippy::cast_possible_truncation)]
            {
                target.color[at] = [light[0] as f32, light[1] as f32, light[2] as f32, 1.0];
            }
            target.depth[at] = depth;
            target.normal[at] = normal.to_f32();
            return;
        }
        if self.material == Material::Gland {
            let lit = self
                .shadow
                .map_or(1.0, |map| map.visibility(world, normal, lighting.sun));
            let light = crate::hairs::gland_light(albedo, normal, view, lit, lighting);
            #[allow(clippy::cast_possible_truncation)]
            {
                target.color[at] = [light[0] as f32, light[1] as f32, light[2] as f32, 1.0];
            }
            target.normal[at] = normal.to_f32();
            return;
        }
        if self.material == Material::Card && normal.dot(view) < 0.0 {
            normal = -normal;
        }
        if let Some(drawn) = baked_normal {
            normal = drawn;
        }
        // A baked card's own plant shades it as baked; the shadow map
        // holds only that plant's cards, a thin shell of what was there.
        let lit = baked_sun.unwrap_or_else(|| {
            self.shadow
                .map_or(1.0, |map| map.visibility(world, normal, lighting.sun))
        });
        let direct = normal.dot(lighting.sun).max(0.0) * lit;
        let through = if matches!(self.material, Material::Card | Material::Baked) {
            (-normal.dot(lighting.sun)).max(0.0) * lit * 0.35
        } else {
            0.0
        };
        let sky = 0.5 + 0.5 * normal.y;
        let mut color = [0.0_f32; 4];
        for channel in 0..3 {
            let light = f64::from(lighting.sun_color[channel]) * (direct + through)
                + f64::from(lighting.sky[channel]) * sky
                + f64::from(lighting.ground[channel]) * (1.0 - sky);
            #[allow(clippy::cast_possible_truncation)]
            {
                color[channel] = (albedo[channel] * light) as f32;
            }
        }
        if let Some((template, lower)) = hair
            && let Some(hairs) = self
                .templates
                .templates
                .get(template)
                .and_then(|drawn| drawn.hairs_on(lower))
        {
            let sheen = crate::hairs::sheen(hairs, normal, view, lit, lighting);
            for channel in 0..3 {
                #[allow(clippy::cast_possible_truncation)]
                {
                    color[channel] += sheen[channel] as f32;
                }
            }
        }
        color[3] = 1.0;
        target.color[at] = color;
        target.normal[at] = normal.to_f32();
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    target: &mut Target,
    mesh: &Mesh,
    material: Material,
    camera: &Camera,
    lighting: Option<&Lighting>,
    templates: &Templates,
    shadow: Option<&ShadowMap>,
    normals_only: bool,
) {
    let basis = camera.basis();
    let turned = if material == Material::Baked {
        turn_cards(mesh, templates, camera, basis.2)
    } else {
        None
    };
    let mesh = turned.as_ref().unwrap_or(mesh);
    let viewport = Viewport::new(camera, target.width, target.height);
    let surface = Surface {
        mesh,
        material,
        templates,
        lighting,
        shadow,
        normals_only,
        eye: camera.eye,
        forward: basis.2,
        perspective: viewport.perspective,
        pixel: 2.0 * viewport.half_y / viewport.height,
    };
    let mut views = VIEW_CORNERS.take();
    views.clear();
    views.extend(
        mesh.positions
            .iter()
            .map(|p| to_view(camera, basis, vec3(*p))),
    );
    let (width, height) = (target.width, target.height);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let corners = [
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        ];
        if material.one_sided() {
            // One-sided: skip triangles facing away from the viewer.
            let [p0, p1, p2] = corners.map(|i| vec3(mesh.positions[i]));
            let face = (p1 - p0).cross(p2 - p0);
            let toward_viewer = if viewport.perspective {
                face.dot(camera.eye - p0)
            } else {
                -face.dot(basis.2)
            };
            if toward_viewer <= 0.0 {
                continue;
            }
        }
        let (clipped, count) = clip_near(corners.map(|i| views[i]), viewport.perspective);
        let screen = clipped.map(|(view, weights)| {
            let (x, y) = viewport.screen(view);
            Corner {
                x,
                y,
                z: view.z,
                weights,
            }
        });
        let card = matches!(material, Material::Card | Material::Baked);
        let mut visit = |at: usize, weights: [f64; 3], depth: f64| {
            target.card_fragments += usize::from(card);
            surface.fragment(target, at, corners, weights, depth);
        };
        for fan in 1..count.saturating_sub(1) {
            rasterize(
                width,
                height,
                [screen[0], screen[fan], screen[fan + 1]],
                viewport.perspective,
                &mut visit,
            );
        }
    }
    VIEW_CORNERS.set(views);
}

/// A picture baked from nearer straight up than this cosine lies on the
/// ground ([`Turn::Heights`]).
const STRAIGHT_UP: f64 = 0.999;

/// `mesh`'s baked cards, each vertex's picture in its colour's alpha
/// ([`Material::Baked`]), turned to `camera` as their pictures say
/// ([`Texture::turn`]), each vertex's alpha then the picture it shows:
/// how PlantGen's renders draw them, and what an engine's vertex shader
/// must do (`shaders/turn_cards.wgsl`, [`crate::billboards::TURN_CARDS_WGSL`]).
#[must_use]
pub fn turned(mesh: &Mesh, templates: &Templates, camera: &Camera) -> Mesh {
    turn_cards(mesh, templates, camera, camera.basis().2).unwrap_or_else(|| mesh.clone())
}

/// `mesh`'s baked cards turned to `camera` as their pictures say
/// ([`Texture::turn`]); `None` when none turns.
fn turn_cards(mesh: &Mesh, templates: &Templates, camera: &Camera, forward: Vec3) -> Option<Mesh> {
    let picture = |i: usize| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let index = mesh.colors[i][3].max(0.0).round() as usize;
        templates.baked.get(index)
    };
    if !(0..mesh.positions.len()).any(|i| picture(i).is_some_and(|t| t.turn != Turn::Fixed)) {
        return None;
    }
    let perspective = matches!(camera.projection, Projection::Perspective { .. });
    let mut turned = mesh.clone();
    for i in 0..mesh.positions.len() {
        let Some(texture) = picture(i) else {
            continue;
        };
        let pivot = vec3(texture.pivot);
        let toward = if perspective {
            (camera.eye - pivot).normalize_or(-forward)
        } else {
            -forward
        };
        // The card's own frame, as it was baked: up is the world's up laid
        // on it, where it can be.
        let normal = vec3(mesh.normals[i]).normalize_or(Vec3::Z);
        let frame = |normal: Vec3, fallback: Vec3| {
            let up = (Vec3::Y - normal * Vec3::Y.dot(normal)).normalize_or_else(|| {
                (fallback - normal * fallback.dot(normal))
                    .normalize_or_else(|| any_perpendicular(normal))
            });
            (up.cross(normal), up)
        };
        let (right, up) = frame(normal, Vec3::Z);
        // The up, way and spread of the picture shown, when it is not
        // the card's own.
        let mut shown: Option<(Vec3, Vec3, Vec3)> = None;
        let facing = match texture.turn {
            Turn::Fixed => continue,
            Turn::Facing => toward,
            Turn::Heights { first, count } => {
                let elevation = |d: Vec3| math::asin(d.normalize_or(Vec3::Z).y.clamp(-1.0, 1.0));
                let eye = elevation(toward);
                let nearest = (first..first + count)
                    .filter_map(|v| templates.baked.get(v).map(|t| (v, t)))
                    .min_by(|a, b| {
                        let off = |t: &Texture| (elevation(vec3(t.seen_from)) - eye).abs();
                        off(a.1).total_cmp(&off(b.1))
                    });
                if let Some((v, t)) = nearest {
                    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                    {
                        turned.colors[i][3] = v as f32;
                    }
                    let way = vec3(t.seen_from).normalize_or(normal);
                    shown = Some((way.cross(right), way, vec3(t.extent)));
                }
                toward
            }
            Turn::Views { first, count } => {
                let nearest = (first..first + count)
                    .filter_map(|v| templates.baked.get(v).map(|t| (v, t)))
                    .max_by(|a, b| {
                        let along = |t: &Texture| vec3(t.seen_from).dot(toward);
                        along(a.1).total_cmp(&along(b.1))
                    });
                if let Some((v, _)) = nearest {
                    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                    {
                        turned.colors[i][3] = v as f32;
                    }
                }
                toward
            }
        };
        let (right_now, up_now) = match shown {
            // Seen from above, a picture baked from above lies on the
            // ground as it was baked, and turns as the eye goes round.
            Some((up_then, way, _)) if way.y > STRAIGHT_UP => {
                let up_now = (up_then - facing * up_then.dot(facing)).normalize_or(up_then);
                (up_now.cross(facing), up_now)
            }
            _ => frame(facing, up),
        };
        // A clump spreads less through its depth than across: turned to
        // an eye beside it, its card narrows to what the eye would see.
        let (up_then, normal_then, extent) = shown.unwrap_or((up, normal, vec3(texture.extent)));
        let across = |axis: Vec3| {
            let (a, b, c) = (axis.dot(right), axis.dot(up_then), axis.dot(normal_then));
            (a * a * extent.x * extent.x
                + b * b * extent.y * extent.y
                + c * c * extent.z * extent.z)
                .sqrt()
        };
        let scale = |now: Vec3, then: f64| {
            if then > 1e-9 {
                (across(now) / then).min(1.0)
            } else {
                1.0
            }
        };
        let offset = vec3(mesh.positions[i]) - pivot;
        let p = pivot
            + right_now * (offset.dot(right) * scale(right_now, extent.x))
            + up_now * (offset.dot(up) * scale(up_now, extent.y));
        turned.positions[i] = p.to_f32();
        turned.normals[i] = facing.to_f32();
    }
    Some(turned)
}

thread_local! {
    /// [`draw`]'s mesh corners in view space, kept on each thread between
    /// calls: an impostor draws its plant from 64 directions, and a fresh
    /// buffer for a large tree's million corners each time cost as much in
    /// page faults as the transform did.
    static VIEW_CORNERS: RefCell<Vec<Vec3>> = const { RefCell::new(Vec::new()) };
}

impl ShadowMap {
    /// 1 where the sun reaches `world`, 0 in shadow.
    /// 1 where `world`, on a surface facing `normal`, sees the light
    /// along `sun`, 0 where something in the map is in the way.
    #[must_use]
    pub fn visibility(&self, world: Vec3, normal: Vec3, sun: Vec3) -> f64 {
        let view = to_view(&self.camera, self.basis, world);
        let (x, y) = self.viewport.screen(view);
        // Also rejects NaN.
        if !(x >= 0.0 && y >= 0.0) {
            return 1.0;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (ix, iy) = (x as usize, y as usize);
        if ix >= self.size || iy >= self.size {
            return 1.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let texel = 2.0 * self.viewport.half_y / self.size as f64;
        let slope = (1.0 - normal.dot(sun).abs()).max(0.0);
        let bias = texel * (1.5 + 3.0 * slope);
        if view.z - bias > self.depth[iy * self.size + ix] {
            0.0
        } else {
            1.0
        }
    }
}

/// Linear 0..1 to an 8-bit value.
#[must_use]
pub fn to_u8(value: f32) -> u8 {
    // Clamped to 0..=255 first.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    byte
}

/// Linear to sRGB transfer function.
#[must_use]
pub fn srgb(linear: f32) -> f32 {
    let value = linear.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * libm::powf(value, 1.0 / 2.4) - 0.055
    }
}

impl Image {
    /// sRGB RGBA8 pixels, composited over `background` (linear RGB) if
    /// given, otherwise with coverage in alpha.
    #[must_use]
    pub fn to_srgb8(&self, background: Option<[f32; 3]>) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.color.len() * 4);
        for color in &self.color {
            let alpha = color[3];
            for channel in 0..3 {
                let value = match background {
                    Some(back) => color[channel] * alpha + back[channel] * (1.0 - alpha),
                    None => color[channel],
                };
                pixels.push(to_u8(srgb(value)));
            }
            pixels.push(if background.is_some() {
                255
            } else {
                to_u8(alpha)
            });
        }
        pixels
    }
}

/// Encode RGBA8 pixels as a PNG with fixed settings, so the bytes depend
/// only on the pixels.
///
/// # Errors
///
/// Fails only if the encoder fails.
pub fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, png::EncodingError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(
            &mut bytes,
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(bytes)
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::mesh::Vertex;

    fn quad(material_y: f32) -> Mesh {
        let mut mesh = Mesh::default();
        for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            mesh.push(Vertex {
                uv: [0.5, 0.5],
                color: [0.5, 0.5, 0.5, 3.0],
                ..Vertex::plain(Vec3::new(x, f64::from(material_y), z), Vec3::Y, [0.5; 3])
            });
        }
        // Counter-clockwise seen from above.
        mesh.indices = vec![0, 2, 1, 0, 3, 2];
        mesh
    }

    fn top_camera() -> Camera {
        Camera {
            eye: Vec3::new(0.0, 5.0, 0.0),
            target: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, -1.0),
            projection: Projection::Orthographic { half_height: 2.0 },
        }
    }

    /// A unit square card standing upright at the origin, facing +z,
    /// picture 0, and that picture as `turn` with `extent`.
    fn standing_card(turn: Turn, extent: [f32; 3]) -> (Mesh, Templates) {
        let mut mesh = Mesh::default();
        for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
            mesh.push(Vertex {
                color: [1.0, 1.0, 1.0, 0.0],
                ..Vertex::plain(Vec3::new(x, y, 0.0), Vec3::Z, [1.0; 3])
            });
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        let templates = Templates {
            baked: vec![Texture {
                width: 1,
                height: 1,
                color: vec![[1.0; 4]],
                normal: vec![[0.0, 0.0, 1.0]],
                sun: Vec::new(),
                turn,
                pivot: [0.0; 3],
                extent,
                seen_from: [0.0, 0.0, 1.0],
            }],
            ..Templates::default()
        };
        (mesh, templates)
    }

    #[test]
    fn a_facing_card_turns_to_the_eye_and_narrows_to_its_depth() {
        let side = Camera {
            eye: Vec3::new(5.0, 0.0, 0.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Orthographic { half_height: 1.0 },
        };
        let (mesh, templates) = standing_card(Turn::Fixed, [0.3; 3]);
        assert!(turn_cards(&mesh, &templates, &side, side.basis().2).is_none());
        // As deep as it is broad: seen from the side it is as wide.
        let (mesh, templates) = standing_card(Turn::Facing, [0.3; 3]);
        let turned = turn_cards(&mesh, &templates, &side, side.basis().2).expect("it turns");
        for (p, n) in turned.positions.iter().zip(&turned.normals) {
            assert!((n[0] - 1.0).abs() < 1e-6, "faces the eye: {n:?}");
            assert!(p[0].abs() < 1e-6, "lies across the view: {p:?}");
            assert!((p[2].abs() - 0.5).abs() < 1e-6, "keeps its width: {p:?}");
        }
        // A third as deep as it is broad: a third as wide from the side.
        let (mesh, templates) = standing_card(Turn::Facing, [0.3, 0.3, 0.1]);
        let turned = turn_cards(&mesh, &templates, &side, side.basis().2).expect("it turns");
        for p in &turned.positions {
            assert!((p[2].abs() - 0.5 / 3.0).abs() < 1e-6, "narrows: {p:?}");
            assert!((p[1].abs() - 0.5).abs() < 1e-6, "keeps its height: {p:?}");
        }
    }

    #[test]
    fn draws_a_quad_with_depth_and_is_deterministic() {
        let mesh = quad(0.0);
        let options = RenderOptions {
            width: 32,
            height: 32,
            supersample: 2,
            shadows: false,
            shadow_bounds: None,
            shadow_texels: SHADOW_TEXELS,
        };
        let templates = Templates::default();
        let render_once = || {
            render(
                &[(&mesh, Material::Opaque)],
                &top_camera(),
                &Lighting::daylight(),
                &templates,
                &options,
            )
        };
        let image = render_once();
        let center = image.color[16 * 32 + 16];
        assert_eq!(center[3], 1.0);
        assert!((image.depth[16 * 32 + 16] - 5.0).abs() < 1e-5);
        // Outside the quad nothing is drawn.
        assert_eq!(image.color[0][3], 0.0);
        assert_eq!(image, render_once());
    }

    #[test]
    fn opaque_back_faces_are_culled_and_shadows_fall_on_lower_surfaces() {
        let templates = Templates::default();
        let options = RenderOptions {
            width: 16,
            height: 16,
            supersample: 1,
            shadows: true,
            shadow_bounds: None,
            shadow_texels: SHADOW_TEXELS,
        };
        let mut flipped = quad(0.0);
        flipped.indices = vec![0, 1, 2, 0, 2, 3];
        let image = render(
            &[(&flipped, Material::Opaque)],
            &top_camera(),
            &Lighting::daylight(),
            &templates,
            &options,
        );
        assert!(image.color.iter().all(|c| c[3] == 0.0));

        // A small roof 1 m up, with the sun 45° to the east, shades the
        // ground 1 m west of it.
        let mut lighting = Lighting::daylight();
        lighting.sun = Vec3::new(1.0, 1.0, 0.0).normalize_or(Vec3::Y);
        let ground = quad(0.0);
        let mut roof = quad(1.0);
        for position in &mut roof.positions {
            position[0] *= 0.25;
            position[2] *= 0.25;
        }
        let lit = render(
            &[(&ground, Material::Opaque)],
            &top_camera(),
            &lighting,
            &templates,
            &options,
        );
        let shaded = render(
            &[(&ground, Material::Opaque), (&roof, Material::Opaque)],
            &top_camera(),
            &lighting,
            &templates,
            &options,
        );
        // World (-0.9, 0, 0) is pixel (4, 8); (0.9, 0, 0) is pixel (11, 8).
        let shadowed = 8 * 16 + 4;
        let sunny = 8 * 16 + 11;
        assert!(shaded.color[shadowed][0] < lit.color[shadowed][0] * 0.8);
        assert!((shaded.color[sunny][0] - lit.color[sunny][0]).abs() < 1e-6);
    }

    #[test]
    fn triangles_crossing_the_near_plane_are_clipped_not_dropped() {
        // A ground quad that reaches behind a camera standing 1.7 m above
        // it fills everything below the horizon.
        let mut ground = quad(0.0);
        for position in &mut ground.positions {
            position[0] *= 100.0;
            position[2] *= 100.0;
        }
        let camera = Camera {
            eye: Vec3::new(0.0, 1.7, 0.0),
            target: Vec3::new(0.0, 1.7, -10.0),
            up: Vec3::Y,
            projection: Projection::Perspective { fov_y: 60.0 },
        };
        let options = RenderOptions {
            width: 32,
            height: 32,
            supersample: 1,
            shadows: false,
            shadow_bounds: None,
            shadow_texels: SHADOW_TEXELS,
        };
        let image = render(
            &[(&ground, Material::Opaque)],
            &camera,
            &Lighting::daylight(),
            &Templates::default(),
            &options,
        );
        assert!((0..32).all(|x| image.color[31 * 32 + x][3] == 1.0));
        assert!((0..32).all(|x| image.color[x][3] == 0.0));
        // The bottom row's centre ray drops 0.96875 * tan(30°) per metre
        // and meets the ground 1.7 m down.
        let expected = 1.7 / (0.968_75 * math::tan(math::radians(30.0)));
        assert!((f64::from(image.depth[31 * 32 + 16]) - expected).abs() < 1e-3);
    }

    #[test]
    fn near_clipping_keeps_weights_on_the_original_triangle() {
        let view = [
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, -2.0),
            Vec3::new(-1.0, 1.0, -2.0),
        ];
        let (corners, count) = clip_near(view, true);
        assert_eq!(count, 3);
        for (point, weights) in &corners[..count] {
            assert!(point.z >= NEAR);
            assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            let rebuilt = view[0] * weights[0] + view[1] * weights[1] + view[2] * weights[2];
            assert!(rebuilt.distance(*point) < 1e-12);
        }
        let (_, count) = clip_near([view[1], view[2], Vec3::new(0.0, 1.0, -3.0)], true);
        assert_eq!(count, 0);
        let (_, count) = clip_near([view[0], Vec3::new(0.0, 1.0, 3.0), view[1]], true);
        assert_eq!(count, 4);
    }

    #[test]
    fn srgb_encoding_matches_the_standard_curve() {
        assert_eq!(to_u8(srgb(0.0)), 0);
        assert_eq!(to_u8(srgb(1.0)), 255);
        assert_eq!(to_u8(srgb(0.215_9)), 128);
    }
}
