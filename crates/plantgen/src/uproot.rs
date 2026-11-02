//! A tree blown over by the roots (plant motion S5, slice B3): the stem
//! stays whole, and the plate of soil its roots hold tips up out of the
//! ground about a hinge at its edge on the side it falls, leaving a pit.
//!
//! **How big the plate is.** Mattheck and Breloer measured the root plates
//! of windthrown trees and drew the plate's radius against the stem's
//! (*The Body Language of Trees*, HMSO, Research for Amenity Trees 4,
//! 1994): a stem 10 cm in radius tears up a plate about 1.3 m round, 20 cm
//! one of 2.3 m, 40 cm one of 3.0 m and 60 cm one of 3.6 m
//! ([`MATTHECK_BRELOER`]). Those four points are read from the book's
//! curve as arborists quote it; the book itself could not be reached
//! from here. Krišāns et al. (2022, *Plants* 11: 1509, table 3) measured
//! the plates of windthrown silver birch on three soils: 21 to 35 cm
//! across at breast height, plates 0.9 to 1.2 m in mean radius (five radii
//! each) and 0.5 to 0.7 m deep at the stem, about twice the stem's
//! diameter ([`DEPTH_PER_DIAMETER`]). Their birch plates are smaller than
//! Mattheck's curve gives; a species or a soil may say otherwise.
//!
//! **Its shape** (assumed, from photos of windthrown trees): deepest under
//! the stem and thinning to a torn rim a third as thick
//! ([`RIM_SHARE`]), the rim ragged, the forest floor still lying on
//! top. The structural roots run out from the stem's foot along the
//! plate's underside and snap a little beyond its rim; sinkers under the
//! stem break off short, and the fine roots at the rim stand out like a
//! fringe. The pit is the plate's mould, a little shallower where loose
//! soil falls back in ([`PIT_SHARE`]), with the snapped ends of the roots
//! that held on the far side sticking out of its wall.
//!
//! **How far it tips.** The tree turns about the hinge, a level line on
//! the ground at the plate's rim square to the fall, until its stem or a
//! thick branch meets the ground: a tall tree comes to rest a little past
//! upright on its plate, the plate standing on edge over its pit.

use crate::graph::PlantGraph;
use crate::math::{self, Vec3};
use crate::mesh::{GROUNDED, Mesh, PlantMesh, Vertex};
use crate::rng::{mix64, unit};

/// Mattheck and Breloer's curve (1994): a windthrown tree's stem radius
/// against its root plate's radius, m.
pub const MATTHECK_BRELOER: [[f64; 2]; 5] =
    [[0.0, 0.0], [0.1, 1.3], [0.2, 2.3], [0.4, 3.0], [0.6, 3.6]];

/// A windthrown plate's depth at the stem, in stem diameters at breast
/// height (Krišāns et al. 2022: 0.5 to 0.7 m under silver birch 21 to
/// 35 cm across).
pub const DEPTH_PER_DIAMETER: f64 = 2.0;

/// The deepest a plate tears out, m (assumed: Krišāns et al. measured
/// stems up to 35 cm, and twice a big stem's diameter would reach far
/// below where most structural roots run).
pub const DEEPEST: f64 = 1.2;

/// The plate's thickness at its rim, as a share of its depth at the stem
/// (assumed).
pub const RIM_SHARE: f64 = 0.3;

/// The pit's depth as a share of the plate's thickness: loose soil falls
/// back in (assumed).
pub const PIT_SHARE: f64 = 0.85;

/// Breast height, m.
pub const BREAST: f64 = 1.3;

/// A root plate: its radius round the stem and its depth under it, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    pub radius: f64,
    pub depth: f64,
}

impl Plate {
    /// The plate a stem `radius` m at breast height tears up: Mattheck and
    /// Breloer's radius (carried on along its last slope past their
    /// largest stem), [`DEPTH_PER_DIAMETER`] deep but no deeper than its
    /// radius or [`DEEPEST`].
    #[must_use]
    pub fn of(radius: f64) -> Self {
        let r = radius.max(0.0);
        let last = MATTHECK_BRELOER[MATTHECK_BRELOER.len() - 1];
        let before = MATTHECK_BRELOER[MATTHECK_BRELOER.len() - 2];
        let plate = if r <= last[0] {
            math::monotone_cubic(&MATTHECK_BRELOER, r)
        } else {
            last[1] + (r - last[0]) * (last[1] - before[1]) / (last[0] - before[0])
        };
        let plate = plate.max(3.0 * r);
        Self {
            radius: plate,
            depth: (DEPTH_PER_DIAMETER * 2.0 * r).min(plate).min(DEEPEST),
        }
    }

