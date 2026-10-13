//! What a package gives a host to sway and prune its plants by (plant
//! motion S2), built only when asked ([`crate::package::Inputs::with_motion`]).
//!
//! The contract with hosts is in jferments/after-claude
//! `project-files/plant-physics/CONTRACT.md`. Per keyframe a package gains
//! one `mechanics` object ([`record`], JSON): the material, the bones of
//! [`crate::mechanics`] with their stiffness, frequencies, strength and
//! bend limits, and the branches a host can prune, each with the stub cap
//! a prune leaves. Per level of detail it gains one `binding` object
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
use crate::math::Vec3;
use crate::mechanics::{self, BoneKind, Material, Mechanics, Settings};
use crate::mesh::PlantMesh;

/// The motion objects' revision: a change to what they hold or how they
/// are computed moves it, and with it the key of every motion package.
pub const MECHANICS_REVISION: u32 = 1;

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
}

/// Areas for drag, m²: its wood's frontal area (twice radius times
/// length) and its foliage's one-sided area, which a host turns to a
/// frontal area with its own reconfiguration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DragArea {
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

/// The `mechanics` record of `found`, made with `motion`.
#[must_use]
pub fn record(found: &Mechanics, motion: &Motion) -> Record {
    let material = &found.material;
    let bones = found
        .bones
        .iter()
        .map(|bone| BoneRecord {
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
    }
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

/// The binding of `plant`, drawn from `graph`, to the bones of `found`.
#[must_use]
pub fn bind(plant: &PlantMesh, graph: &PlantGraph, found: &Mechanics) -> Bound {
    let mut unborne = 0;
    let mut at = |segment: u32, point: Vec3| -> u32 {
        if segment == crate::mesh::GROUNDED {
            return pack(GROUND, 0.0);
        }
        let bound = (segment != crate::mesh::NO_SEGMENT)
            .then_some(segment)
            .and_then(|segment| {
                let binding = found.binding.get(segment as usize).copied().flatten()?;
                let t = share_along(&graph.segments[segment as usize], point);
                Some(pack(
                    binding.bone,
                    binding.from + (binding.along - binding.from) * t,
                ))
            });
        bound.unwrap_or_else(|| {
            unborne += 1;
            pack(GROUND, 0.0)
        })
    };
    let wood = plant
        .wood
        .positions
        .iter()
        .enumerate()
        .map(|(index, &[x, y, z])| {
            at(
                plant.wood.bearer(index),
                Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
            )
        })
        .collect();
    let cards = plant
        .cards
        .iter()
        .map(|card| {
            let [x, y, z] = card.base;
            at(
                card.segment,
                Vec3::new(f64::from(x), f64::from(y), f64::from(z)),
            )
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
