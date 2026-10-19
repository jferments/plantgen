//! Flowers, fruit and cones drawn as solids (plant forms milestone F3).
//!
//! An organ type whose look has a [`Form`] is drawn on its nearest
//! [`Form::levels`] levels of detail as geometry in the plant's wood mesh,
//! and its card is left out there; farther levels draw its card. The form
//! reads the look's own template parameters, so every species gains its
//! solids without new data:
//!
//! - A **flower** ([`Shape::Flower`]) is a floral diagram: `n` petals
//!   evenly round the flower's axis $`\mathbf a`$ (the organ's heading),
//!   each a thin solid leaf (`crate::leaves`) from the rim of the centre
//!   outward, raised by the cup $`\gamma`$ from the plane square to the
//!   axis and curled back by the curl over its length; a domed centre in
//!   the accent colour; and a floral cup when the flower's `tube` is set.
//!   With the flower's radius $`R`$ (half the organ's size) and the
//!   template's centre share $`c`$, petals start at $`r_0 = 0.6\,c R`$ and
//!   reach $`R`$, so a petal is $`\ell = R - r_0`$ long and
//!   $`\min(\ell,\ w\,\pi (r_0 + R) / n)`$ wide for the template's petal
//!   width $`w`$. That is a plain flower
//!   ([`crate::looks::Flower::is_plain`]); any other (sepals, roles, a
//!   corolla tube, stamens, a pistil: spec rework P5) is laid out whorl by
//!   whorl by `crate::flowers`, on the same radii.
//! - A **head** ([`Shape::Head`]) is a capitulum (`crate::capitula`): its
//!   phyllaries, receptacle and florets laid out from the template, its
//!   rays raised by the cup and curled by the curl as petals are.
//! - A **fruit** or bud ([`Shape::Fruit`] with `cone` below a half) is an
//!   ellipsoid along the organ's heading, its length the organ's size and
//!   its width the template's aspect times that.
//! - A **cone** (`cone` of a half or more) is that ellipsoid, shrunk to a
//!   core, under scales packed on a golden-angle spiral (Vogel 1979), each
//!   a small wedge pointing toward the cone's tip.
//!
//! Each organ type's solids on a level are capped at [`TYPE_TRIANGLES`]:
//! a type of laid-out flowers with more than fit drawn fine is drawn one
//! step coarser, as a part mesh is (an organ pipe's crown of flowers with
//! hundreds of stamens each), and a type with more organs than fit even so
//! (a tree in full bloom) keeps its cards on that level ([`step`]).

use serde::{Deserialize, Serialize};

use crate::graph::{GraphOrgan, PlantGraph};
use crate::leaves::{self, Leaf, Section, SolidLeaf};
use crate::looks::{Face, Fruit, Look, Shape, within};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};

/// How an organ is drawn as a solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Form {
    /// A flower, or a flower head (`crate::capitula`).
    Flower(FlowerForm),
    /// A fruit, a bud or a cone.
    Fruit(FruitForm),
    /// A grass or sedge spikelet (`crate::spikelets`), alone or as a
    /// cluster's unit.
    Spikelet(SpikeletForm),
    /// A thorn (plant forms F5): a slender cone along the organ, as wide at
    /// its foot as the card's aspect allows, on the nearest level.
    Thorn,
    /// Prickles (Rosaceae, 2026-10-19): a curved, flattened cone on the
    /// stem's surface at each node, and more between the nodes
    /// (`crate::prickles`), on the nearest level.
    Prickles(crate::prickles::PrickleForm),
    /// A needle shoot (plant roadmap P4): the card's shoot, axes and
    /// needles, as a solid on the nearest level (`crate::shoots`).
    Shoot,
    /// No solid: the card everywhere.
    Card,
}

/// A flower's shape beyond its template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowerForm {
    /// Degrees the petals rise from the plane square to the flower's axis:
    /// 0 a flat star, about 40 a bowl, 70 or more a cup or tube.
    pub cup: f64,
    /// Degrees each petal curls back over its length, up to 270: past
    /// half a turn a revolute tepal rolls back on itself.
    pub curl: f64,
    /// Share of each petal's length from its foot that stays in the cup
    /// before it curls back: 0 curls it evenly from its foot; a Turk's-cap
    /// lily's tepals, reflexed from two fifths of their length, 0.4.
    #[serde(skip_serializing_if = "crate::looks::is_zero")]
    pub curl_from: f64,
    /// Levels, from the nearest, drawn solid: 1 or 2.
    pub levels: u8,
    /// Moved to the flower card (spec rework P5): refused here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tube: Option<serde_json::Value>,
    /// Moved to the flower card (spec rework P5): refused here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamens: Option<serde_json::Value>,
    /// Moved to the flower card (spec rework P5): refused here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamen_length: Option<serde_json::Value>,
}

impl Default for FlowerForm {
    fn default() -> Self {
        Self {
            cup: 30.0,
            curl: 12.0,
            curl_from: 0.0,
            levels: 1,
            tube: None,
            stamens: None,
            stamen_length: None,
        }
    }
}

/// How a spikelet is drawn beyond its template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpikeletForm {
    /// Levels, from the nearest, drawn solid: 1 or 2.
    pub levels: u8,
}

impl Default for SpikeletForm {
    fn default() -> Self {
        Self { levels: 1 }
    }
}

/// A fruit's shape beyond its template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FruitFormCard")]
pub struct FruitForm {
    /// Scales on a cone; 0 picks a count from the cone's size.
    pub scales: u32,
    /// Levels, from the nearest, drawn solid: 1 or 2.
    pub levels: u8,
}

impl Default for FruitForm {
    fn default() -> Self {
        Self {
            scales: 0,
            levels: 1,
        }
    }
}

/// A fruit form as a spec writes it, and the stalk colour it held before
/// L1, read only to say where it went.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FruitFormCard {
    scales: u32,
    levels: u8,
    stalk: Option<serde::de::IgnoredAny>,
}

impl Default for FruitFormCard {
    fn default() -> Self {
        let form = FruitForm::default();
        Self {
            scales: form.scales,
            levels: form.levels,
            stalk: None,
        }
    }
}

impl TryFrom<FruitFormCard> for FruitForm {
    type Error = String;

    fn try_from(card: FruitFormCard) -> Result<Self, String> {
        if card.stalk.is_some() {
            return Err(
                "form fruit `stalk` is now the look's `inflorescence` `stalk`: a cluster of \
                 fruit is an inflorescence whose units are fruit"
                    .to_string(),
            );
        }
        Ok(Self {
            scales: card.scales,
            levels: card.levels,
        })
    }
}

/// Most triangles one organ type's solids take on a level.
pub const TYPE_TRIANGLES: usize = 200_000;

