//! Flowers laid out whorl by whorl (spec rework P5).
//!
//! One closed-form layout, [`layout`], places a flower's parts from the
//! outside in: its floral cup or inferior ovary, its perianth (sepals and
//! petals, two whorls of tepals, or a spiral of tepals), a fused corolla
//! tube, spurs, stamens and staminodes, and its pistil or an orchid's
//! column. The solid ([`draw`]), the reduced flower a cluster draws
//! ([`draw_reduced`]), the card ([`paint`]) and the flower's named parts
//! (`crate::organs::parts`) all come from it.
//!
//! Angles $`\varphi`$ run round the flower's axis $`\mathbf a`$ from the side
//! its [`Frame`] names: for a zygomorphic flower its adaxial side
//! $`\mathbf d`$, toward the axis that bears it, so a pea flower's banner
//! stands toward the tip of its raceme; for a regular one the organ's left,
//! the flower turned at random by up to a petal's angle as before. With
//! $`R`$ the flower's radius, $`c`$ = `centre` $`\cdot R`$ (at most $`0.9R`$),
//! $`s = 0.6c`$ the ring the perianth stands on and $`n`$ = `petals`, petal
//! $`k`$ stands at $`\varphi_k = \psi + 2\pi k/n`$ ($`\psi`$ = `turn`), is
//! $`L = \max(R - s, 0.1R)`$ long and
//! $`\mathrm{clamp}(\texttt{petal\_width}\cdot\pi(s+R)/n,\ 0.08L,\ L)`$ wide,
//! and rises the form's `cup`. Roles (`corolla`) change the shape by place;
//! a fused corolla is one surface of revolution from radius $`s`$ to
//! `throat` $`\cdot L`$ over `fused` $`\cdot L`$ (or `tube_reach`
//! $`\cdot R`$, a trumpet longer than its petals), its petals free lobes
//! on its mouth. Up to $`2n`$ stamens stand in whorls at $`0.5c`$ and
//! $`0.75c`$, the first alternating with the petals; more stand on a
//! spiral, stamen $`i`$ of $`m`$ at radius
//! $`c\,(0.35 + 0.6\sqrt{(i+\frac12)/m})`$ turned $`i \cdot 137.5°`$.
//!
//! A flower that uses none of P5's fields ([`Flower::is_plain`]) is drawn
//! by `crate::blooms` and `crate::clusters` exactly as before; this module
//! draws the rest.
//!
//! The layout keeps the short names of its formulas (`r`, `c`, `s`, `n`,
//! `w`) and the card its point (`x`, `y`) and parameter `u`.
#![allow(clippy::many_single_char_names)]

use std::f64::consts::{PI, TAU};

use crate::blooms::{self, FlowerForm, Part};
use crate::capitula::Painted;
use crate::leaves::{self, Section, SolidLeaf};
use crate::looks::{Corolla, Flower, Ovary, Perianth, Skin};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;

/// An anther's length over its stamen's, where the flower does not say.
pub const ANTHER: f64 = 0.16;

/// The golden angle, in degrees: a spiral perianth's and stamens' step.
const GOLDEN: f64 = 137.507_764;
/// The cup a closed flower's petals take, in degrees: over the centre.
const CLOSED: f64 = 95.0;
/// How far a stamen leans out from the axis: the tangent of its angle.
const LEAN: f64 = 0.35;
/// A torus's core's radius over its carpels' length (a magnolia's
/// gynoecium about 3.5 cm long and 2 cm thick).
const TORUS_CORE: f64 = 0.3;
/// The stamens' zone round a torus's foot over its carpels' length.
const TORUS_ZONE: f64 = 0.4;
/// A carpel's length out from a torus's core over the carpels' length.
const TORUS_CARPEL: f64 = 0.12;
/// How far a stamen on a torus leans out from the axis: the tangent of
/// its angle.
const TORUS_LEAN: f64 = 0.35;
/// The `open` below which a flower's stamens and pistil are hidden.
const SHOWN: f64 = 0.3;
/// How far a flower with no perianth reaches: its stamens' tips, over its
/// radius.
const NAKED_REACH: f64 = 0.9;

/// Whether any perianth part keeps its foot straight and curls from part
/// way along, so it needs more stations.
/// The way across a disymmetric corolla's outer petal part heading
/// `heading` from the foot at `radial`: square to its heading in the plane
/// of the heading and the axis, the heart's plane.
fn in_heart_plane(heading: Vec3, radial: Vec3) -> Vec3 {
    let tangent = heading.cross(radial);
    heading.cross(tangent).normalize_or(radial)
}

fn form_bends(laid: &Floral) -> bool {
    laid.pieces
        .iter()
        .any(|piece| piece.curl_from > 0.0 && piece.curl != 0.0)
}

/// What a perianth part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    Sepal,
    Petal,
    Tepal,
}

impl Term {
    /// Its organ in the vocabulary.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Sepal => "sepal",
            Self::Petal => "petal",
            Self::Tepal => "tepal",
        }
    }
}

/// One perianth part: a thin solid leaf standing on the receptacle, a
/// floral cup's rim or a tube's mouth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub term: Term,
    /// Its role by place, where the corolla gives one: "banner", "lip".
    pub role: Option<&'static str>,
    /// Its angle round the axis from the frame's side, in radians.
    pub angle: f64,
    /// Its foot's distance from the axis and height along it, in metres.
    pub radius: f64,
    pub height: f64,
    /// Degrees it rises from the plane square to the axis.
    pub cup: f64,
    pub length: f64,
    pub width: f64,
    /// Degrees it curls back over its length.
    pub curl: f64,
    /// Share of its length from its foot kept straight before the curl.
    pub curl_from: f64,
    /// A keel's fold (`SolidLeaf::fold`).
    pub fold: f64,
    /// Its width at its foot over its widest: narrow for a free petal,
    /// wide for a lobe on a tube.
    pub base: f64,
    /// The power of its taper to the tip.
    pub taper: f64,
    /// How far its colour is from the petals' toward the calyx's, 0 to 1.
    pub calyx: f32,
}

/// A surface of revolution: a corolla tube, or a calyx's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tube {
    /// Its foot's height along the axis.
    pub height: f64,
    pub length: f64,
    /// Its radius at its foot and at its mouth.
    pub foot: f64,
    pub mouth: f64,
    /// How much it swells in its middle.
    pub belly: f64,
    /// The lobes on its mouth: its vertices round are a multiple.
    pub lobes: u32,
    /// The angle of its first lobe.
    pub turn: f64,
    /// As [`Piece::calyx`].
    pub calyx: f32,
}

impl Tube {
    /// Its radius `u` of the way from its foot to its mouth.
    #[must_use]
    pub fn radius_at(&self, u: f64) -> f64 {
        self.foot + (self.mouth - self.foot) * u + self.belly * 4.0 * u * (1.0 - u)
    }
}

/// Something slender standing on the flower: a stamen, a style, a free
/// pistil, a spur or a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rod {
    /// Its foot: angle round the axis, distance from it, height along it.
    pub angle: f64,
    pub radius: f64,
    pub height: f64,
    /// Radians it leans from the axis toward its angle; past a right
    /// angle, back below the flower (a spur).
    pub lean: f64,
    pub length: f64,
    /// Its radius at its foot.
    pub width: f64,
}

/// A stamen, or a staminode: a sterile stamen, drawn without an anther.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamen {
    pub rod: Rod,
    pub fertile: bool,
    /// Its anther's length, centred on the filament's tip.
    pub anther: f64,
    /// Radians its anther turns from the filament's line toward the
    /// flower's circumference: 0 along it (basifixed), a right angle
    /// across it (versatile).
    pub tilt: f64,
}

/// A flower's centre.
#[derive(Debug, Clone, PartialEq)]
pub enum Pistil {
    /// Hidden: in a closed bud, or in a pea flower's keel.
    None,
    /// The centre dome of before P5, at `height`.
    Dome { height: f64, radius: f64, rise: f64 },
    /// One pistil: an ovary (where it is superior: its centre's height and
    /// radius), a style and as many stigma lobes as carpels.
    One {
        ovary: Option<(f64, f64)>,
        style: Rod,
        lobes: u32,
    },
    /// Free pistils packed on a low dome.
    Free {
        dome: (f64, f64, f64),
        pistils: Vec<Rod>,
    },
    /// An orchid's column: stamens and pistil in one body.
    Column(Rod),
}

/// A flower laid out (see the module), in its frame, in metres.
#[derive(Debug, Clone, PartialEq)]
pub struct Floral {
    pub radius: f64,
    /// The centre's radius $`c`$.
    pub centre: f64,
    /// A floral cup under the perianth: its length and radii at its foot
    /// and rim.
    pub cup: Option<(f64, f64, f64)>,
    /// An ovary below the perianth: its centre's height, half-length and
    /// half-width.
    pub ovary: Option<(f64, f64, f64)>,
    /// A floral tube above an inferior ovary, in the calyx's colour, that
    /// bears the perianth on its rim.
    pub floral_tube: Option<Tube>,
    pub calyx_tube: Option<Tube>,
    /// Its perianth parts: sepals from the first round, then petals or
    /// tepals (an outer whorl before an inner one; a spiral from the
    /// outside in).
    pub pieces: Vec<Piece>,
    pub tube: Option<Tube>,
    pub spurs: Vec<Rod>,
    /// Its stamens, then its staminodes.
    pub stamens: Vec<Stamen>,
    pub pistil: Pistil,
    /// The form's cup, which the card's petals are measured against.
    pub form_cup: f64,
    /// The petals' notch, for the card.
    pub notch: f64,
    /// Dark spots on each petal's or tepal's inner face, toward its foot.
    pub spots: u32,
    /// Share of each petal's or tepal's length a stripe down the middle of
    /// its inner face reaches from its foot; 0 none.
    pub stripe: f64,
    /// What an inferior ovary's skin bears: a cactus's pericarpel.
    pub pericarpel: Skin,
}

/// The whorls a flower's tepals stand in: as many of its `whorl` as its
/// tepals fill, else two.
fn tepal_whorls(flower: &Flower) -> u32 {
    if flower.whorl > 0 && flower.petals % flower.whorl == 0 {
        (flower.petals / flower.whorl).clamp(2, 6)
    } else {
        2
    }
}

