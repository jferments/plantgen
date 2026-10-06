//! `.afterplant` packages: everything needed to draw one species, built
//! once and named by its content.
//!
//! A package is a directory:
//!
//! ```text
//! <species>-<first 16 hex digits of the key>.afterplant/
//!   manifest.json      what is inside: variants, keyframes, levels of
//!                      detail, impostors, validation, the spec itself
//!   objects/<sha256>   each object named by the SHA-256 of its bytes
//! ```
//!
//! The key is a SHA-256 over everything that decides the contents: the
//! package format, [`crate::GENERATOR_REVISION`], the spec, the program
//! source, the quality profile and the organ atlas drawn from the spec's
//! looks. The same inputs
//! give byte-identical packages on every machine, so a key names a package
//! before it is built, and a build whose directory already exists can be
//! skipped. A package is written into a temporary directory and renamed
//! into place, so a reader never sees half of one.
//!
//! Objects are of these kinds:
//!
//! | kind | format |
//! |------|--------|
//! | `program` | the plant program's UTF-8 source |
//! | `graph` | one keyframe's [`PlantGraph`], `APGRAPH1` (below) |
//! | `mesh` | one level of detail, `APMESH1` (below) |
//! | `impostor-albedo` | PNG, sRGB colour with coverage in alpha |
//! | `impostor-normal-depth` | PNG, world normal · 0.5 + 0.5, depth in alpha |
//! | `organ-atlas` | PNG, one template per organ type, side by side in the program's order: brightness / 2 in red, accent weight in green, coverage in alpha |
//!
//! Binary objects are little-endian with 32-bit floats. A graph is a
//! 24-byte header (magic, age, height, segment count, organ count), then
//! 52-byte segments, then 68-byte organs, then, only for a plant with
//! bodies, the left vector of each body segment:
//!
//! ```text
//! segment: u64 id, u32 parent (0xFFFFFFFF: none), u16 order, u8 flags
//!          (bit 0: lateral), u8 body (0: wood), f32×3 start, f32×3 end,
//!          f32 radius, f32 born, f32 shed (+inf: never)
//! organ:   u64 id, u32 segment (0xFFFFFFFF: none), u16 organ type, u16 0,
//!          f32×3 position, f32×3 heading, f32×3 left, f32 size,
//!          f32 born, f32 shed (+inf: never), f32 light
//! bodies:  u32 count B, then B × (u32 segment, f32×3 left), in segment
//!          order; absent when no segment is a body, so a plant without
//!          bodies has the bytes it had before bodies existed
//! ```
//!
//! A mesh is the magic, then the wood and then the organ cards, each as one
//! array per attribute, ready to upload as separate vertex or instance
//! buffers. Every array of single bytes is followed by zero padding to a
//! multiple of 4 bytes.
//!
//! ```text
//! wood:  u32 vertex count V, u32 index count I, positions f32×3V,
//!        normals f32×3V, uvs f32×2V, colours f32×4V, births f32×V,
//!        sheds f32×V, wind levels u8×V, indices u32×I
//! cards: u32 card count C, bases f32×3C, headings f32×3C, lefts f32×3C,
//!        lengths f32×C, widths f32×C, colours f32×3C, births f32×C,
//!        sheds f32×C, templates u8×C
//! tufts: u32 tuft count T, positions f32×3T, normals f32×3T, ups f32×3T,
//!        scales f32×T, greys f32×T, seeds u32×T, births f32×T,
//!        sheds f32×T, body types u8×T
//! ```
//!
//! Tufts, the areoles of fleshy bodies whose spines a renderer expands
//! (see [`crate::spines`]), follow the cards only on a level that has any,
//! so a plant without bodies has the bytes it had before bodies existed.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::body::BodyLook;
use crate::graph::{GraphOrgan, GraphSegment, OrganType, PlantGraph};
use crate::grow::{Growth, GrowthSettings, GrowthStats, grow};
use crate::impostor;
use crate::json;
use crate::looks::Look;
use crate::lsys::program::hex;
use crate::lsys::{GrowthError, Limits, Neighbourhood, Program};
use crate::math::Vec3;
use crate::mesh::{self, Card, LodSpec, Mesh, PlantMesh};
use crate::quality::Quality;
use crate::raster;
use crate::spec::{Environment, PlantSpec, SpecError, Variant, builtin_program};
use crate::spines::Tuft;
use crate::templates::Templates;

/// Version of the package layout and object formats.
pub const PACKAGE_FORMAT: u32 = 1;
pub const EXTENSION: &str = "afterplant";
pub const MANIFEST: &str = "manifest.json";
pub const OBJECTS: &str = "objects";
pub const GRAPH_MAGIC: [u8; 8] = *b"APGRAPH1";
pub const MESH_MAGIC: [u8; 8] = *b"APMESH1\0";
/// The level of detail impostors are rendered from.
pub const IMPOSTOR_LOD: usize = 1;
/// Samples per impostor texel along each axis.
pub const IMPOSTOR_SUPERSAMPLE: usize = 2;

const GRAPH_HEADER: usize = 24;
const SEGMENT_BYTES: usize = 52;
const ORGAN_BYTES: usize = 68;
const BODY_LEFT_BYTES: usize = 16;
const NONE: u32 = u32::MAX;