impl Form {
    /// The form a look of `shape` takes unless it sets its own: flowers
    /// and heads are floral diagrams, fruit, buds and cones solids, needle
    /// sprays shoots; everything else is drawn as cards.
    #[must_use]
    pub fn default_for(shape: &Shape) -> Self {
        match shape {
            // Its petals rise as far as its look says, where it says.
            Shape::Flower(flower) => Self::Flower(FlowerForm {
                cup: flower.rise.unwrap_or(FlowerForm::default().cup),
                ..FlowerForm::default()
            }),
            // A daisy's rays spread nearly flat round its disc.
            Shape::Head(_) => Self::Flower(FlowerForm {
                cup: 8.0,
                curl: 6.0,
                ..FlowerForm::default()
            }),
            Shape::Fruit(_) => Self::Fruit(FruitForm::default()),
            Shape::Spikelet(_) => Self::Spikelet(SpikeletForm::default()),
            Shape::Needles(_) => Self::Shoot,
            _ => Self::Card,
        }
    }

    /// Whether the form draws anything on an organ of `shape`: a flower
    /// only on a flower or a head, a fruit only on a fruit, a needle shoot
    /// only on needles. On any other shape it would take the organ's cards
    /// away on its levels and draw nothing there.
    #[must_use]
    pub fn draws_on(&self, shape: &Shape) -> bool {
        match self {
            Self::Flower(_) => matches!(shape, Shape::Flower(_) | Shape::Head(_)),
            Self::Fruit(_) => matches!(shape, Shape::Fruit(_)),
            Self::Spikelet(_) => matches!(shape, Shape::Spikelet(_)),
            Self::Shoot => matches!(shape, Shape::Needles(_)),
            Self::Thorn | Self::Prickles(_) | Self::Card => true,
        }
    }

    /// Check the values, and that the form draws on `shape`.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self, shape: &Shape) -> Result<(), String> {
        if !self.draws_on(shape) {
            // Thorns and cards draw on every shape.
            let (form, needs) = match self {
                Self::Flower(_) => ("flower", "a flower or a head"),
                Self::Fruit(_) => ("fruit", "a fruit"),
                Self::Spikelet(_) => ("spikelet", "a spikelet"),
                Self::Shoot | Self::Thorn | Self::Prickles(_) | Self::Card => ("shoot", "needles"),
            };
            return Err(format!(
                "form {form} draws only on {needs}, found {}",
                shape.name()
            ));
        }
        match self {
            Self::Flower(form) => {
                within("form flower cup", form.cup, -90.0, 90.0)?;
                within("form flower curl", form.curl, -180.0, 270.0)?;
                within("form flower curl_from", form.curl_from, 0.0, 0.9)?;
                for (key, moved) in [
                    ("tube", &form.tube),
                    ("stamens", &form.stamens),
                    ("stamen_length", &form.stamen_length),
                ] {
                    if moved.is_some() {
                        return Err(format!(
                            "form flower {key} moved to the flower card (P5): set the shape's `{key}`"
                        ));
                    }
                }
                within("form flower levels", f64::from(form.levels), 0.0, 2.0)
            }
            Self::Fruit(form) => {
                within("form fruit scales", f64::from(form.scales), 0.0, 200.0)?;
                within("form fruit levels", f64::from(form.levels), 0.0, 2.0)
            }
            Self::Spikelet(form) => {
                within("form spikelet levels", f64::from(form.levels), 0.0, 2.0)
            }
            Self::Prickles(form) => form.validate(),
            Self::Thorn | Self::Shoot | Self::Card => Ok(()),
        }
    }

    /// Whether level `level` may draw the form solid.
    #[must_use]
    pub fn solid_at(&self, level: usize) -> bool {
        let levels = match self {
            Self::Flower(form) => form.levels,
            Self::Fruit(form) => form.levels,
            Self::Spikelet(form) => form.levels,
            Self::Thorn | Self::Prickles(_) | Self::Shoot => 1,
            Self::Card => 0,
        };
        level < usize::from(levels.min(2))
    }
}

/// About how many triangles one organ of `look` takes as a solid.
fn triangles_per_organ(look: &Look, coarse: usize) -> usize {
    if look.inflorescence.is_some() {
        return crate::clusters::triangles(look);
    }
    match (&look.form, &look.shape) {
        (Form::Flower(_), Shape::Flower(flower)) if flower.is_plain() => {
            flower.petals as usize * 24 + DOME_TRIANGLES
        }
        (Form::Flower(form), Shape::Flower(flower)) => {
            crate::flowers::triangles(flower, form, coarse)
        }
        (Form::Flower(form), Shape::Head(head)) => crate::capitula::triangles(head, form, 0),
        (Form::Fruit(form), Shape::Fruit(fruit)) if !crate::fruit::is_single(fruit) => {
            crate::fruit::triangles(form, fruit)
        }
        (Form::Fruit(form), Shape::Fruit(fruit)) if fruit.cone >= 0.5 && fruit.is_plain_cone() => {
            ELLIPSOID_TRIANGLES + scales(form, fruit.aspect) as usize * WEDGE_TRIANGLES
        }
        (Form::Fruit(form), Shape::Fruit(fruit)) if fruit.cone >= 0.5 => {
            SPINDLE_TRIANGLES + cone_scales(form, fruit) as usize * face_triangles(fruit.face)
        }
        (Form::Fruit(_), _) => ELLIPSOID_TRIANGLES,
        (Form::Spikelet(_), Shape::Spikelet(spikelet)) => crate::spikelets::triangles(spikelet, 0),
        (Form::Thorn, _) => THORN_TRIANGLES,
        (Form::Prickles(_), _) => crate::prickles::PRICKLE_TRIANGLES,
        (Form::Shoot, Shape::Needles(needles)) => crate::shoots::triangles(needles, 0),
        _ => 0,
    }
}

/// The share of the prickles and aciculi between the nodes that organs of
/// type `index` draw so that, with one prickle at each organ, they fit
/// [`crate::prickles::BUDGET`]: 1 where all fit.
#[allow(clippy::cast_precision_loss)]
fn prickle_share(graph: &PlantGraph, index: usize, form: &crate::prickles::PrickleForm) -> f64 {
    let mut nodes = 0usize;
    let mut between = 0.0;
    for organ in graph
        .organs
        .iter()
        .filter(|organ| usize::from(organ.organ) == index)
    {
        nodes += 1;
        if let Some(segment) = organ
            .segment
            .and_then(|segment| graph.segments.get(segment as usize))
        {
            let (prickles, bristles) =
                form.on((segment.end - segment.start).length(), segment.radius);
            between += prickles * crate::prickles::BETWEEN_TRIANGLES as f64
                + bristles * crate::prickles::NEEDLE_TRIANGLES as f64;
        }
    }
    let room =
        crate::prickles::BUDGET.saturating_sub(nodes * crate::prickles::PRICKLE_TRIANGLES) as f64;
    if between <= room { 1.0 } else { room / between }
}

/// Triangles a thorn takes: a frustum's sides.
const THORN_TRIANGLES: usize = 16;

/// Triangles a cone scale takes.
pub(crate) const WEDGE_TRIANGLES: usize = 4;

/// The scales on a cone of width over length `aspect`: the form's, or
/// more on a slender cone, from 16 to 40.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn scales(form: &FruitForm, aspect: f64) -> u32 {
    if form.scales > 0 {
        form.scales
    } else {
        (12.0 / aspect.clamp(0.05, 2.0)).clamp(16.0, 40.0) as u32
    }
}

