//! Where a plant breaks, and what falls (plant motion S5, slice B1).
//!
//! A plant fails wherever the bending moment its load puts on a section of
//! its wood first passes the moment that section can bear: its green
//! modulus of rupture times its section modulus `π r³ / 4`. Under wind
//! that place is often not the foot of a branch, because a tapered axis
//! loses section faster than its moment falls. This module is the
//! reference that finds those places on a grown plant's [`Mechanics`]:
//!
//! - **Loads.** The plant's own weight, as grown, and a steady wind blowing
//!   level from one of [`DIRECTIONS`] directions with the logarithmic
//!   profile of the motion contract, `U(z) = U10 ln((z − d)/z0) /
//!   ln((10 − d)/z0)`. The crown takes the drag Mayhead measured on
//!   whole trees in a wind tunnel, `½ ρ Cd S U²` on its silhouette `S`
//!   (`ForestGALES`'s diamond, width times depth over two) with `Cd = C
//!   u^(−N)` falling as it streamlines ([`Crown`]), shared among its
//!   leaves by area at the wind at each; wood below the crown takes
//!   `½ ρ Cd A |Uₙ| Uₙ` on its frontal area from the wind's part across
//!   it. One pass of the beam model per direction gives every section's
//!   moment per unit of each, at every wind.
//! - **Critical wind.** For each beam, the slowest `U10` at which the
//!   bending moment at its start, weight and wind together, reaches its
//!   breaking moment, in the worst direction ([`Critical`]).
//! - **Break sites.** For each bone, its foot and, where it is weaker, the
//!   section along it that breaks at the slowest wind ([`Site`]), each with
//!   the piece that falls from it: mass, centre of mass, inertia and the
//!   bones it takes ([`Piece`]).
//! - **The stem and the roots.** The wind that snaps the stem at its
//!   weakest section, and the wind that turns the plant over on its roots
//!   (the anchorage's overturning moment), whichever comes first being how
//!   the plant fails ([`Failure`]).
//!
//! The species states its crown's `C` and `N` and its anchorage per kg
//! of stem (`ForestGALES`'s parameters, from tree-pulling and wind-tunnel
//! studies; [`crate::motion`]); the junction of a branch with its stem is
//! taken as strong as the branch. Small deflections only, as in [`crate::mechanics`]: the loads
//! act on the grown shape.

use serde::Serialize;

use crate::math::{self, Vec3};
use crate::mechanics::{Beams, Mechanics};

/// Level wind directions tried, evenly round the compass.
pub const DIRECTIONS: usize = 8;

/// The fastest wind searched, m/s at 10 m: a section that holds beyond it
/// does not break by wind ([`Critical::wind`] is `None`).
pub const FASTEST: f64 = 90.0;

/// The step of the search for a section's critical wind, m/s; each step
/// that crosses is then halved to [`TOLERANCE`].
const STEP: f64 = 3.0;
const TOLERANCE: f64 = 0.01;

/// The slowest wind at which crown drag was measured, m/s: below it a
/// crown's coefficient is taken as at this speed (as `ForestGALES` does).
pub const SLOWEST_TESTED: f64 = 10.0;

/// How a crown's drag falls as it streamlines: Mayhead's (1973) power
/// fits to whole trees in a wind tunnel, `Cd = C u^(−N)` on the crown's
/// silhouette at rest, `u` held between [`SLOWEST_TESTED`] and the
/// fastest wind tested (`ForestGALES`, `drag_fun`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Crown {
    /// `C`.
    pub at_rest: f64,
    /// `N`.
    pub streamlining: f64,
    /// The fastest wind tested, m/s.
    pub tested_to: f64,
}

impl Crown {
    /// The mean of `ForestGALES`'s three broadleaves (birch, beech, oak):
    /// the stand-in for a plant whose crown drag is not stated.
    pub const BROADLEAF: Self = Self {
        at_rest: 2.34,
        streamlining: 0.88,
        tested_to: 20.0,
    };

    /// The drag coefficient on the silhouette at wind `u`, m/s.
    #[must_use]
    pub fn coefficient(&self, u: f64) -> f64 {
        let held = u.clamp(SLOWEST_TESTED, self.tested_to.max(SLOWEST_TESTED));
        self.at_rest * math::pow(held, -self.streamlining)
    }
}

/// The wind and drag a plant is loaded with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Drag {
    /// Air density, kg/m³.
    pub rho: f64,
    /// Roughness length and displacement height of the wind profile, m.
    pub z0: f64,
    pub d: f64,
    /// Drag coefficient of wood below the crown on its frontal area (a
    /// circular cylinder below the drag crisis).
    pub wood: f64,
    /// The crown's drag on its silhouette.
    pub crown: Crown,
}

