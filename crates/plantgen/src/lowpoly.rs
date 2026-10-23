//! Solid low-poly plants for weak GPUs (plant quality tiers, Project
//! After's plan of 2026-10-23): every organ drawn as a few opaque coloured
//! triangles in the shape of its card's outline, no texture and no alpha
//! test, within a triangle budget spent in the order that keeps a species
//! recognisable.
//!
//! A tier spends its budget on the plant's outline first, its wood at a
//! coarse level, then on its signature organs (flowers, heads, fruit,
//! clusters and spikes), and what is left on its foliage. Organs come from
//! the finest level whose cards fit the budget after keeping at least
//! [`SolidTier::floor`] of them; the share kept is drawn evenly over the
//! plant by a hash of each card's place, and each kept organ grows by the
//! square root of one over the share, so the plant keeps its cover
//! (stochastic pruning, as levels 2 and 3 do in a renderer).
//!
//! Each organ is a polygon traced round its card's template: the widest
//! covered span at a few stations along the card, its first and last
//! covered points as base and tip, bent as its card bends. A foxglove's
//! broad toothed leaf and a fireweed's narrow willow leaf stay broad and
//! narrow with six triangles each, and a grass blade keeps its arch.

use crate::body::BodyLook;
use crate::graph::PlantGraph;
use crate::looks::{Look, Shape};
use crate::math::Vec3;
use crate::mesh::{self, Card, LodSpec, Mesh, PlantMesh, Vertex};
use crate::rng::{mix64, unit};
use crate::spec::Appearance;
use crate::templates::Templates;

/// One solid tier: how its wood is built, how much it may draw, and how
/// finely it traces each organ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidTier {
    pub name: &'static str,
    /// The wood's level, for a 15 m plant ([`LodSpec::for_height`]).
    pub wood: LodSpec,
    /// Triangles a plant at most [`SMALL_M`] tall may draw, and one at
    /// least [`LARGE_M`]; between, in proportion to its height.
    pub small: usize,
    pub large: usize,
    /// The most of the budget a plant's wood takes (not a cactus's bodies),
    /// its axes kept from the most visible on.
    pub wood_share: f64,
    /// The most rings an axis keeps, and how far (a share of the plant's
    /// height) its tube may stray from its path where it drops one.
    pub rings: usize,
    pub straying: f64,
    /// Stations along a signature organ's outline, and along a leaf's:
    /// 1 draws a diamond (2 triangles), 2 a hexagon (4), 3 an octagon (6).
    pub signature_stations: usize,
    pub foliage_stations: usize,
    /// The most of the organs' budget signature organs take while the
    /// plant has foliage; foliage takes the rest, and either takes what
    /// the other leaves.
    pub signature_share: f64,
    /// The least share of a level's leaves a plant keeps before it takes
    /// the next coarser level's.
    pub floor: f64,
    /// The most a kept organ grows to stand for those dropped: more keeps
    /// the plant's cover, less keeps its leaves' size.
    pub most_growth: f64,
}

/// The most of a fleshy plant's budget its bodies and wood take: they
/// are its outline.
const BODY_SHARE: f64 = 0.7;

/// How much larger a share of its organs a coarser level must keep to be
/// taken when no level keeps a tier's floor.
const COARSER_GAIN: f64 = 2.5;

/// Plants this tall or lower take a tier's `small` budget.
pub const SMALL_M: f64 = 2.0;
/// Plants this tall or taller take its `large` budget.
pub const LARGE_M: f64 = 15.0;

/// About a Steam Deck or an integrated GPU at 60 frames a second: a few
/// hundred triangles a herb, 1,500 a tree.
pub const LOW: SolidTier = SolidTier {
    name: "low",
    wood: LodSpec {
        min_radius: 0.02,
        ring_edge: 0.4,
        min_sides: 3,
        max_sides: 4,
        bend: 20.0,
        cluster: 0.0,
    },
    small: 400,
    large: 1500,
    wood_share: 0.3,
    rings: 5,
    straying: 0.02,
    signature_stations: 3,
    foliage_stations: 2,
    signature_share: 0.4,
    floor: 0.15,
    most_growth: 1.6,
};

