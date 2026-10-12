//! Spikelets, the units of grass and sedge inflorescences (spec rework
//! P3; LANGUAGE.md 5.2.1 and 5.4).
//!
//! A [`Spikelet`] is laid out by one function, [`layout`], in its own
//! frame: `y` along the spikelet from its foot, `x` across it in the plane
//! its bracts stand in, `z` square to that plane; lengths are shares of
//! the spikelet's length $`\ell`$ (the organ's size), awns left out. The
//! solid ([`draw`]), the reduced spikelet a coarser part mesh draws and the
//! card painted face on ([`paint`]) all come from it, so the card is the
//! solid seen square to its plane.
//!
//! A spikelet is a short axis, the rachilla, bearing bracts in two ranks
//! (`ranks` 2, grasses: each bract on the other side from the one below)
//! or in a spiral (`ranks` 0, many sedges: each turned a golden angle
//! from the one below):
//!
//! - **glumes**, `glumes` empty bracts at its foot: the upper one
//!   `glume` long, the lower `lower` times that, both leaning half the
//!   gape;
//! - **florets**, `florets` of them up the rachilla, each shown by its
//!   lemma, `lemma` long, the lowest from $`y_0 = 0.06`$ and the rest
//!   $`(1 - y_0 - L)/(n - 1)`$ apart for the lemma's length $`L`$, so the
//!   top one's tip reaches 1, each leaning `gape` degrees outward;
//! - **awns** on the upper `awned` share of the florets, `awn` long, from
//!   the lemma's keel `awn_from` of its length below its tip (0 from the
//!   tip, more from its back), leaning out of the lemma a little more the
//!   lower they leave it, and bent `bent` degrees outward at a knee 0.4 of
//!   their length along (a geniculate awn).
//!
//! The layout is then scaled so that its top, the tip of its highest
//! glume or lemma, is 1.
//!
//! **Bracts.** Each glume and lemma is a boat folded along its keel: from
//! its foot along its direction $`\mathbf d`$, widest 0.45 of its length
//! along, where its keel stands $`0.75\,w`$ out from its midline toward
//! its side and its margins $`0.2\,w`$ in, for half the spikelet's width
//! $`w`$ (`aspect` / 2), the margins folded $`0.35\,w`$ either side of the
//! spikelet's plane ($`0.8\,w`$ in a spiral, where scales wrap round the
//! axis). Seen square to the plane, a bract is the kite of its foot, keel,
//! tip and margin; seen edge on, the kite of its foot, margins and tip.
//!
//! **Drawing.** The solid draws each bract as its boat seen from both
//! sides, glumes in the look's colour turned half to its accent, lemmas
//! in its colour, and each awn as a thin three-sided prism in the lemma's
//! colour darkened. A part mesh one step coarser draws each bract as a
//! flat kite and each awn straight; two steps a cluster's reduced
//! spikelet: one kite round its bracts. The card paints the layout square
//! to its plane: lemmas from the lowest up, the glumes over them, the
//! awns over everything.

use crate::blooms::Part;
use crate::capitula::{Frame, Painted, both};
use crate::looks::Spikelet;
use crate::math::{self, Vec3};
use crate::mesh::Mesh;

/// The golden angle, radians.
const GOLDEN: f64 = 2.399_963_229_728_653;

/// Where the lowest floret stands on the rachilla, a share of the
/// spikelet's length.
const FIRST: f64 = 0.06;

/// What a bract is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bract {
    Glume,
    Lemma,
}

/// One glume or lemma: a boat from its foot to its tip, widest at
/// `widest`, its keel and margin there seen face on, its margins folded
/// `flank` either side of the spikelet's plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub bract: Bract,
    pub foot: Vec3,
    pub tip: Vec3,
    pub keel: Vec3,
    pub margin: Vec3,
    pub flank: Vec3,
}

/// One awn: from where it leaves its lemma, to its knee, to its tip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Awn {
    pub from: Vec3,
    pub knee: Vec3,
    pub tip: Vec3,
}

/// A spikelet laid out in its own frame (see the module).
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    /// Glumes first, then the lemmas from the lowest up.
    pub scales: Vec<Scale>,
    pub awns: Vec<Awn>,
    /// The rachilla's top.
    pub top: f64,
}

