//! Preview renders for people: a plant on a patch of ground, lit by a
//! low sun, with a scale beside it, and contact sheets of several renders
//! side by side. The scale is a 1.8 m figure beside plants of at least
//! [`SMALL_PLANT`], and a rod striped in 10 cm bands beside smaller ones.

use crate::math::Vec3;
use crate::mesh::{Mesh, PlantMesh, Vertex};
use crate::raster::{self, Camera, Image, Lighting, Material, Projection, RenderOptions};
use crate::templates::Templates;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Level with the plant, from the south.
    Side,
    /// From the south-east, a little above.
    ThreeQuarter,
    /// Straight down.
    Top,
}

impl View {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "side" => Self::Side,
            "three-quarter" => Self::ThreeQuarter,
            "top" => Self::Top,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewOptions {
    pub width: usize,
    pub height: usize,
    pub view: View,
    pub supersample: usize,
    /// Draw a scale beside the plant: a 1.8 m figure, or a striped rod
    /// beside a plant lower than [`SMALL_PLANT`].
    pub figure: bool,
    /// Frame this height (metres) instead of the plant's own, so renders
    /// of different ages share a scale.
    pub frame_height: Option<f64>,
}

pub const SKY: [f32; 3] = [0.52, 0.62, 0.74];
const GROUND: [f32; 3] = [0.16, 0.19, 0.11];
const FIGURE: [f32; 3] = [0.55, 0.22, 0.12];
const ROD_LIGHT: [f32; 3] = [0.7, 0.7, 0.66];

/// Plants framed lower than this, in metres, get the striped rod instead
/// of the figure: the figure would dwarf them.
pub const SMALL_PLANT: f64 = 1.5;

fn push_quad(mesh: &mut Mesh, corners: [Vec3; 4], normal: Vec3, color: [f32; 3]) {
    let base = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for corner in corners {
        mesh.push(Vertex::plain(corner, normal, color));
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// A square of ground centred under the plant.
fn ground(half: f64) -> Mesh {
    let mut mesh = Mesh::default();
    push_quad(
        &mut mesh,
        [
            Vec3::new(-half, 0.0, half),
            Vec3::new(half, 0.0, half),
            Vec3::new(half, 0.0, -half),
            Vec3::new(-half, 0.0, -half),
        ],
        Vec3::Y,
        GROUND,
    );
    mesh
}

/// A box `2 * wide` across and `2 * deep` deep, centred on `x`, from
/// height `low` to `high`: four sides and a top.
fn push_box(
    mesh: &mut Mesh,
    x: f64,
    (wide, deep): (f64, f64),
    (low, high): (f64, f64),
    color: [f32; 3],
) {
    let at = |dx: f64, y: f64, dz: f64| Vec3::new(x + dx, y, dz);
    let faces = [
        (
            [
                at(-wide, low, deep),
                at(wide, low, deep),
                at(wide, high, deep),
                at(-wide, high, deep),
            ],
            Vec3::Z,
        ),
        (
            [
                at(wide, low, -deep),
                at(-wide, low, -deep),
                at(-wide, high, -deep),
                at(wide, high, -deep),
            ],
            -Vec3::Z,
        ),
        (
            [
                at(wide, low, deep),
                at(wide, low, -deep),
                at(wide, high, -deep),
                at(wide, high, deep),
            ],
            Vec3::X,
        ),
        (
            [
                at(-wide, low, -deep),
                at(-wide, low, deep),
                at(-wide, high, deep),
                at(-wide, high, -deep),
            ],
            -Vec3::X,
        ),
        (
            [
                at(-wide, high, deep),
                at(wide, high, deep),
                at(wide, high, -deep),
                at(-wide, high, -deep),
            ],
            Vec3::Y,
        ),
    ];
    for (corners, normal) in faces {
        push_quad(mesh, corners, normal, color);
    }
}

/// A 1.8 m tall, 0.45 m wide box standing at `x`: the scale figure.
fn figure(x: f64) -> Mesh {
    let mut mesh = Mesh::default();
    push_box(&mut mesh, x, (0.225, 0.14), (0.0, 1.8), FIGURE);
    mesh
}

/// A rod `height` tall standing at `x`, striped in 10 cm bands: the scale
/// beside small plants.
fn rod(x: f64, height: f64) -> Mesh {
    let mut mesh = Mesh::default();
    let half = 0.012;
    // Whole bands only; the count is at most 10.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bands = (height / 0.1).round().max(1.0) as u32;
    for band in 0..bands {
        let low = f64::from(band) * 0.1;
        let color = if band % 2 == 0 { FIGURE } else { ROD_LIGHT };
        push_box(&mut mesh, x, (half, half), (low, low + 0.1), color);
    }
    mesh
}

/// The scale for a plant framed `height` tall: the figure, or for a small
/// plant a rod 0.5 m or 1 m tall. Returns the mesh, its height and the gap
/// to leave between the plant and the scale.
fn scale(height: f64, x: f64) -> (Mesh, f64, f64) {
    if height >= SMALL_PLANT {
        (figure(x + 0.8), 1.8, 0.8)
    } else {
        let tall = if height < 0.5 { 0.5 } else { 1.0 };
        let gap = (0.25 * height).max(0.08);
        (rod(x + gap, tall), tall, gap)
    }
}

/// Render a plant mesh for review.
#[must_use]
pub fn render(plant: &PlantMesh, templates: &Templates, options: &PreviewOptions) -> Image {
    let cards = plant.card_mesh();
    let items = [(&plant.wood, Material::Opaque), (&cards, Material::Card)];
    let (low, high) = raster::bounds(&items).unwrap_or((Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)));
    let reach = low
        .x
        .abs()
        .max(high.x.abs())
        .max(low.z.abs())
        .max(high.z.abs())
        .max(0.1);
    let framed = options.frame_height.unwrap_or(high.y);
    let (figure_mesh, figure_height, gap) = scale(framed, reach);
    let figure_x = reach + gap;
    let height = framed.max(figure_height * 1.1);

    #[allow(clippy::cast_precision_loss)]
    let aspect = options.width as f64 / options.height as f64;
    let fov = 30.0_f64;
    let half_fov = crate::math::tan(crate::math::radians(fov * 0.5));
    // Frame the plant and the figure: x from -reach to the figure's far side.
    let right = if options.figure {
        figure_x + (0.5 * gap).max(0.05)
    } else {
        reach
    };
    let center_x = (right - reach) * 0.5;
    let half_width = f64::midpoint(right, reach);
    let distance = (height * 0.5 / half_fov).max(half_width / (half_fov * aspect)) * 1.12 + reach;
    // Ground far past the frame, so its edge never shows below the horizon.
    let ground_mesh = ground((distance * 30.0).max(200.0));
    let mut meshes = vec![(&ground_mesh, Material::Opaque)];
    if options.figure {
        meshes.push((&figure_mesh, Material::Opaque));
    }
    meshes.extend(items);
    let camera = match options.view {
        View::Side | View::ThreeQuarter => {
            let target = Vec3::new(center_x, height * 0.5, 0.0);
            let direction = if options.view == View::Side {
                Vec3::new(0.0, 0.04, 1.0)
            } else {
                Vec3::new(0.7, 0.3, 0.7)
            }
            .normalize_or(Vec3::Z);
            Camera {
                eye: target + direction * distance,
                target,
                up: Vec3::Y,
                projection: Projection::Perspective { fov_y: fov },
            }
        }
        View::Top => Camera {
            eye: Vec3::new(center_x, height + 50.0, 0.0),
            target: Vec3::new(center_x, 0.0, 0.0),
            up: Vec3::new(0.0, 0.0, -1.0),
            projection: Projection::Orthographic {
                half_height: (half_width / aspect).max(reach * 1.1),
            },
        },
    };
    let mut shaded = vec![(&figure_mesh, Material::Opaque)];
    shaded.extend(items);
    let shadow_bounds = raster::bounds(&shaded).map(|(low, high)| {
        // Room on the ground for the shadows of a low sun.
        let pad = Vec3::new(high.y * 1.5, 0.0, high.y * 1.5);
        (low - pad, high + pad)
    });
    let render_options = RenderOptions {
        width: options.width,
        height: options.height,
        supersample: options.supersample,
        shadows: true,
        shadow_bounds,
    };
    raster::render(
        &meshes,
        &camera,
        &Lighting::daylight(),
        templates,
        &render_options,
    )
}

/// Lay images out in a grid, `columns` wide, on the sky colour with a
/// `gap`-pixel border. Every image must be the same size.
#[must_use]
pub fn contact_sheet(images: &[Image], columns: usize, gap: usize) -> (usize, usize, Vec<u8>) {
    let Some(first) = images.first() else {
        return (0, 0, Vec::new());
    };
    let columns = columns.max(1);
    let rows = images.len().div_ceil(columns);
    let width = columns * first.width + (columns + 1) * gap;
    let height = rows * first.height + (rows + 1) * gap;
    let background = [
        raster::to_u8(raster::srgb(0.9)),
        raster::to_u8(raster::srgb(0.9)),
        raster::to_u8(raster::srgb(0.9)),
        255,
    ];
    let mut pixels = background.repeat(width * height);
    for (index, image) in images.iter().enumerate() {
        let (row, column) = (index / columns, index % columns);
        let x0 = gap + column * (first.width + gap);
        let y0 = gap + row * (first.height + gap);
        let rgba = image.to_srgb8(Some(SKY));
        for y in 0..image.height.min(first.height) {
            let source =
                &rgba[y * image.width * 4..(y * image.width + image.width.min(first.width)) * 4];
            let start = ((y0 + y) * width + x0) * 4;
            pixels[start..start + source.len()].copy_from_slice(source);
        }
    }
    (width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_plants_get_a_striped_rod_and_others_the_figure() {
        let (_, tall, gap) = scale(SMALL_PLANT, 1.0);
        assert!((tall - 1.8).abs() < 1e-12 && (gap - 0.8).abs() < 1e-12);
        let (rod, tall, _) = scale(0.9, 0.3);
        assert!((tall - 1.0).abs() < 1e-12);
        // Ten 10 cm bands, each a box of five faces of two triangles.
        assert_eq!(rod.indices.len(), 10 * 5 * 6);
        let (rod, tall, _) = scale(0.3, 0.1);
        assert!((tall - 0.5).abs() < 1e-12);
        assert_eq!(rod.indices.len(), 5 * 5 * 6);
    }
}
