//! Flower heads, capitula (spec rework P2; LANGUAGE.md 5.2.1 and 5.4).
//!
//! A [`Head`] is laid out by one function, [`layout`], in the head's own
//! frame: `y` along the organ (the head's axis), `x` and `z` across it,
//! lengths in units of the head's radius $`R`$ (half the organ's size).
//! The solid drawn on the nearest levels ([`draw`]), the reduced head a
//! cluster draws for each of its units ([`draw_reduced`]) and the card
//! painted face on ([`paint`]) all come from it, so the card is the solid
//! seen from above.
//!
//! With $`R_d`$ = `disc`, $`H = 2 R_d \cdot`$ `involucre` and
//! $`h = R_d \cdot`$ `receptacle`:
//!
//! - the **involucre** runs from a foot ring of radius $`0.3 R_d`$ at the
//!   head's foot up to the disc's rim, radius $`R_d`$ at height $`H`$,
//!   along $`r(t) = 0.3 R_d + 0.7 R_d \sin(\pi t / 2)`$, $`y(t) = H t`$.
//!   Its `phyllaries` stand in `series` rows round it, the outer rows
//!   shorter: row $`k`$ of $`S`$ (0 the outermost) reaches
//!   $`t_k = 0.5 + 0.5 (k + 1)/S`$ and stands $`0.03 R_d (S - 1 - k)`$
//!   outside the inner ones, each phyllary a kite widest 0.55 of its way,
//!   its width 1.35 times its share of the circle there, so neighbours
//!   overlap, and its upper part turned `spread` degrees outward about its
//!   widest point (an everlasting's dry bracts open round its disc);
//! - the **receptacle** is the dome $`y(\rho) = H + h (1 - \rho^2/R_d^2)`$
//!   over the disc;
//! - its **florets** sit on it in a sunflower spiral (Vogel 1979): floret
//!   $`i`$ of $`n`$ at radius $`0.97 R_d \sqrt{(i + \tfrac12)/n}`$, turned
//!   $`i`$ golden angles, each standing along the receptacle's normal, its
//!   spacing $`s = 0.97 R_d \sqrt{\pi / n}`$, an open one $`0.9 s`$ tall, or
//!   $`R_d \cdot`$ `floret_length` where that is set (a thistle's brush),
//!   a bud $`0.54 s`$;
//! - on a **radiate** head the `rays` stand round the rim, from
//!   $`\rho_0 = 0.92 R_d`$ at height $`H`$, raised by the form's cup, each
//!   $`1 - \rho_0`$ long and $`\min(\ell,\ w \pi (\rho_0 + 1) / n)`$ wide
//!   for the template's `ray_width` $`w`$, as a daisy head's rays were
//!   before P2;
//! - on a **ligulate** head every floret is a strap from its place out to
//!   $`\rho + (1 - \rho)(0.3 + 0.7 q)`$, $`q = (i + \tfrac12)/n`$, raised
//!   $`(1 - q) \cdot 50°`$ more than the cup, so the inner straps are
//!   shorter and stand up, each as wide as $`w`$ times the circle at its
//!   middle over the $`2\sqrt{\pi n}`$ straps a ring holds.
//!
//! **Order.** A head opens from its rim inward: each floret has a rank
//! from 0 (the outermost) to 1 (the centre), and the head's `open`
//! $`o`$ says how far it has got. For $`o \le 1`$ a floret is open if its
//! rank is below $`o`$, else a bud; for $`o > 1`$ it has gone to seed if
//! its rank is below $`o - 1`$, else it is open. The rays, the first to
//! open, unfold over the first fifth of $`o`$ and fall once half the disc
//! is in seed. A floret in seed is a cypsela, and with a `pappus` a tuft
//! of bristles $`P = R_d \cdot`$ `pappus` long spreading from its top.
//!
//! **Drawing.** The solid draws each phyllary as a kite seen from both
//! sides in the head's `bracts` colour, the receptacle as a dome in the
//! accent colour, each ray as a thin solid leaf as a petal is drawn
//! (`crate::blooms`), an open disc floret as a five-sided tube with five
//! spreading lobes and a bud as a five-sided point, both in the accent
//! colour, each strap of a ligulate head as a ray is, in the look's
//! colour, cypselae in the accent darkened and the pappus off-white. A part mesh
//! (`crate::parts`) one step coarser draws florets as points and rays as
//! kites; two steps a cluster's reduced head. A cluster draws each of its
//! heads reduced ([`draw_reduced`]): the involucre a six-sided cup, the
//! disc a low pyramid, the rays or the outer straps as kites. The card
//! paints the same layout face on, each part in the look's colour and
//! accent, or the mix of them nearest its own colour where it has another
//! (`crate::clusters::blend`): straps, the pappus of a head half in seed,
//! the disc's florets each a dot, the rays, then the phyllaries where they
//! show, each the kite it is from above.

use crate::blooms::{FlowerForm, Part};
use crate::leaves::{Section, SolidLeaf};
use crate::looks::{Head, HeadKind};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;

/// The golden angle, radians.
const GOLDEN: f64 = 2.399_963_229_728_653;

/// The pappus's colour: off-white.
pub const PAPPUS: [f32; 3] = [0.9, 0.88, 0.82];

/// What one floret is on the day the head is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Bud,
    Open,
    /// A cypsela, with its pappus if the head has one.
    Seed,
}

/// One phyllary: a kite from its foot to its tip, widest at `widest`, its
/// half-width there `half`, square to it round the head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phyllary {
    /// Its row, 0 the outermost.
    pub series: u32,
    pub foot: Vec3,
    pub widest: Vec3,
    pub tip: Vec3,
    pub half: Vec3,
}

/// One ray floret, or one strap of a ligulate head.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strap {
    pub foot: Vec3,
    /// Unit direction from its foot to its tip.
    pub heading: Vec3,
    pub length: f64,
    pub width: f64,
}

