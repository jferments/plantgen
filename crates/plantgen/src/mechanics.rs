//! A grown plant as a structure of beams (plant motion S1): its masses,
//! stiffness and vibration, from its graph and its species' materials,
//! with the bones a host sways it by.
//!
//! `PlantGen` describes a plant's mechanics; a host moves it (the plan in
//! jferments/after-claude `project-files/plant-physics/`). This module is
//! the description and the reference it is checked against. It reads a
//! grown [`PlantGraph`] and the species' [`Material`] and gives:
//!
//! - **The beam model**, the reference. Every wood segment is an
//!   Euler–Bernoulli beam that bends and twists (St Venant torsion), rigidly
//!   joined to the segment it grows from, the plant's foot clamped. Mass is
//!   lumped at the segments' ends: half of each segment's wood at each of
//!   its ends, and each leaf at the end of the segment that bears it.
//!   [`Beams::deflect`] solves it exactly under loads at those ends, in one
//!   pass from the tips to the foot (the force and moment each segment
//!   carries) and one back (the turn and displacement at each end); and
//!   [`Beams::modes`] finds its lowest vibration modes by subspace
//!   iteration on those static solves. Small deflections only: the model
//!   is linear about the grown shape.
//! - **Bones**: each axis (a stem, or a branch from its foot to its tip)
//!   is one bone, down to the [`Settings::bones`] heaviest; a lighter axis
//!   rides the bone it grows from. Each bone carries its length, radii,
//!   masses, bending stiffness, breaking moment where the species states
//!   its strength, and its own two lowest frequencies: those of its axis
//!   clamped at its foot, flexible, with the bones beyond it held rigid
//!   where they join it. A host swaying a bone as a damped oscillator
//!   driven by its parent ([`Binding`] says where every segment lies on its
//!   bone) moves it as the reference does, mode by mode.
//!
//! For a segment of length $`\ell`$ along the unit vector $`\mathbf a`$,
//! carrying the force $`\mathbf F`$ and the moment $`\mathbf M_0`$ about its
//! foot from everything beyond it, the moment a distance $`x`$ along it is
//! $`\mathbf M(x) = \mathbf M_0 - x\,\mathbf a \times \mathbf F`$, and its
//! compliance takes a moment to a rate of turn,
//!
//! ```math
//! C\mathbf v = \frac{\mathbf v - \mathbf a(\mathbf a\cdot\mathbf v)}{EI}
//!   + \frac{\mathbf a(\mathbf a\cdot\mathbf v)}{GJ},
//! \qquad I = \frac{\pi}{4} r_0^2 r_1^2,\quad J = 2I,\quad G = sE,
//! ```
//!
//! for its radii $`r_0`$, $`r_1`$ at its ends and the shear share $`s`$
//! ([`Settings::shear`]), so its end turns and moves by
//!
//! ```math
//! \boldsymbol\theta_1 = \boldsymbol\theta_0
//!   + C\left(\mathbf M_0\ell - \mathbf a\times\mathbf F\,\frac{\ell^2}{2}\right),
//! \qquad
//! \mathbf u_1 = \mathbf u_0 + \boldsymbol\theta_0\times\ell\mathbf a
//!   + C\left(\mathbf M_0\frac{\ell^2}{2}
//!   - \mathbf a\times\mathbf F\,\frac{\ell^3}{6}\right)\times\mathbf a .
//! ```
//!
//! A uniform cantilever under a tip load deflects
//! $`P L^3 / 3EI`$ by it whatever its number of segments, and vibrates
//! at $`f_1 = \frac{1.8751^2}{2\pi L^2}\sqrt{EI/\mu}`$ as its segments
//! shorten (tests below).
//!
//! **Materials** come from the species' characters (`traits.json`):
//! `wood_density_basic_kg_m3` and `wood_moisture_green_share` give green
//! wood's density, `wood_modulus_green_mpa` its stiffness,
//! `wood_rupture_green_mpa` its strength and `leaf_mass_fresh_kg_m2` what
//! leaves weigh. A range is read as its typical value, else its middle.
//! [`Material::of`] names the characters a species lacks; nothing here
//! stands in for one.
//!
//! What is not modelled yet: segments drawn as fleshy bodies (cacti) or
//! within structures (fronds, grass blades) are carried as mass by the
//! wood that bears them, not bent; flowers, fruit and cones weigh nothing;
//! roots and the soil are a clamp at the foot; and nothing here is drawn
//! or packaged.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::checks::Stated;
use crate::graph::{OrganType, PlantGraph};
use crate::lsys::OrganKind;
use crate::math::{self, Vec3};
use crate::rng;

/// Standard gravity, m/s².
pub const GRAVITY: f64 = 9.806_65;

/// The characters a plant's mechanics read, in the order [`Material::of`]
/// names the missing ones.
pub const CHARACTERS: [&str; 5] = [
    "wood_density_basic_kg_m3",
    "wood_moisture_green_share",
    "wood_modulus_green_mpa",
    "wood_rupture_green_mpa",
    "leaf_mass_fresh_kg_m2",
];

/// What a plant is made of, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Green wood's density, kg/m³: the basic density times one plus the
    /// green moisture share.
    pub wood_density: f64,
    /// Green wood's modulus of elasticity, Pa.
    pub modulus: f64,
    /// Green wood's modulus of rupture, Pa, where the species states it.
    pub rupture: Option<f64>,
    /// Fresh leaf mass per one-sided area, kg/m².
    pub leaf_mass: f64,
}

/// A character's value: a number, or a range's typical value, else its
/// middle.
fn typical(traits: &Map<String, Value>, key: &str) -> Option<f64> {
    let stated = Stated::of(traits.get(key)?)?;
    Some(
        stated
            .typical
            .unwrap_or(f64::midpoint(stated.min, stated.max)),
    )
}