/// The coarse step at which the organs of type `index` in `graph` fit
/// [`TYPE_TRIANGLES`] drawn as solids: 0, or [`COARSER_FLOWERS`] for laid-out
/// flowers (a step that keeps every whorl, with fewer facets); `None` when
/// they do not fit.
#[must_use]
pub fn step(graph: &PlantGraph, look: &Look, index: usize) -> Option<usize> {
    let organs = graph
        .organs
        .iter()
        .filter(|organ| usize::from(organ.organ) == index)
        .count();
    let laid = look.inflorescence.is_none()
        && matches!(
            (&look.form, &look.shape),
            (Form::Flower(_), Shape::Flower(flower)) if !flower.is_plain()
        );
    let steps = if laid { COARSER_FLOWERS } else { 0 };
    (0..=steps).find(|&coarse| organs * triangles_per_organ(look, coarse) <= TYPE_TRIANGLES)
}

/// Whether the organs of type `index` in `graph` fit [`TYPE_TRIANGLES`]
/// drawn as solids, at the step [`step`] gives.
#[must_use]
pub fn fits(graph: &PlantGraph, look: &Look, index: usize) -> bool {
    step(graph, look, index).is_some()
}

/// The coarsest step a laid-out flower's solids take on a level before
/// it keeps its cards: the first step, which draws each whorl as the fine
/// flower does with fewer facets; from the second the flower is reduced
/// to kites (`crate::flowers::draw_reduced`).
pub const COARSER_FLOWERS: usize = 1;

/// [`build`] with each type at its own coarse step (`steps`, by index:
/// `None` leaves it out), as [`step`] gives.
pub fn build_stepped(
    graph: &PlantGraph,
    looks: &[Look],
    steps: &[Option<usize>],
    colour: &dyn Fn(&Look, &GraphOrgan) -> [f32; 3],
    mesh: &mut Mesh,
) {
    let most = steps.iter().flatten().copied().max().unwrap_or(0);
    for coarse in 0..=most {
        let solid: Vec<bool> = steps.iter().map(|step| *step == Some(coarse)).collect();
        if solid.contains(&true) {
            build_coarse(graph, looks, &solid, colour, coarse, mesh);
        }
    }
}

/// Draw every organ of `graph` whose type is in `solid` (by index) and
/// whose look has a flower or fruit form into `mesh`, coloured by `colour`
/// (the organ's card colour).
pub fn build(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    colour: &dyn Fn(&Look, &GraphOrgan) -> [f32; 3],
    mesh: &mut Mesh,
) {
    build_coarse(graph, looks, solid, colour, 0, mesh);
}

/// [`build`], with clusters and fruit drawn `coarse` steps coarser
/// (`crate::fruit::COARSER`), for part meshes that must fit a budget.
#[allow(clippy::too_many_lines)]
pub(crate) fn build_coarse(
    graph: &PlantGraph,
    looks: &[Look],
    solid: &[bool],
    colour: &dyn Fn(&Look, &GraphOrgan) -> [f32; 3],
    coarse: usize,
    mesh: &mut Mesh,
) {
    // Each organ's vertices are borne by its segment: marked when the
    // next organ starts, and after the last.
    let mut drawn: Option<(usize, u32)> = None;
    let mut thinned: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for organ in &graph.organs {
        if let Some((first, segment)) = drawn.take() {
            mesh.bear(first, segment);
        }
        let index = usize::from(organ.organ);
        let Some(look) = looks.get(index).map(|look| look.of(organ.id)) else {
            continue;
        };
        if !solid.get(index).copied().unwrap_or(false) || look.solid.is_some() {
            continue;
        }
        let first = mesh.positions.len();
        let first_index = mesh.indices.len();
        drawn = Some((first, organ.segment.unwrap_or(crate::mesh::GROUNDED)));
        let painted = colour(look, organ);
        let axis = organ.heading.normalize_or(Vec3::Y);
        let side = organ.left.normalize_or(any_perpendicular(axis));
        let part = Part {
            born: organ.born,
            shed: organ.shed,
            coarse,
        };
        if let Some(cluster) = &look.inflorescence {
            crate::clusters::draw(organ, (axis, side), look, cluster, painted, part, mesh);
            continue;
        }
        match (&look.form, &look.shape) {
            (Form::Flower(form), Shape::Flower(flower)) if flower.is_plain() => {
                let petals = Petals {
                    count: flower.petals.max(1),
                    width: flower.petal_share(),
                    centre: flower.centre,
                    pointed: flower.point,
                };
                draw_flower(
                    organ,
                    (axis, side),
                    (form, flower.tube),
                    &petals,
                    painted,
                    look.accent,
                    part,
                    mesh,
                );
            }
            (Form::Flower(form), Shape::Flower(flower)) => {
                // Laid out whorl by whorl (`crate::flowers`), its angles
                // from its adaxial side where it is zygomorphic.
                let spin =
                    unit(mix64(organ.id)) * std::f64::consts::TAU / f64::from(flower.petals.max(1));
                crate::flowers::draw(
                    &crate::flowers::layout(flower, form, organ.size * 0.5, spin),
                    crate::flowers::Frame::new(
                        organ.position,
                        axis,
                        organ.heading.cross(organ.left),
                        side,
                        flower.is_zygomorphic(),
                    ),
                    crate::flowers::Colours {
                        petal: painted,
                        calyx: with_shading(
                            flower.calyx.unwrap_or(crate::flowers::CALYX),
                            look.colour,
                            painted,
                        ),
                        accent: look.accent,
                        filament: flower.filament.unwrap_or(look.accent),
                        anther: flower.anther_colour.unwrap_or(look.accent),
                        ovary: flower.pistil.unwrap_or(look.accent),
                        base: flower
                            .base_colour
                            .map(|base| with_shading(base, look.colour, painted)),
                        base_length: flower.base_length,
                        star: flower
                            .star_colour
                            .map(|star| with_shading(star, look.colour, painted)),
                        stripe: flower
                            .stripe_colour
                            .map(|stripe| with_shading(stripe, look.colour, painted)),
                        back: flower
                            .back_colour
                            .map(|back| with_shading(back, look.colour, painted)),
                        back_base: flower
                            .back_base_colour
                            .map(|back| with_shading(back, look.colour, painted)),
                        back_base_length: flower.back_base_length,
                        tube: flower
                            .tube_colour
                            .map(|tube| with_shading(tube, look.colour, painted)),
                        stigma: flower.stigma.or(flower.filament).unwrap_or(look.accent),
                        pericarpel: with_shading(
                            flower
                                .pericarpel_colour
                                .or(flower.calyx)
                                .unwrap_or(crate::flowers::CALYX),
                            look.colour,
                            painted,
                        ),
                        dress: crate::fruit::Dress {
                            felt: flower.pericarpel_felt,
                            bristles: flower.pericarpel_bristles,
                        },
                    },
                    part,
                    mesh,
                );
            }
            (Form::Flower(form), Shape::Head(head)) => {
                // The disc in the accent, the bracts in their own colour,
                // both darkened with the organ.
                let colours = crate::capitula::Colours::of(
                    head,
                    painted,
                    with_shading(look.accent, look.colour, painted),
                    (look.colour, painted),
                );
                crate::capitula::draw(
                    head,
                    form,
                    crate::capitula::Frame {
                        foot: organ.position,
                        axis,
                        side,
                        radius: organ.size * 0.5,
                    },
                    unit(mix64(organ.id)) * std::f64::consts::TAU,
                    colours,
                    part,
                    mesh,
                );
            }
            (Form::Fruit(form), Shape::Fruit(fruit)) if crate::fruit::is_single(fruit) => {
                draw_fruit(organ, axis, side, form, fruit, painted, part, mesh);
            }
            (Form::Fruit(form), Shape::Fruit(fruit)) => {
                // Unripe fruit, stalks and cups in the accent colour,
                // darkened with the organ as its colour is.
                let accent = with_shading(look.accent, look.colour, painted);
                crate::fruit::draw(organ, axis, side, form, fruit, painted, accent, part, mesh);
            }
            (Form::Spikelet(_), Shape::Spikelet(spikelet)) => {
                crate::spikelets::draw(
                    spikelet,
                    crate::capitula::Frame {
                        foot: organ.position,
                        axis,
                        side,
                        radius: organ.size,
                    },
                    crate::spikelets::Colours::of(
                        painted,
                        with_shading(look.accent, look.colour, painted),
                    ),
                    part,
                    mesh,
                );
            }
            (Form::Shoot, Shape::Needles(needles)) => {
                // The axes in the accent colour, shaded with the organ.
                let accent = with_shading(look.accent, look.colour, painted);
                crate::shoots::draw(organ, look, needles, painted, accent, part, mesh);
            }
            (Form::Prickles(form), shape) => {
                let width = match shape {
                    Shape::Blade(blade) => blade.aspect,
                    _ => 0.3,
                };
                let segment = organ
                    .segment
                    .and_then(|segment| graph.segments.get(segment as usize));
                let kept = *thinned
                    .entry(index)
                    .or_insert_with(|| prickle_share(graph, index, form));
                let (between, bristles) = segment.map_or((0.0, 0.0), |segment| {
                    form.on((segment.end - segment.start).length(), segment.radius)
                });
                crate::prickles::draw_organ(
                    organ,
                    segment,
                    (width, look.bend.droop.to_radians()),
                    (between * kept, bristles * kept),
                    painted,
                    part,
                    mesh,
                );
            }
            (Form::Thorn, shape) => {
                let width = match shape {
                    Shape::Blade(blade) => blade.aspect,
                    _ => 0.08,
                };
                // From the stem's surface, not its axis, so a thick
                // stem does not bury it.
                let segment = organ
                    .segment
                    .and_then(|segment| graph.segments.get(segment as usize));
                let (_, _, foot) = crate::prickles::foot(organ, segment);
                frustum(
                    foot,
                    axis,
                    side,
                    organ.size,
                    organ.size * width * 0.5,
                    0.0,
                    painted,
                    part,
                    mesh,
                );
            }
            _ => continue,
        }
        leaves::smooth_normals(mesh, first, first_index);
    }
    if let Some((first, segment)) = drawn {
        mesh.bear(first, segment);
    }
}