impl Default for Drag {
    fn default() -> Self {
        Self {
            rho: 1.225,
            z0: 0.03,
            d: 0.0,
            wood: 1.2,
            crown: Crown::BROADLEAF,
        }
    }
}

/// What each beam's drag scales with, whatever the wind's direction
/// ([`Drag::exposure`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Exposure {
    /// Per beam, the wood's `½ ρ Cd A U(z)²` per unit wind at its middle;
    /// zero for wood in the crown, whose drag its silhouette carries.
    pub wood: Vec<f64>,
    /// Per beam, its share of the crown's drag per unit wind and unit
    /// silhouette and coefficient: `½ ρ` times its share of the leaf area
    /// times `U(z)²` at its end.
    pub leaves: Vec<f64>,
    /// The crown's silhouette across each of the [`DIRECTIONS`], m²: the
    /// diamond `width × depth / 2` `ForestGALES` takes.
    pub silhouette: [f64; DIRECTIONS],
    /// The crown's width across each direction, its foot and its top, m.
    pub width: [f64; DIRECTIONS],
    pub base: f64,
    pub top: f64,
    /// The crown's mean wind per unit wind at 10 m (root mean square over
    /// its leaves): the wind its coefficient is taken at.
    pub reference: f64,
}

impl Drag {
    /// The wind at height `z` (plant frame, m above its foot) per m/s at
    /// 10 m: the contract's logarithmic profile, still at or below `d + z0`.
    #[must_use]
    pub fn profile(&self, z: f64) -> f64 {
        let above = z - self.d;
        if above <= self.z0 {
            return 0.0;
        }
        math::ln(above / self.z0) / math::ln((10.0 - self.d) / self.z0)
    }

    /// The level wind direction `index` of [`DIRECTIONS`].
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn direction(index: usize) -> Vec3 {
        let angle = 2.0 * math::PI * index as f64 / DIRECTIONS as f64;
        Vec3::new(math::cos(angle), 0.0, math::sin(angle))
    }

    /// The crown and what each beam's drag scales with. The crown is the
    /// leaves' extent, each beam's leaves spread half the side of their
    /// one-sided area round its end; wood whose middle is below it takes
    /// its own drag, and the crown's drag is shared among its leaves by
    /// area at the wind at each.
    #[must_use]
    pub fn exposure(&self, beams: &Beams) -> Exposure {
        let n = beams.len();
        let total: f64 = beams.leaf_area.iter().sum();
        let mut base = f64::INFINITY;
        let mut top = f64::NEG_INFINITY;
        let mut low = [f64::INFINITY; DIRECTIONS];
        let mut high = [f64::NEG_INFINITY; DIRECTIONS];
        let across: Vec<Vec3> = (0..DIRECTIONS)
            .map(|index| {
                let w = Self::direction(index);
                Vec3::new(-w.z, 0.0, w.x)
            })
            .collect();
        let mut leaves = vec![0.0; n];
        let mut square = 0.0;
        #[allow(clippy::needless_range_loop)]
        for beam in 0..n {
            let area = beams.leaf_area[beam];
            if area <= 0.0 {
                continue;
            }
            let end = beams.end[beam];
            let spread = 0.5 * area.sqrt();
            base = base.min(end.y - spread);
            top = top.max(end.y + spread);
            for (index, side) in across.iter().enumerate() {
                let at = end.dot(*side);
                low[index] = low[index].min(at - spread);
                high[index] = high[index].max(at + spread);
            }
            let speed = self.profile(end.y);
            leaves[beam] = 0.5 * self.rho * area / total * speed * speed;
            square += area / total * speed * speed;
        }
        let leafy = total > 0.0;
        if !leafy {
            base = f64::INFINITY;
            top = 0.0;
        }
        let depth = if leafy { top - base.max(0.0) } else { 0.0 };
        let mut width = [0.0; DIRECTIONS];
        let mut silhouette = [0.0; DIRECTIONS];
        for index in 0..DIRECTIONS {
            if leafy {
                width[index] = high[index] - low[index];
                silhouette[index] = 0.5 * width[index] * depth;
            }
        }
        let wood = (0..n)
            .map(|beam| {
                let middle = f64::midpoint(beams.start[beam].y, beams.end[beam].y);
                if middle >= base {
                    return 0.0;
                }
                let speed = self.profile(middle);
                let [r0, r1] = beams.radius[beam];
                0.5 * self.rho * self.wood * (r0 + r1) * beams.length[beam] * speed * speed
            })
            .collect();
        Exposure {
            wood,
            leaves,
            silhouette,
            width,
            base: if leafy { base.max(0.0) } else { 0.0 },
            top,
            reference: square.sqrt(),
        }
    }

