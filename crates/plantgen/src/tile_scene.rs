//! A filled tile as one glTF file ([`crate::fill::TilePlan::glb`]): each
//! prototype's meshes once, and a node per prototype mesh drawing every
//! plant that uses it with `EXT_mesh_gpu_instancing`. Blender, Godot,
//! Unity's `glTFast` and three.js open it with no `PlantGen` code.
//!
//! The meshes are copied from the prototype files `plantc fill` (or
//! [`crate::fill::Fill::write_prototypes`]) wrote, at one level for the
//! whole tile; a caller that wants each plant, or each part of a tall
//! plant, at the level its distance asks for reads the tile's packed
//! instances and the prototype files instead.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::api::Error;
use crate::fill::{Instance, PrototypeFiles, TRIANGLE_PIXELS, TilePlan};
use crate::gltf::{Document, Glb, Index};

/// A placement as glTF composes them: translation, rotation (x, y, z, w)
/// and one scale for every axis, y up.
#[derive(Debug, Clone, Copy)]
struct Place {
    translation: [f64; 3],
    rotation: [f64; 4],
    scale: f64,
}

impl Place {
    const IDENTITY: Self = Self {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: 1.0,
    };

    /// `self` after `inner`: what `inner` places, placed again by `self`.
    fn then(self, inner: Self) -> Self {
        let moved = rotate(self.rotation, inner.translation);
        Self {
            translation: [
                self.translation[0] + self.scale * moved[0],
                self.translation[1] + self.scale * moved[1],
                self.translation[2] + self.scale * moved[2],
            ],
            rotation: product(self.rotation, inner.rotation),
            scale: self.scale * inner.scale,
        }
    }

    /// A node's own placement, refusing a matrix or an uneven scale, which
    /// instances cannot carry.
    fn of_node(node: &Value, name: &str) -> Result<Self, String> {
        if node.get("matrix").is_some() {
            return Err(format!("{name}: a node placed by a matrix"));
        }
        let numbers = |key: &str, n: usize| -> Option<Vec<f64>> {
            let values: Vec<f64> = node
                .get(key)?
                .as_array()?
                .iter()
                .filter_map(Value::as_f64)
                .collect();
            (values.len() == n).then_some(values)
        };
        let translation = numbers("translation", 3).map_or([0.0; 3], |t| [t[0], t[1], t[2]]);
        let rotation =
            numbers("rotation", 4).map_or([0.0, 0.0, 0.0, 1.0], |r| [r[0], r[1], r[2], r[3]]);
        let scale = match numbers("scale", 3) {
            None => 1.0,
            Some(s) => even(&s).ok_or(format!("{name}: a node scaled unevenly"))?,
        };
        Ok(Self {
            translation,
            rotation,
            scale,
        })
    }

    /// A tile instance, from the tile's frame (east, north, up, z up) to
    /// glTF's (x east, y up, z south).
    fn of_instance(instance: &Instance) -> Self {
        let [east, north, up] = instance.position.map(f64::from);
        let [x, y, z, w] = instance.rotation.map(f64::from);
        Self {
            translation: [east, up, -north],
            rotation: [x, z, -y, w],
            scale: f64::from(instance.scale),
        }
    }
}

/// The one scale of `s` when its three agree.
fn even(s: &[f64]) -> Option<f64> {
    let first = *s.first()?;
    s.iter()
        .all(|v| (v - first).abs() <= 1e-6 * first.abs().max(1e-12))
        .then_some(first)
}

fn product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

#[allow(clippy::many_single_char_names)]
fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = q;
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// One drawn mesh of a prototype: the mesh in the tile document, and
/// where the prototype places it (once, or once per instance of its node).
struct Drawn {
    mesh: Index,
    name: String,
    places: Vec<Place>,
}

/// Index maps from one source file into the tile document.
#[derive(Default)]
struct Offsets {
    views: Vec<Index>,
    accessors: Vec<Index>,
    samplers: Vec<Index>,
    images: Vec<Index>,
    textures: Vec<Index>,
    materials: Vec<Index>,
}