/// The weakest machines: an essence of the plant, 30 to 120 triangles a
/// herb and 300 a tree.
pub const MINIMAL: SolidTier = SolidTier {
    name: "minimal",
    wood: LodSpec {
        min_radius: 0.06,
        ring_edge: 1.0,
        min_sides: 3,
        max_sides: 3,
        bend: 35.0,
        cluster: 0.0,
    },
    small: 120,
    large: 300,
    wood_share: 0.4,
    rings: 3,
    straying: 0.05,
    signature_stations: 1,
    foliage_stations: 1,
    signature_share: 0.4,
    floor: 0.1,
    most_growth: 2.2,
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

    /// The most points along a chain of cards drawn as one ribbon.
    #[must_use]
    pub fn ribbon_points(&self, signature: bool) -> usize {
        2 + if signature {
            self.signature_stations
        } else {
            self.foliage_stations
        }
    }

    /// Triangles one organ of `cards` cards takes: a polygon round one
    /// card, or a ribbon along a chain, narrowing to a point.
    #[must_use]
    pub fn chain_triangles(&self, cards: usize, signature: bool) -> usize {
        if cards <= 1 {
            self.organ_triangles(signature)
        } else {
            2 * (cards + 1).min(self.ribbon_points(signature)) - 3
        }
    }

    /// Triangles one organ's polygon takes.
    #[must_use]
    pub fn organ_triangles(&self, signature: bool) -> usize {
        2 * if signature {
            self.signature_stations
        } else {
            self.foliage_stations
        }
    }
}

/// What a solid build drew.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SolidStats {
    pub budget: usize,
    /// One side's triangles: wood, and the organs' polygons (a renderer
    /// draws them two-sided; [`SolidPlant::two_sided`] doubles them).
    pub wood_triangles: usize,
    pub organ_triangles: usize,
    /// The level whose cards the organs came from.
    pub level: usize,
    /// Signature organs and foliage: kept of each.
    pub signature: (usize, usize),
    pub foliage: (usize, usize),
}

impl SolidStats {
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.wood_triangles + self.organ_triangles
    }
}

/// A plant at a solid tier.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SolidPlant {
    pub wood: Mesh,
    /// One-sided organ polygons, their fronts facing as their cards did.
    pub organs: Mesh,
    pub stats: SolidStats,
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