/// A package that could not be built, written or read.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageError {
    Spec(SpecError),
    /// A variant failed to grow.
    Growth {
        variant: String,
        message: String,
    },
    Io {
        path: PathBuf,
        message: String,
    },
    /// Bytes that are not a valid package, manifest or object.
    Format(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(error) => write!(f, "{error}"),
            Self::Growth { variant, message } => write!(f, "{variant}: {message}"),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Format(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<SpecError> for PackageError {
    fn from(error: SpecError) -> Self {
        Self::Spec(error)
    }
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> PackageError + '_ {
    move |error| PackageError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn format_error(message: impl Into<String>) -> PackageError {
    PackageError::Format(message.into())
}

/// A quality profile as the manifest records it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityRecord {
    pub name: String,
    /// LOD0 (closest) to LOD3 (farthest mesh).
    pub lods: Vec<LodSpec>,
    pub impostor_views: usize,
    pub impostor_size: usize,
}

impl From<&Quality> for QualityRecord {
    fn from(quality: &Quality) -> Self {
        Self {
            name: quality.name.to_string(),
            lods: quality.lods.to_vec(),
            impostor_views: quality.impostor_views,
            impostor_size: quality.impostor_size,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramRecord {
    pub name: String,
    pub revision: u32,
    /// The `program` object holding the source.
    pub object: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtlasRecord {
    pub object: String,
    pub width: usize,
    pub height: usize,
    /// One square template per organ type, side by side in the program's
    /// order; a card's template is its organ type's index. Then two per
    /// body type: its spines face on (`spine-star`) and from the side
    /// (`spine-fan`).
    pub layers: Vec<AtlasLayer>,
}

/// One template of the organ atlas and the colours a renderer paints it
/// with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtlasLayer {
    pub organ: String,
    /// The look's template, for example `palmate`.
    pub shape: String,
    /// The look's colour in full light and its accent colour, linear RGB.
    /// A texel's colour is the card's colour blended toward the accent,
    /// darkened as the card's colour is darker than `colour`, by the
    /// texel's accent weight (see [`Templates::albedo`]).
    pub colour: [f32; 3],
    pub accent: [f32; 3],
}

/// Sizes of the plant at one keyframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurements {
    pub height: f64,
    /// Stem diameter at 1.3 m, metres.
    pub dbh: Option<f64>,
    pub crown_base: Option<f64>,
    pub crown_radius: f64,
    /// One-sided leaf area, m².
    pub leaf_area: f64,
    pub segments: usize,
    pub organs: usize,
}

impl Measurements {
    fn of(graph: &PlantGraph, organ_types: &[OrganType]) -> Self {
        Self {
            height: graph.height,
            dbh: graph.diameter_at_breast_height(),
            crown_base: graph.crown_base(),
            crown_radius: graph.crown_radius(),
            leaf_area: graph.leaf_area(organ_types),
            segments: graph.segments.len(),
            organs: graph.organs.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LodRecord {
    pub mesh: String,
    pub wood_vertices: usize,
    pub wood_triangles: usize,
    pub cards: usize,
    /// Areoles of fleshy bodies, on the nearest level only.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tufts: usize,
}

// Serde's `skip_serializing_if` passes a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl LodRecord {
    /// Triangles once every card is drawn as two.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.wood_triangles + self.cards * 2
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImpostorRecord {
    pub albedo: String,
    pub normal_depth: String,
    /// Views along each side of the hemi-octahedral grid.
    pub views: usize,
    /// Texels along each side of one view.
    pub view_size: usize,
    /// The bounding sphere the views frame, metres in the plant's frame.
    pub center: [f64; 3],
    pub radius: f64,
    /// The level of detail the views were rendered from.
    pub lod: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframeRecord {
    pub age: f64,
    pub size: Measurements,
    pub graph: String,
    pub lods: Vec<LodRecord>,
    pub impostor: ImpostorRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantRecord {
    pub environment: Environment,
    pub seed: u64,
    pub neighbourhood: Neighbourhood,
    pub stats: GrowthStats,
    pub keyframes: Vec<KeyframeRecord>,
}

/// A grown variant compared with one of the spec's reference sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecord {
    pub environment: Environment,
    pub seed: u64,
    pub age: f64,
    pub height_reference: f64,
    pub height_grown: f64,
    pub dbh_reference: Option<f64>,
    pub dbh_grown: Option<f64>,
    /// Largest allowed relative difference.
    pub tolerance: f64,
    pub within_tolerance: bool,
}

impl ValidationRecord {
    /// Relative height difference, grown against reference.
    #[must_use]
    pub fn height_error(&self) -> f64 {
        (self.height_grown - self.height_reference) / self.height_reference
    }

    /// Relative diameter difference, if both diameters are known.
    #[must_use]
    pub fn dbh_error(&self) -> Option<f64> {
        Some((self.dbh_grown? - self.dbh_reference?) / self.dbh_reference?)
    }

    /// Grown against reference sizes, for people.
    #[must_use]
    pub fn describe(&self) -> String {
        use std::fmt::Write;
        // Two decimals for plants lower than 10 m, so a grass's sizes
        // still differ in print.
        let places = if self.height_reference < 10.0 { 2 } else { 1 };
        let mut text = format!(
            "height {:.places$} m vs {:.places$} m ({:+.0}%)",
            self.height_grown,
            self.height_reference,
            self.height_error() * 100.0
        );
        if let Some(reference) = self.dbh_reference {
            let _ = match (self.dbh_grown, self.dbh_error()) {
                (Some(grown), Some(error)) => write!(
                    text,
                    ", dbh {:.0} cm vs {:.0} cm ({:+.0}%)",
                    grown * 100.0,
                    reference * 100.0,
                    error * 100.0
                ),
                _ => write!(
                    text,
                    ", no stem at 1.3 m vs dbh {:.0} cm",
                    reference * 100.0
                ),
            };
        }
        text.push_str(if self.within_tolerance {
            ", within tolerance"
        } else {
            ", OUTSIDE tolerance"
        });
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub kind: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always `"afterplant"`.
    pub format: String,
    pub format_version: u32,
    /// Hex SHA-256 over every input; see the module documentation.
    pub key: String,
    pub generator_revision: u32,
    pub species: String,
    pub quality: QualityRecord,
    pub program: ProgramRecord,
    pub organ_types: Vec<OrganType>,
    /// The program's fleshy body types, by name in declaration order; each
    /// has two templates after the organs' in the atlas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_types: Vec<String>,
    pub organ_atlas: AtlasRecord,
    pub variants: Vec<VariantRecord>,
    pub validation: Vec<ValidationRecord>,
    /// The spec the package was built from, sources and licences included.
    pub spec: PlantSpec,
    /// Every object, by hex SHA-256.
    pub objects: BTreeMap<String, ObjectRecord>,
}

impl Manifest {
    /// The directory name of this package.
    #[must_use]
    pub fn directory_name(&self) -> String {
        directory_name(&self.species, &self.key)
    }
}

/// `<species>-<first 16 hex digits of key>.afterplant`.
#[must_use]
pub fn directory_name(species: &str, key: &str) -> String {
    format!("{species}-{}.{EXTENSION}", &key[..key.len().min(16)])
}

/// What a package is built from, and the key those inputs give.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub spec: PlantSpec,
    pub program_source: String,
    pub quality: Quality,
    pub key: String,
    /// One look per organ type of the program.
    pub looks: Vec<Look>,
    /// One look per body type of the program.
    pub bodies: Vec<BodyLook>,
    templates: Templates,
    atlas: (usize, usize, Vec<u8>),
}

impl Inputs {
    /// Inputs for a spec grown by its built-in program.
    ///
    /// # Errors
    ///
    /// Fails if the spec is invalid or names no built-in program.
    pub fn new(spec: &PlantSpec, quality: &Quality) -> Result<Self, PackageError> {
        let source = builtin_program(&spec.generator.program)
            .ok_or_else(|| SpecError(format!("unknown program `{}`", spec.generator.program)))?;
        Self::with_program(spec, source, quality)
    }

    /// Inputs for a spec grown by `source` in place of its built-in
    /// program. The source is part of the key and is stored in the
    /// package.
    ///
    /// # Errors
    ///
    /// Fails if the spec is invalid or the program does not compile with
    /// the spec's parameters.
    pub fn with_program(
        spec: &PlantSpec,
        source: &str,
        quality: &Quality,
    ) -> Result<Self, PackageError> {
        // Build from the spec as the manifest records it, numbers rounded
        // to 15 digits (see `crate::json`), so a manifest's copy of its spec
        // rebuilds the same package.
        let spec_json = json::to_vec(spec).map_err(format_error)?;
        let spec: PlantSpec =
            serde_json::from_slice(&spec_json).map_err(|error| format_error(error.to_string()))?;
        spec.validate()?;
        let (program, _) = spec.program_from(source)?;
        let looks = spec.appearance.looks(program.organs());
        let bodies = spec.appearance.body_looks(program.bodies());
        let named: Vec<(&str, &BodyLook)> = program.bodies().zip(&bodies).collect();
        let templates = Templates::for_plant(&looks, &named);
        let (width, height, pixels) = templates.atlas_rgba();
        let atlas_png = png(width, height, &pixels)?;
        let quality_json = json::to_vec(&QualityRecord::from(quality)).map_err(format_error)?;
        let mut hasher = Sha256::new();
        let parts: [(&str, &[u8]); 6] = [
            ("format", &PACKAGE_FORMAT.to_le_bytes()),
            ("generator", &crate::GENERATOR_REVISION.to_le_bytes()),
            ("spec", &spec_json),
            ("program", source.as_bytes()),
            ("quality", &quality_json),
            ("organ-atlas", &atlas_png),
        ];
        // Each part is labelled and length-prefixed, so no two different
        // sets of inputs hash the same bytes.
        for (label, bytes) in parts {
            hasher.update(label.as_bytes());
            hasher.update([0]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        Ok(Self {
            spec,
            program_source: source.to_string(),
            quality: *quality,
            key: hex(&hasher.finalize()),
            looks,
            bodies,
            templates,
            atlas: (width, height, atlas_png),
        })
    }

    /// The directory name the package will have.
    #[must_use]
    pub fn directory_name(&self) -> String {
        directory_name(&self.spec.id, &self.key)
    }
}

/// A built package, in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub manifest: Manifest,
    /// Object bytes by hex SHA-256.
    pub objects: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// The manifest as written: pretty JSON with a final newline, numbers
    /// rounded to 15 significant digits so that every reader reads them
    /// back exactly (see [`crate::json`]).
    ///
    /// # Errors
    ///
    /// Fails if the manifest cannot be serialised or holds a number too
    /// large to write exactly.
    pub fn manifest_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let mut bytes = json::to_vec_pretty(&self.manifest).map_err(format_error)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Total size of the objects, in bytes; the manifest is not counted.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.manifest
            .objects
            .values()
            .map(|object| object.bytes)
            .sum()
    }
}

fn png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, PackageError> {
    raster::encode_png(width, height, rgba)
        .map_err(|error| format_error(format!("cannot encode PNG: {error}")))
}

/// Objects collected during a build.
#[derive(Default)]
struct Store {
    objects: BTreeMap<String, (String, Vec<u8>)>,
}

impl Store {
    fn add(&mut self, kind: &str, bytes: Vec<u8>) -> String {
        let sha = hex(&Sha256::digest(&bytes));
        self.objects
            .entry(sha.clone())
            .or_insert_with(|| (kind.to_string(), bytes));
        sha
    }

    /// Store one baked keyframe's objects and describe them.
    fn keyframe(
        &mut self,
        graph: &PlantGraph,
        organ_types: &[OrganType],
        baked: BakedKeyframe,
    ) -> KeyframeRecord {
        let lods = baked
            .lods
            .into_iter()
            .map(|(bytes, record)| LodRecord {
                mesh: self.add("mesh", bytes),
                ..record
            })
            .collect();
        let center = baked.impostor.center;
        KeyframeRecord {
            age: graph.age,
            size: Measurements::of(graph, organ_types),
            graph: self.add("graph", baked.graph),
            lods,
            impostor: ImpostorRecord {
                albedo: self.add("impostor-albedo", baked.albedo),
                normal_depth: self.add("impostor-normal-depth", baked.normal_depth),
                views: baked.impostor.views,
                view_size: baked.impostor.size,
                center: [center.x, center.y, center.z],
                radius: baked.impostor.radius,
                lod: IMPOSTOR_LOD,
            },
        }
    }

    /// The objects' bytes and their manifest records, by hash.
    fn finish(self) -> (BTreeMap<String, Vec<u8>>, BTreeMap<String, ObjectRecord>) {
        let mut objects = BTreeMap::new();
        let mut records = BTreeMap::new();
        for (sha, (kind, bytes)) in self.objects {
            records.insert(
                sha.clone(),
                ObjectRecord {
                    kind,
                    bytes: bytes.len() as u64,
                },
            );
            objects.insert(sha, bytes);
        }
        (objects, records)
    }
}

/// Run `work` for every index below `count` on up to `threads` threads and
/// return the results in index order, so the output never depends on
/// scheduling.
fn parallel<T: Send>(count: usize, threads: usize, work: &(dyn Fn(usize) -> T + Sync)) -> Vec<T> {
    let threads = threads.min(count).max(1);
    if threads == 1 {
        return (0..count).map(work).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    let result = work(index);
                    results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)[index] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_iter()
        .flatten()
        .collect()
}

/// Threads a build uses: every core the process may use.
#[must_use]
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}

fn same_age(a: f64, b: f64, dt: f64) -> bool {
    (a - b).abs() <= dt * 0.5
}

/// `(variant, keyframe)` for every keyframe the package keeps, leaving out
/// those grown only to compare with reference sizes.
fn bake_jobs(grown: &[Growth], ages: &[f64], dt: f64) -> Vec<(usize, usize)> {
    grown
        .iter()
        .enumerate()
        .flat_map(|(variant, growth)| {
            growth
                .keyframes
                .iter()
                .enumerate()
                .filter(|(_, graph)| ages.iter().any(|age| same_age(graph.age, *age, dt)))
                .map(move |(keyframe, _)| (variant, keyframe))
        })
        .collect()
}

/// Grow one variant once, keeping the package's ages and the ages the
/// spec gives reference sizes for.
fn grow_variant(
    spec: &PlantSpec,
    program: &Program,
    params: &[f64],
    variant: &Variant,
    ages: &[f64],
) -> Result<Growth, GrowthError> {
    let mut keyframes = ages.to_vec();
    keyframes.extend(
        spec.allometry
            .iter()
            .filter(|point| point.environment == variant.environment)
            .map(|point| point.age),
    );
    grow(
        program,
        params,
        &GrowthSettings {
            seed: variant.seed,
            dt: spec.growth.step,
            years: spec.growth.years,
            keyframes,
            neighbourhood: variant.neighbourhood,
            limits: Limits::default(),
        },
    )
}

/// Compare a grown variant with every reference size the spec gives for
/// its environment, at the keyframes grown for those ages. A reference age
/// without a keyframe is skipped.
#[must_use]
pub fn compare_allometry(
    spec: &PlantSpec,
    variant: &Variant,
    growth: &Growth,
) -> Vec<ValidationRecord> {
    let mut records = Vec::new();
    for point in spec
        .allometry
        .iter()
        .filter(|point| point.environment == variant.environment)
    {
        let Some(graph) = growth
            .keyframes
            .iter()
            .find(|graph| same_age(graph.age, point.age, spec.growth.step))
        else {
            continue;
        };
        let mut record = ValidationRecord {
            environment: variant.environment,
            seed: variant.seed,
            age: point.age,
            height_reference: point.height,
            height_grown: graph.height,
            dbh_reference: point.dbh,
            dbh_grown: graph.diameter_at_breast_height(),
            tolerance: point.tolerance,
            within_tolerance: false,
        };
        // A reference diameter needs a grown stem at breast height.
        let dbh_within = match (point.dbh, record.dbh_error()) {
            (None, _) => true,
            (Some(_), Some(error)) => error.abs() <= point.tolerance,
            (Some(_), None) => false,
        };
        record.within_tolerance = record.height_error().abs() <= point.tolerance && dbh_within;
        records.push(record);
    }
    records
}

/// One keyframe's derived objects, before they enter the store.
struct BakedKeyframe {
    graph: Vec<u8>,
    lods: Vec<(Vec<u8>, LodRecord)>,
    albedo: Vec<u8>,
    normal_depth: Vec<u8>,
    impostor: impostor::Impostor,
}

fn bake_keyframe(graph: &PlantGraph, inputs: &Inputs) -> Result<BakedKeyframe, PackageError> {
    let meshes: Vec<PlantMesh> = inputs
        .quality
        .lods
        .iter()
        .enumerate()
        .map(|(level, lod)| {
            mesh::build(
                graph,
                &inputs.looks,
                &inputs.bodies,
                &inputs.spec.appearance,
                &lod.for_height(graph.height),
                level,
            )
        })
        .collect();
    let impostor = impostor::bake(
        &meshes[IMPOSTOR_LOD],
        &inputs.templates,
        inputs.quality.impostor_views,
        inputs.quality.impostor_size,
        IMPOSTOR_SUPERSAMPLE,
    );
    let atlas = impostor.atlas_size();
    Ok(BakedKeyframe {
        graph: encode_graph(graph),
        lods: meshes
            .iter()
            .map(|plant| {
                (
                    encode_mesh(plant),
                    LodRecord {
                        mesh: String::new(),
                        wood_vertices: plant.wood.vertex_count(),
                        wood_triangles: plant.wood.triangle_count(),
                        cards: plant.cards.len(),
                        tufts: plant.tufts.len(),
                    },
                )
            })
            .collect(),
        albedo: png(atlas, atlas, &impostor.albedo)?,
        normal_depth: png(atlas, atlas, &impostor.normal_depth)?,
        impostor,
    })
}

/// Grow every variant, bake every keyframe and assemble the package.
/// `progress` receives a line per finished variant and keyframe. Work is
/// spread over `threads` threads; the result does not depend on how many.
///
/// # Errors
///
/// Fails if a variant fails to grow or an image cannot be encoded.
pub fn build(
    inputs: &Inputs,
    threads: usize,
    progress: &mut (dyn FnMut(&str) + Send),
) -> Result<Package, PackageError> {
    let spec = &inputs.spec;
    let (program, params) = spec.program_from(&inputs.program_source)?;
    let progress = Mutex::new(progress);
    let say = |line: &str| {
        (progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))(line);
    };

    let mut ages = spec.growth.keyframes.clone();
    ages.sort_by(f64::total_cmp);
    ages.dedup();
    let variants = spec.variant_list();
    let label = |index: usize| {
        format!(
            "{} seed {}",
            variants[index].environment.name(),
            variants[index].seed
        )
    };

    let grown = parallel(variants.len(), threads, &|index| {
        let started = Instant::now();
        let growth =
            grow_variant(spec, &program, &params, &variants[index], &ages).map_err(|error| {
                PackageError::Growth {
                    variant: format!("{} {}", spec.id, label(index)),
                    message: error.to_string(),
                }
            })?;
        say(&format!(
            "grew {}: {} years, peak {} modules, {} segments, {} organs ({:.1} s)",
            label(index),
            spec.growth.years,
            growth.stats.peak_modules,
            growth.stats.peak_segments,
            growth.stats.peak_organs,
            started.elapsed().as_secs_f64()
        ));
        Ok::<_, PackageError>(growth)
    });
    let grown = grown.into_iter().collect::<Result<Vec<_>, _>>()?;

    let validation = variants
        .iter()
        .zip(&grown)
        .flat_map(|(variant, growth)| compare_allometry(spec, variant, growth))
        .collect();

    let jobs = bake_jobs(&grown, &ages, spec.growth.step);
    let baked = parallel(jobs.len(), threads, &|job| {
        let (variant, keyframe) = jobs[job];
        let growth = &grown[variant];
        let graph = &growth.keyframes[keyframe];
        let started = Instant::now();
        let baked = bake_keyframe(graph, inputs)?;
        say(&format!(
            "baked {} at {} years: LOD0 {} triangles, LOD3 {}, impostor {}² ({:.1} s)",
            label(variant),
            graph.age,
            baked.lods[0].1.triangles(),
            baked.lods[3].1.triangles(),
            baked.impostor.atlas_size(),
            started.elapsed().as_secs_f64()
        ));
        Ok::<_, PackageError>(baked)
    });
    let baked = baked.into_iter().collect::<Result<Vec<_>, _>>()?;
    let baked: Vec<_> = jobs.iter().copied().zip(baked).collect();
    assemble(inputs, &program, &variants, &grown, baked, validation)
}

/// The package from its grown variants and baked keyframes, each keyframe
/// with its variant's and keyframe's index.
fn assemble(
    inputs: &Inputs,
    program: &Program,
    variants: &[Variant],
    grown: &[Growth],
    baked: Vec<((usize, usize), BakedKeyframe)>,
    validation: Vec<ValidationRecord>,
) -> Result<Package, PackageError> {
    let spec = &inputs.spec;
    let mut store = Store::default();
    let program_object = store.add("program", inputs.program_source.as_bytes().to_vec());
    let (atlas_width, atlas_height, atlas_png) = &inputs.atlas;
    let atlas_object = store.add("organ-atlas", atlas_png.clone());
    let mut records: Vec<VariantRecord> = variants
        .iter()
        .zip(grown)
        .map(|(variant, growth)| VariantRecord {
            environment: variant.environment,
            seed: variant.seed,
            neighbourhood: variant.neighbourhood,
            stats: growth.stats,
            keyframes: Vec::new(),
        })
        .collect();
    for ((variant, keyframe), baked) in baked {
        let growth = &grown[variant];
        let record = store.keyframe(&growth.keyframes[keyframe], &growth.organ_types, baked);
        records[variant].keyframes.push(record);
    }

    let (objects, object_records) = store.finish();
    let manifest = Manifest {
        format: EXTENSION.to_string(),
        format_version: PACKAGE_FORMAT,
        key: inputs.key.clone(),
        generator_revision: crate::GENERATOR_REVISION,
        species: spec.id.clone(),
        quality: QualityRecord::from(&inputs.quality),
        program: ProgramRecord {
            name: program.name.clone(),
            revision: program.revision,
            object: program_object,
        },
        organ_types: grown
            .first()
            .map(|growth| growth.organ_types.clone())
            .unwrap_or_default(),
        body_types: program.bodies().map(str::to_string).collect(),
        organ_atlas: AtlasRecord {
            object: atlas_object,
            width: *atlas_width,
            height: *atlas_height,
            layers: inputs
                .templates
                .templates
                .iter()
                .map(|template| AtlasLayer {
                    organ: template.organ.clone(),
                    shape: template.shape.to_string(),
                    colour: template.colour,
                    accent: template.accent_colour,
                })
                .collect(),
        },
        variants: records,
        validation,
        spec: spec.clone(),
        objects: object_records,
    };
    let mut package = Package { manifest, objects };
    // Hold the manifest as every reader will see it: its numbers rounded to
    // the digits it is written with.
    package.manifest = serde_json::from_slice(&package.manifest_bytes()?)
        .map_err(|error| format_error(error.to_string()))?;
    Ok(package)
}

/// The package for `key` in `out`, if it has been built there.
#[must_use]
pub fn existing(out: &Path, species: &str, key: &str) -> Option<PathBuf> {
    let path = out.join(directory_name(species, key));
    path.join(MANIFEST).is_file().then_some(path)
}

/// Write a package into `out` and return its directory. A package that is
/// already there is left alone: the same key means the same bytes.
///
/// # Errors
///
/// Fails if a file cannot be written or the directory cannot be renamed
/// into place.
pub fn write(package: &Package, out: &Path) -> Result<PathBuf, PackageError> {
    let manifest = &package.manifest;
    if let Some(path) = existing(out, &manifest.species, &manifest.key) {
        return Ok(path);
    }
    fs::create_dir_all(out).map_err(io_error(out))?;
    let name = manifest.directory_name();
    let target = out.join(&name);
    let temporary = out.join(format!(".{name}.partial-{}", std::process::id()));
    if temporary.exists() {
        fs::remove_dir_all(&temporary).map_err(io_error(&temporary))?;
    }
    let objects = temporary.join(OBJECTS);
    fs::create_dir_all(&objects).map_err(io_error(&objects))?;
    for (sha, bytes) in &package.objects {
        let path = objects.join(sha);
        fs::write(&path, bytes).map_err(io_error(&path))?;
    }
    let path = temporary.join(MANIFEST);
    fs::write(&path, package.manifest_bytes()?).map_err(io_error(&path))?;
    match fs::rename(&temporary, &target) {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            // Another build of the same key may have finished first.
            if target.join(MANIFEST).is_file() {
                Ok(target)
            } else {
                Err(io_error(&target)(error))
            }
        }
    }
}

/// Read a package's manifest.
///
/// # Errors
///
/// Fails if the manifest is missing, malformed, or of another format
/// version.
pub fn read_manifest(package: &Path) -> Result<Manifest, PackageError> {
    let path = package.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(io_error(&path))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format_error(format!("{}: {error}", path.display())))?;
    if value.get("format").and_then(serde_json::Value::as_str) != Some(EXTENSION) {
        return Err(format_error(format!(
            "{} is not an .{EXTENSION} manifest",
            path.display()
        )));
    }
    let version = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(PACKAGE_FORMAT)) {
        return Err(format_error(format!(
            "{} has package format {}; this build reads format {PACKAGE_FORMAT}",
            path.display(),
            version.map_or_else(|| "?".to_string(), |version| version.to_string())
        )));
    }
    serde_json::from_value(value)
        .map_err(|error| format_error(format!("{}: {error}", path.display())))
}

/// Read one object and check its hash.
///
/// # Errors
///
/// Fails if the object is missing or its bytes do not match its name.
pub fn read_object(package: &Path, sha: &str) -> Result<Vec<u8>, PackageError> {
    let valid_name = sha.len() == 64
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if !valid_name {
        return Err(format_error(format!("`{sha}` is not an object name")));
    }
    let path = package.join(OBJECTS).join(sha);
    let bytes = fs::read(&path).map_err(io_error(&path))?;
    if hex(&Sha256::digest(&bytes)) != sha {
        return Err(format_error(format!(
            "{} does not match its hash",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Read a package's manifest and check every object it lists.
///
/// # Errors
///
/// Fails on the first missing, resized or corrupted object, or a
/// directory named for another key.
pub fn verify(package: &Path) -> Result<Manifest, PackageError> {
    let manifest = read_manifest(package)?;
    if package.file_name().and_then(|name| name.to_str())
        != Some(manifest.directory_name().as_str())
    {
        return Err(format_error(format!(
            "{} should be named {} for its key",
            package.display(),
            manifest.directory_name()
        )));
    }
    for (sha, record) in &manifest.objects {
        let bytes = read_object(package, sha)?;
        if bytes.len() as u64 != record.bytes {
            return Err(format_error(format!(
                "object {sha} is {} bytes; the manifest says {}",
                bytes.len(),
                record.bytes
            )));
        }
    }
    Ok(manifest)
}

fn megabytes(bytes: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let bytes = bytes as f64;
    bytes / 1_048_576.0
}

/// A length or an area for people: two decimals below 10 and one above,
/// so a grass and a tree both print with useful precision.
#[must_use]
pub fn size_text(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.1}")
    }
}

/// A short report of a package for people.
#[must_use]
pub fn summary(manifest: &Manifest) -> String {
    use std::fmt::Write;
    let spec = &manifest.spec;
    let total: u64 = manifest.objects.values().map(|object| object.bytes).sum();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "{} ({}, {}), package format {}, generator revision {}",
        manifest.species,
        spec.taxon.common_name,
        spec.taxon.scientific_name,
        manifest.format_version,
        manifest.generator_revision
    );
    let _ = writeln!(text, "  key {}", manifest.key);
    let _ = writeln!(
        text,
        "  program `{}` revision {}, quality `{}`: {} levels of detail, impostors of {}×{} views at {} px",
        manifest.program.name,
        manifest.program.revision,
        manifest.quality.name,
        manifest.quality.lods.len(),
        manifest.quality.impostor_views,
        manifest.quality.impostor_views,
        manifest.quality.impostor_size
    );
    let _ = writeln!(
        text,
        "  {} variants, {} objects, {:.1} MiB",
        manifest.variants.len(),
        manifest.objects.len(),
        megabytes(total)
    );
    for variant in &manifest.variants {
        let _ = writeln!(
            text,
            "  {} seed {}:",
            variant.environment.name(),
            variant.seed
        );
        for keyframe in &variant.keyframes {
            let size = &keyframe.size;
            let triangles: Vec<String> = keyframe
                .lods
                .iter()
                .map(|lod| lod.triangles().to_string())
                .collect();
            let _ = writeln!(
                text,
                "    age {:>5.1}: height {:>5} m, dbh {:>4} cm, crown radius {:>5} m, leaf area {:>6} m², \
                 triangles by level {}",
                keyframe.age,
                size_text(size.height),
                size.dbh
                    .map_or_else(|| "-".to_string(), |dbh| format!("{:.0}", dbh * 100.0)),
                size_text(size.crown_radius),
                size_text(size.leaf_area),
                triangles.join(" / ")
            );
        }
    }
    if manifest.validation.is_empty() {
        let _ = writeln!(text, "  no reference sizes to check");
    } else {
        let within = manifest
            .validation
            .iter()
            .filter(|record| record.within_tolerance)
            .count();
        let _ = writeln!(
            text,
            "  reference sizes: {within} of {} within tolerance",
            manifest.validation.len()
        );
        for record in &manifest.validation {
            let _ = writeln!(
                text,
                "    {} seed {} at {} years: {}",
                record.environment.name(),
                record.seed,
                record.age,
                record.describe()
            );
        }
    }
    text
}

/// Little-endian byte writer.
struct Writer(Vec<u8>);

impl Writer {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.bytes(&value.to_le_bytes());
    }

    #[allow(clippy::cast_possible_truncation)]
    fn real(&mut self, value: f64) {
        self.f32(value as f32);
    }

    fn vec3(&mut self, value: Vec3) {
        for component in value.to_f32() {
            self.f32(component);
        }
    }

    fn count(&mut self, count: usize) {
        self.u32(u32::try_from(count).unwrap_or(u32::MAX));
    }

    fn shed(&mut self, shed: Option<f64>) {
        self.real(shed.unwrap_or(f64::INFINITY));
    }
}

/// Bounds-checked little-endian byte reader.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], PackageError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len());
        let Some(end) = end else {
            return Err(format_error(format!("{} ends early", self.what)));
        };
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PackageError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn u8(&mut self) -> Result<u8, PackageError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, PackageError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, PackageError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, PackageError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, PackageError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// A finite number.
    fn real(&mut self) -> Result<f64, PackageError> {
        let value = self.f32()?;
        if value.is_finite() {
            Ok(f64::from(value))
        } else {
            Err(format_error(format!(
                "{} holds a number that is not finite",
                self.what
            )))
        }
    }

    fn vec3(&mut self) -> Result<Vec3, PackageError> {
        Ok(Vec3::new(self.real()?, self.real()?, self.real()?))
    }

    /// A shed age: a finite age or +infinity for never.
    fn shed(&mut self) -> Result<Option<f64>, PackageError> {
        let value = self.f32()?;
        if value == f32::INFINITY {
            Ok(None)
        } else if value.is_finite() {
            Ok(Some(f64::from(value)))
        } else {
            Err(format_error(format!(
                "{} holds an invalid shed age",
                self.what
            )))
        }
    }

    fn index(&mut self) -> Result<Option<u32>, PackageError> {
        let value = self.u32()?;
        Ok((value != NONE).then_some(value))
    }

    fn f32s(&mut self, count: usize) -> Result<Vec<f32>, PackageError> {
        let bytes = self.take(
            count
                .checked_mul(4)
                .ok_or_else(|| format_error("count overflows"))?,
        )?;
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect())
    }

    fn finish(&self) -> Result<(), PackageError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(format_error(format!(
                "{} has {} bytes after its end",
                self.what,
                self.bytes.len() - self.at
            )))
        }
    }
}

