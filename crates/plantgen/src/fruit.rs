//! Fruit of every kind, singly or in clusters (plant roadmap P4).
//!
//! A [`Fruit`] template's `kind` says what one fruit is, and its `count`,
//! `cluster`, `size` and `spread` how several sit on one organ. The solid
//! drawn on the nearest level ([`draw`]) and the card drawn for the farther
//! ones (`crate::templates`) both come from one [`layout`], so the card is
//! the solid seen face on.
//!
//! In the organ's frame, with lengths in units of the organ's length, `y`
//! along the organ, `x` across its card and `z` out of it, a cluster of
//! $`n`$ fruit $`s`$ long (`size`) whose stalks lean $`\sigma`$ (`spread`)
//! from its axis, each turned the golden angle $`\psi \approx 137.5°`$ from
//! the last, sets fruit $`i`$, with $`q_i = (i + \tfrac12)/n`$:
//!
//! - in a **raceme**, on the axis at $`y_i = (1 - r)(0.1 + 0.9\,q_i)`$, on a
//!   stalk $`0.9\,s`$ long leaning $`\sigma`$, where
//!   $`r = \min(1.9\,s\cos\sigma,\ 0.9)`$ keeps the top fruit within the
//!   organ;
//! - in an **umbel**, on a stalk from $`y_0 = 0.3\,(1 - s)`$ leaning
//!   $`\alpha_i = \sigma\sqrt{q_i}`$ (a Vogel spiral in the cone of
//!   half-angle $`\sigma`$),
//!   $`\max\big((1 - y_0 - \tfrac{w}{2}\sin\alpha_i)/\cos\alpha_i - s,\ 0.3\,s\big)`$
//!   long, so the fruit reach the organ's length and spread flat;
//! - in a **pair**, two fruit from one point leaning $`\pm\sigma`$ in one
//!   plane, on a stalk $`s/4`$ long, and further pairs below, each turned
//!   90° from the last: pair $`p`$ of $`m`$ at
//!   $`y = (1 - r)\,\tfrac{p + 1}{m}`$ with $`r = \min(1.25\,s\cos\sigma,\ 0.95)`$;
//! - in a **head**, packed round a centre on a Fibonacci sphere, pointing
//!   out, their bases on a sphere of radius
//!   $`\max\big(\tfrac{w}{2}\sqrt{0.35\,n} - 0.3\,s,\ 0.02\big)`$ ($`w`$ one
//!   fruit's width) whose top fruit reach 1, on one stalk from the foot.
//!
//! A cluster's own axis (its rachis, or the stalk under an umbel, a pair or
//! a head) runs from the organ's foot to its highest stalk. A single fruit
//! fills the organ, as before. A fruit is unripe, and drawn
//! in the look's accent colour, when a hash of the organ and its index
//! falls below `unripe`; stalks, and the cup or husk of a nut, take the
//! accent colour too.
//!
//! One fruit of length $`\ell`$ and width $`w = a\ell`$ (the template's
//! aspect) is, from its base along its direction:
//!
//! | Kind | Solid |
//! | --- | --- |
//! | berry | an ellipsoid $`\ell \times w`$ |
//! | crowned | the berry with five calyx lobes round its tip |
//! | pome | a rounder ellipsoid with a dark eye at its tip |
//! | drupelets | a core under up to 30 small drupelets on a Fibonacci spiral |
//! | hip | an urn over four fifths of it, five sepals rising from its mouth |
//! | samara | a seed in the first fifth and a flat wing $`w`$ wide beyond it |
//! | acorn | a nut in a scaly cup over its first third |
//! | husked | a nut in its first third inside a husk drawn out into a beak |
//! | pod | flat (a third as thick as wide), bulging over its seeds |
//! | capsule | ribbed with three ribs |
//!
//! A cone (`cone` of a half or more) in a cluster is drawn as a single
//! cone is (`crate::blooms`). Fruit in large clusters take fewer facets,
//! so a cluster of 40 elderberries costs about what one cone does.

