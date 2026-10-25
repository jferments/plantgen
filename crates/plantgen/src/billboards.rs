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
//! low-poly geometry (`crate::lowpoly`) can keep only outlines.
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

use crate::body::BodyLook;
use crate::graph::PlantGraph;
use crate::looks::Look;
use crate::math::{Vec3, any_perpendicular};
use crate::mesh::{self, Card, LodSpec, Mesh, PlantMesh, Vertex};
use crate::raster::{self, Camera, Lighting, Material, Projection, RenderOptions, Texture};
use crate::spec::Appearance;
use crate::templates::Templates;

/// One baked-card tier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardTier {
    pub name: &'static str,
    /// Cards a plant at most [`crate::lowpoly::SMALL_M`] tall may draw,
    /// and one at least [`crate::lowpoly::LARGE_M`].
    pub small: usize,
    pub large: usize,
    /// Texels of all of a plant's pictures together, small and large.
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
        let t = ((height - crate::lowpoly::SMALL_M)
            / (crate::lowpoly::LARGE_M - crate::lowpoly::SMALL_M))
            .clamp(0.0, 1.0);
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

    /// Texels its pictures may hold.
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

/// The most [`keep_coverage`] scales a picture's coverage.
const MOST_COVERAGE_SCALE: f64 = 2.0;

/// The most a clump's card leans from upright, degrees.
pub const MOST_LEAN: f64 = 45.0;

/// The fewest texels across a blade or a stem in a picture.
pub const MIN_TEXELS: f64 = 1.0;

/// The most corners a card's outline keeps.
pub const CORNERS: usize = 8;

/// Rounds of k-means.
const ROUNDS: usize = 16;

/// What a baked-card build drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardStats {
    pub cards: usize,
    pub wood_triangles: usize,
    pub card_triangles: usize,
    pub texels: usize,
    /// The share of the cards' area their pictures cover.
    pub coverage: f64,
}