impl Material {
    /// The material `traits` (a species' characters as it inherits them)
    /// state.
    ///
    /// # Errors
    ///
    /// The characters it needs and does not state, in [`CHARACTERS`]'
    /// order; strength alone may be missing.
    pub fn of(traits: &Map<String, Value>) -> Result<Self, Vec<&'static str>> {
        let get = |key| typical(traits, key);
        let missing: Vec<&'static str> = CHARACTERS
            .iter()
            .copied()
            .filter(|&key| key != "wood_rupture_green_mpa" && get(key).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(missing);
        }
        let basic = get("wood_density_basic_kg_m3").unwrap_or(0.0);
        let moisture = get("wood_moisture_green_share").unwrap_or(0.0);
        Ok(Self {
            wood_density: basic * (1.0 + moisture),
            modulus: get("wood_modulus_green_mpa").unwrap_or(0.0) * 1.0e6,
            rupture: get("wood_rupture_green_mpa").map(|mpa| mpa * 1.0e6),
            leaf_mass: get("leaf_mass_fresh_kg_m2").unwrap_or(0.0),
        })
    }

    /// Checks the values: densities, stiffness and leaf mass positive and
    /// finite, strength too where given.
    ///
    /// # Errors
    ///
    /// Names the first value that is not.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |name: &str, value: f64| {
            if value > 0.0 && value.is_finite() {
                Ok(())
            } else {
                Err(format!("{name} must be positive and finite, not {value}"))
            }
        };
        positive("wood density", self.wood_density)?;
        positive("modulus", self.modulus)?;
        positive("leaf mass", self.leaf_mass)?;
        if let Some(rupture) = self.rupture {
            positive("rupture", rupture)?;
        }
        Ok(())
    }
}

/// How the model is built and solved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Shear modulus over Young's modulus along the grain, for torsion.
    pub shear: f64,
    /// The most bones a plant has; lighter axes ride the bone they grow
    /// from.
    pub bones: usize,
    /// Vibration modes of the whole plant to find.
    pub modes: usize,
    /// Subspace iterations: enough for the modes to settle to a part in
    /// a thousand on the plants tested.
    pub iterations: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Green wood's shear modulus along the grain is about a
            // fifteenth of its Young's modulus; torsion matters only where
            // a branch's weight twists the axis it stands on.
            shear: 1.0 / 16.0,
            bones: 256,
            modes: 6,
            iterations: 60,
        }
    }
}

/// The wood segments of a grown plant as beams, in depth-first order so
/// that every segment's descendants follow it as one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Beams {
    /// For each beam, the graph segment it is.
    pub segment: Vec<u32>,
    /// For each graph segment, its beam, if it is one.
    pub beam_of: Vec<Option<u32>>,
    /// The beam each grows from, if any; a beam with none is clamped at its
    /// foot.
    pub parent: Vec<Option<u32>>,
    /// The beams after each in its run of descendants: beam `b`'s are
    /// `b + 1 .. b + 1 + beyond[b]`.
    pub beyond: Vec<u32>,
    pub start: Vec<Vec3>,
    pub end: Vec<Vec3>,
    /// Unit vector from start to end.
    pub axis: Vec<Vec3>,
    pub length: Vec<f64>,
    /// Radii at the start and the end, m.
    pub radius: Vec<[f64; 2]>,
    /// Bending stiffness `EI`, N m².
    pub bending: Vec<f64>,
    /// Torsional stiffness `GJ`, N m².
    pub torsion: Vec<f64>,
    /// Mass lumped at each beam's end, kg.
    pub mass: Vec<f64>,
    /// Of which leaves, kg.
    pub leaf: Vec<f64>,
    /// One-sided leaf area lumped at each beam's end, m².
    pub leaf_area: Vec<f64>,
    /// Wood mass lumped at plants' feet (half of each first segment), kg.
    pub grounded: f64,
    /// Organs carried by no beam (a rosette's leaves on no segment).
    pub unborne: usize,
    /// Flowers, fruit and cones, which weigh nothing yet.
    pub unweighed: usize,
    /// Graph segments that are not beams (bodies, structures).
    pub carried: usize,
}

/// Whether a graph segment is drawn as wood: not a fleshy body, not within
/// a structure.
fn is_wood(segment: &crate::graph::GraphSegment) -> bool {
    segment.body == 0 && segment.within.is_none()
}

/// A segment's volume as a frustum of its two radii.
fn frustum(length: f64, r0: f64, r1: f64) -> f64 {
    math::PI * length * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0
}