use crate::blooms::{self, FruitForm, Part};
use crate::graph::GraphOrgan;
use crate::leaves::{self, Section, SolidLeaf};
use crate::looks::{Cluster, Fruit, FruitKind};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};

/// The golden angle, degrees.
pub const GOLDEN_DEGREES: f64 = 137.507_764;

/// One fruit of an organ, in the organ's frame (see the module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Where its stalk leaves the cluster's axis.
    pub attach: Vec3,
    /// The fruit's base, at the end of its stalk.
    pub base: Vec3,
    /// Unit direction from its base toward its tip.
    pub direction: Vec3,
    /// Its length.
    pub length: f64,
    /// Whether it is still unripe.
    pub unripe: bool,
}

/// Whether `fruit` is one plain berry or cone, drawn as it was before P4
/// (`crate::blooms`).
#[must_use]
pub fn is_single(fruit: &Fruit) -> bool {
    fruit.count <= 1 && fruit.kind == FruitKind::Berry && fruit.unripe <= 0.0
}

/// Where the fruit of `fruit` sit, the cluster turned `turn` radians round
/// its axis, unripe ones chosen by `seed`. Its names follow the module's
/// formulas.
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn layout(fruit: &Fruit, turn: f64, seed: u64) -> Vec<Placed> {
    let n = fruit.count.clamp(1, 60);
    let unripe = |i: u32| {
        unit(mix64(
            seed ^ (u64::from(i) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        )) < fruit.unripe
    };
    if n == 1 {
        return vec![Placed {
            attach: Vec3::ZERO,
            base: Vec3::ZERO,
            direction: Vec3::Y,
            length: 1.0,
            unripe: unripe(0),
        }];
    }
    let s = fruit.size.clamp(0.05, 1.0);
    let sigma = math::radians(fruit.spread.clamp(0.0, 90.0));
    let golden = math::radians(GOLDEN_DEGREES);
    let lean = |angle: f64, around: f64| -> Vec3 {
        let radial = Vec3::new(math::cos(around), 0.0, math::sin(around));
        (Vec3::Y * math::cos(angle) + radial * math::sin(angle)).normalize_or(Vec3::Y)
    };
    (0..n)
        .map(|i| {
            let k = f64::from(i);
            let q = (k + 0.5) / f64::from(n);
            let (attach, direction, stalk) = match fruit.cluster {
                Cluster::Raceme => {
                    // The axis stops where the top fruit, on its stalk,
                    // reaches the organ's length.
                    let reach = (1.9 * s * math::cos(sigma)).min(0.9);
                    (
                        Vec3::Y * ((1.0 - reach) * (0.1 + 0.9 * q)),
                        lean(sigma, turn + golden * k),
                        s * 0.9,
                    )
                }
                Cluster::Umbel => {
                    let foot = 0.3 * (1.0 - s);
                    let angle = sigma * math::sqrt(q);
                    // Long enough that the fruit's far side reaches the
                    // organ's length.
                    let side = 0.5 * s * fruit.aspect.clamp(0.05, 2.0) * math::sin(angle);
                    let stalk = ((1.0 - foot - side) / math::cos(angle).max(0.2) - s).max(0.3 * s);
                    (Vec3::Y * foot, lean(angle, turn + golden * k), stalk)
                }
                Cluster::Head => {
                    let (centre, inner) = head(fruit);
                    // From just above the stalk (y = -0.6) round to the top.
                    let y = -0.6 + 1.6 * q;
                    let ring = math::sqrt((1.0 - y * y).max(0.0));
                    let around = turn + golden * k;
                    let direction =
                        Vec3::new(ring * math::cos(around), y, ring * math::sin(around));
                    let base = centre + direction * inner;
                    (base, direction, 0.0)
                }
                Cluster::Pair => {
                    let pairs = n.div_ceil(2);
                    let pair = f64::from(i / 2);
                    let other = if i % 2 == 0 { 0.0 } else { math::PI };
                    // The top pair's fruit, on their stalks, reach the
                    // organ's length; the others are spaced below it.
                    let reach = (1.25 * s * math::cos(sigma)).min(0.95);
                    (
                        Vec3::Y * ((1.0 - reach) * (pair + 1.0) / f64::from(pairs)),
                        lean(sigma, turn + std::f64::consts::FRAC_PI_2 * pair + other),
                        s * 0.25,
                    )
                }
            };
            Placed {
                attach,
                base: attach + direction * stalk,
                direction,
                length: s,
                unripe: unripe(i),
            }
        })
        .collect()
}

/// How far up the organ a cluster's own axis runs (its rachis, or the
/// stalk that carries its fruit): to its highest stalk, or a head's
/// centre; 0 for a single fruit.
#[must_use]
pub fn axis_length(fruit: &Fruit, placed: &[Placed]) -> f64 {
    if placed.len() <= 1 {
        return 0.0;
    }
    if fruit.cluster == Cluster::Head {
        return head(fruit).0.y;
    }
    placed.iter().map(|p| p.attach.y).fold(0.0, f64::max)
}

/// A head's centre and the radius its fruit's bases sit at.
#[must_use]
pub fn head(fruit: &Fruit) -> (Vec3, f64) {
    let s = fruit.size.clamp(0.05, 1.0);
    let width = s * fruit.aspect.clamp(0.05, 2.0);
    let inner =
        (width * 0.5 * math::sqrt(0.35 * f64::from(fruit.count.clamp(1, 60))) - 0.3 * s).max(0.02);
    (Vec3::Y * (1.0 - inner - s).max(inner), inner)
}

/// Card width over card length: one fruit's aspect, or for a cluster twice
/// the farthest any fruit reaches across the card (seen unturned), from
/// 0.1 to 2.
#[must_use]
pub fn card_aspect(fruit: &Fruit) -> f64 {
    if fruit.count <= 1 {
        return fruit.aspect;
    }
    let half_width = |placed: &Placed| placed.length * fruit.aspect * 0.5;
    let reach = layout(fruit, 0.0, 0)
        .iter()
        .map(|placed| {
            let tip = placed.base + placed.direction * placed.length;
            placed.base.x.abs().max(tip.x.abs()) + half_width(placed)
        })
        .fold(0.0, f64::max);
    (2.0 * reach * 1.04).clamp(0.1, 2.0)
}

/// How finely one fruit is drawn: facets round and rings along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Detail {
    around: u32,
    rings: u32,
}

