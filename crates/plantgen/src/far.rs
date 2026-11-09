//! A whole filled tile as one far stand-in ([`crate::fill::Fill::canopy`]):
//! a height field of its crowns' tops with a picture of them seen from
//! straight above, for drawing a forest at a kilometre and more.
//!
//! Past the distance at which a tile's farthest level (its plants' cards,
//! [`crate::fill::TilePlan::glb_cells`]) is enough, a tile still holds
//! thousands of plants. Games draw forest that far as a surface: Bruneton
//! and Neyret (2012, "Real-time realistic rendering and lighting of
//! forests", Computer Graphics Forum 31(2)) draw it as a z-field, the
//! canopy's height over the terrain, with its colour and shading in maps;
//! Serious Sam 4 (GDC 2019, "4 million acres") splats its farthest forest
//! into the terrain past its tree impostors. This is the first of those,
//! made from the tile's own plants so it agrees with them:
//!
//! 1. Every placed plant at least [`FarSettings::least_height_m`] tall is
//!    drawn at the farthest level ([`FarSettings::level`]), as the level-3
//!    cells draw it, and rendered from straight above, orthographic, under
//!    even shadowless light ([`crate::raster::Lighting::flat`], as
//!    [`crate::impostor::bake`] bakes a plant's albedo), each prototype in
//!    its own pass so every species keeps its own cards and bark, the
//!    passes merged by depth. Each texel keeps the nearest crown's colour,
//!    the share of it covered (coverage kept as the plants cover,
//!    [`crate::impostor`]'s coverage preservation) and the top's height.
//! 2. The texels' sky view, from the height field they make (crowns where
//!    covered, the ground between): the share of the sky's light that
//!    reaches a level surface there, the mean of `cos² h` over eight
//!    directions, `h` each one's horizon. Gaps between crowns and the
//!    lower crowns among tall ones see less sky, and read darker from
//!    afar, as they do in photographs from the air. It goes to glTF's
//!    occlusion map, which a renderer applies to sky light only.
//! 3. A vertex every [`FarSettings::grid_m`]: the highest covered texel
//!    round it, or the ground where none is covered. Triangles all of
//!    whose corners stand on bare ground are left out, so a lawn costs
//!    nothing; a stand's side drops from its crowns to the ground and is
//!    its edge seen from low.
//!
//! The file ([`FarCanopy::glb`]) is one mesh and one material: the colour
//! with the cover in alpha, cut out at a half (`MASK`), and the
//! occlusion map. Its root sits at the tile's south-west corner (+X east,
//! +Y up, -Z north), as the tile's other files do, so tiles laid side by
//! side meet. It reads its neighbours' plans where given
//! ([`crate::fill::Fill::canopy_among`]) so crowns that reach across a
//! side are drawn on both and the two edges' heights agree.

use std::sync::{Arc, PoisonError};

use serde::Serialize;
use serde_json::{Value, json};

use crate::api::Error;
use crate::fill::{Fill, Instance, Site, TileId, TilePlan, WallClip};
use crate::gltf::Document;
use crate::impostor;
use crate::math::Vec3;
use crate::mesh::{Mesh, PlantMesh};
use crate::raster::{self, Camera, Lighting, Material, Projection, RenderOptions};
use crate::templates::Templates;

/// How a far stand-in is made.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FarSettings {
    /// The side of a texel of its picture, metres (rounded so a whole
    /// number of them spans the tile). Half a metre is a pixel at about
    /// 470 m for a camera 1080 pixels high and 60 degrees across
    /// ([`FarCanopy::from_m`]).
    pub texel_m: f64,
    /// The spacing of its height field's vertices, metres (rounded so a
    /// whole number spans the tile): 2 m gives a crown 6 m across a few
    /// vertices, enough for its dome to catch the sun.
    pub grid_m: f64,
    /// Spaces between crowns narrower than this, or than the tile's
    /// crowns are tall above the ground on average, are bridged, metres:
    /// the height field runs over them and the picture shows what lies
    /// beneath in the shade it stands in, so a closed stand is one
    /// surface and only openings drop to the ground. 5 m, the width of the smallest canopy
    /// gap field ecologists count (Brokaw 1982, Biotropica 14: 158-160:
    /// 20 m², a hole down to 2 m above the ground), as a pit-free canopy
    /// height model from lidar fills the pits between crowns
    /// (Khosravipour et al. 2014, Photogrammetric Engineering & Remote
    /// Sensing 80: 863-872).
    pub gap_m: f64,
    /// The level each plant is drawn at, 0 nearest to 3 farthest: the
    /// level the tile's cells draw just before this takes over.
    pub level: usize,
    /// Plants shorter than this are left out, metres. 0 draws every
    /// plant, as the tile's cells do; a host that draws the ground's
    /// herbs and shrubs with its own ground cover ([`TilePlan::cover`])
    /// may leave them out, say below 1.4 m, the least tree forest
    /// inventories count ([`crate::fill::LayerCover::TREE_MIN_M`]).
    pub least_height_m: f64,
    /// Plants at least this tall make the height field, metres; shorter
    /// ones are in the picture only, on the ground beneath it. 1.4 m, the
    /// least tree forest inventories count
    /// ([`crate::fill::LayerCover::TREE_MIN_M`]).
    pub shell_least_m: f64,
    /// Samples per texel along each side.
    pub supersample: usize,
    /// Threads that grow the prototypes not drawn before.
    #[serde(skip)]
    pub threads: usize,
}

impl Default for FarSettings {
    fn default() -> Self {
        Self {
            texel_m: 0.5,
            grid_m: 2.0,
            gap_m: 5.0,
            level: 3,
            least_height_m: 0.0,
            shell_least_m: crate::fill::LayerCover::TREE_MIN_M,
            supersample: 2,
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        }
    }
}

/// The numbers a far stand-in can be checked against its tile by.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct FarStats {
    /// The plants drawn into it.
    pub plants: usize,
    /// The share of the tile its picture covers (alpha at least a half).
    pub cover: f64,
    /// The mean colour of its covered texels, linear RGB.
    pub colour: [f64; 3],
    /// The mean height of its covered texels above the ground, metres.
    pub height_m: f64,
    /// The mean sky view of its covered texels, 0 to 1.
    pub sky_view: f64,
    /// Triangles in its mesh.
    pub triangles: usize,
}