impl Laid {
    /// How far the spikelet reaches across its plane, either way, awns
    /// included, as a share of its length.
    #[must_use]
    pub fn reach(&self) -> f64 {
        let scales = self
            .scales
            .iter()
            .flat_map(|s| [s.keel.x, s.margin.x, s.tip.x]);
        let awns = self.awns.iter().flat_map(|a| [a.knee.x, a.tip.x]);
        scales.chain(awns).fold(0.0_f64, |far, x| far.max(x.abs()))
    }

    /// How far its glumes and lemmas reach across its plane, either way,
    /// as a share of its length.
    #[must_use]
    pub fn body(&self) -> f64 {
        self.scales
            .iter()
            .flat_map(|s| [s.keel.x, s.margin.x, s.tip.x])
            .fold(0.0_f64, |far, x| far.max(x.abs()))
    }

    /// How far it reaches along its length, awns included.
    #[must_use]
    pub fn height(&self) -> f64 {
        let awns = self.awns.iter().flat_map(|a| [a.knee.y, a.tip.y]);
        awns.fold(1.0, f64::max)
    }
}

/// The side bract `j` (counted from the foot, glumes included) faces: in
/// two ranks alternately −x and +x, in a spiral turned a golden angle from
/// the last.
fn side_of(spikelet: &Spikelet, j: u32) -> Vec3 {
    if spikelet.ranks == 0 {
        let angle = f64::from(j) * GOLDEN;
        Vec3::new(math::cos(angle), 0.0, math::sin(angle))
    } else if j.is_multiple_of(2) {
        -Vec3::X
    } else {
        Vec3::X
    }
}

/// A bract from `foot`, `length` long, leaning `gape` degrees toward
/// `side`, for half the spikelet's width `half`.
fn scale(
    spikelet: &Spikelet,
    bract: Bract,
    foot: Vec3,
    side: Vec3,
    (length, gape): (f64, f64),
    half: f64,
) -> Scale {
    let lean = math::radians(gape);
    let direction = Vec3::Y * math::cos(lean) + side * math::sin(lean);
    // Square to its direction, in the plane of its direction and side:
    // toward its side.
    let out = side * math::cos(lean) - Vec3::Y * math::sin(lean);
    let widest = foot + direction * (length * 0.45);
    let square = side.cross(Vec3::Y).normalize_or(Vec3::Z);
    let fold = if spikelet.ranks == 0 { 0.8 } else { 0.35 };
    Scale {
        bract,
        foot,
        tip: foot + direction * length,
        keel: widest + out * (half * 0.75),
        margin: widest - out * (half * 0.2),
        flank: square * (half * fold),
    }
}

/// `spikelet` laid out in its own frame (see the module).
#[must_use]
pub fn layout(spikelet: &Spikelet) -> Laid {
    let florets = spikelet.florets.max(1);
    let lemma = spikelet.lemma.clamp(0.05, 1.0);
    let half = 0.5 * spikelet.aspect.max(0.02);
    let gape = spikelet.gape.clamp(0.0, 89.0);
    let step = if florets > 1 {
        (1.0 - FIRST - lemma).max(0.0) / f64::from(florets - 1)
    } else {
        0.0
    };
    let glumes = spikelet.glumes.min(2);
    let mut scales = Vec::with_capacity((glumes + florets) as usize);
    for g in 0..glumes {
        // A lone glume is the upper one.
        let length = if g + 1 == glumes {
            spikelet.glume
        } else {
            spikelet.glume * spikelet.lower
        };
        if length <= 0.0 {
            continue;
        }
        scales.push(scale(
            spikelet,
            Bract::Glume,
            Vec3::Y * (0.02 * f64::from(g)),
            side_of(spikelet, g),
            (length, gape * 0.5),
            half,
        ));
    }
    for k in 0..florets {
        scales.push(scale(
            spikelet,
            Bract::Lemma,
            Vec3::Y * (FIRST + step * f64::from(k)),
            side_of(spikelet, glumes + k),
            (lemma, gape),
            half,
        ));
    }
    // Scale so that the highest tip reaches 1.
    let top = scales
        .iter()
        .map(|s| s.tip.y.max(s.keel.y))
        .fold(1e-9, f64::max);
    for s in &mut scales {
        for p in [
            &mut s.foot,
            &mut s.tip,
            &mut s.keel,
            &mut s.margin,
            &mut s.flank,
        ] {
            *p = *p / top;
        }
    }
    let awns = awns(spikelet, &scales);
    Laid {
        top: (FIRST + step * f64::from(florets - 1)) / top,
        scales,
        awns,
    }
}

