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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lsys::OrganKind;

/// The look of one organ type, as a species spec gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganLook {
    /// The template and its shape parameters.
    pub shape: Shape,
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
}

impl Default for Needles {
    fn default() -> Self {
        Self {
            twigs: 6,
            needle: 0.11,
            twig_angle: 45.0,
            ranks: 2,
            aspect: 0.85,
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
    /// Each leaf's blade, as in [`Simple`]: width over length, where it is
    /// widest, its tip and base, and its teeth.
    pub width: f64,
    pub widest: f64,
    pub tip: f64,
    pub base: f64,
    pub teeth: u32,
    pub tooth_depth: f64,
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
            widest: 0.4,
            tip: 0.4,
            base: 0.5,
            teeth: 0,
            tooth_depth: 0.03,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fruit {
    /// Width over length.
    pub aspect: f64,
    /// 1 for the overlapping scales of a cone.
    pub cone: f64,
}

impl Default for Fruit {
    fn default() -> Self {
        Self {
            aspect: 0.7,
            cone: 0.0,
        }
    }
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
                check("width", s.width, 0.1, 1.2)?;
                check("widest", s.widest, 0.15, 0.85)?;
                check("tip", s.tip, 0.0, 1.0)?;
                check("base", s.base, 0.0, 1.0)?;
                count("teeth", s.teeth, 0, 40)?;
                check("tooth_depth", s.tooth_depth, 0.0, 0.2)?;
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
                check("cone", s.cone, 0.0, 1.0)
            }
        }
    }
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
}

impl Look {
    #[must_use]
    pub fn mount(&self) -> Mount {
        self.shape.mount()
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
            },
        })
        .collect()
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
fn within(what: &str, value: f64, low: f64, high: f64) -> Result<(), String> {
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
    /// Check the shape and colours.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range.
    pub fn validate(&self) -> Result<(), String> {
        self.shape.validate()?;
        for colour in [self.colour, self.shade, self.accent].into_iter().flatten() {
            for channel in colour {
                within("colours", f64::from(channel), 0.0, 1.0)?;
            }
        }
        within("face_up", self.face_up, 0.0, 1.0)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

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
}
