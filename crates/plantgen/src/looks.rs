//! How a species looks: the cards its organs are drawn with and the
//! surface of its wood.
//!
//! Every organ type a program declares is drawn as cards cut out by one
//! template (see [`crate::templates`]). A species chooses, per organ name in
//! `appearance.organs`, the template's shape and its parameters, the
//! colours, and how the cards turn toward the sky. Organs the spec does not
//! mention get a default look for their kind, so a program's organ types
//! always resolve to one [`Look`] each, in declaration order, and the
//! index of an organ type is the index of its template in the package's
//! organ atlas.
//!
//! Lengths in shape parameters are fractions of the card's length; the
//! organ's size in the program is the card's length in metres.
//!
//! Wood is a tube per axis (see [`crate::mesh`]); [`Flare`], [`Ridges`] and
//! [`Moss`] shape and colour it.
//!
//! An organ that flowers and fruits may have a [`Season`]: the days of the
//! year it is a bud, a flower, unripe and ripe fruit, and when it falls,
//! with its looks in bud and in flower. Its stage is then a parameter, the
//! day a package is grown for ([`resolve_on`]), never fixed to one time of
//! year; a later phenology stage (plant roadmap P5) sets the day from the
//! date and the local climate.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lsys::OrganKind;

/// The look of one organ type, as a species spec gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganLook {
    /// The template and its shape parameters.
    pub shape: Shape,
    /// The organ is one cluster of units, each drawn with `shape`: a
    /// flower, a head or one fruit (spec rework L1, `crate::clusters`).
    /// Without it the organ is one unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inflorescence: Option<Inflorescence>,
    /// Colour in full light; the appearance's foliage colour if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[f32; 3]>,
    /// Colour in deep shade; the appearance's foliage shade colour if
    /// absent, or for a look with its own colour, that colour darkened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shade: Option<[f32; 3]>,
    /// Second colour, painted where the template marks it: a flower's
    /// centre, a leaf's petiole. The main colour if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<[f32; 3]>,
    /// Colour of its lower (abaxial) face in full light, where it differs
    /// from the upper: the pale, waxy or hairy undersides of many leaves
    /// (a royal palm's pinnae "paler below", whitebeam, silver maple). The
    /// same as the upper face if absent. A card's upper face is the one
    /// that looks more toward the sky (see [`crate::templates::Face`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underside: Option<[f32; 3]>,
    /// Patches of another colour on its blades ([`Variegation`]): pale
    /// margins, centres, stripes, bands or spots, different on every leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variegation: Option<Variegation>,
    /// How far a card that lies along its organ turns flat to face the
    /// sky, from 0 (as the program placed it) to 1 (level). Broad leaves
    /// held toward the light form the layered crowns of shade-tolerant
    /// trees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub face_up: f64,
    /// Degrees a card that lies along its organ turns about its own
    /// length before it turns to the sky, from -180 to 180: 90 sets a
    /// sword leaf's blade edgewise to the stem, its faces in the plane of
    /// its fan, as unifacial leaves are borne (Kaplan; irises).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub roll: f64,
    /// Where a leaf's petiole meets its blade ([`Insertion`]): at the
    /// blade's base in its plane, one card as before, unless set.
    #[serde(default, skip_serializing_if = "Insertion::is_marginal")]
    pub insertion: Insertion,
    /// Draw the organ as a solid leaf on the nearest levels of detail
    /// (`crate::leaves`); its card stands for it farther away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid: Option<crate::leaves::SolidLeaf>,
    /// How the organ's card bends on the levels that keep a card per
    /// organ (`crate::bend`); the shape's default if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<crate::bend::Bend>,
    /// How a flower, head or fruit is drawn as a solid on the nearest
    /// levels of detail (`crate::blooms`); the shape's default if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<crate::blooms::Form>,
    /// Its year, for an organ that flowers and fruits: what it is on each
    /// day, and how it looks then. Without one it looks the same all year.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<Season>,
    /// Its leaves' families (plant roadmap P4): sun and shade leaves, and
    /// juvenile leaves on a young plant. Without them every leaf is alike.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub families: Option<Families>,
    /// The hairs on its face (its indumentum, [`Hairs`]); a bare face if
    /// absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hairs: Option<Hairs>,
}

/// The hairs on an organ's face, its indumentum (Harris and Harris, *Plant
/// Identification Terminology*: glabrous, pubescent, pilose, villous,
/// tomentose and the rest), as a renderer draws them: a sheen and a soft
/// grey cast on its card where they are too fine to see one by one, a
/// fringe along its edge, and up close, strands of their own
/// (`crate::hairs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hairs {
    /// How much of the face the hairs hide, 0 (bare) to 1 (a felt that
    /// hides it).
    pub density: f64,
    /// A hair's length, metres.
    pub length: f64,
    /// How a hair stands, 0 (pressed flat, pointing toward the organ's
    /// tip) to 1 (straight out from the face).
    pub lie: f64,
    /// How a hair bends along its length, 0 (straight) to 1 (curled and
    /// tangled, as wool).
    pub curl: f64,
    /// Their colour, linear RGB: white hairs unless stated.
    pub colour: [f32; 3],
    /// How thickly they fringe its edge, 0 to 1, where that differs from
    /// the face's `density`: a ciliate edge on a bare blade.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<f64>,
}

impl Default for Hairs {
    fn default() -> Self {
        Self {
            density: 0.0,
            length: 0.001,
            lie: 0.5,
            curl: 0.0,
            colour: [0.8, 0.8, 0.76],
            edge: None,
        }
    }
}

impl Hairs {
    /// Whether there are any to draw.
    #[must_use]
    pub fn drawn(&self) -> bool {
        (self.density > 0.0 || self.edge.is_some_and(|edge| edge > 0.0)) && self.length > 0.0
    }
}

/// Patches of a second colour on an organ's blades: variegation, where
/// part of a leaf lacks chlorophyll or another pigment masks it, in the
/// patterns horticulture names (marginata, medio-picta, striata, zebrina,
/// maculata), and natural mottling (cyclamen, trout lily). Every card
/// draws its own share of patches, so leaves differ, as on a variegated
/// plant, some nearly plain where `spread` is wide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variegation {
    pub pattern: VariegationPattern,
    /// The patches' colour in full light, linear RGB; darkened in shade
    /// as the organ's colour is.
    pub colour: [f32; 3],
    /// Share of the blade the patches cover on an average leaf, 0 to 1.
    pub amount: f64,
    /// How much that share differs from leaf to leaf, 0 to 2: each card's
    /// share is `amount` plus `spread` times a number of its own from -0.5
    /// to 0.5.
    #[serde(default = "default_variegation_spread")]
    pub spread: f64,
    /// Size of the stripes, bands, spots or mottles, as a share of the
    /// card's length; margins and centres follow the blade's outline.
    #[serde(default = "default_variegation_scale")]
    pub scale: f64,
}

fn default_variegation_spread() -> f64 {
    0.3
}

fn default_variegation_scale() -> f64 {
    0.08
}

/// Where a [`Variegation`]'s patches lie on a blade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariegationPattern {
    /// Along the margin, narrowest at the tips of lobes and teeth
    /// (marginata: variegated hollies, hostas, Dracaena marginata).
    Margin,
    /// In the middle of the blade, the margin left green (medio-picta).
    Centre,
    /// Stripes along the blade (striata: spider plant, variegated grasses
    /// and irises, Pandanus).
    Stripes,
    /// Bands across the blade (zebrina: zebra grass, snake plant).
    Bands,
    /// Round spots (maculata: Aucuba, lungwort, Dieffenbachia).
    Spots,
    /// Irregular mottling and marbling, fine and coarse together
    /// (cyclamen, trout lily, Pothos 'Marble Queen').
    Mottle,
}

impl Variegation {
    /// The share of the blade a card with `seed` (0 to 1, see
    /// [`crate::templates::card_seed`]) covers in patches, 0 to 1.
    #[must_use]
    pub fn share(&self, seed: f64) -> f64 {
        (self.amount + self.spread * (seed - 0.5)).clamp(0.0, 1.0)
    }

    /// How far a texel of pattern value `pattern` (its rank among the
    /// blade's texels, 0 to 1; see `crate::templates`) is painted in the
    /// patches' colour on a card whose patches cover `share` of its blade:
    /// the texels ranked below `share`, with an edge a fiftieth of the
    /// blade wide.
    #[must_use]
    pub fn weight(pattern: f64, share: f64) -> f64 {
        if share <= 0.0 {
            0.0
        } else if share >= 1.0 {
            1.0
        } else {
            ((share - pattern) / VARIEGATION_EDGE + 0.5).clamp(0.0, 1.0)
        }
    }

    /// Whether its values are in range.
    ///
    /// # Errors
    /// Names the first value out of range.
    pub fn validate(&self) -> Result<(), String> {
        within("variegation amount", self.amount, 0.0, 1.0)?;
        within("variegation spread", self.spread, 0.0, 2.0)?;
        within("variegation scale", self.scale, 0.005, 1.0)?;
        for channel in self.colour {
            within("variegation colour", f64::from(channel), 0.0, 1.0)?;
        }
        Ok(())
    }
}

/// Width of a variegated patch's edge, in the pattern's ranks: a
/// fiftieth of the blade fades from one colour to the other.
pub const VARIEGATION_EDGE: f64 = 0.02;

/// Where a leaf's petiole meets its blade (Kaplan, ch. 18: epeltate and
/// peltate blades). A blade that is not `marginal` is drawn on a petiole
/// of its own, a stalk from the node to where it meets the blade, and its
/// card, the petiole left off it, turns flat to the sky about that point:
/// peltate blades, and latently peltate ones, are held near horizontal on
/// their petioles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Insertion {
    /// At the blade's basal margin, the blade going on from the petiole
    /// in its plane: one card, the petiole painted on it.
    #[default]
    Marginal,
    /// At the blade's basal margin, the blade held flat across the
    /// petiole's tip (latently peltate: sagittate aroids, a digitate leaf
    /// whose leaflets leave a gap toward the stem).
    Latent,
    /// Under the blade, inside its outline, like an umbrella (manifestly
    /// peltate): at the middle of an undivided blade, at the centre of a
    /// palmate one's lobes or leaflets (nasturtium, mayapple, a digitate
    /// leaf whose leaflets close a ring round the petiole's tip).
    Peltate,
}

impl Insertion {
    #[must_use]
    pub fn is_marginal(&self) -> bool {
        *self == Self::Marginal
    }
}

/// A leaf drawn on a petiole of its own ([`Insertion`]), in lengths of its
/// card from the card's foot: where the petiole meets the blade, and the
/// point of the blade that sits on the petiole's tip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stalk {
    /// The petiole's length: the share of the card it was painted on.
    pub petiole: f64,
    /// The point of the blade, on its midline, on the petiole's tip.
    pub attach: f64,
}

/// How an organ type's leaves differ with the light they grew in and the
/// plant's age when they grew (plant roadmap P4, phenotype families).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Families {
    /// Sun and shade leaves, 0 (alike) to 1: how far a leaf grown in full
    /// sun is smaller, narrower and more deeply lobed or toothed than the
    /// type's own look, and one grown in deep shade larger, broader and
    /// shallower, by degrees between ([`light_form`], [`Shape::in_light`]).
    pub plasticity: f64,
    /// Juvenile leaves, on a juvenile shoot or, where the program records
    /// no adultness, on a plant younger than their `until` when they grew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub juvenile: Option<Juvenile>,
}

/// An organ type's juvenile leaves (spec rework P6): their shape, grown
/// on a juvenile shoot (an ivy's palmate leaves on its climbing shoots)
/// or, where the program records no adultness (`adult`), while the plant
/// is younger than `until` years (a juniper's needles before its scales,
/// a pine seedling's single needles, a young madrone's toothed leaves).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Juvenile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<f64>,
    pub shape: Shape,
}

/// The ends of the scale leaves acclimate on, in the growth model's light
/// (its share of open-sky light, `light@1`). In that model a dense crown's
/// outermost leaves see only about a third of the open sky, since each
/// light voxel's own leaves shade it, so the scale tops out there; and its
/// innermost leaves are where short shoots stop leafing out, 0.08 to 0.1
/// in the broadleaf programs (`spur_light`), so the scale bottoms out
/// there (crown calibration; 0.04 before, which no crown's leaves
/// reached). A leaf in [`FULL_SUN`] or more is drawn in its look's colour,
/// one in [`DEEP_SHADE`] or less in its shade colour, and between them it
/// moves with the logarithm of its light ([`sunlit`]), as leaves
/// acclimate.
pub const FULL_SUN: f64 = 0.35;
pub const DEEP_SHADE: f64 = 0.08;

/// Where `light` lies on the scale from [`DEEP_SHADE`] (0) to
/// [`FULL_SUN`] (1), by its logarithm.
#[must_use]
pub fn sunlit(light: f64) -> f64 {
    let floor = crate::math::ln(DEEP_SHADE);
    let span = crate::math::ln(FULL_SUN) - floor;
    ((crate::math::ln(light.max(DEEP_SHADE)) - floor) / span).clamp(0.0, 1.0)
}

/// How far toward its sun form (1) or its shade form (-1) a leaf grown in
/// `light` is (spec rework L3): its place on [`sunlit`]'s scale, the
/// middle fifth of which is the type's own look, from 0.1 down all shade
/// and from 0.9 up all sun. A leaf's size follows it smoothly; its shape
/// is drawn in [`BANDS`] steps ([`Forms`]).
#[must_use]
pub fn light_form(light: f64) -> f64 {
    ((sunlit(light) - 0.5) / 0.4).clamp(-1.0, 1.0)
}

/// The steps a leaf type with sun and shade leaves draws them in, from
/// its deepest shade form to its full sun form, its own look the middle
/// one: each step a template of the atlas.
pub const BANDS: usize = 5;

/// The place on [`light_form`]'s scale of the leaves of step `band`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn band_form(band: usize) -> f64 {
    2.0 * band.min(BANDS - 1) as f64 / (BANDS - 1) as f64 - 1.0
}

impl Families {
    /// Check the plasticity, the juvenile leaves and that `shape` has sun
    /// and shade forms if they are asked for.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self, shape: &Shape) -> Result<(), String> {
        within("families plasticity", self.plasticity, 0.0, 1.0)?;
        if self.plasticity > 0.0 && shape.in_light(1.0, self.plasticity).is_none() {
            return Err(format!(
                "families plasticity needs a simple, lobed, palmate or compound leaf or a sprig, found {}",
                shape.name()
            ));
        }
        if let Some(juvenile) = &self.juvenile {
            if let Some(until) = juvenile.until {
                within("families juvenile until", until, 0.1, 200.0)?;
            }
            juvenile.shape.validate()?;
            if matches!(
                juvenile.shape,
                Shape::Flower(_)
                    | Shape::Head(_)
                    | Shape::Umbel(_)
                    | Shape::Spike(_)
                    | Shape::Panicle(_)
                    | Shape::Spikelet(_)
                    | Shape::Fruit(_)
            ) {
                return Err(format!(
                    "families juvenile shape must be foliage, found {}",
                    juvenile.shape.name()
                ));
            }
        }
        Ok(())
    }
}

/// Where an organ type's leaves of each family are drawn (plant roadmap
/// P4, spec rework L3): the indices among the plant's looks of the looks
/// its leaves wear in each of the [`BANDS`] steps of light, from deep
/// shade to full sun (the middle one its own), its plasticity, which sizes
/// them, and its juvenile look with the plant age it is grown before
/// where the program records no adultness. The default draws every leaf
/// with the type's own look.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Forms {
    pub bands: Option<[usize; BANDS]>,
    pub plasticity: f64,
    pub juvenile: Option<(usize, Option<f64>)>,
}

impl Forms {
    /// The look, of the plant's looks, that `organ`, a leaf of the type
    /// whose own look is `own`, is drawn with, and its size as a share of
    /// the type's. A juvenile leaf ([`Forms::juvenile_leaf`]) wears the
    /// juvenile look; any other the step of its light's [`light_form`]
    /// nearest, at the size of its own place on that scale
    /// ([`Shape::size_in_light`]).
    #[must_use]
    pub fn of(&self, own: usize, organ: &crate::graph::GraphOrgan) -> (usize, f64) {
        if let Some((juvenile, _)) = self.juvenile
            && self.juvenile_leaf(organ)
        {
            return (juvenile, 1.0);
        }
        let Some(bands) = self.bands else {
            return (own, 1.0);
        };
        let form = light_form(organ.light);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let band =
            ((f64::midpoint(form, 1.0) * (BANDS - 1) as f64).round() as usize).min(BANDS - 1);
        (bands[band], Shape::size_in_light(form, self.plasticity))
    }
    /// Whether `organ` is a juvenile leaf of a type with juvenile leaves
    /// (spec rework P6): on a shoot of adultness 0 it is, on one of 1 it
    /// is not, and between, on a shoot changing, it is adult with a chance
    /// of its adultness, drawn from its id. Where the program records no
    /// adultness it is juvenile if the plant was younger than `until` when
    /// it grew.
    #[must_use]
    pub fn juvenile_leaf(&self, organ: &crate::graph::GraphOrgan) -> bool {
        let Some((_, until)) = self.juvenile else {
            return false;
        };
        match organ.adult {
            Some(adult) if adult <= 0.0 => true,
            Some(adult) if adult >= 1.0 => false,
            Some(adult) => {
                crate::rng::unit(crate::rng::mix64(organ.id ^ 0x00AD_0017)) >= f64::from(adult)
            }
            None => until.is_some_and(|until| organ.born < until),
        }
    }
}

/// An organ's year (plant roadmap P4): the days of the year, at the
/// species' reference climate, on which it is a bud, a flower, unripe
/// fruit and ripe fruit, and on which it falls or is eaten. The organ's own
/// look is its look in fruit.
///
/// On day `d` the organ is gone before `bud` and from `fall` on; a bud
/// from `bud`, a flower from `flower` and fruit from `fruit`. Of its fruit
/// a share
///
/// ```text
/// u = 1 - clamp((d - ripe) / ripening, 0, 1)
/// ```
///
/// is still unripe, so it ripens over `ripening` days from `ripe`. A
/// `fall` past 365 keeps the fruit into the next year (rose hips,
/// snowberries): on a day before `bud`, `d + 365` counts while it is before
/// `fall`.
///
/// An organ that is a cluster (an `inflorescence`) opens its units one
/// after another over `opening` days (spec rework L1b): its unit of rank
/// `r`, from 0 for the first to open to 1 for the last, is on day `d` in
/// the stage of day `max(d - r opening, bud)`, while the cluster shows
/// from `bud` and falls whole at `fall`. Each unit is drawn in its stage's
/// look, unless that look is `whole`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Season {
    pub bud: f64,
    pub flower: f64,
    pub fruit: f64,
    pub ripe: f64,
    #[serde(default = "default_ripening")]
    pub ripening: f64,
    pub fall: f64,
    /// Days a cluster takes to open from its first unit to its last, a
    /// head's florets or a lone flower's petals to open; 0, all at once,
    /// by default.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub opening: f64,
    /// Its size in fruit as a share of its size as grown, 0.05 to 20; 1
    /// by default. A lone flower's organ is grown as wide as its flower, so
    /// its fruit is this share of it, and every stage's size is this share
    /// of the organ's; a cluster grown as long as its inflorescence is
    /// drawn this much longer in fruit, where its fruit outgrow it.
    #[serde(default = "full_size", skip_serializing_if = "is_full_size")]
    pub fruit_size: f64,
    /// Its look in bud; without one, a third the size of its flower: a
    /// lone flower or head itself, closed; else a closed bud in the
    /// flower's colour half turned to the foliage's, whole where its
    /// flower look is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bud_look: Option<StageLook>,
    /// Its look in flower; without one it shows its own look then, or
    /// nothing if that is fruit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flower_look: Option<StageLook>,
}

fn default_ripening() -> f64 {
    20.0
}

/// An organ's look in one stage of its [`Season`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageLook {
    pub shape: Shape,
    /// Colours as in [`OrganLook`]; the organ's own if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<[f32; 3]>,
    /// The organ's size in this stage as a share of its size in fruit;
    /// a cluster's unit's, as a share of its unit's in fruit.
    #[serde(default = "full_size", skip_serializing_if = "is_full_size")]
    pub size: f64,
    /// How it is drawn as a solid, as in [`OrganLook`]; the shape's
    /// default if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<crate::blooms::Form>,
    /// For a cluster: whether this look draws the whole organ in this
    /// stage (a dogwood's head of tiny flowers in its four white bracts,
    /// an alder's catkins), not each of its units.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole: bool,
}

fn full_size() -> f64 {
    1.0
}

// Serde's `skip_serializing_if` passes a reference; a size left at its
// default is exactly 1.
#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_full_size(value: &f64) -> bool {
    *value == 1.0
}

/// What an organ with a [`Season`] is on a day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    /// Not on the plant: before its bud shows, or after it fell.
    Gone,
    Bud,
    Flower,
    /// Fruit, `unripe` of it still unripe.
    Fruit {
        unripe: f64,
    },
}