/// When a part appears and is shed.
#[derive(Clone, Copy)]
pub(crate) struct Part {
    pub(crate) born: f64,
    pub(crate) shed: Option<f64>,
    /// Steps coarser than its own detail it is drawn (part meshes that
    /// must fit a budget, `crate::parts`); 0 in a plant's mesh.
    pub(crate) coarse: usize,
}

impl Part {
    pub(crate) fn leaf(
        self,
        base: Vec3,
        heading: Vec3,
        left: Vec3,
        length: f64,
        width: f64,
        colour: [f32; 3],
    ) -> Leaf {
        Leaf {
            underside: None,
            base,
            heading,
            left,
            length,
            width,
            colour,
            born: self.born,
            shed: self.shed,
        }
    }
}

/// A flower's petals from its template.
struct Petals {
    count: u32,
    width: f64,
    centre: f64,
    pointed: f64,
}

pub(crate) const DOME_TRIANGLES: usize = 2 * 8 * 3;
// Two fans at the poles and a band between each pair of the six inner rings.
pub(crate) const ELLIPSOID_TRIANGLES: usize = 2 * 10 * (7 - 1);

#[allow(clippy::too_many_arguments)]
/// A plain flower ([`crate::looks::Flower::is_plain`]) as before P5: its
/// petals round the axis, a floral cup `tube` times its radius long under
/// them, and its centre dome.
fn draw_flower(
    organ: &GraphOrgan,
    (axis, side): (Vec3, Vec3),
    (form, tube): (&FlowerForm, f64),
    petals: &Petals,
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let radius = organ.size * 0.5;
    if radius <= 0.0 {
        return;
    }
    let centre_radius = (petals.centre * radius).clamp(0.0, radius * 0.9);
    let start = 0.6 * centre_radius;
    let tube = tube.max(0.0) * radius;
    let base = organ.position + axis * tube;
    let length = (radius - start).max(radius * 0.1);
    let n = f64::from(petals.count);
    let width =
        (petals.width * std::f64::consts::PI * (start + radius) / n).clamp(length * 0.08, length);
    let cup = math::radians(form.cup);
    let other = axis.cross(side);
    let turn = unit(mix64(organ.id)) * std::f64::consts::TAU / n;
    let petal = SolidLeaf {
        section: Section::Flat,
        thickness: 0.05,
        fold: 0.0,
        widest: 0.55,
        base: 0.35,
        taper: if petals.pointed >= 0.5 { 1.3 } else { 0.45 },
        tip: 0.0,
        arch: form.curl,
        arch_from: form.curl_from,
        twist: 0.0,
        margin: None,
        teeth: None,
        spine: None,
        levels: 1,
    };
    for k in 0..petals.count {
        let angle = turn + std::f64::consts::TAU * f64::from(k) / n;
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let heading = (radial * math::cos(cup) + axis * math::sin(cup)).normalize_or(radial);
        // The petal's face toward the flower's axis, so its curl bends it
        // outward and back.
        let mut left = axis.cross(radial).normalize_or(side);
        if heading.cross(left).dot(axis) < 0.0 {
            left = -left;
        }
        let leaf = part.leaf(base + radial * start, heading, left, length, width, colour);
        let (stations, across) = leaves::detail_of(0, length, width);
        let (most_stations, most_across) = match part.coarse {
            0 => (6, 2),
            1 => (4, 1),
            _ => (3, 1),
        };
        leaf.draw(
            &petal,
            stations.min(most_stations),
            across.min(most_across),
            mesh,
        );
    }
    if tube > 0.0 {
        frustum(
            organ.position,
            axis,
            side,
            tube,
            start * 0.6,
            start,
            colour,
            part,
            mesh,
        );
    }
    if centre_radius > 0.0 {
        dome(
            base,
            axis,
            side,
            centre_radius,
            centre_radius * 0.5,
            accent,
            part,
            mesh,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_fruit(
    organ: &GraphOrgan,
    axis: Vec3,
    side: Vec3,
    form: &FruitForm,
    fruit: &Fruit,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    if length <= 0.0 {
        return;
    }
    let aspect = fruit.aspect;
    let half_length = length * 0.5;
    let half_width = length * aspect.clamp(0.05, 2.0) * 0.5;
    let centre = organ.position + axis * half_length;
    if fruit.cone < 0.5 {
        ellipsoid(
            centre,
            axis,
            side,
            half_length,
            half_width,
            colour,
            part,
            mesh,
        );
        return;
    }
    if !fruit.is_plain_cone() {
        draw_cone(organ, (axis, side), form, fruit, colour, part, mesh);
        return;
    }
    // A cone: a core and spiral scales over it, pointing toward the tip.
    let core = 0.6;
    ellipsoid(
        centre,
        axis,
        side,
        half_length * 0.95,
        half_width * core,
        shade(colour, 0.8),
        part,
        mesh,
    );
    let count = (scales(form, aspect) >> part.coarse.min(2)).max(8);
    let other = axis.cross(side);
    let golden = math::radians(137.507_764);
    for i in 0..count {
        let t = (f64::from(i) + 0.5) / f64::from(count);
        let z = -1.0 + 2.0 * t;
        let ring = math::sqrt((1.0 - z * z).max(0.0));
        let angle = golden * f64::from(i);
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let on_core =
            centre + axis * (z * half_length * 0.95) + radial * (ring * half_width * core);
        let reach = half_width * (1.0 - core) * 1.6 * (0.4 + 0.6 * ring);
        let tip = on_core + (radial * 0.8 + axis * 0.6).normalize_or(radial) * reach;
        let across = axis.cross(radial).normalize_or(side) * (reach * 0.7);
        wedge(
            on_core - across,
            on_core + across,
            tip,
            radial * (reach * 0.25),
            colour,
            part,
            mesh,
        );
    }
}

/// The share of a cone's half-width its core reaches under scales of
/// `face`: deep under wedges that stand out, near the surface under
/// shields that lie on it.
fn core_share(face: Face) -> f64 {
    match face {
        Face::Wedge | Face::Pointed => 0.6,
        Face::Shield => 0.9,
        Face::Facets | Face::Spine => 0.8,
    }
}

/// Triangles one scale of `face` takes.
pub(crate) fn face_triangles(face: Face) -> usize {
    match face {
        Face::Wedge | Face::Pointed => WEDGE_TRIANGLES,
        Face::Shield | Face::Facets => 6,
        Face::Spine => 6 + WEDGE_TRIANGLES,
    }
}

/// A cone with a profile, a point or faces of its own (cones by sex,
/// 2026-10-17): a core of revolution following the profile
/// ([`Fruit::width_at`]), under its scales on a golden-angle spiral from
/// foot to tip, spaced so each covers an equal share of the surface, and
/// drawn as its face says: a face as wide as the card's `face_width` and
/// [`FACE_HEIGHT`] of that high, lying flush on the core so the outline
/// stays smooth; an open cone's scales stand apart, smaller, showing the
/// core between them.
#[allow(clippy::too_many_arguments)]
fn draw_cone(
    organ: &GraphOrgan,
    (axis, side): (Vec3, Vec3),
    form: &FruitForm,
    fruit: &Fruit,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    let half_width = length * fruit.aspect.clamp(0.05, 2.0) * 0.5;
    let foot = organ.position;
    let core = core_share(fruit.face);
    spindle(
        foot,
        (axis, side),
        length,
        half_width * core,
        |t| fruit.width_at(t),
        shade(colour, 0.75),
        part,
        mesh,
    );
    let other = axis.cross(side);
    let open = fruit.open.clamp(0.0, 1.0);
    for (t, angle, (wide, high)) in
        cone_places(form, fruit, part.coarse.min(2), (half_width, length))
    {
        let width = fruit.width_at(t);
        let radius = half_width * width;
        let radial = side * math::cos(angle) + other * math::sin(angle);
        let across = axis.cross(radial).normalize_or(side);
        // Toward the tip along the surface.
        let up = (axis - radial * axis.dot(radial)).normalize_or(axis);
        let on_core = foot + axis * (t * length) + radial * (radius * core);
        let out = radial * (radius * (1.0 - core) + open * high * 0.8);
        match fruit.face {
            Face::Wedge | Face::Pointed => {
                let pointed = fruit.face == Face::Pointed;
                let reach = if fruit.face_width > 0.0 {
                    high * if pointed { 2.4 } else { 1.8 }
                } else if pointed {
                    (high * 1.8).max(half_width * (1.0 - core) * 1.8 * (0.4 + 0.6 * width))
                } else {
                    half_width * (1.0 - core) * 1.6 * (0.4 + 0.6 * width)
                } * (1.0 - 0.3 * open);
                let base = on_core + out * open;
                let toward = if pointed {
                    radial * 0.45 + up
                } else {
                    radial * 0.8 + up * 0.6
                };
                let tip = base + toward.normalize_or(radial) * reach;
                let half = if fruit.face_width > 0.0 {
                    across * wide
                } else {
                    across * (reach * if pointed { 0.8 } else { 0.7 })
                };
                wedge(
                    base - half,
                    base + half,
                    tip,
                    radial * (reach * 0.25),
                    colour,
                    part,
                    mesh,
                );
            }
            Face::Shield | Face::Facets | Face::Spine => {
                let centre = on_core + out;
                // A shield domes a little; facets meet at a raised point.
                let raise = match fruit.face {
                    Face::Facets => 0.6,
                    Face::Spine => 0.3,
                    _ => 0.15,
                };
                hexagon(
                    centre,
                    (across, up),
                    (wide, high),
                    (radial * (high * raise), fruit.rows >= 0.5),
                    colour,
                    part,
                    mesh,
                );
                if fruit.face == Face::Spine {
                    let spine = (length * fruit.spine_at(t)).max(high * 0.3);
                    face_spine(
                        centre,
                        (across, up, radial),
                        (wide, high),
                        spine,
                        colour,
                        part,
                        mesh,
                    );
                }
            }
        }
    }
}

/// An upturned spine `spine` long from the upper edge of a face round
/// `centre`, `wide` across and `high` up its surface (`radial` out of it).
#[allow(clippy::too_many_arguments)]
fn face_spine(
    centre: Vec3,
    (across, up, radial): (Vec3, Vec3, Vec3),
    (wide, high): (f64, f64),
    spine: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let base = centre + up * (high * 0.6);
    let half = across * (wide * 0.25);
    let tip = base + (up * 0.75 + radial * 0.65).normalize_or(up) * spine;
    wedge(
        base - half,
        base + half,
        tip,
        radial * (high * 0.15),
        shade(colour, 1.1),
        part,
        mesh,
    );
}

/// A scale face's height over its width: a cycad's sporophyll faces are
/// about two thirds as high as they are wide (Whitelock 2002, p. 120-330:
/// Zamia 1.5-2 by 2.5-3 cm, Encephalartos septentrionalis 2-2.7 by 4.5-5,
/// Macrozamia communis 1.5-4.3 by 3-8.5).
pub(crate) const FACE_HEIGHT: f64 = 0.65;

/// A hexagon's area over the product of its half-width and half-height
/// to its corners.
const HEXAGON_AREA: f64 = 2.598_076;

/// Most scales one cone is drawn with: a large Cycas pollen cone's
/// hundreds of microsporophylls.
const MOST_SCALES: f64 = 1200.0;

/// A cone's surface along its length: where each share of it is reached,
/// so scales spaced by it each cover the same area.
struct SurfaceShares {
    /// The surface over `TAU` times the half-width times the length.
    total: f64,
    /// The surface below each of [`SURFACE_STEPS`] even steps of the
    /// length, as a share of the total.
    below: [f64; SURFACE_STEPS + 1],
}

const SURFACE_STEPS: usize = 48;

impl SurfaceShares {
    #[allow(clippy::cast_precision_loss)]
    fn of(fruit: &Fruit) -> Self {
        let mut below = [0.0; SURFACE_STEPS + 1];
        let step = 1.0 / SURFACE_STEPS as f64;
        for k in 0..SURFACE_STEPS {
            let middle = (k as f64 + 0.5) * step;
            below[k + 1] = below[k] + fruit.width_at(middle) * step;
        }
        let total = below[SURFACE_STEPS].max(1e-9);
        for share in &mut below {
            *share /= total;
        }
        Self { total, below }
    }

    /// Where along the cone, 0 at its foot to 1 at its tip, `share` of its
    /// surface lies below.
    #[allow(clippy::cast_precision_loss)]
    fn at(&self, share: f64) -> f64 {
        let k = self
            .below
            .partition_point(|&below| below < share)
            .clamp(1, SURFACE_STEPS);
        let (low, high) = (self.below[k - 1], self.below[k]);
        let within = if high > low {
            (share - low) / (high - low)
        } else {
            0.5
        };
        ((k - 1) as f64 + within.clamp(0.0, 1.0)) / SURFACE_STEPS as f64
    }
}

/// The scales on a cone with a profile or faces of its own: the form's;
/// as many faces `face_width` across as cover its surface; or, without a
/// face width, a count from its aspect as on a plain cone.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn cone_scales(form: &FruitForm, fruit: &Fruit) -> u32 {
    if form.scales > 0 || fruit.face_width <= 0.0 {
        return scales(form, fruit.aspect);
    }
    let surface = std::f64::consts::TAU
        * fruit.aspect.clamp(0.05, 2.0)
        * 0.5
        * SurfaceShares::of(fruit).total;
    if let Some((rows, each)) = cone_rows(form, fruit, 0) {
        return rows * each;
    }
    let face = fruit.face_width * fruit.face_width * fruit.face_height();
    (surface / face).clamp(8.0, MOST_SCALES) as u32
}

/// A cone whose scales stand in vertical rows (`rows`, Zamia's
/// orthostichies): how many rows go round it, an even number so each
/// stands half a face above its neighbours all round, and how many faces
/// each row holds. A row's faces fit together at the cone's widest when
/// they are `face_width` across: rows three quarters of a face apart,
/// faces `face_height` of that high. Each coarser step halves both.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn cone_rows(form: &FruitForm, fruit: &Fruit, coarse: usize) -> Option<(u32, u32)> {
    if form.scales > 0 {
        return None;
    }
    fruit_rows(fruit, coarse)
}

/// [`cone_rows`] for a card, which has no form.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn fruit_rows(fruit: &Fruit, coarse: usize) -> Option<(u32, u32)> {
    if fruit.rows < 0.5 || fruit.face_width <= 0.0 {
        return None;
    }
    let girth = std::f64::consts::PI * fruit.aspect.clamp(0.05, 2.0);
    let rows = (girth / (0.75 * fruit.face_width) / 2.0)
        .round()
        .clamp(2.0, 30.0) as u32
        * 2;
    let each = (0.96 / (fruit.face_height() * fruit.face_width))
        .round()
        .clamp(3.0, 60.0) as u32;
    Some((((rows >> coarse) / 2 * 2).max(4), (each >> coarse).max(2)))
}

