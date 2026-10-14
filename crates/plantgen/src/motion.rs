//! What a package gives a host to sway and prune its plants by (plant
//! motion S2), built only when asked ([`crate::package::Inputs::with_motion`]).
//!
//! The contract with hosts is in jferments/after-claude
//! `project-files/plant-physics/CONTRACT.md`. Per keyframe a package gains
//! one `mechanics` object ([`record`], JSON): the material, the bones of
//! [`crate::mechanics`] with their stiffness, frequencies, strength, bend
//! limits, reach and snow areas, the branches a host can prune, each with
//! the stub cap a prune leaves, the perches a body can stand on, the
//! anchorage that holds the plant up and the wood that passes through
//! other wood as grown. Per level of detail it gains one `binding` object
//! ([`encode_binding`]), a stream beside the mesh, never inside it:
//!
//! ```text
//! APBIND1, u32 vertex count V, u32×V, u32 card count C, u32×C
//! ```
//!
//! Each entry is a bone (low 16 bits) and the position along it (the next
//! 8 bits, 0 at its foot and 255 at its tip); the high 8 bits are 0. Bone
//! [`GROUND`] is the ground: roots, and a rosette's leaves no segment
//! bears, which sway only by their own cards. Every level binds to the
//! same bones. Static packages keep their bytes and their key.
//!
//! A species that does not state a material value has it from [`GENERIC`],
//! a middling green wood, and the record names it in `assumed`.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::graph::{GraphSegment, PlantGraph};
use crate::lsys::OrganKind;
use crate::math::Vec3;
use crate::mechanics::{self, BoneKind, Material, Mechanics, Settings};
use crate::mesh::{Mesh, PlantMesh};

/// The motion objects' revision: a change to what they hold or how they
/// are computed moves it, and with it the key of every motion package.
pub const MECHANICS_REVISION: u32 = 2;

/// The bone a vertex or card the ground bears is bound to.
pub const GROUND: u32 = 0xFFFF;

/// The magic of a `binding` object.
pub const BINDING_MAGIC: [u8; 8] = *b"APBIND1\0";

/// Values for a material character a species does not state: a middling
/// green wood and leaf, round numbers inside the range of the green values
/// the Wood Handbook (FPL-GTR-282) gives common North American species
/// (not a computed median), and about what a broad leaf weighs fresh a
/// square metre. They stand in so every plant sways; a record lists each
/// one used.
pub const GENERIC: [(&str, f64); 5] = [
    ("wood_density_basic_kg_m3", 450.0),
    ("wood_moisture_green_share", 0.8),
    ("wood_modulus_green_mpa", 8_000.0),
    ("wood_rupture_green_mpa", 45.0),
    ("leaf_mass_fresh_kg_m2", 0.2),
];

/// A bone's damping ratio, of its critical damping: wood's own and the
/// air's together, about what swaying branches and young trees show
/// (assumed; stated per species later).
pub const DAMPING: f64 = 0.05;

/// Thinner than this at its foot, m, a bone is a twig; thinner still than
/// [`BRUSH`], brush a body pushes through.
pub const TWIG: f64 = 0.02;
pub const BRUSH: f64 = 0.005;

/// The turning moment that overturns a plant, N m per kg of its stem's
/// wood (assumed): a middling value of the straight-line fits that
/// tree-pulling studies give for the critical turning moment against stem
/// mass (for example Nicoll et al. 2006, Canadian Journal of Forest
/// Research 36), until species and soils state their own.
pub const ANCHORAGE_PER_KG: f64 = 150.0;

/// How far the stem's foot turns, radians, as its roots give way at the
/// overturning moment (assumed: a few degrees, as pulled trees show); the
/// anchorage's stiffness is the moment over it.
pub const ROOT_TURN: f64 = 0.07;

/// Where along a branch or twig bone a perch is offered, and the steepest
/// it may slope there: the sine of 40° from level.
pub const PERCH_ALONG: [f64; 3] = [0.25, 0.5, 0.75];
pub const PERCH_SLOPE: f64 = 0.643;

/// How a package's plants are given motion: their material and the
/// settings their bones are found with.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub material: Material,
    /// The characters taken from [`GENERIC`] (and `damping`, always).
    pub assumed: Vec<&'static str>,
    pub settings: Settings,
}

