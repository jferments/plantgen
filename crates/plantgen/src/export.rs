//! Plant models for other engines (plant export v1, `plantc export`): one
//! variant of a species at one age as glTF 2.0 binary files, one per level
//! of detail, and an index describing them.
//!
//! The export is the plant a package holds: the same inputs, whose key it
//! records, grown the same way over all its years ([`grow`]) and drawn at
//! each level as the package draws it ([`crate::package`]). Given the
//! package, [`export`] checks its drawing against the package's meshes,
//! bit for bit. It grows the plant again rather than reading the package's
//! meshes because packages hold bark already darkened by its shade (below);
//! once they keep the shade apart, an export can read them alone. What it
//! writes needs no `PlantGen` code to draw:
//!
//! - **Wood**: stems, branches, roots, fleshy bodies and the organs a level
//!   draws solid, as one mesh in vertex colours (`COLOR_0`). Bark keeps its
//!   own colour: the darkening `PlantGen` draws for the sky a piece of wood
//!   sees through the plant's own leaves (`crate::occlusion`) is an
//!   attribute of its own, `_PLANTGEN_SHADE`, unless [`Shade::Baked`] puts
//!   it in the colour as packages hold it.
//! - **Cards**: each organ card as a quad, a bent card as its grid (its
//!   shared corners stored once), with one primitive and material per
//!   organ type. Its texture is the type's
//!   template in the look's colours (sRGB) with coverage in alpha, cut out
//!   at one half and drawn from both sides; texels the template leaves
//!   uncovered take the colour of the nearest covered one, so a renderer's
//!   mipmaps keep the edges' colour. A card's own colour (sun or shade, and
//!   its small variation) is `COLOR_0`, relative to the look's colour, so
//!   the texture times it is the colour `PlantGen`'s renderers draw: exactly,
//!   except on accent texels of a card whose hue differs from its look's.
//! - **Spines**: on the nearest level, the areoles of fleshy bodies and
//!   their spines as solids (`crate::spines`), as previews draw them.
//! - **Parts**: on the nearest level, flowers, fruit, cones and needle
//!   shoots as their type's part meshes (`crate::parts`), each variant
//!   stored once and placed by one node per organ. An organ's shade is its
//!   material's colour factor, in sixteenths.
//!
//! [`Compression::Quantize`] stores the wood's, cards' and spines'
//! positions as unsigned shorts on a grid their node's translation and
//! uniform scale give back (within half a step, which the files record),
//! normals as bytes and texture coordinates and colours as shorts
//! (`KHR_mesh_quantization`), and draws each part mesh's organs as
//! instances of one node (`EXT_mesh_gpu_instancing`). Files then require
//! both extensions, which three.js, Babylon.js, Unity's glTFast and
//! Blender's current importer read and Godot's does not; the default
//! files need none. [`Compression::Meshopt`] also compresses every
//! vertex, index and instance array (`EXT_meshopt_compression`,
//! [`crate::meshopt`]), which three.js (with its decoder), Unity's glTFast
//! and Blender's current importer read. [`Attributes`] leaves out the
//! growth or wind attributes below for a model that never grows or sways.
//!
//! Positions are metres from the plant's foot with +Y up, as glTF's are.
//! Besides glTF's own attributes, solid meshes carry `PlantGen`'s, which
//! engines that don't know them ignore:
//!
//! | attribute | meaning |
//! |-----------|---------|
//! | `_PLANTGEN_BIRTH` | plant age at which the part appears, years |
//! | `_PLANTGEN_SHED` | plant age at which it is shed, years; the largest single-precision number when never |
//! | `_PLANTGEN_WIND` | wind level: 0 stem, 1 branch, 2 twig, 3 organ |
//! | `_PLANTGEN_RADIUS` | wood: radius of the stem a bark vertex lies on, metres; 0 off bark |
//! | `_PLANTGEN_SHADE` | wood: share of its colour the sky leaves it, [`crate::mesh::WOOD_SHADE`] to 1 |
//!
//! Every file and the index carry the package key, the generator revision,
//! the variant, the age and the species' provenance in `extras`. The same
//! inputs give the same bytes on every machine.
//!
//! A [`Budget`] draws the levels to at most a number of triangles each,
//! or bytes for all the files together, instead of as the package draws
//! them: each level draws the most detail along a dial through the
//! quality's levels and past the farthest ([`crate::budget`]) that fits,
//! and never more than the level before it. The files and the index
//! record where on the dial each level is drawn, the thinnest wood it
//! keeps and the size of its leaf clusters, which an engine can set its
//! switch distances by.
//!
//! Not yet: the bark's
//! pattern (a tile per species or a bake per tree, waiting on the owner's
//! call), normal and roughness maps, tangents, impostors,
//! coverage-preserving mipmaps (PNG holds no mipmaps), wind and growth
//! animation, and other formats.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::budget::Detail;
use crate::gltf::{self, Document, Index, number, numbers};
use crate::graph::PlantGraph;
use crate::library::Library;
use crate::math::{self, Vec3};
use crate::mesh::{Card, Mesh, PlantMesh};
use crate::package::{self, Inputs, Manifest, Measurements, PackageError};
use crate::parts::{PartMesh, Placement};
use crate::quality::{self, Quality};
use crate::spec::{Environment, PlantSpec};
use crate::spines::Tuft;
use crate::templates::Template;

/// Version of the export's layout: its files, attributes and index.
pub const EXPORT_FORMAT: u32 = 1;

/// What a part's shade is rounded to: its material's factor in steps of
/// one over this.
const SHADE_STEPS: f32 = 16.0;

/// Below this, a look's colour channel is too dark to divide a card's
/// colour by; the card's luminance over the look's stands in for it.
const DARKEST_CHANNEL: f64 = 0.01;

/// "Never shed" in `_PLANTGEN_SHED`: glTF accessors hold finite numbers.
pub const NEVER: f32 = f32::MAX;

/// How the bark's shade is written (see the module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    /// As its own attribute, `_PLANTGEN_SHADE`, the colour plain.
    Attribute,
    /// Multiplied into the colour, as packages hold it.
    Baked,
}

impl Shade {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Attribute => "attribute",
            Self::Baked => "baked",
        }
    }
}

/// How the files are encoded (see the module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    /// Single-precision floats everywhere: every glTF viewer reads them.
    None,
    /// Positions, normals, texture coordinates and colours as integers
    /// (`KHR_mesh_quantization`) and parts as instances
    /// (`EXT_mesh_gpu_instancing`), both required by the files.
    Quantize,
    /// As [`Self::Quantize`], with every array compressed
    /// (`EXT_meshopt_compression`), required too.
    Meshopt,
}

impl Compression {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Quantize => "quantize",
            Self::Meshopt => "meshopt",
        }
    }
}

/// Which of `PlantGen`'s own attributes the files carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attributes {
    /// Birth, shed, wind and radius.
    All,
    /// Wind and radius, for a model that sways but never grows.
    Wind,
    /// None of them.
    None,
}

impl Attributes {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Wind => "wind",
            Self::None => "none",
        }
    }
}

/// The most an export's levels may hold; with neither, each level is
/// drawn as the package draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Budget {
    /// Most triangles each level draws, nearest first, counted as a
    /// renderer draws them (each instance of a part mesh too). Levels
    /// may then draw more detail than the quality's level of the same
    /// number, as far as their triangles allow.
    pub triangles: Option<[usize; LEVELS]>,
    /// Most bytes the levels' files hold together. Each level may take
    /// what the coarsest drawing takes (its textures and stems) and a
    /// part of the rest: the nearest level [`NEAREST_SHARE`] of it, each
    /// farther level a quarter of the one before; then the nearest takes
    /// whatever the farther ones leave.
    pub bytes: Option<usize>,
}

impl Budget {
    /// Whether it limits anything.
    #[must_use]
    pub fn is_some(&self) -> bool {
        self.triangles.is_some() || self.bytes.is_some()
    }

    /// `nearest` triangles for the nearest level and a quarter of the
    /// level before for each farther one.
    #[must_use]
    pub fn quartering(nearest: usize) -> [usize; LEVELS] {
        std::array::from_fn(|level| nearest >> (2 * level))
    }
}

/// Levels of an export, as of every quality profile.
pub const LEVELS: usize = 4;

/// Share of a budget's bytes past the coarsest drawings' the nearest
/// level may take at first: 1 over 1 + 1/4 + 1/16 + 1/64.
pub const NEAREST_SHARE: f64 = 64.0 / 85.0;