/// Where each scale of a cone `half_width` and `length` stands, drawn at
/// coarser step `coarse`: along it, 0 at its foot to 1 at its tip; round
/// it, radians; and its face's half-width and half-height. On a spiral
/// the scales are spaced so each covers an equal share of the surface,
/// overlapping a little so no gap shows, and no wider than the cone where
/// it narrows; in vertical rows as [`row_place`] sets them. An open cone's
/// faces are smaller.
fn cone_places(
    form: &FruitForm,
    fruit: &Fruit,
    coarse: usize,
    (half_width, length): (f64, f64),
) -> Vec<(f64, f64, (f64, f64))> {
    let golden = math::radians(137.507_764);
    let shrink = 1.0 - 0.3 * fruit.open.clamp(0.0, 1.0);
    let height = fruit.face_height();
    let grid = cone_rows(form, fruit, coarse);
    let count = grid.map_or_else(
        || (cone_scales(form, fruit) >> coarse).max(8),
        |(rows, each)| rows * each,
    );
    let along = SurfaceShares::of(fruit);
    let area = std::f64::consts::TAU * half_width * length * along.total / f64::from(count);
    let half_height = math::sqrt(1.15 * area * height / HEXAGON_AREA) * shrink;
    let half_across = half_height / height;
    (0..count)
        .map(|i| {
            if let Some(grid) = grid {
                let (t, angle, (wide, high)) = row_place(i, grid, fruit, (half_width, length));
                return (t, angle, (wide * shrink, high * shrink));
            }
            // Kept off the very ends, where the profile closes.
            let t = along.at(0.02 + 0.96 * (f64::from(i) + 0.5) / f64::from(count));
            let radius = half_width * fruit.width_at(t);
            let fit = (radius.max(0.2 * half_width) / half_across).min(1.0);
            (
                t,
                golden * f64::from(i),
                (half_across * fit, half_height * fit),
            )
        })
        .collect()
}

