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
    /// How far a card that lies along its organ turns flat to face the
    /// sky, from 0 (as the program placed it) to 1 (level). Broad leaves
    /// held toward the light form the layered crowns of shade-tolerant
    /// trees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub face_up: f64,
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
}

/// How an organ type's leaves differ with the light they grew in and the
/// plant's age when they grew (plant roadmap P4, phenotype families).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Families {
    /// Sun and shade leaves, 0 (alike) to 1: how far a leaf grown in light
    /// past [`SUN`] of the way along [`sunlit`]'s scale is smaller,
    /// narrower and more deeply lobed or toothed, and one grown in light
    /// short of [`SHADE`] of the way larger, broader and shallower
    /// ([`Shape::in_light`]).
    pub plasticity: f64,
    /// Juvenile leaves, on a plant younger than their `until` when they
    /// grew.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub juvenile: Option<Juvenile>,
}

/// An organ type's juvenile leaves: their shape, grown while the plant is
/// younger than `until` years (a juniper's needles before its scales, a
/// pine seedling's single needles, a young madrone's toothed leaves).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Juvenile {
    pub until: f64,
    pub shape: Shape,
}

/// The ends of the scale leaves acclimate on, in the growth model's light
/// (its share of open-sky light, `light@1`). In that model a dense crown's
/// outermost leaves see only about a third of the open sky, since each
/// light voxel's own leaves shade it, so the scale tops out there: a leaf
/// in [`FULL_SUN`] or more is drawn in its look's colour, one in
/// [`DEEP_SHADE`] or less in its shade colour, and between them it moves
/// with the logarithm of its light ([`sunlit`]), as leaves acclimate.
pub const FULL_SUN: f64 = 0.35;
pub const DEEP_SHADE: f64 = 0.04;

/// Where `light` lies on the scale from [`DEEP_SHADE`] (0) to
/// [`FULL_SUN`] (1), by its logarithm.
#[must_use]
pub fn sunlit(light: f64) -> f64 {
    let floor = crate::math::ln(DEEP_SHADE);
    let span = crate::math::ln(FULL_SUN) - floor;
    ((crate::math::ln(light.max(DEEP_SHADE)) - floor) / span).clamp(0.0, 1.0)
}

/// How far along [`sunlit`]'s scale a leaf's light must lie for it to
/// grow as a sun leaf, and short of which it grows as a shade leaf;
/// between, as its type's own look. On the light itself they fall at
/// about 0.17 and 0.08.
pub const SUN: f64 = 2.0 / 3.0;
pub const SHADE: f64 = 1.0 / 3.0;