/// The awns of `scales`' upper lemmas (see the module).
fn awns(spikelet: &Spikelet, scales: &[Scale]) -> Vec<Awn> {
    if spikelet.awn <= 0.0 {
        return Vec::new();
    }
    let lemmas: Vec<&Scale> = scales.iter().filter(|s| s.bract == Bract::Lemma).collect();
    let count = lemmas.len();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let awned = ((spikelet.awned.clamp(0.0, 1.0) * count as f64).round() as usize).max(1);
    let from = spikelet.awn_from.clamp(0.0, 0.9);
    lemmas[count - awned.min(count)..]
        .iter()
        .map(|s| {
            // Along the keel: from the tip down to the keel's widest, then
            // to the foot.
            let along = 1.0 - from;
            let start = if along >= 0.45 {
                s.keel.lerp(s.tip, (along - 0.45) / 0.55)
            } else {
                s.foot.lerp(s.keel, along / 0.45)
            };
            let heading = (s.tip - s.foot).normalize_or(Vec3::Y);
            let out = (s.keel - s.margin).normalize_or(Vec3::X);
            let tilt = math::radians(6.0 + 30.0 * from);
            let first = (heading * math::cos(tilt) + out * math::sin(tilt)).normalize_or(heading);
            let knee = start + first * (spikelet.awn * 0.4);
            // The bristle beyond the knee, bent outward.
            let bend = math::radians(spikelet.bent.clamp(0.0, 150.0));
            let square = first.cross(out).normalize_or(Vec3::Z);
            let beyond = first.rotate_about(square, -bend);
            let beyond = if beyond.dot(out) < first.dot(out) {
                first.rotate_about(square, bend)
            } else {
                beyond
            };
            Awn {
                from: start,
                knee,
                tip: knee + beyond * (spikelet.awn * 0.6),
            }
        })
        .collect()
}

/// A spikelet's colours in the solid: its glumes', its lemmas' and its
/// awns'.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colours {
    pub glumes: [f32; 3],
    pub lemmas: [f32; 3],
    pub awns: [f32; 3],
}

impl Colours {
    /// The colours of a look of `colour` and `accent`: lemmas in its
    /// colour, glumes turned half to its accent, awns darker.
    #[must_use]
    pub fn of(colour: [f32; 3], accent: [f32; 3]) -> Self {
        Self {
            glumes: std::array::from_fn(|i| f32::midpoint(colour[i], accent[i])),
            lemmas: colour,
            awns: colour.map(|c| c * 0.85),
        }
    }
}

/// Awn thickness, a share of the spikelet's length.
const AWN_WIDTH: f64 = 0.012;

/// A thin three-sided prism from `from` to `to`, `radius` thick.
fn prism(part: Part, from: Vec3, to: Vec3, radius: f64, colour: [f32; 3], mesh: &mut Mesh) {
    let along = (to - from).normalize_or(Vec3::Y);
    let across = math::any_perpendicular(along);
    let other = along.cross(across);
    let leaf = part.leaf(from, along, across, 0.0, 0.0, colour);
    let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for end in [from, to] {
        for k in 0..3 {
            let angle = math::radians(120.0 * f64::from(k));
            let around = across * math::cos(angle) + other * math::sin(angle);
            mesh.push(leaf.vertex(end + around * radius, colour));
        }
    }
    for k in 0..3 {
        let (a0, a1) = (at + k, at + (k + 1) % 3);
        mesh.indices
            .extend_from_slice(&[a0, a1, a0 + 3, a1, a1 + 3, a0 + 3]);
    }
}