fn remap(value: &mut Value, key: &str, to: &[Index], what: &str) -> Result<(), String> {
    if let Some(old) = value.get(key) {
        let old = old
            .as_u64()
            .and_then(|old| usize::try_from(old).ok())
            .ok_or(format!("{what}'s {key} is not an index"))?;
        let new = *to
            .get(old)
            .ok_or(format!("{what} names {key} {old}, which is not there"))?;
        value[key] = json!(new);
    }
    Ok(())
}

/// Every texture reference under a material (`…Texture: {index}`).
fn remap_textures(value: &mut Value, to: &[Index]) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key.ends_with("Texture") || key.ends_with("texture") {
                    remap(inner, "index", to, "a material texture")?;
                }
                remap_textures(inner, to)?;
            }
        }
        Value::Array(items) => {
            for inner in items {
                remap_textures(inner, to)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn array<'a>(json: &'a Value, name: &str) -> &'a [Value] {
    json.get(name)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Copy a prototype file's buffers, accessors, images, textures,
/// materials and meshes into `document`, and list the meshes it draws
/// with their placements within the prototype.
#[allow(clippy::too_many_lines)]
fn copy_prototype(
    document: &mut Document,
    glb: &Glb<'_>,
    name: &str,
) -> Result<Vec<Drawn>, String> {
    let json = &glb.json;
    let fail = |what: String| format!("{name}: {what}");
    for used in array(json, "extensionsUsed") {
        if let Some(used) = used.as_str() {
            let required = array(json, "extensionsRequired")
                .iter()
                .any(|r| r.as_str() == Some(used));
            if used != "EXT_meshopt_compression" {
                document.extension(used, required);
            }
        }
    }
    if json.get("skins").is_some() {
        return Err(fail("a rigged prototype cannot be instanced".into()));
    }
    let mut at = Offsets::default();
    for index in 0..array(json, "bufferViews").len() {
        at.views
            .push(document.copy_view(glb, index).map_err(&fail)?);
    }
    for accessor in array(json, "accessors") {
        let mut accessor = accessor.clone();
        remap(&mut accessor, "bufferView", &at.views, "an accessor").map_err(&fail)?;
        if accessor.get("sparse").is_some() {
            return Err(fail("a sparse accessor".into()));
        }
        at.accessors.push(document.push("accessors", accessor));
    }
    for sampler in array(json, "samplers") {
        at.samplers.push(document.push("samplers", sampler.clone()));
    }
    for image in array(json, "images") {
        if image.get("uri").is_some() {
            return Err(fail("an image outside the file".into()));
        }
        let mut image = image.clone();
        remap(&mut image, "bufferView", &at.views, "an image").map_err(&fail)?;
        at.images.push(document.push("images", image));
    }
    for texture in array(json, "textures") {
        let mut texture = texture.clone();
        remap(&mut texture, "source", &at.images, "a texture").map_err(&fail)?;
        remap(&mut texture, "sampler", &at.samplers, "a texture").map_err(&fail)?;
        at.textures.push(document.push("textures", texture));
    }
    for material in array(json, "materials") {
        let mut material = material.clone();
        remap_textures(&mut material, &at.textures).map_err(&fail)?;
        at.materials.push(document.push("materials", material));
    }
    let mut meshes = Vec::new();
    for mesh in array(json, "meshes") {
        let mut mesh = mesh.clone();
        if let Some(primitives) = mesh.get_mut("primitives").and_then(Value::as_array_mut) {
            for primitive in primitives {
                if let Some(Value::Object(attributes)) = primitive.get_mut("attributes") {
                    for value in attributes.values_mut() {
                        let mut holder = json!({ "a": value.clone() });
                        remap(&mut holder, "a", &at.accessors, "a primitive").map_err(&fail)?;
                        *value = holder["a"].take();
                    }
                }
                if let Some(Value::Array(targets)) = primitive.get_mut("targets") {
                    for target in targets {
                        if let Value::Object(target) = target {
                            for value in target.values_mut() {
                                let mut holder = json!({ "a": value.clone() });
                                remap(&mut holder, "a", &at.accessors, "a morph target")
                                    .map_err(&fail)?;
                                *value = holder["a"].take();
                            }
                        }
                    }
                }
                remap(primitive, "indices", &at.accessors, "a primitive").map_err(&fail)?;
                remap(primitive, "material", &at.materials, "a primitive").map_err(&fail)?;
            }
        }
        meshes.push(document.push("meshes", mesh));
    }
    // Walk the scene, placing each node within the prototype.
    let nodes = array(json, "nodes");
    let scene = json
        .get("scene")
        .and_then(Value::as_u64)
        .and_then(|s| usize::try_from(s).ok())
        .unwrap_or(0);
    let roots: Vec<usize> = json
        .pointer(&format!("/scenes/{scene}/nodes"))
        .and_then(Value::as_array)
        .map(|roots| {
            roots
                .iter()
                .filter_map(|r| r.as_u64().and_then(|r| usize::try_from(r).ok()))
                .collect()
        })
        .unwrap_or_default();
    let mut drawn = Vec::new();
    let mut stack: Vec<(usize, Place, usize)> =
        roots.into_iter().map(|r| (r, Place::IDENTITY, 0)).collect();
    while let Some((index, parent, depth)) = stack.pop() {
        let node = nodes.get(index).ok_or(fail(format!("no node {index}")))?;
        if depth > 64 {
            return Err(fail("nodes nest too deep".into()));
        }
        let place = parent.then(Place::of_node(node, name)?);
        if let Some(mesh) = node.get("mesh").and_then(Value::as_u64) {
            let mesh = usize::try_from(mesh)
                .ok()
                .and_then(|m| meshes.get(m).copied())
                .ok_or(fail(format!("node {index} names no mesh")))?;
            let places = match node.pointer("/extensions/EXT_mesh_gpu_instancing/attributes") {
                None => vec![place],
                Some(attributes) => organ_places(glb, attributes)
                    .map_err(|e| fail(format!("node {index}: {e}")))?
                    .into_iter()
                    .map(|organ| place.then(organ))
                    .collect(),
            };
            let label = node.get("name").and_then(Value::as_str).unwrap_or("mesh");
            drawn.push(Drawn {
                mesh,
                name: label.to_string(),
                places,
            });
        }
        for child in node
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let child = child
                .as_u64()
                .and_then(|c| usize::try_from(c).ok())
                .ok_or(fail("a child that is not an index".into()))?;
            stack.push((child, place, depth + 1));
        }
    }
    Ok(drawn)
}