impl Season {
    /// Its look in bud: its `bud_look`; or for a head (`shape`, the
    /// organ's own), the head itself with its florets all in bud (organ
    /// phases, spec rework M7); or a closed bud of its flower's colours
    /// (`colour`, the organ's own, where it sets none), half turned to
    /// `foliage`'s (its sepals). Either is a third its flower's size, and
    /// whole where its flower look is.
    #[must_use]
    pub fn bud(&self, shape: &Shape, colour: [f32; 3], foliage: [f32; 3]) -> StageLook {
        self.bud_look.clone().unwrap_or_else(|| {
            let (colour, size, whole) = self
                .flower_look
                .as_ref()
                .map_or((colour, 1.0, false), |flower| {
                    (flower.colour.unwrap_or(colour), flower.size, flower.whole)
                });
            if let Shape::Head(head) = shape {
                return StageLook {
                    shape: Shape::Head(Head {
                        open: 0.0,
                        ..head.clone()
                    }),
                    colour: None,
                    accent: None,
                    size: size / 3.0,
                    form: None,
                    whole,
                };
            }
            let closed: [f32; 3] =
                std::array::from_fn(|channel| f32::midpoint(colour[channel], foliage[channel]));
            StageLook {
                shape: Shape::Fruit(Fruit {
                    aspect: 0.7,
                    ..Fruit::default()
                }),
                colour: Some(closed),
                accent: Some(closed),
                size: size / 3.0,
                form: None,
                whole,
            }
        })
    }

    /// A cluster's units in bud and in flower where they are drawn one by
    /// one, not `whole`: each a unit and its size as a share of the own
    /// unit's (see [`Self::bud`] for `shape`, `colour` and `foliage`).
    #[must_use]
    pub fn units(
        &self,
        shape: &Shape,
        colour: [f32; 3],
        foliage: [f32; 3],
    ) -> Vec<(crate::clusters::Unit, f64)> {
        [
            Some(self.bud(shape, colour, foliage)),
            self.flower_look.clone(),
        ]
        .into_iter()
        .flatten()
        .filter(|look| !look.whole)
        .filter_map(|look| crate::clusters::Unit::of(&look.shape).map(|unit| (unit, look.size)))
        .collect()
    }

    /// The season with its stages' heads' unset bracts in `foliage` (see
    /// [`Shape::with_bracts`]).
    #[must_use]
    pub fn with_bracts(&self, foliage: [f32; 3]) -> Self {
        let stage = |look: &Option<StageLook>| {
            look.as_ref().map(|look| StageLook {
                shape: look.shape.with_bracts(foliage),
                ..look.clone()
            })
        };
        Self {
            bud_look: stage(&self.bud_look),
            flower_look: stage(&self.flower_look),
            ..self.clone()
        }
    }

    /// The stage on day `day` of the year (1 to 365).
    #[must_use]
    pub fn stage(&self, day: f64) -> Stage {
        self.unit_stage(day, 0.0)
    }

    /// The stage on day `day` of a cluster's unit of `rank`, from 0 for
    /// the first to open to 1 for the last (see the type).
    #[must_use]
    pub fn unit_stage(&self, day: f64, rank: f64) -> Stage {
        let d = self.on(day);
        if d < self.bud || d >= self.fall {
            return Stage::Gone;
        }
        let d = (d - rank * self.opening).max(self.bud);
        if d < self.flower {
            Stage::Bud
        } else if d < self.fruit {
            Stage::Flower
        } else if self.ripening > 0.0 {
            Stage::Fruit {
                unripe: 1.0 - ((d - self.ripe) / self.ripening).clamp(0.0, 1.0),
            }
        } else {
            Stage::Fruit {
                unripe: if d < self.ripe { 1.0 } else { 0.0 },
            }
        }
    }

    /// How far a lone head with this season has opened on day `day` (its
    /// `open`; organ phases, spec rework M7). Its florets are ranked as a
    /// cluster's units are, 0 at the rim to 1 at the centre: the floret of
    /// rank `r` opens on day `flower + r opening` and goes to seed on day
    /// `fruit + r opening`, so the head is `(day - flower) / opening` open
    /// in flower, up to 1, and `1 + (day - fruit) / opening` in fruit, up
    /// to 2, every floret open before any goes to seed. With no `opening`
    /// every floret opens at once: 0 in bud, 1 in flower, 2 in fruit.
    #[must_use]
    pub fn open(&self, day: f64) -> f64 {
        let d = self.on(day);
        let past = |from: f64| {
            if self.opening > 0.0 {
                ((d - from) / self.opening).clamp(0.0, 1.0)
            } else if d >= from {
                1.0
            } else {
                0.0
            }
        };
        match self.stage(day) {
            Stage::Gone | Stage::Bud => 0.0,
            Stage::Flower => past(self.flower),
            Stage::Fruit { .. } => 1.0 + past(self.fruit),
        }
    }

    /// Whether day `day` of the year is past its fall, not before its bud
    /// (both show nothing).
    #[must_use]
    pub fn fallen(&self, day: f64) -> bool {
        self.on(day) >= self.fall
    }

    /// Day `day` of the year as a day of the season: a day before its bud
    /// is in the next year where its fall comes then.
    fn on(&self, day: f64) -> f64 {
        if day < self.bud && day + 365.0 < self.fall {
            day + 365.0
        } else {
            day
        }
    }

    /// Check the days, in order within one year, and the stage looks.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self) -> Result<(), String> {
        within("season bud", self.bud, 1.0, 365.0)?;
        let days = [self.bud, self.flower, self.fruit, self.ripe, self.fall];
        if days.windows(2).any(|pair| pair[1] < pair[0]) {
            return Err(format!(
                "season days must run bud, flower, fruit, ripe, fall in order, found {days:?}"
            ));
        }
        if self.fall - self.bud > 365.0 {
            return Err(format!(
                "season fall must come within a year of bud, found {} after",
                self.fall - self.bud
            ));
        }
        within("season ripening", self.ripening, 0.0, 200.0)?;
        within("season opening", self.opening, 0.0, 180.0)?;
        within("season fruit size", self.fruit_size, 0.05, 20.0)?;
        for look in [&self.bud_look, &self.flower_look].into_iter().flatten() {
            look.shape.validate()?;
            for colour in [look.colour, look.accent].into_iter().flatten() {
                for channel in colour {
                    within("colours", f64::from(channel), 0.0, 1.0)?;
                }
            }
            within("stage size", look.size, 0.05, 4.0)?;
            if let Some(form) = &look.form {
                form.validate(&look.shape)?;
            }
        }
        Ok(())
    }
}

// Serde's `skip_serializing_if` passes a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// A card template and its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "template", rename_all = "kebab-case")]
pub enum Shape {
    /// A conifer shoot set with needles.
    Needles(Needles),
    /// A flat, branched spray of scale leaves (cedars, cypresses).
    Scales(Scales),
    /// A palmately lobed leaf (maples).
    Palmate(Palmate),
    /// An undivided leaf: ovate, elliptic, lanceolate or round.
    Simple(Simple),
    /// A pinnately lobed leaf (oaks).
    Lobed(Lobed),
    /// A compound leaf: pinnate (ashes, elders, roses) or palmate
    /// (lupines), once or twice divided (mesquites, columbines), or a
    /// blade dissected into segments (yarrow).
    Compound(Compound),
    /// A leafy shoot: simple leaves alternating along a twig and one at
    /// its tip, for small-leaved trees and shrubs (alders, birches,
    /// cherries, willows) whose crowns would need too many single-leaf
    /// cards.
    Sprig(Sprig),
    /// One piece of a grass or sedge blade, or its tip.
    Blade(Blade),
    /// A fern frond.
    Frond(Frond),
    /// A single flower, seen face on.
    Flower(Flower),
    /// A flower head, a capitulum (`crate::capitula`): florets on a shared
    /// receptacle inside an involucre of bracts, seen face on.
    Head(Head),
    /// A flat-topped cluster of tiny flowers, seen from above.
    Umbel(Umbel),
    /// A spike, raceme or plume of flowers or grass spikelets, seen from
    /// the side.
    Spike(Spike),
    /// An open grass panicle seen from the side: whorls of fine branches
    /// and branchlets spreading from the axis, with spikelets clustered at
    /// their tips.
    Panicle(Panicle),
    /// A grass or sedge spikelet (`crate::spikelets`): glumes and florets
    /// on a short axis, seen from the side, as a cluster's unit.
    Spikelet(Spikelet),
    /// A berry, fruit or cone.
    Fruit(Fruit),
}

/// How a card sits on its organ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mount {
    /// The card stands on the organ's position and reaches along its
    /// heading: leaves, needles, blades, spikes.
    Along,
    /// The card is centred on the organ's position and faces along its
    /// heading: flowers seen face on.
    Facing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Needles {
    /// Side twigs on the shoot.
    pub twigs: u32,
    /// Needle length on the main axis.
    pub needle: f64,
    /// Angle of the side twigs to the main axis, degrees.
    pub twig_angle: f64,
    /// 2 for needles in two flat ranks; 0 for needles all round the shoot.
    pub ranks: u32,
    /// Card width over card length.
    pub aspect: f64,
    /// A needle's width over its length, as the solid shoot draws it
    /// (`crate::shoots`): about 0.1 for a yew's flat needles, 0.01 for a
    /// pine's long thin ones. The card draws needles as fine lines.
    pub width: f64,
    /// Needles in a fascicle, which fan a little round the shoot from one
    /// place on it: 2, 3 or 5 for a pine; 1 for needles borne singly; up
    /// to 45 for a short shoot's tuft, a larch's. Only all round a shoot.
    pub bundle: u32,
}