/// One floret on the receptacle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floret {
    /// Where it sits seen face on, `(x, z)`.
    pub place: (f64, f64),
    pub base: Vec3,
    /// The receptacle's normal there.
    pub normal: Vec3,
    /// When it opens among the head's florets, 0 (the rim) to 1.
    pub rank: f64,
    pub stage: Stage,
    /// Its strap, on a ligulate head.
    pub strap: Option<Strap>,
}

/// A head laid out in its own frame (see the module).
#[derive(Debug, Clone, PartialEq)]
pub struct Capitulum {
    pub kind: HeadKind,
    /// The disc's radius, its rim's height and the receptacle's height
    /// above the rim at its centre.
    pub disc: f64,
    pub rim: f64,
    pub dome: f64,
    /// Space between neighbouring florets, and how far an open one stands
    /// above the receptacle.
    pub spacing: f64,
    pub stand: f64,
    /// The pappus's length; 0 for none.
    pub pappus: f64,
    pub phyllaries: Vec<Phyllary>,
    /// The rays that show: none in bud, nor once the head is half seed.
    pub rays: Vec<Strap>,
    pub florets: Vec<Floret>,
}

impl Capitulum {
    /// A floret's tube height and radius, open, in its head's units: a
    /// floret that stands out of its head (a thistle's) slenderer.
    #[must_use]
    pub fn tube(&self) -> (f64, f64) {
        let slender = if self.stand > 2.0 * self.spacing {
            0.22
        } else {
            0.32
        };
        (self.stand, self.spacing * slender)
    }

    /// A floret's height in bud.
    #[must_use]
    pub fn bud(&self) -> f64 {
        self.spacing * 0.54
    }

    /// A cypsela's length.
    #[must_use]
    pub fn cypsela(&self) -> f64 {
        self.spacing * 0.8
    }

    /// The highest point of the head above its foot.
    #[must_use]
    pub fn top(&self) -> f64 {
        let florets = self
            .florets
            .iter()
            .map(|floret| match (floret.stage, floret.strap) {
                (Stage::Open, Some(strap)) => (strap.foot + strap.heading * strap.length).y,
                (Stage::Seed, _) => floret.base.y + self.cypsela() + self.pappus,
                (Stage::Bud, _) => floret.base.y + self.bud(),
                _ => floret.base.y + self.tube().0,
            })
            .fold(self.rim + self.dome, f64::max);
        self.rays
            .iter()
            .map(|ray| (ray.foot + ray.heading * ray.length).y)
            .fold(florets, f64::max)
    }
}

/// The share `t` of the way up the involucre: its radius and height.
fn involucre_at(t: f64, disc: f64, rim: f64) -> (f64, f64) {
    (
        disc * (0.3 + 0.7 * math::sin(std::f64::consts::FRAC_PI_2 * t)),
        rim * t,
    )
}

fn radial(angle: f64) -> Vec3 {
    Vec3::new(math::cos(angle), 0.0, math::sin(angle))
}

fn to_f64(n: u32) -> f64 {
    f64::from(n)
}

/// A floret's stage at `rank` in a head opened `open` (see the module).
#[must_use]
pub fn stage(rank: f64, open: f64) -> Stage {
    if open <= 1.0 {
        if open >= 1.0 || rank < open {
            Stage::Open
        } else {
            Stage::Bud
        }
    } else if open >= 2.0 || rank < open - 1.0 {
        Stage::Seed
    } else {
        Stage::Open
    }
}

