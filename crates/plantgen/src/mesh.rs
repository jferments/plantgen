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
use crate::looks::{Flare, Look, Moss, Mount, Ridges};
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
    /// index of the organ's type in its program.
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
    let mut mesh = PlantMesh::default();
    wood(graph, appearance, lod, &mut mesh.wood);
    // Organs drawn as solid leaves, flowers or fruit at this level leave
    // the cards.
    let mut solid = leaves::solid_types(looks, level);
    let blooms: Vec<bool> = looks
        .iter()
        .enumerate()
        .map(|(index, look)| {
            look.solid.is_none() && look.form.solid_at(level) && blooms::fits(graph, look, index)
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
    body::build(graph, bodies, looks.len(), level, &mut mesh);
    mesh
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

        let base_radius =
            kept[0].radius.max(1e-4) * (1.0 + flare.map_or(0.0, |flare| flare.amount));
        let around = 2.0 * math::PI * base_radius / lod.ring_edge.max(1e-4);
        // Clamped to a small whole number of sides.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sides = (around.ceil() as u32).clamp(lod.min_sides.max(3), lod.max_sides.max(3));
        let bark = Bark::new(appearance, flare, kept[0].radius, sides, segment.id);
        let level = match segment.order {
            0 => 0,
            1 => 1,
            _ => 2,
        };

        let frames = axis_frames(&kept);
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
                    position: current.centre + direction * radius,
                    normal: surface,
                    uv,
                    color: bark.colour(
                        current.radius,
                        surface,
                        current.centre.y,
                        along,
                        angle,
                        furrow,
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
fn place(organ: &GraphOrgan, look: &Look) -> (Vec3, Vec3, Vec3) {
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
        let (Some(look), Ok(template)) = (looks.get(index), u8::try_from(index)) else {
            continue;
        };
        let (base, heading, left) = place(organ, look);
        let card = Placed {
            base,
            heading,
            left,
            length: organ.size,
            width: organ.size * look.shape.aspect(),
            color: organ_color(look, variation, organ.light, organ.id),
            template,
            born: organ.born,
            shed: organ.shed,
            bend: look.bend,
        };
        if let Some(cross) = look.shape.cross() {
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
}

impl Cluster {
    /// The area its organs' cards cover seen along the unit vector `axis`.
    fn cover(&self, axis: Vec3) -> f64 {
        self.faces
            .iter()
            .map(|&(normal, area)| normal.dot(axis).abs() * area)
            .sum()
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
fn card_areas(normals: &[Vec3], cluster: &Cluster) -> Vec<f64> {
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
    let cover: Vec<f64> = directions.iter().map(|&d| cluster.cover(d)).collect();
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
            ..Cluster::default()
        });
        let area = look.shape.aspect() * organ.size * organ.size;
        let normal = heading.cross(left);
        entry.position += organ.position;
        entry.normal += normal * area;
        entry.heading += heading * area;
        entry.faces.push((normal, area));
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
        for ((_, heading, left), area) in facings.into_iter().zip(card_areas(&normals, &cluster)) {
            if area <= 0.0 {
                continue;
            }
            let (length, width) = (math::sqrt(area / aspect), math::sqrt(area * aspect));
            cards.push(
                Placed {
                    base: position - heading * (length * 0.5),
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
        // The straight stem needs only its end rings; the thin branch is gone.
        assert_eq!(coarse.wood.vertex_count(), 2 * 8);
        // The stem stays even below a minimum radius thicker than itself.
        let coarsest = build_plain(&graph(), &looks(), &appearance(), &lod(0.1, 0.0, 10.0));
        assert_eq!(coarsest.wood.vertex_count(), 2 * 8);
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
        // the thin branch is gone. Scaled, the 1 cm ring edge gives the
        // stem the 8-side maximum and the branch is back with 3 sides.
        assert_eq!(tree_scale.wood.vertex_count(), 2 * 8);
        assert_eq!(plant_scale.wood.vertex_count(), 2 * 9 + 2 * 4);
        assert!(plant_scale.cards.len() > tree_scale.cards.len());
    }

    #[test]
    fn clustering_merges_organs_and_keeps_birth_ages() {
        let clustered = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        // Two cells of ten level leaves: each a level card, and no upright
        // card, since level leaves cover nothing seen from the side.
        assert_eq!(clustered.cards.len(), 2);
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
        let tilted = tilted_leaves();
        let detailed = build_plain(&tilted, &looks(), &appearance(), &lod(0.0, 0.0, 0.0));
        let clustered = build_plain(&tilted, &looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        assert_eq!(clustered.cards.len(), 3);
        let mut ratios = Vec::new();
        for elevation in [-45.0_f64, -20.0, 0.0, 20.0, 45.0] {
            for k in 0..8 {
                let (e, azimuth) = (elevation.to_radians(), f64::from(k) * 45.0_f64.to_radians());
                let axis = Vec3::new(
                    math::cos(e) * math::cos(azimuth),
                    math::sin(e),
                    math::cos(e) * math::sin(azimuth),
                );
                let ratio = cover(&clustered, axis) / cover(&detailed, axis);
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
        let level = build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 1.0, 0.0));
        assert!(cover(&level, Vec3::X) < 1e-9 && cover(&level, Vec3::Z) < 1e-9);
        assert!(
            (cover(&level, Vec3::Y)
                / cover(
                    &build_plain(&graph(), &looks(), &appearance(), &lod(0.0, 0.0, 0.0)),
                    Vec3::Y
                )
                - 1.0)
                .abs()
                < 1e-6
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
        for (position, normal) in mesh.wood.positions.iter().zip(&mesh.wood.normals).take(8) {
            // First ring of the stem: radius 0.05 around the Y axis. The
            // stem narrows by 1 cm per metre, so its surface faces up by
            // that slope.
            let radial = (position[0] * position[0] + position[2] * position[2]).sqrt();
            assert!((radial - 0.05).abs() < 1e-6);
            let outward = (normal[0] * position[0] + normal[2] * position[2]) / radial;
            assert!((normal[1] / outward - 0.01).abs() < 1e-4);
        }
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
        }
    }

    fn vector(v: [f32; 3]) -> Vec3 {
        Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
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
        let cards = one_organ(look(round, 0.0), Vec3::X, Vec3::Z);
        assert_eq!(cards.len(), 2);
        assert!((cards[0].width - cards[1].width).abs() < 1e-6);
        assert!(vector(cards[0].left).dot(vector(cards[1].left)).abs() < 1e-6);
    }
}
