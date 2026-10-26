//! Baked-card tiers for weak GPUs: a plant as its trunk and thick limbs,
//! drawn as coarse tubes, and everything finer (leaves, fronds, blades,
//! flowers, twigs and thin stems) as a few dozen flat cards whose
//! pictures are baked from the full plant.
//!
//! This is how games draw their cheapest plants. SpeedTree's lower levels
//! keep fewer, larger leaf cards and end in a billboard; Horizon Zero
//! Dawn's far vegetation is crossed planes; a billboard cloud (Décoret,
//! Durand, Sillion and Dorsey 2003, "Billboard clouds for extreme model
//! simplification", ACM TOG 22(3): 689-696) replaces a model with a few
//! textured planes fitted to it. The shape of a leaf, the gaps between a
//! frond's pinnae and the colour bands of a flower spike live in the
//! picture, not in triangles, so they survive any triangle budget; solid
//! low-poly geometry can keep only outlines.
//!
//! How a plant is cut into cards, the same for every species:
//!
//! 1. Each organ card of the full plant, and each segment of wood thinner
//!    than [`KEEP_RADIUS`] of the plant's height, is an element: where it
//!    is, which way it faces (a card's normal; a stem faces every way
//!    round itself), and how much it covers.
//! 2. Elements are grouped by k-means on place and facing together
//!    ([`FACING`] sets how far apart two facings count against two
//!    places), so a group is a patch of parts that lie near one plane: a
//!    frond, a wedge of a tussock, a clump of a crown.
//! 3. Each group's plane faces the way its parts face on average, weighted
//!    by area, and passes through their middle. The group is rendered
//!    straight onto it (orthographic, even light, no shadows) into a
//!    picture of colour, coverage and normals.
//! 4. The card is the convex hull of the picture's covered texels, grown
//!    to at most [`CORNERS`] corners, so few pixels run the alpha test for
//!    nothing (trimmed cards).
//!
//! A tier spends texels in proportion to each card's area, the same on
//! every card of a plant.
//!
//! Some plants are mostly what a card draws badly. A fleshy body (a
//! cholla's joints) is kept whole on the stem and where it is as thick as
//! half the trunk, and baked like a leaf, spines and all, where thinner.
//! A solid leaf a tenth of the plant's height or longer (an agave's) is
//! drawn whole as a keeled blade of [`WHOLE_LEAF_TRIANGLES`], in place of
//! a card's worth of triangles: a rosette's leaves spread every way, and
//! baked they fall apart into scraps seen from above.

use crate::body::BodyLook;
use crate::cores;
use crate::graph::PlantGraph;
use crate::looks::{Look, Shape};
use crate::math::{Vec3, any_perpendicular};
use crate::mesh::{self, Card, LodSpec, Mesh, PlantMesh, Vertex};
use crate::raster::{self, Camera, Lighting, Material, Projection, RenderOptions, Texture, Turn};
use crate::spec::{Appearance, GrowthForm};
use crate::templates::Templates;

/// How a baked card turns to the eye, in WGSL for an engine's vertex
/// shader: the reference for [`raster::turned`], with vectors from it in
/// `shaders/turn_cards.json`.
pub const TURN_CARDS_WGSL: &str = include_str!("../shaders/turn_cards.wgsl");

/// Plants this tall or lower take a tier's `small` budget.
pub const SMALL_M: f64 = 2.0;
/// Plants this tall or taller take its `large` budget.
pub const LARGE_M: f64 = 15.0;

/// Is a look a signature organ: a flower, head, cluster, spike or fruit,
/// which a botanist reads a species by before its leaves.
#[must_use]
pub fn signature(look: &Look) -> bool {
    look.inflorescence.is_some()
        || matches!(
            look.shape,
            Shape::Flower(_)
                | Shape::Head(_)
                | Shape::Umbel(_)
                | Shape::Spike(_)
                | Shape::Panicle(_)
                | Shape::Spikelet(_)
                | Shape::Fruit(_)
        )
}

/// One baked-card tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardTier {
    pub name: &'static str,
    /// Cards a plant at most [`SMALL_M`] tall may draw,
    /// and one at least [`LARGE_M`].
    pub small: usize,
    pub large: usize,
    /// Texels of a plant's pictures as seen from one height of eye,
    /// small and large: a card turned to the eye holds one picture for
    /// each of [`HEIGHTS`].
    pub small_texels: usize,
    pub large_texels: usize,
    /// The most texels along a card's longer side.
    pub most_texels: usize,
    /// Wood at least this share of the plant's height in radius stays a
    /// tube, with at most this many sides, its rings dropped where it
    /// bends less than `bend` degrees.
    pub keep: f64,
    pub sides: u32,
    pub bend: f64,
    /// The share of cards that may each hold one ribbon's flowering head;
    /// with none, heads are baked with the rest and ribbons are as many
    /// as the wood allows.
    pub heads: f64,
}

/// A Steam Deck or an integrated GPU at 60 frames a second.
pub const LOW: CardTier = CardTier {
    name: "low-cards",
    small: 40,
    large: 128,
    small_texels: 512 * 512,
    large_texels: 1024 * 1024,
    most_texels: 512,
    keep: KEEP_RADIUS,
    sides: 4,
    bend: 25.0,
    heads: 1.0 / 3.0,
};

/// The weakest machines.
pub const MINIMAL: CardTier = CardTier {
    name: "minimal-cards",
    small: 12,
    large: 32,
    small_texels: 256 * 256,
    large_texels: 512 * 512,
    most_texels: 256,
    keep: KEEP_RADIUS * 2.0,
    sides: 3,
    bend: 40.0,
    heads: 0.0,
};

pub const TIERS: [CardTier; 2] = [LOW, MINIMAL];

impl CardTier {
    fn scaled(height: f64, small: usize, large: usize) -> usize {
        let t = ((height - SMALL_M) / (LARGE_M - SMALL_M)).clamp(0.0, 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let n = (small as f64 + (large as f64 - small as f64) * t).round() as usize;
        n
    }

    /// Cards a plant `height` metres tall may draw.
    #[must_use]
    pub fn cards(&self, height: f64) -> usize {
        Self::scaled(height, self.small, self.large)
    }

    /// Texels its pictures seen from one height of eye may hold.
    #[must_use]
    pub fn texels(&self, height: f64) -> usize {
        Self::scaled(height, self.small_texels, self.large_texels)
    }
}

/// Wood at least this share of the plant's height in radius is drawn as
/// tubes at [`LOW`]; thinner wood is baked into the cards.
pub const KEEP_RADIUS: f64 = 0.004;

/// How far apart two facings at right angles count, against places: as
/// far as this share of the plant's height.
pub const FACING: f64 = 0.7;

/// A coarse triangle without bark this many `keep` radii or nearer a
/// leaf's thin axis is that axis (a coarse tube's sides stray from its
/// segments where it drops rings).
const LEAF_AXIS_REACH: f64 = 10.0;

/// The most a clump's card leans from upright, degrees.
pub const MOST_LEAN: f64 = 45.0;

/// The heights of eye, degrees above level, a card turned to the eye is
/// baked from ([`Turn::Heights`]), as an impostor is from level, from 45
/// degrees above and from above ([`ImpostorTier`]). A crown's card baked
/// from the side alone showed it from the side from every height: from
/// 45 degrees down a big sagebrush's low cards were 37 per cent darker
/// than the full plant, whose lit tops show from above.
pub const HEIGHTS: [f64; 3] = [0.0, 45.0, 90.0];

/// The ways a card turned to the eye is baked from ([`HEIGHTS`]), out
/// along its level normal `level` and up, each with the up of its frame:
/// the world's up laid on it, and from above the way back in, so the card's
/// right stays level and the same in every one.
fn height_ways(level: Vec3) -> [(Vec3, Vec3); 3] {
    let right = Vec3::Y.cross(level);
    HEIGHTS.map(|degrees| {
        let e = crate::math::radians(degrees);
        let way =
            (level * crate::math::cos(e) + Vec3::Y * crate::math::sin(e)).normalize_or(Vec3::Y);
        (way, way.cross(right))
    })
}

/// A solid leaf at least this share of the plant's height long (an
/// agave's, an aloe's) is drawn whole rather than baked: a rosette's few
/// long leaves spread every way, and baked onto a few cards they come
/// apart into scraps seen from above.
pub const WHOLE_LEAF: f64 = 0.1;

/// A solid leaf is drawn whole only when at least this many times as
/// long as it is wide.
pub const WHOLE_LEAF_ASPECT: f64 = 3.0;

/// ... and when the diamond it is drawn as covers within this factor of
/// its own area.
pub const WHOLE_LEAF_FIT: f64 = 1.5;

/// A whole leaf's triangles: flat above from its base and its tip to its
/// edges at its widest, keeled below.
pub const WHOLE_LEAF_TRIANGLES: usize = 6;

/// Triangles of an organ drawn solid baked as one part.
const SOLID_CHUNK: usize = 32;

/// The fewest texels a leaf in a clump's picture shows across.
pub const FACE_ON_TEXELS: f64 = 2.0;

/// The fewest texels across a blade or a stem in a picture.
pub const MIN_TEXELS: f64 = 1.0;

/// The most corners a card's outline keeps.
pub const CORNERS: usize = 8;

/// Rounds of k-means.
const ROUNDS: usize = 16;

/// Points measured together on one core in [`kmeans`].
const KMEANS_RUN: usize = 4096;

/// What a baked-card build drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardStats {
    pub cards: usize,
    pub wood_triangles: usize,
    pub card_triangles: usize,
    /// Solid leaves drawn whole ([`WHOLE_LEAF`]).
    pub leaf_triangles: usize,
    /// Texels of all its pictures.
    pub texels: usize,
    /// Texels of its pictures seen from any one height of eye: those
    /// of every card but a turned one's ([`HEIGHTS`]), of which one
    /// picture is seen at a time.
    pub seen_texels: usize,
    /// The share of the cards' area their pictures cover.
    pub coverage: f64,
}

impl CardStats {
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.wood_triangles + self.card_triangles + self.leaf_triangles
    }
}

/// A plant at a baked-card tier.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardPlant {
    /// Trunk and thick limbs.
    pub wood: Mesh,
    /// The cards, each vertex's alpha the index of its picture.
    pub cards: Mesh,
    pub pictures: Vec<Texture>,
    pub stats: CardStats,
}

/// Where each of a plant's pictures lies in one texture, so a renderer
/// binds one texture per species and draws every card of every plant of
/// it in one call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Atlas {
    pub width: usize,
    pub height: usize,
    /// Each picture's bottom-left texel.
    pub places: Vec<(usize, usize)>,
}

impl Atlas {
    /// The share of the atlas the pictures fill.
    #[must_use]
    pub fn fill(&self, pictures: &[Texture]) -> f64 {
        let used: usize = pictures.iter().map(|p| p.width * p.height).sum();
        #[allow(clippy::cast_precision_loss)]
        let share = used as f64 / (self.width * self.height).max(1) as f64;
        share
    }
}

impl CardPlant {
    /// How wide one of its pictures' texels is where its card lies, metres,
    /// averaged over its cards' area: a tier stays as sharp as it was
    /// baked while a texel covers at most a pixel, as a texture is drawn
    /// from its finest mipmap level while it is magnified (OpenGL 4.6
    /// core profile, section 8.14, the level of detail λ ≤ 0). Each
    /// card counts once, with the picture it was laid down with.
    #[must_use]
    pub fn texel(&self) -> f64 {
        let mesh = &self.cards;
        let (mut weighted, mut total) = (0.0, 0.0);
        for t in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize);
            let p = |i: usize| v3(mesh.positions[i]);
            let area = (p(b) - p(a)).cross(p(c) - p(a)).length() * 0.5;
            let uv = |i: usize| (f64::from(mesh.uvs[i][0]), f64::from(mesh.uvs[i][1]));
            let ((u0, v0), (u1, v1), (u2, v2)) = (uv(a), uv(b), uv(c));
            let uv_area = ((u1 - u0) * (v2 - v0) - (u2 - u0) * (v1 - v0)).abs() * 0.5;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let Some(picture) = self
                .pictures
                .get(mesh.colors[a][3].max(0.0).round() as usize)
            else {
                continue;
            };
            #[allow(clippy::cast_precision_loss)]
            let texels = uv_area * (picture.width * picture.height) as f64;
            if area <= 0.0 || texels <= 0.0 {
                continue;
            }
            weighted += area * (area / texels).sqrt();
            total += area;
        }
        if total > 0.0 { weighted / total } else { 0.0 }
    }

    /// The pictures packed in rows (tallest first) into one texture as
    /// wide as a power of two, the least square one that holds them.
    #[must_use]
    pub fn atlas(&self) -> Atlas {
        let mut order: Vec<usize> = (0..self.pictures.len()).collect();
        order
            .sort_by_key(|&i| std::cmp::Reverse((self.pictures[i].height, self.pictures[i].width)));
        let widest = self.pictures.iter().map(|p| p.width).max().unwrap_or(1);
        let mut best = Atlas::default();
        let mut width = widest.next_power_of_two();
        loop {
            let mut places = vec![(0, 0); self.pictures.len()];
            let (mut x, mut y, mut row) = (0, 0, 0);
            for &i in &order {
                let p = &self.pictures[i];
                if x + p.width > width {
                    (x, y, row) = (0, y + row, 0);
                }
                places[i] = (x, y);
                x += p.width;
                row = row.max(p.height);
            }
            let height = y + row;
            let atlas = Atlas {
                width,
                height,
                places,
            };
            let side = |a: &Atlas| a.width.max(a.height);
            if best.places.is_empty() || side(&atlas) < side(&best) {
                best = atlas;
            }
            if height <= width {
                return best;
            }
            width *= 2;
        }
    }
}

/// Something a card is baked from: an organ card or a segment of thin
/// wood.
#[derive(Debug, Clone, Copy)]
struct Element {
    centre: Vec3,
    /// A card's normal; for wood, its direction.
    facing: Vec3,
    wood: bool,
    /// Part of a leaf the program draws as a structure (a frond, a
    /// grass blade): it lies in that leaf's own surface.
    planar: bool,
    area: f64,
    /// Half its length, along it: a blade or a stem spans
    /// `centre ± half`.
    half: Vec3,
}

/// The six numbers of `n nᵀ` that set a facing regardless of its sign:
/// two facings at right angles are 1 apart, opposite ones 0.
fn facing_features(n: Vec3) -> [f64; 6] {
    let r = core::f64::consts::FRAC_1_SQRT_2;
    [
        n.x * n.x * r,
        n.y * n.y * r,
        n.z * n.z * r,
        n.x * n.y,
        n.x * n.z,
        n.y * n.z,
    ]
}

fn features(element: &Element, height: f64) -> [f64; 9] {
    let p = element.centre * (1.0 / height);
    let f = if !element.planar {
        [0.0; 6]
    } else if element.wood {
        // A stem faces every way round itself: the mean of its normals.
        let d = element.facing;
        let n = facing_features(d);
        let r = core::f64::consts::FRAC_1_SQRT_2;
        let identity = [r, r, r, 0.0, 0.0, 0.0];
        core::array::from_fn(|i| (identity[i] - n[i]) * 0.5)
    } else {
        facing_features(element.facing)
    };
    [
        p.x,
        p.y,
        p.z,
        f[0] * FACING,
        f[1] * FACING,
        f[2] * FACING,
        f[3] * FACING,
        f[4] * FACING,
        f[5] * FACING,
    ]
}