    /// Its thickness `share` of the way out from the stem to the rim.
    #[must_use]
    pub fn thickness(&self, share: f64) -> f64 {
        let x = share.clamp(0.0, 1.0);
        self.depth * (RIM_SHARE + (1.0 - RIM_SHARE) * (1.0 - math::pow(x, 1.5)))
    }
}

/// A tree tipped over by the roots.
#[derive(Debug, Clone, PartialEq)]
pub struct Uprooted {
    /// Where the stem stood, on the ground.
    pub foot: Vec3,
    /// The way it fell, level.
    pub toward: Vec3,
    pub plate: Plate,
    /// The stem's radius at breast height, m.
    pub stem: f64,
    /// The hinge's middle, on the ground at the plate's rim down the fall.
    pub hinge: Vec3,
    /// How far it turned about the hinge, radians.
    pub tilt: f64,
    /// The tree as it lies, for a host that wants its bones.
    pub lying: PlantGraph,
    /// Hashes the plate's rim, roots and clods.
    pub seed: u64,
}

impl Uprooted {
    /// The hinge's level line, right-handed so a positive turn tips the
    /// top down the fall.
    #[must_use]
    pub fn axis(&self) -> Vec3 {
        Vec3::Y.cross(self.toward).normalize_or(Vec3::Z)
    }

    /// A point of the tree or its plate as it stood, where it lies.
    #[must_use]
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.hinge + (p - self.hinge).rotate_about(self.axis(), self.tilt)
    }

    /// A direction of the tree or its plate as it stood, where it lies.
    #[must_use]
    pub fn direction(&self, v: Vec3) -> Vec3 {
        v.rotate_about(self.axis(), self.tilt)
    }

    /// The plate's rim `angle` radians round from +X, m from the stem: a
    /// torn, uneven edge.
    #[must_use]
    pub fn rim(&self, angle: f64) -> f64 {
        let phase = |salt: u64| unit(mix64(self.seed ^ salt)) * math::PI * 2.0;
        self.plate.radius
            * (1.0
                + 0.08 * math::sin(2.0 * angle + phase(1))
                + 0.05 * math::sin(3.0 * angle + phase(2))
                + 0.03 * math::sin(7.0 * angle + phase(3)))
    }

    /// Moves a mesh of the tree as it stood to where it lies.
    pub fn place(&self, plant: &mut PlantMesh) {
        place_mesh(self, &mut plant.wood);
        for card in &mut plant.cards {
            card.base = self.point(from(card.base)).to_f32();
            card.heading = self.direction(from(card.heading)).to_f32();
            card.left = self.direction(from(card.left)).to_f32();
        }
        for tuft in &mut plant.tufts {
            tuft.position = self.point(from(tuft.position)).to_f32();
            tuft.normal = self.direction(from(tuft.normal)).to_f32();
            tuft.up = self.direction(from(tuft.up)).to_f32();
        }
        for tube in &mut plant.hair_tubes {
            tube.start = self.point(from(tube.start)).to_f32();
            tube.end = self.point(from(tube.end)).to_f32();
        }
    }
}