/// The placements of an instanced node's instances.
fn organ_places(glb: &Glb<'_>, attributes: &Value) -> Result<Vec<Place>, String> {
    let read = |name: &str, width: usize| -> Result<Option<Vec<f64>>, String> {
        let Some(at) = attributes.get(name) else {
            return Ok(None);
        };
        let at = at
            .as_u64()
            .and_then(|a| usize::try_from(a).ok())
            .ok_or(format!("{name} is not an index"))?;
        let (values, found) = glb.accessor(at)?;
        if found != width {
            return Err(format!("{name} is not {width} wide"));
        }
        Ok(Some(values))
    };
    let translations = read("TRANSLATION", 3)?;
    let rotations = read("ROTATION", 4)?;
    let scales = read("SCALE", 3)?;
    let count = translations
        .as_ref()
        .map(|t| t.len() / 3)
        .or(rotations.as_ref().map(|r| r.len() / 4))
        .or(scales.as_ref().map(|s| s.len() / 3))
        .ok_or("instancing with no attribute")?;
    (0..count)
        .map(|i| {
            let translation = translations
                .as_ref()
                .map_or([0.0; 3], |t| [t[3 * i], t[3 * i + 1], t[3 * i + 2]]);
            let rotation = rotations.as_ref().map_or([0.0, 0.0, 0.0, 1.0], |r| {
                [r[4 * i], r[4 * i + 1], r[4 * i + 2], r[4 * i + 3]]
            });
            let scale = match &scales {
                None => 1.0,
                Some(s) => even(&s[3 * i..3 * i + 3]).ok_or("an instance scaled unevenly")?,
            };
            Ok(Place {
                translation,
                rotation,
                scale,
            })
        })
        .collect()
}