/// Where face `i` of a cone in vertical rows (`grid`, its rows and the
/// faces each holds) stands: along the cone, 0 at its foot to 1 at its
/// tip; round it, radians; and its half-width and half-height. Row
/// `i % rows` goes round the cone, each of its faces in turn up it, every
/// other row half a face higher; a face is as wide as its row's share of
/// the girth where it stands, and as high as its share of the length.
fn row_place(
    i: u32,
    (rows, each): (u32, u32),
    fruit: &Fruit,
    (half_width, length): (f64, f64),
) -> (f64, f64, (f64, f64)) {
    let (row, k) = (i % rows, i / rows);
    let lift = if row % 2 == 1 { 0.5 } else { 0.0 };
    let t = 0.02 + 0.96 * (f64::from(k) + 0.5 + lift) / (f64::from(each) + 0.5);
    let radius = half_width * fruit.width_at(t);
    let wide = std::f64::consts::TAU * radius / (1.5 * f64::from(rows)) * 1.04;
    let high = length * 0.96 / (f64::from(each) + 0.5) * 0.5 * 1.04;
    (
        t,
        std::f64::consts::TAU * f64::from(row) / f64::from(rows),
        (wide, high.min(wide * 1.2)),
    )
}

/// How far into its face a point of a cone card in vertical rows lies, 1
/// at a face's middle to 0 at its edge: `t` along the cone, `u` across
/// it from -1 to 1 seen from the side, the cone's `rows` and the faces
/// `each` row holds.
pub(crate) fn row_face(t: f64, u: f64, (rows, each): (u32, u32)) -> f64 {
    // Round the cone from the side: where the point stands, in rows.
    let around = math::asin(u.clamp(-1.0, 1.0)) / std::f64::consts::TAU * f64::from(rows);
    let row = around.round();
    let lift = if row.rem_euclid(2.0) > 0.5 { 0.5 } else { 0.0 };
    let up = (t - 0.02) / 0.96 * (f64::from(each) + 0.5) - lift;
    let dy = (up - up.floor() - 0.5).abs() * 2.0;
    let dx = (around - row).abs() * 2.0 / 1.33;
    // A flat-topped hexagon: its flat edges above and below, its sides
    // slanting to corners at either hand.
    (1.0 - dy.max(dx + 0.5 * dy)).clamp(0.0, 1.0)
}