impl CardStats {
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.wood_triangles + self.card_triangles
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
    let mut centres = vec![points[first]];
    let mut nearest: Vec<f64> = points.iter().map(|p| distance2(p, &centres[0])).collect();
    while centres.len() < k {
        // The point farthest from every centre, by its weight's root, so a
        // stray hair does not take a card from a frond.
        let next = (0..n)
            .max_by(|&a, &b| {
                (nearest[a] * weights[a].sqrt())
                    .total_cmp(&(nearest[b] * weights[b].sqrt()))
                    .then(b.cmp(&a))
            })
            .unwrap_or(0);
        if nearest[next] <= 0.0 {
            break;
        }
        centres.push(points[next]);
        let c = centres.len() - 1;
        for (i, p) in points.iter().enumerate() {
            nearest[i] = nearest[i].min(distance2(p, &centres[c]));
        }
    }
    let mut group = vec![0; n];
    for _ in 0..ROUNDS {
        let mut changed = false;
        for (i, p) in points.iter().enumerate() {
            let best = (0..centres.len())
                .min_by(|&a, &b| {
                    distance2(p, &centres[a])
                        .total_cmp(&distance2(p, &centres[b]))
                        .then(a.cmp(&b))
                })
                .unwrap_or(0);
            if group[i] != best {
                group[i] = best;
                changed = true;
            }
        }
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

/// Build `graph` at `tier`. `levels` are the package's four mesh levels
/// for a 15 m plant; the cards are baked from the second, every organ a
/// card of its own, and the tubes come from the coarsest.
#[must_use]
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    templates: &Templates,
    levels: &[LodSpec; 4],
    tier: &CardTier,
) -> CardPlant {
    let height = graph.height.max(0.01);
    let coarse_at = |keep: f64| {
        mesh::build(
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
                    bark_radius(&coarse.wood, coarse.wood.indices[t * 3 + k] as usize)
                        .is_none_or(|r| r >= keep)
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
    // Every axis, however fine (a fern's rachis, a grass's culm): the
    // pictures are where they still show.
    let source = mesh::build(
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
        });
    }
    // Free-standing thin axes that hold up flowers or fruit (a grass's
    // culms, a forb's flowering stems) are ribbons in their own place,
    // not baked: baked onto a card they would stand off the card that
    // holds their flowers, and the flowers would float.
    // Each ribbon's head takes a card (the tier's `heads` share of them
    // at most), so that the rest of the plant keeps enough.
    let ribbons = ribbon_axes(
        graph,
        looks,
        keep,
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
        elements.push(Element {
            centre: (segment.start + segment.end) * 0.5,
            facing: along * (1.0 / length),
            wood: true,
            planar: segment.within.is_some(),
            area: (length * segment.radius * 2.0).max(1e-8),
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
    // Tubes: the coarsest level's wood at least `keep` in radius, and
    // fleshy bodies.
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
        } else {
            thin.nearest(centre, &graph.segments).is_none_or(|(s, d)| {
                graph.segments[s].within.is_none() || d > keep * LEAF_AXIS_REACH
            })
        };
        if thick {
            copy_triangle(&mut wood, &coarse.wood, t);
        }
    }

    let k = tier.cards(height);
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
                && looks
                    .get(usize::from(organ.organ))
                    .is_some_and(crate::lowpoly::signature)
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
    let rest: Vec<usize> = (0..elements.len())
        .filter(|&i| heads.get(i).copied().flatten().is_none())
        .collect();
    let points: Vec<[f64; 9]> = rest
        .iter()
        .map(|&i| features(&elements[i], height))
        .collect();
    let weights: Vec<f64> = rest.iter().map(|&i| elements[i].area).collect();
    let clustered = kmeans(&points, &weights, k - headed);
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

    // Each group's parts: its organ cards, and its thin wood's triangles.
    let mut group_cards: Vec<Vec<Card>> = vec![Vec::new(); groups];
    for (i, card) in organ_cards.iter().enumerate() {
        group_cards[group[i]].push(*card);
    }
    let mut group_wood: Vec<Mesh> = vec![Mesh::default(); groups];
    // Each thin segment's triangles, and its mean colour.
    let mut group_triangles: Vec<Vec<(usize, usize)>> = vec![Vec::new(); groups];
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
        if !barked && d > keep {
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
        group_triangles[group[e]].push((t, segment as usize));
    }

    let foliage_centre = {
        let total: f64 = elements.iter().map(|e| e.area).sum::<f64>().max(1e-12);
        elements
            .iter()
            .fold(Vec3::ZERO, |sum, e| sum + e.centre * e.area)
            * (1.0 / total)
    };
    // Planes.
    struct Plane {
        centre: Vec3,
        normal: Vec3,
        right: Vec3,
        up: Vec3,
        low: (f64, f64),
        high: (f64, f64),
        cards: Mesh,
        wood: Mesh,
        group: usize,
        flat: bool,
    }
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
        let total: f64 = members.iter().map(|e| e.area).sum();
        let centre = members
            .iter()
            .fold(Vec3::ZERO, |sum, e| sum + e.centre * e.area)
            * (1.0 / total);
        let mut m = [[0.0; 3]; 3];
        for e in &members {
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
        let planar_area: f64 = members.iter().filter(|e| e.planar).map(|e| e.area).sum();
        let flat = planar_area * 2.0 >= total;
        let start = members
            .iter()
            .filter(|e| !e.wood)
            .max_by(|a, b| a.area.total_cmp(&b.area))
            .map_or_else(|| any_perpendicular(members[0].facing), |e| e.facing);
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
            for e in &members {
                lo = lo.min(e.centre);
                hi = hi.max(e.centre);
            }
            let span = hi - lo;
            let tall = span.y.max(1e-9);
            let broad = span.x.max(span.z);
            let most = crate::math::radians(MOST_LEAN) * (broad / tall).min(1.0);
            let lean = lean.clamp(-most, most);
            level * crate::math::cos(lean) + Vec3::Y * crate::math::sin(lean)
        };
        let cards = PlantMesh {
            cards: group_cards[g].clone(),
            ..PlantMesh::default()
        }
        .card_mesh();
        let wood = std::mem::take(&mut group_wood[g]);
        if cards.indices.is_empty() && wood.indices.is_empty() {
            continue;
        }
        // Up is the world's up laid on the plane, where it can be.
        let up = (Vec3::Y - normal * Vec3::Y.dot(normal))
            .normalize_or_else(|| any_perpendicular(normal));
        let right = up.cross(normal);
        let mut low = (f64::INFINITY, f64::INFINITY);
        let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in cards.positions.iter().chain(&wood.positions) {
            let d = v3(*p) - centre;
            let (x, y) = (d.dot(right), d.dot(up));
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
        }
        planes.push(Plane {
            centre,
            normal,
            right,
            up,
            low,
            high,
            cards,
            wood,
            group: g,
            flat,
        });
    }

    // Texels per metre, the same on every card.
    let area: f64 = planes
        .iter()
        .map(|p| ((p.high.0 - p.low.0) * (p.high.1 - p.low.1)).max(1e-8))
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let density = (tier.texels(height) as f64 / area).sqrt();

    // Nothing thinner than a texel: narrower blades are widened to one,
    // and thinner stems drawn as strips a texel wide in their own colour,
    // so a culm or a blade stays a line in the picture instead of
    // breaking into dots.
    let texel = 1.0 / density;
    for plane in &mut planes {
        let g = plane.group;
        let widened: Vec<Card> = group_cards[g]
            .iter()
            .map(|card| {
                let mut card = *card;
                if !plane.flat {
                    // A clump's picture shows each leaf face on, as crown
                    // cards are painted: a leaf seen edge on from the card
                    // would be thinner than a texel and the clump would
                    // read as bare stripes. Its place and size stay.
                    // It only turns about its own midrib, so its tip
                    // stays where it grows.
                    let n = plane.normal;
                    let heading = v3(card.heading);
                    // heading × left faces the card as nearly as it can.
                    card.left = n.cross(heading).normalize_or(v3(card.left)).to_f32();
                }
                #[allow(clippy::cast_possible_truncation)]
                let least = (texel * MIN_TEXELS) as f32;
                card.width = card.width.max(least);
                card
            })
            .collect();
        plane.cards = PlantMesh {
            cards: widened,
            ..PlantMesh::default()
        }
        .card_mesh();
        let mut wood = Mesh::default();
        let mut strips = Vec::new();
        for &(t, segment) in &group_triangles[g] {
            if graph.segments[segment].radius * 2.0 >= texel * MIN_TEXELS {
                copy_triangle(&mut wood, &source.wood, t);
            }
        }
        // Every thin segment of the group thinner than a texel is a strip:
        // a tube's triangles span many segments, so not every segment has
        // its own.
        for (s, segment) in graph.segments.iter().enumerate() {
            let e = wood_element[s];
            if e != usize::MAX && group[e] == g && segment.radius * 2.0 < texel * MIN_TEXELS {
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
        plane.wood = wood;
        let mut low = (f64::INFINITY, f64::INFINITY);
        let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in plane.cards.positions.iter().chain(&plane.wood.positions) {
            let d = v3(*p) - plane.centre;
            let (x, y) = (d.dot(plane.right), d.dot(plane.up));
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
        }
        plane.low = low;
        plane.high = high;
    }

    // The pictures' margins and rounding are paid from the same budget.
    let budget = tier.texels(height);
    let mut tile_density = density;
    for _ in 0..8 {
        let total: usize = planes
            .iter()
            .map(|p| {
                let (tw, th) = tile(
                    p.high.0 - p.low.0,
                    p.high.1 - p.low.1,
                    tile_density,
                    tier.most_texels,
                );
                tw * th
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
    let mut covered_area = 0.0;
    let mut card_area = 0.0;
    for plane in &planes {
        let (w, h) = (plane.high.0 - plane.low.0, plane.high.1 - plane.low.1);
        let (tw, th) = tile(w, h, tile_density, tier.most_texels);
        // The picture's frame: the group's extent, grown to the texels'
        // aspect.
        #[allow(clippy::cast_precision_loss)]
        let texel = (w / (tw - 2).max(1) as f64).max(h / (th - 2).max(1) as f64);
        #[allow(clippy::cast_precision_loss)]
        let (fw, fh) = (texel * tw as f64, texel * th as f64);
        let mid = plane.centre
            + plane.right * ((plane.low.0 + plane.high.0) * 0.5)
            + plane.up * ((plane.low.1 + plane.high.1) * 0.5);
        let depth = plane
            .cards
            .positions
            .iter()
            .chain(&plane.wood.positions)
            .map(|p| (v3(*p) - mid).dot(plane.normal).abs())
            .fold(0.0, f64::max);
        let camera = Camera {
            eye: mid + plane.normal * (depth + 1.0),
            target: mid,
            up: plane.up,
            projection: Projection::Orthographic {
                half_height: fh * 0.5,
            },
        };
        let wood_material = templates.bark.map_or(Material::Opaque, Material::Bark);
        let image = raster::render(
            &[(&plane.cards, Material::Card), (&plane.wood, wood_material)],
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
        );
        // Rows from the bottom, so v grows upward like the plane's up.
        let mut picture = Texture {
            width: tw,
            height: th,
            color: vec![[0.0; 4]; tw * th],
            normal: vec![[0.0; 3]; tw * th],
        };
        for y in 0..th {
            for x in 0..tw {
                let from = (th - 1 - y) * tw + x;
                picture.color[y * tw + x] = image.color[from];
                picture.normal[y * tw + x] = image.normal[from];
            }
        }
        keep_coverage(&mut picture);
        fill_empty(&mut picture);
        let Some(outline) = outline(&picture) else {
            continue;
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
        card_area += poly_area;
        #[allow(clippy::cast_precision_loss)]
        let covered = picture.color.iter().filter(|c| c[3] >= 0.5).count() as f64 * texel * texel;
        covered_area += covered;
        out.stats.texels += tw * th;
        out.pictures.push(picture);
    }
    out.stats.cards = out.pictures.len();
    // A ribbon's quads are written with both windings; a renderer draws
    // them once, two-sided.
    out.stats.wood_triangles = out.wood.triangle_count() - ribbons.len() * (RIBBON_POINTS - 1) * 2;
    out.stats.card_triangles = out.cards.triangle_count();
    out.stats.coverage = if card_area > 0.0 {
        covered_area / card_area
    } else {
        0.0
    };
    out
}

/// Scale a picture's coverage so that as many texels pass the alpha test
/// (coverage at least one half) as its parts cover on average (Castaño
/// 2010, "Computing alpha mipmaps"; the same as alpha-to-coverage-preserving
/// mipmaps in games). A blade or a stalk thinner than a texel, whose
/// texels are each only partly covered, would otherwise fail the test and
/// break into dots.
///
/// The scale is held to [`MOST_COVERAGE_SCALE`]: a cloud of spikelets or
/// stamens, each a fraction of a texel, would otherwise fill in solid.
fn keep_coverage(picture: &mut Texture) {
    let n = picture.color.len();
    if n == 0 {
        return;
    }
    let mean: f64 = picture.color.iter().map(|c| f64::from(c[3])).sum::<f64>();
    #[allow(clippy::cast_precision_loss)]
    let goal = mean / n as f64;
    let passing = |scale: f64| {
        #[allow(clippy::cast_precision_loss)]
        let count = picture
            .color
            .iter()
            .filter(|c| f64::from(c[3]) * scale >= 0.5)
            .count() as f64;
        count / n as f64
    };
    let (mut low, mut high) = (1.0_f64, MOST_COVERAGE_SCALE);
    if passing(low) >= goal {
        return;
    }
    for _ in 0..24 {
        let mid = f64::midpoint(low, high);
        if passing(mid) < goal {
            low = mid;
        } else {
            high = mid;
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let scale = high as f32;
    for c in &mut picture.color {
        c[3] = (c[3] * scale).min(1.0);
    }
}

/// A ribbon's points along its axis at most.
const RIBBON_POINTS: usize = 4;

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
                .is_some_and(crate::lowpoly::signature)
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