impl Motion {
    /// The motion of a species with characters `traits` (as it inherits
    /// them), filling what it does not state from [`GENERIC`].
    ///
    /// # Panics
    ///
    /// Never: every value [`Material::of`] needs is filled first.
    #[must_use]
    pub fn of(traits: &Map<String, Value>) -> Self {
        let mut filled = traits.clone();
        let mut assumed = Vec::new();
        for (key, value) in GENERIC {
            if crate::checks::Stated::of(traits.get(key).unwrap_or(&Value::Null)).is_none() {
                filled.insert(key.to_string(), Value::from(value));
                assumed.push(key);
            }
        }
        assumed.push("damping");
        assumed.push("anchorage");
        let material = Material::of(&filled).expect("every needed value is filled");
        Self {
            material,
            assumed,
            settings: Settings {
                modes: 0,
                ..Settings::default()
            },
        }
    }

    /// What the package key hashes for it: the revision, the material and
    /// the settings, so a change to any re-keys the package.
    #[must_use]
    pub fn key_bytes(&self) -> Vec<u8> {
        let m = &self.material;
        let s = &self.settings;
        format!(
            "{MECHANICS_REVISION} {:?} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} {}",
            m.wood_density,
            m.modulus,
            m.rupture,
            m.leaf_mass,
            self.assumed,
            s.shear,
            s.bones,
            s.piece,
            s.bend_share,
            s.exact
        )
        .into_bytes()
    }

    /// The mechanics of one grown keyframe.
    #[must_use]
    pub fn of_graph(&self, graph: &PlantGraph, types: &[crate::graph::OrganType]) -> Mechanics {
        mechanics::mechanics(graph, types, &self.material, &self.settings)
    }
}

/// The `mechanics` object of one keyframe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub revision: u32,
    pub material: MaterialRecord,
    /// Everything the plant weighs, kg.
    pub mass: f64,
    pub bones: Vec<BoneRecord>,
    pub branches: Vec<BranchRecord>,
    pub anchorage: Anchorage,
    /// Places on branches and twigs a body can stand on.
    pub perches: Vec<Perch>,
    /// Wood passing through other wood, as grown.
    pub crossings: Crossings,
}

/// What holds the plant up at its foot (assumed per kg of stem; see
/// [`ANCHORAGE_PER_KG`] and [`ROOT_TURN`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Anchorage {
    /// The turning moment at the stem's foot that overturns it, N m.
    pub moment: f64,
    /// Its rotational stiffness, N m per radian.
    pub stiffness: f64,
}

/// A place on top of a branch or twig bone to stand or land on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Perch {
    pub bone: u32,
    /// Its place along the bone, 0 at its foot and 1 at its tip.
    pub along: f64,
    /// The point on top of the wood there, m.
    pub point: [f64; 3],
    /// The branch's unit direction there.
    pub direction: [f64; 3],
    /// The wood's radius there, m.
    pub radius: f64,
    /// The weight it takes there, N, before the moment at the bone's foot
    /// passes the share of its breaking moment that [`mechanics::Bone::bend_limit`]
    /// holds it to; null where strength is unstated.
    pub load: Option<f64>,
}

/// Wood through wood: pairs of wood segments (not fleshy bodies, fronds or
/// blades) whose wood overlaps, apart from
/// those that meet at a joint (the way between them along the plant
/// shorter than twice their radii together).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Crossings {
    pub pairs: u32,
    /// The deepest overlap, m: the radii together less the axes' distance.
    pub deepest: f64,
    /// The share of the wood segments in a crossing.
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MaterialRecord {
    /// Green wood, kg/m³.
    pub wood_density: f64,
    /// Along the grain, Pa.
    pub modulus: f64,
    /// Green modulus of rupture, Pa.
    pub rupture: Option<f64>,
    /// Fresh, kg/m² of one side.
    pub leaf_mass: Option<f64>,
    pub damping: f64,
    pub assumed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoneRecord {
    pub parent: Option<u32>,
    pub depth: u32,
    /// `trunk`, `branch`, `twig` or `brush`.
    pub kind: &'static str,
    /// Its foot, and its rest frame: the turn (a unit quaternion x, y, z,
    /// w) taking +Y to the line from its foot to its tip.
    pub origin: [f64; 3],
    pub frame: [f64; 4],
    pub tip: [f64; 3],
    pub length: f64,
    pub radius_base: f64,
    pub radius_tip: f64,
    pub mass: f64,
    /// Metres along it.
    pub centre_of_mass: f64,
    pub mass_beyond: f64,
    /// Bending stiffness at its foot, N m².
    pub ei: f64,
    /// Hz, lowest first.
    pub frequency: [f64; 2],
    pub damping: f64,
    pub drag_area: DragArea,
    /// The bending moment its load puts on its foot at rest, N m.
    pub weight_moment: f64,
    /// The bending moment at its foot that breaks it, N m.
    pub strength: Option<f64>,
    /// The largest rest bend, radians ([`mechanics::Bone::bend_limit`]).
    pub bend_limit: Option<f64>,
    /// How far its wood and foliage, and those riding it, reach off the
    /// line from its foot to its tip, m: the radius of a capsule round it.
    pub reach: f64,
    /// Upward-facing areas that hold snow, m²: its wood's and riding
    /// axes' seen from above, and its foliage's, each leaf's one side
    /// times how level it lies.
    pub snow_area: SnowArea,
    /// Crossings ([`Crossings`]) its segments are in.
    pub crossings: u32,
}

/// Areas for drag, m²: its wood's frontal area (twice radius times
/// length) and its foliage's one-sided area, which a host turns to a
/// frontal area with its own reconfiguration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DragArea {
    pub wood: f64,
    pub foliage: f64,
}