/// How an export is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub shade: Shade,
    pub compression: Compression,
    pub attributes: Attributes,
    pub budget: Budget,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shade: Shade::Attribute,
            compression: Compression::None,
            attributes: Attributes::All,
            budget: Budget::default(),
        }
    }
}

/// One variant of a spec grown to one age, as its package grows it.
#[derive(Debug, Clone)]
pub struct Plant {
    /// The variant, as an index into the spec's variants
    /// ([`PlantSpec::variant_list`]), which is a package's order.
    pub variant: usize,
    pub environment: Environment,
    pub seed: u64,
    /// A variant grown in a conditions document: its condition class.
    pub class: Option<String>,
    /// The plant at the age, as its keyframe in a package.
    pub graph: PlantGraph,
    pub size: Measurements,
}

/// One level of detail as a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub file: String,
    pub glb: Vec<u8>,
    pub summary: gltf::Summary,
    /// Quantized files: the largest step between stored positions,
    /// metres; a position is within half of it of the plant's.
    pub position_step: Option<f32>,
    /// How it is drawn: one of the quality's levels, or a point of the
    /// dial between and past them ([`crate::budget`]).
    pub detail: Detail,
    /// Drawn to a budget: its share of it.
    pub budget: Option<LevelBudget>,
}

/// A level's share of a [`Budget`]: its triangles, and the bytes it was
/// drawn to (the nearest level's are what the others leave).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelBudget {
    pub triangles: Option<usize>,
    pub bytes: Option<usize>,
    /// Whether the level fits them; no drawing fits fewer than the
    /// coarsest ([`crate::budget::DIAL_MAX`]) takes, so a budget below
    /// that leaves the coarsest drawing over it.
    pub fits: bool,
}

/// An exported plant: its files' common name, a GLB file per level and the
/// index (JSON) naming them.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub stem: String,
    pub levels: Vec<Level>,
    pub index: Vec<u8>,
}

/// A built package to check an export against: its manifest, and its
/// objects by hex SHA-256.
pub struct Built<'a> {
    pub manifest: &'a Manifest,
    pub object: &'a dyn Fn(&str) -> Result<Vec<u8>, PackageError>,
}

fn format_error(message: impl Into<String>) -> PackageError {
    PackageError::Format(message.into())
}

/// The quality profile a manifest records, if this generator has it.
///
/// # Errors
///
/// When it is not one of [`quality::PROFILES`], or no longer the same.
pub fn quality_of(manifest: &Manifest) -> Result<Quality, PackageError> {
    quality::profile(&manifest.quality.name)
        .filter(|quality| package::QualityRecord::from(quality) == manifest.quality)
        .ok_or_else(|| {
            format_error(format!(
                "the package was built with quality `{}`, which this generator does not have",
                manifest.quality.name
            ))
        })
}

/// The inputs a package was built from, rebuilt from its manifest and its
/// `program` object, with a guest's host from `library`.
///
/// # Errors
///
/// When the program is not text, the spec does not load, or the inputs do
/// not give the package's key: it was built by another generator revision
/// or from another host, and must be built again.
pub fn inputs_of(
    manifest: &Manifest,
    program: &[u8],
    quality: &Quality,
    library: &Library,
) -> Result<Inputs, PackageError> {
    let source = std::str::from_utf8(program)
        .map_err(|_| format_error("the package's program is not UTF-8"))?;
    let day = manifest.day.unwrap_or(package::DEFAULT_DAY);
    let inputs = Inputs::with_program_in(&manifest.spec, library, source, quality, day)?;
    if inputs.key != manifest.key {
        return Err(format_error(format!(
            "{} was built by generator revision {} or from other inputs; this is revision {}, \
             whose package of the same spec is {}: build it again",
            manifest.directory_name(),
            manifest.generator_revision,
            crate::GENERATOR_REVISION,
            inputs.directory_name()
        )));
    }
    Ok(inputs)
}

/// The variant a renderer shows by default, as an index into the spec's
/// variants: its typical site and first seed (`drawing::Request::typical`).
#[must_use]
pub fn typical_variant(spec: &PlantSpec, library: &Library) -> usize {
    let environment = crate::drawing::typical_environment(spec, library);
    let seed = spec.variants.seeds.first().copied();
    spec.variant_list()
        .iter()
        .position(|variant| variant.environment == environment && Some(variant.seed) == seed)
        .unwrap_or(0)
}

/// The spec's oldest keyframe age, which a renderer shows by default.
#[must_use]
pub fn oldest_age(spec: &PlantSpec) -> f64 {
    spec.growth.keyframes.iter().copied().fold(0.0, f64::max)
}

/// Grow variant `variant` of the inputs' spec (an index into its variants)
/// over all its years, as its package grows it, keeping it at `age`.
///
/// # Errors
///
/// When the variant does not exist, the age is not within the spec's
/// years, or the plant does not grow.
pub fn grow(inputs: &Inputs, variant: usize, age: f64) -> Result<Plant, PackageError> {
    let variants = inputs.spec.variant_list();
    let chosen = variants.get(variant).ok_or_else(|| {
        format_error(format!(
            "{} has {} variants; there is no variant {variant}",
            inputs.spec.id,
            variants.len()
        ))
    })?;
    if !(age > 0.0 && age <= inputs.spec.growth.years) {
        return Err(format_error(format!(
            "{} grows for {} years; an age must be above 0 and at most that, found {age}",
            inputs.spec.id, inputs.spec.growth.years
        )));
    }
    let growth = package::grow_one(inputs, chosen, &[age])?;
    let graph = growth
        .keyframes
        .into_iter()
        .next()
        .ok_or_else(|| format_error("the growth kept no keyframe"))?;
    let size = Measurements::of(&graph, &growth.organ_types);
    Ok(Plant {
        variant,
        environment: chosen.environment,
        seed: chosen.seed,
        class: chosen.class.map(|class| class.to_string()),
        graph,
        size,
    })
}

/// Export `plant` of `inputs` as `settings` say; with `built`, the package
/// of the same inputs, check that every level is drawn as the package
/// holds it, unless the settings set a budget.
///
/// # Errors
///
/// When `built` is another package, does not keep the plant's variant and
/// age, or holds levels drawn otherwise; or when an object does not decode.
pub fn export(
    inputs: &Inputs,
    plant: &Plant,
    settings: &Settings,
    built: Option<&Built<'_>>,
) -> Result<Export, PackageError> {
    let stem = stem(inputs, plant);
    let file = |level: usize| format!("{stem}-lod{level}.glb");
    let files = if settings.budget.is_some() {
        budgeted(inputs, plant, *settings, &file)?
    } else {
        let levels = package::level_meshes(&plant.graph, inputs, true);
        if let Some(built) = built {
            check_built(inputs, plant, &levels, built)?;
        }
        let mut files = Vec::with_capacity(levels.len());
        for (level, (mesh, shades)) in levels.into_iter().enumerate() {
            let drawn = Drawn {
                mesh,
                shades,
                detail: Detail::level(&inputs.quality, plant.graph.height, level),
                parts: Cow::Borrowed(&inputs.parts),
            };
            files.push(drawn.file(inputs, plant, *settings, level, file(level))?);
        }
        files
    };
    let index = index(inputs, plant, &files, *settings)?;
    Ok(Export {
        stem,
        levels: files,
        index,
    })
}

/// One level drawn: its mesh with its bark's shades apart, how it was
/// drawn and the part meshes it places.
struct Drawn<'a> {
    mesh: PlantMesh,
    shades: Vec<f32>,
    detail: Detail,
    parts: Cow<'a, [PartMesh]>,
}

impl Drawn<'_> {
    /// Level `detail` of `drawn`, a keyframe as drawn on the inputs' day,
    /// never with more wood than `finer` triangles where given (see
    /// [`package::level_mesh`]).
    fn at<'a>(
        drawn: &PlantGraph,
        inputs: &'a Inputs,
        detail: Detail,
        finer: Option<usize>,
    ) -> Drawn<'a> {
        let (mut mesh, shades) = package::level_mesh(
            drawn,
            inputs,
            &detail.lod,
            (detail.level, detail.along),
            finer,
            true,
        );
        if detail.flat > 0.0 && detail.lod.cluster <= 0.0 {
            flatten(&mut mesh.cards, detail.flat);
        }
        #[allow(clippy::float_cmp)]
        let parts = if detail.solid_detail == 1.0 {
            Cow::Borrowed(inputs.parts.as_slice())
        } else if detail.solid_detail > 0.0 {
            Cow::Owned(crate::parts::part_meshes_within(
                &inputs.looks,
                detail.solid_detail,
            ))
        } else {
            Cow::Owned(Vec::new())
        };
        Drawn {
            mesh,
            shades,
            detail,
            parts,
        }
    }

    /// Triangles its file draws, as [`gltf::Summary::triangles`] counts
    /// them.
    fn triangles(&self, inputs: &Inputs) -> usize {
        Pieces::count(&self.mesh, inputs, &self.parts, &self.detail)
    }

    /// Its file as level `level`, named `file`.
    fn file(
        &self,
        inputs: &Inputs,
        plant: &Plant,
        settings: Settings,
        level: usize,
        file: String,
    ) -> Result<Level, PackageError> {
        let pieces = Pieces::of(&self.mesh, inputs, &self.parts, &self.detail);
        let (glb, position_step) = level_glb(
            &self.mesh,
            &self.shades,
            &pieces,
            &self.parts,
            inputs,
            settings,
            &|step| metadata(inputs, plant, Some((level, &self.detail, step)), settings),
        );
        let summary =
            gltf::check(&glb).map_err(|error| format_error(format!("{file}: {error}")))?;
        Ok(Level {
            file,
            glb,
            summary,
            position_step,
            detail: self.detail,
            budget: None,
        })
    }
}