fn from(p: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

fn place_mesh(up: &Uprooted, mesh: &mut Mesh) {
    for position in &mut mesh.positions {
        *position = up.point(from(*position)).to_f32();
    }
    for normal in &mut mesh.normals {
        *normal = up.direction(from(*normal)).to_f32();
    }
}

/// The stem's radius `height` m above its foot, and the segments of its
/// axis.
fn stem_of(graph: &PlantGraph) -> Option<(usize, Vec<usize>)> {
    let root = graph
        .segments
        .iter()
        .position(|segment| segment.parent.is_none() && segment.order == 0)?;
    let mut chain = vec![root];
    let mut at = root;
    while let Some(next) = graph.segments.iter().position(|segment| {
        segment.parent == u32::try_from(at).ok() && !segment.lateral && segment.order == 0
    }) {
        chain.push(next);
        at = next;
    }
    Some((root, chain))
}

/// `graph` blown over towards `toward` (level), tipped `tilt` radians
/// about its plate's hinge, or (`None`) until its stem or a branch at
/// least a quarter as thick meets the ground. `None` if it has no stem.
#[must_use]
pub fn uproot(graph: &PlantGraph, toward: Vec3, tilt: Option<f64>) -> Option<Uprooted> {
    let toward = Vec3::new(toward.x, 0.0, toward.z).normalize_or(Vec3::X);
    let (root, chain) = stem_of(graph)?;
    let foot = graph.segments[root].start;
    let at_breast = foot.y + BREAST;
    let stem = chain
        .iter()
        .find(|at| {
            let s = &graph.segments[**at];
            s.start.y <= at_breast && at_breast < s.end.y
        })
        .or(chain.last())
        .map_or(graph.segments[root].radius, |at| graph.segments[*at].radius);
    let plate = Plate::of(stem);
    let seed = mix64(graph.segments[root].id ^ 0x0055_5052_4F4F_5400);
    let mut up = Uprooted {
        foot,
        toward,
        plate,
        stem,
        hinge: foot,
        tilt: 0.0,
        lying: graph.clone(),
        seed,
    };
    let reach = up.rim(math::atan2(toward.z, toward.x));
    up.hinge = foot + toward * reach;
    if let Some(tilt) = tilt {
        up.tilt = tilt;
    } else {
        // It bears on its stem and its thick branches.
        let r = graph.segments[root].radius;
        let mut on_axis = vec![false; graph.segments.len()];
        for at in &chain {
            on_axis[*at] = true;
        }
        let main: Vec<usize> = (0..graph.segments.len())
            .filter(|at| on_axis[*at] || graph.segments[*at].radius >= 0.25 * r)
            .collect();
        // Wood within the plate rises with it.
        let touches = |up: &Uprooted| {
            main.iter().any(|at| {
                let s = &graph.segments[*at];
                [s.start, s.end].iter().any(|p| {
                    (*p - foot).length() > plate.radius && up.point(*p).y - foot.y < s.radius
                })
            })
        };
        let mut below = 0.0;
        for step in 1..=150 {
            up.tilt = math::radians(f64::from(step));
            if touches(&up) {
                break;
            }
            below = up.tilt;
        }
        // Then to where it just meets the ground.
        let mut above = up.tilt;
        for _ in 0..24 {
            up.tilt = f64::midpoint(below, above);
            if touches(&up) {
                above = up.tilt;
            } else {
                below = up.tilt;
            }
        }
        up.tilt = below;
    }
    let mut lying = up.lying.clone();
    for segment in &mut lying.segments {
        segment.start = up.point(segment.start);
        segment.end = up.point(segment.end);
        segment.left = up.direction(segment.left);
    }
    up.lying = lying;
    Some(up)
}

/// Soil, forest floor and root colours, linear RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Earth {
    /// The mineral soil the plate and the pit show.
    pub soil: [f32; 3],
    /// The forest floor on top of the plate and round the pit.
    pub floor: [f32; 3],
    /// The roots' bark.
    pub roots: [f32; 3],
    /// The pale wood where a root snapped.
    pub wood: [f32; 3],
}

impl Earth {
    /// A brown loam under a brown litter (assumed).
    pub const LOAM: Self = Self {
        soil: [0.10, 0.07, 0.045],
        floor: [0.07, 0.05, 0.03],
        roots: [0.20, 0.11, 0.06],
        wood: [0.42, 0.30, 0.18],
    };
}

/// Pieces round the plate and rings out across it.
const AROUND: u32 = 40;
const OUT: u32 = 12;

/// The roots that hold the plate: structural laterals (assumed: eight,
/// a third of the stem's radius where they leave it, snapped a quarter as
/// thick).
const LATERALS: u32 = 8;
const LATERAL_SHARE: f64 = 0.33;
const SNAPPED_SHARE: f64 = 0.4;

/// Smaller roots across the plate's underside, and fine ones on the
/// nearest levels (assumed).
const MEDIUM: u32 = 28;
const FUZZ: u32 = 500;