    /// Loads at each beam's end per unit wind, for wind blowing along
    /// direction `index`, from `exposure`: the wood's, which scale as
    /// `U10²`, from the wind's part across the beam, and the crown's,
    /// which scale as its coefficient times `U10²`. Half of each beam's
    /// wood drag is at its end and half at its parent's, as its mass is.
    #[must_use]
    pub fn loads(beams: &Beams, exposure: &Exposure, index: usize) -> (Vec<Vec3>, Vec<Vec3>) {
        let n = beams.len();
        let direction = Self::direction(index);
        let silhouette = exposure.silhouette[index];
        let mut wood = vec![Vec3::ZERO; n];
        let mut foliage = vec![Vec3::ZERO; n];
        for beam in 0..n {
            let axis = beams.axis[beam];
            let across = direction - axis * axis.dot(direction);
            let force = across * (exposure.wood[beam] * across.length());
            wood[beam] += force * 0.5;
            if let Some(parent) = beams.parent[beam] {
                wood[parent as usize] += force * 0.5;
            }
            foliage[beam] = direction * (exposure.leaves[beam] * silhouette);
        }
        (wood, foliage)
    }

    /// What its record says it assumed whatever the species states.
    #[must_use]
    pub fn assumed() -> Vec<&'static str> {
        vec!["junction"]
    }
}

/// The force and moment each beam carries at its start from everything
/// beyond it, under `loads` at the beams' ends: the moment about its start.
#[must_use]
pub fn carried(beams: &Beams, loads: &[Vec3]) -> (Vec<Vec3>, Vec<Vec3>) {
    let n = beams.len();
    let mut force = loads.to_vec();
    let mut moment: Vec<Vec3> = (0..n)
        .map(|beam| (beams.end[beam] - beams.start[beam]).cross(loads[beam]))
        .collect();
    for beam in (0..n).rev() {
        if let Some(parent) = beams.parent[beam] {
            let parent = parent as usize;
            let lever = beams.start[beam] - beams.start[parent];
            let (f, m) = (force[beam], moment[beam]);
            force[parent] += f;
            moment[parent] += m + lever.cross(f);
        }
    }
    (force, moment)
}

/// The slowest wind that breaks a section, and where it blows from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Critical {
    /// m/s at 10 m; zero where its own weight breaks it, `None` where no
    /// wind up to [`FASTEST`] does.
    pub wind: Option<f64>,
    /// The direction the wind blows towards.
    pub direction: Vec3,
}

/// The moments at every beam's start, split by what makes them: weight,
/// and per direction the wood's and the foliage's per unit wind.
struct Moments {
    weight: Vec<Vec3>,
    wood: Vec<Vec<Vec3>>,
    foliage: Vec<Vec<Vec3>>,
    /// The same about the plant's foot, all its stems together.
    foot: [Vec<Vec3>; 3],
}

impl Moments {
    fn of(beams: &Beams, exposure: &Exposure) -> Self {
        let roots: Vec<usize> = (0..beams.len())
            .filter(|&beam| beams.parent[beam].is_none())
            .collect();
        // The moment about the foot (the origin): each stem's own, and its
        // force on the lever from the origin to its start.
        let foot = |(force, moment): &(Vec<Vec3>, Vec<Vec3>)| {
            roots.iter().fold(Vec3::ZERO, |sum, &root| {
                sum + moment[root] + beams.start[root].cross(force[root])
            })
        };
        let weight = carried(beams, &beams.weights());
        let mut wood = Vec::with_capacity(DIRECTIONS);
        let mut foliage = Vec::with_capacity(DIRECTIONS);
        let mut feet = [vec![foot(&weight)], Vec::new(), Vec::new()];
        for index in 0..DIRECTIONS {
            let (w, f) = Drag::loads(beams, exposure, index);
            let (w, f) = (carried(beams, &w), carried(beams, &f));
            feet[1].push(foot(&w));
            feet[2].push(foot(&f));
            wood.push(w.1);
            foliage.push(f.1);
        }
        Self {
            weight: weight.1,
            wood,
            foliage,
            foot: feet,
        }
    }

