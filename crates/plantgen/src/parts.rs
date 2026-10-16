//! Part meshes (plant roadmap P4): one small solid per organ type, which a
//! renderer draws at each of the type's organs near the camera in place
//! of its card.
//!
//! Small parts repeat: a maple carries thousands of samara clusters, a fir
//! thousands of cones. Drawn into the plant's own mesh they would cost
//! millions of triangles (`crate::blooms::TYPE_TRIANGLES` keeps such a
//! type as cards), so instead each organ type whose look draws a solid (a
//! flower, a fruit or cluster, a cone, a thorn) gets [`VARIANTS`] meshes of
//! one organ, in the frame of the organ's card: a point $`\mathbf p`$ of
//! the organ is stored as
//!
//! ```math
//! (x, y, z) = \big((\mathbf p - \mathbf b)\cdot\mathbf l,\ (\mathbf p - \mathbf b)\cdot\mathbf h,\ (\mathbf p - \mathbf b)\cdot(\mathbf l \times \mathbf h)\big) / \ell
//! ```
//!
//! for the card's base $`\mathbf b`$, heading $`\mathbf h`$, left $`\mathbf l`$
//! and length $`\ell`$, so a renderer puts it back with
//! $`\mathbf p = \mathbf b + \ell\,(x\,\mathbf l + y\,\mathbf h + z\,\mathbf l \times \mathbf h)`$
//! from any card of the type, whichever way the look mounts its card
//! (`crate::mesh::place`; a look that turns its cards to face up keeps
//! them). Variant `v` is the organ drawn with the id `mix64(v + 1)`, so a
//! cluster's turn and which of its fruit are unripe differ from variant to
//! variant, and where the look varies within its species' ranges
//! (`crate::looks::Look::varieties`, spec rework VARY) variant `v` is its
//! variety `v`; a renderer picks one per organ. Its colours are the look's
//! own, and the renderer shades them as the card's colour is shaded.
//!
//! A part mesh holds at most [`TRIANGLES`] triangles ([`SHOOT_TRIANGLES`]
//! for a needle shoot, `crate::shoots`, and [`CLUSTER_TRIANGLES`] for a
//! cluster, `crate::clusters`, or a flower head, `crate::capitula`, a
//! cluster of florets): a cluster, a head or a shoot that would take more
//! is drawn up to [`coarsest`] steps coarser, and a type that still does
//! not fit keeps its cards.
//!
//! Each type has a span ([`PartMesh::span`]): the share of its card's
//! length whose projection must cover the renderer's least pixels for the
//! part mesh to be drawn. An organ seen whole, as a fruit, has span 1; a
//! needle shoot's is [`SHOOT_SPAN`] times its needles' width, so its
//! needles stay a pixel or more wide where the least is four.
//!
//! **Steps** (spec rework SEMLOD). A part mesh seen small draws more
//! triangles than its pixels show, so each type may also have coarser
//! steps ([`part_steps`]): the same organ drawn by the same layout one or
//! more [`coarsest`] steps coarser, each with at most [`STEP_SHARE`] of
//! the triangles of the one before, kept only where it still looks like the part mesh
//! from every direction above ([`step_difference`]: its cover within
//! [`STEP_COVER`], its colour within [`STEP_COLOUR`]). Each step has a
//! detail ([`detail`]), the side of a square of its triangles' mean area
//! in card lengths, never finer than the step before; a renderer draws the
//! finest step whose detail covers the least pixels, so no triangle is
//! drawn smaller than that and none coarser than it needs.

use crate::blooms::{self, Form};
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::{Look, Shape};
use crate::math::Vec3;
use crate::mesh::{Mesh, PlantMesh};
use crate::rng::mix64;

/// Variants of each part mesh.
pub const VARIANTS: usize = 4;

/// Most triangles a part mesh holds.
pub const TRIANGLES: usize = 1024;

/// Most triangles a needle shoot's part mesh holds: a shoot has hundreds
/// of needles.
pub const SHOOT_TRIANGLES: usize = 8192;

/// Most triangles a cluster's or a flower head's part mesh holds: a
/// cluster has dozens to hundreds of units, a head as many florets.
pub const CLUSTER_TRIANGLES: usize = 8192;

/// A needle shoot's span over its needles' width (see the module).
pub const SHOOT_SPAN: f64 = 4.0;

/// One organ type's part meshes.
#[derive(Debug, Clone, PartialEq)]
pub struct PartMesh {
    /// The organ type (template) it stands for, in the program's order.
    pub template: usize,
    /// [`VARIANTS`] meshes, each one organ in its own frame.
    pub variants: Vec<Mesh>,
    /// The share of its card's length that must cover the renderer's
    /// least pixels for it to be drawn: 1, or less for a part whose detail
    /// is finer than the organ (see the module).
    pub span: f64,
}

impl PartMesh {
    /// The most triangles any variant holds.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.variants
            .iter()
            .map(Mesh::triangle_count)
            .max()
            .unwrap_or(0)
    }
}

