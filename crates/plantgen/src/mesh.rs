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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::bend::Bend;
use crate::body::{self, BodyLook};
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::{Bottle, Flare, Look, Moss, Mount, Ridges};
use crate::math::{self, Vec3, any_perpendicular};
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
}

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
        }
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
        index
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
}

impl PlantMesh {
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.wood.triangle_count()
            + self.cards.len() * 2
            + self.bent_cards() * (crate::bend::VERTICES as usize / 3 - 2)
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
            let color = [
                card.color[0],
                card.color[1],
                card.color[2],
                f32::from(card.template),
            ];
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

/// A bent card as the renderer draws it: [`crate::bend::VERTICES`]
/// corners of its grid, each its own vertex.
fn bent_card(card: &Card, base: Vec3, heading: Vec3, left: Vec3, out: &mut Mesh) {
    let color = [
        card.color[0],
        card.color[1],
        card.color[2],
        f32::from(card.template),
    ];
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
/// sized to cover as much as its organs ([`card_areas`]).
const CLUSTER_DIRECTIONS: usize = 32;

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
    let parts = parts.filter(|_| level == 0);
    let is_part =
        |index: usize| parts.is_some_and(|parts| parts.get(index).copied().unwrap_or(false));
    let mut mesh = PlantMesh::default();
    wood(graph, appearance, lod, &mut mesh.wood);
    twigs(graph, appearance, lod, sticks, &mut mesh.wood);
    // Organs drawn as solid leaves, flowers or fruit at this level leave
    // the cards.
    let mut solid = leaves::solid_types(looks, level);
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
    blooms::build(graph, looks, &blooms, &colour, &mut mesh.wood);
    mesh.cards = cards(graph, looks, &solid, appearance.variation, lod);
    // Bent cards first, so a renderer draws them as one range.
    let (mut bent, flat): (Vec<Card>, Vec<Card>) =
        mesh.cards.drain(..).partition(|card| !card.bend.is_flat());
    bent.extend(flat);
    mesh.cards = bent;
    if parts.is_some() {
        mesh.sites = sites(&mesh.cards, looks, &is_part);
    }
    body::build(graph, bodies, looks.len(), level, &mut mesh);
    if let Some(roots) = &appearance.roots {
        crate::roots::build(graph, roots, appearance.bark, level <= 1, &mut mesh.wood);
    }
    mesh
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
            .is_some_and(|look| look.shape.cross().is_some());
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
type Twig = (Vec3, Vec3, f64, f64, f64, Option<f64>);

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
            let start = centre + direction * low;
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
                            bark.colour(radius, outward, centre.y, along, angle, 0.0),
                            radius,
                        ),
                        born,
                        shed,
                        level: 2,
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

fn wood(graph: &PlantGraph, appearance: &Appearance, lod: &LodSpec, out: &mut Mesh) {
    let continuations = graph.continuations();
    let end_radii = graph.end_radii();
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
        let bark = Bark::new(appearance, flare, bottle, kept[0].radius, sides, segment.id);
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
                out.push(Vertex {
                    position: current.centre + direction * (radius * widen),
                    normal: surface,
                    uv,
                    color: bark_alpha(
                        bark.colour(
                            current.radius,
                            surface,
                            current.centre.y,
                            along,
                            angle,
                            furrow,
                        ),
                        current.radius,
                    ),
                    born: current.born,
                    shed: current.shed,
                    level,
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
}

impl Placed {
    // Plant-frame metres and ages fit f32 to well under a millimetre and a
    // day.
    #[allow(clippy::cast_possible_truncation)]
    fn finish(self) -> Card {
        Card {
            base: self.base.to_f32(),
            heading: self.heading.to_f32(),
            left: self.left.to_f32(),
            length: self.length as f32,
            width: self.width as f32,
            color: self.color,
            template: self.template,
            born: self.born as f32,
            shed: self.shed.map_or(f32::INFINITY, |shed| shed as f32),
            bend: self.bend,
        }
    }
}

fn organ_color(look: &Look, variation: f32, light: f64, id: u64) -> [f32; 3] {
    // Shaded organs are darker and bluer; each organ varies a little.
    #[allow(clippy::cast_possible_truncation)]
    let shade = (1.0 - light.clamp(0.0, 1.0)) as f32;
    #[allow(clippy::cast_possible_truncation)]
    let jitter = 1.0 + variation * (unit(mix64(id)) as f32 - 0.5);
    std::array::from_fn(|channel| {
        let (lit, shaded) = (look.colour[channel], look.shade[channel]);
        ((lit + (shaded - lit) * shade) * jitter).clamp(0.0, 1.0)
    })
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

/// Where an organ's card goes: its base, heading and left.
pub(crate) fn place(organ: &GraphOrgan, look: &Look) -> (Vec3, Vec3, Vec3) {
    match look.mount() {
        // Standing on the organ, reaching along it.
        Mount::Along => {
            let (heading, left) = face_up(organ.heading, organ.left, look.face_up);
            (organ.position, heading, left)
        }
        // Centred on the organ, its face toward the organ's heading: the
        // card's heading is the turtle's up and its left the turtle's right,
        // so that `heading × left` is the organ's heading.
        Mount::Facing => {
            let up = organ.heading.cross(organ.left);
            let (heading, left) = face_up(up, -organ.left, look.face_up);
            (organ.position - heading * (organ.size * 0.5), heading, left)
        }
    }
}

fn cards(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    variation: f32,
    lod: &LodSpec,
) -> Vec<Card> {
    if lod.cluster <= 0.0 {
        organ_cards(graph, looks, solid, variation)
    } else {
        cluster_cards(graph, looks, solid, variation, lod)
    }
}

/// One card per organ, and a second across it for organs that stand out
/// all round their axis.
fn organ_cards(graph: &PlantGraph, looks: &[Look], solid: &[bool], variation: f32) -> Vec<Card> {
    let mut cards = Vec::new();
    for organ in &graph.organs {
        let index = usize::from(organ.organ);
        if solid.get(index).copied().unwrap_or(false) {
            continue;
        }
        let Some(look) = looks.get(index) else {
            continue;
        };
        // A sun, shade or juvenile leaf is drawn with its family's look
        // (plant roadmap P4), placed and coloured as its type's.
        let (drawn, share) = look.forms.of(index, organ.light, organ.born);
        let (Some(shown), Ok(template)) = (looks.get(drawn), u8::try_from(drawn)) else {
            continue;
        };
        let (base, heading, left) = place(organ, look);
        let card = Placed {
            base,
            heading,
            left,
            length: organ.size * share,
            width: organ.size * share * shown.shape.aspect(),
            color: organ_color(look, variation, organ.light, organ.id),
            template,
            born: organ.born,
            shed: organ.shed,
            bend: shown.bend,
        };
        if let Some(cross) = shown.shape.cross() {
            // A second card across the first keeps organs that stand
            // out all round their axis full when seen edge on.
            cards.push(
                Placed {
                    left: heading.cross(left),
                    width: card.width * cross,
                    ..card
                }
                .finish(),
            );
        }
        cards.push(card.finish());
    }
    cards
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
}

/// The most a cluster's card areas are scaled up for its cards hiding
/// each other (see `cluster_cards`).
const CLUSTER_OVERLAP_MAX: f64 = 2.0;

/// Cells a side of the grid a cluster's cover is counted on
/// ([`Cluster::cover`]).
const COVER_GRID: usize = 48;

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
    let mut through = vec![1.0_f64; COVER_GRID * COVER_GRID];
    let mut share: BTreeMap<usize, f64> = BTreeMap::new();
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
        share.clear();
        for i in 0..n1 {
            for j in 0..n2 {
                let (along, across) = (
                    (f64::from(i) + 0.5) / f64::from(n1),
                    (f64::from(j) + 0.5) / f64::from(n2),
                );
                let p = [
                    flat[0][0] + e1[0] * along + e2[0] * across,
                    flat[0][1] + e1[1] * along + e2[1] * across,
                ];
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (column, row) = (
                    ((p[0] / cell[0]).floor().max(0.0) as usize).min(COVER_GRID - 1),
                    ((p[1] / cell[1]).floor().max(0.0) as usize).min(COVER_GRID - 1),
                );
                *share.entry(row * COVER_GRID + column).or_insert(0.0) += weight;
            }
        }
        for (&index, &covered) in &share {
            through[index] *= 1.0 - fill * covered.min(1.0);
        }
    }
    through.iter().map(|t| 1.0 - t).sum::<f64>() * cell_area
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
fn cover_directions() -> Vec<Vec3> {
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
    for organ in &graph.organs {
        let index = usize::from(organ.organ);
        if solid.get(index).copied().unwrap_or(false) {
            continue;
        }
        let (Some(look), Ok(template)) = (looks.get(index), u8::try_from(index)) else {
            continue;
        };
        let (_, heading, left) = place(organ, look);
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
            fill: crate::templates::fill_share(&look.shape),
            low: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            high: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            ..Cluster::default()
        });
        let area = look.shape.aspect() * organ.size * organ.size;
        // The organ's card as the levels that keep it draw it: bent by its
        // look (`crate::bend`: folded, cupped, drooping), traced at its
        // base, middle and tip, so a cluster reaches as far as its leaves
        // do and faces as they face. Flat looks bend nowhere.
        let half = 0.5 * look.shape.aspect() * organ.size;
        let at = |x: f64, v: f64| {
            look.bend
                .point(organ.position, heading, left, half, organ.size, x, v)
        };
        let ring = [0.0, 0.5, 1.0].map(|v| [at(-1.0, v).0, at(0.0, v).0, at(1.0, v).0]);
        let centre = ring[1][1];
        entry.centres.push(centre);
        for k in 0..2 {
            entry
                .quads
                .push([ring[k][0], ring[k][2], ring[k + 1][2], ring[k + 1][0]]);
        }
        if let Some(cross) = look.shape.cross() {
            let across = heading.cross(left) * (half * cross.min(1.0));
            entry.quads.push([
                organ.position - across,
                organ.position + across,
                organ.position + heading * organ.size + across,
                organ.position + heading * organ.size - across,
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
        if let Some(cross) = look.shape.cross() {
            // The card across faces along the organ's left (see
            // `organ_cards`).
            entry.faces.push((left, area * cross));
        }
        entry.light += organ.light;
        entry.count += 1.0;
        // A cluster shows from its first organ's birth until its last
        // organ is shed.
        entry.born = entry.born.min(organ.born);
        entry.shed = entry.shed.max(organ.shed.unwrap_or(f64::INFINITY));
    }
    let mut cards = Vec::new();
    for ((template, ..), cluster) in clusters {
        let look = &looks[usize::from(template)];
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
        let aspect = look.shape.aspect().max(MIN_CLUSTER_ASPECT);
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
            ridges: None,
            flare: None,
            moss: None,
            bottle: None,
            roots: None,
            bark_pattern: None,
            bodies: BTreeMap::new(),
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
                })
                .collect(),
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
        let fill = crate::templates::fill_share(&looks()[0].shape);
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
        };
        build_plain(&graph, &looks, &appearance, &lod(0.0, 0.0, 0.0)).cards
    }

    fn look(shape: Shape, face_up: f64) -> OrganLook {
        OrganLook {
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
        }
    }

    fn vector(v: [f32; 3]) -> Vec3 {
        Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
    }

    #[test]
    fn leaves_are_drawn_by_their_family() {
        // Sun and shade leaves by their light, juvenile leaves while the
        // plant was young (plant roadmap P4).
        let mut appearance = appearance();
        appearance.organs.insert(
            "leaf".into(),
            OrganLook {
                families: Some(crate::looks::Families {
                    plasticity: 0.6,
                    juvenile: Some(crate::looks::Juvenile {
                        until: 4.0,
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
                leaf(0.9, 8.0),
                leaf(0.5, 8.0),
                leaf(0.1, 8.0),
                leaf(0.9, 2.0),
            ],
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
        let (sun, own, shade) = (
            by_template(forms.sun.unwrap()),
            by_template(0),
            by_template(forms.shade.unwrap()),
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