impl Detail {
    /// From the finest to the coarsest.
    const STEPS: [Self; 5] = [
        Self {
            around: 14,
            rings: 9,
        },
        Self {
            around: 10,
            rings: 7,
        },
        Self {
            around: 8,
            rings: 5,
        },
        Self {
            around: 6,
            rings: 4,
        },
        Self {
            around: 5,
            rings: 3,
        },
    ];

    /// Fewer facets for larger clusters, and `coarse` steps fewer again.
    fn for_count(count: u32, coarse: usize) -> Self {
        let step = match count {
            0..=1 => 0,
            2..=3 => 1,
            4..=12 => 2,
            _ => 3,
        };
        Self::STEPS[(step + coarse).min(Self::STEPS.len() - 1)]
    }
}

/// Coarser steps [`draw`] can take, beyond its own.
pub const COARSER: usize = 4;

/// Draw the fruit of `fruit` on `organ` into `mesh`: ripe ones in
/// `colour`, unripe ones, stalks, cups and husks in `accent`; the part's
/// `coarse` steps (up to [`COARSER`]) fewer facets than its count asks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    organ: &GraphOrgan,
    axis: Vec3,
    side: Vec3,
    form: &FruitForm,
    fruit: &Fruit,
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    if length <= 0.0 {
        return;
    }
    let other = axis.cross(side);
    let to_world = |p: Vec3| {
        organ.position + side * (p.x * length) + axis * (p.y * length) + other * (p.z * length)
    };
    let to_direction = |d: Vec3| (side * d.x + axis * d.y + other * d.z).normalize_or(axis);
    let turn = unit(mix64(organ.id ^ 0x00F2_0175)) * std::f64::consts::TAU;
    let detail = Detail::for_count(fruit.count, part.coarse);
    let width_share = fruit.aspect.clamp(0.05, 2.0);
    let stalk_colour = form.stalk.unwrap_or(STALK);
    let placed = layout(fruit, turn, organ.id);
    let reach = axis_length(fruit, &placed);
    if reach > 0.0 {
        stalk_between(
            organ.position,
            axis,
            reach * length,
            fruit.size * width_share * 0.1 * length,
            stalk_colour,
            part,
            mesh,
        );
    }
    for placed in placed {
        let direction = to_direction(placed.direction);
        let base = to_world(placed.base);
        let fruit_length = placed.length * length;
        let width = fruit_length * width_share;
        let stalk = (placed.base - placed.attach).length() * length;
        if stalk > 1e-6 {
            let start = to_world(placed.attach);
            let along = (base - start).normalize_or(direction);
            stalk_between(start, along, stalk, width * 0.07, stalk_colour, part, mesh);
        }
        let body = if placed.unripe { accent } else { colour };
        let one = One {
            base,
            direction,
            across: any_perpendicular(direction),
            length: fruit_length,
            width,
            detail,
        };
        if fruit.cone >= 0.5 {
            let cone = GraphOrgan {
                position: base,
                heading: direction,
                left: one.across,
                size: fruit_length,
                ..organ.clone()
            };
            blooms::draw_fruit(
                &cone,
                direction,
                one.across,
                form,
                fruit.aspect,
                fruit.cone,
                body,
                part,
                mesh,
            );
            continue;
        }
        one.draw(fruit.kind, body, accent, part, mesh);
    }
}