/// The torn-up plate where it lies: its top under the forest floor, its
/// ragged rim and soil underside, the structural roots along it snapped
/// beyond its rim, sinkers under the stem and, on `fine` levels, the fringe
/// of fine roots at the rim. Placed with the tree.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn plate_mesh(up: &Uprooted, earth: &Earth, fine: bool) -> Mesh {
    let mut mesh = Mesh::default();
    let foot = up.foot;
    let plate = up.plate;
    let jitter = |a: u64, b: u64| unit(mix64(up.seed ^ mix64(a.wrapping_mul(0x9E37) ^ b)));
    let clod = |i: u32, j: u32, salt: u64| jitter(u64::from(i) << 16 | u64::from(j), salt);
    let angle = |i: u32| math::PI * 2.0 * f64::from(i % AROUND) / f64::from(AROUND);
    let dir = |a: f64| Vec3::new(math::cos(a), 0.0, math::sin(a));
    let inner = up.stem * 0.8;
    let shade = |c: [f32; 3], k: f64| {
        #[allow(clippy::cast_possible_truncation)]
        let k = k as f32;
        [c[0] * k, c[1] * k, c[2] * k]
    };
    // The top: the forest floor, from inside the stem's foot to the rim.
    grid(&mut mesh, |i, j| {
        let a = angle(i);
        let rim = up.rim(a);
        let t = f64::from(j) / f64::from(OUT);
        let r = inner + (rim - inner) * t;
        let y = foot.y + 0.012 * (clod(i, j, 11) - 0.5) + 0.02 * (1.0 - t);
        (
            foot + dir(a) * r + Vec3::Y * (y - foot.y),
            shade(earth.floor, 0.85 + 0.3 * clod(i, j, 12)),
        )
    });
    // The underside: soil torn out of the ground, lumpy with clods.
    let under = grid(&mut mesh, |i, j| {
        let a = angle(i);
        let rim = up.rim(a);
        let t = f64::from(j) / f64::from(OUT);
        let r = rim * t;
        // Big clods and small ones.
        let big = math::sin(3.0 * a + 6.0 * clod(0, 0, 23))
            * math::sin(2.0 * math::PI * t + 6.0 * clod(0, 0, 24));
        let small = math::sin(7.0 * a + 6.0 * clod(0, 0, 25))
            * math::sin(5.0 * math::PI * t + 6.0 * clod(0, 0, 26));
        let lump = 1.0
            + (0.25 * big + 0.12 * small + 0.15 * (clod(i, j, 21) - 0.5))
                * math::smoothstep(0.0, 0.35, t);
        let y = foot.y - plate.thickness(r / plate.radius) * lump;
        (
            foot + dir(a) * r + Vec3::Y * (y - foot.y),
            shade(earth.soil, 0.75 + 0.45 * clod(i, j, 22)),
        )
    });
    // The rim's torn wall, from the top's edge down to the underside's.
    let wall = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for i in 0..AROUND {
        let a = angle(i);
        let rim = up.rim(a);
        let thick = plate.thickness(rim / plate.radius);
        for k in 0..=3u32 {
            let s = f64::from(k) / 3.0;
            let bulge = if k == 0 || k == 3 {
                0.0
            } else {
                0.06 * thick * (clod(i, k, 31) - 0.3)
            };
            let p = foot + dir(a) * (rim + bulge) - Vec3::Y * (thick * s);
            let colour = if k == 0 {
                earth.floor
            } else {
                shade(earth.soil, 0.7 + 0.4 * clod(i, k, 32))
            };
            mesh.push(Vertex {
                segment: GROUNDED,
                ..Vertex::plain(p, dir(a), colour)
            });
        }
    }
    for i in 0..AROUND {
        let n = (i + 1) % AROUND;
        for k in 0..3u32 {
            let a0 = wall + i * 4 + k;
            let a1 = wall + n * 4 + k;
            mesh.indices
                .extend_from_slice(&[a0, a1, a0 + 1, a0 + 1, a1, a1 + 1]);
        }
    }
    flip(&mut mesh, under);
    smooth(&mut mesh, 0);
    // The roots, proud of the soil they tore out of: the structural
    // laterals from the stem's foot, tapering fast and snapped beyond the
    // rim, and smaller roots across the underside.
    let r_root = up.stem * LATERAL_SHARE;
    let golden = math::radians(137.507_764);
    // A root wanders across its way, more the further it has run, and
    // more freely past the rim.
    let wander = |out: Vec3, k: u32, root: u64, run: f64| {
        let side = Vec3::Y.cross(out);
        let w = 0.18 * run * (jitter(root << 8 | u64::from(k), 52) - 0.5);
        let v = 0.12 * run * (jitter(root << 8 | u64::from(k), 53) - 0.5);
        side * w + Vec3::Y * v
    };
    let along = |out: Vec3, r: f64, proud: f64, lift: f64| {
        let out_share = (r / plate.radius).min(1.0);
        foot + out * r - Vec3::Y * (plate.thickness(out_share) * (1.0 - lift) + proud)
    };
    for i in 0..LATERALS {
        let a = golden * f64::from(i) + 0.4 * (jitter(u64::from(i), 41) - 0.5);
        let out = dir(a);
        let rim = up.rim(a);
        let beyond = rim + plate.radius * (0.2 + 0.3 * jitter(u64::from(i), 42));
        let curl = 0.8 * (jitter(u64::from(i), 48) - 0.5);
        let r0 = r_root * (0.7 + 0.6 * jitter(u64::from(i), 43));
        // Down from the stem's foot into the plate, then out along its
        // underside, half as thick a quarter of the way out.
        let mut points = vec![(foot + out * (up.stem * 0.7) - Vec3::Y * r0, r0)];
        for k in 1..=6u32 {
            let t = f64::from(k) / 6.0;
            let r = up.stem + (beyond - up.stem) * t;
            let lift = if k == 1 { 0.5 } else { 0.0 };
            let radius = r0 * (SNAPPED_SHARE + (1.0 - SNAPPED_SHARE) * math::exp(-1.5 * t));
            let past = r - r.min(rim);
            let wander = wander(out, k, u64::from(i), r - up.stem);
            points.push((
                along(out, r.min(rim), 0.7 * radius, lift)
                    + out * past
                    + Vec3::Y * (curl * past)
                    + wander,
                radius,
            ));
        }
        snapped(&points, earth, &mut mesh);
    }
    for i in 0..MEDIUM {
        let a = math::PI * 2.0 * (f64::from(i) + jitter(u64::from(i), 44)) / f64::from(MEDIUM);
        let out = dir(a);
        let rim = up.rim(a);
        let from = plate.radius * (0.15 + 0.35 * jitter(u64::from(i), 45));
        let beyond = rim + plate.radius * (0.05 + 0.2 * jitter(u64::from(i), 46));
        let curl = 0.8 * (jitter(u64::from(i), 49) - 0.5);
        let r0 = up.stem * (0.08 + 0.08 * jitter(u64::from(i), 47));
        let points: Vec<(Vec3, f64)> = (0..=4u32)
            .map(|k| {
                let t = f64::from(k) / 4.0;
                let r = from + (beyond - from) * t;
                let radius = r0 * (1.0 - 0.5 * t);
                let past = r - r.min(rim);
                (
                    along(out, r.min(rim), 0.7 * radius, 0.0)
                        + out * past
                        + Vec3::Y * (curl * past),
                    radius,
                )
            })
            .collect();
        snapped(&points, earth, &mut mesh);
    }
    // Sinkers under the stem, broken off short.
    for i in 0..3u32 {
        let a = math::PI * 2.0 * (f64::from(i) + jitter(u64::from(i), 51)) / 3.0;
        let out = dir(a);
        let base = foot + out * (up.stem * 0.6) - Vec3::Y * (0.9 * plate.depth);
        let end = base + out * (0.1 * plate.depth) - Vec3::Y * (0.25 * plate.depth);
        let r0 = up.stem * 0.18;
        round(
            base,
            (base + end) * 0.5,
            end,
            r0,
            r0 * 0.4,
            4,
            earth.roots,
            0.0,
            &mut mesh,
        );
    }
    // Fine roots all over the underside and, below, a fringe at the rim.
    if fine {
        for i in 0..FUZZ {
            let n = u64::from(i) + 1000;
            let a = math::PI * 2.0 * jitter(n, 65);
            let out = dir(a);
            let rim = up.rim(a);
            let r = rim * (0.12 + 0.86 * math::sqrt(jitter(n, 66)));
            let base = along(out, r, 0.0, 0.0);
            let side = Vec3::new(0.0, 1.0, 0.0).cross(out);
            let way =
                (out * 0.5 - Vec3::Y * 0.8 + side * (jitter(n, 67) - 0.5)).normalize_or(-Vec3::Y);
            let length = 0.03 + 0.12 * jitter(n, 68);
            let end = base + way * length;
            let r0 = 0.0015 + 0.0025 * jitter(n, 69);
            crate::roots::tube(
                base,
                base + way * (0.5 * length) + out * (0.2 * length),
                end,
                r0,
                r0 * 0.5,
                2,
                earth.roots,
                0.0,
                &mut mesh,
            );
        }
        for i in 0..(AROUND * 2) {
            let a = math::PI * (f64::from(i) + jitter(u64::from(i), 61)) / f64::from(AROUND);
            let out = dir(a);
            let rim = up.rim(a);
            let thick = plate.thickness(rim / plate.radius);
            let base =
                foot + out * rim - Vec3::Y * (thick * (0.2 + 0.7 * jitter(u64::from(i), 62)));
            let length = 0.05 + 0.25 * jitter(u64::from(i), 63);
            let end = base + out * length - Vec3::Y * (0.3 * length);
            let r0 = 0.002 + 0.004 * jitter(u64::from(i), 64);
            crate::roots::tube(
                base,
                (base + end) * 0.5 + Vec3::Y * (0.1 * length),
                end,
                r0,
                r0 * 0.5,
                3,
                earth.roots,
                0.0,
                &mut mesh,
            );
        }
    }
    place_mesh(up, &mut mesh);
    mesh
}