/// Build `graph` at `tier`. `levels` are the package's four mesh levels
/// for a 15 m plant (a quality profile's `lods`); the organs come from
/// the finest of them that fits.
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
    // A plant with fleshy bodies (a cactus) keeps the coarse level's
    // bodies, which are its outline; any other draws its axes as thin
    // three-sided tubes, the most visible first, within its share.
    let fleshy = graph.segments.iter().any(|segment| segment.body != 0);
    let room = (budget as f64 * if fleshy { BODY_SHARE } else { tier.wood_share }) as usize;
    let mut wood = Mesh::default();
    if fleshy {
        let mut drawn = PlantMesh::default();
        crate::body::build_within(graph, bodies, looks.len(), room, &mut drawn);
        append(&mut wood, &drawn.wood, false);
    }
    let woody = axes(
        graph,
        appearance,
        tier,
        room.saturating_sub(wood.triangle_count()),
    );
    append(&mut wood, &woody, false);
    // Organs drawn solid near (an agave's thick leaves, a lily's petals,
    // lace) drawn as their cards: their templates still describe them.
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
    let left = budget.saturating_sub(wood.triangle_count());
    let is_signature = |card: &Card| looks.get(usize::from(card.template)).is_some_and(signature);
    // Every coarse level's organs, each split into signature organs and
    // foliage. An organ is a chain of cards: a grass blade is drawn in
    // pieces, which are pruned and drawn together.
    let built: Vec<(usize, Vec<Vec<Card>>, Vec<Vec<Card>>)> = levels
        .iter()
        .enumerate()
        .skip(1)
        .map(|(level, lod)| {
            let plant = mesh::build(
                graph,
                &carded,
                bodies,
                appearance,
                &lod.for_height(height),
                level,
            );
            let (signatures, foliage): (Vec<Card>, Vec<Card>) =
                plant.cards.into_iter().partition(|card| is_signature(card));
            (level, chains(signatures), chains(foliage))
        })
        .collect();
    let has_foliage = built.iter().any(|(_, _, foliage)| !foliage.is_empty());
    let has_signature = built
        .iter()
        .any(|(_, signatures, _)| !signatures.is_empty());
    // The organs' budget split between them: signature organs at most
    // their share while the plant has leaves.
    let room_s = if !has_foliage {
        left
    } else if has_signature {
        (left as f64 * tier.signature_share) as usize
    } else {
        0
    };
    let cost = |chain: &[Card], signature: bool| tier.chain_triangles(chain.len(), signature);
    // How many of `organs` fit `room`, kept in their hash order.
    let fitting = |organs: &[Vec<Card>], room: usize, signature: bool| {
        let mut used = 0;
        let mut kept = 0;
        for chain in organs {
            used += cost(chain, signature);
            if used > room {
                break;
            }
            kept += 1;
        }
        kept
    };
    // Each class from the finest level that keeps at least the floor of
    // it within its room (the coarsest when none does): a fireweed's
    // leaves one by one, and its racemes as the spikes their clusters
    // draw.
    let pick = |room: usize, signature: bool| {
        let share = |organs: &[Vec<Card>]| {
            if organs.is_empty() {
                1.0
            } else {
                fitting(organs, room, signature) as f64 / organs.len() as f64
            }
        };
        let of = |entry: &(usize, Vec<Vec<Card>>, Vec<Vec<Card>>)| {
            if signature {
                share(&entry.1)
            } else {
                share(&entry.2)
            }
        };
        // When none keeps its floor, the finest unless a coarser one
        // keeps a far larger share: merged leaves are larger than leaves,
        // and only worth it when they are far fewer.
        let mut best = 0;
        for (at, entry) in built.iter().enumerate() {
            if of(entry) >= tier.floor {
                return at;
            }
            if of(entry) >= of(&built[best]) * COARSER_GAIN {
                best = at;
            }
        }
        best
    };
    let at_s = pick(room_s, true);
    let signatures: &[Vec<Card>] = built.get(at_s).map_or(&[], |entry| &entry.1);
    let keep_signature = fitting(signatures, room_s, true);
    let used_s: usize = signatures
        .iter()
        .take(keep_signature)
        .map(|c| cost(c, true))
        .sum();
    let room_f = left.saturating_sub(used_s);
    let at_f = pick(room_f, false);
    let foliage: &[Vec<Card>] = built.get(at_f).map_or(&[], |entry| &entry.2);
    let keep_foliage = fitting(foliage, room_f, false);
    let used_f: usize = foliage
        .iter()
        .take(keep_foliage)
        .map(|c| cost(c, false))
        .sum();
    // Signature organs take what foliage leaves.
    let keep_signature = fitting(signatures, left.saturating_sub(used_f), true);
    let level = built.get(at_f.max(at_s)).map_or(0, |entry| entry.0);
    let (signature_count, foliage_count) = (signatures.len(), foliage.len());
    let signature_outlines = Outlines::new(templates, tier.signature_stations);
    let foliage_outlines = Outlines::new(templates, tier.foliage_stations);
    let mut organs = Mesh::default();
    // Leaves too many to keep their floor one by one: those crowded on
    // one axis (a fern's pinnae, a shoot's needles) drawn as the frond or
    // spray they make, the rest one by one with what is left.
    let mut keep_foliage = keep_foliage;
    let mut foliage_chains: Vec<Vec<Card>> = foliage.to_vec();
    let mut combed = 0;
    let mut clumped = 0;
    let mut room_c = room_f;
    // Only a herb's: a shrub's or a tree's leafy shoots read as the
    // clumps of its crown below.
    if !foliage.is_empty() && height <= SMALL_M {
        let cards: Vec<Card> = foliage.iter().flatten().copied().collect();
        let (combs, rest) = combs(
            graph,
            &cards,
            &foliage_outlines,
            tier.ribbon_points(false) + 1,
        );
        if !combs.is_empty() {
            let mut used = 0;
            for comb in &combs {
                if used + comb.triangles() > room_f {
                    continue;
                }
                used += comb.triangles();
                comb.draw(&mut organs);
                combed += comb.organs;
            }
            foliage_chains = chains(rest);
            keep_foliage = fitting(&foliage_chains, room_f.saturating_sub(used), false);
            room_c = room_f.saturating_sub(used);
        }
    }
    // Leaves still far too many to keep one by one, as a tree's crown's:
    // merged by place into the clumps they make, each drawn as one of
    // them grown to their cover.
    let singles = foliage_chains
        .iter()
        .filter(|chain| chain.len() == 1)
        .count();
    if !foliage_chains.is_empty()
        && (keep_foliage as f64) < tier.floor * foliage_chains.len() as f64
        && singles * 10 >= foliage_chains.len() * 9
    {
        let cards: Vec<Card> = foliage_chains.iter().flatten().copied().collect();
        let merged = clumps(&cards, room_c / tier.organ_triangles(false).max(1));
        clumped = cards.len().min(foliage_count);
        foliage_chains = merged.into_iter().map(|card| vec![card]).collect();
        keep_foliage = foliage_chains.len();
    }
    let foliage: &[Vec<Card>] = &foliage_chains;
    for (chains, keep, outlines, signature) in [
        (signatures, keep_signature, &signature_outlines, true),
        (foliage, keep_foliage, &foliage_outlines, false),
    ] {
        if keep == 0 || chains.is_empty() {
            continue;
        }
        let share = keep as f64 / chains.len() as f64;
        for chain in chains.iter().take(keep) {
            if chain.len() == 1 {
                outlines.polygon(&chain[0], share, tier.most_growth, &mut organs);
            } else {
                outlines.ribbon(chain, share, tier.ribbon_points(signature), &mut organs);
            }
        }
    }
    let stats = SolidStats {
        budget,
        wood_triangles: wood.triangle_count(),
        organ_triangles: organs.triangle_count(),
        level,
        signature: (keep_signature, signature_count),
        foliage: (
            if clumped > 0 { clumped } else { keep_foliage } + combed,
            foliage_count,
        ),
    };
    SolidPlant {
        wood,
        organs,
        stats,
    }
}