/// The flower `flower` drawn with `form`, `radius` metres in radius, a
/// regular one turned `spin` radians.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn layout(flower: &Flower, form: &FlowerForm, radius: f64, spin: f64) -> Floral {
    // The sepals rise half the form's cup; the petals their stated rise,
    // where the flower states one, in place of the form's.
    let sepal_cup = flower.sepal_rise.unwrap_or(form.cup * 0.5);
    let ruled;
    let form = match flower.petal_rise {
        Some(cup) => {
            ruled = FlowerForm {
                cup,
                ..form.clone()
            };
            &ruled
        }
        None => form,
    };
    let r = radius.max(0.0);
    let c = (flower.centre * r).clamp(0.0, r * 0.9);
    let s = 0.6 * c;
    let n = flower.petals.max(1);
    let nf = f64::from(n);
    let length = (r - s).max(r * 0.1);
    let spread = |count: f64, from: f64, to: f64| flower.petal_share() * PI * (from + to) / count;
    // A petal whose outline is stated is as wide for its length as stated,
    // as its card draws it; else its share of the circle.
    let width = flower
        .petal_ratio
        .map_or_else(|| spread(nf, s, r), |ratio| ratio * length)
        .clamp(length * 0.08, length);
    let open = flower.open.clamp(0.0, 1.0);
    // Closing raises every part toward the axis and shortens the petals.
    let rise = |cup: f64| cup + (CLOSED - cup) * (1.0 - open);
    let grown = 0.5 + 0.5 * open;
    let curl = form.curl * open;
    let taper = flower
        .taper
        .unwrap_or(if flower.point >= 0.5 { 1.3 } else { 0.45 });
    let psi = math::radians(flower.turn) + if flower.is_zygomorphic() { 0.0 } else { spin };

    // The seat: where the perianth stands.
    let cup_length = flower.tube * r;
    let ovary_length = if cup_length > 0.0 {
        cup_length
    } else {
        (1.2 * c).max(0.3 * r)
    };
    let floral_cup = (cup_length > 0.0).then_some((cup_length, 0.6 * s, s));
    // A flower with no carpels (a staminate or a sterile flower) has no
    // ovary to stand on, whatever its taxa's ovary position.
    let position = if flower.carpels == 0 {
        Ovary::Superior
    } else {
        flower.ovary
    };
    let (seat, cup, ovary) = match position {
        Ovary::Superior => (cup_length, floral_cup, None),
        Ovary::Half => {
            let seat = cup_length.max(ovary_length * 0.5);
            (
                seat,
                floral_cup,
                Some((seat, ovary_length * 0.5, s.max(0.15 * ovary_length))),
            )
        }
        Ovary::Inferior => (
            ovary_length,
            None,
            Some((
                ovary_length * 0.5,
                ovary_length * 0.5,
                // A pericarpel the tepals open straight from, with no
                // floral tube between (a prickly pear's or a cholla's), is
                // a globose to top-shaped joint of stem, not a stalk.
                if flower.pericarpel != Skin::Naked && flower.floral_tube <= 0.0 {
                    s.max(0.4 * ovary_length)
                } else {
                    s.max(0.12 * ovary_length)
                },
            )),
        ),
    };
    // A floral tube on the inferior ovary, widening a little to the rim
    // the perianth stands on.
    let floral_tube = (position == Ovary::Inferior && flower.floral_tube > 0.0).then(|| {
        let neck = s.max(0.12 * ovary_length) * 0.7;
        Tube {
            height: seat,
            length: flower.floral_tube * r,
            foot: neck,
            mouth: (s * 1.05).max(neck * 1.3),
            belly: 0.0,
            lobes: flower.sepals.max(n),
            turn: psi,
            calyx: 1.0,
        }
    });
    let seat = floral_tube.map_or(seat, |t| t.height + t.length);

    // Sepals, between the petals, unless they fell as it opened.
    let mut pieces = Vec::new();
    let mut calyx_tube = None;
    if flower.perianth == Perianth::Corolla
        && flower.sepals > 0
        && open <= flower.calyx_shed
        && flower.corolla != Corolla::Labellate
    {
        let m = f64::from(flower.sepals);
        let sepal = flower.sepal_length * length;
        let fused = flower.sepals_fused;
        let foot = (s * 1.05).max(r * 0.04);
        let (height, at) = if fused > 0.0 {
            let tube = Tube {
                height: seat,
                length: fused * sepal,
                foot,
                mouth: foot + fused * sepal * 0.35,
                belly: 0.0,
                lobes: flower.sepals,
                turn: psi + PI / m,
                calyx: 1.0,
            };
            calyx_tube = Some(tube);
            (seat + tube.length, tube.mouth)
        } else {
            (seat, foot)
        };
        let tooth = (1.0 - fused) * sepal;
        for j in 0..flower.sepals {
            pieces.push(Piece {
                term: Term::Sepal,
                role: None,
                angle: psi + TAU * f64::from(j) / m + PI / m,
                radius: at,
                height,
                cup: rise(sepal_cup),
                length: tooth,
                width: (spread(m, s, r) * 0.8).clamp(tooth * 0.08, tooth.max(1e-9)),
                curl: curl * 0.5,
                curl_from: form.curl_from,
                fold: 0.0,
                base: if fused > 0.0 { 0.9 } else { 0.35 },
                taper: 1.3,
                calyx: 1.0,
            });
        }
    }

    // The corolla tube, and where the petals stand on it.
    let whorled = matches!(flower.perianth, Perianth::Corolla | Perianth::Tepals);
    let tube = (whorled && flower.fused > 0.0).then(|| Tube {
        height: seat,
        length: if flower.tube_reach > 0.0 {
            flower.tube_reach * r
        } else {
            flower.fused * length
        },
        foot: s.max(r * 0.04),
        mouth: flower.throat * length,
        belly: flower.belly * length,
        lobes: n,
        turn: psi,
        calyx: 0.0,
    });
    let (lobe_height, lobe_radius, lobe_length, lobe_width, lobe_base) = match &tube {
        Some(t) => {
            let lobe = (1.0 - flower.fused) * length;
            (
                t.height + t.length,
                t.mouth,
                lobe,
                spread(nf, t.mouth, t.mouth + lobe).clamp(lobe * 0.08, lobe),
                0.9,
            )
        }
        None => (seat, s, length, width, 0.35),
    };
    let piece =
        |term: Term, role: Option<&'static str>, angle: f64, cup: f64, (long, wide): (f64, f64)| {
            Piece {
                term,
                role,
                angle: psi + angle,
                radius: lobe_radius,
                height: lobe_height,
                cup: rise(cup),
                length: lobe_length * long * grown,
                width: lobe_width * wide,
                curl,
                curl_from: form.curl_from,
                fold: 0.0,
                base: lobe_base,
                taper,
                calyx: 0.0,
            }
        };
    let degrees = math::radians;
    match (flower.perianth, flower.corolla) {
        (Perianth::Corolla, Corolla::Papilionaceous) => {
            let banner = Piece {
                curl: curl.max(0.0) + 15.0,
                ..piece(Term::Petal, Some("banner"), 0.0, 10.0, (1.0, flower.banner))
            };
            let wing = |angle| piece(Term::Petal, Some("wing"), angle, 80.0, (0.85, 0.6));
            let keel = |angle| Piece {
                fold: 0.6,
                ..piece(Term::Petal, Some("keel"), angle, 85.0, (0.8, 0.6))
            };
            pieces.extend([
                banner,
                wing(degrees(72.0)),
                keel(PI - degrees(12.0)),
                keel(PI + degrees(12.0)),
                wing(degrees(288.0)),
            ]);
        }
        (Perianth::Corolla, Corolla::Bilabiate) => {
            for k in 0..n {
                let angle = PI / nf + TAU * f64::from(k) / nf;
                let upper = math::cos(angle) > 1e-9;
                let middle = (angle - PI).abs() < 1e-9;
                pieces.push(piece(
                    Term::Petal,
                    Some(if upper { "upper lip" } else { "lower lip" }),
                    angle,
                    if upper {
                        form.cup + 15.0
                    } else {
                        form.cup - 15.0
                    },
                    (1.0, if middle { flower.lip } else { 1.0 }),
                ));
            }
        }
        (Perianth::Corolla, Corolla::Labellate) => {
            // An orchid's sepals are coloured like its petals, and stand
            // in the same ring.
            for (angle, role) in [
                (60.0, "lateral sepal"),
                (180.0, "dorsal sepal"),
                (300.0, "lateral sepal"),
            ] {
                pieces.push(piece(
                    Term::Sepal,
                    Some(role),
                    degrees(angle),
                    form.cup,
                    (flower.sepal_length, 0.4),
                ));
            }
            let lip = piece(
                Term::Petal,
                Some("lip"),
                0.0,
                form.cup - 20.0,
                (flower.lip, 0.6 * flower.lip),
            );
            pieces.push(Piece {
                width: lip.width.min(lip.length.max(1e-9)),
                ..lip
            });
            for angle in [120.0, 240.0] {
                pieces.push(piece(
                    Term::Petal,
                    Some("lateral petal"),
                    degrees(angle),
                    form.cup + 10.0,
                    (0.8, 0.4),
                ));
            }
        }
        (Perianth::Corolla, Corolla::Unequal) => {
            for k in 0..n {
                let upper = k == 0;
                pieces.push(piece(
                    Term::Petal,
                    Some(if upper { "upper petal" } else { "petal" }),
                    TAU * f64::from(k) / nf,
                    if upper {
                        form.cup + flower.odd_rise
                    } else {
                        form.cup
                    },
                    (1.0, if upper { flower.banner } else { 1.0 }),
                ));
            }
        }
        (Perianth::Corolla, Corolla::Disymmetric) => {
            // A bleeding heart's: each outer petal pouched at its foot,
            // bowing out from the receptacle (its pouch), then running back
            // in (its blade) to meet the other in the heart's point, where
            // its tip turns out (FNA: reflexed portion 2-5 mm of 16-19); the
            // two narrow inner petals hang straight between them and stand
            // out beyond the point. Each outer petal is drawn as these
            // three parts end to end.
            let foot = 0.1 * r;
            let g = grown;
            // The heart's plane square to the bearing axis, so a raceme
            // seen from the side shows its hearts face on.
            for angle in [PI / 2.0, 1.5 * PI] {
                let part = |role, (radius, height): (f64, f64), cup: f64, length: f64, wide: f64| {
                    Piece {
                        radius,
                        height,
                        length: length * g,
                        width: wide * r,
                        curl: 0.0,
                        base: 0.7,
                        taper: 0.6,
                        ..piece(Term::Petal, Some(role), angle, cup, (1.0, 1.0))
                    }
                };
                let end = |p: &Piece| {
                    let cup = degrees(p.cup);
                    (
                        p.radius + p.length * math::cos(cup),
                        p.height + p.length * math::sin(cup),
                    )
                };
                // The pouch runs out nearly square to the axis, a lobe of
                // the heart either side of the pedicel; the blade runs back
                // in to the point, the heart about half as wide as long.
                let pouch = part("pouch", (foot, lobe_height), -10.0, 0.5 * r, 0.7);
                let blade = part("blade", end(&pouch), 108.0, 1.35 * r, 0.6);
                let tip = part("tip", end(&blade), 20.0, 0.25 * r, 0.2);
                pieces.extend([pouch, blade, tip]);
            }
            for angle in [0.0, PI] {
                let inner = piece(Term::Petal, Some("inner petal"), angle, 86.0, (1.0, 1.0));
                pieces.push(Piece {
                    radius: 0.5 * foot,
                    length: 1.65 * r * g,
                    width: 0.22 * r,
                    curl: 0.0,
                    base: 0.5,
                    taper: 1.3,
                    ..inner
                });
            }
        }
        (Perianth::Corolla, Corolla::Regular) if flower.petal_whorls == 2 => {
            // Two like whorls, as tepals stand, the inner turned half a
            // petal and a little narrower and more upright. Two crossed
            // pairs (a poppy's 2 + 2) open together as one cup, the outer
            // pair as upright as the inner it wrapped in bud.
            let h = f64::from((n / 2).max(1));
            let outer = if n / 2 <= 2 { form.cup } else { form.cup * 0.85 };
            for k in 0..(n / 2).max(1) {
                let angle = TAU * f64::from(k) / h;
                pieces.push(piece(Term::Petal, None, angle, outer, (1.0, 1.0)));
            }
            for k in 0..(n / 2).max(1) {
                let angle = TAU * f64::from(k) / h + PI / h;
                pieces.push(piece(Term::Petal, None, angle, form.cup, (1.0, 0.85)));
            }
        }
        (Perianth::Corolla, _) => {
            for k in 0..n {
                pieces.push(piece(
                    Term::Petal,
                    None,
                    TAU * f64::from(k) / nf,
                    form.cup,
                    (1.0, 1.0),
                ));
            }
        }
        // Zygomorphic tepals (a gladiolus's): the two whorls stand
        // as before, the outer one's first tepal toward the axis, and take
        // their roles by place as a corolla's petals do.
        (Perianth::Tepals, Corolla::Bilabiate | Corolla::Unequal) => {
            let h = f64::from((n / 2).max(1));
            for inner in [false, true] {
                for k in 0..(n / 2).max(1) {
                    let angle = TAU * f64::from(k) / h + if inner { PI / h } else { 0.0 };
                    let (cup, wide) = if inner {
                        (form.cup, 0.85)
                    } else {
                        (form.cup * 0.85, 1.0)
                    };
                    pieces.push(if flower.corolla == Corolla::Bilabiate {
                        let upper = math::cos(angle) > 1e-9;
                        let middle = (angle - PI).abs() < 1e-9;
                        piece(
                            Term::Tepal,
                            Some(if upper { "upper lip" } else { "lower lip" }),
                            angle,
                            if upper { cup + 15.0 } else { cup - 15.0 },
                            (1.0, wide * if middle { flower.lip } else { 1.0 }),
                        )
                    } else {
                        let upper = !inner && k == 0;
                        piece(
                            Term::Tepal,
                            Some(if upper { "upper tepal" } else { "tepal" }),
                            angle,
                            if upper { cup + flower.odd_rise } else { cup },
                            (1.0, wide * if upper { flower.banner } else { 1.0 }),
                        )
                    });
                }
            }
        }
        (Perianth::Tepals, _) => {
            // Whorls of alike tepals, each turned half a tepal from the
            // one outside it, a little narrower and more upright: two, as
            // a lily's stand, or as many as the tepals over the merosity
            // (a magnolia's three or four whorls of three), the inner ones
            // a little shorter too. Free tepals raised into a bell widen
            // to overlap.
            let whorls = tepal_whorls(flower);
            let per = (n / whorls).max(1);
            let h = f64::from(per);
            let wide = bell_width(
                form.cup,
                tube.is_some(),
                nf,
                (lobe_radius, lobe_length, lobe_width),
            );
            for j in 0..whorls {
                let t = f64::from(j) / f64::from(whorls - 1);
                let long = if whorls > 2 { 1.0 - 0.12 * t } else { 1.0 };
                for k in 0..per {
                    let angle = TAU * f64::from(k) / h + PI / h * f64::from(j);
                    let cup = form.cup * (0.85 + 0.15 * t);
                    pieces.push(piece(
                        Term::Tepal,
                        None,
                        angle,
                        cup,
                        (long, (1.0 - 0.15 * t) * wide),
                    ));
                }
            }
        }
        (Perianth::Spiral, _) => {
            let wide = spread(nf.min(13.0), s, r).clamp(length * 0.08, length);
            for i in 0..n {
                let t = f64::from(i) / f64::from((n - 1).max(1));
                let tepal_length = length * (0.55 + 0.45 * t) * grown;
                pieces.push(Piece {
                    term: Term::Tepal,
                    role: None,
                    angle: psi + degrees(GOLDEN) * f64::from(i),
                    radius: s * (1.0 - 0.25 * t),
                    height: seat,
                    cup: rise((form.cup - 30.0 + 60.0 * t).clamp(-20.0, 90.0)),
                    length: tepal_length,
                    width: (wide * (0.7 + 0.3 * t)).min(tepal_length.max(1e-9)),
                    curl,
                    curl_from: form.curl_from,
                    fold: 0.0,
                    base: 0.35,
                    taper,
                    #[allow(clippy::cast_possible_truncation)]
                    calyx: (1.0 - 2.0 * t).max(0.0) as f32,
                });
            }
        }
        (Perianth::Absent, _) => {}
    }

    // Spurs: every petal's of a regular corolla, a lower lip's middle
    // petal's, a lip's, an unequal corolla's upper petal's.
    let mut spurs = Vec::new();
    if flower.spur > 0.0 {
        let spurred = |p: &Piece| match flower.corolla {
            Corolla::Regular => p.term != Term::Sepal,
            Corolla::Bilabiate => p.role == Some("lower lip") && (p.angle - psi - PI).abs() < 1e-9,
            Corolla::Labellate => p.role == Some("lip"),
            Corolla::Papilionaceous | Corolla::Disymmetric => false,
            Corolla::Unequal => matches!(p.role, Some("upper petal" | "upper tepal")),
        };
        let spur_length = flower.spur * r;
        spurs.extend(pieces.iter().filter(|p| spurred(p)).map(|p| Rod {
            angle: p.angle,
            radius: p.radius,
            height: p.height,
            lean: degrees(180.0 - 20.0),
            length: spur_length,
            width: (p.width * 0.2).max(r * 0.03).min(spur_length * 0.3),
        }));
    }

    // An elongate receptacle: its carpels' length, its core's radius and
    // the stamens' zone round its foot.
    let torus = (flower.torus > 0.0
        && flower.carpels > 5
        && flower.ovary == Ovary::Superior
        && tube.is_none())
    .then(|| {
        let length = flower.torus * r;
        (length, TORUS_CORE * length, TORUS_ZONE * length)
    });

    // Stamens and staminodes.
    let shown = open >= SHOWN;
    let hidden = matches!(flower.corolla, Corolla::Papilionaceous | Corolla::Labellate);
    let mut stamens = Vec::new();
    if shown && !hidden {
        let total = flower.stamens + flower.staminodes;
        let whorl = match flower.perianth {
            Perianth::Corolla => (n / flower.petal_whorls.max(1)).max(1),
            Perianth::Tepals => (n / tepal_whorls(flower)).max(1),
            Perianth::Spiral | Perianth::Absent => n.min(6),
        };
        let wf = f64::from(whorl);
        let filament = flower.stamen_length * r;
        for i in 0..total {
            let fi = f64::from(i);
            let (angle, at) = if total <= 2 * whorl {
                if i < whorl {
                    (PI / wf + TAU * fi / wf, 0.5 * c)
                } else {
                    (TAU * f64::from(i - whorl) / wf, 0.75 * c)
                }
            } else {
                (
                    degrees(GOLDEN) * fi,
                    c * (0.35 + 0.6 * math::sqrt((fi + 0.5) / f64::from(total))),
                )
            };
            let fertile = i < flower.stamens;
            let length = if fertile { filament } else { filament * 0.8 };
            let (height, at, lean) = match (&tube, torus) {
                // Adnate to the tube, halfway up.
                (Some(t), _) => (
                    t.height + t.length * 0.5,
                    t.radius_at(0.5) * 0.85,
                    flower.stamen_lean.unwrap_or(0.15),
                ),
                // Crowded on a spiral up the torus's foot, the lowest
                // first, spreading.
                (None, Some((_, core, zone))) => (
                    seat + zone * (fi + 0.5) / f64::from(total),
                    core,
                    flower.stamen_lean.unwrap_or(TORUS_LEAN),
                ),
                (None, None) => (seat, at, flower.stamen_lean.unwrap_or(LEAN)),
            };
            stamens.push(Stamen {
                rod: Rod {
                    angle: psi + angle,
                    radius: at,
                    height,
                    lean: math::atan(lean),
                    length,
                    width: length * flower.stamen_width,
                },
                fertile,
                anther: flower.anther * filament,
                tilt: math::radians(flower.anther_tilt),
            });
        }
    }

    // The pistil, or a column.
    let pistil = if flower.corolla == Corolla::Labellate {
        let length = flower.column * r;
        Pistil::Column(Rod {
            angle: psi + PI,
            radius: 0.0,
            height: seat,
            lean: degrees(25.0),
            length,
            width: length * 0.12,
        })
    } else if hidden || !shown {
        Pistil::None
    } else if flower.carpels == 0 {
        if c > 0.0 {
            Pistil::Dome {
                height: seat,
                radius: c,
                rise: c * 0.5,
            }
        } else {
            Pistil::None
        }
    } else if flower.carpels <= 5 || flower.ovary != Ovary::Superior {
        // An inferior ovary is one, however many carpels it holds: a
        // cactus's style ends in a ring of stigma lobes.
        let size = (0.3 * c).max(0.04 * r);
        let superior = flower.ovary == Ovary::Superior;
        let style = flower.style_length.unwrap_or(flower.stamen_length) * r;
        Pistil::One {
            ovary: superior.then_some((seat + size, size)),
            style: Rod {
                angle: psi,
                radius: 0.0,
                height: if superior { seat + 2.0 * size } else { seat },
                lean: 0.0,
                length: style,
                width: style * 0.04,
            },
            lobes: flower.carpels,
        }
    } else if let Some((length, core, zone)) = torus {
        // The carpels on a spiral up a half ellipsoid over the stamens,
        // each standing out square to its face.
        let k = f64::from(flower.carpels);
        let foot = seat + zone * 0.9;
        let carpel = TORUS_CARPEL * length;
        let pistils = (0..flower.carpels)
            .map(|i| {
                let u = 0.92 * (f64::from(i) + 0.5) / k;
                let across = core * math::sqrt((1.0 - u * u).max(0.0));
                let normal = (across / (core * core), u * length / (length * length));
                Rod {
                    angle: psi + degrees(GOLDEN) * f64::from(i),
                    radius: across,
                    height: foot + length * u,
                    lean: math::atan2(normal.0, normal.1),
                    length: carpel,
                    width: carpel * 0.5,
                }
            })
            .collect();
        Pistil::Free {
            dome: (foot, core, length),
            pistils,
        }
    } else {
        let k = f64::from(flower.carpels);
        let rise = c * 0.3;
        let pistils = (0..flower.carpels)
            .map(|i| {
                let fi = f64::from(i);
                let at = c * 0.75 * math::sqrt((fi + 0.5) / k);
                let up = if c > 0.0 {
                    rise * math::sqrt((1.0 - (at / c).powi(2)).max(0.0))
                } else {
                    0.0
                };
                let length = (0.6 * c).max(0.08 * r);
                Rod {
                    angle: psi + degrees(GOLDEN) * fi,
                    radius: at,
                    height: seat + up,
                    lean: math::atan(0.25),
                    length,
                    width: length * 0.08,
                }
            })
            .collect();
        Pistil::Free {
            dome: (seat, c, rise),
            pistils,
        }
    };

    // A flower with no perianth is only its stamens and pistil, so its
    // stated size is theirs: they are grown to reach its radius (a
    // chloranthus's connectives, a willow's catkin flower), as a
    // perianth's flower is as wide as its petals reach.
    let mut stamens = stamens;
    let mut pistil = pistil;
    let mut c = c;
    if pieces.is_empty() && tube.is_none() && calyx_tube.is_none() && cup.is_none() {
        let reach = stamens
            .iter()
            .map(|stamen| {
                stamen.rod.radius
                    + (stamen.rod.length + stamen.anther * 0.5) * math::sin(stamen.rod.lean)
            })
            .fold(c, f64::max);
        if reach > 0.0 && reach < NAKED_REACH * r {
            let k = NAKED_REACH * r / reach;
            let grow = |rod: &mut Rod| {
                rod.radius *= k;
                rod.height *= k;
                rod.length *= k;
                rod.width *= k;
            };
            for stamen in &mut stamens {
                grow(&mut stamen.rod);
                stamen.anther *= k;
            }
            match &mut pistil {
                Pistil::None => {}
                Pistil::Dome {
                    height,
                    radius,
                    rise,
                } => {
                    *height *= k;
                    *radius *= k;
                    *rise *= k;
                }
                Pistil::One { ovary, style, .. } => {
                    if let Some((height, size)) = ovary {
                        *height *= k;
                        *size *= k;
                    }
                    grow(style);
                }
                Pistil::Free { dome, pistils } => {
                    dome.0 *= k;
                    dome.1 *= k;
                    dome.2 *= k;
                    pistils.iter_mut().for_each(grow);
                }
                Pistil::Column(rod) => grow(rod),
            }
            c = (c * k).min(r * 0.9);
        }
    }

    Floral {
        radius: r,
        centre: c,
        cup,
        ovary,
        floral_tube,
        calyx_tube,
        pieces,
        tube,
        spurs,
        stamens,
        pistil,
        form_cup: form.cup,
        notch: flower.notch,
        spots: flower.spots,
        // Not on a closed bud's hidden faces.
        stripe: if open >= SHOWN {
            flower.stripe_length
        } else {
            0.0
        },
        pericarpel: flower.pericarpel,
    }
}