/// The pit the plate left, where the tree stood: the plate's mould, the
/// snapped ends of the roots that held on the windward side sticking out
/// of its wall.
#[must_use]
pub fn pit_mesh(up: &Uprooted, earth: &Earth) -> Mesh {
    let mut mesh = Mesh::default();
    let foot = up.foot;
    let plate = up.plate;
    let jitter = |a: u64, b: u64| unit(mix64(up.seed ^ mix64(a.wrapping_mul(0x9E37) ^ b)));
    let clod = |i: u32, j: u32, salt: u64| jitter(u64::from(i) << 16 | u64::from(j), salt);
    let angle = |i: u32| math::PI * 2.0 * f64::from(i % AROUND) / f64::from(AROUND);
    let dir = |a: f64| Vec3::new(math::cos(a), 0.0, math::sin(a));
    let shade = |c: [f32; 3], k: f64| {
        #[allow(clippy::cast_possible_truncation)]
        let k = k as f32;
        [c[0] * k, c[1] * k, c[2] * k]
    };
    grid(&mut mesh, |i, j| {
        let a = angle(i);
        let rim = up.rim(a);
        let t = f64::from(j) / f64::from(OUT);
        let r = rim * t;
        let lump = 1.0 + 0.2 * (clod(i, j, 71) - 0.5);
        let deep = if j == OUT {
            0.0
        } else {
            PIT_SHARE * plate.thickness(r / plate.radius) * lump
        };
        // Damp at the bottom, drier up the walls.
        let wet = 0.6 + 0.35 * t + 0.2 * clod(i, j, 72);
        (foot + dir(a) * r - Vec3::Y * deep, shade(earth.soil, wet))
    });
    smooth(&mut mesh, 0);
    // The windward roots' snapped ends: what the plate's roots tore from.
    let r_root = up.stem * LATERAL_SHARE * SNAPPED_SHARE;
    let golden = math::radians(137.507_764);
    for i in 0..LATERALS {
        let a = golden * f64::from(i) + 0.4 * (jitter(u64::from(i), 41) - 0.5);
        let out = dir(a);
        if out.dot(up.toward) > 0.3 {
            continue;
        }
        let rim = up.rim(a);
        let y = -PIT_SHARE * plate.thickness(rim / plate.radius) * 0.5;
        let start = foot + out * (rim + 0.3) + Vec3::Y * y;
        let end = foot + out * (rim - 0.1 - 0.2 * jitter(u64::from(i), 91)) + Vec3::Y * (y - 0.03);
        round(
            start,
            (start + end) * 0.5,
            end,
            r_root * 1.2,
            r_root,
            3,
            earth.roots,
            0.0,
            &mut mesh,
        );
    }
    mesh
}

