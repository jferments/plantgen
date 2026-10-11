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
//! - **Cards**: each organ card as a quad, a bent card as its grid, with
//!   one primitive and material per organ type. Its texture is the type's
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
//! Not yet: the bark's pattern (a tile per species or a bake per tree,
//! waiting on the owner's call), normal and roughness maps, tangents,
//! impostors, coverage-preserving mipmaps (PNG holds no mipmaps), wind and
//! growth animation, and other formats.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::gltf::{self, Document, Index, number, numbers};
use crate::graph::PlantGraph;
use crate::library::Library;
use crate::math::{self, Vec3};
use crate::mesh::{Card, Mesh, PlantMesh};
use crate::package::{self, Inputs, Manifest, Measurements, PackageError};
use crate::parts::Placement;
use crate::quality::{self, Quality};
use crate::spec::{Environment, PlantSpec};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub file: String,
    pub glb: Vec<u8>,
    pub summary: gltf::Summary,
}

/// An exported plant: its files' common name, a GLB file per level and the
/// index (JSON) naming them.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Export `plant` of `inputs`, writing the bark's shade as `shade`; with
/// `built`, the package of the same inputs, check that every level is
/// drawn as the package holds it.
///
/// # Errors
///
/// When `built` is another package, does not keep the plant's variant and
/// age, or holds levels drawn otherwise; or when an object does not decode.
pub fn export(
    inputs: &Inputs,
    plant: &Plant,
    shade: Shade,
    built: Option<&Built<'_>>,
) -> Result<Export, PackageError> {
    let levels = package::level_meshes(&plant.graph, inputs, true);
    if let Some(built) = built {
        check_built(inputs, plant, &levels, built)?;
    }
    let stem = stem(inputs, plant);
    let mut files = Vec::with_capacity(levels.len());
    for (level, (mesh, shades)) in levels.into_iter().enumerate() {
        let file = format!("{stem}-lod{level}.glb");
        let extras = metadata(inputs, plant, Some(level), shade);
        let glb = level_glb(&mesh, &shades, inputs, shade, extras);
        let summary =
            gltf::check(&glb).map_err(|error| format_error(format!("{file}: {error}")))?;
        files.push(Level { file, glb, summary });
    }
    let index = index(inputs, plant, &files, shade)?;
    Ok(Export {
        stem,
        levels: files,
        index,
    })
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

    /// Write them into `document` as a primitive drawn with `material`.
    fn primitive(&self, document: &mut Document, material: Index) -> Value {
        let mut attributes = serde_json::Map::new();
        attributes.insert(
            "POSITION".into(),
            json!(document.attribute(&self.positions, 3, true)),
        );
        attributes.insert(
            "NORMAL".into(),
            json!(document.attribute(&self.normals, 3, false)),
        );
        if !self.uvs.is_empty() {
            attributes.insert(
                "TEXCOORD_0".into(),
                json!(document.attribute(&self.uvs, 2, false)),
            );
        }
        attributes.insert(
            "COLOR_0".into(),
            json!(document.attribute(&self.colours, 3, false)),
        );
        for (name, values) in [
            ("_PLANTGEN_BIRTH", &self.births),
            ("_PLANTGEN_SHED", &self.sheds),
            ("_PLANTGEN_WIND", &self.winds),
            ("_PLANTGEN_RADIUS", &self.radii),
            ("_PLANTGEN_SHADE", &self.shades),
        ] {
            if !values.is_empty() {
                attributes.insert(name.into(), json!(document.attribute(values, 1, false)));
            }
        }
        json!({
            "attributes": attributes,
            "indices": document.indices(&self.indices),
            "material": material,
        })
    }
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

/// One level as a GLB file.
#[allow(clippy::too_many_lines)]
fn level_glb(
    plant: &PlantMesh,
    shades: &[f32],
    inputs: &Inputs,
    shade: Shade,
    extras: Value,
) -> Vec<u8> {
    let templates = &inputs.templates;
    let mut document = Document::new();
    let mut children = Vec::new();

    // Wood: plain colours, the bark's shade apart or multiplied in.
    if !plant.wood.indices.is_empty() {
        let wood = &plant.wood;
        let mut arrays = Arrays::of(wood, true, &|vertex| {
            let [r, g, b, _] = wood.colors[vertex];
            match shade {
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
        if shade == Shade::Attribute {
            arrays.shades = shades.to_vec();
        }
        let material = document.material(plain_material("wood", 0.9, 1.0));
        let primitive = arrays.primitive(&mut document, material);
        let mesh = document.mesh("wood", vec![primitive]);
        children.push(document.node(json!({ "name": "wood", "mesh": mesh })));
    }

    // Parts on the nearest level; their cards are left out.
    let placements = crate::parts::placements(plant, &inputs.looks, &inputs.parts);
    let mut dropped = vec![false; plant.cards.len()];
    for placement in &placements {
        dropped[placement.card] = true;
        if let Some(cross) = placement.cross {
            dropped[cross] = true;
        }
    }

    // Spines as solids where the level has areoles, in place of their
    // cards, as previews draw them.
    let solid_spines = !plant.tufts.is_empty() && !templates.bodies.is_empty();
    if solid_spines {
        let first = templates.first_spine_template();
        for (card, drop) in plant.cards.iter().zip(dropped.iter_mut()) {
            *drop |= usize::from(card.template) >= first;
        }
        let looks: Vec<&crate::body::BodyLook> = templates.bodies.iter().collect();
        let spines = crate::spines::tuft_mesh(&plant.tufts, &looks);
        if !spines.indices.is_empty() {
            let arrays = Arrays::of(&spines, false, &|vertex| {
                let [r, g, b, _] = spines.colors[vertex];
                [r, g, b]
            });
            let material = document.material(plain_material("spines", 0.6, 1.0));
            let primitive = arrays.primitive(&mut document, material);
            let mesh = document.mesh("spines", vec![primitive]);
            children.push(document.node(json!({ "name": "spines", "mesh": mesh })));
        }
    }

    // Cards: a primitive per organ type, each with its own texture.
    let sampler = document.sampler(json!({
        "magFilter": gltf::LINEAR,
        "minFilter": gltf::LINEAR_MIPMAP_LINEAR,
        "wrapS": gltf::CLAMP_TO_EDGE,
        "wrapT": gltf::CLAMP_TO_EDGE,
    }));
    let mut primitives = Vec::new();
    for (index, template) in templates.templates.iter().enumerate() {
        let cards: Vec<Card> = plant
            .cards
            .iter()
            .zip(&dropped)
            .filter(|(card, drop)| !**drop && usize::from(card.template) == index)
            .map(|(card, _)| *card)
            .collect();
        if cards.is_empty() {
            continue;
        }
        let quads = PlantMesh {
            cards,
            ..PlantMesh::default()
        }
        .card_mesh();
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
        let arrays = Arrays::of(&quads, true, &|vertex| {
            ratios[vertex].map(|ratio| (ratio * scale) as f32)
        });
        primitives.push(arrays.primitive(&mut document, material));
    }
    if !primitives.is_empty() {
        let mesh = document.mesh("cards", primitives);
        children.push(document.node(json!({ "name": "cards", "mesh": mesh })));
    }

    if !placements.is_empty() {
        let parts = part_nodes(&mut document, &placements, inputs);
        children.push(document.node(json!({ "name": "parts", "children": parts })));
    }

    let root = document.node(json!({
        "name": inputs.spec.id,
        "children": children,
    }));
    document.glb(
        json!({
            "version": "2.0",
            "generator": format!("PlantGen plantc, generator revision {}", crate::GENERATOR_REVISION),
        }),
        &[root],
        Value::Object(std::iter::once(("plantgen".to_owned(), extras)).collect()),
    )
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

/// The part meshes of `placements` and a node placing each; the nodes.
fn part_nodes(document: &mut Document, placements: &[Placement], inputs: &Inputs) -> Vec<Index> {
    let parts = &inputs.parts;
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
    let mut nodes = Vec::with_capacity(placements.len());
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
                let arrays = Arrays::of(mesh, false, &|vertex| {
                    let [r, g, b, _] = mesh.colors[vertex];
                    [r * scale, g * scale, b * scale]
                });
                arrays.primitive(document, 0)
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
        nodes.push(document.node(json!({
            "mesh": mesh,
            "translation": numbers(&translation),
            "rotation": numbers(&rotation),
            "scale": numbers(&[length, length, length]),
        })));
    }
    nodes
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
fn metadata(inputs: &Inputs, plant: &Plant, level: Option<usize>, shade: Shade) -> Value {
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
        "shade": shade.name(),
        "provenance": spec.provenance,
    });
    if let (Some(level), Value::Object(fields)) = (level, &mut value) {
        let lod = inputs.quality.lods[level].for_height(plant.graph.height);
        fields.insert("level".into(), json!(level));
        fields.insert(
            "level_detail".into(),
            json!({
                "thinnest_wood_radius": lod.min_radius,
                "ring_edge": lod.ring_edge,
                "cluster": lod.cluster,
            }),
        );
    }
    value
}

/// The index of an export: the plant, and each level's file and size.
fn index(
    inputs: &Inputs,
    plant: &Plant,
    levels: &[Level],
    shade: Shade,
) -> Result<Vec<u8>, PackageError> {
    let mut value = metadata(inputs, plant, None, shade);
    if let Value::Object(fields) = &mut value {
        fields.insert(
            "files".into(),
            Value::Array(
                levels
                    .iter()
                    .enumerate()
                    .map(|(level, file)| {
                        let lod = inputs.quality.lods[level].for_height(plant.graph.height);
                        json!({
                            "level": level,
                            "file": file.file,
                            "bytes": file.glb.len(),
                            "triangles": file.summary.triangles,
                            "nodes": file.summary.nodes,
                            "thinnest_wood_radius": lod.min_radius,
                            "cluster": lod.cluster,
                        })
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
    fn quaternions_turn_the_axes_onto_the_frame() {
        let axes = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(1.0, 2.0, -0.5),
            Vec3::new(-0.3, 0.1, 0.9),
        ];
        // Turns near a half turn reach every branch of the conversion.
        for axis in axes {
            let axis = axis.normalize_or(Vec3::Y);
            for angle in [0.0, 0.4, 1.7, 3.0, 3.14, -2.2] {
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