/// Where a flower is drawn: its foot, its axis and the side its angles
/// run from, square to the axis; all unit vectors.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub foot: Vec3,
    pub axis: Vec3,
    pub side: Vec3,
}

impl Frame {
    /// A flower's frame where it faces `axis` from `foot`: a zygomorphic
    /// one's side is its adaxial side, toward the axis heading `bearing`
    /// (the part of it square to `axis`), or `fallback` where the two are
    /// nearly parallel; a regular one's is `fallback`.
    #[must_use]
    pub fn new(foot: Vec3, axis: Vec3, bearing: Vec3, fallback: Vec3, zygomorphic: bool) -> Self {
        let axis = axis.normalize_or(Vec3::Y);
        let square = |v: Vec3| v - axis * v.dot(axis);
        let fallback = square(fallback).normalize_or(any_perpendicular(axis));
        let side = if zygomorphic {
            let toward = square(bearing);
            if toward.length() > 1e-3 * bearing.length().max(1e-12) {
                toward.normalize_or(fallback)
            } else {
                fallback
            }
        } else {
            fallback
        };
        Self { foot, axis, side }
    }

    fn other(&self) -> Vec3 {
        self.axis.cross(self.side)
    }

    fn radial(&self, angle: f64) -> Vec3 {
        self.side * math::cos(angle) + self.other() * math::sin(angle)
    }

    fn point(&self, angle: f64, radius: f64, height: f64) -> Vec3 {
        self.foot + self.radial(angle) * radius + self.axis * height
    }

    /// The way something at `angle` points rising `degrees` from the plane
    /// square to the axis.
    fn rising(&self, angle: f64, degrees: f64) -> Vec3 {
        let radial = self.radial(angle);
        let cup = math::radians(degrees);
        (radial * math::cos(cup) + self.axis * math::sin(cup)).normalize_or(radial)
    }

    fn rod(&self, rod: &Rod) -> (Vec3, Vec3) {
        let radial = self.radial(rod.angle);
        let direction = (self.axis * math::cos(rod.lean) + radial * math::sin(rod.lean))
            .normalize_or(self.axis);
        (self.point(rod.angle, rod.radius, rod.height), direction)
    }
}

/// A flower's colours as drawn: its petals', its calyx's and its accent
/// (stamens and pistil).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub petal: [f32; 3],
    pub calyx: [f32; 3],
    pub accent: [f32; 3],
    /// The filaments' and the style's.
    pub filament: [f32; 3],
    /// The anthers': the flower's `anther_colour`, else the accent.
    pub anther: [f32; 3],
    /// A single pistil's ovary.
    pub ovary: [f32; 3],
    /// Each petal's foot on its inner face, fading into the petal's
    /// colour at `base_length` of its length; none if absent.
    pub base: Option<[f32; 3]>,
    pub base_length: f64,
    /// The nectaries at each petal's very foot; none if absent.
    pub star: Option<[f32; 3]>,
    /// The stripe down the middle of each petal's inner face; the petal's
    /// own if absent.
    pub stripe: Option<[f32; 3]>,
    /// Each petal's outer face; the inner face's, a shade darker, if
    /// absent.
    pub back: Option<[f32; 3]>,
    /// Each petal's outer face at its foot, fading into the outer face's
    /// colour at `back_base_length` of its length; none if absent.
    pub back_base: Option<[f32; 3]>,
    pub back_base_length: f64,
    /// The corolla tube's, where it differs from the petals'.
    pub tube: Option<[f32; 3]>,
    /// Its stigma lobes.
    pub stigma: [f32; 3],
    /// An inferior ovary's skin: the calyx's unless the flower states a
    /// pericarpel colour.
    pub pericarpel: [f32; 3],
    /// A cactus pericarpel's areoles' felt and spines or glochids.
    pub dress: crate::fruit::Dress,
}