/// Draw `share` of `cards`' bent cards flat, the smallest first.
fn flatten(cards: &mut [Card], share: f64) {
    let mut bent: Vec<usize> = (0..cards.len())
        .filter(|&card| !cards[card].bend.is_flat())
        .collect();
    let area = |card: usize| f64::from(cards[card].length) * f64::from(cards[card].width);
    bent.sort_by(|&a, &b| area(a).total_cmp(&area(b)).then(a.cmp(&b)));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let flat = ((bent.len() as f64 * share.clamp(0.0, 1.0)).round() as usize).min(bent.len());
    for &card in &bent[..flat] {
        cards[card].bend = crate::bend::Bend::FLAT;
    }
}

/// Drawings of a level tried along the dial, uncapped (see [`Drawn::at`]):
/// the same for every level but for the cap on their sticks, which only
/// takes wood away.
struct Trials<'a> {
    inputs: &'a Inputs,
    plant: &'a Plant,
    settings: Settings,
    /// The keyframe as drawn on the inputs' day.
    drawn: &'a PlantGraph,
    /// By the dial's bits: its triangles, and its file's bytes once
    /// written.
    tried: HashMap<u64, (usize, Option<usize>)>,
}

impl<'a> Trials<'a> {
    fn detail(&self, dial: f64) -> Detail {
        Detail::at(&self.inputs.quality, self.plant.graph.height, dial)
    }

    fn draw(&self, dial: f64) -> Drawn<'a> {
        Drawn::at(self.drawn, self.inputs, self.detail(dial), None)
    }

    /// The drawing at `dial`'s triangles, and the drawing if it was drawn
    /// for them.
    fn triangles(&mut self, dial: f64) -> (usize, Option<Drawn<'a>>) {
        if let Some(&(triangles, _)) = self.tried.get(&dial.to_bits()) {
            return (triangles, None);
        }
        let drawing = self.draw(dial);
        let triangles = drawing.triangles(self.inputs);
        self.tried.insert(dial.to_bits(), (triangles, None));
        (triangles, Some(drawing))
    }

    /// The bytes of the drawing at `dial`'s file, given the drawing if it
    /// was just drawn.
    fn bytes(&mut self, dial: f64, drawing: Option<Drawn<'a>>) -> Result<usize, PackageError> {
        let (triangles, drawn) = match drawing {
            Some(drawing) => (drawing.triangles(self.inputs), Some(drawing)),
            None => self.triangles(dial),
        };
        if let Some(&(_, Some(bytes))) = self.tried.get(&dial.to_bits()) {
            return Ok(bytes);
        }
        let drawing = drawn.unwrap_or_else(|| self.draw(dial));
        let bytes = drawing
            .file(self.inputs, self.plant, self.settings, 0, String::new())?
            .glb
            .len();
        self.tried.insert(dial.to_bits(), (triangles, Some(bytes)));
        Ok(bytes)
    }

    /// How much of `triangles` and `bytes` the drawing at `dial` takes,
    /// the larger share; 0 with neither.
    #[allow(clippy::cast_precision_loss)]
    fn taken(
        &mut self,
        dial: f64,
        triangles: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<f64, PackageError> {
        let share = |taken: usize, most: usize| taken as f64 / most.max(1) as f64;
        let mut drawing = None;
        let mut taken = 0.0;
        if let Some(most) = triangles {
            let (drawn, fresh) = self.triangles(dial);
            taken = share(drawn, most);
            drawing = fresh;
            if taken > 1.0 {
                return Ok(taken);
            }
        }
        if let Some(most) = bytes {
            taken = f64::max(taken, share(self.bytes(dial, drawing)?, most));
        }
        Ok(taken)
    }
}

/// The levels drawn to `settings`' budget, finest first: each the most
/// detail along the dial ([`crate::budget`]) that fits its share, and
/// never more than the level before it. Without a triangle budget, no
/// level draws more than the quality's level of its number.
fn budgeted(
    inputs: &Inputs,
    plant: &Plant,
    settings: Settings,
    file: &dyn Fn(usize) -> String,
) -> Result<Vec<Level>, PackageError> {
    let budget = settings.budget;
    let staged = crate::looks::staged(&plant.graph, &inputs.sizes);
    let mut trials = Trials {
        inputs,
        plant,
        settings,
        drawn: staged.as_ref().unwrap_or(&plant.graph),
        tried: HashMap::new(),
    };
    let triangles = |level: usize| budget.triangles.map(|triangles| triangles[level]);
    #[allow(clippy::cast_precision_loss)]
    let low = |level: usize| {
        if budget.triangles.is_some() {
            0.0
        } else {
            level as f64
        }
    };
    // Each level's share of the bytes, before the nearest takes what the
    // others leave: the coarsest drawing's, which no drawing goes below
    // (its textures, its stems), and of the rest a part for each level a
    // quarter of the level before's.
    let mut given: Vec<Option<usize>> = vec![None; LEVELS];
    if let Some(bytes) = budget.bytes {
        let floor = trials.bytes(crate::budget::DIAL_MAX, None)?;
        let rest = bytes.saturating_sub(floor * LEVELS);
        for (level, given) in given.iter_mut().enumerate() {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let part = (rest as f64 * NEAREST_SHARE / f64::from(1_u32 << (2 * level))) as usize;
            *given = Some(floor + part);
        }
    }
    let mut dials: Vec<f64> = Vec::with_capacity(LEVELS);
    for (level, &bytes) in given.iter().enumerate() {
        let finest = low(level).max(dials.last().copied().unwrap_or(0.0));
        let (dial, _) =
            crate::budget::most_detail(finest, |dial| trials.taken(dial, triangles(level), bytes))?;
        dials.push(dial);
    }
    // The nearest level takes the bytes the others leave.
    if let Some(bytes) = budget.bytes {
        let mut others = 0;
        for &dial in &dials[1..] {
            others += trials.bytes(dial, None)?;
        }
        let left = bytes.saturating_sub(others);
        let more = given[0].is_some_and(|share| left > share);
        given[0] = Some(left);
        if more && dials[0] > low(0) {
            let (dial, fits) = crate::budget::most_detail(low(0), |dial| {
                trials.taken(dial, triangles(0), Some(left))
            })?;
            if fits && dial < dials[0] {
                dials[0] = dial;
            }
        }
    }
    let mut files: Vec<Level> = Vec::with_capacity(LEVELS);
    let mut finer = None;
    for (level, dial) in dials.into_iter().enumerate() {
        let drawing = Drawn::at(trials.drawn, inputs, trials.detail(dial), finer);
        finer = Some(drawing.mesh.wood.triangle_count());
        let mut written = drawing.file(inputs, plant, settings, level, file(level))?;
        let (drawn, bytes) = (written.summary.triangles, written.glb.len());
        // The search counts a drawing's triangles as its file draws them.
        debug_assert_eq!(drawing.triangles(inputs), drawn);
        written.budget = Some(LevelBudget {
            triangles: triangles(level),
            bytes: given[level],
            fits: triangles(level).is_none_or(|most| drawn <= most)
                && given[level].is_none_or(|most| bytes <= most),
        });
        files.push(written);
    }
    Ok(files)
}