/// Triangles one organ of `fruit` takes drawn as a solid: counted by
/// drawing one.
#[must_use]
pub fn triangles(form: &FruitForm, fruit: &Fruit) -> usize {
    let organ = GraphOrgan {
        id: 1,
        organ: 0,
        segment: None,
        position: Vec3::ZERO,
        heading: Vec3::Y,
        left: Vec3::X,
        size: 1.0,
        born: 0.0,
        shed: None,
        light: 1.0,
    };
    let mut mesh = Mesh::default();
    let part = Part {
        born: 0.0,
        shed: None,
        coarse: 0,
    };
    draw(
        &organ,
        Vec3::Y,
        Vec3::X,
        form,
        fruit,
        [0.5; 3],
        [0.5; 3],
        part,
        &mut mesh,
    );
    mesh.triangle_count()
}

/// A cluster's stalks unless its form sets a colour: green.
const STALK: [f32; 3] = [0.3, 0.38, 0.14];

/// One fruit, in the plant's frame.
struct One {
    base: Vec3,
    direction: Vec3,
    across: Vec3,
    length: f64,
    width: f64,
    detail: Detail,
}

impl One {
    fn at(&self, along: f64) -> Vec3 {
        self.base + self.direction * (along * self.length)
    }

    fn body(&self) -> Spheroid {
        Spheroid {
            centre: self.at(0.5),
            a: self.length * 0.5,
            b: self.width * 0.5,
            c: self.width * 0.5,
            ribs: 0,
            rib_depth: 0.0,
            waves: 0,
            wave_depth: 0.0,
            detail: self.detail,
        }
    }