/// Encode a keyframe graph as `APGRAPH1`.
#[must_use]
pub fn encode_graph(graph: &PlantGraph) -> Vec<u8> {
    let mut out = Writer(Vec::with_capacity(
        GRAPH_HEADER + graph.segments.len() * SEGMENT_BYTES + graph.organs.len() * ORGAN_BYTES,
    ));
    out.bytes(&GRAPH_MAGIC);
    out.real(graph.age);
    out.real(graph.height);
    out.count(graph.segments.len());
    out.count(graph.organs.len());
    for segment in &graph.segments {
        out.u64(segment.id);
        out.u32(segment.parent.unwrap_or(NONE));
        out.u16(segment.order);
        out.u8(u8::from(segment.lateral));
        out.u8(segment.body);
        out.vec3(segment.start);
        out.vec3(segment.end);
        out.real(segment.radius);
        out.real(segment.born);
        out.shed(segment.shed);
    }
    for organ in &graph.organs {
        out.u64(organ.id);
        out.u32(organ.segment.unwrap_or(NONE));
        out.u16(organ.organ);
        out.u16(0);
        out.vec3(organ.position);
        out.vec3(organ.heading);
        out.vec3(organ.left);
        out.real(organ.size);
        out.real(organ.born);
        out.shed(organ.shed);
        out.real(organ.light);
    }
    let bodies: Vec<(usize, &GraphSegment)> = graph
        .segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.body != 0)
        .collect();
    if !bodies.is_empty() {
        out.count(bodies.len());
        for (index, segment) in bodies {
            out.count(index);
            out.vec3(segment.left);
        }
    }
    out.0
}

