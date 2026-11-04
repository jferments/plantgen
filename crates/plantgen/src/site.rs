//! Sites: plants grown on a mesh someone hands over (sites plan S4,
//! "grow here").
//!
//! A [`SiteMesh`] is a triangle mesh with a material per triangle: ground,
//! rocks, logs, walls, poles, from any modelling tool (an OBJ file,
//! [`SiteMesh::from_obj`]). For each plant set on it, `PlantGen` cuts what
//! that plant grows in from the mesh, in the plant's own frame (its foot at
//! the origin, +Y up):
//!
//! - the substrate ([`SiteMesh::substrate_round`]): an ordinary
//!   `substrate` grid of the conditions document (`crate::substrate`),
//!   signed distances and materials, so roots, sprawling stems and
//!   `substrate@1` meet the mesh as they meet a preset;
//! - the surfaces a climber can hold to ([`SiteMesh::host_round`]): its
//!   rock, bark, wood and built triangles as a `host@1` host
//!   (`crate::lsys::tools::Host`), so a vine climbs the site's wall or pole
//!   in place of the stock host tree its species names.
//!
//! Nothing here is After's: the mesh and its material table are plain
//! numbers, and what comes out is the schema 1 conditions document that
//! growth already reads.
//!
//! The mesh should be closed solids, or a ground surface open to the sky,
//! wound counter-clockwise seen from outside (OBJ's and glTF's rule).
//! Parts too thin to model as solids, a fence's wires, a trellis's slats,
//! a sheet of mesh, are faces of their own [`Surface`] with a thickness
//! (`thin_m`): they need not be closed, and each stands as a slab that
//! thick round its face. A surface a climber must not hold to (a window
//! kept clear, a painted gutter) is still solid but not climbable
//! (`climb` false).
//! Within a narrow band of a surface the sign of the distance comes from
//! the nearest surface's angle-weighted pseudo-normal (Bærentzen and Aanæs,
//! 2005); farther off, each column of the grid carries the sign of the
//! nearest surface above it, and a column's top starts inside when a
//! vertical ray from the sky crosses the mesh an odd number of times.

use std::collections::HashMap;

use crate::graph::PlantGraph;
use crate::lsys::tools::{Host, closest_on_triangle};
use crate::math::Vec3;
use crate::pavement::Pavement;
use crate::substrate::{
    AIR, BARK, BUILT, MAX_SUBSTRATE_CELLS, ROCK, SOIL, SUBSTRATE_VERSION, Substrate, WOOD,
    base64_encode,
};

/// Corners closer than this are one vertex, metres.
const WELD_M: f64 = 1.0e-4;

/// Edge of a cell of the vertical-ray index, metres.
const COLUMN_CELL: f64 = 1.0;

/// Most triangles a site mesh may hold.
pub const MAX_TRIANGLES: usize = 4_000_000;

/// What one face of a site is: its substrate material, whether it is a
/// thin part and how thick, whether a climber may hold to it, and the
/// colour it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    /// A substrate solid, [`SOIL`] to [`BUILT`].
    pub material: u8,
    /// 0 for a face of a closed solid; above 0, a thin part's thickness in
    /// metres (a wire's, a slat's), standing as a slab round the face.
    pub thin_m: f64,
    /// Whether a climber may hold to it (when its material is
    /// [`climbable`]).
    pub climb: bool,
    /// Whether it is kept clear of plants, as a gardener keeps a window or
    /// a door clear: nothing holds to it, and what grows onto it is cut
    /// back ([`SiteMesh::cut_back`]).
    pub clear: bool,
    /// Linear RGB to draw it in; the material's colour when `None`.
    pub colour: Option<[f32; 3]>,
    /// Depth of soil lying on it where it faces up, metres: what has
    /// gathered on a ledge, in a gutter or on a decayed roof, as the host
    /// states it. 0 for a clean surface. The site field
    /// ([`crate::site_field`]) gives it to the patches it covers.
    pub soil_m: f64,
}

impl Surface {
    /// A face of a closed solid of `material` that climbers may hold to.
    #[must_use]
    pub const fn solid(material: u8) -> Self {
        Self {
            material,
            thin_m: 0.0,
            climb: true,
            clear: false,
            colour: None,
            soil_m: 0.0,
        }
    }

    pub(crate) fn thin(&self) -> bool {
        self.thin_m > 0.0
    }
}

/// The reaches and depths, metres, a site's [`SiteMesh::rooting`] table
/// gives the rootable soil at.
pub const ROOTING_REACHES: [f64; 9] = [0.02, 0.05, 0.1, 0.2, 0.4, 0.8, 1.6, 3.2, 6.4];

/// A site's mesh: triangles with a substrate material each.
#[derive(Debug, Clone)]
pub struct SiteMesh {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    materials: Vec<u8>,
    /// Each triangle's surface.
    surfaces: Vec<Surface>,
    /// Each triangle's unit normal.
    normals: Vec<Vec3>,
    /// Each vertex's angle-weighted pseudo-normal.
    vertex_normals: Vec<Vec3>,
    /// Each edge's pseudo-normal (the sum of its faces' normals), by its
    /// vertices, the lower first.
    edge_normals: HashMap<(u32, u32), Vec3>,
    /// Triangles by the column cells (X, Z) their footprint covers.
    columns: HashMap<(i32, i32), Vec<u32>>,
    /// The same, thin parts' triangles included.
    columns_all: HashMap<(i32, i32), Vec<u32>>,
    /// Paved tiles with their cracks ([`Self::add_pavement`]), which
    /// stand in for the mesh at and below their surface.
    pavements: Vec<Pavement>,
}

/// The box round a plant that its substrate grid covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    /// Metres either side of the foot, east-west and north-south.
    pub half_m: f64,
    /// Metres below the foot.
    pub below_m: f64,
    /// Metres above the foot.
    pub above_m: f64,
    /// Edge of a cell, metres.
    pub cell_m: f64,
}

impl Cut {
    /// The box for a plant `height_m` tall that spreads `reach_m` from its
    /// foot: a margin round both, and cells fine enough for its stems
    /// (2.5 cm, as the presets', for a herb; coarser for a tree, so the
    /// grid stays near a million cells).
    #[must_use]
    pub fn for_plant(height_m: f64, reach_m: f64) -> Self {
        let half_m = (reach_m * 1.3 + 0.3).max(1.0);
        let above_m = (height_m * 1.2 + 0.3).max(1.0);
        let below_m = (height_m * 0.25).clamp(0.4, 3.0);
        let cell_m = (half_m / 48.0).clamp(0.025, 0.5);
        Self {
            half_m,
            below_m,
            above_m,
            cell_m,
        }
    }
}

