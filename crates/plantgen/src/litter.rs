//! Litter: what each canopy tree sheds on the ground under it, drawn as a
//! ground look of its own.
//!
//! The ground cover's litter words (`after_ecology::cover::Ground`'s
//! `Needles` and `Leaves`) are generic. Where the canopy's species are
//! known, the ground lays each species' own litter instead, by its share
//! of the canopy overhead. [`LITTERS`] says what falls from each canopy
//! species of the built-in catalogue: needles of its length and width,
//! single or in the pines' bundles; flat sprays of scale leaves; or leaves
//! of its own shape, the look its crown wears (`spec::Appearance::organs`,
//! a sprig's leaf taken singly) unless a leaf seen whole needs a truer one;
//! the colours it falls in and the colours it dries to; and what falls with
//! it: cones, cone scales, fruits, winged seeds, catkins, curls of bark and
//! twigs.
//!
//! Litter falls year after year, so a look lays its oldest pieces first
//! and lowest: smaller, darker, broken and eaten through as they rot, the
//! sooner the more the species' litter decays ([`Litter::decay`]). The
//! newest share fell this season and is still drying ([`DRYING_SHARE`]):
//! a drying leaf browns from its margin and its tip inward, in blotches,
//! from the colour it fell in to the colour it dries to, and cups across
//! its midrib and curls along it as far as its species' leaves do
//! ([`Litter::curl`]). About half the leaves lie face down, arched and
//! showing their paler undersides; a leaf face up that cups past upright
//! shows its underside over its trough. Older leaves lie flattened under
//! the newer ones. Heights are metres, so a look's relief is what its
//! pieces stand to.
//!
//! Drawing follows the ground looks' rules (`crate::ground`): every random
//! number is a hash of the seed and what it is for, transcendental
//! functions go through `libm`, every piece wraps around the texture's
//! edges, and the colours are scaled so their mean is the litter's
//! [`Litter::mean`]. Shading is never directional, since the ground's
//! light reads the relief: only hollows are darker and crests lighter.

use crate::ground::{
    Canvas, Draws, GROUND_LOOK_SIZE, GroundLook, fbm, mix, organ_templates, scale, spot, to_f64,
    value_noise,
};
use crate::looks::{Fruit, Lobed, Scales, Shape, Simple};
use crate::math::{self, PI};
use crate::rng::{hash_str, hash_words};
use crate::spec::PlantSpec;
use crate::templates::{Templates, Texel};

/// What falls from a tree as its foliage.
#[derive(Debug, Clone, PartialEq)]
pub enum Fall {
    /// Single needles `length_m` long and `width_m` wide, or bundles of
    /// `bundle` joined at a sheath (the pines' fascicles).
    Needles {
        length_m: [f64; 2],
        width_m: f64,
        bundle: u32,
    },
    /// Flat sprays of scale leaves `length_m` long, of the species' own
    /// spray look.
    Sprays { length_m: [f64; 2] },
    /// Leaves `length_m` long from the petiole's base to the tip, of the
    /// shape the species' crown wears, or of `shape` where a leaf seen
    /// whole needs a truer one than the crown's cards.
    Leaves {
        length_m: [f64; 2],
        shape: Option<Shape>,
    },
}

/// What else falls with the foliage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraKind {
    /// Woody cones.
    Cone,
    /// The scales of cones that break up on the tree (true firs).
    ConeScale,
    /// Round fruits, nuts and berries: acorns, crabapples, juniper
    /// berries.
    Fruit,
    /// Winged seeds joined in pairs (maples).
    PairedSamara,
    /// Single winged seeds (ashes).
    Samara,
    /// Spent catkins (alders, cottonwoods).
    Catkin,
    /// Thin curls and strips of shed bark (madrone, paper birch,
    /// ponderosa pine's flakes).
    Bark,
    /// Fallen twigs.
    Twig,
}

/// Something that falls with the foliage: `per_m2` pieces a square metre,
/// each `length_m` long and `aspect` as wide as long, in `colour`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extra {
    pub kind: ExtraKind,
    pub length_m: [f64; 2],
    pub aspect: f64,
    pub colour: [f64; 3],
    pub per_m2: f64,
}

const fn extra(
    kind: ExtraKind,
    length_m: [f64; 2],
    aspect: f64,
    colour: [f64; 3],
    per_m2: f64,
) -> Extra {
    Extra {
        kind,
        length_m,
        aspect,
        colour,
        per_m2,
    }
}

/// What one canopy species sheds, and how its litter looks.
#[derive(Debug, Clone, PartialEq)]
pub struct Litter {
    /// The species, as in `spec::SPECIES`.
    pub species: &'static str,
    /// Metres of ground its look covers before it repeats.
    pub tile_m: f32,
    /// Mean albedo of its look, linear RGB reflectance: the drawing's own
    /// mean hue, at the brightness of the species' litter.
    pub mean: [f32; 3],
    /// Height of the highest texel over the lowest as drawn, metres.
    pub relief_m: f32,
    pub fall: Fall,
    /// The colours its foliage falls in, linear RGB, as they vary between
    /// pieces: a broadleaf's autumn colours, a conifer's dead needles.
    /// The look is scaled to [`Self::mean`].
    pub shed: &'static [[f64; 3]],
    /// The colours its foliage dries to on the ground.
    pub dry: &'static [[f64; 3]],
    /// How far its leaves cup and curl as they dry: 0 they stay flat, 1
    /// their edges roll past upright.
    pub curl: f64,
    /// How much paler and greyer a leaf's underside is than its top, 0 to
    /// 1.
    pub pale_beneath: f64,
    /// How fast its litter rots, 0 to 1: how soon old pieces darken, break
    /// up and are eaten through.
    pub decay: f64,
    pub extras: &'static [Extra],
    /// Where its values come from.
    pub evidence: &'static str,
}

use ExtraKind::{Bark, Catkin, Cone, ConeScale, PairedSamara, Samara, Twig};

/// Twigs under every tree: `per_m2` of them, `length_m` long.
const fn twigs(length_m: [f64; 2], per_m2: f64) -> Extra {
    extra(Twig, length_m, 0.04, [0.20, 0.15, 0.10], per_m2)
}

/// Needles as they fall: rusty orange to brown.
const RUSTY: [[f64; 3]; 4] = [
    [0.46, 0.22, 0.08],
    [0.42, 0.21, 0.09],
    [0.50, 0.28, 0.12],
    [0.38, 0.22, 0.11],
];

/// Rusty needles as they dry and weather: duller browns.
const RUSTY_DRY: [[f64; 3]; 4] = [
    [0.32, 0.19, 0.10],
    [0.28, 0.17, 0.10],
    [0.36, 0.23, 0.13],
    [0.26, 0.18, 0.12],
];

/// How many canopy species have a litter of their own: [`LITTERS`]'s
/// length.
pub const LITTER_LOOKS: usize = 23;