    /// The moment at `beam`'s start at wind `u` from direction `index`,
    /// with its part along `axis` taken out (bending only).
    fn bending(&self, beam: usize, index: usize, wind: Wind, axis: Vec3) -> f64 {
        let m = self.weight[beam]
            + self.wood[index][beam] * wind.square
            + self.foliage[index][beam] * wind.foliage;
        (m - axis * axis.dot(m)).length()
    }

    /// The slowest wind at which `bending` reaches `strength`, searched
    /// on the grid `winds` (each step of [`STEP`] from zero to
    /// [`FASTEST`]) and then halved to [`TOLERANCE`].
    fn critical(
        strength: f64,
        winds: &[Wind],
        pace: &Pace,
        bending: impl Fn(usize, Wind) -> f64,
    ) -> Critical {
        let mut best: Option<(f64, usize)> = None;
        for index in 0..DIRECTIONS {
            if bending(index, winds[0]) >= strength {
                return Critical {
                    wind: Some(0.0),
                    direction: Drag::direction(index),
                };
            }
            let limit = best.map_or(FASTEST, |(u, _)| u);
            for pair in winds.windows(2) {
                let (low, high) = (pair[0], pair[1]);
                if low.speed >= limit {
                    break;
                }
                if bending(index, high) >= strength {
                    let (mut a, mut b) = (low.speed, high.speed);
                    while b - a > TOLERANCE {
                        let mid = f64::midpoint(a, b);
                        if bending(index, pace.at(mid)) >= strength {
                            b = mid;
                        } else {
                            a = mid;
                        }
                    }
                    if b < limit {
                        best = Some((b, index));
                    }
                    break;
                }
            }
        }
        Critical {
            wind: best.map(|(u, _)| u),
            direction: Drag::direction(best.map_or(0, |(_, index)| index)),
        }
    }
}

/// A wind speed and what drag scales with at it: its square for wood,
/// and for the crown its coefficient at the crown's wind times the square.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Wind {
    speed: f64,
    square: f64,
    foliage: f64,
}

/// How a crown's coefficient follows the wind at 10 m.
struct Pace {
    crown: Crown,
    /// The crown's wind per unit wind at 10 m ([`Exposure::reference`]).
    reference: f64,
}

impl Pace {
    fn at(&self, speed: f64) -> Wind {
        let square = speed * speed;
        Wind {
            speed,
            square,
            foliage: self.crown.coefficient(speed * self.reference) * square,
        }
    }

    /// The search grid: zero to [`FASTEST`] in steps of [`STEP`].
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn grid(&self) -> Vec<Wind> {
        let steps = (FASTEST / STEP).round() as usize;
        (0..=steps)
            .map(|step| self.at(STEP * step as f64))
            .collect()
    }
}

/// What falls when a plant breaks at a site: everything beyond it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Piece {
    /// kg.
    pub mass: f64,
    /// Its centre of mass, plant frame, m.
    pub centre: [f64; 3],
    /// Its inertia about its centre of mass, kg m²: xx, yy, zz, xy, xz, yz.
    pub inertia: [f64; 6],
    /// The bones wholly in it, in bone order.
    pub bones: Vec<u32>,
}

/// A place a bone breaks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Site {
    /// Along the bone, 0 at its foot and 1 at its tip.
    pub along: f64,
    /// Where it is, plant frame, m, and the wood's radius there.
    pub point: [f64; 3],
    pub radius: f64,
    /// The bending moment that breaks it there, N m.
    pub strength: f64,
    /// The slowest wind that breaks it, m/s at 10 m: zero if its own
    /// weight does, null if no wind up to [`FASTEST`] does.
    pub wind: Option<f64>,
    /// The level direction that wind blows towards.
    pub direction: [f64; 3],
    pub piece: Piece,
    /// The beam whose start it is (not recorded: hosts have `along`).
    #[serde(skip)]
    pub beam: u32,
}

/// How the plant fails as a whole.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    /// The wind that snaps a stem at its weakest section at least a twig's
    /// radius thick ([`crate::motion::TWIG`]), m/s at 10 m, with the stem
    /// bone and the place along it.
    pub snap: Option<f64>,
    pub snap_bone: Option<u32>,
    pub snap_along: f64,
    /// The wind that turns it over on its roots: the moment at the foot
    /// reaching the anchorage's overturning moment.
    pub overturn: Option<f64>,
    /// `snap` or `overturn`, whichever wind is slower; `none` if neither
    /// comes up to [`FASTEST`].
    pub first: &'static str,
    /// The wind and drag it was found with.
    pub drag: Drag,
}