impl SiteMesh {
    /// The mesh of `positions`, `triangles` (three indices each, counter-
    /// clockwise seen from outside) and a material per triangle ([`SOIL`]
    /// to [`BUILT`]). Corners within 0.1 mm are joined and triangles with
    /// no area dropped.
    ///
    /// # Errors
    ///
    /// On an index past the positions, a material that is not one of the
    /// substrate's solids, a position not finite, or no triangles left.
    pub fn new(
        positions: &[Vec3],
        triangles: &[[u32; 3]],
        materials: &[u8],
    ) -> Result<Self, String> {
        let surfaces: Vec<Surface> = materials.iter().map(|m| Surface::solid(*m)).collect();
        Self::with_surfaces(positions, triangles, &surfaces)
    }

    /// The mesh of `positions`, `triangles` and a [`Surface`] per triangle,
    /// as [`Self::new`]. Thin parts' faces do not count toward the closed
    /// solids' normals or the rays that tell inside from outside.
    ///
    /// # Errors
    ///
    /// As [`Self::new`], and on a thickness that is not finite or below 0.
    #[allow(clippy::too_many_lines)]
    pub fn with_surfaces(
        positions: &[Vec3],
        triangles: &[[u32; 3]],
        surfaces: &[Surface],
    ) -> Result<Self, String> {
        if triangles.len() != surfaces.len() {
            return Err(format!(
                "a site mesh of {} triangles has {} materials",
                triangles.len(),
                surfaces.len()
            ));
        }
        if let Some(bad) = surfaces
            .iter()
            .find(|s| !s.thin_m.is_finite() || s.thin_m < 0.0)
        {
            return Err(format!("a site part's thickness is {} m", bad.thin_m));
        }
        let materials: Vec<u8> = surfaces.iter().map(|s| s.material).collect();
        if triangles.len() > MAX_TRIANGLES {
            return Err(format!(
                "a site mesh has {} triangles; the limit is {MAX_TRIANGLES}",
                triangles.len()
            ));
        }
        if let Some(bad) = positions.iter().find(|p| !p.is_finite()) {
            return Err(format!("a site mesh has a corner at {bad:?}"));
        }
        if let Some(bad) = materials.iter().find(|m| !(SOIL..=BUILT).contains(*m)) {
            return Err(format!("site material {bad} is not one of 1 to 5"));
        }
        // Weld corners by position, so faces that meet share an edge.
        let mut welded: Vec<Vec3> = Vec::new();
        let mut by_key: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut remap = Vec::with_capacity(positions.len());
        for p in positions {
            #[allow(clippy::cast_possible_truncation)]
            let key = (
                (p.x / WELD_M).round() as i64,
                (p.y / WELD_M).round() as i64,
                (p.z / WELD_M).round() as i64,
            );
            let index = *by_key.entry(key).or_insert_with(|| {
                welded.push(*p);
                u32::try_from(welded.len() - 1).unwrap_or(u32::MAX)
            });
            remap.push(index);
        }
        let mut kept = Vec::with_capacity(triangles.len());
        let mut kept_materials = Vec::with_capacity(triangles.len());
        let mut kept_surfaces = Vec::with_capacity(triangles.len());
        let mut normals = Vec::with_capacity(triangles.len());
        for ((corners, material), surface) in triangles.iter().zip(&materials).zip(surfaces) {
            let mut new = [0_u32; 3];
            for (slot, &index) in new.iter_mut().zip(corners) {
                *slot = *remap.get(index as usize).ok_or_else(|| {
                    format!(
                        "a site triangle names corner {index} of {}",
                        positions.len()
                    )
                })?;
            }
            let [a, b, c] = new.map(|i| welded[i as usize]);
            let cross = (b - a).cross(c - a);
            if new[0] == new[1] || new[1] == new[2] || new[0] == new[2] || cross.length() < 1.0e-12
            {
                continue;
            }
            kept.push(new);
            kept_materials.push(*material);
            kept_surfaces.push(*surface);
            normals.push(cross.normalize_or(Vec3::Y));
        }
        if kept.is_empty() {
            return Err("a site mesh needs at least one triangle with an area".into());
        }
        let mut vertex_normals = vec![Vec3::ZERO; welded.len()];
        let mut edge_normals: HashMap<(u32, u32), Vec3> = HashMap::new();
        let mut columns: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut columns_all: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, corners) in kept.iter().enumerate() {
            let [a, b, c] = corners.map(|i| welded[i as usize]);
            let (low, high) = (a.min(b).min(c), a.max(b).max(c));
            let (low, high) = (column_of(low.x, low.z), column_of(high.x, high.z));
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    #[allow(clippy::cast_possible_truncation)]
                    columns_all.entry((x, z)).or_default().push(index as u32);
                }
            }
        }
        for (index, (corners, normal)) in kept.iter().zip(&normals).enumerate() {
            if kept_surfaces[index].thin() {
                continue;
            }
            for k in 0..3 {
                let (here, next, last) = (corners[k], corners[(k + 1) % 3], corners[(k + 2) % 3]);
                let p = welded[here as usize];
                let angle = (welded[next as usize] - p)
                    .normalize_or(Vec3::X)
                    .dot((welded[last as usize] - p).normalize_or(Vec3::X))
                    .clamp(-1.0, 1.0);
                vertex_normals[here as usize] += *normal * crate::math::acos(angle);
                let edge = (here.min(next), here.max(next));
                let sum = edge_normals.entry(edge).or_insert(Vec3::ZERO);
                *sum += *normal;
            }
            let [a, b, c] = corners.map(|i| welded[i as usize]);
            let (low, high) = (a.min(b).min(c), a.max(b).max(c));
            let (low, high) = (column_of(low.x, low.z), column_of(high.x, high.z));
            for x in low.0..=high.0 {
                for z in low.1..=high.1 {
                    #[allow(clippy::cast_possible_truncation)]
                    columns.entry((x, z)).or_default().push(index as u32);
                }
            }
        }
        Ok(Self {
            positions: welded,
            triangles: kept,
            materials: kept_materials,
            surfaces: kept_surfaces,
            normals,
            vertex_normals,
            edge_normals,
            columns,
            columns_all,
            pavements: Vec::new(),
        })
    }

    /// Lays a paved tile on the site. Over its rectangle, at and below its
    /// surface, it stands in for the mesh: its slab, base and the soil
    /// under them and in its cracks are what a plant there grows in, and a
    /// plant set on it roots in a crack or not at all. The mesh still
    /// counts above the surface (a curb, a wall, a post standing on it).
    pub fn add_pavement(&mut self, pavement: Pavement) {
        self.pavements.push(pavement);
    }

    /// The paved tiles laid on the site.
    #[must_use]
    pub fn pavements(&self) -> &[Pavement] {
        &self.pavements
    }

    /// The soil a plant at `foot` can root in, as a [`Rooting`] table
    /// (soil depth design D1), when it stands on a paved tile; `None`
    /// elsewhere, where the soil is as deep as roots go. Over a tile the
    /// ground counts as [`Pavement::column`] gives it, off it the ground's
    /// soil counts whole down from its surface, and anything else nothing.
    ///
    /// [`Rooting`]: crate::conditions::Rooting
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn rooting(&self, foot: Vec3) -> Option<crate::conditions::Rooting> {
        // The tile the foot stands in: in a crack or hole, below its top.
        let tile = self
            .pavements
            .iter()
            .find(|tile| tile.covers(foot.x, foot.z) && foot.y <= tile.top() + 1.0e-6)?;
        let radii = ROOTING_REACHES.to_vec();
        let depths = ROOTING_REACHES.to_vec();
        let mut ring = vec![0.0; radii.len() * depths.len()];
        let mut add = |x: f64, z: f64, area: f64| {
            let distance = (x - foot.x).hypot(z - foot.z);
            let Some(r) = radii.iter().position(|r| distance <= *r) else {
                return;
            };
            let column = self
                .pavements
                .iter()
                .find_map(|t| t.covers(x, z).then(|| t.column(x, z)).flatten());
            let column = column.unwrap_or_else(|| match self.ground_at(x, z, Some(foot.y + 1.0)) {
                Some((y, SOIL)) => vec![(y, f64::NEG_INFINITY, 1.0)],
                _ => Vec::new(),
            });
            for (d, depth) in depths.iter().enumerate() {
                let low = foot.y - depth;
                let rootable: f64 = column
                    .iter()
                    .map(|&(top, bottom, share)| {
                        (top.min(foot.y) - bottom.max(low)).max(0.0) * share
                    })
                    .sum();
                ring[r * depths.len() + d] += rootable * area;
            }
        };
        // Near the foot at the tile's own cells, so a crack counts as it
        // is; farther out on a coarser grid, which is right on average.
        let near = 0.4;
        let fine = tile.cell();
        let n = (near / fine).round() as i64;
        for k in -n..n {
            for i in -n..n {
                let x = foot.x + (i as f64 + 0.5) * fine;
                let z = foot.z + (k as f64 + 0.5) * fine;
                add(x, z, fine * fine);
            }
        }
        let coarse = 0.04;
        let far = radii[radii.len() - 1];
        let m = (far / coarse).ceil() as i64;
        for k in -m..m {
            for i in -m..m {
                let (dx, dz) = ((i as f64 + 0.5) * coarse, (k as f64 + 0.5) * coarse);
                if dx.abs() < near && dz.abs() < near {
                    continue;
                }
                add(foot.x + dx, foot.z + dz, coarse * coarse);
            }
        }
        // Cumulative over the reaches.
        let columns = depths.len();
        for r in 1..radii.len() {
            for d in 0..columns {
                ring[r * columns + d] += ring[(r - 1) * columns + d];
            }
        }
        Some(crate::conditions::Rooting {
            version: 1,
            radii_m: radii,
            depths_m: depths,
            volume_m3: ring,
        })
    }

    /// The tile under (`x`, `z`) whose surface is at or below `below`.
    fn pavement_at(&self, x: f64, z: f64, below: Option<f64>) -> Option<&Pavement> {
        self.pavements.iter().find(|tile| {
            tile.covers(x, z) && below.is_none_or(|limit| tile.top() <= limit + 1.0e-9)
        })
    }

    /// Read a Wavefront OBJ file's `v` and `f` lines (polygons are fanned
    /// into triangles; texture and normal indices are ignored), giving each
    /// face the surface `surface_of` returns for the name of the `usemtl`
    /// in force (`None` before the first). With `z_up`, the file's +Z is up
    /// and its +Y north, as in many CAD and GIS tools, and is turned to
    /// `PlantGen`'s +Y up, −Z north.
    ///
    /// # Errors
    ///
    /// On a line that does not parse, or as [`Self::new`].
    pub fn from_obj(
        text: &str,
        z_up: bool,
        surface_of: &dyn Fn(Option<&str>) -> Result<Surface, String>,
    ) -> Result<Self, String> {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        let mut materials = Vec::new();
        let mut current = surface_of(None)?;
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut words = line.split_whitespace();
            let fail = |what: &str| format!("OBJ line {}: {what}", number + 1);
            match words.next() {
                Some("v") => {
                    let values: Vec<f64> = words
                        .take(3)
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .map_err(|_| fail("a vertex needs three numbers"))?;
                    let [x, y, z] = values[..] else {
                        return Err(fail("a vertex needs three numbers"));
                    };
                    positions.push(if z_up {
                        Vec3::new(x, z, -y)
                    } else {
                        Vec3::new(x, y, z)
                    });
                }
                Some("f") => {
                    let corners: Vec<u32> = words
                        .map(|word| {
                            let first = word.split('/').next().unwrap_or("");
                            let index: i64 = first
                                .parse()
                                .map_err(|_| fail(&format!("bad corner `{word}`")))?;
                            #[allow(clippy::cast_possible_wrap)]
                            let count = positions.len() as i64;
                            let resolved = if index < 0 { count + index } else { index - 1 };
                            if !(0..count).contains(&resolved) {
                                return Err(fail(&format!("corner `{word}` names no vertex")));
                            }
                            u32::try_from(resolved).map_err(|_| fail("too many vertices"))
                        })
                        .collect::<Result<_, _>>()?;
                    if corners.len() < 3 {
                        return Err(fail("a face needs three corners"));
                    }
                    for k in 1..corners.len() - 1 {
                        triangles.push([corners[0], corners[k], corners[k + 1]]);
                        materials.push(current);
                    }
                }
                Some("usemtl") => {
                    let name = words.next().unwrap_or("");
                    current = surface_of(Some(name)).map_err(|error| fail(&error))?;
                }
                _ => {}
            }
        }
        Self::with_surfaces(&positions, &triangles, &materials)
    }

    /// The low and high corners of the box round the mesh.
    #[must_use]
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.positions.iter().fold(
            (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(low, high), p| (low.min(*p), high.max(*p)),
        )
    }

    /// How many triangles the mesh holds.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Triangle `index`'s corners, as welded.
    pub(crate) fn triangle_corners(&self, index: usize) -> [u32; 3] {
        self.triangles[index]
    }

    /// Triangle `index`'s unit normal.
    pub(crate) fn normal(&self, index: usize) -> Vec3 {
        self.normals[index]
    }

    /// Triangle `index`'s surface.
    pub(crate) fn surface(&self, index: usize) -> Surface {
        self.surfaces[index]
    }

    /// The triangles whose footprint covers the column cell of (`x`, `z`),
    /// thin parts' included.
    pub(crate) fn column_triangles(&self, x: f64, z: f64) -> &[u32] {
        self.columns_all
            .get(&column_of(x, z))
            .map_or(&[], Vec::as_slice)
    }

    /// Edge of a column cell, metres.
    pub(crate) const COLUMN_M: f64 = COLUMN_CELL;

    pub(crate) fn corners(&self, index: usize) -> [Vec3; 3] {
        self.triangles[index].map(|i| self.positions[i as usize])
    }

    /// Where a vertical line through (`x`, `z`) meets the mesh: each
    /// crossing's height and triangle, highest first.
    #[allow(clippy::many_single_char_names)]
    pub(crate) fn crossings(&self, x: f64, z: f64) -> Vec<(f64, usize)> {
        // Nudged off the lines a modelling tool's grid snaps corners to, so
        // a line meets the faces that share an edge or corner once.
        let (x, z) = (x + 1.234_567e-7, z + 2.718_281e-7);
        let mut out = Vec::new();
        for &index in self.columns.get(&column_of(x, z)).into_iter().flatten() {
            let index = index as usize;
            if self.surfaces[index].thin() {
                continue;
            }
            let [a, b, c] = self.corners(index);
            // Barycentric weights in the XZ plane.
            let area = (b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z);
            if area.abs() < 1.0e-12 {
                continue;
            }
            let w0 = ((b.x - x) * (c.z - z) - (c.x - x) * (b.z - z)) / area;
            let w1 = ((c.x - x) * (a.z - z) - (a.x - x) * (c.z - z)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                out.push((w0 * a.y + w1 * b.y + w2 * c.y, index));
            }
        }
        out.sort_by(|p, q| q.0.total_cmp(&p.0));
        out
    }

    /// The top of the ground at (`x`, `z`): the height of the highest
    /// upward-facing surface there, at or below `below` when given, and its
    /// material.
    #[must_use]
    pub fn ground_at(&self, x: f64, z: f64, below: Option<f64>) -> Option<(f64, u8)> {
        if let Some(tile) = self.pavement_at(x, z, below) {
            return tile.footing(x, z);
        }
        self.crossings(x, z)
            .into_iter()
            .find(|&(y, index)| {
                self.normals[index].y > 0.0 && below.is_none_or(|limit| y <= limit + 1.0e-9)
            })
            .map(|(y, index)| (y, self.materials[index]))
    }

    /// Where a plant set at (`x`, `z`) stands: on the highest soil there
    /// (at or below `below` when given), so a plant set under a roof's
    /// eaves or a balcony roots in the ground below them; with no soil
    /// there, on the highest upward-facing surface, as [`Self::ground_at`].
    #[must_use]
    pub fn footing(&self, x: f64, z: f64, below: Option<f64>) -> Option<(f64, u8)> {
        if let Some(tile) = self.pavement_at(x, z, below) {
            return tile.footing(x, z);
        }
        let up = |y: f64, index: usize| {
            self.normals[index].y > 0.0 && below.is_none_or(|limit| y <= limit + 1.0e-9)
        };
        let crossings = self.crossings(x, z);
        crossings
            .iter()
            .find(|&&(y, index)| up(y, index) && self.materials[index] == SOIL)
            .or_else(|| crossings.iter().find(|&&(y, index)| up(y, index)))
            .map(|&(y, index)| (y, self.materials[index]))
    }

    /// The pseudo-normal at the point of triangle `index` nearest `p`
    /// (the face's own normal inside it, its edge's or corner's on them).
    #[allow(clippy::many_single_char_names)]
    fn pseudo_normal(&self, index: usize, closest: Vec3) -> Vec3 {
        let corners = self.triangles[index];
        let [a, b, c] = corners.map(|i| self.positions[i as usize]);
        let scale = (b - a).length().max((c - a).length()).max(1.0e-9);
        let near = |q: Vec3| (q - closest).length() <= scale * 1.0e-7;
        for (k, point) in [a, b, c].into_iter().enumerate() {
            if near(point) {
                return self.vertex_normals[corners[k] as usize];
            }
        }
        for k in 0..3 {
            let (i, j) = (corners[k], corners[(k + 1) % 3]);
            let (p, q) = (self.positions[i as usize], self.positions[j as usize]);
            let along = q - p;
            let t = (closest - p).dot(along) / along.dot(along).max(1.0e-18);
            if (p + along * t.clamp(0.0, 1.0) - closest).length() <= scale * 1.0e-7 {
                return self.edge_normals[&(i.min(j), i.max(j))];
            }
        }
        self.normals[index]
    }

    /// The substrate grid round a plant whose foot stands at `foot`, in
    /// the plant's frame (`foot` at its origin, +Y up), covering `cut`.
    ///
    /// # Errors
    ///
    /// When `cut` is not positive and finite, or makes a grid larger than
    /// the substrate allows.
    #[allow(clippy::too_many_lines, clippy::many_single_char_names)]
    pub fn substrate_round(&self, foot: Vec3, cut: Cut) -> Result<Substrate, String> {
        let Cut {
            half_m,
            below_m,
            above_m,
            cell_m,
        } = cut;
        if ![half_m, below_m, above_m, cell_m]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
            || !foot.is_finite()
        {
            return Err(format!(
                "a site cut must be positive, found {cut:?} at {foot:?}"
            ));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = |extent: f64| ((extent / cell_m).ceil() as usize).max(2);
        let size = [
            count(2.0 * half_m),
            count(below_m + above_m),
            count(2.0 * half_m),
        ];
        let cells = size.iter().map(|n| *n as u64).product::<u64>();
        if cells > MAX_SUBSTRATE_CELLS {
            return Err(format!(
                "a site cut of {cells} cells is larger than the substrate's {MAX_SUBSTRATE_CELLS}"
            ));
        }
        let [nx, ny, nz] = size;
        let origin = Vec3::new(-half_m, -below_m, -half_m);
        let world_origin = foot + origin;
        let scale = cell_m / 16.0;
        let band = 127.0 * scale;
        #[allow(clippy::cast_precision_loss)]
        let centre = |x: usize, y: usize, z: usize| {
            world_origin + Vec3::new(x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5) * cell_m
        };
        let index = |x: usize, y: usize, z: usize| (y * nz + z) * nx + x;
        let total = nx * ny * nz;
        // Within the band: the unsigned distance, the sign and the material
        // of the nearest surface.
        let mut nearest = vec![f64::INFINITY; total];
        let mut inside = vec![false; total];
        let mut stuff = vec![AIR; total];
        // Thin parts: the signed distance to the nearest slab, and its
        // material.
        let mut slab = vec![f64::INFINITY; total];
        let mut slab_stuff = vec![AIR; total];
        #[allow(clippy::cast_precision_loss)]
        let high_corner = world_origin + Vec3::new(nx as f64, ny as f64, nz as f64) * cell_m;
        let reach = Vec3::new(band, band, band);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let to_cell = |v: f64, low: f64, n: usize| -> usize {
            (((v - low) / cell_m).floor().max(0.0) as usize).min(n - 1)
        };
        for triangle in 0..self.triangles.len() {
            let [a, b, c] = self.corners(triangle);
            let half_thick = self.surfaces[triangle].thin_m * 0.5;
            let reach = reach + Vec3::new(half_thick, half_thick, half_thick);
            let low = a.min(b).min(c) - reach;
            let high = a.max(b).max(c) + reach;
            if low.x > high_corner.x
                || low.y > high_corner.y
                || low.z > high_corner.z
                || high.x < world_origin.x
                || high.y < world_origin.y
                || high.z < world_origin.z
            {
                continue;
            }
            let (x0, x1) = (
                to_cell(low.x, world_origin.x, nx),
                to_cell(high.x, world_origin.x, nx),
            );
            let (y0, y1) = (
                to_cell(low.y, world_origin.y, ny),
                to_cell(high.y, world_origin.y, ny),
            );
            let (z0, z1) = (
                to_cell(low.z, world_origin.z, nz),
                to_cell(high.z, world_origin.z, nz),
            );
            let normal = self.normals[triangle];
            for y in y0..=y1 {
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        let p = centre(x, y, z);
                        // A cell farther from the triangle's plane than the
                        // band is farther from the triangle too.
                        if (p - a).dot(normal).abs() >= band + half_thick {
                            continue;
                        }
                        let closest = closest_on_triangle(p, a, b, c);
                        let gap = (p - closest).length();
                        let at = index(x, y, z);
                        if half_thick > 0.0 {
                            let signed = gap - half_thick;
                            if signed < band && signed < slab[at] {
                                slab[at] = signed;
                                slab_stuff[at] = self.materials[triangle];
                            }
                            continue;
                        }
                        if gap < band && gap < nearest[at] {
                            nearest[at] = gap;
                            let outward = self.pseudo_normal(triangle, closest);
                            inside[at] = (p - closest).dot(outward) < 0.0;
                            stuff[at] = self.materials[triangle];
                        }
                    }
                }
            }
        }
        // Beyond the band: each column carries the sign and material of the
        // nearest surface above, from what a ray from the sky finds.
        let mut distance = vec![0_u8; total];
        let mut material = vec![AIR; total];
        let top = world_origin.y + cell_m * f64::from(u32::try_from(ny).unwrap_or(u32::MAX));
        for z in 0..nz {
            for x in 0..nx {
                let p = centre(x, 0, z);
                let above: Vec<(f64, usize)> = self
                    .crossings(p.x, p.z)
                    .into_iter()
                    .filter(|(y, _)| *y > top)
                    .collect();
                let mut solid = above.len() % 2 == 1;
                let mut what = above.last().map_or(SOIL, |&(_, t)| self.materials[t]);
                for y in (0..ny).rev() {
                    let at = index(x, y, z);
                    let mut signed = if nearest[at].is_finite() {
                        solid = inside[at];
                        what = stuff[at];
                        if solid { -nearest[at] } else { nearest[at] }
                    } else if solid {
                        -band
                    } else {
                        band
                    };
                    let mut here = what;
                    // A thin part's slab joins the solids where it is
                    // nearer.
                    if slab[at] < signed {
                        signed = slab[at];
                        here = slab_stuff[at];
                    }
                    // A paved tile stands in for the mesh at and below its
                    // surface, and joins it above.
                    let p = centre(x, y, z);
                    for tile in &self.pavements {
                        if let Some((paved, stuff)) = tile.classify(p, cell_m) {
                            if p.y <= tile.top() || paved < signed {
                                signed = paved;
                                here = stuff;
                            }
                            break;
                        }
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let units = (signed / scale).round().clamp(-127.0, 127.0) as i8;
                    #[allow(clippy::cast_sign_loss)]
                    {
                        distance[at] = units as u8;
                    }
                    material[at] = if units < 0 { here } else { AIR };
                }
            }
        }
        Ok(Substrate {
            version: SUBSTRATE_VERSION,
            preset: None,
            seed: None,
            scale: None,
            origin_m: Some([origin.x, origin.y, origin.z]),
            cell_m: Some(cell_m),
            size: Some(size.map(|n| u32::try_from(n).unwrap_or(u32::MAX))),
            distance_scale_m: Some(scale),
            distance: Some(base64_encode(&distance)),
            material: Some(base64_encode(&material)),
            up: Some([0.0, 1.0, 0.0]),
        })
    }

    /// The surfaces a climber standing at `foot` can hold to within `cut`:
    /// the mesh's triangles of the materials `holds` accepts, in the
    /// plant's frame, as a `host@1` host, with the surfaces kept clear,
    /// which `host@1` reads as `gc`. `None` when there are neither.
    #[must_use]
    pub fn host_round(&self, foot: Vec3, cut: Cut, holds: &dyn Fn(u8) -> bool) -> Option<Host> {
        let low = Vec3::new(-cut.half_m, -cut.below_m, -cut.half_m);
        let high = Vec3::new(cut.half_m, cut.above_m, cut.half_m);
        let within = |[a, b, c]: &[Vec3; 3]| {
            let (l, h) = (a.min(*b).min(*c), a.max(*b).max(*c));
            l.x <= high.x
                && l.y <= high.y
                && l.z <= high.z
                && h.x >= low.x
                && h.y >= low.y
                && h.z >= low.z
        };
        let triangles: Vec<[Vec3; 3]> = (0..self.triangles.len())
            .filter(|&index| {
                let surface = self.surfaces[index];
                surface.climb && !surface.clear && holds(self.materials[index])
            })
            .map(|index| self.corners(index).map(|p| p - foot))
            .filter(within)
            .collect();
        let clear: Vec<[Vec3; 3]> = (0..self.triangles.len())
            .filter(|&index| self.surfaces[index].clear)
            .map(|index| self.corners(index).map(|p| p - foot))
            .filter(within)
            .collect();
        if triangles.is_empty() && clear.is_empty() {
            return None;
        }
        Some(
            Host::new(Vec::new())
                .with_triangles(triangles, (low, high))
                .with_clear(clear, (low, high)),
        )
    }

    /// Cut back what of `graph` (a plant standing at `foot`, in its own
    /// frame) has grown onto a surface kept clear, as a gardener keeps a
    /// window or a door clear: every segment that ends within `gap` of one,
    /// with all that grows from it, and every organ within `gap` of one.
    /// How many segments and organs were cut.
    pub fn cut_back(&self, graph: &mut PlantGraph, foot: Vec3, gap: f64) -> (usize, usize) {
        let clear: Vec<[Vec3; 3]> = (0..self.triangles.len())
            .filter(|&index| self.surfaces[index].clear)
            .map(|index| self.corners(index).map(|p| p - foot))
            .collect();
        if clear.is_empty() {
            return (0, 0);
        }
        let (low, high) = graph.segments.iter().fold(
            (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ),
            |(low, high), segment| (low.min(segment.end), high.max(segment.end)),
        );
        let margin = Vec3::new(gap, gap, gap) * 2.0;
        let near = Host::new(Vec::new()).with_triangles(clear, (low - margin, high + margin));
        let touches = |p: Vec3| near.nearest(p, gap).is_some();
        // A cut carries to all that grows from it.
        let mut cut: Vec<bool> = graph
            .segments
            .iter()
            .map(|segment| touches(segment.end))
            .collect();
        loop {
            let mut changed = false;
            for index in 0..graph.segments.len() {
                if !cut[index]
                    && graph.segments[index]
                        .parent
                        .is_some_and(|parent| cut[parent as usize])
                {
                    cut[index] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut renumbered = vec![None; graph.segments.len()];
        let mut kept = Vec::with_capacity(graph.segments.len());
        for (index, segment) in graph.segments.iter().enumerate() {
            if !cut[index] {
                renumbered[index] = u32::try_from(kept.len()).ok();
                kept.push(segment.clone());
            }
        }
        for segment in &mut kept {
            segment.parent = segment
                .parent
                .and_then(|parent| renumbered[parent as usize]);
        }
        let segments_cut = graph.segments.len() - kept.len();
        graph.segments = kept;
        let before = graph.organs.len();
        graph.organs.retain(|organ| {
            organ.segment.is_none_or(|segment| !cut[segment as usize]) && !touches(organ.position)
        });
        for organ in &mut graph.organs {
            organ.segment = organ
                .segment
                .and_then(|segment| renumbered[segment as usize]);
        }
        graph.height = graph
            .segments
            .iter()
            .map(|segment| segment.end.y)
            .chain(graph.organs.iter().map(|organ| organ.position.y))
            .fold(0.0, f64::max);
        (segments_cut, before - graph.organs.len())
    }

    /// The mesh's triangles within the box `low` to `high`, moved by
    /// `offset`, as a mesh to draw: flat-shaded, coloured by material.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn drawn(&self, low: Vec3, high: Vec3, offset: Vec3) -> crate::mesh::Mesh {
        let mut mesh = crate::mesh::Mesh::default();
        for index in 0..self.triangles.len() {
            let [a, b, c] = self.corners(index);
            let (l, h) = (a.min(b).min(c), a.max(b).max(c));
            if l.x > high.x
                || l.y > high.y
                || l.z > high.z
                || h.x < low.x
                || h.y < low.y
                || h.z < low.z
            {
                continue;
            }
            let surface = self.surfaces[index];
            let color = surface
                .colour
                .unwrap_or(COLOURS[usize::from(self.materials[index])]);
            // A thin part is seen from both sides.
            let faces: &[(Vec3, [Vec3; 3])] = &[
                (self.normals[index], [a, b, c]),
                (-self.normals[index], [a, c, b]),
            ];
            if self.pavements.iter().any(|tile| {
                let (l, h) = tile.extent();
                let centre = (a + b + c) * (1.0 / 3.0);
                centre.x > l[0]
                    && centre.x < h[0]
                    && centre.z > l[1]
                    && centre.z < h[1]
                    && centre.y <= tile.top() + 1.0e-6
            }) {
                // Under a paved tile, which draws itself.
                continue;
            }
            for (normal, corners) in &faces[..if surface.thin() { 2 } else { 1 }] {
                let normal = normal.to_f32();
                let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
                for p in corners {
                    mesh.positions.push((*p + offset).to_f32());
                    mesh.normals.push(normal);
                    mesh.uvs.push([0.0, 0.0]);
                    mesh.colors.push([color[0], color[1], color[2], 1.0]);
                    mesh.births.push(0.0);
                    mesh.sheds.push(f32::INFINITY);
                    mesh.levels.push(0);
                }
                mesh.indices.extend([first, first + 1, first + 2]);
            }
        }
        for tile in &self.pavements {
            crate::damage::append(&mut mesh, &tile.drawn(low, high, offset));
        }
        mesh
    }
}

/// Linear colours a site is drawn in, by material: air (unused), soil,
/// rock, bark, wood, built.
const COLOURS: [[f32; 3]; 6] = [
    [0.0, 0.0, 0.0],
    [0.13, 0.11, 0.07],
    [0.25, 0.25, 0.24],
    [0.12, 0.08, 0.05],
    [0.30, 0.21, 0.12],
    [0.33, 0.14, 0.09],
];

/// Whether a climber's holdfasts, tendrils or twining stems can hold to a
/// surface of `material`: anything solid above the ground (rock, bark,
/// wood, built), not the soil it roots in.
#[must_use]
pub fn climbable(material: u8) -> bool {
    matches!(material, ROCK | BARK | WOOD | BUILT)
}

/// A substrate material by its name in a site's material table.
///
/// # Errors
///
/// On a name that is not one.
pub fn material_named(name: &str) -> Result<u8, String> {
    match name {
        "soil" => Ok(SOIL),
        "rock" => Ok(ROCK),
        "bark" => Ok(BARK),
        "wood" => Ok(WOOD),
        "built" => Ok(BUILT),
        other => Err(format!(
            "unknown site material `{other}`; use soil, rock, bark, wood or built"
        )),
    }
}

fn column_of(x: f64, z: f64) -> (i32, i32) {
    #[allow(clippy::cast_possible_truncation)]
    let cell = |v: f64| (v / COLUMN_CELL).floor() as i32;
    (cell(x), cell(z))
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// A box from `low` to `high`, its faces wound outward.
    fn cuboid(low: Vec3, high: Vec3, material: u8, out: &mut (Vec<Vec3>, Vec<[u32; 3]>, Vec<u8>)) {
        let first = u32::try_from(out.0.len()).unwrap();
        for i in 0..8 {
            out.0.push(Vec3::new(
                if i & 1 == 0 { low.x } else { high.x },
                if i & 2 == 0 { low.y } else { high.y },
                if i & 4 == 0 { low.z } else { high.z },
            ));
        }
        // Quads as corner bit patterns, counter-clockwise from outside.
        let quads = [
            [0, 4, 6, 2], // -X
            [1, 3, 7, 5], // +X
            [0, 1, 5, 4], // -Y
            [2, 6, 7, 3], // +Y
            [0, 2, 3, 1], // -Z
            [4, 5, 7, 6], // +Z
        ];
        for [a, b, c, d] in quads {
            out.1.push([first + a, first + b, first + c]);
            out.1.push([first + a, first + c, first + d]);
            out.2.extend([material, material]);
        }
    }

    /// Level ground 10 m across at height 2 with a brick wall standing on
    /// it north of the origin, 3 m high and 0.3 m thick.
    fn wall_site() -> SiteMesh {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-5.0, 0.0, -5.0),
            Vec3::new(5.0, 2.0, 5.0),
            SOIL,
            &mut parts,
        );
        cuboid(
            Vec3::new(-2.0, 2.0, -0.8),
            Vec3::new(2.0, 5.0, -0.5),
            BUILT,
            &mut parts,
        );
        SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap()
    }

    #[test]
    fn the_ground_is_found_under_a_point() {
        let site = wall_site();
        assert_eq!(site.ground_at(0.0, 1.0, None), Some((2.0, SOIL)));
        // On the wall's top, unless asked for the ground beside it.
        let (top, material) = site.ground_at(0.0, -0.6, None).unwrap();
        assert!((top - 5.0).abs() < 1e-9 && material == BUILT);
        assert_eq!(site.ground_at(0.0, -0.6, Some(3.0)), Some((2.0, SOIL)));
        assert_eq!(site.ground_at(9.0, 0.0, None), None);
    }

    #[test]
    fn a_cut_reads_like_the_mesh_it_came_from() {
        let site = wall_site();
        let foot = Vec3::new(0.0, 2.0, 0.0);
        let substrate = site
            .substrate_round(
                foot,
                Cut {
                    half_m: 1.0,
                    below_m: 0.5,
                    above_m: 2.0,
                    cell_m: 0.025,
                },
            )
            .unwrap();
        substrate.validate().unwrap();
        let field = substrate.field().unwrap();
        // Ground under the foot, air above it, the wall's face 0.5 m north.
        assert!((field.distance(Vec3::new(0.3, 0.2, 0.3)) - 0.2).abs() < 0.01);
        assert!(field.solid(Vec3::new(0.3, -0.2, 0.3)));
        assert_eq!(field.material_at(Vec3::new(0.3, -0.2, 0.3)), SOIL);
        assert!((field.distance(Vec3::new(0.0, 1.0, -0.4)) - 0.1).abs() < 0.01);
        assert!(field.solid(Vec3::new(0.0, 1.0, -0.65)));
        assert_eq!(field.material_at(Vec3::new(0.0, 1.0, -0.65)), BUILT);
        // Deep in the ground, past the band, is still solid.
        assert!(field.solid(Vec3::new(0.0, -0.45, 0.0)));
        assert!(!field.solid(Vec3::new(0.9, 1.9, 0.9)));
    }

    #[test]
    fn a_climber_finds_the_wall_but_not_the_ground() {
        let site = wall_site();
        let foot = Vec3::new(0.0, 2.0, 0.0);
        let cut = Cut {
            half_m: 1.0,
            below_m: 0.5,
            above_m: 2.0,
            cell_m: 0.025,
        };
        let host = site.host_round(foot, cut, &climbable).unwrap();
        // From 10 cm above the foot the nearest surface to hold is the wall,
        // 0.5 m north (−Z), not the soil 10 cm below.
        let (gap, toward) = host.nearest(Vec3::new(0.0, 0.1, 0.0), 1.0).unwrap();
        assert!((gap - 0.5).abs() < 1e-9, "{gap}");
        assert!((toward.z + 1.0).abs() < 1e-9, "{toward:?}");
        let only_soil = site.host_round(
            Vec3::new(4.0, 2.0, 4.0),
            Cut { half_m: 0.5, ..cut },
            &climbable,
        );
        assert!(only_soil.is_none());
    }

    #[test]
    fn obj_faces_and_materials_are_read() {
        let text = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl brick\nf 1 2 3 4\nusemtl lawn\nf -4/1/1 -2/2/2 -1/3/3\n";
        let names = |name: Option<&str>| match name {
            Some("brick") => Ok(Surface::solid(BUILT)),
            _ => Ok(Surface::solid(SOIL)),
        };
        let site = SiteMesh::from_obj(text, false, &names).unwrap();
        assert_eq!(site.triangle_count(), 3);
        assert_eq!(site.materials, vec![BUILT, BUILT, SOIL]);
        let z_up = SiteMesh::from_obj(text, true, &names).unwrap();
        assert_eq!(z_up.bounds().1.y, 0.0);
        assert!(SiteMesh::from_obj("v 0 0 0\nf 1 2 3\n", false, &names).is_err());
    }

    /// Ground 4 m across with its top at 0, and an open, one-sided panel
    /// 2 m wide and 1.5 m high standing at z = 0, of `panel`.
    fn panel_site(panel: Surface) -> SiteMesh {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-2.0, -1.0, -2.0),
            Vec3::new(2.0, 0.0, 2.0),
            SOIL,
            &mut parts,
        );
        let first = u32::try_from(parts.0.len()).unwrap();
        parts.0.extend([
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.5, 0.0),
            Vec3::new(-1.0, 1.5, 0.0),
        ]);
        parts.1.push([first, first + 1, first + 2]);
        parts.1.push([first, first + 2, first + 3]);
        let mut surfaces: Vec<Surface> = parts.2.iter().map(|m| Surface::solid(*m)).collect();
        surfaces.extend([panel, panel]);
        SiteMesh::with_surfaces(&parts.0, &parts.1, &surfaces).unwrap()
    }

    /// The signed distance (metres) and material of the cell of `grid`
    /// whose centre is nearest `p`, in the plant's frame.
    fn cell_at(grid: &Substrate, p: Vec3) -> (f64, u8) {
        let size = grid.size.unwrap().map(|n| n as usize);
        let origin = grid.origin_m.unwrap();
        let cell = grid.cell_m.unwrap();
        let distance = crate::substrate::base64_decode(grid.distance.as_deref().unwrap()).unwrap();
        let material = crate::substrate::base64_decode(grid.material.as_deref().unwrap()).unwrap();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at = |v: f64, low: f64| ((v - low) / cell).floor() as usize;
        let (x, y, z) = (at(p.x, origin[0]), at(p.y, origin[1]), at(p.z, origin[2]));
        let index = (y * size[2] + z) * size[0] + x;
        #[allow(clippy::cast_possible_wrap)]
        let units = f64::from(distance[index] as i8);
        (units * grid.distance_scale_m.unwrap(), material[index])
    }

    #[test]
    fn a_plant_under_the_eaves_stands_on_the_ground() {
        let mut parts = (Vec::new(), Vec::new(), Vec::new());
        cuboid(
            Vec3::new(-2.0, -1.0, -2.0),
            Vec3::new(2.0, 0.0, 2.0),
            SOIL,
            &mut parts,
        );
        // A roof slab overhanging the point, 2.5 m up.
        cuboid(
            Vec3::new(-1.0, 2.5, -1.0),
            Vec3::new(1.0, 2.7, 1.0),
            BUILT,
            &mut parts,
        );
        let site = SiteMesh::new(&parts.0, &parts.1, &parts.2).unwrap();
        assert_eq!(site.ground_at(0.3, 0.3, None), Some((2.7, BUILT)));
        assert_eq!(site.footing(0.3, 0.3, None), Some((0.0, SOIL)));
        // Set on the roof by height, it stands there.
        let roof = SiteMesh::new(
            &parts.0[8..],
            &parts.1[12..]
                .iter()
                .map(|t| t.map(|i| i - 8))
                .collect::<Vec<_>>(),
            &parts.2[12..],
        )
        .unwrap();
        assert_eq!(roof.footing(0.3, 0.3, None), Some((2.7, BUILT)));
    }

    #[test]
    fn a_thin_part_stands_as_a_slab_on_both_sides_of_its_face() {
        let wood = Surface {
            thin_m: 0.06,
            ..Surface::solid(WOOD)
        };
        let site = panel_site(wood);
        let cut = Cut {
            half_m: 1.5,
            below_m: 0.5,
            above_m: 2.0,
            cell_m: 0.02,
        };
        let grid = site.substrate_round(Vec3::new(0.0, 0.0, 0.3), cut).unwrap();
        // In the plant's frame the panel stands at z = -0.3. In the slab:
        // solid wood; a hand's breadth either side: air.
        let (inside, stuff) = cell_at(&grid, Vec3::new(0.01, 0.75, -0.29));
        assert!(inside < 0.0, "{inside}");
        assert_eq!(stuff, WOOD);
        for z in [-0.42, -0.18] {
            let (out, stuff) = cell_at(&grid, Vec3::new(0.01, 0.75, z));
            assert!(out > 0.05, "{out} at {z}");
            assert_eq!(stuff, AIR);
        }
        // Nothing under the panel's open edges turns solid: the air beside
        // it reaches down to the ground on both sides.
        let (beside, _) = cell_at(&grid, Vec3::new(1.4, 0.75, -0.6));
        assert!(beside > 0.0);
        let (ground, stuff) = cell_at(&grid, Vec3::new(0.5, -0.2, -0.6));
        assert!(ground < 0.0);
        assert_eq!(stuff, SOIL);
        // A climber holds to the panel.
        let host = site
            .host_round(Vec3::new(0.0, 0.0, 0.3), cut, &climbable)
            .unwrap();
        assert!(!host.is_empty());
    }

    #[test]
    fn a_climber_does_not_hold_to_a_surface_kept_clear() {
        let glass = Surface {
            climb: false,
            thin_m: 0.06,
            ..Surface::solid(BUILT)
        };
        let site = panel_site(glass);
        let cut = Cut {
            half_m: 1.5,
            below_m: 0.5,
            above_m: 2.0,
            cell_m: 0.02,
        };
        assert!(
            site.host_round(Vec3::new(0.0, 0.0, 0.3), cut, &climbable)
                .is_none()
        );
        // It still stands in the substrate, so stems lie on it.
        let grid = site.substrate_round(Vec3::new(0.0, 0.0, 0.3), cut).unwrap();
        let (inside, stuff) = cell_at(&grid, Vec3::new(0.01, 0.75, -0.29));
        assert!(inside < 0.0, "{inside}");
        assert_eq!(stuff, BUILT);
    }
}