/// Decode an `APGRAPH1` graph. Values come back at 32-bit precision.
///
/// # Errors
///
/// Fails on a wrong magic, a wrong length, a parent that does not come
/// before its child, a segment index out of range, or a body section that
/// does not list exactly the body segments in order.
#[allow(clippy::too_many_lines)]
pub fn decode_graph(bytes: &[u8]) -> Result<PlantGraph, PackageError> {
    let mut input = Reader {
        bytes,
        at: 0,
        what: "graph",
    };
    if input.array::<8>()? != GRAPH_MAGIC {
        return Err(format_error("not an APGRAPH1 graph"));
    }
    let age = input.real()?;
    let height = input.real()?;
    let segments = input.u32()? as usize;
    let organs = input.u32()? as usize;
    let expected = segments
        .checked_mul(SEGMENT_BYTES)
        .zip(organs.checked_mul(ORGAN_BYTES))
        .and_then(|(a, b)| a.checked_add(b))
        .and_then(|body| body.checked_add(GRAPH_HEADER));
    // A plant with bodies adds a count and a left vector per body segment;
    // the count itself is checked when the section is read.
    let fits = expected.is_some_and(|base| {
        bytes.len() == base
            || (bytes.len() >= base + 4 && (bytes.len() - base - 4).is_multiple_of(BODY_LEFT_BYTES))
    });
    if !fits {
        return Err(format_error(format!(
            "a graph of {segments} segments and {organs} organs cannot be {} bytes",
            bytes.len()
        )));
    }
    let mut graph = PlantGraph {
        age,
        height,
        segments: Vec::with_capacity(segments),
        organs: Vec::with_capacity(organs),
    };
    for index in 0..segments {
        let id = input.u64()?;
        let parent = input.index()?;
        if parent.is_some_and(|parent| parent as usize >= index) {
            return Err(format_error(format!(
                "segment {index} has a parent that does not come before it"
            )));
        }
        let order = input.u16()?;
        let flags = input.u8()?;
        let body = input.u8()?;
        graph.segments.push(GraphSegment {
            id,
            parent,
            lateral: flags & 1 != 0,
            order,
            start: input.vec3()?,
            end: input.vec3()?,
            radius: input.real()?,
            born: input.real()?,
            shed: input.shed()?,
            body,
            left: Vec3::ZERO,
        });
    }
    for index in 0..organs {
        let id = input.u64()?;
        let segment = input.index()?;
        if segment.is_some_and(|segment| segment as usize >= segments) {
            return Err(format_error(format!(
                "organ {index} is on a segment that does not exist"
            )));
        }
        let organ = input.u16()?;
        input.u16()?;
        graph.organs.push(GraphOrgan {
            id,
            organ,
            segment,
            position: input.vec3()?,
            heading: input.vec3()?,
            left: input.vec3()?,
            size: input.real()?,
            born: input.real()?,
            shed: input.shed()?,
            light: input.real()?,
        });
    }
    let body_segments: Vec<usize> = graph
        .segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.body != 0)
        .map(|(index, _)| index)
        .collect();
    if input.at < bytes.len() || !body_segments.is_empty() {
        let count = input.u32()? as usize;
        if count != body_segments.len() {
            return Err(format_error(format!(
                "a graph with {} body segments lists {count} body left vectors",
                body_segments.len()
            )));
        }
        for expected in body_segments {
            let index = input.u32()? as usize;
            if index != expected {
                return Err(format_error(format!(
                    "the body left vectors list segment {index} where segment {expected} was due"
                )));
            }
            graph.segments[index].left = input.vec3()?;
        }
    }
    input.finish()?;
    Ok(graph)
}