/// A plant's break sites, per bone, and how it fails.
#[derive(Debug, Clone, PartialEq)]
pub struct Breakage {
    pub sites: Vec<Vec<Site>>,
    pub failure: Failure,
    /// Per beam, the slowest wind that breaks its start's section (bones'
    /// own beams only; `None` for the rest).
    pub critical: Vec<Critical>,
}

/// The piece beyond `beam`'s start.
fn piece(found: &Mechanics, beam: usize, bone_first: &[Option<u32>]) -> Piece {
    let beams = &found.beams;
    let run = beams.run(beam);
    let [r0, r1] = beams.radius[beam];
    let own =
        found.material.wood_density * math::PI * beams.length[beam] * (r0 * r0 + r0 * r1 + r1 * r1)
            / 3.0;
    // Masses at the ends of its beams, and the half of the first beam's
    // wood lumped at its parent, put at that beam's start.
    let mut points: Vec<(f64, Vec3)> = run
        .clone()
        .map(|at| (beams.mass[at], beams.end[at]))
        .collect();
    points.push((0.5 * own, beams.start[beam]));
    let mass: f64 = points.iter().map(|(m, _)| m).sum();
    let centre = if mass > 0.0 {
        points.iter().fold(Vec3::ZERO, |sum, (m, p)| sum + *p * *m) * (1.0 / mass)
    } else {
        beams.start[beam]
    };
    let mut inertia = [0.0; 6];
    for (m, p) in &points {
        let r = *p - centre;
        inertia[0] += m * (r.y * r.y + r.z * r.z);
        inertia[1] += m * (r.x * r.x + r.z * r.z);
        inertia[2] += m * (r.x * r.x + r.y * r.y);
        inertia[3] -= m * r.x * r.y;
        inertia[4] -= m * r.x * r.z;
        inertia[5] -= m * r.y * r.z;
    }
    let bones = run
        .filter_map(|at| bone_first[at])
        .collect::<std::collections::BTreeSet<u32>>()
        .into_iter()
        .collect();
    Piece {
        mass,
        centre: [centre.x, centre.y, centre.z],
        inertia,
        bones,
    }
}

/// The most a section loses to the knots through it: a whorl of branches
/// leaves it at least half its strength.
const KNOT_LOSS_MOST: f64 = 0.5;

/// What is left of each beam's strength at its start for the knots there:
/// the branches thinner than it that leave its axis at that section. A
/// branch's knot runs from the pith out to the bark, and its wood carries
/// no tension along the axis it crosses, so a knot of diameter `d` takes
/// out a radial strip of the section `d` wide: `4 d cos²θ / (3π R)` of a
/// round section's bending strength, `θ` its angle from the tension side.
/// The wind comes from every side, so each knot counts at the mean
/// `cos²θ` of ½: `4 r / (3π R)` of a radius-`R` section for a branch of
/// radius `r`. This is the Wood Handbook's (2021, ch. 7, "Knots" and
/// figure 7-2) idealisation of a knot as a reduced cross-section, taken
/// round a stem; it lets stems snap at whorls and near the crown's base
/// rather than in clear wood below it.
fn knots(beams: &Beams) -> Vec<f64> {
    let n = beams.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for beam in 0..n {
        if let Some(parent) = beams.parent[beam] {
            children[parent as usize].push(beam);
        }
    }
    (0..n)
        .map(|beam| {
            let Some(parent) = beams.parent[beam] else {
                return 1.0;
            };
            let big = beams.radius[beam][0];
            if big <= 0.0 {
                return 1.0;
            }
            let lost: f64 = children[parent as usize]
                .iter()
                .filter(|&&other| other != beam && beams.radius[other][0] < big)
                .map(|&other| 4.0 * beams.radius[other][0] / (3.0 * math::PI * big))
                .sum();
            1.0 - lost.min(KNOT_LOSS_MOST)
        })
        .collect()
}