/// Every canopy species' litter, in the order the ground numbers their
/// looks after its words.
pub static LITTERS: [Litter; LITTER_LOOKS] = [
    Litter {
        species: "abies-grandis",
        tile_m: 1.0,
        mean: [0.113, 0.07, 0.04],
        relief_m: 0.016,
        fall: Fall::Needles {
            length_m: [0.02, 0.05],
            width_m: 0.002,
            bundle: 1,
        },
        shed: &RUSTY,
        dry: &RUSTY_DRY,
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.3,
        extras: &[
            extra(ConeScale, [0.02, 0.03], 0.9, [0.30, 0.20, 0.12], 12.0),
            twigs([0.06, 0.25], 3.0),
        ],
        evidence: "Authored from botanical descriptions of grand fir: flat needles 2 to 5 cm long \
            and about 2 mm wide, notched at the tip, rusty brown when shed and duller as they \
            weather; the cones break up on the tree, so the floor holds their fan-shaped scales, \
            about 2 to 3 cm across, not whole cones. Conifer needles rot slowly. Colours chosen \
            by eye from photographs of fir litter; numbers of scales and twigs plausible, not \
            measured.",
    },
    Litter {
        species: "juniperus-scopulorum",
        tile_m: 0.75,
        mean: [0.144, 0.1, 0.061],
        relief_m: 0.008,
        fall: Fall::Sprays {
            length_m: [0.02, 0.05],
        },
        shed: &[[0.40, 0.25, 0.13], [0.36, 0.22, 0.12], [0.44, 0.29, 0.16]],
        dry: &[[0.30, 0.21, 0.13], [0.26, 0.19, 0.12], [0.34, 0.24, 0.15]],
        curl: 0.2,
        pale_beneath: 0.0,
        decay: 0.25,
        extras: &[
            extra(
                ExtraKind::Fruit,
                [0.005, 0.007],
                0.95,
                [0.22, 0.26, 0.40],
                8.0,
            ),
            twigs([0.04, 0.15], 6.0),
        ],
        evidence: "Authored from botanical descriptions of Rocky Mountain juniper: scale leaves on \
            slender branchlets, shed as small brown sprays a few centimetres long that grey as \
            they weather; berry-like cones 4 to 7 mm across, blue under a waxy bloom. Colours \
            chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "picea-sitchensis",
        tile_m: 1.0,
        mean: [0.103, 0.075, 0.046],
        relief_m: 0.037,
        fall: Fall::Needles {
            length_m: [0.015, 0.025],
            width_m: 0.0012,
            bundle: 1,
        },
        shed: &[[0.42, 0.29, 0.15], [0.38, 0.26, 0.14], [0.46, 0.33, 0.18]],
        dry: &[[0.30, 0.22, 0.14], [0.27, 0.20, 0.13], [0.33, 0.25, 0.16]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.3,
        extras: &[
            extra(Cone, [0.05, 0.09], 0.45, [0.42, 0.30, 0.17], 2.0),
            twigs([0.06, 0.2], 3.0),
        ],
        evidence: "Authored from botanical descriptions of Sitka spruce: stiff, sharp needles 1.5 \
            to 2.5 cm long and about 1 mm across, brown when shed; cones 5 to 9 cm long with thin, \
            papery, wavy scales, light brown. Conifer needles rot slowly. Colours chosen by eye; \
            cone numbers plausible, not measured.",
    },
    Litter {
        species: "pinus-contorta",
        tile_m: 1.5,
        mean: [0.135, 0.095, 0.054],
        relief_m: 0.043,
        fall: Fall::Needles {
            length_m: [0.03, 0.07],
            width_m: 0.0015,
            bundle: 2,
        },
        shed: &[[0.50, 0.32, 0.14], [0.46, 0.29, 0.13], [0.54, 0.37, 0.17]],
        dry: &[[0.36, 0.26, 0.15], [0.32, 0.23, 0.14], [0.40, 0.29, 0.17]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.25,
        extras: &[
            extra(Cone, [0.03, 0.06], 0.65, [0.40, 0.29, 0.17], 1.0),
            twigs([0.06, 0.2], 3.0),
        ],
        evidence: "Authored from botanical descriptions of lodgepole and shore pine: needles in \
            bundles of two, 3 to 7 cm long, twisted, about 1.5 mm wide, orange-brown when shed; \
            cones 3 to 6 cm, egg-shaped and often lopsided. Pine needles rot slowly. Colours \
            chosen by eye; cone numbers plausible, not measured.",
    },
    Litter {
        species: "pinus-monticola",
        tile_m: 2.0,
        mean: [0.131, 0.095, 0.058],
        relief_m: 0.079,
        fall: Fall::Needles {
            length_m: [0.05, 0.10],
            width_m: 0.0008,
            bundle: 5,
        },
        shed: &[[0.46, 0.32, 0.17], [0.42, 0.29, 0.16], [0.50, 0.36, 0.20]],
        dry: &[[0.34, 0.26, 0.16], [0.30, 0.23, 0.15], [0.38, 0.29, 0.18]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.25,
        extras: &[
            extra(Cone, [0.10, 0.25], 0.3, [0.42, 0.31, 0.19], 0.3),
            twigs([0.08, 0.25], 2.0),
        ],
        evidence: "Authored from botanical descriptions of western white pine: slender needles in \
            bundles of five, 5 to 10 cm long; long, narrow, curved cones 10 to 25 cm long. Pine \
            needles rot slowly. Colours chosen by eye; cone numbers plausible, not measured.",
    },
    Litter {
        species: "pinus-ponderosa",
        tile_m: 3.0,
        mean: [0.176, 0.11, 0.056],
        relief_m: 0.11,
        fall: Fall::Needles {
            length_m: [0.12, 0.25],
            width_m: 0.0016,
            bundle: 3,
        },
        shed: &[[0.60, 0.31, 0.12], [0.56, 0.29, 0.11], [0.64, 0.37, 0.15]],
        dry: &[[0.44, 0.27, 0.13], [0.40, 0.25, 0.13], [0.48, 0.31, 0.16]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.2,
        extras: &[
            extra(Cone, [0.07, 0.14], 0.7, [0.40, 0.27, 0.15], 0.5),
            extra(Bark, [0.02, 0.06], 0.6, [0.55, 0.32, 0.18], 3.0),
            twigs([0.08, 0.3], 2.0),
        ],
        evidence: "Authored from botanical descriptions of ponderosa pine: needles in bundles of \
            three, 12 to 25 cm long, orange-brown when dead and lying in a deep, loose mat that \
            rots slowly; cones 7 to 14 cm; jigsaw-puzzle flakes of orange bark shed from the \
            trunk. Colours chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "pseudotsuga-menziesii",
        tile_m: 1.0,
        mean: [0.105, 0.065, 0.037],
        relief_m: 0.052,
        fall: Fall::Needles {
            length_m: [0.02, 0.035],
            width_m: 0.0015,
            bundle: 1,
        },
        shed: &RUSTY,
        dry: &RUSTY_DRY,
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.3,
        extras: &[
            extra(Cone, [0.05, 0.10], 0.45, [0.38, 0.24, 0.13], 2.0),
            twigs([0.08, 0.3], 3.0),
        ],
        evidence: "Authored from botanical descriptions of Douglas-fir: flat, blunt needles 2 to \
            3.5 cm long and about 1.5 mm wide, rusty brown when shed and duller as they weather; \
            cones 5 to 10 cm with three-pointed bracts. Conifer needles rot slowly. Colours chosen \
            by eye from photographs of duff; cone numbers plausible, not measured.",
    },
    Litter {
        species: "taxus-brevifolia",
        tile_m: 0.75,
        mean: [0.085, 0.055, 0.032],
        relief_m: 0.014,
        fall: Fall::Needles {
            length_m: [0.01, 0.025],
            width_m: 0.002,
            bundle: 1,
        },
        shed: &[[0.38, 0.21, 0.09], [0.34, 0.19, 0.09], [0.42, 0.25, 0.11]],
        dry: &[[0.26, 0.17, 0.10], [0.23, 0.15, 0.10], [0.29, 0.19, 0.11]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.35,
        extras: &[twigs([0.04, 0.15], 6.0)],
        evidence: "Authored from botanical descriptions of Pacific yew: flat, sharp-pointed needles \
            1 to 2.5 cm long, about 2 mm wide, yellow-brown when shed; no cones (the seeds sit in \
            red arils that animals take). Colours chosen by eye; twig numbers plausible, not \
            measured.",
    },
    Litter {
        species: "thuja-plicata",
        tile_m: 1.0,
        mean: [0.13, 0.07, 0.035],
        relief_m: 0.021,
        fall: Fall::Sprays {
            length_m: [0.05, 0.15],
        },
        shed: &[[0.58, 0.24, 0.08], [0.52, 0.22, 0.08], [0.62, 0.30, 0.11]],
        dry: &[[0.38, 0.20, 0.10], [0.33, 0.18, 0.10], [0.42, 0.24, 0.12]],
        curl: 0.25,
        pale_beneath: 0.0,
        decay: 0.2,
        extras: &[
            extra(Cone, [0.01, 0.014], 0.55, [0.40, 0.25, 0.14], 10.0),
            twigs([0.05, 0.2], 3.0),
        ],
        evidence: "Authored from botanical descriptions of western redcedar: whole flat sprays of \
            scale leaves, 5 to 15 cm long, shed each autumn rusty orange and drying red-brown; \
            small cones 1 to 1.4 cm long. Cedar litter rots slowly. Colours chosen by eye from \
            photographs of cedar litter; numbers plausible, not measured.",
    },
    Litter {
        species: "tsuga-heterophylla",
        tile_m: 0.75,
        mean: [0.087, 0.058, 0.032],
        relief_m: 0.021,
        fall: Fall::Needles {
            length_m: [0.005, 0.02],
            width_m: 0.0015,
            bundle: 1,
        },
        shed: &[[0.42, 0.26, 0.11], [0.38, 0.23, 0.10], [0.46, 0.30, 0.13]],
        dry: &[[0.28, 0.19, 0.11], [0.25, 0.17, 0.10], [0.31, 0.21, 0.12]],
        curl: 0.0,
        pale_beneath: 0.0,
        decay: 0.35,
        extras: &[
            extra(Cone, [0.015, 0.025], 0.6, [0.36, 0.25, 0.15], 8.0),
            twigs([0.04, 0.15], 5.0),
        ],
        evidence: "Authored from botanical descriptions of western hemlock: flat needles of \
            unequal lengths, 0.5 to 2 cm, with rounded tips, yellow-brown when shed; small cones \
            1.5 to 2.5 cm, borne in great numbers. Colours chosen by eye; numbers plausible, not \
            measured.",
    },
    Litter {
        species: "acer-macrophyllum",
        tile_m: 3.0,
        mean: [0.161, 0.11, 0.055],
        relief_m: 0.19,
        fall: Fall::Leaves {
            length_m: [0.25, 0.5],
            shape: None,
        },
        shed: &[
            [0.62, 0.45, 0.06],
            [0.58, 0.38, 0.05],
            [0.62, 0.33, 0.06],
            [0.50, 0.44, 0.08],
        ],
        dry: &[
            [0.42, 0.27, 0.12],
            [0.36, 0.23, 0.11],
            [0.46, 0.32, 0.16],
            [0.30, 0.20, 0.11],
        ],
        curl: 0.85,
        pale_beneath: 0.25,
        decay: 0.55,
        extras: &[
            extra(PairedSamara, [0.03, 0.05], 0.3, [0.46, 0.36, 0.20], 3.0),
            twigs([0.1, 0.4], 2.0),
        ],
        evidence: "Authored from botanical descriptions of bigleaf maple: five-lobed blades 15 to \
            30 cm across on petioles about as long, so 25 to 50 cm from the petiole's base to the \
            tip, the largest leaves of any maple, yellow to gold in autumn and drying tan to \
            brown, the large thin blades curling strongly as they dry, paler beneath; paired \
            winged seeds, each wing 3 to 5 cm. The shape is the crown's own leaf. Colours and \
            curl chosen by eye from photographs; numbers plausible, not measured.",
    },
    Litter {
        species: "alnus-rubra",
        tile_m: 1.5,
        mean: [0.072, 0.058, 0.03],
        relief_m: 0.028,
        fall: Fall::Leaves {
            length_m: [0.09, 0.17],
            shape: None,
        },
        shed: &[[0.20, 0.24, 0.07], [0.26, 0.27, 0.09], [0.30, 0.26, 0.10]],
        dry: &[[0.15, 0.11, 0.06], [0.12, 0.09, 0.05], [0.19, 0.14, 0.08]],
        curl: 0.3,
        pale_beneath: 0.2,
        decay: 0.9,
        extras: &[
            extra(Cone, [0.015, 0.025], 0.6, [0.26, 0.17, 0.10], 4.0),
            extra(Catkin, [0.05, 0.10], 0.1, [0.30, 0.18, 0.12], 2.0),
            twigs([0.06, 0.25], 3.0),
        ],
        evidence: "Authored from botanical descriptions of red alder: doubly toothed blades 7 to \
            15 cm on petioles of 1.5 to 2 cm, which fall still green and soon turn dark brown to \
            black, lying rather flat; their nitrogen-rich litter rots within a year, the fastest \
            here; small woody cones 1.5 to 2.5 cm and spent male catkins 5 to 10 cm long. Colours \
            chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "arbutus-menziesii",
        tile_m: 1.5,
        mean: [0.129, 0.085, 0.047],
        relief_m: 0.05,
        fall: Fall::Leaves {
            length_m: [0.09, 0.17],
            shape: None,
        },
        shed: &[[0.62, 0.20, 0.06], [0.56, 0.30, 0.06], [0.54, 0.42, 0.10]],
        dry: &[[0.40, 0.26, 0.13], [0.34, 0.22, 0.12], [0.46, 0.31, 0.16]],
        curl: 0.95,
        pale_beneath: 0.45,
        decay: 0.35,
        extras: &[
            extra(Bark, [0.03, 0.10], 0.4, [0.50, 0.20, 0.10], 5.0),
            extra(
                ExtraKind::Fruit,
                [0.008, 0.012],
                0.95,
                [0.62, 0.18, 0.06],
                2.0,
            ),
            twigs([0.05, 0.2], 2.0),
        ],
        evidence: "Authored from botanical descriptions of Pacific madrone: leathery, untoothed \
            blades 7 to 15 cm on petioles of 1 to 2.5 cm, pale beneath, shed in early summer \
            turning red-orange to yellow and drying brown into crisp curls; leathery leaves rot \
            slowly; thin curls of red-brown bark peeling from the trunk; orange-red berries about \
            1 cm. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "betula-papyrifera",
        tile_m: 1.0,
        mean: [0.162, 0.12, 0.056],
        relief_m: 0.028,
        fall: Fall::Leaves {
            length_m: [0.065, 0.12],
            shape: None,
        },
        shed: &[[0.64, 0.50, 0.10], [0.58, 0.44, 0.08], [0.60, 0.52, 0.14]],
        dry: &[[0.44, 0.31, 0.14], [0.38, 0.27, 0.13], [0.48, 0.35, 0.17]],
        curl: 0.5,
        pale_beneath: 0.25,
        decay: 0.6,
        extras: &[
            extra(Bark, [0.03, 0.10], 0.5, [0.80, 0.78, 0.72], 1.0),
            twigs([0.05, 0.2], 3.0),
        ],
        evidence: "Authored from botanical descriptions of paper birch: doubly toothed blades 5 to \
            10 cm on petioles of 1.5 to 2.5 cm, yellow in autumn and drying tan; strips of white, \
            papery bark. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "cornus-nuttallii",
        tile_m: 1.0,
        mean: [0.137, 0.075, 0.053],
        relief_m: 0.036,
        fall: Fall::Leaves {
            length_m: [0.08, 0.13],
            shape: None,
        },
        shed: &[
            [0.50, 0.10, 0.09],
            [0.56, 0.18, 0.11],
            [0.44, 0.12, 0.16],
            [0.58, 0.30, 0.10],
        ],
        dry: &[[0.34, 0.18, 0.13], [0.30, 0.17, 0.12], [0.38, 0.22, 0.14]],
        curl: 0.6,
        pale_beneath: 0.35,
        decay: 0.7,
        extras: &[
            extra(
                ExtraKind::Fruit,
                [0.008, 0.012],
                0.8,
                [0.60, 0.10, 0.06],
                1.0,
            ),
            twigs([0.05, 0.2], 2.0),
        ],
        evidence: "Authored from botanical descriptions of Pacific dogwood: untoothed oval blades \
            7 to 12 cm with veins curving toward the tip, on petioles of about 1 cm, red to purple \
            in autumn, paler beneath, cupping as they dry; dogwood litter rots quickly; bright \
            red fruits about 1 cm in heads. Colours and curl chosen by eye; numbers plausible, \
            not measured.",
    },
    Litter {
        species: "frangula-purshiana",
        tile_m: 1.5,
        mean: [0.127, 0.095, 0.043],
        relief_m: 0.031,
        fall: Fall::Leaves {
            length_m: [0.07, 0.16],
            shape: None,
        },
        shed: &[[0.60, 0.50, 0.10], [0.54, 0.44, 0.10], [0.56, 0.40, 0.08]],
        dry: &[[0.40, 0.29, 0.13], [0.34, 0.25, 0.12], [0.44, 0.32, 0.15]],
        curl: 0.45,
        pale_beneath: 0.25,
        decay: 0.6,
        extras: &[twigs([0.05, 0.2], 3.0)],
        evidence: "Authored from botanical descriptions of cascara: elliptic blades 5 to 15 cm \
            with prominent, parallel side veins, on petioles of 1 to 2 cm, yellow in autumn and \
            drying brown. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "fraxinus-latifolia",
        tile_m: 2.0,
        mean: [0.144, 0.115, 0.062],
        relief_m: 0.087,
        fall: Fall::Leaves {
            length_m: [0.15, 0.30],
            shape: None,
        },
        shed: &[[0.58, 0.52, 0.14], [0.52, 0.48, 0.15], [0.56, 0.46, 0.10]],
        dry: &[[0.42, 0.33, 0.17], [0.36, 0.28, 0.15], [0.46, 0.36, 0.19]],
        curl: 0.45,
        pale_beneath: 0.3,
        decay: 0.65,
        extras: &[
            extra(Samara, [0.03, 0.05], 0.22, [0.48, 0.40, 0.24], 5.0),
            twigs([0.08, 0.3], 2.0),
        ],
        evidence: "Authored from botanical descriptions of Oregon ash: compound leaves 15 to 30 cm \
            long of five to seven leaflets, yellow in autumn and falling whole, drying tan; ash \
            litter rots quickly; single winged seeds 3 to 5 cm long. The shape is the crown's own \
            leaf. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "malus-fusca",
        tile_m: 1.0,
        mean: [0.13, 0.085, 0.042],
        relief_m: 0.025,
        fall: Fall::Leaves {
            length_m: [0.05, 0.12],
            shape: None,
        },
        shed: &[[0.60, 0.34, 0.08], [0.56, 0.44, 0.09], [0.52, 0.22, 0.08]],
        dry: &[[0.40, 0.25, 0.12], [0.34, 0.22, 0.11], [0.44, 0.29, 0.14]],
        curl: 0.5,
        pale_beneath: 0.3,
        decay: 0.6,
        extras: &[
            extra(
                ExtraKind::Fruit,
                [0.01, 0.015],
                0.75,
                [0.62, 0.36, 0.10],
                2.0,
            ),
            twigs([0.05, 0.2], 3.0),
        ],
        evidence: "Authored from botanical descriptions of Pacific crabapple: toothed blades 4 to \
            10 cm, sometimes lobed, on petioles of 1 to 3 cm, orange to red in autumn and drying \
            brown; small oblong apples 1 to 1.5 cm, yellow to red. Colours and curl chosen by eye; \
            numbers plausible, not measured.",
    },
    Litter {
        species: "populus-tremuloides",
        tile_m: 1.0,
        mean: [0.194, 0.145, 0.068],
        relief_m: 0.025,
        fall: Fall::Leaves {
            length_m: [0.05, 0.12],
            shape: None,
        },
        shed: &[[0.68, 0.52, 0.06], [0.64, 0.46, 0.05], [0.64, 0.38, 0.06]],
        dry: &[[0.48, 0.36, 0.16], [0.42, 0.30, 0.14], [0.36, 0.29, 0.18]],
        curl: 0.4,
        pale_beneath: 0.2,
        decay: 0.6,
        extras: &[twigs([0.05, 0.2], 3.0)],
        evidence: "Authored from botanical descriptions of quaking aspen: nearly round, finely \
            toothed blades 3 to 7 cm on flattened petioles about as long, golden in autumn and \
            drying tan to grey-brown. Colours and curl chosen by eye; numbers plausible, not \
            measured.",
    },
    Litter {
        species: "populus-trichocarpa",
        tile_m: 2.0,
        mean: [0.145, 0.115, 0.067],
        relief_m: 0.047,
        fall: Fall::Leaves {
            length_m: [0.10, 0.20],
            shape: None,
        },
        shed: &[[0.62, 0.52, 0.10], [0.56, 0.46, 0.09], [0.58, 0.42, 0.08]],
        dry: &[[0.40, 0.30, 0.16], [0.35, 0.28, 0.18], [0.44, 0.33, 0.18]],
        curl: 0.55,
        pale_beneath: 0.6,
        decay: 0.6,
        extras: &[
            extra(Catkin, [0.04, 0.08], 0.12, [0.34, 0.26, 0.16], 2.0),
            twigs([0.08, 0.3], 2.0),
        ],
        evidence: "Authored from botanical descriptions of black cottonwood: finely toothed, \
            broadly ovate blades 7 to 15 cm, whitish beneath, on petioles of 3 to 6 cm, yellow in \
            autumn and drying brown; spent catkins. Colours and curl chosen by eye; numbers \
            plausible, not measured.",
    },
    Litter {
        species: "prunus-emarginata",
        tile_m: 1.0,
        mean: [0.124, 0.08, 0.04],
        relief_m: 0.018,
        fall: Fall::Leaves {
            length_m: [0.04, 0.09],
            shape: None,
        },
        shed: &[[0.60, 0.38, 0.08], [0.56, 0.22, 0.08], [0.58, 0.48, 0.10]],
        dry: &[[0.38, 0.23, 0.12], [0.33, 0.21, 0.11], [0.42, 0.27, 0.13]],
        curl: 0.5,
        pale_beneath: 0.25,
        decay: 0.6,
        extras: &[twigs([0.04, 0.15], 4.0)],
        evidence: "Authored from botanical descriptions of bitter cherry: finely toothed blades 3 \
            to 8 cm with rounded tips, on petioles of 0.5 to 1.5 cm, yellow to red in autumn and \
            drying brown. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "quercus-garryana",
        tile_m: 1.5,
        mean: [0.123, 0.09, 0.054],
        relief_m: 0.048,
        fall: Fall::Leaves {
            length_m: [0.07, 0.16],
            shape: Some(Shape::Lobed(Lobed {
                width: 0.62,
                lobes: 4,
                depth: 0.55,
                round: 1.0,
                petiole: 0.08,
            })),
        },
        shed: &[[0.46, 0.34, 0.12], [0.42, 0.30, 0.11], [0.50, 0.40, 0.15]],
        dry: &[[0.38, 0.26, 0.14], [0.32, 0.22, 0.13], [0.42, 0.31, 0.18]],
        curl: 0.7,
        pale_beneath: 0.4,
        decay: 0.25,
        extras: &[
            extra(ExtraKind::Fruit, [0.02, 0.03], 0.7, [0.40, 0.28, 0.14], 1.5),
            twigs([0.05, 0.25], 3.0),
        ],
        evidence: "Authored from botanical descriptions of Oregon white oak: leathery blades 5 to \
            15 cm, deeply cut into rounded lobes, paler beneath, on petioles of 1 to 2 cm, \
            yellow-brown in autumn, drying brown and crinkled and slow to rot; acorns 2 to 3 cm. \
            The crown wears sprigs of toothed leaves; a fallen leaf seen whole takes the lobed \
            shape. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
    Litter {
        species: "salix-lasiandra",
        tile_m: 1.5,
        mean: [0.138, 0.11, 0.057],
        relief_m: 0.029,
        fall: Fall::Leaves {
            length_m: [0.06, 0.16],
            shape: None,
        },
        shed: &[[0.62, 0.54, 0.12], [0.54, 0.50, 0.14], [0.58, 0.46, 0.10]],
        dry: &[[0.42, 0.32, 0.16], [0.36, 0.28, 0.15], [0.46, 0.35, 0.18]],
        curl: 0.65,
        pale_beneath: 0.45,
        decay: 0.65,
        extras: &[twigs([0.05, 0.25], 4.0)],
        evidence: "Authored from botanical descriptions of Pacific willow: narrow, long-pointed, \
            finely toothed blades 5 to 15 cm long and 1 to 3 cm wide, whitish beneath, on \
            petioles of 0.5 to 1.5 cm, yellow in autumn, drying brown and rolling along their \
            length. Colours and curl chosen by eye; numbers plausible, not measured.",
    },
];

/// The newest share of a look's pieces: they fell this season and are
/// still drying, while the older ones have dried.
pub const DRYING_SHARE: f64 = 0.25;

/// The angle, radians, the edges of a leaf of the most curl turn through
/// across its template's half-width once dry: past upright, so they roll
/// over.
const CUP_RADIANS: f64 = 2.2;

/// How far in from a blade's margin, as a share of its template, drying
/// starts before it reaches the middle.
const MARGIN_REACH: f64 = 0.1;

/// The number of a species' litter in [`LITTERS`].
#[must_use]
pub fn litter_index(species: &str) -> Option<usize> {
    LITTERS.iter().position(|litter| litter.species == species)
}

/// Whether litter `index` of [`LITTERS`] is a conifer's, needles or
/// sprays of scale leaves, which the ground's generic `Needles` word
/// stands for; a broadleaf's leaves stand in for its `Leaves`.
#[must_use]
pub fn litter_needles(index: usize) -> Option<bool> {
    LITTERS
        .get(index)
        .map(|litter| !matches!(litter.fall, Fall::Leaves { .. }))
}

/// The tile, mean colour and relief of litter look `index` without drawing
/// it.
#[must_use]
pub fn litter_look_size(index: usize) -> Option<(f32, [f32; 3], f32)> {
    LITTERS
        .get(index)
        .map(|litter| (litter.tile_m, litter.mean, litter.relief_m))
}

/// Draw every litter look for `seed`, in [`LITTERS`] order.
#[must_use]
pub fn litter_looks(seed: u64) -> Vec<GroundLook> {
    (0..LITTERS.len())
        .filter_map(|index| litter_look(index, seed))
        .collect()
}

/// Draw litter look `index` of [`LITTERS`] for `seed`: each look is drawn
/// on its own, so a viewer can draw them on several threads.
#[must_use]
pub fn litter_look(index: usize, seed: u64) -> Option<GroundLook> {
    litter_drawn(index, seed).map(|drawn| drawn.look)
}

/// A drawn litter look and what its drawing gives before it is scaled to
/// its litter's mean: for tuning [`Litter::mean`] and [`Litter::relief_m`].
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnLitter {
    pub look: GroundLook,
    /// The mean colour of the pieces as drawn, linear RGB, before scaling.
    pub drawn_mean: [f64; 3],
    /// The height of the highest texel over the lowest as drawn, metres.
    pub drawn_relief_m: f64,
}

/// [`litter_look`] with what its drawing gives before scaling.
#[must_use]
pub fn litter_drawn(index: usize, seed: u64) -> Option<DrawnLitter> {
    let litter = LITTERS.get(index)?;
    let mut canvas = Canvas::new(GROUND_LOOK_SIZE);
    draw(
        &mut canvas,
        litter,
        hash_words(&[seed, hash_str("litter"), hash_str(litter.species)]),
    );
    Some(DrawnLitter {
        look: GroundLook {
            name: litter.species,
            tile_m: litter.tile_m,
            mean: litter.mean,
            relief_m: litter.relief_m,
            rgba: canvas.finish(litter.mean),
        },
        drawn_mean: canvas.mean_colour(),
        drawn_relief_m: canvas.height_span(),
    })
}

/// The leaf a species' crown wears, taken singly: its `leaf` organ's
/// look, a sprig's leaf as a simple leaf.
#[must_use]
pub fn own_leaf(species: &str) -> Option<Shape> {
    let spec = PlantSpec::builtin(species).ok()?;
    let look = spec.appearance.organs.get("leaf")?;
    Some(match &look.shape {
        Shape::Sprig(sprig) => Shape::Simple(Simple {
            width: sprig.width,
            widest: sprig.widest,
            tip: sprig.tip,
            base: sprig.base,
            teeth: sprig.teeth,
            tooth_depth: sprig.tooth_depth,
            petiole: sprig.petiole,
        }),
        other => other.clone(),
    })
}

/// The spray of scale leaves a species' crown wears.
fn own_spray(species: &str) -> Option<Shape> {
    let spec = PlantSpec::builtin(species).ok()?;
    let look = spec.appearance.organs.get("spray")?;
    matches!(look.shape, Shape::Scales(_)).then(|| look.shape.clone())
}

/// The share of a template's square its organ covers.
fn template_cover(templates: &Templates, template: usize) -> f64 {
    let steps = 32_u32;
    let mut sum = 0.0;
    for j in 0..steps {
        for i in 0..steps {
            // Sample centres of a 32 by 32 grid, well inside f32.
            #[allow(clippy::cast_possible_truncation)]
            let (u, v) = (
                ((f64::from(i) + 0.5) / f64::from(steps)) as f32,
                ((f64::from(j) + 0.5) / f64::from(steps)) as f32,
            );
            sum += f64::from(templates.sample(template, u, v).coverage);
        }
    }
    (sum / f64::from(steps * steps)).max(0.05)
}

/// How many pieces of `area` square texels cover `cover` of a canvas
/// `size` texels a side, at least one and at most `most`.
fn pieces(size: usize, area: f64, cover: f64, most: u64) -> u64 {
    let edge = to_f64(size);
    // A count of at most `most`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = libm::ceil(cover * edge * edge / area.max(1.0)) as u64;
    count.clamp(1, most)
}

/// How far piece `i` has dried, 0 as it fell to 1 dry, from how new it is
/// (`fresh`, 0 the oldest to 1 the newest): of the newest
/// [`DRYING_SHARE`], the newer a piece the less it may have dried.
fn dryness(draws: Draws, i: u64, fresh: f64) -> f64 {
    let newness = ((fresh - (1.0 - DRYING_SHARE)) / DRYING_SHARE).clamp(0.0, 1.0);
    1.0 - newness * draws.range(i, 12, 0.3, 1.0)
}

/// A piece's colour as it rots by `age` (0 the newest to 1 the oldest):
/// darker and greyer, the sooner the more its litter decays.
fn rotted(colour: [f64; 3], age: f64, decay: f64) -> [f64; 3] {
    let rot = (age * (0.5 + 0.7 * decay)).min(1.0);
    let dark = mix(scale(colour, 0.45), [0.10, 0.08, 0.06], 0.35);
    mix(colour, dark, rot * rot)
}

/// A leaf's underside: paler and greyer than its top by `pale`.
fn beneath(colour: [f64; 3], pale: f64) -> [f64; 3] {
    let grey = 0.2126 * colour[0] + 0.7152 * colour[1] + 0.0722 * colour[2];
    scale(mix(colour, [grey; 3], 0.4 * pale), 1.0 + 0.35 * pale)
}

/// How deep the loose pieces of `fall` lie over the duff, metres.
fn stack_m(fall: &Fall) -> f64 {
    match fall {
        Fall::Needles {
            length_m, width_m, ..
        } => (0.1 * length_m[1]).max(6.0 * width_m).clamp(0.006, 0.03),
        Fall::Sprays { length_m } => (0.1 * length_m[1]).clamp(0.006, 0.02),
        Fall::Leaves { length_m, .. } => (0.08 * length_m[1]).clamp(0.008, 0.04),
    }
}

/// How far piece `i` of `count` lies from the bottom of the litter (the
/// oldest, 0) to its top (the newest, 1).
fn layer(i: u64, count: u64) -> f64 {
    // Counts of at most a few hundred thousand: exact in f64.
    #[allow(clippy::cast_precision_loss)]
    let fraction = (i as f64 + 0.5) / count.max(1) as f64;
    fraction
}

/// The point `length` texels from `a` toward `angle`.
fn toward(a: [f64; 2], angle: f64, length: f64) -> [f64; 2] {
    [
        a[0] + length * math::cos(angle),
        a[1] + length * math::sin(angle),
    ]
}

/// Whether the texel at `(u, v)` of a piece survives its rotting: holes
/// open where `noise` falls below `eaten`.
fn intact(holes: Draws, cells: usize, eaten: f64, u: f64, v: f64) -> bool {
    eaten <= 0.0 || value_noise(holes, cells, 0, u * 512.0, v * 512.0, 512) > eaten
}

fn draw(canvas: &mut Canvas, litter: &Litter, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let per_m = to_f64(size) / f64::from(litter.tile_m);
    let stack = stack_m(&litter.fall);
    // The duff the litter rots into: humps of a few millimetres.
    let humus = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(humus, 8, 4, x, y, size);
        (mix([0.06, 0.04, 0.025], [0.12, 0.08, 0.045], n), 0.003 * n)
    });
    match &litter.fall {
        Fall::Needles {
            length_m,
            width_m,
            bundle,
        } => draw_needles(
            canvas,
            litter,
            draws,
            per_m,
            [length_m[0], length_m[1], *width_m],
            *bundle,
        ),
        Fall::Sprays { length_m } => draw_sprays(canvas, litter, draws, per_m, *length_m),
        Fall::Leaves { length_m, shape } => {
            let shape = shape
                .clone()
                .or_else(|| own_leaf(litter.species))
                .unwrap_or_else(|| Shape::Simple(Simple::default()));
            draw_leaves(canvas, litter, draws, per_m, *length_m, &shape);
        }
    }
    for (k, extra) in (0_u64..).zip(litter.extras) {
        draw_extra(
            canvas,
            extra,
            draws.stream(100 + k),
            [per_m, f64::from(litter.tile_m), stack],
        );
    }
}

/// Needles `size_m` = `[shortest, longest, width]`, singly or in bundles
/// of `bundle`, oldest first.
fn draw_needles(
    canvas: &mut Canvas,
    litter: &Litter,
    draws: Draws,
    per_m: f64,
    size_m: [f64; 3],
    bundle: u32,
) {
    let size = canvas.size;
    let stack = stack_m(&litter.fall);
    let [low, high, width] = size_m.map(|metres| metres * per_m);
    let width = width.max(1.0);
    // A needle stands about as high as it is wide.
    let thick_m = size_m[2];
    let bundle = bundle.max(1);
    let count = pieces(
        size,
        f64::midpoint(low, high) * width * f64::from(bundle),
        1.1,
        60_000,
    );
    let needles = draws.stream(2);
    for i in 0..count {
        let a = spot(needles, i, size);
        let angle = needles.unit(i, 3) * 2.0 * PI;
        let fresh = layer(i, count);
        let age = 1.0 - fresh;
        // Old needles break into shorter pieces.
        let length = needles.range(i, 4, low, high) * (1.0 - 0.4 * litter.decay * age);
        let dried = dryness(needles, i, fresh);
        let colour = scale(
            rotted(
                mix(
                    needles.pick(i, 5, litter.shed),
                    needles.pick(i, 13, litter.dry),
                    dried,
                ),
                age,
                litter.decay,
            ),
            needles.range(i, 6, 0.8, 1.15),
        );
        let base = stack * fresh;
        let thick = width * needles.range(i, 7, 0.8, 1.2);
        // Rounded across, a little lighter along its crest.
        let rod = |round: f64| (scale(colour, 0.85 + 0.2 * round), base + thick_m * round);
        if bundle == 1 {
            // Bent a little.
            let bend = needles.range(i, 8, -0.12, 0.12);
            let mid = toward(a, angle, 0.5 * length);
            canvas.stroke_with(a, mid, thick, rod);
            canvas.stroke_with(mid, toward(mid, angle + bend, 0.5 * length), thick, rod);
        } else {
            // Splayed a little from their sheath, each bent a little.
            for k in 0..bundle {
                let n = i * 8 + u64::from(k);
                let splay = needles.range(n, 9, -0.12, 0.12);
                let own = length * needles.range(n, 10, 0.9, 1.0);
                let mid = toward(a, angle + splay, 0.5 * own);
                let bend = needles.range(n, 11, -0.1, 0.1);
                canvas.stroke_with(a, mid, thick, rod);
                canvas.stroke_with(
                    mid,
                    toward(mid, angle + splay + bend, 0.5 * own),
                    thick,
                    rod,
                );
            }
            // The sheath.
            canvas.stroke_with(a, toward(a, angle, 0.06 * length), thick * 1.6, |round| {
                (
                    scale(colour, 0.6 * (0.85 + 0.2 * round)),
                    base + 1.3 * thick_m * round,
                )
            });
        }
    }
}

/// Flat sprays of scale leaves, oldest first, over their broken bits.
fn draw_sprays(canvas: &mut Canvas, litter: &Litter, draws: Draws, per_m: f64, length_m: [f64; 2]) {
    let size = canvas.size;
    let stack = stack_m(&litter.fall);
    let shape = own_spray(litter.species).unwrap_or_else(|| Shape::Scales(Scales::default()));
    let templates = organ_templates(&[shape]);
    let aspect = templates.templates.first().map_or(0.8, |t| t.aspect);
    let cover = template_cover(&templates, 0);
    let [low, high] = length_m.map(|length| length * per_m);
    let mean = f64::midpoint(low, high);
    // Bits of old sprays broken up in the duff.
    let bits = draws.stream(3);
    let bit_count = pieces(size, 6.0, 0.35, 40_000);
    for i in 0..bit_count {
        let a = spot(bits, i, size);
        let angle = bits.unit(i, 3) * 2.0 * PI;
        let colour = scale(
            rotted(
                bits.pick(i, 4, litter.dry),
                bits.range(i, 5, 0.3, 1.0),
                litter.decay,
            ),
            bits.range(i, 6, 0.75, 1.2),
        );
        let base = 0.2 * stack * bits.unit(i, 8);
        canvas.stroke_with(
            a,
            toward(a, angle, bits.range(i, 7, 2.0, 6.0)),
            1.5,
            |round| (scale(colour, 0.85 + 0.2 * round), base + 0.001 * round),
        );
    }
    let sprays = draws.stream(2);
    let count = pieces(size, mean * mean * aspect * cover, 1.4, 20_000);
    for i in 0..count {
        let fresh = layer(i, count);
        let age = 1.0 - fresh;
        let dried = dryness(sprays, i, fresh);
        let colour = scale(
            rotted(
                mix(
                    sprays.pick(i, 5, litter.shed),
                    sprays.pick(i, 13, litter.dry),
                    dried,
                ),
                age,
                litter.decay,
            ),
            sprays.range(i, 6, 0.8, 1.15),
        );
        let holes = sprays.stream(1000 + i);
        let eaten = 0.7 * litter.decay * age * age;
        let length = sprays.range(i, 4, low, high) * (1.0 - 0.3 * age);
        // A drying spray turns up at its sides.
        let curl_m = 0.12 * litter.curl * dried * length / per_m;
        canvas.stamp_worn(
            &templates,
            0,
            spot(sprays, i, size),
            [length, sprays.unit(i, 7) * 2.0 * PI],
            colour,
            stack * fresh + 0.0015,
            curl_m,
            |u, v| intact(holes, 4, eaten, u, v),
        );
    }
}

/// A leaf's template seen whole, with how near each of its texels lies to
/// the blade's margin.
struct Blade {
    templates: Templates,
    /// Width over length.
    aspect: f64,
    /// Per template texel, row by row from the base: 1 on the margin,
    /// falling to 0 [`MARGIN_REACH`] of the template in.
    margin: Vec<f32>,
}

impl Blade {
    fn new(shape: &Shape) -> Self {
        let templates = organ_templates(std::slice::from_ref(shape));
        let aspect = templates.templates.first().map_or(0.5, |t| t.aspect);
        let margin = templates
            .templates
            .first()
            .map_or_else(Vec::new, |t| margin_field(&t.coverage, templates.size));
        Self {
            templates,
            aspect,
            margin,
        }
    }

    /// The template's texel at `(u, v)`, across the blade and along it
    /// from its base, 0 to 1.
    fn texel(&self, u: f64, v: f64) -> Texel {
        // Inside the template's square.
        #[allow(clippy::cast_possible_truncation)]
        self.templates.sample(0, u as f32, v as f32)
    }

    /// How near the texel at `(u, v)` lies to the blade's margin, 1 on it.
    fn margin(&self, u: f64, v: f64) -> f64 {
        let size = self.templates.size;
        let texel = |coordinate: f64| -> usize {
            // Clamped into the template.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = (coordinate.clamp(0.0, 0.999_999) * to_f64(size)) as usize;
            index.min(size - 1)
        };
        self.margin
            .get(texel(v) * size + texel(u))
            .map_or(0.0, |&near| f64::from(near))
    }
}

/// How near each texel of a template's `coverage` (`size` a side) lies to
/// its blade's margin: a chamfer distance to the nearest uncovered texel or
/// the square's edge, 1 on the margin, falling to 0 [`MARGIN_REACH`] of
/// the template in.
fn margin_field(coverage: &[f32], size: usize) -> Vec<f32> {
    let far = f64::from(u32::MAX);
    let mut distance: Vec<f64> = coverage
        .iter()
        .map(|&cover| if cover < 0.5 { 0.0 } else { far })
        .collect();
    // Past the square's edge is uncovered.
    let at = |distance: &[f64], x: usize, y: usize, dx: isize, dy: isize| -> f64 {
        match (x.checked_add_signed(dx), y.checked_add_signed(dy)) {
            (Some(x), Some(y)) if x < size && y < size => distance[y * size + x],
            _ => 0.0,
        }
    };
    let diagonal = core::f64::consts::SQRT_2;
    for y in 0..size {
        for x in 0..size {
            let here = distance[y * size + x];
            let best = here
                .min(at(&distance, x, y, -1, 0) + 1.0)
                .min(at(&distance, x, y, 0, -1) + 1.0)
                .min(at(&distance, x, y, -1, -1) + diagonal)
                .min(at(&distance, x, y, 1, -1) + diagonal);
            distance[y * size + x] = best;
        }
    }
    for y in (0..size).rev() {
        for x in (0..size).rev() {
            let here = distance[y * size + x];
            let best = here
                .min(at(&distance, x, y, 1, 0) + 1.0)
                .min(at(&distance, x, y, 0, 1) + 1.0)
                .min(at(&distance, x, y, 1, 1) + diagonal)
                .min(at(&distance, x, y, -1, 1) + diagonal);
            distance[y * size + x] = best;
        }
    }
    let reach = MARGIN_REACH * to_f64(size);
    distance
        .iter()
        .map(|&texels| {
            // Between 0 and 1.
            #[allow(clippy::cast_possible_truncation)]
            let near = (1.0 - (texels / reach).min(1.0)) as f32;
            near
        })
        .collect()
}

/// One fallen leaf as it lies on the litter.
struct Leaf {
    /// Its centre, texels.
    centre: [f64; 2],
    /// Texels from its petiole's base to its tip, and the angle it points
    /// along.
    length: f64,
    angle: f64,
    /// Metres a texel.
    texel_m: f64,
    /// The height it lies at, metres.
    base_m: f64,
    /// Whether it lies face up, cupping; face down it arches.
    face_up: bool,
    /// The angle, radians, its blade turns through across its template's
    /// half-width, and the angle each end turns through along its length.
    cup: f64,
    bend: f64,
    /// The height of its crinkles, metres.
    crinkle_m: f64,
    /// How far it has dried, 0 to 1, from the colour it fell in to the
    /// colour it dries to.
    dryness: f64,
    shed: [f64; 3],
    dry: [f64; 3],
    /// Its age (0 the newest to 1 the oldest), its litter's decay and the
    /// share of it eaten through.
    age: f64,
    decay: f64,
    eaten: f64,
    pale_beneath: f64,
    /// Its own draws: its blotches, crinkles and holes.
    draws: Draws,
}

/// What shows of a curled leaf at one point seen from above.
struct Shown {
    /// The flat distance from the midrib of the point that shows, in leaf
    /// lengths.
    s: f64,
    /// Whether its underside shows.
    beneath: bool,
    /// How high the point rises over the leaf's lowest, leaf lengths.
    rise: f64,
}

/// The flat distance along a leaf from its middle, leaf lengths, of the
/// point that shows `along` from the middle when the leaf curls along its
/// length with curvature `k` (per leaf length), and how high that point
/// rises, leaf lengths: face up the ends lift, face down the middle
/// arches.
fn unbend(along: f64, k: f64, face_up: bool) -> Option<(f64, f64)> {
    if k < 1.0e-6 {
        return Some((along, 0.0));
    }
    let q = k * along;
    if q.abs() > 1.0 {
        return None;
    }
    let turn = math::asin(q);
    let rise = if face_up {
        (1.0 - math::cos(turn)) / k
    } else {
        (math::cos(turn) - math::cos(0.5 * k)) / k
    };
    Some((turn / k, rise))
}

/// What shows `across` leaf lengths from the midrib of a blade cupped with
/// curvature `k` (per leaf length) out to `half`, its template's
/// half-width. Face up the blade cups, its edges rising, and an edge
/// rolled past upright shows its underside over the trough; face down it
/// arches, and what rolls past upright hides under it. `covered(s)` says
/// whether the blade is there at flat distance `s` from the midrib.
fn unroll(
    across: f64,
    k: f64,
    face_up: bool,
    half: f64,
    covered: impl Fn(f64) -> bool,
) -> Option<Shown> {
    let side = if across < 0.0 { -1.0 } else { 1.0 };
    if k < 1.0e-6 {
        return (across.abs() <= half && covered(across)).then_some(Shown {
            s: across,
            beneath: !face_up,
            rise: 0.0,
        });
    }
    let q = k * across.abs();
    if q > 1.0 {
        return None;
    }
    let turn = math::asin(q);
    let flat = turn / k;
    if face_up {
        let over = (PI - turn) / k;
        if over <= half && covered(side * over) {
            return Some(Shown {
                s: side * over,
                beneath: true,
                rise: (1.0 + math::cos(turn)) / k,
            });
        }
        (flat <= half && covered(side * flat)).then(|| Shown {
            s: side * flat,
            beneath: false,
            rise: (1.0 - math::cos(turn)) / k,
        })
    } else {
        let edge = (k * half).min(0.5 * PI);
        (flat <= half && covered(side * flat)).then(|| Shown {
            s: side * flat,
            beneath: true,
            rise: (math::cos(turn) - math::cos(edge)) / k,
        })
    }
}

/// Lay `leaf` of `blade`: each texel shows the part of the curled leaf
/// above it, dried from its margin and tip inward in blotches.
fn lay_leaf(canvas: &mut Canvas, blade: &Blade, leaf: &Leaf) {
    let half = 0.5 * blade.aspect;
    // Curvatures per leaf length: across, so the template's edge turns
    // through `cup`; along, so each end turns through `bend`.
    let across_k = leaf.cup / half.max(1.0e-3);
    let along_k = 2.0 * leaf.bend;
    let length_m = leaf.length * leaf.texel_m;
    let width = leaf.length * blade.aspect;
    let reach = 0.5 * libm::sqrt(leaf.length * leaf.length + width * width) + 1.0;
    let (cos, sin) = (math::cos(leaf.angle), math::sin(leaf.angle));
    let c = leaf.centre;
    let (blotches, crinkles, fungi) = (
        leaf.draws.stream(1),
        leaf.draws.stream(2),
        leaf.draws.stream(3),
    );
    // Fungi spot a leaf darker as it dries and ages.
    let darkening = 0.35 * leaf.dryness * (0.4 + 0.6 * leaf.age);
    // A face-up leaf's trough is shaded by its raised edges.
    let hollow = 0.3 * math::smoothstep(0.0, 0.5 * PI, leaf.cup);
    #[allow(clippy::cast_possible_truncation)]
    for y in (libm::floor(c[1] - reach) as i64)..=(libm::ceil(c[1] + reach) as i64) {
        #[allow(clippy::cast_possible_truncation)]
        for x in (libm::floor(c[0] - reach) as i64)..=(libm::ceil(c[0] + reach) as i64) {
            #[allow(clippy::cast_precision_loss)]
            let p = [x as f64 + 0.5 - c[0], y as f64 + 0.5 - c[1]];
            let along = (p[0] * cos + p[1] * sin) / leaf.length;
            let across = (-p[0] * sin + p[1] * cos) / leaf.length;
            let Some((flat_along, lift)) = unbend(along, along_k, leaf.face_up) else {
                continue;
            };
            let v = flat_along + 0.5;
            if !(0.0..1.0).contains(&v) {
                continue;
            }
            let covered = |s: f64| {
                let u = s / blade.aspect + 0.5;
                (0.0..1.0).contains(&u)
                    && blade.texel(u, v).coverage > 0.02
                    && intact(leaf.draws, 5, leaf.eaten, u, v)
            };
            let Some(shown) = unroll(across, across_k, leaf.face_up, half, covered) else {
                continue;
            };
            let u = shown.s / blade.aspect + 0.5;
            let texel = blade.texel(u, v);
            // Drying starts at the margin and the tip and spreads inward
            // in blotches.
            let blotch = value_noise(blotches, 5, 0, u * 256.0, v * 256.0, 256);
            let resists = 0.12 + 0.6 * (1.0 - blade.margin(u, v))
                - 0.18 * math::smoothstep(0.6, 1.0, v)
                + 0.3 * (blotch - 0.5);
            let dried = math::smoothstep(resists - 0.1, resists + 0.1, leaf.dryness);
            let mould = math::smoothstep(
                0.6,
                0.75,
                value_noise(fungi, 9, 0, u * 256.0, v * 256.0, 256),
            );
            let mut colour = scale(
                rotted(mix(leaf.shed, leaf.dry, dried), leaf.age, leaf.decay),
                1.0 - darkening * mould,
            );
            if shown.beneath {
                colour = beneath(colour, leaf.pale_beneath);
            }
            // How far round the blade has turned at this point, 0 at the
            // midrib to 1 at the template's edge.
            let turned = (across_k * shown.s.abs() / leaf.cup.max(1.0e-6)).min(1.0);
            let open = if !leaf.face_up {
                // Arched: its edges dip toward the litter.
                1.0 - 0.12 * turned * turned
            } else if shown.beneath {
                1.0
            } else {
                1.0 - hollow * (1.0 - turned)
            };
            let crinkle = leaf.crinkle_m
                * (value_noise(crinkles, 7, 0, u * 256.0, v * 256.0, 256) - 0.5)
                * leaf.dryness;
            canvas.lay(
                x,
                y,
                scale(colour, f64::from(texel.brightness).clamp(0.6, 1.3) * open),
                leaf.base_m + (shown.rise + lift) * length_m + crinkle,
                f64::from(texel.coverage),
            );
        }
    }
}

/// Leaves of `shape`, oldest first: the old ones smaller, darker, flatter
/// and eaten through; the newest drying, curling and cupping on top.
fn draw_leaves(
    canvas: &mut Canvas,
    litter: &Litter,
    draws: Draws,
    per_m: f64,
    length_m: [f64; 2],
    shape: &Shape,
) {
    let size = canvas.size;
    let stack = stack_m(&litter.fall);
    let blade = Blade::new(shape);
    let cover = template_cover(&blade.templates, 0);
    let [low, high] = length_m.map(|length| length * per_m);
    let mean = f64::midpoint(low, high);
    let leaves = draws.stream(2);
    let count = pieces(size, mean * mean * blade.aspect * cover, 2.0, 30_000);
    for i in 0..count {
        let fresh = layer(i, count);
        let age = 1.0 - fresh;
        let dryness = dryness(leaves, i, fresh);
        // Older leaves lie flattened under the newer ones.
        let curled = litter.curl * dryness * (1.0 - 0.5 * age);
        let length = leaves.range(i, 4, low, high) * (1.0 - 0.3 * litter.decay * age);
        let tint = leaves.range(i, 6, 0.85, 1.15);
        lay_leaf(
            canvas,
            &blade,
            &Leaf {
                centre: spot(leaves, i, size),
                length,
                angle: leaves.unit(i, 7) * 2.0 * PI,
                texel_m: 1.0 / per_m,
                base_m: stack * fresh,
                face_up: leaves.unit(i, 9) < 0.5,
                cup: CUP_RADIANS * curled * leaves.range(i, 10, 0.6, 1.15),
                bend: curled * leaves.range(i, 11, 0.0, 0.5),
                crinkle_m: 0.03 * curled * length / per_m,
                dryness,
                shed: scale(leaves.pick(i, 5, litter.shed), tint),
                dry: scale(leaves.pick(i, 13, litter.dry), tint),
                age,
                decay: litter.decay,
                eaten: 0.8 * litter.decay * age * age,
                pale_beneath: litter.pale_beneath,
                draws: leaves.stream(1000 + i),
            },
        );
    }
}

/// `extra`'s pieces on top of the foliage, `scales` = `[texels a metre,
/// metres a tile, metres deep the foliage lies]`.
#[allow(clippy::too_many_lines)] // One arm per kind of piece.
fn draw_extra(canvas: &mut Canvas, extra: &Extra, draws: Draws, scales: [f64; 3]) {
    let [per_m, tile_m, stack] = scales;
    let size = canvas.size;
    let texel_m = 1.0 / per_m;
    // At most a few hundred a tile.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = libm::round(extra.per_m2 * tile_m * tile_m).max(0.0) as u64;
    let [low, high] = extra.length_m.map(|length| length * per_m);
    let cones = (extra.kind == Cone).then(|| {
        organ_templates(&[Shape::Fruit(Fruit {
            aspect: extra.aspect,
            cone: 1.0,
        })])
    });
    for i in 0..count {
        let c = spot(draws, i, size);
        let angle = draws.unit(i, 3) * 2.0 * PI;
        let length = draws.range(i, 4, low, high).max(2.0);
        let colour = scale(extra.colour, draws.range(i, 5, 0.8, 1.15));
        // Lying on the litter, some sunk into it.
        let base = stack * draws.range(i, 11, 0.5, 1.0);
        // Its width, metres.
        let wide_m = length * extra.aspect * texel_m;
        match extra.kind {
            Cone => {
                // Scales over a body rounded across: highest along its
                // axis.
                if let Some(templates) = &cones {
                    canvas.stamp(
                        templates,
                        0,
                        c,
                        length,
                        angle,
                        colour,
                        base + wide_m,
                        -0.8 * wide_m,
                    );
                }
            }
            ExtraKind::Fruit => {
                let radius = 0.5 * length;
                canvas.dome(
                    c,
                    radius,
                    radius * extra.aspect,
                    angle,
                    base,
                    0.5 * wide_m,
                    |_, _, dome| scale(colour, 0.6 + 0.5 * dome),
                );
            }
            ConeScale => {
                let radius = 0.5 * length;
                canvas.dome(
                    c,
                    radius,
                    radius * extra.aspect,
                    angle,
                    base,
                    0.002,
                    |_, v, dome| scale(colour, 0.75 + 0.3 * dome + 0.1 * v),
                );
            }
            PairedSamara | Samara => {
                let wings = if extra.kind == PairedSamara { 2 } else { 1 };
                let spread = draws.range(i, 6, 1.0, 1.6);
                for wing in 0..wings {
                    let turn = angle + f64::from(wing) * spread;
                    // The wing, then the seed at its base.
                    canvas.dome(
                        toward(c, turn, 0.55 * length),
                        0.45 * length,
                        0.45 * length * extra.aspect,
                        turn,
                        base,
                        0.0015,
                        |u, _, _| scale(colour, 0.95 + 0.1 * u),
                    );
                    canvas.dome(
                        toward(c, turn, 0.12 * length),
                        0.14 * length,
                        0.1 * length,
                        turn,
                        base,
                        0.004,
                        |_, _, dome| scale(colour, 0.6 + 0.3 * dome),
                    );
                }
            }
            Catkin => {
                // A limp chain of scales along a gentle curve.
                let thick = (extra.aspect * length).max(1.5);
                let bend = draws.range(i, 6, -0.6, 0.6);
                // A few dozen beads.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let beads = libm::floor((length / thick).clamp(3.0, 40.0)) as u32;
                for k in 0..beads {
                    let t = f64::from(k) / f64::from(beads);
                    let heading = angle + bend * t;
                    canvas.dome(
                        toward(c, heading, length * t),
                        0.6 * thick,
                        0.5 * thick,
                        heading,
                        base,
                        0.5 * thick * texel_m,
                        |_, _, dome| scale(colour, 0.6 + 0.45 * dome),
                    );
                }
            }
            Bark => {
                // A curled strip, lighter along its middle.
                let curve = draws.range(i, 6, -1.2, 1.2);
                let wide = (extra.aspect * length).max(1.5);
                let steps = 6_u32;
                let mut at = c;
                for k in 0..steps {
                    let heading = angle + curve * f64::from(k) / f64::from(steps);
                    let next = toward(at, heading, length / f64::from(steps));
                    canvas.stroke_with(at, next, wide, |round| {
                        (scale(colour, 0.85 + 0.2 * round), base + 0.004 * round)
                    });
                    canvas.stroke_with(at, next, (0.35 * wide).max(1.0), |round| {
                        (scale(colour, 1.1 + 0.15 * round), base + 0.005)
                    });
                    at = next;
                }
            }
            Twig => {
                let bend = draws.range(i, 6, -0.5, 0.5);
                let width = (extra.aspect * length).clamp(1.2, 4.0);
                // A twig stands as high as it is thick.
                let rod = |thick: f64| {
                    move |round: f64| {
                        (
                            scale(colour, 0.8 + 0.3 * round),
                            base + thick * texel_m * round,
                        )
                    }
                };
                let mid = toward(c, angle, 0.5 * length);
                canvas.stroke_with(c, mid, width, rod(width));
                canvas.stroke_with(
                    mid,
                    toward(mid, angle + bend, 0.5 * length),
                    width * 0.8,
                    rod(width * 0.8),
                );
                // A side shoot now and then.
                if draws.unit(i, 7) < 0.5 {
                    let fork = toward(c, angle, draws.range(i, 8, 0.2, 0.7) * length);
                    let flank = if draws.unit(i, 10) < 0.5 { -1.0 } else { 1.0 };
                    let turn = angle + flank * draws.range(i, 9, 0.5, 1.0);
                    canvas.stroke_with(
                        fork,
                        toward(fork, turn, 0.3 * length),
                        width * 0.6,
                        rod(width * 0.6),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::{GROUND_LOOK_NAMES, mip_chain, srgb_to_linear};
    use crate::spec::SPECIES;

    #[test]
    fn every_litter_names_a_builtin_species_once() {
        for (index, litter) in LITTERS.iter().enumerate() {
            let name = litter.species;
            assert!(SPECIES.iter().any(|(id, _)| *id == name), "{name}");
            assert_eq!(litter_index(name), Some(index));
            assert!(!GROUND_LOOK_NAMES.contains(&name));
            assert!(!litter.shed.is_empty() && !litter.dry.is_empty(), "{name}");
            assert!(litter.evidence.starts_with("Authored"), "{name}");
            assert!(litter.tile_m >= 0.75 && litter.relief_m > 0.0, "{name}");
            for value in [litter.curl, litter.pale_beneath, litter.decay] {
                assert!((0.0..=1.0).contains(&value), "{name}");
            }
            match &litter.fall {
                Fall::Leaves { shape, length_m } => {
                    // A broadleaf's own leaf, unless it names a truer one.
                    assert!(
                        shape.is_some() || own_leaf(name).is_some(),
                        "{name} has no leaf"
                    );
                    assert!(length_m[0] > 0.0 && length_m[0] <= length_m[1]);
                }
                Fall::Sprays { length_m } => {
                    assert!(own_spray(name).is_some(), "{name}");
                    assert!(length_m[0] > 0.0 && length_m[0] <= length_m[1]);
                }
                Fall::Needles { length_m, .. } => {
                    assert!(length_m[0] > 0.0 && length_m[0] <= length_m[1]);
                    // Needles do not curl.
                    assert!(litter.curl == 0.0, "{name}");
                }
            }
        }
    }

    #[test]
    fn litter_looks_are_deterministic_and_average_to_their_means() {
        for index in [0, 5, 6, 8, 10, 11, 21] {
            let look = litter_look(index, 1).unwrap();
            assert_eq!(Some(&look), litter_look(index, 1).as_ref());
            assert_ne!(look.rgba, litter_look(index, 2).unwrap().rgba);
            assert_eq!(look.name, LITTERS[index].species);
            let texels = mip_chain(&look.rgba, GROUND_LOOK_SIZE);
            let last = texels.last().unwrap();
            for (channel, &byte) in last.iter().enumerate().take(3) {
                let mean = srgb_to_linear(byte);
                let want = f64::from(look.mean[channel]);
                assert!(
                    (mean - want).abs() < 0.02 + 0.06 * want,
                    "{}: channel {channel} averages {mean:.3}, wants {want:.3}",
                    look.name
                );
            }
            let (texels, _) = look.rgba.as_chunks::<4>();
            let (low, high) = texels.iter().fold((255, 0), |(lo, hi), texel| {
                (lo.min(texel[3]), hi.max(texel[3]))
            });
            assert_eq!((low, high), (0, 255), "{}", look.name);
        }
    }

    #[test]
    fn every_litter_declares_the_hue_and_relief_it_draws() {
        // Scaling a look to its mean keeps the drawing's hue only if the
        // mean has it: otherwise the scaling tints every piece, which
        // turned maple and alder litter olive. The relief is what the
        // pieces stand to, for the light and the parallax.
        for (index, litter) in LITTERS.iter().enumerate() {
            let drawn = litter_drawn(index, 1).unwrap();
            let [r, g, b] = drawn.drawn_mean;
            let mean = litter.mean.map(f64::from);
            for (declared, wanted) in [(mean[0] / mean[1], r / g), (mean[2] / mean[1], b / g)] {
                let ratio = declared / wanted;
                assert!(
                    (0.9..1.11).contains(&ratio),
                    "{}: declared {mean:.3?}, drawn hue {:.3?}",
                    litter.species,
                    drawn.drawn_mean.map(|channel| channel * mean[1] / g)
                );
            }
            let relief = f64::from(litter.relief_m);
            assert!(
                (drawn.drawn_relief_m - relief).abs() <= 0.25 * relief,
                "{}: declared relief {relief:.3} m, drawn {:.3} m",
                litter.species,
                drawn.drawn_relief_m
            );
        }
    }

    #[test]
    fn litter_is_not_one_flat_colour() {
        // Pieces of different browns over darker duff: a look's
        // brightness varies, but not as wildly as noise.
        for index in [6, 8, 10, 11] {
            let look = litter_look(index, 3).unwrap();
            let (texels, _) = look.rgba.as_chunks::<4>();
            let luminance: Vec<f64> = texels
                .iter()
                .map(|t| {
                    0.2126 * srgb_to_linear(t[0])
                        + 0.7152 * srgb_to_linear(t[1])
                        + 0.0722 * srgb_to_linear(t[2])
                })
                .collect();
            let n = to_f64(luminance.len());
            let mean = luminance.iter().sum::<f64>() / n;
            let spread =
                (luminance.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / n).sqrt() / mean;
            assert!(spread > 0.15 && spread < 1.2, "{}: {spread:.2}", look.name);
        }
    }

    /// A plain leaf 100 texels long and 1 mm a texel, pointing along x
    /// from texel 128 at 1 m over an empty canvas.
    fn test_leaf(dryness: f64, cup: f64, face_up: bool) -> Leaf {
        Leaf {
            centre: [128.0, 128.0],
            length: 100.0,
            angle: 0.0,
            texel_m: 0.001,
            base_m: 1.0,
            face_up,
            cup,
            bend: 0.0,
            crinkle_m: 0.0,
            dryness,
            shed: [0.6, 0.5, 0.1],
            dry: [0.3, 0.2, 0.1],
            age: 0.0,
            decay: 0.0,
            eaten: 0.0,
            pale_beneath: 0.5,
            draws: Draws(7),
        }
    }

    /// `leaf` laid on an empty canvas, with the number of texels it
    /// covers.
    fn lay_one(leaf: &Leaf) -> (Canvas, usize) {
        let blade = Blade::new(&Shape::Simple(Simple::default()));
        let mut canvas = Canvas::new(256);
        lay_leaf(&mut canvas, &blade, leaf);
        let mut covered = 0;
        for y in 0..256 {
            for x in 0..256 {
                if canvas.height_at(x, y) > 0.0 {
                    covered += 1;
                }
            }
        }
        (canvas, covered)
    }

    /// How far the leaf reaches across texel column `x` from its midrib,
    /// texels.
    fn reach(canvas: &Canvas, x: usize) -> usize {
        (0..128)
            .rev()
            .find(|&d| canvas.height_at(x, 128 + d) > 0.0)
            .unwrap_or(0)
    }

    #[test]
    fn a_drying_leaf_browns_from_its_margin_and_tip_inward() {
        let (canvas, _) = lay_one(&test_leaf(0.45, 0.0, true));
        // How far a texel's colour has gone from shed toward dry, by its
        // green over red: the shed colour's 0.833, the dry one's 0.667.
        let dried = |x: usize, y: usize| {
            let [r, g, _] = canvas.colour_at(x, y);
            (0.833 - g / r) / (0.833 - 0.667)
        };
        // The middle of the blade, its outermost texel and near its tip.
        let middle = dried(125, 128);
        let margin = dried(125, 128 + reach(&canvas, 125));
        let tip = dried(173, 128);
        assert!(middle < 0.1, "middle {middle:.2}");
        assert!(margin > 0.8, "margin {margin:.2}");
        assert!(tip > 0.8, "tip {tip:.2}");
        // Fully dry, the whole blade has its dry colour.
        let (canvas, _) = lay_one(&test_leaf(1.0, 0.0, true));
        let [r, g, _] = canvas.colour_at(125, 128);
        assert!((g / r - 0.667).abs() < 0.01, "{:.3}", g / r);
    }

    #[test]
    fn a_curled_leaf_narrows_rises_and_shows_its_underside() {
        let (flat, flat_cover) = lay_one(&test_leaf(1.0, 0.0, true));
        let (cupped, cupped_cover) = lay_one(&test_leaf(1.0, 1.4, true));
        let (rolled, _) = lay_one(&test_leaf(1.0, 2.8, true));
        let (arched, _) = lay_one(&test_leaf(1.0, 1.4, false));
        // Cupped, it covers less ground, standing higher at its edges than
        // along its midrib; rolled past upright, narrower and higher
        // still.
        assert!(cupped_cover < flat_cover, "{cupped_cover} {flat_cover}");
        assert!(reach(&cupped, 125) + 2 < reach(&flat, 125));
        assert!(reach(&rolled, 125) < reach(&cupped, 125));
        // Flat, it lies level at its 1 m.
        assert!((flat.height_span() - 1.0).abs() < 1.0e-9);
        let inside = |canvas: &Canvas| 128 + reach(canvas, 125) * 7 / 10;
        assert!(cupped.height_at(125, inside(&cupped)) > cupped.height_at(125, 128) + 0.002);
        assert!(rolled.height_span() > cupped.height_span() + 0.002);
        // Face down it arches, its midrib highest.
        assert!(arched.height_at(125, 128) > arched.height_at(125, inside(&arched)) + 0.002);
        // Face down its paler, greyer underside shows, and so does a
        // rolled edge's over the trough: green over red rises from the
        // dry colour's 0.667 toward grey's 1.
        let greyness = |canvas: &Canvas, x: usize, y: usize| {
            let [r, g, _] = canvas.colour_at(x, y);
            g / r
        };
        assert!((greyness(&flat, 125, 128) - 0.667).abs() < 0.01);
        assert!(greyness(&arched, 125, 128) > 0.7);
        let edge = 128 + reach(&rolled, 125) - 1;
        assert!(greyness(&rolled, 125, edge) > 0.7);
        assert!(greyness(&cupped, 125, 128) < 0.68);
    }
}
