//! Fruit of every kind (plant roadmap P4).
//!
//! A [`Fruit`] template's `kind` says what one fruit is. A look whose fruit
//! is one plain berry or cone is drawn as before P4 (`crate::blooms`);
//! any other single fruit is drawn here ([`draw`]), and a cluster of
//! fruit is an inflorescence whose units are fruit (`crate::clusters`,
//! spec rework L1), each drawn here too. A fruit is unripe, and drawn
//! in the look's accent colour, when a hash of the organ (and in a
//! cluster its index) falls below `unripe`; the cup or husk of a nut takes
//! the accent colour too.
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
//! | follicles | a core under up to 24 beaked follicles on a Fibonacci spiral, a ripe one's seeds hanging from its split |
//! | samaras | a spindle under up to 24 narrow wings rising round it, overlapping |
//! | berries | three berries splayed from the stalk, each as wide as the aspect makes it |
//!
//! A fruit with a `cup` stands in a shallow cup on a swollen stalk, in that
//! colour when ripe and in the accent before.
//!
//! A cone (`cone` of a half or more) is drawn as a single cone is
//! (`crate::blooms`). Fruit in large clusters take fewer facets, so a
//! cluster of 40 elderberries costs about what one cone does.

use crate::blooms::{self, FruitForm, Part};
use crate::graph::GraphOrgan;
use crate::leaves::{self, Section, SolidLeaf};
use crate::looks::{Fruit, FruitKind};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{mix64, unit};

/// The golden angle, degrees.
pub const GOLDEN_DEGREES: f64 = 137.507_764;

/// Whether `fruit` is one plain berry or cone, drawn as it was before P4
/// (`crate::blooms`).
#[must_use]
pub fn is_single(fruit: &Fruit) -> bool {
    fruit.kind == FruitKind::Berry && fruit.unripe <= 0.0
}

/// How finely one fruit is drawn: facets round and rings along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Detail {
    around: u32,
    rings: u32,
    /// The facets and rings of the finest step this one is coarser than:
    /// a body drawn coarser is widened to cover as much as it does there
    /// ([`Detail::widening`]).
    fine: Option<(u32, u32)>,
}

impl Detail {
    /// From the finest to the coarsest.
    const STEPS: [Self; 5] = [
        Self {
            around: 14,
            rings: 9,
            fine: None,
        },
        Self {
            around: 10,
            rings: 7,
            fine: None,
        },
        Self {
            around: 8,
            rings: 5,
            fine: None,
        },
        Self {
            around: 6,
            rings: 4,
            fine: None,
        },
        Self {
            around: 5,
            rings: 3,
            fine: None,
        },
    ];

    /// Fewer facets for larger clusters, and `coarse` steps fewer again.
    pub(crate) fn for_count(count: u32, coarse: usize) -> Self {
        let step = match count {
            0..=1 => 0,
            2..=3 => 1,
            4..=12 => 2,
            _ => 3,
        };
        let fine = Self::STEPS[step];
        let drawn = Self::STEPS[(step + coarse).min(Self::STEPS.len() - 1)];
        Self {
            fine: (drawn != fine).then_some((fine.around, fine.rings)),
            ..drawn
        }
    }

    /// How much wider (across, along) a body drawn at this detail is made
    /// than its true size so that, seen from the side or end, it covers
    /// what it does at its finest step: the ratio of the areas of the
    /// polygons its facets and its rings inscribe in a circle, the finest
    /// over its own, square-rooted; 1 at the finest.
    fn widening(self) -> (f64, f64) {
        // The area of a regular n-gon inscribed in the unit circle, over
        // the circle's.
        let share = |n: u32| {
            let n = f64::from(n.max(3));
            n * math::sin(std::f64::consts::TAU / n) / std::f64::consts::TAU
        };
        self.fine.map_or((1.0, 1.0), |(around, rings)| {
            let along = math::sqrt(share(2 * rings) / share(2 * self.rings));
            (
                math::sqrt(share(around) / share(self.around)) * along,
                along,
            )
        })
    }

