//! Solid low-poly plants for weak GPUs (plant quality tiers, Project
//! After's plan of 2026-10-23): a plant as its own skeleton and the
//! masses its organs make on it, opaque and untextured, within a triangle
//! budget, every part attached to the wood that bears it.
//!
//! Low-poly games draw a tree as a trunk and a few branches with a mass
//! of leaves wrapped round each branch's end, and a herb or a grass as
//! its stems with the leaves, blades and flowers they bear. This module
//! draws a grown plant the same way, from the plant itself:
//!
//! 1. A tree starts as one mass, the convex hull of its organs wrapped
//!    round the points where it stands. A herb, a grass, a fern or a
//!    cactus starts with nothing but its flower clusters, and draws its
//!    stems one at a time, the largest first: fewer stems, never stems
//!    merged into a lump.
//! 2. The largest part is split along the plant's own branching while
//!    the budget allows: a mass of sibling branches into two masses of
//!    half of them each; a branch into its tube, a part for its own
//!    branches and one for the organs it bears itself. Those organs are a
//!    few of the real leaves at their real size, evenly along the stem
//!    (more as the budget allows, narrow blades widened for the ones left
//!    out), or a mass split in halves along it.
//! 3. Every part touches what bears it: a mass wraps the points where its
//!    branches leave the drawn wood, a tube starts on its parent's drawn
//!    path, a leaf sits on its drawn stem, and flowers and fruit hang
//!    from the mass they belong to. Every shape is drawn from real
//!    organs.
//!
//! Some organs keep a shape of their own. A fern's frond (organs spread
//! flat to either side of a stalk) is one strip along its own arch, as
//! wide as its pinnae reach. A spike, raceme or fruit is a spindle in its
//! own colour; an airy grass panicle stays a spray of thin spokes. Fleshy
//! bodies are tubes of their own cross-section and swell, a pad flat,
//! with a dome at the tip.

use crate::body::BodyLook;
use crate::graph::PlantGraph;
use crate::looks::{Look, Shape};
use crate::math::Vec3;
use crate::mesh::{self, Card, LodSpec, Mesh, PlantMesh, Vertex};
use crate::rng::{mix64, unit};
use crate::spec::Appearance;
use crate::templates::Templates;

/// One solid tier: how much it may draw and how finely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidTier {
    pub name: &'static str,
    /// Triangles a plant at most [`SMALL_M`] tall may draw, and one at
    /// least [`LARGE_M`]; between, in proportion to its height.
    pub small: usize,
    pub large: usize,
    /// The most points a drawn axis keeps, and how far (a share of the
    /// plant's height) its tube may stray from its path where it drops one.
    pub rings: usize,
    pub straying: f64,
    /// Sides round a woody tube, and round a fleshy body.
    pub sides: u32,
    pub body_sides: u32,
    /// Directions a mass is wrapped along ([`DIRECTIONS`]): a hull of at
    /// most twice as many points.
    pub directions: usize,
    /// Stations along an organ's outline drawn alone: 1 draws a diamond
    /// (2 triangles), 2 a hexagon (4), 3 an octagon (6).
    pub stations: usize,
}

/// Plants this tall or lower take a tier's `small` budget.
pub const SMALL_M: f64 = 2.0;
/// Plants this tall or taller take its `large` budget.
pub const LARGE_M: f64 = 15.0;

/// About a Steam Deck or an integrated GPU at 60 frames a second: a few
/// hundred triangles a herb, 1,500 a tree.
pub const LOW: SolidTier = SolidTier {
    name: "low",
    small: 400,
    large: 1500,
    rings: 6,
    straying: 0.01,
    sides: 3,
    body_sides: 6,
    directions: 3,
    stations: 2,
};

/// The weakest machines: an essence of the plant, 120 triangles a herb
/// and 300 a tree.
pub const MINIMAL: SolidTier = SolidTier {
    name: "minimal",
    small: 120,
    large: 300,
    rings: 3,
    straying: 0.03,
    sides: 3,
    body_sides: 4,
    directions: 3,
    stations: 1,
};

pub const TIERS: [SolidTier; 2] = [LOW, MINIMAL];

impl SolidTier {
    /// The budget of a plant `height` metres tall.
    #[must_use]
    pub fn budget(&self, height: f64) -> usize {
        let t = ((height - SMALL_M) / (LARGE_M - SMALL_M)).clamp(0.0, 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let budget =
            (self.small as f64 + (self.large as f64 - self.small as f64) * t).round() as usize;
        budget
    }
}

/// What a solid build drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SolidStats {
    pub budget: usize,
    /// Triangles of the tubes, of the closed masses, and of the flat
    /// pieces (single organs and flat masses, drawn two-sided).
    pub wood_triangles: usize,
    pub mass_triangles: usize,
    pub organ_triangles: usize,
    /// Axes drawn of the plant's axes; masses; organs drawn alone, of all.
    pub axes: (usize, usize),
    pub masses: usize,
    pub singles: (usize, usize),
}

impl SolidStats {
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.wood_triangles + self.mass_triangles + self.organ_triangles
    }
}

/// One piece of a solid plant and where it is attached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Piece {
    /// The points it hangs from: on a drawn path, or where a stem stands.
    pub attach: Vec<Vec3>,
    /// Its vertices.
    pub points: Vec<Vec3>,
    /// A single organ (attached at its base), not a mass.
    pub single: bool,
}

/// A plant at a solid tier.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SolidPlant {
    /// Tubes and closed masses, their faces outward.
    pub wood: Mesh,
    /// Flat pieces facing one way, drawn two-sided.
    pub organs: Mesh,
    pub stats: SolidStats,
    /// Every mass and single organ, with where it hangs from.
    pub pieces: Vec<Piece>,
    /// The drawn axes' paths (place and radius), each starting on its
    /// parent's; and where the plant's stems stand.
    pub paths: Vec<Vec<(Vec3, f64)>>,
    pub grounds: Vec<Vec3>,
}

impl SolidPlant {
    /// Wood and organs in one mesh, the organs' backs added, for a
    /// renderer or preview that draws every face one-sided.
    #[must_use]
    pub fn two_sided(&self) -> PlantMesh {
        let mut wood = self.wood.clone();
        append(&mut wood, &self.organs, false);
        append(&mut wood, &self.organs, true);
        PlantMesh {
            wood,
            ..PlantMesh::default()
        }
    }
}

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

/// Directions a mass is wrapped along: the three axes, the four
/// diagonals of a cube, then the six diagonals of its faces.
const DIRECTIONS: [[f64; 3]; 13] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, -1.0],
    [1.0, -1.0, 1.0],
    [-1.0, 1.0, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, 1.0, -1.0],
];

/// A mass thinner than this share of its length is drawn flat, both
/// faces: a fern's frond, a pad, a fan of blades in one plane.
const FLAT: f64 = 0.06;

/// Bodies narrower than this share of the plant's height are drawn with
/// the wood's sides.
const SLIM: f64 = 0.02;

/// Organs no thicker across their axis than this share of their spread
/// are drawn as a strip along it ([`Solid::blade`]).
const BLADE_THIN: f64 = 0.35;
/// ... and only when there are at least this many, each shorter than
/// this share of the stalk.
const BLADE_MANY: usize = 6;
const BLADE_SMALL: f64 = 0.3;

/// Flowers and fruit spread over more than this share of a woody part
/// are left to its leaves' mass: hung on its side they would float.
const SCATTERED: f64 = 0.6;

/// Leaves at least this share of their part's size on average are drawn
/// as themselves ([`Kind::Bin`]), at first this many.
const LEAFY: f64 = 0.1;
/// Sampled blades widen at most this much: more, and a culm's leaves
/// would be boards.
const WIDEST_BLADE: f64 = 1.5;
const SAMPLE: usize = 3;

/// The most splits a build tries.
const MOST_SPLITS: usize = 20_000;

/// Build `graph` at `tier`. `levels` are the package's four mesh levels
/// for a 15 m plant (a quality profile's `lods`); the organs come from
/// the second, every organ a card of its own.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    templates: &Templates,
    levels: &[LodSpec; 4],
    tier: &SolidTier,
) -> SolidPlant {
    let height = graph.height;
    let budget = tier.budget(height);
    // Organs drawn solid near (an agave's thick leaves, a lily's petals,
    // lace) taken as their cards: their templates still describe them.
    let carded: Vec<Look> = looks
        .iter()
        .cloned()
        .map(|mut look| {
            look.solid = None;
            look.form = crate::blooms::Form::Card;
            if let Shape::Compound(compound) = &mut look.shape {
                compound.lace = None;
            }
            look
        })
        .collect();
    let plant = mesh::build(
        graph,
        &carded,
        bodies,
        appearance,
        &levels[1].for_height(height),
        1,
    );
    let outlines = Outlines::new(templates, tier.stations);
    let skeleton = Skeleton::new(graph);
    // Spine cards (templates past the looks') are left to the bodies'
    // colour.
    let cards: Vec<Card> = plant
        .cards
        .into_iter()
        .filter(|card| usize::from(card.template) < looks.len())
        .collect();
    let items = items(&skeleton, graph, chains(cards), looks, &outlines);
    let mut solid = Solid {
        skeleton: &skeleton,
        items: &items,
        outlines: &outlines,
        bodies,
        tier,
        bark: [
            appearance.bark[0],
            appearance.bark[1],
            appearance.bark[2],
            1.0,
        ],
        directions: DIRECTIONS
            .iter()
            .take(tier.directions.clamp(3, DIRECTIONS.len()))
            .map(|d| Vec3::new(d[0], d[1], d[2]).normalize_or(Vec3::Y))
            .collect(),
        drawn: vec![None; skeleton.axes.len()],
        own: {
            let mut own = vec![Vec::new(); skeleton.axes.len()];
            for (i, item) in items.iter().enumerate() {
                own[item.axis].push(i);
            }
            own
        },
        tolerance: height * tier.straying,
        height,
        // A cactus is drawn stem by stem at any height: its joints
        // wrapped together would be a boulder.
        herb: height <= SMALL_M || skeleton.axes.iter().any(|axis| axis.body != 0),
    };
    solid.refine(budget)
}