/// Draw `spikelet` in `frame` (its `radius` the spikelet's length): at
/// `part.coarse` 0 every bract as its boat and every awn bent at its
/// knee, at 1 bracts as flat kites and awns straight, from 2 one kite
/// round its bracts and its longest awn (see the module).
pub(crate) fn draw(
    spikelet: &Spikelet,
    frame: Frame,
    colours: Colours,
    part: Part,
    mesh: &mut Mesh,
) {
    if frame.radius <= 0.0 {
        return;
    }
    let laid = layout(spikelet);
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    let colour_of = |s: &Scale| match s.bract {
        Bract::Glume => colours.glumes,
        Bract::Lemma => colours.lemmas,
    };
    if part.coarse >= 2 {
        // One kite round every bract.
        let reach = laid
            .scales
            .iter()
            .flat_map(|s| [s.keel.x, s.margin.x])
            .fold(0.0_f64, |far, x| far.max(x.abs()))
            .max(spikelet.aspect * 0.25);
        let at = 0.45 * laid.top.max(0.3);
        both(
            part,
            &[
                frame.point(Vec3::ZERO),
                frame.point(Vec3::new(reach, at, 0.0)),
                frame.point(Vec3::Y),
                frame.point(Vec3::new(-reach, at, 0.0)),
            ],
            colours.lemmas,
            mesh,
        );
        if let Some(awn) = laid.awns.iter().max_by(|a, b| a.tip.y.total_cmp(&b.tip.y)) {
            prism(
                part,
                frame.point(awn.from),
                frame.point(awn.tip),
                AWN_WIDTH * frame.radius,
                colours.awns,
                mesh,
            );
        }
        return;
    }
    for s in &laid.scales {
        let colour = colour_of(s);
        if part.coarse == 0 {
            let plus = frame.point(s.margin + s.flank);
            let minus = frame.point(s.margin - s.flank);
            let (foot, keel, tip) = (frame.point(s.foot), frame.point(s.keel), frame.point(s.tip));
            both(part, &[foot, keel, tip, plus], colour, mesh);
            both(part, &[foot, minus, tip, keel], colour, mesh);
        } else {
            both(
                part,
                &[
                    frame.point(s.foot),
                    frame.point(s.keel),
                    frame.point(s.tip),
                    frame.point(s.margin),
                ],
                colour,
                mesh,
            );
        }
    }
    let radius = AWN_WIDTH * frame.radius;
    for awn in &laid.awns {
        let (from, knee, tip) = (
            frame.point(awn.from),
            frame.point(awn.knee),
            frame.point(awn.tip),
        );
        if part.coarse == 0 {
            prism(part, from, knee, radius, colours.awns, mesh);
            prism(part, knee, tip, radius * 0.7, colours.awns, mesh);
        } else {
            prism(part, from, tip, radius, colours.awns, mesh);
        }
    }
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// A spikelet's sizes as its characters measure them (spec rework M8B),
/// each a share of its drawn length (to its highest tip, awns left out):
/// its length to the tip of its last floret, its upper and lower glumes,
/// its lemma and its awn, each 0 where it has none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimensions {
    pub length: f64,
    pub upper_glume: f64,
    pub lower_glume: f64,
    pub lemma: f64,
    pub awn: f64,
}

/// `spikelet`'s [`Dimensions`], from its layout.
#[must_use]
pub fn dimensions(spikelet: &Spikelet) -> Dimensions {
    let laid = layout(spikelet);
    let long = |s: &Scale| (s.tip - s.foot).length();
    let glumes: Vec<f64> = laid
        .scales
        .iter()
        .filter(|s| s.bract == Bract::Glume)
        .map(long)
        .collect();
    // A lone glume is the upper; of two, the lower comes first.
    let (lower, upper) = match glumes.as_slice() {
        [] => (0.0, 0.0),
        [upper] => (0.0, *upper),
        [lower, upper, ..] => (*lower, *upper),
    };
    let lemmas = laid.scales.iter().filter(|s| s.bract == Bract::Lemma);
    Dimensions {
        length: lemmas
            .clone()
            .map(|s| s.tip.y.max(s.keel.y))
            .fold(0.0, f64::max),
        upper_glume: upper,
        lower_glume: lower,
        lemma: lemmas.map(long).fold(0.0, f64::max),
        awn: laid
            .awns
            .iter()
            .map(|a| (a.knee - a.from).length() + (a.tip - a.knee).length())
            .fold(0.0, f64::max),
    }
}

/// Triangles [`draw`] takes for `spikelet` at `coarse`.
#[must_use]
pub fn triangles(spikelet: &Spikelet, coarse: usize) -> usize {
    let mut mesh = Mesh::default();
    draw(
        spikelet,
        Frame {
            foot: Vec3::ZERO,
            axis: Vec3::Y,
            side: Vec3::X,
            radius: 1.0,
        },
        Colours::of([1.0; 3], [1.0; 3]),
        Part {
            born: 0.0,
            shed: None,
            coarse,
        },
        &mut mesh,
    );
    mesh.triangle_count()
}