/// The tile as one GLB: see the module's documentation.
pub(crate) fn tile_glb(
    plan: &TilePlan,
    prototypes: &[PrototypeFiles],
    dir: &Path,
    level: usize,
    meshopt: bool,
) -> Result<Vec<u8>, Error> {
    let all: Vec<&Instance> = plan.instances.iter().collect();
    let (glb, _) = scene_glb(
        plan,
        &all,
        [0.0, 0.0],
        &format!("tile {}_{}", plan.tile.x, plan.tile.y),
        prototypes,
        dir,
        level,
        meshopt,
        json!({}),
    )?;
    Ok(glb)
}

/// What [`scene_glb`] drew: each prototype's index and its plants.
type Placed = Vec<(u32, usize)>;

/// `instances` of `plan` (tile metres) as one GLB at `level`, its root at
/// `origin` (east, north) in the tile, named `name`; `meta` joins the
/// scene's `extras.plantgen`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn scene_glb(
    plan: &TilePlan,
    instances: &[&Instance],
    origin: [f64; 2],
    name: &str,
    prototypes: &[PrototypeFiles],
    dir: &Path,
    level: usize,
    meshopt: bool,
    meta: Value,
) -> Result<(Vec<u8>, Placed), Error> {
    let mut document = Document::new();
    document.extension("EXT_mesh_gpu_instancing", true);
    if meshopt {
        document.meshopt();
    }
    // Each prototype's instances, in tile order, from the root.
    let mut by_prototype: BTreeMap<u32, Vec<Place>> = BTreeMap::new();
    for instance in instances {
        #[allow(clippy::cast_possible_truncation)]
        let local = Instance {
            position: [
                (f64::from(instance.position[0]) - origin[0]) as f32,
                (f64::from(instance.position[1]) - origin[1]) as f32,
                instance.position[2],
            ],
            ..**instance
        };
        by_prototype
            .entry(instance.prototype)
            .or_default()
            .push(Place::of_instance(&local));
    }
    let mut children = Vec::new();
    let mut listed = Vec::new();
    let mut drawn_list = Vec::new();
    for (prototype, placed) in &by_prototype {
        let record = prototypes
            .iter()
            .find(|p| p.index == *prototype)
            .ok_or_else(|| Error(format!("tile glb: prototype {prototype} has no files")))?;
        let file = record
            .plant
            .levels
            .iter()
            .find(|l| l.level == level)
            .ok_or_else(|| {
                Error(format!(
                    "tile glb: prototype {prototype} has no level {level}"
                ))
            })?;
        let path = dir.join(&file.file);
        let bytes = std::fs::read(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let glb = Glb::read(&bytes).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        let drawn = copy_prototype(&mut document, &glb, &file.file).map_err(Error)?;
        for mesh in drawn {
            let count = placed.len() * mesh.places.len();
            let (mut translations, mut rotations, mut scales) = (
                Vec::with_capacity(3 * count),
                Vec::with_capacity(4 * count),
                Vec::with_capacity(3 * count),
            );
            for tile in placed {
                for inner in &mesh.places {
                    let place = tile.then(*inner);
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        translations.extend(place.translation.map(|v| v as f32));
                        let r = place.rotation;
                        let length = r.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
                        rotations.extend(r.map(|v| (v / length) as f32));
                        scales.extend([place.scale as f32; 3]);
                    }
                }
            }
            let attributes = json!({
                "TRANSLATION": document.instance_attribute(&translations, 3),
                "ROTATION": document.instance_attribute(&rotations, 4),
                "SCALE": document.instance_attribute(&scales, 3),
            });
            children.push(document.node(json!({
                "name": format!("{} {}", record.plant.stem, mesh.name),
                "mesh": mesh.mesh,
                "extensions": { "EXT_mesh_gpu_instancing": { "attributes": attributes } },
            })));
        }
        listed.push(json!({
            "index": prototype,
            "stem": record.plant.stem,
            "file": file.file,
            "instances": placed.len(),
        }));
        drawn_list.push((*prototype, placed.len()));
    }
    #[allow(clippy::cast_precision_loss)]
    let corner = [
        f64::from(plan.tile.x) * plan.tile_m + origin[0],
        f64::from(plan.tile.y) * plan.tile_m + origin[1],
    ];
    let root = document.node(json!({
        "name": name,
        "translation": [corner[0], 0.0, -corner[1]],
        "children": children,
    }));
    let mut about = json!({
        "format": "plantgen-tile",
        "generator_revision": crate::GENERATOR_REVISION,
        "tile": plan.tile,
        "tile_m": plan.tile_m,
        "level": level,
        "instances": instances.len(),
        "prototypes": listed,
        "units": "metres; +X east, +Y up, -Z north; the root node sits at the south-west corner of what it holds",
    });
    if let (Some(about), Value::Object(meta)) = (about.as_object_mut(), meta) {
        about.extend(meta);
    }
    let mut extras = Map::new();
    extras.insert("plantgen".into(), about);
    Ok((
        document.glb(
            json!({ "version": "2.0", "generator": format!("plantgen {}", crate::GENERATOR_REVISION) }),
            &[root],
            Value::Object(extras),
        ),
        drawn_list,
    ))
}