/// The part meshes of `looks`: one for each organ type whose look draws a
/// solid that fits its [`cap`], in the types' order.
#[must_use]
pub fn part_meshes(looks: &[Look]) -> Vec<PartMesh> {
    part_meshes_within(looks, 1.0)
}

/// [`part_meshes`] with each type's cap ([`cap`]) times `share`, at most
/// 1: an export drawn to a budget (`crate::export`) draws its parts
/// coarser, and a type whose coarsest part mesh no longer fits keeps its
/// cards.
#[must_use]
pub fn part_meshes_within(looks: &[Look], share: f64) -> Vec<PartMesh> {
    (0..looks.len())
        .filter_map(|index| {
            let cap = cap(&looks[index]);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let cap = if share >= 1.0 {
                cap
            } else {
                (cap as f64 * share.max(0.0)) as usize
            };
            part_mesh(looks, index, cap)
        })
        .collect()
}

/// The most triangles a part mesh of `look` may hold.
#[must_use]
pub fn cap(look: &Look) -> usize {
    match look.form {
        Form::Shoot => SHOOT_TRIANGLES,
        _ if look.inflorescence.is_some()
            || matches!(look.shape, Shape::Head(_) | Shape::Spikelet(_)) =>
        {
            CLUSTER_TRIANGLES
        }
        _ => TRIANGLES,
    }
}

/// The most steps coarser a part mesh of `look` is drawn to fit its
/// [`cap`]: `crate::fruit::COARSER`, or fewer for a cluster whose coarsest
/// steps would draw it worse than its card (`crate::clusters::coarsest`).
#[must_use]
pub fn coarsest(look: &Look) -> usize {
    look.inflorescence
        .as_ref()
        .map_or(crate::fruit::COARSER, |cluster| {
            crate::clusters::coarsest(&cluster.cluster)
        })
}

/// The span of a part mesh of `look` (see the module).
#[must_use]
pub fn span(look: &Look) -> f64 {
    match (&look.form, &look.shape) {
        (Form::Shoot, Shape::Needles(needles)) => {
            (SHOOT_SPAN * crate::shoots::needle_width(needles)).min(1.0)
        }
        _ => 1.0,
    }
}

/// Whether organ type `index` of `looks` draws as a part mesh: its look
/// draws a solid on level 0 and its card's frame follows its organ's.
#[must_use]
pub fn drawn_as_part(looks: &[Look], index: usize) -> bool {
    looks
        .get(index)
        .is_some_and(|look| look.solid.is_none() && look.form.solid_at(0) && look.face_up <= 0.0)
}

/// Each organ type of `looks`, whether it has a part mesh in `parts`.
#[must_use]
pub fn types(looks: &[Look], parts: &[PartMesh]) -> Vec<bool> {
    let mut types = vec![false; looks.len()];
    for part in parts {
        if let Some(slot) = types.get_mut(part.template) {
            *slot = true;
        }
    }
    types
}

fn part_mesh(looks: &[Look], index: usize, cap: usize) -> Option<PartMesh> {
    let coarsest = coarsest(looks.get(index)?);
    (0..=coarsest).find_map(|coarse| {
        let variants = variants_at(looks, index, coarse)?;
        fits(&variants, cap).then(|| PartMesh {
            template: index,
            variants,
            span: span(&looks[index]),
        })
    })
}

/// Whether every variant draws something and holds at most `cap`
/// triangles.
fn fits(variants: &[Mesh], cap: usize) -> bool {
    variants.iter().all(|mesh| mesh.triangle_count() > 0)
        && variants.iter().all(|mesh| mesh.triangle_count() <= cap)
}

/// Organ type `index` of `looks` drawn `coarse` steps coarser, each of
/// [`VARIANTS`] organs in its card's frame; none for a type that is not
/// drawn as a part.
fn variants_at(looks: &[Look], index: usize, coarse: usize) -> Option<Vec<Mesh>> {
    if !drawn_as_part(looks, index) {
        return None;
    }
    let organ = u16::try_from(index).ok()?;
    let mut solid = vec![false; looks.len()];
    solid[index] = true;
    let at = |id: u64| GraphOrgan {
        id,
        organ,
        segment: None,
        position: Vec3::ZERO,
        heading: Vec3::Y,
        left: Vec3::X,
        size: 1.0,
        born: 0.0,
        shed: None,
        light: 1.0,
        within: None,
        adult: None,
    };
    // The frame of a card of this type on the organ drawn.
    let (base, heading, left, length) = crate::mesh::place(&at(0), &looks[index]);
    let out = left.cross(heading);
    Some(
        (0..VARIANTS as u64)
            .map(|variant| {
                // Variant `k` is the look's variety `k` where it varies.
                let varied;
                let looks = match usize::try_from(variant)
                    .ok()
                    .and_then(|variant| looks[index].varieties.get(variant))
                {
                    Some(variety) => {
                        let mut own = looks.to_vec();
                        own[index] = variety.clone();
                        varied = own;
                        &varied[..]
                    }
                    None => looks,
                };
                let graph = PlantGraph {
                    age: 1.0,
                    height: 1.0,
                    segments: Vec::new(),
                    organs: vec![at(mix64(variant + 1))],
                    structures: Vec::new(),
                };
                let mut mesh = Mesh::default();
                blooms::build_coarse(
                    &graph,
                    looks,
                    &solid,
                    &|look, _| look.colour,
                    coarse,
                    &mut mesh,
                );
                into_frame(&mut mesh, (base, heading, left, out), length);
                mesh
            })
            .collect(),
    )
}

