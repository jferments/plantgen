//! Triangle meshes baked from a [`PlantGraph`].
//!
//! Wood is drawn as generalised cylinders: each axis (a stem or branch and
//! its continuations) becomes one tube whose rings follow the axis with
//! parallel-transport frames, so tubes never twist. A species' [`Flare`]
//! widens the base of the stem into a root flare or buttresses, its
//! [`Ridges`] cut furrows into thick wood, and its [`Moss`] tints it.
//! Organs are drawn as [`Card`]s that a texture cuts out, kept as one
//! record per card (a GPU expands them into quads; [`PlantMesh::card_mesh`]
//! does it on the CPU), sized, mounted and coloured by each organ type's
//! [`Look`].
//! Each [`LodSpec`] removes thin branches, drops rings where an axis
//! barely bends, and merges organs into larger cluster cards, which is how
//! the far levels stay cheap. A cluster draws its merged card plus upright
//! cards facing out from the plant's axis and around it, sized so that
//! the cluster covers about as much as its organs do from every side a
//! walker sees it from: a coarser level shows the same crown, as open or
//! as dense. Stems stay at every level.
//!
//! Every vertex and card records the plant ages at which its part appears
//! and is shed, so a renderer can grow a plant smoothly between keyframes,
//! and every wood vertex a wind level (0 stem, 1 branch, 2 twig; cards are
//! level 3) for the wind hierarchy.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::bend::Bend;
use crate::body::{self, BodyLook};
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::{Bottle, Flare, Look, Moss, Mount, Ridges};
use crate::math::{self, Vec3, any_perpendicular};
use crate::occlusion::SkyLight;
use crate::rng::{mix64, unit};
use crate::spec::Appearance;
use crate::spines::Tuft;
use crate::{blooms, leaves};

/// One drawable surface: wood or organ cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGB colour and, for cards, the organ template in alpha: the
    /// index of the organ's type in its program. On wood, alpha is minus
    /// the radius of the stem where the vertex is bark (`bark_alpha`), and
    /// 1 on organs drawn solid.
    pub colors: Vec<[f32; 4]>,
    /// Plant age at which the vertex's part appears, in years.
    pub births: Vec<f32>,
    /// Plant age at which the vertex's part is shed, in years; infinity
    /// when it is never shed.
    pub sheds: Vec<f32>,
    /// Wind hierarchy level: 0 stem, 1 branch, 2 twig, 3 organ.
    pub levels: Vec<u8>,
    pub indices: Vec<u32>,
    /// The graph segment that bears each vertex, [`NO_SEGMENT`] where none
    /// does (roots, scenery), for binding it to the plant's bones (plant
    /// motion S2). Not packaged with the mesh; it may run short of the
    /// vertices, the rest bearing none.
    pub segments: Vec<u32>,
}

/// A vertex or card no graph segment bears.
pub const NO_SEGMENT: u32 = u32::MAX;

/// A vertex or card the ground bears: roots, and organs at the plant's foot
/// that no segment bears (a rosette's leaves).
pub const GROUNDED: u32 = u32::MAX - 1;

/// One vertex, for building a [`Mesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub born: f64,
    pub shed: Option<f64>,
    pub level: u8,
    /// The graph segment that bears it, or [`NO_SEGMENT`].
    pub segment: u32,
}

impl Vertex {
    /// A vertex of a part that is always there, with no texture: for
    /// scenery and test shapes.
    #[must_use]
    pub fn plain(position: Vec3, normal: Vec3, color: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            uv: [0.0, 0.0],
            color: [color[0], color[1], color[2], 1.0],
            born: 0.0,
            shed: None,
            level: 0,
            segment: NO_SEGMENT,
        }
    }

    /// The same vertex, borne by graph segment `segment`.
    #[must_use]
    pub fn on(self, segment: u32) -> Self {
        Self { segment, ..self }
    }
}

impl Mesh {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Append a vertex and return its index.
    pub fn push(&mut self, vertex: Vertex) -> u32 {
        let index = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.push(vertex.position.to_f32());
        self.normals.push(vertex.normal.to_f32());
        self.uvs.push(vertex.uv);
        self.colors.push(vertex.color);
        // Ages are small; f32 keeps them to well under a day.
        #[allow(clippy::cast_possible_truncation)]
        {
            self.births.push(vertex.born as f32);
            self.sheds
                .push(vertex.shed.map_or(f32::INFINITY, |shed| shed as f32));
        }
        self.levels.push(vertex.level);
        if vertex.segment != NO_SEGMENT || !self.segments.is_empty() {
            self.segments.resize(index as usize, NO_SEGMENT);
            self.segments.push(vertex.segment);
        }
        index
    }

    /// Marks the vertices from `first` on as borne by graph segment
    /// `segment`: for parts drawn whole, an organ or a body.
    pub fn bear(&mut self, first: usize, segment: u32) {
        let count = self.positions.len();
        if segment == NO_SEGMENT && self.segments.is_empty() {
            return;
        }
        self.segments.resize(count, NO_SEGMENT);
        for at in first.min(count)..count {
            self.segments[at] = segment;
        }
    }

    /// The graph segment that bears vertex `index`, if any.
    #[must_use]
    pub fn segment(&self, index: usize) -> Option<u32> {
        Some(self.bearer(index)).filter(|&segment| segment != NO_SEGMENT && segment != GROUNDED)
    }

    /// What bears vertex `index`: a graph segment, [`GROUNDED`] or
    /// [`NO_SEGMENT`].
    #[must_use]
    pub fn bearer(&self, index: usize) -> u32 {
        self.segments.get(index).copied().unwrap_or(NO_SEGMENT)
    }
}

/// One organ card: a quad standing on `base`, reaching `length` along
/// `heading` and `width` across `left`, cut out by organ template
/// `template`, the index of the organ's type in its program. Its front
/// faces `heading × left`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    pub base: [f32; 3],
    pub heading: [f32; 3],
    pub left: [f32; 3],
    pub length: f32,
    pub width: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    pub template: u8,
    /// Plant age at which the card appears, in years.
    pub born: f32,
    /// Plant age at which it is shed; infinity when it never is.
    pub shed: f32,
    /// How it bends on the levels that keep a card per organ
    /// ([`crate::bend`]); flat elsewhere. A level's bent cards come first.
    pub bend: Bend,
    /// The graph segment that bears its organ (the first organ of a
    /// cluster), or [`NO_SEGMENT`]; not packaged with the mesh.
    pub segment: u32,
    /// Its number from 0 to 1, which sets its share of variegated patches
    /// and is packed into its vertices' alpha ([`card_color`]): its own
    /// ([`crate::templates::card_seed`], from its base), moved toward its
    /// plant's where its look asks ([`PlantMesh::vary_by_plant`]).
    pub seed: f32,
}

/// Wood and organ cards of one plant at one level of detail, and on the
/// nearest level the areoles of its fleshy bodies, whose spines a renderer
/// expands (see [`crate::spines`]). Fleshy bodies are part of the wood
/// mesh, and their spine cards follow the organ cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlantMesh {
    pub wood: Mesh,
    pub cards: Vec<Card>,
    pub tufts: Vec<Tuft>,
    /// On the nearest level built for a renderer that draws part meshes
    /// ([`build_with`]), the organs it may draw as their part mesh: each
    /// the index in `cards` of the organ's card (the second of a crossed
    /// pair), in order. Empty otherwise.
    pub sites: Vec<u32>,
    /// On the nearest levels (0 and 1), the young wood and leaf stalks
    /// that bear the plant's stem hairs, which previews draw as strands
    /// up close (`crate::hairs::strand_mesh`). Not packaged; empty where
    /// the plant states none.
    pub hair_tubes: Vec<crate::hairs::Tube>,
}

impl PlantMesh {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.wood.triangle_count()
            + self.cards.len() * 2
            + self.bent_cards() * (crate::bend::VERTICES as usize / 3 - 2)
    }

    /// Move each card's seed toward the plant's own
    /// ([`crate::templates::plant_seed`] of the `seed` it grew from) by its
    /// look's variegation `plants`, so a plant's leaves share their share
    /// of patches where the look says the plant sets it. A card whose
    /// template is not one of `looks` keeps its own.
    pub fn vary_by_plant(&mut self, looks: &[Look], seed: u64) {
        let plant = crate::templates::plant_seed(seed);
        for card in &mut self.cards {
            let plants = looks
                .get(usize::from(card.template))
                .and_then(|look| look.variegation)
                .map_or(0.0, |variegation| variegation.plants);
            if plants > 0.0 {
                // A share from 0 to 1 fits an f32.
                #[allow(clippy::cast_possible_truncation)]
                let plants = plants as f32;
                card.seed += (plant - card.seed) * plants;
            }
        }
    }

    /// The bent cards, which lead the level's cards.
    #[must_use]
    pub fn bent_cards(&self) -> usize {
        self.cards
            .iter()
            .take_while(|card| !card.bend.is_flat())
            .count()
    }

    /// The cards as a triangle mesh: four vertices and two triangles per
    /// card, wind level 3, uv (0, 0) at the left of the base and (1, 1) at
    /// the right of the tip, and the template in the colour's alpha.
    #[must_use]
    pub fn card_mesh(&self) -> Mesh {
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        let mut out = Mesh::default();
        for card in &self.cards {
            let (base, heading, left) =
                (vector(card.base), vector(card.heading), vector(card.left));
            if !card.bend.is_flat() {
                bent_card(card, base, heading, left, &mut out);
                continue;
            }
            let normal = heading.cross(left);
            let half = left * f64::from(card.width * 0.5);
            let tip = heading * f64::from(card.length);
            let color = card_color(card);
            let shed = card.shed.is_finite().then_some(f64::from(card.shed));
            let corners = [
                (base - half, [0.0, 0.0]),
                (base + half, [1.0, 0.0]),
                (base + half + tip, [1.0, 1.0]),
                (base - half + tip, [0.0, 1.0]),
            ];
            let first = u32::try_from(out.positions.len()).unwrap_or(0);
            for (corner, uv) in corners {
                out.push(Vertex {
                    position: corner,
                    normal,
                    uv,
                    color,
                    born: f64::from(card.born),
                    shed,
                    level: 3,
                    segment: card.segment,
                });
            }
            // Counter-clockwise seen from the side the normal points to.
            out.indices.extend_from_slice(&[
                first,
                first + 2,
                first + 1,
                first,
                first + 3,
                first + 2,
            ]);
        }
        out
    }
}

/// A card's vertex colour: its colour, and in alpha its template plus its
/// face ([`crate::templates::pack_face`]): its seed and whether its normal
/// points down.
fn card_color(card: &Card) -> [f32; 4] {
    let alpha = crate::templates::pack_face(
        card.template,
        card.seed,
        crate::templates::card_flipped(card.heading, card.left),
    );
    [card.color[0], card.color[1], card.color[2], alpha]
}

/// A bent card as the renderer draws it: [`crate::bend::VERTICES`]
/// corners of its grid, each its own vertex.
fn bent_card(card: &Card, base: Vec3, heading: Vec3, left: Vec3, out: &mut Mesh) {
    let color = card_color(card);
    let shed = card.shed.is_finite().then_some(f64::from(card.shed));
    let half = f64::from(card.width) * 0.5;
    let length = f64::from(card.length);
    for k in 0..crate::bend::VERTICES {
        let (x, v) = crate::bend::corner(k);
        let (position, normal) = card.bend.point(base, heading, left, half, length, x, v);
        #[allow(clippy::cast_possible_truncation)]
        let index = out.push(Vertex {
            position,
            normal,
            uv: [((x + 1.0) * 0.5) as f32, v as f32],
            color,
            born: f64::from(card.born),
            shed,
            level: 3,
            segment: card.segment,
        });
        out.indices.push(index);
    }
}

/// How much detail one level keeps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LodSpec {
    /// Branches thinner than this radius are left out, metres. Stems
    /// (axes with no parent) are always kept.
    pub min_radius: f64,
    /// Target distance between ring vertices around a tube, metres.
    pub ring_edge: f64,
    pub min_sides: u32,
    pub max_sides: u32,
    /// Rings are dropped where an axis bends less than this, degrees.
    pub bend: f64,
    /// Organs within one cell of this size merge into one card; 0 keeps
    /// every organ.
    pub cluster: f64,
}

/// Plants at least this tall, in metres, use a level's lengths as given;
/// see [`LodSpec::for_height`].
pub const LOD_REFERENCE_HEIGHT: f64 = 15.0;

/// An upright card of a cluster is left out when the cosine between its
/// facing and the merged card's is above this (within about 30°), since
/// the merged card already covers that side.
pub const CLUSTER_UPRIGHT_SKIP: f64 = 0.866;

/// Directions over the half sphere along which a cluster's cards are
/// sized to cover as much as its organs ([`card_areas`]). Each costs a
/// count of the cluster's cover ([`painted`]) and another of its cards',
/// the most of a coarse level's bake: 16 keep the levels' widths and
/// heights as 32 did (revision 15, `tests/lod_widths.rs`) at half the cost;
/// 12 leave gaps between them, and skunk cabbage's cards grew 1.24 times as
/// tall as its leaves.
const CLUSTER_DIRECTIONS: usize = 16;

/// Views at most this sine of elevation (40°) above or below level, the
/// way a walker sees crowns at the distances clusters are drawn, count
/// fully in the fit; steeper views count [`HIGH_VIEW_WEIGHT`].
const SIDE_VIEW_SINE: f64 = 0.643;
const HIGH_VIEW_WEIGHT: f64 = 0.25;

/// A cluster card is never narrower than this over its length, so a
/// cluster of needles stays a spray rather than a sliver.
const MIN_CLUSTER_ASPECT: f64 = 0.05;

impl LodSpec {
    /// This level for a plant `height` metres tall. A plant lower than
    /// [`LOD_REFERENCE_HEIGHT`] has the level's lengths (`min_radius`,
    /// `ring_edge` and `cluster`) scaled by its height over that, down to
    /// 1/100, so each level keeps about the same detail on screen whatever
    /// the plant's size: a grass at LOD1 keeps its culms as a tree keeps its
    /// branches.
    #[must_use]
    pub fn for_height(&self, height: f64) -> Self {
        let scale = (height / LOD_REFERENCE_HEIGHT).clamp(0.01, 1.0);
        Self {
            min_radius: self.min_radius * scale,
            ring_edge: self.ring_edge * scale,
            cluster: self.cluster * scale,
            ..*self
        }
    }
}

/// Build the mesh of `graph` at level of detail `level` (0 nearest).
/// `looks` holds one look per organ type of the plant's program (see
/// [`Appearance::looks`]) and `bodies` one per body type (see
/// [`Appearance::body_looks`]). The caller scales the level to the plant
/// (see [`LodSpec::for_height`]); fleshy bodies keep their own detail per
/// level (see [`crate::body`]).
#[must_use]
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    lod: &LodSpec,
    level: usize,
) -> PlantMesh {
    build_with(graph, looks, bodies, appearance, lod, level, None)
}

/// [`build`] for a renderer that draws part meshes (`crate::parts`): on
/// level 0 the organ types marked in `parts` are not drawn solid into the
/// wood mesh but keep their cards, and their organs are listed as
/// [`PlantMesh::sites`], where the renderer draws their part meshes near
/// the camera instead of the cards. Other levels, and `None`, build as
/// [`build`] does.
#[must_use]
pub fn build_with(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    lod: &LodSpec,
    level: usize,
    parts: Option<&[bool]>,
) -> PlantMesh {
    build_capped(graph, looks, bodies, appearance, lod, level, parts, None)
}

/// [`build_with`] with at most `sticks` stand-in sticks for the thin wood
/// the level drops ([`twigs`]); `None` for as many as cover it. A package
/// caps them where a level would otherwise draw more wood than the level
/// before it.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_capped(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    lod: &LodSpec,
    level: usize,
    parts: Option<&[bool]>,
    sticks: Option<usize>,
) -> PlantMesh {
    let (mesh, _) = build_shaded(
        graph, looks, bodies, appearance, lod, level, 0.0, parts, sticks, false,
    );
    mesh
}

/// [`build_capped`] with the sky's darkening of the bark kept out of its
/// colour: the same mesh, each bark vertex in its bark's own colour, and
/// beside it each wood vertex's shade, the share of its colour the sky it
/// sees through the plant's leaves leaves it (1 where nothing shades it,
/// down to [`WOOD_SHADE`] deep in a crown). Multiplying the two gives the
/// colours [`build_capped`] draws, bit for bit. An exported model carries
/// the shade as an attribute of its own (`crate::export`), so an engine
/// that darkens wood by its own occlusion does not darken it twice.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_unshaded(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    lod: &LodSpec,
    level: usize,
    parts: Option<&[bool]>,
    sticks: Option<usize>,
) -> (PlantMesh, Vec<f32>) {
    build_shaded(
        graph, looks, bodies, appearance, lod, level, 0.0, parts, sticks, true,
    )
}

/// [`build_capped`], with the bark's shade in its colour, or kept `apart`
/// with one shade per wood vertex (see [`build_unshaded`]); empty unless
/// apart. Fleshy bodies are drawn `along` of the way to the next level's
/// detail (`crate::body::build_between`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_shaded(
    graph: &PlantGraph,
    looks: &[Look],
    bodies: &[BodyLook],
    appearance: &Appearance,
    lod: &LodSpec,
    level: usize,
    along: f64,
    parts: Option<&[bool]>,
    sticks: Option<usize>,
    apart: bool,
) -> (PlantMesh, Vec<f32>) {
    // Pieces of a strap stand on the drawn ends of the pieces before them.
    let seated = seated(graph, looks);
    let (graph, twists) = match &seated {
        Some(seated) => (&seated.graph, &seated.twists[..]),
        None => (graph, &[][..]),
    };
    let parts = parts.filter(|_| level == 0);
    let is_part =
        |index: usize| parts.is_some_and(|parts| parts.get(index).copied().unwrap_or(false));
    let mut mesh = PlantMesh::default();
    // The sky each piece of wood sees through the plant's leaves darkens
    // its bark, as renderers light wood as if in the open.
    let sky = SkyLight::of(graph, looks);
    let mut shade = Shade {
        sky: sky.as_ref(),
        apart: apart.then(Vec::new),
    };
    wood(graph, appearance, lod, &mut shade, &mut mesh.wood);
    twigs(graph, appearance, lod, sticks, &mut shade, &mut mesh.wood);
    // Organs drawn as solid leaves, flowers or fruit at this level leave
    // the cards.
    let mut solid = leaves::solid_types(looks, level);
    for (solid, lace) in solid.iter_mut().zip(crate::lace::lace_types(looks, level)) {
        *solid |= lace;
    }
    let blooms: Vec<bool> = looks
        .iter()
        .enumerate()
        .map(|(index, look)| {
            look.solid.is_none()
                && !is_part(index)
                && look.form.solid_at(level)
                && blooms::fits(graph, look, index)
        })
        .collect();
    for (solid, bloom) in solid.iter_mut().zip(&blooms) {
        *solid |= *bloom;
    }
    let variation = appearance.variation;
    let colour =
        |look: &Look, organ: &GraphOrgan| organ_color(look, variation, organ.light, organ.id);
    leaves::build(graph, looks, level, &colour, &mut mesh.wood);
    crate::lace::build(graph, looks, level, &colour, &mut mesh.wood);
    blooms::build(graph, looks, &blooms, &colour, &mut mesh.wood);
    if level <= 1 {
        let stem_hairs = appearance.stem_hairs.as_ref().filter(|hairs| hairs.drawn());
        petioles(graph, looks, stem_hairs, &mut mesh);
        if let Some(hairs) = stem_hairs {
            stem_tubes(graph, appearance, lod, hairs, &mut mesh.hair_tubes);
        }
    }
    mesh.cards = cards(graph, looks, &solid, twists, appearance.variation, lod);
    // Bent cards first, so a renderer draws them as one range.
    let (mut bent, flat): (Vec<Card>, Vec<Card>) =
        mesh.cards.drain(..).partition(|card| !card.bend.is_flat());
    bent.extend(flat);
    mesh.cards = bent;
    if parts.is_some() {
        mesh.sites = sites(&mesh.cards, looks, &is_part);
    }
    body::build_between(graph, bodies, looks.len(), level, along, &mut mesh);
    if let Some(roots) = &appearance.roots {
        crate::roots::build(graph, roots, appearance.bark, level <= 1, &mut mesh.wood);
    }
    grounded(&mut mesh.cards);
    // Only tubes and sticks, the first wood drawn, are shaded.
    let mut shades = shade.apart.unwrap_or_default();
    if apart {
        shades.resize(mesh.wood.positions.len(), 1.0);
    }
    (mesh, shades)
}