/// One axis of a plant: a stem, a branch or a fleshy body and their
/// continuations.
struct Axis {
    /// Its segments, base first.
    segments: Vec<usize>,
    /// Its path: the first segment's start, then each segment's end, with
    /// the radius there.
    points: Vec<(Vec3, f64)>,
    born: f64,
    /// Its fleshy body type, 0 for wood, and the left vector its first
    /// segment's cross-section is set by.
    body: u8,
    left: Vec3,
}

/// The plant's axes that stand at its age.
fn axis_paths(graph: &PlantGraph) -> Vec<Axis> {
    let segments = &graph.segments;
    let alive = |index: usize| segments[index].shed.is_none_or(|shed| shed > graph.age);
    // Each segment's continuation: its child that is not lateral.
    let mut next = vec![None; segments.len()];
    for (index, segment) in segments.iter().enumerate() {
        if let (Some(parent), false) = (segment.parent, segment.lateral) {
            next[parent as usize].get_or_insert(index);
        }
    }
    let mut axes = Vec::new();
    for (first, segment) in segments.iter().enumerate() {
        let starts = segment.parent.is_none() || segment.lateral;
        if !starts || !alive(first) {
            continue;
        }
        let mut points = vec![(segment.start, segment.radius)];
        let mut members = Vec::new();
        let mut at = Some(first);
        while let Some(index) = at.filter(|&index| alive(index)) {
            let end = &segments[index];
            let radius = next[index].map_or(end.radius * 0.5, |n| segments[n].radius);
            points.push((
                end.end,
                if segment.body != 0 {
                    end.radius
                } else {
                    radius
                },
            ));
            members.push(index);
            at = next[index];
            if members.len() > segments.len() {
                break;
            }
        }
        axes.push(Axis {
            segments: members,
            points,
            born: segment.born,
            body: segment.body,
            left: segment.left,
        });
    }
    axes
}

/// The plant's axes and how they branch.
struct Skeleton {
    axes: Vec<Axis>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    /// Each segment's axis and its place along it.
    place: Vec<Option<(usize, usize)>>,
}

impl Skeleton {
    fn new(graph: &PlantGraph) -> Self {
        let axes = axis_paths(graph);
        let mut place = vec![None; graph.segments.len()];
        for (a, axis) in axes.iter().enumerate() {
            for (k, &segment) in axis.segments.iter().enumerate() {
                place[segment] = Some((a, k));
            }
        }
        let mut children = vec![Vec::new(); axes.len()];
        let mut roots = Vec::new();
        for (a, axis) in axes.iter().enumerate() {
            let parent = axis
                .segments
                .first()
                .and_then(|&first| graph.segments[first].parent)
                .and_then(|parent| place[parent as usize]);
            match parent {
                Some((p, _)) if p != a => children[p].push(a),
                _ => roots.push(a),
            }
        }
        Self {
            axes,
            children,
            roots,
            place,
        }
    }

    /// Every axis of the subtrees of `axes`.
    fn subtree(&self, axes: &[usize]) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = axes.to_vec();
        while let Some(a) = stack.pop() {
            out.push(a);
            stack.extend_from_slice(&self.children[a]);
        }
        out
    }
}

/// One organ: a card, or a blade's pieces, and the axis that bears it.
struct Item {
    cards: Vec<Card>,
    /// Its axis and its place along it.
    axis: usize,
    k: usize,
    signature: bool,
    /// An airy organ (a grass's panicle, a spray of florets): drawn as
    /// spokes, never wrapped solid.
    airy: bool,
    /// A grass's or sedge's blade: itself, never part of a strip.
    grassy: bool,
    /// Its outline's points: base, tip, and the ends of its widest span.
    points: Vec<Vec3>,
    colour: [f64; 3],
    area: f64,
    /// Its length, its cards' end to end.
    length: f64,
    born: f64,
}

/// The organs of `chains` and the axes that bear them. A card whose
/// segment no standing axis holds is borne by the nearest that does,
/// from its segment's ancestors, or by place.
fn items(
    skeleton: &Skeleton,
    graph: &PlantGraph,
    chains: Vec<Vec<Card>>,
    looks: &[Look],
    outlines: &Outlines,
) -> Vec<Item> {
    let v = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
    let mut out = Vec::new();
    for cards in chains {
        let Some(first) = cards.first().copied() else {
            continue;
        };
        let mut place = None;
        let mut segment = (first.segment != mesh::NO_SEGMENT).then_some(first.segment as usize);
        while let Some(index) = segment.filter(|&index| index < graph.segments.len()) {
            if let Some(found) = skeleton.place[index] {
                place = Some(found);
                break;
            }
            segment = graph.segments[index].parent.map(|parent| parent as usize);
        }
        if place.is_none() {
            // The nearest point of any axis.
            let base = v(first.base);
            let mut best = (f64::INFINITY, None);
            for (a, axis) in skeleton.axes.iter().enumerate() {
                for (k, point) in axis.points.iter().enumerate() {
                    let d = (point.0 - base).length();
                    if d < best.0 {
                        best = (d, Some((a, k.saturating_sub(1))));
                    }
                }
            }
            place = best.1;
        }
        let Some((axis, k)) = place else {
            continue;
        };
        let mut points = Vec::new();
        let mut colour = [0.0; 3];
        let mut area = 0.0;
        let mut airy = false;
        let signature = looks
            .get(usize::from(first.template))
            .is_some_and(signature);
        for card in &cards {
            let Some(Some(outline)) = outlines.shapes.get(usize::from(card.template)) else {
                continue;
            };
            let (base, heading, left) = (v(card.base), v(card.heading), v(card.left));
            airy |= outline.solidity < AIRY;
            // A flower's or fruit's mass is its whole outline: a raceme's
            // flowers fill it.
            let solidity = if signature {
                1.0
            } else {
                outline.solidity.clamp(MIN_SOLIDITY, 1.0)
            };
            let half = f64::from(card.width) * 0.5 * solidity;
            let at = |x: f64, y: f64| {
                card.bend
                    .point(base, heading, left, half, f64::from(card.length), x, y)
                    .0
            };
            let (v_top, l_top, r_top) = outline
                .spans
                .iter()
                .copied()
                .max_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1)))
                .unwrap_or((0.5, -1.0, 1.0));
            points.push(at(outline.base.0, outline.base.1));
            points.push(at(outline.tip.0, outline.tip.1));
            points.push(at(l_top, v_top));
            points.push(at(r_top, v_top));
            let a =
                f64::from(card.length) * f64::from(card.width) * outline.widest * outline.solidity;
            for c in 0..3 {
                colour[c] += f64::from(card.color[c]) * outline.tint[c] * a;
            }
            area += a;
        }
        if points.is_empty() {
            continue;
        }
        let total = area.max(1.0e-12);
        out.push(Item {
            axis,
            k,
            signature,
            airy,
            grassy: looks
                .get(usize::from(first.template))
                .is_some_and(|look| matches!(look.shape, Shape::Blade(_))),
            points,
            colour: colour.map(|c| c / total),
            area,
            length: cards.iter().map(|card| f64::from(card.length)).sum(),
            born: cards
                .iter()
                .map(|card| f64::from(card.born))
                .fold(f64::INFINITY, f64::min),
            cards,
        });
    }
    out
}

/// A part of the plant still to draw.
#[derive(Debug, Clone)]
enum Kind {
    /// Sibling axes not drawn, each hanging from its point. On a woody
    /// plant, the masses their organs make. On a herb, largest first,
    /// none of them yet but the first so many that show well as a mass
    /// ([`Solid::preview`]): a fern's flat fronds, a raceme; a low-poly
    /// herb has fewer stems, not stems merged into a lump.
    Bundle(Vec<usize>, Vec<Vec3>, Option<usize>),
    /// Organs one drawn axis bears itself, as masses hanging from where
    /// they stand on its drawn path; or, leaves large beside the part
    /// ([`LEAFY`]), this many of them in their own outlines, evenly over
    /// it, and its flowers as a mass.
    Bin(usize, Vec<usize>, usize),
    /// One organ in its own outline.
    Single(usize),
}

/// A part drawn: its meshes, where they hang, and how large it shows.
struct Group {
    kind: Kind,
    wood: Mesh,
    organs: Mesh,
    pieces: Vec<Piece>,
    extent: f64,
    /// Its split, once made: `None` inside for a part that cannot split.
    split: Option<Option<Split>>,
    /// Drawn as one strip along its stalk ([`Solid::blade`]): a frond.
    strip: bool,
}

/// A part's split: the tube of an axis it draws, and the parts taking its
/// place.
struct Split {
    tube: Option<(usize, Mesh)>,
    parts: Vec<Group>,
}

impl Split {
    fn triangles(&self) -> usize {
        self.tube
            .as_ref()
            .map_or(0, |(_, mesh)| mesh.triangle_count())
            + self.parts.iter().map(Group::triangles).sum::<usize>()
    }
}

impl Group {
    fn triangles(&self) -> usize {
        self.wood.triangle_count() + self.organs.triangle_count()
    }

    fn add(&mut self, drawn: Drawn) {
        append(&mut self.wood, &drawn.wood, false);
        append(&mut self.organs, &drawn.organs, false);
        self.pieces.push(drawn.piece);
    }

    fn take(&mut self, other: Group) {
        append(&mut self.wood, &other.wood, false);
        append(&mut self.organs, &other.organs, false);
        self.pieces.extend(other.pieces);
    }
}

/// One hull drawn.
struct Drawn {
    wood: Mesh,
    organs: Mesh,
    piece: Piece,
}

/// A build under way.
struct Solid<'a> {
    skeleton: &'a Skeleton,
    items: &'a [Item],
    outlines: &'a Outlines,
    bodies: &'a [BodyLook],
    tier: &'a SolidTier,
    bark: [f32; 4],
    directions: Vec<Vec3>,
    /// Each drawn axis's path.
    drawn: Vec<Option<Vec<(Vec3, f64)>>>,
    /// The organs each axis bears itself.
    own: Vec<Vec<usize>>,
    tolerance: f64,
    /// A herb, whose stems are drawn one by one ([`Kind::Bundle`]).
    herb: bool,
    height: f64,
}