/// A coarser step of an organ type's part mesh (spec rework SEMLOD): the
/// type drawn `coarse` steps coarser than its primitive's finest, as
/// [`VARIANTS`] meshes in its card's frame like the part mesh's own.
#[derive(Debug, Clone, PartialEq)]
pub struct PartStep {
    pub coarse: usize,
    pub variants: Vec<Mesh>,
    /// The size of its triangles ([`detail`]), a share of its card's
    /// length, and at least the finer steps' (see [`PartSteps`]).
    pub detail: f64,
    /// The most cover it loses or gains against the part mesh from any
    /// direction, and on average over them ([`step_cover`]), as measured
    /// when it was drawn: a renderer that wants its parts faithful from
    /// every side stops before a step whose `cover` it finds too great.
    pub cover: f64,
    pub mean_cover: f64,
}

/// An organ type's part mesh in steps (spec rework SEMLOD): the part mesh
/// ([`PartMesh`]) is the finest, and each step after it is drawn coarser
/// with at most [`STEP_SHARE`] of the triangles of the one before.
///
/// A renderer draws, of the part mesh and its steps, the finest whose
/// `detail` times the card's projected length covers at least its least
/// pixels, and the coarsest when none does; a part too small for the part
/// mesh's [`PartMesh::span`] keeps its card as before.
#[derive(Debug, Clone, PartialEq)]
pub struct PartSteps {
    /// The organ type, as [`PartMesh::template`].
    pub template: usize,
    /// The part mesh's own detail.
    pub detail: f64,
    /// The coarser steps, finest first; none where no coarser step halves
    /// the triangles.
    pub steps: Vec<PartStep>,
}

/// Each of `parts`' types in steps (see [`PartSteps`]), in their order.
#[must_use]
pub fn part_steps(looks: &[Look], parts: &[PartMesh]) -> Vec<PartSteps> {
    parts
        .iter()
        .map(|part| {
            let index = part.template;
            let mut last = part.triangles();
            let finest = (0..=looks.get(index).map_or(0, coarsest))
                .find(|&coarse| {
                    variants_at(looks, index, coarse)
                        .is_some_and(|variants| variants == part.variants)
                })
                .unwrap_or(0);
            let finest_detail = detail(&part.variants[0]);
            let mut coarsest_detail = finest_detail;
            let mut steps = Vec::new();
            for coarse in finest + 1..=looks.get(index).map_or(0, coarsest) {
                let Some(variants) = variants_at(looks, index, coarse) else {
                    break;
                };
                let triangles = variants.iter().map(Mesh::triangle_count).max().unwrap_or(0);
                if !fits(&variants, last * 2 / 3) {
                    continue;
                }
                let measured = step_cover(&part.variants, &variants);
                if cover_rule(&looks[index], coarse).holds(&measured) {
                    last = triangles;
                    // Each step's detail at least the finer one's, so a
                    // renderer moves to it no sooner.
                    coarsest_detail = detail(&variants[0]).max(coarsest_detail);
                    steps.push(PartStep {
                        coarse,
                        detail: coarsest_detail,
                        variants,
                        cover: measured.worst,
                        mean_cover: measured.mean,
                    });
                }
            }
            PartSteps {
                template: index,
                detail: finest_detail,
                steps,
            }
        })
        .collect()
}

/// Most of the triangles of the step before it a step may hold: two
/// thirds, so each step is a third or more cheaper.
pub const STEP_SHARE: f64 = 2.0 / 3.0;

/// Most a step may lose or gain of the part mesh's cover seen from any
/// direction, as a share of that direction's cover or, where it shows
/// less than the mean over the directions (a flat part seen edge on), of
/// the mean.
pub const STEP_COVER: f64 = 0.1;

/// Most a step's mean colour seen from any direction may differ from the
/// part mesh's in any linear channel, weighed by how much of the part mesh
/// that direction shows against its mean, at most 1: a petal's underside a shade
/// lighter where the step draws it with fewer vertices passes, a cluster
/// whose units turn a different colour does not.
pub const STEP_COLOUR: f64 = 0.07;

/// Cells across the square a step's cover is measured in.
const VIEW_CELLS: usize = 48;

/// How far a step may stray from the part mesh in cover (see
/// [`step_cover`]): from its worst direction, and on average over them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverRule {
    pub worst: f64,
    pub mean: f64,
}

impl CoverRule {
    /// Whether `measured` keeps within the rule, and its colour within
    /// [`STEP_COLOUR`].
    #[must_use]
    pub fn holds(&self, measured: &StepCover) -> bool {
        measured.worst <= self.worst && measured.mean <= self.mean && measured.colour <= STEP_COLOUR
    }
}

