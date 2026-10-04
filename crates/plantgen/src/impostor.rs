//! Hemi-octahedral impostors: a plant rendered from a grid of directions
//! over the upper hemisphere, for drawing it far away as one card.
//!
//! The grid maps the upper hemisphere onto a square rotated by 45°. A cell
//! at grid coordinates `(u, v)` in `[-1, 1]²` looks along the direction
//!
//! ```text
//! p = ((u + v) / 2, (u - v) / 2)
//! d = normalize(p.x, 1 - |p.x| - |p.y|, p.y)
//! ```
//!
//! and a direction `d` (with `d.y >= 0`) is found again from
//! `p = (d.x, d.z) / (|d.x| + |d.y| + |d.z|)`, `u = p.x + p.y`, `v = p.x - p.y`.
//! A renderer picks the cells around the camera's direction and blends them.
//!
//! Each view is an orthographic render of the plant's bounding sphere, from
//! `centre + d · 2r` toward the centre with world +Y as up (−Z for a view
//! within a hair of straight down, which a grid with an even number of
//! views never has). Two atlases are baked: albedo with coverage in alpha
//! under even, shadowless light, and world-space normal with depth in
//! alpha, where 0 is the front of the bounding sphere and 1 its back.
//!
//! Empty texels take the colour and normal of the nearest covered texel in
//! their view (keeping zero coverage), so texture filtering and mipmaps
//! never blend black into the silhouette.

use std::collections::VecDeque;

use crate::math::Vec3;
use crate::mesh::PlantMesh;
use crate::raster::{self, Camera, Lighting, Material, Projection, RenderOptions};
use crate::templates::Templates;

/// A baked impostor: two square atlases of `views × views` tiles of
/// `size × size` pixels, tile `(column, row)` at pixel
/// `(column · size, row · size)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Impostor {
    pub views: usize,
    pub size: usize,
    /// Centre and radius of the bounding sphere the views frame, metres.
    pub center: Vec3,
    pub radius: f64,
    /// sRGB colour, coverage in alpha.
    pub albedo: Vec<u8>,
    /// World-space normal as `n · 0.5 + 0.5`, depth in alpha.
    pub normal_depth: Vec<u8>,
}

impl Impostor {
    /// Pixels along each side of an atlas.
    #[must_use]
    pub fn atlas_size(&self) -> usize {
        self.views * self.size
    }
}

/// Grid coordinate in `[-1, 1]` of the centre of cell `index` of `views`.
fn cell_center(index: usize, views: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let (index, views) = (index as f64, views as f64);
    (index + 0.5) / views * 2.0 - 1.0
}

/// The direction (pointing from the plant toward the viewer) of the cell in
/// `column` and `row` of a `views × views` grid.
#[must_use]
pub fn view_direction(column: usize, row: usize, views: usize) -> Vec3 {
    let (u, v) = (cell_center(column, views), cell_center(row, views));
    let (px, pz) = (f64::midpoint(u, v), (u - v) * 0.5);
    Vec3::new(px, 1.0 - px.abs() - pz.abs(), pz).normalize_or(Vec3::Y)
}

/// Grid coordinates `(u, v)` in `[-1, 1]²` of a direction in the upper
/// hemisphere; directions below the horizon are treated as level.
#[must_use]
pub fn grid_coordinates(direction: Vec3) -> (f64, f64) {
    let up = direction.y.max(0.0);
    let sum = direction.x.abs() + up + direction.z.abs();
    if sum <= 0.0 {
        return (0.0, 0.0);
    }
    let (px, pz) = (direction.x / sum, direction.z / sum);
    (px + pz, px - pz)
}

/// The camera for one view of a sphere.
#[must_use]
pub fn view_camera(direction: Vec3, center: Vec3, radius: f64) -> Camera {
    let up = if direction.y > 0.999 {
        Vec3::new(0.0, 0.0, -1.0)
    } else {
        Vec3::Y
    };
    Camera {
        eye: center + direction * (radius * 2.0),
        target: center,
        up,
        projection: Projection::Orthographic {
            half_height: radius,
        },
    }
}