fn distance2(a: &[f64; 9], b: &[f64; 9]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Group `points` (with weights) into at most `k` groups by k-means,
/// seeded by farthest points from the heaviest; each point's group.
fn kmeans(points: &[[f64; 9]], weights: &[f64], k: usize) -> Vec<usize> {
    let n = points.len();
    if n == 0 || k == 0 {
        return vec![0; n];
    }
    let k = k.min(n);
    let first = (0..n)
        .max_by(|&a, &b| weights[a].total_cmp(&weights[b]).then(b.cmp(&a)))
        .unwrap_or(0);
    // Each point's distance to every centre is measured on its own, so
    // the points are shared out over the idle cores in runs.
    let lease = cores::Lease::take(if n * k > 1 << 20 {
        cores::per_task()
    } else {
        1
    });
    let threads = lease.threads();
    let runs = |n: usize| -> Vec<std::ops::Range<usize>> {
        (0..n)
            .step_by(KMEANS_RUN)
            .map(|a| a..(a + KMEANS_RUN).min(n))
            .collect()
    };
    let roots: Vec<f64> = weights.iter().map(|w| w.sqrt()).collect();
    let mut centres = vec![points[first]];
    let mut nearest: Vec<f64> = points.iter().map(|p| distance2(p, &centres[0])).collect();
    while centres.len() < k {
        // The point farthest from every centre, by its weight's root, so a
        // stray hair does not take a card from a frond.
        let next = (0..n)
            .max_by(|&a, &b| {
                (nearest[a] * roots[a])
                    .total_cmp(&(nearest[b] * roots[b]))
                    .then(b.cmp(&a))
            })
            .unwrap_or(0);
        if nearest[next] <= 0.0 {
            break;
        }
        centres.push(points[next]);
        let centre = &centres[centres.len() - 1];
        let nearer = cores::map(runs(n), threads, &|run: std::ops::Range<usize>| {
            run.map(|i| nearest[i].min(distance2(&points[i], centre)))
                .collect::<Vec<f64>>()
        });
        nearest = nearer.concat();
    }
    let mut group = vec![0; n];
    for _ in 0..ROUNDS {
        // The nearest centre, the first of equals.
        let best = cores::map(runs(n), threads, &|run: std::ops::Range<usize>| {
            run.map(|i| {
                let p = &points[i];
                let mut best = 0;
                let mut off = distance2(p, &centres[0]);
                for (c, centre) in centres.iter().enumerate().skip(1) {
                    let d = distance2(p, centre);
                    if d.total_cmp(&off) == std::cmp::Ordering::Less {
                        best = c;
                        off = d;
                    }
                }
                best
            })
            .collect::<Vec<usize>>()
        })
        .concat();
        let changed = best != group;
        group = best;
        let mut sums = vec![([0.0; 9], 0.0); centres.len()];
        for (i, p) in points.iter().enumerate() {
            let (sum, weight) = &mut sums[group[i]];
            for (s, v) in sum.iter_mut().zip(p) {
                *s += v * weights[i];
            }
            *weight += weights[i];
        }
        for (centre, (sum, weight)) in centres.iter_mut().zip(sums) {
            if weight > 0.0 {
                *centre = sum.map(|s| s / weight);
            }
        }
        if !changed {
            break;
        }
    }
    group
}

/// The unit vector a symmetric 3×3 matrix stretches most, by power
/// iteration from `start`.
fn dominant(m: [[f64; 3]; 3], start: Vec3) -> Vec3 {
    let mut v = start.normalize_or(Vec3::Y);
    for _ in 0..64 {
        let next = Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        );
        let next = next.normalize_or(v);
        if (next - v).length() < 1e-9 {
            return next;
        }
        v = next;
    }
    v
}

/// The plane a group of elements is drawn on: its area-weighted middle,
/// its normal, and whether it is flat (most of its area is parts of
/// leaves, which keep their leaf's plane).
fn fit_plane(
    members: &[&Element],
    foliage_centre: Vec3,
    turning: bool,
) -> (Vec3, Vec3, bool, Turn) {
    let total: f64 = members.iter().map(|e| e.area).sum();
    let centre = members
        .iter()
        .fold(Vec3::ZERO, |sum, e| sum + e.centre * e.area)
        * (1.0 / total);
    let mut m = [[0.0; 3]; 3];
    for e in members {
        if e.wood {
            // A plane that holds the stem: every normal round it.
            let mut round = [[0.0; 3]; 3];
            for (i, row) in round.iter_mut().enumerate() {
                row[i] = e.area * 0.5;
            }
            add(&mut m, round);
            add(&mut m, outer(e.facing, -e.area * 0.5));
        } else {
            add(&mut m, outer(e.facing, e.area));
        }
    }
    // Parts of leaves (a frond's pinnae, a grass's blades) keep their
    // leaf's own plane: the way they face on average. Leaves on
    // shoots make clumps with depth; a clump's card faces out from
    // the plant's middle, as game artists turn crown cards outward,
    // leaning at most [`MOST_LEAN`] from upright, since plants are
    // seen from the side and from above at an angle more than from
    // straight above.
    // A clump that wraps round an upright axis (a palm's or a Joshua
    // tree's skirt of dead leaves, a ring of foliage round a bare middle)
    // looks alike from every side, as a crown mass does, and any one
    // plane through it is a band slanting across the trunk: its card
    // stands upright and turns to the eye.
    if wraps(members, centre) {
        // Baked from outside the plant, where its light comes from.
        let out = centre - foliage_centre;
        let level = Vec3::new(out.x, 0.0, out.z).normalize_or_else(|| any_perpendicular(Vec3::Y));
        return (centre, level, false, Turn::Facing);
    }
    let planar_area: f64 = members.iter().filter(|e| e.planar).map(|e| e.area).sum();
    let flat = planar_area * 2.0 >= total;
    let start = members
        .iter()
        .filter(|e| !e.wood)
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .map_or_else(|| any_perpendicular(members[0].facing), |e| e.facing);
    let mut turn = Turn::Fixed;
    let normal = if flat {
        dominant(m, start + Vec3::new(1e-3, 2e-3, 3e-3))
    } else {
        let out = (centre - foliage_centre).normalize_or_else(|| any_perpendicular(Vec3::Y));
        let level = Vec3::new(out.x, 0.0, out.z).normalize_or(Vec3::X);
        let lean = crate::math::asin(out.y.clamp(-1.0, 1.0));
        // A clump taller than it is broad (shoots, racemes, culms)
        // stands upright, as forb and grass cards do: leaning it would
        // swing its foot or top out past the plant.
        let (mut lo, mut hi) = (centre, centre);
        for e in members {
            lo = lo.min(e.centre);
            hi = hi.max(e.centre);
        }
        let span = hi - lo;
        let tall = span.y.max(1e-9);
        let broad = span.x.max(span.z);
        let most = crate::math::radians(MOST_LEAN) * (broad / tall).min(1.0);
        let lean = lean.clamp(-most, most);
        let outward = level * crate::math::cos(lean) + Vec3::Y * crate::math::sin(lean);
        // A clump whose parts spread in a sheet (a wedge of a tussock,
        // whose blades fan out from its foot; a fan of shoots) takes
        // the plane that holds them closest, as a billboard cloud fits
        // its planes to the geometry (Décoret et al. 2003): drawn on
        // a plane facing out, a wedge fanning sideways is seen edge on
        // from the side and the tussock loses its flanks. A clump with
        // depth every way (a crown's) keeps facing out.
        let mut spread = [[0.0; 3]; 3];
        for e in members {
            for p in [e.centre - e.half, e.centre, e.centre + e.half] {
                add(&mut spread, outer(p - centre, e.area));
            }
        }
        let (thinnest, least) = smallest_axis(spread);
        let across = |n: Vec3| {
            let mn = [
                spread[0][0] * n.x + spread[0][1] * n.y + spread[0][2] * n.z,
                spread[1][0] * n.x + spread[1][1] * n.y + spread[1][2] * n.z,
                spread[2][0] * n.x + spread[2][1] * n.y + spread[2][2] * n.z,
            ];
            n.x * mn[0] + n.y * mn[1] + n.z * mn[2]
        };
        if least <= across(outward) * SHEET * SHEET {
            thinnest
        } else {
            // A piece of a broadleaf crown, leaves many times
            // smaller than itself, is a mass that looks alike from every
            // side, and a card facing out is edge on wherever the clump
            // is on the plant's outline: it turns to the eye, as leaf
            // cards do ("leaves that always billboard (face) the
            // camera", SpeedTree Modeler glossary). Leaves on herbs'
            // shoots and in tufts keep their card, since their shapes
            // and the way they face show.
            let leaves: Vec<&&Element> = members.iter().filter(|e| !e.wood).collect();
            let leaf_area: f64 = leaves.iter().map(|e| e.area).sum();
            let leaf_length = leaves
                .iter()
                .map(|e| e.half.length() * 2.0 * e.area)
                .sum::<f64>()
                / leaf_area.max(1e-12);
            if turning && span.length() >= MASS * leaf_length {
                turn = Turn::Facing;
            }
            outward
        }
    };
    (centre, normal, flat, turn)
}

/// Within this share of their mean distance from a clump's upright axis
/// at most [`WRAP_INNER`] of its leaves lie, round it no gap is wider than
/// [`WRAP_GAP`] degrees, they reach at least [`WRAP_TALL`] times that
/// distance up it, and they hang, their tips below their bases by a
/// cosine of at least [`WRAP_HANG`] on average: it is a skirt of
/// hanging leaves round a stem ([`wraps`]), not a tussock's, a shoot's or
/// a crown's fan.
const WRAP_HOLLOW: f64 = 0.4;

/// See [`WRAP_HOLLOW`].
const WRAP_TALL: f64 = 2.0;

/// See [`WRAP_HOLLOW`].
const WRAP_HANG: f64 = 0.5;

/// See [`WRAP_HOLLOW`].
const WRAP_INNER: f64 = 0.15;

/// See [`WRAP_HOLLOW`].
const WRAP_GAP: f64 = 90.0;

/// Whether a clump's leaves ring the upright line through `centre`,
/// leaving its middle bare: a skirt round a trunk, not a heap.
fn wraps(members: &[&Element], centre: Vec3) -> bool {
    let leaves: Vec<(f64, f64, f64)> = members
        .iter()
        .filter(|e| !e.wood)
        .map(|e| {
            let d = e.centre - centre;
            (d.x.hypot(d.z), crate::math::atan2(d.z, d.x), e.area)
        })
        .collect();
    let total: f64 = leaves.iter().map(|l| l.2).sum();
    if leaves.len() < 8 || total <= 0.0 {
        return false;
    }
    let mean = leaves.iter().map(|l| l.0 * l.2).sum::<f64>() / total;
    if mean <= 1e-6 {
        return false;
    }
    // Tall for its width, its leaves hanging down it.
    let (low, high) = members
        .iter()
        .filter(|e| !e.wood)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), e| {
            (lo.min(e.centre.y), hi.max(e.centre.y))
        });
    if high - low < WRAP_TALL * mean {
        return false;
    }
    // A leaf's half runs from its base to its tip.
    let hanging = -members
        .iter()
        .filter(|e| !e.wood)
        .map(|e| e.half.normalize_or(Vec3::ZERO).y * e.area)
        .sum::<f64>()
        / total;
    if hanging < WRAP_HANG {
        return false;
    }
    let inner: f64 = leaves
        .iter()
        .filter(|l| l.0 < WRAP_HOLLOW * mean)
        .map(|l| l.2)
        .sum();
    if inner > WRAP_INNER * total {
        return false;
    }
    let mut angles: Vec<f64> = leaves
        .iter()
        .filter(|l| l.0 >= WRAP_HOLLOW * mean)
        .map(|l| l.1)
        .collect();
    angles.sort_by(f64::total_cmp);
    let (Some(&first), Some(&last)) = (angles.first(), angles.last()) else {
        return false;
    };
    let widest = angles
        .windows(2)
        .map(|w| w[1] - w[0])
        .fold(first + core::f64::consts::TAU - last, f64::max);
    widest < crate::math::radians(WRAP_GAP)
}

/// The unit axis along which a symmetric 3×3 matrix is least, and that
/// least value (Jacobi rotations).
fn smallest_axis(m: [[f64; 3]; 3]) -> (Vec3, f64) {
    let mut a = m;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (mut p, mut q, mut largest) = (0, 1, 0.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > largest {
                (p, q, largest) = (i, j, a[i][j].abs());
            }
        }
        if largest < 1e-15 {
            break;
        }
        let theta = 0.5 * crate::math::atan2(2.0 * a[p][q], a[q][q] - a[p][p]);
        let (c, s) = (crate::math::cos(theta), crate::math::sin(theta));
        let mut b = a;
        for k in 0..3 {
            b[p][k] = c * a[p][k] - s * a[q][k];
            b[q][k] = s * a[p][k] + c * a[q][k];
        }
        let mut d = b;
        for k in 0..3 {
            d[k][p] = c * b[k][p] - s * b[k][q];
            d[k][q] = s * b[k][p] + c * b[k][q];
        }
        a = d;
        for row in &mut v {
            let (x, y) = (row[p], row[q]);
            row[p] = c * x - s * y;
            row[q] = s * x + c * y;
        }
    }
    let i = (0..3)
        .min_by(|&i, &j| a[i][i].total_cmp(&a[j][j]))
        .unwrap_or(0);
    (
        Vec3::new(v[0][i], v[1][i], v[2][i]).normalize_or(Vec3::Z),
        a[i][i],
    )
}

fn outer(n: Vec3, w: f64) -> [[f64; 3]; 3] {
    let a = [n.x, n.y, n.z];
    core::array::from_fn(|i| core::array::from_fn(|j| a[i] * a[j] * w))
}

fn add(m: &mut [[f64; 3]; 3], o: [[f64; 3]; 3]) {
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] += o[i][j];
        }
    }
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// The radius of the wood at vertex `i`, if it is bark.
fn bark_radius(mesh: &Mesh, i: usize) -> Option<f64> {
    let a = mesh.colors[i][3];
    (a < 0.0).then(|| f64::from(-a))
}

/// Add `part`'s cards and pictures to `out`'s, after those it has: its
/// pictures' indices, and the vertices' in its triangles, shift by as many
/// as come before.
fn join_cards(out: &mut CardPlant, part: CardPlant) {
    let pictures = out.pictures.len();
    let vertices = u32::try_from(out.cards.positions.len()).unwrap_or(u32::MAX);
    let cards = part.cards;
    out.cards.positions.extend(cards.positions);
    out.cards.normals.extend(cards.normals);
    out.cards.uvs.extend(cards.uvs);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    out.cards.colors.extend(
        cards
            .colors
            .into_iter()
            .map(|[r, g, b, a]| [r, g, b, (a as usize + pictures) as f32]),
    );
    out.cards.births.extend(cards.births);
    out.cards.sheds.extend(cards.sheds);
    out.cards.levels.extend(cards.levels);
    debug_assert!(cards.segments.is_empty() && out.cards.segments.is_empty());
    out.cards
        .indices
        .extend(cards.indices.into_iter().map(|i| i + vertices));
    out.pictures
        .extend(part.pictures.into_iter().map(|mut picture| {
            match &mut picture.turn {
                Turn::Views { first, .. } | Turn::Heights { first, .. } => *first += pictures,
                _ => {}
            }
            picture
        }));
    out.stats.texels += part.stats.texels;
    out.stats.seen_texels += part.stats.seen_texels;
}

/// Copy triangle `t` of `from` into `to`.
fn copy_triangle(to: &mut Mesh, from: &Mesh, t: usize) {
    for k in 0..3 {
        let i = from.indices[t * 3 + k] as usize;
        let index = u32::try_from(to.positions.len()).unwrap_or(u32::MAX);
        to.positions.push(from.positions[i]);
        to.normals.push(from.normals[i]);
        to.uvs.push(from.uvs.get(i).copied().unwrap_or([0.0, 0.0]));
        to.colors.push(from.colors[i]);
        to.births.push(from.births.get(i).copied().unwrap_or(0.0));
        to.sheds
            .push(from.sheds.get(i).copied().unwrap_or(f32::INFINITY));
        to.levels.push(from.levels.get(i).copied().unwrap_or(0));
        to.indices.push(index);
    }
}