/// A hexagonal face round `centre`, spanned by `across` and `up`,
/// reaching `wide` and `high` along them to its corners, its middle raised
/// by `raise`: six triangles. A `flat` one has its corners to the sides
/// and flat edges `high` above and below, so faces in vertical rows, each
/// row half a face above the next, fit together.
fn hexagon(
    centre: Vec3,
    (across, up): (Vec3, Vec3),
    (wide, high): (f64, f64),
    (raise, flat): (Vec3, bool),
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let leaf = part.leaf(centre, up, across, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    mesh.push(leaf.vertex(centre + raise, shade(colour, 1.1)));
    let flat_corners = [
        (1.0, 0.0),
        (0.5, 1.0),
        (-0.5, 1.0),
        (-1.0, 0.0),
        (-0.5, -1.0),
        (0.5, -1.0),
    ];
    for (k, (x, y)) in (0..6).zip(flat_corners) {
        let corner = if flat {
            across * (wide * x) + up * (high * y)
        } else {
            let angle = std::f64::consts::FRAC_PI_3 * f64::from(k) + std::f64::consts::FRAC_PI_6;
            across * (wide * math::cos(angle)) + up * (high * math::sin(angle))
        };
        mesh.push(leaf.vertex(centre + corner, colour));
    }
    for k in 0..6 {
        mesh.indices
            .extend_from_slice(&[first, first + 1 + k, first + 1 + (k + 1) % 6]);
    }
}

/// Rings a cone's core of revolution is drawn with, from foot to tip, and
/// facets round each.
const SPINDLE_RINGS: u32 = 9;
const SPINDLE_AROUND: u32 = 10;
/// Two fans at the ends and a band between each pair of inner rings.
pub(crate) const SPINDLE_TRIANGLES: usize =
    2 * SPINDLE_AROUND as usize * (SPINDLE_RINGS as usize - 1);

/// A solid of revolution from `foot` along `axis`, `length` long, its
/// radius at `t` (0 at the foot to 1 at the tip) `radius` times
/// `profile(t)`.
#[allow(clippy::too_many_arguments)]
fn spindle(
    foot: Vec3,
    (axis, side): (Vec3, Vec3),
    length: f64,
    radius: f64,
    profile: impl Fn(f64) -> f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let other = axis.cross(side);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let leaf = part.leaf(foot, axis, side, 0.0, 0.0, colour);
    mesh.push(leaf.vertex(foot, colour));
    for r in 1..SPINDLE_RINGS {
        let t = f64::from(r) / f64::from(SPINDLE_RINGS);
        let out = profile(t) * radius;
        for k in 0..SPINDLE_AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(SPINDLE_AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(foot + axis * (t * length) + radial * out, colour));
        }
    }
    let top = mesh.push(leaf.vertex(foot + axis * length, colour));
    let at = |r: u32, k: u32| first + 1 + (r - 1) * SPINDLE_AROUND + k % SPINDLE_AROUND;
    for k in 0..SPINDLE_AROUND {
        mesh.indices
            .extend_from_slice(&[first, at(1, k + 1), at(1, k)]);
        mesh.indices.extend_from_slice(&[
            top,
            at(SPINDLE_RINGS - 1, k),
            at(SPINDLE_RINGS - 1, k + 1),
        ]);
    }
    for r in 1..SPINDLE_RINGS - 1 {
        for k in 0..SPINDLE_AROUND {
            let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
}

/// An ellipsoid of half-length `a` along `axis` and half-width `b`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ellipsoid(
    centre: Vec3,
    axis: Vec3,
    side: Vec3,
    a: f64,
    b: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 10;
    const RINGS: u32 = 7;
    let other = axis.cross(side);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let leaf = part.leaf(centre, axis, side, 0.0, 0.0, colour);
    mesh.push(leaf.vertex(centre - axis * a, colour));
    for r in 1..RINGS {
        let phi = std::f64::consts::PI * f64::from(r) / f64::from(RINGS);
        let along = -math::cos(phi) * a;
        let out = math::sin(phi) * b;
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(centre + axis * along + radial * out, colour));
        }
    }
    let top = mesh.push(leaf.vertex(centre + axis * a, colour));
    let at = |r: u32, k: u32| first + 1 + (r - 1) * AROUND + k % AROUND;
    for k in 0..AROUND {
        mesh.indices
            .extend_from_slice(&[first, at(1, k + 1), at(1, k)]);
        mesh.indices
            .extend_from_slice(&[top, at(RINGS - 1, k), at(RINGS - 1, k + 1)]);
    }
    for r in 1..RINGS - 1 {
        for k in 0..AROUND {
            let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
}

/// A dome over `base` along `axis`: `radius` wide, `height` tall.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dome(
    base: Vec3,
    axis: Vec3,
    side: Vec3,
    radius: f64,
    height: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 8;
    const RINGS: u32 = 3;
    let other = axis.cross(side);
    let leaf = part.leaf(base, axis, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for r in 0..RINGS {
        let phi = std::f64::consts::FRAC_PI_2 * f64::from(r) / f64::from(RINGS);
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(
                base + radial * (radius * math::cos(phi)) + axis * (height * math::sin(phi)),
                colour,
            ));
        }
    }
    let top = mesh.push(leaf.vertex(base + axis * height, colour));
    let at = |r: u32, k: u32| first + r * AROUND + k % AROUND;
    for r in 0..RINGS - 1 {
        for k in 0..AROUND {
            let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
            mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        }
    }
    for k in 0..AROUND {
        mesh.indices
            .extend_from_slice(&[at(RINGS - 1, k), at(RINGS - 1, k + 1), top]);
    }
}