/// Level ground round the pit, from `inner` to `outer` times the pit's
/// ragged rim, `colour` (`mottled`: shaded a little): the forest floor round it, or
/// a host's ground with the pit's hole left open.
#[must_use]
pub fn floor_ring(up: &Uprooted, inner: f64, outer: f64, colour: [f32; 3], mottled: bool) -> Mesh {
    let mut mesh = Mesh::default();
    let angle = |i: u32| math::PI * 2.0 * f64::from(i % AROUND) / f64::from(AROUND);
    for i in 0..AROUND {
        let a = angle(i);
        let rim = up.rim(a);
        for k in 0..=2u32 {
            let share = inner + (outer - inner) * f64::from(k) / 2.0;
            let tone = if mottled {
                0.85 + 0.3 * unit(mix64(up.seed ^ mix64(u64::from(i) << 8 | u64::from(k))))
            } else {
                1.0
            };
            #[allow(clippy::cast_possible_truncation)]
            let tone = tone as f32;
            let p = up.foot + Vec3::new(math::cos(a), 0.0, math::sin(a)) * (rim * share);
            mesh.push(Vertex {
                segment: GROUNDED,
                ..Vertex::plain(p, Vec3::Y, colour.map(|c| c * tone))
            });
        }
    }
    for i in 0..AROUND {
        let n = (i + 1) % AROUND;
        for k in 0..2u32 {
            let a0 = i * 3 + k;
            let a1 = n * 3 + k;
            mesh.indices
                .extend_from_slice(&[a0, a1, a0 + 1, a0 + 1, a1, a1 + 1]);
        }
    }
    mesh
}