impl Colours {
    /// The colour `share` of the way from the petals' to the calyx's.
    fn toward_calyx(&self, share: f32) -> [f32; 3] {
        std::array::from_fn(|c| self.petal[c] + (self.calyx[c] - self.petal[c]) * share)
    }

    /// A tube's colour: a corolla tube's own where it has one.
    fn of_tube(&self, tube: &Tube) -> [f32; 3] {
        match self.tube {
            Some(colour) if tube.calyx == 0.0 => colour,
            _ => self.toward_calyx(tube.calyx),
        }
    }
}

/// The calyx's colour where neither the flower nor the plant sets one: a
/// leaf green.
pub const CALYX: [f32; 3] = crate::capitula::BRACTS;

/// Draw `laid` in `frame` as a solid: on the nearest step solid leaves of
/// six stations, tubes four vertices round per lobe, filaments with
/// anthers and a pistil with stigma lobes; on the next, four stations,
/// two vertices per lobe, bare filaments; from the step after,
/// [`draw_reduced`].
#[allow(clippy::too_many_lines)]
pub(crate) fn draw(laid: &Floral, frame: Frame, colours: Colours, part: Part, mesh: &mut Mesh) {
    if laid.radius <= 0.0 {
        return;
    }
    if part.coarse >= 2 {
        draw_reduced(
            laid,
            frame,
            colours,
            Part {
                coarse: part.coarse - 2,
                ..part
            },
            mesh,
        );
        return;
    }
    let fine = part.coarse == 0;
    let (axis, side) = (frame.axis, frame.side);
    if let Some((length, r0, r1)) = laid.cup {
        blooms::frustum(
            frame.foot,
            axis,
            side,
            length,
            r0,
            r1,
            colours.calyx,
            part,
            mesh,
        );
    }
    if let Some((height, a, b)) = laid.ovary {
        blooms::ellipsoid(
            frame.foot + axis * height,
            axis,
            side,
            a,
            b,
            colours.pericarpel,
            part,
            mesh,
        );
        // A cactus's pericarpel, scaled, felted or spiny as its fruit
        // will be.
        crate::fruit::skin(
            &crate::fruit::Surface {
                centre: frame.foot + axis * height,
                axis,
                across: side,
                long: a,
                wide: b,
            },
            laid.pericarpel,
            if fine { (21, 3) } else { (8, 0) },
            colours.pericarpel,
            colours.dress,
            part,
            mesh,
        );
    }
    for tube in [&laid.floral_tube, &laid.calyx_tube, &laid.tube]
        .into_iter()
        .flatten()
    {
        let round = if fine { 4 } else { 2 };
        revolve(
            tube,
            frame,
            (tube.lobes * round).clamp(8, 96),
            if fine { 6 } else { 3 },
            colours.of_tube(tube),
            part,
            mesh,
        );
    }
    // A petal bent back from part way along needs stations past the bend.
    let (most_stations, most_across) = match (fine, form_bends(laid)) {
        (true, true) => (10, 1),
        (true, false) => (6, 2),
        (false, true) => (6, 1),
        (false, false) => (4, 1),
    };
    for piece in &laid.pieces {
        if !(piece.length > 0.0 && piece.width > 0.0) {
            continue;
        }
        let radial = frame.radial(piece.angle);
        let heading = frame.rising(piece.angle, piece.cup);
        // The face toward the flower's axis, so the curl bends it out.
        let mut left = axis.cross(radial).normalize_or(side);
        if heading.cross(left).dot(axis) < 0.0 {
            left = -left;
        }
        if matches!(piece.role, Some("pouch" | "blade" | "tip")) {
            left = in_heart_plane(heading, radial);
        }
        let solid = SolidLeaf {
            section: Section::Flat,
            thickness: 0.05,
            fold: piece.fold,
            widest: 0.55,
            base: piece.base,
            taper: piece.taper,
            tip: 0.0,
            arch: piece.curl,
            arch_from: piece.curl_from,
            twist: 0.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 1,
        };
        let mut leaf = part.leaf(
            frame.point(piece.angle, piece.radius, piece.height),
            heading,
            left,
            piece.length,
            piece.width,
            colours.toward_calyx(piece.calyx),
        );
        if piece.term != Term::Sepal {
            leaf.underside = colours.back;
        }
        // A petal is seen close: on the nearest step it takes every
        // station and both samples across, however short it is, so its
        // outline curves rather than breaking into a few facets.
        let (stations, across) = if fine {
            (most_stations, most_across)
        } else {
            leaves::detail_of(0, piece.length, piece.width)
        };
        let (stations, across) = (stations.min(most_stations), across.min(most_across));
        let first = mesh.positions.len();
        leaf.draw(&solid, stations, across, mesh);
        if piece.term != Term::Sepal {
            tint_foot(mesh, first, stations, across, &colours, leaf.colour);
            tint_back(mesh, first, stations, across, &colours);
        }
        if fine && laid.stripe > 0.0 && piece.term != Term::Sepal {
            stripe(
                &leaf,
                &solid,
                laid.stripe,
                colours.stripe.unwrap_or(leaf.colour),
                mesh,
            );
        }
        if fine && laid.spots > 0 && piece.term != Term::Sepal {
            spots(&leaf, &solid, laid.spots, colours.accent, mesh);
        }
    }
    for spur in &laid.spurs {
        let (foot, direction) = frame.rod(spur);
        spindle(
            (foot, direction),
            spur.length,
            (spur.width, spur.width * 0.15),
            if fine { 6 } else { 4 },
            colours.petal,
            part,
            mesh,
        );
    }
    for stamen in &laid.stamens {
        let (foot, direction) = frame.rod(&stamen.rod);
        rod_cone(foot, direction, &stamen.rod, colours.filament, part, mesh);
        if fine && stamen.fertile {
            let along = if stamen.tilt == 0.0 {
                direction
            } else {
                let across = direction.cross(axis).normalize_or(frame.side);
                (direction * math::cos(stamen.tilt) + across * math::sin(stamen.tilt))
                    .normalize_or(direction)
            };
            anther(
                foot + direction * stamen.rod.length,
                along,
                stamen.anther,
                colours.anther,
                part,
                mesh,
            );
        }
    }
    match &laid.pistil {
        Pistil::None => {}
        Pistil::Dome {
            height,
            radius,
            rise,
        } => blooms::dome(
            frame.foot + axis * *height,
            axis,
            side,
            *radius,
            *rise,
            colours.accent,
            part,
            mesh,
        ),
        Pistil::One {
            ovary,
            style,
            lobes,
        } => {
            if let Some((height, size)) = ovary {
                blooms::ellipsoid(
                    frame.foot + axis * *height,
                    axis,
                    side,
                    *size,
                    *size,
                    colours.ovary,
                    part,
                    mesh,
                );
            }
            let (foot, direction) = frame.rod(style);
            rod_cone(foot, direction, style, colours.filament, part, mesh);
            if fine {
                let tip = foot + direction * style.length;
                let count = (*lobes).max(1);
                for j in 0..count {
                    let angle = style.angle + TAU * f64::from(j) / f64::from(count);
                    let out = if count == 1 {
                        axis
                    } else {
                        (axis * 0.4 + frame.radial(angle)).normalize_or(axis)
                    };
                    let lobe = Rod {
                        length: style.length * 0.15,
                        width: style.length * 0.03,
                        ..*style
                    };
                    rod_cone(tip, out, &lobe, colours.stigma, part, mesh);
                }
            }
        }
        Pistil::Free { dome, pistils } => {
            let (height, radius, rise) = *dome;
            if radius > 0.0 {
                blooms::dome(
                    frame.foot + axis * height,
                    axis,
                    side,
                    radius,
                    rise,
                    colours.ovary,
                    part,
                    mesh,
                );
            }
            for pistil in pistils {
                let (foot, direction) = frame.rod(pistil);
                rod_cone(foot, direction, pistil, colours.ovary, part, mesh);
            }
        }
        Pistil::Column(column) => {
            let (foot, direction) = frame.rod(column);
            spindle(
                (foot, direction),
                column.length,
                (column.width, column.width * 0.6),
                4,
                colours.petal,
                part,
                mesh,
            );
            anther(
                foot + direction * column.length,
                direction,
                column.length * 1.5 * ANTHER,
                colours.accent,
                part,
                mesh,
            );
        }
    }
}

/// Where a reduced blunt petal's shoulders stand, a share of its length.
const SHOULDER: f64 = 0.85;