    /// A cluster's units merged into one solid (`crate::clusters`).
    pub(crate) fn merged() -> Self {
        Self::STEPS[3]
    }
}

/// Coarser steps a part mesh can take (`crate::parts`), beyond a solid's
/// own.
pub const COARSER: usize = 4;

/// Draw the single fruit of `fruit` on `organ` into `mesh`: ripe in
/// `colour`, unripe and its cup or husk in `accent`; the part's `coarse`
/// steps (up to [`COARSER`]) fewer facets.
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
    let unripe = unit(mix64(organ.id ^ 0x9E37_79B9_7F4A_7C15)) < fruit.unripe;
    draw_unit(
        organ,
        fruit,
        form,
        (to_world(Vec3::ZERO), to_direction(Vec3::Y), length),
        unripe,
        Detail::for_count(1, part.coarse),
        colour,
        accent,
        part,
        mesh,
    );
}

/// Draw one fruit of `fruit` into `mesh` from its base along its
/// direction, its length given: unripe ones, and cups and husks, in
/// `accent`; a cone as `crate::blooms` draws one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_unit(
    organ: &GraphOrgan,
    fruit: &Fruit,
    form: &FruitForm,
    (base, direction, length): (Vec3, Vec3, f64),
    unripe: bool,
    detail: Detail,
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let width = length * fruit.aspect.clamp(0.05, 2.0);
    let body = if unripe { accent } else { colour };
    // A ripe fruit's seeds and cup in their own colours; an unripe one's
    // seeds stay shut in it and its cup is green.
    let extras = Extras {
        seeds: if unripe {
            None
        } else {
            Some(fruit.seeds.unwrap_or(colour))
        },
        cup: fruit.cup.map(|cup| if unripe { accent } else { cup }),
        threads: fruit.threads,
    };
    let one = One {
        base,
        direction,
        across: any_perpendicular(direction),
        length,
        width,
        detail,
    };
    if fruit.cone >= 0.5 {
        let cone = GraphOrgan {
            position: base,
            heading: direction,
            left: one.across,
            size: length,
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
        return;
    }
    one.draw(fruit.kind, body, accent, extras, part, mesh);
}

/// The silky white of the threads a magnolia's seeds hang on.
const THREAD: [f32; 3] = [0.88, 0.86, 0.8];

/// What a fruit shows beyond its body: the colour of its seeds where it
/// opens on them (`None` while it is shut), and of its cup, if it has one.
#[derive(Debug, Clone, Copy)]
struct Extras {
    seeds: Option<[f32; 3]>,
    cup: Option<[f32; 3]>,
    /// How far shown seeds hang on their threads, a share of the length.
    threads: f64,
}