impl Solid<'_> {
    /// Split the largest part whose split fits while any does.
    fn refine(&mut self, budget: usize) -> SolidPlant {
        let roots = self.skeleton.roots.clone();
        let grounds: Vec<Vec3> = roots
            .iter()
            .map(|&a| self.skeleton.axes[a].points[0].0)
            .collect();
        let first = self.bundle(roots, grounds.clone());
        let mut groups: Vec<Group> = first.into_iter().map(|kind| self.group(kind)).collect();
        let mut tubes: Vec<(usize, Mesh)> = Vec::new();
        let mut total: usize = groups.iter().map(Group::triangles).sum();
        for _ in 0..MOST_SPLITS {
            for at in 0..groups.len() {
                if groups[at].split.is_none() {
                    let kind = groups[at].kind.clone();
                    groups[at].split = Some(self.split(&kind));
                }
            }
            let best = groups
                .iter()
                .enumerate()
                .filter(|(_, group)| {
                    matches!(&group.split, Some(Some(split)) if total + split.triangles() <= budget + group.triangles())
                })
                .max_by(|a, b| a.1.extent.total_cmp(&b.1.extent))
                .map(|(at, _)| at);
            let Some(at) = best else {
                break;
            };
            let group = groups.swap_remove(at);
            let now = group.triangles();
            let Some(Some(split)) = group.split else {
                continue;
            };
            total = total + split.triangles() - now;
            tubes.extend(split.tube);
            groups.extend(split.parts);
        }
        let mut plant = SolidPlant {
            grounds,
            ..SolidPlant::default()
        };
        for (axis, tube) in &tubes {
            append(&mut plant.wood, tube, false);
            if let Some(path) = &self.drawn[*axis] {
                plant.paths.push(path.clone());
            }
        }
        plant.stats.wood_triangles = plant.wood.triangle_count();
        let mut singles = 0;
        for group in groups {
            for piece in &group.pieces {
                if piece.single {
                    singles += 1;
                } else {
                    plant.stats.masses += 1;
                }
            }
            plant.stats.mass_triangles += group.wood.triangle_count();
            append(&mut plant.wood, &group.wood, false);
            append(&mut plant.organs, &group.organs, false);
            plant.pieces.extend(group.pieces);
        }
        plant.stats.organ_triangles = plant.organs.triangle_count();
        plant.stats.budget = budget;
        plant.stats.axes = (tubes.len(), self.skeleton.axes.len());
        plant.stats.singles = (singles, self.items.len());
        plant
    }

    /// A part's split, or `None` for one that cannot split.
    fn split(&mut self, kind: &Kind) -> Option<Split> {
        match kind {
            Kind::Single(_) => None,
            Kind::Bin(axis, members, sample) => {
                if self.leafy(members) {
                    let (flowers, leaves): (Vec<usize>, Vec<usize>) = members
                        .iter()
                        .copied()
                        .partition(|&i| self.items[i].signature);
                    if *sample < leaves.len() {
                        let more = (sample + sample.div_ceil(4)).max(SAMPLE);
                        return Some(Split {
                            tube: None,
                            parts: vec![self.group(Kind::Bin(*axis, members.clone(), more))],
                        });
                    }
                    // Every leaf drawn: they stay, and the flowers split.
                    if flowers.len() < 2 {
                        return None;
                    }
                    let mut parts: Vec<Group> = leaves
                        .iter()
                        .map(|&i| self.group(Kind::Single(i)))
                        .collect();
                    parts.push(self.group(Kind::Bin(*axis, flowers, 0)));
                    return Some(Split { tube: None, parts });
                }
                if members.len() <= 2 {
                    return Some(Split {
                        tube: None,
                        parts: members
                            .iter()
                            .map(|&i| self.group(Kind::Single(i)))
                            .collect(),
                    });
                }
                // Halves along the long axis of where the organs stand: a
                // stem's leaves below and above, a tuft's to either side.
                let sorted = self.along(members);
                let (low, high) = sorted.split_at(sorted.len() / 2);
                Some(Split {
                    tube: None,
                    parts: vec![
                        self.group(Kind::Bin(*axis, low.to_vec(), SAMPLE)),
                        self.group(Kind::Bin(*axis, high.to_vec(), SAMPLE)),
                    ],
                })
            }
            Kind::Bundle(axes, attach, Some(shown)) => {
                // A herb: the next stem shown as its mass if it shows well
                // so, else the first drawn whole.
                if *shown < axes.len() && self.preview(axes[*shown], attach[*shown]).is_some() {
                    return Some(Split {
                        tube: None,
                        parts: vec![self.group(Kind::Bundle(
                            axes.clone(),
                            attach.clone(),
                            Some(shown + 1),
                        ))],
                    });
                }
                // A stem shown flat (a fern's frond) stays so: its
                // rachis and pinnae would cost many times its one plane.
                let j = (0..axes.len()).find(|&j| {
                    j >= *shown
                        || self
                            .preview(axes[j], attach[j])
                            .is_none_or(|part| !part.strip)
                })?;
                let mut split = self.open(axes[j], attach[j]);
                if axes.len() > 1 {
                    let mut rest = axes.clone();
                    let mut points = attach.clone();
                    rest.remove(j);
                    points.remove(j);
                    let shown = if j < *shown { shown - 1 } else { *shown };
                    split
                        .parts
                        .push(self.group(Kind::Bundle(rest, points, Some(shown))));
                }
                Some(split)
            }
            Kind::Bundle(axes, attach, None) => {
                if axes.len() == 1 {
                    return Some(self.open(axes[0], attach[0]));
                }
                // Halves by where their wood stands, across the widest
                // spread of their middles.
                let middles: Vec<Vec3> = axes.iter().map(|&a| self.middle(a)).collect();
                let along = principal_axes(&middles)[0];
                let mut order: Vec<usize> = (0..axes.len()).collect();
                order.sort_by(|&i, &j| middles[i].dot(along).total_cmp(&middles[j].dot(along)));
                let half = order.len() / 2;
                let pick = |part: &[usize]| {
                    Kind::Bundle(
                        part.iter().map(|&i| axes[i]).collect(),
                        part.iter().map(|&i| attach[i]).collect(),
                        None,
                    )
                };
                Some(Split {
                    tube: None,
                    parts: vec![
                        self.group(pick(&order[..half])),
                        self.group(pick(&order[half..])),
                    ],
                })
            }
        }
    }

    /// Axis `a` drawn from `start`: its tube, a part for its branches and
    /// one for the organs it bears itself.
    fn open(&mut self, a: usize, start: Vec3) -> Split {
        let axis = &self.skeleton.axes[a];
        let mut path = simplify(&axis.points, self.tolerance, self.tier.rings.max(2));
        if let Some(first) = path.first_mut() {
            first.0 = start;
        }
        let mut tube = Mesh::default();
        self.tube(axis, &path, &mut tube);
        self.drawn[a] = Some(path.clone());
        let mut parts = Vec::new();
        let children: Vec<usize> = self.skeleton.children[a].clone();
        if !children.is_empty() {
            let points: Vec<Vec3> = children
                .iter()
                .map(|&c| nearest_on(&path, self.skeleton.axes[c].points[0].0))
                .collect();
            for bundle in self.bundle(children, points) {
                parts.push(self.group(bundle));
            }
        }
        let own = self.own[a].clone();
        if !own.is_empty() {
            parts.push(self.group(Kind::Bin(a, own, SAMPLE)));
        }
        Split {
            tube: Some((a, tube)),
            parts,
        }
    }

    /// Sibling axes as parts: a woody plant's as masses; a herb's one by
    /// one, the largest first, but those bearing only flowers (a raceme,
    /// a head, a spike's flowers each on its stalk) as masses from the
    /// start.
    fn bundle(&self, axes: Vec<usize>, attach: Vec<Vec3>) -> Vec<Kind> {
        if !self.herb {
            return vec![Kind::Bundle(axes, attach, None)];
        }
        let flowery = |a: usize| {
            let subtree = self.skeleton.subtree(&[a]);
            let mut any = false;
            for &b in &subtree {
                // A cactus's stem bears only flowers and spines, but is
                // the plant's body.
                if self.skeleton.axes[b].body != 0 {
                    return false;
                }
                for &i in &self.own[b] {
                    if !self.items[i].signature {
                        return false;
                    }
                    any = true;
                }
            }
            any
        };
        let mut order: Vec<(bool, f64, usize)> = axes
            .iter()
            .enumerate()
            .map(|(i, &a)| (flowery(a), self.reach(a), i))
            .collect();
        order.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.total_cmp(&x.1)));
        let flowery = order.iter().take_while(|x| x.0).count();
        let pick = |part: &[(bool, f64, usize)]| -> (Vec<usize>, Vec<Vec3>) {
            (
                part.iter().map(|&(_, _, i)| axes[i]).collect(),
                part.iter().map(|&(_, _, i)| attach[i]).collect(),
            )
        };
        let mut kinds = Vec::new();
        if flowery > 0 {
            let (axes, attach) = pick(&order[..flowery]);
            kinds.push(Kind::Bundle(axes, attach, None));
        }
        if flowery < order.len() {
            let (axes, attach) = pick(&order[flowery..]);
            kinds.push(Kind::Bundle(axes, attach, Some(0)));
        }
        kinds
    }

    /// The size of `axis`'s subtree: the box round its wood and organs.
    fn reach(&self, axis: usize) -> f64 {
        let subtree = self.skeleton.subtree(&[axis]);
        let mut points: Vec<Vec3> = Vec::new();
        for &a in &subtree {
            points.extend(self.skeleton.axes[a].points.iter().map(|p| p.0));
            for &i in &self.own[a] {
                points.extend_from_slice(&self.items[i].points);
            }
        }
        extent(&points)
    }

    /// A herb's stem shown as the mass it makes, if that shows it well: a
    /// stem bearing only flowers (a raceme, a head), or one drawn as a
    /// strip (a fern's frond).
    fn preview(&self, axis: usize, attach: Vec3) -> Option<Group> {
        let group = self.group(Kind::Bundle(vec![axis], vec![attach], None));
        let subtree = self.skeleton.subtree(&[axis]);
        let flowery = subtree.iter().all(|&a| {
            self.skeleton.axes[a].body == 0 && self.own[a].iter().all(|&i| self.items[i].signature)
        });
        (group.triangles() > 0 && (flowery || group.strip)).then_some(group)
    }

    /// Whether `members`' leaves are large beside the part they make, so
    /// a few of them drawn as they are show it better than a mass.
    fn leafy(&self, members: &[usize]) -> bool {
        let leaves: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&i| !self.items[i].signature)
            .collect();
        if leaves.is_empty() {
            return false;
        }
        let points: Vec<Vec3> = leaves
            .iter()
            .flat_map(|&i| self.items[i].points.iter().copied())
            .collect();
        let mean = leaves.iter().map(|&i| self.items[i].length).sum::<f64>() / leaves.len() as f64;
        mean >= LEAFY * extent(&points)
    }

    /// `members` in order along the long axis of where they stand.
    fn along(&self, members: &[usize]) -> Vec<usize> {
        let middle = |i: usize| {
            let points = &self.items[i].points;
            points.iter().fold(Vec3::ZERO, |a, &b| a + b) / points.len() as f64
        };
        let middles: Vec<Vec3> = members.iter().map(|&i| middle(i)).collect();
        let along = principal_axes(&middles)[0];
        let mut order: Vec<usize> = members.to_vec();
        order.sort_by(|&a, &b| middle(a).dot(along).total_cmp(&middle(b).dot(along)));
        order
    }

    /// The middle of the wood of `axis`'s subtree.
    fn middle(&self, axis: usize) -> Vec3 {
        let subtree = self.skeleton.subtree(&[axis]);
        let mut sum = Vec3::ZERO;
        let mut count = 0.0;
        for &a in &subtree {
            for point in &self.skeleton.axes[a].points {
                sum += point.0;
                count += 1.0;
            }
        }
        sum / f64::max(count, 1.0)
    }

    /// Draw a part.
    fn group(&self, kind: Kind) -> Group {
        let mut group = Group {
            kind: kind.clone(),
            wood: Mesh::default(),
            organs: Mesh::default(),
            pieces: Vec::new(),
            extent: 0.0,
            split: None,
            strip: false,
        };
        match &kind {
            Kind::Single(i) if self.items[*i].signature && !self.items[*i].airy => {
                if let Some(mut drawn) = self.mass(&[*i], &[self.items[*i].points[0]]) {
                    drawn.piece.single = true;
                    group.add(drawn);
                }
            }
            Kind::Single(i) => self.leaf(*i, 1.0, &mut group),
            Kind::Bin(axis, members, sample) => {
                let path = self.drawn[*axis].clone().unwrap_or_default();
                // Each class hangs from the stretch of the path its organs
                // stand along.
                let attach_of = |class: &[usize]| -> Vec<Vec3> {
                    let points = &self.skeleton.axes[*axis].points;
                    let first = class.iter().map(|&i| self.items[i].k).min().unwrap_or(0);
                    let last = class.iter().map(|&i| self.items[i].k).max().unwrap_or(0);
                    vec![
                        nearest_on(&path, points[first.min(points.len() - 1)].0),
                        nearest_on(&path, points[(last + 1).min(points.len() - 1)].0),
                    ]
                };
                let (flowers, leaves): (Vec<usize>, Vec<usize>) = members
                    .iter()
                    .copied()
                    .partition(|&i| self.items[i].signature);
                if self.leafy(members) {
                    // A few of its leaves, evenly along it, as they are.
                    let leaves = self.along(&leaves);
                    let take = (*sample).clamp(1, leaves.len());
                    // Narrow ones (blades) widen for those left out,
                    // from where they stand, so a tuft stays a tuft.
                    let share = (take as f64 / leaves.len() as f64)
                        .sqrt()
                        .max(1.0 / WIDEST_BLADE);
                    for j in 0..take {
                        let i = leaves[(j * leaves.len() + leaves.len() / 2) / take];
                        self.leaf(i, share, &mut group);
                    }
                } else if let Some(drawn) = self.mass(&leaves, &attach_of(&leaves)) {
                    group.add(drawn);
                }
                if let Some(drawn) = self.mass(&flowers, &attach_of(&flowers)) {
                    group.add(drawn);
                }
            }
            Kind::Bundle(axes, attach, Some(shown)) => {
                for (&a, &p) in axes.iter().zip(attach).take(*shown) {
                    if let Some(part) = self.preview(a, p) {
                        group.take(part);
                    }
                }
                // Stems not drawn stand for less the more of their
                // siblings are, as sampled leaves do.
                let sibling = axes.first().map_or(0, |&a| a);
                let parent = self
                    .skeleton
                    .children
                    .iter()
                    .position(|c| c.contains(&sibling));
                let siblings = parent.map_or(&self.skeleton.roots, |p| &self.skeleton.children[p]);
                let drawn = siblings
                    .iter()
                    .filter(|&&a| self.drawn[a].is_some())
                    .count();
                group.extent = axes.first().map_or(0.0, |&a| self.reach(a)) * SAMPLE as f64
                    / (SAMPLE + drawn) as f64;
                return group;
            }
            Kind::Bundle(axes, attach, None) => {
                let subtree = self.skeleton.subtree(axes);
                let members: Vec<usize> = subtree
                    .iter()
                    .flat_map(|&a| self.own[a].iter().copied())
                    .collect();
                let (flowers, leaves): (Vec<usize>, Vec<usize>) = members
                    .iter()
                    .copied()
                    .partition(|&i| self.items[i].signature);
                let wood = self.wood_points(&subtree);
                // Fleshy bodies are the plant's outline, in their colour,
                // and organs hang from them; else leaves make the mass and
                // flowers and fruit hang from it; bare wood is a mass in
                // bark.
                let fleshy = subtree
                    .iter()
                    .map(|&a| self.skeleton.axes[a].body)
                    .find(|&b| b != 0);
                let (base, hanging): (Option<Drawn>, Vec<&Vec<usize>>) = if let Some(body) = fleshy
                {
                    let mut points = wood.clone();
                    points.extend_from_slice(attach);
                    (
                        self.hull(&points, attach, self.body_colour(body), 0.0),
                        vec![&leaves, &flowers],
                    )
                } else if members.is_empty() {
                    // Bare wood waits to be drawn as its branches: a
                    // bark-coloured lump inside a crown reads as nothing.
                    group.extent = extent(&wood);
                    return group;
                } else if leaves.is_empty() {
                    (self.mass(&flowers, attach), Vec::new())
                } else {
                    // Only many small organs make a strip (a frond's
                    // pinnae); a culm's few long blades are themselves.
                    let mean = leaves.iter().map(|&i| self.items[i].length).sum::<f64>()
                        / leaves.len() as f64;
                    let stalk: f64 = self.skeleton.axes[axes[0]]
                        .points
                        .windows(2)
                        .map(|pair| (pair[1].0 - pair[0].0).length())
                        .sum();
                    let blade = (axes.len() == 1
                        && leaves.iter().filter(|&&i| self.items[i].grassy).count() * 2
                            < leaves.len()
                        && leaves.len() >= BLADE_MANY
                        && mean < BLADE_SMALL * stalk)
                        .then(|| {
                            let mut path = simplify(
                                &self.skeleton.axes[axes[0]].points,
                                self.tolerance,
                                self.tier.rings.max(2),
                            );
                            path[0].0 = attach[0];
                            let points: Vec<Vec3> = leaves
                                .iter()
                                .flat_map(|&i| self.items[i].points.iter().copied())
                                .collect();
                            self.blade(
                                &path,
                                &points,
                                attach,
                                self.colour_of(&leaves),
                                self.born_of(&leaves),
                            )
                        })
                        .flatten();
                    group.strip = blade.is_some();
                    (blade.or_else(|| self.mass(&leaves, attach)), vec![&flowers])
                };
                let Some(base) = base else {
                    return group;
                };
                let anchors = base.piece.points.clone();
                let whole = extent(&base.piece.points);
                group.add(base);
                for class in hanging {
                    if class.is_empty() {
                        continue;
                    }
                    let points: Vec<Vec3> = class
                        .iter()
                        .flat_map(|&i| self.items[i].points.iter().copied())
                        .collect();
                    // Flowers or fruit strewn through a crown (a maple's
                    // samaras) wait for a finer split; a spike or a head
                    // shows.
                    if fleshy.is_none() && extent(&points) > SCATTERED * whole {
                        continue;
                    }
                    let centre =
                        points.iter().fold(Vec3::ZERO, |a, &b| a + b) / points.len() as f64;
                    let touch = anchors
                        .iter()
                        .copied()
                        .min_by(|a, b| (*a - centre).length().total_cmp(&(*b - centre).length()))
                        .unwrap_or(attach[0]);
                    if let Some(drawn) = self.mass(class, &[touch]) {
                        group.add(drawn);
                    }
                }
            }
        }
        let all: Vec<Vec3> = group
            .pieces
            .iter()
            .flat_map(|piece| piece.points.iter().copied())
            .collect();
        group.extent = extent(&all);
        // Sampled leaves stand for the rest by how far apart they are.
        if let Kind::Bin(_, members, sample) = &kind {
            if self.leafy(members) {
                group.extent *= SAMPLE as f64 / (*sample).max(1) as f64;
            }
        }
        group
    }

    /// Organ `i` in its own outline, a narrow one widened for `share`
    /// of its kind drawn.
    fn leaf(&self, i: usize, share: f64, group: &mut Group) {
        let item = &self.items[i];
        let mut organs = Mesh::default();
        if item.cards.len() == 1 {
            self.outlines
                .polygon(&item.cards[0], share, 1.0, &mut organs);
        } else {
            self.outlines
                .ribbon(&item.cards, share, self.tier.stations + 2, &mut organs);
        }
        // Set on the stem as drawn: the drawn path strays a little from
        // the real one, and the organ moves with it.
        let base = item.points[0];
        let shift = self.drawn[item.axis]
            .as_ref()
            .map_or(Vec3::ZERO, |path| nearest_on(path, base) - base);
        #[allow(clippy::cast_possible_truncation)]
        for p in &mut organs.positions {
            *p = [
                p[0] + shift.x as f32,
                p[1] + shift.y as f32,
                p[2] + shift.z as f32,
            ];
        }
        group.pieces.push(Piece {
            attach: vec![base + shift],
            points: organs.positions.iter().map(|p| v32(*p)).collect(),
            single: true,
        });
        append(&mut group.organs, &organs, false);
    }

    /// Organs spread flat to either side of `path` (a fern's pinnae on
    /// its rachis, a cedar's spray) as one strip along it, as wide each
    /// side at each point as they reach there and turned the way they
    /// spread: the frond's own outline and arch. `None` when they stand
    /// round it rather than flat.
    fn blade(
        &self,
        path: &[(Vec3, f64)],
        points: &[Vec3],
        attach: &[Vec3],
        colour: [f32; 4],
        born: f64,
    ) -> Option<Drawn> {
        let n = path.len();
        if n < 2 || points.len() < 3 {
            return None;
        }
        // Each point's station (the nearer end of its nearest stretch)
        // and its offset from the path, across it.
        let mut at: Vec<Vec<Vec3>> = vec![Vec::new(); n];
        for &p in points {
            let mut best = (f64::INFINITY, 0, Vec3::ZERO);
            for k in 0..n - 1 {
                let (a, b) = (path[k].0, path[k + 1].0);
                let ab = b - a;
                let t = if ab.dot(ab) > 0.0 {
                    ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let q = a + ab * t;
                let d = (q - p).length();
                if d < best.0 {
                    let tangent = ab.normalize_or(Vec3::Y);
                    let offset = p - q;
                    best = (
                        d,
                        if t < 0.5 { k } else { k + 1 },
                        offset - tangent * offset.dot(tangent),
                    );
                }
            }
            at[best.1].push(best.2);
        }
        let all: Vec<Vec3> = at.iter().flatten().copied().collect();
        // Offsets lie across the path: the third axis is along it.
        let [wide, thin, _] = principal_axes(&all);
        let reach = |d: Vec3, set: &[Vec3]| set.iter().map(|o| o.dot(d).abs()).fold(0.0, f64::max);
        // A strip runs along a stalk longer than its organs reach across
        // it (a frond, a spray), not round a rosette's point.
        let length: f64 = path
            .windows(2)
            .map(|pair| (pair[1].0 - pair[0].0).length())
            .sum();
        if reach(thin, &all) > BLADE_THIN * reach(wide, &all) || length < reach(wide, &all) {
            return None;
        }
        let mut side = wide;
        let mut ring_left = Vec::with_capacity(n);
        let mut ring_right = Vec::with_capacity(n);
        for k in 0..n {
            let tangent =
                (path[(k + 1).min(n - 1)].0 - path[k.saturating_sub(1)].0).normalize_or(Vec3::Y);
            if at[k].len() >= 3 {
                let local = principal_axes(&at[k])[0];
                side = if local.dot(side) < 0.0 { -local } else { local };
            }
            side = (side - tangent * side.dot(tangent))
                .normalize_or_else(|| crate::math::any_perpendicular(tangent));
            let (mut left, mut right) = (0.0_f64, 0.0_f64);
            for o in &at[k] {
                let d = o.dot(side);
                if d > 0.0 {
                    right = right.max(d)
                } else {
                    left = left.max(-d)
                }
            }
            ring_left.push(path[k].0 - side * left);
            ring_right.push(path[k].0 + side * right);
        }
        let mut organs = Mesh::default();
        let vertex = |position: Vec3, normal: Vec3| Vertex {
            position,
            normal,
            uv: [0.0, 0.0],
            color: colour,
            born,
            shed: None,
            level: 3,
            segment: mesh::NO_SEGMENT,
        };
        for k in 0..n - 1 {
            for [a, b, c] in [
                [ring_left[k], ring_right[k], ring_right[k + 1]],
                [ring_left[k], ring_right[k + 1], ring_left[k + 1]],
            ] {
                let cross = (b - a).cross(c - a);
                if cross.length() < 1.0e-9 {
                    continue;
                }
                let normal = cross.normalize_or(Vec3::Y);
                let first = organs.push(vertex(a, normal));
                organs.push(vertex(b, normal));
                organs.push(vertex(c, normal));
                organs
                    .indices
                    .extend_from_slice(&[first, first + 1, first + 2]);
            }
        }
        if organs.triangle_count() == 0 {
            return None;
        }
        let drawn = organs.positions.iter().map(|p| v32(*p)).collect();
        Some(Drawn {
            wood: Mesh::default(),
            organs,
            piece: Piece {
                attach: attach.to_vec(),
                points: drawn,
                single: false,
            },
        })
    }

    /// Points round the wood of `axes`: each path point and four points
    /// a radius out across it, a body's flattened as its cross-section.
    fn wood_points(&self, axes: &[usize]) -> Vec<Vec3> {
        let mut wood = Vec::new();
        for &a in axes {
            let axis = &self.skeleton.axes[a];
            let flatten = self.flatten(axis);
            for (k, &(place, radius)) in axis.points.iter().enumerate() {
                let next = axis
                    .points
                    .get(k + 1)
                    .map_or(place - axis.points[k.saturating_sub(1)].0, |n| n.0 - place);
                let tangent = next.normalize_or(Vec3::Y);
                let side = (axis.left - tangent * axis.left.dot(tangent))
                    .normalize_or_else(|| crate::math::any_perpendicular(tangent));
                let other = tangent.cross(side);
                for (s, o) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    wood.push(place + side * (radius * s) + other * (radius * flatten * o));
                }
            }
        }
        wood
    }

    /// The points a mass wraps for organ `i`: its outline's, and a
    /// flower or fruit's widest span turned a quarter about its length
    /// too, so a raceme or a spike is a spindle, not a sliver edge on.
    fn points_of(&self, i: usize) -> Vec<Vec3> {
        let item = &self.items[i];
        let mut points = item.points.clone();
        if item.signature {
            for card in item.points.chunks_exact(4) {
                let length = (card[1] - card[0]).normalize_or(Vec3::Y);
                let middle = (card[2] + card[3]) * 0.5;
                let turned = length.cross((card[3] - card[2]) * 0.5);
                points.push(middle + turned);
                points.push(middle - turned);
            }
        }
        points
    }

    /// The mass of organs `class`, wrapping `attach`.
    fn mass(&self, class: &[usize], attach: &[Vec3]) -> Option<Drawn> {
        if class.is_empty() {
            return None;
        }
        if class
            .iter()
            .all(|&i| self.items[i].airy && self.items[i].signature)
        {
            // Airy organs stay their spokes: wrapped they would be solid
            // rock where the plant has a haze.
            let mut organs = Mesh::default();
            for &i in class {
                for card in &self.items[i].cards {
                    self.outlines.polygon(card, 1.0, 1.0, &mut organs);
                }
            }
            let points = organs.positions.iter().map(|p| v32(*p)).collect();
            return (organs.triangle_count() > 0).then(|| Drawn {
                wood: Mesh::default(),
                organs,
                piece: Piece {
                    // Each spray hangs from its own foot on the stem.
                    attach: class.iter().map(|&i| self.items[i].points[0]).collect(),
                    points,
                    single: false,
                },
            });
        }
        let mut points: Vec<Vec3> = class.iter().flat_map(|&i| self.points_of(i)).collect();
        points.extend_from_slice(attach);
        self.hull(&points, attach, self.colour_of(class), self.born_of(class))
    }

    /// The tier's directions in the frame of `points`' principal axes, so
    /// a mass follows its own length, width and thickness: a stem's leaves
    /// a spindle, a frond a flat diamond.
    fn directions_of(&self, points: &[Vec3]) -> Vec<Vec3> {
        let axes = principal_axes(points);
        self.directions
            .iter()
            .map(|d| (axes[0] * d.x + axes[1] * d.y + axes[2] * d.z).normalize_or(Vec3::Y))
            .collect()
    }

    fn colour_of(&self, class: &[usize]) -> [f32; 4] {
        let mut colour = [0.0; 3];
        let mut total = 0.0;
        for &i in class {
            let item = &self.items[i];
            for c in 0..3 {
                colour[c] += item.colour[c] * item.area;
            }
            total += item.area;
        }
        let total = f64::max(total, 1.0e-12);
        #[allow(clippy::cast_possible_truncation)]
        let colour = [
            (colour[0] / total) as f32,
            (colour[1] / total) as f32,
            (colour[2] / total) as f32,
            1.0,
        ];
        colour
    }

    fn born_of(&self, class: &[usize]) -> f64 {
        let born = class
            .iter()
            .map(|&i| self.items[i].born)
            .fold(f64::INFINITY, f64::min);
        if born.is_finite() { born } else { 0.0 }
    }

    /// How much thinner than wide a body's cross-section is: 1 round.
    fn flatten(&self, axis: &Axis) -> f64 {
        if axis.body == 0 {
            1.0
        } else {
            self.bodies
                .get(usize::from(axis.body - 1))
                .map_or(1.0, |look| look.flatten.clamp(0.05, 1.0))
        }
    }

    fn body_colour(&self, body: u8) -> [f32; 4] {
        self.bodies
            .get(usize::from(body.saturating_sub(1)))
            .map_or(self.bark, |look| {
                let (tint, cover) = look.tint();
                let mix = |a: f32, b: f32| a + (b - a) * cover as f32;
                [
                    mix(look.colour[0], tint[0]),
                    mix(look.colour[1], tint[1]),
                    mix(look.colour[2], tint[2]),
                    1.0,
                ]
            })
    }

    /// The convex hull of `points` along the tier's directions (and
    /// `attach`, always on it or inside), flat-shaded: closed, or a flat
    /// polygon drawn two-sided when thin; `None` for a degenerate one.
    fn hull(&self, points: &[Vec3], attach: &[Vec3], colour: [f32; 4], born: f64) -> Option<Drawn> {
        let mut candidates: Vec<Vec3> = attach.to_vec();
        for d in &self.directions_of(points) {
            let most = points
                .iter()
                .copied()
                .max_by(|a, b| a.dot(*d).total_cmp(&b.dot(*d)));
            let least = points
                .iter()
                .copied()
                .min_by(|a, b| a.dot(*d).total_cmp(&b.dot(*d)));
            candidates.extend(most);
            candidates.extend(least);
        }
        let size = extent(&candidates).max(1.0e-6);
        let mut unique: Vec<Vec3> = Vec::new();
        for p in candidates {
            if unique.iter().all(|q| (*q - p).length() > size * 1.0e-3) {
                unique.push(p);
            }
        }
        let vertex = |position: Vec3, normal: Vec3| Vertex {
            position,
            normal,
            uv: [0.0, 0.0],
            color: colour,
            born,
            shed: None,
            level: 3,
            segment: mesh::NO_SEGMENT,
        };
        let (mut wood, mut organs) = (Mesh::default(), Mesh::default());
        match hull3(&unique, size) {
            Hull::Solid(faces) => {
                for [a, b, c] in faces {
                    let (pa, pb, pc) = (unique[a], unique[b], unique[c]);
                    let cross = (pb - pa).cross(pc - pa);
                    let normal = cross.normalize_or(Vec3::Y);
                    let first = wood.push(vertex(pa, normal));
                    wood.push(vertex(pb, normal));
                    wood.push(vertex(pc, normal));
                    wood.indices
                        .extend_from_slice(&[first, first + 1, first + 2]);
                }
            }
            Hull::Flat(ring, normal) => {
                let first = u32::try_from(organs.positions.len()).unwrap_or(0);
                for &p in &ring {
                    organs.push(vertex(unique[p], normal));
                }
                for k in 1..ring.len().saturating_sub(1) {
                    let k = u32::try_from(k).unwrap_or(0);
                    organs
                        .indices
                        .extend_from_slice(&[first, first + k, first + k + 1]);
                }
            }
            Hull::None => return None,
        }
        let mut drawn: Vec<Vec3> = wood.positions.iter().map(|p| v32(*p)).collect();
        drawn.extend(organs.positions.iter().map(|p| v32(*p)));
        Some(Drawn {
            wood,
            organs,
            piece: Piece {
                attach: attach.to_vec(),
                points: drawn,
                single: false,
            },
        })
    }

    /// `axis` drawn along `path` as a tube of the tier's sides, or a body
    /// of its own cross-section and a rounded tip.
    fn tube(&self, axis: &Axis, path: &[(Vec3, f64)], out: &mut Mesh) {
        if path.len() < 2 {
            return;
        }
        // A body slim beside the plant (a cholla's joint) takes the
        // wood's sides: its many joints show the plant better than a few
        // round ones.
        let widest = path.iter().map(|p| p.1).fold(0.0, f64::max);
        let (sides, colour, flatten) = if axis.body == 0 {
            (self.tier.sides, self.bark, 1.0)
        } else if widest < SLIM * self.height {
            (
                self.tier.sides,
                self.body_colour(axis.body),
                self.flatten(axis),
            )
        } else {
            (
                self.tier.body_sides,
                self.body_colour(axis.body),
                self.flatten(axis),
            )
        };
        // A body's dome lies within its length: the last ring stands
        // where the dome begins and the apex at the very tip.
        let mut path = path.to_vec();
        let mut apex_at = None;
        if axis.body != 0 && path.len() >= 2 {
            let n = path.len();
            let (end, radius) = path[n - 1];
            let before = path[n - 2].0;
            let dome = self
                .bodies
                .get(usize::from(axis.body - 1))
                .map_or(1.0, |look| look.dome.clamp(0.1, 2.0));
            let back = (radius * dome).min((end - before).length() * 0.5);
            path[n - 1].0 = end - (end - before).normalize_or(Vec3::Y) * back;
            apex_at = Some(end);
        }
        let path = &path[..];
        let first = u32::try_from(out.positions.len()).unwrap_or(0);
        let level = 0;
        let mut side = axis.left;
        let mut tangent = Vec3::Y;
        for (k, &(place, radius)) in path.iter().enumerate() {
            let before = if k > 0 {
                place - path[k - 1].0
            } else {
                Vec3::ZERO
            };
            let after = if k + 1 < path.len() {
                path[k + 1].0 - place
            } else {
                Vec3::ZERO
            };
            tangent = (before.normalize_or(Vec3::ZERO) + after.normalize_or(Vec3::ZERO))
                .normalize_or(Vec3::Y);
            side = (side - tangent * side.dot(tangent))
                .normalize_or_else(|| crate::math::any_perpendicular(tangent));
            let other = tangent.cross(side);
            // Out far enough that a triangle's mean width is the stem's.
            let reach = if sides == 3 {
                radius * std::f64::consts::PI / (3.0 * (std::f64::consts::PI / 3.0).sin())
            } else {
                radius
            };
            for s in 0..sides {
                let angle = std::f64::consts::TAU * f64::from(s) / f64::from(sides);
                let out_dir = side * angle.cos() + other * (angle.sin() * flatten);
                out.push(Vertex {
                    position: place + out_dir * reach,
                    normal: (side * angle.cos() + other * angle.sin()).normalize_or(Vec3::Y),
                    uv: [0.0, 0.0],
                    color: colour,
                    born: axis.born,
                    shed: None,
                    level,
                    segment: mesh::NO_SEGMENT,
                });
            }
        }
        let rings = u32::try_from(path.len()).unwrap_or(0);
        for ring in 0..rings - 1 {
            for s in 0..sides {
                let a = first + ring * sides + s;
                let b = first + ring * sides + (s + 1) % sides;
                let (c, d) = (a + sides, b + sides);
                out.indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        if axis.body != 0 {
            // A rounded tip: a point half a radius past the last ring.
            let (end, radius) = path[path.len() - 1];
            let apex = out.push(Vertex {
                position: apex_at.unwrap_or(end + tangent * (radius * 0.5)),
                normal: tangent,
                uv: [0.0, 0.0],
                color: colour,
                born: axis.born,
                shed: None,
                level,
                segment: mesh::NO_SEGMENT,
            });
            let last = first + (rings - 1) * sides;
            for s in 0..sides {
                out.indices
                    .extend_from_slice(&[last + s, last + (s + 1) % sides, apex]);
            }
        }
    }
}

fn v32(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

/// The principal axes of `points`, longest first (Jacobi's rotations of
/// their covariance); the world's axes for fewer than three.
fn principal_axes(points: &[Vec3]) -> [Vec3; 3] {
    if points.len() < 3 {
        return [Vec3::X, Vec3::Y, Vec3::Z];
    }
    let n = points.len() as f64;
    let mean = points.iter().fold(Vec3::ZERO, |a, &b| a + b) / n;
    let mut m = [[0.0_f64; 3]; 3];
    for p in points {
        let d = [p.x - mean.x, p.y - mean.y, p.z - mean.z];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += d[i] * d[j] / n;
            }
        }
    }
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..24 {
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if m[p][q].abs() < 1.0e-18 {
                continue;
            }
            let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (mkp, mkq) = (m[k][p], m[k][q]);
                m[k][p] = c * mkp - s * mkq;
                m[k][q] = s * mkp + c * mkq;
            }
            for k in 0..3 {
                let (mpk, mqk) = (m[p][k], m[q][k]);
                m[p][k] = c * mpk - s * mqk;
                m[q][k] = s * mpk + c * mqk;
            }
            for row in &mut v {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut order = [0, 1, 2];
    order.sort_by(|&a, &b| m[b][b].total_cmp(&m[a][a]));
    let column = |k: usize| Vec3::new(v[0][k], v[1][k], v[2][k]).normalize_or(Vec3::Y);
    let first = column(order[0]);
    let second = column(order[1]);
    let third = first.cross(second).normalize_or_else(|| column(order[2]));
    [first, second, third]
}

/// The diagonal of the box round `points`.
fn extent(points: &[Vec3]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let (low, high) = points
        .iter()
        .fold((points[0], points[0]), |(l, h), &p| (l.min(p), h.max(p)));
    (high - low).length()
}

/// The point of `path` nearest `p`; `p` itself for no path.
fn nearest_on(path: &[(Vec3, f64)], p: Vec3) -> Vec3 {
    let mut best = (f64::INFINITY, p);
    if path.len() == 1 {
        return path[0].0;
    }
    for pair in path.windows(2) {
        let (a, b) = (pair[0].0, pair[1].0);
        let ab = b - a;
        let t = if ab.dot(ab) > 0.0 {
            ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = a + ab * t;
        let d = (q - p).length();
        if d < best.0 {
            best = (d, q);
        }
    }
    best.1
}

/// A convex hull: its faces outward, or a flat polygon's ring and the
/// way it faces.
enum Hull {
    Solid(Vec<[usize; 3]>),
    Flat(Vec<usize>, Vec3),
    None,
}

/// The convex hull of a few points (incremental, Clarkson and Shor's
/// order of insertion aside), flat when thinner than [`FLAT`] of `size`.
fn hull3(points: &[Vec3], size: f64) -> Hull {
    let n = points.len();
    if n < 3 {
        return Hull::None;
    }
    // The farthest pair, the farthest from their line, and from their plane.
    let mut pair = (0, 1, 0.0);
    for i in 0..n {
        for j in i + 1..n {
            let d = (points[i] - points[j]).length();
            if d > pair.2 {
                pair = (i, j, d);
            }
        }
    }
    let (i0, i1) = (pair.0, pair.1);
    let line = (points[i1] - points[i0]).normalize_or(Vec3::Y);
    let off_line = |p: Vec3| {
        let o = p - points[i0];
        (o - line * o.dot(line)).length()
    };
    let i2 = (0..n)
        .max_by(|&a, &b| off_line(points[a]).total_cmp(&off_line(points[b])))
        .unwrap_or(0);
    let normal = (points[i1] - points[i0])
        .cross(points[i2] - points[i0])
        .normalize_or(Vec3::ZERO);
    if normal == Vec3::ZERO || off_line(points[i2]) < size * 1.0e-4 {
        return Hull::None;
    }
    let off_plane = |p: Vec3| (p - points[i0]).dot(normal);
    let i3 = (0..n)
        .max_by(|&a, &b| {
            off_plane(points[a])
                .abs()
                .total_cmp(&off_plane(points[b]).abs())
        })
        .unwrap_or(0);
    if off_plane(points[i3]).abs() < FLAT * pair.2 {
        // Flat: the 2D hull in the plane (Andrew's monotone chain).
        let u = line;
        let w = normal.cross(u);
        let mut order: Vec<usize> = (0..n).collect();
        let at = |i: usize| {
            (
                (points[i] - points[i0]).dot(u),
                (points[i] - points[i0]).dot(w),
            )
        };
        order.sort_by(|&a, &b| {
            at(a)
                .0
                .total_cmp(&at(b).0)
                .then(at(a).1.total_cmp(&at(b).1))
        });
        let cross = |o: usize, a: usize, b: usize| {
            let (o, a, b) = (at(o), at(a), at(b));
            (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
        };
        let mut ring: Vec<usize> = Vec::new();
        for pass in 0..2 {
            let start = ring.len();
            let sequence: Vec<usize> = if pass == 0 {
                order.clone()
            } else {
                order.iter().rev().copied().collect()
            };
            for &p in &sequence {
                while ring.len() >= start + 2
                    && cross(ring[ring.len() - 2], ring[ring.len() - 1], p) <= 0.0
                {
                    ring.pop();
                }
                ring.push(p);
            }
            ring.pop();
        }
        if ring.len() < 3 {
            return Hull::None;
        }
        return Hull::Flat(ring, normal);
    }
    let eps = size * 1.0e-7;
    let centre = (points[i0] + points[i1] + points[i2] + points[i3]) / 4.0;
    let orient = |f: [usize; 3]| -> [usize; 3] {
        let (a, b, c) = (points[f[0]], points[f[1]], points[f[2]]);
        if (b - a).cross(c - a).dot(centre - a) > 0.0 {
            [f[0], f[2], f[1]]
        } else {
            f
        }
    };
    let mut faces: Vec<[usize; 3]> = vec![
        orient([i0, i1, i2]),
        orient([i0, i1, i3]),
        orient([i1, i2, i3]),
        orient([i0, i2, i3]),
    ];
    for p in 0..n {
        if [i0, i1, i2, i3].contains(&p) {
            continue;
        }
        let visible: Vec<bool> = faces
            .iter()
            .map(|f| {
                let (a, b, c) = (points[f[0]], points[f[1]], points[f[2]]);
                let normal = (b - a).cross(c - a).normalize_or(Vec3::ZERO);
                normal.dot(points[p] - a) > eps
            })
            .collect();
        if !visible.iter().any(|&v| v) {
            continue;
        }
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for (f, face) in faces.iter().enumerate() {
            if visible[f] {
                for e in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
                    edges.push(e);
                }
            }
        }
        let horizon: Vec<(usize, usize)> = edges
            .iter()
            .copied()
            .filter(|&(a, b)| !edges.contains(&(b, a)))
            .collect();
        let mut kept: Vec<[usize; 3]> = faces
            .iter()
            .zip(&visible)
            .filter(|(_, v)| !**v)
            .map(|(f, _)| *f)
            .collect();
        for (a, b) in horizon {
            kept.push([a, b, p]);
        }
        faces = kept;
    }
    Hull::Solid(faces)
}

/// At most `most` of `points`, the ends and those that stray furthest
/// from the line between the ones kept, until none strays more than
/// `tolerance`.
fn simplify(points: &[(Vec3, f64)], tolerance: f64, most: usize) -> Vec<(Vec3, f64)> {
    let last = points.len() - 1;
    let mut kept = vec![0, last];
    while kept.len() < most {
        let mut worst = (tolerance, None);
        for pair in kept.windows(2) {
            let (a, b) = (points[pair[0]].0, points[pair[1]].0);
            let (ra, rb) = (points[pair[0]].1, points[pair[1]].1);
            let span = (b - a).length().max(1.0e-12);
            let along = (b - a).normalize_or(Vec3::Y);
            for index in pair[0] + 1..pair[1] {
                let offset = points[index].0 - a;
                // Off the line, or off the radius it would give there: a
                // pad's swell is its shape.
                let t = (offset.dot(along) / span).clamp(0.0, 1.0);
                let swell = (points[index].1 - (ra + (rb - ra) * t)).abs();
                let off = (offset - along * offset.dot(along)).length().max(swell);
                if off > worst.0 {
                    worst = (off, Some(index));
                }
            }
        }
        let Some(index) = worst.1 else { break };
        kept.push(index);
        kept.sort_unstable();
    }
    kept.into_iter().map(|index| points[index]).collect()
}

/// `cards` as organs, in an order hashed from their places: a card alone,
/// or a chain of cards each standing where the one before it ends (a
/// grass blade's pieces), borne by one segment and cut by one template.
fn chains(cards: Vec<Card>) -> Vec<Vec<Card>> {
    let tip = |card: &Card| {
        let v = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
        card.bend
            .point(
                v(card.base),
                v(card.heading),
                v(card.left),
                f64::from(card.width) * 0.5,
                f64::from(card.length),
                0.0,
                1.0,
            )
            .0
    };
    let v3 = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
    let place = |card: &Card| {
        Vec3::new(
            f64::from(card.base[0]),
            f64::from(card.base[1]),
            f64::from(card.base[2]),
        )
    };
    // Grouped by segment only: a blade's tip is a template of its own.
    let mut groups: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    let mut out: Vec<Vec<Card>> = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        if card.segment == mesh::NO_SEGMENT {
            out.push(vec![*card]);
        } else {
            groups.entry(card.segment).or_default().push(index);
        }
    }
    for members in groups.values() {
        // Each card's follower: the one whose base is nearest its tip,
        // within a fifth of its length, the closest pairs joined first so
        // no piece of a blade is left starting a chain of its own.
        let mut follower = vec![None; members.len()];
        let mut followed = vec![false; members.len()];
        let mut pairs = Vec::new();
        // Bases by cell, a cell as wide as the farthest reach.
        let cell = members
            .iter()
            .map(|&a| f64::from(cards[a].length) * 0.2)
            .fold(1.0e-6, f64::max);
        let key = |p: Vec3| {
            (
                (p.x / cell).floor() as i64,
                (p.y / cell).floor() as i64,
                (p.z / cell).floor() as i64,
            )
        };
        let mut cells: std::collections::HashMap<(i64, i64, i64), Vec<usize>> =
            std::collections::HashMap::new();
        for (j, &b) in members.iter().enumerate() {
            cells.entry(key(place(&cards[b]))).or_default().push(j);
        }
        for (i, &a) in members.iter().enumerate() {
            let end = tip(&cards[a]);
            let reach = f64::from(cards[a].length) * 0.2;
            let (x, y, z) = key(end);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for &j in cells
                            .get(&(x + dx, y + dy, z + dz))
                            .map_or(&[][..], Vec::as_slice)
                        {
                            let next = &cards[members[j]];
                            let gap = (place(next) - end).length();
                            // A blade goes on the way it was heading and
                            // no wider: not into a neighbour's base.
                            let ahead = v3(next.heading).dot(v3(cards[a].heading));
                            if i != j
                                && gap < reach
                                && ahead > 0.7
                                && next.width <= cards[a].width * 1.2
                            {
                                pairs.push((gap, i, j));
                            }
                        }
                    }
                }
            }
        }
        pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
        for (_, i, j) in pairs {
            if follower[i].is_none() && !followed[j] {
                follower[i] = Some(j);
                followed[j] = true;
            }
        }
        for start in 0..members.len() {
            if followed[start] {
                continue;
            }
            let mut chain = Vec::new();
            let mut at = Some(start);
            while let Some(i) = at {
                chain.push(cards[members[i]]);
                at = follower[i];
                if chain.len() > members.len() {
                    break;
                }
            }
            out.push(chain);
        }
    }
    let mut keyed: Vec<(f64, Vec<Card>)> = out
        .into_iter()
        .map(|chain| (hash(&chain[0]), chain))
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    keyed.into_iter().map(|(_, chain)| chain).collect()
}