/// A root's tube (`crate::roots::tube`) with sides enough to look round
/// up close.
#[allow(clippy::too_many_arguments)]
fn round(
    a: Vec3,
    b: Vec3,
    c: Vec3,
    r0: f64,
    r1: f64,
    steps: u32,
    colour: [f32; 3],
    born: f64,
    mesh: &mut Mesh,
) {
    crate::roots::tube_round(a, b, c, r0, r1, steps, 10, colour, born, mesh);
}

/// A root through `points`, snapped at its end: its bark, and the pale
/// wood of its broken end.
fn snapped(points: &[(Vec3, f64)], earth: &Earth, mesh: &mut Mesh) {
    chain(points, earth.roots, mesh);
    if let [.., (before, _), (end, radius)] = points {
        let along = (*end - *before).normalize_or(Vec3::Y);
        let tip = *end + along * (0.8 * radius);
        round(
            *end,
            (*end + tip) * 0.5,
            tip,
            *radius * 0.97,
            radius * 0.3,
            2,
            earth.wood,
            0.0,
            mesh,
        );
    }
}

/// A root through `points` (where, how thick), curving smoothly: each
/// piece runs between the middles of two legs, bent through the point
/// between them.
fn chain(points: &[(Vec3, f64)], colour: [f32; 3], mesh: &mut Mesh) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let middle = |i: usize| {
        let ((a, ra), (b, rb)) = (points[i], points[i + 1]);
        ((a + b) * 0.5, f64::midpoint(ra, rb))
    };
    let mut from = points[0];
    for i in 1..n {
        let to = if i + 1 < n { middle(i) } else { points[n - 1] };
        let (bend, _) = if i + 1 < n {
            points[i]
        } else {
            ((from.0 + to.0) * 0.5, 0.0)
        };
        round(from.0, bend, to.0, from.1, to.1, 3, colour, 0.0, mesh);
        from = to;
    }
}

/// A patch of `AROUND` × (`OUT` + 1) vertices from `at(i, j)` (round,
/// out), quads facing up, closed round. The index of its first vertex.
fn grid(mesh: &mut Mesh, at: impl Fn(u32, u32) -> (Vec3, [f32; 3])) -> u32 {
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for i in 0..AROUND {
        for j in 0..=OUT {
            let (p, colour) = at(i, j);
            mesh.push(Vertex {
                segment: GROUNDED,
                ..Vertex::plain(p, Vec3::Y, colour)
            });
        }
    }
    let row = OUT + 1;
    for i in 0..AROUND {
        let n = (i + 1) % AROUND;
        for j in 0..OUT {
            let a0 = first + i * row + j;
            let a1 = first + n * row + j;
            mesh.indices
                .extend_from_slice(&[a0, a1, a0 + 1, a0 + 1, a1, a1 + 1]);
        }
    }
    first
}

/// Turns the triangles of the patch from vertex `first` on to face the
/// other way (the plate's underside faces down).
fn flip(mesh: &mut Mesh, first: u32) {
    let end = first + AROUND * (OUT + 1);
    for triangle in mesh.indices.as_chunks_mut::<3>().0 {
        if triangle.iter().all(|v| (first..end).contains(v)) {
            triangle.swap(1, 2);
        }
    }
}