    fn draw(&self, kind: FruitKind, body: [f32; 3], accent: [f32; 3], part: Part, mesh: &mut Mesh) {
        let first = mesh.positions.len();
        let first_index = mesh.indices.len();
        match kind {
            FruitKind::Berry => self
                .body()
                .draw(self.direction, self.across, body, part, mesh),
            FruitKind::Crowned => {
                self.body()
                    .draw(self.direction, self.across, body, part, mesh);
                self.calyx(0.92, 5, 0.32, 0.08, blooms::shade(body, 0.45), part, mesh);
            }
            FruitKind::Pome => {
                let mut pome = self.body();
                pome.b *= 1.08;
                pome.c *= 1.08;
                pome.draw(self.direction, self.across, body, part, mesh);
                self.calyx(0.95, 5, 0.22, 0.03, blooms::shade(body, 0.35), part, mesh);
            }
            FruitKind::Drupelets => self.drupelets(body, part, mesh),
            FruitKind::Hip => {
                let mut urn = self.body();
                urn.centre = self.at(0.4);
                urn.a = self.length * 0.4;
                urn.draw(self.direction, self.across, body, part, mesh);
                self.calyx(0.78, 5, 0.45, 0.2, accent, part, mesh);
            }
            FruitKind::Samara => self.samara(body, part, mesh),
            FruitKind::Acorn => {
                let mut nut = self.body();
                nut.centre = self.at(0.55);
                nut.a = self.length * 0.45;
                nut.draw(self.direction, self.across, body, part, mesh);
                // The cup over the nut's first third, open toward its tip.
                let mut cup = self.body();
                cup.centre = self.at(0.42);
                cup.a = self.length * 0.34;
                cup.b = self.width * 0.56;
                cup.c = self.width * 0.56;
                cup.waves = 4;
                cup.wave_depth = 0.06;
                cup.half(self.direction, self.across, accent, part, mesh);
            }
            FruitKind::Husked => {
                let mut nut = self.body();
                nut.centre = self.at(0.2);
                nut.a = self.length * 0.2;
                nut.b *= 0.9;
                nut.c *= 0.9;
                nut.draw(self.direction, self.across, body, part, mesh);
                // The husk: round the nut, then drawn out into a beak.
                blooms::frustum(
                    self.at(0.05),
                    self.direction,
                    self.across,
                    self.length * 0.4,
                    self.width * 0.5,
                    self.width * 0.42,
                    accent,
                    part,
                    mesh,
                );
                blooms::frustum(
                    self.at(0.45),
                    self.direction,
                    self.across,
                    self.length * 0.55,
                    self.width * 0.42,
                    self.width * 0.1,
                    accent,
                    part,
                    mesh,
                );
            }
            FruitKind::Pod => {
                let mut pod = self.body();
                pod.c = self.width * 0.17;
                pod.waves = 5;
                pod.wave_depth = 0.12;
                pod.draw(self.direction, self.across, body, part, mesh);
            }
            FruitKind::Capsule => {
                let mut capsule = self.body();
                capsule.ribs = 3;
                capsule.rib_depth = 0.14;
                capsule.draw(self.direction, self.across, body, part, mesh);
            }
        }
        leaves::smooth_normals(mesh, first, first_index);
    }

    /// `count` lobes round the fruit at `at` of its length, each `reach`
    /// of the width long, rising `rise` of the length beyond it.
    #[allow(clippy::too_many_arguments)]
    fn calyx(
        &self,
        at: f64,
        count: u32,
        reach: f64,
        rise: f64,
        colour: [f32; 3],
        part: Part,
        mesh: &mut Mesh,
    ) {
        let other = self.direction.cross(self.across);
        let tip = self.at(at);
        for k in 0..count {
            let angle = std::f64::consts::TAU * f64::from(k) / f64::from(count.max(1));
            let radial = self.across * math::cos(angle) + other * math::sin(angle);
            let out = self.width * reach;
            let foot = tip + radial * (out * 0.35);
            let side = self.direction.cross(radial).normalize_or(self.across) * (out * 0.3);
            blooms::wedge(
                foot - side,
                foot + side,
                foot + radial * out + self.direction * (self.length * rise),
                self.direction * (out * 0.1),
                colour,
                part,
                mesh,
            );
        }
    }