/// A number from 0 to 1 drawn from a card's place.
fn hash(card: &Card) -> f64 {
    let bits = card
        .base
        .iter()
        .fold(0x9e37_79b9_7f4a_7c15_u64, |acc, value| {
            mix64(acc ^ u64::from(value.to_bits()))
        });
    unit(bits)
}

/// An organ is narrow when its widest span is at most this share of its
/// length; it grows only across, up to [`NARROW_GROWTH`] times.
const NARROW: f64 = 0.2;
const NARROW_GROWTH: f64 = 4.0;
/// An organ whose template covers less than this share of its outline is
/// airy, drawn as spokes each [`SPOKE`] of its width wide at the foot.
const AIRY: f64 = 0.6;
const SPOKE: f64 = 0.06;

/// An airy organ is drawn at least this share of its outline's width.
const MIN_SOLIDITY: f64 = 0.15;

/// Samples across and along a template when tracing its outline.
const TRACE: usize = 48;

/// Each template's outline: its first and last covered stations along
/// the card, and the covered span across at evenly spaced stations
/// between, as `x` from -1 (left) to 1 (right); and its mean colour, as
/// a multiplier of the card's.
struct Outlines {
    shapes: Vec<Option<Outline>>,
}

#[derive(Debug, Clone)]
struct Outline {
    base: (f64, f64),
    tip: (f64, f64),
    /// At each station: v, and the span's left and right x.
    spans: Vec<(f64, f64, f64)>,
    tint: [f64; 3],
    /// The share of its outline its template covers: about 1 for a leaf,
    /// a fifth for an airy grass panicle.
    solidity: f64,
    /// Its widest span over its length, as a share of the card's width.
    widest: f64,
}