impl Default for Needles {
    fn default() -> Self {
        Self {
            twigs: 6,
            needle: 0.11,
            twig_angle: 45.0,
            ranks: 2,
            aspect: 0.85,
            width: 0.06,
            bundle: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scales {
    /// Branchlets on each side of the main axis.
    pub branchlets: u32,
    /// Angle of the branchlets, degrees.
    pub angle: f64,
    /// Width of the scale-covered cords.
    pub cord: f64,
    pub aspect: f64,
}

impl Default for Scales {
    fn default() -> Self {
        Self {
            branchlets: 6,
            angle: 50.0,
            cord: 0.02,
            aspect: 0.8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palmate {
    /// Main lobes: 3, 5, 7 or 9.
    pub lobes: u32,
    /// How deep the sinuses between lobes cut toward the centre, 0 to 1.
    pub depth: f64,
    /// Lobe width over lobe length.
    pub lobe_width: f64,
    /// Large teeth on each side of a lobe.
    pub teeth: u32,
    /// How far the lobes fan out: at 1 the outermost lobes stand 75° from
    /// the tip on a three-lobed leaf, 105° on a five-lobed one and 115° on
    /// a seven-lobed one.
    pub spread: f64,
    /// Length of the petiole, from the card's base to where the main veins
    /// meet.
    pub petiole: f64,
}

impl Default for Palmate {
    fn default() -> Self {
        Self {
            lobes: 5,
            depth: 0.5,
            lobe_width: 0.55,
            teeth: 2,
            spread: 1.0,
            petiole: 0.4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Simple {
    /// Blade width over blade length.
    pub width: f64,
    /// The blade's outline, when it is drawn from one: its half-width
    /// along its length as `[u, w]` points, `u` from the base (0) to the
    /// tip (1) and `w` a share of the widest point's, drawn through them
    /// with a monotone cubic ([`crate::math::monotone_cubic`]). It draws
    /// what `widest`, `tip` and `base` cannot: parallel sides, a narrow
    /// claw under a broad tip. A card with one leaves those three unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<[f64; 2]>>,
    /// Where the blade is widest, as a fraction of its length from the base.
    pub widest: f64,
    /// How drawn out the tip is, 0 (round) to 1 (long-pointed).
    pub tip: f64,
    /// How full the base is, 0 (tapered) to 1 (rounded).
    pub base: f64,
    /// Teeth on each side; 0 for an entire margin.
    pub teeth: u32,
    pub tooth_depth: f64,
    /// How rounded the teeth are, from 0 (saw teeth pointing to the tip,
    /// serrate) to 1 (rounded scallops, crenate), as a sprig's leaves'.
    #[serde(skip_serializing_if = "is_zero")]
    pub round: f64,
    pub petiole: f64,
    /// How far the blade's two basal lobes reach back past where the
    /// petiole meets it, beside the petiole, as a share of the blade's
    /// length: the sinus of a cordate or reniform base. 0 for none. The
    /// lobes never reach past the petiole's own length.
    #[serde(skip_serializing_if = "is_zero")]
    pub sinus: f64,
    /// Pairs of main veins that leave the midrib's foot with it and fan
    /// out across the blade (palmate venation); 0 for a pinnately veined
    /// blade, whose veins all leave the midrib along its length.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub fan: u32,
    /// How the blade narrows over its last [`crate::templates::ENDS`] to
    /// its tip, as the power of that narrowing: 1 straight sides to a
    /// point (acute), above 1 hollow sides drawn out (acuminate), below 1
    /// full sides, rounded near 0.35 and cut nearly square near 0.1
    /// (truncate). Unset, the outline (or `tip`) alone shapes the tip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apex_taper: Option<f64>,
    /// The same, for the blade's first stretch above its base: 1 a
    /// straight wedge (cuneate), above 1 a long taper (attenuate), below
    /// 1 rounded or cut square (truncate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_taper: Option<f64>,
    /// A narrow point beyond the blade's tip, as a share of the blade's
    /// length: a mucro, a cusp, a tail (caudate). 0 for none.
    #[serde(skip_serializing_if = "is_zero")]
    pub apex_point: f64,
    /// How deep a notch cuts into the blade's tip along the midrib, as a
    /// share of the blade's length (emarginate, retuse). 0 for none.
    #[serde(skip_serializing_if = "is_zero")]
    pub apex_notch: f64,
}

impl Default for Simple {
    fn default() -> Self {
        Self {
            width: 0.5,
            outline: None,
            widest: 0.4,
            tip: 0.4,
            base: 0.5,
            teeth: 0,
            tooth_depth: 0.03,
            round: 0.0,
            petiole: 0.15,
            sinus: 0.0,
            fan: 0,
            apex_taper: None,
            base_taper: None,
            apex_point: 0.0,
            apex_notch: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lobed {
    /// Blade width over blade length.
    pub width: f64,
    /// Lobes on each side.
    pub lobes: u32,
    /// How deep the sinuses cut toward the midrib, 0 to 1.
    pub depth: f64,
    /// 1 for rounded lobes, 0 for pointed ones.
    pub round: f64,
    pub petiole: f64,
}

impl Default for Lobed {
    fn default() -> Self {
        Self {
            width: 0.6,
            lobes: 4,
            depth: 0.5,
            round: 1.0,
            petiole: 0.08,
        }
    }
}

/// A compound leaf (spec rework P4; laid out by [`crate::divided`]). The
/// fields after `petiole` are unset by default, and an unset field takes
/// the value [`crate::divided::Form::of`] gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compound {
    /// Leaflets, the terminal one included; in a twice-divided leaf, its
    /// divisions (pinnae). A pinnate leaf with an odd count ends in a
    /// leaflet (odd-pinnate), one with an even count in its top pair
    /// (even-pinnate).
    pub leaflets: u32,
    /// Leaflet width over leaflet length.
    pub leaflet_width: f64,
    /// Teeth on each side of a leaflet; 0 for entire.
    pub teeth: u32,
    pub petiole: f64,
    /// How the leaflets are set: along a rachis (pinnate, the default) or
    /// from the petiole's tip (palmate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Pattern>,
    /// Leaflets on each division of a twice-divided leaf (bipinnate,
    /// biternate), their count's parity saying how a pinna ends; unset or
    /// 0 for a leaf divided once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinnules: Option<u32>,
    /// Segments on each pinnule of a thrice-divided leaf (tripinnate,
    /// triternate, ternate-pinnate), set as `inner` says; unset or 0 for
    /// a leaf divided twice or once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<u32>,
    /// How the divisions after the first are set, where not as `pattern`
    /// says: a ternate-pinnate leaf's three divisions are each pinnate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner: Option<Pattern>,
    /// How far each division's base runs along its rachis as a wing: 0
    /// separate leaflets, 1 a dissected blade's segments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confluent: Option<f64>,
    /// Degrees between a pinnate leaf's lateral leaflets (or pinnae) and
    /// its rachis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Degrees between a pinna's leaflets and its rachis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinnule_angle: Option<f64>,
    /// Degrees between a pinnule's segments and its rachis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_angle: Option<f64>,
    /// Degrees between a palmate leaf's outermost leaflets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<f64>,
    /// Where along the rachis the lateral leaflets are longest, 0 at its
    /// foot to 1 at its top.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub largest: Option<f64>,
    /// The shortest leaflets' length as a share of the longest's: a
    /// pinnate leaf's lowest (and, with `largest` below 1, its topmost), a
    /// palmate leaf's outermost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowest: Option<f64>,
    /// The terminal leaflet's length as a share of the longest lateral's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_size: Option<f64>,
    /// Each leaflet's blade, as in [`Simple`]: its outline, or where it is
    /// widest, its tip and its base.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<[f64; 2]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widest: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<f64>,
    /// Each leaflet's stalk, as a share of its length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub petiolule: Option<f64>,
    /// The blade's width over its length, where it bounds a pinnate
    /// leaf's laterals: they reach no farther than its edge, as a
    /// strap-shaped leaf's short pinnae do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blade_width: Option<f64>,
    /// The nearest levels of detail on which the leaf is drawn as
    /// geometry, its rachises and each leaflet or segment in three
    /// dimensions, instead of on its card (`crate::lace`); unset or 0 for
    /// the card on every level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lace: Option<u8>,
    /// Degrees each primary division rises out of the leaf's plane, drawn
    /// as geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lift: Option<f64>,
    /// Degrees each division turns at random about its rachis, either way,
    /// at most: a loose spray.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scatter: Option<f64>,
    /// Degrees each primary division turns about the rachis from the one
    /// below, 0 for two ranks in the leaf's plane, 137.5 for divisions all
    /// round it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll: Option<f64>,
    /// Degrees each leaflet of a division turns about its rachis from the
    /// one before: a bottlebrush's segments all round each pinna.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_roll: Option<f64>,
}

impl Default for Compound {
    fn default() -> Self {
        Self {
            leaflets: 7,
            leaflet_width: 0.4,
            teeth: 0,
            petiole: 0.15,
            pattern: None,
            pinnules: None,
            segments: None,
            inner: None,
            confluent: None,
            angle: None,
            pinnule_angle: None,
            segment_angle: None,
            spread: None,
            largest: None,
            lowest: None,
            terminal_size: None,
            outline: None,
            widest: None,
            tip: None,
            base: None,
            petiolule: None,
            blade_width: None,
            lace: None,
            lift: None,
            scatter: None,
            roll: None,
            inner_roll: None,
        }
    }
}

/// How a compound leaf's leaflets are set (spec rework P4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// In pairs along a rachis: ashes, roses, a mesquite's pinnae.
    #[default]
    Pinnate,
    /// From the tip of the petiole: lupines, a blackberry's five
    /// leaflets, a wood sorrel's three.
    Palmate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sprig {
    /// Leaves on the shoot, the one at its tip included.
    pub leaves: u32,
    /// Angle of the side leaves to the twig, degrees.
    pub angle: f64,
    /// Bare twig below the lowest leaf, as a fraction of the card's
    /// length.
    pub stalk: f64,
    /// Each leaf's blade, as in [`Simple`]: width over length, its outline
    /// or where it is widest, its tip and base, and its teeth.
    pub width: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<[f64; 2]>>,
    pub widest: f64,
    pub tip: f64,
    pub base: f64,
    pub teeth: u32,
    pub tooth_depth: f64,
    /// How rounded the teeth are, from 0 (saw teeth pointing to the tip)
    /// to 1 (rounded lobes, as an oak's).
    #[serde(skip_serializing_if = "is_zero")]
    pub round: f64,
    /// Length of each leaf's petiole, as a fraction of the leaf's length.
    pub petiole: f64,
    /// Each leaf's length, petiole included, as a fraction of the card's
    /// length: the species' own leaf on a card as long as the twig it
    /// stands for (a rule sets it from `leaf_length_m`). 0 sizes the
    /// leaves to fit `leaves` of them along the twig, as before.
    #[serde(skip_serializing_if = "is_zero")]
    pub leaf: f64,
    /// Side twigs, alternating up the main one, each set with leaves of
    /// that size and one at its tip, so that a twig of small leaves covers
    /// its card as a shoot with its side shoots does. With `leaf` set,
    /// leaves stand along every twig a third of a leaf apart and `leaves` is
    /// not read.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub twigs: u32,
}

impl Default for Sprig {
    fn default() -> Self {
        Self {
            leaves: 7,
            angle: 50.0,
            stalk: 0.1,
            width: 0.5,
            outline: None,
            widest: 0.4,
            tip: 0.4,
            base: 0.5,
            teeth: 0,
            tooth_depth: 0.03,
            round: 0.0,
            petiole: 0.12,
            leaf: 0.0,
            twigs: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Blade {
    /// Fraction of the card's length over which the blade narrows to its
    /// point: 0 for a piece of a longer blade, 1 for a tip that narrows
    /// all the way.
    pub taper: f64,
    /// Blade width over card length.
    pub aspect: f64,
}

impl Default for Blade {
    fn default() -> Self {
        Self {
            taper: 0.0,
            aspect: 0.12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Frond {
    /// Leaflets (pinnae) on each side.
    pub pinnae: u32,
    /// Frond width over frond length.
    pub width: f64,
    /// 1 when each pinna is itself divided, as in bracken.
    pub divided: f64,
    /// Length of the bare stalk at the base.
    pub stipe: f64,
}

impl Default for Frond {
    fn default() -> Self {
        Self {
            pinnae: 18,
            width: 0.32,
            divided: 0.0,
            stipe: 0.12,
        }
    }
}

/// A flower (spec rework P5, `crate::flowers`): laid out whorl by whorl,
/// its floral cup, perianth, stamens and pistil. Lengths are shares of the
/// flower's radius, half the organ's size, unless they say otherwise. The
/// fields after `centre` default to a flower drawn as before P5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flower {
    /// Perianth parts in all: the petals, both whorls of tepals, or a
    /// spiral's tepals.
    pub petals: u32,
    /// Petal width; 1 leaves a little gap between neighbours, 1.4 makes
    /// them overlap.
    pub petal_width: f64,
    /// 0 for round petal tips, 1 for pointed ones.
    pub point: f64,
    /// The power of a petal's taper from its widest to its tip, where
    /// `point` alone does not say it: 0.45 a round tip, 1.3 straight sides
    /// to a point (as `point` gives below and from 0.5), between them an
    /// elliptic petal with an acute tip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taper: Option<f64>,
    /// A petal's width over its length where widest, as its outline is
    /// stated (`petal_shape`): set, it gives `petal_width` for the
    /// flower's own count of petals (see [`Flower::petal_share`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub petal_ratio: Option<f64>,
    /// Depth of a notch in each petal tip, 0 to 1; a lip's lobes.
    pub notch: f64,
    /// Radius of the centre, in the accent colour, over the flower's.
    pub centre: f64,
    /// What surrounds the stamens.
    #[serde(skip_serializing_if = "Perianth::is_corolla")]
    pub perianth: Perianth,
    /// Sepals between the petals; 0 draws none.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub sepals: u32,
    /// Sepal length over a petal's.
    #[serde(skip_serializing_if = "is_default_sepal_length")]
    pub sepal_length: f64,
    /// Share of the sepals' length joined in a calyx cup.
    #[serde(skip_serializing_if = "is_zero")]
    pub sepals_fused: f64,
    /// The sepals' colour, and a spiral's outer tepals'; a leaf green if
    /// absent. A labellate flower's sepals take the petals' colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calyx: Option<[f32; 3]>,
    /// The filaments' and the style's colour; the accent, as the anthers
    /// are, if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filament: Option<[f32; 3]>,
    /// A single pistil's ovary's colour; the accent if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pistil: Option<[f32; 3]>,
    /// How the petals differ by place.
    #[serde(skip_serializing_if = "Corolla::is_regular")]
    pub corolla: Corolla,
    /// Whorls a regular corolla's petals stand in: 1, or 2 like whorls,
    /// the inner turned half a petal from the outer (barberries' three
    /// and three, bleeding heart's outer and inner pair). `petals` counts
    /// both.
    #[serde(skip_serializing_if = "is_one_count")]
    pub petal_whorls: u32,
    /// A papilionaceous banner's, or an unequal corolla's upper petal's,
    /// width over a petal's.
    #[serde(skip_serializing_if = "is_default_banner")]
    pub banner: f64,
    /// Degrees an unequal corolla's odd petal or tepal stands above the
    /// others' cup: 15 more upright; negative bends it out and down, as a
    /// camas's lowest tepal stands apart once the flower is turned half
    /// round.
    #[serde(skip_serializing_if = "is_default_odd_rise")]
    pub odd_rise: f64,
    /// A bilabiate lower lip's middle petal, or a labellate lip, over a
    /// petal.
    #[serde(skip_serializing_if = "is_default_lip")]
    pub lip: f64,
    /// Share of each petal's length joined in a corolla tube.
    #[serde(skip_serializing_if = "is_zero")]
    pub fused: f64,
    /// The corolla tube's mouth radius over a petal's length.
    #[serde(skip_serializing_if = "is_default_throat")]
    pub throat: f64,
    /// How much the tube swells in its middle, over a petal's length.
    #[serde(skip_serializing_if = "is_zero")]
    pub belly: f64,
    /// A spur's length over the flower's radius: every petal's on a regular
    /// corolla, a bilabiate lower lip's middle petal's, a lip's.
    #[serde(skip_serializing_if = "is_zero")]
    pub spur: f64,
    /// A floral cup (a hypanthium or calyx tube) under the perianth, over
    /// the flower's radius.
    #[serde(skip_serializing_if = "is_zero")]
    pub tube: f64,
    /// Stamens: in whorls up to twice the petals, on a spiral above.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub stamens: u32,
    /// Their length over the flower's radius.
    #[serde(skip_serializing_if = "is_default_stamen_length")]
    pub stamen_length: f64,
    /// An anther's length over its stamen's.
    #[serde(skip_serializing_if = "is_default_anther")]
    pub anther: f64,
    /// Degrees an anther turns from its filament's line: 0 along it, as a
    /// basifixed anther stands; toward 90 across it, as a versatile one
    /// hangs on the filament's tip.
    #[serde(skip_serializing_if = "is_zero")]
    pub anther_tilt: f64,
    /// Sterile stamens, drawn without anthers.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub staminodes: u32,
    /// 0 for the centre dome; 1 to 5, or any number below the whorls (an
    /// inferior or half-inferior ovary), one pistil with as many stigma
    /// lobes; more above them, as many free pistils.
    #[serde(skip_serializing_if = "is_zero_count")]
    pub carpels: u32,
    /// Where the ovary sits.
    #[serde(skip_serializing_if = "Ovary::is_superior")]
    pub ovary: Ovary,
    /// A labellate flower's column over the flower's radius.
    #[serde(skip_serializing_if = "is_default_column")]
    pub column: f64,
    /// Degrees the whole flower turns about its axis from its
    /// developmental frame: 180 for a resupinate orchid.
    #[serde(skip_serializing_if = "is_zero")]
    pub turn: f64,
    /// How far it has opened: 0 a closed bud, 1 open.
    #[serde(skip_serializing_if = "is_one")]
    pub open: f64,
}

impl Default for Flower {
    fn default() -> Self {
        Self {
            petals: 5,
            petal_width: 1.0,
            point: 0.0,
            taper: None,
            petal_ratio: None,
            notch: 0.0,
            centre: 0.18,
            perianth: Perianth::Corolla,
            sepals: 0,
            sepal_length: SEPAL_LENGTH,
            sepals_fused: 0.0,
            calyx: None,
            filament: None,
            pistil: None,
            corolla: Corolla::Regular,
            petal_whorls: 1,
            banner: BANNER,
            odd_rise: ODD_RISE,
            lip: LIP,
            fused: 0.0,
            throat: THROAT,
            belly: 0.0,
            spur: 0.0,
            tube: 0.0,
            stamens: 0,
            stamen_length: STAMEN_LENGTH,
            anther: crate::flowers::ANTHER,
            anther_tilt: 0.0,
            staminodes: 0,
            carpels: 0,
            ovary: Ovary::Superior,
            column: COLUMN,
            turn: 0.0,
            open: 1.0,
        }
    }
}

impl Flower {
    /// Petal width as `petal_width` gives it, 1 leaving a little gap
    /// between neighbours: from `petal_ratio` where it is set, a petal as
    /// wide for its length as its outline is stated, among as many petals
    /// as the flower draws.
    #[must_use]
    pub fn petal_share(&self) -> f64 {
        self.petal_ratio.map_or(self.petal_width, |ratio| {
            let half = std::f64::consts::PI / f64::from(self.petals.max(3));
            (ratio / (1.32 * crate::math::tan(half))).clamp(0.1, 1.5)
        })
    }

    /// Whether the flower is drawn as before P5: one whorl of alike petals
    /// round a centre dome, nothing that the layout adds.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn is_plain(&self) -> bool {
        self.perianth == Perianth::Corolla
            && self.sepals == 0
            && self.corolla == Corolla::Regular
            && self.petal_whorls == 1
            && self.fused == 0.0
            && self.spur == 0.0
            && self.stamens == 0
            && self.staminodes == 0
            && self.carpels == 0
            && self.ovary == Ovary::Superior
            && self.turn == 0.0
            && self.open == 1.0
    }

    /// Whether the flower's plane of symmetry is fixed by the axis that
    /// bears it: a zygomorphic corolla, or a flower turned.
    #[must_use]
    pub fn is_zygomorphic(&self) -> bool {
        self.corolla != Corolla::Regular || self.turn != 0.0
    }
}

const SEPAL_LENGTH: f64 = 0.5;
const BANNER: f64 = 1.6;
const ODD_RISE: f64 = 15.0;
const LIP: f64 = 1.3;
const THROAT: f64 = 0.3;
const STAMEN_LENGTH: f64 = 0.35;
const COLUMN: f64 = 0.45;

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero_count(value: &u32) -> bool {
    *value == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one_count(value: &u32) -> bool {
    *value == 1
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_one(value: &f64) -> bool {
    *value == 1.0
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_sepal_length(value: &f64) -> bool {
    *value == SEPAL_LENGTH
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_banner(value: &f64) -> bool {
    *value == BANNER
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_anther(value: &f64) -> bool {
    *value == crate::flowers::ANTHER
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_odd_rise(value: &f64) -> bool {
    *value == ODD_RISE
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_lip(value: &f64) -> bool {
    *value == LIP
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_throat(value: &f64) -> bool {
    *value == THROAT
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_stamen_length(value: &f64) -> bool {
    *value == STAMEN_LENGTH
}

#[allow(clippy::trivially_copy_pass_by_ref, clippy::float_cmp)]
fn is_default_column(value: &f64) -> bool {
    *value == COLUMN
}

/// What surrounds a flower's stamens (spec rework P5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Perianth {
    /// One whorl of petals, with the flower's `sepals` below them.
    #[default]
    Corolla,
    /// Two like whorls of tepals (lilies, irises, agaves, orchids).
    Tepals,
    /// Tepals on a spiral (water lilies, cacti).
    Spiral,
    /// No perianth.
    Absent,
}

impl Perianth {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_corolla(&self) -> bool {
        *self == Self::Corolla
    }
}

/// How a flower's petals differ by place (spec rework P5): each place a
/// role, and a role is a petal still.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corolla {
    /// All alike.
    #[default]
    Regular,
    /// A pea flower's banner, two wings and keel (5 petals).
    Papilionaceous,
    /// Two lips: the petals toward the bearing axis the upper; or, of
    /// tepals (a gladiolus's), the tepals on that side.
    Bilabiate,
    /// An orchid's lip and column: three sepals coloured like the petals,
    /// two lateral petals and the lip.
    Labellate,
    /// Petals alike but the one toward the bearing axis, wider (by
    /// `banner`) and more upright (by `odd_rise`): a zygomorphy of no named kind, as the
    /// banner of a palo verde, which is not a pea flower's; of tepals, the
    /// outer one toward the axis.
    Unequal,
}

impl Corolla {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_regular(&self) -> bool {
        *self == Self::Regular
    }
}

/// Where a flower's ovary sits (spec rework P5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ovary {
    /// On the receptacle, inside the whorls.
    #[default]
    Superior,
    /// Half below the whorls.
    Half,
    /// Below the whorls, which stand on its top.
    Inferior,
}

impl Ovary {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_superior(&self) -> bool {
        *self == Self::Superior
    }
}

/// A flower head, a capitulum (spec rework P2, `crate::capitula`): florets
/// packed on a shared receptacle inside an involucre of bracts
/// (phyllaries), as in the daisy family. Lengths are shares of the head's
/// radius, half the organ's size, unless they say otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Head {
    /// Ray florets round the rim of a radiate head; 0 for a head of disc
    /// florets only.
    pub rays: u32,
    /// Ray width over the width of its share of the circle.
    pub ray_width: f64,
    /// A ray's tip: 0 a blunt point, 1 a parallel-sided strap with a
    /// square end (an oxeye daisy's truncate, toothed rays).
    #[serde(skip_serializing_if = "is_zero")]
    pub ray_tip: f64,
    /// Radius of the disc, the receptacle, over the head's.
    pub disc: f64,
    /// What its florets are: rays round a disc of small tubes (`radiate`,
    /// the rays possibly 0), disc florets only (`discoid`) or straps only
    /// (`ligulate`, as in dandelions and cat's-ears).
    #[serde(skip_serializing_if = "HeadKind::is_radiate")]
    pub kind: HeadKind,
    /// Disc florets, or every strap of a ligulate head, packed on the
    /// receptacle in a sunflower spiral.
    pub florets: u32,
    /// Bracts of the involucre round the head's foot.
    pub phyllaries: u32,
    /// Rows they stand in, the outer shorter.
    pub series: u32,
    /// The involucre's height over the disc's width: about 0.3 for a flat
    /// daisy, more than 1 for yarrow's cylinder.
    pub involucre: f64,
    /// The receptacle's dome, its height over the disc's radius: 0 flat,
    /// 1 a hemisphere, more a cone.
    pub receptacle: f64,
    /// Degrees the phyllaries' upper halves turn outward from the
    /// involucre's wall: 0 pressed to the head; an everlasting's dry
    /// bracts open round its disc like petals.
    #[serde(skip_serializing_if = "is_zero")]
    pub spread: f64,
    /// How far the disc florets stand above the receptacle when open,
    /// over the disc's radius: 0 for about their spacing, a daisy's short
    /// tubes; a thistle's stand out of its involucre as a brush.
    #[serde(skip_serializing_if = "is_zero")]
    pub floret_length: f64,
    /// How far the head has opened from its rim inward: 0 a bud, 1 every
    /// floret open, 2 every floret gone to seed; between, the outer share
    /// of its florets a stage on.
    pub open: f64,
    /// Length of the pappus each fruit carries, over the disc's radius: 0
    /// for none, as in daisies; drawn on florets gone to seed.
    #[serde(skip_serializing_if = "is_zero")]
    pub pappus: f64,
    /// Colour of the phyllaries; the plant's foliage colour if absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bracts: Option<[f32; 3]>,
}

impl Default for Head {
    fn default() -> Self {
        Self {
            rays: 21,
            ray_width: 0.7,
            ray_tip: 0.0,
            disc: 0.32,
            kind: HeadKind::Radiate,
            florets: 60,
            phyllaries: 16,
            series: 2,
            involucre: 0.4,
            receptacle: 0.25,
            spread: 0.0,
            floret_length: 0.0,
            open: 0.6,
            pappus: 0.0,
            bracts: None,
        }
    }
}

/// What a head's florets are (spec rework P2): the states of the
/// `capitulum_kind` character, a disciform head drawn as a discoid one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadKind {
    /// Ray florets round a disc of tubular ones (daisies, asters).
    #[default]
    Radiate,
    /// Tubular disc florets only (thistles, pearly everlasting).
    Discoid,
    /// Strap-shaped florets only (dandelions, cat's-ears).
    Ligulate,
}

impl HeadKind {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_radiate(&self) -> bool {
        *self == Self::Radiate
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Umbel {
    /// Small clusters in the umbel; 1 for a single dense cluster.
    pub clusters: u32,
    /// Flowers in each cluster.
    pub florets: u32,
}

impl Default for Umbel {
    fn default() -> Self {
        Self {
            clusters: 9,
            florets: 14,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spike {
    /// Flowers or spikelets along the axis.
    pub florets: u32,
    /// Spike width over card length.
    pub aspect: f64,
    /// 0 for distinct flowers, 1 for a feathery plume of fine spikelets.
    pub plume: f64,
    /// Length of bare stalk below the flowers.
    pub stalk: f64,
}

impl Default for Spike {
    fn default() -> Self {
        Self {
            florets: 14,
            aspect: 0.3,
            plume: 0.0,
            stalk: 0.1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panicle {
    /// Whorls of branches along the axis.
    pub whorls: u32,
    /// Branches in each whorl.
    pub branches: u32,
    /// Angle at which the branches leave the axis, degrees; their outer
    /// halves turn half as far again.
    pub spread: f64,
    /// Spikelets on each branch, clustered at its tip and at the tips of
    /// its two branchlets.
    pub spikelets: u32,
    /// Panicle width over card length.
    pub aspect: f64,
    /// Length of bare stalk below the lowest whorl.
    pub stalk: f64,
}

impl Default for Panicle {
    fn default() -> Self {
        Self {
            whorls: 7,
            branches: 3,
            spread: 40.0,
            spikelets: 4,
            aspect: 0.6,
            stalk: 0.05,
        }
    }
}

/// A grass or sedge spikelet (spec rework P3, `crate::spikelets`): empty
/// glumes at the foot of a short axis, the rachilla, and above them its
/// florets, each shown by its lemma, in two ranks or a spiral. Lengths are
/// shares of the spikelet's length, from its foot to the highest tip of a
/// glume or lemma, awns left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spikelet {
    /// Florets on the rachilla.
    pub florets: u32,
    /// Empty glumes at its foot: 2, 1 (the upper alone) or 0.
    pub glumes: u32,
    /// The upper glume's length.
    pub glume: f64,
    /// The lower glume's length over the upper's.
    pub lower: f64,
    /// Each lemma's length.
    pub lemma: f64,
    /// The spikelet's width across its bracts' plane over its length.
    pub aspect: f64,
    /// Length of a lemma's awn; 0 for none.
    #[serde(skip_serializing_if = "is_zero")]
    pub awn: f64,
    /// Where an awn leaves its lemma's keel, a share of the lemma below its
    /// tip: 0 from the tip, more from its back.
    #[serde(skip_serializing_if = "is_zero")]
    pub awn_from: f64,
    /// The share of the florets, from the top, that bear awns.
    pub awned: f64,
    /// Degrees an awn bends outward at its knee: 0 straight, as a
    /// geniculate awn is bent.
    #[serde(skip_serializing_if = "is_zero")]
    pub bent: f64,
    /// 2 for bracts in two ranks (grasses), 0 for a spiral (many sedges).
    pub ranks: u32,
    /// Degrees each lemma leans out from the rachilla; the glumes half as
    /// far.
    pub gape: f64,
}

impl Default for Spikelet {
    fn default() -> Self {
        Self {
            florets: 4,
            glumes: 2,
            glume: 0.45,
            lower: 0.8,
            lemma: 0.4,
            aspect: 0.3,
            awn: 0.0,
            awn_from: 0.0,
            awned: 1.0,
            bent: 0.0,
            ranks: 2,
            gape: 15.0,
        }
    }
}

/// One fruit or cone. Several on one organ are a cluster, set by the look's
/// `inflorescence` (spec rework L1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FruitCard")]
pub struct Fruit {
    /// Width over length of one fruit.
    pub aspect: f64,
    /// 1 for the overlapping scales of a cone.
    pub cone: f64,
    /// What the fruit is (plant roadmap P4, `crate::fruit`); a berry or
    /// bud unless set.
    #[serde(skip_serializing_if = "FruitKind::is_berry")]
    pub kind: FruitKind,
    /// Share of the fruit still unripe, drawn in the look's accent colour
    /// (which also draws stalks, and the cup or husk of a nut).
    #[serde(skip_serializing_if = "is_zero")]
    pub unripe: f64,
}

impl Default for Fruit {
    fn default() -> Self {
        Self {
            aspect: 0.7,
            cone: 0.0,
            kind: FruitKind::Berry,
            unripe: 0.0,
        }
    }
}

/// A fruit card as a spec writes it: a [`Fruit`], and the keys that set a
/// cluster before L1, read only to say where they went.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FruitCard {
    aspect: f64,
    cone: f64,
    kind: FruitKind,
    unripe: f64,
    count: Option<serde::de::IgnoredAny>,
    cluster: Option<serde::de::IgnoredAny>,
    size: Option<serde::de::IgnoredAny>,
    spread: Option<serde::de::IgnoredAny>,
}

impl Default for FruitCard {
    fn default() -> Self {
        let fruit = Fruit::default();
        Self {
            aspect: fruit.aspect,
            cone: fruit.cone,
            kind: fruit.kind,
            unripe: fruit.unripe,
            count: None,
            cluster: None,
            size: None,
            spread: None,
        }
    }
}

impl TryFrom<FruitCard> for Fruit {
    type Error = String;

    fn try_from(card: FruitCard) -> Result<Self, String> {
        let moved = [
            ("count", card.count.is_some(), "flowers"),
            ("cluster", card.cluster.is_some(), "kind"),
            ("size", card.size.is_some(), "size"),
            ("spread", card.spread.is_some(), "spread"),
        ];
        if let Some((key, _, now)) = moved.iter().find(|(_, set, _)| *set) {
            return Err(format!(
                "fruit `{key}` is now the look's `inflorescence` `{now}`: a cluster of fruit is \
                 an inflorescence whose units are fruit"
            ));
        }
        Ok(Self {
            aspect: card.aspect,
            cone: card.cone,
            kind: card.kind,
            unripe: card.unripe,
        })
    }
}

/// What a fruit is: the solid drawn up close and the card drawn from the
/// same layout (`crate::fruit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FruitKind {
    /// A berry, drupe or bud: an ellipsoid.
    #[default]
    Berry,
    /// A berry crowned by its dried calyx (huckleberries, salal,
    /// serviceberry, currants).
    Crowned,
    /// A pome (crabapples): a rounded fruit dimpled at its top.
    Pome,
    /// An aggregate of drupelets (blackberries, salmonberry,
    /// thimbleberry).
    Drupelets,
    /// A rose hip: an urn with the sepals at its tip.
    Hip,
    /// A samara (maples, ashes): a seed at the base of a flat wing.
    Samara,
    /// An acorn: a nut in a scaly cup.
    Acorn,
    /// A nut in a husk drawn out into a beak (hazelnuts).
    Husked,
    /// A legume pod: long, flat and bulging over its seeds.
    Pod,
    /// A capsule or follicle: ribbed lengthwise.
    Capsule,
}

impl FruitKind {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_berry(&self) -> bool {
        *self == Self::Berry
    }
}

/// How a cluster's units are arranged (spec rework L1): each kind its
/// botanical definition, laid out by `crate::clusters`. The states of the
/// `inflorescence` character, less a solitary flower and a spadix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterKind {
    /// Units on pedicels up an axis, the top one reaching its tip (salal,
    /// serviceberry, lupine).
    #[default]
    Raceme,
    /// A raceme of sessile units (vanilla leaf, velvet mesquite).
    Spike,
    /// A dense spike of small units, often drooping (alders, birches).
    Catkin,
    /// A raceme whose lower pedicels are longer, so every unit's top is
    /// level (yarrow's heads).
    Corymb,
    /// Pedicels from one point, each reaching the top (onion, milkweed).
    Umbel,
    /// An umbel whose rays end in umbellets (cow parsnip).
    CompoundUmbel,
    /// A raceme whose nodes bear branches (oceanspray, grape).
    Panicle,
    /// The axis ends in a unit, with branches below it that end in units
    /// and fork again (elderberry's cymules).
    Cyme,
    /// A cyme forking once at each node, always to one side, so its units
    /// lie on a coil (phacelias, forget-me-nots).
    Scorpioid,
    /// Units packed on a sphere at the top of the peduncle (Pacific
    /// dogwood, ninebark).
    Head,
}

impl ClusterKind {
    /// Every kind, in the order the `inflorescence` character lists them.
    pub const ALL: [Self; 10] = [
        Self::Raceme,
        Self::Spike,
        Self::Catkin,
        Self::Corymb,
        Self::Umbel,
        Self::CompoundUmbel,
        Self::Panicle,
        Self::Cyme,
        Self::Scorpioid,
        Self::Head,
    ];

    /// The kind's name in a spec.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Raceme => "raceme",
            Self::Spike => "spike",
            Self::Catkin => "catkin",
            Self::Corymb => "corymb",
            Self::Umbel => "umbel",
            Self::CompoundUmbel => "compound_umbel",
            Self::Panicle => "panicle",
            Self::Cyme => "cyme",
            Self::Scorpioid => "scorpioid",
            Self::Head => "head",
        }
    }
}

/// An organ drawn as one cluster of units (spec rework L1), as a spec gives
/// it: every key may be left out for its default, which may depend on the
/// kind and on the other keys (`crate::clusters::Cluster::of`), so `{}`
/// is a raceme of 12.
/// Lengths are shares of the axis that bears them: the organ's, or a
/// branch's.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Inflorescence {
    /// How the units are arranged; a raceme unless set, or for a branch
    /// its cluster's branches' kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ClusterKind>,
    /// Units on each axis that bears them: the main axis, or each branch
    /// of a compound cluster; a cyme's units in all. 1 to 200. Unless set
    /// (or, for a branch, set on its cluster), a raceme, spike, catkin or
    /// corymb holds as many as stand half a unit's `size` apart above its
    /// peduncle (12 with neither set), and other kinds 12.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flowers: Option<u32>,
    /// Branches the axis bears in place of units, making a compound
    /// cluster: 0 to 60; 8 in a panicle, 12 in a compound umbel, else 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches: Option<u32>,
    /// How each branch is laid out: a raceme in a panicle, an umbel in a
    /// compound umbel, else the cluster's own kind, with its `flowers`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Box<Inflorescence>>,
    /// Units or branches at one node, spread evenly round the axis: 1 to
    /// 8; 2 in a cyme, else 1. Two are pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_node: Option<u32>,
    /// Degrees from one node to the next, 0 to 180: the golden angle
    /// unless set; 90 with pairs crosses them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergence: Option<f64>,
    /// Degrees a pedicel or branch leans from its axis, 0 to 90; 35.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spread: Option<f64>,
    /// A unit's stalk, 0 to 1: as long as the unit (`size`) in a raceme,
    /// else 0.15, and 0 where the axis bears branches.
    /// Spikes, catkins and heads are sessile; umbels and corymbs reach the
    /// top whatever it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pedicel: Option<f64>,
    /// Bare stalk below the lowest node, 0 to 1: 0.3 in an umbel or a
    /// compound umbel, else 0.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peduncle: Option<f64>,
    /// One unit's length (a flower's or head's width), or a branch's,
    /// 0.005 to 1: 0.15, or 0.4 where the axis bears branches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// How much smaller the upper units and branches are, 0 to 1: the top
    /// one 1 - taper of the lowest; 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taper: Option<f64>,
    /// Degrees the axis bends over by its tip, 0 to 180: 90 nodding, 180
    /// hanging; 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub droop: Option<f64>,
    /// Colour of the axes and pedicels, on the solid and the card; green
    /// unless set, or for a branch its cluster's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stalk: Option<[f32; 3]>,
    /// A spathe (spec rework E6b): one bract sheathing the axis below the
    /// lowest unit and opening into a blade behind the units, as long as
    /// this share of the axis, 0 to 3; none (0) unless set. Only the
    /// cluster's own axis bears one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spathe: Option<f64>,
    /// The spathe's widest width over its length, 0.1 to 1.5; 0.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spathe_width: Option<f64>,
    /// Degrees the spathe's blade arches forward over the units from its
    /// foot to its tip, 0 to 120; 20.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spathe_hood: Option<f64>,
    /// How deeply the spathe cups round the units, 0 (flat) to 1; 0.5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spathe_cup: Option<f64>,
    /// The spathe's colour, on the solid and the card; the look's colour
    /// unless set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spathe_colour: Option<[f32; 3]>,
}

impl Shape {
    /// The default look of an organ kind.
    #[must_use]
    pub fn default_for(kind: OrganKind) -> Self {
        match kind {
            OrganKind::Foliage => Self::Needles(Needles::default()),
            OrganKind::Leaf => Self::Simple(Simple::default()),
            OrganKind::Flower => Self::Flower(Flower::default()),
            OrganKind::Fruit => Self::Fruit(Fruit::default()),
            OrganKind::Cone => Self::Fruit(Fruit {
                aspect: 0.5,
                cone: 1.0,
                ..Fruit::default()
            }),
        }
    }

    /// The shape, a head's unset bracts (its phyllaries' colour) in
    /// `foliage`, the plant's leaves' colour.
    #[must_use]
    pub fn with_bracts(&self, foliage: [f32; 3]) -> Self {
        match self {
            Self::Head(head) if head.bracts.is_none() => Self::Head(Head {
                bracts: Some(foliage),
                ..head.clone()
            }),
            _ => self.clone(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Needles(_) => "needles",
            Self::Scales(_) => "scales",
            Self::Palmate(_) => "palmate",
            Self::Simple(_) => "simple",
            Self::Lobed(_) => "lobed",
            Self::Compound(_) => "compound",
            Self::Sprig(_) => "sprig",
            Self::Blade(_) => "blade",
            Self::Frond(_) => "frond",
            Self::Flower(_) => "flower",
            Self::Head(_) => "head",
            Self::Umbel(_) => "umbel",
            Self::Spike(_) => "spike",
            Self::Panicle(_) => "panicle",
            Self::Spikelet(_) => "spikelet",
            Self::Fruit(_) => "fruit",
        }
    }

    #[must_use]
    pub fn mount(&self) -> Mount {
        match self {
            Self::Flower(_) | Self::Head(_) | Self::Umbel(_) => Mount::Facing,
            _ => Mount::Along,
        }
    }

    /// For a leaf on a petiole, in lengths of its drawing from its foot:
    /// where the petiole meets the blade, and where it would meet it under
    /// the blade were the blade peltate (the centre of a palmate blade's
    /// lobes or leaflets, the middle of any other). `None` for shapes
    /// without a petiole.
    #[must_use]
    pub fn junction(&self) -> Option<(f64, f64)> {
        let middle = |p: f64| (p, p + 0.5 * (1.0 - p));
        match self {
            Self::Simple(s) => Some(middle(s.petiole)),
            Self::Lobed(s) => Some(middle(s.petiole)),
            Self::Palmate(s) => {
                let (junction, _) = s.junction();
                Some((junction, junction))
            }
            Self::Compound(c) => Some(match c.pattern.unwrap_or_default() {
                Pattern::Palmate => (c.petiole, c.petiole),
                Pattern::Pinnate => middle(c.petiole),
            }),
            _ => None,
        }
    }

    /// Width of a second card that crosses the first, relative to it, for
    /// organs that stand out all round their axis and would vanish seen
    /// edge on: needles, spikes, panicles and fruit. `None` for flat
    /// organs.
    #[must_use]
    pub fn cross(&self) -> Option<f64> {
        match self {
            // Needles all round the shoot fill a full crossing card; flat
            // two-ranked sprays keep a narrower one, so they still read
            // as foliage from the side.
            Self::Needles(needles) => Some(if needles.ranks == 0 { 1.0 } else { 0.6 }),
            Self::Spike(_) | Self::Panicle(_) | Self::Spikelet(_) | Self::Fruit(_) => Some(1.0),
            _ => None,
        }
    }

    /// Card width over card length.
    #[must_use]
    pub fn aspect(&self) -> f64 {
        match self {
            Self::Needles(needles) => needles.aspect,
            Self::Scales(scales) => scales.aspect,
            Self::Palmate(palmate) => palmate.aspect(),
            Self::Simple(simple) => simple.width * (1.0 - simple.petiole) * 1.08 + 0.02,
            Self::Lobed(lobed) => lobed.width * (1.0 - lobed.petiole) * 1.08 + 0.02,
            Self::Compound(compound) => compound.aspect(),
            Self::Sprig(sprig) => sprig.aspect(),
            Self::Blade(blade) => blade.aspect,
            Self::Frond(frond) => frond.width,
            Self::Flower(_) | Self::Head(_) | Self::Umbel(_) => 1.0,
            Self::Spike(spike) => spike.aspect,
            Self::Panicle(panicle) => panicle.aspect,
            Self::Spikelet(spikelet) => spikelet.aspect,
            Self::Fruit(fruit) => fruit.aspect,
        }
    }

    /// The size, as a share of its type's, of a leaf `form` of the way to
    /// its sun form (1) or its shade form (-1), for a `plasticity` 0 to 1
    /// (see [`Self::in_light`]).
    #[must_use]
    pub fn size_in_light(form: f64, plasticity: f64) -> f64 {
        1.0 - 0.12 * plasticity.clamp(0.0, 1.0) * form.clamp(-1.0, 1.0)
    }

    /// This leaf as grown `form` of the way to its sun form (1) or its
    /// shade form (-1), for a `plasticity` 0 to 1 (plant roadmap P4, spec
    /// rework L3), and its size as a share of the leaf's own: a sun leaf
    /// is smaller, narrower and more deeply lobed or toothed; a shade leaf
    /// larger, broader and shallower (sun and shade leaves of one crown, as
    /// of oaks and maples). A tooth's depth is taken as a share of the
    /// blade's half-width, as a lobe's is, so that the sun deepens how far
    /// the teeth cut. `None` for shapes without the two forms.
    #[must_use]
    pub fn in_light(&self, form: f64, plasticity: f64) -> Option<(Shape, f64)> {
        let sun = form > 0.0;
        let p = plasticity.clamp(0.0, 1.0) * form.abs().min(1.0);
        // Narrower in the sun, broader in the shade.
        let wide = |value: f64, low: f64, high: f64, narrow: f64, broad: f64| {
            (if sun {
                value * (1.0 - narrow * p)
            } else {
                value * (1.0 + broad * p)
            })
            .clamp(low, high)
        };
        // Deeper in the sun, toward `most`; shallower in the shade.
        let deep = |value: f64, most: f64, toward: f64| {
            if sun {
                value + (most - value).max(0.0) * toward * p
            } else {
                value * (1.0 - toward * p)
            }
        };
        // A tooth's depth as a share of the half-width it cuts into, from
        // `half` to `then`.
        let cut = |depth: f64, half: f64, then: f64| {
            if half <= 0.0 {
                return depth;
            }
            (deep(depth / half, 0.85, 0.4) * then).min(0.2)
        };
        let size = Self::size_in_light(form, plasticity);
        let shape = match self {
            Self::Simple(s) => {
                let width = wide(s.width, 0.03, 1.5, 0.25, 0.15);
                let half = |width: f64| 0.5 * width * (1.0 - s.petiole);
                Self::Simple(Simple {
                    width,
                    tooth_depth: cut(s.tooth_depth, half(s.width), half(width)),
                    ..s.clone()
                })
            }
            Self::Lobed(s) => Self::Lobed(Lobed {
                width: wide(s.width, 0.1, 1.5, 0.12, 0.1),
                depth: deep(s.depth, 0.9, 0.5),
                ..s.clone()
            }),
            Self::Palmate(s) => Self::Palmate(Palmate {
                lobe_width: wide(s.lobe_width, 0.1, 1.2, 0.15, 0.1),
                depth: deep(s.depth, 0.9, 0.4),
                ..s.clone()
            }),
            Self::Compound(s) => Self::Compound(Compound {
                leaflet_width: wide(s.leaflet_width, 0.05, 1.0, 0.2, 0.15),
                ..s.clone()
            }),
            Self::Sprig(s) => {
                let width = wide(s.width, 0.03, 1.2, 0.2, 0.15);
                Self::Sprig(Sprig {
                    width,
                    tooth_depth: cut(s.tooth_depth, 0.5 * s.width, 0.5 * width),
                    ..s.clone()
                })
            }
            _ => return None,
        };
        Some((shape, size))
    }

    /// Check the parameters.
    ///
    /// # Errors
    ///
    /// Describes the first parameter outside its range.
    // One short arm per shape: a table, clearer whole than split.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), String> {
        let check = |name: &str, value: f64, low: f64, high: f64| {
            if (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "{} {name} must be between {low} and {high}, found {value}",
                    self.name()
                ))
            }
        };
        let count = |name: &str, value: u32, low: u32, high: u32| {
            if (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "{} {name} must be between {low} and {high}, found {value}",
                    self.name()
                ))
            }
        };
        match self {
            Self::Needles(s) => {
                count("twigs", s.twigs, 0, 12)?;
                // A shoot with no side twigs (a pine's tuft) has room across
                // its card for longer needles.
                check(
                    "needle",
                    s.needle,
                    0.01,
                    if s.twigs == 0 { 0.6 } else { 0.4 },
                )?;
                check("twig_angle", s.twig_angle, 10.0, 80.0)?;
                if s.ranks != 0 && s.ranks != 2 {
                    return Err(format!("needles ranks must be 0 or 2, found {}", s.ranks));
                }
                check("width", s.width, 0.003, 0.3)?;
                count("bundle", s.bundle, 1, 45)?;
                if s.bundle > 1 && s.ranks != 0 {
                    return Err(
                        "needles in bundles stand all round the shoot: ranks must be 0".to_string(),
                    );
                }
                check("aspect", s.aspect, 0.1, 2.0)
            }
            Self::Scales(s) => {
                count("branchlets", s.branchlets, 1, 10)?;
                check("angle", s.angle, 10.0, 80.0)?;
                check("cord", s.cord, 0.005, 0.06)?;
                check("aspect", s.aspect, 0.2, 2.0)
            }
            Self::Palmate(s) => {
                if !(3..=9).contains(&s.lobes) || s.lobes % 2 == 0 {
                    return Err(format!(
                        "palmate lobes must be 3, 5, 7 or 9, found {}",
                        s.lobes
                    ));
                }
                check("depth", s.depth, 0.0, 0.9)?;
                check("lobe_width", s.lobe_width, 0.1, 1.2)?;
                count("teeth", s.teeth, 0, 8)?;
                check("spread", s.spread, 0.4, 1.2)?;
                check("petiole", s.petiole, 0.0, 0.7)
            }
            Self::Simple(s) => {
                check("width", s.width, 0.03, 1.5)?;
                let default = Simple::default();
                outline(
                    self.name(),
                    s.outline.as_deref(),
                    [
                        ("widest", s.widest, default.widest),
                        ("tip", s.tip, default.tip),
                        ("base", s.base, default.base),
                    ],
                )?;
                check("widest", s.widest, 0.15, 0.85)?;
                check("tip", s.tip, 0.0, 1.0)?;
                check("base", s.base, 0.0, 1.0)?;
                count("teeth", s.teeth, 0, 60)?;
                check("tooth_depth", s.tooth_depth, 0.0, 0.2)?;
                check("round", s.round, 0.0, 1.0)?;
                check("sinus", s.sinus, 0.0, 0.5)?;
                count("fan", s.fan, 0, 4)?;
                if let Some(power) = s.apex_taper {
                    check("apex_taper", power, 0.05, 4.0)?;
                }
                if let Some(power) = s.base_taper {
                    check("base_taper", power, 0.05, 4.0)?;
                }
                check("apex_point", s.apex_point, 0.0, 0.3)?;
                check("apex_notch", s.apex_notch, 0.0, 0.2)?;
                check("petiole", s.petiole, 0.0, 0.7)
            }
            Self::Lobed(s) => {
                check("width", s.width, 0.1, 1.5)?;
                count("lobes", s.lobes, 1, 10)?;
                check("depth", s.depth, 0.0, 0.9)?;
                check("round", s.round, 0.0, 1.0)?;
                check("petiole", s.petiole, 0.0, 0.7)
            }
            Self::Compound(s) => s.validate(),
            Self::Sprig(s) => {
                count("leaves", s.leaves, 2, 15)?;
                if s.leaf != 0.0 {
                    check("leaf", s.leaf, 0.03, 0.45)?;
                }
                count("twigs", s.twigs, 0, 12)?;
                check("angle", s.angle, 20.0, 80.0)?;
                check("stalk", s.stalk, 0.0, 0.4)?;
                check("width", s.width, 0.03, 1.5)?;
                let default = Sprig::default();
                outline(
                    self.name(),
                    s.outline.as_deref(),
                    [
                        ("widest", s.widest, default.widest),
                        ("tip", s.tip, default.tip),
                        ("base", s.base, default.base),
                    ],
                )?;
                check("widest", s.widest, 0.15, 0.85)?;
                check("tip", s.tip, 0.0, 1.0)?;
                check("base", s.base, 0.0, 1.0)?;
                count("teeth", s.teeth, 0, 40)?;
                check("tooth_depth", s.tooth_depth, 0.0, 0.2)?;
                check("round", s.round, 0.0, 1.0)?;
                check("petiole", s.petiole, 0.0, 0.5)
            }
            Self::Blade(s) => {
                check("taper", s.taper, 0.0, 1.0)?;
                check("aspect", s.aspect, 0.01, 1.0)
            }
            Self::Frond(s) => {
                count("pinnae", s.pinnae, 3, 40)?;
                check("width", s.width, 0.05, 1.0)?;
                check("divided", s.divided, 0.0, 1.0)?;
                check("stipe", s.stipe, 0.0, 0.6)
            }
            Self::Flower(s) => {
                count("petals", s.petals, 3, 60)?;
                if s.petals > 12 && s.perianth != Perianth::Spiral {
                    return Err(format!(
                        "flower petals above 12 stand on a spiral: perianth must be spiral, found {}",
                        s.petals
                    ));
                }
                check("petal_width", s.petal_width, 0.1, 1.5)?;
                if let Some(ratio) = s.petal_ratio {
                    check("petal_ratio", ratio, 0.05, 2.0)?;
                }
                check("point", s.point, 0.0, 1.0)?;
                if let Some(taper) = s.taper {
                    check("taper", taper, 0.2, 3.0)?;
                }
                check("anther", s.anther, 0.05, 0.6)?;
                check("anther_tilt", s.anther_tilt, 0.0, 90.0)?;
                check("notch", s.notch, 0.0, 0.8)?;
                check("centre", s.centre, 0.0, 0.8)?;
                count("sepals", s.sepals, 0, 12)?;
                check("sepal_length", s.sepal_length, 0.1, 1.5)?;
                check("sepals_fused", s.sepals_fused, 0.0, 0.95)?;
                check("odd_rise", s.odd_rise, -90.0, 90.0)?;
                for channel in s.filament.into_iter().flatten() {
                    check("filament", f64::from(channel), 0.0, 1.0)?;
                }
                for channel in s.pistil.into_iter().flatten() {
                    check("pistil", f64::from(channel), 0.0, 1.0)?;
                }
                for channel in s.calyx.into_iter().flatten() {
                    check("calyx", f64::from(channel), 0.0, 1.0)?;
                }
                check("banner", s.banner, 0.5, 3.0)?;
                check("lip", s.lip, 0.5, 3.0)?;
                check("fused", s.fused, 0.0, 0.95)?;
                check("throat", s.throat, 0.05, 1.0)?;
                check("belly", s.belly, 0.0, 0.5)?;
                check("spur", s.spur, 0.0, 3.0)?;
                check("tube", s.tube, 0.0, 3.0)?;
                count("stamens", s.stamens, 0, 200)?;
                check("stamen_length", s.stamen_length, 0.05, 2.0)?;
                count("staminodes", s.staminodes, 0, 50)?;
                count("carpels", s.carpels, 0, 30)?;
                check("column", s.column, 0.1, 1.5)?;
                check("turn", s.turn, 0.0, 360.0)?;
                check("open", s.open, 0.0, 1.0)?;
                if s.petal_whorls == 2
                    && (s.perianth != Perianth::Corolla
                        || s.corolla != Corolla::Regular
                        || s.petals % 2 != 0
                        || s.petals < 4)
                {
                    return Err(format!(
                        "petals in two whorls are a regular corolla of an even number, \
                         at least 4: found {} {:?} {:?}",
                        s.petals, s.perianth, s.corolla
                    ));
                }
                if !(1..=2).contains(&s.petal_whorls) {
                    return Err(format!("petal_whorls is 1 or 2, found {}", s.petal_whorls));
                }
                if s.perianth == Perianth::Tepals && s.petals % 2 != 0 {
                    return Err(format!(
                        "tepals stand in two like whorls: petals must be even, found {}",
                        s.petals
                    ));
                }
                match s.corolla {
                    Corolla::Papilionaceous if s.petals != 5 || s.perianth != Perianth::Corolla => {
                        Err(format!(
                            "a papilionaceous corolla is one whorl of 5 petals, found {} {}",
                            s.petals,
                            if s.perianth == Perianth::Corolla {
                                "petals"
                            } else {
                                "tepals"
                            }
                        ))
                    }
                    Corolla::Bilabiate
                        if !match s.perianth {
                            Perianth::Corolla => (4..=6).contains(&s.petals),
                            Perianth::Tepals => (4..=12).contains(&s.petals),
                            _ => false,
                        } =>
                    {
                        Err(format!(
                            "a bilabiate corolla is one whorl of 4 to 6 petals or two whorls \
                             of 4 to 12 tepals, found {} {:?}",
                            s.petals, s.perianth
                        ))
                    }
                    Corolla::Unequal
                        if !matches!(s.perianth, Perianth::Corolla | Perianth::Tepals) =>
                    {
                        Err(
                            "an unequal corolla is one whorl of petals or two of tepals: \
                             perianth sepals_and_petals or tepals"
                                .to_string(),
                        )
                    }
                    Corolla::Labellate
                        if s.petals != 3 || s.sepals != 3 || s.perianth != Perianth::Corolla =>
                    {
                        Err(format!(
                            "a labellate flower has three sepals and three petals, one of them the lip: petals 3, sepals 3 and perianth sepals_and_petals, found petals {} sepals {}",
                            s.petals, s.sepals
                        ))
                    }
                    _ => Ok(()),
                }
            }
            Self::Head(s) => {
                count("rays", s.rays, 0, 60)?;
                check("ray_width", s.ray_width, 0.1, 1.2)?;
                check("ray_tip", s.ray_tip, 0.0, 1.0)?;
                check("disc", s.disc, 0.1, 1.0)?;
                if s.kind != HeadKind::Radiate && s.rays > 0 {
                    return Err(format!(
                        "rays are the rim florets of a radiate head; a {} head has none, found {}",
                        if s.kind == HeadKind::Discoid {
                            "discoid"
                        } else {
                            "ligulate"
                        },
                        s.rays
                    ));
                }
                count("florets", s.florets, 1, 1000)?;
                count("phyllaries", s.phyllaries, 0, 200)?;
                count("series", s.series, 1, 12)?;
                check("involucre", s.involucre, 0.05, 3.0)?;
                check("receptacle", s.receptacle, 0.0, 3.0)?;
                check("spread", s.spread, 0.0, 150.0)?;
                check("floret_length", s.floret_length, 0.0, 3.0)?;
                check("open", s.open, 0.0, 2.0)?;
                check("pappus", s.pappus, 0.0, 3.0)?;
                for channel in s.bracts.into_iter().flatten() {
                    check("bracts", f64::from(channel), 0.0, 1.0)?;
                }
                Ok(())
            }
            Self::Umbel(s) => {
                count("clusters", s.clusters, 1, 30)?;
                count("florets", s.florets, 1, 60)
            }
            Self::Spikelet(s) => {
                count("florets", s.florets, 1, 40)?;
                count("glumes", s.glumes, 0, 2)?;
                check("glume", s.glume, 0.0, 1.5)?;
                check("lower", s.lower, 0.1, 1.5)?;
                check("lemma", s.lemma, 0.05, 1.0)?;
                check("aspect", s.aspect, 0.02, 1.5)?;
                check("awn", s.awn, 0.0, 6.0)?;
                check("awn_from", s.awn_from, 0.0, 0.9)?;
                check("awned", s.awned, 0.0, 1.0)?;
                check("bent", s.bent, 0.0, 150.0)?;
                if s.ranks != 0 && s.ranks != 2 {
                    return Err(format!(
                        "spikelet ranks must be 2 (two ranks) or 0 (a spiral), found {}",
                        s.ranks
                    ));
                }
                check("gape", s.gape, 0.0, 80.0)
            }
            Self::Spike(s) => {
                count("florets", s.florets, 1, 80)?;
                check("aspect", s.aspect, 0.03, 1.0)?;
                check("plume", s.plume, 0.0, 1.0)?;
                check("stalk", s.stalk, 0.0, 0.8)
            }
            Self::Panicle(s) => {
                count("whorls", s.whorls, 1, 20)?;
                count("branches", s.branches, 1, 8)?;
                check("spread", s.spread, 10.0, 80.0)?;
                count("spikelets", s.spikelets, 1, 12)?;
                check("aspect", s.aspect, 0.1, 1.2)?;
                check("stalk", s.stalk, 0.0, 0.6)
            }
            Self::Fruit(s) => {
                check("aspect", s.aspect, 0.1, 2.0)?;
                check("cone", s.cone, 0.0, 1.0)?;
                check("unripe", s.unripe, 0.0, 1.0)
            }
        }
    }
}

/// The most points an outline may have.
pub const OUTLINE_POINTS: usize = 16;

/// Checks a `template` card's `outline`, if it has one: 2 to
/// [`OUTLINE_POINTS`] points from `u` 0 to 1 in order, each `w` from 0 to 1,
/// rising above 0 and ending at 0 at the tip; and none of the `fields` it
/// replaces set to other than their defaults.
// The ends are 0 and 1 exactly, and a field is set where it is not its
// default.
#[allow(clippy::float_cmp)]
fn outline(
    template: &str,
    outline: Option<&[[f64; 2]]>,
    fields: [(&str, f64, f64); 3],
) -> Result<(), String> {
    let Some(points) = outline else {
        return Ok(());
    };
    if !(2..=OUTLINE_POINTS).contains(&points.len()) {
        return Err(format!(
            "{template} outline has 2 to {OUTLINE_POINTS} points, found {}",
            points.len()
        ));
    }
    let ordered = points.windows(2).all(|pair| pair[0][0] < pair[1][0]);
    if !ordered || points[0][0] != 0.0 || points[points.len() - 1][0] != 1.0 {
        return Err(format!(
            "{template} outline points must run from u 0 to u 1 in order"
        ));
    }
    if let Some([_, w]) = points.iter().find(|[_, w]| !(0.0..=1.0).contains(w)) {
        return Err(format!(
            "{template} outline w must be between 0 and 1, found {w}"
        ));
    }
    if points[points.len() - 1][1] != 0.0 || points.iter().all(|[_, w]| *w == 0.0) {
        return Err(format!(
            "{template} outline must rise above w 0 and end at w 0, at the tip"
        ));
    }
    if let Some((field, _, _)) = fields.iter().find(|(_, value, default)| value != default) {
        return Err(format!(
            "a {template} card with an outline takes its shape from it: drop `{field}`"
        ));
    }
    Ok(())
}

impl Palmate {
    /// Angle of lobe `k` from the tip (negative to the left) and its
    /// length as a fraction of the blade's radius, for `k` from
    /// `-(lobes - 1) / 2` to `(lobes - 1) / 2`.
    fn lobe_unit(&self, k: i32) -> (f64, f64) {
        let half = f64::from(self.lobes.saturating_sub(1) / 2).max(1.0);
        let t = f64::from(k.unsigned_abs()) / half;
        // The outermost lobes stand at 75° from the tip on a three-lobed
        // leaf and fan further back the more lobes there are.
        let outermost = (0.75 - 0.33 / half) * crate::math::PI;
        let angle = f64::from(k) / half * self.spread * outermost;
        // Lobes shorten toward the base, the basal ones most.
        (angle, 1.0 - 0.38 * t * t)
    }

    /// Height of the point where the main veins meet, and the blade's
    /// radius from there to the tip. The junction is the petiole's length,
    /// raised when needed so that lobes pointing back past it stay on the
    /// card.
    #[must_use]
    pub fn junction(&self) -> (f64, f64) {
        let half = i32::try_from(self.lobes / 2).unwrap_or(0);
        // How far below the junction the lowest lobe reaches, per unit
        // radius, with room for its width.
        let mut below: f64 = 0.0;
        for k in 0..=half {
            let (angle, length) = self.lobe_unit(k);
            let down = -length * crate::math::cos(angle);
            let side = 0.5 * length * self.lobe_width * crate::math::sin(angle).abs();
            below = below.max(down + side * 0.5);
        }
        let junction = self.petiole.max((below + 0.01) / (1.0 + below));
        (junction, 1.0 - junction)
    }

    /// Angle of lobe `k` from the tip (negative to the left) and its
    /// length, for `k` from `-(lobes - 1) / 2` to `(lobes - 1) / 2`.
    #[must_use]
    pub fn lobe(&self, k: i32) -> (f64, f64) {
        let (angle, length) = self.lobe_unit(k);
        let (_, radius) = self.junction();
        (angle, radius * length)
    }

    fn aspect(&self) -> f64 {
        let half = i32::try_from(self.lobes / 2).unwrap_or(0);
        // The blade between the lobes reaches the sinuses' depth.
        let (_, radius) = self.junction();
        let mut reach: f64 = radius * (1.0 - self.depth);
        for k in 0..=half {
            let (angle, length) = self.lobe(k);
            // The lobe's tip and the widest part of its side.
            let side = length * self.lobe_width * 0.5;
            reach = reach.max(length * crate::math::sin(angle).abs());
            reach = reach.max(
                length * 0.55 * crate::math::sin(angle).abs()
                    + side * crate::math::cos(angle).abs(),
            );
        }
        (2.0 * reach * 1.06 + 0.02).min(1.6)
    }
}

impl Compound {
    /// Length of the longest lateral leaflet of a pinnate leaf divided
    /// once.
    #[must_use]
    pub fn leaflet_length(&self) -> f64 {
        crate::divided::Form::of(self).primary_longest()
    }

    fn aspect(&self) -> f64 {
        let form = crate::divided::Form::of(self);
        if form.pattern == Pattern::Pinnate && form.pinnules == 0 {
            let length = self.leaflet_length();
            let terminal = if self.leaflets % 2 == 1 {
                0.5 * length * form.terminal_size * self.leaflet_width
            } else {
                0.0
            };
            return 2.0
                * (length * crate::math::sin(crate::math::radians(form.angle))
                    + length * self.leaflet_width * 0.5)
                    .max(terminal)
                + 0.03;
        }
        crate::divided::reach(self) * 2.0 + 0.03
    }

    fn validate(&self) -> Result<(), String> {
        let form = crate::divided::Form::of(self);
        let check = |field: &str, value: f64, low: f64, high: f64| {
            if (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "compound {field} must be between {low} and {high}, found {value}"
                ))
            }
        };
        let divisions = |field: &str, n: u32| -> Result<(), String> {
            match form.pattern {
                Pattern::Pinnate if !(2..=81).contains(&n) => Err(format!(
                    "compound {field} of a pinnate leaf must be 2 to 81, found {n}"
                )),
                Pattern::Palmate if !(3..=17).contains(&n) => Err(format!(
                    "compound {field} of a palmate leaf must be 3 to 17, found {n}"
                )),
                _ => Ok(()),
            }
        };
        let inner = |field: &str, n: u32| -> Result<(), String> {
            match form.inner {
                Pattern::Pinnate if !(2..=41).contains(&n) => Err(format!(
                    "compound {field} of a pinnate division must be 2 to 41, found {n}"
                )),
                Pattern::Palmate if !(3..=17).contains(&n) => Err(format!(
                    "compound {field} of a palmate division must be 3 to 17, found {n}"
                )),
                _ => Ok(()),
            }
        };
        divisions("leaflets", self.leaflets)?;
        if self.segments.unwrap_or(0) > 0 && form.pinnules == 0 {
            return Err("compound segments are borne on pinnules: set `pinnules`".to_owned());
        }
        if form.pinnules > 0 {
            inner("pinnules", form.pinnules)?;
            let segments = form.segments.max(1);
            if self.leaflets * form.pinnules * segments > 1200 {
                return Err(format!(
                    "a compound leaf has at most 1200 leaflets, found {} pinnae of {} of {segments}",
                    self.leaflets, form.pinnules
                ));
            }
        }
        if form.segments > 0 {
            inner("segments", form.segments)?;
        }
        check("leaflet_width", self.leaflet_width, 0.05, 1.0)?;
        if self.teeth > 30 {
            return Err(format!(
                "compound teeth must be between 0 and 30, found {}",
                self.teeth
            ));
        }
        check("petiole", self.petiole, 0.0, 0.7)?;
        if self.lace.is_some_and(|levels| levels > 2) {
            return Err(format!(
                "compound lace is drawn on at most the 2 nearest levels, found {}",
                self.lace.unwrap_or(0)
            ));
        }
        let set = |value: Option<f64>| value.unwrap_or(f64::NAN);
        for (field, value, low, high) in [
            ("confluent", self.confluent, 0.0, 1.0),
            ("angle", self.angle, 20.0, 90.0),
            ("pinnule_angle", self.pinnule_angle, 20.0, 90.0),
            ("segment_angle", self.segment_angle, 20.0, 90.0),
            ("spread", self.spread, 30.0, 340.0),
            ("largest", self.largest, 0.0, 1.0),
            ("lowest", self.lowest, 0.2, 1.0),
            ("terminal_size", self.terminal_size, 0.3, 2.0),
            ("widest", self.widest, 0.15, 0.85),
            ("tip", self.tip, 0.0, 1.0),
            ("base", self.base, 0.0, 1.0),
            ("petiolule", self.petiolule, 0.0, 0.5),
            ("blade_width", self.blade_width, 0.02, 3.0),
            ("lift", self.lift, -90.0, 90.0),
            ("scatter", self.scatter, 0.0, 90.0),
            ("roll", self.roll, 0.0, 360.0),
            ("inner_roll", self.inner_roll, 0.0, 360.0),
        ] {
            if value.is_some() {
                check(field, set(value), low, high)?;
            }
        }
        if let Some(field) = [
            ("widest", self.widest),
            ("tip", self.tip),
            ("base", self.base),
        ]
        .iter()
        .find_map(|(field, value)| value.is_some().then_some(field))
            && self.outline.is_some()
        {
            return Err(format!(
                "a compound card with an outline takes its leaflets' shape from it: drop `{field}`"
            ));
        }
        outline("compound", self.outline.as_deref(), [("", 0.0, 0.0); 3])
    }
}

/// Degrees a sprig's side twigs stand from its main twig.
pub const SPRIG_TWIG_ANGLE: f64 = 40.0;

impl Sprig {
    /// Length of the longest leaf, petiole included, as a fraction of the
    /// card's length.
    #[must_use]
    pub fn leaf_length(&self) -> f64 {
        if self.leaf > 0.0 {
            return self.leaf;
        }
        let side = f64::from(self.leaves.saturating_sub(1));
        ((1.0 - self.stalk) * 2.4 / (side / 2.0 + 1.5)).min(0.45)
    }

    /// Whether the leaves stand at their own size along twigs, rather
    /// than `leaves` of them sized to fit the card.
    #[must_use]
    pub fn sized(&self) -> bool {
        self.leaf > 0.0 || self.twigs > 0
    }

    /// Length of each side twig, as a fraction of the card's length: half
    /// the leafy part of the main twig, at most a third of the card.
    #[must_use]
    pub fn twig_length(&self) -> f64 {
        if self.twigs == 0 {
            return 0.0;
        }
        (0.5 * (1.0 - self.leaf_length() - self.stalk)).clamp(0.05, 0.34)
    }

    /// Card width over card length: room for the side twigs and the side
    /// leaves at their angle, with their blades' width.
    #[must_use]
    pub fn aspect(&self) -> f64 {
        let length = self.leaf_length();
        let angle = crate::math::radians(self.angle);
        let blade = length * (1.0 - self.petiole) * self.width * 0.5;
        let twig = self.twig_length() * crate::math::sin(crate::math::radians(SPRIG_TWIG_ANGLE));
        (2.0 * (twig + length * crate::math::sin(angle) + blade * crate::math::cos(angle)) + 0.03)
            .min(1.6)
    }
}

/// An organ type's look with every default filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The organ's name in the program.
    pub organ: String,
    pub shape: Shape,
    pub colour: [f32; 3],
    pub shade: [f32; 3],
    pub accent: [f32; 3],
    /// Its lower face's colour ([`OrganLook::underside`]), if it differs.
    pub underside: Option<[f32; 3]>,
    pub variegation: Option<Variegation>,
    pub face_up: f64,
    /// Degrees its card turns about its own length ([`OrganLook::roll`]).
    pub roll: f64,
    /// Where its petiole meets its blade, for a leaf.
    pub insertion: Insertion,
    pub solid: Option<crate::leaves::SolidLeaf>,
    pub bend: crate::bend::Bend,
    pub form: crate::blooms::Form,
    /// Its leaves' families' looks; the default for none.
    pub forms: Forms,
    /// The cluster the organ is, each of its units drawn with `shape`
    /// (spec rework L1); `None` for an organ that is one unit.
    pub inflorescence: Option<crate::clusters::ClusterLook>,
    /// Its [`VARIETIES`] varieties (spec rework VARY), where values of its
    /// look were set from ranges and it is drawn solid: each organ is
    /// drawn as one ([`Look::of`]). Empty for a look that does not vary;
    /// its card is always drawn with the look itself.
    pub varieties: Vec<Look>,
    /// The hairs on its face, where it has any to draw.
    pub hairs: Option<Hairs>,
}

/// The varieties of each look that varies (spec rework VARY): as many as
/// a part mesh's variants, so variant `k` is variety `k`.
pub const VARIETIES: usize = crate::parts::VARIANTS;

/// The variety of its type's look the organ with id `id` is drawn as.
#[must_use]
pub fn variety_of(id: u64) -> usize {
    usize::try_from(crate::rng::mix64(id ^ 0x0056_4152_4945) % VARIETIES as u64).unwrap_or(0)
}

/// How far through the spread of the value at `path` (in an appearance's
/// `spreads`) variety `variety` draws it: the middle of its own of
/// [`VARIETIES`] equal parts of the spread, the parts dealt to the
/// varieties in an order `path` shuffles, so the varieties draw every
/// value across its spread, and pair the values of different paths at
/// random.
#[must_use]
pub fn variety_share(variety: usize, path: &str) -> f64 {
    crate::rng::dealt(variety, VARIETIES, path)
}

/// `looks`, a species' looks by organ name, as variety `variety` draws
/// them: each value `spreads` (its appearance's) holds a spread for,
/// under `organs.<organ>`, drawn at [`variety_share`] of its spread.
#[must_use]
pub fn variety(
    looks: &BTreeMap<String, OrganLook>,
    spreads: &serde_json::Map<String, serde_json::Value>,
    variety: usize,
) -> BTreeMap<String, OrganLook> {
    use serde_json::Value;
    let mut drawn = looks.clone();
    let Some(Value::Object(organs)) = spreads.get("organs") else {
        return drawn;
    };
    for (organ, spread) in organs {
        let (Some(look), Value::Object(spread)) = (looks.get(organ), spread) else {
            continue;
        };
        let Ok(mut value) = serde_json::to_value(look) else {
            continue;
        };
        for (path, spread) in crate::inherit::Spread::all(spread) {
            let at = spread.at(variety_share(variety, &format!("organs.{organ}.{path}")));
            #[allow(clippy::cast_possible_truncation)]
            let number = if spread.whole {
                Value::from(at as i64)
            } else {
                Value::from(at)
            };
            if let Some(slot) = value
                .pointer_mut(&format!("/{}", path.replace('.', "/")))
                .filter(|slot| slot.is_number())
            {
                *slot = number;
            }
        }
        if let Ok(look) = serde_json::from_value(value) {
            drawn.insert(organ.clone(), look);
        }
    }
    drawn
}

impl Look {
    /// The look organ `id` is drawn with: its variety ([`variety_of`])
    /// where the look varies, else the look itself.
    #[must_use]
    pub fn of(&self, id: u64) -> &Look {
        self.varieties.get(variety_of(id)).unwrap_or(self)
    }

    /// Whether organs of this look are drawn one by one as solids near
    /// the camera, so they can show its varieties.
    #[must_use]
    pub fn draws_solid(&self) -> bool {
        self.solid.is_none() && self.form.solid_at(0)
    }

    /// How the organ's card sits on it: as its shape's, or as its
    /// cluster's (`crate::clusters::CardFrame`).
    #[must_use]
    pub fn mount(&self) -> Mount {
        match &self.inflorescence {
            Some(cluster) => cluster.card.mount,
            None => self.shape.mount(),
        }
    }

    /// Card width over card length.
    #[must_use]
    pub fn aspect(&self) -> f64 {
        match &self.inflorescence {
            Some(cluster) => cluster.card.aspect,
            None => self.shape.aspect(),
        }
    }

    /// Width of the card across the first, relative to it, if it has one
    /// (see [`Shape::cross`]).
    #[must_use]
    pub fn cross(&self) -> Option<f64> {
        match &self.inflorescence {
            Some(cluster) => cluster.card.cross,
            None => self.shape.cross(),
        }
    }

    /// Where the card is, in lengths of its organ: `(foot, length)`, its
    /// base that far along the organ (a facing card's centre) and its
    /// length. `(0, 1)` for every look but a cluster's.
    #[must_use]
    pub fn span(&self) -> (f64, f64) {
        self.inflorescence.as_ref().map_or((0.0, 1.0), |cluster| {
            (cluster.card.foot, cluster.card.length)
        })
    }

    /// The petiole its leaf is drawn on, apart from its card, where its
    /// [`Insertion`] is not marginal; `None` for a leaf drawn on one card,
    /// as every other organ is.
    #[must_use]
    pub fn stalk(&self) -> Option<Stalk> {
        if self.insertion.is_marginal() || self.inflorescence.is_some() || self.solid.is_some() {
            return None;
        }
        let (junction, peltate_at) = self.shape.junction()?;
        let attach = match self.insertion {
            Insertion::Peltate => peltate_at,
            _ => junction,
        };
        let (_, along) = crate::templates::drawn_share(&self.shape);
        Some(Stalk {
            petiole: junction * along,
            attach: attach * along,
        })
    }
}

/// One look per organ type, in the program's declaration order. `organs`
/// lists the program's organ types by name and kind (see
/// [`crate::lsys::Program::organs`]); `looks` are the species' looks by
/// organ name.
#[must_use]
pub fn resolve<'a>(
    organs: impl IntoIterator<Item = (&'a str, OrganKind)>,
    looks: &BTreeMap<String, OrganLook>,
    foliage: [f32; 3],
    foliage_shade: [f32; 3],
) -> Vec<Look> {
    organs
        .into_iter()
        .map(|(name, kind)| match looks.get(name) {
            Some(look) => {
                let colour = look.colour.unwrap_or(foliage);
                let drawn = look.shape.with_bracts(foliage);
                let shade = look.shade.unwrap_or(if look.colour.is_some() {
                    [colour[0] * 0.35, colour[1] * 0.42, colour[2] * 0.55]
                } else {
                    foliage_shade
                });
                Look {
                    organ: name.to_string(),
                    colour,
                    shade,
                    accent: look.accent.unwrap_or(colour),
                    underside: look.underside,
                    variegation: look.variegation,
                    face_up: look.face_up,
                    roll: look.roll,
                    insertion: look.insertion,
                    solid: look.solid.clone(),
                    bend: look
                        .bend
                        .unwrap_or_else(|| crate::bend::Bend::default_for(&look.shape)),
                    form: look
                        .form
                        .clone()
                        .unwrap_or_else(|| crate::blooms::Form::default_for(&look.shape)),
                    forms: Forms::default(),
                    inflorescence: look.inflorescence.as_ref().and_then(|inflorescence| {
                        crate::clusters::ClusterLook::new(
                            crate::clusters::Cluster::of(inflorescence),
                            &drawn,
                            &look
                                .season
                                .as_ref()
                                .map(|season| season.units(&drawn, colour, foliage))
                                .unwrap_or_default(),
                        )
                    }),
                    shape: drawn,
                    varieties: Vec::new(),
                    hairs: look.hairs.clone().filter(Hairs::drawn),
                }
            }
            None => Look {
                underside: None,
                variegation: None,
                organ: name.to_string(),
                bend: crate::bend::Bend::default_for(&Shape::default_for(kind)),
                form: crate::blooms::Form::default_for(&Shape::default_for(kind)),
                shape: Shape::default_for(kind),
                colour: foliage,
                shade: foliage_shade,
                accent: foliage,
                face_up: 0.0,
                roll: 0.0,
                insertion: Insertion::Marginal,
                solid: None,
                forms: Forms::default(),
                inflorescence: None,
                varieties: Vec::new(),
                hairs: None,
            },
        })
        .collect::<Vec<_>>()
        .with_families(looks)
}

/// Looks extended by their leaves' families.
trait WithFamilies {
    fn with_families(self, looks: &BTreeMap<String, OrganLook>) -> Self;
}

impl WithFamilies for Vec<Look> {
    /// The looks of the organ types, then each family's look (sun, shade,
    /// juvenile) of the types that have families, which each type's
    /// [`Forms`] point to.
    fn with_families(mut self, looks: &BTreeMap<String, OrganLook>) -> Self {
        let types = self.len();
        for index in 0..types {
            let Some(families) = looks
                .get(&self[index].organ)
                .and_then(|own| own.families.as_ref())
            else {
                continue;
            };
            let own = self[index].clone();
            let mut forms = Forms::default();
            let variant = |shape: Shape| Look {
                bend: crate::bend::Bend::default_for(&shape),
                form: crate::blooms::Form::default_for(&shape),
                shape,
                solid: None,
                forms: Forms::default(),
                inflorescence: None,
                ..own.clone()
            };
            if families.plasticity > 0.0 && own.shape.in_light(1.0, families.plasticity).is_some() {
                let mut bands = [index; BANDS];
                for (band, at) in bands.iter_mut().enumerate() {
                    let form = band_form(band);
                    if form == 0.0 {
                        continue;
                    }
                    let Some((shape, _)) = own.shape.in_light(form, families.plasticity) else {
                        continue;
                    };
                    *at = self.len();
                    self.push(Look {
                        bend: own.bend,
                        ..variant(shape)
                    });
                }
                forms.bands = Some(bands);
                forms.plasticity = families.plasticity;
            }
            if let Some(juvenile) = &families.juvenile {
                forms.juvenile = Some((self.len(), juvenile.until));
                self.push(variant(juvenile.shape.clone()));
            }
            self[index].forms = forms;
        }
        self
    }
}

/// The looks of [`resolve`] on day `day` of the year, with each organ
/// type's size there as a share of its size in fruit, 0 for organs gone
/// that day. An organ with a [`Season`] shows its stage's look: in bud its
/// bud look; in flower its flower look, or its own if that is not fruit;
/// in fruit its own look, its fruit that share unripe, or nothing if its
/// own look is a flower. A cluster stays at its full size, each of its
/// units in its own stage's look (spec rework L1b), its card in the two
/// colours most of them show, unless the stage's look is `whole`. Organs
/// without a season, and every organ when `day` is `None`, keep their own
/// look at full size.
#[must_use]
pub fn resolve_on<'a>(
    organs: impl IntoIterator<Item = (&'a str, OrganKind)>,
    looks: &BTreeMap<String, OrganLook>,
    foliage: [f32; 3],
    foliage_shade: [f32; 3],
    day: Option<f64>,
) -> (Vec<Look>, Vec<f64>) {
    resolve(organs, looks, foliage, foliage_shade)
        .into_iter()
        .map(|look| {
            let season = looks.get(&look.organ).and_then(|own| own.season.as_ref());
            match (season, day) {
                (Some(season), Some(day)) => {
                    in_season(look, &season.with_bracts(foliage), day, foliage)
                }
                _ => (look, 1.0),
            }
        })
        .unzip()
}

/// `graph` as drawn with each organ type's `sizes` from [`resolve_on`]:
/// organs gone that day left out, the others at their stage's size; `None`
/// when every size is 1, so the graph is drawn as it is.
#[must_use]
pub fn staged(graph: &crate::graph::PlantGraph, sizes: &[f64]) -> Option<crate::graph::PlantGraph> {
    if sizes.iter().all(is_full_size) {
        return None;
    }
    let size = |organ: &crate::graph::GraphOrgan| {
        sizes.get(usize::from(organ.organ)).copied().unwrap_or(1.0)
    };
    Some(crate::graph::PlantGraph {
        organs: graph
            .organs
            .iter()
            .filter(|organ| size(organ) > 0.0)
            .map(|organ| crate::graph::GraphOrgan {
                size: organ.size * size(organ),
                ..organ.clone()
            })
            .collect(),
        ..graph.clone()
    })
}

/// `look` as it is on `day` of `season`, and its size as a share of its
/// size as grown: a lone organ's every stage, and a cluster in fruit,
/// scaled by the season's `fruit_size`.
fn in_season(look: Look, season: &Season, day: f64, foliage: [f32; 3]) -> (Look, f64) {
    let lone = look.inflorescence.is_none();
    let fruit = matches!(season.stage(day), Stage::Fruit { .. });
    let (look, size) = in_stage_of(look, season, day, foliage);
    if lone || fruit {
        (look, size * season.fruit_size)
    } else {
        (look, size)
    }
}

/// `look` as it is on `day` of `season`, and its size as a share of its
/// size in fruit.
fn in_stage_of(look: Look, season: &Season, day: f64, foliage: [f32; 3]) -> (Look, f64) {
    let stage = season.stage(day);
    let whole = match stage {
        Stage::Bud => season.bud(&look.shape, look.colour, foliage).whole,
        Stage::Flower => season
            .flower_look
            .as_ref()
            .is_some_and(|flower| flower.whole),
        Stage::Gone | Stage::Fruit { .. } => false,
    };
    if look.inflorescence.is_some() && !whole && stage != Stage::Gone {
        return in_units(look, season, day, stage, foliage);
    }
    let is_fruit = matches!(look.shape, Shape::Fruit(_));
    // A head opens by the day, whichever stage look draws it.
    let open = season.open(day);
    match stage {
        Stage::Gone => (look, 0.0),
        Stage::Bud => {
            let bud = season.bud(&look.shape, look.colour, foliage);
            if season.bud_look.is_none() && matches!(look.shape, Shape::Head(_)) {
                // The head itself, in its own colours and form.
                return (opened(look, open), bud.size);
            }
            if season.bud_look.is_none() {
                // A lone flower's bud is the flower itself, closed (spec
                // rework P5).
                let flower = season
                    .flower_look
                    .as_ref()
                    .map_or_else(|| look.clone(), |flower| in_stage(&look, flower));
                if let Shape::Flower(shape) = &flower.shape {
                    let closed = Shape::Flower(Flower {
                        open: 0.0,
                        ..shape.clone()
                    });
                    return (
                        Look {
                            shape: closed,
                            ..flower
                        },
                        bud.size,
                    );
                }
            }
            (opened(in_stage(&look, &bud), open), bud.size)
        }
        Stage::Flower => match &season.flower_look {
            Some(flower) => (
                flower_opened(opened(in_stage(&look, flower), open), open),
                flower.size,
            ),
            None if is_fruit => (look, 0.0),
            None => (flower_opened(opened(look, open), open), 1.0),
        },
        Stage::Fruit { unripe } => {
            if matches!(look.shape, Shape::Flower(_)) {
                return (look, 0.0);
            }
            let mut look = opened(look, open);
            if let Shape::Fruit(fruit) = &mut look.shape {
                fruit.unripe = unripe;
            }
            (look, 1.0)
        }
    }
}

/// `look` with its flower, if it draws one, opened `open` of the way
/// from closed to as open as its shape sets (see [`Flower::open`]): over
/// a season's `opening` days from its `flower`.
fn flower_opened(mut look: Look, open: f64) -> Look {
    if let Shape::Flower(flower) = &mut look.shape
        && open < 1.0
    {
        flower.open *= open.clamp(0.0, 1.0);
    }
    look
}

/// `look` with its head, if it draws one, opened to `open` (see
/// [`Head::open`]).
fn opened(mut look: Look, open: f64) -> Look {
    if let Shape::Head(head) = &mut look.shape {
        head.open = open;
    }
    look
}

/// A cluster's `look` on `day` of `season`, `stage` its first unit's,
/// which is not `whole`: each unit in a stage of its own (see [`Season`]),
/// the cluster at its full size, or gone while none of its units shows.
#[allow(clippy::float_cmp)]
fn in_units(
    mut look: Look,
    season: &Season,
    day: f64,
    stage: Stage,
    foliage: [f32; 3],
) -> (Look, f64) {
    let own_fruit = matches!(look.shape, Shape::Fruit(_));
    if season.opening == 0.0 {
        // Every unit in the look's own stage: drawn as the look is, its
        // fruit as unripe as the cluster.
        match stage {
            Stage::Fruit { unripe } if own_fruit => {
                if let Shape::Fruit(fruit) = &mut look.shape {
                    fruit.unripe = unripe;
                }
                if let Some(cluster) = &mut look.inflorescence
                    && let Some(unit) = crate::clusters::Unit::of(&look.shape)
                {
                    cluster.unit = unit;
                }
                return (look, 1.0);
            }
            Stage::Flower if !own_fruit && season.flower_look.is_none() => {
                return (opened(look, 1.0), 1.0);
            }
            _ => {}
        }
    }
    // A head is a unit too: its florets in bud, open, or gone to seed in
    // fruit (organ phases, spec rework M7), each head in its own stage.
    let head = matches!(look.shape, Shape::Head(_));
    let unit = |stage: &StageLook, open: f64| {
        let colour = stage.colour.unwrap_or(look.colour);
        let mut shape = stage.shape.clone();
        if let Shape::Head(head) = &mut shape {
            head.open = open;
        }
        (!stage.whole).then(|| crate::clusters::StageUnit {
            form: stage
                .form
                .clone()
                .unwrap_or_else(|| crate::blooms::Form::default_for(&stage.shape)),
            shape,
            colour,
            accent: stage.accent.unwrap_or(colour),
            size: stage.size,
        })
    };
    let own = |open: f64| crate::clusters::StageUnit {
        shape: opened(look.clone(), open).shape,
        form: look.form.clone(),
        colour: look.colour,
        accent: look.accent,
        size: 1.0,
    };
    let flower = match &season.flower_look {
        Some(flower) => unit(flower, 1.0),
        None => (!own_fruit).then(|| own(1.0)),
    };
    let opening = crate::clusters::Opening {
        season: Season {
            bud_look: None,
            flower_look: None,
            ..season.clone()
        },
        day,
        looks: [
            unit(&season.bud(&look.shape, look.colour, foliage), 0.0),
            flower,
            (own_fruit || head).then(|| own(2.0)),
        ],
    };
    let Some(cluster) = &mut look.inflorescence else {
        return (look, 1.0);
    };
    let laid = cluster.layout(0.0, 0);
    if !laid.units.iter().any(|p| opening.showing(p).is_some()) {
        return (look, 0.0);
    }
    // The card's two colours.
    let (colour, accent) = opening.slots(&laid);
    cluster.opening = Some(Box::new(opening));
    if colour != look.colour {
        look.shade = [colour[0] * 0.35, colour[1] * 0.42, colour[2] * 0.55];
    }
    look.colour = colour;
    look.accent = accent;
    (look, 1.0)
}

/// `look` with a stage's shape and colours.
fn in_stage(look: &Look, stage: &StageLook) -> Look {
    let colour = stage.colour.unwrap_or(look.colour);
    Look {
        underside: None,
        variegation: None,
        organ: look.organ.clone(),
        shape: stage.shape.clone(),
        colour,
        shade: [colour[0] * 0.35, colour[1] * 0.42, colour[2] * 0.55],
        accent: stage.accent.unwrap_or(colour),
        face_up: look.face_up,
        roll: 0.0,
        // A stage's look is a flower or a fruit, on no petiole.
        insertion: Insertion::Marginal,
        solid: None,
        bend: crate::bend::Bend::default_for(&stage.shape),
        form: crate::blooms::Form::default_for(&stage.shape),
        forms: Forms::default(),
        // A stage look draws the whole organ.
        inflorescence: None,
        varieties: Vec::new(),
        hairs: None,
    }
}

/// A swollen stem (plant forms F5): a baobab's or a bottle tree's trunk.
/// At height `h` below `height` the stem is `1 + amount * w(h / height)`
/// times as thick, with `w(x) = max(0, 1 - ((x - peak) / s)^2)` and
/// `s = max(peak, 1 - peak)`: thickest at the share `peak` of `height`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bottle {
    /// Height of the swollen part, metres.
    pub height: f64,
    /// How much thicker the stem is where it is thickest, as a fraction.
    pub amount: f64,
    /// Where it is thickest, as a share of `height`.
    pub peak: f64,
}

impl Default for Bottle {
    fn default() -> Self {
        Self {
            height: 6.0,
            amount: 1.0,
            peak: 0.3,
        }
    }
}

impl Bottle {
    /// Check the values.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        within("bottle height", self.height, 0.1, 50.0)?;
        within("bottle amount", self.amount, 0.0, 5.0)?;
        within("bottle peak", self.peak, 0.0, 1.0)
    }

    /// How much thicker the stem is `height` metres above the ground.
    #[must_use]
    pub fn factor(&self, height: f64) -> f64 {
        if self.height <= 0.0 {
            return 1.0;
        }
        let x = (height / self.height).max(0.0);
        let span = self.peak.max(1.0 - self.peak).max(1.0e-3);
        1.0 + self.amount * (1.0 - ((x - self.peak) / span).powi(2)).max(0.0)
    }
}

/// A widened stem base: root flare and buttresses. At height `h` the stem
/// is up to `1 + amount * exp(-h / height)` times as thick: all round, or
/// with `ridge` above 0 most at its buttresses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flare {
    /// Height over which the flare fades, metres.
    pub height: f64,
    /// How much thicker the stem is at the ground, as a fraction: all
    /// round, or at the crest of each buttress.
    pub amount: f64,
    /// Buttresses round the base; 0 for a round flare.
    pub buttresses: u32,
    /// How much of the flare is in the buttresses: 0 for a round flare, 1
    /// for buttresses with no flare between them.
    pub ridge: f64,
}

impl Default for Flare {
    fn default() -> Self {
        Self {
            height: 0.8,
            amount: 0.5,
            buttresses: 0,
            ridge: 0.5,
        }
    }
}

/// Furrowed bark, cut into the wood's surface: ridges running up each
/// stem and branch thick enough to show them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ridges {
    /// Distance between ridges round the base of a stem, metres. Ridges
    /// merge as a stem tapers.
    pub spacing: f64,
    /// Furrow depth as a fraction of the radius.
    pub depth: f64,
    /// How far the ridges spiral round the stem, degrees per metre of
    /// stem.
    pub twist: f64,
    /// How much darker the furrows are, 0 to 1.
    pub contrast: f32,
}

impl Default for Ridges {
    fn default() -> Self {
        Self {
            spacing: 0.08,
            depth: 0.05,
            twist: 0.0,
            contrast: 0.35,
        }
    }
}

/// Moss on thick wood: on the upper side of limbs and round the base of
/// the stem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Moss {
    /// Linear RGB.
    pub colour: [f32; 3],
    /// Cover where moss grows best, 0 to 1.
    pub amount: f32,
    /// Wood thinner than this radius stays bare, metres.
    pub min_radius: f64,
}

impl Default for Moss {
    fn default() -> Self {
        Self {
            colour: [0.07, 0.13, 0.03],
            amount: 0.6,
            min_radius: 0.05,
        }
    }
}

/// Check that `value` lies in `low..=high`.
pub(crate) fn within(what: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
    if (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be between {low} and {high}, found {value}"
        ))
    }
}

impl Flare {
    /// Check the values.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        within("flare height", self.height, 0.05, 20.0)?;
        within("flare amount", self.amount, 0.0, 3.0)?;
        within("flare buttresses", f64::from(self.buttresses), 0.0, 16.0)?;
        within("flare ridge", self.ridge, 0.0, 1.0)
    }
}

impl Ridges {
    /// Check the values.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        within("ridges spacing", self.spacing, 0.01, 2.0)?;
        within("ridges depth", self.depth, 0.0, 0.5)?;
        within("ridges twist", self.twist, -720.0, 720.0)?;
        within("ridges contrast", f64::from(self.contrast), 0.0, 1.0)
    }
}

impl Moss {
    /// Check the values.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        for channel in self.colour {
            within("moss colour", f64::from(channel), 0.0, 1.0)?;
        }
        within("moss amount", f64::from(self.amount), 0.0, 1.0)?;
        within("moss min_radius", self.min_radius, 0.0, 5.0)
    }
}

impl OrganLook {
    /// Check that `season`'s cluster keys suit the look: `opening` is for
    /// a cluster or a head (whose florets open over it), `whole` for a
    /// cluster, and a cluster's stage looks that are not `whole` draw one
    /// unit each.
    fn check_stages(&self, season: &Season) -> Result<(), String> {
        let looks = [
            ("bud_look", &season.bud_look),
            ("flower_look", &season.flower_look),
        ];
        if self.inflorescence.is_none() {
            let flower = season
                .flower_look
                .as_ref()
                .map_or(&self.shape, |look| &look.shape);
            if season.opening > 0.0
                && !matches!(self.shape, Shape::Head(_))
                && !matches!(flower, Shape::Flower(_))
            {
                return Err(
                    "season opening is for a look with an inflorescence, a head or a flower"
                        .to_string(),
                );
            }
            if let Some((key, _)) = looks
                .iter()
                .find(|(_, look)| look.as_ref().is_some_and(|look| look.whole))
            {
                return Err(format!(
                    "season {key} `whole` is for a look with an inflorescence; any other's \
                     stage looks draw its whole organ"
                ));
            }
            return Ok(());
        }
        for (key, look) in looks {
            if let Some(look) = look
                && !look.whole
                && crate::clusters::Unit::of(&look.shape).is_none()
            {
                return Err(format!(
                    "season {key} of an inflorescence draws each of its units, a flower, a \
                     head, a spikelet or a fruit, found {}; set `whole` to draw the whole organ",
                    look.shape.name()
                ));
            }
        }
        Ok(())
    }

    /// Check the shape and colours.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        self.shape.validate()?;
        if let Some(inflorescence) = &self.inflorescence {
            crate::clusters::Cluster::check(inflorescence)?;
            if crate::clusters::Unit::of(&self.shape).is_none() {
                return Err(format!(
                    "an inflorescence's units are flowers, heads, spikelets or fruit, found {}",
                    self.shape.name()
                ));
            }
            if self.solid.is_some() || self.families.is_some() {
                return Err(
                    "solid leaves and families are for leaves, not an inflorescence".to_string(),
                );
            }
        }
        for colour in [self.colour, self.shade, self.accent, self.underside]
            .into_iter()
            .flatten()
        {
            for channel in colour {
                within("colours", f64::from(channel), 0.0, 1.0)?;
            }
        }
        if let Some(variegation) = &self.variegation {
            variegation.validate()?;
        }
        if let Some(season) = &self.season {
            season.validate()?;
            self.check_stages(season)?;
        }
        if let Some(families) = &self.families {
            if self.season.is_some() {
                return Err("families are for leaves, without a season".to_string());
            }
            families.validate(&self.shape)?;
        }
        if let Some(solid) = &self.solid {
            solid.validate()?;
        }
        if let Some(bend) = &self.bend {
            bend.validate()?;
        }
        if let Some(form) = &self.form {
            form.validate(&self.shape)?;
        }
        if !self.insertion.is_marginal() {
            if self.shape.junction().is_none() {
                return Err(format!(
                    "an insertion is a leaf's, on a petiole; a {} has none",
                    self.shape.name()
                ));
            }
            if self.inflorescence.is_some() || self.solid.is_some() {
                return Err(
                    "an insertion is for a leaf drawn on a card, not a cluster or a solid leaf"
                        .to_string(),
                );
            }
        }
        within("roll", self.roll, -180.0, 180.0)?;
        within("face_up", self.face_up, 0.0, 1.0)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_petal_drawn_from_its_ratio_is_as_wide_among_any_count() {
        let ratio = |petals: u32| Flower {
            petals,
            petal_ratio: Some(0.5),
            ..Flower::default()
        };
        // The plain card's widest petal over its length (`templates.rs`):
        // 1.32 tan(pi / petals) times the share.
        for petals in [3, 5, 8] {
            let flower = ratio(petals);
            let drawn = 1.32
                * crate::math::tan(std::f64::consts::PI / f64::from(petals))
                * flower.petal_share();
            assert!((drawn - 0.5).abs() < 1e-9, "{petals}");
        }
        let unset = Flower::default();
        assert_eq!(unset.petal_share(), unset.petal_width);
    }

    /// Only a leaf on a petiole, drawn on a card, has an insertion.
    #[test]
    fn an_insertion_is_a_leafs() {
        let parse = |text: &str| serde_json::from_str::<OrganLook>(text).unwrap().validate();
        assert!(parse(r#"{"shape": {"template": "simple"}, "insertion": "peltate"}"#).is_ok());
        assert!(parse(r#"{"shape": {"template": "compound"}, "insertion": "latent"}"#).is_ok());
        let flower = parse(r#"{"shape": {"template": "flower"}, "insertion": "peltate"}"#);
        assert!(flower.unwrap_err().contains("a flower has none"));
        assert!(
            serde_json::from_str::<OrganLook>(
                r#"{"shape": {"template": "simple"}, "insertion": "sideways"}"#
            )
            .is_err()
        );
        // Left unset it is marginal, and so not written.
        let marginal =
            serde_json::from_str::<OrganLook>(r#"{"shape": {"template": "simple"}}"#).unwrap();
        assert_eq!(marginal.insertion, Insertion::Marginal);
        assert!(
            !serde_json::to_string(&marginal)
                .unwrap()
                .contains("insertion")
        );
    }

    #[test]
    fn sun_leaves_are_smaller_narrower_and_deeper_than_shade_leaves() {
        let shapes = [
            Shape::Simple(Simple {
                teeth: 12,
                ..Simple::default()
            }),
            Shape::Lobed(Lobed::default()),
            Shape::Palmate(Palmate::default()),
            Shape::Compound(Compound::default()),
            Shape::Sprig(Sprig {
                teeth: 4,
                tooth_depth: 0.1,
                ..Sprig::default()
            }),
        ];
        // (width, depth) of each leaf shape.
        let measure = |shape: &Shape| match shape {
            Shape::Simple(s) => (s.width, s.tooth_depth),
            Shape::Lobed(s) => (s.width, s.depth),
            Shape::Palmate(s) => (s.lobe_width, s.depth),
            Shape::Compound(s) => (s.leaflet_width, 0.0),
            Shape::Sprig(s) => (s.width, s.tooth_depth),
            _ => unreachable!(),
        };
        for shape in shapes {
            let (sun, sun_size) = shape.in_light(1.0, 0.8).unwrap();
            let (shade, shade_size) = shape.in_light(-1.0, 0.8).unwrap();
            let (half_sun, half_size) = shape.in_light(0.5, 0.8).unwrap();
            sun.validate().unwrap();
            shade.validate().unwrap();
            let (own, sun, half_sun, shade) = (
                measure(&shape),
                measure(&sun),
                measure(&half_sun),
                measure(&shade),
            );
            assert!(
                sun.0 < half_sun.0 && half_sun.0 < own.0 && own.0 < shade.0,
                "{shape:?}"
            );
            assert!(
                sun.1 >= half_sun.1 && half_sun.1 >= own.1 && own.1 >= shade.1,
                "{shape:?}"
            );
            assert!(sun_size < half_size && half_size < 1.0 && shade_size > 1.0);
            assert_eq!(shape.in_light(1.0, 0.0).unwrap().1, 1.0);
            assert_eq!(shape.in_light(0.0, 0.8).unwrap(), (shape.clone(), 1.0));
        }
        // A sun leaf's teeth cut deeper into its blade, by share.
        let oak = Shape::Sprig(Sprig {
            width: 0.6,
            teeth: 4,
            tooth_depth: 0.2,
            round: 0.8,
            ..Sprig::default()
        });
        let incision = |shape: &Shape| match shape {
            Shape::Sprig(s) => s.tooth_depth / (0.5 * s.width),
            _ => unreachable!(),
        };
        let (sun, shade) = (
            oak.in_light(1.0, 0.8).unwrap().0,
            oak.in_light(-1.0, 0.8).unwrap().0,
        );
        assert!(incision(&sun) > incision(&oak) + 0.05 && incision(&shade) < incision(&oak) - 0.15);
        assert!(
            Shape::Needles(Needles::default())
                .in_light(1.0, 1.0)
                .is_none()
        );
    }

    #[test]
    fn leaves_acclimate_on_the_logarithm_of_their_light() {
        assert_eq!(sunlit(0.0), 0.0);
        assert_eq!(sunlit(DEEP_SHADE), 0.0);
        assert_eq!(sunlit(FULL_SUN), 1.0);
        assert_eq!(sunlit(1.0), 1.0);
        // Equal ratios of light are equal steps along the scale.
        let middle = crate::math::sqrt(DEEP_SHADE * FULL_SUN);
        assert!((sunlit(middle) - 0.5).abs() < 1e-12);
        assert!(sunlit(0.1) < sunlit(0.2) && sunlit(0.2) < sunlit(0.3));
        // A dense crown's surface (light about 0.3 to 0.4 in the growth
        // model) is sunlit; the leaves where short shoots stop leafing out
        // (0.08) wear their shade form.
        assert!(light_form(0.3) > 0.5 && light_form(0.08) < -0.5);
        assert!(light_form(middle).abs() < 1e-12);
        assert_eq!((light_form(FULL_SUN), light_form(DEEP_SHADE)), (1.0, -1.0));
    }

    #[test]
    fn a_leafs_family_picks_its_look() {
        let organs = BTreeMap::from([(
            "leaf".to_string(),
            OrganLook {
                hairs: None,
                underside: None,
                variegation: None,
                roll: 0.0,
                insertion: crate::looks::Insertion::Marginal,
                shape: Shape::Lobed(Lobed::default()),
                colour: None,
                shade: None,
                accent: None,
                face_up: 0.0,
                solid: None,
                bend: None,
                form: None,
                season: None,
                families: Some(Families {
                    plasticity: 0.5,
                    juvenile: Some(Juvenile {
                        until: Some(3.0),
                        shape: Shape::Simple(Simple::default()),
                    }),
                }),
                inflorescence: None,
            },
        )]);
        organs["leaf"].validate().unwrap();
        let looks = resolve(
            [("leaf", OrganKind::Leaf), ("bloom", OrganKind::Flower)],
            &organs,
            [0.1, 0.3, 0.1],
            [0.05, 0.1, 0.05],
        );
        // The two types' looks, then the leaf's four steps from shade to
        // sun (its own the middle one) and its juvenile look.
        assert_eq!(looks.len(), 7);
        let forms = looks[0].forms;
        assert_eq!(
            (forms.bands, forms.juvenile),
            (Some([2, 3, 0, 4, 5]), Some((6, Some(3.0))))
        );
        assert_eq!(looks[1].forms, Forms::default());
        assert!(
            looks[2..]
                .iter()
                .all(|look| look.organ == "leaf" && look.forms == Forms::default())
        );
        assert!(matches!(looks[6].shape, Shape::Simple(_)));
        // Juvenile on a young plant whatever its light; then by its light,
        // its size smoothly.
        assert_eq!(forms.of(0, &leaf(0.9, 1.0, None, 1)), (6, 1.0));
        assert_eq!(
            forms.of(0, &leaf(0.9, 10.0, None, 1)),
            (5, 1.0 - 0.12 * 0.5)
        );
        assert_eq!(forms.of(0, &leaf(0.2, 10.0, None, 1)).0, 4);
        assert_eq!(forms.of(0, &leaf(0.17, 10.0, None, 1)).0, 0);
        assert_eq!(forms.of(0, &leaf(0.11, 10.0, None, 1)).0, 3);
        assert_eq!(
            forms.of(0, &leaf(0.01, 10.0, None, 1)),
            (2, 1.0 + 0.12 * 0.5)
        );
        let sizes: Vec<f64> = [0.08, 0.1, 0.12, 0.15, 0.2, 0.3]
            .iter()
            .map(|&light| forms.of(0, &leaf(light, 10.0, None, 1)).1)
            .collect();
        assert!(sizes.windows(2).all(|pair| pair[1] < pair[0]), "{sizes:?}");
        // Families are for leaves.
        let mut flower = organs["leaf"].clone();
        flower.shape = Shape::Flower(Flower::default());
        assert!(flower.validate().is_err());
    }

    fn leaf(light: f64, born: f64, adult: Option<f32>, id: u64) -> crate::graph::GraphOrgan {
        crate::graph::GraphOrgan {
            id,
            organ: 0,
            segment: None,
            position: crate::math::Vec3::ZERO,
            heading: crate::math::Vec3::Y,
            left: crate::math::Vec3::X,
            size: 1.0,
            born,
            shed: None,
            light,
            within: None,
            adult,
        }
    }

    #[test]
    fn a_leaf_on_a_juvenile_shoot_wears_the_juvenile_look() {
        // Spec rework P6: a shoot's adultness picks the look, whatever the
        // plant's age; between 0 and 1 a share of its leaves are adult.
        let forms = Forms {
            bands: None,
            plasticity: 0.0,
            juvenile: Some((1, None)),
        };
        assert_eq!(forms.of(0, &leaf(0.3, 30.0, Some(0.0), 7)), (1, 1.0));
        assert_eq!(forms.of(0, &leaf(0.3, 0.5, Some(1.0), 7)), (0, 1.0));
        // Without a recorded adultness and without `until`, adult.
        assert_eq!(forms.of(0, &leaf(0.3, 0.5, None, 7)), (0, 1.0));
        let adult = (0..2000)
            .filter(|&id| forms.of(0, &leaf(0.3, 5.0, Some(0.3), id)).0 == 0)
            .count();
        assert!((500..700).contains(&adult), "{adult}");
        // A recorded adultness wins over `until`.
        let until = Forms {
            juvenile: Some((1, Some(4.0))),
            ..forms
        };
        assert_eq!(until.of(0, &leaf(0.3, 10.0, Some(0.0), 7)), (1, 1.0));
        assert_eq!(until.of(0, &leaf(0.3, 1.0, Some(1.0), 7)), (0, 1.0));
        assert_eq!(until.of(0, &leaf(0.3, 1.0, None, 7)), (1, 1.0));
        // A type without juvenile leaves wears its own look.
        assert_eq!(
            Forms::default().of(0, &leaf(0.3, 1.0, Some(0.0), 7)),
            (0, 1.0)
        );
    }

    #[test]
    fn looks_parse_with_defaults_and_reject_unknown_fields() {
        let look: OrganLook = serde_json::from_str(
            r#"{ "shape": { "template": "palmate", "lobes": 7 }, "face_up": 0.5 }"#,
        )
        .unwrap();
        let Shape::Palmate(palmate) = &look.shape else {
            panic!("not palmate: {look:?}");
        };
        assert_eq!(palmate.lobes, 7);
        assert_eq!(palmate.depth, Palmate::default().depth);
        assert!(look.colour.is_none());
        assert!(
            serde_json::from_str::<OrganLook>(
                r#"{ "shape": { "template": "palmate", "lobs": 7 } }"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<OrganLook>(r#"{ "shape": { "template": "tentacle" } }"#)
                .is_err()
        );
        // Written back, a look reads the same.
        let text = serde_json::to_string(&look).unwrap();
        assert_eq!(serde_json::from_str::<OrganLook>(&text).unwrap(), look);
    }

    #[test]
    fn every_shape_validates_its_defaults_and_fits_its_card() {
        let shapes = [
            Shape::Needles(Needles::default()),
            Shape::Scales(Scales::default()),
            Shape::Palmate(Palmate::default()),
            Shape::Simple(Simple::default()),
            Shape::Lobed(Lobed::default()),
            Shape::Compound(Compound::default()),
            Shape::Sprig(Sprig::default()),
            Shape::Blade(Blade::default()),
            Shape::Frond(Frond::default()),
            Shape::Flower(Flower::default()),
            Shape::Head(Head::default()),
            Shape::Umbel(Umbel::default()),
            Shape::Spike(Spike::default()),
            Shape::Panicle(Panicle::default()),
            Shape::Spikelet(Spikelet::default()),
            Shape::Fruit(Fruit::default()),
        ];
        for shape in shapes {
            shape.validate().unwrap();
            let aspect = shape.aspect();
            assert!((0.01..=1.6).contains(&aspect), "{shape:?}: {aspect}");
        }
        let bad = Shape::Palmate(Palmate {
            lobes: 4,
            ..Palmate::default()
        });
        assert!(bad.validate().unwrap_err().contains("3, 5, 7 or 9"));
    }

    /// An outline runs from base to tip in order, rises and closes at the
    /// tip, and takes the place of `widest`, `tip` and `base`; its card
    /// writes it only when it has one.
    #[test]
    fn outlines_are_checked_and_replace_the_blade_s_shape() {
        let simple = |outline: &[[f64; 2]]| {
            Shape::Simple(Simple {
                outline: Some(outline.to_vec()),
                ..Simple::default()
            })
        };
        let spatulate = [[0.0, 0.1], [0.5, 0.3], [0.8, 1.0], [1.0, 0.0]];
        simple(&spatulate).validate().unwrap();
        Shape::Sprig(Sprig {
            outline: Some(spatulate.to_vec()),
            width: 0.05,
            ..Sprig::default()
        })
        .validate()
        .unwrap();
        for (outline, wanted) in [
            (&[[0.0, 0.0]][..], "2 to 16 points"),
            (&[[0.0, 0.5]; 17][..], "2 to 16 points"),
            (&[[0.1, 0.5], [1.0, 0.0]][..], "from u 0 to u 1 in order"),
            (&[[0.0, 0.5], [0.9, 0.0]][..], "from u 0 to u 1 in order"),
            (
                &[[0.0, 0.5], [0.6, 1.0], [0.5, 0.5], [1.0, 0.0]][..],
                "in order",
            ),
            (
                &[[0.0, 0.5], [0.5, 1.2], [1.0, 0.0]][..],
                "between 0 and 1, found 1.2",
            ),
            (&[[0.0, 0.5], [1.0, 0.1]][..], "end at w 0"),
            (&[[0.0, 0.0], [1.0, 0.0]][..], "rise above w 0"),
        ] {
            let error = simple(outline).validate().unwrap_err();
            assert!(error.contains(wanted), "{outline:?}: {error}");
        }
        let both = Shape::Sprig(Sprig {
            outline: Some(spatulate.to_vec()),
            tip: 0.9,
            ..Sprig::default()
        });
        assert_eq!(
            both.validate().unwrap_err(),
            "a sprig card with an outline takes its shape from it: drop `tip`"
        );
        // Written only when set, so a card without one keeps its bytes.
        let plain = serde_json::to_value(Shape::Simple(Simple::default())).unwrap();
        assert!(plain.get("outline").is_none());
        let drawn = serde_json::to_value(simple(&spatulate)).unwrap();
        assert_eq!(drawn["outline"], serde_json::json!(spatulate));
        let back: Shape = serde_json::from_value(drawn).unwrap();
        back.validate().unwrap();
        assert_eq!(back, simple(&spatulate));
    }

    #[test]
    fn a_bottle_trunk_is_thickest_at_its_peak() {
        let bottle = Bottle {
            height: 10.0,
            amount: 1.5,
            peak: 0.3,
        };
        assert!((bottle.factor(3.0) - 2.5).abs() < 1e-12);
        assert!(bottle.factor(0.0) > 1.0 && bottle.factor(0.0) < bottle.factor(3.0));
        assert!((bottle.factor(10.0) - 1.0).abs() < 1e-12);
        assert!((bottle.factor(20.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn unlisted_organs_get_their_kind_s_default_look() {
        let types = [("leaf", OrganKind::Leaf), ("bloom", OrganKind::Flower)];
        let mut organs = BTreeMap::new();
        organs.insert(
            "bloom".to_string(),
            OrganLook {
                hairs: None,
                underside: None,
                variegation: None,
                roll: 0.0,
                insertion: crate::looks::Insertion::Marginal,
                shape: Shape::Head(Head::default()),
                colour: Some([0.6, 0.2, 0.7]),
                shade: None,
                accent: Some([0.9, 0.7, 0.1]),
                face_up: 0.0,
                solid: None,
                bend: None,
                form: None,
                season: None,
                families: None,
                inflorescence: None,
            },
        );
        let green = [0.1, 0.3, 0.05];
        let dark = [0.03, 0.1, 0.03];
        let looks = resolve(types, &organs, green, dark);
        assert_eq!(looks.len(), 2);
        assert_eq!(looks[0].shape, Shape::Simple(Simple::default()));
        assert_eq!(looks[0].colour, green);
        assert_eq!(looks[0].shade, dark);
        assert_eq!(looks[1].mount(), Mount::Facing);
        assert_eq!(looks[1].accent, [0.9, 0.7, 0.1]);
        // A look with its own colour darkens it for shade.
        assert!(looks[1].shade[0] < 0.6 && looks[1].shade[0] > 0.0);
    }

    fn cherry() -> Season {
        Season {
            bud: 95.0,
            flower: 110.0,
            fruit: 135.0,
            ripe: 175.0,
            ripening: 20.0,
            fall: 245.0,
            opening: 0.0,
            fruit_size: 1.0,
            bud_look: None,
            flower_look: Some(StageLook {
                shape: Shape::Flower(Flower::default()),
                colour: Some([0.95, 0.95, 0.9]),
                accent: None,
                size: 0.5,
                form: None,
                whole: false,
            }),
        }
    }

    #[test]
    fn a_season_runs_from_bud_to_fall() {
        let season = cherry();
        assert!(season.validate().is_ok());
        assert_eq!(season.stage(1.0), Stage::Gone);
        assert_eq!(season.stage(100.0), Stage::Bud);
        assert_eq!(season.stage(120.0), Stage::Flower);
        assert_eq!(season.stage(150.0), Stage::Fruit { unripe: 1.0 });
        assert_eq!(season.stage(185.0), Stage::Fruit { unripe: 0.5 });
        assert_eq!(season.stage(200.0), Stage::Fruit { unripe: 0.0 });
        assert_eq!(season.stage(245.0), Stage::Gone);
        // Fruit kept into the winter shows on the next year's early days.
        let hips = Season {
            fall: 430.0,
            ..cherry()
        };
        assert!(hips.validate().is_ok());
        assert_eq!(hips.stage(20.0), Stage::Fruit { unripe: 0.0 });
        assert_eq!(hips.stage(66.0), Stage::Gone);
        // Days out of order, or a year too long, are refused.
        for wrong in [
            Season {
                fruit: 100.0,
                ..cherry()
            },
            Season {
                fall: 470.0,
                ..cherry()
            },
            Season {
                bud: 0.0,
                ..cherry()
            },
        ] {
            assert!(wrong.validate().is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn an_organ_looks_as_it_is_on_the_day() {
        let types = [("leaf", OrganKind::Leaf), ("bloom", OrganKind::Flower)];
        let fruit = |season: Option<Season>| OrganLook {
            hairs: None,
            underside: None,
            variegation: None,
            roll: 0.0,
            insertion: crate::looks::Insertion::Marginal,
            shape: Shape::Fruit(Fruit::default()),
            colour: Some([0.6, 0.05, 0.06]),
            shade: None,
            accent: Some([0.45, 0.5, 0.15]),
            face_up: 0.0,
            solid: None,
            bend: None,
            form: None,
            season,
            families: None,
            inflorescence: None,
        };
        let mut organs = BTreeMap::new();
        organs.insert("bloom".to_string(), fruit(Some(cherry())));
        let green = [0.1, 0.3, 0.05];
        let dark = [0.03, 0.1, 0.03];
        let on = |organs: &BTreeMap<String, OrganLook>, day| {
            resolve_on(types, organs, green, dark, Some(day))
        };
        // Leaves keep their look all year.
        for day in [1.0, 100.0, 200.0] {
            let (looks, sizes) = on(&organs, day);
            assert_eq!(looks[0], resolve(types, &organs, green, dark)[0]);
            assert_eq!(sizes[0], 1.0);
        }
        // Gone before its bud shows.
        assert_eq!(on(&organs, 50.0).1[1], 0.0);
        // Its flower closed, a third of its flower's size (spec rework
        // P5), in its flower's colours.
        let (looks, sizes) = on(&organs, 100.0);
        assert!(matches!(&looks[1].shape, Shape::Flower(f) if f.open == 0.0));
        assert_eq!(looks[1].colour, [0.95, 0.95, 0.9]);
        assert!((sizes[1] - 0.5 / 3.0).abs() < 1e-12);
        // Its flower look in flower.
        let (looks, sizes) = on(&organs, 120.0);
        assert!(matches!(looks[1].shape, Shape::Flower(_)));
        assert_eq!(looks[1].colour, [0.95, 0.95, 0.9]);
        assert_eq!(sizes[1], 0.5);
        // Its own look in fruit, ripening.
        let (looks, sizes) = on(&organs, 185.0);
        let Shape::Fruit(drawn) = &looks[1].shape else {
            panic!("not fruit: {:?}", looks[1].shape);
        };
        assert_eq!(drawn.unripe, 0.5);
        assert_eq!(sizes[1], 1.0);
        // Without a day it keeps its own look; without a season too.
        let (looks, sizes) = resolve_on(types, &organs, green, dark, None);
        assert_eq!(looks, resolve(types, &organs, green, dark));
        assert_eq!(sizes, vec![1.0, 1.0]);
        // A flower without a fruit look is gone once its fruit sets.
        organs.insert(
            "bloom".to_string(),
            OrganLook {
                shape: Shape::Flower(Flower::default()),
                ..fruit(Some(Season {
                    flower_look: None,
                    ..cherry()
                }))
            },
        );
        assert_eq!(on(&organs, 120.0).1[1], 1.0);
        assert_eq!(on(&organs, 150.0).1[1], 0.0);
    }

    /// A daisy's head: in bud from day 120, its florets opening over 10
    /// days from 150, going to seed over 10 days from 170.
    fn daisy() -> Season {
        Season {
            bud: 120.0,
            flower: 150.0,
            fruit: 170.0,
            ripe: 185.0,
            ripening: 10.0,
            fall: 200.0,
            opening: 10.0,
            fruit_size: 1.0,
            bud_look: None,
            flower_look: None,
        }
    }

    /// Looks with a `bloom` that is a head with `season`, alone or in
    /// `inflorescence`.
    fn headed(season: Season, inflorescence: Option<Inflorescence>) -> BTreeMap<String, OrganLook> {
        let mut organs = BTreeMap::new();
        organs.insert(
            "bloom".to_string(),
            OrganLook {
                hairs: None,
                underside: None,
                variegation: None,
                roll: 0.0,
                insertion: crate::looks::Insertion::Marginal,
                shape: Shape::Head(Head::default()),
                colour: Some([0.95, 0.95, 0.9]),
                shade: None,
                accent: Some([0.9, 0.7, 0.1]),
                face_up: 0.0,
                solid: None,
                bend: None,
                form: None,
                season: Some(season),
                families: None,
                inflorescence,
            },
        );
        organs
    }

    #[test]
    fn a_head_opens_from_its_rim_by_the_day() {
        let daisy = daisy();
        assert_eq!(daisy.open(130.0), 0.0);
        assert_eq!(daisy.open(150.0), 0.0);
        assert_eq!(daisy.open(155.0), 0.5);
        assert_eq!(daisy.open(165.0), 1.0);
        assert_eq!(daisy.open(172.0), 1.2);
        assert_eq!(daisy.open(190.0), 2.0);
        let all_at_once = Season {
            opening: 0.0,
            ..daisy.clone()
        };
        assert_eq!(all_at_once.open(150.0), 1.0);
        assert_eq!(all_at_once.open(170.0), 2.0);

        let types = [("bloom", OrganKind::Flower)];
        let open = |look: &Look| match &look.shape {
            Shape::Head(head) => head.open,
            other => panic!("not a head: {other:?}"),
        };
        let organs = headed(daisy, None);
        // Its florets may open over days, as a cluster's units and a lone
        // flower's petals may; a fruit's may not.
        assert!(organs["bloom"].validate().is_ok());
        let flower = OrganLook {
            shape: Shape::Flower(Flower::default()),
            ..organs["bloom"].clone()
        };
        assert!(flower.validate().is_ok());
        let fruit = OrganLook {
            shape: Shape::Fruit(Fruit::default()),
            ..organs["bloom"].clone()
        };
        assert!(fruit.validate().unwrap_err().contains("opening"));
        let on = |day| {
            resolve_on(
                types,
                &organs,
                [0.1, 0.3, 0.05],
                [0.03, 0.1, 0.03],
                Some(day),
            )
        };
        // In bud it is itself, closed, a third its size, in its own colours.
        let (looks, sizes) = on(130.0);
        assert_eq!(open(&looks[0]), 0.0);
        assert!((sizes[0] - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(looks[0].colour, [0.95, 0.95, 0.9]);
        // Then open from its rim inward, and to seed, at its full size.
        for (day, opened) in [(155.0, 0.5), (165.0, 1.0), (175.0, 1.5), (190.0, 2.0)] {
            let (looks, sizes) = on(day);
            assert!((open(&looks[0]) - opened).abs() < 1e-12, "day {day}");
            assert_eq!(sizes[0], 1.0);
        }
        assert_eq!(on(200.0).1[0], 0.0);
        // Without a day it keeps the `open` its look sets.
        let (looks, _) = resolve_on(types, &organs, [0.0; 3], [0.0; 3], None);
        assert_eq!(open(&looks[0]), Head::default().open);
    }

    #[test]
    fn a_clusters_heads_each_open_in_their_own_stage() {
        // A corymb of heads opening over 40 days: each head a unit in its
        // own stage, its florets all in bud, all open, or all in seed.
        let corymb = Some(Inflorescence {
            kind: Some(ClusterKind::Corymb),
            flowers: Some(12),
            ..Inflorescence::default()
        });
        let organs = headed(
            Season {
                opening: 40.0,
                ..daisy()
            },
            corymb,
        );
        let types = [("bloom", OrganKind::Flower)];
        let (looks, _) = resolve_on(types, &organs, [0.0; 3], [0.0; 3], Some(172.0));
        let cluster = looks[0].inflorescence.as_ref().expect("a cluster");
        let opening = cluster.opening.as_ref().expect("units in stages");
        let shown: Vec<f64> = cluster
            .layout(0.0, 0)
            .units
            .iter()
            .filter_map(|p| opening.showing(p))
            .map(|(unit, _)| match &unit.shape {
                Shape::Head(head) => head.open,
                other => panic!("not a head: {other:?}"),
            })
            .collect();
        for stage in [0.0, 1.0, 2.0] {
            assert!(shown.contains(&stage), "{shown:?}");
        }
    }

    /// A cherry's umbel of six (or `kind` of `flowers`), with `season`.
    fn clustered(kind: ClusterKind, flowers: u32, season: Season) -> OrganLook {
        OrganLook {
            hairs: None,
            underside: None,
            variegation: None,
            roll: 0.0,
            insertion: crate::looks::Insertion::Marginal,
            shape: Shape::Fruit(Fruit::default()),
            colour: Some([0.6, 0.05, 0.06]),
            shade: None,
            accent: Some([0.45, 0.5, 0.15]),
            face_up: 0.0,
            solid: None,
            bend: None,
            form: None,
            season: Some(season),
            families: None,
            inflorescence: Some(Inflorescence {
                kind: Some(kind),
                flowers: Some(flowers),
                ..Inflorescence::default()
            }),
        }
    }

    /// The stage each unit of `look`'s cluster shows (0 bud, 1 flower, 2
    /// fruit), and its height up the organ, lowest first.
    fn stages_of(look: &Look) -> Vec<(f64, Option<usize>)> {
        let cluster = look.inflorescence.as_ref().expect("a cluster");
        let opening = cluster.opening.as_ref().expect("units in stages");
        let mut units: Vec<(f64, Option<usize>)> = cluster
            .layout(0.0, 0)
            .units
            .iter()
            .map(|p| (p.base.y, opening.shown(p).map(|(stage, _)| stage)))
            .collect();
        units.sort_by(|a, b| a.0.total_cmp(&b.0));
        units
    }

    #[test]
    fn a_clusters_units_open_one_after_another() {
        let types = [("bloom", OrganKind::Flower)];
        let on = |look: OrganLook, day: f64| {
            let mut organs = BTreeMap::new();
            organs.insert("bloom".to_string(), look);
            let (mut looks, sizes) = resolve_on(
                types,
                &organs,
                [0.1, 0.3, 0.05],
                [0.03, 0.1, 0.03],
                Some(day),
            );
            (looks.remove(0), sizes[0])
        };
        // All at once: buds, then flowers, each unit its own, the cluster
        // at its full size.
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, cherry()), 100.0);
        assert_eq!(size, 1.0);
        assert!(stages_of(&look).iter().all(|&(_, stage)| stage == Some(0)));
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, cherry()), 120.0);
        assert_eq!(size, 1.0);
        assert!(stages_of(&look).iter().all(|&(_, stage)| stage == Some(1)));
        // The card's colours are the flower's.
        assert_eq!(look.colour, [0.95, 0.95, 0.9]);
        assert_eq!(look.accent, [0.95, 0.95, 0.9]);
        // In fruit every unit is its own look, as before units had stages.
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, cherry()), 185.0);
        assert_eq!(size, 1.0);
        let cluster = look.inflorescence.as_ref().expect("its cluster");
        assert!(cluster.opening.is_none());
        assert_eq!(cluster.cluster.flowers, 6);
        assert!(matches!(
            cluster.unit,
            crate::clusters::Unit::Fruit { unripe, .. } if (unripe - 0.5).abs() < 1e-12
        ));
        // Opening over 40 days, a raceme is fruit at its foot, flowers in
        // its middle and buds at its top.
        let slow = Season {
            opening: 40.0,
            ..cherry()
        };
        let (look, size) = on(clustered(ClusterKind::Raceme, 12, slow.clone()), 140.0);
        assert_eq!(size, 1.0);
        let stages: Vec<usize> = stages_of(&look)
            .iter()
            .map(|&(_, stage)| stage.expect("every unit shows"))
            .collect();
        assert!(
            stages.windows(2).all(|pair| pair[0] >= pair[1]),
            "{stages:?}"
        );
        for stage in 0..3 {
            assert!(stages.contains(&stage), "{stages:?}");
        }
        // Its card takes the colours most of it shows.
        let (colour, accent) = (look.colour, look.accent);
        assert_ne!(colour, accent);
        for slot in [colour, accent] {
            assert!(
                [[0.95, 0.95, 0.9], [0.6, 0.05, 0.06], [0.45, 0.5, 0.15]].contains(&slot)
                    || slot
                        == slow
                            .bud(
                                &Shape::default_for(OrganKind::Fruit),
                                [0.6, 0.05, 0.06],
                                [0.1, 0.3, 0.05]
                            )
                            .colour
                            .unwrap(),
                "{slot:?}"
            );
        }
        // Once the last has set fruit, units ripen by their rank.
        let (look, _) = on(clustered(ClusterKind::Raceme, 12, slow.clone()), 200.0);
        let opening = look.inflorescence.as_ref().and_then(|c| c.opening.as_ref());
        assert!(opening.is_some());
        assert!(stages_of(&look).iter().all(|&(_, stage)| stage == Some(2)));
        // A whole flower look draws the whole organ in flower, as other
        // organs' stage looks do.
        let mut whole = cherry();
        if let Some(flower) = &mut whole.flower_look {
            flower.whole = true;
        }
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, whole.clone()), 120.0);
        assert!(look.inflorescence.is_none());
        assert!(matches!(look.shape, Shape::Flower(_)));
        assert_eq!(size, 0.5);
        // So is its bud, the flower closed, unless a bud look of its own
        // says not.
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, whole), 100.0);
        assert!(look.inflorescence.is_none());
        assert!(matches!(&look.shape, Shape::Flower(f) if f.open == 0.0));
        assert!((size - 0.5 / 3.0).abs() < 1e-12, "{size}");
        // Fruit with no flower look shows nothing in flower: gone.
        let bare = Season {
            flower_look: None,
            ..cherry()
        };
        assert_eq!(on(clustered(ClusterKind::Umbel, 6, bare), 120.0).1, 0.0);
    }

    #[test]
    fn a_clusters_stage_looks_draw_units_unless_whole() {
        let mut look = clustered(ClusterKind::Raceme, 8, cherry());
        assert!(look.validate().is_ok());
        let spike = StageLook {
            shape: Shape::Spike(Spike::default()),
            colour: None,
            accent: None,
            size: 1.0,
            form: None,
            whole: false,
        };
        if let Some(season) = &mut look.season {
            season.flower_look = Some(spike.clone());
        }
        let error = look.validate().unwrap_err();
        assert!(
            error.contains("flower_look") && error.contains("`whole`"),
            "{error}"
        );
        if let Some(season) = &mut look.season {
            season.flower_look = Some(StageLook {
                whole: true,
                ..spike
            });
        }
        assert!(look.validate().is_ok());
        // Opening and whole are a cluster's.
        look.inflorescence = None;
        let error = look.validate().unwrap_err();
        assert!(
            error.contains("whole") && error.contains("inflorescence"),
            "{error}"
        );
        if let Some(season) = &mut look.season {
            season.flower_look = None;
            season.opening = 10.0;
        }
        let error = look.validate().unwrap_err();
        assert!(error.contains("opening"), "{error}");
    }

    #[test]
    fn a_staged_graph_leaves_out_gone_organs_and_resizes_the_rest() {
        let organ = |organ: u16, size: f64| crate::graph::GraphOrgan {
            id: u64::from(organ),
            organ,
            segment: None,
            position: crate::math::Vec3::ZERO,
            heading: crate::math::Vec3::Y,
            left: crate::math::Vec3::X,
            size,
            born: 0.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        };
        let graph = crate::graph::PlantGraph {
            age: 5.0,
            height: 2.0,
            segments: Vec::new(),
            organs: vec![organ(0, 0.1), organ(1, 0.04), organ(2, 0.2)],
            structures: Vec::new(),
        };
        assert!(staged(&graph, &[1.0, 1.0, 1.0]).is_none());
        let drawn = staged(&graph, &[1.0, 0.0, 0.5]).unwrap();
        assert_eq!(drawn.organs.len(), 2);
        assert_eq!(drawn.organs[0].size, 0.1);
        assert_eq!(drawn.organs[1].organ, 2);
        assert_eq!(drawn.organs[1].size, 0.1);
        assert_eq!(drawn.height, graph.height);
    }

    /// The fields `crate::inherit::TEMPLATES` lets rules set are the card
    /// templates' own: every template, each field with what it takes, and
    /// every field a template writes.
    #[test]
    fn rules_know_every_template_and_field() {
        use crate::inherit::{Field, TEMPLATES};
        let parse = |value: serde_json::Value| serde_json::from_value::<Shape>(value);
        // An unknown template's error lists the templates serde knows.
        let error = parse(serde_json::json!({"template": "none"}))
            .unwrap_err()
            .to_string();
        let known: Vec<&str> = error
            .split("expected one of ")
            .nth(1)
            .unwrap_or_default()
            .split(", ")
            .map(|name| name.trim_matches('`'))
            .collect();
        let listed: Vec<&str> = TEMPLATES.iter().map(|(name, _)| *name).collect();
        assert_eq!(listed, known, "{error}");
        for (template, fields) in TEMPLATES {
            let with = |field: &str, value: serde_json::Value| {
                parse(serde_json::json!({"template": template, field: value}))
            };
            let shape = parse(serde_json::json!({"template": template})).unwrap();
            assert_eq!(shape.name(), template);
            for (field, takes) in fields {
                let at = format!("{template}.{field}");
                match takes {
                    Field::Count => {
                        assert!(with(field, 2.into()).is_ok(), "{at}");
                        assert!(with(field, 2.5.into()).is_err(), "{at} takes a fraction");
                    }
                    Field::Number => assert!(with(field, 2.5.into()).is_ok(), "{at}"),
                    Field::Word => {
                        let error = with(field, "none".into()).unwrap_err().to_string();
                        assert!(error.contains("unknown variant"), "{at}: {error}");
                    }
                    Field::Outline => {
                        let points = serde_json::json!([[0, 0.5], [1, 0]]);
                        assert!(with(field, points).is_ok(), "{at}");
                        assert!(with(field, 2.5.into()).is_err(), "{at} takes a number");
                    }
                }
            }
            // Every field the template writes is listed.
            let written = serde_json::to_value(&shape).unwrap();
            for field in written.as_object().unwrap().keys() {
                assert!(
                    field == "template" || fields.iter().any(|(name, _)| name == field),
                    "{template}.{field} is not in TEMPLATES"
                );
            }
        }
    }

    /// The keys `crate::inherit::INFLORESCENCE` lets rules set are the
    /// inflorescence's own, each with what it takes; only its `branch`,
    /// `stalk` and `spathe_colour` are left to the files.
    #[test]
    fn rules_know_every_inflorescence_key() {
        use crate::inherit::{Field, INFLORESCENCE};
        let parse = |value: serde_json::Value| serde_json::from_value::<Inflorescence>(value);
        let error = parse(serde_json::json!({"none": 1}))
            .unwrap_err()
            .to_string();
        let mut known: Vec<&str> = error
            .split("expected one of ")
            .nth(1)
            .unwrap_or_default()
            .split(", ")
            .map(|name| name.trim_matches('`'))
            .collect();
        known.retain(|key| !["branch", "stalk", "spathe_colour"].contains(key));
        let listed: Vec<&str> = INFLORESCENCE.iter().map(|(key, _)| *key).collect();
        assert_eq!(listed, known, "{error}");
        for (key, takes) in INFLORESCENCE {
            let with = |value: serde_json::Value| parse(serde_json::json!({key: value}));
            match takes {
                Field::Count => {
                    assert!(with(2.into()).is_ok(), "{key}");
                    assert!(with(2.5.into()).is_err(), "{key} takes a fraction");
                }
                Field::Number => assert!(with(0.5.into()).is_ok(), "{key}"),
                Field::Word => {
                    assert!(with("umbel".into()).is_ok(), "{key}");
                    let error = with("none".into()).unwrap_err().to_string();
                    assert!(error.contains("unknown variant"), "{key}: {error}");
                }
                Field::Outline => panic!("{key} takes no outline"),
            }
        }
    }
}