/// The plant's axes as three-sided tubes within `budget` triangles: each
/// axis (a stem or a branch and its continuations) as a path through at
/// most [`SolidTier::rings`] of its points, those that keep it within
/// [`SolidTier::straying`] of the plant's height of its path, taken from
/// the most visible (longest and thickest) on while they fit.
fn axes(graph: &PlantGraph, appearance: &Appearance, tier: &SolidTier, budget: usize) -> Mesh {
    let mut paths: Vec<(f64, Vec<(Vec3, f64)>, u16, f64)> = axis_paths(graph)
        .into_iter()
        .map(|axis| {
            let length: f64 = axis
                .points
                .windows(2)
                .map(|w| (w[1].0 - w[0].0).length())
                .sum();
            let mean = axis.points.iter().map(|p| p.1).sum::<f64>() / axis.points.len() as f64;
            (length * mean, axis.points, axis.order, axis.born)
        })
        .collect();
    paths.sort_by(|a, b| b.0.total_cmp(&a.0));
    let tolerance = graph.height * tier.straying;
    let colour = [
        appearance.bark[0],
        appearance.bark[1],
        appearance.bark[2],
        1.0,
    ];
    let mut out = Mesh::default();
    for (_, points, order, born) in paths {
        let kept = simplify(&points, tolerance, tier.rings.max(2));
        let cost = (kept.len() - 1) * 6;
        if out.triangle_count() + cost > budget {
            continue;
        }
        tube(&kept, colour, order.min(2) as u8, born, &mut out);
    }
    out
}

