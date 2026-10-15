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
//! `throat` $`\cdot L`$ over `fused` $`\cdot L`$, its petals free lobes on
//! its mouth. Up to $`2n`$ stamens stand in whorls at $`0.5c`$ and
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
use crate::looks::{Corolla, Flower, Ovary, Perianth};
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
/// The `open` below which a flower's stamens and pistil are hidden.
const SHOWN: f64 = 0.3;

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
}

/// The flower `flower` drawn with `form`, `radius` metres in radius, a
/// regular one turned `spin` radians.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn layout(flower: &Flower, form: &FlowerForm, radius: f64, spin: f64) -> Floral {
    let r = radius.max(0.0);
    let c = (flower.centre * r).clamp(0.0, r * 0.9);
    let s = 0.6 * c;
    let n = flower.petals.max(1);
    let nf = f64::from(n);
    let length = (r - s).max(r * 0.1);
    let spread = |count: f64, from: f64, to: f64| flower.petal_width * PI * (from + to) / count;
    let width = spread(nf, s, r).clamp(length * 0.08, length);
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
    let (seat, cup, ovary) = match flower.ovary {
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
                s.max(0.12 * ovary_length),
            )),
        ),
    };

    // Sepals, between the petals.
    let mut pieces = Vec::new();
    let mut calyx_tube = None;
    if flower.perianth == Perianth::Corolla
        && flower.sepals > 0
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
                cup: rise(form.cup * 0.5),
                length: tooth,
                width: (spread(m, s, r) * 0.8).clamp(tooth * 0.08, tooth.max(1e-9)),
                curl: curl * 0.5,
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
        length: flower.fused * length,
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
        (Perianth::Corolla, Corolla::Regular) if flower.petal_whorls == 2 => {
            // Two like whorls, as tepals stand, the inner turned half a
            // petal and a little narrower and more upright.
            let h = f64::from((n / 2).max(1));
            for k in 0..(n / 2).max(1) {
                let angle = TAU * f64::from(k) / h;
                pieces.push(piece(Term::Petal, None, angle, form.cup * 0.85, (1.0, 1.0)));
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
            let h = f64::from((n / 2).max(1));
            for k in 0..(n / 2).max(1) {
                let angle = TAU * f64::from(k) / h;
                pieces.push(piece(Term::Tepal, None, angle, form.cup * 0.85, (1.0, 1.0)));
            }
            for k in 0..(n / 2).max(1) {
                let angle = TAU * f64::from(k) / h + PI / h;
                pieces.push(piece(Term::Tepal, None, angle, form.cup, (1.0, 0.85)));
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
            Corolla::Papilionaceous => false,
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

    // Stamens and staminodes.
    let shown = open >= SHOWN;
    let hidden = matches!(flower.corolla, Corolla::Papilionaceous | Corolla::Labellate);
    let mut stamens = Vec::new();
    if shown && !hidden {
        let total = flower.stamens + flower.staminodes;
        let whorl = match flower.perianth {
            Perianth::Corolla => (n / flower.petal_whorls.max(1)).max(1),
            Perianth::Tepals => (n / 2).max(1),
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
            let (height, at, lean) = match &tube {
                // Adnate to the tube, halfway up.
                Some(t) => (t.height + t.length * 0.5, t.radius_at(0.5) * 0.85, 0.15),
                None => (seat, at, LEAN),
            };
            stamens.push(Stamen {
                rod: Rod {
                    angle: psi + angle,
                    radius: at,
                    height,
                    lean: math::atan(lean),
                    length,
                    width: length * 0.03,
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
        let style = flower.stamen_length * r;
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

    Floral {
        radius: r,
        centre: c,
        cup,
        ovary,
        calyx_tube,
        pieces,
        tube,
        spurs,
        stamens,
        pistil,
        form_cup: form.cup,
        notch: flower.notch,
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
    /// The filaments' and the style's; the anthers stay in the accent.
    pub filament: [f32; 3],
    /// A single pistil's ovary.
    pub ovary: [f32; 3],
}

impl Colours {
    /// The colour `share` of the way from the petals' to the calyx's.
    fn toward_calyx(&self, share: f32) -> [f32; 3] {
        std::array::from_fn(|c| self.petal[c] + (self.calyx[c] - self.petal[c]) * share)
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
            colours.calyx,
            part,
            mesh,
        );
    }
    for tube in [&laid.calyx_tube, &laid.tube].into_iter().flatten() {
        let round = if fine { 4 } else { 2 };
        revolve(
            tube,
            frame,
            (tube.lobes * round).clamp(8, 96),
            if fine { 6 } else { 3 },
            colours.toward_calyx(tube.calyx),
            part,
            mesh,
        );
    }
    let (most_stations, most_across) = if fine { (6, 2) } else { (4, 1) };
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
        let solid = SolidLeaf {
            section: Section::Flat,
            thickness: 0.05,
            fold: piece.fold,
            widest: 0.55,
            base: piece.base,
            taper: piece.taper,
            tip: 0.0,
            arch: piece.curl,
            twist: 0.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 1,
        };
        let leaf = part.leaf(
            frame.point(piece.angle, piece.radius, piece.height),
            heading,
            left,
            piece.length,
            piece.width,
            colours.toward_calyx(piece.calyx),
        );
        // A petal is seen close: on the nearest step it takes every
        // station and both samples across, however short it is, so its
        // outline curves rather than breaking into a few facets.
        let (stations, across) = if fine {
            (most_stations, most_across)
        } else {
            leaves::detail_of(0, piece.length, piece.width)
        };
        leaf.draw(
            &solid,
            stations.min(most_stations),
            across.min(most_across),
            mesh,
        );
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
                colours.accent,
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
                    rod_cone(tip, out, &lobe, colours.filament, part, mesh);
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
                    colours.accent,
                    part,
                    mesh,
                );
            }
            for pistil in pistils {
                let (foot, direction) = frame.rod(pistil);
                rod_cone(foot, direction, pistil, colours.accent, part, mesh);
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

/// A flower reduced for a cluster's unit, or a lone flower's part mesh
/// drawn coarse (`part.coarse` counted from the reduced flower): its
/// perianth parts as kites seen from both sides (one facet each from the
/// first coarse step), its tubes as `n`-sided bands, a low pyramid of a
/// centre or a column in the accent, and stamens while the flower is a
/// centimetre across or more and drawn fine; from the second coarse step
/// one hexagon round its petal tips.
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
        let (ring, rise, paint) = if petals.is_empty() {
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
            let corners: Vec<Vec3> = (0..6)
                .map(|k| frame.point(TAU * f64::from(k) / 6.0, ring, rise))
                .collect();
            both(&corners, paint, mesh);
        }
        leaves::smooth_normals(mesh, first, first_index);
        return;
    }
    for tube in [&laid.calyx_tube, &laid.tube].into_iter().flatten() {
        let sides = tube.lobes.max(3);
        let colour = colours.toward_calyx(tube.calyx);
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
        let left = axis.cross(radial).normalize_or(frame.side) * (piece.width * 0.5);
        let foot = frame.point(piece.angle, piece.radius, piece.height);
        let colour = colours.toward_calyx(piece.calyx);
        if part.coarse == 0 {
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
    if part.coarse <= 1 && laid.radius * 2.0 >= 0.01 {
        for stamen in laid.stamens.iter().take(12) {
            let (foot, direction) = frame.rod(&stamen.rod);
            let across =
                direction.cross(axis).normalize_or(frame.side) * (stamen.rod.length * 0.06);
            both(
                &[
                    foot - across,
                    foot + across,
                    foot + direction * stamen.rod.length,
                ],
                colours.filament,
                mesh,
            );
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
            ovary: [1.0; 3],
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
                        ovary: [0.9, 0.7, 0.1],
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
}