/// Areas that hold snow, m²: wood seen from above, and foliage's one side
/// times how level it lies.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct SnowArea {
    pub wood: f64,
    pub foliage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BranchRecord {
    /// Its first segment's graph id, the same at every age, in hex.
    pub id: String,
    pub parent: Option<u32>,
    pub bone: u32,
    pub chain: Vec<u32>,
    pub base: [f64; 3],
    pub direction: [f64; 3],
    pub order: u16,
    pub length: f64,
    /// m³.
    pub volume: f64,
    pub mass_beyond: f64,
    pub stub: StubRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StubRecord {
    pub centre: [f64; 3],
    pub normal: [f64; 3],
    pub radius: f64,
}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The shortest turn taking +Y to `direction`, as x, y, z, w.
#[must_use]
pub fn turn_from_up(direction: Vec3) -> [f64; 4] {
    let d = direction.normalize_or(Vec3::Y);
    let w = 1.0 + d.y;
    if w < 1.0e-9 {
        // Straight down: half a turn about X.
        return [1.0, 0.0, 0.0, 0.0];
    }
    let axis = Vec3::Y.cross(d);
    let norm = (axis.dot(axis) + w * w).sqrt();
    [axis.x / norm, axis.y / norm, axis.z / norm, w / norm]
}

/// The `mechanics` record of `found`, made with `motion` from `graph`
/// (grown with organ types `types`).
#[must_use]
pub fn record(
    found: &Mechanics,
    motion: &Motion,
    graph: &PlantGraph,
    types: &[crate::graph::OrganType],
) -> Record {
    let material = &found.material;
    let around = Surroundings::of(graph, types, found);
    let bones = found
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| BoneRecord {
            parent: bone.parent,
            depth: bone.depth,
            kind: match bone.kind {
                BoneKind::Stem => "trunk",
                BoneKind::Branch if bone.radius[0] >= TWIG => "branch",
                BoneKind::Branch if bone.radius[0] >= BRUSH => "twig",
                BoneKind::Branch => "brush",
            },
            origin: array(bone.foot),
            frame: turn_from_up(bone.tip - bone.foot),
            tip: array(bone.tip),
            length: bone.length,
            radius_base: bone.radius[0],
            radius_tip: bone.radius[1],
            mass: bone.mass,
            centre_of_mass: bone.centre_of_mass,
            mass_beyond: bone.mass_beyond,
            ei: bone.bending,
            frequency: bone.frequencies,
            damping: DAMPING,
            drag_area: DragArea {
                wood: bone.wood_area,
                foliage: bone.leaf_area,
            },
            weight_moment: bone.weight_moment,
            strength: bone.breaking_moment,
            bend_limit: bone.bend_limit,
            reach: around.reach[index],
            snow_area: around.snow[index],
            crossings: around.bone_crossings[index],
        })
        .collect();
    let branches = found
        .branches
        .iter()
        .map(|branch| BranchRecord {
            id: format!("{:016x}", branch.id),
            parent: branch.parent,
            bone: branch.bone,
            chain: branch.chain.clone(),
            base: array(branch.base),
            direction: array(branch.direction),
            order: branch.order,
            length: branch.length,
            volume: branch.volume,
            mass_beyond: branch.mass_beyond,
            stub: StubRecord {
                centre: array(branch.stub.centre),
                normal: array(branch.stub.normal),
                radius: branch.stub.radius,
            },
        })
        .collect();
    Record {
        revision: MECHANICS_REVISION,
        material: MaterialRecord {
            wood_density: material.wood_density,
            modulus: material.modulus,
            rupture: material.rupture,
            leaf_mass: material.leaf_mass,
            damping: DAMPING,
            assumed: motion
                .assumed
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
        },
        mass: found.mass(),
        bones,
        branches,
        anchorage: anchorage(found),
        perches: perches(found, motion.settings.bend_share),
        crossings: around.crossings,
    }
}

/// What a plant's bones gather from the graph around them: how far each
/// reaches, the snow each holds, and the wood through wood.
struct Surroundings {
    reach: Vec<f64>,
    snow: Vec<SnowArea>,
    bone_crossings: Vec<u32>,
    crossings: Crossings,
}