/// Normals from the triangles round each vertex from `first` on.
fn smooth(mesh: &mut Mesh, first: usize) {
    let mut sums = vec![Vec3::ZERO; mesh.positions.len()];
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|v| v as usize);
        if a < first || b < first || c < first {
            continue;
        }
        let (pa, pb, pc) = (
            from(mesh.positions[a]),
            from(mesh.positions[b]),
            from(mesh.positions[c]),
        );
        let n = (pb - pa).cross(pc - pa);
        for v in [a, b, c] {
            sums[v] += n;
        }
    }
    for (at, sum) in sums.iter().enumerate().skip(first) {
        if sum.length() > 1.0e-12 {
            mesh.normals[at] = sum.normalize_or(Vec3::Y).to_f32();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphSegment;

    /// A 15 m stem 25 cm across at its foot, with a branch out of its top.
    fn tree() -> PlantGraph {
        let segment = |index: u32, y0: f64, y1: f64, radius: f64| GraphSegment {
            id: 7,
            parent: index.checked_sub(1),
            lateral: index == 0,
            order: 0,
            start: Vec3::new(0.0, y0, 0.0),
            end: Vec3::new(0.0, y1, 0.0),
            left: Vec3::X,
            radius,
            born: 0.0,
            shed: None,
            body: 0,
            within: None,
            young: 0.0,
        };
        let mut segments: Vec<GraphSegment> = (0..10)
            .map(|i| {
                let y0 = f64::from(i) * 1.5;
                segment(i, y0, y0 + 1.5, 0.125 * (1.0 - f64::from(i) / 12.0))
            })
            .collect();
        let mut branch = segment(10, 12.0, 12.0, 0.03);
        branch.parent = Some(7);
        branch.lateral = true;
        branch.order = 1;
        branch.end = Vec3::new(2.0, 13.0, 0.0);
        segments.push(branch);
        PlantGraph {
            age: 40.0,
            height: 15.0,
            segments,
            organs: Vec::new(),
            structures: Vec::new(),
        }
    }

    #[test]
    fn the_plate_follows_mattheck_and_breloer_and_is_twice_the_stem_deep() {
        let plate = Plate::of(0.2);
        assert!((plate.radius - 2.3).abs() < 1.0e-9);
        assert!((plate.depth - 0.8).abs() < 1.0e-9);
        let small = Plate::of(0.1);
        assert!((small.radius - 1.3).abs() < 1.0e-9);
        // Past the curve it carries on along its last slope.
        assert!((Plate::of(0.8).radius - 4.2).abs() < 1.0e-9);
        assert!((Plate::of(0.8).depth - DEEPEST).abs() < 1.0e-9);
        // Thinnest at the rim, a third as thick.
        assert!((plate.thickness(1.0) - RIM_SHARE * plate.depth).abs() < 1.0e-9);
        assert!((plate.thickness(0.0) - plate.depth).abs() < 1.0e-9);
    }

    #[test]
    fn a_blown_tree_turns_about_its_plate_s_rim_until_it_lies_on_the_ground() {
        let graph = tree();
        let up = uproot(&graph, Vec3::X, None).expect("a stem");
        // The hinge is on the ground at the rim, down the fall.
        assert!((up.hinge.y).abs() < 1.0e-9);
        assert!(up.hinge.x > 0.8 * up.plate.radius && up.hinge.z.abs() < 1.0e-9);
        // A tall stem comes to rest just past upright on its plate.
        assert!(up.tilt > math::radians(85.0) && up.tilt < math::radians(100.0));
        // The stem's foot is lifted, the plate standing on edge.
        let foot = up.point(Vec3::ZERO);
        assert!(foot.y > 0.8 * up.plate.radius, "{foot:?}");
        // Its top lies down the fall on the ground.
        let top = up.lying.segments[9].end;
        assert!(top.x > 14.0 && top.y < 1.0, "{top:?}");
        // None of its stem is under the ground (thin branches may be:
        // they would bend or break where they hit it).
        assert!(
            up.lying.segments[..10]
                .iter()
                .all(|s| s.start.y.min(s.end.y) > -0.01)
        );
    }

    #[test]
    fn the_plate_stands_on_edge_over_its_pit_with_its_roots_snapped_beyond_it() {
        let up = uproot(&tree(), Vec3::X, None).expect("a stem");
        let plate = plate_mesh(&up, &Earth::LOAM, true);
        assert!(!plate.indices.is_empty());
        let highest = plate
            .positions
            .iter()
            .map(|p| f64::from(p[1]))
            .fold(f64::MIN, f64::max);
        // It reaches about its diameter up off the ground.
        assert!(highest > 1.6 * up.plate.radius, "{highest}");
        // Nothing of it reaches back past the pit's far side.
        let back = plate
            .positions
            .iter()
            .map(|p| f64::from(p[0]))
            .fold(f64::MAX, f64::min);
        assert!(back > -up.plate.radius, "{back}");
        let pit = pit_mesh(&up, &Earth::LOAM);
        let deepest = pit
            .positions
            .iter()
            .map(|p| f64::from(p[1]))
            .fold(f64::MAX, f64::min);
        assert!(deepest < -0.6 * up.plate.depth && deepest > -1.2 * up.plate.depth);
        // Down-facing underside, up-facing pit.
        assert!(pit.normals.iter().take(100).all(|n| n[1] > 0.0));
    }

    #[test]
    fn a_tilt_given_is_kept() {
        let up = uproot(&tree(), Vec3::Z, Some(math::radians(30.0))).expect("a stem");
        assert!((up.tilt - math::radians(30.0)).abs() < 1.0e-12);
        let top = up.lying.segments[9].end;
        assert!(top.z > 5.0 && top.y > 10.0, "{top:?}");
    }
}