impl Beams {
    /// The beams of `graph`, its organs weighed by the `types` of its
    /// growth, made of `material`.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn of(
        graph: &PlantGraph,
        types: &[OrganType],
        material: &Material,
        settings: &Settings,
    ) -> Self {
        let segments = &graph.segments;
        let n = segments.len();
        let end_radii = graph.end_radii();
        // The nearest wood segment at or below each segment, following
        // parents (a segment's parent comes before it in the graph).
        let mut wood_at: Vec<Option<u32>> = vec![None; n];
        for (index, segment) in segments.iter().enumerate() {
            wood_at[index] = if is_wood(segment) {
                u32::try_from(index).ok()
            } else {
                segment
                    .parent
                    .and_then(|parent| wood_at.get(parent as usize).copied().flatten())
            };
        }
        // Children of each wood segment among the wood, in graph order.
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut roots = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            if !is_wood(segment) {
                continue;
            }
            let below = segment
                .parent
                .and_then(|parent| wood_at.get(parent as usize).copied().flatten());
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            match below {
                Some(parent) => children[parent as usize].push(index),
                None => roots.push(index),
            }
        }
        // Depth-first order.
        let mut order = Vec::new();
        let mut stack: Vec<u32> = roots.iter().rev().copied().collect();
        while let Some(at) = stack.pop() {
            order.push(at);
            stack.extend(children[at as usize].iter().rev());
        }
        let count = order.len();
        let mut beam_of = vec![None; n];
        for (beam, &segment) in order.iter().enumerate() {
            beam_of[segment as usize] = u32::try_from(beam).ok();
        }
        let mut beams = Self {
            segment: order.clone(),
            beam_of,
            parent: vec![None; count],
            beyond: vec![0; count],
            start: Vec::with_capacity(count),
            end: Vec::with_capacity(count),
            axis: Vec::with_capacity(count),
            length: Vec::with_capacity(count),
            radius: Vec::with_capacity(count),
            bending: Vec::with_capacity(count),
            torsion: Vec::with_capacity(count),
            mass: vec![0.0; count],
            leaf: vec![0.0; count],
            leaf_area: vec![0.0; count],
            grounded: 0.0,
            unborne: 0,
            unweighed: 0,
            carried: segments.iter().filter(|segment| !is_wood(segment)).count(),
        };
        for (beam, &index) in order.iter().enumerate() {
            let segment = &segments[index as usize];
            beams.parent[beam] = segment
                .parent
                .and_then(|parent| wood_at[parent as usize])
                .and_then(|parent| beams.beam_of[parent as usize]);
            let span = segment.end - segment.start;
            let length = span.length();
            let r0 = segment.radius.max(0.0);
            let r1 = end_radii
                .get(index as usize)
                .copied()
                .unwrap_or(r0)
                .max(0.0);
            let second = 0.25 * math::PI * r0 * r0 * r1 * r1;
            beams.start.push(segment.start);
            beams.end.push(segment.end);
            beams.axis.push(span.normalize_or(Vec3::Y));
            beams.length.push(length);
            beams.radius.push([r0, r1]);
            beams.bending.push(material.modulus * second);
            beams
                .torsion
                .push(settings.shear * material.modulus * 2.0 * second);
            // Half the wood at each end; the foot's half to the ground.
            let wood = material.wood_density * frustum(length, r0, r1);
            beams.mass[beam] += 0.5 * wood;
            match beams.parent[beam] {
                Some(parent) => beams.mass[parent as usize] += 0.5 * wood,
                None => beams.grounded += 0.5 * wood,
            }
        }
        // Runs of descendants, from the last beam back.
        for beam in (0..count).rev() {
            if let Some(parent) = beams.parent[beam] {
                beams.beyond[parent as usize] += 1 + beams.beyond[beam];
            }
        }
        // What is carried: segments that are not wood weigh as wood at the
        // end of the wood bearing them; leaves as their area.
        for (index, segment) in segments.iter().enumerate() {
            if is_wood(segment) {
                continue;
            }
            let r0 = segment.radius.max(0.0);
            let r1 = end_radii.get(index).copied().unwrap_or(r0).max(0.0);
            let wood = material.wood_density * frustum(segment.end.distance(segment.start), r0, r1);
            match wood_at[index].and_then(|at| beams.beam_of[at as usize]) {
                Some(beam) => beams.mass[beam as usize] += wood,
                None => beams.grounded += wood,
            }
        }
        for organ in &graph.organs {
            let Some(kind) = types.get(usize::from(organ.organ)) else {
                continue;
            };
            if !matches!(kind.kind, OrganKind::Leaf | OrganKind::Foliage) {
                beams.unweighed += 1;
                continue;
            }
            let area = kind.area * organ.size * organ.size;
            let beam = organ
                .segment
                .and_then(|segment| wood_at.get(segment as usize).copied().flatten())
                .and_then(|segment| beams.beam_of[segment as usize]);
            match beam {
                Some(beam) => {
                    let mass = material.leaf_mass * area;
                    beams.mass[beam as usize] += mass;
                    beams.leaf[beam as usize] += mass;
                    beams.leaf_area[beam as usize] += area;
                }
                None => beams.unborne += 1,
            }
        }
        beams
    }

    /// Beams in all.
    #[must_use]
    pub fn len(&self) -> usize {
        self.segment.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segment.is_empty()
    }

    /// The run of beam `root` and its descendants.
    fn run(&self, root: usize) -> std::ops::Range<usize> {
        root..root + 1 + self.beyond[root] as usize
    }

    /// Displacements of the beams' ends under `loads` (newtons at each
    /// beam's end), the plant's feet clamped. With `within`, only the run
    /// of that beam and its descendants is solved, clamped at its foot, and
    /// only the beams `flexible` says bend; the rest are rigid.
    #[must_use]
    pub fn deflect(
        &self,
        loads: &[Vec3],
        within: Option<usize>,
        flexible: Option<&[bool]>,
    ) -> Vec<Vec3> {
        let mut turned = vec![Vec3::ZERO; self.len()];
        let mut moved = vec![Vec3::ZERO; self.len()];
        let runs: Vec<std::ops::Range<usize>> = match within {
            Some(root) => vec![self.run(root)],
            None => (0..self.len())
                .filter(|&beam| self.parent[beam].is_none())
                .map(|root| self.run(root))
                .collect(),
        };
        let mut force = vec![Vec3::ZERO; self.len()];
        let mut moment = vec![Vec3::ZERO; self.len()];
        for run in runs {
            let first = run.start;
            for beam in run.clone() {
                force[beam] = loads[beam];
                moment[beam] = (self.end[beam] - self.start[beam]).cross(loads[beam]);
            }
            for beam in run.clone().rev() {
                if beam == first {
                    continue;
                }
                if let Some(parent) = self.parent[beam] {
                    let parent = parent as usize;
                    let lever = self.start[beam] - self.start[parent];
                    let (f, m) = (force[beam], moment[beam]);
                    force[parent] += f;
                    moment[parent] += m + lever.cross(f);
                }
            }
            for beam in run {
                let (turn, at) = match self.parent[beam] {
                    Some(parent) if beam != first => {
                        (turned[parent as usize], moved[parent as usize])
                    }
                    _ => (Vec3::ZERO, Vec3::ZERO),
                };
                let length = self.length[beam];
                let axis = self.axis[beam];
                let bends = flexible.is_none_or(|flexible| flexible[beam]);
                let comply = |v: Vec3| {
                    if !bends {
                        return Vec3::ZERO;
                    }
                    let along = axis * axis.dot(v);
                    (v - along) * (1.0 / self.bending[beam]) + along * (1.0 / self.torsion[beam])
                };
                let shear = axis.cross(force[beam]);
                let m0 = moment[beam];
                turned[beam] = turn + comply(m0 * length - shear * (0.5 * length * length));
                moved[beam] = at
                    + turn.cross(axis * length)
                    + comply(
                        m0 * (0.5 * length * length) - shear * (length * length * length / 6.0),
                    )
                    .cross(axis);
            }
        }
        moved
    }

    /// The beams' weights, newtons, straight down.
    #[must_use]
    pub fn weights(&self) -> Vec<Vec3> {
        self.mass
            .iter()
            .map(|&mass| Vec3::new(0.0, -mass * GRAVITY, 0.0))
            .collect()
    }

    /// The lowest `count` vibration modes of the run of `within` (all the
    /// plant when `None`), only the `flexible` beams bending: each one's
    /// frequency, Hz, and its shape, the ends' displacements (mass
    /// normalised: the sum of mass times squared displacement is 1).
    #[must_use]
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    pub fn modes(
        &self,
        count: usize,
        within: Option<usize>,
        flexible: Option<&[bool]>,
        iterations: u32,
    ) -> Vec<Mode> {
        let run: Vec<usize> = match within {
            Some(root) => self.run(root).collect(),
            None => (0..self.len()).collect(),
        };
        let massive = run.iter().filter(|&&beam| self.mass[beam] > 0.0).count();
        let count = count.min(3 * massive);
        if count == 0 {
            return Vec::new();
        }
        // Guard vectors speed the lowest modes' convergence.
        let width = (2 * count).max(count + 4).min(3 * massive);
        let foot = run.first().map_or(Vec3::ZERO, |&beam| self.start[beam]);
        // Start from bending one way and the other across the plant, then
        // from hashed shapes.
        let mut basis: Vec<Vec<Vec3>> = (0..width)
            .map(|column| {
                let mut shape = vec![Vec3::ZERO; self.len()];
                for &beam in &run {
                    let lever = (self.end[beam] - foot).length();
                    shape[beam] = match column {
                        0 => Vec3::X * lever,
                        1 => Vec3::Z * lever,
                        _ => {
                            let draw = |axis: u64| {
                                rng::unit(rng::hash_words(&[column as u64, beam as u64, axis]))
                                    - 0.5
                            };
                            Vec3::new(draw(0), draw(1), draw(2)) * lever
                        }
                    };
                }
                shape
            })
            .collect();
        let mut values = vec![0.0; width];
        let dot = |a: &[Vec3], b: &[Vec3]| -> f64 {
            run.iter()
                .map(|&beam| self.mass[beam] * a[beam].dot(b[beam]))
                .fold(0.0, |total, value| total + value)
        };
        for _ in 0..iterations.max(1) {
            let images: Vec<Vec<Vec3>> = basis
                .iter()
                .map(|shape| {
                    let loads: Vec<Vec3> = shape
                        .iter()
                        .zip(&self.mass)
                        .map(|(&displacement, &mass)| displacement * mass)
                        .collect();
                    self.deflect(&loads, within, flexible)
                })
                .collect();
            // Rayleigh–Ritz: K_r = Yᵀ M X (= Yᵀ K Y), M_r = Yᵀ M Y.
            let mut stiff = vec![vec![0.0; width]; width];
            let mut inertia = vec![vec![0.0; width]; width];
            for row in 0..width {
                for column in row..width {
                    let k = f64::midpoint(
                        dot(&images[row], &basis[column]),
                        dot(&images[column], &basis[row]),
                    );
                    let m = dot(&images[row], &images[column]);
                    stiff[row][column] = k;
                    stiff[column][row] = k;
                    inertia[row][column] = m;
                    inertia[column][row] = m;
                }
            }
            let Some((eigenvalues, vectors)) = generalized_eigen(&stiff, &inertia) else {
                break;
            };
            values = eigenvalues;
            basis = (0..width)
                .map(|column| {
                    let mut shape = vec![Vec3::ZERO; self.len()];
                    for &beam in &run {
                        let mut sum = Vec3::ZERO;
                        for (row, image) in images.iter().enumerate() {
                            sum += image[beam] * vectors[row][column];
                        }
                        shape[beam] = sum;
                    }
                    shape
                })
                .collect();
        }
        (0..count)
            .filter(|&column| values[column] > 0.0)
            .map(|column| {
                let norm = dot(&basis[column], &basis[column]);
                let scale = if norm > 0.0 {
                    1.0 / math::sqrt(norm)
                } else {
                    0.0
                };
                Mode {
                    frequency: math::sqrt(values[column]) / (2.0 * math::PI),
                    shape: basis[column].iter().map(|&v| v * scale).collect(),
                }
            })
            .collect()
    }
}