impl Writer {
    fn f32s<'a>(&mut self, values: impl IntoIterator<Item = &'a f32>) {
        for value in values {
            self.f32(*value);
        }
    }

    fn pad(&mut self) {
        while !self.0.len().is_multiple_of(4) {
            self.u8(0);
        }
    }
}

/// Encode one level of detail as `APMESH1`.
#[must_use]
pub fn encode_mesh(plant: &PlantMesh) -> Vec<u8> {
    let (wood, cards) = (&plant.wood, &plant.cards);
    let mut out = Writer(Vec::with_capacity(
        16 + wood.vertex_count() * WOOD_VERTEX_BYTES
            + wood.indices.len() * 4
            + cards.len() * CARD_BYTES
            + 8,
    ));
    out.bytes(&MESH_MAGIC);
    out.count(wood.vertex_count());
    out.count(wood.indices.len());
    out.f32s(wood.positions.as_flattened());
    out.f32s(wood.normals.as_flattened());
    out.f32s(wood.uvs.as_flattened());
    out.f32s(wood.colors.as_flattened());
    out.f32s(&wood.births);
    out.f32s(&wood.sheds);
    out.bytes(&wood.levels);
    out.pad();
    for index in &wood.indices {
        out.u32(*index);
    }
    out.count(cards.len());
    out.f32s(cards.iter().flat_map(|card| &card.base));
    out.f32s(cards.iter().flat_map(|card| &card.heading));
    out.f32s(cards.iter().flat_map(|card| &card.left));
    out.f32s(cards.iter().map(|card| &card.length));
    out.f32s(cards.iter().map(|card| &card.width));
    out.f32s(cards.iter().flat_map(|card| &card.color));
    out.f32s(cards.iter().map(|card| &card.born));
    out.f32s(cards.iter().map(|card| &card.shed));
    for card in cards {
        out.u8(card.template);
    }
    out.pad();
    let tufts = &plant.tufts;
    if !tufts.is_empty() {
        out.count(tufts.len());
        out.f32s(tufts.iter().flat_map(|tuft| &tuft.position));
        out.f32s(tufts.iter().flat_map(|tuft| &tuft.normal));
        out.f32s(tufts.iter().flat_map(|tuft| &tuft.up));
        out.f32s(tufts.iter().map(|tuft| &tuft.scale));
        out.f32s(tufts.iter().map(|tuft| &tuft.grey));
        for tuft in tufts {
            out.u32(tuft.seed);
        }
        out.f32s(tufts.iter().map(|tuft| &tuft.born));
        out.f32s(tufts.iter().map(|tuft| &tuft.shed));
        for tuft in tufts {
            out.u8(tuft.body);
        }
        out.pad();
    }
    out.0
}