/// The organs of part types among `cards`: each organ's card, or the
/// second of its crossed pair (`organ_cards` pushes the crossing card
/// first, and the two share a base, heading and template).
fn sites(cards: &[Card], looks: &[Look], is_part: &dyn Fn(usize) -> bool) -> Vec<u32> {
    let mut sites = Vec::new();
    let mut index = 0;
    while index < cards.len() {
        let template = usize::from(cards[index].template);
        let crossed = looks
            .get(template)
            .is_some_and(|look| look.cross().is_some());
        // The crossing card copies the organ's card's base and heading.
        #[allow(clippy::float_cmp)]
        let pair = crossed
            && cards.get(index + 1).is_some_and(|next| {
                next.template == cards[index].template
                    && next.base == cards[index].base
                    && next.heading == cards[index].heading
            });
        let card = if pair { index + 1 } else { index };
        if is_part(template) {
            sites.push(u32::try_from(card).unwrap_or(u32::MAX));
        }
        index = card + 1;
    }
    sites
}

/// One ring of a tube: its centre, the radius of its segment, and the age
/// it appears and is shed at.
#[derive(Debug, Clone, Copy)]
struct Ring {
    centre: Vec3,
    radius: f64,
    born: f64,
    shed: Option<f64>,
    /// The graph segment it starts (ends, for an axis's last ring).
    segment: u32,
}

/// The rings of the axis that starts at segment `start`: the start of every
/// segment along it that is thick enough for `lod`, then the end of the
/// last, without the rings where the axis barely bends. A stem (an axis
/// with no parent) keeps all its segments. `None` if fewer than two rings
/// are left.
fn axis_rings(
    graph: &PlantGraph,
    continuations: &[Option<u32>],
    end_radii: &[f64],
    start: usize,
    lod: &LodSpec,
) -> Option<Vec<Ring>> {
    let min_radius = if graph.segments[start].parent.is_none() {
        0.0
    } else {
        lod.min_radius
    };
    let mut chain = vec![start];
    let mut cursor = start;
    while let Some(next) = continuations[cursor] {
        let next = next as usize;
        // Thin wood ends the tube, and so does a fleshy body, which
        // `crate::body` draws.
        if graph.segments[next].radius < min_radius || graph.segments[next].body != 0 {
            break;
        }
        chain.push(next);
        cursor = next;
    }
    let mut points: Vec<Ring> = chain
        .iter()
        .map(|&index| {
            let segment = &graph.segments[index];
            Ring {
                centre: segment.start,
                radius: segment.radius,
                born: segment.born,
                shed: segment.shed,
                segment: u32::try_from(index).unwrap_or(NO_SEGMENT),
            }
        })
        .collect();
    let end = chain[chain.len() - 1];
    let last = &graph.segments[end];
    points.push(Ring {
        centre: last.end,
        radius: end_radii[end],
        born: last.born,
        shed: last.shed,
        segment: u32::try_from(end).unwrap_or(NO_SEGMENT),
    });
    // Keep the ends.
    let cos_bend = math::cos(math::radians(lod.bend));
    let mut kept = vec![points[0]];
    for window in 1..points.len() - 1 {
        let previous = kept[kept.len() - 1].centre;
        let here = points[window].centre;
        let next = points[window + 1].centre;
        let a = (here - previous).normalize_or(Vec3::Y);
        let b = (next - here).normalize_or(Vec3::Y);
        if a.dot(b) < cos_bend || lod.bend <= 0.0 {
            kept.push(points[window]);
        }
    }
    kept.push(points[points.len() - 1]);
    kept.dedup_by(|a, b| (a.centre - b.centre).length() < 1e-9);
    (kept.len() >= 2).then_some(kept)
}

/// Extra rings near the ground, so the curve of a root flare shows: pieces
/// of the stem below three flare heights are cut to at most half a flare
/// height each.
fn flare_rings(rings: &[Ring], flare: &Flare) -> Vec<Ring> {
    let top = flare.height * 3.0;
    let mut out = Vec::with_capacity(rings.len() + 12);
    for pair in rings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        out.push(a);
        if a.centre.y.min(b.centre.y) >= top {
            continue;
        }
        let rise = (b.centre.y.min(top) - a.centre.y.max(0.0)).abs();
        // A small whole number of pieces.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pieces = ((rise / (flare.height * 0.5)).ceil() as u32).clamp(1, 8);
        for piece in 1..pieces {
            let t = f64::from(piece) / f64::from(pieces);
            out.push(Ring {
                centre: a.centre.lerp(b.centre, t),
                radius: a.radius + (b.radius - a.radius) * t,
                ..a
            });
        }
    }
    out.push(rings[rings.len() - 1]);
    out
}

/// How a species' wood surface is shaped and coloured, for one axis.
struct Bark<'a> {
    colour: [f32; 3],
    /// The stem's flare and the angle its buttresses start from; only on
    /// the plant's first axis.
    flare: Option<(&'a Flare, f64)>,
    /// The stem's swelling; only on the plant's first axis.
    bottle: Option<&'a Bottle>,
    /// Furrows: the ridges, how many round this axis, and their contrast.
    ridges: Option<(&'a Ridges, f64)>,
    moss: Option<&'a Moss>,
    /// Phases of this axis's moss patches.
    phase: (f64, f64),
}

impl<'a> Bark<'a> {
    /// The bark of one axis with `sides` sides whose first ring has radius
    /// `radius`, its patterns placed by the axis's `id`.
    fn new(
        appearance: &'a Appearance,
        flare: Option<&'a Flare>,
        bottle: Option<&'a Bottle>,
        radius: f64,
        sides: u32,
        id: u64,
    ) -> Self {
        // Ridges need three sides each to show; fewer, wider ridges on a
        // coarse tube, none on a very coarse one.
        let ridges = appearance.ridges.as_ref().and_then(|ridges| {
            let wanted = (2.0 * math::PI * radius / ridges.spacing).round();
            let count = wanted.min(f64::from(sides / 3)).min(64.0);
            (count >= 3.0).then_some((ridges, count))
        });
        Self {
            colour: appearance.bark,
            flare: flare.map(|flare| (flare, unit(mix64(id ^ 0xF1A2)) * 2.0 * math::PI)),
            bottle,
            ridges,
            moss: appearance.moss.as_ref(),
            phase: (
                unit(mix64(id ^ 0x0055)) * 2.0 * math::PI,
                unit(mix64(id ^ 0x0077)) * 2.0 * math::PI,
            ),
        }
    }

    /// Radius of the surface at angle `angle` round a ring of radius
    /// `radius` whose centre is `height` above the ground and `along`
    /// metres up the axis, and the furrow's depth there (0 on a ridge, 1
    /// at the bottom of a furrow).
    fn radius(&self, radius: f64, height: f64, along: f64, angle: f64) -> (f64, f64) {
        let mut r = radius;
        if let Some((flare, start)) = self.flare {
            let fade = math::exp(-height.max(0.0) / flare.height);
            let buttress = if flare.buttresses > 0 {
                math::pow(
                    0.5 + 0.5 * math::cos(f64::from(flare.buttresses) * (angle - start)),
                    4.0,
                )
            } else {
                1.0
            };
            r *= 1.0 + flare.amount * fade * (1.0 - flare.ridge * (1.0 - buttress));
        }
        if let Some(bottle) = self.bottle {
            r *= bottle.factor(height);
        }
        let mut furrow = 0.0;
        if let Some((ridges, count)) = self.ridges {
            let twisted = angle + math::radians(ridges.twist) * along;
            furrow = math::pow(0.5 + 0.5 * math::cos(count * twisted), 3.0);
            r *= 1.0 - ridges.depth * furrow;
        }
        (r, furrow)
    }

    /// Colour of the surface at a vertex: bark darkened in the furrows,
    /// under moss where it grows.
    fn colour(
        &self,
        segment_radius: f64,
        normal: Vec3,
        height: f64,
        along: f64,
        angle: f64,
        furrow: f64,
    ) -> [f32; 4] {
        let contrast = self.ridges.map_or(0.0, |(ridges, _)| ridges.contrast);
        #[allow(clippy::cast_possible_truncation)]
        let dark = 1.0 - contrast * furrow as f32;
        let mut colour = self.colour.map(|channel| channel * dark);
        if let Some(moss) = self.moss {
            let cover = math::smoothstep(moss.min_radius, moss.min_radius * 2.0, segment_radius);
            let facing = ((normal.y + 0.25) / 0.9).clamp(0.0, 1.0);
            // The stem's base stays damp all round.
            let base = if self.flare.is_some() {
                0.6 * math::exp(-height.max(0.0) / 1.5)
            } else {
                0.0
            };
            let patches = math::smoothstep(
                0.3,
                0.6,
                0.5 + 0.4
                    * math::sin(2.0 * angle + 1.7 * along + self.phase.0)
                    * math::sin(3.0 * angle - 1.1 * along + self.phase.1),
            );
            let weight = (f64::from(moss.amount) * cover * (facing + base).min(1.0) * patches)
                .clamp(0.0, 1.0);
            #[allow(clippy::cast_possible_truncation)]
            let weight = weight as f32;
            for (channel, value) in colour.iter_mut().enumerate() {
                *value += (moss.colour[channel] - *value) * weight;
            }
        }
        [colour[0], colour[1], colour[2], 1.0]
    }
}

/// How dark bark is drawn in the deepest shade, as a share of its colour.
/// Renderers light wood by the sun and an even sky, so a limb deep in a
/// crown would read as brightly as a stem in the open. Its colour carries
/// the sky it sees instead ([`crate::occlusion`]), as a share of the light
/// a crown's surface gets ([`crate::looks::FULL_SUN`]): wood seeing that
/// much or more keeps its colour.
pub const WOOD_SHADE: f32 = 0.2;

/// The sky light each piece of wood sees through the plant's leaves, and
/// what [`wood`] and [`twigs`] do with it: darken the bark's colour, or
/// keep each vertex's shade apart (see [`build_unshaded`]).
struct Shade<'a> {
    /// `None` for a plant without leaves: open sky everywhere.
    sky: Option<&'a SkyLight>,
    /// One shade per vertex drawn, in order, when kept apart.
    apart: Option<Vec<f32>>,
}

impl Shade<'_> {
    /// The sky light at `point` through the plant's leaves.
    fn light(&self, point: Vec3) -> f64 {
        self.sky.map_or(1.0, |sky| sky.at(point))
    }

    /// The colour of a vertex of bark `colour` that sees `light`: darkened
    /// by it, or as it is with its shade kept apart.
    fn colour(&mut self, colour: [f32; 4], light: f64) -> [f32; 4] {
        match &mut self.apart {
            None => shaded(colour, light),
            Some(shades) => {
                shades.push(shade(light));
                colour
            }
        }
    }
}

/// The share of its colour wood keeps under the sky `light` it sees.
fn shade(light: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    ((light / crate::looks::FULL_SUN) as f32).clamp(WOOD_SHADE, 1.0)
}

/// Bark `colour` darkened for the sky `light` it sees.
fn shaded(colour: [f32; 4], light: f64) -> [f32; 4] {
    let factor = shade(light);
    [
        colour[0] * factor,
        colour[1] * factor,
        colour[2] * factor,
        colour[3],
    ]
}

/// `colour` (RGB and alpha) of wood drawn with youth `youth` and `age`
/// years old, moved toward the appearance's young axis colour as far as
/// it is young ([`crate::spec::YoungAxis`]).
fn young_colour(appearance: &Appearance, colour: [f32; 4], youth: f32, age: f64) -> [f32; 4] {
    let Some(young) = &appearance.young_axis else {
        return colour;
    };
    // Weights are between 0 and 1.
    #[allow(clippy::cast_possible_truncation)]
    let weight = young.weight(youth, age) as f32;
    [
        colour[0] + (young.colour[0] - colour[0]) * weight,
        colour[1] + (young.colour[1] - colour[1]) * weight,
        colour[2] + (young.colour[2] - colour[2]) * weight,
        colour[3],
    ]
}

/// A bark vertex's colour: `colour` with minus the radius of the wood it
/// lies on, metres, in alpha, which a renderer drawing a bark pattern
/// (`crate::bark`) reads; other wood keeps alpha 1 (see [`Mesh::colors`]).
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn bark_alpha(colour: [f32; 4], radius: f64) -> [f32; 4] {
    [
        colour[0],
        colour[1],
        colour[2],
        -(radius.max(1.0e-4) as f32),
    ]
}

/// Which segments [`wood`] draws as tubes at a level whose thinnest wood
/// is `min_radius`: the axes it starts (a stem, a lateral, or the first
/// past a fleshy body; not thinner than `min_radius` unless a stem) and
/// the segments each continues through until one is thinner or a body.
fn drawn_segments(graph: &PlantGraph, continuations: &[Option<u32>], min_radius: f64) -> Vec<bool> {
    let mut drawn = vec![false; graph.segments.len()];
    for (start, segment) in graph.segments.iter().enumerate() {
        let after_body = segment
            .parent
            .is_some_and(|parent| graph.segments[parent as usize].body != 0);
        if segment.body != 0 || !(segment.lateral || segment.parent.is_none() || after_body) {
            continue;
        }
        let root = segment.parent.is_none();
        if segment.radius < min_radius && !root {
            continue;
        }
        let limit = if root { 0.0 } else { min_radius };
        drawn[start] = true;
        let mut cursor = start;
        while let Some(next) = continuations[cursor] {
            let next = next as usize;
            let following = &graph.segments[next];
            if following.radius < limit || following.body != 0 {
                break;
            }
            drawn[next] = true;
            cursor = next;
        }
    }
    drawn
}

/// Sides of a stand-in stick ([`twigs`]), and its triangles: two rings.
pub const STICK_SIDES: u32 = 3;
pub const STICK_TRIANGLES: usize = 2 * STICK_SIDES as usize;

/// A twig a level drops (see [`twigs`]): its middle, its direction
/// (pointing up), its length, its mean radius, and its birth and shed ages.
type Twig = (Vec3, Vec3, f64, f64, f64, Option<f64>, u32);

/// A cell for gathering the twigs a level drops, in its `min_radius`s,
/// where the level merges no organs (see [`twigs`]).
const TWIG_CELL_RADII: f64 = 50.0;