/// `head` laid out in its own frame, its rays raised `cup` degrees,
/// turned `turn` radians round its axis (see the module).
#[must_use]
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub fn layout(head: &Head, cup: f64, turn: f64) -> Capitulum {
    let disc = head.disc.clamp(0.05, 1.0);
    let rim = 2.0 * disc * head.involucre.max(0.0);
    let dome = disc * head.receptacle.max(0.0);
    let n = head.florets.max(1);
    let nf = to_f64(n);
    let spread = 0.97 * disc;
    let spacing = spread * math::sqrt(std::f64::consts::PI / nf);
    let cup = math::radians(cup);
    let outward_by = math::radians(head.spread.clamp(0.0, 150.0));
    // Phyllaries, row by row from the outermost.
    let mut phyllaries = Vec::new();
    let total = head.phyllaries;
    let series = head.series.clamp(1, total.max(1));
    for k in 0..series {
        let count = total * (k + 1) / series - total * k / series;
        if count == 0 {
            continue;
        }
        let reach = 0.5 + 0.5 * to_f64(k + 1) / to_f64(series);
        let out = 0.03 * disc * to_f64(series - 1 - k);
        let widest_at = 0.55 * reach;
        let (widest_r, widest_y) = involucre_at(widest_at, disc, rim);
        let (tip_r, tip_y) = involucre_at(reach, disc, rim);
        let (foot_r, _) = involucre_at(0.0, disc, rim);
        let half = (1.35 * std::f64::consts::PI * (widest_r + out) / to_f64(count)).min(disc);
        for j in 0..count {
            let angle =
                turn + std::f64::consts::TAU * (to_f64(j) + 0.5 * to_f64(k % 2)) / to_f64(count);
            let out_dir = radial(angle);
            let around = Vec3::Y.cross(out_dir).normalize_or(Vec3::X);
            let widest = out_dir * (widest_r + out) + Vec3::Y * widest_y;
            let mut tip = out_dir * (tip_r + out) + Vec3::Y * tip_y;
            if outward_by > 0.0 {
                // The upper half turned outward about the phyllary's
                // widest point.
                let upper = tip - widest;
                let along = upper.normalize_or(Vec3::Y);
                let outward = (out_dir - along * along.dot(out_dir)).normalize_or(out_dir);
                tip = widest
                    + (along * math::cos(outward_by) + outward * math::sin(outward_by))
                        * upper.length();
            }
            phyllaries.push(Phyllary {
                series: k,
                foot: out_dir * (foot_r + out),
                widest,
                tip,
                half: around * half,
            });
        }
    }
    let open = head.open.clamp(0.0, 2.0);
    // Rays: unfolding over the first fifth of the opening, gone once half
    // the disc is seed.
    let mut rays = Vec::new();
    if head.kind == HeadKind::Radiate && head.rays > 0 && open > 0.0 && open < 1.5 {
        let count = to_f64(head.rays);
        let foot = 0.92 * disc;
        let length = (1.0 - foot).max(0.05) * (open / 0.2).min(1.0);
        let width = (head.ray_width * std::f64::consts::PI * (foot + 1.0) / count)
            .clamp(length * 0.08, length.max(1e-6));
        for k in 0..head.rays {
            let angle = turn + std::f64::consts::TAU * to_f64(k) / count;
            let out = radial(angle);
            rays.push(Strap {
                foot: out * foot + Vec3::Y * rim,
                heading: (out * math::cos(cup) + Vec3::Y * math::sin(cup)).normalize_or(out),
                length,
                width,
            });
        }
    }
    // Florets on the receptacle, the outermost last.
    let ring = 2.0 * math::sqrt(std::f64::consts::PI * nf);
    let florets = (0..n)
        .map(|i| {
            let fi = to_f64(i);
            let q = (fi + 0.5) / nf;
            let rho = spread * math::sqrt(q);
            let angle = turn + fi * GOLDEN;
            let out = radial(angle);
            let lift = rim + dome * (1.0 - (rho / disc).powi(2));
            let base = out * rho + Vec3::Y * lift;
            let normal = (Vec3::Y + out * (2.0 * dome * rho / (disc * disc))).normalize_or(Vec3::Y);
            let rank = if n > 1 { 1.0 - fi / (nf - 1.0) } else { 0.0 };
            let stage = stage(rank, open);
            let strap = (head.kind == HeadKind::Ligulate).then(|| {
                let foot = 0.9 * disc * math::sqrt(q);
                let length = (1.0 - foot) * (0.3 + 0.7 * q);
                let rise = cup + (1.0 - q) * math::radians(50.0);
                let width = (head.ray_width * std::f64::consts::TAU * (foot + 0.5 * length) / ring)
                    .clamp(length * 0.08, length * 0.6);
                Strap {
                    foot: out * foot + Vec3::Y * (rim + dome * (1.0 - (foot / disc).powi(2))),
                    heading: (out * math::cos(rise) + Vec3::Y * math::sin(rise)).normalize_or(out),
                    length,
                    width,
                }
            });
            Floret {
                place: (base.x, base.z),
                base,
                normal,
                rank,
                stage,
                strap,
            }
        })
        .collect();
    Capitulum {
        kind: head.kind,
        disc,
        rim,
        dome,
        spacing,
        stand: if head.floret_length > 0.0 {
            disc * head.floret_length
        } else {
            spacing * 0.9
        },
        pappus: disc * head.pappus.max(0.0),
        phyllaries,
        rays,
        florets,
    }
}

/// The colours a head is drawn in: its rays' or straps' (the look's
/// colour), its disc florets' (the accent) and its phyllaries'.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub rays: [f32; 3],
    pub disc: [f32; 3],
    pub bracts: [f32; 3],
}

impl Colours {
    /// A head's colours: `rays` and `disc` as painted on the organ, its
    /// bracts darkened as the organ's look's colour is to its colour as
    /// painted, `shading` (`crate::blooms::with_shading`).
    #[must_use]
    pub fn of(
        head: &Head,
        rays: [f32; 3],
        disc: [f32; 3],
        (look, painted): ([f32; 3], [f32; 3]),
    ) -> Self {
        let bracts = head.bracts.unwrap_or(BRACTS);
        Self {
            rays,
            disc,
            bracts: crate::blooms::with_shading(bracts, look, painted),
        }
    }

    /// A cypsela's colour: the disc's, darkened toward brown.
    fn seed(self) -> [f32; 3] {
        let brown = [0.36_f32, 0.27, 0.15];
        std::array::from_fn(|c| self.disc[c] * 0.35 + brown[c] * 0.65)
    }

    /// A bud floret's colour: the disc's, a third toward the bracts'.
    fn bud(self) -> [f32; 3] {
        std::array::from_fn(|c| self.disc[c] * 0.65 + self.bracts[c] * 0.35)
    }
}

/// The bracts' colour where neither the head nor the plant sets one: a
/// leaf green.
pub const BRACTS: [f32; 3] = [0.2, 0.32, 0.1];

/// A head's frame in the world: its foot, axis, one direction across and
/// its radius in metres.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) foot: Vec3,
    pub(crate) axis: Vec3,
    pub(crate) side: Vec3,
    pub(crate) radius: f64,
}

impl Frame {
    pub(crate) fn point(&self, p: Vec3) -> Vec3 {
        let other = self.axis.cross(self.side);
        self.foot + (self.side * p.x + self.axis * p.y + other * p.z) * self.radius
    }

    pub(crate) fn direction(&self, d: Vec3) -> Vec3 {
        let other = self.axis.cross(self.side);
        (self.side * d.x + self.axis * d.y + other * d.z).normalize_or(self.axis)
    }
}

/// A flat face seen from both sides, each side its own vertices so their
/// normals are not averaged away.
pub(crate) fn both(part: Part, corners: &[Vec3], colour: [f32; 3], mesh: &mut Mesh) {
    let leaf = part.leaf(corners[0], Vec3::Y, Vec3::X, 0.0, 0.0, colour);
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
}

/// Florets a head draws one by one at most at the finest step: more are
/// drawn as points.
const FINE_FLORETS: usize = 400;