/// Render the impostor atlases of a plant mesh.
#[must_use]
pub fn bake(
    plant: &PlantMesh,
    templates: &Templates,
    views: usize,
    size: usize,
    supersample: usize,
) -> Impostor {
    let cards = plant.card_mesh();
    let items = [(&plant.wood, Material::Opaque), (&cards, Material::Card)];
    let (low, high) = raster::bounds(&items).unwrap_or((Vec3::ZERO, Vec3::ZERO));
    let center = (low + high) * 0.5;
    let radius = ((high - low).length() * 0.5).max(0.05);
    let atlas = views * size;
    let mut albedo = vec![0_u8; atlas * atlas * 4];
    let mut normal_depth = vec![0_u8; atlas * atlas * 4];
    let options = RenderOptions {
        width: size,
        height: size,
        supersample,
        shadows: false,
        shadow_bounds: None,
    };
    for row in 0..views {
        for column in 0..views {
            let direction = view_direction(column, row, views);
            let camera = view_camera(direction, center, radius);
            let image = raster::render(&items, &camera, &Lighting::flat(), templates, &options);
            let colour = image.to_srgb8(None);
            let covered: Vec<bool> = colour
                .as_chunks::<4>()
                .0
                .iter()
                .map(|texel| texel[3] > 0)
                .collect();
            let nearest = nearest_covered(&covered, size);
            for y in 0..size {
                for x in 0..size {
                    let own = y * size + x;
                    let source = nearest.as_ref().map_or(own, |nearest| nearest[own]);
                    let target = ((row * size + y) * atlas + column * size + x) * 4;
                    albedo[target..target + 3].copy_from_slice(&colour[source * 4..source * 4 + 3]);
                    albedo[target + 3] = colour[own * 4 + 3];
                    let normal = image.normal[source];
                    // The camera is 2r from the centre: the sphere spans
                    // depths r to 3r.
                    #[allow(clippy::cast_possible_truncation)]
                    let depth = ((f64::from(image.depth[own]) - radius) / (2.0 * radius)) as f32;
                    normal_depth[target..target + 4].copy_from_slice(&[
                        raster::to_u8(normal[0] * 0.5 + 0.5),
                        raster::to_u8(normal[1] * 0.5 + 0.5),
                        raster::to_u8(normal[2] * 0.5 + 0.5),
                        raster::to_u8(if depth.is_finite() { depth } else { 1.0 }),
                    ]);
                }
            }
        }
    }
    Impostor {
        views,
        size,
        center,
        radius,
        albedo,
        normal_depth,
    }
}