/// Stand-ins for the thin wood a level drops (plant leftovers L10 and
/// L12): a leafless crown of twigs (a palo verde, a mesquite in winter)
/// otherwise thins to bare posts at its coarse levels. The dropped
/// segments (those the nearest level draws and this one does not,
/// [`drawn_segments`]) are gathered per cell (the level's `cluster` edge, else
/// [`TWIG_CELL_RADII`] of its `min_radius`). A cell's twigs cover `A =
/// sum 2 r l` seen across them; they are cut, in order across the cell,
/// into `k` groups, `k` as many sticks as the level's thinnest wood a cell
/// long would need for `A` (at most one per twig), and each group becomes
/// one three-sided stick along its twigs' mean direction, at their mean
/// place, as long as they reach along it and as thick as covers their
/// `A`. The wood keeps its colour and the level its look from afar.
#[allow(clippy::too_many_lines)]
fn twigs(
    graph: &PlantGraph,
    appearance: &Appearance,
    lod: &LodSpec,
    cap: Option<usize>,
    sky: &mut Shade<'_>,
    out: &mut Mesh,
) {
    if lod.min_radius <= 0.0 {
        return;
    }
    let cell = if lod.cluster > 0.0 {
        lod.cluster
    } else {
        TWIG_CELL_RADII * lod.min_radius
    };
    // Each dropped segment: its middle, direction (pointing up, so
    // opposite twigs agree), length and radius.
    let mut cells: BTreeMap<(i64, i64, i64), Vec<Twig>> = BTreeMap::new();
    // A segment tapers to the next's radius, a tip to six tenths of its
    // own, as `wood` draws it: its mean radius is what it covers.
    let end_radii = graph.end_radii();
    // What the nearest level draws and this one does not.
    let continuations = graph.continuations();
    let finest = drawn_segments(graph, &continuations, 0.0);
    let here = drawn_segments(graph, &continuations, lod.min_radius);
    for (index, (segment, &end_radius)) in graph.segments.iter().zip(&end_radii).enumerate() {
        if !finest[index] || here[index] {
            continue;
        }
        let axis = segment.end - segment.start;
        let length = axis.length();
        if length <= 1e-9 {
            continue;
        }
        let mut direction = axis / length;
        if direction.y < 0.0 {
            direction = -direction;
        }
        let middle = (segment.start + segment.end) * 0.5;
        #[allow(clippy::cast_possible_truncation)]
        let key = (
            (middle.x / cell).floor() as i64,
            (middle.y / cell).floor() as i64,
            (middle.z / cell).floor() as i64,
        );
        cells.entry(key).or_default().push((
            middle,
            direction,
            length,
            f64::midpoint(segment.radius, end_radius),
            segment.born,
            segment.shed,
            u32::try_from(index).unwrap_or(NO_SEGMENT),
        ));
    }
    let widen = math::PI / (f64::from(STICK_SIDES) * math::sin(math::PI / f64::from(STICK_SIDES)));
    // Sticks per cell: as many as the level's thinnest wood a cell long
    // would need for the cell's cover, at most one per twig; under a cap,
    // fewer and thicker, at least one for the cells covering most while
    // the cap allows.
    let counts: Vec<usize> = {
        let wanted: Vec<(f64, usize)> = cells
            .values()
            .map(|twigs| {
                let cover: f64 = twigs.iter().map(|t| 2.0 * t.3 * t.2).sum();
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let wanted = ((cover / (2.0 * lod.min_radius * cell)).round().max(1.0) as usize)
                    .min(twigs.len())
                    .max(1);
                (cover, wanted)
            })
            .collect();
        let total: usize = wanted.iter().map(|&(_, w)| w).sum();
        match cap {
            Some(cap) if total > cap => {
                let mut counts: Vec<usize> = wanted
                    .iter()
                    .map(|&(_, w)| (w * cap / total).max(1))
                    .collect();
                if counts.iter().sum::<usize>() > cap {
                    // One stick each for the cells covering most.
                    let mut order: Vec<usize> = (0..wanted.len()).collect();
                    order.sort_by(|&a, &b| wanted[b].0.total_cmp(&wanted[a].0).then(a.cmp(&b)));
                    counts = vec![0; wanted.len()];
                    for &index in order.iter().take(cap) {
                        counts[index] = 1;
                    }
                }
                counts
            }
            _ => wanted.iter().map(|&(_, w)| w).collect(),
        }
    };
    for ((key, twigs), groups) in cells.into_iter().zip(counts) {
        if groups == 0 {
            continue;
        }
        let mean_direction = twigs
            .iter()
            .fold(Vec3::ZERO, |sum, t| sum + t.1 * (t.3 * t.2))
            .normalize_or(Vec3::Y);
        // Across the cell: the direction its twigs spread most, of two
        // square to their mean direction.
        let across_a = any_perpendicular(mean_direction);
        let across_b = mean_direction.cross(across_a);
        let spread = |axis: Vec3| {
            let (low, high) = twigs
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), t| {
                    (l.min(t.0.dot(axis)), h.max(t.0.dot(axis)))
                });
            high - low
        };
        let across = if spread(across_a) >= spread(across_b) {
            across_a
        } else {
            across_b
        };
        let mut order: Vec<usize> = (0..twigs.len()).collect();
        order.sort_by(|&a, &b| {
            twigs[a]
                .0
                .dot(across)
                .total_cmp(&twigs[b].0.dot(across))
                .then(a.cmp(&b))
        });
        for g in 0..groups {
            let share = &order[g * twigs.len() / groups..(g + 1) * twigs.len() / groups];
            if share.is_empty() {
                continue;
            }
            let weight: f64 = share.iter().map(|&i| twigs[i].3 * twigs[i].2).sum();
            let centre = share.iter().fold(Vec3::ZERO, |sum, &i| {
                sum + twigs[i].0 * (twigs[i].3 * twigs[i].2)
            }) / weight.max(1e-12);
            let direction = share
                .iter()
                .fold(Vec3::ZERO, |sum, &i| {
                    sum + twigs[i].1 * (twigs[i].3 * twigs[i].2)
                })
                .normalize_or(mean_direction);
            // As long as its twigs reach along it, ends included.
            let (low, high) =
                share
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &i| {
                        let t = &twigs[i];
                        let along = (t.0 - centre).dot(direction);
                        let half = 0.5 * t.2 * t.1.dot(direction).abs();
                        (l.min(along - half), h.max(along + half))
                    });
            let length = (high - low).max(1e-3);
            // Twigs crossing each other cover their crossing once: their
            // cover counted cell by cell as cluster cards count organs
            // ([`painted`]), seen across the stick both ways, never more
            // than their summed `2 r l`.
            let summed = 2.0 * weight;
            let (mut low_box, mut high_box) = (
                Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            );
            for &i in share {
                let t = &twigs[i];
                for end in [t.0 - t.1 * (0.5 * t.2), t.0 + t.1 * (0.5 * t.2)] {
                    low_box = low_box.min(end - Vec3::new(t.3, t.3, t.3));
                    high_box = high_box.max(end + Vec3::new(t.3, t.3, t.3));
                }
            }
            let side_a = any_perpendicular(direction);
            let side_b = direction.cross(side_a);
            let seen = |axis: Vec3| {
                let quads: Vec<[Vec3; 4]> = share
                    .iter()
                    .map(|&i| {
                        let t = &twigs[i];
                        let half = t.1 * (0.5 * t.2);
                        let across = t.1.cross(axis).normalize_or(any_perpendicular(t.1)) * t.3;
                        [
                            t.0 - half - across,
                            t.0 - half + across,
                            t.0 + half + across,
                            t.0 + half - across,
                        ]
                    })
                    .collect();
                painted(&quads, low_box, high_box, axis, 1.0)
            };
            let area = f64::midpoint(seen(side_a), seen(side_b)).min(summed);
            let radius = area / (2.0 * length);
            let born = share
                .iter()
                .map(|&i| twigs[i].4)
                .fold(f64::INFINITY, f64::min);
            let shed = share
                .iter()
                .map(|&i| twigs[i].5)
                .try_fold(f64::NEG_INFINITY, |s, shed| shed.map(|shed| s.max(shed)));
            // Borne by the segment of the share that covers most.
            let bearer = share
                .iter()
                .max_by(|&&a, &&b| {
                    (twigs[a].3 * twigs[a].2)
                        .total_cmp(&(twigs[b].3 * twigs[b].2))
                        .then(b.cmp(&a))
                })
                .map_or(NO_SEGMENT, |&i| twigs[i].6);
            let mut start = centre + direction * low;
            // A stick standing in for twigs near the ground stands on it:
            // its lower end ring (its direction points up) clears it.
            let reach = radius * widen * (1.0 - direction.y * direction.y).max(0.0).sqrt();
            if start.y - reach < GROUND {
                start.y = GROUND + reach;
            }
            let normal = any_perpendicular(direction);
            let binormal = direction.cross(normal);
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let id = key.0.unsigned_abs()
                ^ (key.1.unsigned_abs() << 21)
                ^ (key.2.unsigned_abs() << 42)
                ^ (g as u64);
            let bark = Bark::new(appearance, None, None, radius, STICK_SIDES, id);
            let base = u32::try_from(out.positions.len()).unwrap_or(0);
            for along in [0.0, length] {
                for side in 0..=STICK_SIDES {
                    let angle = 2.0 * math::PI * f64::from(side) / f64::from(STICK_SIDES);
                    let outward = normal * math::cos(angle) + binormal * math::sin(angle);
                    let centre = start + direction * along;
                    #[allow(clippy::cast_possible_truncation)]
                    let uv = [
                        (f64::from(side) / f64::from(STICK_SIDES)) as f32,
                        along as f32,
                    ];
                    out.push(Vertex {
                        position: centre + outward * (radius * widen),
                        normal: outward,
                        uv,
                        color: bark_alpha(
                            sky.colour(
                                bark.colour(radius, outward, centre.y, along, angle, 0.0),
                                sky.light(centre),
                            ),
                            radius,
                        ),
                        born,
                        shed,
                        level: 2,
                        segment: bearer,
                    });
                }
            }
            for side in 0..STICK_SIDES {
                let (a, b) = (base + side, base + STICK_SIDES + 1 + side);
                out.indices
                    .extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
            }
        }
    }
}

/// Tangent, normal and binormal at each ring of an axis, with the distance
/// along the axis. Normals follow the axis by parallel transport, so the
/// tube does not twist.
fn axis_frames(rings: &[Ring]) -> Vec<(Vec3, Vec3, Vec3, f64)> {
    let mut frames = Vec::with_capacity(rings.len());
    let mut normal = any_perpendicular((rings[1].centre - rings[0].centre).normalize_or(Vec3::Y));
    let mut along = 0.0;
    for (ring, current) in rings.iter().enumerate() {
        let point = current.centre;
        let incoming = if ring > 0 {
            (point - rings[ring - 1].centre).normalize_or(Vec3::Y)
        } else {
            (rings[1].centre - point).normalize_or(Vec3::Y)
        };
        let outgoing = if ring + 1 < rings.len() {
            (rings[ring + 1].centre - point).normalize_or(incoming)
        } else {
            incoming
        };
        let tangent = (incoming + outgoing).normalize_or(incoming);
        // Parallel transport: remove the tangent component of the last
        // normal instead of choosing a new one.
        normal = (normal - tangent * tangent.dot(normal)).normalize_or(any_perpendicular(tangent));
        let binormal = tangent.cross(normal);
        if ring > 0 {
            along += (point - rings[ring - 1].centre).length();
        }
        frames.push((tangent, normal, binormal, along));
    }
    frames
}

#[allow(clippy::too_many_lines)]
fn wood(
    graph: &PlantGraph,
    appearance: &Appearance,
    lod: &LodSpec,
    sky: &mut Shade<'_>,
    out: &mut Mesh,
) {
    let continuations = graph.continuations();
    let end_radii = graph.end_radii();
    let stem_hairs = appearance.stem_hairs.as_ref().filter(|hairs| hairs.drawn());
    for (start, segment) in graph.segments.iter().enumerate() {
        // An axis starts at a segment that does not continue its parent,
        // or continues a fleshy body; bodies themselves are drawn by
        // `crate::body`.
        let after_body = segment
            .parent
            .is_some_and(|parent| graph.segments[parent as usize].body != 0);
        if segment.body != 0 || !(segment.lateral || segment.parent.is_none() || after_body) {
            continue;
        }
        // Thin branches drop out; a stem stays at every level, as a tree
        // keeps its trunk, so a herb's leaves and flowers never float.
        let root = segment.parent.is_none();
        if segment.radius < lod.min_radius && !root {
            continue;
        }
        let Some(mut kept) = axis_rings(graph, &continuations, &end_radii, start, lod) else {
            continue;
        };
        let flare = appearance.flare.as_ref().filter(|_| root);
        if let Some(flare) = flare {
            kept = flare_rings(&kept, flare);
        }
        let bottle = appearance.bottle.as_ref().filter(|_| root);

        let base_radius = kept[0].radius.max(1e-4)
            * (1.0 + flare.map_or(0.0, |flare| flare.amount))
            * (1.0 + bottle.map_or(0.0, |bottle| bottle.amount));
        let around = 2.0 * math::PI * base_radius / lod.ring_edge.max(1e-4);
        // Clamped to a small whole number of sides.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sides = (around.ceil() as u32).clamp(lod.min_sides.max(3), lod.max_sides.max(3));
        let mut bark = Bark::new(appearance, flare, bottle, kept[0].radius, sides, segment.id);
        // Wood within a leaf (structures are leaves) is its petiole and
        // rachis: plain, in the leaf axis's colour, when the look gives one.
        let leaf_axis = appearance.leaf_axis.filter(|_| segment.within.is_some());
        if let Some(colour) = leaf_axis {
            bark.colour = colour;
            bark.ridges = None;
            bark.moss = None;
        }
        let level = match segment.order {
            0 => 0,
            1 => 1,
            _ => 2,
        };

        let frames = axis_frames(&kept);
        // A ring of `n` vertices on the radius draws a polygon whose mean
        // width over every direction, its perimeter over pi, is
        // `(n / pi) sin(pi / n)` of the circle's (0.83 at 3 sides, 0.97 at
        // 8), so coarse levels drew thinner wood than fine ones. Vertices
        // go out by the inverse, so every level's mean width is the
        // stem's diameter.
        let widen = math::PI / (f64::from(sides) * math::sin(math::PI / f64::from(sides)));
        // Surface radii per ring and side, for positions and for the slope
        // of the surface along the axis.
        let angle_of = |side: u32| 2.0 * math::PI * f64::from(side) / f64::from(sides);
        let radii: Vec<Vec<(f64, f64)>> = kept
            .iter()
            .zip(&frames)
            .map(|(ring, frame)| {
                (0..=sides)
                    .map(|side| bark.radius(ring.radius, ring.centre.y, frame.3, angle_of(side)))
                    .collect()
            })
            .collect();

        let mut first_ring = None;
        for (ring, current) in kept.iter().enumerate() {
            let (tangent, normal, binormal, along) = frames[ring];
            let light = sky.light(current.centre);
            let base = out.positions.len();
            let (before, after) = (ring.saturating_sub(1), (ring + 1).min(kept.len() - 1));
            let span = (frames[after].3 - frames[before].3).max(1e-9);
            for side in 0..=sides {
                let angle = angle_of(side);
                let direction = normal * math::cos(angle) + binormal * math::sin(angle);
                let (radius, furrow) = radii[ring][side as usize];
                // The surface normal leans against the radius's change
                // round the ring and along the axis.
                let previous = radii[ring][((side + sides - 1) % sides) as usize].0;
                let next = radii[ring][((side + 1) % sides) as usize].0;
                let step = 2.0 * math::PI / f64::from(sides);
                let round = (next - previous) / (2.0 * step * radius.max(1e-9));
                let slope = (radii[after][side as usize].0 - radii[before][side as usize].0) / span;
                let around = binormal * math::cos(angle) - normal * math::sin(angle);
                let surface =
                    (direction - around * round - tangent * slope).normalize_or(direction);
                // u wraps once around; v runs along the axis in metres.
                #[allow(clippy::cast_possible_truncation)]
                let uv = [(f64::from(side) / f64::from(sides)) as f32, along as f32];
                let surface_colour = bark.colour(
                    current.radius,
                    surface,
                    current.centre.y,
                    along,
                    angle,
                    furrow,
                );
                let surface_colour = young_colour(
                    appearance,
                    surface_colour,
                    graph.segments[current.segment as usize].young,
                    graph.age - current.born,
                );
                let [r, g, b] = crate::hairs::cast_wood(
                    [surface_colour[0], surface_colour[1], surface_colour[2]],
                    crate::hairs::youth(graph.age - current.born),
                    stem_hairs,
                );
                let colour = sky.colour([r, g, b, surface_colour[3]], light);
                out.push(Vertex {
                    position: footed(root, current.centre, direction * (radius * widen)),
                    normal: surface,
                    uv,
                    // A leaf's axis keeps alpha 1: no bark pattern on it.
                    color: if leaf_axis.is_some() {
                        colour
                    } else {
                        bark_alpha(colour, current.radius)
                    },
                    born: current.born,
                    shed: current.shed,
                    level,
                    segment: current.segment,
                });
            }
            if let Some(previous) = first_ring {
                for side in 0..sides {
                    let a = u32::try_from(previous).unwrap_or(0) + side;
                    let b = u32::try_from(base).unwrap_or(0) + side;
                    // Counter-clockwise seen from outside the tube.
                    out.indices
                        .extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
                }
            }
            first_ring = Some(base);
        }
    }
}

/// The point `out` from `centre` on a ring of an axis, a `stem`'s cut
/// flat at the ground: a stem's foot meets level ground in a flat cut, so
/// a ring standing on the ground whose side would dip into it, as a
/// leaning or flared trunk's foot ring does, keeps that side on it.
fn footed(stem: bool, centre: Vec3, out: Vec3) -> Vec3 {
    let mut position = centre + out;
    if stem && centre.y >= GROUND && position.y < GROUND {
        position.y = GROUND;
    }
    position
}

/// A card while it is placed, in double precision.
#[derive(Debug, Clone, Copy)]
struct Placed {
    base: Vec3,
    heading: Vec3,
    left: Vec3,
    length: f64,
    width: f64,
    color: [f32; 3],
    template: u8,
    born: f64,
    shed: Option<f64>,
    bend: Bend,
    segment: u32,
}

impl Placed {
    // Plant-frame metres and ages fit f32 to well under a millimetre and a
    // day.
    #[allow(clippy::cast_possible_truncation)]
    fn finish(self) -> Card {
        let base = self.base.to_f32();
        Card {
            base,
            heading: self.heading.to_f32(),
            left: self.left.to_f32(),
            length: self.length as f32,
            width: self.width as f32,
            color: self.color,
            template: self.template,
            born: self.born as f32,
            shed: self.shed.map_or(f32::INFINITY, |shed| shed as f32),
            bend: self.bend,
            segment: self.segment,
            seed: crate::templates::card_seed(base),
        }
    }
}

/// The ground a plant stands on in its own frame: the level plane of its
/// foot. No card reaches below it ([`grounded`]).
pub const GROUND: f64 = 0.0;

/// How far below the [`GROUND`] a keyframe reaches, in metres (0 when
/// nothing does): its wood as grown (segment ends), its organs where they
/// stand, and as drawn at one level, its wood mesh (tubes, sticks, solid
/// leaves and flowers, fleshy bodies and any roots) and its cards.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Below {
    pub segments: f64,
    pub organs: f64,
    pub wood: f64,
    pub cards: f64,
}

impl Below {
    /// The graph's depths, with no mesh.
    #[must_use]
    pub fn graph(graph: &PlantGraph) -> Self {
        let depth = |y: f64| (GROUND - y).max(0.0);
        Self {
            segments: graph
                .segments
                .iter()
                .map(|segment| depth(segment.start.y.min(segment.end.y)))
                .fold(0.0, f64::max),
            organs: graph
                .organs
                .iter()
                .map(|organ| depth(organ.position.y))
                .fold(0.0, f64::max),
            wood: 0.0,
            cards: 0.0,
        }
    }

    /// The graph's depths, and `mesh`'s, one of its levels.
    #[must_use]
    pub fn drawn(graph: &PlantGraph, mesh: &PlantMesh) -> Self {
        let depth = |y: f32| (GROUND - f64::from(y)).max(0.0);
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        Self {
            wood: mesh
                .wood
                .positions
                .iter()
                .map(|position| depth(position[1]))
                .fold(0.0, f64::max),
            cards: mesh
                .cards
                .iter()
                .map(|card| {
                    let low = card_low(
                        card,
                        vector(card.base),
                        vector(card.heading),
                        vector(card.left),
                    );
                    (GROUND - low).max(0.0)
                })
                .fold(0.0, f64::max),
            ..Self::graph(graph)
        }
    }

    /// The deepest of them.
    #[must_use]
    pub fn most(&self) -> f64 {
        self.segments
            .max(self.organs)
            .max(self.wood)
            .max(self.cards)
    }
}

/// The lowest height `card` reaches, drawn as a renderer draws it: its four
/// corners, or every corner of a bent card's grid ([`crate::bend`]).
fn card_low(card: &Card, base: Vec3, heading: Vec3, left: Vec3) -> f64 {
    let half = f64::from(card.width) * 0.5;
    let length = f64::from(card.length);
    if card.bend.is_flat() {
        return base.y + (heading.y * length).min(0.0) - left.y.abs() * half;
    }
    (0..crate::bend::VERTICES)
        .map(|k| {
            let (x, v) = crate::bend::corner(k);
            card.bend
                .position(base, heading, left, half, length, x, v)
                .y
        })
        .fold(f64::INFINITY, f64::min)
}

/// `cards` kept above the [`GROUND`]: a card that would reach below it, a
/// leaf drooping into the soil or a spray placed at a seedling's foot, is
/// turned up about its base by the least angle that lifts it clear, as a
/// leaf comes to rest on the ground; one that no turn clears (its base at
/// the ground, or it already points up) is raised until it stands on it.
/// Cards clear of the ground keep their bytes.
fn grounded(cards: &mut [Card]) {
    let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    for card in cards.iter_mut() {
        let (base, heading, left) = (vector(card.base), vector(card.heading), vector(card.left));
        // No point of a card is farther from its base than its length and
        // half its width.
        if base.y - f64::from(card.length) - f64::from(card.width) * 0.5 >= GROUND {
            continue;
        }
        let low = card_low(card, base, heading, left);
        if low >= GROUND {
            continue;
        }
        // Turn toward up about the level axis across the heading (about
        // its left when it points straight down).
        let across = heading.cross(Vec3::Y);
        let axis = if across.length() > 1.0e-9 {
            across.normalize_or(left)
        } else {
            left.normalize_or(Vec3::X)
        };
        let most = math::acos(heading.y.clamp(-1.0, 1.0));
        let turned = |angle: f64| {
            (
                heading.rotate_about(axis, angle),
                left.rotate_about(axis, angle),
            )
        };
        let clears = |angle: f64| {
            let (h, l) = turned(angle);
            card_low(card, base, h, l) >= GROUND
        };
        let (heading, left, base) = if most > 1.0e-6 && clears(most) {
            // The least turn that clears, to well under a millimetre.
            let (mut short, mut enough) = (0.0, most);
            for _ in 0..32 {
                let middle = f64::midpoint(short, enough);
                if clears(middle) {
                    enough = middle;
                } else {
                    short = middle;
                }
            }
            let (h, l) = turned(enough);
            (h, l, base)
        } else {
            (heading, left, base + Vec3::Y * (GROUND - low))
        };
        card.base = base.to_f32();
        card.heading = heading.to_f32();
        card.left = left.to_f32();
    }
}

fn organ_color(look: &Look, variation: f32, light: f64, id: u64) -> [f32; 3] {
    // Shaded organs are darker and bluer, along the scale leaves acclimate
    // on (`looks::sunlit`); each organ varies a little.
    #[allow(clippy::cast_possible_truncation)]
    let shade = (1.0 - crate::looks::sunlit(light)) as f32;
    #[allow(clippy::cast_possible_truncation)]
    let jitter = 1.0 + variation * (unit(mix64(id)) as f32 - 0.5);
    std::array::from_fn(|channel| {
        let (lit, shaded) = (look.colour[channel], look.shade[channel]);
        ((lit + (shaded - lit) * shade) * jitter).clamp(0.0, 1.0)
    })
}