impl Outlines {
    fn new(templates: &Templates, stations: usize) -> Self {
        let count = templates.templates.len();
        let shapes = (0..count)
            .map(|template| trace(templates, template, stations))
            .collect();
        Self { shapes }
    }

    /// Append `card`'s polygon to `out`, grown by `grow` about its middle.
    /// Append `card`'s polygon to `out`, grown to stand for the organs
    /// pruned when `share` of them are kept, up to `most` in length and
    /// width each.
    fn polygon(&self, card: &Card, share: f64, most: f64, out: &mut Mesh) {
        let Some(Some(outline)) = self.shapes.get(usize::from(card.template)) else {
            return;
        };
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        let (base, heading, left) = (vector(card.base), vector(card.heading), vector(card.left));
        // A narrow organ (a blade, a needle, a pinna) grows only across,
        // so a tussock keeps its height and its blades their length; any
        // other grows alike both ways.
        let wanted = 1.0 / share.max(1.0e-3);
        let narrow = f64::from(card.width) * outline.widest < NARROW * f64::from(card.length);
        let (along, across) = if narrow {
            (1.0, wanted.min(NARROW_GROWTH))
        } else {
            let both = wanted.sqrt().min(most);
            (both, both)
        };
        let length = f64::from(card.length) * along;
        // An airy organ's outline drawn solid would cover far more than it
        // does: drawn as narrow as covers what its template covers.
        let half = f64::from(card.width) * 0.5 * across * outline.solidity.clamp(MIN_SOLIDITY, 1.0);
        // Grown about the card's middle, as a pruned card grows.
        let middle = base + heading * (f64::from(card.length) * 0.5);
        let base = middle - heading * (length * 0.5);
        let at = |x: f64, v: f64| card.bend.point(base, heading, left, half, length, x, v);
        let colour = [
            (f64::from(card.color[0]) * outline.tint[0]) as f32,
            (f64::from(card.color[1]) * outline.tint[1]) as f32,
            (f64::from(card.color[2]) * outline.tint[2]) as f32,
            1.0,
        ];
        let shed = card.shed.is_finite().then_some(f64::from(card.shed));
        if outline.solidity < AIRY {
            // An airy organ (a grass's panicle, a spray of florets) as
            // three thin spokes from its base to its tip and the ends of
            // its widest span: its reach, not a solid of its outline.
            let (v_top, l_top, r_top) = outline
                .spans
                .iter()
                .copied()
                .max_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1)))
                .unwrap_or((outline.tip.1, 0.0, 0.0));
            let at = |x: f64, v: f64| {
                card.bend.point(
                    base,
                    heading,
                    left,
                    f64::from(card.width) * 0.5 * across,
                    length,
                    x,
                    v,
                )
            };
            let (root, normal) = at(outline.base.0, outline.base.1);
            let foot = (f64::from(card.width) * across * SPOKE).max(1.0e-4);
            let side = left * foot;
            for (x, v) in [
                (l_top, v_top),
                (outline.tip.0, outline.tip.1),
                (r_top, v_top),
            ] {
                let first = u32::try_from(out.positions.len()).unwrap_or(0);
                for position in [root - side, root + side, at(x, v).0] {
                    out.push(Vertex {
                        position,
                        normal,
                        uv: [0.0, 0.0],
                        color: colour,
                        born: f64::from(card.born),
                        shed,
                        level: 3,
                        segment: card.segment,
                    });
                }
                out.indices
                    .extend_from_slice(&[first, first + 1, first + 2]);
            }
            return;
        }
        let mut ring = vec![outline.base];
        ring.extend(outline.spans.iter().map(|&(v, _, right)| (right, v)));
        ring.push(outline.tip);
        ring.extend(outline.spans.iter().rev().map(|&(v, left, _)| (left, v)));
        let first = out.positions.len();
        let mut points = Vec::with_capacity(ring.len());
        for &(x, v) in &ring {
            let (position, normal) = at(x, v);
            points.push(position);
            out.push(Vertex {
                position,
                normal,
                uv: [0.0, 0.0],
                color: colour,
                born: f64::from(card.born),
                shed,
                level: 3,
                segment: card.segment,
            });
        }
        // A fan from the base, which is convex enough for a leaf's outline,
        // wound counter-clockwise seen from the card's front.
        let face = heading.cross(left);
        let turn = (1..points.len().saturating_sub(1))
            .map(|k| {
                (points[k] - points[0])
                    .cross(points[k + 1] - points[0])
                    .dot(face)
            })
            .sum::<f64>();
        let first = u32::try_from(first).unwrap_or(0);
        let n = u32::try_from(ring.len()).unwrap_or(0);
        for k in 1..n.saturating_sub(1) {
            if turn >= 0.0 {
                out.indices
                    .extend_from_slice(&[first, first + k, first + k + 1]);
            } else {
                out.indices
                    .extend_from_slice(&[first, first + k + 1, first + k]);
            }
        }
    }
}