/// Check `levels`, drawn with their shades apart, against the package's.
fn check_built(
    inputs: &Inputs,
    plant: &Plant,
    levels: &[(PlantMesh, Vec<f32>)],
    built: &Built<'_>,
) -> Result<(), PackageError> {
    let manifest = built.manifest;
    if manifest.key != inputs.key {
        return Err(format_error("the package was built from other inputs"));
    }
    let record = manifest
        .variants
        .get(plant.variant)
        .filter(|record| record.environment == plant.environment && record.seed == plant.seed)
        .ok_or_else(|| format_error("the package does not keep this variant"))?;
    let keyframe = record
        .keyframes
        .iter()
        .find(|keyframe| (keyframe.age - plant.graph.age).abs() <= inputs.spec.growth.step * 0.5)
        .ok_or_else(|| {
            format_error(format!(
                "the package keeps no keyframe at {} years",
                plant.graph.age
            ))
        })?;
    if keyframe.lods.len() != levels.len() {
        return Err(format_error("the package holds another number of levels"));
    }
    for (level, ((mesh, shades), lod)) in levels.iter().zip(&keyframe.lods).enumerate() {
        let packaged = package::decode_mesh(&(built.object)(&lod.mesh)?)?;
        if !same_drawing(mesh, shades, &packaged) {
            return Err(format_error(format!(
                "level {level} is drawn otherwise than the package holds it"
            )));
        }
    }
    Ok(())
}

/// Write an export's files into `directory`, creating it; the paths
/// written, the index last.
///
/// # Errors
///
/// When a file cannot be written.
pub fn write(export: &Export, directory: &Path) -> Result<Vec<PathBuf>, PackageError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |error: std::io::Error| PackageError::Io {
            path,
            message: error.to_string(),
        }
    };
    std::fs::create_dir_all(directory).map_err(io(directory))?;
    let mut written = Vec::with_capacity(export.levels.len() + 1);
    for level in &export.levels {
        let path = directory.join(&level.file);
        std::fs::write(&path, &level.glb).map_err(io(&path))?;
        written.push(path);
    }
    let path = directory.join(format!("{}.json", export.stem));
    std::fs::write(&path, &export.index).map_err(io(&path))?;
    written.push(path);
    Ok(written)
}

/// `<species>-<environment>-<seed>-<age>y`, with the first eight digits
/// of a conditions class after the environment: what one variant's files
/// at one age are called.
fn stem(inputs: &Inputs, plant: &Plant) -> String {
    let class = plant.class.as_ref().map_or(String::new(), |class| {
        format!("-{}", &class[..class.len().min(8)])
    });
    format!(
        "{}-{}{class}-{}-{}y",
        inputs.spec.id,
        plant.environment.name(),
        plant.seed,
        plant.graph.age
    )
}

/// Whether `plant`, drawn with its bark's shades kept apart, is the
/// package's level `packaged` once they are multiplied in: the same
/// geometry, cards, areoles and sites, and the same colours bit for bit.
#[allow(clippy::float_cmp)]
fn same_drawing(plant: &PlantMesh, shades: &[f32], packaged: &PlantMesh) -> bool {
    let (wood, theirs) = (&plant.wood, &packaged.wood);
    let colours = wood.colors.len() == theirs.colors.len()
        && shades.len() == wood.colors.len()
        && wood.colors.iter().zip(shades).zip(&theirs.colors).all(
            |((colour, &shade), packaged)| {
                [
                    colour[0] * shade,
                    colour[1] * shade,
                    colour[2] * shade,
                    colour[3],
                ] == *packaged
            },
        );
    let cards = plant.cards.len() == packaged.cards.len()
        && plant.cards.iter().zip(&packaged.cards).all(|(a, b)| {
            // A package's cards keep no bend (its atlas records it).
            Card { bend: b.bend, ..*a } == *b
        });
    colours
        && cards
        && wood.positions == theirs.positions
        && wood.normals == theirs.normals
        && wood.uvs == theirs.uvs
        && wood.births == theirs.births
        && wood.sheds == theirs.sheds
        && wood.levels == theirs.levels
        && wood.indices == theirs.indices
        && plant.tufts == packaged.tufts
        && plant.sites == packaged.sites
}

/// One primitive's arrays, as glTF wants them.
#[derive(Default)]
struct Arrays {
    positions: Vec<f32>,
    normals: Vec<f32>,
    uvs: Vec<f32>,
    colours: Vec<f32>,
    births: Vec<f32>,
    sheds: Vec<f32>,
    winds: Vec<f32>,
    /// Wood only.
    radii: Vec<f32>,
    shades: Vec<f32>,
    indices: Vec<u32>,
}

impl Arrays {
    /// `mesh`'s vertices with each colour from `colour(vertex)`, and with
    /// texture coordinates when `uvs`. Normals are made unit length; one
    /// with no direction points up.
    fn of(mesh: &Mesh, uvs: bool, colour: &dyn Fn(usize) -> [f32; 3]) -> Self {
        let mut out = Self {
            positions: mesh.positions.iter().flatten().copied().collect(),
            normals: Vec::with_capacity(mesh.normals.len() * 3),
            colours: Vec::with_capacity(mesh.colors.len() * 3),
            births: mesh.births.clone(),
            sheds: mesh
                .sheds
                .iter()
                .map(|&shed| if shed.is_finite() { shed } else { NEVER })
                .collect(),
            winds: mesh.levels.iter().map(|&level| f32::from(level)).collect(),
            indices: mesh.indices.clone(),
            ..Self::default()
        };
        if uvs {
            out.uvs = mesh.uvs.iter().flatten().copied().collect();
        }
        for normal in &mesh.normals {
            let n = Vec3::new(
                f64::from(normal[0]),
                f64::from(normal[1]),
                f64::from(normal[2]),
            )
            .normalize_or(Vec3::Y);
            out.normals.extend_from_slice(&n.to_f32());
        }
        // glTF holds colours within 0 to 1.
        for vertex in 0..mesh.positions.len() {
            out.colours
                .extend(colour(vertex).map(|channel| channel.clamp(0.0, 1.0)));
        }
        out
    }

    /// Leave out the attributes `attributes` does not keep.
    fn keep(&mut self, attributes: Attributes) {
        if attributes != Attributes::All {
            self.births.clear();
            self.sheds.clear();
        }
        if attributes == Attributes::None {
            self.winds.clear();
            self.radii.clear();
        }
    }

    /// Write them into `document` as a primitive drawn with `material`;
    /// with a `grid`, positions, normals, texture coordinates and colours
    /// as integers (`KHR_mesh_quantization`).
    fn primitive(&self, document: &mut Document, material: Index, grid: Option<&Grid>) -> Value {
        let mut attributes = serde_json::Map::new();
        let mut insert = |name: &str, accessor: Index| {
            attributes.insert(name.into(), json!(accessor));
        };
        if let Some(grid) = grid {
            let positions: Vec<i32> = self
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|position| grid.stored(*position))
                .collect();
            insert(
                "POSITION",
                document.integer_attribute(&positions, 3, gltf::UNSIGNED_SHORT, false, true),
            );
            insert(
                "NORMAL",
                document.integer_attribute(
                    &signed_bytes(&self.normals),
                    3,
                    gltf::BYTE,
                    true,
                    false,
                ),
            );
            if !self.uvs.is_empty() {
                let uvs = if self.uvs.iter().all(|uv| (0.0..=1.0).contains(uv)) {
                    let shorts = unsigned_shorts(&self.uvs);
                    document.integer_attribute(&shorts, 2, gltf::UNSIGNED_SHORT, true, false)
                } else {
                    document.attribute(&self.uvs, 2, false)
                };
                insert("TEXCOORD_0", uvs);
            }
            let colours = unsigned_shorts(&self.colours);
            insert(
                "COLOR_0",
                document.integer_attribute(&colours, 3, gltf::UNSIGNED_SHORT, true, false),
            );
        } else {
            insert("POSITION", document.attribute(&self.positions, 3, true));
            insert("NORMAL", document.attribute(&self.normals, 3, false));
            if !self.uvs.is_empty() {
                insert("TEXCOORD_0", document.attribute(&self.uvs, 2, false));
            }
            insert("COLOR_0", document.attribute(&self.colours, 3, false));
        }
        for (name, values) in [
            ("_PLANTGEN_BIRTH", &self.births),
            ("_PLANTGEN_SHED", &self.sheds),
            ("_PLANTGEN_WIND", &self.winds),
            ("_PLANTGEN_RADIUS", &self.radii),
            ("_PLANTGEN_SHADE", &self.shades),
        ] {
            if !values.is_empty() {
                insert(name, document.attribute(values, 1, false));
            }
        }
        json!({
            "attributes": attributes,
            "indices": document.indices(&self.indices),
            "material": material,
        })
    }
}