/// The distance from `point` to the line from `a` to `b` (its ends past
/// them).
fn off_line(point: Vec3, a: Vec3, b: Vec3) -> f64 {
    let span = b - a;
    let length = span.dot(span);
    let t = if length > 0.0 {
        ((point - a).dot(span) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    point.distance(a + span * t)
}

/// The closest distance between the segments `p0`–`p1` and `q0`–`q1`.
#[allow(clippy::many_single_char_names)]
fn between(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> f64 {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (s, t) = if a <= 1.0e-18 && e <= 1.0e-18 {
        (0.0, 0.0)
    } else if a <= 1.0e-18 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= 1.0e-18 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let mut s = if denominator > 1.0e-18 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p0 + d1 * s).distance(q0 + d2 * t)
}

impl Surroundings {
    #[allow(clippy::too_many_lines)]
    fn of(graph: &PlantGraph, types: &[crate::graph::OrganType], found: &Mechanics) -> Self {
        let count = found.bones.len();
        let mut reach = vec![0.0_f64; count];
        let mut snow = vec![SnowArea::default(); count];
        let segments = &graph.segments;
        let end_radii = graph.end_radii();
        let bone_of = |segment: usize| {
            found
                .binding
                .get(segment)
                .copied()
                .flatten()
                .map(|binding| binding.bone as usize)
                .filter(|&bone| bone < count)
        };
        for (index, segment) in segments.iter().enumerate() {
            let Some(bone) = bone_of(index) else {
                continue;
            };
            let (foot, tip) = (found.bones[bone].foot, found.bones[bone].tip);
            let (r0, r1) = (segment.radius, end_radii[index]);
            reach[bone] = reach[bone]
                .max(off_line(segment.start, foot, tip) + r0)
                .max(off_line(segment.end, foot, tip) + r1);
            let span = segment.end - segment.start;
            let level = Vec3::new(span.x, 0.0, span.z).length();
            snow[bone].wood += (r0 + r1) * level;
        }
        for organ in &graph.organs {
            let Some(bone) = organ.segment.and_then(|segment| bone_of(segment as usize)) else {
                continue;
            };
            let (foot, tip) = (found.bones[bone].foot, found.bones[bone].tip);
            reach[bone] = reach[bone].max(off_line(organ.position, foot, tip) + organ.size);
            if let Some(kind) = types.get(usize::from(organ.organ))
                && matches!(kind.kind, OrganKind::Leaf | OrganKind::Foliage)
            {
                let facing = organ
                    .heading
                    .cross(organ.left)
                    .normalize_or(Vec3::Y)
                    .y
                    .abs();
                snow[bone].foliage += kind.area * organ.size * organ.size * facing;
            }
        }
        let (bone_crossings, crossings) = Self::crossings(graph, &end_radii, &bone_of, count);
        Self {
            reach,
            snow,
            bone_crossings,
            crossings,
        }
    }

    /// Wood through wood ([`Crossings`]), and the crossings each bone's
    /// segments are in: segments' capsules (their larger radius) on a grid.
    #[allow(clippy::too_many_lines)]
    fn crossings(
        graph: &PlantGraph,
        end_radii: &[f64],
        bone_of: &dyn Fn(usize) -> Option<usize>,
        count: usize,
    ) -> (Vec<u32>, Crossings) {
        let segments = &graph.segments;
        let n = segments.len();
        let mut per_bone = vec![0_u32; count];
        let none = Crossings {
            pairs: 0,
            deepest: 0.0,
            share: 0.0,
        };
        if n < 2 {
            return (per_bone, none);
        }
        let radius = |at: usize| segments[at].radius.max(end_radii[at]);
        let mut lengths: Vec<f64> = segments
            .iter()
            .map(|segment| segment.start.distance(segment.end))
            .collect();
        let middle = n / 2;
        lengths.select_nth_unstable_by(middle, f64::total_cmp);
        // Cells about twice a typical segment, and big enough that the
        // largest crosses at most eight a side.
        let largest = (0..n)
            .map(|at| segments[at].start.distance(segments[at].end) + 2.0 * radius(at))
            .fold(0.0, f64::max);
        let cell = (lengths[middle] * 2.0).clamp(0.05, 1.0).max(largest / 8.0);
        #[allow(clippy::cast_possible_truncation)]
        let key = |v: f64| (v / cell).floor() as i64;
        let reach = |at: usize| {
            let (a, b, r) = (segments[at].start, segments[at].end, radius(at));
            (
                [
                    key(a.x.min(b.x) - r),
                    key(a.y.min(b.y) - r),
                    key(a.z.min(b.z) - r),
                ],
                [
                    key(a.x.max(b.x) + r),
                    key(a.y.max(b.y) + r),
                    key(a.z.max(b.z) + r),
                ],
            )
        };
        // Wood only: not fleshy bodies, nor the axes of fronds and blades.
        let wood: Vec<bool> = segments
            .iter()
            .map(|segment| segment.body == 0 && segment.within.is_none())
            .collect();
        let mut grid: std::collections::HashMap<[i64; 3], Vec<u32>> =
            std::collections::HashMap::new();
        for at in (0..n).filter(|&at| wood[at]) {
            let (low, high) = reach(at);
            for x in low[0]..=high[0] {
                for y in low[1]..=high[1] {
                    for z in low[2]..=high[2] {
                        grid.entry([x, y, z])
                            .or_default()
                            .push(u32::try_from(at).unwrap_or(u32::MAX));
                    }
                }
            }
        }
        // Two segments meet at a joint, and may overlap there, when the
        // way between them along the plant is shorter than twice their
        // radii together: a branch still inside a thick trunk. `from_root`
        // is how far each segment's start is from its plant's foot along
        // the plant (parents come first).
        let parent = |at: usize| segments[at].parent.map(|p| p as usize);
        let mut from_root = vec![0.0; n];
        for at in 0..n {
            if let Some(p) = parent(at) {
                from_root[at] = from_root[p] + segments[p].start.distance(segments[p].end);
            }
        }
        // The way from `at`'s start back to the end of `up`, an ancestor or
        // itself: none for itself.
        let back = |at: usize, up: usize| {
            (from_root[at] - from_root[up] - segments[up].start.distance(segments[up].end)).max(0.0)
        };
        let near = |a: usize, b: usize| {
            let limit = 2.0 * (radius(a) + radius(b));
            let (mut x, mut y) = (a, b);
            loop {
                // Going up only lengthens the way.
                if back(a, x) + back(b, y) > limit {
                    return false;
                }
                if x == y {
                    return true;
                }
                let up = if from_root[x] >= from_root[y] {
                    parent(x).map(|p| x = p)
                } else {
                    parent(y).map(|p| y = p)
                };
                if up.is_none() && parent(x).is_none() && parent(y).is_none() {
                    return false;
                }
                if up.is_none() {
                    // One way up is at its foot: go up the other.
                    if let Some(p) = parent(x) {
                        x = p;
                    } else if let Some(p) = parent(y) {
                        y = p;
                    }
                }
            }
        };
        let mut seen = vec![usize::MAX; n];
        let mut crossed = vec![false; n];
        let (mut pairs, mut deepest) = (0_u32, 0.0_f64);
        for a in (0..n).filter(|&a| wood[a]) {
            let (low, high) = reach(a);
            for x in low[0]..=high[0] {
                for y in low[1]..=high[1] {
                    for z in low[2]..=high[2] {
                        let Some(list) = grid.get(&[x, y, z]) else {
                            continue;
                        };
                        for &b in list {
                            let b = b as usize;
                            if b <= a || seen[b] == a {
                                continue;
                            }
                            seen[b] = a;
                            let (sa, sb) = (&segments[a], &segments[b]);
                            let depth =
                                radius(a) + radius(b) - between(sa.start, sa.end, sb.start, sb.end);
                            if depth > 0.0 && !near(a, b) {
                                pairs += 1;
                                deepest = deepest.max(depth);
                                crossed[a] = true;
                                crossed[b] = true;
                                for at in [a, b] {
                                    if let Some(bone) = bone_of(at) {
                                        per_bone[bone] += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let share = crossed.iter().filter(|&&crossed| crossed).count() as f64
            / wood.iter().filter(|&&wood| wood).count().max(1) as f64;
        (
            per_bone,
            Crossings {
                pairs,
                deepest,
                share,
            },
        )
    }
}

/// What holds the plant up at its foot: [`ANCHORAGE_PER_KG`] per kg of its
/// stems' wood, and the stiffness that gives way at [`ROOT_TURN`].
fn anchorage(found: &Mechanics) -> Anchorage {
    let stem: f64 = found
        .bones
        .iter()
        .filter(|bone| bone.depth == 0)
        .map(|bone| (bone.mass - bone.leaf_mass).max(0.0))
        .sum();
    let moment = ANCHORAGE_PER_KG * stem;
    Anchorage {
        moment,
        stiffness: moment / ROOT_TURN,
    }
}

/// Perches on branch and twig bones: at [`PERCH_ALONG`], where the bone
/// slopes at most [`PERCH_SLOPE`], the point on top of its wood and the
/// weight it takes there before the moment at its foot passes
/// `bend_share` of its breaking moment.
fn perches(found: &Mechanics, bend_share: f64) -> Vec<Perch> {
    let mut out = Vec::new();
    for (index, bone) in found.bones.iter().enumerate() {
        if bone.depth == 0 || bone.radius[0] < BRUSH || bone.length <= 0.0 {
            continue;
        }
        let span = bone.tip - bone.foot;
        let direction = span.normalize_or(Vec3::Y);
        if direction.y.abs() > PERCH_SLOPE {
            continue;
        }
        let up = (Vec3::Y - direction * direction.y).normalize_or(Vec3::Y);
        for along in PERCH_ALONG {
            let radius = bone.radius[0] + (bone.radius[1] - bone.radius[0]) * along;
            if radius < BRUSH * 0.5 {
                continue;
            }
            let centre = bone.foot + span * along;
            // The weight's lever about the foot is level.
            let lever = Vec3::new(span.x, 0.0, span.z).length() * along;
            let load = bone.breaking_moment.map(|breaking| {
                ((bend_share * breaking - bone.weight_moment).max(0.0)) / lever.max(1.0e-6)
            });
            out.push(Perch {
                bone: u32::try_from(index).unwrap_or(u32::MAX),
                along,
                point: array(centre + up * radius),
                direction: array(direction),
                radius,
                load,
            });
        }
    }
    out
}

/// One level's binding: per wood vertex and per card, its bone and place
/// along it, packed as the `binding` object holds them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bound {
    pub wood: Vec<u32>,
    pub cards: Vec<u32>,
    /// Wood vertices and cards nothing was recorded to bear, bound to the
    /// ground as roots are.
    pub unborne: usize,
}

/// A bone and a place along it (0 to 1), packed.
#[must_use]
pub fn pack(bone: u32, along: f32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let along = (along.clamp(0.0, 1.0) * 255.0).round() as u32;
    (bone.min(0xFFFF)) | (along << 16)
}

/// The bone and place along it `pack` packed.
#[must_use]
pub fn unpack(packed: u32) -> (u32, f32) {
    #[allow(clippy::cast_precision_loss)]
    (packed & 0xFFFF, ((packed >> 16) & 0xFF) as f32 / 255.0)
}

/// Where `point` falls along `segment`, 0 at its start and 1 at its end.
fn share_along(segment: &GraphSegment, point: Vec3) -> f32 {
    let span = segment.end - segment.start;
    let length = span.dot(span);
    if length <= 0.0 {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    {
        ((point - segment.start).dot(span) / length).clamp(0.0, 1.0) as f32
    }
}

/// What `bearer` (a graph segment, [`crate::mesh::GROUNDED`] or
/// [`crate::mesh::NO_SEGMENT`]) binds a point at `point` to among the bones
/// of `found`, packed; none when nothing bears it or no bone binds its
/// segment.
fn bound_at(found: &Mechanics, graph: &PlantGraph, bearer: u32, point: Vec3) -> Option<u32> {
    if bearer == crate::mesh::GROUNDED {
        return Some(pack(GROUND, 0.0));
    }
    if bearer == crate::mesh::NO_SEGMENT {
        return None;
    }
    let binding = found.binding.get(bearer as usize).copied().flatten()?;
    let t = share_along(&graph.segments[bearer as usize], point);
    Some(pack(
        binding.bone,
        binding.from + (binding.along - binding.from) * t,
    ))
}

fn point(position: [f32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(position[0]),
        f64::from(position[1]),
        f64::from(position[2]),
    )
}

/// Each vertex of `mesh`, drawn from `graph`, bound to the bones of
/// `found`, packed; and how many nothing bears, bound to the ground.
#[must_use]
pub fn bind_mesh(mesh: &Mesh, graph: &PlantGraph, found: &Mechanics) -> (Vec<u32>, usize) {
    let mut unborne = 0;
    let bound = mesh
        .positions
        .iter()
        .enumerate()
        .map(|(index, &position)| {
            bound_at(found, graph, mesh.bearer(index), point(position)).unwrap_or_else(|| {
                unborne += 1;
                pack(GROUND, 0.0)
            })
        })
        .collect();
    (bound, unborne)
}

/// The binding of `plant`, drawn from `graph`, to the bones of `found`.
#[must_use]
pub fn bind(plant: &PlantMesh, graph: &PlantGraph, found: &Mechanics) -> Bound {
    let (wood, mut unborne) = bind_mesh(&plant.wood, graph, found);
    let cards = plant
        .cards
        .iter()
        .map(|card| {
            bound_at(found, graph, card.segment, point(card.base)).unwrap_or_else(|| {
                unborne += 1;
                pack(GROUND, 0.0)
            })
        })
        .collect();
    Bound {
        wood,
        cards,
        unborne,
    }
}

/// A `binding` object's bytes.
#[must_use]
pub fn encode_binding(bound: &Bound) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + 4 * (bound.wood.len() + bound.cards.len()));
    out.extend_from_slice(&BINDING_MAGIC);
    for list in [&bound.wood, &bound.cards] {
        let count = u32::try_from(list.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&count.to_le_bytes());
        for entry in list {
            out.extend_from_slice(&entry.to_le_bytes());
        }
    }
    out
}

/// A `binding` object read back.
///
/// # Errors
///
/// When the bytes are not one.
pub fn decode_binding(bytes: &[u8]) -> Result<Bound, String> {
    let rest = bytes
        .strip_prefix(&BINDING_MAGIC[..])
        .ok_or("not an APBIND1 binding")?;
    let (chunks, _) = rest.as_chunks::<4>();
    let mut words = chunks.iter().map(|&chunk| u32::from_le_bytes(chunk));
    let mut list = || -> Result<Vec<u32>, String> {
        let count = words.next().ok_or("a binding ends early")? as usize;
        let list: Vec<u32> = words.by_ref().take(count).collect();
        if list.len() == count {
            Ok(list)
        } else {
            Err("a binding ends early".to_string())
        }
    };
    let wood = list()?;
    let cards = list()?;
    if rest.len() != 8 + 4 * (wood.len() + cards.len()) {
        return Err("a binding has bytes after its cards".into());
    }
    Ok(Bound {
        wood,
        cards,
        unborne: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_keeps_bone_and_place() {
        for (bone, along) in [(0, 0.0), (7, 0.5), (511, 1.0), (65_535, 0.25)] {
            let (b, a) = unpack(pack(bone, along));
            assert_eq!(b, bone);
            assert!((a - along).abs() <= 0.5 / 255.0 + 1.0e-6);
        }
        assert_eq!(pack(3, 2.0) >> 24, 0);
    }

    #[test]
    fn a_binding_reads_back() {
        let bound = Bound {
            wood: vec![pack(1, 0.5), pack(2, 1.0)],
            cards: vec![pack(0, 0.0)],
            unborne: 0,
        };
        let bytes = encode_binding(&bound);
        assert_eq!(bytes.len(), 8 + 4 + 8 + 4 + 4);
        assert_eq!(decode_binding(&bytes).unwrap(), bound);
        assert!(decode_binding(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn frames_turn_up_onto_the_bone() {
        for direction in [Vec3::Y, Vec3::X, -Vec3::Y, Vec3::new(1.0, 2.0, -0.5)] {
            let [x, y, z, w] = turn_from_up(direction);
            // Rotate +Y by the quaternion.
            let q = Vec3::new(x, y, z);
            let v = Vec3::Y;
            let t = q.cross(v) * 2.0;
            let turned = v + t * w + q.cross(t);
            let d = direction.normalize_or(Vec3::Y);
            assert!(turned.distance(d) < 1.0e-9, "{direction:?} -> {turned:?}");
        }
    }

    fn segment(parent: Option<u32>, start: Vec3, end: Vec3, radius: f64) -> GraphSegment {
        GraphSegment {
            id: 0,
            parent,
            lateral: false,
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

    /// A straight rod from `foot` along `direction`, in `pieces`, growing
    /// from `parent`.
    fn rod(
        segments: &mut Vec<GraphSegment>,
        parent: Option<u32>,
        foot: Vec3,
        direction: Vec3,
        (length, radius, pieces): (f64, f64, u32),
    ) {
        let step = length / f64::from(pieces);
        let mut parent = parent;
        for piece in 0..pieces {
            let start = foot + direction * (step * f64::from(piece));
            let mut made = segment(parent, start, start + direction * step, radius);
            made.lateral = piece == 0 && parent.is_some();
            made.id = 100 + segments.len() as u64;
            segments.push(made);
            parent = u32::try_from(segments.len() - 1).ok();
        }
    }

    fn graph(segments: Vec<GraphSegment>) -> PlantGraph {
        PlantGraph {
            age: 1.0,
            height: 4.0,
            segments,
            organs: Vec::new(),
            structures: Vec::new(),
        }
    }

    /// A 4 m trunk and a level branch 1.5 m long from it at 2 m; with
    /// `crossing`, a second branch from 3 m slanting down through the
    /// first.
    fn tree(crossing: bool) -> PlantGraph {
        let mut segments = Vec::new();
        rod(&mut segments, None, Vec3::ZERO, Vec3::Y, (4.0, 0.1, 8));
        rod(
            &mut segments,
            Some(3),
            Vec3::Y * 2.0,
            Vec3::X,
            (1.5, 0.03, 3),
        );
        if crossing {
            let down = Vec3::new(1.0, -1.0, 0.0).normalize_or(Vec3::Y);
            rod(&mut segments, Some(5), Vec3::Y * 3.0, down, (1.6, 0.03, 4));
        }
        graph(segments)
    }

    fn found(plant: &PlantGraph) -> (Mechanics, Motion) {
        let traits: Map<String, Value> =
            serde_json::from_str(r#"{"wood_rupture_green_mpa": 50}"#).unwrap();
        let motion = Motion::of(&traits);
        (motion.of_graph(plant, &[]), motion)
    }

    #[test]
    fn segments_between_measure_their_closest_points() {
        let close = |found: f64, expected: f64| assert!((found - expected).abs() < 1.0e-9);
        // Square across, 0.5 apart; side by side; end to end.
        close(
            between(
                Vec3::ZERO,
                Vec3::X * 2.0,
                Vec3::new(1.0, 0.5, -1.0),
                Vec3::new(1.0, 0.5, 1.0),
            ),
            0.5,
        );
        close(
            between(Vec3::ZERO, Vec3::X, Vec3::Y * 0.3, Vec3::Y * 0.3 + Vec3::X),
            0.3,
        );
        close(
            between(Vec3::ZERO, Vec3::X, Vec3::X * 3.0, Vec3::X * 4.0),
            2.0,
        );
        close(off_line(Vec3::new(0.5, 2.0, 0.0), Vec3::ZERO, Vec3::X), 2.0);
    }

    #[test]
    fn wood_through_wood_is_counted_but_not_at_joints() {
        let plant = tree(false);
        let (mechanics, motion) = found(&plant);
        let clear = record(&mechanics, &motion, &plant, &[]);
        assert_eq!(
            clear.crossings.pairs, 0,
            "a branch meets its trunk at a joint"
        );
        let plant = tree(true);
        let (mechanics, motion) = found(&plant);
        let crossed = record(&mechanics, &motion, &plant, &[]);
        assert!(crossed.crossings.pairs >= 1);
        assert!(crossed.crossings.deepest > 0.0 && crossed.crossings.deepest <= 0.06);
        // Both branches' bones know it; the trunk's do not.
        assert!(
            crossed
                .bones
                .iter()
                .filter(|bone| bone.crossings > 0)
                .count()
                >= 2
        );
        assert!(
            crossed
                .bones
                .iter()
                .filter(|bone| bone.depth == 0)
                .all(|bone| bone.crossings == 0)
        );
    }

    #[test]
    fn level_branches_offer_perches_and_trunks_anchor() {
        let plant = tree(false);
        let (mechanics, motion) = found(&plant);
        let made = record(&mechanics, &motion, &plant, &[]);
        assert_eq!(made.perches.len(), PERCH_ALONG.len());
        for perch in &made.perches {
            let bone = &made.bones[perch.bone as usize];
            assert_eq!(bone.depth, 1);
            // On top of the level branch, its radius above its axis.
            assert!((perch.point[1] - (2.0 + perch.radius)).abs() < 1.0e-9);
            assert!(perch.load.unwrap() > 0.0);
        }
        // Nearer the foot, a perch takes more.
        assert!(made.perches[0].load > made.perches[2].load);
        // The branch reaches its radius off its own line.
        let branch = made.bones.iter().find(|bone| bone.depth == 1).unwrap();
        assert!((branch.reach - 0.03).abs() < 1.0e-9);
        assert!(branch.snow_area.wood > 1.5 * 0.05 && branch.snow_area.wood < 1.5 * 0.07);
        let stem: f64 = mechanics
            .bones
            .iter()
            .filter(|bone| bone.depth == 0)
            .map(|bone| bone.mass - bone.leaf_mass)
            .sum();
        assert!((made.anchorage.moment - ANCHORAGE_PER_KG * stem).abs() < 1.0e-6);
        assert!((made.anchorage.stiffness * ROOT_TURN - made.anchorage.moment).abs() < 1.0e-6);
        assert!(motion.assumed.contains(&"anchorage"));
    }

    #[test]
    fn unstated_materials_are_generic_and_named() {
        let traits: Map<String, Value> =
            serde_json::from_str(r#"{"wood_density_basic_kg_m3": 600}"#).unwrap();
        let motion = Motion::of(&traits);
        assert!(!motion.assumed.contains(&"wood_density_basic_kg_m3"));
        assert!(motion.assumed.contains(&"wood_modulus_green_mpa"));
        assert!(motion.assumed.contains(&"damping"));
        assert!((motion.material.wood_density - 600.0 * 1.8).abs() < 1.0e-9);
        assert!(motion.material.rupture.is_some());
    }
}