/// Bytes per wood vertex, per card and per tuft, without padding.
const WOOD_VERTEX_BYTES: usize = 57;
const CARD_BYTES: usize = 65;
const TUFT_BYTES: usize = 57;

fn triples(values: &[f32]) -> Vec<[f32; 3]> {
    values.as_chunks::<3>().0.to_vec()
}

impl Reader<'_> {
    /// Refuse a count the remaining bytes cannot hold before allocating
    /// for it.
    fn check_room(&self, bytes: usize) -> Result<(), PackageError> {
        if bytes > self.bytes.len() - self.at {
            Err(format_error(format!("{} ends early", self.what)))
        } else {
            Ok(())
        }
    }

    fn skip_padding(&mut self) -> Result<(), PackageError> {
        while !self.at.is_multiple_of(4) {
            if self.u8()? != 0 {
                return Err(format_error(format!("{} has non-zero padding", self.what)));
            }
        }
        Ok(())
    }
}

fn decode_wood(input: &mut Reader<'_>) -> Result<Mesh, PackageError> {
    let vertices = input.u32()? as usize;
    let indices = input.u32()? as usize;
    if !indices.is_multiple_of(3) {
        return Err(format_error("a mesh's index count is not a multiple of 3"));
    }
    input.check_room(
        vertices
            .saturating_mul(WOOD_VERTEX_BYTES)
            .saturating_add(indices.saturating_mul(4)),
    )?;
    let positions = triples(&input.f32s(vertices * 3)?);
    let normals = triples(&input.f32s(vertices * 3)?);
    let uvs = input.f32s(vertices * 2)?.as_chunks::<2>().0.to_vec();
    let colors = input.f32s(vertices * 4)?.as_chunks::<4>().0.to_vec();
    let births = input.f32s(vertices)?;
    let sheds = input.f32s(vertices)?;
    let levels = input.take(vertices)?.to_vec();
    input.skip_padding()?;
    let mut mesh = Mesh {
        positions,
        normals,
        uvs,
        colors,
        births,
        sheds,
        levels,
        indices: Vec::with_capacity(indices),
    };
    for _ in 0..indices {
        let index = input.u32()?;
        if index as usize >= vertices {
            return Err(format_error("a mesh index points past the last vertex"));
        }
        mesh.indices.push(index);
    }
    Ok(mesh)
}