    /// A core under drupelets on a Fibonacci spiral, leaving its base bare.
    fn drupelets(&self, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let mut core = self.body();
        core.a *= 0.8;
        core.b *= 0.8;
        core.c *= 0.8;
        core.detail = Detail {
            around: 6,
            rings: 4,
        };
        core.draw(self.direction, self.across, colour, part, mesh);
        let count: u32 = match self.detail.around {
            10.. => 30,
            8..=9 => 16,
            _ => 10,
        };
        let facets = match self.detail.around {
            14.. => Detail {
                around: 7,
                rings: 4,
            },
            8..=13 => Detail {
                around: 5,
                rings: 3,
            },
            _ => Detail {
                around: 4,
                rings: 2,
            },
        };
        let other = self.direction.cross(self.across);
        let golden = math::radians(GOLDEN_DEGREES);
        let radius = self.width
            * match count {
                30.. => 0.13,
                16..=29 => 0.17,
                _ => 0.21,
            };
        for i in 0..count {
            // From near the base (z = -0.8) to the tip (z = 1).
            let z = -0.8 + 1.8 * (f64::from(i) + 0.5) / f64::from(count);
            let ring = math::sqrt((1.0 - z * z).max(0.0));
            let angle = golden * f64::from(i);
            let radial = self.across * math::cos(angle) + other * math::sin(angle);
            let centre = self.at(0.5)
                + self.direction * (z * self.length * 0.42)
                + radial * (ring * self.width * 0.42);
            Spheroid {
                centre,
                a: radius,
                b: radius,
                c: radius,
                ribs: 0,
                rib_depth: 0.0,
                waves: 0,
                wave_depth: 0.0,
                detail: facets,
            }
            .draw(self.direction, self.across, colour, part, mesh);
        }
    }

    /// A seed in the first fifth and a flat wing beyond it.
    fn samara(&self, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let mut seed = self.body();
        seed.centre = self.at(0.1);
        seed.a = self.length * 0.11;
        seed.b = self.width * 0.32;
        seed.c = self.width * 0.22;
        seed.draw(
            self.direction,
            self.across,
            blooms::shade(colour, 0.8),
            part,
            mesh,
        );
        let wing = SolidLeaf {
            section: Section::Flat,
            thickness: 0.03,
            fold: 0.0,
            widest: 0.62,
            base: 0.2,
            taper: 0.45,
            arch: 6.0,
            twist: 12.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 1,
        };
        let leaf = part.leaf(
            self.at(0.12),
            self.direction,
            self.across,
            self.length * 0.88,
            self.width,
            colour,
        );
        let stations = match self.detail.around {
            14.. => 5,
            10..=13 => 4,
            8..=9 => 3,
            _ => 2,
        };
        leaf.draw(&wing, stations, 1, mesh);
    }
}

/// A short stalk: an open four-sided tube `length` long from `start`.
fn stalk_between(
    start: Vec3,
    along: Vec3,
    length: f64,
    radius: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    const SIDES: u32 = 4;
    let across = any_perpendicular(along);
    let other = along.cross(across);
    let leaf = part.leaf(start, along, across, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for end in [0.0, length] {
        for k in 0..SIDES {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(SIDES);
            let radial = across * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(start + along * end + radial * radius, colour));
        }
    }
    for k in 0..SIDES {
        let (a0, a1) = (first + k, first + (k + 1) % SIDES);
        let (b0, b1) = (a0 + SIDES, a1 + SIDES);
        mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
    }
}

/// A closed body round an axis: half-length `a` along it, half-widths `b`
/// toward `across` and `c` square to both, its radius modulated by `ribs`
/// ribs round it and `waves` bulges along it.
struct Spheroid {
    centre: Vec3,
    a: f64,
    b: f64,
    c: f64,
    ribs: u32,
    rib_depth: f64,
    waves: u32,
    wave_depth: f64,
    detail: Detail,
}