/// The solid leaves among `solid`, triangles of `wood`, at least `least`
/// long, drawn whole ([`WHOLE_LEAF`]) in at most `budget` triangles, the
/// longest first: the blades, and which of `solid` they replace.
fn whole_leaves(
    graph: &PlantGraph,
    looks: &[Look],
    wood: &Mesh,
    solid: &[usize],
    least: f64,
    budget: usize,
) -> (Mesh, Vec<usize>) {
    fn find(parent: &mut [usize], mut a: usize) -> usize {
        while parent[a] != a {
            parent[a] = parent[parent[a]];
            a = parent[a];
        }
        a
    }
    // A leaf is the triangles that touch: its mesh may not share vertices,
    // so they are matched by where they are, to a hundredth of a
    // millimetre.
    let mut parent: Vec<usize> = (0..solid.len()).collect();
    let mut seen = std::collections::HashMap::new();
    for (j, &t) in solid.iter().enumerate() {
        for k in 0..3 {
            let p = wood.positions[wood.indices[t * 3 + k] as usize];
            #[allow(clippy::cast_possible_truncation)]
            let key = p.map(|c| (f64::from(c) * 1e5).round() as i64);
            let other = *seen.entry(key).or_insert(j);
            let (a, b) = (find(&mut parent, other), find(&mut parent, j));
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut parts: Vec<Vec<usize>> = Vec::new();
    let mut part_of = vec![usize::MAX; solid.len()];
    for j in 0..solid.len() {
        let root = find(&mut parent, j);
        if part_of[root] == usize::MAX {
            part_of[root] = parts.len();
            parts.push(Vec::new());
        }
        parts[part_of[root]].push(solid[j]);
    }

    struct Blade {
        /// The area of its outline seen face on.
        area: f64,
        length: f64,
        triangles: Vec<usize>,
        top: [Vec3; 4],
        keel: Vec3,
        normal: Vec3,
        colour: [f32; 4],
        born: f64,
        segment: u32,
    }
    const BINS: usize = 8;
    let blade = |triangles: Vec<usize>| -> Option<Blade> {
        let corner = |t: usize, k: usize| wood.indices[t * 3 + k] as usize;
        let mut weighted = Vec::with_capacity(triangles.len() * 3);
        for &t in &triangles {
            let [a, b, c] = [0, 1, 2].map(|k| v3(wood.positions[corner(t, k)]));
            let w = (b - a).cross(c - a).length() / 6.0;
            weighted.extend([
                (a, w, corner(t, 0)),
                (b, w, corner(t, 1)),
                (c, w, corner(t, 2)),
            ]);
        }
        let total: f64 = weighted.iter().map(|p| p.1).sum();
        if total <= 0.0 {
            return None;
        }
        let centre = weighted
            .iter()
            .fold(Vec3::ZERO, |sum, &(p, w, _)| sum + p * w)
            * (1.0 / total);
        let mut spread = [[0.0; 3]; 3];
        for &(p, w, _) in &weighted {
            add(&mut spread, outer(p - centre, w));
        }
        let mut along = dominant(spread, Vec3::new(0.3, 1.0, 0.2));
        let mut normal = smallest_axis(spread).0;
        let segment = wood.segment(weighted[0].2).unwrap_or(mesh::NO_SEGMENT);
        let bearer = graph.segments.get(segment as usize);
        // Its base is the end nearer the axis that bears it.
        let u = |p: Vec3, along: Vec3| (p - centre).dot(along);
        let (low, high) = weighted.iter().fold(
            ((f64::INFINITY, centre), (f64::NEG_INFINITY, centre)),
            |(low, high), &(p, _, _)| {
                let at = u(p, along);
                (
                    if at < low.0 { (at, p) } else { low },
                    if at > high.0 { (at, p) } else { high },
                )
            },
        );
        let off = |p: Vec3| {
            bearer.map_or(p.y, |s| {
                let axis = s.end - s.start;
                let t = ((p - s.start).dot(axis) / axis.dot(axis).max(1e-12)).clamp(0.0, 1.0);
                (p - (s.start + axis * t)).length()
            })
        };
        if off(high.1) < off(low.1) {
            along = -along;
        }
        let up = bearer
            .map_or(Vec3::Y, |s| s.end - s.start)
            .normalize_or(Vec3::Y);
        if normal.dot(up) < 0.0 {
            normal = -normal;
        }
        let side = normal.cross(along).normalize_or(any_perpendicular(along));
        let (start, length) = (
            u(low.1, along).min(u(high.1, along)),
            (high.0 - low.0).abs(),
        );
        if length <= 0.0 {
            return None;
        }
        let mut tip = (f64::NEG_INFINITY, centre);
        let mut bins = [(
            0.0,
            Vec3::ZERO,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ); BINS];
        let mut colour = [0.0_f64; 3];
        for &(p, w, i) in &weighted {
            let at = u(p, along) - start;
            if at > tip.0 {
                tip = (at, p);
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let b = ((at / length * BINS as f64) as usize).min(BINS - 1);
            let bin = &mut bins[b];
            bin.0 += w;
            bin.1 = bin.1 + p * w;
            let (v, n) = ((p - centre).dot(side), (p - centre).dot(normal));
            bin.2 = bin.2.min(v);
            bin.3 = bin.3.max(v);
            bin.4 = bin.4.min(n);
            bin.5 = bin.5.max(n);
            for (c, sum) in colour.iter_mut().enumerate() {
                *sum += f64::from(wood.colors[i][c]) * w;
            }
        }
        let middle = |b: usize| bins[b].1 * (1.0 / bins[b].0);
        let base = (0..BINS).find(|&b| bins[b].0 > 0.0).map(middle)?;
        // At its widest, short of its base and its tip.
        let widest = (1..BINS - 1)
            .filter(|&b| bins[b].0 > 0.0)
            .max_by(|&a, &b| (bins[a].3 - bins[a].2).total_cmp(&(bins[b].3 - bins[b].2)))?;
        let at = middle(widest);
        let from = (at - centre).dot(side);
        let (left, right) = (
            at + side * (bins[widest].2 - from),
            at + side * (bins[widest].3 - from),
        );
        let keel = at - normal * (bins[widest].5 - bins[widest].4);
        #[allow(clippy::cast_possible_truncation)]
        let colour = colour.map(|c| (c / total) as f32);
        // Its outline's area, from its width along its length.
        #[allow(clippy::cast_precision_loss)]
        let area = bins
            .iter()
            .filter(|bin| bin.0 > 0.0)
            .map(|bin| (bin.3 - bin.2) * length / BINS as f64)
            .sum();
        Some(Blade {
            area,
            length,
            triangles,
            top: [base, right, tip.1, left],
            keel,
            normal,
            colour: [colour[0], colour[1], colour[2], 1.0],
            born: f64::from(wood.births.get(weighted[0].2).copied().unwrap_or(0.0)),
            segment,
        })
    };
    let mut blades: Vec<Blade> = parts.into_iter().filter_map(blade).collect();
    blades.sort_by(|a, b| b.length.total_cmp(&a.length));

    let mut out = Mesh::default();
    let mut replaced = Vec::new();
    for blade in blades {
        if blade.length < least || out.triangle_count() + WHOLE_LEAF_TRIANGLES > budget {
            break;
        }
        // A diamond keeps the outline of a long leaf. A round one would
        // lose a third of itself, and a blade on a long stalk drawn as one
        // (a water lily's pad on its petiole) would be a long diamond
        // with no pad: they stay baked.
        let width = (blade.top[1] - blade.top[3]).length();
        let diamond = blade.length * width * 0.5;
        if blade.length < WHOLE_LEAF_ASPECT * width
            || !(blade.area / WHOLE_LEAF_FIT..=blade.area * WHOLE_LEAF_FIT).contains(&diamond)
        {
            continue;
        }
        // Leaves only: a flower's tepals (a water lily's) stay in its
        // picture. The organ grown nearest its base is the one it is.
        let organ = graph.organs.iter().min_by(|a, b| {
            (a.position - blade.top[0])
                .length()
                .total_cmp(&(b.position - blade.top[0]).length())
        });
        if !organ
            .and_then(|o| looks.get(usize::from(o.organ)))
            .is_some_and(|look| look.solid.is_some() && !signature(look))
        {
            continue;
        }
        let [base, right, tip, left] = blade.top;
        let k = blade.keel;
        let faces = [
            (base, right, tip, blade.normal),
            (base, tip, left, blade.normal),
            (base, left, k, -blade.normal),
            (base, k, right, -blade.normal),
            (k, left, tip, -blade.normal),
            (k, tip, right, -blade.normal),
        ];
        for (a, b, c, want) in faces {
            let mut n = (b - a).cross(c - a).normalize_or(want);
            let corners = if n.dot(want) < 0.0 {
                n = -n;
                [a, c, b]
            } else {
                [a, b, c]
            };
            let first = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
            for p in corners {
                out.push(Vertex {
                    position: p,
                    normal: n,
                    uv: [0.0, 0.0],
                    color: blade.colour,
                    born: blade.born,
                    shed: None,
                    level: 3,
                    segment: blade.segment,
                });
            }
            out.indices
                .extend_from_slice(&[first, first + 1, first + 2]);
        }
        replaced.extend(blade.triangles);
    }
    (out, replaced)
}

/// What every tier of a plant is baked from: the plant at the second of
/// the package's mesh levels with every organ a card of its own and every
/// axis however fine, and its shadow from each way the sun can shine
/// ([`SUN_DIRECTIONS`]). The shadows took a quarter of a tree's bake; made
/// once, they serve all its tiers.
pub struct Source {
    mesh: PlantMesh,
    sky: Sky,
}

impl Source {
    /// `graph`'s source; `levels` are the package's four mesh levels for a
    /// 15 m plant.
    #[must_use]
    pub fn new(
        graph: &PlantGraph,
        looks: &[Look],
        bodies: &[BodyLook],
        appearance: &Appearance,
        templates: &Templates,
        levels: &[LodSpec; 4],
    ) -> Self {
        let height = graph.height.max(0.01);
        // Every axis, however fine (a fern's rachis, a grass's culm): the
        // pictures are where they still show.
        let mesh = mesh::build(
            graph,
            looks,
            bodies,
            appearance,
            &LodSpec {
                min_radius: 0.0,
                ..levels[1].for_height(height)
            },
            1,
        );
        let sky = Sky::new(&mesh.wood, &mesh.card_mesh(), templates);
        Self { mesh, sky }
    }
}

/// Every tier of `graph` from one [`Source`], by name: [`TIERS`]' cards,
/// then the [`FAR`] impostor.
#[must_use]
pub fn tiers(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    form: GrowthForm,
    templates: &Templates,
    levels: &[LodSpec; 4],
) -> Vec<(&'static str, CardPlant)> {
    let from = Source::new(graph, looks, bodies, appearance, templates, levels);
    let mut out: Vec<(&'static str, CardPlant)> = TIERS
        .iter()
        .map(|tier| {
            let plant = build_from(
                &from, graph, looks, bodies, appearance, form, templates, levels, tier,
            );
            (tier.name, plant)
        })
        .collect();
    out.push((
        FAR.name,
        impostor_from(&from, graph, looks, appearance, form, templates, &FAR),
    ));
    out
}

/// Build `graph` at `tier`. `levels` are the package's four mesh levels
/// for a 15 m plant; the cards are baked from the second, every organ a
/// card of its own ([`Source`]), and the tubes come from the coarsest.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    form: GrowthForm,
    templates: &Templates,
    levels: &[LodSpec; 4],
    tier: &CardTier,
) -> CardPlant {
    let from = Source::new(graph, looks, bodies, appearance, templates, levels);
    build_from(
        &from, graph, looks, bodies, appearance, form, templates, levels, tier,
    )
}

/// [`build`] from `graph`'s [`Source`].
#[must_use]
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn build_from(
    from: &Source,
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    form: GrowthForm,
    templates: &Templates,
    levels: &[LodSpec; 4],
    tier: &CardTier,
) -> CardPlant {
    let height = graph.height.max(0.01);
    let crowned = matches!(
        form,
        GrowthForm::ExcurrentTree
            | GrowthForm::DecurrentTree
            | GrowthForm::ScaleLeavedTree
            | GrowthForm::Shrub
    );
    // A crown is a heap of leafy masses, round from every side. A
    // conifer's too: its cards facing out along its tiers were seen edge
    // on from the side, and a Douglas fir's and a redcedar's card tiers
    // hid a quarter less of the sky than the full tree (`plantc tiers`
    // cover); turned to the eye, about as much.
    let turning = crowned;
    // A climber's thin stems lie under its leaves on its host: drawn a
    // texel wide they would cover them, and ivy's cards came out brown.
    let climbing = form == GrowthForm::Vine;
    let coarse_at = |keep: f64| {
        mesh::build_wood(
            graph,
            looks,
            bodies,
            appearance,
            &LodSpec {
                min_radius: keep,
                min_sides: 3,
                max_sides: tier.sides,
                bend: tier.bend,
                ..levels[3].for_height(height)
            },
            3,
        )
    };
    // However many triangles it takes, the trunk is kept, and what is as
    // thick as half of it: a plant whose every part is fleshy (a cholla)
    // baked whole falls apart into scraps.
    let trunk = graph
        .segments
        .iter()
        .filter(|s| s.parent.is_none())
        .map(|s| s.radius)
        .fold(0.0, f64::max)
        * 0.5;
    // A fleshy body off the stem (a cholla's joints, a prickly pear's
    // pads) thinner than `keep` and than half the trunk is baked as a
    // solid organ is, its spines with it: a cholla's thousand joints
    // drawn whole took 24,000 triangles, a hundred times the tubes'
    // budget. The stem's own bodies (a saguaro's column) stay whole.
    let thin_body = |mesh: &Mesh, i: usize, keep: f64| {
        mesh.segment(i)
            .and_then(|s| graph.segments.get(s as usize))
            .is_some_and(|s| s.body != 0 && s.order > 0 && s.radius < keep.min(trunk))
    };
    // The bodies kept however thick `keep` grows.
    let fixed_body = |mesh: &Mesh, i: usize| {
        mesh.segment(i)
            .and_then(|s| graph.segments.get(s as usize))
            .is_some_and(|s| s.body != 0 && (s.order == 0 || s.radius >= trunk))
    };
    // Tubes take at most two thirds as many triangles as the cards:
    // thicker wood is kept first, and the rest is baked into the cards.
    let wood_budget = tier.cards(height) * (CORNERS - 2) * 2 / 3;
    let mut keep = height * tier.keep;
    let mut coarse = coarse_at(keep);
    let mut tube_triangles = 0;
    for _ in 0..12 {
        let drawn = (0..coarse.wood.triangle_count())
            .filter(|t| {
                (0..3).any(|k| {
                    let i = coarse.wood.indices[t * 3 + k] as usize;
                    bark_radius(&coarse.wood, i).map_or(
                        !thin_body(&coarse.wood, i, keep) && !fixed_body(&coarse.wood, i),
                        |r| r >= keep,
                    )
                })
            })
            .count();
        tube_triangles = drawn;
        if drawn <= wood_budget {
            break;
        }
        keep *= 1.5;
        coarse = coarse_at(keep);
    }
    let (source, sky) = (&from.mesh, &from.sky);

    // Elements: organ cards, then thin wood segments.
    let mut elements = Vec::new();
    let organ_cards: Vec<Card> = source.cards.clone();
    for card in &organ_cards {
        let (base, heading, left) = (v3(card.base), v3(card.heading), v3(card.left));
        let length = f64::from(card.length);
        elements.push(Element {
            centre: base + heading * (length * 0.5),
            facing: heading.cross(left).normalize_or(Vec3::Z),
            wood: false,
            planar: graph
                .segments
                .get(card.segment as usize)
                .is_some_and(|segment| segment.within.is_some()),
            area: (length * f64::from(card.width)).max(1e-8),
            half: heading * (length * 0.5),
        });
    }
    // Free-standing thin axes that hold up flowers or fruit (a grass's
    // culms, a forb's flowering stems) are ribbons in their own place,
    // not baked: baked onto a card they would stand off the card that
    // holds their flowers, and the flowers would float.
    // Each ribbon's head takes a card (the tier's `heads` share of them
    // at most), so that the rest of the plant keeps enough. A crown's
    // cone- and fruit-bearing twigs lie among its foliage, not free of
    // it: as ribbons their heads took a third of a Douglas fir's cards,
    // and its crown's cards covered 0.43 of their area rather than 0.54.
    let ribbons = ribbon_axes(
        graph,
        looks,
        if crowned { 0.0 } else { keep },
        wood_budget.saturating_sub(tube_triangles),
    )
    .into_iter()
    .take(if tier.heads > 0.0 {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let most = (tier.cards(height) as f64 * tier.heads) as usize;
        most
    } else {
        usize::MAX
    })
    .collect::<Vec<_>>();
    let mut ribboned = vec![false; graph.segments.len()];
    for axis in &ribbons {
        for &s in axis {
            ribboned[s] = true;
        }
    }
    // A leaf's own axis (a frond's stipe and rachis, a blade's midrib)
    // lies in its leaf's plane, so it faces the way its leaf does: the
    // area-weighted facing of the organs drawn in the same leaf. Facing
    // every way round itself, a fertile frond's erect stipe would join
    // the flat sterile fronds at its foot and be drawn on their level card,
    // where it is seen end on from the side and the frond floats.
    let leaf_of = |within: u32| {
        let mut at = within as usize;
        for _ in 0..64 {
            match graph.structures.get(at).and_then(|s| s.within) {
                Some(up) => at = up as usize,
                None => break,
            }
        }
        at
    };
    let mut leaf_facing = vec![[[0.0; 3]; 3]; graph.structures.len()];
    for (card, element) in organ_cards.iter().zip(&elements) {
        if let Some(within) = graph
            .segments
            .get(card.segment as usize)
            .and_then(|segment| segment.within)
        {
            let leaf = leaf_of(within);
            if let Some(m) = leaf_facing.get_mut(leaf) {
                add(m, outer(element.facing, element.area));
            }
        }
    }
    let leaf_normals: Vec<Option<Vec3>> = leaf_facing
        .iter()
        .map(|m| {
            let start = Vec3::new(m[0][0], m[1][1], m[2][2]) + Vec3::new(1e-3, 2e-3, 3e-3);
            (m[0][0] + m[1][1] + m[2][2] > 0.0).then(|| dominant(*m, start))
        })
        .collect();
    let leaf_normal = |within: u32| leaf_normals.get(leaf_of(within)).copied().flatten();
    let mut wood_element = vec![usize::MAX; graph.segments.len()];
    for (s, segment) in graph.segments.iter().enumerate() {
        if segment.radius >= keep || segment.body != 0 || ribboned[s] {
            continue;
        }
        let along = segment.end - segment.start;
        let length = along.length();
        if length <= 0.0 {
            continue;
        }
        wood_element[s] = elements.len();
        // Only where it does lie in that plane: a maidenhair's erect
        // stipe holds its blade level at its top, square to it.
        let leaf = segment
            .within
            .and_then(leaf_normal)
            .filter(|n| (along * (1.0 / length)).dot(*n).abs() <= IN_LEAF_PLANE);
        elements.push(Element {
            centre: (segment.start + segment.end) * 0.5,
            facing: leaf.unwrap_or(along * (1.0 / length)),
            wood: leaf.is_none(),
            planar: segment.within.is_some(),
            area: (length * segment.radius * 2.0).max(1e-8),
            half: along * 0.5,
        });
    }

    let thin = SegmentGrid::new(
        &graph.segments,
        graph
            .segments
            .iter()
            .enumerate()
            .filter(|(s, _)| wood_element[*s] != usize::MAX || ribboned[*s])
            .map(|(s, _)| s),
    );
    // Organs the plant draws solid at every level (an agave's thick
    // leaves) are in the wood: they are baked as leaves are, a few
    // triangles of one organ at a time, unless the tubes keep them or
    // they are drawn whole; so are thin fleshy bodies.
    // An organ's vertices are at the organ level of the wind hierarchy.
    let organ_at = |i: usize| source.wood.levels.get(i) == Some(&3);
    // The longest are drawn whole, in up to two thirds of the cards'
    // triangles, and as many fewer cards.
    let solid: Vec<usize> = (0..source.wood.triangle_count())
        .filter(|&t| {
            (0..3).all(|k| {
                let i = source.wood.indices[t * 3 + k] as usize;
                bark_radius(&source.wood, i).is_none() && organ_at(i)
            })
        })
        .collect();
    let (leaves, replaced) = whole_leaves(
        graph,
        looks,
        &source.wood,
        &solid,
        height * WHOLE_LEAF,
        tier.cards(height) * (CORNERS - 2) * 2 / 3,
    );
    let mut whole = vec![false; source.wood.triangle_count()];
    for t in replaced {
        whole[t] = true;
    }
    let mut solid_chunks: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut chunk: Vec<usize> = Vec::new();
    let mut chunk_segment = u32::MAX;
    let flush = |chunk: &mut Vec<usize>,
                 elements: &mut Vec<Element>,
                 chunks: &mut Vec<(usize, Vec<usize>)>| {
        if chunk.is_empty() {
            return;
        }
        let mut area = 0.0;
        let mut centre = Vec3::ZERO;
        let mut facing = Vec3::ZERO;
        let (mut lo, mut hi) = (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for &t in chunk.iter() {
            let [a, b, c] = [0, 1, 2]
                .map(|k| v3(source.wood.positions[source.wood.indices[t * 3 + k] as usize]));
            let n = (b - a).cross(c - a);
            let w = n.length() * 0.5;
            area += w;
            centre = centre + (a + b + c) * (w / 3.0);
            // Facing either way alike.
            facing = facing + if n.dot(facing) < 0.0 { -n } else { n };
            for p in [a, b, c] {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        if area > 0.0 {
            chunks.push((elements.len(), std::mem::take(chunk)));
            elements.push(Element {
                centre: centre * (1.0 / area),
                facing: facing.normalize_or(Vec3::Y),
                wood: false,
                planar: false,
                area,
                half: (hi - lo) * 0.5,
            });
        }
        chunk.clear();
    };
    for t in 0..source.wood.triangle_count() {
        let corners = [0, 1, 2].map(|k| source.wood.indices[t * 3 + k] as usize);
        if corners
            .iter()
            .any(|&i| bark_radius(&source.wood, i).is_some())
        {
            continue;
        }
        let body = thin_body(&source.wood, corners[0], keep);
        if whole[t] || (!body && !corners.iter().all(|&i| organ_at(i))) {
            continue;
        }
        let centre = corners
            .iter()
            .fold(Vec3::ZERO, |sum, &i| sum + v3(source.wood.positions[i]))
            * (1.0 / 3.0);
        // Near a thin axis baked with it, unless that is a ribbon (a
        // yarrow's leaves along its flowering stem).
        if !body
            && thin
                .nearest(centre, &graph.segments)
                .is_some_and(|(s, d)| d <= keep * LEAF_AXIS_REACH && wood_element[s] != usize::MAX)
        {
            continue;
        }
        let segment = source
            .wood
            .segments
            .get(corners[0])
            .copied()
            .unwrap_or(u32::MAX);
        if segment != chunk_segment || chunk.len() >= SOLID_CHUNK {
            flush(&mut chunk, &mut elements, &mut solid_chunks);
            chunk_segment = segment;
        }
        chunk.push(t);
    }
    flush(&mut chunk, &mut elements, &mut solid_chunks);

    // Tubes: the coarsest level's wood at least `keep` in radius, and
    // fleshy bodies on the stem or as thick.
    let mut wood = Mesh::default();
    for t in 0..coarse.wood.triangle_count() {
        let corners = [0, 1, 2].map(|k| coarse.wood.indices[t * 3 + k] as usize);
        // A coarse tube can run from a thick base to a fine tip in one
        // step, so a triangle is thin only where all its corners are.
        // A leaf's axis (a rachis) is drawn without bark; it is told from
        // a fleshy body by the thin segment it lies on.
        let centre = corners
            .iter()
            .fold(Vec3::ZERO, |sum, &i| sum + v3(coarse.wood.positions[i]))
            * (1.0 / 3.0);
        let thick = if corners
            .iter()
            .any(|&i| bark_radius(&coarse.wood, i).is_some())
        {
            corners
                .iter()
                .any(|&i| bark_radius(&coarse.wood, i).is_some_and(|r| r >= keep))
        } else if coarse
            .wood
            .segment(corners[0])
            .and_then(|s| graph.segments.get(s as usize))
            .is_some_and(|s| s.body != 0)
        {
            !thin_body(&coarse.wood, corners[0], keep)
        } else {
            thin.nearest(centre, &graph.segments).is_none_or(|(s, d)| {
                graph.segments[s].within.is_none() || d > keep * LEAF_AXIS_REACH
            })
        };
        if thick {
            copy_triangle(&mut wood, &coarse.wood, t);
        }
    }

    let k = tier
        .cards(height)
        .saturating_sub(leaves.triangle_count().div_ceil(CORNERS - 2))
        .max(1);
    // A ribbon's flowering head (a raceme, a spike, a panicle) is its own
    // card in its own place, as a grass's head is a card of its own in
    // games: baked onto a card shared with other parts it would stand off
    // its ribbon seen from the side, and float.
    let heads = {
        let mut axis_of = vec![usize::MAX; graph.segments.len()];
        for (a, axis) in ribbons.iter().enumerate() {
            for &s in axis {
                axis_of[s] = a;
            }
        }
        let mut bearing = vec![false; graph.segments.len()];
        for organ in &graph.organs {
            if let Some(s) = organ.segment
                && looks.get(usize::from(organ.organ)).is_some_and(signature)
            {
                bearing[s as usize] = true;
            }
        }
        let mut head = vec![usize::MAX; organ_cards.len()];
        for (i, card) in organ_cards.iter().enumerate() {
            let mut at = card.segment as usize;
            if !bearing.get(at).copied().unwrap_or(false) {
                continue;
            }
            let mut walked = 0.0;
            loop {
                if axis_of[at] != usize::MAX {
                    head[i] = axis_of[at];
                    break;
                }
                let g = &graph.segments[at];
                walked += (g.end - g.start).length();
                match g.parent {
                    Some(parent) if walked < graph.height * RIBBON_LENGTH => at = parent as usize,
                    _ => break,
                }
            }
        }
        if tier.heads <= 0.0 {
            head.fill(usize::MAX);
        }
        let mut used: Vec<usize> = head.iter().copied().filter(|&a| a != usize::MAX).collect();
        used.sort_unstable();
        used.dedup();
        head.iter()
            .map(|a| used.iter().position(|u| u == a))
            .collect::<Vec<Option<usize>>>()
    };
    let headed = heads.iter().flatten().copied().max().map_or(0, |h| h + 1);
    // Heads of flowers seen face on (a capitulum, a single flower, an
    // umbel or a corymb) look unlike from above and from the side.
    let faced: Vec<bool> = (0..headed)
        .map(|h| {
            organ_cards.iter().zip(&heads).any(|(card, head)| {
                *head == Some(h)
                    && looks.get(usize::from(card.template)).is_some_and(|look| {
                        matches!(
                            look.shape,
                            Shape::Umbel(_) | Shape::Head(_) | Shape::Flower(_)
                        )
                    })
            })
        })
        .collect();
    let rest: Vec<usize> = (0..elements.len())
        .filter(|&i| heads.get(i).copied().flatten().is_none())
        .collect();
    let points: Vec<[f64; 9]> = rest
        .iter()
        .map(|&i| features(&elements[i], height))
        .collect();
    let weights: Vec<f64> = rest.iter().map(|&i| elements[i].area).collect();
    let clustered = kmeans(&points, &weights, k.saturating_sub(headed).max(1));
    let mut group = vec![0; elements.len()];
    for (j, &i) in rest.iter().enumerate() {
        group[i] = headed + clustered[j];
    }
    for (i, h) in heads.iter().enumerate() {
        if let Some(h) = h {
            group[i] = *h;
        }
    }
    let groups = group.iter().copied().max().map_or(0, |g| g + 1);
    let foliage_centre = {
        let total: f64 = elements.iter().map(|e| e.area).sum::<f64>().max(1e-12);
        elements
            .iter()
            .fold(Vec3::ZERO, |sum, e| sum + e.centre * e.area)
            * (1.0 / total)
    };
    // Two last k-means steps against each group's own plane rather than
    // its parts' mean facing: a part lying edge on to its group's card
    // (an erect stipe among level fronds) would draw as a line or not at
    // all, so it moves to the group whose card shows it best nearby.
    for _ in 0..2 {
        let fits: Vec<Option<(Vec3, Vec3)>> = (0..groups)
            .map(|g| {
                let members: Vec<&Element> = elements
                    .iter()
                    .zip(&group)
                    .filter(|(_, at)| **at == g)
                    .map(|(e, _)| e)
                    .collect();
                (!members.is_empty()).then(|| {
                    let (centre, normal, _, _) = fit_plane(&members, foliage_centre, turning);
                    (centre, normal)
                })
            })
            .collect();
        for (i, e) in elements.iter().enumerate() {
            if heads.get(i).copied().flatten().is_some() {
                continue;
            }
            let misfit = |n: Vec3| {
                if e.planar && !e.wood {
                    1.0 - e.facing.dot(n).powi(2)
                } else if e.wood {
                    e.facing.dot(n).powi(2)
                } else {
                    0.0
                }
            };
            let best = fits
                .iter()
                .enumerate()
                .skip(headed)
                .filter_map(|(g, fit)| fit.map(|(c, n)| (g, c, n)))
                .map(|(g, c, n)| {
                    let d = (e.centre - c) * (1.0 / height);
                    (g, d.dot(d) + FACING * FACING * misfit(n))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            if let Some((g, _)) = best {
                group[i] = g;
            }
        }
    }

    // Each group's parts: its organ cards, and its thin wood's triangles.
    let mut group_cards: Vec<Vec<Card>> = vec![Vec::new(); groups];
    for (i, card) in organ_cards.iter().enumerate() {
        group_cards[group[i]].push(*card);
    }
    let mut group_wood: Vec<Mesh> = vec![Mesh::default(); groups];
    // Each thin segment's triangles, and its mean colour.
    // Each triangle, the thin segment it is baked with, and whether it
    // is bark (an organ's triangle, a yarrow's cut blade along its
    // rachis, is drawn however thin the rachis).
    let mut group_triangles: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); groups];
    let mut segment_colour = vec![([0.0_f64; 3], 0.0_f64); graph.segments.len()];
    for t in 0..source.wood.triangle_count() {
        let corners = [0, 1, 2].map(|k| source.wood.indices[t * 3 + k] as usize);
        let centre = corners
            .iter()
            .fold(Vec3::ZERO, |sum, &i| sum + v3(source.wood.positions[i]))
            * (1.0 / 3.0);
        let barked = corners
            .iter()
            .any(|&i| bark_radius(&source.wood, i).is_some());
        let radius = corners
            .iter()
            .filter_map(|&i| bark_radius(&source.wood, i))
            .fold(0.0, f64::max);
        if barked && radius >= keep {
            continue;
        }
        let Some((segment, d)) = thin.nearest(centre, &graph.segments) else {
            continue;
        };
        // A leaf drawn whole along its axis (a yarrow's cut blade) is
        // baked with that axis as far out as solid organs are left to it.
        if !barked && d > keep * LEAF_AXIS_REACH {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let segment = segment as u32;
        let (sum, count) = &mut segment_colour[segment as usize];
        for &i in &corners {
            for c in 0..3 {
                sum[c] += f64::from(source.wood.colors[i][c]);
            }
            *count += 1.0;
        }
        let Some(&e) = wood_element.get(segment as usize) else {
            continue;
        };
        if e == usize::MAX {
            continue;
        }
        copy_triangle(&mut group_wood[group[e]], &source.wood, t);
        group_triangles[group[e]].push((t, segment as usize, barked));
    }
    let mut group_solids: Vec<Vec<usize>> = vec![Vec::new(); groups];
    for (e, triangles) in &solid_chunks {
        for &t in triangles {
            copy_triangle(&mut group_wood[group[*e]], &source.wood, t);
            group_solids[group[*e]].push(t);
        }
    }

    // Planes.
    #[derive(Clone, Copy)]
    struct Plane {
        centre: Vec3,
        normal: Vec3,
        right: Vec3,
        up: Vec3,
        low: (f64, f64),
        high: (f64, f64),
        group: usize,
        flat: bool,
        turn: Turn,
        extent: [f64; 3],
        /// Baked from [`head_ways`] and drawn as an impostor.
        views: bool,
        /// Turned to the eye: baked from [`height_ways`] out along
        /// `level`, how far its parts spread along each one's right, up
        /// and way.
        heights: Option<(Vec3, [[f64; 3]; 3])>,
    }
    // Where the plant stands: a card leaning out from its parts would
    // show the ones behind it below there, under the ground.
    let floor = graph
        .segments
        .iter()
        .flat_map(|s| [s.start.y, s.end.y])
        .chain(graph.organs.iter().map(|o| o.position.y))
        .fold(f64::INFINITY, f64::min);
    // A frame's lowest row on a card whose up rises `up.y` a metre:
    // right is level, so only up moves it.
    let lowest = |centre: Vec3, up: Vec3, low: f64| {
        if floor.is_finite() && up.y > 1e-3 {
            low.max((floor - centre.y) / up.y)
        } else {
            low
        }
    };
    let mut planes = Vec::new();
    for g in 0..groups {
        let members: Vec<&Element> = elements
            .iter()
            .enumerate()
            .filter(|(i, _)| group[*i] == g)
            .map(|(_, e)| e)
            .collect();
        if members.is_empty() {
            continue;
        }
        let (centre, normal, flat, turn) = fit_plane(&members, foliage_centre, turning);
        let cards = PlantMesh {
            cards: group_cards[g].clone(),
            ..PlantMesh::default()
        }
        .card_mesh();
        let wood = std::mem::take(&mut group_wood[g]);
        if cards.indices.is_empty() && wood.indices.is_empty() {
            continue;
        }
        // A level card lies at the top of its parts, not their middle:
        // what lies on a surface (a water lily's pads on the water, a
        // rosette on the ground) is drawn on it, not into it.
        let centre = if normal.y.abs() >= LEVEL {
            let lift = cards
                .positions
                .iter()
                .chain(&wood.positions)
                .map(|p| v3(*p).y - centre.y)
                .fold(0.0, f64::max);
            centre + Vec3::Y * lift
        } else {
            centre
        };
        // Up is the world's up laid on the plane, where it can be.
        let up = (Vec3::Y - normal * Vec3::Y.dot(normal))
            .normalize_or_else(|| any_perpendicular(normal));
        let right = up.cross(normal);
        let mut low = (f64::INFINITY, f64::INFINITY);
        let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        // A crown's clump is as wide as its leaves: a coarse tube's
        // triangle near its twigs can reach metres past them, and the
        // card would be mostly empty.
        let bounded = crowned && !flat && g >= headed && !cards.indices.is_empty();
        let reach = if bounded {
            &[][..]
        } else {
            &wood.positions[..]
        };
        for p in cards.positions.iter().chain(reach) {
            let d = v3(*p) - centre;
            let (x, y) = (d.dot(right), d.dot(up));
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
        }
        low.1 = lowest(centre, up, low.1).min(high.1);
        // How far the clump spreads along the card's right, up and normal.
        let mut spread = [0.0; 3];
        let mut weight = 0.0;
        for e in &members {
            for p in [e.centre - e.half, e.centre, e.centre + e.half] {
                let d = p - centre;
                for (sum, axis) in spread.iter_mut().zip([right, up, normal]) {
                    *sum += e.area * d.dot(axis) * d.dot(axis);
                }
                weight += e.area;
            }
        }
        let extent = spread.map(|v| (v / weight.max(1e-12)).sqrt());
        // A head (a raceme, a spike, a panicle) is much alike round its
        // axis, and turns to the eye about its own middle on the stem,
        // so it stays on its ribbon from every side. One seen face on (a
        // yarrow's corymb, an umbel, a daisy) looks unlike from above and
        // from the side, a disc of flowers over a cone of rays or a cup
        // of bracts: lying flat it would be a line from the side, and
        // turned it would show the disc. It is drawn as a small impostor
        // ([`HEAD_AROUND`]).
        let views = g < headed && faced[g];
        let turn = if g < headed && !views {
            Turn::Facing
        } else {
            turn
        };
        // A card turned to the eye shows its parts as seen from the
        // height nearest the eye's ([`HEIGHTS`]); its right is level.
        let heights = (turn == Turn::Facing).then(|| {
            let level = right.cross(Vec3::Y).normalize_or(Vec3::Z);
            let ways = height_ways(level);
            let mut spread = [[0.0; 3]; 3];
            let mut weight = 0.0;
            for e in &members {
                for p in [e.centre - e.half, e.centre, e.centre + e.half] {
                    let d = p - centre;
                    for ((way, up), sums) in ways.iter().zip(&mut spread) {
                        for (sum, axis) in sums.iter_mut().zip([right, *up, *way]) {
                            *sum += e.area * d.dot(axis) * d.dot(axis);
                        }
                    }
                    weight += e.area;
                }
            }
            let weight = weight.max(1e-12);
            (level, spread.map(|sums| sums.map(|v| (v / weight).sqrt())))
        });
        planes.push(Plane {
            centre,
            normal,
            extent,
            right,
            up,
            low,
            high,
            group: g,
            flat,
            turn,
            views,
            heights,
        });
    }

    // Texels per metre, the same on every card.
    let area: f64 = planes
        .iter()
        .map(|p| ((p.high.0 - p.low.0) * (p.high.1 - p.low.1)).max(1e-8))
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let mut density = (tier.texels(height) as f64 / area).sqrt();

    // Nothing thinner than a texel: narrower blades are widened to one,
    // and thinner stems drawn as strips a texel wide in their own colour,
    // so a culm or a blade stays a line in the picture instead of
    // breaking into dots.
    let mut texel = 1.0 / density;
    let draw = |plane: &Plane, texel: f64, lit: bool| -> Baked {
        let g = plane.group;
        // In a crown's mass, as with its twigs, thin flowers and fruit
        // (catkins, cones) keep their own width: a texel wide they would
        // stand out round its edge. Its leaves and needles are what it is
        // made of, and stay at least a texel wide.
        let crown = crowned && !plane.flat && g >= headed;
        let wide = |card: &Card| {
            let mut card = *card;
            #[allow(clippy::cast_possible_truncation)]
            let least = (texel * MIN_TEXELS) as f32;
            let flower = looks.get(usize::from(card.template)).is_some_and(signature);
            if !(crown && flower) {
                card.width = card.width.max(least);
            }
            card
        };
        let widened: Vec<Card> = group_cards[g]
            .iter()
            .map(|card| {
                let mut card = wide(card);
                if !plane.flat {
                    // A clump's picture shows each leaf at least
                    // [`FACE_ON_TEXELS`] wide, turned toward face on as
                    // little as that takes, as crown cards are painted: a
                    // leaf seen edge on from the card would be thinner
                    // than a texel and the clump would read as bare
                    // stripes, while a broad leaf turned all the way
                    // would show wider than it is from most sides. Its
                    // place and size stay. It only turns about its own
                    // midrib, so its tip stays where it grows.
                    let heading = v3(card.heading).normalize_or(Vec3::Y);
                    let toward = (plane.normal - heading * plane.normal.dot(heading))
                        .normalize_or_else(|| any_perpendicular(heading));
                    let aside = heading.cross(toward);
                    let now = heading.cross(v3(card.left)).normalize_or(toward);
                    let width = f64::from(card.width).max(1e-9);
                    let least = (texel * FACE_ON_TEXELS / width).min(1.0);
                    let cos = now.dot(toward).abs();
                    if cos < least {
                        let sin = (1.0 - least * least).max(0.0).sqrt();
                        let side = if now.dot(aside) < 0.0 { -1.0 } else { 1.0 };
                        let normal = toward * least + aside * (sin * side);
                        // heading × left is the leaf's normal.
                        card.left = normal.cross(heading).normalize_or(v3(card.left)).to_f32();
                    }
                }
                card
            })
            .collect();
        let cards = PlantMesh {
            cards: widened,
            ..PlantMesh::default()
        }
        .card_mesh();
        let lit = (lit && !plane.flat).then(|| {
            // But each leaf keeps its own normal in the picture, so it
            // takes the light as it would in the crown: turned face on,
            // a clump's leaves would all face the sun together and the
            // sunny side of a crown would draw as bright flat plates.
            PlantMesh {
                cards: group_cards[g].iter().map(wide).collect(),
                ..PlantMesh::default()
            }
            .card_mesh()
        });
        let mut wood = Mesh::default();
        let mut strips = Vec::new();
        for &(t, segment, barked) in &group_triangles[g] {
            if !barked || graph.segments[segment].radius * 2.0 >= texel * MIN_TEXELS {
                copy_triangle(&mut wood, &source.wood, t);
            }
        }
        for &t in &group_solids[g] {
            copy_triangle(&mut wood, &source.wood, t);
        }
        // Every thin segment of the group thinner than a texel is a strip:
        // a tube's triangles span many segments, so not every segment has
        // its own.
        for (s, segment) in graph.segments.iter().enumerate() {
            let e = wood_element[s];
            // In a crown's mass the twigs lie among the leaves: drawn a
            // texel wide they would stand out round its edge, so they
            // keep their own width and only those as wide as a texel
            // show.
            if e != usize::MAX
                && group[e] == g
                && !crown
                && !climbing
                && segment.radius * 2.0 < texel * MIN_TEXELS
            {
                strips.push(s);
            }
        }
        for segment in strips {
            let s = &graph.segments[segment];
            // Its own colour, or the nearest coloured one it grows from.
            let mut from = segment;
            let mut steps = 0;
            while segment_colour[from].1 == 0.0 && steps < 256 {
                match graph.segments[from].parent {
                    Some(parent) => from = parent as usize,
                    None => break,
                }
                steps += 1;
            }
            let (sum, count) = segment_colour[from];
            let colour = if count > 0.0 {
                sum.map(|c| c / count)
            } else {
                [
                    f64::from(appearance.bark[0]),
                    f64::from(appearance.bark[1]),
                    f64::from(appearance.bark[2]),
                ]
            };
            let along = s.end - s.start;
            let side = along
                .cross(plane.normal)
                .normalize_or_else(|| any_perpendicular(along))
                * (texel * MIN_TEXELS * 0.5);
            let first = u32::try_from(wood.positions.len()).unwrap_or(u32::MAX);
            for corner in [s.start - side, s.start + side, s.end + side, s.end - side] {
                #[allow(clippy::cast_possible_truncation)]
                wood.push(Vertex {
                    position: corner,
                    normal: plane.normal,
                    uv: [0.0, 0.0],
                    color: [colour[0] as f32, colour[1] as f32, colour[2] as f32, 1.0],
                    born: s.born,
                    shed: s.shed,
                    level: 1,
                    segment: mesh::NO_SEGMENT,
                });
            }
            // Both windings: the strip shows from either side.
            wood.indices.extend_from_slice(&[
                first,
                first + 1,
                first + 2,
                first,
                first + 2,
                first + 3,
                first,
                first + 2,
                first + 1,
                first,
                first + 3,
                first + 2,
            ]);
        }
        Baked { cards, lit, wood }
    };
    // Twice, the second time at the density of the bounds the first
    // drew: leaves widened to a texel, and the thin wood left out, change
    // them.
    for pass in 0..2 {
        if pass == 1 {
            let area: f64 = planes
                .iter()
                .map(|p| ((p.high.0 - p.low.0) * (p.high.1 - p.low.1)).max(1e-8))
                .sum();
            #[allow(clippy::cast_precision_loss)]
            {
                density = (tier.texels(height) as f64 / area).sqrt();
            }
            texel = 1.0 / density;
        }
        for plane in &mut planes {
            let crown = crowned && !plane.flat && plane.group >= headed;
            let drawn = draw(plane, texel, false);
            let mut low = (f64::INFINITY, f64::INFINITY);
            let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            let reach = if crown && !drawn.cards.indices.is_empty() {
                &[][..]
            } else {
                &drawn.wood.positions[..]
            };
            // A turned card's frame holds its parts from every height, the
            // same about its middle in each, so one outline serves all.
            let ups: Vec<Vec3> = match plane.heights {
                Some((level, _)) => height_ways(level).iter().map(|w| w.1).collect(),
                None => vec![plane.up],
            };
            for p in drawn.cards.positions.iter().chain(reach) {
                let d = v3(*p) - plane.centre;
                let x = d.dot(plane.right);
                for up in &ups {
                    let y = d.dot(*up);
                    low = (low.0.min(x), low.1.min(y));
                    high = (high.0.max(x), high.1.max(y));
                }
            }
            if plane.heights.is_none() {
                low.1 = lowest(plane.centre, plane.up, low.1).min(high.1);
            }
            plane.low = low;
            plane.high = high;
        }
    }

    // The pictures' margins and rounding are paid from the same budget.
    let budget = tier.texels(height);
    let mut tile_density = density;
    for _ in 0..8 {
        let total: usize = planes
            .iter()
            .map(|p| {
                let (w, h) = (p.high.0 - p.low.0, p.high.1 - p.low.1);
                if p.views {
                    let (side, _) = tile(w.max(h), w.max(h), tile_density, tier.most_texels);
                    side * side * head_ways().len()
                } else {
                    let (tw, th) = tile(w, h, tile_density, tier.most_texels);
                    tw * th
                }
            })
            .sum();
        if total <= budget {
            break;
        }
        #[allow(clippy::cast_precision_loss)]
        let shrink = (budget as f64 / total as f64).sqrt() * 0.98;
        tile_density *= shrink;
    }
    let mut out = CardPlant {
        wood,
        ..CardPlant::default()
    };
    for axis in &ribbons {
        ribbon(
            &mut out.wood,
            graph,
            axis,
            &segment_colour,
            appearance,
            texel * MIN_TEXELS,
        );
    }
    // Heads seen face on are baked from the full model, where an umbel or
    // a corymb is drawn flower by flower on its rays: the coarser level
    // draws it as one flat card, a line from the side.
    let fine = planes.iter().any(|p| p.views).then(|| {
        let fine = mesh::build(
            graph,
            looks,
            bodies,
            appearance,
            &LodSpec {
                min_radius: 0.0,
                ..levels[0].for_height(height)
            },
            0,
        );
        let cards = fine.card_mesh();
        (fine.wood, cards)
    });
    // Each card is baked on its own, on as many cores as are idle, and
    // they are joined in order: the same pictures however many bake them.
    let bake_plane = |plane: &Plane| -> (CardPlant, f64, f64) {
        let mut out = CardPlant::default();
        let drawn_texel = texel;
        let (w, h) = (plane.high.0 - plane.low.0, plane.high.1 - plane.low.1);
        if plane.views {
            let drawn = draw(plane, texel, true);
            let (side, _) = tile(w.max(h), w.max(h), tile_density, tier.most_texels);
            let mid = plane.centre
                + plane.right * ((plane.low.0 + plane.high.0) * 0.5)
                + plane.up * ((plane.low.1 + plane.high.1) * 0.5);
            let drawn = match &fine {
                Some((wood, cards)) => near_head(&drawn, mid, wood, cards),
                None => drawn,
            };
            let (covered, area) = add_views(&mut out, &drawn, mid, side, templates, &sky);
            return (out, covered, area);
        }
        let (tw, th) = tile(w, h, tile_density, tier.most_texels);
        // The picture's frame: the group's extent, grown to the texels'
        // aspect.
        #[allow(clippy::cast_precision_loss)]
        let texel = (w / (tw - 2).max(1) as f64).max(h / (th - 2).max(1) as f64);
        #[allow(clippy::cast_precision_loss)]
        let (fw, fh) = (texel * tw as f64, texel * th as f64);
        if let Some((level, extents)) = plane.heights {
            // Each drawn for its own way: its leaves turned toward face on
            // and its thin stems laid across as seen from there. Their
            // light, from the leaves as they grow, is the same for all.
            let mut first = true;
            let drawn = height_ways(level).map(|(way, _)| {
                draw(
                    &Plane {
                        normal: way,
                        ..*plane
                    },
                    drawn_texel,
                    std::mem::take(&mut first),
                )
            });
            let (covered, area) = add_heights(
                &mut out,
                &drawn,
                &Frame {
                    centre: plane.centre,
                    level,
                    right: plane.right,
                    low: plane.low,
                    high: plane.high,
                    size: (fw, fh),
                    texels: (tw, th),
                },
                extents,
                templates,
                &sky,
            );
            return (out, covered, area);
        }
        let drawn = draw(plane, drawn_texel, true);
        let mid = plane.centre
            + plane.right * ((plane.low.0 + plane.high.0) * 0.5)
            + plane.up * ((plane.low.1 + plane.high.1) * 0.5);
        let mut picture = bake(
            &Shot {
                cards: &drawn.cards,
                lit: drawn.lit.as_ref(),
                wood: &drawn.wood,
                strips: None,
                mid,
                right: plane.right,
                up: plane.up,
                normal: plane.normal,
                size: (fw, fh),
                texels: (tw, th),
            },
            templates,
            &sky,
        );
        picture.turn = plane.turn;
        #[allow(clippy::cast_possible_truncation)]
        {
            picture.extent = plane.extent.map(|v| v as f32);
        }
        keep_coverage(&mut picture);
        fill_empty(&mut picture);
        let Some(outline) = outline(&picture) else {
            return (out, 0.0, 0.0);
        };
        let index = out.pictures.len();
        #[allow(clippy::cast_precision_loss)]
        let index_alpha = index as f32;
        let first = u32::try_from(out.cards.positions.len()).unwrap_or(u32::MAX);
        let corner_world =
            |(u, v): (f64, f64)| mid + plane.right * ((u - 0.5) * fw) + plane.up * ((v - 0.5) * fh);
        for &(u, v) in &outline {
            #[allow(clippy::cast_possible_truncation)]
            out.cards.push(Vertex {
                position: corner_world((u, v)),
                normal: plane.normal,
                uv: [u as f32, v as f32],
                color: [1.0, 1.0, 1.0, index_alpha],
                born: 0.0,
                shed: None,
                level: 3,
                segment: mesh::NO_SEGMENT,
            });
        }
        let n = u32::try_from(outline.len()).unwrap_or(0);
        for i in 1..n.saturating_sub(1) {
            out.cards
                .indices
                .extend_from_slice(&[first, first + i, first + i + 1]);
        }
        let poly_area = polygon_area(&outline) * fw * fh;
        #[allow(clippy::cast_precision_loss)]
        let covered = picture.color.iter().filter(|c| c[3] >= 0.5).count() as f64 * texel * texel;
        out.stats.texels += tw * th;
        out.stats.seen_texels += tw * th;
        // A card turns about the middle of its parts: a skirt about its
        // trunk, whatever its outline's shape.
        picture.pivot = plane.centre.to_f32();
        out.pictures.push(picture);
        (out, covered, poly_area)
    };
    let lease = cores::Lease::take(planes.len().min(cores::per_task()));
    let baked = cores::map(planes.iter().collect(), lease.threads(), &bake_plane);
    drop(lease);
    let mut covered_area = 0.0;
    let mut card_area = 0.0;
    for (part, covered, area) in baked {
        join_cards(&mut out, part);
        covered_area += covered;
        card_area += area;
    }
    out.stats.cards = out
        .pictures
        .iter()
        .enumerate()
        .filter(|(i, p)| match p.turn {
            Turn::Views { first, .. } | Turn::Heights { first, .. } => first == *i,
            _ => true,
        })
        .count();
    // A ribbon's quads are written with both windings; a renderer draws
    // them once, two-sided.
    out.stats.wood_triangles = out.wood.triangle_count() - ribbons.len() * (RIBBON_POINTS - 1) * 2;
    out.stats.card_triangles = out.cards.triangle_count();
    out.stats.leaf_triangles = leaves.triangle_count();
    for t in 0..leaves.triangle_count() {
        copy_triangle(&mut out.wood, &leaves, t);
    }
    out.stats.coverage = if card_area > 0.0 {
        covered_area / card_area
    } else {
        0.0
    };
    out
}

/// Keep a picture's coverage where it is, through the alpha test
/// (coverage at least one half). A leaf lobe, a bristle or a moss thread
/// thinner than a texel covers each of its texels only in part, and would
/// fail the test and vanish; a stalk a texel wide would break into dots.
///
/// The texels below the cut and not touching one above it are dithered
/// by error diffusion (Floyd and Steinberg 1976, serpentine): each turns
/// fully on once the coverage it and its neighbours have carried reaches
/// one half, and passes what it kept or overshot on. Each patch of thin
/// parts then keeps as many texels as its parts cover, drawn where they
/// cover most: lace stays lace and a cloud of spikelets stays a cloud,
/// where one scale for the whole picture (Castaño 2010, "Computing alpha
/// mipmaps") either drops the faint parts or fills the dense ones in.
/// Edges of parts the texels resolve are left alone, so outlines stay
/// smooth. Only texels a part covers turn on: an empty texel has no
/// colour, and only carries coverage across a gap.
fn keep_coverage(picture: &mut Texture) {
    let (w, h) = (picture.width, picture.height);
    let cut = 0.5_f32;
    let on: Vec<bool> = picture.color.iter().map(|c| c[3] >= cut).collect();
    let touches = |x: usize, y: usize| {
        (x > 0 && on[y * w + x - 1])
            || (x + 1 < w && on[y * w + x + 1])
            || (y > 0 && on[(y - 1) * w + x])
            || (y + 1 < h && on[(y + 1) * w + x])
    };
    let free: Vec<bool> = (0..w * h)
        .map(|i| !on[i] && !touches(i % w, i / w))
        .collect();
    let mut carried = vec![0.0_f32; w * h];
    for y in 0..h {
        let back = y % 2 == 1;
        for k in 0..w {
            let x = if back { w - 1 - k } else { k };
            let at = y * w + x;
            if !free[at] {
                continue;
            }
            let alpha = picture.color[at][3];
            let value = alpha + carried[at];
            let left = if value >= cut && alpha > 0.0 {
                picture.color[at][3] = 1.0;
                value - 1.0
            } else {
                value
            };
            // Ahead in this row 7/16, and below: behind 3/16, under 5/16,
            // ahead 1/16; shared among those still free.
            #[allow(clippy::cast_possible_wrap)]
            let (xi, step) = (x as i64, if back { -1 } else { 1 });
            let mut to = [(0usize, 0.0_f32); 4];
            let mut count = 0;
            let mut weight = 0.0;
            for (dx, dy, share) in [(step, 0, 7.0), (-step, 1, 3.0), (0, 1, 5.0), (step, 1, 1.0)] {
                let (nx, ny) = (xi + dx, y + dy);
                #[allow(clippy::cast_possible_wrap)]
                if nx < 0 || nx >= w as i64 || ny >= h {
                    continue;
                }
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                let n = ny * w + nx as usize;
                if free[n] {
                    to[count] = (n, share);
                    count += 1;
                    weight += share;
                }
            }
            for &(n, share) in &to[..count] {
                carried[n] += left * share / weight;
            }
        }
    }
}

/// A ribbon's points along its axis at most.
const RIBBON_POINTS: usize = 4;

/// A leaf's axis lies in its leaf's plane, and faces as the leaf does,
/// when it leans at most this far out of it (the sine: 30 degrees).
pub const IN_LEAF_PLANE: f64 = 0.5;

/// A clump is a mass of leaves, whose card turns to the eye, when it
/// spans at least this many leaf lengths.
pub const MASS: f64 = 4.0;

/// A card whose normal is at least this close to straight up (within
/// about 25°) is level.
pub const LEVEL: f64 = 0.9;

/// A clump is a sheet, and takes its own closest plane, when its parts lie
/// at most this share as far from that plane as from the plane facing out.
pub const SHEET: f64 = 0.5;

/// Axes this share of the plant's height or longer may be ribbons.
const RIBBON_LENGTH: f64 = 0.1;

/// The free-standing thin axes that bear a flower, a head or a fruit
/// along them, longest first, as many as `budget` triangles allow
/// (`2 (RIBBON_POINTS - 1)` each): each its segments, base first.
fn ribbon_axes(graph: &PlantGraph, looks: &[Look], keep: f64, budget: usize) -> Vec<Vec<usize>> {
    let n = graph.segments.len();
    let mut next = vec![None; n];
    for (s, segment) in graph.segments.iter().enumerate() {
        if let Some(parent) = segment.parent
            && !segment.lateral
        {
            next[parent as usize] = Some(s);
        }
    }
    let mut signature = vec![false; n];
    for organ in &graph.organs {
        if let Some(s) = organ.segment
            && looks
                .get(usize::from(organ.organ))
                .is_some_and(self::signature)
        {
            // Its own segment and those it hangs from within a short
            // reach: a flower on a pedicel or a raceme's side branch is
            // borne by the stem it grows from.
            let mut at = s as usize;
            let mut walked = 0.0;
            loop {
                signature[at] = true;
                let g = &graph.segments[at];
                walked += (g.end - g.start).length();
                match g.parent {
                    Some(parent) if walked < graph.height * RIBBON_LENGTH => at = parent as usize,
                    _ => break,
                }
            }
        }
    }
    let mut axes: Vec<(f64, Vec<usize>)> = Vec::new();
    for (start, segment) in graph.segments.iter().enumerate() {
        if !(segment.lateral || segment.parent.is_none()) {
            continue;
        }
        let mut chain = vec![start];
        let mut cursor = start;
        while let Some(following) = next[cursor] {
            chain.push(following);
            cursor = following;
        }
        let thin = chain.iter().all(|&s| {
            let g = &graph.segments[s];
            g.radius < keep && g.body == 0 && g.within.is_none()
        });
        let bears = chain.iter().any(|&s| signature[s]);
        let length: f64 = chain
            .iter()
            .map(|&s| (graph.segments[s].end - graph.segments[s].start).length())
            .sum();
        if thin && bears && length >= graph.height * RIBBON_LENGTH {
            axes.push((length, chain));
        }
    }
    axes.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1[0].cmp(&b.1[0])));
    let each = 2 * (RIBBON_POINTS - 1);
    axes.into_iter()
        .take(budget / each)
        .map(|(_, chain)| chain)
        .collect()
}

/// Draw `axis` as a ribbon at least `least` wide, facing out from the
/// plant's middle, both faces.
fn ribbon(
    out: &mut Mesh,
    graph: &PlantGraph,
    axis: &[usize],
    colours: &[([f64; 3], f64)],
    appearance: &Appearance,
    least: f64,
) {
    let mut path: Vec<(Vec3, f64)> = vec![(
        graph.segments[axis[0]].start,
        graph.segments[axis[0]].radius,
    )];
    for &s in axis {
        path.push((graph.segments[s].end, graph.segments[s].radius));
    }
    // Evenly spaced points along it.
    let picked: Vec<(Vec3, f64)> = (0..RIBBON_POINTS)
        .map(|i| {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let at = ((i as f64 / (RIBBON_POINTS - 1) as f64) * (path.len() - 1) as f64).round()
                as usize;
            path[at.min(path.len() - 1)]
        })
        .collect();
    let (mut sum, mut count) = ([0.0; 3], 0.0);
    for &s in axis {
        for c in 0..3 {
            sum[c] += colours[s].0[c];
        }
        count += colours[s].1;
    }
    let colour = if count > 0.0 {
        sum.map(|c| c / count)
    } else {
        appearance.bark.map(f64::from)
    };
    let first = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
    for (i, &(p, r)) in picked.iter().enumerate() {
        let along = if i + 1 < picked.len() {
            picked[i + 1].0 - p
        } else {
            p - picked[i - 1].0
        };
        let out_dir = Vec3::new(p.x, 0.0, p.z).normalize_or(Vec3::Z);
        let side = along
            .cross(out_dir)
            .normalize_or_else(|| any_perpendicular(along))
            * (r.max(least * 0.5));
        let normal = side.cross(along).normalize_or(out_dir);
        for corner in [p - side, p + side] {
            #[allow(clippy::cast_possible_truncation)]
            out.push(Vertex {
                position: corner,
                normal,
                uv: [0.0, 0.0],
                color: [colour[0] as f32, colour[1] as f32, colour[2] as f32, 1.0],
                born: 0.0,
                shed: None,
                level: 1,
                segment: mesh::NO_SEGMENT,
            });
        }
    }
    for i in 0..u32::try_from(RIBBON_POINTS - 1).unwrap_or(0) {
        let (a, b, c, d) = (
            first + 2 * i,
            first + 2 * i + 1,
            first + 2 * i + 2,
            first + 2 * i + 3,
        );
        out.indices
            .extend_from_slice(&[a, b, d, a, d, c, a, d, b, a, c, d]);
    }
}

/// Thin wood segments in a grid, to find the one a wood triangle belongs
/// to (tubes do not record their segments).
struct SegmentGrid {
    cell: f64,
    cells: std::collections::HashMap<(i64, i64, i64), Vec<usize>>,
}

impl SegmentGrid {
    fn key(&self, p: Vec3) -> (i64, i64, i64) {
        (
            crate::math::floor_index(p.x / self.cell),
            crate::math::floor_index(p.y / self.cell),
            crate::math::floor_index(p.z / self.cell),
        )
    }

    fn new(segments: &[crate::graph::GraphSegment], thin: impl Iterator<Item = usize>) -> Self {
        let thin: Vec<usize> = thin.collect();
        let cell = thin
            .iter()
            .map(|&s| (segments[s].end - segments[s].start).length())
            .fold(1e-3, f64::max);
        let mut grid = Self {
            cell,
            cells: std::collections::HashMap::new(),
        };
        for s in thin {
            let key = grid.key((segments[s].start + segments[s].end) * 0.5);
            grid.cells.entry(key).or_default().push(s);
        }
        grid
    }

    /// The nearest thin segment to `p` and how far `p` is from its
    /// surface.
    fn nearest(&self, p: Vec3, segments: &[crate::graph::GraphSegment]) -> Option<(usize, f64)> {
        let (x, y, z) = self.key(p);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.cells.get(&(x + dx, y + dy, z + dz)) else {
                        continue;
                    };
                    for &s in list {
                        let (a, b) = (segments[s].start, segments[s].end);
                        let ab = b - a;
                        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
                        let d = (a + ab * t - p).length() - segments[s].radius;
                        if best.is_none_or(|(bd, bs)| d < bd || (d == bd && s < bs)) {
                            best = Some((d, s));
                        }
                    }
                }
            }
        }
        best.map(|(d, s)| (s, d))
    }
}

/// Texels across and up a picture of a `w × h` m frame at `density`
/// texels a metre, with a texel of margin round it, at most `most` and at
/// least 4 along a side.
/// A plant drawn far off as one card that shows it baked from the
/// nearest of a few ways: the last level of detail, as `SpeedTree`'s
/// trees "fade into a billboard image" far from the camera (`SpeedTree`
/// runtime level-of-detail guide) and Unity's billboard asset holds
/// "rendered images of the 3D tree at different view angles".
#[derive(Debug, Clone, Copy)]
pub struct ImpostorTier {
    pub name: &'static str,
    /// Pictures round the plant at each height of eye: level and from
    /// 45 degrees above; one more from straight above.
    pub around: usize,
    /// Texels along each side of a picture.
    pub texels: usize,
}

/// A plant a few dozen pixels tall.
pub const FAR: ImpostorTier = ImpostorTier {
    name: "far-impostor",
    around: 8,
    texels: 128,
};

/// Build `graph` as an impostor ([`ImpostorTier`]), baked from the same
/// full plant as [`build`]'s cards.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn impostor(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    form: GrowthForm,
    templates: &Templates,
    levels: &[LodSpec; 4],
    tier: &ImpostorTier,
) -> CardPlant {
    let from = Source::new(graph, looks, bodies, appearance, templates, levels);
    impostor_from(&from, graph, looks, appearance, form, templates, tier)
}

/// [`impostor`] from `graph`'s [`Source`].
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn impostor_from(
    from: &Source,
    graph: &PlantGraph,
    looks: &[Look],
    appearance: &Appearance,
    form: GrowthForm,
    templates: &Templates,
    tier: &ImpostorTier,
) -> CardPlant {
    let (source, sky) = (&from.mesh, &from.sky);
    let grown = source.card_mesh();
    let items = [(&source.wood, Material::Opaque), (&grown, Material::Card)];
    let Some((low, high)) = raster::bounds(&items) else {
        return CardPlant::default();
    };
    let centre = (low + high) * 0.5;
    let radius = grown
        .positions
        .iter()
        .chain(&source.wood.positions)
        .map(|p| (v3(*p) - centre).length())
        .fold(1e-3, f64::max);
    // Nothing thinner than a texel, as on the cards ([`build`]), in a
    // herb, a grass or a fern: leaves are widened to one, and thin stems
    // (a grass's culms) drawn as strips a texel wide turned to each way,
    // or the heads would float. A crown, a texel there tens of
    // centimetres, is drawn as it grows: widened, its twigs would ring it
    // in a halo and its leaves would fill it in solid.
    let crowned = matches!(
        form,
        GrowthForm::ExcurrentTree
            | GrowthForm::DecurrentTree
            | GrowthForm::ScaleLeavedTree
            | GrowthForm::Shrub
    );
    #[allow(clippy::cast_precision_loss)]
    let texel = radius * 2.0 / tier.texels as f64;
    // A tree's cards are gigabytes: kept as they grew, not copied.
    let cards = if crowned {
        grown
    } else {
        drop(grown);
        PlantMesh {
            cards: source
                .cards
                .iter()
                .map(|card| {
                    let mut card = *card;
                    #[allow(clippy::cast_possible_truncation)]
                    let least = (texel * MIN_TEXELS) as f32;
                    card.width = card.width.max(least);
                    card
                })
                .collect(),
            ..PlantMesh::default()
        }
        .card_mesh()
    };
    let mut colour = vec![([0.0_f64; 3], 0.0_f64); graph.segments.len()];
    for (i, c) in source.wood.colors.iter().enumerate() {
        if let Some(s) = source.wood.segment(i)
            && let Some((sum, count)) = colour.get_mut(s as usize)
        {
            for k in 0..3 {
                sum[k] += f64::from(c[k]);
            }
            *count += 1.0;
        }
    }
    // Only the free-standing stems that hold up flowers or seed heads
    // ([`ribbon_axes`]): a vine's stems lie under its leaves, and drawn a
    // texel wide they would cover them.
    let thin: Vec<(usize, [f32; 3])> = if crowned {
        Vec::new()
    } else {
        ribbon_axes(graph, looks, texel * MIN_TEXELS * 0.5, usize::MAX)
            .into_iter()
            .flatten()
            .filter(|&i| graph.segments[i].end != graph.segments[i].start)
            .map(|i| {
                let mut from = i;
                for _ in 0..256 {
                    match graph.segments[from].parent {
                        Some(parent) if colour[from].1 == 0.0 => from = parent as usize,
                        _ => break,
                    }
                }
                let (sum, count) = colour[from];
                #[allow(clippy::cast_possible_truncation)]
                let c = if count > 0.0 {
                    sum.map(|c| (c / count) as f32)
                } else {
                    [appearance.bark[0], appearance.bark[1], appearance.bark[2]]
                };
                (i, c)
            })
            .collect()
    };
    let strips_facing = |way: Vec3| {
        let mut strips = Mesh::default();
        for &(i, c) in &thin {
            let s = &graph.segments[i];
            let along = s.end - s.start;
            let side = along
                .cross(way)
                .normalize_or_else(|| any_perpendicular(along))
                * (texel * MIN_TEXELS * 0.5);
            let first = u32::try_from(strips.positions.len()).unwrap_or(u32::MAX);
            for corner in [s.start - side, s.start + side, s.end + side, s.end - side] {
                strips.push(Vertex {
                    position: corner,
                    normal: way,
                    uv: [0.0, 0.0],
                    color: [c[0], c[1], c[2], 1.0],
                    born: s.born,
                    shed: s.shed,
                    level: 1,
                    segment: mesh::NO_SEGMENT,
                });
            }
            strips.indices.extend_from_slice(&[
                first,
                first + 1,
                first + 2,
                first,
                first + 2,
                first + 3,
                first,
                first + 2,
                first + 1,
                first,
                first + 3,
                first + 2,
            ]);
        }
        strips
    };
    let ways = ways_round(tier.around);
    let count = ways.len();
    let mut out = CardPlant::default();
    let view = |way: Vec3| -> Texture {
        let up = (Vec3::Y - way * Vec3::Y.dot(way)).normalize_or_else(|| Vec3::Z);
        let strips = strips_facing(way);
        let mut picture = bake(
            &Shot {
                cards: &cards,
                lit: None,
                wood: &source.wood,
                strips: Some(&strips),
                mid: centre,
                right: up.cross(way),
                up,
                normal: way,
                size: (radius * 2.0, radius * 2.0),
                texels: (tier.texels, tier.texels),
            },
            templates,
            &sky,
        );
        keep_coverage(&mut picture);
        fill_empty(&mut picture);
        picture.turn = Turn::Views { first: 0, count };
        picture.pivot = centre.to_f32();
        #[allow(clippy::cast_possible_truncation)]
        {
            picture.extent = [radius as f32; 3];
        }
        picture
    };
    let lease = cores::Lease::take(count.min(cores::per_task()));
    out.pictures = cores::map(ways, lease.threads(), &view);
    drop(lease);
    out.stats.texels = count * tier.texels * tier.texels;
    out.stats.seen_texels = out.stats.texels;
    // One card, baked facing the first way, round what every picture
    // covers; it turns as it is drawn.
    let mut union = out.pictures[0].clone();
    for picture in &out.pictures[1..] {
        for (to, from) in union.color.iter_mut().zip(&picture.color) {
            to[3] = to[3].max(from[3]);
        }
    }
    let Some(outline) = outline(&union) else {
        return CardPlant::default();
    };
    let way = Vec3::Z;
    let right = Vec3::Y.cross(way);
    for &(u, v) in &outline {
        #[allow(clippy::cast_possible_truncation)]
        out.cards.push(Vertex {
            position: centre
                + right * ((u - 0.5) * radius * 2.0)
                + Vec3::Y * ((v - 0.5) * radius * 2.0),
            normal: way,
            uv: [u as f32, v as f32],
            color: [1.0, 1.0, 1.0, 0.0],
            born: 0.0,
            shed: None,
            level: 3,
            segment: mesh::NO_SEGMENT,
        });
    }
    let n = u32::try_from(outline.len()).unwrap_or(0);
    for i in 1..n.saturating_sub(1) {
        out.cards.indices.extend_from_slice(&[0, i, i + 1]);
    }
    out.stats.cards = 1;
    out.stats.card_triangles = out.cards.triangle_count();
    #[allow(clippy::cast_precision_loss)]
    let covered = out
        .pictures
        .iter()
        .map(|p| p.color.iter().filter(|c| c[3] >= 0.5).count())
        .sum::<usize>() as f64;
    #[allow(clippy::cast_precision_loss)]
    {
        out.stats.coverage = covered / out.stats.texels.max(1) as f64;
    }
    out
}

/// What a plane's picture is baked from: its cards, the cards as they
/// grew, not turned to face the card, where the picture's normals and its
/// parts' places are read from (clumps only, and only when `lit`), and its
/// wood. Drawn again for each use and dropped: a tree's are gigabytes.
struct Baked {
    cards: Mesh,
    lit: Option<Mesh>,
    wood: Mesh,
}

/// Pictures round a flat-topped head at each height of eye, level and
/// from 45 degrees above; one more from straight above. A head is much
/// alike round its axis, so a few do.
pub const HEAD_AROUND: usize = 4;

/// The ways a flat-topped head is baked from ([`HEAD_AROUND`]).
fn head_ways() -> Vec<Vec3> {
    ways_round(HEAD_AROUND)
}

/// `around` ways round the plant level, as many from 45 degrees above,
/// and one from straight above: the ways an impostor is baked from.
fn ways_round(around: usize) -> Vec<Vec3> {
    let mut ways = Vec::new();
    for elevation in [0.0_f64, 45.0] {
        for k in 0..around {
            #[allow(clippy::cast_precision_loss)]
            let azimuth = core::f64::consts::TAU * k as f64 / around as f64;
            let (e, a) = (crate::math::radians(elevation), azimuth);
            ways.push(Vec3::new(
                crate::math::cos(e) * crate::math::sin(a),
                crate::math::sin(e),
                crate::math::cos(e) * crate::math::cos(a),
            ));
        }
    }
    ways.push(Vec3::Y);
    ways
}

/// How far round a head's coarse card its full model is taken, in the
/// card's own reach from its middle: an umbel's or a corymb's rays reach
/// down below its flowers about as far as they spread across.
const HEAD_REACH: f64 = 2.0;

/// The triangles of the full model (`wood`, `cards`) within
/// [`HEAD_REACH`] of a head's coarse card (`coarse`) round `mid`.
fn near_head(coarse: &Baked, mid: Vec3, wood: &Mesh, cards: &Mesh) -> Baked {
    let reach = coarse
        .cards
        .positions
        .iter()
        .map(|p| (v3(*p) - mid).length())
        .fold(0.0, f64::max)
        * HEAD_REACH;
    let take = |from: &Mesh| {
        let mut to = Mesh::default();
        for t in 0..from.triangle_count() {
            let centre = (0..3).fold(Vec3::ZERO, |sum, k| {
                sum + v3(from.positions[from.indices[t * 3 + k] as usize])
            }) * (1.0 / 3.0);
            if (centre - mid).length() <= reach {
                copy_triangle(&mut to, from, t);
            }
        }
        to
    };
    Baked {
        cards: take(cards),
        lit: None,
        wood: take(wood),
    }
}

/// Bake `drawn` round `mid` from each of [`head_ways`], `side` texels a
/// side, and add it to `out` as one card showing the nearest: a small
/// impostor. The covered and the card's area, square metres.
fn add_views(
    out: &mut CardPlant,
    drawn: &Baked,
    mid: Vec3,
    side: usize,
    templates: &Templates,
    sky: &Sky,
) -> (f64, f64) {
    let radius = drawn
        .cards
        .positions
        .iter()
        .chain(&drawn.wood.positions)
        .map(|p| (v3(*p) - mid).length())
        .fold(1e-3, f64::max);
    let ways = head_ways();
    let first = out.pictures.len();
    let mut union: Option<Texture> = None;
    let mut covered = 0.0;
    #[allow(clippy::cast_precision_loss)]
    let texel = radius * 2.0 / side as f64;
    for &way in &ways {
        let up = (Vec3::Y - way * Vec3::Y.dot(way)).normalize_or_else(|| Vec3::Z);
        let mut picture = bake(
            &Shot {
                cards: &drawn.cards,
                lit: drawn.lit.as_ref(),
                wood: &drawn.wood,
                strips: None,
                mid,
                right: up.cross(way),
                up,
                normal: way,
                size: (radius * 2.0, radius * 2.0),
                texels: (side, side),
            },
            templates,
            sky,
        );
        keep_coverage(&mut picture);
        fill_empty(&mut picture);
        picture.turn = Turn::Views {
            first,
            count: ways.len(),
        };
        picture.pivot = mid.to_f32();
        #[allow(clippy::cast_possible_truncation)]
        {
            picture.extent = [radius as f32; 3];
        }
        match &mut union {
            Some(union) => {
                for (to, from) in union.color.iter_mut().zip(&picture.color) {
                    to[3] = to[3].max(from[3]);
                }
            }
            None => union = Some(picture.clone()),
        }
        #[allow(clippy::cast_precision_loss)]
        {
            covered += picture.color.iter().filter(|c| c[3] >= 0.5).count() as f64 * texel * texel;
        }
        out.stats.texels += side * side;
        out.stats.seen_texels += side * side;
        out.pictures.push(picture);
    }
    let Some(outline) = union.as_ref().and_then(outline) else {
        out.pictures.truncate(first);
        return (0.0, 0.0);
    };
    // Baked facing the first way; it turns as it is drawn.
    let way = ways[0];
    let up = (Vec3::Y - way * Vec3::Y.dot(way)).normalize_or_else(|| Vec3::Z);
    let right = up.cross(way);
    #[allow(clippy::cast_precision_loss)]
    let index_alpha = first as f32;
    let start = u32::try_from(out.cards.positions.len()).unwrap_or(u32::MAX);
    for &(u, v) in &outline {
        #[allow(clippy::cast_possible_truncation)]
        out.cards.push(Vertex {
            position: mid + right * ((u - 0.5) * radius * 2.0) + up * ((v - 0.5) * radius * 2.0),
            normal: way,
            uv: [u as f32, v as f32],
            color: [1.0, 1.0, 1.0, index_alpha],
            born: 0.0,
            shed: None,
            level: 3,
            segment: mesh::NO_SEGMENT,
        });
    }
    let n = u32::try_from(outline.len()).unwrap_or(0);
    for i in 1..n.saturating_sub(1) {
        out.cards
            .indices
            .extend_from_slice(&[start, start + i, start + i + 1]);
    }
    #[allow(clippy::cast_precision_loss)]
    let views = ways.len() as f64;
    (
        covered / views,
        polygon_area(&outline) * radius * radius * 4.0,
    )
}

/// Where a turned card's pictures lie ([`add_heights`]): about `centre`,
/// from `low` to `high` along `right` and each height's up, `size`
/// metres and `texels` across and up with their margin.
struct Frame {
    centre: Vec3,
    level: Vec3,
    right: Vec3,
    low: (f64, f64),
    high: (f64, f64),
    size: (f64, f64),
    texels: (usize, usize),
}

/// Bake `drawn`, one for each of [`height_ways`], in `frame` and add it to
/// `out` as one card turned to the eye that shows the picture from the
/// nearest height. `extents`: how far its parts spread along each
/// picture's right, up and way. The covered and the card's area, square
/// metres.
fn add_heights(
    out: &mut CardPlant,
    drawn: &[Baked; 3],
    frame: &Frame,
    extents: [[f64; 3]; 3],
    templates: &Templates,
    sky: &Sky,
) -> (f64, f64) {
    let (fw, fh) = frame.size;
    let (tw, th) = frame.texels;
    let (x, y) = (
        (frame.low.0 + frame.high.0) * 0.5,
        (frame.low.1 + frame.high.1) * 0.5,
    );
    #[allow(clippy::cast_precision_loss)]
    let texel = fw / tw as f64;
    let first = out.pictures.len();
    let mut union: Option<Texture> = None;
    let mut covered = 0.0;
    let ways = height_ways(frame.level);
    // Only the first holds the light ([`build`]); it serves all three.
    let lit = drawn[0].lit.as_ref();
    for (((way, up), extent), drawn) in ways.iter().zip(extents).zip(drawn) {
        let mut picture = bake(
            &Shot {
                cards: &drawn.cards,
                lit,
                wood: &drawn.wood,
                strips: None,
                mid: frame.centre + frame.right * x + *up * y,
                right: frame.right,
                up: *up,
                normal: *way,
                size: frame.size,
                texels: frame.texels,
            },
            templates,
            sky,
        );
        keep_coverage(&mut picture);
        fill_empty(&mut picture);
        picture.turn = Turn::Heights {
            first,
            count: ways.len(),
        };
        // It turns about the middle of its parts: a skirt about its
        // trunk, whatever its outline's shape.
        picture.pivot = frame.centre.to_f32();
        #[allow(clippy::cast_possible_truncation)]
        {
            picture.extent = extent.map(|v| v as f32);
        }
        match &mut union {
            Some(union) => {
                for (to, from) in union.color.iter_mut().zip(&picture.color) {
                    to[3] = to[3].max(from[3]);
                }
            }
            None => union = Some(picture.clone()),
        }
        #[allow(clippy::cast_precision_loss)]
        {
            covered += picture.color.iter().filter(|c| c[3] >= 0.5).count() as f64 * texel * texel;
        }
        out.stats.texels += tw * th;
        out.pictures.push(picture);
    }
    out.stats.seen_texels += tw * th;
    let Some(outline) = union.as_ref().and_then(outline) else {
        out.pictures.truncate(first);
        out.stats.texels -= tw * th * ways.len();
        out.stats.seen_texels -= tw * th;
        return (0.0, 0.0);
    };
    // Laid facing out level, the first way; it turns as it is drawn, and
    // each picture's texels lie at the same place along its own right
    // and up from the middle.
    #[allow(clippy::cast_precision_loss)]
    let index_alpha = first as f32;
    let start = u32::try_from(out.cards.positions.len()).unwrap_or(u32::MAX);
    for &(u, v) in &outline {
        #[allow(clippy::cast_possible_truncation)]
        out.cards.push(Vertex {
            position: frame.centre
                + frame.right * (x + (u - 0.5) * fw)
                + Vec3::Y * (y + (v - 0.5) * fh),
            normal: frame.level,
            uv: [u as f32, v as f32],
            color: [1.0, 1.0, 1.0, index_alpha],
            born: 0.0,
            shed: None,
            level: 3,
            segment: mesh::NO_SEGMENT,
        });
    }
    let n = u32::try_from(outline.len()).unwrap_or(0);
    for i in 1..n.saturating_sub(1) {
        out.cards
            .indices
            .extend_from_slice(&[start, start + i, start + i + 1]);
    }
    #[allow(clippy::cast_precision_loss)]
    let heights = ways.len() as f64;
    (covered / heights, polygon_area(&outline) * fw * fh)
}

/// What a picture is baked of, and the frame it is seen in.
struct Shot<'a> {
    /// Drawn for the colour.
    cards: &'a Mesh,
    /// Drawn for the normals and the places the sun is read at, when
    /// they differ from `cards` (leaves as they grew, before they were
    /// turned face on).
    lit: Option<&'a Mesh>,
    wood: &'a Mesh,
    /// Strips drawn with the wood, if any.
    strips: Option<&'a Mesh>,
    /// The middle of the picture, its right and up, and the way it faces.
    mid: Vec3,
    right: Vec3,
    up: Vec3,
    normal: Vec3,
    /// Metres across and up.
    size: (f64, f64),
    texels: (usize, usize),
}