/// A whole tile's far stand-in ([`Fill::canopy`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FarCanopy {
    pub tile: TileId,
    pub tile_m: f64,
    /// Texels along each side of its picture.
    pub texels: usize,
    /// Vertices along each side of its height field.
    pub grid: usize,
    /// Its vertices' heights, metres in the tile's frame (as the
    /// instances' up), row by row from the south-west corner, `grid *
    /// grid` of them.
    pub heights_m: Vec<f32>,
    /// The ground under each vertex, metres, in the same order.
    pub ground_m: Vec<f32>,
    /// The tile's own plants seen level from outside each side, for its
    /// skirts: south, north, west and east, one above the other, each
    /// `texels` wide and `side_rows` high, sRGB with cover in alpha; each
    /// seen from outside, left to right.
    pub side: Vec<[u8; 4]>,
    pub side_rows: usize,
    /// The height of the sides' pictures' top row's top, metres; a row is
    /// a texel high.
    pub side_top_m: f64,
    /// Which vertices stand on covered texels, in the same order.
    pub covered: Vec<bool>,
    /// Its picture, sRGB with the cover in alpha, row by row from the
    /// north-west corner (as an image is stored), `texels * texels` of
    /// them.
    pub colour: Vec<[u8; 4]>,
    /// Each texel's sky view, 0 to 255, in the same order.
    pub sky_view: Vec<u8>,
    pub stats: FarStats,
    pub settings: FarSettings,
}

/// The pixels high and the field of view across them (degrees) a
/// stand-in's distance is given for ([`FarCanopy::from_m`]), as the tile
/// cells' index gives theirs.
pub const REFERENCE_PIXELS: f64 = 1080.0;
pub const REFERENCE_FOV_DEG: f64 = 60.0;

impl FarCanopy {
    /// Metres a texel spans.
    #[must_use]
    pub fn texel_m(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let texels = self.texels as f64;
        self.tile_m / texels
    }

    /// The distance from which a texel is no wider than a pixel for a
    /// camera [`REFERENCE_PIXELS`] high and [`REFERENCE_FOV_DEG`] across,
    /// metres: from there on it shows all it holds.
    #[must_use]
    pub fn from_m(&self) -> f64 {
        let span = 2.0 * crate::math::tan(crate::math::radians(0.5 * REFERENCE_FOV_DEG));
        self.texel_m() * REFERENCE_PIXELS / span
    }

    /// The picture as a PNG: sRGB colour, the cover in alpha.
    ///
    /// # Errors
    ///
    /// When it cannot be encoded.
    pub fn colour_png(&self) -> Result<Vec<u8>, Error> {
        let rgba: Vec<u8> = self.colour.iter().flatten().copied().collect();
        raster::encode_png(self.texels, self.texels, &rgba).map_err(|e| Error(e.to_string()))
    }

    /// The sky view as a PNG, in red (glTF's occlusion channel), the same
    /// grey in green and blue for viewers that show the image itself.
    ///
    /// # Errors
    ///
    /// When it cannot be encoded.
    pub fn sky_view_png(&self) -> Result<Vec<u8>, Error> {
        let rgba: Vec<u8> = self.sky_view.iter().flat_map(|&v| [v, v, v, 255]).collect();
        raster::encode_png(self.texels, self.texels, &rgba).map_err(|e| Error(e.to_string()))
    }

    /// The sides' pictures as one PNG ([`Self::side`]).
    ///
    /// # Errors
    ///
    /// When it cannot be encoded.
    pub fn side_png(&self) -> Result<Vec<u8>, Error> {
        let rgba: Vec<u8> = self.side.iter().flatten().copied().collect();
        raster::encode_png(self.texels, 4 * self.side_rows, &rgba).map_err(|e| Error(e.to_string()))
    }