/// One axis of a plant: a stem or a branch and its continuations.
struct Axis {
    /// Its segments, base first.
    segments: Vec<usize>,
    /// Its path: the first segment's start, then each segment's end, with
    /// the radius there.
    points: Vec<(Vec3, f64)>,
    order: u16,
    born: f64,
}

/// The plant's axes that stand at its age, other than fleshy bodies.
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
        if !starts || !alive(first) || segment.body != 0 {
            continue;
        }
        let mut points = vec![(segment.start, segment.radius)];
        let mut members = Vec::new();
        let mut at = Some(first);
        while let Some(index) = at.filter(|&index| alive(index)) {
            let end = &segments[index];
            let radius = next[index].map_or(end.radius * 0.5, |n| segments[n].radius);
            points.push((end.end, radius));
            members.push(index);
            at = next[index];
            if members.len() > segments.len() {
                break;
            }
        }
        axes.push(Axis {
            segments: members,
            points,
            order: segment.order,
            born: segment.born,
        });
    }
    axes
}

/// Organs on one axis drawn as one ribbon along it: its leaves or pinnae
/// as the frond or leafy shoot they make.
struct Comb {
    /// The axis's path where the organs stand, at most a few points, each
    /// with the half-width the organs reach to either side of it there.
    points: Vec<(Vec3, f64)>,
    /// The side the organs reach toward, across the axis.
    side: Vec3,
    colour: [f32; 4],
    born: f64,
    organs: usize,
}

/// At least this many organs on one axis make a comb, if none reaches
/// further from it than this share of its length.
const COMB_ORGANS: usize = 10;
const COMB_REACH: f64 = 0.12;