/// `left` turned `degrees` about `heading`: a card's roll about its own
/// length.
fn rolled(heading: Vec3, left: Vec3, degrees: f64) -> Vec3 {
    if degrees == 0.0 {
        left
    } else {
        left.rotate_about(heading.normalize_or(Vec3::Y), degrees.to_radians())
    }
}

/// Turn a card about its base so its face moves `amount` of the way to
/// level, toward whichever of up or down it faces more.
fn face_up(heading: Vec3, left: Vec3, amount: f64) -> (Vec3, Vec3) {
    if amount <= 0.0 {
        return (heading, left);
    }
    let normal = heading.cross(left);
    let target = if normal.y < 0.0 { -Vec3::Y } else { Vec3::Y };
    let axis = normal.cross(target);
    let sin = axis.length();
    if sin < 1e-9 {
        return (heading, left);
    }
    let angle = math::atan2(sin, normal.dot(target)) * amount.min(1.0);
    let axis = axis * (1.0 / sin);
    (
        heading.rotate_about(axis, angle),
        left.rotate_about(axis, angle),
    )
}

/// Where an organ's card goes: its base, heading, left and length. A
/// cluster's card (`crate::clusters::CardFrame`) starts its foot along the
/// organ, or is centred there when it faces along it, and is its length
/// times the organ's.
pub(crate) fn place(organ: &GraphOrgan, look: &Look) -> (Vec3, Vec3, Vec3, f64) {
    if let Some(stalk) = look.stalk() {
        return on_stalk(organ, look, stalk, 1.0);
    }
    if let Some(cluster) = &look.inflorescence {
        let foot = cluster.card.foot * organ.size;
        let length = cluster.card.length * organ.size;
        return match cluster.card.mount {
            Mount::Along => {
                let (heading, left) = face_up(organ.heading, organ.left, look.face_up);
                (organ.position + heading * foot, heading, left, length)
            }
            Mount::Facing => {
                let up = organ.heading.cross(organ.left);
                let (heading, left) = face_up(up, -organ.left, look.face_up);
                let centre = organ.position + organ.heading * foot;
                (centre - heading * (length * 0.5), heading, left, length)
            }
        };
    }
    match look.mount() {
        // Standing on the organ, reaching along it, turned about its
        // length by its roll, then toward the sky.
        Mount::Along => {
            let left = rolled(organ.heading, organ.left, look.roll);
            let (heading, left) = face_up(organ.heading, left, look.face_up);
            (organ.position, heading, left, organ.size)
        }
        // Centred on the organ, its face toward the organ's heading: the
        // card's heading is the turtle's up and its left the turtle's right,
        // so that `heading × left` is the organ's heading.
        Mount::Facing => {
            let up = organ.heading.cross(organ.left);
            let (heading, left) = face_up(up, -organ.left, look.face_up);
            (
                organ.position - heading * (organ.size * 0.5),
                heading,
                left,
                organ.size,
            )
        }
    }
}

/// Where the card of a leaf drawn on a petiole of its own goes
/// ([`crate::looks::Insertion`]), `share` as long as its organ (a sun,
/// shade or juvenile leaf's): turned level about the point of its blade
/// that sits on the petiole's tip, as bent, so that point stays there.
/// The petiole runs along the organ from its position ([`petioles`]).
fn on_stalk(
    organ: &GraphOrgan,
    look: &Look,
    stalk: crate::looks::Stalk,
    share: f64,
) -> (Vec3, Vec3, Vec3, f64) {
    let tip = organ.position + organ.heading * (stalk.petiole * organ.size);
    let (heading, left) = face_up(organ.heading, organ.left, 1.0);
    let length = organ.size * share;
    let half = 0.5 * look.aspect() * length;
    let (attach, _) = look
        .bend
        .point(Vec3::ZERO, heading, left, half, length, 0.0, stalk.attach);
    (tip - attach, heading, left, length)
}

/// The petioles of leaves drawn on petioles of their own
/// ([`crate::looks::Insertion`]), into `mesh` as wood each organ's segment
/// bears: a slender stalk along the organ to where its blade sits, in the
/// accent colour its card painted the petiole in, a hundredth as thick as
/// it is long and at least half a millimetre, cast toward the plant's
/// stem `hairs` where it states them, which the stalk bears as a
/// [`crate::hairs::Tube`].
pub(crate) fn petioles(
    graph: &PlantGraph,
    looks: &[Look],
    hairs: Option<&crate::looks::Hairs>,
    plant: &mut PlantMesh,
) {
    let mesh = &mut plant.wood;
    for organ in &graph.organs {
        let Some(look) = looks.get(usize::from(organ.organ)) else {
            continue;
        };
        let Some(stalk) = look.stalk() else {
            continue;
        };
        let length = stalk.petiole * organ.size;
        if length <= 0.0 {
            continue;
        }
        let first = mesh.positions.len();
        let along = organ.heading.normalize_or(Vec3::Y);
        let radius = (0.005 * length).max(0.0005);
        // A living leaf's stalk keeps its hairs.
        let colour = crate::hairs::cast_wood(look.accent, 1.0, hairs);
        if let Some(tube) = hairs.and_then(|hairs| {
            crate::hairs::Tube::new(
                organ.position,
                organ.position + along * length,
                radius,
                look.accent,
                (organ.born, organ.shed),
                organ.segment.unwrap_or(GROUNDED),
                hairs,
                1.0,
            )
        }) {
            plant.hair_tubes.push(tube);
        }
        crate::fruit::stalk_between(
            organ.position,
            along,
            length,
            radius,
            colour,
            crate::blooms::Part {
                born: organ.born,
                shed: organ.shed,
                coarse: 0,
            },
            mesh,
        );
        mesh.bear(first, organ.segment.unwrap_or(GROUNDED));
    }
}

/// The young wood the plant's stem `hairs` grow on, into `tubes`: every
/// piece of an axis drawn at this level younger than
/// [`crate::hairs::YOUNG_YEARS`], in its wood's colour.
fn stem_tubes(
    graph: &PlantGraph,
    appearance: &Appearance,
    lod: &LodSpec,
    hairs: &crate::looks::Hairs,
    tubes: &mut Vec<crate::hairs::Tube>,
) {
    for (index, segment) in graph.segments.iter().enumerate() {
        if segment.body != 0 || (segment.radius < lod.min_radius && segment.parent.is_some()) {
            continue;
        }
        let colour = appearance
            .leaf_axis
            .filter(|_| segment.within.is_some())
            .unwrap_or(appearance.bark);
        let [r, g, b, _] = young_colour(
            appearance,
            [colour[0], colour[1], colour[2], 1.0],
            segment.young,
            graph.age - segment.born,
        );
        let colour = [r, g, b];
        tubes.extend(crate::hairs::Tube::new(
            segment.start,
            segment.end,
            segment.radius,
            colour,
            (segment.born, segment.shed),
            u32::try_from(index).unwrap_or(NO_SEGMENT),
            hairs,
            crate::hairs::youth(graph.age - segment.born),
        ));
    }
}

/// The most a piece of a strap twists to meet the next at their joint,
/// degrees: about what a grass blade or basal leaf turns over a fifth of
/// its length. Where the edge across both midribs lies farther round (a
/// joint that turns the strap little, or about its own face), the pieces
/// meet as near it as this reaches.
const STRAP_TWIST: f64 = 15.0;

/// A graph as its cards are drawn ([`seated`]), and the twist of each
/// organ's card where it is not its look's, degrees.
struct Seated {
    graph: PlantGraph,
    twists: Vec<Option<f64>>,
}

/// `graph` with the pieces of every strap turned so their drawn cards
/// span them, or `None` when no organ changes. A program draws an arching
/// strap (a grass blade, a basal leaf) as a chain of pieces, each standing
/// where the turtle left the one before, and arches it by the turns between
/// them. Each piece's card bends by its look ([`crate::bend`]: its midrib
/// droops along an arc), so drawn on the turtle's line it ended short of
/// the next piece and below it, and the strap fell apart into floating
/// pieces. A piece of a chain (an organ that stands on the straight end of
/// another in the same structure, or that another stands on) is turned
/// against its droop by half of it and lengthened by the arc over its
/// chord, and drawn at its own size when its look's sun and shade forms
/// would size it otherwise: its card then runs from where the organ stands
/// to where the next one stands, as the program arched it. Joints keep the
/// difference between the program's turn and the droop, a few degrees.
///
/// One strap is one leaf, so its pieces join edge to edge and draw alike.
/// Each joint's edge lies across both pieces' midribs there (the axis the
/// strap turns about at the joint): the piece above stands with its left
/// along it, and the piece below twists to end along it, in place of its
/// look's twist, and the strap's last piece keeps its look's. The pieces
/// take the light and id of the piece the strap starts from, so they
/// draw in one form and one shade of colour. Turned by the program's rolls
/// and twisted by their looks, lit and varied piece by piece, they had
/// stepped in width and colour at every joint.
#[allow(clippy::too_many_lines)]
fn seated(graph: &PlantGraph, looks: &[Look]) -> Option<Seated> {
    // Ends to the micrometre: the turtle's own sums, so continuing pieces
    // stand on them bit for bit.
    #[allow(clippy::cast_possible_truncation)]
    let key = |within: Option<u32>, at: Vec3| {
        (
            within,
            (at.x * 1e6).round() as i64,
            (at.y * 1e6).round() as i64,
            (at.z * 1e6).round() as i64,
        )
    };
    let along = |index: usize| {
        looks.get(index).is_some_and(|look| {
            look.inflorescence.is_none()
                && look.solid.is_none()
                && look.stalk().is_none()
                && look.mount() == Mount::Along
        })
    };
    let candidates = || {
        graph
            .organs
            .iter()
            .enumerate()
            .filter(|(_, organ)| along(usize::from(organ.organ)))
    };
    let ends: std::collections::HashMap<_, usize> = candidates()
        .map(|(number, organ)| {
            (
                key(organ.within, organ.position + organ.heading * organ.size),
                number,
            )
        })
        .collect();
    let mut chained = vec![false; graph.organs.len()];
    let mut stands_on: Vec<Option<usize>> = vec![None; graph.organs.len()];
    for (number, organ) in candidates() {
        if let Some(&before) = ends.get(&key(organ.within, organ.position))
            && before != number
        {
            chained[before] = true;
            chained[number] = true;
            stands_on[number] = Some(before);
        }
    }
    // The piece each chain starts from, the one that stands on no other.
    let root = |mut number: usize| {
        for _ in 0..graph.organs.len() {
            match stands_on[number] {
                Some(before) => number = before,
                None => break,
            }
        }
        number
    };
    let mut out: Option<Seated> = None;
    // Each piece seated so far: its card's left as rolled, and its midrib's
    // heading at its tip.
    let mut seated: Vec<Option<(Vec3, Vec3)>> = vec![None; graph.organs.len()];
    let mut twists: Vec<Option<f64>> = vec![None; graph.organs.len()];
    for (number, organ) in graph.organs.iter().enumerate() {
        if !chained[number] {
            continue;
        }
        let first = &graph.organs[root(number)];
        let organ = &GraphOrgan {
            light: first.light,
            id: first.id,
            ..organ.clone()
        };
        let index = usize::from(organ.organ);
        let (drawn, share) = looks[index].forms.of(index, organ);
        let Some(shown) = looks.get(drawn) else {
            continue;
        };
        let droop = math::radians(shown.bend.droop);
        let chord = if droop.abs() < 1e-9 {
            1.0
        } else {
            2.0 * math::sin(0.5 * droop) / droop
        };
        // The card droops toward its face as rolled; the piece keeps a left
        // that its roll turns back to the rolled one about its new heading.
        // A piece standing on another stands with its left along the
        // joint's edge, and the one below twists to meet it there: the edge
        // across both midribs where it can within [`STRAP_TWIST`], else as
        // near it as that twist reaches, the piece above taking what the
        // one below ends on, less the part along its own midrib.
        let roll = looks[index].roll;
        let own = rolled(organ.heading, organ.left, roll);
        let left = match stands_on[number].and_then(|before| Some((before, seated[before]?))) {
            Some((before, (below, tip))) => {
                let across = tip.cross(organ.heading).normalize_or(below);
                let across = if across.dot(below) < 0.0 {
                    -across
                } else {
                    across
                };
                let twist = math::atan2(below.cross(across).dot(tip), below.dot(across))
                    .clamp(-STRAP_TWIST.to_radians(), STRAP_TWIST.to_radians());
                twists[before] = Some(twist.to_degrees());
                let edge = below.rotate_about(tip, twist);
                (edge - organ.heading * edge.dot(organ.heading)).normalize_or(own)
            }
            None => own,
        };
        let face = organ.heading.cross(left);
        let heading = organ.heading * math::cos(0.5 * droop) + face * math::sin(0.5 * droop);
        let normal = heading.cross(left);
        seated[number] = Some((left, heading * math::cos(droop) - normal * math::sin(droop)));
        let drawn = GraphOrgan {
            heading,
            left: rolled(heading, left, -roll),
            size: organ.size / (share * chord),
            ..organ.clone()
        };
        if drawn != graph.organs[number] {
            seat(&mut out, graph).organs[number] = drawn;
        }
    }
    if twists.iter().any(Option::is_some) {
        seat(&mut out, graph);
    }
    if let Some(out) = &mut out {
        out.twists = twists;
    }
    out
}

/// `out`'s graph, made a copy of `graph` the first time a piece of a
/// strap changes.
fn seat<'a>(out: &'a mut Option<Seated>, graph: &PlantGraph) -> &'a mut PlantGraph {
    &mut out
        .get_or_insert_with(|| Seated {
            graph: graph.clone(),
            twists: vec![None; graph.organs.len()],
        })
        .graph
}

/// The organs whose cards float: each organ's index in `graph` and how far
/// its card's foot is from what bears it, metres, for every organ more than
/// `tolerance` from it. An organ is borne by the wood it grew on (its
/// segment's axis, out to its radius) or, a piece of a strap, by the drawn
/// end of the piece it stands on, or by an organ on the same wood it is
/// within the size of; the graph is drawn as [`build`] draws it.
/// An organ on neither (a basal leaf, the first piece of a strap) is
/// where its program put it, and never loose.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn loose_organs(graph: &PlantGraph, looks: &[Look], tolerance: f64) -> Vec<(usize, f64)> {
    let seated = seated(graph, looks);
    let drawn = seated.as_ref().map_or(graph, |seated| &seated.graph);
    // The drawn end of each card that reaches along its organ, by where
    // the organ's straight end was.
    #[allow(clippy::cast_possible_truncation)]
    let key = |within: Option<u32>, at: Vec3| {
        (
            within,
            (at.x * 1e6).round() as i64,
            (at.y * 1e6).round() as i64,
            (at.z * 1e6).round() as i64,
        )
    };
    let mut ends = std::collections::HashMap::new();
    for (number, (organ, shown)) in graph.organs.iter().zip(&drawn.organs).enumerate() {
        let index = usize::from(shown.organ);
        let Some(look) = looks.get(index) else {
            continue;
        };
        if look.inflorescence.is_some()
            || look.solid.is_some()
            || look.stalk().is_some()
            || look.mount() != Mount::Along
        {
            continue;
        }
        let (form, share) = look.forms.of(index, shown);
        let Some(form) = looks.get(form) else {
            continue;
        };
        let (base, heading, left, length) = place(shown, look);
        let length = length * share;
        let half = 0.5 * length * form.aspect();
        let tip = form
            .bend
            .point(base, heading, left, half, length, 0.0, 1.0)
            .0;
        ends.insert(
            key(organ.within, organ.position + organ.heading * organ.size),
            (number, tip),
        );
    }
    // Each organ's foot, and how far it is from the wood that bears it.
    let feet: Vec<Vec3> = drawn
        .organs
        .iter()
        .map(|shown| match looks.get(usize::from(shown.organ)) {
            Some(look)
                if look.inflorescence.is_none()
                    && look.stalk().is_none()
                    && look.mount() == Mount::Along =>
            {
                place(shown, look).0
            }
            _ => shown.position,
        })
        .collect();
    // The piece each organ stands on, if any.
    let pieces: Vec<Option<usize>> = graph
        .organs
        .iter()
        .enumerate()
        .map(|(number, organ)| {
            ends.get(&key(organ.within, organ.position))
                .map(|&(before, _)| before)
                .filter(|&before| before != number)
        })
        .collect();
    let mut distance: Vec<f64> = graph
        .organs
        .iter()
        .zip(&feet)
        .zip(&pieces)
        .map(|((organ, &foot), piece)| {
            let Some(segment) = organ
                .segment
                .and_then(|segment| graph.segments.get(segment as usize))
            else {
                // On no wood: where its program put it, on the ground or
                // a clump's foot, unless it stands on a piece (below).
                return if piece.is_some() { f64::INFINITY } else { 0.0 };
            };
            let axis = segment.end - segment.start;
            let t = ((foot - segment.start).dot(axis) / axis.dot(axis).max(1e-18)).clamp(0.0, 1.0);
            // A fleshy body's ribs and dome reach past its segment's
            // radius; one more radius holds what sits on them.
            let reach = segment.radius * if segment.body == 0 { 1.0 } else { 2.0 };
            ((foot - (segment.start + axis * t)).length() - reach).max(0.0)
        })
        .collect();
    // Then by what is borne, until nothing changes: a piece by the drawn
    // end of the piece it stands on, any organ by another on the same wood
    // it touches (within that one's size of where it stands), as a chain
    // of fruit hangs from fruit.
    let mut by_segment: std::collections::HashMap<Option<u32>, Vec<usize>> =
        std::collections::HashMap::new();
    for (number, organ) in graph.organs.iter().enumerate() {
        by_segment.entry(organ.segment).or_default().push(number);
    }
    loop {
        let mut changed = false;
        for number in 0..graph.organs.len() {
            if distance[number] <= tolerance {
                continue;
            }
            let organ = &graph.organs[number];
            let foot = feet[number];
            let mut best = distance[number];
            if let Some(before) = pieces[number]
                && distance[before] <= tolerance
            {
                let (_, end) = ends[&key(organ.within, organ.position)];
                best = best.min((foot - end).length());
            }
            for &other in &by_segment[&organ.segment] {
                if other != number && distance[other] <= tolerance {
                    let next = &drawn.organs[other];
                    best = best.min(((foot - next.position).length() - next.size).max(0.0));
                }
            }
            if best < distance[number] {
                changed |= best <= tolerance;
                distance[number] = best;
            }
        }
        if !changed {
            break;
        }
    }
    distance
        .into_iter()
        .enumerate()
        .filter(|&(_, distance)| distance > tolerance)
        .collect()
}

/// `organ`'s card as the coarse levels trace it: its foot, heading, left
/// and length. A cluster's is where [`place`] puts it; every other is
/// traced from its organ's position, at its organ's size.
fn traced(organ: &GraphOrgan, look: &Look) -> (Vec3, Vec3, Vec3, f64) {
    let (base, heading, left, length) = place(organ, look);
    if look.inflorescence.is_some() || look.stalk().is_some() {
        (base, heading, left, length)
    } else {
        (organ.position, heading, left, organ.size)
    }
}

fn cards(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    twists: &[Option<f64>],
    variation: f32,
    lod: &LodSpec,
) -> Vec<Card> {
    if lod.cluster <= 0.0 {
        organ_cards(graph, looks, solid, twists, variation)
    } else {
        cluster_cards(graph, looks, solid, variation, lod)
    }
}

/// One card per organ, and a second across it for organs that stand out
/// all round their axis; a piece of a strap twisted as `twists` holds
/// ([`seated`]).
fn organ_cards(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    twists: &[Option<f64>],
    variation: f32,
) -> Vec<Card> {
    let mut cards = Vec::new();
    for (number, organ) in graph.organs.iter().enumerate() {
        if solid
            .get(usize::from(organ.organ))
            .copied()
            .unwrap_or(false)
        {
            continue;
        }
        let start = cards.len();
        organ_card(organ, looks, variation, 1.0, None, &mut cards);
        if let Some(twist) = twists.get(number).copied().flatten() {
            for card in &mut cards[start..] {
                card.bend.twist = twist;
            }
        }
    }
    cards
}