/// One vibration mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    /// Hz.
    pub frequency: f64,
    /// Each beam end's displacement, mass normalised.
    pub shape: Vec<Vec3>,
}

/// Eigenvalues (ascending) and eigenvectors (columns) of `K q = λ M q` for
/// small symmetric `K` and positive definite `M`, or `None` when `M` is not.
#[allow(clippy::needless_range_loop, clippy::many_single_char_names)]
fn generalized_eigen(k: &[Vec<f64>], m: &[Vec<f64>]) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = k.len();
    // M = L Lᵀ.
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut sum = m[i][j];
            for p in 0..j {
                sum -= l[i][p] * l[j][p];
            }
            if i == j {
                if sum.is_nan() || sum <= 0.0 {
                    return None;
                }
                l[i][i] = math::sqrt(sum);
            } else {
                l[i][j] = sum / l[j][j];
            }
        }
    }
    // A = L⁻¹ K L⁻ᵀ: first B = L⁻¹ K, then A = B L⁻ᵀ = (L⁻¹ Bᵀ)ᵀ.
    let solve_lower = |b: &[f64]| -> Vec<f64> {
        let mut x = vec![0.0; n];
        for i in 0..n {
            let mut sum = b[i];
            for p in 0..i {
                sum -= l[i][p] * x[p];
            }
            x[i] = sum / l[i][i];
        }
        x
    };
    let columns_of_b: Vec<Vec<f64>> = (0..n)
        .map(|column| solve_lower(&(0..n).map(|row| k[row][column]).collect::<Vec<_>>()))
        .collect();
    // columns_of_b[c][r] = B[r][c]; A[r][c] = (L⁻¹ (row r of B)ᵀ)[c].
    let mut a = vec![vec![0.0; n]; n];
    for row in 0..n {
        let b_row: Vec<f64> = (0..n).map(|column| columns_of_b[column][row]).collect();
        a[row].copy_from_slice(&solve_lower(&b_row));
    }
    for row in 0..n {
        for column in row + 1..n {
            let mean = f64::midpoint(a[row][column], a[column][row]);
            a[row][column] = mean;
            a[column][row] = mean;
        }
    }
    let (values, vectors) = jacobi(a);
    // q = L⁻ᵀ v.
    let mut q = vec![vec![0.0; n]; n];
    for column in 0..n {
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut sum = vectors[i][column];
            for p in i + 1..n {
                sum -= l[p][i] * x[p];
            }
            x[i] = sum / l[i][i];
        }
        for row in 0..n {
            q[row][column] = x[row];
        }
    }
    let mut ranked: Vec<usize> = (0..n).collect();
    ranked.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let sorted_values = ranked.iter().map(|&i| values[i]).collect();
    let sorted_vectors = (0..n)
        .map(|row| ranked.iter().map(|&column| q[row][column]).collect())
        .collect();
    Some((sorted_values, sorted_vectors))
}