    /// The stand-in as one GLB: its height field as one mesh, its picture
    /// as the base colour, cut out at a half, and its sky view as the
    /// occlusion map; the root at the tile's south-west corner (+X east,
    /// +Y up, -Z north). The scene's extras give its tile, texel, grid,
    /// [`Self::from_m`] and [`FarStats`].
    ///
    /// # Errors
    ///
    /// When an image cannot be encoded.
    pub fn glb(&self) -> Result<Vec<u8>, Error> {
        let (positions, normals, uvs, indices) = self.mesh();
        let mut document = Document::new();
        let position = document.attribute(&positions, 3, true);
        let normal = document.attribute(&normals, 3, false);
        let uv = document.attribute(&uvs, 2, false);
        let index = document.indices(&indices);
        let sampler = document.sampler(json!({
            "magFilter": 9729,
            "minFilter": 9987,
            "wrapS": 33071,
            "wrapT": 33071,
        }));
        let colour = document.png("far colour", &self.colour_png()?);
        let colour = document.texture(colour, sampler);
        let sky = document.png("far sky view", &self.sky_view_png()?);
        let sky = document.texture(sky, sampler);
        let material = document.material(json!({
            "name": "far canopy",
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": colour },
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            },
            "occlusionTexture": { "index": sky },
            "alphaMode": "MASK",
            "alphaCutoff": 0.5,
        }));
        let mut primitives = Vec::new();
        let (skirt_positions, skirt_normals, skirt_uvs, skirt_indices) = self.skirts();
        if !skirt_indices.is_empty() {
            let position = document.attribute(&skirt_positions, 3, true);
            let normal = document.attribute(&skirt_normals, 3, false);
            let uv = document.attribute(&skirt_uvs, 2, false);
            let index = document.indices(&skirt_indices);
            let side = document.png("far sides", &self.side_png()?);
            let side = document.texture(side, sampler);
            let material = document.material(json!({
                "name": "far canopy sides",
                "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": side },
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0,
                },
                "alphaMode": "MASK",
                "alphaCutoff": 0.5,
            }));
            primitives.push(json!({
                "attributes": { "POSITION": position, "NORMAL": normal, "TEXCOORD_0": uv },
                "indices": index,
                "material": material,
            }));
        }
        let mesh = document.mesh(
            "far canopy",
            vec![json!({
                "attributes": {
                    "POSITION": position,
                    "NORMAL": normal,
                    "TEXCOORD_0": uv,
                },
                "indices": index,
                "material": material,
            })]
            .into_iter()
            .chain(primitives)
            .collect(),
        );
        let node = document.node(json!({
            "name": format!("far {}_{}", self.tile.x, self.tile.y),
            "mesh": mesh,
        }));
        Ok(document.glb(
            json!({
                "version": "2.0",
                "generator": format!("plantgen {}", crate::GENERATOR_REVISION),
            }),
            &[node],
            self.extras(),
        ))
    }

    /// What the scene's extras and `X_Y-far.json` say.
    #[must_use]
    pub fn extras(&self) -> Value {
        json!({
            "tile": self.tile,
            "tile_m": self.tile_m,
            "texels": self.texels,
            "texel_m": self.texel_m(),
            "grid": self.grid,
            "from_m": self.from_m(),
            "settings": self.settings,
            "stats": self.stats,
        })
    }

    /// Its height field's vertices (glTF frame), normals, texture
    /// coordinates and triangles, the bare ones left out.
    fn mesh(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<u32>) {
        let n = self.grid;
        #[allow(clippy::cast_precision_loss)]
        let step = self.tile_m / (n - 1) as f64;
        let field = Field {
            heights: &self.heights_m,
            grid: n,
            step,
        };
        let mut positions = Vec::with_capacity(n * n * 3);
        let mut normals = Vec::with_capacity(n * n * 3);
        let mut uvs = Vec::with_capacity(n * n * 2);
        for j in 0..n {
            for i in 0..n {
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (i as f64 * step, j as f64 * step);
                let up = field.height(i, j);
                let normal = field.normal(i, j);
                #[allow(clippy::cast_possible_truncation)]
                {
                    positions.extend_from_slice(&[east as f32, up as f32, -north as f32]);
                    normals.extend_from_slice(&[normal.x as f32, normal.y as f32, normal.z as f32]);
                    uvs.extend_from_slice(&[
                        (east / self.tile_m) as f32,
                        (1.0 - north / self.tile_m) as f32,
                    ]);
                }
            }
        }
        let mut indices = Vec::new();
        let at = |i: usize, j: usize| u32::try_from(j * n + i).unwrap_or(u32::MAX);
        for j in 0..n - 1 {
            for i in 0..n - 1 {
                let corners = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                // Counter-clockwise seen from above (+y): south-west,
                // south-east, north-east (north is -z).
                for triangle in [
                    [corners[0], corners[1], corners[2]],
                    [corners[0], corners[2], corners[3]],
                ] {
                    if triangle.iter().any(|&v| self.covered[v as usize]) {
                        indices.extend_from_slice(&triangle);
                    }
                }
            }
        }
        (positions, normals, uvs, indices)
    }

    /// Its skirts ([`crate::far`]): along each side, a wall from each
    /// vertex standing above the ground down to it, facing out and showing
    /// that side's picture, so a stand's edge at the tile's side reads as
    /// one from afar and no crack opens between neighbours (where the
    /// neighbour's crowns run on, the wall is hidden under them).
    /// Positions, normals, texture coordinates into [`Self::side`] and
    /// triangles.
    fn skirts(&self) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<u32>) {
        let n = self.grid;
        #[allow(clippy::cast_precision_loss)]
        let step = self.tile_m / (n - 1) as f64;
        #[allow(clippy::cast_precision_loss)]
        let rows = (4 * self.side_rows) as f64;
        let texel_m = self.texel_m();
        let (mut positions, mut normals, mut uvs, mut indices) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let tile = self.tile_m;
        // Each side: its vertices, outward, and how far along it a point
        // lies seen from outside, 0 to 1.
        let along: [fn(f64, f64, f64) -> f64; 4] = [
            |east, _, tile| east / tile,
            |east, _, tile| (tile - east) / tile,
            |_, north, tile| (tile - north) / tile,
            |_, north, tile| north / tile,
        ];
        let sides: [(Vec<(usize, usize)>, Vec3); 4] = [
            ((0..n).map(|i| (i, 0)).collect(), Vec3::new(0.0, 0.0, 1.0)),
            (
                (0..n).map(|i| (i, n - 1)).collect(),
                Vec3::new(0.0, 0.0, -1.0),
            ),
            ((0..n).map(|j| (0, j)).collect(), Vec3::new(-1.0, 0.0, 0.0)),
            (
                (0..n).map(|j| (n - 1, j)).collect(),
                Vec3::new(1.0, 0.0, 0.0),
            ),
        ];
        for (side, (run, out)) in sides.into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let first_row = (side * self.side_rows) as f64;
            let mut pairs = Vec::with_capacity(run.len());
            for &(i, j) in &run {
                let k = j * n + i;
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (i as f64 * step, j as f64 * step);
                let u = along[side](east, north, tile);
                let base = u32::try_from(positions.len() / 3).unwrap_or(u32::MAX);
                for y in [self.heights_m[k], self.ground_m[k]] {
                    let row = (self.side_top_m - f64::from(y)) / texel_m;
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        positions.extend_from_slice(&[east as f32, y, -north as f32]);
                        normals.extend_from_slice(&[out.x as f32, out.y as f32, out.z as f32]);
                        uvs.extend_from_slice(&[u as f32, ((first_row + row) / rows) as f32]);
                    }
                }
                pairs.push(base);
            }
            for s in 0..run.len() - 1 {
                let (ka, kb) = (run[s].1 * n + run[s].0, run[s + 1].1 * n + run[s + 1].0);
                let tall = |k: usize| self.heights_m[k] > self.ground_m[k] + 0.5;
                if !(tall(ka) || tall(kb)) {
                    continue;
                }
                let (ta, ba, tb, bb) = (pairs[s], pairs[s] + 1, pairs[s + 1], pairs[s + 1] + 1);
                for mut triangle in [[ba, bb, tb], [ba, tb, ta]] {
                    let p = |v: u32| {
                        let v = v as usize * 3;
                        Vec3::new(
                            f64::from(positions[v]),
                            f64::from(positions[v + 1]),
                            f64::from(positions[v + 2]),
                        )
                    };
                    let facing =
                        (p(triangle[1]) - p(triangle[0])).cross(p(triangle[2]) - p(triangle[0]));
                    if facing.dot(out) < 0.0 {
                        triangle.swap(1, 2);
                    }
                    indices.extend_from_slice(&triangle);
                }
            }
        }
        (positions, normals, uvs, indices)
    }
}

/// A plant drawn as an instance places it: its prototype's mesh turned,
/// scaled and moved to its foot, in a frame with +x east, +y up and -z
/// north in which the tile's south-west corner (at height 0) is at
/// `origin`; a climber cut back to its wall ([`WallClip::keeps`]). The
/// same transform the tile's GLB gives ([`TilePlan::glb`]).
#[must_use]
pub fn placed(plant: &PlantMesh, instance: &Instance, climber: bool, origin: Vec3) -> PlantMesh {
    let [qx, qy, qz, qw] = instance.rotation;
    // Growth's frame (y up) to the instance's (z up), turned, and back.
    let turn = |v: [f32; 3]| {
        let u = [v[0], -v[2], v[1]];
        let t = [
            2.0 * (qy * u[2] - qz * u[1]),
            2.0 * (qz * u[0] - qx * u[2]),
            2.0 * (qx * u[1] - qy * u[0]),
        ];
        let r = [
            u[0] + qw * t[0] + (qy * t[2] - qz * t[1]),
            u[1] + qw * t[1] + (qz * t[0] - qx * t[2]),
            u[2] + qw * t[2] + (qx * t[1] - qy * t[0]),
        ];
        [r[0], r[2], -r[1]]
    };
    #[allow(clippy::cast_possible_truncation)]
    let foot = [
        (origin.x + f64::from(instance.position[0])) as f32,
        (origin.y + f64::from(instance.position[2])) as f32,
        (origin.z - f64::from(instance.position[1])) as f32,
    ];
    let scale = instance.scale;
    let point = |v: [f32; 3]| {
        let t = turn(v);
        [
            t[0] * scale + foot[0],
            t[1] * scale + foot[1],
            t[2] * scale + foot[2],
        ]
    };
    let clip = if climber {
        WallClip::from_extra(instance.extra)
    } else {
        None
    };
    let mut out = plant.clone();
    if let Some(clip) = clip {
        let positions = &plant.wood.positions;
        let mut kept = Vec::with_capacity(out.wood.indices.len());
        for triangle in plant.wood.indices.as_chunks::<3>().0 {
            if triangle
                .iter()
                .all(|index| clip.keeps(positions[*index as usize]))
            {
                kept.extend_from_slice(triangle);
            }
        }
        out.wood.indices = kept;
        out.cards.retain(|card| clip.keeps(card.base));
        out.tufts.retain(|tuft| clip.keeps(tuft.position));
    }
    out.wood.positions.iter_mut().for_each(|p| *p = point(*p));
    out.wood.normals.iter_mut().for_each(|n| *n = turn(*n));
    for colour in &mut out.wood.colors {
        if colour[3] < 0.0 {
            colour[3] *= scale;
        }
    }
    for card in &mut out.cards {
        card.base = point(card.base);
        card.heading = turn(card.heading);
        card.left = turn(card.left);
        card.length *= scale;
        card.width *= scale;
    }
    for tuft in &mut out.tufts {
        tuft.position = point(tuft.position);
    }
    out.hair_tubes.clear();
    out
}