impl Outlines {
    /// Append `chain`'s ribbon to `out`: through its first card's base and
    /// each card's tip, kept to at most `most` points, each as wide as its
    /// cards' outlines, the last a point; widened to stand for the organs
    /// pruned when `share` of them are kept, as a narrow organ is.
    fn ribbon(&self, chain: &[Card], share: f64, most: usize, out: &mut Mesh) {
        let v = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
        let widest = |card: &Card| {
            self.shapes
                .get(usize::from(card.template))
                .and_then(Option::as_ref)
                .map_or(1.0, |outline| {
                    outline.widest * outline.solidity.clamp(MIN_SOLIDITY, 1.0)
                })
        };
        let across = (1.0 / share.max(1.0e-3)).min(NARROW_GROWTH);
        let first = &chain[0];
        let mut points = vec![(v(first.base), f64::from(first.width) * widest(first))];
        for (k, card) in chain.iter().enumerate() {
            let end = card
                .bend
                .point(
                    v(card.base),
                    v(card.heading),
                    v(card.left),
                    f64::from(card.width) * 0.5,
                    f64::from(card.length),
                    0.0,
                    1.0,
                )
                .0;
            let next = chain
                .get(k + 1)
                .map_or(0.0, |n| f64::from(n.width) * widest(n));
            points.push((end, next));
        }
        let kept = simplify(&points, 0.0, most.max(2));
        let outline = self
            .shapes
            .get(usize::from(first.template))
            .and_then(Option::as_ref);
        let tint = outline.map_or([1.0; 3], |o| o.tint);
        let colour = [
            (f64::from(first.color[0]) * tint[0]) as f32,
            (f64::from(first.color[1]) * tint[1]) as f32,
            (f64::from(first.color[2]) * tint[2]) as f32,
            1.0,
        ];
        let face = v(first.heading).cross(v(first.left)).normalize_or(Vec3::Y);
        let shed = first.shed.is_finite().then_some(f64::from(first.shed));
        let base = u32::try_from(out.positions.len()).unwrap_or(0);
        let n = kept.len();
        for (k, &(at, width)) in kept.iter().enumerate() {
            let along = if k + 1 < n {
                kept[k + 1].0 - at
            } else {
                at - kept[k - 1].0
            };
            let side = face.cross(along).normalize_or(v(first.left));
            let normal = along.cross(side).normalize_or(face);
            let half = width * 0.5 * across;
            for sign in [-1.0, 1.0] {
                out.push(Vertex {
                    position: at + side * (half * sign),
                    normal,
                    uv: [0.0, 0.0],
                    color: colour,
                    born: f64::from(first.born),
                    shed,
                    level: 3,
                    segment: first.segment,
                });
            }
        }
        let n = u32::try_from(n).unwrap_or(0);
        for k in 0..n.saturating_sub(1) {
            let (a, b, c, d) = (
                base + 2 * k,
                base + 2 * k + 1,
                base + 2 * k + 2,
                base + 2 * k + 3,
            );
            if k + 2 == n {
                // The tip: one triangle to its point.
                out.indices.extend_from_slice(&[a, b, c]);
            } else {
                out.indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
    }
}

fn trace(templates: &Templates, template: usize, stations: usize) -> Option<Outline> {
    let sample = |k: usize| (k as f64 + 0.5) / TRACE as f64;
    let covered = |u: f64, v: f64| templates.coverage(template, u as f32, v as f32) >= 0.5;
    // The covered span across row `v`, as x from -1 to 1.
    let span = |v: f64| {
        let row: Vec<f64> = (0..TRACE).map(sample).filter(|&u| covered(u, v)).collect();
        Some((*row.first()? * 2.0 - 1.0, *row.last()? * 2.0 - 1.0))
    };
    let rows: Vec<f64> = (0..TRACE)
        .map(sample)
        .filter(|&v| span(v).is_some())
        .collect();
    let (&low, &high) = (rows.first()?, rows.last()?);
    let centre = |v: f64| span(v).map_or(0.0, |(l, r)| (l + r) * 0.5);
    let mut spans = Vec::new();
    for k in 0..stations {
        // Stations through the middle of the covered length.
        let v = low + (high - low) * (k as f64 + 1.0) / (stations as f64 + 1.0);
        // The widest span near the station, so a lobe between samples
        // is not lost.
        let near = (high - low) / (stations as f64 + 1.0) * 0.5;
        let (mut l, mut r) = span(v).unwrap_or((0.0, 0.0));
        for &w in rows.iter().filter(|&&w| (w - v).abs() <= near) {
            if let Some((wl, wr)) = span(w) {
                l = l.min(wl);
                r = r.max(wr);
            }
        }
        spans.push((v, l, r));
    }
    // The mean colour of what it covers, for a white card.
    let mut sum = [0.0; 3];
    let mut kept = 0.0;
    for j in 0..TRACE {
        for i in 0..TRACE {
            let (u, v) = (sample(i), sample(j));
            if covered(u, v) {
                let texel = templates.sample(template, u as f32, v as f32);
                let albedo = templates.albedo(template, [1.0, 1.0, 1.0], texel);
                for c in 0..3 {
                    sum[c] += albedo[c];
                }
                kept += 1.0;
            }
        }
    }
    // Texels inside each row's covered span, against those covered.
    let mut inside = 0.0;
    let mut widest: f64 = 0.0;
    for &v in &rows {
        if let Some((l, r)) = span(v) {
            inside += ((r - l) * 0.5 * TRACE as f64).round() + 1.0;
            widest = widest.max((r - l) * 0.5 + 1.0 / TRACE as f64);
        }
    }
    let solidity = if inside > 0.0 {
        (kept / inside).min(1.0)
    } else {
        1.0
    };
    let tint = if kept > 0.0 {
        [sum[0] / kept, sum[1] / kept, sum[2] / kept]
    } else {
        [1.0; 3]
    };
    Some(Outline {
        base: (centre(low), (low - 0.5 / TRACE as f64).max(0.0)),
        tip: (centre(high), (high + 0.5 / TRACE as f64).min(1.0)),
        spans,
        tint,
        solidity,
        widest,
    })
}

/// Append `from` to `to`, its faces reversed and normals flipped when
/// `back`.
fn append(to: &mut Mesh, from: &Mesh, back: bool) {
    let offset = u32::try_from(to.positions.len()).unwrap_or(0);
    to.positions.extend_from_slice(&from.positions);
    if back {
        to.normals
            .extend(from.normals.iter().map(|n| [-n[0], -n[1], -n[2]]));
    } else {
        to.normals.extend_from_slice(&from.normals);
    }
    to.uvs.extend_from_slice(&from.uvs);
    to.colors.extend_from_slice(&from.colors);
    to.births.extend_from_slice(&from.births);
    to.sheds.extend_from_slice(&from.sheds);
    to.levels.extend_from_slice(&from.levels);
    to.segments.extend_from_slice(&from.segments);
    for triangle in from.indices.chunks_exact(3) {
        let [a, b, c] = [
            triangle[0] + offset,
            triangle[1] + offset,
            triangle[2] + offset,
        ];
        if back {
            to.indices.extend_from_slice(&[a, c, b]);
        } else {
            to.indices.extend_from_slice(&[a, b, c]);
        }
    }
    if !to.segments.is_empty() || !from.segments.is_empty() {
        to.segments.resize(offset as usize, mesh::NO_SEGMENT);
        to.segments.extend_from_slice(&from.segments);
        to.segments.resize(to.positions.len(), mesh::NO_SEGMENT);
    }
}