/// A flower reduced for a cluster's unit, or a lone flower's part mesh
/// drawn coarse (`part.coarse` counted from the reduced flower): its
/// perianth parts as kites seen from both sides (blunt ones shouldered
/// toward a round tip; one facet each from the
/// first coarse step), its tubes as `n`-sided bands, a low pyramid of a
/// centre or a column in the accent, and stamens while the flower is a
/// centimetre across or more and drawn fine; from the second coarse step
/// one flat polygon through the tips of its three to six petals, or a
/// hexagon round them where it has more or fewer.
#[allow(clippy::too_many_lines)]
pub(crate) fn draw_reduced(
    laid: &Floral,
    frame: Frame,
    colours: Colours,
    part: Part,
    mesh: &mut Mesh,
) {
    if laid.radius <= 0.0 {
        return;
    }
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    let axis = frame.axis;
    let leaf = part.leaf(frame.foot, axis, frame.side, 0.0, 0.0, colours.petal);
    // A naked flower (no perianth, no tube) shows only its stamens and
    // pistil, so they are what its colour is stated for (a chloranthus's
    // white connectives): drawn in it at any size, as broad as a stamen's
    // fleshy connective or anther, and from the flat steps as a disc as
    // wide as they reach.
    let naked = laid.pieces.is_empty() && laid.tube.is_none() && laid.calyx_tube.is_none();
    let reach = laid
        .stamens
        .iter()
        .map(|stamen| stamen.rod.radius + stamen.rod.length * math::sin(stamen.rod.lean))
        .fold(laid.centre, f64::max);
    let both = |corners: &[Vec3], colour: [f32; 3], mesh: &mut Mesh| {
        for flip in [false, true] {
            let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for &corner in corners {
                mesh.push(leaf.vertex(corner, colour));
            }
            for k in 1..u32::try_from(corners.len()).unwrap_or(2) - 1 {
                if flip {
                    mesh.indices.extend_from_slice(&[at, at + k + 1, at + k]);
                } else {
                    mesh.indices.extend_from_slice(&[at, at + k, at + k + 1]);
                }
            }
        }
    };
    if part.coarse >= crate::clusters::FLAT {
        let petals: Vec<&Piece> = laid
            .pieces
            .iter()
            .filter(|p| p.term != Term::Sepal)
            .collect();
        let (ring, rise, paint) = if naked && reach > 0.0 {
            (reach, 0.0, colours.petal)
        } else if petals.is_empty() {
            (laid.centre, 0.0, colours.accent)
        } else {
            #[allow(clippy::cast_precision_loss)]
            let count = petals.len() as f64;
            let reach = |p: &Piece| {
                let cup = math::radians(p.cup);
                (
                    p.radius + p.length * 0.85 * math::cos(cup),
                    p.height + p.length * 0.85 * math::sin(cup),
                )
            };
            let (ring, rise) = petals.iter().fold((0.0, 0.0), |(a, b), p| {
                let (x, y) = reach(p);
                (a + x / count, b + y / count)
            });
            (ring.abs(), rise, colours.petal)
        };
        if ring > 0.0 {
            // Three to six petals: a polygon through their tips, turned as
            // they are, so a four-petalled flower draws a square; else a
            // hexagon.
            let corners: Vec<Vec3> = if (3..=6).contains(&petals.len()) && !naked {
                petals
                    .iter()
                    .map(|p| {
                        let cup = math::radians(p.cup);
                        frame.point(
                            p.angle,
                            (p.radius + p.length * math::cos(cup)).abs(),
                            p.height + p.length * math::sin(cup),
                        )
                    })
                    .collect()
            } else {
                (0..6)
                    .map(|k| frame.point(TAU * f64::from(k) / 6.0, ring, rise))
                    .collect()
            };
            both(&corners, paint, mesh);
        }
        leaves::smooth_normals(mesh, first, first_index);
        return;
    }
    // What the perianth stands on: a floral cup, as an open frustum, or
    // an inferior ovary (a cactus's pericarpel), as on the full flower;
    // its length sets the seat the pieces stand at, so leaving it out
    // would float them.
    if let Some((length, r0, r1)) = laid.cup {
        blooms::frustum(
            frame.foot,
            axis,
            frame.side,
            length,
            r0,
            r1,
            colours.calyx,
            part,
            mesh,
        );
    }
    if let Some((height, a, b)) = laid.ovary {
        blooms::ellipsoid(
            frame.foot + axis * height,
            axis,
            frame.side,
            a,
            b,
            colours.pericarpel,
            part,
            mesh,
        );
        crate::fruit::skin(
            &crate::fruit::Surface {
                centre: frame.foot + axis * height,
                axis,
                across: frame.side,
                long: a,
                wide: b,
            },
            laid.pericarpel,
            (8, 0),
            colours.pericarpel,
            colours.dress,
            part,
            mesh,
        );
    }
    for tube in [&laid.floral_tube, &laid.calyx_tube, &laid.tube]
        .into_iter()
        .flatten()
    {
        let sides = tube.lobes.max(3);
        let colour = colours.of_tube(tube);
        for k in 0..sides {
            let a0 = tube.turn + TAU * f64::from(k) / f64::from(sides);
            let a1 = tube.turn + TAU * f64::from(k + 1) / f64::from(sides);
            let top = tube.height + tube.length;
            both(
                &[
                    frame.point(a0, tube.foot, tube.height),
                    frame.point(a1, tube.foot, tube.height),
                    frame.point(a1, tube.mouth, top),
                    frame.point(a0, tube.mouth, top),
                ],
                colour,
                mesh,
            );
        }
    }
    for piece in &laid.pieces {
        let radial = frame.radial(piece.angle);
        let heading = frame.rising(piece.angle, piece.cup);
        let mut left = axis.cross(radial).normalize_or(frame.side) * (piece.width * 0.5);
        let foot = frame.point(piece.angle, piece.radius, piece.height);
        let colour = colours.toward_calyx(piece.calyx);
        // A disymmetric corolla's outer petal, drawn in parts end to end
        // (pouch, blade, tip): each part broad at its foot and, but the
        // tip, at its end, so they join along an edge, not a point, and
        // flat in the heart's plane, so the two outer petals show the heart
        // face on.
        let joined = match piece.role {
            Some("pouch") => Some(0.9),
            Some("blade") => Some(0.5),
            Some("tip") => Some(0.0),
            _ => None,
        };
        if joined.is_some() {
            left = in_heart_plane(heading, radial) * (piece.width * 0.5);
        }
        if let Some(end) = joined {
            let widest = foot + heading * (piece.length * 0.55);
            let tip = foot + heading * piece.length;
            both(
                &[
                    foot - left * piece.base,
                    foot + left * piece.base,
                    widest + left,
                    tip + left * end,
                    tip - left * end,
                    widest - left,
                ],
                colour,
                mesh,
            );
        } else if part.coarse == 0 && piece.taper < 1.0 {
            // A blunt petal (obovate, orbicular) keeps its full shoulders
            // toward a rounded tip, as its solid narrows ([`SolidLeaf`]).
            let widest = foot + heading * (piece.length * 0.55);
            let shoulder = foot + heading * (piece.length * SHOULDER);
            let held = math::pow((1.0 - SHOULDER) / 0.45, piece.taper.max(0.05));
            both(
                &[
                    foot,
                    widest + left,
                    shoulder + left * held,
                    foot + heading * piece.length,
                    shoulder - left * held,
                    widest - left,
                ],
                colour,
                mesh,
            );
        } else if part.coarse == 0 {
            let widest = foot + heading * (piece.length * 0.55);
            both(
                &[
                    foot,
                    widest + left,
                    foot + heading * piece.length,
                    widest - left,
                ],
                colour,
                mesh,
            );
        } else {
            let end = foot + heading * (piece.length * 0.85);
            both(&[foot, end + left, end - left], colour, mesh);
        }
    }
    if part.coarse == 0 {
        for spur in &laid.spurs {
            let (foot, direction) = frame.rod(spur);
            let across = direction.cross(axis).normalize_or(frame.side) * spur.width;
            both(
                &[foot - across, foot + across, foot + direction * spur.length],
                colours.petal,
                mesh,
            );
        }
    }
    let pyramid = |height: f64, radius: f64, mesh: &mut Mesh| {
        let apex =
            mesh.push(leaf.vertex(frame.foot + axis * (height + radius * 0.5), colours.accent));
        let ring = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for k in 0..6 {
            let angle = TAU * f64::from(k) / 6.0;
            mesh.push(leaf.vertex(frame.point(angle, radius, height), colours.accent));
        }
        for k in 0..6 {
            mesh.indices
                .extend_from_slice(&[apex, ring + k, ring + (k + 1) % 6]);
        }
    };
    if naked
        && let Pistil::One {
            ovary: Some((height, size)),
            ..
        } = &laid.pistil
    {
        pyramid(*height - *size, *size, mesh);
    }
    match &laid.pistil {
        Pistil::Dome { height, radius, .. } => pyramid(*height, *radius, mesh),
        Pistil::Free { dome, .. } if dome.1 > 0.0 => pyramid(dome.0, dome.1, mesh),
        Pistil::One { style, .. } => {
            let (foot, direction) = frame.rod(style);
            let across = any_perpendicular(direction) * (style.length * 0.08);
            both(
                &[
                    foot - across,
                    foot + across,
                    foot + direction * style.length,
                ],
                colours.accent,
                mesh,
            );
        }
        Pistil::Column(column) => {
            let (foot, direction) = frame.rod(column);
            let across = direction.cross(axis).normalize_or(frame.side) * column.width;
            both(
                &[
                    foot - across,
                    foot + across,
                    foot + direction * column.length,
                ],
                colours.accent,
                mesh,
            );
        }
        Pistil::Free { .. } | Pistil::None => {}
    }
    if part.coarse <= 1 && (naked || laid.radius * 2.0 >= 0.01) {
        let (broad, colour) = if naked {
            (0.2, colours.petal)
        } else {
            (0.06, colours.filament)
        };
        for stamen in laid.stamens.iter().take(12) {
            let (foot, direction) = frame.rod(&stamen.rod);
            let across =
                direction.cross(axis).normalize_or(frame.side) * (stamen.rod.length * broad);
            let tip = foot + direction * stamen.rod.length;
            if naked {
                // Blunt, broadest toward its tip: a fleshy connective or
                // an anther, not a filament.
                let shoulder = foot + direction * (stamen.rod.length * 0.7);
                both(
                    &[
                        foot - across * 0.4,
                        foot + across * 0.4,
                        shoulder + across,
                        tip + across * 0.5,
                        tip - across * 0.5,
                        shoulder - across,
                    ],
                    colour,
                    mesh,
                );
            } else {
                both(&[foot - across, foot + across, tip], colour, mesh);
            }
        }
    }
    leaves::smooth_normals(mesh, first, first_index);
}

/// A tube drawn as two surfaces of revolution, its outer face and a
/// little inside it its inner face, `around` vertices round each of
/// `stations` + 1 rings, each ring closed on itself.
fn revolve(
    tube: &Tube,
    frame: Frame,
    around: u32,
    stations: u32,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    if tube.length <= 0.0 {
        return;
    }
    let leaf = part.leaf(frame.foot, frame.axis, frame.side, 0.0, 0.0, colour);
    for inner in [false, true] {
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        let paint = if inner {
            blooms::shade(colour, 0.8)
        } else {
            colour
        };
        for i in 0..=stations {
            let u = f64::from(i) / f64::from(stations);
            let radius = tube.radius_at(u) * if inner { 0.94 } else { 1.0 };
            for k in 0..around {
                let angle = tube.turn + TAU * f64::from(k) / f64::from(around);
                mesh.push(leaf.vertex(
                    frame.point(angle, radius, tube.height + tube.length * u),
                    paint,
                ));
            }
        }
        let at = |i: u32, k: u32| first + i * around + k % around;
        for i in 0..stations {
            for k in 0..around {
                let (a0, a1, b0, b1) = (at(i, k), at(i, k + 1), at(i + 1, k), at(i + 1, k + 1));
                if inner {
                    mesh.indices.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
                } else {
                    mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
                }
            }
        }
    }
}