/// The reference camera of a cell index's `from_m`: 1080 pixels high and
/// 60 degrees across vertically.
const REFERENCE_PIXELS: f64 = 1080.0;

/// How wide a pixel may be, metres, where it falls, before `level` of a
/// plant `height` tall and `radius` in crown, drawn in `triangles`, is
/// enough: its triangles then cover [`TRIANGLE_PIXELS`] each on average
/// over its height times its breadth. None for a level with none.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn level_pixel(height: f64, radius: f64, triangles: usize) -> Option<f64> {
    (triangles > 0).then(|| {
        let area = height.max(0.01) * (2.0 * radius).max(0.01);
        (area / (TRIANGLE_PIXELS * triangles as f64)).sqrt()
    })
}

/// The tile cut into square cells `cell_m` across, each written at every
/// one of `levels`, and an index: see [`crate::fill::TilePlan::glb_cells`].
pub(crate) fn tile_cells(
    plan: &TilePlan,
    prototypes: &[PrototypeFiles],
    dir: &Path,
    cell_m: f64,
    levels: &[usize],
    meshopt: bool,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
    if !(cell_m.is_finite() && cell_m > 0.0) {
        return Err(Error(format!("tile cells: a cell {cell_m} m across")));
    }
    if levels.is_empty() {
        return Err(Error("tile cells: no levels".into()));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cols = ((plan.tile_m / cell_m).ceil() as usize).max(1);
    let mut cells: Vec<Vec<&Instance>> = vec![Vec::new(); cols * cols];
    for instance in &plan.instances {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at = |v: f32| ((f64::from(v) / cell_m).floor().max(0.0) as usize).min(cols - 1);
        cells[at(instance.position[1]) * cols + at(instance.position[0])].push(instance);
    }
    let stem = format!("{}_{}", plan.tile.x, plan.tile.y);
    let s = 2.0 * (std::f64::consts::PI / 6.0).tan() / REFERENCE_PIXELS;
    let mut files = Vec::new();
    let mut listed = Vec::new();
    for (i, instances) in cells.iter().enumerate() {
        if instances.is_empty() {
            continue;
        }
        let (col, row) = (i % cols, i / cols);
        #[allow(clippy::cast_precision_loss)]
        let origin = [col as f64 * cell_m, row as f64 * cell_m];
        let mut by_level = Vec::new();
        let mut placed = Vec::new();
        for &level in levels {
            let name = format!("{stem}/c{col}_{row}-lod{level}.glb");
            let (glb, drawn) = scene_glb(
                plan,
                instances,
                origin,
                &format!("tile {stem} cell {col},{row} level {level}"),
                prototypes,
                dir,
                level,
                meshopt,
                json!({ "cell": [col, row], "cell_m": cell_m }),
            )?;
            // The narrowest pixel past which this level is enough for
            // every plant of the cell: the most demanding plant's.
            let mut pixel = f64::NEG_INFINITY;
            for (prototype, _) in &drawn {
                let Some(record) = prototypes.iter().find(|p| p.index == *prototype) else {
                    continue;
                };
                let size = &record.plant.size;
                let triangles = record
                    .plant
                    .levels
                    .iter()
                    .find(|l| l.level == level)
                    .map_or(0, |l| l.triangles);
                if let Some(p) = level_pixel(size.height, size.crown_radius, triangles) {
                    pixel = pixel.max(p);
                }
            }
            if placed.is_empty() {
                placed = drawn;
            }
            by_level.push(json!({
                "level": level,
                "file": name,
                "bytes": glb.len(),
                "pixel_m": pixel.is_finite().then_some(pixel),
                "from_m": pixel.is_finite().then_some(pixel / s),
            }));
            files.push((name, glb));
        }
        let tallest = placed
            .iter()
            .filter_map(|(p, _)| prototypes.iter().find(|r| r.index == *p))
            .map(|r| r.plant.size.height)
            .fold(0.0, f64::max);
        listed.push(json!({
            "cell": [col, row],
            "corner": origin,
            "tallest_m": tallest,
            "instances": instances.len(),
            "levels": by_level,
        }));
    }
    let index = json!({
        "format": "plantgen-tile-cells",
        "schema": 1,
        "generator_revision": crate::GENERATOR_REVISION,
        "tile": plan.tile,
        "tile_m": plan.tile_m,
        "cell_m": cell_m,
        "cols": cols,
        "triangle_pixels": TRIANGLE_PIXELS,
        "reference_camera": { "pixels_high": REFERENCE_PIXELS, "fov_y_degrees": 60.0 },
        "units": "metres; corner (east, north) from the tile's south-west corner; each file's root node sits at its cell's south-west corner, +X east, +Y up, -Z north",
        "choose": "draw a cell at the coarsest level whose pixel_m is at most the width a pixel spans at the cell's nearest point: distance x 2 tan(fov_y / 2) / pixels high; from_m is that distance for the reference camera",
        "cells": listed,
    });
    let text = serde_json::to_vec_pretty(&index).map_err(|e| Error(e.to_string()))?;
    files.push((format!("{stem}/cells.json"), text));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_compose_as_gltf_nests_nodes() {
        let quarter = std::f64::consts::FRAC_PI_4;
        let outer = Place {
            translation: [10.0, 0.0, 0.0],
            rotation: [0.0, quarter.sin(), 0.0, quarter.cos()], // 90 degrees about +y
            scale: 2.0,
        };
        let inner = Place {
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: 0.5,
        };
        let both = outer.then(inner);
        // +x turned 90 degrees about +y is -z, doubled.
        assert!((both.translation[0] - 10.0).abs() < 1e-9);
        assert!((both.translation[2] + 2.0).abs() < 1e-9);
        assert!((both.scale - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_tile_instance_turns_into_the_gltf_frame() {
        let instance = Instance {
            prototype: 0,
            position: [3.0, 4.0, 5.0],
            rotation: Instance::lean(0.0, [0.1, 0.0]),
            scale: 1.0,
            age_years: 1.0,
            tint: [0.0, 0.0],
            extra: 0,
        };
        let place = Place::of_instance(&instance);
        for (a, b) in place.translation.iter().zip([3.0, 5.0, -4.0]) {
            assert!((a - b).abs() < 1e-9);
        }
        // The plant's up (glTF +y) leans 0.1 toward east (+x).
        let up = rotate(place.rotation, [0.0, 1.0, 0.0]);
        assert!((up[0] / up[1] - 0.1).abs() < 1e-5, "{up:?}");
        assert!(up[2].abs() < 1e-6);
    }
}