/// A picture of `shot` under flat light, with its normals and the sun
/// that reaches each texel through the whole plant ([`Sky`]). It neither
/// turns nor knows its pivot.
fn bake(shot: &Shot, templates: &Templates, sky: &Sky) -> Texture {
    let (tw, th) = shot.texels;
    let (fw, fh) = shot.size;
    let depth = shot
        .cards
        .positions
        .iter()
        .chain(&shot.wood.positions)
        .map(|p| (v3(*p) - shot.mid).dot(shot.normal).abs())
        .fold(0.0, f64::max);
    let camera = Camera {
        eye: shot.mid + shot.normal * (depth + 1.0),
        target: shot.mid,
        up: shot.up,
        projection: Projection::Orthographic {
            half_height: fh * 0.5,
        },
    };
    let wood_material = templates.bark.map_or(Material::Opaque, Material::Bark);
    let render = |cards: &Mesh| {
        let mut items = vec![(cards, Material::Card), (shot.wood, wood_material)];
        if let Some(strips) = shot.strips {
            items.push((strips, wood_material));
        }
        raster::render(
            &items,
            &camera,
            &Lighting::flat(),
            templates,
            &RenderOptions {
                width: tw,
                height: th,
                supersample: 4,
                shadows: false,
                shadow_bounds: None,
                shadow_texels: 1,
            },
        )
    };
    let image = render(shot.cards);
    let grown = shot.lit.map(render);
    // Rows from the bottom, so v grows upward like the frame's up.
    let mut picture = Texture {
        width: tw,
        height: th,
        color: vec![[0.0; 4]; tw * th],
        normal: vec![[0.0; 3]; tw * th],
        sun: vec![[0.0; 4]; tw * th],
        turn: Turn::Fixed,
        pivot: [0.0; 3],
        extent: [0.0; 3],
        seen_from: shot.normal.to_f32(),
    };
    for y in 0..th {
        for x in 0..tw {
            let from = (th - 1 - y) * tw + x;
            picture.color[y * tw + x] = image.color[from];
            picture.normal[y * tw + x] = match &grown {
                Some(grown) if grown.depth[from].is_finite() => grown.normal[from],
                _ => image.normal[from],
            };
        }
    }
    // How much sun reaches each texel through the whole plant, from
    // every side: on its card a leaf no longer lies in the crown's
    // shade, and a shell of cards casts too thin a shadow on itself.
    for y in 0..th {
        for x in 0..tw {
            let at = y * tw + x;
            if picture.color[at][3] <= 0.0 {
                continue;
            }
            let from = (th - 1 - y) * tw + x;
            let distance = match &grown {
                Some(grown) if grown.depth[from].is_finite() => grown.depth[from],
                _ => image.depth[from],
            };
            // A texel only partly covered may have no depth: it lies on
            // the frame.
            let distance = Some(f64::from(distance))
                .filter(|d| d.is_finite())
                .unwrap_or(depth + 1.0);
            #[allow(clippy::cast_precision_loss)]
            let (u, v) = ((x as f64 + 0.5) / tw as f64, (y as f64 + 0.5) / th as f64);
            let p = shot.mid
                + shot.right * ((u - 0.5) * fw)
                + shot.up * ((v - 0.5) * fh)
                + shot.normal * (depth + 1.0 - distance);
            let n = v3(picture.normal[at]).normalize_or(shot.normal);
            picture.sun[at] = sky.visibility(p, n);
        }
    }
    picture
}