/// An open frustum along `axis` from `base`, `length` long, from radius
/// `r0` to `r1`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn frustum(
    base: Vec3,
    axis: Vec3,
    side: Vec3,
    length: f64,
    r0: f64,
    r1: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const AROUND: u32 = 8;
    let other = axis.cross(side);
    let leaf = part.leaf(base, axis, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for (along, radius) in [(0.0, r0), (length, r1)] {
        for k in 0..AROUND {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(AROUND);
            let radial = side * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(base + axis * along + radial * radius, colour));
        }
    }
    for k in 0..AROUND {
        let (a0, a1) = (first + k, first + (k + 1) % AROUND);
        let (b0, b1) = (a0 + AROUND, a1 + AROUND);
        mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
    }
}

/// A scale: a wedge on the edge `a`–`b` reaching to `tip`, `lift` thick.
pub(crate) fn wedge(
    a: Vec3,
    b: Vec3,
    tip: Vec3,
    lift: Vec3,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let leaf = part.leaf(a, Vec3::Y, Vec3::X, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    let middle = (a + b) * 0.5 + lift;
    for corner in [a, b, tip, middle] {
        mesh.push(leaf.vertex(corner, shade(colour, 1.0)));
    }
    let v = |k: u32| first + k;
    mesh.indices.extend_from_slice(&[
        v(0),
        v(1),
        v(2),
        v(1),
        v(3),
        v(2),
        v(3),
        v(0),
        v(2),
        v(0),
        v(3),
        v(1),
    ]);
}

pub(crate) fn shade(colour: [f32; 3], factor: f32) -> [f32; 3] {
    colour.map(|channel| (channel * factor).clamp(0.0, 1.0))
}

/// `accent` darkened or lightened as `painted` is from `colour`, by
/// luminance: an organ's second colour shaded with the organ.
pub(crate) fn with_shading(accent: [f32; 3], colour: [f32; 3], painted: [f32; 3]) -> [f32; 3] {
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    shade(accent, luminance(painted) / luminance(colour).max(1e-4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::looks::{Flower, Fruit};

    fn organ(size: f64) -> GraphOrgan {
        GraphOrgan {
            id: 7,
            organ: 0,
            segment: None,
            position: Vec3::ZERO,
            heading: Vec3::Y,
            left: Vec3::X,
            size,
            born: 2.0,
            shed: Some(3.0),
            light: 1.0,
            within: None,
            adult: None,
        }
    }

    #[test]
    fn a_plain_flower_has_its_petals_cup_and_centre_within_its_size() {
        let mut mesh = Mesh::default();
        let form = FlowerForm::default();
        let petals = Petals {
            count: 5,
            width: 1.0,
            centre: 0.2,
            pointed: 0.0,
        };
        let part = Part {
            born: 2.0,
            shed: Some(3.0),
            coarse: 0,
        };
        draw_flower(
            &organ(0.05),
            (Vec3::Y, Vec3::X),
            (&form, 0.2),
            &petals,
            [0.9, 0.9, 0.9],
            [0.9, 0.7, 0.1],
            part,
            &mut mesh,
        );
        assert!(mesh.triangle_count() > 5 * 8, "petals, a cup and a dome");
        for position in &mesh.positions {
            let p = Vec3::new(
                f64::from(position[0]),
                f64::from(position[1]),
                f64::from(position[2]),
            );
            assert!(p.length() <= 0.05, "within the flower's diameter: {p:?}");
            assert!(p.y >= -0.01, "the flower opens along its axis: {p:?}");
        }
        assert!(mesh.births.iter().all(|&b| (b - 2.0).abs() < 1e-6));
    }

    #[test]
    fn a_fruit_is_an_ellipsoid_along_its_stalk_and_a_cone_has_scales() {
        let part = Part {
            born: 0.0,
            shed: None,
            coarse: 0,
        };
        let mut berry = Mesh::default();
        draw_fruit(
            &organ(0.02),
            Vec3::Y,
            Vec3::X,
            &FruitForm::default(),
            &Fruit {
                aspect: 0.8,
                ..Fruit::default()
            },
            [0.3, 0.1, 0.4],
            part,
            &mut berry,
        );
        assert_eq!(berry.triangle_count(), ELLIPSOID_TRIANGLES);
        let top = berry
            .positions
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!(
            (top - 0.02).abs() < 1e-6,
            "the fruit reaches its length along the stalk"
        );
        let mut cone = Mesh::default();
        draw_fruit(
            &organ(0.08),
            Vec3::Y,
            Vec3::X,
            &FruitForm::default(),
            &Fruit {
                aspect: 0.45,
                cone: 1.0,
                ..Fruit::default()
            },
            [0.4, 0.25, 0.1],
            part,
            &mut cone,
        );
        assert!(cone.triangle_count() >= ELLIPSOID_TRIANGLES + 20 * 4);
    }

    #[test]
    fn a_thorn_is_a_cone_its_organ_s_length() {
        let mut graph = PlantGraph {
            age: 1.0,
            height: 1.0,
            segments: Vec::new(),
            organs: vec![organ(0.03)],
            structures: Vec::new(),
        };
        graph.organs[0].born = 0.0;
        graph.organs[0].shed = None;
        let looks = crate::looks::resolve(
            [("thorn", crate::lsys::OrganKind::Leaf)],
            &std::collections::BTreeMap::from([(
                "thorn".to_string(),
                crate::looks::OrganLook {
                    hairs: None,
                    hairs_beneath: None,
                    underside: None,
                    variegation: None,
                    sori: None,
                    roll: 0.0,
                    insertion: crate::looks::Insertion::Marginal,
                    shape: Shape::Blade(crate::looks::Blade {
                        taper: 1.0,
                        aspect: 0.12,
                        foot: 0.0,
                    }),
                    colour: None,
                    shade: None,
                    accent: None,
                    face_up: 0.0,
                    solid: None,
                    bend: None,
                    form: Some(Form::Thorn),
                    season: None,
                    leafing: None,
                    families: None,
                    inflorescence: None,
                },
            )]),
            [0.3; 3],
            [0.1; 3],
        );
        assert!(fits(&graph, &looks[0], 0));
        let mut mesh = Mesh::default();
        build(&graph, &looks, &[true], &|look, _| look.colour, &mut mesh);
        assert_eq!(mesh.triangle_count(), THORN_TRIANGLES);
        let top = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((top - 0.03).abs() < 1e-6, "the tip at the thorn's length");
        assert!(Form::Thorn.solid_at(0) && !Form::Thorn.solid_at(1));
    }

    #[test]
    fn forms_default_by_shape_and_level() {
        assert!(matches!(
            Form::default_for(&Shape::Flower(Flower::default())),
            Form::Flower(_)
        ));
        assert!(matches!(
            Form::default_for(&Shape::Fruit(Fruit::default())),
            Form::Fruit(_)
        ));
        let form = Form::default_for(&Shape::Fruit(Fruit::default()));
        assert!(form.solid_at(0) && !form.solid_at(1));
        assert!(!Form::Card.solid_at(0));
    }
}