/// A `sides`-sided tapering body from `foot` along `direction`, `length`
/// long, from radius `r0` to `r1`, closed at its tip.
fn spindle(
    (foot, direction): (Vec3, Vec3),
    length: f64,
    (r0, r1): (f64, f64),
    sides: u32,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let side = any_perpendicular(direction);
    let other = direction.cross(side);
    let leaf = part.leaf(foot, direction, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for (along, radius) in [(0.0, r0), (length, r1)] {
        for k in 0..sides {
            let angle = TAU * f64::from(k) / f64::from(sides);
            let around = side * math::cos(angle) + other * math::sin(angle);
            mesh.push(leaf.vertex(foot + direction * along + around * radius, colour));
        }
    }
    let tip = mesh.push(leaf.vertex(foot + direction * (length * 1.04), colour));
    for k in 0..sides {
        let (a0, a1) = (first + k, first + (k + 1) % sides);
        let (b0, b1) = (a0 + sides, a1 + sides);
        mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
        mesh.indices.extend_from_slice(&[b0, b1, tip]);
    }
}

/// A rod as a three-sided cone, as stamens were drawn before P5.
fn rod_cone(foot: Vec3, direction: Vec3, rod: &Rod, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
    let left = any_perpendicular(direction);
    let leaf = part.leaf(foot, direction, left, rod.length, rod.width * 2.0, colour);
    leaves::cone(
        &leaf,
        foot,
        direction,
        left,
        direction.cross(left),
        rod.length,
        rod.width,
        colour,
        mesh,
    );
}

/// An anther at a filament's `tip`: a short three-sided bar `length` long
/// along `direction`, centred on the tip, its ends pointed.
fn anther(tip: Vec3, direction: Vec3, length: f64, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
    let side = any_perpendicular(direction);
    let other = direction.cross(side);
    let (half, wide) = (length * 0.5, length * 0.218_75);
    let leaf = part.leaf(tip, direction, side, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    mesh.push(leaf.vertex(tip - direction * half, colour));
    mesh.push(leaf.vertex(tip + direction * half, colour));
    // Two rings, a little in from each end, so the anther is a bar.
    for end in [-0.6, 0.6] {
        for k in 0..3 {
            let angle = TAU * f64::from(k) / 3.0;
            mesh.push(leaf.vertex(
                tip + direction * (half * end)
                    + (side * math::cos(angle) + other * math::sin(angle)) * wide,
                colour,
            ));
        }
    }
    let low = |k: u32| first + 2 + k % 3;
    let high = |k: u32| first + 5 + k % 3;
    for k in 0..3 {
        mesh.indices.extend_from_slice(&[first, low(k + 1), low(k)]);
        mesh.indices
            .extend_from_slice(&[first + 1, high(k), high(k + 1)]);
        mesh.indices.extend_from_slice(&[
            low(k),
            low(k + 1),
            high(k + 1),
            low(k),
            high(k + 1),
            high(k),
        ]);
    }
}

/// The rise, in degrees from the flower's plane, from which free tepals
/// make a bell or a funnel rather than a flat or cupped perianth: above a
/// cupped bloom's 55 (a magnolia's), below a lily's bell at 60.
pub const BELL: f64 = 58.0;

/// How much wider than drawn flat each of `n` free tepals (`radius` from
/// the axis, `length` long, `width` wide) is drawn where they rise as a
/// bell (`cup` of [`BELL`] or more, no tube): wide enough that its two
/// whorls, the inner 0.85 of the outer, overlap at their widest, as a
/// bell's tepals do, but no wider than three fifths of their length; 1
/// otherwise.
fn bell_width(cup: f64, tubed: bool, n: f64, (radius, length, width): (f64, f64, f64)) -> f64 {
    if tubed || cup < BELL || width <= 0.0 {
        return 1.0;
    }
    let widest = radius + 0.55 * length * math::cos(math::radians(cup * 0.85));
    let needed = 1.15 * TAU * widest / (n * 0.925);
    (needed.min(0.6 * length) / width).max(1.0)
}

/// Share of a petal's length its foot colour takes to fade into the
/// petal's, centred on where it reaches.
const FOOT_FADE: f64 = 0.16;

/// Share of a petal's length its nectaries' colour reaches from its foot.
const STAR_LENGTH: f64 = 0.14;

/// Paint the inner face of a petal just drawn from vertex `first` (by
/// [`leaves::Leaf::draw`], `stations` rings of `4 * across` vertices, the
/// inner face first in each) in its foot's colours: the flower's `base`
/// colour fading into `petal` at `base_length` of its length, and its
/// nectaries' `star` at its very foot.
fn tint_foot(
    mesh: &mut Mesh,
    first: usize,
    stations: usize,
    across: usize,
    colours: &Colours,
    petal: [f32; 3],
) {
    if colours.base.is_none() && colours.star.is_none() {
        return;
    }
    let ring = 4 * across;
    let mix = |a: [f32; 3], b: [f32; 3], t: f64| -> [f32; 3] {
        #[allow(clippy::cast_possible_truncation)]
        let t = t as f32;
        std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
    };
    for station in 0..stations {
        #[allow(clippy::cast_precision_loss)]
        let u = station as f64 / stations as f64;
        let mut colour = petal;
        if let Some(base) = colours.base {
            let reach = colours.base_length;
            let t = math::smoothstep(reach - 0.5 * FOOT_FADE, reach + 0.5 * FOOT_FADE, u);
            colour = mix(base, colour, t);
        }
        if let Some(star) = colours.star {
            let t = math::smoothstep(0.5 * STAR_LENGTH, 1.5 * STAR_LENGTH, u);
            colour = mix(star, colour, t);
        }
        for k in 0..=2 * across {
            if let Some(vertex) = mesh.colors.get_mut(first + station * ring + k) {
                vertex[0] = colour[0];
                vertex[1] = colour[1];
                vertex[2] = colour[2];
            }
        }
    }
}

/// The midrib of a perianth part drawn as `leaf` with `solid`, as
/// [`leaves::Leaf::draw`] walks it: `steps + 1` points evenly along it from
/// its foot to its tip, each with the tangent there.
#[allow(clippy::cast_precision_loss)]
fn midrib(leaf: &leaves::Leaf, solid: &SolidLeaf, steps: usize) -> Vec<(Vec3, Vec3)> {
    let arch = math::radians(solid.arch);
    let from = solid.arch_from;
    let mut point = leaf.base;
    let mut walk = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let u = i as f64 / steps as f64;
        let bent = if from > 0.0 {
            let mid = (u + 0.5 / steps as f64).min(1.0);
            ((mid - from) / (1.0 - from)).clamp(0.0, 1.0)
        } else {
            u
        };
        let tangent = leaf.heading.rotate_about(leaf.left, arch * bent);
        walk.push((point, tangent));
        point += tangent * (leaf.length / steps as f64);
    }
    walk
}

/// Share of a petal's width at each point a stripe down its middle takes
/// there: the median furrow a lily's nectary runs up from the tepal's foot,
/// the stripe narrowing to a point where it stops.
const STRIPE_WIDTH: f64 = 0.3;

/// A stripe down the middle of the inner face of a perianth part drawn as
/// `leaf` with `solid`, from its foot to `reach` of its length in
/// `colour`: a ribbon standing just off the face along the midrib, a
/// [`STRIPE_WIDTH`] of the part's width and narrowing to a point at its
/// end, as Kellogg's lily's yellow stripe runs from its nectary.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn stripe(leaf: &leaves::Leaf, solid: &SolidLeaf, reach: f64, colour: [f32; 3], mesh: &mut Mesh) {
    const STEPS: usize = 24;
    const BANDS: usize = 4;
    let walk = midrib(leaf, solid, STEPS);
    let reach = reach.clamp(0.0, 1.0);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for band in 0..=BANDS {
        let t = band as f64 / BANDS as f64;
        let u = reach * t;
        let station = ((u * STEPS as f64).round() as usize).min(STEPS);
        let (centre, tangent) = walk[station];
        let face = tangent.cross(leaf.left).normalize_or(leaf.heading);
        let half = 0.5 * STRIPE_WIDTH * solid.width_at(u) * leaf.width * (1.0 - t * t * t);
        let lift = solid.thickness * leaf.width * 0.6;
        for side in [-1.0, 1.0] {
            mesh.push(leaf.vertex(centre + leaf.left * (side * half) + face * lift, colour));
        }
    }
    // One face, turned to the inner face's side: a back face is culled.
    let (centre, tangent) = walk[0];
    let face = tangent.cross(leaf.left).normalize_or(leaf.heading);
    let ahead = walk[(reach * STEPS as f64).round().max(1.0) as usize].0 - centre;
    let toward = leaf.left.cross(ahead).dot(face) > 0.0;
    for band in 0..BANDS as u32 {
        let (a, b) = (first + 2 * band, first + 2 * band + 1);
        let (c, d) = (a + 2, b + 2);
        if toward {
            mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
        } else {
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
}

/// Paint the outer face of a petal just drawn from vertex `first` (as
/// [`tint_foot`] reads it, the outer face last in each ring) at its foot in
/// the flower's `back_base` colour, fading into the outer face's own at
/// `back_base_length` of its length.
fn tint_back(mesh: &mut Mesh, first: usize, stations: usize, across: usize, colours: &Colours) {
    let Some(base) = colours.back_base else {
        return;
    };
    let ring = 4 * across;
    for station in 0..stations {
        #[allow(clippy::cast_precision_loss)]
        let u = station as f64 / stations as f64;
        let reach = colours.back_base_length;
        #[allow(clippy::cast_possible_truncation)]
        let t = math::smoothstep(reach - 0.5 * FOOT_FADE, reach + 0.5 * FOOT_FADE, u) as f32;
        for k in 2 * across + 1..ring {
            if let Some(vertex) = mesh.colors.get_mut(first + station * ring + k) {
                for c in 0..3 {
                    vertex[c] = base[c] + (vertex[c] - base[c]) * t;
                }
            }
        }
    }
}

/// `count` dark spots on the inner face of a perianth part drawn as
/// `leaf` with `solid`, scattered over its lower half and kept off its
/// margins, each a small diamond standing just off the face. Where they
/// fall is fixed by the part's own position, so every flower's differ.
fn spots(leaf: &leaves::Leaf, solid: &SolidLeaf, count: u32, colour: [f32; 3], mesh: &mut Mesh) {
    const STEPS: usize = 24;
    let walk = midrib(leaf, solid, STEPS);
    let seed =
        (leaf.base.x * 7919.0 + leaf.base.y * 104_729.0 + leaf.base.z * 1_299_709.0).to_bits();
    let size = (leaf.width * 0.07).max(leaf.length * 0.012);
    for k in 0..count {
        let a = crate::rng::unit(crate::rng::mix64(seed ^ (u64::from(k) * 0x9E37_79B9)));
        let b = crate::rng::unit(crate::rng::mix64(seed ^ (u64::from(k) * 0x85EB_CA6B + 1)));
        // Crowded toward the foot and thinning out by two thirds of the
        // length, where a lily's floras say its spots stop ("concentrated
        // in proximal 1/2-2/3").
        let u = 0.05 + 0.6 * a.powf(1.5);
        let station = ((u * STEPS as f64).round() as usize).min(STEPS);
        let (centre, tangent) = walk[station];
        let side = leaf.left;
        let face = tangent.cross(side).normalize_or(leaf.heading);
        let w = solid.width_at(u) * 0.5 * leaf.width * 0.75 * (2.0 * b - 1.0);
        let lift = solid.thickness * leaf.width * 0.6 + size * 0.15;
        let at = centre + side * w + face * lift;
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        let long = size * (0.8 + 0.6 * b);
        for (along, across) in [
            (long, 0.0),
            (0.0, size * 0.6),
            (-long, 0.0),
            (0.0, -size * 0.6),
        ] {
            mesh.push(leaf.vertex(at + tangent * along + side * across, colour));
        }
        // One face, turned to the part's inner face (the diamond's corners
        // run counter-clockwise seen from there): its back, against the
        // part, is culled.
        mesh.indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// Triangles one flower takes drawn solid `coarse` steps coarser.
#[must_use]
pub fn triangles(flower: &Flower, form: &FlowerForm, coarse: usize) -> usize {
    let mut mesh = Mesh::default();
    draw(
        &layout(flower, form, 0.5, 0.0),
        Frame {
            foot: Vec3::ZERO,
            axis: Vec3::Y,
            side: Vec3::X,
        },
        Colours {
            petal: [1.0; 3],
            calyx: CALYX,
            accent: [1.0; 3],
            filament: [1.0; 3],
            anther: [1.0; 3],
            ovary: [1.0; 3],
            base: None,
            base_length: 0.4,
            star: None,
            stripe: None,
            back: None,
            back_base: None,
            back_base_length: 0.4,
            tube: None,
            stigma: [1.0; 3],
            pericarpel: CALYX,
            dress: crate::fruit::Dress::default(),
        },
        Part {
            born: 0.0,
            shed: None,
            coarse,
        },
        &mut mesh,
    );
    mesh.triangle_count()
}

/// The card of a flower that is not plain at `(x, y)`, in shares of its
/// radius from its centre, its axis toward the viewer and its adaxial side
/// up: its perianth parts by the card's petal profile, each as long as it
/// shows past its foot against a petal rising the form's cup; sepals where
/// they show past the petals, in the mix of `slots` (the look's colour and
/// accent) nearest `calyx`; a fused tube's throat; anthers as accent dots;
/// its pistil, centre or column in the accent. Painted by a cached
/// painter, as a card paints thousands of points of one flower.
#[must_use]
pub fn paint(flower: &Flower, slots: ([f32; 3], [f32; 3]), x: f64, y: f64) -> Painted {
    thread_local! {
        static PAINTERS: std::cell::RefCell<Vec<Painter>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    PAINTERS.with(|painters| {
        let mut painters = painters.borrow_mut();
        let index = painters
            .iter()
            .position(|painter| painter.flower == *flower && painter.slots == slots)
            .unwrap_or_else(|| {
                if painters.len() >= 8 {
                    painters.remove(0);
                }
                painters.push(Painter::new(flower, slots));
                painters.len() - 1
            });
        painters[index].paint(x, y)
    })
}

/// Paints one flower's card (see [`paint`]).
struct Painter {
    flower: Flower,
    slots: ([f32; 3], [f32; 3]),
    laid: Floral,
    /// Each part's reach on the card over its length.
    reach: Vec<f64>,
    /// The farthest the flower reaches, which the card fits.
    extent: f64,
    /// The calyx's colour as the slots mix it: its brightness and accent.
    calyx: (f64, f64),
    /// Where each anther shows.
    anthers: Vec<(f64, f64)>,
}

impl Painter {
    fn new(flower: &Flower, slots: ([f32; 3], [f32; 3])) -> Self {
        let laid = layout(flower, &FlowerForm::default(), 1.0, 0.0);
        let against = math::cos(math::radians(laid.form_cup)).max(0.2);
        let reach: Vec<f64> = laid
            .pieces
            .iter()
            .map(|p| (math::cos(math::radians(p.cup)) / against).clamp(0.3, 1.2))
            .collect();
        let anthers: Vec<(f64, f64)> = laid
            .stamens
            .iter()
            .filter(|s| s.fertile)
            .map(|s| {
                let out = s.rod.radius + s.rod.length * math::sin(s.rod.lean);
                (out * math::sin(s.rod.angle), out * math::cos(s.rod.angle))
            })
            .collect();
        let extent = laid
            .pieces
            .iter()
            .zip(&reach)
            .map(|(p, k)| p.radius + p.length * k)
            .chain(
                anthers
                    .iter()
                    .map(|&(x, y)| math::sqrt(x * x + y * y) + 0.04),
            )
            .fold(1.0, f64::max);
        let calyx = flower.calyx.unwrap_or(CALYX);
        let (a, b) = crate::clusters::blend(calyx, slots);
        let calyx = if a + b <= 1e-9 {
            (0.3, 0.0)
        } else {
            ((a + b).min(2.0), b / (a + b))
        };
        Self {
            flower: flower.clone(),
            slots,
            laid,
            reach,
            extent,
            calyx,
            anthers,
        }
    }

    fn paint(&self, x: f64, y: f64) -> Painted {
        let solid = |bright: f64, accent: f64| Painted {
            cover: 1.0,
            bright,
            accent,
        };
        let (x, y) = (x * self.extent, y * self.extent);
        let r = math::sqrt(x * x + y * y);
        let laid = &self.laid;
        if self
            .anthers
            .iter()
            .any(|&(ax, ay)| (x - ax).powi(2) + (y - ay).powi(2) < 0.04 * 0.04)
        {
            return solid(1.2, 1.0);
        }
        match &laid.pistil {
            Pistil::Dome { radius, .. }
            | Pistil::Free {
                dome: (_, radius, _),
                ..
            } if r < *radius => {
                return solid(0.8 + 0.4 * (1.0 - r / radius.max(1e-9)), 1.0);
            }
            Pistil::One { style, .. } if r < (style.length * 0.2).max(0.05) => {
                return solid(1.1, 1.0);
            }
            Pistil::Column(column) => {
                let out = column.length * math::sin(column.lean) * 0.5;
                let (cx, cy) = (out * math::sin(column.angle), out * math::cos(column.angle));
                if (x - cx).powi(2) + (y - cy).powi(2) < (column.length * 0.3).powi(2) {
                    return solid(1.0, 1.0);
                }
            }
            _ => {}
        }
        if let Some(tube) = &laid.tube
            && r < tube.mouth
        {
            return solid(0.55, 0.1);
        }
        // The inner parts over the outer: the petals over the sepals.
        for (piece, k) in laid.pieces.iter().zip(&self.reach).rev() {
            let direction = (math::sin(piece.angle), math::cos(piece.angle));
            let (dx, dy) = (
                x - piece.radius * direction.0,
                y - piece.radius * direction.1,
            );
            let along = dx * direction.0 + dy * direction.1;
            let across = dx * direction.1 - dy * direction.0;
            let widest = 0.6 * piece.width;
            if widest <= 0.0 {
                continue;
            }
            let notch = laid.notch * 0.4 * (1.0 - across.abs() / (0.55 * widest)).max(0.0);
            let reach = piece.length * k * (1.0 - notch);
            if reach <= 0.0 {
                continue;
            }
            let u = along / reach;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let point = if piece.taper > 1.0 { 1.0 } else { 0.0 };
            let k = 0.35 + 0.9 * point;
            let peak = 1.1 / (1.1 + k);
            let norm = math::pow(peak, 1.1) * math::pow(1.0 - peak, k);
            // A lobe on a tube is wide at its foot.
            let rising = if piece.base > 0.5 {
                math::pow(u.max(0.0), 0.3)
            } else {
                math::pow(u, 1.1)
            };
            let width = widest * rising * math::pow(1.0 - u, k) / norm.max(1e-9);
            if across.abs() >= width {
                continue;
            }
            let shade = 0.88 + 0.16 * u + 0.03 * math::cos(across / width.max(1e-6) * 3.0 * PI) * u;
            if piece.calyx > 0.0 {
                let mix = f64::from(piece.calyx);
                let (bright, accent) = self.calyx;
                return solid((1.0 - mix) * shade + mix * bright * shade, mix * accent);
            }
            return solid(shade, (1.0 - u / 0.35).max(0.0) * 0.5);
        }
        Painted::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flower(change: impl FnOnce(&mut Flower)) -> Flower {
        let mut flower = Flower::default();
        change(&mut flower);
        flower
    }

    fn form() -> FlowerForm {
        FlowerForm::default()
    }

    fn degrees_of(angle: f64) -> f64 {
        math::degrees(angle).rem_euclid(360.0)
    }

    #[test]
    fn sepals_alternate_with_the_petals() {
        let laid = layout(
            &flower(|f| {
                f.petals = 5;
                f.sepals = 5;
            }),
            &form(),
            1.0,
            0.0,
        );
        let sepals: Vec<f64> = laid
            .pieces
            .iter()
            .filter(|p| p.term == Term::Sepal)
            .map(|p| degrees_of(p.angle))
            .collect();
        let petals: Vec<f64> = laid
            .pieces
            .iter()
            .filter(|p| p.term == Term::Petal)
            .map(|p| degrees_of(p.angle))
            .collect();
        assert_eq!((sepals.len(), petals.len()), (5, 5));
        for sepal in sepals {
            let nearest = petals
                .iter()
                .map(|p| ((sepal - p + 540.0) % 360.0 - 180.0).abs())
                .fold(f64::MAX, f64::min);
            assert!((nearest - 36.0).abs() < 1e-9, "a sepal between two petals");
        }
        assert_eq!(laid.pieces[0].term, Term::Sepal, "sepals come first");
    }

    #[test]
    fn roles_stand_where_their_word_puts_them() {
        let pea = layout(
            &flower(|f| f.corolla = Corolla::Papilionaceous),
            &form(),
            1.0,
            1.0,
        );
        let at = |laid: &Floral, role| {
            laid.pieces
                .iter()
                .filter(|p| p.role == Some(role))
                .map(|p| degrees_of(p.angle))
                .collect::<Vec<_>>()
        };
        assert_eq!(at(&pea, "banner"), vec![0.0], "the banner is adaxial");
        let keel = at(&pea, "keel");
        assert!(keel.iter().all(|a| (a - 180.0).abs() < 13.0), "{keel:?}");
        assert!(pea.stamens.is_empty(), "a pea's stamens are in its keel");
        let orchid = layout(
            &flower(|f| {
                f.petals = 3;
                f.sepals = 3;
                f.sepal_length = 1.0;
                f.corolla = Corolla::Labellate;
                f.turn = 180.0;
            }),
            &form(),
            1.0,
            0.0,
        );
        assert_eq!(at(&orchid, "lip"), vec![180.0], "resupinate: the lip below");
        assert_eq!(at(&orchid, "dorsal sepal"), vec![0.0]);
        assert!(matches!(orchid.pistil, Pistil::Column(_)));
        let violet = layout(
            &flower(|f| {
                f.corolla = Corolla::Bilabiate;
                f.spur = 0.5;
            }),
            &form(),
            1.0,
            0.0,
        );
        assert_eq!(at(&violet, "upper lip").len(), 2);
        assert_eq!(at(&violet, "lower lip").len(), 3);
        assert_eq!(
            violet.spurs.len(),
            1,
            "the lower lip's middle petal spurred"
        );
        assert!((degrees_of(violet.spurs[0].angle) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn an_unequal_corolla_stands_its_upper_petal_apart() {
        let laid = layout(&flower(|f| f.corolla = Corolla::Unequal), &form(), 1.0, 1.0);
        let petals: Vec<_> = laid
            .pieces
            .iter()
            .filter(|p| p.term == Term::Petal)
            .collect();
        assert_eq!(petals.len(), 5);
        let upper: Vec<_> = petals
            .iter()
            .filter(|p| p.role == Some("upper petal"))
            .collect();
        assert_eq!(upper.len(), 1);
        assert!(
            degrees_of(upper[0].angle).abs() < 1e-9,
            "the upper petal is adaxial"
        );
        let others: Vec<_> = petals.iter().filter(|p| p.role == Some("petal")).collect();
        assert!(
            others
                .iter()
                .all(|p| p.width < upper[0].width && p.cup < upper[0].cup)
        );
        assert!(
            others
                .iter()
                .all(|p| (p.width - others[0].width).abs() < 1e-12),
            "the rest alike"
        );
        assert!(flower(|f| f.corolla = Corolla::Unequal).is_zygomorphic());
        let shape = |f: Flower| crate::looks::Shape::Flower(f).validate();
        assert!(
            shape(flower(|f| {
                f.corolla = Corolla::Unequal;
                f.petals = 6;
                f.perianth = Perianth::Spiral;
            }))
            .unwrap_err()
            .contains("unequal")
        );
    }

    #[test]
    fn zygomorphic_tepals_take_their_roles_by_place() {
        // A gladiolus's six tepals: three upper, three lower, the middle
        // lower one the lip.
        let tepals = |kind| {
            flower(|f| {
                f.corolla = kind;
                f.petals = 6;
                f.sepals = 0;
                f.perianth = Perianth::Tepals;
            })
        };
        let shape = |f: Flower| crate::looks::Shape::Flower(f).validate();
        assert!(shape(tepals(Corolla::Bilabiate)).is_ok());
        assert!(shape(tepals(Corolla::Unequal)).is_ok());
        let laid = layout(&tepals(Corolla::Bilabiate), &form(), 1.0, 1.0);
        let at = |role| -> Vec<f64> {
            laid.pieces
                .iter()
                .filter(|p| p.term == Term::Tepal && p.role == Some(role))
                .map(|p| degrees_of(p.angle))
                .collect()
        };
        assert_eq!(at("upper lip").len(), 3);
        assert_eq!(at("lower lip").len(), 3);
        let lip = laid
            .pieces
            .iter()
            .find(|p| (degrees_of(p.angle) - 180.0).abs() < 1e-9)
            .unwrap();
        // Wider than the inner upper tepals beside it, and bent down.
        let beside = laid
            .pieces
            .iter()
            .find(|p| (degrees_of(p.angle) - 60.0).abs() < 1e-9)
            .unwrap();
        assert_eq!(lip.role, Some("lower lip"));
        assert!(lip.width > beside.width && lip.cup < beside.cup);
        // Of no named kind: one upper tepal unlike the rest.
        let laid = layout(&tepals(Corolla::Unequal), &form(), 1.0, 1.0);
        let upper: Vec<_> = laid
            .pieces
            .iter()
            .filter(|p| p.role == Some("upper tepal"))
            .collect();
        assert_eq!(upper.len(), 1);
        assert!(degrees_of(upper[0].angle).abs() < 1e-9);
        assert!(
            laid.pieces
                .iter()
                .filter(|p| p.role == Some("tepal"))
                .all(|p| p.width < upper[0].width)
        );
    }

    #[test]
    fn petals_in_two_whorls_stand_three_and_three() {
        // A barberry's six petals: three outer, three inner between them.
        let barberry = flower(|f| {
            f.petals = 6;
            f.petal_whorls = 2;
            f.stamens = 6;
        });
        let laid = layout(&barberry, &form(), 1.0, 0.0);
        let petals: Vec<_> = laid
            .pieces
            .iter()
            .filter(|p| p.term == Term::Petal)
            .collect();
        assert_eq!(petals.len(), 6);
        let (outer, inner) = petals.split_at(3);
        for (k, p) in (0_u32..).zip(outer) {
            assert!((degrees_of(p.angle) - 120.0 * f64::from(k)).abs() < 1e-9);
        }
        for (k, p) in (0_u32..).zip(inner) {
            assert!((degrees_of(p.angle) - 60.0 - 120.0 * f64::from(k)).abs() < 1e-9);
            assert!(p.width < outer[0].width && p.cup > outer[0].cup);
        }
        // Its stamens stand in whorls of three, one before each petal.
        assert_eq!(laid.stamens.len(), 6);
        let shape = |f: Flower| crate::looks::Shape::Flower(f).validate();
        assert!(shape(barberry).is_ok());
        assert!(
            shape(flower(|f| {
                f.petals = 5;
                f.petal_whorls = 2;
            }))
            .unwrap_err()
            .contains("two whorls")
        );
    }

    #[test]
    fn wrong_counts_for_a_role_are_refused() {
        let shape = |f: Flower| crate::looks::Shape::Flower(f).validate();
        let pea = flower(|f| {
            f.corolla = Corolla::Papilionaceous;
            f.petals = 4;
        });
        assert!(shape(pea).unwrap_err().contains("papilionaceous"));
        let orchid = flower(|f| {
            f.corolla = Corolla::Labellate;
            f.petals = 6;
            f.perianth = Perianth::Tepals;
        });
        assert!(shape(orchid).unwrap_err().contains("labellate"));
        let many = flower(|f| f.petals = 20);
        assert!(shape(many).unwrap_err().contains("spiral"));
    }

    #[test]
    fn stamens_stand_in_whorls_then_on_a_spiral() {
        let few = layout(&flower(|f| f.stamens = 10), &form(), 1.0, 0.0);
        let radii: Vec<f64> = few.stamens.iter().map(|s| s.rod.radius).collect();
        let c = few.centre;
        assert!(radii[..5].iter().all(|r| (r - 0.5 * c).abs() < 1e-12));
        assert!(radii[5..].iter().all(|r| (r - 0.75 * c).abs() < 1e-12));
        let many = layout(&flower(|f| f.stamens = 60), &form(), 1.0, 0.0);
        let spiral: Vec<f64> = many.stamens.iter().map(|s| s.rod.radius).collect();
        assert!(spiral.windows(2).all(|w| w[1] > w[0]), "out along a spiral");
        let sterile = layout(
            &flower(|f| {
                f.stamens = 4;
                f.staminodes = 1;
            }),
            &form(),
            1.0,
            0.0,
        );
        assert_eq!(
            sterile.stamens.iter().filter(|s| !s.fertile).count(),
            1,
            "a staminode is drawn, without an anther"
        );
    }

    #[test]
    fn a_fused_tube_is_one_connected_surface() {
        let laid = layout(
            &flower(|f| {
                f.fused = 0.6;
                f.throat = 0.25;
                f.belly = 0.1;
            }),
            &form(),
            0.5,
            0.0,
        );
        let tube = laid.tube.expect("a fused corolla has a tube");
        let mut mesh = Mesh::default();
        revolve(
            &tube,
            Frame {
                foot: Vec3::ZERO,
                axis: Vec3::Y,
                side: Vec3::X,
            },
            20,
            6,
            [1.0; 3],
            Part {
                born: 0.0,
                shed: None,
                coarse: 0,
            },
            &mut mesh,
        );
        // Each face: 7 rings of 20, no seam's vertices twice.
        assert_eq!(mesh.positions.len(), 2 * 7 * 20);
        let outer = &mesh.positions[..7 * 20];
        for (i, a) in outer.iter().enumerate() {
            for b in &outer[i + 1..] {
                let d: f32 = (0..3).map(|c| (a[c] - b[c]).powi(2)).sum();
                assert!(d > 1e-12, "no duplicate vertex");
            }
        }
        // The lobes stand on its mouth.
        for piece in &laid.pieces {
            assert!((piece.radius - tube.mouth).abs() < 1e-12);
            assert!((piece.height - (tube.height + tube.length)).abs() < 1e-12);
        }
        assert!(tube.radius_at(0.5) > tube.foot + (tube.mouth - tube.foot) * 0.5);
    }

    #[test]
    fn an_inferior_ovary_sits_below_the_perianth() {
        let laid = layout(
            &flower(|f| {
                f.ovary = Ovary::Inferior;
                f.tube = 1.0;
                f.carpels = 4;
            }),
            &form(),
            0.5,
            0.0,
        );
        let (centre, half, _) = laid.ovary.expect("an ovary below");
        assert!(centre + half <= laid.pieces[0].height + 1e-12);
        assert!(laid.cup.is_none(), "the ovary is the floral cup");
        assert!(matches!(laid.pistil, Pistil::One { ovary: None, .. }));
    }

    #[test]
    fn many_carpels_are_one_pistil_below_the_whorls_and_free_above() {
        let carpels = |ovary| {
            let laid = layout(
                &flower(|f| {
                    f.ovary = ovary;
                    f.carpels = 12;
                }),
                &form(),
                0.5,
                0.0,
            );
            laid.pistil
        };
        assert!(matches!(
            carpels(Ovary::Inferior),
            Pistil::One { lobes: 12, .. }
        ));
        assert!(matches!(
            carpels(Ovary::Half),
            Pistil::One { lobes: 12, .. }
        ));
        assert!(
            matches!(carpels(Ovary::Superior), Pistil::Free { pistils, .. } if pistils.len() == 12)
        );
    }

    #[test]
    fn a_closed_flower_hides_its_centre_and_stays_small() {
        let open = layout(&flower(|f| f.stamens = 10), &form(), 1.0, 0.0);
        let closed = layout(
            &flower(|f| {
                f.stamens = 10;
                f.open = 0.0;
            }),
            &form(),
            1.0,
            0.0,
        );
        assert!(closed.stamens.is_empty() && closed.pistil == Pistil::None);
        assert!(closed.pieces[0].cup > 90.0);
        assert!(closed.pieces[0].length < open.pieces[0].length);
    }

    #[test]
    fn the_solid_stays_within_its_size() {
        let showcase = [
            flower(|f| {
                f.sepals = 5;
                f.stamens = 60;
                f.carpels = 20;
                f.tube = 0.3;
            }),
            flower(|f| {
                f.corolla = Corolla::Papilionaceous;
                f.sepals = 5;
                f.sepals_fused = 0.6;
                f.stamens = 10;
            }),
            flower(|f| {
                f.corolla = Corolla::Bilabiate;
                f.fused = 0.75;
                f.throat = 0.3;
                f.stamens = 4;
                f.carpels = 2;
            }),
            flower(|f| {
                f.petals = 3;
                f.sepals = 3;
                f.sepal_length = 1.0;
                f.corolla = Corolla::Labellate;
                f.turn = 180.0;
                f.spur = 1.0;
            }),
            flower(|f| {
                f.petals = 30;
                f.perianth = Perianth::Spiral;
                f.stamens = 100;
            }),
            flower(|f| {
                f.petals = 30;
                f.perianth = Perianth::Spiral;
                f.stamens = 100;
                f.carpels = 10;
                f.ovary = Ovary::Inferior;
                f.tube = 1.2;
                f.fused = 0.6;
                f.pericarpel = Skin::Spiny;
            }),
        ];
        for f in &showcase {
            for coarse in 0..5 {
                let mut mesh = Mesh::default();
                draw(
                    &layout(f, &form(), 0.05, 0.3),
                    Frame::new(Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z, f.is_zygomorphic()),
                    Colours {
                        petal: [0.9; 3],
                        calyx: CALYX,
                        accent: [0.9, 0.7, 0.1],
                        filament: [0.9, 0.7, 0.1],
                        anther: [0.9, 0.7, 0.1],
                        ovary: [0.9, 0.7, 0.1],
                        base: None,
                        base_length: 0.4,
                        star: None,
                        stripe: None,
                        back: None,
                        back_base: None,
                        back_base_length: 0.4,
                        tube: None,
                        stigma: [0.9, 0.7, 0.1],
                        pericarpel: CALYX,
                        dress: crate::fruit::Dress::default(),
                    },
                    Part {
                        born: 1.0,
                        shed: None,
                        coarse,
                    },
                    &mut mesh,
                );
                assert!(mesh.triangle_count() > 0, "{f:?} at {coarse}");
                for p in &mesh.positions {
                    let length = p.iter().map(|c| c * c).sum::<f32>().sqrt();
                    assert!(length <= 0.11, "within twice its size: {p:?} {f:?}");
                }
            }
            assert!(triangles(f, &form(), 0) >= triangles(f, &form(), 1));
            assert!(triangles(f, &form(), 1) >= triangles(f, &form(), 3));
        }
    }

    /// A cactus flower's pericarpel wears its fruit's skin, spines the
    /// most, and its stigma lobes take their own colour.
    #[test]
    fn a_cactus_pericarpel_wears_its_skin() {
        let drawn = |skin: Skin| {
            let f = flower(|f| {
                f.petals = 20;
                f.perianth = Perianth::Spiral;
                f.stamens = 40;
                f.carpels = 8;
                f.ovary = Ovary::Inferior;
                f.tube = 1.0;
                f.pericarpel = skin;
                f.stigma = Some([0.1, 0.5, 0.1]);
            });
            let mut mesh = Mesh::default();
            draw(
                &layout(&f, &form(), 0.05, 0.3),
                Frame::new(Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z, false),
                Colours {
                    petal: [0.9; 3],
                    calyx: CALYX,
                    accent: [0.9, 0.7, 0.1],
                    filament: [0.9, 0.7, 0.1],
                    anther: [0.9, 0.7, 0.1],
                    ovary: [0.9, 0.7, 0.1],
                    base: None,
                    base_length: 0.4,
                    star: None,
                    stripe: None,
                    back: None,
                    back_base: None,
                    back_base_length: 0.4,
                    tube: None,
                    stigma: [0.1, 0.5, 0.1],
                    pericarpel: CALYX,
                    dress: crate::fruit::Dress::default(),
                },
                Part {
                    born: 1.0,
                    shed: None,
                    coarse: 0,
                },
                &mut mesh,
            );
            mesh
        };
        let naked = drawn(Skin::Naked);
        let [scaly, areolate, spiny] =
            [Skin::Scaly, Skin::Areolate, Skin::Spiny].map(|skin| drawn(skin).triangle_count());
        assert!(scaly > naked.triangle_count());
        assert!(spiny > areolate && areolate > naked.triangle_count());
        assert!(
            naked
                .colors
                .iter()
                .any(|c| (c[0] - 0.1).abs() < 1e-6 && (c[1] - 0.5).abs() < 1e-6),
            "the stigma lobes are green"
        );
    }

    #[test]
    fn a_zygomorphic_frame_faces_its_bearing_axis() {
        let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::new(0.3, 1.0, 0.0), Vec3::Z, true);
        assert!((frame.side - Vec3::Y).length() < 1e-12);
        let regular = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z, false);
        assert!((regular.side - Vec3::Z).length() < 1e-12);
    }

    #[test]
    fn the_card_paints_the_banner_up_and_sepals_toward_the_calyx() {
        let pea = flower(|f| {
            f.corolla = Corolla::Papilionaceous;
            f.sepals = 5;
        });
        let slots = ([0.9, 0.8, 0.1], [0.2, 0.3, 0.1]);
        let up = paint(&pea, slots, 0.0, 0.6);
        assert!(up.cover > 0.0, "the banner stands up");
        let trillium = flower(|f| {
            f.petals = 3;
            f.sepals = 3;
            f.sepal_length = 1.0;
            f.petal_width = 0.6;
            f.calyx = Some([0.2, 0.3, 0.1]);
        });
        // Down, between two petals, a sepal.
        let between = paint(&trillium, ([1.0; 3], [0.2, 0.3, 0.1]), 0.0, -0.6);
        assert!(between.cover > 0.0 && between.accent > 0.5, "{between:?}");
    }

    #[test]
    fn a_tepal_foot_takes_its_own_colour_and_the_nectaries_a_star() {
        let lily = Flower {
            perianth: Perianth::Tepals,
            petals: 6,
            centre: 0.08,
            ..Flower::default()
        };
        let draw_with = |base: Option<[f32; 3]>, star: Option<[f32; 3]>| {
            let mut mesh = Mesh::default();
            draw(
                &layout(&lily, &FlowerForm::default(), 0.05, 0.0),
                Frame::new(Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z, false),
                Colours {
                    petal: [0.9, 0.2, 0.1],
                    calyx: CALYX,
                    accent: [0.3, 0.1, 0.1],
                    filament: [0.5, 0.7, 0.3],
                    anther: [0.3, 0.1, 0.1],
                    ovary: [0.5, 0.7, 0.3],
                    base,
                    base_length: 0.4,
                    star,
                    stripe: None,
                    back: None,
                    back_base: None,
                    back_base_length: 0.4,
                    tube: None,
                    stigma: [0.5, 0.7, 0.3],
                    pericarpel: CALYX,
                    dress: crate::fruit::Dress::default(),
                },
                Part {
                    born: 1.0,
                    shed: None,
                    coarse: 0,
                },
                &mut mesh,
            );
            mesh
        };
        let has = |mesh: &Mesh, colour: [f32; 3]| {
            mesh.colors
                .iter()
                .any(|c| (0..3).all(|i| (c[i] - colour[i]).abs() < 1e-4))
        };
        let yellow = [0.9, 0.8, 0.1];
        let green = [0.2, 0.5, 0.1];
        let plain = draw_with(None, None);
        assert!(!has(&plain, yellow) && !has(&plain, green));
        let two_toned = draw_with(Some(yellow), None);
        assert!(has(&two_toned, yellow), "the foot in its own colour");
        assert!(has(&two_toned, [0.9, 0.2, 0.1]), "the tips in the petal's");
        let starred = draw_with(Some(yellow), Some(green));
        assert!(has(&starred, green), "the nectaries' star at the very foot");
        assert_eq!(plain.positions.len(), two_toned.positions.len());
    }

    #[test]
    fn a_tepal_takes_its_stripe_and_its_outer_colours() {
        let lily = |stripe_length: f64| Flower {
            perianth: Perianth::Tepals,
            petals: 6,
            centre: 0.08,
            stripe_length,
            ..Flower::default()
        };
        let (yellow, red, green) = ([0.9, 0.8, 0.1], [0.6, 0.05, 0.02], [0.2, 0.5, 0.1]);
        let draw_with = |flower: &Flower, colours: Colours| {
            let mut mesh = Mesh::default();
            draw(
                &layout(flower, &FlowerForm::default(), 0.05, 0.0),
                Frame::new(Vec3::ZERO, Vec3::Y, Vec3::X, Vec3::Z, false),
                colours,
                Part {
                    born: 1.0,
                    shed: None,
                    coarse: 0,
                },
                &mut mesh,
            );
            mesh
        };
        let colours = Colours {
            petal: [0.9, 0.5, 0.6],
            calyx: CALYX,
            accent: [0.3, 0.1, 0.1],
            filament: [0.5, 0.7, 0.3],
            anther: [0.3, 0.1, 0.1],
            ovary: [0.5, 0.7, 0.3],
            base: None,
            base_length: 0.4,
            star: None,
            stripe: Some(yellow),
            back: None,
            back_base: None,
            back_base_length: 0.4,
            tube: None,
            stigma: [0.5, 0.7, 0.3],
            pericarpel: CALYX,
            dress: crate::fruit::Dress::default(),
        };
        let has = |mesh: &Mesh, colour: [f32; 3]| {
            mesh.colors
                .iter()
                .filter(|c| (0..3).all(|i| (c[i] - colour[i]).abs() < 1e-4))
                .count()
        };
        let plain = draw_with(&lily(0.0), colours);
        let striped = draw_with(&lily(0.5), colours);
        // Five pairs of corners down each of the six tepals, one face.
        assert_eq!(has(&plain, yellow), 0);
        assert_eq!(has(&striped, yellow), 6 * 10);
        assert_eq!(
            striped.triangle_count() - plain.triangle_count(),
            6 * 8,
            "four bands of two triangles each"
        );
        let outside = draw_with(
            &lily(0.0),
            Colours {
                back: Some(red),
                back_base: Some(green),
                ..colours
            },
        );
        assert!(has(&outside, red) > 0, "the outer face in its colour");
        assert!(has(&outside, green) > 0, "and its foot in another");
        assert_eq!(plain.positions.len(), outside.positions.len());
    }

    #[test]
    fn free_tepals_in_a_bell_widen_to_overlap() {
        assert!((bell_width(30.0, false, 6.0, (0.01, 0.05, 0.01)) - 1.0).abs() < 1e-12);
        assert!((bell_width(60.0, true, 6.0, (0.01, 0.05, 0.01)) - 1.0).abs() < 1e-12);
        let wide = bell_width(60.0, false, 6.0, (0.002, 0.05, 0.008));
        assert!(wide > 1.0, "{wide}");
        assert!(
            wide * 0.008 <= 0.6 * 0.05 + 1e-12,
            "no wider than 3/5 of its length"
        );
    }

}