/// Most a thinned step of a cluster of spikelets
/// (`crate::clusters::thinned`) may lose or gain of the part mesh's cover
/// from its worst direction; on average it keeps within [`STEP_COVER`].
/// An airy panicle covers few cells of the grid it is measured on, so
/// even its part mesh and its finest drawing differ by more than a tenth
/// from some side.
pub const THINNED_COVER: f64 = 0.35;

/// Most a cluster of spikelets drawn as its branches' outlines
/// (`crate::clusters::outlined`) may lose or gain of the part mesh's
/// cover from its worst direction, and on average: a step for a renderer
/// that trades faithfulness for speed.
pub const OUTLINE_COVER: f64 = 0.5;

/// The [`CoverRule`] a step `coarse` steps coarser of a part mesh of
/// `look` keeps: [`THINNED_COVER`] or [`OUTLINE_COVER`] for a cluster of
/// spikelets thinned or outlined, else [`STEP_COVER`] from every
/// direction.
#[must_use]
pub fn cover_rule(look: &Look, coarse: usize) -> CoverRule {
    let cluster = look.inflorescence.as_ref();
    let (worst, mean) = if cluster.is_some_and(|cluster| crate::clusters::outlined(cluster, coarse))
    {
        (OUTLINE_COVER, OUTLINE_COVER)
    } else if cluster.is_some_and(|cluster| crate::clusters::thinned(cluster, coarse)) {
        (THINNED_COVER, STEP_COVER)
    } else {
        (STEP_COVER, STEP_COVER)
    };
    CoverRule { worst, mean }
}

/// How far a step is from the part mesh ([`step_cover`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepCover {
    /// The most cover it loses or gains from any direction, and on
    /// average over the directions and variants.
    pub worst: f64,
    pub mean: f64,
    /// The most its colour differs.
    pub colour: f64,
}

/// How far `step` is from the part mesh `finest`, variant by variant and
/// seen along each of the cover directions (`crate::mesh`), the upper
/// hemisphere: the most cover it loses or gains, as a share of the part
/// mesh's cover that way or of its mean over the directions, whichever is
/// more, and the most its mean seen colour differs in any channel,
/// weighed by that direction's cover over the mean, at most 1. Each view is a square of [`VIEW_CELLS`] cells round the part
/// mesh's outline, a cell covered when its centre falls in a triangle
/// turned toward the viewer and coloured by the nearest one.
#[must_use]
pub fn step_difference(finest: &[Mesh], step: &[Mesh]) -> (f64, f64) {
    let measured = step_cover(finest, step);
    (measured.worst, measured.colour)
}

/// [`step_difference`], with the cover lost or gained on average over
/// the directions and variants too.
#[must_use]
pub fn step_cover(finest: &[Mesh], step: &[Mesh]) -> StepCover {
    let directions = crate::mesh::cover_directions();
    let mut worst = (0.0_f64, 0.0_f64);
    let (mut sum, mut seen) = (0.0, 0_u32);
    for (fine, coarse) in finest.iter().zip(step) {
        let views: Vec<(View, View)> = directions
            .iter()
            .map(|&direction| {
                let square = outline(fine, direction);
                (
                    view(fine, direction, square),
                    view(coarse, direction, square),
                )
            })
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let mean = views
            .iter()
            .map(|(fine, _)| fine.covered as f64)
            .sum::<f64>()
            / views.len().max(1) as f64;
        if mean <= 0.0 {
            continue;
        }
        for (fine, coarse) in &views {
            #[allow(clippy::cast_precision_loss)]
            let cover =
                (coarse.covered as f64 - fine.covered as f64).abs() / mean.max(fine.covered as f64);
            #[allow(clippy::cast_precision_loss)]
            let colour = (0..3)
                .map(|k| {
                    (coarse.colour[k] - fine.colour[k]).abs()
                        * (fine.covered as f64 / mean).min(1.0)
                })
                .fold(0.0, f64::max);
            worst = (worst.0.max(cover), worst.1.max(colour));
            sum += cover;
            seen += 1;
        }
    }
    StepCover {
        worst: worst.0,
        mean: if seen == 0 {
            0.0
        } else {
            sum / f64::from(seen)
        },
        colour: worst.1,
    }
}

/// A mesh seen along a direction: the cells it covers and their mean
/// colour.
struct View {
    covered: usize,
    colour: [f64; 3],
}

fn vertex(mesh: &Mesh, index: u32) -> Vec3 {
    mesh.positions.get(index as usize).map_or(Vec3::ZERO, |p| {
        Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
    })
}

/// Two axes across `direction`.
fn across(direction: Vec3) -> (Vec3, Vec3) {
    let up = if direction.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let a = up.cross(direction).normalize_or(Vec3::X);
    (a, direction.cross(a))
}