/// A plant to draw: its instance and the (x, z) of its tile's south-west
/// corner in the frame of the tile being baked.
type Placing = (Instance, [f64; 2]);

/// A height field's vertices, `grid` square and `step` metres apart, row
/// by row from the south-west corner.
struct Field<'a> {
    heights: &'a [f32],
    grid: usize,
    step: f64,
}

impl Field<'_> {
    fn height(&self, i: usize, j: usize) -> f64 {
        f64::from(self.heights[j * self.grid + i])
    }

    /// Vertex (i, j)'s normal (+x east, +y up, -z north), from central
    /// differences, one-sided at the sides.
    #[allow(clippy::many_single_char_names)]
    fn normal(&self, i: usize, j: usize) -> Vec3 {
        let n = self.grid;
        let (w, e) = (i.saturating_sub(1), (i + 1).min(n - 1));
        let (s, nn) = (j.saturating_sub(1), (j + 1).min(n - 1));
        #[allow(clippy::cast_precision_loss)]
        let dx = (self.height(e, j) - self.height(w, j)) / ((e - w).max(1) as f64 * self.step);
        #[allow(clippy::cast_precision_loss)]
        let dn = (self.height(i, nn) - self.height(i, s)) / ((nn - s).max(1) as f64 * self.step);
        Vec3::new(-dx, 1.0, dn).normalize_or(Vec3::Y)
    }
}