/// Values within -1 to 1 as normalized signed bytes.
fn signed_bytes(values: &[f32]) -> Vec<i32> {
    #[allow(clippy::cast_possible_truncation)]
    values
        .iter()
        .map(|&value| (value.clamp(-1.0, 1.0) * 127.0).round() as i32)
        .collect()
}

/// Values within 0 to 1 as normalized unsigned shorts.
fn unsigned_shorts(values: &[f32]) -> Vec<i32> {
    #[allow(clippy::cast_possible_truncation)]
    values
        .iter()
        .map(|&value| (f64::from(value.clamp(0.0, 1.0)) * 65535.0).round() as i32)
        .collect()
}

/// The grid a quantized mesh's positions are stored on: each is `origin`
/// plus `step` times its three unsigned shorts, which its node's
/// translation and uniform scale give back. A uniform scale keeps the
/// normals as they are.
#[derive(Debug, Clone, Copy)]
struct Grid {
    origin: [f32; 3],
    step: f32,
}

impl Grid {
    /// The finest grid of unsigned shorts that holds every position of
    /// `arrays`.
    fn around<'a>(arrays: impl Iterator<Item = &'a Arrays>) -> Self {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for arrays in arrays {
            for position in arrays.positions.as_chunks::<3>().0 {
                for axis in 0..3 {
                    low[axis] = low[axis].min(position[axis]);
                    high[axis] = high[axis].max(position[axis]);
                }
            }
        }
        if !low[0].is_finite() {
            return Self {
                origin: [0.0; 3],
                step: 1.0,
            };
        }
        let extent = (0..3)
            .map(|axis| f64::from(high[axis]) - f64::from(low[axis]))
            .fold(0.0, f64::max);
        // A hair over the extent's share, so the farthest position fits.
        #[allow(clippy::cast_possible_truncation)]
        let step = ((extent / f64::from(u16::MAX)) * (1.0 + 1.0e-6)).max(1.0e-9) as f32;
        Self { origin: low, step }
    }

    /// `position`'s integers on the grid.
    fn stored(&self, position: [f32; 3]) -> [i32; 3] {
        #[allow(clippy::cast_possible_truncation)]
        std::array::from_fn(|axis| {
            ((f64::from(position[axis]) - f64::from(self.origin[axis])) / f64::from(self.step))
                .round()
                .clamp(0.0, f64::from(u16::MAX)) as i32
        })
    }
}

/// `mesh` with each run of identical vertices stored once, in the order
/// they first appear, and its triangles unchanged: a bent card's grid
/// shares its inner corners.
fn welded(mesh: &Mesh) -> Mesh {
    let mut out = Mesh::default();
    let mut seen: HashMap<[u32; 15], u32> = HashMap::with_capacity(mesh.positions.len());
    let mut index_of = Vec::with_capacity(mesh.positions.len());
    for vertex in 0..mesh.positions.len() {
        let (p, n, uv, c) = (
            mesh.positions[vertex],
            mesh.normals[vertex],
            mesh.uvs[vertex],
            mesh.colors[vertex],
        );
        let key = [
            p[0].to_bits(),
            p[1].to_bits(),
            p[2].to_bits(),
            n[0].to_bits(),
            n[1].to_bits(),
            n[2].to_bits(),
            uv[0].to_bits(),
            uv[1].to_bits(),
            c[0].to_bits(),
            c[1].to_bits(),
            c[2].to_bits(),
            c[3].to_bits(),
            mesh.births[vertex].to_bits(),
            mesh.sheds[vertex].to_bits(),
            u32::from(mesh.levels[vertex]),
        ];
        let next = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
        let index = *seen.entry(key).or_insert_with(|| {
            out.positions.push(p);
            out.normals.push(n);
            out.uvs.push(uv);
            out.colors.push(c);
            out.births.push(mesh.births[vertex]);
            out.sheds.push(mesh.sheds[vertex]);
            out.levels.push(mesh.levels[vertex]);
            next
        });
        index_of.push(index);
    }
    out.indices = mesh
        .indices
        .iter()
        .map(|&index| index_of[index as usize])
        .collect();
    out
}

/// A plain material: vertex colours times white, not metallic.
fn plain_material(name: &str, roughness: f32, factor: f32) -> Value {
    json!({
        "name": name,
        "pbrMetallicRoughness": {
            "baseColorFactor": numbers(&[factor, factor, factor, 1.0]),
            "metallicFactor": 0,
            "roughnessFactor": number(roughness),
        },
    })
}

/// `primitives` as one mesh named `name` and a node drawing it; quantized,
/// on one grid whose step it returns.
fn mesh_node(
    document: &mut Document,
    name: &str,
    primitives: &[(Arrays, Index)],
    quantize: bool,
) -> (Index, Option<f32>) {
    let grid = quantize.then(|| Grid::around(primitives.iter().map(|(arrays, _)| arrays)));
    let written = primitives
        .iter()
        .map(|(arrays, material)| arrays.primitive(document, *material, grid.as_ref()))
        .collect();
    let mesh = document.mesh(name, written);
    let mut node = json!({ "name": name, "mesh": mesh });
    if let Some(grid) = &grid {
        node["translation"] = numbers(&grid.origin);
        node["scale"] = numbers(&[grid.step; 3]);
    }
    (document.node(node), grid.map(|grid| grid.step))
}

/// What a level's file holds besides its wood: the organs drawn as part
/// meshes, the spines drawn as solids and each organ type's cards left.
struct Pieces {
    placements: Vec<Placement>,
    spines: Option<Mesh>,
    /// Each organ type's cards as one mesh, a bent card's grid sharing
    /// its corners, by the type's template.
    cards: Vec<(usize, Mesh)>,
}

/// Hash salts picking which organs and areoles a level drawn between two
/// levels draws as solids ([`Detail::solid_share`]), and which areoles
/// keep their cards ([`Detail::spine_cards`]).
const PART_SALT: u64 = 0x5011_D9A7;
const AREOLE_SALT: u64 = 0x5011_DA2E;

/// Whether item `number` of a level is among the `share` of them kept,
/// picked by its number's hash; at share 1, every one.
fn solid(number: usize, salt: u64, share: f64) -> bool {
    share >= 1.0 || crate::rng::unit(crate::rng::mix64(number as u64 ^ salt)) < share
}

impl Pieces {
    /// Where `plant`, a level placing `parts`, draws its organs as part
    /// meshes; which of its cards they and solid spines leave out; and the
    /// tufts it draws as solid spines. With `detail`, a level between two
    /// ([`Detail::solid_share`]), only that share of its organs and
    /// areoles; the others keep their cards.
    fn left_out(
        plant: &PlantMesh,
        inputs: &Inputs,
        parts: &[PartMesh],
        detail: &Detail,
    ) -> (Vec<Placement>, Vec<bool>, Vec<Tuft>) {
        let templates = &inputs.templates;
        let share = detail.solid_share;
        // Parts on the nearest level; their cards are left out.
        let mut placements = crate::parts::placements(plant, &inputs.looks, parts);
        let mut number = 0;
        placements.retain(|_| {
            number += 1;
            solid(number - 1, PART_SALT, share)
        });
        let mut dropped = vec![false; plant.cards.len()];
        for placement in &placements {
            dropped[placement.card] = true;
            if let Some(cross) = placement.cross {
                dropped[cross] = true;
            }
        }
        // Spines as solids where the level has areoles, in place of their
        // cards, as previews draw them. Each tuft's star and fan are the
        // spine cards in its order (`crate::body`).
        let mut tufts = Vec::new();
        if !plant.tufts.is_empty() && !templates.bodies.is_empty() {
            let first = templates.first_spine_template();
            let spine_cards: Vec<usize> = (0..plant.cards.len())
                .filter(|&card| usize::from(plant.cards[card].template) >= first)
                .collect();
            let paired = spine_cards.len() == 2 * plant.tufts.len();
            for (number, tuft) in plant.tufts.iter().enumerate() {
                if !paired || solid(number, AREOLE_SALT, share) {
                    tufts.push(*tuft);
                    if paired {
                        dropped[spine_cards[2 * number]] = true;
                        dropped[spine_cards[2 * number + 1]] = true;
                    }
                }
            }
            if !paired {
                for &card in &spine_cards {
                    dropped[card] = true;
                }
            }
        } else if detail.spine_cards < 1.0 && !templates.bodies.is_empty() {
            // Areoles as their star and fan, in order: only a share.
            let first = templates.first_spine_template();
            let spine_cards: Vec<usize> = (0..plant.cards.len())
                .filter(|&card| usize::from(plant.cards[card].template) >= first)
                .collect();
            let (pairs, rest) = spine_cards.as_chunks::<2>();
            if rest.is_empty() {
                for (number, [star, fan]) in pairs.iter().enumerate() {
                    if !solid(number, AREOLE_SALT, detail.spine_cards) {
                        dropped[*star] = true;
                        dropped[*fan] = true;
                    }
                }
            }
        }
        (placements, dropped, tufts)
    }