/// The square, as its corner and side, round `mesh` seen along
/// `direction`, a tenth wider than its outline.
fn outline(mesh: &Mesh, direction: Vec3) -> (f64, f64, f64) {
    let (a, b) = across(direction);
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &mesh.positions {
        let p = Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        for (k, v) in [p.dot(a), p.dot(b)].into_iter().enumerate() {
            low[k] = low[k].min(v);
            high[k] = high[k].max(v);
        }
    }
    let side = (high[0] - low[0]).max(high[1] - low[1]) * 1.1 + 1e-9;
    (
        (low[0] + high[0] - side) / 2.0,
        (low[1] + high[1] - side) / 2.0,
        side,
    )
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]
fn view(mesh: &Mesh, direction: Vec3, square: (f64, f64, f64)) -> View {
    let (a, b) = across(direction);
    let (x0, y0, side) = square;
    let cell = side / VIEW_CELLS as f64;
    let mut depth = vec![f64::MIN; VIEW_CELLS * VIEW_CELLS];
    let mut colour = vec![[0.0; 3]; VIEW_CELLS * VIEW_CELLS];
    let to_cell = |v: f64| (v.max(0.0) as usize).min(VIEW_CELLS);
    for t in mesh.indices.as_chunks::<3>().0 {
        let p: [(f64, f64, f64); 3] = std::array::from_fn(|k| {
            let q = vertex(mesh, t[k]);
            (
                (q.dot(a) - x0) / cell,
                (q.dot(b) - y0) / cell,
                q.dot(direction),
            )
        });
        let c: [f64; 3] = std::array::from_fn(|k| {
            t.iter()
                .map(|&i| mesh.colors.get(i as usize).map_or(0.0, |c| f64::from(c[k])))
                .sum::<f64>()
                / 3.0
        });
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        // Only the faces turned toward the viewer, their corners turning
        // anticlockwise as seen.
        if area < 1e-12 {
            continue;
        }
        let xs = p.map(|q| q.0);
        let ys = p.map(|q| q.1);
        let (x_low, x_high) = (
            to_cell(xs.iter().copied().fold(f64::MAX, f64::min).floor()),
            to_cell(xs.iter().copied().fold(f64::MIN, f64::max).ceil()),
        );
        let (y_low, y_high) = (
            to_cell(ys.iter().copied().fold(f64::MAX, f64::min).floor()),
            to_cell(ys.iter().copied().fold(f64::MIN, f64::max).ceil()),
        );
        for y in y_low..y_high {
            for x in x_low..x_high {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                let at = y * VIEW_CELLS + x;
                if z > depth[at] {
                    depth[at] = z;
                    colour[at] = c;
                }
            }
        }
    }
    let covered = depth.iter().filter(|z| **z > f64::MIN).count();
    let mut mean = [0.0; 3];
    for (_, c) in depth.iter().zip(&colour).filter(|(z, _)| **z > f64::MIN) {
        for k in 0..3 {
            mean[k] += c[k] / covered as f64;
        }
    }
    View {
        covered,
        colour: mean,
    }
}

/// The size of `mesh`'s triangles: the side of a square of their mean
/// area.
#[must_use]
pub fn detail(mesh: &Mesh) -> f64 {
    let point = |i: u32| {
        mesh.positions.get(i as usize).map_or(Vec3::ZERO, |p| {
            Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
        })
    };
    let triangles = mesh.triangle_count();
    if triangles == 0 {
        return 0.0;
    }
    let area: f64 = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            0.5 * (point(t[1]) - point(t[0]))
                .cross(point(t[2]) - point(t[0]))
                .length()
        })
        .sum();
    #[allow(clippy::cast_precision_loss)]
    let mean = area / triangles as f64;
    crate::math::sqrt(mean)
}

/// One site of a nearest level drawn as its type's part mesh: which part
/// and variant, the frame it is put back in (`p = base + length (x left +
/// y heading + z out)`, see the module) and the cards it replaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The organ's card in the level's cards, and the crossing card of a
    /// crossed pair; a renderer drawing the part leaves both out.
    pub card: usize,
    pub cross: Option<usize>,
    /// The part mesh, as an index into the `parts` placed, and its variant.
    pub part: usize,
    pub variant: usize,
    pub base: Vec3,
    /// Unit vectors: the card's heading, its left made square to it, and
    /// `left × heading`.
    pub heading: Vec3,
    pub left: Vec3,
    pub out: Vec3,
    pub length: f64,
    /// What the part's colours are multiplied by: as much darker as its
    /// card's colour is than its look's, 0 to 2.
    pub darkening: f32,
}