/// An ellipsoid at `centre`, half-lengths `(a, b, c)` along `axis`,
/// `across` and square to both, at `detail`: a cluster's units merged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spheroid(
    centre: Vec3,
    (a, b, c): (f64, f64, f64),
    detail: Detail,
    axis: Vec3,
    across: Vec3,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    Spheroid {
        centre,
        a: a.max(1e-6),
        b: b.max(1e-6),
        c: c.max(1e-6),
        ribs: 0,
        rib_depth: 0.0,
        waves: 0,
        wave_depth: 0.0,
        detail,
    }
    .draw(axis, across, colour, part, mesh);
    leaves::smooth_normals(mesh, first, first_index);
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
        within: None,
        adult: None,
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

    fn draw(
        &self,
        kind: FruitKind,
        body: [f32; 3],
        accent: [f32; 3],
        extras: Extras,
        part: Part,
        mesh: &mut Mesh,
    ) {
        let first = mesh.positions.len();
        let first_index = mesh.indices.len();
        if let Some(cup) = extras.cup {
            self.cup(cup, part, mesh);
        }
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
            FruitKind::Follicles => self.follicles(body, extras, part, mesh),
            FruitKind::Samaras => self.samaras(body, part, mesh),
            FruitKind::Berries => self.berries(body, part, mesh),
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
            fine: None,
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
                fine: None,
            },
            8..=13 => Detail {
                around: 5,
                rings: 3,
                fine: None,
            },
            _ => Detail {
                around: 4,
                rings: 2,
                fine: None,
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

    /// How many parts an aggregate of follicles or samaras is drawn with,
    /// and their facets, by its detail.
    fn parts(&self) -> (u32, Detail) {
        let (count, around, rings) = match self.detail.around {
            14.. => (24, 6, 4),
            10..=13 => (18, 5, 3),
            8..=9 => (12, 4, 3),
            _ => (8, 4, 2),
        };
        (
            count,
            Detail {
                around,
                rings,
                fine: None,
            },
        )
    }

    /// The point and outward direction `i` of `count` round the fruit on
    /// a Fibonacci spiral, from `low` to `high` of its length, `out` of its
    /// half-width from its axis there.
    fn spiral(&self, i: u32, count: u32, (low, high): (f64, f64), out: f64) -> (Vec3, Vec3, f64) {
        let other = self.direction.cross(self.across);
        let t = low + (high - low) * (f64::from(i) + 0.5) / f64::from(count.max(1));
        // The body's half-width at `t`, as an ellipsoid's.
        let z = 2.0 * t - 1.0;
        let ring = math::sqrt((1.0 - z * z).max(0.0));
        let angle = math::radians(GOLDEN_DEGREES) * f64::from(i);
        let radial = self.across * math::cos(angle) + other * math::sin(angle);
        (
            self.at(t) + radial * (ring * self.width * 0.5 * out),
            radial,
            ring,
        )
    }

    /// A core under beaked follicles on a Fibonacci spiral; where `seeds`
    /// is set, every third follicle split, a seed hanging from it on a
    /// thread.
    fn follicles(&self, colour: [f32; 3], extras: Extras, part: Part, mesh: &mut Mesh) {
        let mut core = self.body();
        core.a *= 0.92;
        core.b *= 0.72;
        core.c *= 0.72;
        core.detail = Detail {
            around: 6,
            rings: 4,
            fine: None,
        };
        core.draw(
            self.direction,
            self.across,
            blooms::shade(colour, 0.8),
            part,
            mesh,
        );
        let (count, facets) = self.parts();
        let size = self.width * 0.5 * 2.6 / math::sqrt(f64::from(count));
        for i in 0..count {
            let (centre, radial, ring) = self.spiral(i, count, (0.08, 0.95), 0.72);
            let size = size * (0.6 + 0.4 * ring);
            // Each follicle a flattened, beaked bulge standing out from the
            // core, its beak toward the tip.
            Spheroid {
                centre,
                a: size * 0.7,
                b: size * 0.55,
                c: size * 0.5,
                ribs: 0,
                rib_depth: 0.0,
                waves: 0,
                wave_depth: 0.0,
                detail: facets,
            }
            .draw(
                (self.direction + radial * 0.6).normalize_or(self.direction),
                radial,
                colour,
                part,
                mesh,
            );
            // Its seed: out of the split and hanging on its thread when
            // ripe, shut inside it until then.
            let (seed, out) = match extras.seeds {
                Some(seed) if i % 3 != 0 => (seed, 1.0),
                _ => (colour, 0.0),
            };
            let mut hang = centre + radial * (size * (0.35 + 0.45 * out))
                - self.direction * (size * 0.6 * out);
            if out > 0.0 && extras.threads > 0.0 {
                // Dangling straight down from the split on its thread.
                let mouth = centre + radial * (size * 0.4);
                let drop = extras.threads * self.length;
                stalk_between(mouth, -Vec3::Y, drop, size * 0.03, THREAD, part, mesh);
                hang = mouth - Vec3::Y * (drop + size * 0.3);
            }
            Spheroid {
                centre: hang,
                a: size * 0.42,
                b: size * 0.34,
                c: size * 0.34,
                ribs: 0,
                rib_depth: 0.0,
                waves: 0,
                wave_depth: 0.0,
                detail: Detail {
                    around: 5,
                    rings: 3,
                    fine: None,
                },
            }
            .draw(self.direction, radial, seed, part, mesh);
        }
    }

    /// A slender spindle under narrow wings on a Fibonacci spiral, each
    /// rising from the spindle and leaning out a little, overlapping up
    /// to the tip as a cone's scales do.
    fn samaras(&self, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let mut spindle = self.body();
        spindle.b *= 0.3;
        spindle.c *= 0.3;
        spindle.detail = Detail {
            around: 5,
            rings: 4,
            fine: None,
        };
        spindle.draw(
            self.direction,
            self.across,
            blooms::shade(colour, 0.75),
            part,
            mesh,
        );
        let (count, _) = self.parts();
        let wing = SolidLeaf {
            section: Section::Flat,
            thickness: 0.04,
            fold: 0.0,
            widest: 0.7,
            base: 0.15,
            taper: 0.4,
            tip: 0.0,
            arch: 4.0,
            twist: 0.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 1,
        };
        let stations = if self.detail.around >= 10 { 3 } else { 2 };
        for i in 0..count {
            let (foot, radial, _) = self.spiral(i, count, (0.04, 0.62), 0.25);
            let tilt = (self.direction + radial * 0.32).normalize_or(self.direction);
            let side = tilt.cross(radial).normalize_or(self.across);
            let leaf = part.leaf(
                foot,
                tilt,
                side,
                self.length * 0.45,
                self.width * 0.42,
                colour,
            );
            leaf.draw(&wing, stations, 1, mesh);
        }
    }

    /// Three berries splayed from the stalk, each the fruit's length and
    /// width, leaning out to either side and forward.
    fn berries(&self, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let other = self.direction.cross(self.across);
        for (k, lean) in [(-1.0, 0.0), (1.0, 0.0), (0.0, 1.0)]
            .into_iter()
            .enumerate()
        {
            let out = (self.across * lean.0 + other * lean.1) * 0.42;
            let direction = (self.direction + out).normalize_or(self.direction);
            let mut berry = self.body();
            berry.a *= 0.92;
            berry.b *= 0.9;
            berry.c *= 0.9;
            berry.centre = self.base + direction * (self.length * 0.48);
            // Each a little different in size, as they ripen unevenly.
            let grown = [1.0, 0.86, 0.74][k];
            berry.a *= grown;
            berry.b *= grown;
            berry.c *= grown;
            berry.draw(direction, self.across, colour, part, mesh);
        }
    }

    /// A shallow cup round the fruit's base on a swollen stalk under it,
    /// reaching up a sixth of its length.
    fn cup(&self, colour: [f32; 3], part: Part, mesh: &mut Mesh) {
        let mut cup = self.body();
        cup.centre = self.at(0.16);
        cup.a = self.length * 0.16;
        cup.b = self.width * 0.42;
        cup.c = self.width * 0.42;
        cup.detail = Detail {
            around: self.detail.around.min(8),
            rings: 3,
            fine: None,
        };
        cup.half(self.direction, self.across, colour, part, mesh);
        blooms::frustum(
            self.base - self.direction * (self.length * 0.3),
            self.direction,
            self.across,
            self.length * 0.3,
            self.width * 0.07,
            self.width * 0.2,
            colour,
            part,
            mesh,
        );
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
            tip: 0.0,
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

/// A short stalk: an open four-sided tube `length` long from `start`,
/// three-sided from the `crate::clusters::FLAT` step.
pub(crate) fn stalk_between(
    start: Vec3,
    along: Vec3,
    length: f64,
    radius: f64,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let sides: u32 = if part.coarse >= crate::clusters::FLAT {
        3
    } else {
        4
    };
    let across = any_perpendicular(along);
    let other = along.cross(across);
    let leaf = part.leaf(start, along, across, 0.0, 0.0, colour);
    let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
    for end in [0.0, length] {
        for k in 0..sides {
            let theta = std::f64::consts::TAU * f64::from(k) / f64::from(sides);
            let radial = across * math::cos(theta) + other * math::sin(theta);
            mesh.push(leaf.vertex(start + along * end + radial * radius, colour));
        }
    }
    for k in 0..sides {
        let (a0, a1) = (first + k, first + (k + 1) % sides);
        let (b0, b1) = (a0 + sides, a1 + sides);
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
        let Detail { around, rings, .. } = self.detail;
        let (wide, long) = self.detail.widening();
        let (a, b, c) = (self.a * long, self.b * wide, self.c * wide);
        let other = axis.cross(across);
        let leaf = part.leaf(self.centre, axis, across, 0.0, 0.0, colour);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        mesh.push(leaf.vertex(self.centre - axis * a, colour));
        // Rings from the bottom up to the rim at the centre.
        let rings = rings.max(3);
        for r in 1..=rings {
            let phi = std::f64::consts::FRAC_PI_2 * f64::from(r) / f64::from(rings);
            let t = f64::from(r) / f64::from(rings);
            let wave = 1.0
                + self.wave_depth
                    * math::cos(std::f64::consts::TAU * f64::from(self.waves) * t).max(0.0);
            let along = -math::cos(phi) * a;
            let out = math::sin(phi) * wave;
            for k in 0..around {
                let theta = std::f64::consts::TAU * f64::from(k) / f64::from(around);
                let point = self.centre
                    + axis * along
                    + across * (math::cos(theta) * b * out)
                    + other * (math::sin(theta) * c * out);
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
        let Detail { around, rings, .. } = self.detail;
        let (wide, long) = self.detail.widening();
        let (a, b, c) = (self.a * long, self.b * wide, self.c * wide);
        let other = axis.cross(across);
        let leaf = part.leaf(self.centre, axis, across, 0.0, 0.0, colour);
        let first = u32::try_from(mesh.positions.len()).unwrap_or(0);
        mesh.push(leaf.vertex(self.centre - axis * a, colour));
        for r in 1..rings {
            let phi = math::PI * f64::from(r) / f64::from(rings);
            let t = f64::from(r) / f64::from(rings);
            let wave = 1.0
                + self.wave_depth
                    * math::cos(std::f64::consts::TAU * f64::from(self.waves) * t).max(0.0);
            let along = -math::cos(phi) * a;
            let out = math::sin(phi) * wave;
            for k in 0..around {
                let theta = std::f64::consts::TAU * f64::from(k) / f64::from(around);
                let rib = 1.0 + self.rib_depth * math::cos(f64::from(self.ribs) * theta);
                let point = self.centre
                    + axis * along
                    + across * (math::cos(theta) * b * out * rib)
                    + other * (math::sin(theta) * c * out * rib);
                mesh.push(leaf.vertex(point, colour));
            }
        }
        let top = mesh.push(leaf.vertex(self.centre + axis * a, colour));
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

#[allow(clippy::float_cmp)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_berries_are_drawn_as_before_and_other_fruit_here() {
        assert!(is_single(&Fruit::default()));
        assert!(!is_single(&Fruit {
            unripe: 0.3,
            ..Fruit::default()
        }));
        assert!(!is_single(&Fruit {
            kind: FruitKind::Samara,
            ..Fruit::default()
        }));
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
            FruitKind::Follicles,
            FruitKind::Samaras,
            FruitKind::Berries,
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
            within: None,
            adult: None,
        };
        let part = Part {
            born: 1.0,
            shed: Some(2.0),
            coarse: 0,
        };
        for kind in kinds {
            let fruit = Fruit {
                kind,
                aspect: 0.8,
                unripe: 0.4,
                ..Fruit::default()
            };
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
                "{kind:?}"
            );
            for position in &mesh.positions {
                let p = Vec3::new(
                    f64::from(position[0]),
                    f64::from(position[1]),
                    f64::from(position[2]),
                ) - organ.position;
                assert!(p.length() <= 0.1 * 1.3, "{kind:?}: {p:?}");
            }
        }
    }
}