fn tile(w: f64, h: f64, density: f64, most: usize) -> (usize, usize) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    {
        let scale = (most as f64 / (w.max(h) * density).max(1e-9)).min(1.0);
        (
            ((w * density * scale).ceil() as usize + 2).clamp(4, most),
            ((h * density * scale).ceil() as usize + 2).clamp(4, most),
        )
    }
}

/// Give empty texels the colour and normal of a covered neighbour, a few
/// rings out, keeping their zero coverage, so filtering never blends
/// black into an edge.
fn fill_empty(picture: &mut Texture) {
    let (w, h) = (picture.width, picture.height);
    for _ in 0..4 {
        let before = picture.clone();
        for y in 0..h {
            for x in 0..w {
                let at = y * w + x;
                if before.color[at][3] > 0.0 || before.normal[at] != [0.0; 3] {
                    continue;
                }
                let mut sum = [0.0_f32; 3];
                let mut normal = [0.0_f32; 3];
                let mut count = 0.0_f32;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    #[allow(clippy::cast_possible_wrap)]
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let n = ny as usize * w + nx as usize;
                    if before.normal[n] == [0.0; 3] {
                        continue;
                    }
                    let c = before.color[n];
                    for i in 0..3 {
                        sum[i] += c[i];
                        normal[i] += before.normal[n][i];
                    }
                    count += 1.0;
                }
                if count > 0.0 {
                    picture.color[at] = [sum[0] / count, sum[1] / count, sum[2] / count, 0.0];
                    picture.normal[at] = normal.map(|v| v / count);
                }
            }
        }
    }
}