/// The combs of `cards` with at least [`COMB_ORGANS`] on one axis of
/// `graph`, at most `most` points each, most organs first; and the cards
/// on no comb.
fn combs(
    graph: &PlantGraph,
    cards: &[Card],
    outlines: &Outlines,
    most: usize,
) -> (Vec<Comb>, Vec<Card>) {
    let axes = axis_paths(graph);
    // Each segment's axis and place along it.
    let mut on = vec![None; graph.segments.len()];
    for (a, axis) in axes.iter().enumerate() {
        for (k, &segment) in axis.segments.iter().enumerate() {
            on[segment] = Some((a, k));
        }
    }
    let mut by_axis: std::collections::BTreeMap<usize, Vec<(usize, &Card)>> =
        std::collections::BTreeMap::new();
    let mut rest = Vec::new();
    for card in cards {
        match on.get(card.segment as usize).copied().flatten() {
            Some((a, k)) => by_axis.entry(a).or_default().push((k, card)),
            None => rest.push(*card),
        }
    }
    let v = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
    let mut out = Vec::new();
    for (a, members) in by_axis {
        if members.len() < COMB_ORGANS {
            rest.extend(members.iter().map(|(_, card)| **card));
            continue;
        }
        let axis = &axes[a];
        let (low, high) = members
            .iter()
            .fold((usize::MAX, 0), |(l, h), (k, _)| (l.min(*k), h.max(*k)));
        // The axis from the first organ's segment to past the last's.
        let path: Vec<Vec3> = axis.points[low..=(high + 1).min(axis.points.len() - 1)]
            .iter()
            .map(|p| p.0)
            .collect();
        let tangent = (*path.last().unwrap_or(&Vec3::ZERO) - path[0]).normalize_or(Vec3::Y);
        // Where each organ reaches across the axis: its tip's offset from
        // the axis, off the tangent.
        let reach = |card: &Card, k: usize| {
            let tip = card
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
            let from = axis.points[k].0;
            let offset = tip - from;
            offset - tangent * offset.dot(tangent)
        };
        let reference = reach(members[0].1, members[0].0);
        let mut side = Vec3::ZERO;
        for (k, card) in &members {
            let r = reach(card, *k);
            side += if r.dot(reference) >= 0.0 { r } else { -r };
        }
        let side = side.normalize_or_else(|| crate::math::any_perpendicular(tangent));
        // Only organs short beside their axis make a comb: a fern's
        // pinnae, not a fireweed's leaves or a grass's blades.
        let length: f64 = path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        let widest = members
            .iter()
            .map(|(k, card)| reach(card, *k).length())
            .fold(0.0, f64::max);
        if widest > COMB_REACH * length {
            rest.extend(members.iter().map(|(_, card)| **card));
            continue;
        }
        // The half-width at each kept point: the furthest the organs whose
        // segments lie nearest it reach along the side.
        let kept = simplify(
            &path.iter().map(|&p| (p, 0.0)).collect::<Vec<_>>(),
            0.0,
            most.max(3),
        );
        let place_of = |k: usize| axis.points[k].0;
        let mut points = Vec::new();
        for (i, &(p, _)) in kept.iter().enumerate() {
            let mut half: f64 = 0.0;
            for (k, card) in &members {
                let nearest = kept
                    .iter()
                    .enumerate()
                    .min_by(|x, y| {
                        (x.1.0 - place_of(*k))
                            .length()
                            .total_cmp(&(y.1.0 - place_of(*k)).length())
                    })
                    .map_or(0, |(j, _)| j);
                if nearest == i {
                    half = half.max(reach(card, *k).dot(side).abs());
                }
            }
            points.push((p, half));
        }
        // The tip narrows to a point; empty points between take their
        // neighbours' width.
        if let Some(last) = points.last_mut() {
            last.1 = 0.0;
        }
        for i in 1..points.len().saturating_sub(1) {
            if points[i].1 == 0.0 {
                points[i].1 = 0.5 * (points[i - 1].1 + points[i + 1].1);
            }
        }
        let mut colour = [0.0_f64; 3];
        for (_, card) in &members {
            let tint = outlines
                .shapes
                .get(usize::from(card.template))
                .and_then(Option::as_ref)
                .map_or([1.0; 3], |o| o.tint);
            for c in 0..3 {
                colour[c] += f64::from(card.color[c]) * tint[c];
            }
        }
        let n = members.len() as f64;
        out.push(Comb {
            points,
            side,
            colour: [
                (colour[0] / n) as f32,
                (colour[1] / n) as f32,
                (colour[2] / n) as f32,
                1.0,
            ],
            born: members
                .iter()
                .map(|(_, c)| f64::from(c.born))
                .fold(f64::INFINITY, f64::min),
            organs: members.len(),
        });
    }
    out.sort_by(|a, b| b.organs.cmp(&a.organs));
    (out, rest)
}

impl Comb {
    fn triangles(&self) -> usize {
        2 * self.points.len().saturating_sub(1) - 1
    }