/// Where each site of `plant`, a nearest level built for part meshes
/// (`crate::mesh::build_with`), is drawn as one of its type's `parts`, in
/// the order of its sites; sites whose type has no part mesh are left out.
/// Site `n` draws the variant `mix64(n)` picks.
#[must_use]
pub fn placements(plant: &PlantMesh, looks: &[Look], parts: &[PartMesh]) -> Vec<Placement> {
    let to_vec = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mut out = Vec::with_capacity(plant.sites.len());
    for (n, &site) in plant.sites.iter().enumerate() {
        let index = site as usize;
        let Some(&card) = plant.cards.get(index) else {
            continue;
        };
        let template = usize::from(card.template);
        let (Some(part), Some(look)) = (
            parts.iter().position(|part| part.template == template),
            looks.get(template),
        ) else {
            continue;
        };
        let variant = usize::try_from(mix64(n as u64) % VARIANTS as u64).unwrap_or(0);
        if variant >= parts[part].variants.len() {
            continue;
        }
        let heading = to_vec(card.heading).normalize_or(Vec3::Y);
        let along = to_vec(card.left);
        let left = (along - heading * along.dot(heading))
            .normalize_or(crate::math::any_perpendicular(heading));
        // The crossing card, pushed just before the organ's own with a copy
        // of its base and heading.
        #[allow(clippy::float_cmp)]
        let cross = index.checked_sub(1).filter(|&cross| {
            plant.cards.get(cross).is_some_and(|other| {
                other.template == card.template
                    && other.base == card.base
                    && other.heading == card.heading
            })
        });
        out.push(Placement {
            card: index,
            cross,
            part,
            variant,
            base: to_vec(card.base),
            heading,
            left,
            out: left.cross(heading),
            length: f64::from(card.length),
            darkening: (luminance(card.color) / luminance(look.colour).max(1.0e-6)).clamp(0.0, 2.0),
        });
    }
    out
}

/// Draw each site of `plant`, a nearest level built for part meshes
/// (`crate::mesh::build_with`), as one of its type's `parts`, into its wood
/// mesh, and leave out the site's cards (both of a crossed pair): what a
/// renderer that draws part meshes shows near the camera, for previews.
/// Each site draws the variant its index picks, in the part mesh's
/// colours darkened with its card as the renderer darkens them.
pub fn place(plant: &mut PlantMesh, looks: &[Look], parts: &[PartMesh]) {
    let to_vec = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut dropped = vec![false; plant.cards.len()];
    for placed in placements(plant, looks, parts) {
        let mesh = &parts[placed.part].variants[placed.variant];
        let card = plant.cards[placed.card];
        let (base, left, heading, out) = (placed.base, placed.left, placed.heading, placed.out);
        let darkening = placed.darkening;
        let first = u32::try_from(plant.wood.positions.len()).unwrap_or(u32::MAX);
        for ((position, normal), colour) in
            mesh.positions.iter().zip(&mesh.normals).zip(&mesh.colors)
        {
            let p = to_vec(*position);
            let n = to_vec(*normal);
            plant
                .wood
                .positions
                .push((base + (left * p.x + heading * p.y + out * p.z) * placed.length).to_f32());
            plant
                .wood
                .normals
                .push((left * n.x + heading * n.y + out * n.z).to_f32());
            plant.wood.uvs.push([0.0, 0.0]);
            plant.wood.colors.push([
                colour[0] * darkening,
                colour[1] * darkening,
                colour[2] * darkening,
                1.0,
            ]);
            plant.wood.births.push(card.born);
            plant.wood.sheds.push(card.shed);
            plant.wood.levels.push(3);
        }
        plant
            .wood
            .indices
            .extend(mesh.indices.iter().map(|&i| first + i));
        dropped[placed.card] = true;
        if let Some(cross) = placed.cross {
            dropped[cross] = true;
        }
    }
    let mut kept = dropped.iter();
    plant
        .cards
        .retain(|_| !kept.next().copied().unwrap_or(false));
    plant.sites.clear();
}