impl Spheroid {
    /// The half below its centre (toward `-axis`), as an open bowl.
    fn half(&self, axis: Vec3, across: Vec3, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let Detail { around, rings } = self.detail;
        let other = axis.cross(across);
        let leaf = part.leaf(self.centre, axis, across, 0.0, 0.0, colour);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        mesh.push(leaf.vertex(self.centre - axis * self.a, colour));
        // Rings from the bottom up to the rim at the centre.
        let rings = rings.max(3);
        for r in 1..=rings {
            let phi = std::f64::consts::FRAC_PI_2 * f64::from(r) / f64::from(rings);
            let t = f64::from(r) / f64::from(rings);
            let wave = 1.0
                + self.wave_depth
                    * math::cos(std::f64::consts::TAU * f64::from(self.waves) * t).max(0.0);
            let along = -math::cos(phi) * self.a;
            let out = math::sin(phi) * wave;
            for k in 0..around {
                let theta = std::f64::consts::TAU * f64::from(k) / f64::from(around);
                let point = self.centre
                    + axis * along
                    + across * (math::cos(theta) * self.b * out)
                    + other * (math::sin(theta) * self.c * out);
                mesh.push(leaf.vertex(point, colour));
            }
        }
        let at = |r: u32, k: u32| first + 1 + (r - 1) * around + k % around;
        for k in 0..around {
            mesh.indices
                .extend_from_slice(&[first, at(1, k + 1), at(1, k)]);
        }
        for r in 1..rings {
            for k in 0..around {
                let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
                mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
            }
        }
    }

    fn draw(&self, axis: Vec3, across: Vec3, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let Detail { around, rings } = self.detail;
        let other = axis.cross(across);
        let leaf = part.leaf(self.centre, axis, across, 0.0, 0.0, colour);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        mesh.push(leaf.vertex(self.centre - axis * self.a, colour));
        for r in 1..rings {
            let phi = math::PI * f64::from(r) / f64::from(rings);
            let t = f64::from(r) / f64::from(rings);
            let wave = 1.0
                + self.wave_depth
                    * math::cos(std::f64::consts::TAU * f64::from(self.waves) * t).max(0.0);
            let along = -math::cos(phi) * self.a;
            let out = math::sin(phi) * wave;
            for k in 0..around {
                let theta = std::f64::consts::TAU * f64::from(k) / f64::from(around);
                let rib = 1.0 + self.rib_depth * math::cos(f64::from(self.ribs) * theta);
                let point = self.centre
                    + axis * along
                    + across * (math::cos(theta) * self.b * out * rib)
                    + other * (math::sin(theta) * self.c * out * rib);
                mesh.push(leaf.vertex(point, colour));
            }
        }
        let top = mesh.push(leaf.vertex(self.centre + axis * self.a, colour));
        let at = |r: u32, k: u32| first + 1 + (r - 1) * around + k % around;
        for k in 0..around {
            mesh.indices
                .extend_from_slice(&[first, at(1, k + 1), at(1, k)]);
            mesh.indices
                .extend_from_slice(&[top, at(rings - 1, k), at(rings - 1, k + 1)]);
        }
        for r in 1..rings - 1 {
            for k in 0..around {
                let (a0, a1, b0, b1) = (at(r, k), at(r, k + 1), at(r + 1, k), at(r + 1, k + 1));
                mesh.indices.extend_from_slice(&[a0, a1, b0, a1, b1, b0]);
            }
        }
    }
}

#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
#[cfg(test)]
mod tests {
    use super::*;

    fn cluster(kind: FruitKind, count: u32, cluster: Cluster) -> Fruit {
        Fruit {
            kind,
            count,
            cluster,
            aspect: 0.8,
            size: 0.25,
            spread: 40.0,
            unripe: 0.4,
            ..Fruit::default()
        }
    }