/// For every texel of a `size × size` view, the index of a nearest covered
/// texel, found breadth-first over the four neighbours so that ties always
/// go the same way; `None` when nothing in the view is covered.
fn nearest_covered(covered: &[bool], size: usize) -> Option<Vec<usize>> {
    let mut source = vec![usize::MAX; covered.len()];
    let mut queue = VecDeque::new();
    for (index, _) in covered.iter().enumerate().filter(|(_, covered)| **covered) {
        source[index] = index;
        queue.push_back(index);
    }
    if queue.is_empty() {
        return None;
    }
    while let Some(index) = queue.pop_front() {
        let (x, y) = (index % size, index / size);
        let neighbours = [
            (x > 0).then(|| index - 1),
            (x + 1 < size).then(|| index + 1),
            (y > 0).then(|| index - size),
            (y + 1 < size).then(|| index + size),
        ];
        for neighbour in neighbours.into_iter().flatten() {
            if source[neighbour] == usize::MAX {
                source[neighbour] = source[index];
                queue.push_back(neighbour);
            }
        }
    }
    Some(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Mesh, Vertex};

    #[test]
    fn grid_covers_the_upper_hemisphere_and_round_trips() {
        let views = 8;
        for row in 0..views {
            for column in 0..views {
                let direction = view_direction(column, row, views);
                assert!((direction.length() - 1.0).abs() < 1e-12);
                assert!(direction.y >= 0.0);
                let (u, v) = grid_coordinates(direction);
                assert!(
                    (u - cell_center(column, views)).abs() < 1e-12,
                    "{column} {row}"
                );
                assert!(
                    (v - cell_center(row, views)).abs() < 1e-12,
                    "{column} {row}"
                );
            }
        }
        // The middle of the grid looks straight down; its corners are level.
        assert!(view_direction(3, 3, 8).y > 0.95);
        assert!(view_direction(0, 0, 8).y < 0.2);
        // Straight down maps to the centre.
        assert_eq!(grid_coordinates(Vec3::Y), (0.0, 0.0));
    }

    #[test]
    fn baking_frames_the_plant_and_is_deterministic() {
        // A 1 m cube standing on the ground, faces wound outward.
        let mut wood = Mesh::default();
        let p = Vec3::new;
        let faces = [
            (
                [
                    p(-0.5, 0.0, 0.5),
                    p(0.5, 0.0, 0.5),
                    p(0.5, 1.0, 0.5),
                    p(-0.5, 1.0, 0.5),
                ],
                Vec3::Z,
            ),
            (
                [
                    p(0.5, 0.0, -0.5),
                    p(-0.5, 0.0, -0.5),
                    p(-0.5, 1.0, -0.5),
                    p(0.5, 1.0, -0.5),
                ],
                -Vec3::Z,
            ),
            (
                [
                    p(0.5, 0.0, 0.5),
                    p(0.5, 0.0, -0.5),
                    p(0.5, 1.0, -0.5),
                    p(0.5, 1.0, 0.5),
                ],
                Vec3::X,
            ),
            (
                [
                    p(-0.5, 0.0, -0.5),
                    p(-0.5, 0.0, 0.5),
                    p(-0.5, 1.0, 0.5),
                    p(-0.5, 1.0, -0.5),
                ],
                -Vec3::X,
            ),
            (
                [
                    p(-0.5, 1.0, 0.5),
                    p(0.5, 1.0, 0.5),
                    p(0.5, 1.0, -0.5),
                    p(-0.5, 1.0, -0.5),
                ],
                Vec3::Y,
            ),
            (
                [
                    p(-0.5, 0.0, -0.5),
                    p(0.5, 0.0, -0.5),
                    p(0.5, 0.0, 0.5),
                    p(-0.5, 0.0, 0.5),
                ],
                -Vec3::Y,
            ),
        ];
        for (corners, normal) in faces {
            let base = u32::try_from(wood.positions.len()).unwrap();
            for corner in corners {
                wood.push(Vertex::plain(corner, normal, [0.5, 0.3, 0.1]));
            }
            wood.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let plant = PlantMesh {
            wood,
            cards: Vec::new(),
        };
        let templates = Templates::default();
        let impostor = bake(&plant, &templates, 4, 16, 1);
        assert_eq!(impostor.atlas_size(), 64);
        assert_eq!(impostor.albedo.len(), 64 * 64 * 4);
        assert!((impostor.center.y - 0.5).abs() < 1e-6);
        // Every view sees the cube, and no view is filled by it.
        for row in 0..4 {
            for column in 0..4 {
                let covered = (0..16 * 16)
                    .filter(|pixel| {
                        let (x, y) = (pixel % 16, pixel / 16);
                        impostor.albedo[((row * 16 + y) * 64 + column * 16 + x) * 4 + 3] > 0
                    })
                    .count();
                assert!(
                    covered > 0 && covered < 256,
                    "view {column},{row} covers {covered}"
                );
            }
        }
        // Empty texels carry the cube's colour, not black, with no coverage.
        let corner = &impostor.albedo[0..4];
        assert_eq!(corner[3], 0);
        assert!(corner[0] > 0 && corner[1] > 0, "{corner:?}");
        assert_eq!(impostor, bake(&plant, &templates, 4, 16, 1));
    }

    #[test]
    fn nearest_covered_texels_fill_outward_from_the_silhouette() {
        // A 4 × 4 view with only texel (1, 1) covered.
        let mut covered = vec![false; 16];
        covered[5] = true;
        let nearest = nearest_covered(&covered, 4).unwrap();
        assert!(nearest.iter().all(|source| *source == 5));
        assert!(nearest_covered(&[false; 16], 4).is_none());
    }
}