/// Express `mesh`, drawn in the organ's frame, in the frame of its card:
/// base `base`, heading `heading`, left `left` and `out` = left × heading,
/// `length` long (1, or a cluster's card length).
#[allow(clippy::cast_possible_truncation)] // meshes hold f32
fn into_frame(mesh: &mut Mesh, (base, heading, left, out): (Vec3, Vec3, Vec3, Vec3), length: f64) {
    let to_card = |p: Vec3| [p.dot(left), p.dot(heading), p.dot(out)];
    for position in &mut mesh.positions {
        let p = Vec3::new(
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        ) - base;
        *position = to_card(p).map(|value| (value / length) as f32);
    }
    for normal in &mut mesh.normals {
        let n = Vec3::new(
            f64::from(normal[0]),
            f64::from(normal[1]),
            f64::from(normal[2]),
        );
        *normal = to_card(n).map(|value| value as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::{Cluster, ClusterLook};
    use crate::looks::{ClusterKind, Flower, Fruit, FruitKind, Inflorescence, Needles, Simple};

    fn look(shape: Shape) -> Look {
        Look {
            roll: 0.0,
            insertion: crate::looks::Insertion::Marginal,
            organ: "bloom".to_string(),
            form: Form::default_for(&shape),
            bend: crate::bend::Bend::FLAT,
            shape,
            colour: [0.7, 0.1, 0.1],
            shade: [0.2, 0.05, 0.05],
            accent: [0.3, 0.5, 0.1],
            face_up: 0.0,
            solid: None,
            forms: crate::looks::Forms::default(),
            inflorescence: None,
            varieties: Vec::new(),
        }
    }

    /// `shape`'s units in a cluster of `flowers` of `kind`.
    fn clustered(shape: Shape, kind: ClusterKind, flowers: u32) -> Look {
        let mut look = look(shape);
        let spec = Inflorescence {
            kind: Some(kind),
            flowers: Some(flowers),
            ..Inflorescence::default()
        };
        look.inflorescence = ClusterLook::new(Cluster::of(&spec), &look.shape, &[]);
        assert!(look.inflorescence.is_some());
        look
    }

    #[test]
    fn flowers_fruit_and_clusters_get_part_meshes_and_leaves_do_not() {
        let looks = vec![
            look(Shape::Simple(Simple::default())),
            look(Shape::Flower(Flower::default())),
            clustered(
                Shape::Fruit(Fruit {
                    kind: FruitKind::Drupelets,
                    unripe: 0.5,
                    ..Fruit::default()
                }),
                ClusterKind::Raceme,
                8,
            ),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Umbel, 12),
        ];
        let parts = part_meshes(&looks);
        assert_eq!(
            parts.iter().map(|p| p.template).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        for part in &parts {
            assert_eq!(part.variants.len(), VARIANTS);
            assert!(
                part.triangles() <= cap(&looks[part.template]),
                "{}",
                part.triangles()
            );
            for mesh in &part.variants {
                for p in &mesh.positions {
                    let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                    assert!(length <= 1.3, "within the organ's length: {p:?}");
                }
            }
        }
        // Variants differ: the cluster turns and its unripe fruit change.
        assert_ne!(
            parts[1].variants[0].positions,
            parts[1].variants[1].positions
        );
    }

    #[test]
    fn a_part_put_back_by_its_card_lands_where_the_organ_is() {
        // A flower's card is centred on it and faces along its heading
        // (Mount::Facing); a fruit's stands on it (Mount::Along); a
        // cluster's card is its card frame's. Placed back by a card of a
        // turned and moved organ, the part mesh is the organ drawn there.
        for shown in [
            look(Shape::Flower(Flower::default())),
            look(Shape::Fruit(Fruit::default())),
            look(Shape::Needles(Needles::default())),
            look(Shape::Needles(Needles {
                ranks: 0,
                ..Needles::default()
            })),
            clustered(Shape::Fruit(Fruit::default()), ClusterKind::Raceme, 3),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Umbel, 4),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Cyme, 3),
        ] {
            // Drawn whole: a part mesh at its finest.
            assert!(
                shown.inflorescence.is_none() || crate::clusters::triangles(&shown) <= cap(&shown),
                "{}",
                crate::clusters::triangles(&shown)
            );
            let looks = vec![shown];
            let parts = part_meshes(&looks);
            let organ = GraphOrgan {
                id: crate::rng::mix64(1),
                organ: 0,
                segment: None,
                position: Vec3::new(1.0, 2.0, -3.0),
                heading: Vec3::new(0.0, 0.6, 0.8),
                left: Vec3::X,
                // Leaf-like parts take detail from their size in metres:
                // the same size as the part mesh, so the two match.
                size: 1.0,
                born: 0.0,
                shed: None,
                light: 1.0,
                within: None,
                adult: None,
            };
            let (base, heading, left, length) = crate::mesh::place(&organ, &looks[0]);
            let out = left.cross(heading);
            let graph = PlantGraph {
                age: 1.0,
                height: 1.0,
                segments: Vec::new(),
                organs: vec![organ],
                structures: Vec::new(),
            };
            let mut drawn = Mesh::default();
            blooms::build(&graph, &looks, &[true], &|look, _| look.colour, &mut drawn);
            let back: Vec<Vec3> = parts[0].variants[0]
                .positions
                .iter()
                .map(|p| {
                    base + (left * f64::from(p[0])
                        + heading * f64::from(p[1])
                        + out * f64::from(p[2]))
                        * length
                })
                .collect();
            assert_eq!(back.len(), drawn.positions.len());
            for (b, d) in back.iter().zip(&drawn.positions) {
                let d = Vec3::new(f64::from(d[0]), f64::from(d[1]), f64::from(d[2]));
                assert!((*b - d).length() < 1e-5, "{b:?} {d:?}");
            }
        }
    }

    #[test]
    fn a_large_cluster_is_drawn_coarser_to_fit() {
        // A cluster takes more than a single fruit may.
        let fruit = Fruit {
            kind: FruitKind::Drupelets,
            ..Fruit::default()
        };
        let small = clustered(Shape::Fruit(fruit.clone()), ClusterKind::Raceme, 8);
        assert!(crate::clusters::triangles(&small) > TRIANGLES);
        let parts = part_meshes(&[small]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() > TRIANGLES);
        // One that takes more than a cluster may is drawn coarser, each of
        // its flowers still drawn.
        let flower = Flower {
            petals: 10,
            ..Flower::default()
        };
        let large = clustered(Shape::Flower(flower), ClusterKind::Raceme, 200);
        assert!(
            crate::clusters::triangles(&large) > CLUSTER_TRIANGLES,
            "this cluster needs coarsening: {}",
            crate::clusters::triangles(&large)
        );
        let parts = part_meshes(&[large]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() <= CLUSTER_TRIANGLES);
        assert!(parts[0].triangles() >= 200 * 8, "{}", parts[0].triangles());
    }

    #[test]
    fn only_a_compound_cluster_is_merged_to_fit() {
        // Too many drupelets to draw one by one: as a raceme the cluster
        // keeps its cards rather than become one solid; as a panicle each
        // of its racemes is merged into one.
        let fruit = Shape::Fruit(Fruit {
            kind: FruitKind::Drupelets,
            ..Fruit::default()
        });
        let raceme = clustered(fruit.clone(), ClusterKind::Raceme, 200);
        assert!(part_meshes(&[raceme]).is_empty());
        let mut panicle = look(fruit);
        let spec = Inflorescence {
            kind: Some(ClusterKind::Panicle),
            branches: Some(8),
            flowers: Some(25),
            ..Inflorescence::default()
        };
        panicle.inflorescence = ClusterLook::new(Cluster::of(&spec), &panicle.shape, &[]);
        let parts = part_meshes(&[panicle]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() <= CLUSTER_TRIANGLES);
    }

    #[test]
    fn needle_shoots_are_part_meshes_spanning_their_needles_width() {
        let needles = Needles::default();
        let looks = vec![
            look(Shape::Needles(needles.clone())),
            look(Shape::Fruit(Fruit::default())),
        ];
        let parts = part_meshes(&looks);
        assert_eq!(
            parts.iter().map(|p| p.template).collect::<Vec<_>>(),
            vec![0, 1]
        );
        let width = needles.width * needles.needle;
        assert!((parts[0].span - SHOOT_SPAN * width).abs() < 1e-12);
        assert!((parts[1].span - 1.0).abs() < 1e-12);
        assert!(parts[0].triangles() <= SHOOT_TRIANGLES);
        // A dense spray of short needles takes more than a fruit may.
        let dense = Needles {
            needle: 0.035,
            twigs: 5,
            aspect: 0.7,
            ..Needles::default()
        };
        let parts_dense = part_meshes(&[look(Shape::Needles(dense))]);
        assert_eq!(parts_dense.len(), 1);
        assert!(parts_dense[0].triangles() > TRIANGLES);
        assert!(parts_dense[0].triangles() <= SHOOT_TRIANGLES);
        // The variants differ out of the card's plane alone.
        assert_ne!(
            parts[0].variants[0].positions,
            parts[0].variants[1].positions
        );
        let flat = |mesh: &Mesh| -> Vec<[f32; 2]> {
            mesh.positions.iter().map(|p| [p[0], p[1]]).collect()
        };
        assert_eq!(
            flat(&parts[0].variants[0]).len(),
            flat(&parts[0].variants[1]).len()
        );
    }

    #[test]
    fn placed_at_their_sites_parts_replace_both_cards() {
        // Three fruit, each a crossed pair of cards, the second its site.
        let looks = vec![look(Shape::Fruit(Fruit::default()))];
        let parts = part_meshes(&looks);
        let mut plant = PlantMesh::default();
        for k in 0..3_u8 {
            let card = crate::mesh::Card {
                base: [f32::from(k), 1.0, 0.0],
                heading: [0.0, 1.0, 0.0],
                left: [1.0, 0.0, 0.0],
                length: 0.1,
                width: 0.05,
                color: looks[0].colour,
                template: 0,
                born: 0.0,
                shed: f32::INFINITY,
                bend: crate::bend::Bend::FLAT,
                segment: crate::mesh::NO_SEGMENT,
            };
            plant.cards.push(crate::mesh::Card {
                left: [0.0, 0.0, 1.0],
                ..card
            });
            plant.cards.push(card);
            plant.sites.push(u32::from(k) * 2 + 1);
        }
        place(&mut plant, &looks, &parts);
        assert!(plant.cards.is_empty() && plant.sites.is_empty());
        let per_site: Vec<usize> = (0..3)
            .map(|n: u64| {
                parts[0].variants[usize::try_from(mix64(n) % VARIANTS as u64).unwrap()]
                    .triangle_count()
            })
            .collect();
        assert_eq!(plant.wood.triangle_count(), per_site.iter().sum::<usize>());
        // Each lies within its organ's length of its card's base.
        for p in &plant.wood.positions {
            let nearest = (0..3_u8).map(|k| {
                let dx = p[0] - f32::from(k);
                (dx * dx + (p[1] - 1.0).powi(2) + p[2] * p[2]).sqrt()
            });
            assert!(nearest.fold(f32::INFINITY, f32::min) <= 0.13, "{p:?}");
        }
    }

    #[test]
    fn part_meshes_are_the_same_every_time() {
        let looks = vec![clustered(
            Shape::Fruit(Fruit {
                unripe: 0.3,
                ..Fruit::default()
            }),
            ClusterKind::Umbel,
            12,
        )];
        assert_eq!(part_meshes(&looks), part_meshes(&looks));
    }
}