    /// The pieces of `plant`, a level placing `parts`, drawn with `detail`'s
    /// solids.
    fn of(plant: &PlantMesh, inputs: &Inputs, parts: &[PartMesh], detail: &Detail) -> Self {
        let templates = &inputs.templates;
        let (placements, dropped, tufts) = Self::left_out(plant, inputs, parts, detail);
        let spines = (!tufts.is_empty()).then(|| {
            let looks: Vec<&crate::body::BodyLook> = templates.bodies.iter().collect();
            crate::spines::tuft_mesh_within(&tufts, &looks, detail.solid_detail)
        });
        let cards = (0..templates.templates.len())
            .filter_map(|index| {
                let cards: Vec<Card> = plant
                    .cards
                    .iter()
                    .zip(&dropped)
                    .filter(|(card, drop)| !**drop && usize::from(card.template) == index)
                    .map(|(card, _)| *card)
                    .collect();
                let mesh = PlantMesh {
                    cards,
                    ..PlantMesh::default()
                }
                .card_mesh();
                (!mesh.indices.is_empty()).then(|| (index, welded(&mesh)))
            })
            .collect();
        Self {
            placements,
            spines,
            cards,
        }
    }

    /// Triangles the file of [`Self::of`] draws, each placement counted,
    /// without drawing its pieces.
    fn count(plant: &PlantMesh, inputs: &Inputs, parts: &[PartMesh], detail: &Detail) -> usize {
        let (placements, dropped, tufts) = Self::left_out(plant, inputs, parts, detail);
        let looks: Vec<&crate::body::BodyLook> = inputs.templates.bodies.iter().collect();
        let spines = crate::spines::tuft_triangles(&tufts, &looks, detail.solid_detail);
        let bent = 2 * (crate::bend::ACROSS * crate::bend::ALONG) as usize;
        let cards: usize = plant
            .cards
            .iter()
            .zip(&dropped)
            .filter(|(_, drop)| !**drop)
            .map(|(card, _)| if card.bend.is_flat() { 2 } else { bent })
            .sum();
        let placed: usize = placements
            .iter()
            .map(|placed| parts[placed.part].variants[placed.variant].triangle_count())
            .sum();
        plant.wood.triangle_count() + spines + cards + placed
    }
}

/// One level as a GLB file, with `extras` given the level's largest
/// position step when quantized; and that step.
#[allow(clippy::too_many_lines)]
fn level_glb(
    plant: &PlantMesh,
    shades: &[f32],
    pieces: &Pieces,
    parts: &[PartMesh],
    inputs: &Inputs,
    settings: Settings,
    extras: &dyn Fn(Option<f32>) -> Value,
) -> (Vec<u8>, Option<f32>) {
    let templates = &inputs.templates;
    let quantize = settings.compression != Compression::None;
    let mut document = Document::new();
    if quantize {
        document.extension("KHR_mesh_quantization", true);
    }
    if settings.compression == Compression::Meshopt {
        document.meshopt();
    }
    let mut children = Vec::new();
    let mut steps = Vec::new();

    // Wood: plain colours, the bark's shade apart or multiplied in.
    if !plant.wood.indices.is_empty() {
        let wood = &plant.wood;
        let mut arrays = Arrays::of(wood, true, &|vertex| {
            let [r, g, b, _] = wood.colors[vertex];
            match settings.shade {
                Shade::Attribute => [r, g, b],
                Shade::Baked => {
                    let s = shades.get(vertex).copied().unwrap_or(1.0);
                    [r * s, g * s, b * s]
                }
            }
        });
        // Bark vertices hold minus their stem's radius in alpha.
        arrays.radii = wood
            .colors
            .iter()
            .map(|colour| if colour[3] < 0.0 { -colour[3] } else { 0.0 })
            .collect();
        if settings.shade == Shade::Attribute {
            arrays.shades = shades.to_vec();
        }
        arrays.keep(settings.attributes);
        let material = document.material(plain_material("wood", 0.9, 1.0));
        let (node, step) = mesh_node(&mut document, "wood", &[(arrays, material)], quantize);
        children.push(node);
        steps.extend(step);
    }

    // Spines as solids where the level has areoles.
    if let Some(spines) = pieces
        .spines
        .as_ref()
        .filter(|mesh| !mesh.indices.is_empty())
    {
        let mut arrays = Arrays::of(spines, false, &|vertex| {
            let [r, g, b, _] = spines.colors[vertex];
            [r, g, b]
        });
        arrays.keep(settings.attributes);
        let material = document.material(plain_material("spines", 0.6, 1.0));
        let (node, step) = mesh_node(&mut document, "spines", &[(arrays, material)], quantize);
        children.push(node);
        steps.extend(step);
    }

    // Cards: a primitive per organ type, each with its own texture, all
    // sampled alike.
    let mut sampler = None;
    let mut primitives = Vec::new();
    for (index, quads) in &pieces.cards {
        let template = &templates.templates[*index];
        // Each card's colour over the look's, and the texture's scale: as
        // natural as the colours' range allows (see `texture_scale`).
        let look = template.colour.map(f64::from);
        let ratios: Vec<[f64; 3]> = quads
            .colors
            .iter()
            .map(|&[r, g, b, _]| relative([r, g, b], look))
            .collect();
        let brightest = ratios.iter().flatten().copied().fold(0.0, f64::max);
        let albedo = template_albedo(template);
        let scale = texture_scale(&albedo, template, brightest);
        let png = texture(template, &albedo, scale);
        let name = format!("{} {}", template.organ, template.shape);
        let image = document.png(&name, &png);
        let sampler = *sampler.get_or_insert_with(|| {
            document.sampler(json!({
                "magFilter": gltf::LINEAR,
                "minFilter": gltf::LINEAR_MIPMAP_LINEAR,
                "wrapS": gltf::CLAMP_TO_EDGE,
                "wrapT": gltf::CLAMP_TO_EDGE,
            }))
        });
        let texture = document.texture(image, sampler);
        let material = document.material(json!({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": texture },
                "metallicFactor": 0,
                "roughnessFactor": number(0.8),
            },
            "alphaMode": "MASK",
            "alphaCutoff": 0.5,
            "doubleSided": true,
        }));
        #[allow(clippy::cast_possible_truncation)]
        let mut arrays = Arrays::of(quads, true, &|vertex| {
            ratios[vertex].map(|ratio| (ratio * scale) as f32)
        });
        arrays.keep(settings.attributes);
        primitives.push((arrays, material));
    }
    if !primitives.is_empty() {
        let (node, step) = mesh_node(&mut document, "cards", &primitives, quantize);
        children.push(node);
        steps.extend(step);
    }

    if !pieces.placements.is_empty() {
        let parts = part_nodes(&mut document, &pieces.placements, parts, inputs, settings);
        children.push(document.node(json!({ "name": "parts", "children": parts })));
    }

    let root = document.node(json!({
        "name": inputs.spec.id,
        "children": children,
    }));
    let step = steps.into_iter().reduce(f32::max);
    let glb = document.glb(
        json!({
            "version": "2.0",
            "generator": format!("PlantGen plantc, generator revision {}", crate::GENERATOR_REVISION),
        }),
        &[root],
        Value::Object(std::iter::once(("plantgen".to_owned(), extras(step))).collect()),
    );
    (glb, step)
}

/// A card's colour over its look's colour, channel by channel: what the
/// look's texture is multiplied by to give the card's. A channel too dark
/// to divide by takes the luminance ratio, as
/// [`crate::templates::Templates::albedo`] darkens an accent.
fn relative(colour: [f32; 3], look: [f64; 3]) -> [f64; 3] {
    let luminance = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let card = colour.map(f64::from);
    let darkening = if luminance(look) > 1e-6 {
        (luminance(card) / luminance(look)).clamp(0.0, 2.0)
    } else {
        1.0
    };
    std::array::from_fn(|channel| {
        if look[channel] > DARKEST_CHANNEL {
            card[channel] / look[channel]
        } else {
            darkening
        }
    })
}

/// Each texel of a template in its look's own colours: its albedo for a
/// card of exactly the look's colour.
fn template_albedo(template: &Template) -> Vec<[f64; 3]> {
    let look = template.colour.map(f64::from);
    let accent = template.accent_colour.map(f64::from);
    (0..template.coverage.len())
        .map(|at| {
            let weight = f64::from(template.accent[at]);
            let brightness = f64::from(template.brightness[at]);
            std::array::from_fn(|c| (look[c] + (accent[c] - look[c]) * weight) * brightness)
        })
        .collect()
}