/// Whether `p` is inside the triangle `a`, `b`, `c` (either winding).
fn inside(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let side = |u: (f64, f64), v: (f64, f64)| (v.0 - u.0) * (p.1 - u.1) - (v.1 - u.1) * (p.0 - u.0);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// The distance from `point` to the segment `from` to `to`.
fn to_segment(point: (f64, f64), from: (f64, f64), to: (f64, f64)) -> f64 {
    let along = (to.0 - from.0, to.1 - from.1);
    let length = along.0 * along.0 + along.1 * along.1;
    let share = if length > 1e-18 {
        (((point.0 - from.0) * along.0 + (point.1 - from.1) * along.1) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let off = (
        from.0 + along.0 * share - point.0,
        from.1 + along.1 * share - point.1,
    );
    math::sqrt(off.0 * off.0 + off.1 * off.1)
}

/// The spikelet's card at `(x, y)`, in shares of its length, seen square
/// to its plane, lines at least `fine` wide: lemmas in its colour, glumes
/// half in its accent, awns over them (see the module).
#[must_use]
pub fn paint(spikelet: &Spikelet, x: f64, y: f64, fine: f64) -> Painted {
    thread_local! {
        static LAID: std::cell::RefCell<Option<(Spikelet, Laid)>> =
            const { std::cell::RefCell::new(None) };
    }
    LAID.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_none_or(|(s, _)| s != spikelet) {
            *cache = Some((spikelet.clone(), layout(spikelet)));
        }
        let Some((_, laid)) = cache.as_ref() else {
            return Painted::EMPTY;
        };
        painted(laid, x, y, fine)
    })
}

fn painted(laid: &Laid, x: f64, y: f64, fine: f64) -> Painted {
    let p = (x, y);
    let flat = |v: Vec3| (v.x, v.y);
    for awn in &laid.awns {
        if to_segment(p, flat(awn.from), flat(awn.knee)) < fine * 0.6
            || to_segment(p, flat(awn.knee), flat(awn.tip)) < fine * 0.5
        {
            return Painted {
                cover: 1.0,
                bright: 0.85,
                accent: 0.0,
            };
        }
    }
    // Glumes over the lemmas, the higher lemmas over the lower.
    let mut order: Vec<&Scale> = laid
        .scales
        .iter()
        .filter(|s| s.bract == Bract::Glume)
        .collect();
    order.extend(laid.scales.iter().filter(|s| s.bract == Bract::Lemma).rev());
    for s in order {
        let (foot, keel, tip, margin) = (flat(s.foot), flat(s.keel), flat(s.tip), flat(s.margin));
        let outer = inside(p, foot, keel, tip);
        if outer || inside(p, foot, tip, margin) {
            // Lit along the keel, darker toward the margin.
            let bright = if outer { 1.05 } else { 0.85 };
            return Painted {
                cover: 1.0,
                bright,
                accent: if s.bract == Bract::Glume { 0.5 } else { 0.0 },
            };
        }
    }
    Painted::EMPTY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fescue() -> Spikelet {
        Spikelet {
            florets: 5,
            glume: 0.45,
            lemma: 0.4,
            awn: 0.2,
            ..Spikelet::default()
        }
    }

    #[test]
    fn florets_climb_the_rachilla_in_two_ranks_above_the_glumes() {
        let laid = layout(&fescue());
        let glumes: Vec<&Scale> = laid
            .scales
            .iter()
            .filter(|s| s.bract == Bract::Glume)
            .collect();
        let lemmas: Vec<&Scale> = laid
            .scales
            .iter()
            .filter(|s| s.bract == Bract::Lemma)
            .collect();
        assert_eq!((glumes.len(), lemmas.len()), (2, 5));
        // The lower glume shorter than the upper.
        let length = |s: &Scale| (s.tip - s.foot).length();
        assert!(length(glumes[0]) < length(glumes[1]));
        // Each lemma higher than the last, on the other side.
        for pair in lemmas.windows(2) {
            assert!(pair[1].foot.y > pair[0].foot.y);
            assert!(pair[1].keel.x * pair[0].keel.x < 0.0);
        }
        // The top reaches 1.
        let top = laid.scales.iter().map(|s| s.tip.y).fold(0.0, f64::max);
        assert!((top - 1.0).abs() < 1e-9, "{top}");
        // Every lemma bears an awn.
        assert_eq!(laid.awns.len(), 5);
        assert!(laid.height() > 1.0);
    }

    #[test]
    fn long_glumes_set_the_length_and_a_lone_glume_is_the_upper() {
        let reed = Spikelet {
            florets: 1,
            glume: 1.0,
            lemma: 0.7,
            ..Spikelet::default()
        };
        let laid = layout(&reed);
        let upper = laid.scales[1];
        assert!((upper.tip.y - 1.0).abs() < 0.05, "{upper:?}");
        let lone = layout(&Spikelet {
            glumes: 1,
            ..reed.clone()
        });
        assert_eq!(lone.scales[0].bract, Bract::Glume);
        assert!((lone.scales[0].tip - lone.scales[0].foot).length() > 0.9);
        let bare = layout(&Spikelet { glumes: 0, ..reed });
        assert!(bare.scales.iter().all(|s| s.bract == Bract::Lemma));
    }

    #[test]
    fn only_the_upper_florets_are_awned_and_a_bent_awn_turns_out() {
        let holcus = Spikelet {
            florets: 2,
            awn: 0.3,
            awned: 0.5,
            bent: 60.0,
            awn_from: 0.2,
            ..Spikelet::default()
        };
        let laid = layout(&holcus);
        assert_eq!(laid.awns.len(), 1);
        let awn = laid.awns[0];
        let lemma = laid.scales.last().unwrap();
        // From the upper lemma's back, below its tip.
        assert!(awn.from.y < lemma.tip.y);
        let first = (awn.knee - awn.from).normalize_or(Vec3::Y);
        let second = (awn.tip - awn.knee).normalize_or(Vec3::Y);
        let turned = math::degrees(math::acos(first.dot(second).clamp(-1.0, 1.0)));
        assert!((turned - 60.0).abs() < 1.0, "{turned}");
        // Outward: the bristle leans further toward the lemma's side.
        assert!(second.x * lemma.keel.x.signum() > first.x * lemma.keel.x.signum());
    }

    #[test]
    fn a_spiral_turns_each_scale_a_golden_angle() {
        let bulrush = Spikelet {
            florets: 30,
            glumes: 0,
            ranks: 0,
            lemma: 0.25,
            aspect: 0.4,
            ..Spikelet::default()
        };
        let laid = layout(&bulrush);
        let angle = |s: &Scale| {
            let out = s.keel - s.margin;
            math::atan2(out.z, out.x)
        };
        let turn =
            (angle(&laid.scales[1]) - angle(&laid.scales[0])).rem_euclid(std::f64::consts::TAU);
        assert!((turn - GOLDEN).abs() < 1e-6, "{turn}");
    }

    #[test]
    fn each_step_draws_fewer_triangles() {
        let s = fescue();
        let fine = triangles(&s, 0);
        let kites = triangles(&s, 1);
        let reduced = triangles(&s, 2);
        // Seven bracts as boats (eight triangles each) and five awns of
        // two prisms (six each).
        assert_eq!(fine, 7 * 8 + 5 * 12);
        assert_eq!(kites, 7 * 4 + 5 * 6);
        assert_eq!(reduced, 4 + 6);
    }

    #[test]
    fn the_card_paints_glumes_lemmas_and_awns() {
        let s = fescue();
        let laid = layout(&s);
        // The lower glume's middle, at its keel side.
        let glume = laid.scales[0];
        let at = glume.foot.lerp(glume.keel, 0.6);
        let paint = paint(&s, at.x, at.y, 0.01);
        assert!(
            paint.cover > 0.5 && (paint.accent - 0.5).abs() < 1e-9,
            "{paint:?}"
        );
        // An awn's tip.
        let awn = laid.awns[4];
        let paint = super::paint(&s, awn.tip.x, awn.tip.y, 0.01);
        assert!(paint.cover > 0.5 && paint.accent < 1e-9);
        // Far to the side, nothing.
        assert!(super::paint(&s, 2.0, 0.5, 0.01).cover < 1e-12);
    }
}