/// Eigenvalues and eigenvectors (columns) of a symmetric matrix by cyclic
/// Jacobi rotations.
#[allow(clippy::needless_range_loop, clippy::many_single_char_names)]
fn jacobi(mut a: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut v = vec![vec![0.0; n]; n];
    for i in 0..n {
        v[i][i] = 1.0;
    }
    for _ in 0..100 {
        let mut off = 0.0;
        let mut scale = 0.0;
        for i in 0..n {
            scale += a[i][i] * a[i][i];
            for j in i + 1..n {
                off += a[i][j] * a[i][j];
            }
        }
        if off <= 1.0e-30 * scale.max(f64::MIN_POSITIVE) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q] == 0.0 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum().max(0.0).mul_add(2.0, -1.0)
                    / (theta.abs() + math::sqrt(theta * theta + 1.0));
                let c = 1.0 / math::sqrt(t * t + 1.0);
                let s = t * c;
                for k in 0..n {
                    let akp = a[k][p];
                    let akq = a[k][q];
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let apk = a[p][k];
                    let aqk = a[q][k];
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let vkp = v[k][p];
                    let vkq = v[k][q];
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i][i]).collect(), v)
}

/// What kind of axis a bone is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoneKind {
    /// A plant's first axis: its stem.
    Stem,
    /// Any other axis that is a bone.
    Branch,
}

/// One bone: an axis of the plant, from its foot to its tip.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub kind: BoneKind,
    /// The bone it grows from, always earlier in the list.
    pub parent: Option<u32>,
    /// Axes between it and the stem: 0 for a stem, 1 for an axis on it.
    /// Bones come shallowest first, then heaviest first, so any first
    /// bones of the list are a rig of their own.
    pub depth: u32,
    /// Its beams, foot first.
    pub beams: Vec<u32>,
    /// Where it starts and ends.
    pub foot: Vec3,
    pub tip: Vec3,
    /// Along its beams, m.
    pub length: f64,
    /// At its foot and its tip, m.
    pub radius: [f64; 2],
    /// Bending stiffness at its foot, N m².
    pub bending: f64,
    /// What it carries itself, wood and leaves and the axes riding it, kg.
    pub mass: f64,
    /// Of which leaves, kg.
    pub leaf_mass: f64,
    /// It and everything beyond it, kg.
    pub mass_beyond: f64,
    /// Its own one-sided leaf area, m², and the frontal area of its own
    /// wood and riding axes (twice radius times length), m², for drag.
    pub leaf_area: f64,
    pub wood_area: f64,
    /// Its two lowest frequencies, Hz: its axis clamped at its foot, the
    /// bones beyond it rigid.
    pub frequencies: [f64; 2],
    /// The bending moment at its foot from the weight of all it carries,
    /// N m, at rest in the grown shape.
    pub weight_moment: f64,
    /// The bending moment at its foot that breaks it, N m: the green
    /// modulus of rupture times the section modulus `π r³ / 4`, where the
    /// species states its strength.
    pub breaking_moment: Option<f64>,
}

/// Where a graph segment lies on the bones: the bone that moves it and how
/// far along that bone its end is, 0 at the bone's foot and 1 at its tip.
/// A segment of an axis that rides another bone lies where its axis joins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub bone: u32,
    pub along: f32,
}

/// A plant's mechanics: its beams, bones and modes.
#[derive(Debug, Clone, PartialEq)]
pub struct Mechanics {
    pub material: Material,
    pub beams: Beams,
    pub bones: Vec<Bone>,
    /// Per graph segment; none for a segment no wood bears.
    pub binding: Vec<Option<Binding>>,
    /// The whole plant's lowest modes.
    pub modes: Vec<Mode>,
    /// The axes there were, bones or not.
    pub axes: usize,
}

impl Mechanics {
    /// Every mass the plant has, kg: the beams' and its feet's.
    #[must_use]
    pub fn mass(&self) -> f64 {
        self.beams
            .mass
            .iter()
            .fold(self.beams.grounded, |total, mass| total + mass)
    }

    /// Its leaves' mass, kg.
    #[must_use]
    pub fn leaf_mass(&self) -> f64 {
        self.beams.leaf.iter().fold(0.0, |total, mass| total + mass)
    }

    /// How far each beam end moves under the plant's own weight added
    /// once more to the grown shape, m: the largest.
    #[must_use]
    pub fn sag(&self) -> f64 {
        self.beams
            .deflect(&self.beams.weights(), None, None)
            .iter()
            .map(|v| v.length())
            .fold(0.0, f64::max)
    }
}