fn decode_cards(input: &mut Reader<'_>) -> Result<Vec<Card>, PackageError> {
    let count = input.u32()? as usize;
    input.check_room(count.saturating_mul(CARD_BYTES))?;
    let bases = triples(&input.f32s(count * 3)?);
    let headings = triples(&input.f32s(count * 3)?);
    let lefts = triples(&input.f32s(count * 3)?);
    let lengths = input.f32s(count)?;
    let widths = input.f32s(count)?;
    let colors = triples(&input.f32s(count * 3)?);
    let births = input.f32s(count)?;
    let sheds = input.f32s(count)?;
    let templates = input.take(count)?;
    let cards = (0..count)
        .map(|index| Card {
            base: bases[index],
            heading: headings[index],
            left: lefts[index],
            length: lengths[index],
            width: widths[index],
            color: colors[index],
            template: templates[index],
            born: births[index],
            shed: sheds[index],
        })
        .collect();
    input.skip_padding()?;
    Ok(cards)
}

fn decode_tufts(input: &mut Reader<'_>) -> Result<Vec<Tuft>, PackageError> {
    let count = input.u32()? as usize;
    if count == 0 {
        return Err(format_error("a mesh's tuft section is empty"));
    }
    input.check_room(count.saturating_mul(TUFT_BYTES))?;
    let positions = triples(&input.f32s(count * 3)?);
    let normals = triples(&input.f32s(count * 3)?);
    let ups = triples(&input.f32s(count * 3)?);
    let scales = input.f32s(count)?;
    let greys = input.f32s(count)?;
    let mut seeds = Vec::with_capacity(count);
    for _ in 0..count {
        seeds.push(input.u32()?);
    }
    let births = input.f32s(count)?;
    let shed_ages = input.f32s(count)?;
    let bodies = input.take(count)?;
    let tufts = (0..count)
        .map(|index| Tuft {
            position: positions[index],
            normal: normals[index],
            up: ups[index],
            scale: scales[index],
            grey: greys[index],
            seed: seeds[index],
            body: bodies[index],
            born: births[index],
            shed: shed_ages[index],
        })
        .collect();
    input.skip_padding()?;
    Ok(tufts)
}

/// Decode an `APMESH1` level of detail.
///
/// # Errors
///
/// Fails on a wrong magic, a wrong length or an index out of range.
pub fn decode_mesh(bytes: &[u8]) -> Result<PlantMesh, PackageError> {
    let mut input = Reader {
        bytes,
        at: 0,
        what: "mesh",
    };
    if input.array::<8>()? != MESH_MAGIC {
        return Err(format_error("not an APMESH1 mesh"));
    }
    let wood = decode_wood(&mut input)?;
    let cards = decode_cards(&mut input)?;
    let tufts = if input.at < bytes.len() {
        decode_tufts(&mut input)?
    } else {
        Vec::new()
    };
    input.finish()?;
    Ok(PlantMesh { wood, cards, tufts })
}