fn polygon_area(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    let mut twice = 0.0;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        twice += a.0 * b.1 - b.0 * a.1;
    }
    twice.abs() * 0.5
}

fn cross2(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// The convex outline, in `[0, 1]²`, of a picture's covered texels, cut
/// to at most [`CORNERS`] corners by dropping, each time, the edge whose
/// neighbours, extended to meet, add the least area. Counter-clockwise.
fn outline(picture: &Texture) -> Option<Vec<(f64, f64)>> {
    let (w, h) = (picture.width, picture.height);
    #[allow(clippy::cast_precision_loss)]
    let (fw, fh) = (w as f64, h as f64);
    let mut points = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if picture.color[y * w + x][3] >= 0.25 {
                #[allow(clippy::cast_precision_loss)]
                let (x0, y0) = (x as f64, y as f64);
                for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    points.push(((x0 + dx) / fw, (y0 + dy) / fh));
                }
            }
        }
    }
    if points.len() < 3 {
        return None;
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup();
    // Monotone chain.
    let mut hull: Vec<(f64, f64)> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &(f64, f64)>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross2(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    while hull.len() > CORNERS {
        let n = hull.len();
        let mut best: Option<(f64, usize, (f64, f64))> = None;
        for i in 0..n {
            // Drop edge i → i+1: extend edges (i-1, i) and (i+1, i+2).
            let (a, b) = (hull[(i + n - 1) % n], hull[i]);
            let (c, d) = (hull[(i + 1) % n], hull[(i + 2) % n]);
            let r = (b.0 - a.0, b.1 - a.1);
            let s = (c.0 - d.0, c.1 - d.1);
            let denom = r.0 * s.1 - r.1 * s.0;
            if denom.abs() < 1e-12 {
                continue;
            }
            let t = ((d.0 - b.0) * s.1 - (d.1 - b.1) * s.0) / denom;
            if t < 0.0 {
                continue;
            }
            let x = (b.0 + r.0 * t, b.1 + r.1 * t);
            // A corner past the picture's frame would only add texels the
            // test cuts.
            if !(-0.02..=1.02).contains(&x.0) || !(-0.02..=1.02).contains(&x.1) {
                continue;
            }
            let added = cross2(b, x, c).abs() * 0.5;
            if best.is_none_or(|(area, _, _)| added < area) {
                best = Some((added, i, x));
            }
        }
        let Some((_, i, x)) = best else {
            break;
        };
        let next = (i + 1) % n;
        hull[i] = x;
        hull.remove(next);
    }
    Some(hull)
}

/// How a plant draws from the side and from 45 degrees above ([`tone`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct Drawn {
    /// The mean luminance of the covered pixels.
    pub tone: [f64; 2],
    /// Card pixels drawn for each pixel the plant covers.
    pub overdraw: [f64; 2],
    /// The area the plant covers, square metres: how much of the sky
    /// shows through it.
    pub cover: [f64; 2],
}