/// `organ`'s card as level 0 draws it, `widen` times as wide, and a second
/// across it for organs that stand out all round their axis, or else, when
/// `across` is given, one that share of the card's width (a thick leaf's
/// edge).
fn organ_card(
    organ: &GraphOrgan,
    looks: &[Look],
    variation: f32,
    widen: f64,
    across: Option<f64>,
    out: &mut Vec<Card>,
) {
    let index = usize::from(organ.organ);
    let Some(look) = looks.get(index) else {
        return;
    };
    // A sun, shade or juvenile leaf is drawn with its family's look
    // (plant roadmap P4), placed and coloured as its type's.
    let (drawn, share) = look.forms.of(index, organ);
    let (Some(shown), Ok(template)) = (looks.get(drawn), u8::try_from(drawn)) else {
        return;
    };
    let (base, heading, left, length) = match look.stalk() {
        Some(stalk) => on_stalk(organ, shown, stalk, share),
        None => {
            let (base, heading, left, length) = place(organ, look);
            (base, heading, left, length * share)
        }
    };
    let card = Placed {
        base,
        heading,
        left,
        length,
        width: length * shown.aspect() * widen,
        color: organ_color(look, variation, organ.light, organ.id),
        template,
        born: organ.born,
        shed: organ.shed,
        bend: shown.bend,
        segment: organ.segment.unwrap_or(GROUNDED),
    };
    if let Some(cross) = shown.cross().or(across) {
        // A second card across the first keeps organs that stand
        // out all round their axis full when seen edge on.
        out.push(
            Placed {
                left: heading.cross(left),
                width: card.width * cross,
                ..card
            }
            .finish(),
        );
    }
    out.push(card.finish());
}

/// The quads of `organ`'s card, `widen` times as wide, as the levels that
/// keep it draw it: bent by its look, traced at its base, middle and tip
/// (two quads), and the card across it for organs that stand out all
/// round their axis.
fn organ_quads(organ: &GraphOrgan, look: &Look, widen: f64) -> Vec<[Vec3; 4]> {
    let (base, heading, left, length) = traced(organ, look);
    let half = 0.5 * look.aspect() * length * widen;
    let at = |x: f64, v: f64| look.bend.point(base, heading, left, half, length, x, v).0;
    let ring = [0.0, 0.5, 1.0].map(|v| [at(-1.0, v), at(1.0, v)]);
    let mut quads = vec![
        [ring[0][0], ring[0][1], ring[1][1], ring[1][0]],
        [ring[1][0], ring[1][1], ring[2][1], ring[2][0]],
    ];
    if let Some(cross) = look.cross() {
        let across = heading.cross(left) * (half * cross.min(1.0));
        quads.push([
            base - across,
            base + across,
            base + heading * length + across,
            base + heading * length - across,
        ]);
    }
    quads
}

/// The area of `organ`'s card as the coarse levels trace it ([`traced`]).
fn traced_area(organ: &GraphOrgan, look: &Look) -> f64 {
    let (.., length) = traced(organ, look);
    look.aspect() * length * length
}

/// A look at most this wide for its length is a blade (yucca, sotol,
/// agave, grasses): its organs can form a rosette.
const STAR_ASPECT: f64 = 0.25;
/// A cell's blades are a rosette when their bases lie within this share
/// of their mean length of their mean base...
const STAR_BASE_SHARE: f64 = 0.25;
/// ...and their unit headings average to a vector at most this long, so
/// they point every way rather than along one (blades spread evenly over
/// the upper half of the sphere average to 0.5, a parallel bundle to 1).
const STAR_SPREAD: f64 = 0.8;
/// The blades a coarse level keeps of a rosette (render review S1).
const STAR_BLADES: usize = 16;
/// The most a kept blade is widened; past it more blades are kept.
const STAR_WIDEN_MAX: f64 = 4.0;

/// A rosette's coarse form (render review S1): cluster cards fit a cell's
/// organs by the way they face, and a rosette's blades face every way from
/// one point, so its cards became a flat cross through the head. Instead
/// a coarse level keeps [`STAR_BLADES`] of its blades, spread over their
/// directions (each next the one whose heading lies furthest from those
/// kept), each drawn as level 0 draws it and widened alike so that the
/// star covers what the whole rosette covers ([`Cluster::cover`], the
/// mean over [`cover_directions`]). `None` when the cell's organs are not
/// a rosette.
fn star_cards(
    cluster: &Cluster,
    graph: &PlantGraph,
    look: &Look,
    looks: &[Look],
    variation: f32,
) -> Option<Vec<Card>> {
    let organs: Vec<&GraphOrgan> = cluster.organs.iter().map(|&i| &graph.organs[i]).collect();
    if look.aspect() > STAR_ASPECT || organs.len() < 3 {
        return None;
    }
    let count = to_f64(organs.len());
    let base = organs
        .iter()
        .fold(Vec3::ZERO, |sum, organ| sum + organ.position)
        / count;
    let length = organs.iter().map(|organ| organ.size).sum::<f64>() / count;
    // Where each blade reaches, its bend included: a rosette's blades
    // often leave the head nearly together and arch out every way.
    let headings: Vec<Vec3> = organs
        .iter()
        .map(|organ| {
            let (base, heading, left, length) = traced(organ, look);
            let half = 0.5 * look.aspect() * length;
            let tip = look
                .bend
                .point(base, heading, left, half, length, 0.0, 1.0)
                .0;
            (tip - base).normalize_or(heading)
        })
        .collect();
    let mean = headings.iter().fold(Vec3::ZERO, |sum, &h| sum + h) / count;
    let reach = organs
        .iter()
        .map(|organ| (organ.position - base).length())
        .fold(0.0, f64::max);
    if reach > STAR_BASE_SHARE * length || mean.length() > STAR_SPREAD {
        return None;
    }
    let directions = cover_directions();
    let target = directions.iter().map(|&d| cluster.cover(d)).sum::<f64>();
    // The kept blades' cover at `widen`, measured as the cluster's is.
    let cover = |kept: &[usize], widen: f64| {
        let quads: Vec<[Vec3; 4]> = kept
            .iter()
            .flat_map(|&i| organ_quads(organs[i], look, widen))
            .collect();
        let (mut low, mut high) = (cluster.low, cluster.high);
        for corner in quads.iter().flatten() {
            low = low.min(*corner);
            high = high.max(*corner);
        }
        let area: f64 = kept
            .iter()
            .map(|&i| traced_area(organs[i], look) * widen)
            .sum();
        directions
            .iter()
            .map(|&d| {
                // Never more than the blades' own area.
                let summed = area;
                if cluster.fill <= 1e-6 {
                    summed
                } else {
                    (painted(&quads, low, high, d, cluster.fill) / cluster.fill).min(summed)
                }
            })
            .sum::<f64>()
    };
    let mut keep = STAR_BLADES.min(organs.len());
    loop {
        // Farthest-point picking over the headings, from the longest blade.
        let first = (0..organs.len())
            .max_by(|&a, &b| organs[a].size.total_cmp(&organs[b].size).then(b.cmp(&a)))
            .unwrap_or(0);
        let mut kept = vec![first];
        let mut nearest: Vec<f64> = headings.iter().map(|h| h.dot(headings[first])).collect();
        while kept.len() < keep {
            let Some(next) = (0..organs.len())
                .filter(|i| !kept.contains(i))
                .min_by(|&a, &b| nearest[a].total_cmp(&nearest[b]).then(a.cmp(&b)))
            else {
                break;
            };
            kept.push(next);
            for (i, h) in headings.iter().enumerate() {
                nearest[i] = nearest[i].max(h.dot(headings[next]));
            }
        }
        let mut widen = 1.0;
        for _ in 0..3 {
            let got = cover(&kept, widen);
            if got <= 1e-12 {
                break;
            }
            widen = (widen * target / got).max(1.0);
        }
        if widen <= STAR_WIDEN_MAX || keep >= organs.len() {
            let widen = widen.min(STAR_WIDEN_MAX);
            // A leaf level 0 draws solid shows its edge seen edge on: its
            // thickness, or the depth of its channel or keel.
            let across = look
                .solid
                .as_ref()
                .map(|solid| solid.thickness.max(0.5 * solid.fold).min(1.0));
            let mut cards = Vec::new();
            kept.sort_unstable();
            for &i in &kept {
                organ_card(organs[i], looks, variation, widen, across, &mut cards);
            }
            return Some(cards);
        }
        keep = organs.len().min(keep * 2);
    }
}

/// The organs of one template in one cell, summed.
#[derive(Default)]
struct Cluster {
    position: Vec3,
    normal: Vec3,
    heading: Vec3,
    light: f64,
    count: f64,
    born: f64,
    /// Infinity while any organ is never shed.
    shed: f64,
    id: u64,
    /// Every card of its organs: unit normal and area, metres squared.
    faces: Vec<(Vec3, f64)>,
    /// The box its organs' cards lie in, corners in the plant's frame.
    low: Vec3,
    high: Vec3,
    /// The share of its look's card drawn solid.
    fill: f64,
    /// Each organ's card's centre.
    centres: Vec<Vec3>,
    /// Each organ's card's corners (the crossing card's too).
    quads: Vec<[Vec3; 4]>,
    /// Its organs, indices into the graph's.
    organs: Vec<usize>,
}

/// The most a cluster's card areas are scaled up for its cards hiding
/// each other (see `cluster_cards`).
const CLUSTER_OVERLAP_MAX: f64 = 2.0;

/// Cells a side of the grid a cluster's cover is counted on
/// ([`Cluster::cover`]). A card is sampled at most half a cell apart, so
/// the cost of a count goes with the square of this: 24 keep the levels'
/// widths as 48 did (revision 15) at about a quarter of the cost.
const COVER_GRID: usize = 24;