    fn draw(&self, out: &mut Mesh) {
        let base = u32::try_from(out.positions.len()).unwrap_or(0);
        let n = self.points.len();
        for (k, &(at, half)) in self.points.iter().enumerate() {
            let along = if k + 1 < n {
                self.points[k + 1].0 - at
            } else {
                at - self.points[k - 1].0
            };
            let normal = along.cross(self.side).normalize_or(Vec3::Y);
            for sign in [-1.0, 1.0] {
                out.push(Vertex {
                    position: at + self.side * (half * sign),
                    normal,
                    uv: [0.0, 0.0],
                    color: self.colour,
                    born: self.born,
                    shed: None,
                    level: 3,
                    segment: mesh::NO_SEGMENT,
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
                out.indices.extend_from_slice(&[a, b, c]);
            } else {
                out.indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
    }
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
            let along = (b - a).normalize_or(Vec3::Y);
            for index in pair[0] + 1..pair[1] {
                let offset = points[index].0 - a;
                let off = (offset - along * offset.dot(along)).length();
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

/// A three-sided tube along `points` (place and radius), its rings turned
/// by parallel transport.
fn tube(points: &[(Vec3, f64)], colour: [f32; 4], level: u8, born: f64, out: &mut Mesh) {
    const SIDES: u32 = 3;
    let first = u32::try_from(out.positions.len()).unwrap_or(0);
    let mut normal = None;
    for (k, &(place, radius)) in points.iter().enumerate() {
        let before = if k > 0 {
            place - points[k - 1].0
        } else {
            Vec3::ZERO
        };
        let after = if k + 1 < points.len() {
            points[k + 1].0 - place
        } else {
            Vec3::ZERO
        };
        let tangent = (before.normalize_or(Vec3::ZERO) + after.normalize_or(Vec3::ZERO))
            .normalize_or(Vec3::Y);
        let n: Vec3 = normal.unwrap_or_else(|| crate::math::any_perpendicular(tangent));
        let n = (n - tangent * n.dot(tangent))
            .normalize_or_else(|| crate::math::any_perpendicular(tangent));
        normal = Some(n);
        let b = tangent.cross(n);
        // Out far enough that the triangle's mean width is the stem's.
        let reach = radius * std::f64::consts::PI / (3.0 * (std::f64::consts::PI / 3.0).sin());
        for side in 0..SIDES {
            let angle = std::f64::consts::TAU * f64::from(side) / f64::from(SIDES);
            let out_dir = n * angle.cos() + b * angle.sin();
            out.push(Vertex {
                position: place + out_dir * reach,
                normal: out_dir,
                uv: [0.0, 0.0],
                color: colour,
                born,
                shed: None,
                level,
                segment: mesh::NO_SEGMENT,
            });
        }
    }
    for ring in 0..u32::try_from(points.len().saturating_sub(1)).unwrap_or(0) {
        for side in 0..SIDES {
            let a = first + ring * SIDES + side;
            let b = first + ring * SIDES + (side + 1) % SIDES;
            let (c, d) = (a + SIDES, b + SIDES);
            out.indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
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
    let place = |card: &Card| {
        Vec3::new(
            f64::from(card.base[0]),
            f64::from(card.base[1]),
            f64::from(card.base[2]),
        )
    };
    let mut groups: std::collections::BTreeMap<(u32, u8), Vec<usize>> =
        std::collections::BTreeMap::new();
    let mut out: Vec<Vec<Card>> = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        if card.segment == mesh::NO_SEGMENT {
            out.push(vec![*card]);
        } else {
            groups
                .entry((card.segment, card.template))
                .or_default()
                .push(index);
        }
    }
    for members in groups.values() {
        // Each card's follower: the one whose base is nearest its tip,
        // within a fifth of its length.
        let mut follower = vec![None; members.len()];
        let mut followed = vec![false; members.len()];
        for (i, &a) in members.iter().enumerate() {
            let end = tip(&cards[a]);
            let reach = f64::from(cards[a].length) * 0.2;
            let mut best = (reach, None);
            for (j, &b) in members.iter().enumerate() {
                if i == j || followed[j] {
                    continue;
                }
                let gap = (place(&cards[b]) - end).length();
                if gap < best.0 {
                    best = (gap, Some(j));
                }
            }
            if let Some(j) = best.1 {
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

/// Of the cover a clump's leaves give, the share its one card covers:
/// leaves in a clump overlap. And the most it grows each way.
const CLUMP_FILL: f64 = 0.5;
const CLUMP_GROWTH: f64 = 4.0;
/// Radians a clump's tip turns from straight up at most.
const CLUMP_TURN: f64 = 0.7;

/// `cards` merged by place into at most `count` clumps, in an order
/// hashed from their places: each the largest of its cards, centred on
/// theirs, heading as they head on the whole, grown to cover
/// [`CLUMP_FILL`] of their area.
fn clumps(cards: &[Card], count: usize) -> Vec<Card> {
    if cards.is_empty() || count == 0 {
        return Vec::new();
    }
    let v = |x: [f32; 3]| Vec3::new(f64::from(x[0]), f64::from(x[1]), f64::from(x[2]));
    let f = |x: Vec3| [x.x as f32, x.y as f32, x.z as f32];
    let middle = |card: &Card| v(card.base) + v(card.heading) * (f64::from(card.length) * 0.5);
    let area = |card: &Card| f64::from(card.length) * f64::from(card.width);
    let (low, high) = cards.iter().fold(
        (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(l, h), card| {
            let m = middle(card);
            (l.min(m), h.max(m))
        },
    );
    let axis = cards.iter().map(middle).fold(Vec3::ZERO, |a, b| a + b) / cards.len() as f64;
    let extent = (high - low).max(Vec3::new(1.0e-3, 1.0e-3, 1.0e-3));
    let mut cell = crate::math::pow(extent.x * extent.y * extent.z / count as f64, 1.0 / 3.0);
    let mut groups: std::collections::BTreeMap<(i64, i64, i64), Vec<usize>> =
        std::collections::BTreeMap::new();
    for _ in 0..40 {
        groups.clear();
        for (index, card) in cards.iter().enumerate() {
            let at = (middle(card) - low) / cell;
            groups
                .entry((at.x as i64, at.y as i64, at.z as i64))
                .or_default()
                .push(index);
        }
        if groups.len() <= count {
            break;
        }
        cell *= 1.15;
    }
    let mut out: Vec<Card> = groups
        .values()
        .map(|members| {
            let largest = members
                .iter()
                .copied()
                .max_by(|&a, &b| area(&cards[a]).total_cmp(&area(&cards[b])))
                .unwrap_or(members[0]);
            let mut card = cards[largest];
            let n = members.len() as f64;
            let centre = members
                .iter()
                .map(|&i| middle(&cards[i]))
                .fold(Vec3::ZERO, |a, b| a + b)
                / n;
            // Facing out of the crown and a little up, as a crown's leaf
            // masses show from any side, its tip up but turned by up to
            // [`CLUMP_TURN`] either way.
            let outward = Vec3::new(centre.x - axis.x, 0.0, centre.z - axis.z);
            let outward = (outward.normalize_or(Vec3::ZERO) + Vec3::Y * 0.5).normalize_or(Vec3::Y);
            let up = (Vec3::Y - outward * outward.dot(Vec3::Y))
                .normalize_or_else(|| crate::math::any_perpendicular(outward));
            let across = outward.cross(up);
            let turn = (hash(&card) * 2.0 - 1.0) * CLUMP_TURN;
            let heading = up * turn.cos() + across * turn.sin();
            let left = outward.cross(heading);
            let cover: f64 = members.iter().map(|&i| area(&cards[i])).sum();
            let grow = (cover * CLUMP_FILL / area(&card).max(1.0e-9))
                .sqrt()
                .clamp(1.0, CLUMP_GROWTH);
            card.length = (f64::from(card.length) * grow) as f32;
            card.width = (f64::from(card.width) * grow) as f32;
            card.heading = f(heading);
            card.left = f(left);
            card.base = f(centre - heading * (f64::from(card.length) * 0.5));
            let mut colour = [0.0_f64; 3];
            for &i in members {
                for c in 0..3 {
                    colour[c] += f64::from(cards[i].color[c]) / n;
                }
            }
            card.color = [colour[0] as f32, colour[1] as f32, colour[2] as f32];
            card.born = members
                .iter()
                .map(|&i| cards[i].born)
                .fold(f32::INFINITY, f32::min);
            card
        })
        .collect();
    out.sort_by(|a, b| hash(a).total_cmp(&hash(b)));
    out
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