/// The break sites and failure of `found`, with the anchorage's
/// overturning moment `anchorage` (N m), loaded by `drag`. A stem with a
/// root `flare` (`amount`, `height` m: its radius `1 + amount e^(−y /
/// height)` times its wood's, as [`crate::mesh`] draws it) is that much
/// stronger near the ground.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn breakage(
    found: &Mechanics,
    anchorage: f64,
    drag: &Drag,
    flare: Option<(f64, f64)>,
) -> Breakage {
    let beams = &found.beams;
    let n = beams.len();
    let exposure = drag.exposure(beams);
    let moments = Moments::of(beams, &exposure);
    let pace = Pace {
        crown: drag.crown,
        reference: exposure.reference,
    };
    let winds = pace.grid();
    let rupture = found.material.rupture.unwrap_or(0.0);
    // The stems, bones of depth 0, flare at their feet.
    let mut stem = vec![false; n];
    for bone in found.bones.iter().filter(|bone| bone.depth == 0) {
        for &beam in &bone.beams {
            stem[beam as usize] = true;
        }
    }
    let knots = knots(beams);
    let strength = |beam: usize| {
        let mut r = beams.radius[beam][0];
        if let Some((amount, height)) = flare.filter(|_| stem[beam]) {
            r *= 1.0 + amount * math::exp(-beams.start[beam].y.max(0.0) / height.max(1.0e-3));
        }
        rupture * 0.25 * math::PI * r * r * r * knots[beam]
    };
    // Only a bone's own beams can be a site; a beam riding a bone breaks
    // with it.
    let mut on_bone = vec![false; n];
    for bone in &found.bones {
        for &beam in &bone.beams {
            on_bone[beam as usize] = true;
        }
    }
    let critical: Vec<Critical> = (0..n)
        .map(|beam| {
            if !on_bone[beam] {
                return Critical {
                    wind: None,
                    direction: Drag::direction(0),
                };
            }
            let axis = beams.axis[beam];
            Moments::critical(strength(beam), &winds, &pace, |index, wind| {
                moments.bending(beam, index, wind, axis)
            })
        })
        .collect();
    // Each bone's first beam, so a piece names the bones it takes.
    let mut bone_first = vec![None; n];
    for (index, bone) in found.bones.iter().enumerate() {
        if let Some(&first) = bone.beams.first() {
            bone_first[first as usize] = u32::try_from(index).ok();
        }
    }
    let slower = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => a < b,
        (Some(_), None) => true,
        _ => false,
    };
    let mut sites = Vec::with_capacity(found.bones.len());
    let mut snap: (Option<f64>, Option<u32>, f64) = (None, None, 0.0);
    for (index, bone) in found.bones.iter().enumerate() {
        let mut own = Vec::new();
        let mut walked = 0.0;
        let mut weakest: Option<(usize, f64)> = None;
        for &beam in &bone.beams {
            let beam = beam as usize;
            if weakest.is_none_or(|(at, _)| slower(critical[beam].wind, critical[at].wind)) {
                weakest = Some((beam, walked));
            }
            walked += beams.length[beam];
        }
        let Some(&foot) = bone.beams.first() else {
            sites.push(own);
            continue;
        };
        let foot = foot as usize;
        let mut chosen = vec![(foot, 0.0)];
        if let Some((beam, at)) = weakest
            && beam != foot
            && slower(critical[beam].wind, critical[foot].wind)
        {
            chosen.push((beam, at));
        }
        for (beam, at) in chosen {
            let c = critical[beam];
            let along = if bone.length > 0.0 {
                at / bone.length
            } else {
                0.0
            };
            let start = beams.start[beam];
            own.push(Site {
                along,
                point: [start.x, start.y, start.z],
                radius: beams.radius[beam][0],
                strength: strength(beam),
                wind: c.wind,
                direction: [c.direction.x, c.direction.y, c.direction.z],
                piece: piece(found, beam, &bone_first),
                beam: u32::try_from(beam).unwrap_or(u32::MAX),
            });
            // A stem snaps where it is wood, not twig: its leader's tip
            // breaking is not the stem failing.
            if bone.depth == 0
                && beams.radius[beam][0] >= crate::motion::TWIG
                && slower(c.wind, snap.0)
            {
                snap = (c.wind, u32::try_from(index).ok(), along);
            }
        }
        sites.push(own);
    }
    // Overturning: the moment of everything about the plant's foot, across
    // the vertical, against the anchorage.
    let has_roots = (0..n).any(|beam| beams.parent[beam].is_none());
    let overturn = if !has_roots || anchorage <= 0.0 {
        None
    } else {
        Moments::critical(anchorage, &winds, &pace, |index, wind| {
            let m = moments.foot[0][0]
                + moments.foot[1][index] * wind.square
                + moments.foot[2][index] * wind.foliage;
            (m - Vec3::Y * m.y).length()
        })
        .wind
    };
    let first = match (snap.0, overturn) {
        (None, None) => "none",
        (Some(_), None) => "snap",
        (None, Some(_)) => "overturn",
        (Some(s), Some(o)) => {
            if s <= o {
                "snap"
            } else {
                "overturn"
            }
        }
    };
    Breakage {
        sites,
        failure: Failure {
            snap: snap.0,
            snap_bone: snap.1,
            snap_along: snap.2,
            overturn,
            first,
            drag: *drag,
        },
        critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphSegment, PlantGraph};
    use crate::mechanics::{self, Material, Settings};

    /// A straight upright rod of `n` segments, `length` long, its radius
    /// tapering from `r0` to `r1`.
    fn rod(length: f64, r0: f64, r1: f64, n: u32) -> PlantGraph {
        let step = length / f64::from(n);
        let segments = (0..n)
            .map(|index| {
                let t = f64::from(index) / f64::from(n);
                GraphSegment {
                    id: u64::from(index),
                    parent: index.checked_sub(1),
                    lateral: false,
                    order: 0,
                    start: Vec3::Y * (step * f64::from(index)),
                    end: Vec3::Y * (step * f64::from(index + 1)),
                    radius: r0 + (r1 - r0) * t,
                    born: 0.0,
                    shed: None,
                    body: 0,
                    left: Vec3::ZERO,
                    within: None,
                    young: 0.0,
                }
            })
            .collect();
        PlantGraph {
            age: 1.0,
            height: length,
            segments,
            organs: Vec::new(),
            structures: Vec::new(),
        }
    }

    fn material() -> Material {
        Material {
            wood_density: 800.0,
            modulus: 9.0e9,
            rupture: Some(50.0e6),
            leaf_mass: None,
        }
    }

    /// A section through a whorl keeps less strength than one in clear
    /// wood: each branch's knot takes `4 r / (3π R)` of it.
    #[test]
    fn a_knot_weakens_the_section_it_runs_through() {
        let mut graph = rod(4.0, 0.1, 0.1, 4);
        let at = graph.segments[1].end;
        graph.segments.push(GraphSegment {
            id: 99,
            parent: Some(1),
            lateral: true,
            order: 1,
            start: at,
            end: at + Vec3::X,
            radius: 0.05,
            ..graph.segments[0].clone()
        });
        let found = mechanics::mechanics(&graph, &[], &material(), &Settings::default());
        let left = knots(&found.beams);
        let beam = |segment: usize| found.beams.beam_of[segment].expect("a beam") as usize;
        let expected = 1.0 - 4.0 * 0.05 / (3.0 * math::PI * 0.1);
        assert!(
            (left[beam(2)] - expected).abs() < 1.0e-9,
            "{}",
            left[beam(2)]
        );
        assert!(
            (left[beam(1)] - 1.0).abs() < 1.0e-12,
            "clear wood keeps it all"
        );
        assert!(
            (left[beam(4)] - 1.0).abs() < 1.0e-12,
            "the branch itself has no knot"
        );
    }

    #[test]
    fn a_uniform_upright_rod_breaks_at_its_foot_at_the_wind_that_matches_its_strength() {
        let graph = rod(10.0, 0.1, 0.1, 20);
        let found = mechanics::mechanics(&graph, &[], &material(), &Settings::default());
        let drag = Drag::default();
        let broken = breakage(&found, 0.0, &drag, None);
        let stem = &broken.sites[0];
        assert_eq!(stem.len(), 1, "a uniform rod's weakest section is its foot");
        let wind = stem[0].wind.expect("breaks below the fastest wind");
        // The moment at the foot by hand: each segment's drag at its
        // middle's height times that height.
        let strength = 50.0e6 * 0.25 * math::PI * 1.0e-3;
        let beams = &found.beams;
        let moment = |u: f64| {
            (0..beams.len())
                .map(|i| {
                    let z = f64::midpoint(beams.start[i].y, beams.end[i].y);
                    let v = u * drag.profile(z);
                    let area = (beams.radius[i][0] + beams.radius[i][1]) * beams.length[i];
                    0.5 * drag.rho * drag.wood * area * v * v * z
                })
                .sum::<f64>()
        };
        assert!(moment(wind - 0.05) < strength && moment(wind + 0.05) > strength);
        assert!((stem[0].strength - strength).abs() < 1.0e-6 * strength);
        assert_eq!(broken.failure.first, "snap");
    }

    #[test]
    fn a_tapered_rod_breaks_up_its_length_and_the_piece_is_what_is_above() {
        // A tapered pole with a crown of leaves at its top: the section
        // shrinks faster than the crown's moment falls, so it breaks
        // above its foot, about four fifths of the way up.
        let graph = rod(10.0, 0.15, 0.03, 40);
        let whole = Settings {
            piece: 20.0,
            ..Settings::default()
        };
        let mut found = mechanics::mechanics(&graph, &[], &material(), &whole);
        let top = found.beams.len() - 1;
        for beam in top - 3..=top {
            found.beams.leaf_area[beam] = 8.0;
        }
        let broken = breakage(&found, 0.0, &Drag::default(), None);
        let stem = &broken.sites[0];
        assert_eq!(stem.len(), 2);
        let (foot, up) = (&stem[0], &stem[1]);
        assert!(up.along > 0.7 && up.along < 0.95, "{}", up.along);
        let wind = up.wind.expect("the crown breaks it");
        assert!(foot.wind.is_none_or(|at| wind < at));
        // The foot's piece is the whole rod; the upper one weighs less and
        // sits higher.
        let all: f64 = found.beams.mass.iter().sum::<f64>() + found.beams.grounded;
        assert!((foot.piece.mass - all).abs() < 1.0e-9 * all);
        assert!(up.piece.mass < foot.piece.mass);
        assert!(up.piece.centre[1] > foot.piece.centre[1]);
        assert!(up.piece.inertia[0] > 0.0 && up.piece.inertia[2] > 0.0);
    }

    #[test]
    fn a_weak_anchorage_overturns_before_the_stem_snaps() {
        let graph = rod(10.0, 0.1, 0.1, 20);
        let found = mechanics::mechanics(&graph, &[], &material(), &Settings::default());
        let strong = breakage(&found, 1.0e12, &Drag::default(), None);
        assert_eq!(strong.failure.first, "snap");
        assert!(strong.failure.overturn.is_none());
        let weak = breakage(&found, 2_000.0, &Drag::default(), None);
        assert_eq!(weak.failure.first, "overturn");
        assert!(weak.failure.overturn.unwrap() < weak.failure.snap.unwrap());
    }

    /// Leaves on the rod's top eight beams, `area` m² each.
    fn crowned(found: &mut Mechanics, area: f64) {
        let n = found.beams.len();
        for beam in n - 8..n {
            found.beams.leaf_area[beam] = area;
        }
    }

    #[test]
    fn the_crown_takes_its_silhouette_and_wood_within_it_takes_none() {
        let graph = rod(10.0, 0.1, 0.1, 20);
        let mut found = mechanics::mechanics(&graph, &[], &material(), &Settings::default());
        crowned(&mut found, 4.0);
        let drag = Drag::default();
        let exposure = drag.exposure(&found.beams);
        // Leaves at 6.5 to 10 m, each spread 1 m round its beam's end.
        assert!((exposure.top - 11.0).abs() < 1.0e-9);
        assert!((exposure.base - 5.5).abs() < 1.0e-9);
        assert!((exposure.width[0] - 2.0).abs() < 1.0e-9);
        assert!((exposure.silhouette[0] - 5.5).abs() < 1.0e-9);
        for beam in 0..found.beams.len() {
            let middle = f64::midpoint(found.beams.start[beam].y, found.beams.end[beam].y);
            assert_eq!(exposure.wood[beam] > 0.0, middle < 5.5, "{beam}");
        }
        let share: f64 = exposure.leaves.iter().sum();
        let square = exposure.reference * exposure.reference;
        assert!((share - 0.5 * drag.rho * square).abs() < 1.0e-9);
    }

    #[test]
    fn crowns_that_streamline_need_a_faster_wind_to_break_the_stem() {
        let graph = rod(10.0, 0.1, 0.1, 20);
        let mut found = mechanics::mechanics(&graph, &[], &material(), &Settings::default());
        let bare = breakage(&found, 0.0, &Drag::default(), None)
            .failure
            .snap
            .unwrap();
        crowned(&mut found, 4.0);
        let stiff = Drag {
            crown: Crown {
                streamlining: 0.0,
                ..Crown::BROADLEAF
            },
            ..Drag::default()
        };
        let rigid = breakage(&found, 0.0, &stiff, None).failure.snap.unwrap();
        let bending = breakage(&found, 0.0, &Drag::default(), None)
            .failure
            .snap
            .unwrap();
        assert!(rigid < bare);
        // Above the slowest wind tested a streamlined crown drags less.
        assert!(rigid > SLOWEST_TESTED);
        assert!(bending > rigid);
    }

    #[test]
    fn a_crowns_coefficient_is_held_to_the_winds_tested() {
        let crown = Crown {
            at_rest: 2.4,
            streamlining: 0.7,
            tested_to: 25.0,
        };
        let at = |u: f64| 2.4 * math::pow(u, -0.7);
        assert!((crown.coefficient(3.0) - at(10.0)).abs() < 1.0e-12);
        assert!((crown.coefficient(15.0) - at(15.0)).abs() < 1.0e-12);
        assert!((crown.coefficient(40.0) - at(25.0)).abs() < 1.0e-12);
    }
}