fn to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The area `quads` (cards, each drawn solid over the share `fill` of it)
/// paint seen along the unit vector `axis`, overlaps counted once: the
/// shadow of the box `low`..`high` cut into [`COVER_GRID`]² cells, each
/// card's area spread over the cells it falls on (sampled at most half a
/// cell apart, so a card thinner than a cell still counts its share), and
/// a cell a card covers the share `o` of letting `1 - fill o` through
/// (Beer and Lambert's law cell by cell): `sum_c A_c (1 - prod_i (1 -
/// fill o_i))`.
fn painted(quads: &[[Vec3; 4]], low: Vec3, high: Vec3, axis: Vec3, fill: f64) -> f64 {
    let side = any_perpendicular(axis);
    let rise = axis.cross(side);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for x in [low.x, high.x] {
        for y in [low.y, high.y] {
            for z in [low.z, high.z] {
                let p = Vec3::new(x, y, z);
                let q = [p.dot(side), p.dot(rise)];
                for k in 0..2 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
    }
    let cell = [
        (hi[0] - lo[0]).max(1e-9) / to_f64(COVER_GRID),
        (hi[1] - lo[1]).max(1e-9) / to_f64(COVER_GRID),
    ];
    let cell_area = cell[0] * cell[1];
    let inverse = [1.0 / cell[0], 1.0 / cell[1]];
    PAINT_GRID.with_borrow_mut(|grid| {
        grid.clear();
        for quad in quads {
            // The card seen along `axis`: a corner and its two edges.
            let flat = quad.map(|c| [c.dot(side) - lo[0], c.dot(rise) - lo[1]]);
            let edge = |a: usize, b: usize| [flat[b][0] - flat[a][0], flat[b][1] - flat[a][1]];
            let (e1, e2) = (edge(0, 1), edge(0, 3));
            let area = (e1[0] * e2[1] - e1[1] * e2[0]).abs();
            if area <= 0.0 {
                continue;
            }
            let steps = |e: [f64; 2]| {
                let span = (e[0] / cell[0]).abs().max((e[1] / cell[1]).abs());
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = (2.0 * span).ceil().clamp(1.0, 4.0 * to_f64(COVER_GRID)) as u32;
                n
            };
            let (n1, n2) = (steps(e1), steps(e2));
            let weight = area / (f64::from(n1) * f64::from(n2)) / cell_area;
            // The samples' offsets along the second edge are the same in
            // every row.
            grid.offsets.clear();
            grid.offsets.extend((0..n2).map(|j| {
                let across = (f64::from(j) + 0.5) / f64::from(n2);
                [e2[0] * across, e2[1] * across]
            }));
            for i in 0..n1 {
                let along = (f64::from(i) + 0.5) / f64::from(n1);
                let start = [flat[0][0] + e1[0] * along, flat[0][1] + e1[1] * along];
                for j in 0..grid.offsets.len() {
                    let offset = grid.offsets[j];
                    grid.sample(
                        cell_index(start[1] + offset[1], cell[1], inverse[1]) * COVER_GRID
                            + cell_index(start[0] + offset[0], cell[0], inverse[0]),
                    );
                }
            }
            grid.shade(weight, fill);
        }
        grid.stopped() * cell_area
    })
}

/// [`painted`]'s grid, kept on each thread between calls so a call costs
/// what its cards cover rather than the whole grid: the light each cell
/// lets through, the current card's samples in each cell and the running
/// sums of its weight, its samples' offsets along its second edge, the
/// cells it touches, and the cells any card touched (listed and flagged).
struct PaintGrid {
    through: Vec<f64>,
    count: Vec<u32>,
    sums: Vec<f64>,
    offsets: Vec<[f64; 2]>,
    card: Vec<usize>,
    touched: Vec<usize>,
    lit: Vec<bool>,
}

thread_local! {
    static PAINT_GRID: RefCell<PaintGrid> = RefCell::new(PaintGrid {
        through: vec![1.0; COVER_GRID * COVER_GRID],
        count: vec![0; COVER_GRID * COVER_GRID],
        sums: Vec::new(),
        offsets: Vec::new(),
        card: Vec::new(),
        touched: Vec::new(),
        lit: vec![false; COVER_GRID * COVER_GRID],
    });
}

impl PaintGrid {
    /// Clears what is left. Each call leaves the grid clear; one cut short
    /// by a panic would not.
    fn clear(&mut self) {
        for &index in &self.card {
            self.count[index] = 0;
        }
        for &index in &self.touched {
            self.through[index] = 1.0;
            self.lit[index] = false;
        }
        self.card.clear();
        self.touched.clear();
    }

    /// Counts one of the current card's samples in the cell `index`.
    fn sample(&mut self, index: usize) {
        if self.count[index] == 0 {
            self.card.push(index);
        }
        self.count[index] += 1;
    }

    /// Lets through `1 - fill o` of each cell the current card's samples
    /// fell in. Every sample adds the same `weight` to its cell's share
    /// `o`, so a cell's share is `weight` added once per sample there,
    /// summed here as the samples would add it.
    fn shade(&mut self, weight: f64, fill: f64) {
        let most = self.card.iter().map(|&index| self.count[index]).max();
        self.sums.clear();
        self.sums.push(0.0);
        let mut sum = 0.0;
        for _ in 0..most.unwrap_or(0) {
            sum += weight;
            self.sums.push(sum);
        }
        for &index in &self.card {
            let covered = self.sums[std::mem::take(&mut self.count[index]) as usize];
            self.through[index] *= 1.0 - fill * covered.min(1.0);
            if !self.lit[index] {
                self.lit[index] = true;
                self.touched.push(index);
            }
        }
        self.card.clear();
    }

    /// The light the cards stopped, in cells, leaving the grid clear. The
    /// cells no card touched let all of it through and add nothing; the
    /// others are summed in the grid's order, as the whole grid would be.
    fn stopped(&mut self) -> f64 {
        self.touched.sort_unstable();
        let mut sum = 0.0;
        for &index in &self.touched {
            sum += 1.0 - self.through[index];
            self.through[index] = 1.0;
            self.lit[index] = false;
        }
        self.touched.clear();
        sum
    }
}

/// How near a cell's edge, in cells, a sample is placed by dividing rather
/// than by multiplying ([`cell_index`]).
const CELL_EDGE: f64 = 1e-6;

/// The cell along one side of [`painted`]'s grid that `x` falls in, cells
/// `cell` wide (`inverse` being `1 / cell`), as `floor(x / cell)` kept to
/// the grid would place it: a float cast truncates toward zero and takes
/// NaN and negatives to 0, which for an index is `floor().max(0.0)`
/// without its call into libm. `x * inverse` differs from `x / cell` by a
/// few units in the last place, so away from a cell's edge it gives the
/// same cell, and near an edge the division decides.
fn cell_index(x: f64, cell: f64, inverse: f64) -> usize {
    let scaled = x * inverse;
    // Inside the grid (which also turns NaN away).
    if scaled >= 0.0 && scaled < to_f64(COVER_GRID) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let near = scaled as u32;
        let within = scaled - f64::from(near);
        if within > CELL_EDGE && within < 1.0 - CELL_EDGE {
            return near as usize;
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (x / cell) as usize;
    index.min(COVER_GRID - 1)
}

impl Cluster {
    /// The area of card its organs need seen along the unit vector `axis`,
    /// where they overlap counted once. Their cards draw the share `f` of
    /// their area solid ([`crate::templates::fill_share`]). Seen along
    /// `axis`, the box's shadow is cut into [`COVER_GRID`]² cells, and a
    /// cell an organ's card covers the share `o` of lets `1 - f o` of the
    /// light through, so the organs paint `sum_c A_c (1 - prod_i (1 - f
    /// o_i))` (Beer and Lambert's law cell by cell, so organs clumped on
    /// their shoots overlap as they do), and a card of the same look
    /// paints that with `1 / f` of it in card: the summed cover while they
    /// are sparse. Summed alone, a rosette's hundred leaves from one point
    /// made a card many times the rosette.
    fn cover(&self, axis: Vec3) -> f64 {
        let summed: f64 = self
            .faces
            .iter()
            .map(|&(normal, area)| normal.dot(axis).abs() * area)
            .sum();
        if self.fill <= 1e-6 || self.quads.is_empty() {
            return summed;
        }
        let painted = painted(&self.quads, self.low, self.high, axis, self.fill);
        // Never more card than the organs' own.
        (painted / self.fill).min(summed)
    }

    /// The length of its box along the unit vector `axis`.
    fn extent(&self, axis: Vec3) -> f64 {
        let size = self.high - self.low;
        size.x * axis.x.abs() + size.y * axis.y.abs() + size.z * axis.z.abs()
    }
}

/// Directions a cluster's cover is matched along: a Fibonacci lattice of
/// [`CLUSTER_DIRECTIONS`] points on the upper half of the unit sphere
/// (cover is the same seen from opposite sides).
pub(crate) fn cover_directions() -> Vec<Vec3> {
    // The golden angle, radians.
    const TURN: f64 = 2.399_963_229_728_653;
    (0..CLUSTER_DIRECTIONS)
        .map(|i| {
            let i = f64::from(u32::try_from(i).unwrap_or(0));
            let count = f64::from(u32::try_from(CLUSTER_DIRECTIONS).unwrap_or(1));
            let y = (i + 0.5) / count;
            let ring = math::sqrt(1.0 - y * y);
            let turn = TURN * i;
            Vec3::new(ring * math::cos(turn), y, ring * math::sin(turn))
        })
        .collect()
}

/// Areas for cards facing `normals` (unit vectors) so that together they
/// cover as much as `cluster`'s organs seen from every side: the
/// non-negative least-squares fit
///
/// ```text
/// minimise  sum_d ( sum_k |n_k . d| a_k  -  C(d) )^2   over a_k >= 0
/// ```
///
/// over the directions `d` of [`cover_directions`], `C(d)` being the
/// organs' cover along `d` ([`Cluster::cover`]). Every subset of the cards
/// is solved by its normal equations and the best fit with no negative
/// area is kept; a single card always gives one.
fn card_areas(normals: &[Vec3], cover: &[f64]) -> Vec<f64> {
    let directions = cover_directions();
    let weights: Vec<f64> = directions
        .iter()
        .map(|d| {
            if d.y <= SIDE_VIEW_SINE {
                1.0
            } else {
                HIGH_VIEW_WEIGHT
            }
        })
        .collect();
    // Each direction's row scaled by the square root of its weight, so the
    // normal equations below fit the weighted sum.
    let root: Vec<f64> = weights.iter().map(|&w| math::sqrt(w)).collect();
    let seen: Vec<Vec<f64>> = normals
        .iter()
        .map(|&normal| directions.iter().map(|&d| normal.dot(d).abs()).collect())
        .collect();
    let seen_weighted: Vec<Vec<f64>> = seen
        .iter()
        .map(|row| row.iter().zip(&root).map(|(s, r)| s * r).collect())
        .collect();
    let cover_weighted: Vec<f64> = cover.iter().zip(&root).map(|(c, r)| c * r).collect();
    let mut best: Option<(f64, Vec<f64>)> = None;
    for subset in 1_usize..(1 << normals.len()) {
        let kept: Vec<usize> = (0..normals.len())
            .filter(|k| subset & (1 << k) != 0)
            .collect();
        let gram: Vec<Vec<f64>> = kept
            .iter()
            .map(|&j| {
                kept.iter()
                    .map(|&k| {
                        seen_weighted[j]
                            .iter()
                            .zip(&seen_weighted[k])
                            .map(|(a, b)| a * b)
                            .sum()
                    })
                    .collect()
            })
            .collect();
        let target: Vec<f64> = kept
            .iter()
            .map(|&j| {
                seen_weighted[j]
                    .iter()
                    .zip(&cover_weighted)
                    .map(|(a, c)| a * c)
                    .sum()
            })
            .collect();
        let Some(solution) = solve(gram, target) else {
            continue;
        };
        if solution.iter().any(|&area| area < 0.0) {
            continue;
        }
        let mut areas = vec![0.0; normals.len()];
        for (&k, &area) in kept.iter().zip(&solution) {
            areas[k] = area;
        }
        let error: f64 = cover
            .iter()
            .enumerate()
            .map(|(d, c)| {
                let drawn: f64 = areas.iter().zip(&seen).map(|(a, s)| a * s[d]).sum();
                weights[d] * (drawn - c) * (drawn - c)
            })
            .sum();
        if best.as_ref().is_none_or(|(least, _)| error < *least) {
            best = Some((error, areas));
        }
    }
    best.map_or_else(|| vec![0.0; normals.len()], |(_, areas)| areas)
}

/// The solution of the small linear system `matrix · x = rhs` by Gaussian
/// elimination with partial pivoting; `None` when it is singular.
fn solve(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Option<Vec<f64>> {
    let n = rhs.len();
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))?;
        if matrix[pivot][column].abs() < 1e-12 {
            return None;
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let pivot_row = matrix[column].clone();
        for row in column + 1..n {
            let factor = matrix[row][column] / pivot_row[column];
            for (value, above) in matrix[row][column..n].iter_mut().zip(&pivot_row[column..n]) {
                *value -= factor * above;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let rest: f64 = (row + 1..n).map(|k| matrix[row][k] * x[k]).sum();
        x[row] = (rhs[row] - rest) / matrix[row][row];
    }
    Some(x)
}

/// Organs merged per template and cell of edge `lod.cluster`: a card
/// facing as its organs do, and upright cards beside it.
#[allow(clippy::too_many_lines)]
fn cluster_cards(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    variation: f32,
    lod: &LodSpec,
) -> Vec<Card> {
    // BTreeMap keeps the output order independent of hashing.
    let mut clusters: BTreeMap<(u8, i64, i64, i64), Cluster> = BTreeMap::new();
    for (number, organ) in graph.organs.iter().enumerate() {
        let index = usize::from(organ.organ);
        if solid.get(index).copied().unwrap_or(false) {
            continue;
        }
        let (Some(look), Ok(template)) = (looks.get(index), u8::try_from(index)) else {
            continue;
        };
        let (base, heading, left, length) = traced(organ, look);
        #[allow(clippy::cast_possible_truncation)]
        let key = (
            template,
            (organ.position.x / lod.cluster).floor() as i64,
            (organ.position.y / lod.cluster).floor() as i64,
            (organ.position.z / lod.cluster).floor() as i64,
        );
        let entry = clusters.entry(key).or_insert_with(|| Cluster {
            born: f64::INFINITY,
            shed: f64::NEG_INFINITY,
            id: organ.id,
            fill: crate::templates::fill_share(look),
            low: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            high: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ..Cluster::default()
        });
        let area = traced_area(organ, look);
        // The organ's card as the levels that keep it draw it: bent by its
        // look (`crate::bend`: folded, cupped, drooping), traced at its
        // base, middle and tip, so a cluster reaches as far as its leaves
        // do and faces as they face. Flat looks bend nowhere.
        let half = 0.5 * look.aspect() * length;
        let at = |x: f64, v: f64| look.bend.point(base, heading, left, half, length, x, v);
        let ring = [0.0, 0.5, 1.0].map(|v| [at(-1.0, v).0, at(0.0, v).0, at(1.0, v).0]);
        let centre = ring[1][1];
        entry.centres.push(centre);
        for k in 0..2 {
            entry
                .quads
                .push([ring[k][0], ring[k][2], ring[k + 1][2], ring[k + 1][0]]);
        }
        if let Some(cross) = look.cross() {
            let across = heading.cross(left) * (half * cross.min(1.0));
            entry.quads.push([
                base - across,
                base + across,
                base + heading * length + across,
                base + heading * length - across,
            ]);
        }
        for corner in ring.iter().flatten() {
            entry.low = entry.low.min(*corner);
            entry.high = entry.high.max(*corner);
        }
        let chord = (ring[2][1] - ring[0][1]).normalize_or(heading);
        entry.position += centre;
        entry.heading += chord * area;
        for v in [0.25, 0.75] {
            let normal = at(0.0, v).1;
            entry.normal += normal * (0.5 * area);
            entry.faces.push((normal, 0.5 * area));
        }
        if let Some(cross) = look.cross() {
            // The card across faces along the organ's left (see
            // `organ_cards`).
            entry.faces.push((left, area * cross));
        }
        entry.light += organ.light;
        entry.count += 1.0;
        entry.organs.push(number);
        // A cluster shows from its first organ's birth until its last
        // organ is shed.
        entry.born = entry.born.min(organ.born);
        entry.shed = entry.shed.max(organ.shed.unwrap_or(f64::INFINITY));
    }
    let mut cards = Vec::new();
    for ((template, ..), cluster) in clusters {
        let look = &looks[usize::from(template)];
        if let Some(star) = star_cards(&cluster, graph, look, looks, variation) {
            cards.extend(star);
            continue;
        }
        let position = cluster.position / cluster.count;
        let normal = cluster.normal.normalize_or(Vec3::Y);
        let heading = (cluster.heading - normal * normal.dot(cluster.heading))
            .normalize_or(any_perpendicular(normal));
        let left = normal.cross(heading);
        // Leaves turned to the light merge into a level card, which
        // vanishes edge on, so two upright cards, one facing out from the
        // plant's axis and one around it, carry the cover the leaves give
        // from the side; one facing nearly as the merged card is left out.
        // A cluster never has more cards than organs, so a coarser level
        // never draws more cards than a finer one.
        let level = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        let out = level(position).normalize_or(level(heading).normalize_or(Vec3::X));
        let around = Vec3::Y.cross(out);
        let mut facings = vec![(normal, heading, left)];
        let mut spare = cluster.count - 1.0;
        for facing in [out, around] {
            if spare < 1.0 || facing.dot(normal).abs() > CLUSTER_UPRIGHT_SKIP {
                continue;
            }
            spare -= 1.0;
            // Along the organs' mean heading as it shows on the upright
            // card, so a spray of needles along a level shoot stays level.
            let along =
                (cluster.heading - facing * facing.dot(cluster.heading)).normalize_or(Vec3::Y);
            facings.push((facing, along, facing.cross(along)));
        }
        let normals: Vec<Vec3> = facings.iter().map(|&(facing, ..)| facing).collect();
        // Each card keeps the organs' proportions: length by width at the
        // look's aspect.
        let aspect = look.aspect().max(MIN_CLUSTER_ASPECT);
        // A card never reaches past its organs' box and keeps the look's
        // aspect, so where one card of the area would, the area is shared
        // by cards in a row, each as large as the box allows. The extra
        // cards come from the organs left spare, so a cluster never has
        // more cards than organs, shared among the facings by how many each
        // wants.
        // The organs' cover along each of the fit's directions, counted
        // once for the fit and the overlap below.
        let covers: Vec<f64> = cover_directions()
            .iter()
            .map(|&d| cluster.cover(d))
            .collect();
        let fitted: Vec<_> = facings
            .into_iter()
            .zip(card_areas(&normals, &covers))
            .filter(|&(_, area)| area > 0.0)
            .map(|((_, heading, left), area)| (heading, left, area))
            .collect();
        // The cards for the fitted areas times `scale`, as (centre,
        // heading, left, length, width).
        let plan = |scale: f64| {
            let planned: Vec<_> = fitted
                .iter()
                .map(|&(heading, left, area)| {
                    let area = area * scale;
                    let (length, width) = (math::sqrt(area / aspect), math::sqrt(area * aspect));
                    let (along, across) = (
                        cluster.extent(heading) / length,
                        cluster.extent(left) / width,
                    );
                    // The cards go in a row where the box has more room
                    // for them: end to end when it is too narrow for one,
                    // side by side when too short.
                    let row = if across < along { heading } else { left };
                    let fit = along.min(across).min(1.0);
                    let largest = area * fit * fit;
                    // Enough cards to hold the area, none too large.
                    let wanted = if largest > 1e-12 {
                        (area / largest - 1e-9).ceil().max(1.0)
                    } else {
                        0.0
                    };
                    (heading, left, area, largest, row, wanted)
                })
                // A facing its organs' box has no room for (an upright
                // card on level leaves, whose box has no height) draws
                // nothing.
                .filter(|&(.., wanted)| wanted >= 1.0)
                .collect();
            // The organs not standing for a facing's first card.
            #[allow(clippy::cast_precision_loss)]
            let spare = (cluster.count - planned.len() as f64).max(0.0);
            let extra: f64 = planned.iter().map(|&(.., wanted)| wanted - 1.0).sum();
            let portion = if extra > 0.0 {
                (spare / extra).min(1.0)
            } else {
                0.0
            };
            let mut placed = Vec::new();
            for (heading, left, area, largest, row, wanted) in planned {
                let count = 1.0 + ((wanted - 1.0) * portion).floor();
                let each = (area / count).min(largest);
                let (length, width) = (math::sqrt(each / aspect), math::sqrt(each * aspect));
                // Each card stands at the middle of its share of the
                // organs, taken in order along the row: a row of cards
                // follows its organs, where cards spread evenly over the
                // box would fill its empty corners (a rosette is a star,
                // not a square).
                let mut along: Vec<(f64, usize)> = cluster
                    .centres
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.dot(row), i))
                    .collect();
                along.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let groups = count as usize;
                for k in 0..groups {
                    let (from, to) = (k * along.len() / groups, (k + 1) * along.len() / groups);
                    let share = &along[from..to.max(from + 1).min(along.len())];
                    let centre = if groups == 1 || share.is_empty() {
                        position
                    } else {
                        let sum = share
                            .iter()
                            .fold(Vec3::ZERO, |sum, &(_, i)| sum + cluster.centres[i]);
                        let mean = sum / to_f64(share.len());
                        position + row * (mean - position).dot(row)
                    };
                    placed.push((centre, heading, left, length, width));
                }
            }
            placed
        };
        // A cluster's cards stand among each other (its level card and its
        // upright ones cross), so together they draw less than their fitted
        // areas add up to: measured over the fit's directions and weights,
        // their areas are scaled once so they draw what the organs do.
        let drawn = |placed: &[(Vec3, Vec3, Vec3, f64, f64)], axis: Vec3| {
            let quads: Vec<[Vec3; 4]> = placed
                .iter()
                .map(|&(centre, heading, left, length, width)| {
                    let (h, l) = (heading * (0.5 * length), left * (0.5 * width));
                    [
                        centre - h - l,
                        centre - h + l,
                        centre + h + l,
                        centre + h - l,
                    ]
                })
                .collect();
            let (mut low, mut high) = (cluster.low, cluster.high);
            for corner in quads.iter().flatten() {
                low = low.min(*corner);
                high = high.max(*corner);
            }
            painted(&quads, low, high, axis, cluster.fill)
        };
        let first = plan(1.0);
        let (mut wanted, mut got) = (0.0, 0.0);
        if cluster.fill > 1e-6 && first.len() > 1 {
            for (direction, &cover) in cover_directions().iter().zip(&covers) {
                let direction = *direction;
                let weight = if direction.y <= SIDE_VIEW_SINE {
                    1.0
                } else {
                    HIGH_VIEW_WEIGHT
                };
                wanted += weight * cluster.fill * cover;
                got += weight * drawn(&first, direction);
            }
        }
        let placed = if got > 1e-12 {
            plan((wanted / got).clamp(1.0, CLUSTER_OVERLAP_MAX))
        } else {
            first
        };
        for (centre, heading, left, length, width) in placed {
            cards.push(
                Placed {
                    base: centre - heading * (length * 0.5),
                    heading,
                    left,
                    length,
                    width,
                    color: organ_color(look, variation, cluster.light / cluster.count, cluster.id),
                    template,
                    born: cluster.born,
                    shed: cluster.shed.is_finite().then_some(cluster.shed),
                    bend: Bend::FLAT,
                    segment: cluster.organs.first().map_or(NO_SEGMENT, |&first| {
                        graph.organs[first].segment.unwrap_or(GROUNDED)
                    }),
                }
                .finish(),
            );
        }
    }
    cards
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::graph::{GraphOrgan, GraphSegment};
    use crate::looks::{Flower, Needles, OrganLook, Shape};
    use crate::lsys::OrganKind;

    fn card_at(base: [f32; 3], heading: [f32; 3], bend: Bend) -> Card {
        Card {
            base,
            heading,
            left: [0.0, 0.0, 1.0],
            length: 0.3,
            width: 0.1,
            color: [0.2, 0.5, 0.2],
            template: 0,
            born: 0.0,
            shed: f32::INFINITY,
            bend,
            segment: NO_SEGMENT,
            seed: crate::templates::card_seed(base),
        }
    }

    fn low_of(card: &Card) -> f64 {
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        card_low(
            card,
            vector(card.base),
            vector(card.heading),
            vector(card.left),
        )
    }

    /// A leaf drooping into the soil turns up about its base just enough to
    /// rest on it; a spray standing at the foot is raised onto the ground;
    /// a card clear of the ground keeps its bytes.
    #[test]
    fn cards_stay_above_the_ground() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let drooping = card_at([0.0, 0.1, 0.0], [s, -s, 0.0], Bend::FLAT);
        // Face up, so its midrib droops toward the ground.
        let arching = Card {
            left: [0.0, 0.0, -1.0],
            ..card_at(
                [0.0, 0.05, 0.0],
                [1.0, 0.0, 0.0],
                Bend {
                    droop: 60.0,
                    ..Bend::FLAT
                },
            )
        };
        let sunk = card_at([0.0, -0.15, 0.0], [0.0, 1.0, 0.0], Bend::FLAT);
        let high = card_at([0.0, 0.3, 0.0], [s, -s, 0.0], Bend::FLAT);
        let mut cards = vec![drooping, arching, sunk, high];
        assert!(cards[..3].iter().all(|card| low_of(card) < -0.01));
        grounded(&mut cards);
        for card in &cards {
            assert!(low_of(card) > -1.0e-5, "{card:?}");
        }
        // Turned, not moved, and only as far as the ground.
        assert_eq!(cards[0].base, drooping.base);
        assert!(cards[0].heading[1] > drooping.heading[1]);
        assert!(low_of(&cards[0]) < 1.0e-3);
        assert_eq!(cards[1].base, arching.base);
        assert!(low_of(&cards[1]) < 1.0e-3);
        // Pointing up already: raised to stand on the ground.
        assert_eq!(cards[2].heading, sunk.heading);
        assert!((cards[2].base[1] - 0.0).abs() < 1.0e-5);
        assert_eq!(cards[3], high);
    }

    #[test]
    fn a_plant_sets_its_leaves_share_where_its_look_says() {
        let mut looks = looks();
        let mut variegation: crate::looks::Variegation = serde_json::from_str(
            r#"{"pattern": "spots", "colour": [0.5, 0.5, 0.5], "amount": 0.3}"#,
        )
        .expect("reads");
        let cards: Vec<Card> = (0..5_u8)
            .map(|k| card_at([f32::from(k) * 0.1, 0.2, 0.0], [0.0, 1.0, 0.0], Bend::FLAT))
            .collect();
        let mut seeds = |plants: f64, seed: u64| {
            variegation.plants = plants;
            looks[0].variegation = Some(variegation);
            let mut plant = PlantMesh {
                cards: cards.clone(),
                ..PlantMesh::default()
            };
            plant.vary_by_plant(&looks, seed);
            plant
                .cards
                .iter()
                .map(|card| card.seed)
                .collect::<Vec<f32>>()
        };
        // Each leaf its own, as before; or every leaf the plant's, and
        // two plants different.
        let own: Vec<f32> = cards
            .iter()
            .map(|card| crate::templates::card_seed(card.base))
            .collect();
        assert_eq!(seeds(0.0, 1), own);
        let first = seeds(1.0, 1);
        assert!(
            first
                .iter()
                .all(|&seed| seed == crate::templates::plant_seed(1))
        );
        assert_ne!(first[0], seeds(1.0, 2)[0]);
        // Halfway: halfway between.
        let half = seeds(0.5, 1);
        for (k, seed) in half.iter().enumerate() {
            assert!((seed - 0.5 * (own[k] + first[k])).abs() < 1e-6);
        }
    }

    /// A plant without bodies at its nearest level.
    fn build_plain(
        graph: &PlantGraph,
        looks: &[Look],
        appearance: &Appearance,
        lod: &LodSpec,
    ) -> PlantMesh {
        build(graph, looks, &[], appearance, lod, 0)
    }

    fn appearance() -> Appearance {
        Appearance {
            bark: [0.3, 0.2, 0.1],
            foliage: [0.1, 0.3, 0.1],
            foliage_shade: [0.05, 0.15, 0.08],
            variation: 0.2,
            organs: BTreeMap::new(),
            defaults: BTreeMap::new(),
            ridges: None,
            flare: None,
            moss: None,
            bottle: None,
            roots: None,
            bark_pattern: None,
            leaf_axis: None,
            young_axis: None,
            stem_hairs: None,
            bodies: BTreeMap::new(),
            spreads: serde_json::Map::new(),
        }
    }

    fn looks() -> Vec<Look> {
        appearance().looks([("leaf", OrganKind::Leaf)])
    }

    fn lod(min_radius: f64, cluster: f64, bend: f64) -> LodSpec {
        LodSpec {
            min_radius,
            ring_edge: 0.05,
            min_sides: 3,
            max_sides: 8,
            bend,
            cluster,
        }
    }

    fn segment(
        parent: Option<u32>,
        lateral: bool,
        start: Vec3,
        end: Vec3,
        radius: f64,
    ) -> GraphSegment {
        GraphSegment {
            id: 0,
            parent,
            lateral,
            order: u16::from(lateral),
            start,
            end,
            radius,
            born: 0.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
            young: 0.0,
        }
    }

    fn graph() -> PlantGraph {
        let up = |y: f64| Vec3::new(0.0, y, 0.0);
        PlantGraph {
            age: 1.0,
            height: 3.0,
            segments: vec![
                segment(None, true, up(0.0), up(1.0), 0.05),
                segment(Some(0), false, up(1.0), up(2.0), 0.04),
                segment(Some(1), false, up(2.0), up(3.0), 0.03),
                segment(Some(0), true, up(1.0), Vec3::new(1.0, 1.5, 0.0), 0.002),
            ],
            organs: (0..20)
                .map(|i| GraphOrgan {
                    id: i,
                    organ: 0,
                    segment: Some(2),
                    position: Vec3::new(0.1 * f64::from(u32::try_from(i).unwrap()), 2.5, 0.0),
                    heading: Vec3::X,
                    left: Vec3::Z,
                    size: 0.1,
                    born: 0.0,
                    shed: None,
                    light: 0.5,
                    within: None,
                    adult: None,
                })
                .collect(),
            structures: Vec::new(),
        }
    }

    #[test]
    fn straight_axes_collapse_to_two_rings_and_thin_wood_is_pruned() {
        let detailed = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        let coarse = build_plain(&graph(), &looks(), &appearance(), &lod(0.01, 0.0, 10.0));
        // Stem: 4 rings of 7 sides (8 vertices with the seam); the 2 mm
        // branch gets the 3-side minimum.
        assert_eq!(detailed.wood.vertex_count(), 4 * 8 + 2 * 4);
        // The straight stem needs only its end rings; the thin branch is
        // gone, a three-sided stick of two rings in its place (L10).
        assert_eq!(coarse.wood.vertex_count(), 2 * 8 + 2 * 4);
        // The stem stays even below a minimum radius thicker than itself.
        let coarsest = build_plain(&graph(), &looks(), &appearance(), &lod(0.1, 0.0, 10.0));
        assert_eq!(coarsest.wood.vertex_count(), 2 * 8 + 2 * 4);
        assert_eq!(detailed.cards.len(), 20);
        // Simple leaves bend by default: each card a grid of 16 triangles.
        assert_eq!(detailed.bent_cards(), 20);
        assert_eq!(
            detailed.card_mesh().triangle_count(),
            20 * crate::bend::VERTICES as usize / 3
        );
    }

    /// A level scaled to a small plant keeps the wood it would drop at a
    /// tree's scale: the 3 m test plant's 2 mm branch survives a 1 cm
    /// minimum radius once the level is scaled by 3 / 15.
    #[test]
    fn levels_scale_with_the_plant() {
        let coarse = lod(0.01, 1.0, 10.0);
        let scaled = coarse.for_height(3.0);
        assert!((scaled.min_radius - 0.002).abs() < 1e-12);
        assert!((scaled.ring_edge - 0.01).abs() < 1e-12);
        assert!((scaled.cluster - 0.2).abs() < 1e-12);
        assert_eq!((scaled.min_sides, scaled.max_sides), (3, 8));
        assert!((scaled.bend - 10.0).abs() < 1e-12);
        // Trees at or above the reference height use the level as given,
        // and the scale stops at 1/100.
        assert_eq!(coarse.for_height(LOD_REFERENCE_HEIGHT), coarse);
        assert_eq!(coarse.for_height(80.0), coarse);
        assert!((coarse.for_height(0.0).min_radius - 0.0001).abs() < 1e-12);

        let graph = graph();
        let tree_scale = build_plain(&graph, &looks(), &appearance(), &coarse);
        let plant_scale = build_plain(&graph, &looks(), &appearance(), &scaled);
        // At a tree's scale the straight stem has two rings of 7 sides and
        // the thin branch is a stick (L10). Scaled, the 1 cm ring edge
        // gives the stem the 8-side maximum and the branch is back with 3
        // sides.
        assert_eq!(tree_scale.wood.vertex_count(), 2 * 8 + 2 * 4);
        assert_eq!(plant_scale.wood.vertex_count(), 2 * 9 + 2 * 4);
        // Scaled, the 0.2 m cells merge less: leaves scattered over a patch
        // keep more cards.
        let scattered = scattered_leaves();
        assert!(
            build_plain(&scattered, &looks(), &appearance(), &scaled)
                .cards
                .len()
                > build_plain(&scattered, &looks(), &appearance(), &coarse)
                    .cards
                    .len()
        );
    }

    #[test]
    fn clustering_merges_organs_and_keeps_birth_ages() {
        // Leaves scattered over a patch merge: two cells of ten flat level
        // leaves, each a level card, and no upright card, since level
        // leaves cover nothing seen from the side.
        let scattered = scattered_leaves();
        let merged = build_plain(
            &scattered,
            &flat_looks(),
            &appearance(),
            &lod(0.0, 1.0, 0.0),
        );
        assert_eq!(merged.cards.len(), 2);
        // Leaves in one row along a twig, a metre long and a leaf wide,
        // stay a row: one card at their aspect would be far wider than
        // they are (plant leftover L12), so each cell's cover is shared by
        // cards end to end along the row, never more than its leaves (here,
        // leaves touching end to end, one card each).
        let clustered = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        assert!(clustered.cards.len() > 2 && clustered.cards.len() <= 20);
        let leaf_width = 0.1 * looks()[0].shape.aspect();
        for card in &clustered.cards {
            let across = f64::from(card.width);
            assert!(
                across <= leaf_width + 1e-6,
                "a card {across} m wide on a row of leaves {leaf_width} m wide"
            );
        }
        assert!(
            clustered
                .cards
                .iter()
                .all(|card| card.born == 0.0 && card.shed == f32::INFINITY)
        );
        let mesh = clustered.card_mesh();
        assert!(mesh.births.iter().all(|birth| *birth == 0.0));
        assert!(mesh.sheds.iter().all(|shed| *shed == f32::INFINITY));
    }

    /// The area `meshes`' cards draw seen along `axis`, each solid over
    /// the test look's share, overlaps counted once ([`painted`]) over one
    /// box round all of them, so meshes compare as a renderer shows them.
    fn drawn(meshes: &[&PlantMesh], axis: Vec3) -> Vec<f64> {
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        let quads: Vec<Vec<[Vec3; 4]>> = meshes
            .iter()
            .map(|mesh| {
                mesh.cards
                    .iter()
                    .map(|card| {
                        let (base, h, l) =
                            (vector(card.base), vector(card.heading), vector(card.left));
                        let half = l * (0.5 * f64::from(card.width));
                        let tip = h * f64::from(card.length);
                        [
                            base - half,
                            base + half,
                            base + tip + half,
                            base + tip - half,
                        ]
                    })
                    .collect()
            })
            .collect();
        let (mut low, mut high) = (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for corner in quads.iter().flatten().flatten() {
            low = low.min(*corner);
            high = high.max(*corner);
        }
        let fill = crate::templates::fill_share(&looks()[0]);
        quads
            .iter()
            .map(|quads| painted(quads, low, high, axis, fill))
            .collect()
    }

    /// The test look drawn flat, as level leaves with no bend.
    fn flat_looks() -> Vec<Look> {
        let mut looks = looks();
        for look in &mut looks {
            look.bend = crate::bend::Bend::FLAT;
        }
        looks
    }

    /// The test plant's twenty leaves spread over two patches, a cell of 1
    /// m each, ten leaves a patch on a square grid, as a crown's leaves
    /// lie rather than in one line.
    fn scattered_leaves() -> PlantGraph {
        let mut graph = graph();
        for (i, organ) in (0_u32..).zip(&mut graph.organs) {
            let (patch, k) = (f64::from(i / 10), i % 10);
            organ.position = Vec3::new(
                0.1 + 1.0 * patch + 0.2 * f64::from(k % 4),
                2.5,
                0.1 + 0.25 * f64::from(k / 4),
            );
        }
        graph
    }

    /// The area of `mesh`'s cards seen along `axis`.
    fn cover(mesh: &PlantMesh, axis: Vec3) -> f64 {
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        mesh.cards
            .iter()
            .map(|card| {
                let normal = vector(card.heading).cross(vector(card.left));
                normal.dot(axis).abs() * f64::from(card.length) * f64::from(card.width)
            })
            .sum()
    }

    /// A rosette: 120 narrow blades from one point, spread over the upper
    /// half of the sphere (a golden spiral), as a yucca's head.
    fn rosette() -> (PlantGraph, Vec<Look>) {
        let mut graph = graph();
        graph.organs = (0_u32..120)
            .map(|i| {
                let up = 0.05 + 0.9 * (f64::from(i) + 0.5) / 120.0;
                let ring = math::sqrt(1.0 - up * up);
                let turn = 2.399_963_229_728_653 * f64::from(i);
                let heading = Vec3::new(ring * math::cos(turn), up, ring * math::sin(turn));
                GraphOrgan {
                    id: u64::from(i),
                    organ: 0,
                    segment: Some(2),
                    position: Vec3::new(0.0, 2.5, 0.0),
                    heading,
                    left: any_perpendicular(heading),
                    size: 0.5,
                    born: 0.0,
                    shed: None,
                    light: 0.5,
                    within: None,
                    adult: None,
                }
            })
            .collect();
        let mut looks = flat_looks();
        looks[0].shape = Shape::Blade(crate::looks::Blade {
            taper: 1.0,
            aspect: 0.04,
            foot: 0.0,
        });
        (graph, looks)
    }

    /// Render review S1: a coarse level draws a rosette as a star of its
    /// own blades, fewer and wider, covering what the rosette covers from
    /// every side and reaching no further, not as a cross of cluster cards.
    #[test]
    fn rosettes_become_stars_of_their_own_blades() {
        let (graph, looks) = rosette();
        let detailed = build_plain(&graph, &looks, &appearance(), &lod(0.0, 0.0, 0.0));
        let coarse = build_plain(&graph, &looks, &appearance(), &lod(0.0, 1.0, 0.0));
        assert_eq!(detailed.cards.len(), 120);
        assert!(
            (STAR_BLADES..=4 * STAR_BLADES).contains(&coarse.cards.len()),
            "{} blades",
            coarse.cards.len()
        );
        // Each kept blade is one of the rosette's own, widened.
        let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        for card in &coarse.cards {
            let heading = vector(card.heading);
            assert!(
                graph
                    .organs
                    .iter()
                    .any(|organ| (place(organ, &looks[0]).1 - heading).length() < 1e-5),
                "a blade not the rosette's own"
            );
            assert!(f64::from(card.width) >= 0.04 * 0.5 * 0.999);
        }
        // It reaches no further than the rosette.
        let reach = |mesh: &PlantMesh| {
            mesh.cards
                .iter()
                .map(|card| {
                    let tip = vector(card.heading) * f64::from(card.length);
                    math::sqrt(tip.x * tip.x + tip.z * tip.z)
                })
                .fold(0.0, f64::max)
        };
        assert!(reach(&coarse) <= reach(&detailed) + 1e-6);
        // It covers as the rosette does, from the side and above.
        let mut ratios = Vec::new();
        for elevation in [0.0_f64, 20.0, 45.0, 90.0] {
            for k in 0..8 {
                let (e, azimuth) = (elevation.to_radians(), f64::from(k) * 45.0_f64.to_radians());
                let axis = Vec3::new(
                    math::cos(e) * math::cos(azimuth),
                    math::sin(e),
                    math::cos(e) * math::sin(azimuth),
                );
                let seen = drawn(&[&coarse, &detailed], axis);
                let ratio = seen[0] / seen[1];
                assert!(
                    (0.6..=1.4).contains(&ratio),
                    "the star covers {ratio} times the rosette at {elevation} degrees, azimuth {k} x 45"
                );
                ratios.push(ratio);
            }
        }
        let mean = ratios.iter().sum::<f64>() / to_f64(ratios.len());
        assert!(
            (mean - 1.0).abs() < 0.15,
            "the star covers {mean} times the rosette"
        );
    }

    /// Twenty leaves in one cell, tilted up to 50° from level toward
    /// every side in turn.
    fn tilted_leaves() -> PlantGraph {
        let mut graph = graph();
        for (i, organ) in (0_u32..).zip(&mut graph.organs) {
            let turn = f64::from(i) * 2.399_963;
            let tilt = 0.87 * f64::from(i % 5) / 4.0;
            let toward = Vec3::new(math::cos(turn), 0.0, math::sin(turn));
            let normal = Vec3::Y * math::cos(tilt) + toward * math::sin(tilt);
            organ.heading = any_perpendicular(normal);
            organ.left = normal.cross(organ.heading);
            organ.position = Vec3::new(0.3 + 0.02 * f64::from(i), 2.5, 0.0);
        }
        graph
    }

    /// Merged into clusters, leaves cover a crown about as much as they do
    /// themselves, seen from any side a walker sees it from, so a coarser
    /// level is neither denser nor sparser than a finer one: within a
    /// third at every level and side view, and within a tenth on average.
    /// Level leaves give no cover from the side, and neither do their
    /// clusters.
    #[test]
    fn clusters_cover_a_crown_as_its_leaves_do() {
        // Flat leaves, so both sides of the comparison draw them as `drawn`
        // reads them.
        let tilted = tilted_leaves();
        let detailed = build_plain(&tilted, &flat_looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        let clustered = build_plain(&tilted, &flat_looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        // A row of leaves 0.4 m long: cards end to end along it, fewer than
        // its leaves (see `clustering_merges_organs_and_keeps_birth_ages`).
        assert!(clustered.cards.len() < tilted.organs.len());
        let mut ratios = Vec::new();
        for elevation in [-45.0_f64, -20.0, 0.0, 20.0, 45.0] {
            for k in 0..8 {
                let (e, azimuth) = (elevation.to_radians(), f64::from(k) * 45.0_f64.to_radians());
                let axis = Vec3::new(
                    math::cos(e) * math::cos(azimuth),
                    math::sin(e),
                    math::cos(e) * math::sin(azimuth),
                );
                let seen = drawn(&[&clustered, &detailed], axis);
                let ratio = seen[0] / seen[1];
                assert!(
                    (0.67..=1.33).contains(&ratio),
                    "clusters cover {ratio} times the leaves at {elevation} degrees, azimuth {k} x 45"
                );
                ratios.push(ratio);
            }
        }
        let count = f64::from(u32::try_from(ratios.len()).unwrap());
        let mean = ratios.iter().sum::<f64>() / count;
        assert!(
            (mean - 1.0).abs() < 0.1,
            "clusters cover {mean} times the leaves on average"
        );
        // Flat level leaves cover nothing from the side, and neither do
        // their clusters; from above, a row of leaves end to end covers as
        // its leaves do.
        let level = build_plain(&graph(), &flat_looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        assert!(cover(&level, Vec3::X) < 1e-9 && cover(&level, Vec3::Z) < 1e-9);
        let leaves = build_plain(&graph(), &flat_looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        let above = drawn(&[&level, &leaves], Vec3::Y);
        assert!(
            (above[0] / above[1] - 1.0).abs() < 0.1,
            "level clusters draw {} of their leaves from above",
            above[0] / above[1]
        );
        // One leaf alone stays one card, so a coarse level never has more
        // cards than the organs it merges.
        let mut lone = graph();
        lone.organs.truncate(1);
        assert_eq!(
            build_plain(&lone, &looks(), &appearance(), &lod(0.0, 1.0, 0.0))
                .cards
                .len(),
            1
        );
    }

    #[test]
    fn tube_rings_wrap_the_axis() {
        let mesh = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        // Seven sides, their vertices out from the 0.05 m radius so the
        // ring's mean width is the stem's.
        let widen = math::PI / (7.0 * math::sin(math::PI / 7.0));
        for (position, normal) in mesh.wood.positions.iter().zip(&mesh.wood.normals).take(8) {
            // First ring of the stem: radius 0.05 around the Y axis. The
            // stem narrows by 1 cm per metre, so its surface faces up by
            // that slope.
            let radial = (position[0] * position[0] + position[2] * position[2]).sqrt();
            assert!((f64::from(radial) - 0.05 * widen).abs() < 1e-6);
            let outward = (normal[0] * position[0] + normal[2] * position[2]) / radial;
            assert!((normal[1] / outward - 0.01).abs() < 1e-4);
        }
    }

    /// Every level draws wood as wide as it is, whatever its sides (plant
    /// leftover L12): a ring's polygon, seen from every direction round
    /// its axis, averages the stem's diameter.
    #[test]
    fn rings_average_the_stems_width_at_every_level() {
        for max_sides in [3, 4, 5, 6, 8, 16] {
            let lod = LodSpec {
                min_radius: 0.0,
                ring_edge: 10.0,
                min_sides: max_sides,
                max_sides,
                bend: 0.0,
                cluster: 0.0,
            };
            let mesh = build_plain(&graph(), &looks(), &appearance(), &lod);
            let sides = max_sides as usize;
            let ring: Vec<[f32; 3]> = mesh.wood.positions[..sides].to_vec();
            let mut widths = 0.0;
            let views = 360;
            for k in 0..views {
                let angle = 2.0 * math::PI * f64::from(k) / f64::from(views);
                let (c, s) = (math::cos(angle), math::sin(angle));
                let along = |p: &[f32; 3]| f64::from(p[0]) * c + f64::from(p[2]) * s;
                let (low, high) = ring
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), p| {
                        (l.min(along(p)), h.max(along(p)))
                    });
                widths += high - low;
            }
            let mean = widths / f64::from(views);
            assert!(
                (mean / 0.1 - 1.0).abs() < 0.002,
                "{max_sides} sides average {mean} m across a 0.1 m stem"
            );
        }
    }

    /// The thin wood a level drops comes back as sticks covering as much
    /// as it did (plant leftover L10): the test plant's 2 mm branch, 1.1 m
    /// long, becomes one three-sided stick of the same cover.
    #[test]
    fn dropped_twigs_become_sticks_that_cover_as_they_did() {
        let coarse = build_plain(&graph(), &looks(), &appearance(), &lod(0.01, 0.0, 10.0));
        // The stem's two rings of 8 vertices, then the stick's two of 4.
        let stick = &coarse.wood.positions[16..24];
        let point = |p: [f32; 3]| Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        let a = point(stick[0]);
        let centre =
            |ring: &[[f32; 3]]| ring[..3].iter().fold(Vec3::ZERO, |sum, &p| sum + point(p)) / 3.0;
        let (start, end) = (centre(&stick[..4]), centre(&stick[4..]));
        let length = (end - start).length();
        let radius = (a - start).length() / (math::PI / (3.0 * math::sin(math::PI / 3.0)));
        let branch = (Vec3::new(1.0, 1.5, 0.0) - Vec3::new(0.0, 1.0, 0.0)).length();
        assert!(
            (length - branch).abs() < 1e-6,
            "the stick is {length} m long"
        );
        // The branch is a tip: `wood` tapers it to six tenths of its 2 mm,
        // so it covers (2 + 1.2) mm along its length, and so does the
        // stick.
        let covered = (0.002 + 0.0012) * branch;
        assert!(
            (2.0 * radius * length / covered - 1.0).abs() < 1e-6,
            "the stick covers {} of the branch",
            2.0 * radius * length / covered
        );
        // A level that drops nothing adds none.
        let detailed = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        assert_eq!(detailed.wood.vertex_count(), 4 * 8 + 2 * 4);
    }

    /// A single straight stem, 0.3 m thick, 4 m tall.
    fn stem() -> PlantGraph {
        let up = |y: f64| Vec3::new(0.0, y, 0.0);
        PlantGraph {
            age: 30.0,
            height: 4.0,
            segments: vec![
                segment(None, false, up(0.0), up(2.0), 0.3),
                segment(Some(0), false, up(2.0), up(4.0), 0.25),
            ],
            organs: Vec::new(),
            structures: Vec::new(),
        }
    }

    fn radial(position: [f32; 3]) -> f32 {
        (position[0] * position[0] + position[2] * position[2]).sqrt()
    }

    #[test]
    fn a_flare_widens_the_stem_base_into_buttresses() {
        let mut look = appearance();
        look.flare = Some(Flare {
            height: 0.5,
            amount: 1.0,
            buttresses: 4,
            ridge: 1.0,
        });
        let fine = LodSpec {
            max_sides: 32,
            ..lod(0.0, 0.0, 0.0)
        };
        let plain = build_plain(&stem(), &looks(), &appearance(), &fine);
        let flared = build_plain(&stem(), &looks(), &look, &fine);
        // Extra rings near the ground show the flare's curve.
        assert!(flared.wood.vertex_count() > plain.wood.vertex_count());
        let ground: Vec<f32> = flared
            .wood
            .positions
            .iter()
            .filter(|p| p[1].abs() < 1e-6)
            .map(|p| radial(*p))
            .collect();
        let (low, high) = ground.iter().fold((f32::MAX, 0.0_f32), |(low, high), r| {
            (low.min(*r), high.max(*r))
        });
        // Buttresses reach out to twice the radius (a vertex may miss the
        // crest by half a side); between them the stem keeps its own.
        assert!((0.55..=0.6).contains(&high), "widest {high}");
        assert!((0.3..0.31).contains(&low), "narrowest {low}");
        // Higher up the flare is gone.
        let top = |mesh: &PlantMesh| {
            mesh.wood
                .positions
                .iter()
                .filter(|p| (p[1] - 4.0).abs() < 1e-6)
                .map(|p| radial(*p))
                .fold(0.0_f32, f32::max)
        };
        assert!((top(&flared) - top(&plain)).abs() < 1e-3);
    }

    #[test]
    fn ridges_cut_darker_furrows_into_thick_wood() {
        let mut look = appearance();
        look.ridges = Some(Ridges {
            spacing: 0.2,
            depth: 0.1,
            twist: 0.0,
            contrast: 0.5,
        });
        let fine = LodSpec {
            max_sides: 32,
            ..lod(0.0, 0.0, 0.0)
        };
        let mesh = build_plain(&stem(), &looks(), &look, &fine).wood;
        let base: Vec<(f32, f32)> = mesh
            .positions
            .iter()
            .zip(&mesh.colors)
            .filter(|(p, _)| p[1].abs() < 1e-6)
            .map(|(p, c)| (radial(*p), c[0]))
            .collect();
        let deepest = base.iter().map(|(r, _)| *r).fold(f32::MAX, f32::min);
        let outermost = base.iter().map(|(r, _)| *r).fold(0.0, f32::max);
        assert!((outermost - 0.3).abs() < 0.005);
        assert!((deepest - 0.27).abs() < 0.005);
        // The furrows' bottoms are the darkest bark.
        let darkest = base
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(r, _)| *r)
            .unwrap();
        assert!((darkest - deepest).abs() < 0.005);
    }

    #[test]
    fn moss_grows_on_top_of_thick_limbs() {
        let mut look = appearance();
        look.moss = Some(Moss {
            colour: [0.0, 1.0, 0.0],
            amount: 1.0,
            min_radius: 0.05,
        });
        let limb = PlantGraph {
            age: 30.0,
            height: 3.0,
            segments: vec![segment(
                None,
                false,
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::new(4.0, 3.0, 0.0),
                0.2,
            )],
            organs: Vec::new(),
            structures: Vec::new(),
        };
        let mesh = build_plain(&limb, &looks(), &look, &lod(0.0, 0.0, 0.0)).wood;
        let green = |above: bool| -> f32 {
            mesh.normals
                .iter()
                .zip(&mesh.colors)
                .filter(|(n, _)| if above { n[1] > 0.7 } else { n[1] < -0.7 })
                .map(|(_, c)| c[1] - 0.2)
                .sum()
        };
        assert!(green(true) > 0.0);
        assert!(green(false).abs() < 1e-6);
    }

    fn organ(heading: Vec3, left: Vec3) -> GraphOrgan {
        GraphOrgan {
            id: 7,
            organ: 0,
            segment: None,
            position: Vec3::new(0.0, 1.0, 0.0),
            heading,
            left,
            size: 0.2,
            born: 0.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        }
    }

    fn one_organ(look: OrganLook, heading: Vec3, left: Vec3) -> Vec<Card> {
        let mut appearance = appearance();
        appearance.organs.insert("leaf".into(), look);
        let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
        let graph = PlantGraph {
            age: 1.0,
            height: 1.0,
            segments: Vec::new(),
            organs: vec![organ(heading, left)],
            structures: Vec::new(),
        };
        build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards
    }

    fn look(shape: Shape, face_up: f64) -> OrganLook {
        OrganLook {
            hairs: None,
            hairs_beneath: None,
            underside: None,
            variegation: None,
            sori: None,
            roll: 0.0,
            insertion: crate::looks::Insertion::Marginal,
            shape,
            colour: None,
            shade: None,
            accent: None,
            face_up,
            solid: None,
            bend: None,
            form: None,
            season: None,
            families: None,
            inflorescence: None,
        }
    }

    fn vector(v: [f32; 3]) -> Vec3 {
        Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
    }

    /// A peltate leaf's blade sits level on the tip of its own petiole,
    /// the point where the petiole meets it there; a latent one meets it
    /// at its foot; a marginal one stays one card on the organ, as before.
    #[test]
    fn a_peltate_blade_sits_level_on_its_petiole() {
        use crate::looks::{Compound, Insertion, Pattern, Simple};
        let heading = Vec3::new(0.0, 0.9, 0.44).normalize_or(Vec3::Y);
        let shapes = [
            Shape::Compound(Compound {
                pattern: Some(Pattern::Palmate),
                leaflets: 11,
                petiole: 0.6,
                leaflet_width: 0.2,
                ..Compound::default()
            }),
            Shape::Simple(Simple {
                petiole: 0.4,
                ..Simple::default()
            }),
        ];
        for shape in shapes {
            for insertion in [Insertion::Latent, Insertion::Peltate] {
                let mut appearance = appearance();
                appearance.organs.insert(
                    "leaf".into(),
                    OrganLook {
                        insertion,
                        ..look(shape.clone(), 0.0)
                    },
                );
                let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
                let stalk = looks[0].stalk().unwrap();
                let graph = PlantGraph {
                    age: 1.0,
                    height: 1.0,
                    segments: Vec::new(),
                    organs: vec![organ(heading, Vec3::X)],
                    structures: Vec::new(),
                };
                let mesh = build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0));
                let [card] = mesh.cards[..] else {
                    panic!("{} cards", mesh.cards.len());
                };
                // Level: its face turned to the sky.
                let face = vector(card.heading).cross(vector(card.left));
                assert!(face.y.abs() > 0.999, "{face:?}");
                // The blade's point on the petiole's tip.
                let at = card
                    .bend
                    .point(
                        vector(card.base),
                        vector(card.heading),
                        vector(card.left),
                        0.5 * f64::from(card.width),
                        f64::from(card.length),
                        0.0,
                        stalk.attach,
                    )
                    .0;
                let tip = graph.organs[0].position + heading * (stalk.petiole * 0.2);
                assert!((at - tip).length() < 1e-5, "{insertion:?}: {at:?} {tip:?}");
                if insertion == Insertion::Latent {
                    assert_eq!(stalk.attach, stalk.petiole);
                } else {
                    assert!(stalk.attach >= stalk.petiole);
                }
                // The petiole is drawn, from the organ to the blade.
                assert!(!mesh.wood.positions.is_empty());
                assert!(loose_organs(&graph, &looks, 1e-5).is_empty());
            }
        }
        // A marginal leaf is one card standing on its organ, as before.
        let cards = one_organ(look(shapes_simple(), 0.0), heading, Vec3::X);
        assert_eq!(vector(cards[0].base), organ(heading, Vec3::X).position);
    }

    fn shapes_simple() -> Shape {
        Shape::Simple(crate::looks::Simple {
            petiole: 0.4,
            ..crate::looks::Simple::default()
        })
    }

    /// A strap of three blade pieces, the turtle pitching down between
    /// them: each piece's card, bent by its look, ends where the next one
    /// starts, and the last ends where the program's line ends.
    #[test]
    fn the_pieces_of_a_strap_join() {
        let mut appearance = appearance();
        appearance.organs.insert(
            "leaf".into(),
            look(Shape::Blade(crate::looks::Blade::default()), 0.0),
        );
        let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
        assert!(looks[0].bend.droop > 0.0 && looks[0].bend.twist > 0.0);
        let (mut position, left) = (Vec3::ZERO, Vec3::X);
        let mut heading = Vec3::new(0.0, 0.9, 0.44).normalize_or(Vec3::Y);
        let mut organs = Vec::new();
        for (k, size) in [0.12, 0.1, 0.088].into_iter().enumerate() {
            organs.push(GraphOrgan {
                id: k as u64,
                position,
                heading,
                left,
                size,
                within: Some(0),
                ..organ(heading, left)
            });
            position += heading * size;
            // Pitch down 14 degrees about the left.
            heading = heading.rotate_about(left, -14f64.to_radians());
        }
        let end = position;
        let graph = PlantGraph {
            age: 1.0,
            height: 0.3,
            segments: Vec::new(),
            organs,
            structures: Vec::new(),
        };
        let cards = build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards;
        assert_eq!(cards.len(), 3);
        let tip = |card: &Card| {
            let half = 0.5 * f64::from(card.width);
            card.bend
                .point(
                    vector(card.base),
                    vector(card.heading),
                    vector(card.left),
                    half,
                    f64::from(card.length),
                    0.0,
                    1.0,
                )
                .0
        };
        for pair in cards.windows(2) {
            let gap = (tip(&pair[0]) - vector(pair[1].base)).length();
            assert!(gap < 1e-5, "a gap of {gap} m between pieces");
        }
        assert!((tip(&cards[2]) - end).length() < 1e-5);
        // Undrawn, the first piece's droop leaves it short and below.
        let flat = place(&graph.organs[0], &looks[0]);
        let straight = looks[0]
            .bend
            .point(flat.0, flat.1, flat.2, 0.006, flat.3, 0.0, 1.0)
            .0;
        assert!((straight - graph.organs[1].position).length() > 0.01);
        assert!(loose_organs(&graph, &looks, 1e-5).is_empty());
    }

    /// A strap of three blade pieces, the turtle rolling 4 degrees and
    /// pitching down 14 between them, as programs draw grass blades and
    /// basal leaves, each piece lit differently: the pieces meet edge to
    /// edge, so the strap does not step in width at its joints (on the
    /// turtle's lefts and twisted by their look, they parted by 4
    /// degrees), and draw in one shade.
    #[test]
    fn the_edges_of_a_strap_meet() {
        let mut appearance = appearance();
        appearance.organs.insert(
            "leaf".into(),
            look(Shape::Blade(crate::looks::Blade::default()), 0.0),
        );
        let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
        let (mut position, mut left) = (Vec3::new(0.0, 0.05, 0.0), Vec3::X);
        let mut heading = Vec3::new(0.0, 0.9, 0.44).normalize_or(Vec3::Y);
        let mut organs = Vec::new();
        for (k, (size, light)) in [(0.12, 0.0), (0.1, 0.3), (0.088, 0.6)]
            .into_iter()
            .enumerate()
        {
            organs.push(GraphOrgan {
                id: k as u64,
                position,
                heading,
                left,
                size,
                light,
                within: Some(0),
                ..organ(heading, left)
            });
            position += heading * size;
            // Roll 6 degrees about the heading, then pitch down 14.
            left = left.rotate_about(heading, 4f64.to_radians());
            heading = heading.rotate_about(left, -14f64.to_radians());
        }
        let graph = PlantGraph {
            age: 1.0,
            height: 0.3,
            segments: Vec::new(),
            organs,
            structures: Vec::new(),
        };
        let cards = build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards;
        assert_eq!(cards.len(), 3);
        let edge = |card: &Card, v: f64| {
            let half = 0.5 * f64::from(card.width);
            let at = |x: f64| {
                card.bend
                    .point(
                        vector(card.base),
                        vector(card.heading),
                        vector(card.left),
                        half,
                        f64::from(card.length),
                        x,
                        v,
                    )
                    .0
            };
            at(1.0) - at(-1.0)
        };
        for pair in cards.windows(2) {
            let (tip, foot) = (edge(&pair[0], 1.0), edge(&pair[1], 0.0));
            let turn = tip.normalize_or(Vec3::X).dot(foot.normalize_or(Vec3::X));
            assert!(
                turn > 0.999_999,
                "the edges meet {} degrees apart",
                turn.acos().to_degrees()
            );
            assert_eq!(pair[0].color, pair[1].color);
        }
        assert!(loose_organs(&graph, &looks, 1e-4).is_empty());
    }

    /// A strap of three blade pieces turned edgewise (a sword leaf's
    /// roll), the turtle pitching down between them: each piece's card,
    /// bent by its look toward its rolled face, ends where the next one
    /// starts, and the last ends where the program's line ends.
    #[test]
    fn the_pieces_of_a_rolled_strap_join() {
        let mut appearance = appearance();
        appearance.organs.insert(
            "leaf".into(),
            OrganLook {
                roll: 90.0,
                ..look(Shape::Blade(crate::looks::Blade::default()), 0.0)
            },
        );
        let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
        assert!(looks[0].bend.droop > 0.0 && looks[0].bend.twist > 0.0);
        // Above the ground, which would turn a card dipping into it.
        let (mut position, left) = (Vec3::new(0.0, 0.05, 0.0), Vec3::X);
        let mut heading = Vec3::new(0.0, 0.9, 0.44).normalize_or(Vec3::Y);
        let mut organs = Vec::new();
        for (k, size) in [0.12, 0.1, 0.088].into_iter().enumerate() {
            organs.push(GraphOrgan {
                id: k as u64,
                position,
                heading,
                left,
                size,
                within: Some(0),
                ..organ(heading, left)
            });
            position += heading * size;
            // Pitch down 14 degrees about the left.
            heading = heading.rotate_about(left, -14f64.to_radians());
        }
        let end = position;
        let graph = PlantGraph {
            age: 1.0,
            height: 0.3,
            segments: Vec::new(),
            organs,
            structures: Vec::new(),
        };
        let cards = build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards;
        assert_eq!(cards.len(), 3);
        let tip = |card: &Card| {
            let half = 0.5 * f64::from(card.width);
            card.bend
                .point(
                    vector(card.base),
                    vector(card.heading),
                    vector(card.left),
                    half,
                    f64::from(card.length),
                    0.0,
                    1.0,
                )
                .0
        };
        for pair in cards.windows(2) {
            let gap = (tip(&pair[0]) - vector(pair[1].base)).length();
            assert!(gap < 1e-5, "a gap of {gap} m between pieces");
        }
        assert!((tip(&cards[2]) - end).length() < 1e-5);
        // Undrawn, the first piece's droop leaves it short and below.
        let flat = place(&graph.organs[0], &looks[0]);
        let straight = looks[0]
            .bend
            .point(flat.0, flat.1, flat.2, 0.006, flat.3, 0.0, 1.0)
            .0;
        assert!((straight - graph.organs[1].position).length() > 0.01);
        assert!(loose_organs(&graph, &looks, 1e-5).is_empty());
    }

    #[test]
    fn leaves_are_drawn_by_their_family() {
        // Sun and shade leaves by their light, juvenile leaves while the
        // plant was young (plant roadmap P4, spec rework L3).
        let mut appearance = appearance();
        appearance.organs.insert(
            "leaf".into(),
            OrganLook {
                families: Some(crate::looks::Families {
                    plasticity: 0.6,
                    juvenile: Some(crate::looks::Juvenile {
                        until: Some(4.0),
                        shape: Shape::Simple(crate::looks::Simple::default()),
                    }),
                }),
                ..look(Shape::Lobed(crate::looks::Lobed::default()), 0.0)
            },
        );
        let looks = appearance.looks([("leaf", OrganKind::Leaf)]);
        let forms = looks[0].forms;
        let leaf = |light: f64, born: f64| GraphOrgan {
            born,
            light,
            ..organ(Vec3::Y, Vec3::X)
        };
        let graph = PlantGraph {
            age: 12.0,
            height: 1.0,
            segments: Vec::new(),
            organs: vec![
                leaf(0.3, 8.0),
                leaf(0.17, 8.0),
                leaf(0.05, 8.0),
                leaf(0.3, 2.0),
            ],
            structures: Vec::new(),
        };
        let cards = build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards;
        let template = |at: usize| usize::from(cards[at].template);
        assert_eq!(cards.len(), 4);
        let by_template = |index: usize| {
            cards
                .iter()
                .find(|card| usize::from(card.template) == index)
                .copied()
                .unwrap()
        };
        let bands = forms.bands.unwrap();
        let (sun, own, shade) = (
            by_template(bands[crate::looks::BANDS - 1]),
            by_template(0),
            by_template(bands[0]),
        );
        assert!(sun.length < own.length && own.length < shade.length);
        assert!(
            cards
                .iter()
                .any(|card| usize::from(card.template) == forms.juvenile.unwrap().0)
        );
        assert!((0..4).all(|at| template(at) < looks.len()));
    }

    #[test]
    fn leaves_held_to_the_light_turn_level_about_their_base() {
        // A leaf hanging straight down, its face to the side.
        let hanging = (-Vec3::Y, Vec3::Z);
        let leaf = Shape::Simple(crate::looks::Simple::default());
        let as_placed = one_organ(look(leaf.clone(), 0.0), hanging.0, hanging.1);
        let level = one_organ(look(leaf.clone(), 1.0), hanging.0, hanging.1);
        let half = one_organ(look(leaf, 0.5), hanging.0, hanging.1);
        assert_eq!(as_placed.len(), 1);
        let normal = |card: &Card| vector(card.heading).cross(vector(card.left));
        assert!(normal(&as_placed[0]).y.abs() < 1e-6);
        assert!((normal(&level[0]).y.abs() - 1.0).abs() < 1e-6);
        // Halfway turns the face 45 degrees.
        assert!((normal(&half[0]).y.abs() - math::sin(math::radians(45.0))).abs() < 1e-6);
        // The base stays on the twig.
        assert_eq!(level[0].base, as_placed[0].base);
    }

    #[test]
    fn flowers_face_along_their_heading_centred_on_the_organ() {
        // Drawn as a card (a flower's default form is solid up close).
        let flower = OrganLook {
            form: Some(crate::blooms::Form::Card),
            ..look(Shape::Flower(Flower::default()), 0.0)
        };
        let cards = one_organ(flower, Vec3::Y, Vec3::X);
        assert_eq!(cards.len(), 1);
        let card = cards[0];
        let normal = vector(card.heading).cross(vector(card.left));
        assert!((normal - Vec3::Y).length() < 1e-6);
        let centre = vector(card.base) + vector(card.heading) * f64::from(card.length * 0.5);
        assert!((centre - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn needles_all_round_get_a_full_crossing_card() {
        let round = Shape::Needles(Needles {
            ranks: 0,
            ..Needles::default()
        });
        // The nearest level draws the shoot solid (`crate::shoots`) and
        // leaves its cards; drawn as a card, it crosses a second.
        assert!(one_organ(look(round.clone(), 0.0), Vec3::X, Vec3::Z).is_empty());
        let card = OrganLook {
            form: Some(crate::blooms::Form::Card),
            ..look(round, 0.0)
        };
        let cards = one_organ(card, Vec3::X, Vec3::Z);
        assert_eq!(cards.len(), 2);
        assert!((cards[0].width - cards[1].width).abs() < 1e-6);
        assert!(vector(cards[0].left).dot(vector(cards[1].left)).abs() < 1e-6);
    }
}