/// `field` (`wide` square, `NEG_INFINITY` where empty) closed over a disc
/// `radius` texels across: the greatest within the disc, then the least
/// of that. Spaces narrower than the disc fill at the height of the
/// lower side; wider ones stay empty.
fn close(field: &[f64], wide: usize, radius: usize) -> Vec<f64> {
    if radius == 0 {
        return field.to_vec();
    }
    let r = i64::try_from(radius).unwrap_or(0);
    let offsets: Vec<(i64, i64)> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|(dx, dy)| dx * dx + dy * dy <= r * r)
        .collect();
    let w = i64::try_from(wide).unwrap_or(0);
    let pass = |from: &[f64], greatest: bool| -> Vec<f64> {
        (0..wide * wide)
            .map(|k| {
                let (x, y) = (
                    i64::try_from(k % wide).unwrap_or(0),
                    i64::try_from(k / wide).unwrap_or(0),
                );
                let mut best = if greatest {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                for (dx, dy) in &offsets {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= w || sy >= w {
                        continue;
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let value = from[(sy as usize) * wide + sx as usize];
                    best = if greatest {
                        best.max(value)
                    } else {
                        best.min(value)
                    };
                }
                best
            })
            .collect()
    };
    let grown = pass(field, true);
    let closed = pass(&grown, false);
    // Closing never lowers what was there.
    closed.iter().zip(field).map(|(&c, &f)| c.max(f)).collect()
}

/// A prototype drawn at one level, with the pictures of its cards and
/// its bark: what a far stand-in draws of it.
#[derive(Debug)]
pub(crate) struct Drawn {
    mesh: PlantMesh,
    templates: Templates,
}

impl Fill {
    /// Each of `indices` drawn at `level`, grown on up to `threads`
    /// threads where not drawn before, and kept for the next tile.
    fn drawn_for_far(
        &self,
        indices: &[u32],
        level: usize,
        threads: usize,
    ) -> Result<std::collections::BTreeMap<u32, Arc<Drawn>>, Error> {
        let lock = || self.drawn.lock().unwrap_or_else(PoisonError::into_inner);
        let missing: Vec<u32> = {
            let held = lock();
            indices
                .iter()
                .copied()
                .filter(|index| !held.contains_key(&(*index, level)))
                .collect()
        };
        let made = crate::fill::run_jobs(missing.len(), threads, |job| {
            let (generator, plant) = self.grow_prototype(missing[job] as usize)?;
            let (mesh, looks) = generator.draw(&plant, level);
            let templates = Templates::for_plant(looks, &[])
                .with_bark(generator.spec().appearance.bark_params());
            Ok(Arc::new(Drawn { mesh, templates }))
        })?;
        let mut held = lock();
        for (index, drawn) in missing.iter().zip(made) {
            held.insert((*index, level), drawn);
        }
        Ok(indices
            .iter()
            .filter_map(|index| Some((*index, Arc::clone(held.get(&(*index, level))?))))
            .collect())
    }
}

/// The steepest a stand-in's surface rises between its crowns, rise
/// over run: tan 60°, so no slope stretches the top-down picture more
/// than twofold (one mip level) where it is seen from the side.
pub const STEEPEST: f64 = 1.732_050_807_568_877_2;

/// `field` (`wide` square, `NEG_INFINITY` where empty) raised where it
/// falls more steeply than `rise` per texel (along the axes; diagonals
/// √2 of it) toward a finite neighbour: each finite texel at least its
/// neighbours less the rise to them, so a pit between tall crowns fills
/// as a cone. Texels `kept` (a crown's own top) are never raised, so an
/// emergent stands over the canopy beside it instead of draping a tent
/// over it. Empty texels stay empty.
fn limit_slope(field: &[f64], kept: &[bool], wide: usize, rise: f64) -> Vec<f64> {
    let mut out = field.to_vec();
    let diagonal = rise * std::f64::consts::SQRT_2;
    let neighbours: [(i64, i64, f64); 8] = [
        (-1, 0, rise),
        (1, 0, rise),
        (0, -1, rise),
        (0, 1, rise),
        (-1, -1, diagonal),
        (1, -1, diagonal),
        (-1, 1, diagonal),
        (1, 1, diagonal),
    ];
    let w = i64::try_from(wide).unwrap_or(0);
    // Forward and back sweeps until nothing rises (two or three do).
    for _ in 0..8 {
        let mut changed = false;
        for pass in 0..2 {
            for n in 0..wide * wide {
                let k = if pass == 0 { n } else { wide * wide - 1 - n };
                if !out[k].is_finite() || kept[k] {
                    continue;
                }
                let (x, y) = (
                    i64::try_from(k % wide).unwrap_or(0),
                    i64::try_from(k / wide).unwrap_or(0),
                );
                for (dx, dy, by) in neighbours {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= w || sy >= w {
                        continue;
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let there = out[(sy as usize) * wide + sx as usize];
                    if there - by > out[k] {
                        out[k] = there - by;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    out
}

/// What a bake reads of one texel of the extended grid.
#[derive(Clone, Copy)]
struct Sample {
    /// Linear RGB summed over the covered samples.
    colour: [f64; 3],
    covered: usize,
    /// How many of them a plant of the height field covers.
    shell: usize,
    /// The highest of those samples' height.
    top: f64,
}

impl Fill {
    /// The far stand-in of a filled tile ([`crate::far`]): its plants'
    /// crowns as a height field with a picture of them from above. `site`
    /// is the tile's site (for the ground between crowns); [`Fill::tile`]
    /// made `plan` from it.
    ///
    /// # Errors
    ///
    /// When a prototype the plan places does not grow, or the settings'
    /// texel or grid is not positive.
    pub fn canopy(
        &self,
        plan: &TilePlan,
        site: &Site,
        settings: &FarSettings,
    ) -> Result<FarCanopy, Error> {
        self.canopy_among(plan, site, &[], settings)
    }

    /// [`Fill::canopy`], with the plans of neighbouring tiles (any of the
    /// eight round it) whose crowns may reach across its sides: those
    /// within reach are drawn too, so two neighbours' stand-ins agree
    /// along the side they share.
    ///
    /// # Errors
    ///
    /// As [`Fill::canopy`].
    #[allow(clippy::too_many_lines)]
    pub fn canopy_among(
        &self,
        plan: &TilePlan,
        site: &Site,
        around: &[&TilePlan],
        settings: &FarSettings,
    ) -> Result<FarCanopy, Error> {
        if !(settings.texel_m > 0.0 && settings.grid_m > 0.0) {
            return Err(Error("canopy: texel and grid must be positive".into()));
        }
        let tile_m = plan.tile_m;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let texels = ((tile_m / settings.texel_m).round() as usize).max(1);
        #[allow(clippy::cast_precision_loss)]
        let texel_m = tile_m / texels as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let grid = ((tile_m / settings.grid_m).round() as usize).max(1) + 1;
        #[allow(clippy::cast_precision_loss)]
        let step = tile_m / (grid - 1) as f64;
        // Texels past each side: half a grid step or a bridged gap, so a
        // side's vertices and gaps read the texels on both sides.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let margin = (0.5 * step.max(settings.gap_m) / texel_m).ceil() as usize + 1;
        let wide = texels + 2 * margin;
        let ss = settings.supersample.max(1);
        let pixels = wide * ss;
        #[allow(clippy::cast_precision_loss)]
        let half = 0.5 * wide as f64 * texel_m;
        let centre_x = 0.5 * tile_m;
        let centre_z = -0.5 * tile_m;

        // Every plant to draw, by prototype, with the corner of its tile.
        let mut chosen: std::collections::BTreeMap<(u32, bool), Vec<Placing>> =
            std::collections::BTreeMap::new();
        let reach_m = half + 40.0;
        for (tile_plan, corner) in
            std::iter::once((plan, [0.0, 0.0])).chain(around.iter().map(|other| {
                #[allow(clippy::cast_precision_loss)]
                let corner = [
                    f64::from(other.tile.x - plan.tile.x) * tile_m,
                    -f64::from(other.tile.y - plan.tile.y) * tile_m,
                ];
                (*other, corner)
            }))
        {
            for instance in &tile_plan.instances {
                let index = instance.prototype as usize;
                let Some(prototype) = self.prototypes().get(index) else {
                    continue;
                };
                if prototype.key.on_wall {
                    continue;
                }
                let height = self.size(index).map_or(f64::INFINITY, |size| {
                    size.height * f64::from(instance.scale)
                });
                if height < settings.least_height_m {
                    continue;
                }
                let shell = height >= settings.shell_least_m;
                let x = corner[0] + f64::from(instance.position[0]);
                let z = corner[1] - f64::from(instance.position[1]);
                if (x - centre_x).abs() > reach_m || (z - centre_z).abs() > reach_m {
                    continue;
                }
                chosen
                    .entry((instance.prototype, shell))
                    .or_default()
                    .push((*instance, corner));
            }
        }

        // Each prototype's pass, merged by depth into the samples.
        let top_y = 1.0e4;
        let camera = Camera {
            eye: Vec3::new(centre_x, top_y, centre_z),
            target: Vec3::new(centre_x, 0.0, centre_z),
            up: Vec3::new(0.0, 0.0, -1.0),
            projection: Projection::Orthographic { half_height: half },
        };
        let options = RenderOptions {
            width: pixels,
            height: pixels,
            supersample: 1,
            shadows: false,
            shadow_bounds: None,
            shadow_texels: raster::SHADOW_TEXELS,
        };
        let mut depth = vec![f32::INFINITY; pixels * pixels];
        let mut colour = vec![[0.0_f32; 3]; pixels * pixels];
        // The tops of the plants that make the height field.
        let mut shell_depth = vec![f32::INFINITY; pixels * pixels];
        let mut plants = 0;
        let mut wanted: Vec<u32> = chosen.keys().map(|(index, _)| *index).collect();
        wanted.dedup();
        let drawn = self.drawn_for_far(&wanted, settings.level, settings.threads)?;

        // The four sides seen from outside, level and orthographic, for
        // the skirts: south, north, west, east, each over the tile's side
        // and from the lowest foot to the highest top of its own plants.
        let (mut low_y, mut high_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for ((index, _), instances) in &chosen {
            let height = self.size(*index as usize).map_or(0.0, |size| size.height);
            for (instance, corner) in instances {
                if *corner == [0.0, 0.0] {
                    let foot = f64::from(instance.position[2]);
                    low_y = low_y.min(foot);
                    high_y = high_y.max(foot + height * f64::from(instance.scale));
                }
            }
        }
        if !low_y.is_finite() {
            (low_y, high_y) = (0.0, 1.0);
        }
        let (low_y, high_y) = (low_y - texel_m, high_y + texel_m);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let side_rows = (((high_y - low_y) / texel_m).ceil() as usize).max(1);
        #[allow(clippy::cast_precision_loss)]
        let side_top = low_y + side_rows as f64 * texel_m;
        let side_mid = f64::midpoint(low_y, side_top);
        #[allow(clippy::cast_precision_loss)]
        let side_half = 0.5 * side_rows as f64 * texel_m;
        let away = 1.0e4;
        let side_cameras: Vec<Camera> = [
            (
                Vec3::new(centre_x, side_mid, away),
                Vec3::new(centre_x, side_mid, centre_z),
            ),
            (
                Vec3::new(centre_x, side_mid, -away),
                Vec3::new(centre_x, side_mid, centre_z),
            ),
            (
                Vec3::new(-away, side_mid, centre_z),
                Vec3::new(centre_x, side_mid, centre_z),
            ),
            (
                Vec3::new(away, side_mid, centre_z),
                Vec3::new(centre_x, side_mid, centre_z),
            ),
        ]
        .into_iter()
        .map(|(eye, target)| Camera {
            eye,
            target,
            up: Vec3::Y,
            projection: Projection::Orthographic {
                half_height: side_half,
            },
        })
        .collect();
        let side_options = RenderOptions {
            width: texels * ss,
            height: side_rows * ss,
            ..options
        };
        let side_pixels = texels * ss * side_rows * ss;
        let mut side_depth = vec![vec![f32::INFINITY; side_pixels]; 4];
        let mut side_colour = vec![vec![[0.0_f32; 3]; side_pixels]; 4];

        for (&(index, shell), instances) in &chosen {
            let Some(one) = drawn.get(&index) else {
                continue;
            };
            let (drawn, templates) = (&one.mesh, &one.templates);
            let wood_material = templates.bark.map_or(Material::Opaque, Material::Bark);
            let mut scene = PlantMesh::default();
            let mut own = PlantMesh::default();
            for (instance, corner) in instances {
                let one = placed(drawn, instance, false, Vec3::new(corner[0], 0.0, corner[1]));
                append(&mut scene, &one);
                if *corner == [0.0, 0.0] {
                    plants += 1;
                    if !around.is_empty() {
                        append(&mut own, &one);
                    }
                }
            }
            let cards = scene.card_mesh();
            let items: [(&Mesh, Material); 2] =
                [(&scene.wood, wood_material), (&cards, Material::Card)];
            let image = raster::render(&items, &camera, &Lighting::flat(), templates, &options);
            for (k, &d) in image.depth.iter().enumerate() {
                if shell && image.color[k][3] > 0.0 && d < shell_depth[k] {
                    shell_depth[k] = d;
                }
                if image.color[k][3] > 0.0 && d < depth[k] {
                    depth[k] = d;
                    let c = image.color[k];
                    colour[k] = [c[0], c[1], c[2]];
                }
            }
            let own_cards = if around.is_empty() {
                None
            } else {
                Some(own.card_mesh())
            };
            let side_items: [(&Mesh, Material); 2] = match &own_cards {
                Some(cards) => [(&own.wood, wood_material), (cards, Material::Card)],
                None => items,
            };
            for (side, camera) in side_cameras.iter().enumerate() {
                let image = raster::render(
                    &side_items,
                    camera,
                    &Lighting::flat(),
                    templates,
                    &side_options,
                );
                for (k, &d) in image.depth.iter().enumerate() {
                    if image.color[k][3] > 0.0 && d < side_depth[side][k] {
                        side_depth[side][k] = d;
                        let c = image.color[k];
                        side_colour[side][k] = [c[0], c[1], c[2]];
                    }
                }
            }
        }
        // The sides' pictures, one above the other.
        let mut side_rgba = Vec::with_capacity(4 * texels * side_rows * 4);
        for side in 0..4 {
            for y in 0..side_rows {
                for x in 0..texels {
                    let (mut sum, mut count) = ([0.0_f64; 3], 0_usize);
                    for sy in 0..ss {
                        for sx in 0..ss {
                            let k = (y * ss + sy) * texels * ss + x * ss + sx;
                            if side_depth[side][k].is_finite() {
                                for (total, c) in sum.iter_mut().zip(side_colour[side][k]) {
                                    *total += f64::from(c);
                                }
                                count += 1;
                            }
                        }
                    }
                    #[allow(clippy::cast_precision_loss)]
                    for total in sum {
                        let mean = if count > 0 { total / count as f64 } else { 0.0 };
                        #[allow(clippy::cast_possible_truncation)]
                        side_rgba.push(raster::to_u8(raster::srgb(mean as f32)));
                    }
                    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                    side_rgba.push(raster::to_u8((count as f64 / (ss * ss) as f64) as f32));
                }
            }
        }
        impostor::preserve_coverage(&mut side_rgba);
        let side_kept: Vec<bool> = side_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|t| t[3] >= impostor::KEPT_ALPHA)
            .collect();
        let mut side_out: Vec<[u8; 4]> = side_rgba.as_chunks::<4>().0.to_vec();
        // Filled outward as the top's picture is (the nearest search
        // works on a square, so pad the atlas to one).
        let square = texels.max(4 * side_rows);
        let mut padded = vec![false; square * square];
        for (k, &kept) in side_kept.iter().enumerate() {
            padded[(k / texels) * square + k % texels] = kept;
        }
        if let Some(nearest) = impostor::nearest_covered(&padded, square) {
            for k in 0..side_out.len() {
                let source = nearest[(k / texels) * square + k % texels];
                let (sx, sy) = (source % square, source / square);
                if sx < texels && sy < 4 * side_rows {
                    let [r, g, b, _] = side_out[sy * texels + sx];
                    side_out[k] = [r, g, b, side_out[k][3]];
                }
            }
        }

        // The extended grid's texels.
        let mut samples = vec![
            Sample {
                colour: [0.0; 3],
                covered: 0,
                shell: 0,
                top: f64::NEG_INFINITY,
            };
            wide * wide
        ];
        for py in 0..pixels {
            for px in 0..pixels {
                let k = py * pixels + px;
                if !depth[k].is_finite() {
                    continue;
                }
                let sample = &mut samples[(py / ss) * wide + px / ss];
                for (sum, c) in sample.colour.iter_mut().zip(colour[k]) {
                    *sum += f64::from(c);
                }
                sample.covered += 1;
                if shell_depth[k].is_finite() {
                    sample.shell += 1;
                    sample.top = sample.top.max(top_y - f64::from(shell_depth[k]));
                }
            }
        }
        // A texel's (east, north) centre in the tile.
        #[allow(clippy::cast_precision_loss)]
        let texel_at = |x: usize, y: usize| {
            (
                (x as f64 - margin as f64 + 0.5) * texel_m,
                tile_m - (y as f64 - margin as f64 + 0.5) * texel_m,
            )
        };
        let ground: Vec<f64> = (0..wide * wide)
            .map(|k| {
                let (east, north) = texel_at(k % wide, k / wide);
                site.height_at(east, north)
            })
            .collect();
        // Crown tops where a texel is covered.
        let tops: Vec<f64> = samples
            .iter()
            .zip(&ground)
            .map(|(sample, &ground)| {
                if 2 * sample.shell >= ss * ss {
                    sample.top.max(ground)
                } else {
                    f64::NEG_INFINITY
                }
            })
            .collect();
        // The canopy's surface: the tops, the spaces between crowns
        // narrower than a gap bridged. An opening reaches the ground where
        // it is wider than the canopy round it is tall (Canham et al.
        // 1990, Canadian Journal of Forest Research 20: 620-631: gaps of a
        // diameter under their canopy's height let little sun reach their
        // floor, and look into shade), and at least `gap_m` across.
        let (mut rise, mut counted) = (0.0, 0.0);
        for (top, ground) in tops.iter().zip(&ground) {
            if top.is_finite() {
                rise += top - ground;
                counted += 1.0;
            }
        }
        let canopy_m = if counted > 0.0 { rise / counted } else { 0.0 };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let radius = (0.5 * settings.gap_m.max(canopy_m) / texel_m).round() as usize;
        // Where the canopy stands: the crowns' cover with every opening
        // narrower than that closed. There the surface starts at the tops,
        // or at the ground in a bridged space, and rises no more steeply
        // than STEEPEST from it, so a bridged space is a shaded pit.
        let mask: Vec<f64> = tops
            .iter()
            .map(|top| {
                if top.is_finite() {
                    0.0
                } else {
                    f64::NEG_INFINITY
                }
            })
            .collect();
        let mask = close(&mask, wide, radius);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let slot = (0.5 * settings.gap_m / texel_m).round() as usize;
        let start: Vec<f64> = close(&tops, wide, slot)
            .iter()
            .zip(&mask)
            .zip(&ground)
            .map(|((&top, &inside), &ground)| {
                if top.is_finite() {
                    top
                } else if inside.is_finite() {
                    ground
                } else {
                    f64::NEG_INFINITY
                }
            })
            .collect();
        let kept: Vec<bool> = tops.iter().map(|top| top.is_finite()).collect();
        let canopy = limit_slope(&start, &kept, wide, texel_m * STEEPEST);
        let surface: Vec<f64> = canopy
            .iter()
            .zip(&ground)
            .map(|(&top, &ground)| if top.is_finite() { top } else { ground })
            .collect();
        let sky = sky_view(&surface, wide, texel_m);
        // What a bridged space between crowns lets the sky reach: the
        // surface with the space open down to what stands in it.
        let open: Vec<f64> = (0..wide * wide)
            .map(|k| {
                if tops[k].is_finite() {
                    tops[k]
                } else {
                    ground[k]
                }
            })
            .collect();
        let sky_open = sky_view(&open, wide, texel_m);

        // The height field's vertices: the highest point of the canopy
        // within half a step, else the ground.
        let mut heights = Vec::with_capacity(grid * grid);
        let mut covered = Vec::with_capacity(grid * grid);
        let mut grounds = Vec::with_capacity(grid * grid);
        #[allow(clippy::cast_precision_loss)]
        let margin_m = margin as f64 * texel_m;
        let first = |at: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = ((at + margin_m - 0.5 * step) / texel_m - 0.5)
                .ceil()
                .max(0.0) as usize;
            v
        };
        let last = |at: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = ((at + margin_m + 0.5 * step) / texel_m - 0.5)
                .floor()
                .max(0.0) as usize;
            v.min(wide - 1)
        };
        for j in 0..grid {
            for i in 0..grid {
                #[allow(clippy::cast_precision_loss)]
                let (east, north) = (i as f64 * step, j as f64 * step);
                let from_top = tile_m - north;
                let mut top = f64::NEG_INFINITY;
                for y in first(from_top)..=last(from_top) {
                    for x in first(east)..=last(east) {
                        top = top.max(canopy[y * wide + x]);
                    }
                }
                let is_covered = top.is_finite();
                #[allow(clippy::cast_possible_truncation)]
                let below = site.height_at(east, north);
                #[allow(clippy::cast_possible_truncation)]
                {
                    heights.push(if is_covered { top } else { below } as f32);
                    grounds.push(below as f32);
                }
                covered.push(is_covered);
            }
        }

        // The picture: the tile's own texels.
        #[allow(clippy::cast_precision_loss)]
        let per_texel = (ss * ss) as f64;
        let mut rgba = Vec::with_capacity(texels * texels * 4);
        for y in 0..texels {
            for x in 0..texels {
                let sample = &samples[(y + margin) * wide + x + margin];
                #[allow(clippy::cast_precision_loss)]
                let mean = if sample.covered > 0 {
                    sample.colour.map(|c| c / sample.covered as f64)
                } else {
                    [0.0; 3]
                };
                for c in mean {
                    #[allow(clippy::cast_possible_truncation)]
                    rgba.push(raster::to_u8(raster::srgb(c as f32)));
                }
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                rgba.push(raster::to_u8((sample.covered as f64 / per_texel) as f32));
            }
        }
        impostor::preserve_coverage(&mut rgba);
        // Empty texels take their nearest covered one's colour,
        // so filtering and mipmaps never blend black into the crowns.
        let kept: Vec<bool> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|t| t[3] >= impostor::KEPT_ALPHA)
            .collect();
        let mut colour_out: Vec<[u8; 4]> = rgba.as_chunks::<4>().0.to_vec();
        if let Some(nearest) = impostor::nearest_covered(&kept, texels) {
            for (k, &source) in nearest.iter().enumerate() {
                if source != k {
                    let [r, g, b, _] = colour_out[source];
                    colour_out[k] = [r, g, b, colour_out[k][3]];
                }
            }
        }
        // A bridged space is part of the surface: opaque, showing what
        // stands in it (or its nearest crown's side), as dark as the sky
        // it sees down there lets it be; the height field's own sky view
        // no longer finds it in a pit.
        for y in 0..texels {
            for x in 0..texels {
                let k = (y + margin) * wide + x + margin;
                // Raised over what stands there by more than a texel.
                if !canopy[k].is_finite() || canopy[k] - tops[k] <= texel_m {
                    continue;
                }
                let texel = &mut colour_out[y * texels + x];
                #[allow(clippy::cast_possible_truncation)]
                let dim = (sky_open[k] / sky[k].max(1e-3)).min(1.0);
                for c in &mut texel[..3] {
                    *c = crate::texture::linear_to_srgb(crate::texture::srgb_to_linear(*c) * dim);
                }
                texel[3] = 255;
            }
        }
        // A stand's side, where it drops to an opening over one step of
        // the height field, is opaque down to the ground: the picture's
        // crowns reach a step past their edge.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let band = (step / texel_m).ceil() as i64;
        let wide_i = i64::try_from(wide).unwrap_or(0);
        for y in 0..texels {
            for x in 0..texels {
                let k = (y + margin) * wide + x + margin;
                if canopy[k].is_finite() || colour_out[y * texels + x][3] >= impostor::KEPT_ALPHA {
                    continue;
                }
                let (cx, cy) = (
                    i64::try_from(x + margin).unwrap_or(0),
                    i64::try_from(y + margin).unwrap_or(0),
                );
                let near = (-band..=band).any(|dy| {
                    (-band..=band).any(|dx| {
                        let (sx, sy) = (cx + dx, cy + dy);
                        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                        let inside = sx >= 0 && sy >= 0 && sx < wide_i && sy < wide_i;
                        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                        let finite =
                            inside && canopy[(sy as usize) * wide + sx as usize].is_finite();
                        finite
                    })
                });
                if near {
                    colour_out[y * texels + x][3] = 255;
                }
            }
        }
        let mut sky_out = Vec::with_capacity(texels * texels);
        for y in 0..texels {
            for x in 0..texels {
                #[allow(clippy::cast_possible_truncation)]
                sky_out.push(raster::to_u8(sky[(y + margin) * wide + x + margin] as f32));
            }
        }

        // Its numbers.
        let mut stats = FarStats {
            plants,
            ..FarStats::default()
        };
        let (mut count, mut colour_sum, mut height_sum, mut sky_sum) = (0.0, [0.0; 3], 0.0, 0.0);
        for y in 0..texels {
            for x in 0..texels {
                if colour_out[y * texels + x][3] < impostor::KEPT_ALPHA {
                    continue;
                }
                let k = (y + margin) * wide + x + margin;
                let sample = &samples[k];
                count += 1.0;
                #[allow(clippy::cast_precision_loss)]
                for (sum, c) in colour_sum.iter_mut().zip(sample.colour) {
                    *sum += c / sample.covered.max(1) as f64;
                }
                if sample.shell > 0 {
                    height_sum += sample.top - ground[k];
                }
                sky_sum += sky[k];
            }
        }
        if count > 0.0 {
            #[allow(clippy::cast_precision_loss)]
            let all = (texels * texels) as f64;
            stats.cover = count / all;
            stats.colour = colour_sum.map(|c| c / count);
            stats.height_m = height_sum / count;
            stats.sky_view = sky_sum / count;
        }
        let mut far = FarCanopy {
            tile: plan.tile,
            tile_m,
            texels,
            grid,
            heights_m: heights,
            ground_m: grounds,
            side: side_out,
            side_rows,
            side_top_m: side_top,
            covered,
            colour: colour_out,
            sky_view: sky_out,
            stats,
            settings: *settings,
        };
        far.stats.triangles = (far.mesh().3.len() + far.skirts().3.len()) / 3;
        Ok(far)
    }
}

/// `plant`'s meshes added to `scene`'s.
fn append(scene: &mut PlantMesh, plant: &PlantMesh) {
    let base = u32::try_from(scene.wood.positions.len()).unwrap_or(u32::MAX);
    scene
        .wood
        .positions
        .extend_from_slice(&plant.wood.positions);
    scene.wood.normals.extend_from_slice(&plant.wood.normals);
    scene.wood.uvs.extend_from_slice(&plant.wood.uvs);
    scene.wood.colors.extend_from_slice(&plant.wood.colors);
    scene.wood.births.extend_from_slice(&plant.wood.births);
    scene.wood.sheds.extend_from_slice(&plant.wood.sheds);
    scene.wood.levels.extend_from_slice(&plant.wood.levels);
    scene
        .wood
        .indices
        .extend(plant.wood.indices.iter().map(|i| i + base));
    scene.cards.extend_from_slice(&plant.cards);
}

/// The most metres the sky view looks along each direction.
pub const SKY_REACH_M: f64 = 20.0;

/// Each texel's sky view over the height field `surface` (`wide` square,
/// `texel_m` apart): the share of an overcast sky's light on a level
/// surface there that its surroundings leave, the mean over eight
/// directions of `cos² h`, `h` the horizon's elevation that way (the
/// view factor of a sky above a horizon of that height), looking up to
/// [`SKY_REACH_M`].
#[must_use]
pub fn sky_view(surface: &[f64], wide: usize, texel_m: f64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = ((SKY_REACH_M / texel_m).ceil() as usize).max(1);
    let directions: [(i64, i64); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    let wide_i = i64::try_from(wide).unwrap_or(i64::MAX);
    (0..wide * wide)
        .map(|k| {
            let (x, y) = (
                i64::try_from(k % wide).unwrap_or(0),
                i64::try_from(k / wide).unwrap_or(0),
            );
            let here = surface[k];
            let mut sum = 0.0;
            for (dx, dy) in directions {
                #[allow(clippy::cast_precision_loss)]
                let per = texel_m * ((dx * dx + dy * dy) as f64).sqrt();
                let mut tangent: f64 = 0.0;
                for s in 1..=i64::try_from(steps).unwrap_or(1) {
                    let (sx, sy) = (x + dx * s, y + dy * s);
                    if sx < 0 || sy < 0 || sx >= wide_i || sy >= wide_i {
                        break;
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let there = surface[(sy as usize) * wide + sx as usize];
                    #[allow(clippy::cast_precision_loss)]
                    let rise = (there - here) / (per * s as f64);
                    tangent = tangent.max(rise);
                }
                // cos² of the horizon's elevation: 1 / (1 + tan²).
                sum += 1.0 / (1.0 + tangent * tangent);
            }
            sum / 8.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_surface_sees_the_whole_sky_and_a_pit_less() {
        let wide = 21;
        let flat = vec![0.0; wide * wide];
        assert!(
            sky_view(&flat, wide, 1.0)
                .iter()
                .all(|&v| (v - 1.0).abs() < 1e-12)
        );
        // A texel 10 m down among a level 0: its horizon rises 45° at one
        // texel away along the axes, so it sees little.
        let mut pit = flat;
        pit[10 * wide + 10] = -10.0;
        let sky = sky_view(&pit, wide, 1.0);
        assert!(sky[10 * wide + 10] < 0.1, "{}", sky[10 * wide + 10]);
        assert!((sky[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn an_emergent_stands_over_the_crowns_beside_it_and_a_pit_fills() {
        // A row: a 60 m emergent, low crowns at 10 m, a bridged pit at
        // the ground, low crowns again.
        let wide = 9;
        let mut field = vec![10.0; wide * wide];
        let mut kept = vec![true; wide * wide];
        for y in 0..wide {
            field[y * wide] = 60.0;
            field[y * wide + 6] = 0.0;
            kept[y * wide + 6] = false;
        }
        let out = limit_slope(&field, &kept, wide, 2.0);
        // The crowns keep their own tops beside the emergent.
        assert!((out[4 * wide + 1] - 10.0).abs() < 1e-12);
        // The pit rises to its neighbours less the rise to them.
        assert!((out[4 * wide + 6] - 8.0).abs() < 1e-12);
    }
}