/// The mechanics of `graph`, grown with organ `types`, made of `material`.
#[must_use]
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::similar_names
)]
pub fn mechanics(
    graph: &PlantGraph,
    types: &[OrganType],
    material: &Material,
    settings: &Settings,
) -> Mechanics {
    let beams = Beams::of(graph, types, material, settings);
    let count = beams.len();
    let segments = &graph.segments;
    // Axes: a beam starts one where it has no parent beam or is lateral.
    let starts_axis = |beam: usize| {
        beams.parent[beam].is_none() || segments[beams.segment[beam] as usize].lateral
    };
    // Each beam's axis (the index of the beam that starts it), and each
    // axis's beams; a beam continuing its parent's axis is its first
    // non-lateral child (graph order), any other starts a new axis.
    let mut axis_of = vec![0_usize; count];
    let mut continued = vec![false; count];
    for beam in 0..count {
        let parent = beams.parent[beam].map(|p| p as usize);
        axis_of[beam] = match parent {
            Some(parent) if !starts_axis(beam) && !continued[parent] => {
                continued[parent] = true;
                axis_of[parent]
            }
            _ => beam,
        };
    }
    let mut axes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (beam, &axis) in axis_of.iter().enumerate() {
        axes.entry(axis).or_default().push(beam);
    }
    // Mass beyond each beam: its run's.
    let beyond_mass: Vec<f64> = (0..count)
        .map(|beam| {
            beams
                .run(beam)
                .map(|at| beams.mass[at])
                .fold(0.0, |total, mass| total + mass)
        })
        .collect();
    // The heaviest axes are bones; ties go to the earlier, so an axis's
    // parent (as heavy at least, and earlier) is always kept before it.
    let mut ranked: Vec<usize> = axes.keys().copied().collect();
    ranked.sort_by(|&a, &b| beyond_mass[b].total_cmp(&beyond_mass[a]).then(a.cmp(&b)));
    ranked.truncate(settings.bones.max(1));
    // Coarse first, so any first bones are a rig of their own: by depth
    // (the stem 0, axes on it 1, and so on), then heaviest first. Every
    // bone's parent, shallower, comes before it.
    let mut depth_of = vec![0_u32; count];
    for beam in 0..count {
        let axis = axis_of[beam];
        depth_of[beam] = if beam == axis {
            beams.parent[beam].map_or(0, |p| depth_of[p as usize] + 1)
        } else {
            depth_of[axis]
        };
    }
    ranked.sort_by(|&a, &b| {
        depth_of[a]
            .cmp(&depth_of[b])
            .then(beyond_mass[b].total_cmp(&beyond_mass[a]))
            .then(a.cmp(&b))
    });
    let mut bone_of_axis: BTreeMap<usize, u32> = BTreeMap::new();
    for (bone, &axis) in ranked.iter().enumerate() {
        bone_of_axis.insert(axis, bone as u32);
    }
    // Each beam's bone: its axis's, else the bone its axis rides, which is
    // the bone of the beam its axis grows from.
    let mut bone_of = vec![0_u32; count];
    let mut rides = vec![false; count];
    for beam in 0..count {
        let axis = axis_of[beam];
        if let Some(&bone) = bone_of_axis.get(&axis) {
            bone_of[beam] = bone;
        } else {
            let from = beams.parent[axis].map_or(axis, |p| p as usize);
            bone_of[beam] = bone_of[from];
            rides[beam] = true;
        }
    }
    // Arc length from each bone's foot to each beam's end; a riding beam
    // keeps the place where its axis joins.
    let mut along = vec![0.0_f64; count];
    for beam in 0..count {
        let parent = beams.parent[beam].map(|p| p as usize);
        along[beam] = if rides[beam] {
            let axis = axis_of[beam];
            match beams.parent[axis] {
                Some(from) if beam == axis => along[from as usize],
                _ => parent.map_or(0.0, |p| along[p]),
            }
        } else if beam == axis_of[beam] {
            beams.length[beam]
        } else {
            parent.map_or(0.0, |p| along[p]) + beams.length[beam]
        };
    }
    let weights = beams.weights();
    let mut bones = Vec::with_capacity(ranked.len());
    for &axis in &ranked {
        let own = &axes[&axis];
        let bone = bone_of_axis[&axis];
        let first = own[0];
        let last = *own.last().unwrap_or(&first);
        let length = own.iter().map(|&beam| beams.length[beam]).sum::<f64>();
        let parent = beams.parent[first].map(|p| bone_of[p as usize]);
        // Its own mass: its beams' and those riding it.
        let carried: Vec<usize> = beams
            .run(first)
            .filter(|&beam| bone_of[beam] == bone)
            .collect();
        let mass = carried.iter().map(|&beam| beams.mass[beam]).sum::<f64>();
        let leaf_mass = carried.iter().map(|&beam| beams.leaf[beam]).sum::<f64>();
        let leaf_area = carried
            .iter()
            .map(|&beam| beams.leaf_area[beam])
            .sum::<f64>();
        let wood_area = carried
            .iter()
            .map(|&beam| (beams.radius[beam][0] + beams.radius[beam][1]) * beams.length[beam])
            .sum::<f64>();
        // Its frequencies: its own beams flexible, the rest rigid.
        let mut flexible = vec![false; count];
        for &beam in own {
            flexible[beam] = true;
        }
        let modes = beams.modes(2, Some(first), Some(&flexible), settings.iterations);
        let frequency = |index: usize| modes.get(index).map_or(0.0, |mode| mode.frequency);
        // The moment of everything it carries about its foot, across it.
        let foot = beams.start[first];
        let mut moment = Vec3::ZERO;
        for at in beams.run(first) {
            moment += (beams.end[at] - foot).cross(weights[at]);
        }
        let axis_dir = beams.axis[first];
        let bending_moment = (moment - axis_dir * axis_dir.dot(moment)).length();
        let r0 = beams.radius[first][0];
        bones.push(Bone {
            kind: if beams.parent[first].is_none() {
                BoneKind::Stem
            } else {
                BoneKind::Branch
            },
            parent,
            depth: depth_of[first],
            beams: own.iter().map(|&beam| beam as u32).collect(),
            foot,
            tip: beams.end[last],
            length,
            radius: [r0, beams.radius[last][1]],
            bending: beams.bending[first],
            mass,
            leaf_mass,
            mass_beyond: beyond_mass[first],
            leaf_area,
            wood_area,
            frequencies: [frequency(0), frequency(1)],
            weight_moment: bending_moment,
            breaking_moment: material
                .rupture
                .map(|rupture| rupture * 0.25 * math::PI * r0 * r0 * r0),
        });
    }
    // Bindings for every graph segment, through the wood bearing it.
    let mut binding: Vec<Option<Binding>> = vec![None; segments.len()];
    let mut wood_at: Vec<Option<u32>> = vec![None; segments.len()];
    for (index, segment) in segments.iter().enumerate() {
        wood_at[index] = beams.beam_of[index].or_else(|| {
            segment
                .parent
                .and_then(|parent| wood_at.get(parent as usize).copied().flatten())
        });
        binding[index] = wood_at[index].map(|beam| {
            let beam = beam as usize;
            let bone = bone_of[beam];
            let length = bones[bone as usize].length;
            Binding {
                bone,
                along: if length > 0.0 {
                    (along[beam] / length).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                },
            }
        });
    }
    let modes = beams.modes(settings.modes, None, None, settings.iterations);
    Mechanics {
        material: *material,
        beams,
        bones,
        binding,
        modes,
        axes: axes.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphSegment, PlantGraph};

    const MATERIAL: Material = Material {
        wood_density: 800.0,
        modulus: 10.0e9,
        rupture: Some(50.0e6),
        leaf_mass: 0.2,
    };

    fn segment(
        parent: Option<u32>,
        lateral: bool,
        start: Vec3,
        end: Vec3,
        radius: f64,
    ) -> GraphSegment {
        GraphSegment {
            id: 0,
            parent,
            lateral,
            order: 0,
            start,
            end,
            radius,
            born: 0.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
        }
    }

    /// A straight uniform rod from `foot` along `direction`, in `pieces`.
    fn rod(
        segments: &mut Vec<GraphSegment>,
        parent: Option<u32>,
        foot: Vec3,
        direction: Vec3,
        length: f64,
        radius: f64,
        pieces: usize,
    ) {
        #[allow(clippy::cast_precision_loss)]
        let step = length / pieces as f64;
        let mut parent = parent;
        let mut lateral = parent.is_some();
        for piece in 0..pieces {
            #[allow(clippy::cast_precision_loss)]
            let start = foot + direction * (step * piece as f64);
            segments.push(segment(
                parent,
                lateral,
                start,
                start + direction * step,
                radius,
            ));
            parent = u32::try_from(segments.len() - 1).ok();
            lateral = false;
        }
    }

    fn graph(segments: Vec<GraphSegment>) -> PlantGraph {
        PlantGraph {
            age: 1.0,
            height: 1.0,
            segments,
            organs: Vec::new(),
            structures: Vec::new(),
        }
    }

    fn close(found: f64, expected: f64, share: f64) {
        assert!(
            (found - expected).abs() <= share * expected.abs(),
            "found {found}, expected {expected} within {share}"
        );
    }

    /// The tip of the last beam moves, along `load`, by the work of the
    /// moments the load sets up, `∫ M·C M ds / |P|` over every beam from
    /// the foot to that tip (Simpson's rule, exact for a moment linear
    /// along each beam).
    fn tip_by_work(beams: &Beams, load: Vec3) -> f64 {
        let last = beams.len() - 1;
        let tip = beams.end[last];
        let mut chain = vec![last];
        while let Some(parent) = beams.parent[*chain.last().unwrap()] {
            chain.push(parent as usize);
        }
        let mut work = 0.0;
        for beam in chain {
            let axis = beams.axis[beam];
            let energy = |at: Vec3| {
                let moment = (tip - at).cross(load);
                let along = axis * axis.dot(moment);
                let across = moment - along;
                across.dot(across) / beams.bending[beam] + along.dot(along) / beams.torsion[beam]
            };
            let (start, end) = (beams.start[beam], beams.end[beam]);
            work += beams.length[beam] / 6.0
                * (energy(start) + 4.0 * energy((start + end) * 0.5) + energy(end));
        }
        work / load.length()
    }

    /// A cantilever bends `P L³ / 3EI` under a tip load whatever its
    /// pieces (the tip piece tapers, as the graph draws it, so the check
    /// is the moments' work, and in one piece the formula).
    #[test]
    fn cantilever_under_a_tip_load() {
        for pieces in [1, 7, 40] {
            let mut segments = Vec::new();
            rod(&mut segments, None, Vec3::ZERO, Vec3::X, 2.0, 0.05, pieces);
            let beams = Beams::of(&graph(segments), &[], &MATERIAL, &Settings::default());
            let load = Vec3::new(0.0, -100.0, 0.0);
            let mut loads = vec![Vec3::ZERO; beams.len()];
            *loads.last_mut().unwrap() = load;
            let moved = beams.deflect(&loads, None, None);
            close(-moved.last().unwrap().y, tip_by_work(&beams, load), 1.0e-9);
            if pieces == 1 {
                let ei = beams.bending[0];
                close(-moved[0].y, 100.0 * 8.0 / (3.0 * ei), 1.0e-9);
            }
        }
    }

    /// Under its own weight a uniform cantilever's tip falls `q L⁴ / 8EI`
    /// as its pieces shorten (lumping halves of each piece at its ends).
    #[test]
    fn cantilever_under_its_weight() {
        let (length, radius) = (3.0, 0.04);
        let mut segments = Vec::new();
        rod(
            &mut segments,
            None,
            Vec3::ZERO,
            Vec3::X,
            length,
            radius,
            200,
        );
        let beams = Beams::of(&graph(segments), &[], &MATERIAL, &Settings::default());
        let moved = beams.deflect(&beams.weights(), None, None);
        let q = MATERIAL.wood_density * math::PI * radius * radius * GRAVITY;
        let expected = q * length.powi(4) / (8.0 * beams.bending[0]);
        close(-moved.last().unwrap().y, expected, 0.01);
    }

    /// A uniform cantilever vibrates at `1.8751² / (2π L²) √(EI/μ)`, its
    /// second mode the same way at `4.6941²` (Euler–Bernoulli), each twice
    /// over: once each way across it.
    #[test]
    fn cantilever_vibrates_as_beam_theory_says() {
        let (length, radius) = (2.0, 0.03);
        let mut segments = Vec::new();
        rod(
            &mut segments,
            None,
            Vec3::ZERO,
            Vec3::Y,
            length,
            radius,
            120,
        );
        let beams = Beams::of(&graph(segments), &[], &MATERIAL, &Settings::default());
        let modes = beams.modes(4, None, None, 80);
        let mu = MATERIAL.wood_density * math::PI * radius * radius;
        let base = (beams.bending[0] / mu).sqrt() / (2.0 * math::PI * length * length);
        let first = 1.875_104_f64.powi(2) * base;
        let second = 4.694_091_f64.powi(2) * base;
        close(modes[0].frequency, first, 0.01);
        close(modes[1].frequency, first, 0.01);
        close(modes[2].frequency, second, 0.02);
        close(modes[3].frequency, second, 0.02);
    }

    /// An L of a post and an arm, pushed sideways at the arm's tip: the arm
    /// bends, the post bends, and the post twists (for uniform rods,
    /// `PA³/3EI + PH³/3EI + PA²H/GJ`).
    #[test]
    fn a_post_twists_under_its_arm() {
        let (height, arm, radius) = (1.5, 0.8, 0.02);
        let mut segments = Vec::new();
        rod(&mut segments, None, Vec3::ZERO, Vec3::Y, height, radius, 10);
        let top = u32::try_from(segments.len() - 1).ok();
        rod(
            &mut segments,
            top,
            Vec3::Y * height,
            Vec3::X,
            arm,
            radius,
            10,
        );
        let settings = Settings::default();
        let beams = Beams::of(&graph(segments), &[], &MATERIAL, &settings);
        let mut loads = vec![Vec3::ZERO; beams.len()];
        *loads.last_mut().unwrap() = Vec3::new(0.0, 0.0, 10.0);
        let moved = beams.deflect(&loads, None, None);
        // The post's and arm's tips taper as the graph draws them, so the
        // check is the moments' work (bending in both, twist in the post).
        let p = 10.0;
        let found = moved.last().unwrap().z;
        close(found, tip_by_work(&beams, Vec3::new(0.0, 0.0, p)), 1.0e-9);
        // Most of it is the post twisting.
        let gj = beams.torsion[0];
        assert!(found > p * arm * arm * height / gj);
    }

    /// A trunk with two branches and twigs: with two bones the twigs ride
    /// the branches' bones... and the branches ride the trunk's when there
    /// is one bone; every segment is bound to a bone, along it in order.
    #[test]
    fn light_axes_ride_heavier_ones() {
        let mut segments = Vec::new();
        rod(&mut segments, None, Vec3::ZERO, Vec3::Y, 4.0, 0.1, 8);
        let trunk_mid = Some(3_u32);
        rod(
            &mut segments,
            trunk_mid,
            Vec3::Y * 2.0,
            Vec3::new(0.7, 0.7, 0.0),
            2.0,
            0.04,
            4,
        );
        let branch_end = u32::try_from(segments.len() - 1).ok();
        rod(
            &mut segments,
            branch_end,
            Vec3::new(1.4, 3.4, 0.0),
            Vec3::X,
            0.4,
            0.01,
            2,
        );
        rod(
            &mut segments,
            Some(5),
            Vec3::Y * 3.0,
            Vec3::new(-0.7, 0.7, 0.0),
            1.0,
            0.02,
            3,
        );
        let plant = graph(segments);
        let settings = Settings {
            bones: 2,
            ..Settings::default()
        };
        let found = mechanics(&plant, &[], &MATERIAL, &settings);
        assert_eq!(found.axes, 4);
        assert_eq!(found.bones.len(), 2);
        assert_eq!(found.bones[0].kind, BoneKind::Stem);
        assert_eq!(found.bones[1].parent, Some(0));
        // The twig rides the branch's bone at the branch's tip.
        let twig = found.binding[12].unwrap();
        assert_eq!(twig.bone, 1);
        assert!((twig.along - 1.0).abs() < 1.0e-6);
        // The small branch rides the trunk where it joins (end of piece 5).
        let small = found.binding[14].unwrap();
        assert_eq!(small.bone, 0);
        assert!((small.along - 0.75).abs() < 1.0e-6, "{}", small.along);
        // Along each bone the bindings grow.
        let trunk: Vec<f32> = (0..8).map(|at| found.binding[at].unwrap().along).collect();
        assert!(trunk.windows(2).all(|pair| pair[1] > pair[0]));
        // Coarse first: every parent earlier and shallower.
        for (at, bone) in found.bones.iter().enumerate() {
            if let Some(parent) = bone.parent {
                assert!((parent as usize) < at);
                assert_eq!(found.bones[parent as usize].depth + 1, bone.depth);
            }
        }
        // Masses add up.
        let total: f64 = found.bones.iter().map(|bone| bone.mass).sum();
        close(total, found.mass() - found.beams.grounded, 1.0e-12);
        close(found.bones[0].mass_beyond, total, 1.0e-12);
        // A stiffer trunk than branch; each bone's own frequency is above the
        // whole plant's lowest, which lets every part move.
        for bone in &found.bones {
            assert!(bone.frequencies[0] > found.modes[0].frequency * 0.999);
            assert!(bone.frequencies[0] <= bone.frequencies[1] * (1.0 + 1.0e-9));
        }
        assert!(found.bones[0].breaking_moment.unwrap() > found.bones[0].weight_moment);
    }

    #[test]
    fn material_names_what_is_missing() {
        let traits: Map<String, Value> = serde_json::from_str(
            r#"{"wood_density_basic_kg_m3": 440, "wood_modulus_green_mpa": {"min": 9000, "max": 11000}}"#,
        )
        .unwrap();
        assert_eq!(
            Material::of(&traits),
            Err(vec!["wood_moisture_green_share", "leaf_mass_fresh_kg_m2"])
        );
        let traits: Map<String, Value> = serde_json::from_str(
            r#"{"wood_density_basic_kg_m3": 440, "wood_moisture_green_share": 0.5,
                "wood_modulus_green_mpa": {"min": 9000, "max": 11000}, "leaf_mass_fresh_kg_m2": 0.3}"#,
        )
        .unwrap();
        let material = Material::of(&traits).unwrap();
        close(material.wood_density, 660.0, 1.0e-12);
        close(material.modulus, 10.0e9, 1.0e-12);
        assert_eq!(material.rupture, None);
    }
}