    #[test]
    fn a_single_fruit_fills_its_organ_and_clusters_keep_their_count() {
        let one = layout(&Fruit::default(), 0.0, 3);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].length, 1.0);
        assert!(is_single(&Fruit::default()));
        for arrangement in [
            Cluster::Raceme,
            Cluster::Umbel,
            Cluster::Pair,
            Cluster::Head,
        ] {
            let fruit = cluster(FruitKind::Berry, 7, arrangement);
            assert!(!is_single(&fruit));
            let placed = layout(&fruit, 0.0, 3);
            assert_eq!(placed.len(), 7);
            for p in &placed {
                let tip = p.base + p.direction * p.length;
                assert!(p.attach.y >= 0.0 && tip.y <= 1.05, "{arrangement:?}: {p:?}");
                assert!((p.direction.length() - 1.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn the_card_holds_every_fruit_seen_face_on() {
        for arrangement in [
            Cluster::Raceme,
            Cluster::Umbel,
            Cluster::Pair,
            Cluster::Head,
        ] {
            let fruit = cluster(FruitKind::Berry, 9, arrangement);
            let half = card_aspect(&fruit) * 0.5;
            for p in layout(&fruit, 0.0, 0) {
                let tip = p.base + p.direction * p.length;
                let reach = p.base.x.abs().max(tip.x.abs()) + p.length * fruit.aspect * 0.5;
                assert!(reach <= half + 1e-9, "{arrangement:?}");
            }
        }
        assert_eq!(card_aspect(&Fruit::default()), Fruit::default().aspect);
    }

    #[test]
    fn about_the_unripe_share_is_unripe() {
        let fruit = Fruit {
            count: 60,
            unripe: 0.3,
            ..Fruit::default()
        };
        let unripe = (0..20)
            .flat_map(|seed| layout(&fruit, 0.0, seed))
            .filter(|p| p.unripe)
            .count();
        let share = unripe as f64 / 1200.0;
        assert!((share - 0.3).abs() < 0.05, "{share}");
    }

    #[test]
    fn every_kind_draws_within_its_organ_and_counts_its_triangles() {
        let kinds = [
            FruitKind::Berry,
            FruitKind::Crowned,
            FruitKind::Pome,
            FruitKind::Drupelets,
            FruitKind::Hip,
            FruitKind::Samara,
            FruitKind::Acorn,
            FruitKind::Husked,
            FruitKind::Pod,
            FruitKind::Capsule,
        ];
        let organ = GraphOrgan {
            id: 11,
            organ: 0,
            segment: None,
            position: Vec3::new(1.0, 2.0, 3.0),
            heading: Vec3::Y,
            left: Vec3::X,
            size: 0.1,
            born: 1.0,
            shed: Some(2.0),
            light: 1.0,
        };
        let part = Part {
            born: 1.0,
            shed: Some(2.0),
            coarse: 0,
        };
        for kind in kinds {
            for count in [1, 5, 20] {
                let fruit = cluster(kind, count, Cluster::Raceme);
                let mut mesh = Mesh::default();
                draw(
                    &organ,
                    Vec3::Y,
                    Vec3::X,
                    &FruitForm::default(),
                    &fruit,
                    [0.6, 0.1, 0.1],
                    [0.2, 0.5, 0.1],
                    part,
                    &mut mesh,
                );
                assert!(mesh.triangle_count() > 0, "{kind:?}");
                assert_eq!(
                    mesh.triangle_count(),
                    triangles(&FruitForm::default(), &fruit),
                    "{kind:?} x {count}"
                );
                for position in &mesh.positions {
                    let p = Vec3::new(
                        f64::from(position[0]),
                        f64::from(position[1]),
                        f64::from(position[2]),
                    ) - organ.position;
                    assert!(p.length() <= 0.1 * 1.3, "{kind:?} x {count}: {p:?}");
                }
            }
        }
    }

    #[test]
    fn layouts_are_the_same_every_time() {
        let fruit = cluster(FruitKind::Samara, 12, Cluster::Pair);
        assert_eq!(layout(&fruit, 0.4, 9), layout(&fruit, 0.4, 9));
    }
}