/// What a template's texture is divided by and its cards' colours (over
/// the look's, at most `brightest`) multiplied by, so both stay within 0 to
/// 1 as glTF requires: 1 when they already do, else the least that keeps
/// the texture within range, the texture as near the look's own colours as
/// the cards' colours allow. Where no scale fits both, the texture keeps
/// its range and the brightest cards' colours are clipped at 1.
fn texture_scale(albedo: &[[f64; 3]], template: &Template, brightest: f64) -> f64 {
    let lightest = albedo
        .iter()
        .zip(&template.coverage)
        .filter(|(_, coverage)| **coverage > 0.0)
        .flat_map(|(colour, _)| colour.iter().copied())
        .fold(0.0_f64, f64::max);
    let most = if brightest > 0.0 {
        1.0 / brightest
    } else {
        1.0
    };
    lightest.max(most.min(1.0)).max(1e-6)
}

/// A template's texture as PNG: each texel its albedo (see
/// [`template_albedo`]) over `scale`, sRGB, coverage in alpha; uncovered
/// texels take the colour of the nearest covered one.
fn texture(template: &Template, albedo: &[[f64; 3]], scale: f64) -> Vec<u8> {
    let texels = template.coverage.len();
    let size = side_of(texels);
    // Colour bleeds outward from covered texels, nearest first.
    let mut source: Vec<Option<usize>> = (0..texels)
        .map(|at| (template.coverage[at] > 0.0).then_some(at))
        .collect();
    let mut frontier: Vec<usize> = (0..texels).filter(|&at| source[at].is_some()).collect();
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for &at in &frontier {
            let (x, y) = (at % size, at / size);
            let neighbours = [
                (x > 0).then(|| at - 1),
                (x + 1 < size).then(|| at + 1),
                (y > 0).then(|| at - size),
                (y + 1 < size).then(|| at + size),
            ];
            for neighbour in neighbours.into_iter().flatten() {
                if source[neighbour].is_none() {
                    source[neighbour] = source[at];
                    next.push(neighbour);
                }
            }
        }
        frontier = next;
    }
    let mut rgba = Vec::with_capacity(texels * 4);
    for at in 0..texels {
        let colour = albedo[source[at].unwrap_or(at)];
        for channel in colour {
            #[allow(clippy::cast_possible_truncation)]
            let linear = (channel / scale) as f32;
            rgba.push(crate::raster::to_u8(crate::raster::srgb(linear)));
        }
        rgba.push(crate::raster::to_u8(template.coverage[at]));
    }
    // Row 0 is the cards' base, v = 0, which is a glTF image's first row.
    crate::raster::encode_png(size, size, &rgba).unwrap_or_default()
}

/// The side of a square template of `texels` texels.
fn side_of(texels: usize) -> usize {
    let mut side = 1;
    while side * side < texels {
        side += 1;
    }
    side
}

/// The part meshes of `placements` and the nodes placing them: a node per
/// organ, or, quantized, a node per mesh drawing each of its organs as an
/// instance (`EXT_mesh_gpu_instancing`).
fn part_nodes(
    document: &mut Document,
    placements: &[Placement],
    parts: &[PartMesh],
    inputs: &Inputs,
    settings: Settings,
) -> Vec<Index> {
    // Each part's colours are multiplied by its lightest organ's shade, as
    // far as they stay within 1, so its materials' factors, the shades over
    // that, stay at most 1 (a lighter organ's is clipped there).
    let mut scales = vec![0.0_f32; parts.len()];
    for placement in placements {
        scales[placement.part] = scales[placement.part].max(placement.darkening);
    }
    for (scale, part) in scales.iter_mut().zip(parts) {
        let lightest = part
            .variants
            .iter()
            .flat_map(|mesh| {
                mesh.colors
                    .iter()
                    .flat_map(|colour| colour[..3].iter().copied())
            })
            .fold(0.0_f32, f32::max);
        if lightest > 0.0 {
            *scale = scale.min(1.0 / lightest);
        }
    }
    // Accessors per part variant, meshes per variant and shade step, a
    // material per part and shade step, as they are first needed.
    let mut accessors: BTreeMap<(usize, usize), Value> = BTreeMap::new();
    let mut meshes: BTreeMap<(usize, usize, u32), Index> = BTreeMap::new();
    let mut materials: BTreeMap<(usize, u32), Index> = BTreeMap::new();
    // Each placement's mesh, translation, rotation and scale.
    let mut placed = Vec::with_capacity(placements.len());
    for placement in placements {
        let (part, variant) = (placement.part, placement.variant);
        let scale = if scales[part] > 0.0 {
            scales[part]
        } else {
            1.0
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let step = ((placement.darkening / scale * SHADE_STEPS).round() as u32).clamp(1, 16);
        let organ = inputs
            .templates
            .templates
            .get(parts[part].template)
            .map_or("part", |template| template.organ.as_str());
        let material = *materials.entry((part, step)).or_insert_with(|| {
            #[allow(clippy::cast_precision_loss)]
            let factor = step as f32 / SHADE_STEPS;
            document.material(plain_material(&format!("{organ} part"), 0.7, factor))
        });
        let primitive = accessors
            .entry((part, variant))
            .or_insert_with(|| {
                let mesh = &parts[part].variants[variant];
                let mut arrays = Arrays::of(mesh, false, &|vertex| {
                    let [r, g, b, _] = mesh.colors[vertex];
                    [r * scale, g * scale, b * scale]
                });
                arrays.keep(settings.attributes);
                arrays.primitive(document, 0, None)
            })
            .clone();
        let mesh = *meshes.entry((part, variant, step)).or_insert_with(|| {
            let mut primitive = primitive;
            primitive["material"] = json!(material);
            document.mesh(&format!("{organ} part {variant}"), vec![primitive])
        });
        let rotation = quaternion(placement.left, placement.heading, placement.out);
        #[allow(clippy::cast_possible_truncation)]
        let (translation, length) = (placement.base.to_f32(), placement.length as f32);
        placed.push((mesh, translation, rotation, length));
    }
    if settings.compression == Compression::None {
        return placed
            .into_iter()
            .map(|(mesh, translation, rotation, length)| {
                document.node(json!({
                    "mesh": mesh,
                    "translation": numbers(&translation),
                    "rotation": numbers(&rotation),
                    "scale": numbers(&[length, length, length]),
                }))
            })
            .collect();
    }
    // Instances by mesh, each in the order its organs were placed.
    document.extension("EXT_mesh_gpu_instancing", true);
    let mut by_mesh: BTreeMap<Index, [Vec<f32>; 3]> = BTreeMap::new();
    for (mesh, translation, rotation, length) in placed {
        let [translations, rotations, scales] = by_mesh.entry(mesh).or_default();
        translations.extend_from_slice(&translation);
        rotations.extend_from_slice(&rotation);
        scales.extend_from_slice(&[length; 3]);
    }
    by_mesh
        .into_iter()
        .map(|(mesh, [translations, rotations, scales])| {
            let attributes = json!({
                "TRANSLATION": document.instance_attribute(&translations, 3),
                "ROTATION": document.instance_attribute(&rotations, 4),
                "SCALE": document.instance_attribute(&scales, 3),
            });
            document.node(json!({
                "mesh": mesh,
                "extensions": { "EXT_mesh_gpu_instancing": { "attributes": attributes } },
            }))
        })
        .collect()
}

/// The rotation, as a unit quaternion `[x, y, z, w]`, that turns the axes
/// x, y and z to `x`, `y` and `z`, a right-handed orthonormal frame.
#[allow(clippy::many_single_char_names)]
fn quaternion(x: Vec3, y: Vec3, z: Vec3) -> [f32; 4] {
    // Rows and columns of the rotation matrix whose columns are x, y, z.
    let m = [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]];
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = math::sqrt(trace + 1.0) * 2.0;
        [
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        ]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = math::sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2.0;
        [
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        ]
    } else if m[1][1] > m[2][2] {
        let s = math::sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2.0;
        [
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        ]
    } else {
        let s = math::sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2.0;
        [
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        ]
    };
    let length = math::sqrt(q.iter().map(|value| value * value).sum());
    #[allow(clippy::cast_possible_truncation)]
    q.map(|value| (value / length) as f32)
}