impl Families {
    /// Check the plasticity, the juvenile leaves and that `shape` has sun
    /// and shade forms if they are asked for.
    ///
    /// # Errors
    ///
    /// Describes the first problem.
    pub fn validate(&self, shape: &Shape) -> Result<(), String> {
        within("families plasticity", self.plasticity, 0.0, 1.0)?;
        if self.plasticity > 0.0 && shape.in_light(true, self.plasticity).is_none() {
            return Err(format!(
                "families plasticity needs a simple, lobed, palmate or compound leaf or a sprig, found {}",
                shape.name()
            ));
        }
        if let Some(juvenile) = &self.juvenile {
            within("families juvenile until", juvenile.until, 0.1, 200.0)?;
            juvenile.shape.validate()?;
            if matches!(
                juvenile.shape,
                Shape::Flower(_)
                    | Shape::Head(_)
                    | Shape::Umbel(_)
                    | Shape::Spike(_)
                    | Shape::Panicle(_)
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
/// P4): the indices of its sun, shade and juvenile looks among the
/// plant's looks, when a leaf is juvenile, and the sizes of sun and shade
/// leaves as shares of the type's. The default draws every leaf with the
/// type's own look.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Forms {
    pub sun: Option<usize>,
    pub shade: Option<usize>,
    pub juvenile: Option<(usize, f64)>,
    pub sizes: (f64, f64),
}

impl Forms {
    /// The look, of the plant's looks, that a leaf of the type whose own
    /// look is `own` is drawn with, grown in `light` on a plant `born`
    /// years old, and its size as a share of the type's.
    #[must_use]
    pub fn of(&self, own: usize, light: f64, born: f64) -> (usize, f64) {
        if let Some((juvenile, until)) = self.juvenile
            && born < until
        {
            return (juvenile, 1.0);
        }
        let lit = sunlit(light);
        match (self.sun, self.shade) {
            (Some(sun), _) if lit > SUN => (sun, self.sizes.0),
            (_, Some(shade)) if lit < SHADE => (shade, self.sizes.1),
            _ => (own, 1.0),
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
    /// Days a cluster takes to open from its first unit to its last; 0,
    /// all at once, by default.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub opening: f64,
    /// Its look in bud; without one, a closed bud a third the size of its
    /// flower, in the flower's colour half turned to the foliage's, whole
    /// where its flower look is.
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
    /// Its look in bud: its `bud_look`, or a closed bud of its flower's
    /// colours (`colour`, the organ's own, where it sets none), half
    /// turned to `foliage`'s (its sepals), a third its flower's size, and
    /// whole where its flower look is.
    #[must_use]
    pub fn bud(&self, colour: [f32; 3], foliage: [f32; 3]) -> StageLook {
        self.bud_look.clone().unwrap_or_else(|| {
            let (colour, size, whole) = self
                .flower_look
                .as_ref()
                .map_or((colour, 1.0, false), |flower| {
                    (flower.colour.unwrap_or(colour), flower.size, flower.whole)
                });
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
    /// unit's (see [`Self::bud`] for `colour` and `foliage`).
    #[must_use]
    pub fn units(&self, colour: [f32; 3], foliage: [f32; 3]) -> Vec<(crate::clusters::Unit, f64)> {
        [Some(self.bud(colour, foliage)), self.flower_look.clone()]
            .into_iter()
            .flatten()
            .filter(|look| !look.whole)
            .filter_map(|look| crate::clusters::Unit::of(&look.shape).map(|unit| (unit, look.size)))
            .collect()
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
        let d = if day < self.bud && day + 365.0 < self.fall {
            day + 365.0
        } else {
            day
        };
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
    /// A pinnately compound leaf (ashes, elders, rowans).
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
    /// A daisy-like head of ray and disc flowers, seen face on.
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
    /// place on it: 2, 3 or 5 for a pine; 1 for needles borne singly. Only
    /// all round a shoot.
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
    pub petiole: f64,
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
            petiole: 0.15,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Compound {
    /// Leaflets, the terminal one included.
    pub leaflets: u32,
    /// Leaflet width over leaflet length.
    pub leaflet_width: f64,
    /// Teeth on each side of a leaflet; 0 for entire.
    pub teeth: u32,
    pub petiole: f64,
}

impl Default for Compound {
    fn default() -> Self {
        Self {
            leaflets: 7,
            leaflet_width: 0.4,
            teeth: 0,
            petiole: 0.15,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flower {
    pub petals: u32,
    /// Petal width; 1 leaves a little gap between neighbours, 1.4 makes
    /// them overlap.
    pub petal_width: f64,
    /// 0 for round petal tips, 1 for pointed ones.
    pub point: f64,
    /// Depth of a notch in each petal tip, 0 to 1.
    pub notch: f64,
    /// Radius of the centre, in the accent colour, over the flower's.
    pub centre: f64,
}

impl Default for Flower {
    fn default() -> Self {
        Self {
            petals: 5,
            petal_width: 1.0,
            point: 0.0,
            notch: 0.0,
            centre: 0.18,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Head {
    /// Ray flowers; 0 for a head of disc flowers only.
    pub rays: u32,
    /// Ray width over the width of its share of the circle.
    pub ray_width: f64,
    /// Radius of the disc, in the accent colour, over the head's.
    pub disc: f64,
}

impl Default for Head {
    fn default() -> Self {
        Self {
            rays: 21,
            ray_width: 0.7,
            disc: 0.32,
        }
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
/// kind (`crate::clusters::Cluster::of`), so `{}` is a raceme of 12.
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
    /// of a compound cluster; a cyme's units in all. 1 to 200; 12 unless
    /// set, or for a branch its cluster's.
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
    /// A unit's stalk, 0 to 1: 0.15, or 0 where the axis bears branches.
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
            Self::Spike(_) | Self::Panicle(_) | Self::Fruit(_) => Some(1.0),
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
            Self::Fruit(fruit) => fruit.aspect,
        }
    }

    /// This leaf as grown in the sun (`sun`) or the shade, for a
    /// `plasticity` 0 to 1 (plant roadmap P4), and its size as a share of
    /// the leaf's own: a sun leaf is smaller, narrower and more deeply
    /// lobed or toothed; a shade leaf larger, broader and shallower (sun
    /// and shade leaves of one crown, as of oaks and maples). `None` for
    /// shapes without the two forms.
    #[must_use]
    pub fn in_light(&self, sun: bool, plasticity: f64) -> Option<(Shape, f64)> {
        let p = plasticity.clamp(0.0, 1.0);
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
        let size = if sun { 1.0 - 0.12 * p } else { 1.0 + 0.12 * p };
        let shape = match self {
            Self::Simple(s) => Self::Simple(Simple {
                width: wide(s.width, 0.03, 1.5, 0.25, 0.15),
                tooth_depth: deep(s.tooth_depth, 0.2, 0.3),
                ..s.clone()
            }),
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
                leaflet_width: wide(s.leaflet_width, 0.1, 1.0, 0.2, 0.15),
                ..s.clone()
            }),
            Self::Sprig(s) => Self::Sprig(Sprig {
                width: wide(s.width, 0.03, 1.2, 0.2, 0.15),
                tooth_depth: deep(s.tooth_depth, 0.2, 0.4),
                ..s.clone()
            }),
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
                check("needle", s.needle, 0.01, 0.4)?;
                check("twig_angle", s.twig_angle, 10.0, 80.0)?;
                if s.ranks != 0 && s.ranks != 2 {
                    return Err(format!("needles ranks must be 0 or 2, found {}", s.ranks));
                }
                check("width", s.width, 0.003, 0.3)?;
                count("bundle", s.bundle, 1, 5)?;
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
                check("petiole", s.petiole, 0.0, 0.7)
            }
            Self::Lobed(s) => {
                check("width", s.width, 0.1, 1.5)?;
                count("lobes", s.lobes, 1, 10)?;
                check("depth", s.depth, 0.0, 0.9)?;
                check("round", s.round, 0.0, 1.0)?;
                check("petiole", s.petiole, 0.0, 0.7)
            }
            Self::Compound(s) => {
                if !(3..=25).contains(&s.leaflets) || s.leaflets % 2 == 0 {
                    return Err(format!(
                        "compound leaflets must be odd, 3 to 25, found {}",
                        s.leaflets
                    ));
                }
                check("leaflet_width", s.leaflet_width, 0.1, 1.0)?;
                count("teeth", s.teeth, 0, 30)?;
                check("petiole", s.petiole, 0.0, 0.6)
            }
            Self::Sprig(s) => {
                count("leaves", s.leaves, 2, 15)?;
                check("angle", s.angle, 20.0, 80.0)?;
                check("stalk", s.stalk, 0.0, 0.4)?;
                check("width", s.width, 0.03, 1.2)?;
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
                count("petals", s.petals, 3, 12)?;
                check("petal_width", s.petal_width, 0.1, 1.5)?;
                check("point", s.point, 0.0, 1.0)?;
                check("notch", s.notch, 0.0, 0.8)?;
                check("centre", s.centre, 0.0, 0.8)
            }
            Self::Head(s) => {
                count("rays", s.rays, 0, 60)?;
                check("ray_width", s.ray_width, 0.1, 1.2)?;
                check("disc", s.disc, 0.1, 1.0)
            }
            Self::Umbel(s) => {
                count("clusters", s.clusters, 1, 30)?;
                count("florets", s.florets, 1, 60)
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
    /// Length of one lateral leaflet.
    #[must_use]
    pub fn leaflet_length(&self) -> f64 {
        let pairs = f64::from(self.leaflets / 2);
        ((1.0 - self.petiole) * 2.4 / (pairs + 1.5)).min(0.45)
    }

    fn aspect(&self) -> f64 {
        let length = self.leaflet_length();
        2.0 * (length * crate::math::sin(crate::math::radians(62.0))
            + length * self.leaflet_width * 0.5)
            + 0.03
    }
}

impl Sprig {
    /// Length of the longest leaf, petiole included, as a fraction of the
    /// card's length.
    #[must_use]
    pub fn leaf_length(&self) -> f64 {
        let side = f64::from(self.leaves.saturating_sub(1));
        ((1.0 - self.stalk) * 2.4 / (side / 2.0 + 1.5)).min(0.45)
    }

    /// Card width over card length: room for the side leaves at their
    /// angle, with their blades' width.
    #[must_use]
    pub fn aspect(&self) -> f64 {
        let length = self.leaf_length();
        let angle = crate::math::radians(self.angle);
        let blade = length * (1.0 - self.petiole) * self.width * 0.5;
        (2.0 * (length * crate::math::sin(angle) + blade * crate::math::cos(angle)) + 0.03).min(1.6)
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
    pub face_up: f64,
    pub solid: Option<crate::leaves::SolidLeaf>,
    pub bend: crate::bend::Bend,
    pub form: crate::blooms::Form,
    /// Its leaves' families' looks; the default for none.
    pub forms: Forms,
    /// The cluster the organ is, each of its units drawn with `shape`
    /// (spec rework L1); `None` for an organ that is one unit.
    pub inflorescence: Option<crate::clusters::ClusterLook>,
}

impl Look {
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
                let shade = look.shade.unwrap_or(if look.colour.is_some() {
                    [colour[0] * 0.35, colour[1] * 0.42, colour[2] * 0.55]
                } else {
                    foliage_shade
                });
                Look {
                    organ: name.to_string(),
                    shape: look.shape.clone(),
                    colour,
                    shade,
                    accent: look.accent.unwrap_or(colour),
                    face_up: look.face_up,
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
                            &look.shape,
                            &look
                                .season
                                .as_ref()
                                .map(|season| season.units(colour, foliage))
                                .unwrap_or_default(),
                        )
                    }),
                }
            }
            None => Look {
                organ: name.to_string(),
                bend: crate::bend::Bend::default_for(&Shape::default_for(kind)),
                form: crate::blooms::Form::default_for(&Shape::default_for(kind)),
                shape: Shape::default_for(kind),
                colour: foliage,
                shade: foliage_shade,
                accent: foliage,
                face_up: 0.0,
                solid: None,
                forms: Forms::default(),
                inflorescence: None,
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
            if families.plasticity > 0.0
                && let (Some((sun, sun_size)), Some((shade, shade_size))) = (
                    own.shape.in_light(true, families.plasticity),
                    own.shape.in_light(false, families.plasticity),
                )
            {
                forms.sun = Some(self.len());
                self.push(Look {
                    bend: own.bend,
                    ..variant(sun)
                });
                forms.shade = Some(self.len());
                self.push(Look {
                    bend: own.bend,
                    ..variant(shade)
                });
                forms.sizes = (sun_size, shade_size);
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
                (Some(season), Some(day)) => in_season(look, season, day, foliage),
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

/// `look` as it is on `day` of `season`, and its size as a share.
fn in_season(look: Look, season: &Season, day: f64, foliage: [f32; 3]) -> (Look, f64) {
    let stage = season.stage(day);
    let whole = match stage {
        Stage::Bud => season.bud(look.colour, foliage).whole,
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
    match stage {
        Stage::Gone => (look, 0.0),
        Stage::Bud => {
            let bud = season.bud(look.colour, foliage);
            (in_stage(&look, &bud), bud.size)
        }
        Stage::Flower => match &season.flower_look {
            Some(flower) => (in_stage(&look, flower), flower.size),
            None if is_fruit => (look, 0.0),
            None => (look, 1.0),
        },
        Stage::Fruit { unripe } => {
            if matches!(look.shape, Shape::Flower(_)) {
                return (look, 0.0);
            }
            let mut look = look;
            if let Shape::Fruit(fruit) = &mut look.shape {
                fruit.unripe = unripe;
            }
            (look, 1.0)
        }
    }
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
            Stage::Flower if !own_fruit && season.flower_look.is_none() => return (look, 1.0),
            _ => {}
        }
    }
    let unit = |stage: &StageLook| {
        let colour = stage.colour.unwrap_or(look.colour);
        (!stage.whole).then(|| crate::clusters::StageUnit {
            form: stage
                .form
                .clone()
                .unwrap_or_else(|| crate::blooms::Form::default_for(&stage.shape)),
            shape: stage.shape.clone(),
            colour,
            accent: stage.accent.unwrap_or(colour),
            size: stage.size,
        })
    };
    let own = crate::clusters::StageUnit {
        shape: look.shape.clone(),
        form: look.form.clone(),
        colour: look.colour,
        accent: look.accent,
        size: 1.0,
    };
    let flower = match &season.flower_look {
        Some(flower) => unit(flower),
        None => (!own_fruit).then(|| own.clone()),
    };
    let opening = crate::clusters::Opening {
        season: Season {
            bud_look: None,
            flower_look: None,
            ..season.clone()
        },
        day,
        looks: [
            unit(&season.bud(look.colour, foliage)),
            flower,
            own_fruit.then_some(own),
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
        organ: look.organ.clone(),
        shape: stage.shape.clone(),
        colour,
        shade: [colour[0] * 0.35, colour[1] * 0.42, colour[2] * 0.55],
        accent: stage.accent.unwrap_or(colour),
        face_up: look.face_up,
        solid: None,
        bend: crate::bend::Bend::default_for(&stage.shape),
        form: crate::blooms::Form::default_for(&stage.shape),
        forms: Forms::default(),
        // A stage look draws the whole organ.
        inflorescence: None,
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
    /// Check that `season`'s cluster keys suit the look: `opening` and
    /// `whole` are for a cluster, and a cluster's stage looks that are not
    /// `whole` draw one unit each.
    fn check_stages(&self, season: &Season) -> Result<(), String> {
        let looks = [
            ("bud_look", &season.bud_look),
            ("flower_look", &season.flower_look),
        ];
        if self.inflorescence.is_none() {
            if season.opening > 0.0 {
                return Err("season opening is for a look with an inflorescence".to_string());
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
                     head or a fruit, found {}; set `whole` to draw the whole organ",
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
            if !matches!(
                self.shape,
                Shape::Flower(_) | Shape::Head(_) | Shape::Fruit(_)
            ) {
                return Err(format!(
                    "an inflorescence's units are flowers, heads or fruit, found {}",
                    self.shape.name()
                ));
            }
            if self.solid.is_some() || self.families.is_some() {
                return Err(
                    "solid leaves and families are for leaves, not an inflorescence".to_string(),
                );
            }
        }
        for colour in [self.colour, self.shade, self.accent].into_iter().flatten() {
            for channel in colour {
                within("colours", f64::from(channel), 0.0, 1.0)?;
            }
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
        within("face_up", self.face_up, 0.0, 1.0)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

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
            let (sun, sun_size) = shape.in_light(true, 0.8).unwrap();
            let (shade, shade_size) = shape.in_light(false, 0.8).unwrap();
            sun.validate().unwrap();
            shade.validate().unwrap();
            let (own, sun, shade) = (measure(&shape), measure(&sun), measure(&shade));
            assert!(sun.0 < own.0 && own.0 < shade.0, "{shape:?}");
            assert!(sun.1 >= own.1 && own.1 >= shade.1, "{shape:?}");
            assert!(sun_size < 1.0 && shade_size > 1.0);
            assert_eq!(shape.in_light(true, 0.0).unwrap().1, 1.0);
        }
        assert!(
            Shape::Needles(Needles::default())
                .in_light(true, 1.0)
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
        // A dense crown's surface (light about 0.2 to 0.3 in the growth
        // model) is mostly sunlit.
        assert!(sunlit(0.2) > SUN && sunlit(0.08) < SHADE);
    }

    #[test]
    fn a_leafs_family_picks_its_look() {
        let organs = BTreeMap::from([(
            "leaf".to_string(),
            OrganLook {
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
                        until: 3.0,
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
        // The two types' looks, then the leaf's sun, shade and juvenile.
        assert_eq!(looks.len(), 5);
        let forms = looks[0].forms;
        assert_eq!(
            (forms.sun, forms.shade, forms.juvenile),
            (Some(2), Some(3), Some((4, 3.0)))
        );
        assert_eq!(looks[1].forms, Forms::default());
        assert!(
            looks[2..]
                .iter()
                .all(|look| look.organ == "leaf" && look.forms == Forms::default())
        );
        assert!(matches!(looks[4].shape, Shape::Simple(_)));
        // Juvenile on a young plant whatever its light; then by its light.
        assert_eq!(forms.of(0, 0.9, 1.0), (4, 1.0));
        assert_eq!(forms.of(0, 0.9, 10.0), (2, forms.sizes.0));
        assert_eq!(forms.of(0, 0.2, 10.0), (2, forms.sizes.0));
        assert_eq!(forms.of(0, 0.12, 10.0), (0, 1.0));
        assert_eq!(forms.of(0, 0.06, 10.0), (3, forms.sizes.1));
        // Families are for leaves.
        let mut flower = organs["leaf"].clone();
        flower.shape = Shape::Flower(Flower::default());
        assert!(flower.validate().is_err());
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
        // A closed bud a third of its flower's size.
        let (looks, sizes) = on(&organs, 100.0);
        assert!(matches!(looks[1].shape, Shape::Fruit(_)));
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

    /// A cherry's umbel of six (or `kind` of `flowers`), with `season`.
    fn clustered(kind: ClusterKind, flowers: u32, season: Season) -> OrganLook {
        OrganLook {
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
                            .bud([0.6, 0.05, 0.06], [0.1, 0.3, 0.05])
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
        // So is its closed bud, unless a bud look of its own says not.
        let (look, size) = on(clustered(ClusterKind::Umbel, 6, whole), 100.0);
        assert!(look.inflorescence.is_none());
        assert!(matches!(look.shape, Shape::Fruit(_)));
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
        };
        let graph = crate::graph::PlantGraph {
            age: 5.0,
            height: 2.0,
            segments: Vec::new(),
            organs: vec![organ(0, 0.1), organ(1, 0.04), organ(2, 0.2)],
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
    /// inflorescence's own, each with what it takes; only its `branch` and
    /// `stalk` are left to the files.
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
        known.retain(|key| !["branch", "stalk"].contains(key));
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