/// How bright `items` draw under [`Lighting::daylight`] with shadows, with
/// no ground, how much their cards overdraw and how much they cover, from
/// the side and from 45 degrees above, orthographic, `size` pixels square.
/// A tier should keep the full plant's tone and cover; this measures them.
#[must_use]
pub fn tone(items: &[(&Mesh, Material)], templates: &Templates, size: usize) -> Drawn {
    let Some((low, high)) = raster::bounds(items) else {
        return Drawn::default();
    };
    let centre = (low + high) * 0.5;
    let radius = ((high - low).length() * 0.5).max(1e-3);
    let views = [Vec3::Z, Vec3::new(0.0, 1.0, 1.0)].map(|direction| {
        let direction = direction.normalize_or(Vec3::Z);
        let camera = Camera {
            eye: centre + direction * (radius * 3.0),
            target: centre,
            up: if direction.y.abs() > 0.99 {
                Vec3::Z
            } else {
                Vec3::Y
            },
            projection: Projection::Orthographic {
                half_height: radius,
            },
        };
        let image = raster::render(
            items,
            &camera,
            &Lighting::daylight(),
            templates,
            &RenderOptions {
                width: size,
                height: size,
                supersample: 2,
                shadows: true,
                shadow_bounds: None,
                shadow_texels: raster::SHADOW_TEXELS,
            },
        );
        let (sum, count) =
            image
                .color
                .iter()
                .filter(|c| c[3] >= 0.5)
                .fold((0.0, 0.0), |(sum, count), c| {
                    (
                        sum + 0.2126 * f64::from(c[0])
                            + 0.7152 * f64::from(c[1])
                            + 0.0722 * f64::from(c[2]),
                        count + 1.0,
                    )
                });
        #[allow(clippy::cast_precision_loss)]
        let pixel = radius * 2.0 / size as f64;
        if count > 0.0 {
            (
                sum / count,
                image.card_fragments / count,
                count * pixel * pixel,
            )
        } else {
            (0.0, 0.0, 0.0)
        }
    });
    Drawn {
        tone: views.map(|v| v.0),
        overdraw: views.map(|v| v.1),
        cover: views.map(|v| v.2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smallest_axis_finds_a_sheets_normal() {
        // Points spread along x and y, thin along (1, 0, 1).
        let n = Vec3::new(1.0, 0.0, 1.0).normalize_or(Vec3::Z);
        let mut m = [[0.0; 3]; 3];
        for (a, b, c) in [
            (1.0, 0.0, 0.1),
            (-1.0, 0.0, 0.1),
            (0.0, 2.0, -0.1),
            (0.0, -2.0, -0.1),
        ] {
            let u = Vec3::new(1.0, 0.0, -1.0).normalize_or(Vec3::X);
            let p = u * a + Vec3::Y * b + n * c;
            add(&mut m, outer(p, 1.0));
        }
        let (axis, least) = smallest_axis(m);
        assert!(axis.dot(n).abs() > 0.999, "{axis:?}");
        assert!((least - 0.04).abs() < 1e-9, "{least}");
    }

    #[test]
    fn a_skirt_wraps_its_stem_and_a_tussock_does_not() {
        let leaf = |angle: f64, y: f64, out: f64, down: bool| {
            let (sin, cos) = (crate::math::sin(angle), crate::math::cos(angle));
            let radial = Vec3::new(cos, 0.0, sin);
            let half = if down {
                Vec3::new(0.0, -0.3, 0.0)
            } else {
                (radial * out + Vec3::Y) * 0.3
            };
            Element {
                centre: radial * (0.5 + out * y) + Vec3::Y * y,
                facing: radial,
                wood: false,
                planar: false,
                area: 0.1,
                half,
            }
        };
        let around = |down: bool, out: f64| -> Vec<Element> {
            (0..48)
                .map(|i| {
                    #[allow(clippy::cast_precision_loss)]
                    let angle = core::f64::consts::TAU * (i % 12) as f64 / 12.0;
                    #[allow(clippy::cast_precision_loss)]
                    let y = (i / 12) as f64 * 0.8;
                    leaf(angle, y, out, down)
                })
                .collect()
        };
        let centre = |e: &[Element]| {
            e.iter().fold(Vec3::ZERO, |sum, e| sum + e.centre) * (1.0 / e.len() as f64)
        };
        // Dead leaves hanging round a trunk half a metre out.
        let skirt = around(true, 0.0);
        let members: Vec<&Element> = skirt.iter().collect();
        assert!(wraps(&members, centre(&skirt)));
        // Blades rising and spreading from a tussock's foot.
        let tussock = around(false, 1.0);
        let members: Vec<&Element> = tussock.iter().collect();
        assert!(!wraps(&members, centre(&tussock)));
        // Half a skirt, round one side.
        let half: Vec<Element> = skirt.iter().filter(|e| e.centre.x > 0.0).copied().collect();
        let members: Vec<&Element> = half.iter().collect();
        assert!(!wraps(&members, centre(&half)));
    }

    fn picture(width: usize, height: usize, alpha: impl Fn(usize, usize) -> f32) -> Texture {
        Texture {
            width,
            height,
            color: (0..width * height)
                .map(|i| [0.2, 0.5, 0.1, alpha(i % width, i / width)])
                .collect(),
            normal: vec![[0.0, 0.0, 1.0]; width * height],
            sun: Vec::new(),
            turn: Turn::Fixed,
            pivot: [0.0; 3],
            extent: [0.0; 3],
            seen_from: [0.0; 3],
        }
    }

    fn passing(picture: &Texture) -> usize {
        picture.color.iter().filter(|c| c[3] >= 0.5).count()
    }

    #[test]
    fn keeps_faint_parts_as_their_share_of_texels() {
        // A patch of lace a fifth covered keeps a fifth of its texels, in
        // every quarter of it, and none outside it.
        let (w, h) = (64, 64);
        let mut lace = picture(w, h, |x, y| {
            if (16..48).contains(&x) && (16..48).contains(&y) {
                0.2
            } else {
                0.0
            }
        });
        keep_coverage(&mut lace);
        let kept = passing(&lace) as f64;
        assert!((kept / (32.0 * 32.0) - 0.2).abs() < 0.02, "{kept}");
        for (x0, y0) in [(16, 16), (32, 16), (16, 32), (32, 32)] {
            let quarter = (y0..y0 + 16)
                .flat_map(|y| (x0..x0 + 16).map(move |x| (x, y)))
                .filter(|&(x, y)| lace.color[y * w + x][3] >= 0.5)
                .count();
            assert!((30..=72).contains(&quarter), "{x0},{y0}: {quarter}");
        }
        for y in 0..h {
            for x in 0..w {
                if !((16..48).contains(&x) && (16..48).contains(&y)) {
                    assert!(lace.color[y * w + x][3] < 0.5, "{x},{y}");
                }
            }
        }
        // A leaf the texels resolve keeps its outline.
        let leaf = picture(w, h, |x, _| match x {
            0..20 => 1.0,
            20 => 0.3,
            _ => 0.0,
        });
        let mut kept = leaf.clone();
        keep_coverage(&mut kept);
        assert_eq!(kept, leaf);
    }
}

/// Directions the sun is shadowed from when each texel's
/// [`Texture::sun`] is baked, spread evenly over the sphere.
const SUN_DIRECTIONS: usize = 32;

/// Texels along each side of those shadow maps.
const SUN_TEXELS: usize = 1024;

/// The full plant's shadow seen from [`SUN_DIRECTIONS`] directions, to
/// bake how much sun reaches each texel by direction.
struct Sky {
    maps: Vec<(Vec3, raster::ShadowMap)>,
}

impl Sky {
    fn new(wood: &Mesh, cards: &Mesh, templates: &Templates) -> Self {
        let items = [(wood, Material::Opaque), (cards, Material::Card)];
        let Some(bounds) = raster::bounds(&items) else {
            return Self { maps: Vec::new() };
        };
        let golden = core::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        let sun = |i: usize| {
            #[allow(clippy::cast_precision_loss)]
            let z = 1.0 - 2.0 * (i as f64 + 0.5) / SUN_DIRECTIONS as f64;
            let r = (1.0 - z * z).max(0.0).sqrt();
            #[allow(clippy::cast_precision_loss)]
            let phi = i as f64 * golden;
            // y up: the first direction is straight overhead.
            let d = Vec3::new(r * crate::math::cos(phi), z, r * crate::math::sin(phi));
            (
                d,
                raster::ShadowMap::new(&items, templates, d, bounds, SUN_TEXELS),
            )
        };
        let lease = cores::Lease::take(SUN_DIRECTIONS.min(cores::per_task()));
        let maps = cores::map((0..SUN_DIRECTIONS).collect(), lease.threads(), &sun);
        Self { maps }
    }

    /// The four spherical-harmonic coefficients of the sun's visibility at
    /// `p` on a surface facing `normal` (projected from the maps, each
    /// direction an equal share of the sphere).
    fn visibility(&self, p: Vec3, normal: Vec3) -> [f32; 4] {
        if self.maps.is_empty() {
            // Seen from everywhere: 1 in every direction.
            #[allow(clippy::cast_possible_truncation)]
            return [(1.0 / raster::harmonics(Vec3::Y)[0]) as f32, 0.0, 0.0, 0.0];
        }
        let mut c = [0.0; 4];
        for (d, map) in &self.maps {
            // Seen from either face: a leaf is lit through from behind.
            let seen = map.visibility(p, normal, *d);
            let h = raster::harmonics(*d);
            for i in 0..4 {
                c[i] += seen * h[i];
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let share = 4.0 * core::f64::consts::PI / self.maps.len() as f64;
        #[allow(clippy::cast_possible_truncation)]
        c.map(|v| (v * share) as f32)
    }
}