/// What every file says about the plant, in `extras.plantgen`; with a
/// level, that level's.
fn metadata(
    inputs: &Inputs,
    plant: &Plant,
    level: Option<(usize, &Detail, Option<f32>)>,
    settings: Settings,
) -> Value {
    let spec = &inputs.spec;
    let mut value = json!({
        "format": "plantgen-export",
        "format_version": EXPORT_FORMAT,
        "species": spec.id,
        "scientific_name": spec.taxon.scientific_name,
        "common_name": spec.taxon.common_name,
        "family": spec.taxon.family,
        "package_key": inputs.key,
        "generator_revision": crate::GENERATOR_REVISION,
        "quality": inputs.quality.name,
        "levels": inputs.quality.lods.len(),
        "environment": plant.environment.name(),
        "seed": plant.seed,
        "class": plant.class,
        "age": plant.graph.age,
        "day": inputs.day,
        "size": plant.size,
        "units": "metres from the plant's foot, +Y up; ages in years",
        "shade": settings.shade.name(),
        "compression": settings.compression.name(),
        "attributes": settings.attributes.name(),
        "provenance": spec.provenance,
    });
    if let (Some((level, detail, step)), Value::Object(fields)) = (level, &mut value) {
        fields.insert("level".into(), json!(level));
        if let Some(step) = step {
            fields.insert("position_step".into(), number(step));
        }
        fields.insert("level_detail".into(), level_detail(detail));
    }
    value
}

/// How a level is drawn, as the files and the index record it: the
/// quality's level whose features it draws, the dial
/// ([`crate::budget`]), the thinnest wood it keeps (stems are always
/// kept), its tubes' ring spacing and most sides, the size of its leaf
/// clusters (0 for a card per organ), the share of their caps its part
/// meshes hold and of their spines its solid areoles keep, the share of
/// its organs and areoles drawn as solids, the share of its areoles drawn
/// as cards that keep them and the share of its bent cards drawn flat.
fn level_detail(detail: &Detail) -> Value {
    let lod = &detail.lod;
    json!({
        "dial": detail.dial,
        "features_of_level": detail.level,
        "thinnest_wood_radius": lod.min_radius,
        "ring_edge": lod.ring_edge,
        "max_sides": lod.max_sides,
        "bend": lod.bend,
        "cluster": lod.cluster,
        "solid_detail": detail.solid_detail,
        "solid_share": detail.solid_share,
        "spine_card_share": detail.spine_cards,
        "flat_share": detail.flat,
    })
}

/// The index of an export: the plant, its budget, and each level's file,
/// size and drawing.
fn index(
    inputs: &Inputs,
    plant: &Plant,
    levels: &[Level],
    settings: Settings,
) -> Result<Vec<u8>, PackageError> {
    let mut value = metadata(inputs, plant, None, settings);
    if let Value::Object(fields) = &mut value {
        let budget = settings.budget;
        if budget.is_some() {
            let fits = levels
                .iter()
                .all(|level| level.budget.is_some_and(|given| given.fits));
            fields.insert(
                "budget".into(),
                json!({ "triangles": budget.triangles, "bytes": budget.bytes, "fits": fits }),
            );
        }
        fields.insert(
            "files".into(),
            Value::Array(
                levels
                    .iter()
                    .enumerate()
                    .map(|(level, file)| {
                        let mut entry = json!({
                            "level": level,
                            "file": file.file,
                            "bytes": file.glb.len(),
                            "triangles": file.summary.triangles,
                            "nodes": file.summary.nodes,
                            "instances": file.summary.instances,
                            "position_step": file.position_step.map(number),
                            "thinnest_wood_radius": file.detail.lod.min_radius,
                            "cluster": file.detail.lod.cluster,
                            "detail": level_detail(&file.detail),
                        });
                        if let Some(given) = file.budget {
                            entry["budget"] = json!({
                                "triangles": given.triangles,
                                "bytes": given.bytes,
                                "fits": given.fits,
                            });
                        }
                        entry
                    })
                    .collect(),
            ),
        );
    }
    let mut bytes = crate::json::to_vec_pretty(&value).map_err(format_error)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `v` turned by the unit quaternion `q` (`[x, y, z, w]`), as glTF turns
    /// a node.
    fn turn(q: [f32; 4], v: Vec3) -> Vec3 {
        let u = Vec3::new(f64::from(q[0]), f64::from(q[1]), f64::from(q[2]));
        let t = u.cross(v) * 2.0;
        v + t * f64::from(q[3]) + u.cross(t)
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn a_bent_cards_grid_shares_its_corners() {
        use crate::bend::{self, Bend};
        let card = Card {
            segment: crate::mesh::NO_SEGMENT,
            base: [0.0, 1.0, 0.0],
            heading: [0.0, 1.0, 0.0],
            left: [1.0, 0.0, 0.0],
            length: 0.1,
            width: 0.04,
            color: [0.2, 0.5, 0.1],
            template: 0,
            born: 2.0,
            shed: f32::INFINITY,
            bend: Bend {
                fold: 20.0,
                droop: 30.0,
                ..Bend::FLAT
            },
        };
        let flat = Card {
            bend: Bend::FLAT,
            ..card
        };
        let mesh = PlantMesh {
            cards: vec![card, flat],
            ..PlantMesh::default()
        }
        .card_mesh();
        let shared = welded(&mesh);
        // The bent card's grid of cells keeps one vertex per corner, the
        // flat card's quad its four.
        let corners = (bend::ACROSS + 1) * (bend::ALONG + 1) + 4;
        assert_eq!(shared.positions.len(), corners as usize);
        assert!(mesh.positions.len() > shared.positions.len());
        assert_eq!(shared.indices.len(), mesh.indices.len());
        for (&old, &new) in mesh.indices.iter().zip(&shared.indices) {
            let (old, new) = (old as usize, new as usize);
            assert_eq!(mesh.positions[old], shared.positions[new]);
            assert_eq!(mesh.normals[old], shared.normals[new]);
            assert_eq!(mesh.uvs[old], shared.uvs[new]);
            assert_eq!(mesh.colors[old], shared.colors[new]);
            assert_eq!(mesh.births[old].to_bits(), shared.births[new].to_bits());
            assert_eq!(mesh.sheds[old].to_bits(), shared.sheds[new].to_bits());
            assert_eq!(mesh.levels[old], shared.levels[new]);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn positions_sit_within_half_a_step_of_the_grid() {
        let arrays = Arrays {
            positions: vec![-0.5, 0.0, 0.25, 3.0, 12.5, -0.75, 0.1, 0.2, 0.3],
            ..Arrays::default()
        };
        let grid = Grid::around(std::iter::once(&arrays));
        assert_eq!(grid.origin, [-0.5, 0.0, -0.75]);
        // The tallest axis, 12.5 m, over 65,535 steps.
        assert!((f64::from(grid.step) - 12.5 / 65535.0).abs() < 1.0e-9);
        for position in arrays.positions.as_chunks::<3>().0 {
            let stored = grid.stored(*position);
            for axis in 0..3 {
                assert!((0..=i32::from(u16::MAX)).contains(&stored[axis]));
                let back =
                    f64::from(grid.origin[axis]) + f64::from(grid.step) * f64::from(stored[axis]);
                assert!((back - f64::from(position[axis])).abs() <= f64::from(grid.step) * 0.5);
            }
        }
        assert_eq!(grid.stored([-0.5, 12.5, -0.75])[1], i32::from(u16::MAX));
    }

    #[test]
    fn quaternions_turn_the_axes_onto_the_frame() {
        let turns = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(1.0, 2.0, -0.5),
            Vec3::new(-0.3, 0.1, 0.9),
        ];
        // Turns near a half turn reach every branch of the conversion.
        for axis in turns {
            let axis = axis.normalize_or(Vec3::Y);
            for angle in [0.0, 0.4, 1.7, 3.0, std::f64::consts::PI - 0.002, -2.2] {
                let frame = [Vec3::X, Vec3::Y, Vec3::Z].map(|v| v.rotate_about(axis, angle));
                let q = quaternion(frame[0], frame[1], frame[2]);
                for (from, to) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(frame) {
                    assert!(turn(q, from).distance(to) < 1e-5, "{axis:?} {angle}");
                }
            }
        }
    }

    #[test]
    fn card_colours_are_relative_to_their_look() {
        let look = [0.2, 0.4, 0.005];
        let [r, g, b] = relative([0.1, 0.6, 0.5], look);
        assert!((r - 0.5).abs() < 1e-6 && (g - 1.5).abs() < 1e-6);
        // Too dark a channel takes the luminance ratio.
        let luminance = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let ratio = luminance([0.1, 0.6, 0.5]) / luminance([0.2, 0.4, 0.005]);
        assert!((b - ratio).abs() < 1e-6);
    }
}