/// Draw `head`, laid out with `form`'s cup, in `frame`, turned `turn`
/// (see the module): at `part.coarse` 0 every part, at 1 its florets as
/// points and its rays as kites, from 2 as a cluster's reduced head.
#[allow(clippy::too_many_lines)]
pub(crate) fn draw(
    head: &Head,
    form: &FlowerForm,
    frame: Frame,
    turn: f64,
    colours: Colours,
    part: Part,
    mesh: &mut Mesh,
) {
    if frame.radius <= 0.0 {
        return;
    }
    if part.coarse >= 2 {
        draw_reduced(head, form, frame, turn, colours, part, mesh);
        return;
    }
    let laid = layout(head, form.cup, turn);
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    for p in &laid.phyllaries {
        both(
            part,
            &[
                frame.point(p.foot),
                frame.point(p.widest + p.half),
                frame.point(p.tip),
                frame.point(p.widest - p.half),
            ],
            colours.bracts,
            mesh,
        );
    }
    crate::blooms::dome(
        frame.point(Vec3::Y * laid.rim),
        frame.axis,
        frame.side,
        laid.disc * frame.radius,
        laid.dome.max(laid.disc * 0.05) * frame.radius,
        if laid.florets.iter().all(|f| f.stage == Stage::Seed) {
            colours.seed()
        } else {
            colours.disc
        },
        part,
        mesh,
    );
    let ray = SolidLeaf {
        section: Section::Flat,
        thickness: 0.05,
        fold: 0.0,
        widest: 0.55,
        base: 0.35,
        // Blunt, as a ray's tip is.
        taper: 0.45,
        arch: form.curl,
        twist: 0.0,
        margin: None,
        teeth: None,
        spine: None,
        levels: 1,
    };
    let strap = |s: &Strap, colour: [f32; 3], mesh: &mut Mesh| {
        let foot = frame.point(s.foot);
        let heading = frame.direction(s.heading);
        let length = s.length * frame.radius;
        let width = s.width * frame.radius;
        // The strap's face toward the head's axis, so its curl bends it
        // outward and back.
        let out = frame.direction(Vec3::new(s.heading.x, 0.0, s.heading.z));
        let mut left = frame.axis.cross(out).normalize_or(frame.side);
        if heading.cross(left).dot(frame.axis) < 0.0 {
            left = -left;
        }
        if part.coarse == 0 {
            let leaf = part.leaf(foot, heading, left, length, width, colour);
            let (stations, across) = crate::leaves::detail_of(0, length, width);
            leaf.draw(&ray, stations.min(6), across.min(2), mesh);
        } else {
            let widest = foot + heading * (length * 0.55);
            let half = left * (width * 0.5);
            both(
                part,
                &[foot, widest + half, foot + heading * length, widest - half],
                colour,
                mesh,
            );
        }
    };
    for s in &laid.rays {
        strap(s, colours.rays, mesh);
    }
    let points = part.coarse > 0 || laid.florets.len() > FINE_FLORETS;
    let (tube, tube_radius) = laid.tube();
    for floret in &laid.florets {
        let base = frame.point(floret.base);
        let normal = frame.direction(floret.normal);
        let across = any_perpendicular(normal);
        let other = normal.cross(across);
        let ring = |radius: f64, k: u32, sides: u32| {
            let angle = std::f64::consts::TAU * f64::from(k) / f64::from(sides);
            (across * math::cos(angle) + other * math::sin(angle)) * radius
        };
        match (floret.stage, floret.strap) {
            (Stage::Open, Some(s)) => strap(&s, colours.rays, mesh),
            (Stage::Seed, _) => {
                seed(
                    &laid,
                    base,
                    normal,
                    (across, other),
                    frame.radius,
                    colours,
                    part,
                    mesh,
                );
            }
            (stage, _) => {
                let colour = if stage == Stage::Bud {
                    colours.bud()
                } else {
                    colours.disc
                };
                let radius = tube_radius * frame.radius;
                let height = if stage == Stage::Bud {
                    laid.bud()
                } else {
                    tube
                } * frame.radius;
                let leaf = part.leaf(base, normal, across, 0.0, 0.0, colour);
                if points || stage == Stage::Bud {
                    // A five-sided point.
                    let apex = mesh.push(leaf.vertex(base + normal * height, colour));
                    let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
                    for k in 0..5 {
                        mesh.push(leaf.vertex(base + ring(radius, k, 5), colour));
                    }
                    for k in 0..5 {
                        mesh.indices
                            .extend_from_slice(&[apex, at + k, at + (k + 1) % 5]);
                    }
                    continue;
                }
                // A five-sided tube and five lobes spreading from its top.
                let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
                for end in [0.0, height] {
                    for k in 0..5 {
                        mesh.push(leaf.vertex(base + normal * end + ring(radius, k, 5), colour));
                    }
                }
                for k in 0..5 {
                    let (a0, a1) = (at + k, at + (k + 1) % 5);
                    let (b0, b1) = (a0 + 5, a1 + 5);
                    mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
                }
                let top = base + normal * height;
                for k in 0..5 {
                    let a = top + ring(radius, k, 5);
                    let b = top + ring(radius, k + 1, 5);
                    let tip = top + ring(radius * 2.1, 2 * k + 1, 10) + normal * (radius * 0.4);
                    let at = mesh.push(leaf.vertex(a, colour));
                    mesh.push(leaf.vertex(tip, colour));
                    mesh.push(leaf.vertex(b, colour));
                    mesh.indices.extend_from_slice(&[at, at + 1, at + 2]);
                }
            }
        }
    }
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// One floret gone to seed: a cypsela along `normal` from `base`, and its
/// pappus spreading from its top.
#[allow(clippy::too_many_arguments)]
fn seed(
    laid: &Capitulum,
    base: Vec3,
    normal: Vec3,
    (across, other): (Vec3, Vec3),
    scale: f64,
    colours: Colours,
    part: Part,
    mesh: &mut Mesh,
) {
    let length = laid.cypsela() * scale;
    let leaf = part.leaf(base, normal, across, 0.0, 0.0, colours.seed());
    crate::leaves::cone(
        &leaf,
        base,
        normal,
        across,
        other,
        length,
        laid.spacing * 0.2 * scale,
        colours.seed(),
        mesh,
    );
    if laid.pappus <= 0.0 {
        return;
    }
    let top = base + normal * length;
    let bristles: u32 = if part.coarse == 0 { 5 } else { 3 };
    let reach = laid.pappus * scale;
    let spread = math::radians(50.0);
    for k in 0..bristles {
        let angle = std::f64::consts::TAU * f64::from(k) / f64::from(bristles);
        let out = across * math::cos(angle) + other * math::sin(angle);
        let direction = (normal * math::cos(spread) + out * math::sin(spread)).normalize_or(normal);
        let side = normal.cross(out).normalize_or(across) * (reach * 0.05);
        both(
            part,
            &[top - side, top + side, top + direction * reach],
            PAPPUS,
            mesh,
        );
    }
}

/// A head drawn small, as a cluster draws each of its heads: the involucre
/// a six-sided cup in the bracts' colour, the disc a low six-sided pyramid
/// in the accent (in seed, cypselae brown, a pappus a white star), the
/// rays, or a ligulate head's outer ring of straps, as kites; one facet
/// each from `part.coarse` 1.
#[allow(clippy::too_many_lines)]
pub(crate) fn draw_reduced(
    head: &Head,
    form: &FlowerForm,
    frame: Frame,
    turn: f64,
    colours: Colours,
    part: Part,
    mesh: &mut Mesh,
) {
    let laid = layout(head, form.cup, turn);
    let r = frame.radius;
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    let ring = |radius: f64, height: f64, k: u32| {
        let angle = turn + std::f64::consts::TAU * f64::from(k) / 6.0;
        frame.point(radial(angle) * radius + Vec3::Y * height)
    };
    // The involucre's cup, seen from both sides.
    if !laid.phyllaries.is_empty() && laid.rim > 0.0 {
        let (foot, _) = involucre_at(0.0, laid.disc, laid.rim);
        for k in 0..6 {
            both(
                part,
                &[
                    ring(foot, 0.0, k),
                    ring(foot, 0.0, k + 1),
                    ring(laid.disc, laid.rim, k + 1),
                    ring(laid.disc, laid.rim, k),
                ],
                colours.bracts,
                mesh,
            );
        }
    }
    let seeding = laid
        .florets
        .iter()
        .filter(|f| f.stage == Stage::Seed)
        .count()
        * 2
        > laid.florets.len();
    let open = laid
        .florets
        .iter()
        .filter(|f| f.stage == Stage::Open)
        .count();
    let disc_colour = if seeding {
        colours.seed()
    } else if open * 2 < laid.florets.len() {
        colours.bud()
    } else {
        colours.disc
    };
    // The disc: a low pyramid over the rim.
    let leaf = part.leaf(frame.foot, frame.axis, frame.side, 0.0, 0.0, disc_colour);
    let apex = mesh.push(leaf.vertex(
        frame.point(Vec3::Y * (laid.rim + laid.dome.max(laid.disc * 0.25))),
        disc_colour,
    ));
    let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for k in 0..6 {
        mesh.push(leaf.vertex(ring(laid.disc, laid.rim, k), disc_colour));
    }
    for k in 0..6 {
        mesh.indices
            .extend_from_slice(&[apex, at + k, at + (k + 1) % 6]);
    }
    if seeding && laid.pappus > 0.0 {
        // The pappus: a six-pointed star over the disc, seen from both
        // sides.
        let reach = laid.disc + laid.pappus;
        let height = laid.rim + laid.dome + laid.pappus * 0.6;
        for k in 0..6 {
            both(
                part,
                &[
                    frame.point(Vec3::Y * height),
                    ring(reach, height - laid.pappus * 0.3, k),
                    ring(reach * 0.6, height, k),
                ],
                PAPPUS,
                mesh,
            );
        }
    }
    // Rays, or the outermost straps of a ligulate head.
    let straps: Vec<Strap> = if laid.kind == HeadKind::Ligulate {
        let count = laid.florets.len();
        // A ring's worth of straps: about 2 √(π n) of the n.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let outer = (2.0 * math::sqrt(std::f64::consts::PI * to_f64(head.florets.max(1)))) as usize;
        laid.florets[count.saturating_sub(outer.max(3))..]
            .iter()
            .filter(|f| f.stage == Stage::Open)
            .filter_map(|f| f.strap)
            .collect()
    } else {
        laid.rays.clone()
    };
    for s in &straps {
        let foot = frame.point(s.foot);
        let heading = frame.direction(s.heading);
        let out = frame.direction(Vec3::new(s.heading.x, 0.0, s.heading.z));
        let half = frame.axis.cross(out).normalize_or(frame.side) * (s.width * r * 0.5);
        let length = s.length * r;
        if part.coarse == 0 {
            let widest = foot + heading * (length * 0.55);
            both(
                part,
                &[foot, widest + half, foot + heading * length, widest - half],
                colours.rays,
                mesh,
            );
        } else {
            let end = foot + heading * (length * 0.85);
            both(part, &[foot, end + half, end - half], colours.rays, mesh);
        }
    }
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// What the card paints at `(x, y)`, in units of the head's radius, the
/// head seen face on: its cover, its brightness and how far toward the
/// accent it is, the look's colour and accent being `slots`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Painted {
    pub cover: f64,
    pub bright: f64,
    pub accent: f64,
}

impl Painted {
    pub(crate) const EMPTY: Self = Self {
        cover: 0.0,
        bright: 0.0,
        accent: 0.0,
    };

    /// `colour` as near as the two slots make it, `bright` times as bright.
    fn of(colour: [f32; 3], slots: ([f32; 3], [f32; 3]), bright: f64) -> Self {
        let (a, b) = crate::clusters::blend(colour, slots);
        let sum = a + b;
        if sum <= 1e-9 {
            return Self {
                cover: 1.0,
                bright: 0.3 * bright,
                accent: 0.0,
            };
        }
        Self {
            cover: 1.0,
            bright: (sum * bright).min(2.0),
            accent: b / sum,
        }
    }
}

/// Paints a head's card face on (see the module).
#[derive(Debug, Clone, PartialEq)]
pub struct Painter {
    head: Head,
    laid: Capitulum,
    bracts: [f32; 3],
    slots: ([f32; 3], [f32; 3]),
}

impl Painter {
    /// A painter of `head`'s card, its look's colour and accent `slots`.
    #[must_use]
    pub fn new(head: &Head, slots: ([f32; 3], [f32; 3])) -> Self {
        Self {
            head: head.clone(),
            laid: layout(head, 8.0, 0.0),
            bracts: head.bracts.unwrap_or(BRACTS),
            slots,
        }
    }

    /// The head at `(x, y)`, units of its radius from its centre.
    #[must_use]
    pub fn paint(&self, x: f64, y: f64) -> Painted {
        let laid = &self.laid;
        let r = math::sqrt(x * x + y * y);
        if r > 1.0 {
            return Painted::EMPTY;
        }
        let colours = Colours {
            rays: self.slots.0,
            disc: self.slots.1,
            bracts: self.bracts,
        };
        // Straps of a ligulate head, the innermost (highest) on top.
        if laid.kind == HeadKind::Ligulate {
            for floret in &laid.florets {
                let (Stage::Open, Some(s)) = (floret.stage, floret.strap) else {
                    continue;
                };
                let (dx, dz) = (s.heading.x, s.heading.z);
                let flat = math::sqrt(dx * dx + dz * dz).max(1e-6);
                let (ux, uz) = (dx / flat, dz / flat);
                let (px, py) = (x - s.foot.x, y - s.foot.z);
                let along = (px * ux + py * uz) / (s.length * flat);
                let across = (-px * uz + py * ux).abs();
                if !(0.0..=1.0).contains(&along) {
                    continue;
                }
                let half = s.width * 0.5 * strap_width(along);
                if across < half {
                    return Painted {
                        cover: 1.0,
                        bright: 0.86 + 0.14 * along + 0.06 * (1.0 - across / half.max(1e-9)),
                        accent: 0.0,
                    };
                }
            }
        }
        // The pappus of florets in seed, a soft disc of radiating hairs.
        if laid.pappus > 0.0 {
            let seeds = laid
                .florets
                .iter()
                .filter(|f| f.stage == Stage::Seed)
                .count();
            if seeds * 2 >= laid.florets.len() {
                let reach = laid.disc + laid.pappus * 0.85;
                if r < reach {
                    let angle = math::atan2(y, x);
                    let streak = 0.5 + 0.5 * math::cos(angle * 47.0 + r * 31.0);
                    if streak > 0.25 || r < laid.disc * 0.5 {
                        return Painted::of(PAPPUS, self.slots, 0.92 + 0.12 * streak);
                    }
                }
            }
        }
        if r < laid.disc {
            return self.disc((x, y), r, colours);
        }
        // Rays.
        if !laid.rays.is_empty() {
            let count = self.head.rays.max(1);
            let share = std::f64::consts::TAU / f64::from(count);
            let phi = math::atan2(y, x);
            let delta = phi - (phi / share).round() * share;
            let (along, across) = (r * math::cos(delta), r * math::sin(delta));
            let ray = laid.rays[0];
            let start = math::sqrt(ray.foot.x * ray.foot.x + ray.foot.z * ray.foot.z) * 0.92;
            let reach = start + ray.length * math::cos(math::radians(8.0));
            let u = (along - start) / (reach - start).max(1e-6);
            if (0.0..=1.0).contains(&u) {
                let mut width = math::sin(std::f64::consts::PI / f64::from(count))
                    * self.head.ray_width
                    * (1.0 - 0.2 * u)
                    * reach;
                if u > 0.85 {
                    width *= math::sqrt((1.0 - ((u - 0.85) / 0.15).powi(2)).max(0.0));
                }
                if across.abs() < width {
                    let groove = if across.abs() < width * 0.12 {
                        -0.06
                    } else {
                        0.0
                    };
                    return Painted {
                        cover: 1.0,
                        bright: 0.94 + 0.1 * (1.0 - across.abs() / width.max(1e-6)) + groove,
                        accent: 0.0,
                    };
                }
            }
        }
        // The phyllaries seen from above, where the disc and the rays
        // leave them showing: the inner rows the brighter.
        let rows = laid.phyllaries.last().map_or(1, |last| last.series + 1);
        for p in laid.phyllaries.iter().rev() {
            let corners = [p.foot, p.widest + p.half, p.tip, p.widest - p.half];
            let flat = corners.map(|c| (c.x, c.z));
            if inside((x, y), flat[0], flat[1], flat[2])
                || inside((x, y), flat[0], flat[2], flat[3])
            {
                let inner = to_f64(p.series + 1) / to_f64(rows);
                return Painted::of(self.bracts, self.slots, 0.72 + 0.22 * inner);
            }
        }
        Painted::EMPTY
    }

    /// The disc at `(x, y)`, `radius` from its centre: the nearest floret, a
    /// bright dot where it is open, darker in bud, brown in seed.
    fn disc(&self, (x, y): (f64, f64), radius: f64, colours: Colours) -> Painted {
        let laid = &self.laid;
        let count = laid.florets.len();
        if count == 0 {
            return Painted::of(colours.disc, self.slots, 0.85);
        }
        // Florets in a sunflower spiral within a ring's width of radius
        // `radius`: the indices near n r² / ρ², the spiral's spread ρ.
        let spread = 0.97 * laid.disc;
        #[allow(clippy::cast_precision_loss)]
        let many = count as f64;
        let centre = (many * (radius / spread).powi(2) - 0.5).clamp(0.0, many - 1.0);
        let window = 3.0 * math::sqrt(many) + 4.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (low, high) = (
            (centre - window).max(0.0) as usize,
            ((centre + window) as usize).min(count - 1),
        );
        let mut nearest = (f64::MAX, 0);
        for i in low..=high {
            let (px, pz) = laid.florets[i].place;
            let d = (x - px).powi(2) + (y - pz).powi(2);
            if d < nearest.0 {
                nearest = (d, i);
            }
        }
        let floret = &laid.florets[nearest.1];
        let d = math::sqrt(nearest.0) / laid.spacing;
        let shading = 0.85 + 0.15 * radius / laid.disc.max(1e-9);
        match floret.stage {
            Stage::Open => Painted {
                cover: 1.0,
                bright: if d < 0.4 { 1.1 } else { 0.78 } * shading,
                accent: 1.0,
            },
            Stage::Bud => {
                let mut paint = Painted::of(colours.bud(), self.slots, shading);
                if d < 0.3 {
                    paint.bright *= 1.08;
                }
                paint
            }
            Stage::Seed => Painted::of(
                colours.seed(),
                self.slots,
                if d < 0.3 { 1.0 } else { 0.8 } * shading,
            ),
        }
    }
}

/// Whether `p` is inside the triangle `a`, `b`, `c` (either winding).
fn inside(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let side = |u: (f64, f64), v: (f64, f64)| (v.0 - u.0) * (p.1 - u.1) - (v.1 - u.1) * (p.0 - u.0);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// The card of `head` at `(x, y)`, as [`Painter::paint`] paints it: the
/// last few painters are kept, as a card paints thousands of points of
/// one head.
#[must_use]
#[allow(clippy::float_cmp)]
pub fn paint(head: &Head, slots: ([f32; 3], [f32; 3]), x: f64, y: f64) -> Painted {
    thread_local! {
        static PAINTERS: std::cell::RefCell<Vec<Painter>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    PAINTERS.with(|painters| {
        let mut painters = painters.borrow_mut();
        let index = painters
            .iter()
            .position(|painter| painter.head == *head && painter.slots == slots)
            .unwrap_or_else(|| {
                if painters.len() >= 8 {
                    painters.remove(0);
                }
                painters.push(Painter::new(head, slots));
                painters.len() - 1
            });
        painters[index].paint(x, y)
    })
}

/// A strap's half-width along it, as a share of its widest: narrow at its
/// foot, widest past its middle, rounded at its tip.
fn strap_width(u: f64) -> f64 {
    let rising = (u / 0.15).min(1.0);
    let tip = if u > 0.85 {
        math::sqrt((1.0 - ((u - 0.85) / 0.15).powi(2)).max(0.0))
    } else {
        1.0
    };
    (0.45 + 0.55 * rising) * tip
}

/// A head's sizes as its characters measure them (spec rework M8B), each a
/// share of the head's width (its organ's size, twice its radius $`R`$):
/// the involucre's ring of phyllaries across and up to its inner tips, a
/// ray's strap fully unfolded, the pappus and a cypsela, each 0 where the
/// head has none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimensions {
    pub involucre_diameter: f64,
    pub involucre_height: f64,
    pub ray_length: f64,
    pub pappus_length: f64,
    /// A cypsela, the head's fruit.
    pub cypsela: f64,
}

/// `head`'s [`Dimensions`], from its layout fully open.
#[must_use]
pub fn dimensions(head: &Head) -> Dimensions {
    let open = Head {
        open: 1.0,
        ..head.clone()
    };
    let laid = layout(&open, 0.0, 0.0);
    // The ring of phyllaries across its widest points and tips, and up to
    // the tips of the innermost row; the disc's rim where it has none.
    let across = laid
        .phyllaries
        .iter()
        .flat_map(|p| [p.widest, p.tip])
        .map(|point| math::sqrt(point.x * point.x + point.z * point.z))
        .fold(laid.disc, f64::max);
    let inner = laid.phyllaries.iter().map(|p| p.series).max();
    let up = laid
        .phyllaries
        .iter()
        .filter(|p| Some(p.series) == inner)
        .map(|p| p.tip.y)
        .fold(laid.rim, f64::max);
    // The layout's lengths are in R, half the head's width.
    Dimensions {
        involucre_diameter: across,
        involucre_height: 0.5 * up,
        ray_length: 0.5 * laid.rays.first().map_or(0.0, |ray| ray.length),
        pappus_length: 0.5 * laid.pappus,
        cypsela: 0.5 * laid.cypsela(),
    }
}

/// Triangles one head takes drawn solid `coarse` steps coarser.
#[must_use]
pub fn triangles(head: &Head, form: &FlowerForm, coarse: usize) -> usize {
    let mut mesh = Mesh::default();
    draw(
        head,
        form,
        Frame {
            foot: Vec3::ZERO,
            axis: Vec3::Y,
            side: Vec3::X,
            radius: 0.5,
        },
        0.0,
        Colours {
            rays: [1.0; 3],
            disc: [1.0; 3],
            bracts: BRACTS,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn daisy() -> Head {
        Head {
            rays: 21,
            florets: 300,
            phyllaries: 30,
            series: 3,
            ..Head::default()
        }
    }

    #[test]
    fn florets_fill_the_disc_in_a_spiral_and_open_from_the_rim() {
        let laid = layout(&daisy(), 8.0, 0.0);
        assert_eq!(laid.florets.len(), 300);
        for floret in &laid.florets {
            let (x, z) = floret.place;
            assert!(math::sqrt(x * x + z * z) <= laid.disc + 1e-9, "on the disc");
            assert!(floret.base.y >= laid.rim - 1e-9, "on the receptacle");
        }
        // The outermost florets open first.
        let outer = laid.florets.last().unwrap();
        let inner = &laid.florets[0];
        assert!(outer.rank < 1e-9 && (inner.rank - 1.0).abs() < 1e-9);
        assert_eq!(outer.stage, Stage::Open);
        assert_eq!(inner.stage, Stage::Bud);
        let open = laid
            .florets
            .iter()
            .filter(|f| f.stage == Stage::Open)
            .count();
        assert!((170..=190).contains(&open), "about 0.6 open: {open}");
        // The outermost stand near the rim.
        let (x, z) = outer.place;
        assert!(math::sqrt(x * x + z * z) > 0.9 * laid.disc);
        // Their spacing covers the disc: area per floret π ρ² / n.
        let expected = 0.97 * laid.disc * (std::f64::consts::PI / 300.0).sqrt();
        assert!((laid.spacing - expected).abs() < 1e-12);
    }

    #[test]
    fn stages_follow_the_opening() {
        assert_eq!(stage(0.5, 0.0), Stage::Bud);
        assert_eq!(stage(0.0, 0.0), Stage::Bud);
        assert_eq!(stage(0.3, 0.5), Stage::Open);
        assert_eq!(stage(0.7, 0.5), Stage::Bud);
        assert_eq!(stage(1.0, 1.0), Stage::Open);
        assert_eq!(stage(0.3, 1.5), Stage::Seed);
        assert_eq!(stage(0.7, 1.5), Stage::Open);
        assert_eq!(stage(1.0, 2.0), Stage::Seed);
        // Rays: none in bud, none once half the disc is seed.
        let head = |open| Head { open, ..daisy() };
        assert!(layout(&head(0.0), 8.0, 0.0).rays.is_empty());
        assert_eq!(layout(&head(0.6), 8.0, 0.0).rays.len(), 21);
        assert!(layout(&head(1.6), 8.0, 0.0).rays.is_empty());
    }

    #[test]
    fn phyllaries_stand_in_rows_the_outer_shorter() {
        let laid = layout(&daisy(), 8.0, 0.0);
        assert_eq!(laid.phyllaries.len(), 30);
        let tip = |series: u32| {
            laid.phyllaries
                .iter()
                .filter(|p| p.series == series)
                .map(|p| p.tip.y)
                .fold(0.0, f64::max)
        };
        assert!(tip(0) < tip(1) && tip(1) < tip(2));
        assert!(
            (tip(2) - laid.rim).abs() < 1e-9,
            "the inner row reaches the rim"
        );
        for p in &laid.phyllaries {
            assert!(p.foot.y.abs() < 1e-12 && p.tip.y <= laid.rim + 1e-9);
        }
    }

    #[test]
    fn a_ligulate_head_is_all_straps_the_inner_shorter() {
        let head = Head {
            kind: HeadKind::Ligulate,
            rays: 0,
            florets: 80,
            open: 1.0,
            ..Head::default()
        };
        let laid = layout(&head, 8.0, 0.0);
        assert!(laid.rays.is_empty());
        assert!(laid.florets.iter().all(|f| f.strap.is_some()));
        let length = |f: &Floret| f.strap.unwrap().length;
        assert!(length(&laid.florets[0]) < length(laid.florets.last().unwrap()));
        let reach = |f: &Floret| {
            let s = f.strap.unwrap();
            let tip = s.foot + s.heading * s.length;
            math::sqrt(tip.x * tip.x + tip.z * tip.z)
        };
        assert!(laid.florets.iter().all(|f| reach(f) <= 1.0 + 1e-9));
        assert!(crate::looks::Shape::Head(head.clone()).validate().is_ok());
        let rayed = Head { rays: 5, ..head };
        assert!(crate::looks::Shape::Head(rayed).validate().is_err());
    }

    #[test]
    fn a_solid_head_stays_within_its_reach_and_draws_coarser_at_each_step() {
        let mut counts = Vec::new();
        for coarse in 0..4 {
            counts.push(triangles(&daisy(), &FlowerForm::default(), coarse));
        }
        assert!(counts[0] > counts[1] && counts[1] > counts[2], "{counts:?}");
        assert!(counts[0] < 8_000, "{counts:?}");
        let mut mesh = Mesh::default();
        draw(
            &daisy(),
            &FlowerForm::default(),
            Frame {
                foot: Vec3::ZERO,
                axis: Vec3::Y,
                side: Vec3::X,
                radius: 0.05,
            },
            0.3,
            Colours {
                rays: [1.0; 3],
                disc: [1.0, 0.8, 0.1],
                bracts: BRACTS,
            },
            Part {
                born: 1.0,
                shed: None,
                coarse: 0,
            },
            &mut mesh,
        );
        for p in &mesh.positions {
            let (x, z) = (f64::from(p[0]), f64::from(p[2]));
            let radial = math::sqrt(x * x + z * z);
            assert!(radial <= 0.0505, "within the head's radius: {p:?}");
            assert!(p[1] >= -1e-6, "above its foot: {p:?}");
        }
    }

    #[test]
    fn seeds_carry_their_pappus() {
        let head = Head {
            kind: HeadKind::Ligulate,
            rays: 0,
            florets: 40,
            open: 2.0,
            pappus: 1.5,
            ..Head::default()
        };
        let laid = layout(&head, 8.0, 0.0);
        assert!(laid.florets.iter().all(|f| f.stage == Stage::Seed));
        assert!(laid.top() > laid.rim + laid.pappus);
        let bare = Head {
            pappus: 0.0,
            ..head.clone()
        };
        assert!(
            triangles(&head, &FlowerForm::default(), 0)
                > triangles(&bare, &FlowerForm::default(), 0)
        );
        // The card paints the clock off-white, as near as two slots allow.
        let painter = Painter::new(&head, ([0.9, 0.88, 0.82], [0.3, 0.2, 0.1]));
        let paint = painter.paint(0.1, 0.05);
        assert!(paint.cover > 0.5 && paint.accent < 0.1, "{paint:?}");
    }

    #[test]
    fn the_card_paints_rays_disc_and_rim() {
        let painter = Painter::new(&daisy(), ([1.0; 3], [0.9, 0.7, 0.1]));
        let disc = painter.paint(0.0, 0.1);
        assert!(disc.cover > 0.5 && disc.accent > 0.5, "{disc:?}");
        // A ray along +x.
        let ray = painter.paint(0.7, 0.0);
        assert!(ray.cover > 0.5 && ray.accent < 0.5, "{ray:?}");
        assert!(painter.paint(1.2, 0.0).cover < 1e-12);
    }
}
