//! glTF 2.0 binary files (`.glb`): a small writer for the models `PlantGen`
//! exports (`crate::export`), and a reader that checks one.
//!
//! A GLB file is a 12-byte header (`glTF`, version 2, total length), a JSON
//! chunk describing the scene and a binary chunk holding every array and
//! image it points to (glTF 2.0, section 4.4). The writer lays each array
//! out as its own buffer view, four-byte aligned, and gives every position
//! accessor its bounds, as the specification asks. It needs nothing beyond
//! `serde_json`, so the generator keeps its dependencies.
//!
//! Numbers in the JSON are single precision where the data is, written in
//! their shortest form, so the same model always gives the same bytes.

use serde_json::{Map, Value, json};

/// `glTF`, little-endian.
const MAGIC: u32 = 0x4654_6C67;
/// `JSON` and `BIN\0`, little-endian.
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;

/// Component types (glTF 2.0, 3.6.2.2).
pub const FLOAT: u32 = 5126;
pub const UNSIGNED_SHORT: u32 = 5123;
pub const UNSIGNED_INT: u32 = 5125;

/// Buffer view targets.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Sampler filters and wrapping.
pub const LINEAR: u32 = 9729;
pub const LINEAR_MIPMAP_LINEAR: u32 = 9987;
pub const CLAMP_TO_EDGE: u32 = 33071;

/// A single-precision number as JSON, in its shortest form.
#[must_use]
pub fn number(value: f32) -> Value {
    if !value.is_finite() {
        return Value::Null;
    }
    // The shortest decimal that reads back as `value`, held as the double
    // nearest to it, which `serde_json` writes in that same short form.
    let shortest: f64 = format!("{value}").parse().unwrap_or(f64::from(value));
    json!(shortest)
}

/// Numbers as a JSON array (see [`number`]).
#[must_use]
pub fn numbers(values: &[f32]) -> Value {
    Value::Array(values.iter().map(|&value| number(value)).collect())
}

/// A glTF document being written: its JSON arrays and its binary buffer.
#[derive(Debug, Clone, Default)]
pub struct Document {
    buffer: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    images: Vec<Value>,
    samplers: Vec<Value>,
    textures: Vec<Value>,
    materials: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
}

/// An index into one of a document's arrays.
pub type Index = usize;

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `bytes` as a buffer view, four-byte aligned.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> Index {
        while !self.buffer.len().is_multiple_of(4) {
            self.buffer.push(0);
        }
        let mut view = Map::new();
        view.insert("buffer".into(), json!(0));
        view.insert("byteOffset".into(), json!(self.buffer.len()));
        view.insert("byteLength".into(), json!(bytes.len()));
        if let Some(target) = target {
            view.insert("target".into(), json!(target));
        }
        self.buffer.extend_from_slice(bytes);
        self.buffer_views.push(Value::Object(view));
        self.buffer_views.len() - 1
    }

    /// A vertex attribute of `width` floats per vertex (1 for `SCALAR`, 2
    /// to 4 for `VEC2` to `VEC4`); with `bounds`, its per-component minimum
    /// and maximum, which `POSITION` must have.
    ///
    /// # Panics
    ///
    /// When `width` is not 1 to 4 or `values` is not a whole number of
    /// vertices.
    pub fn attribute(&mut self, values: &[f32], width: usize, bounds: bool) -> Index {
        assert!(
            (1..=4).contains(&width) && values.len().is_multiple_of(width),
            "an attribute is 1 to 4 floats per vertex"
        );
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let mut accessor = Map::new();
        accessor.insert("bufferView".into(), json!(view));
        accessor.insert("componentType".into(), json!(FLOAT));
        accessor.insert("count".into(), json!(values.len() / width));
        accessor.insert(
            "type".into(),
            json!(["SCALAR", "VEC2", "VEC3", "VEC4"][width - 1]),
        );
        if bounds && !values.is_empty() {
            let mut low = vec![f32::INFINITY; width];
            let mut high = vec![f32::NEG_INFINITY; width];
            for vertex in values.chunks_exact(width) {
                for (axis, &value) in vertex.iter().enumerate() {
                    low[axis] = low[axis].min(value);
                    high[axis] = high[axis].max(value);
                }
            }
            accessor.insert("min".into(), numbers(&low));
            accessor.insert("max".into(), numbers(&high));
        }
        self.accessors.push(Value::Object(accessor));
        self.accessors.len() - 1
    }

    /// Triangle indices, as unsigned shorts when every one fits.
    pub fn indices(&mut self, indices: &[u32]) -> Index {
        let short = indices.iter().all(|&index| index < u32::from(u16::MAX));
        let bytes: Vec<u8> = if short {
            indices
                .iter()
                .flat_map(|&index| u16::try_from(index).unwrap_or(0).to_le_bytes())
                .collect()
        } else {
            indices
                .iter()
                .flat_map(|index| index.to_le_bytes())
                .collect()
        };
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": if short { UNSIGNED_SHORT } else { UNSIGNED_INT },
            "count": indices.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }

    /// A PNG image held in the binary chunk.
    pub fn png(&mut self, name: &str, png: &[u8]) -> Index {
        let view = self.view(png, None);
        self.images.push(json!({
            "name": name,
            "bufferView": view,
            "mimeType": "image/png",
        }));
        self.images.len() - 1
    }

    pub fn sampler(&mut self, sampler: Value) -> Index {
        self.samplers.push(sampler);
        self.samplers.len() - 1
    }

    pub fn texture(&mut self, image: Index, sampler: Index) -> Index {
        self.textures
            .push(json!({ "source": image, "sampler": sampler }));
        self.textures.len() - 1
    }

    pub fn material(&mut self, material: Value) -> Index {
        self.materials.push(material);
        self.materials.len() - 1
    }

    /// A mesh named `name` of triangle `primitives`, each `{"attributes":
    /// …, "indices": …, "material": …}`.
    pub fn mesh(&mut self, name: &str, primitives: Vec<Value>) -> Index {
        let mut mesh = Map::new();
        mesh.insert("name".into(), json!(name));
        mesh.insert("primitives".into(), Value::Array(primitives));
        self.meshes.push(Value::Object(mesh));
        self.meshes.len() - 1
    }

    pub fn node(&mut self, node: Value) -> Index {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// The finished file: one scene of `roots`, with `asset` describing it
    /// (glTF 2.0, 5.2) and `extras` on the scene.
    #[must_use]
    pub fn glb(self, asset: Value, roots: &[Index], extras: Value) -> Vec<u8> {
        let mut root = Map::new();
        root.insert("asset".into(), asset);
        root.insert("scene".into(), json!(0));
        let mut scene = Map::new();
        scene.insert("nodes".into(), json!(roots));
        scene.insert("extras".into(), extras);
        root.insert("scenes".into(), json!([scene]));
        let arrays = [
            ("nodes", self.nodes),
            ("meshes", self.meshes),
            ("materials", self.materials),
            ("textures", self.textures),
            ("samplers", self.samplers),
            ("images", self.images),
            ("accessors", self.accessors),
            ("bufferViews", self.buffer_views),
        ];
        for (name, array) in arrays {
            if !array.is_empty() {
                root.insert(name.into(), Value::Array(array));
            }
        }
        let mut binary = self.buffer;
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        if !binary.is_empty() {
            root.insert("buffers".into(), json!([{ "byteLength": binary.len() }]));
        }
        let mut text = serde_json::to_vec(&Value::Object(root)).unwrap_or_default();
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let chunk = |kind: u32, data: &[u8], out: &mut Vec<u8>| {
            out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(data);
        };
        let total = 12
            + 8
            + text.len()
            + if binary.is_empty() {
                0
            } else {
                8 + binary.len()
            };
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(total).unwrap_or(u32::MAX).to_le_bytes());
        chunk(JSON_CHUNK, &text, &mut out);
        if !binary.is_empty() {
            chunk(BIN_CHUNK, &binary, &mut out);
        }
        out
    }
}

/// What a checked file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub nodes: usize,
    pub meshes: usize,
    pub materials: usize,
    pub images: usize,
    /// Triangles over every primitive of every node that draws a mesh, as a
    /// renderer draws them.
    pub triangles: usize,
}

/// A GLB file split into its two chunks.
#[derive(Debug, Clone)]
pub struct Glb<'a> {
    /// The JSON chunk: the document.
    pub json: Value,
    /// The binary chunk, empty when the file has none.
    pub binary: &'a [u8],
}

impl<'a> Glb<'a> {
    /// Split a GLB file into its chunks, checking the header.
    ///
    /// # Errors
    ///
    /// When the header, a chunk or the JSON is not as glTF 2.0 writes them.
    pub fn read(glb: &'a [u8]) -> Result<Self, String> {
        let word = |at: usize| -> Result<u32, String> {
            glb.get(at..at + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| "the file ends early".to_string())
        };
        if word(0)? != MAGIC || word(4)? != 2 {
            return Err("not a glTF 2.0 binary file".into());
        }
        if word(8)? as usize != glb.len() {
            return Err("the header's length is not the file's".into());
        }
        let json_length = word(12)? as usize;
        if word(16)? != JSON_CHUNK || !json_length.is_multiple_of(4) {
            return Err("the first chunk is not JSON, four-byte aligned".into());
        }
        let text = glb
            .get(20..20 + json_length)
            .ok_or("the JSON chunk ends early")?;
        let json: Value = serde_json::from_slice(text).map_err(|error| error.to_string())?;
        let binary: &[u8] = if glb.len() > 20 + json_length {
            let at = 20 + json_length;
            let length = word(at)? as usize;
            if word(at + 4)? != BIN_CHUNK || !length.is_multiple_of(4) {
                return Err("the second chunk is not BIN, four-byte aligned".into());
            }
            glb.get(at + 8..at + 8 + length)
                .ok_or("the binary chunk ends early")?
        } else {
            &[]
        };
        Ok(Self { json, binary })
    }

    /// Accessor `index`'s components as doubles, and how many make one
    /// element (1 for `SCALAR` to 4 for `VEC4`).
    ///
    /// # Errors
    ///
    /// When there is no such accessor, or it is not inside its buffer view
    /// and the view inside the binary chunk.
    pub fn accessor(&self, index: usize) -> Result<(Vec<f64>, usize), String> {
        let accessor = self
            .json
            .pointer(&format!("/accessors/{index}"))
            .ok_or(format!("there is no accessor {index}"))?;
        let view = self
            .json
            .pointer(&format!(
                "/bufferViews/{}",
                field(accessor, "bufferView").ok_or("an accessor has no buffer view")?
            ))
            .ok_or(format!("accessor {index} names no buffer view"))?;
        let width = match accessor.get("type").and_then(Value::as_str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err(format!("accessor {index} has an unknown type")),
        };
        let kind = field(accessor, "componentType").unwrap_or(0);
        let size = match u32::try_from(kind) {
            Ok(FLOAT | UNSIGNED_INT) => 4,
            Ok(UNSIGNED_SHORT) => 2,
            _ => return Err(format!("accessor {index} has an unknown component type")),
        };
        let count = field(accessor, "count").ok_or("an accessor has no count")?;
        let view_start = field(view, "byteOffset").unwrap_or(0);
        let view_end = view_start + field(view, "byteLength").unwrap_or(0);
        let start = view_start + field(accessor, "byteOffset").unwrap_or(0);
        let end = start + count * width * size;
        if end > view_end {
            return Err(format!("accessor {index} runs past its buffer view"));
        }
        let bytes = self
            .binary
            .get(start..end)
            .ok_or(format!("accessor {index} is outside the buffer"))?;
        let values = bytes
            .chunks_exact(size)
            .map(|bytes| match (kind, bytes) {
                (5126, &[a, b, c, d]) => f64::from(f32::from_le_bytes([a, b, c, d])),
                (_, &[a, b, c, d]) => f64::from(u32::from_le_bytes([a, b, c, d])),
                (_, &[a, b]) => f64::from(u16::from_le_bytes([a, b])),
                _ => f64::NAN,
            })
            .collect();
        Ok((values, width))
    }
}

/// A non-negative integer field of a JSON object.
fn field(value: &Value, name: &str) -> Option<usize> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

/// Read a GLB file and check what `PlantGen`'s writer promises: the header
/// and chunks, every buffer view inside the buffer, every accessor inside
/// its view, every float finite, position bounds that match the data,
/// indices inside their primitive's vertices and unit normals. The
/// Khronos glTF Validator checks the rest of the specification.
///
/// # Errors
///
/// The first problem found.
pub fn check(glb: &[u8]) -> Result<Summary, String> {
    let glb = Glb::read(glb)?;
    let root = &glb.json;
    let array = |name: &str| -> Vec<Value> {
        root.get(name)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    if root.pointer("/asset/version").and_then(Value::as_str) != Some("2.0") {
        return Err("asset.version is not 2.0".into());
    }
    for (index, view) in array("bufferViews").iter().enumerate() {
        let offset = field(view, "byteOffset").unwrap_or(0);
        let length = field(view, "byteLength").ok_or("a buffer view has no length")?;
        if offset + length > glb.binary.len() || !offset.is_multiple_of(4) {
            return Err(format!("buffer view {index} is outside the buffer"));
        }
    }
    // Each accessor's components and width.
    let accessors = array("accessors");
    let mut data: Vec<(Vec<f64>, usize)> = Vec::with_capacity(accessors.len());
    for (index, accessor) in accessors.iter().enumerate() {
        let (values, width) = glb.accessor(index)?;
        if values.iter().any(|value| !value.is_finite()) {
            return Err(format!("accessor {index} holds a value that is not finite"));
        }
        for (bound, keep) in [("min", f64::min as fn(f64, f64) -> f64), ("max", f64::max)] {
            let Some(declared) = accessor.get(bound).and_then(Value::as_array) else {
                continue;
            };
            for (axis, declared) in declared.iter().enumerate() {
                let actual = values
                    .iter()
                    .skip(axis)
                    .step_by(width)
                    .copied()
                    .reduce(keep)
                    .unwrap_or(0.0);
                let declared = declared.as_f64().unwrap_or(f64::NAN);
                // Declared bounds are single precision, as the data.
                #[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
                if f64::from(declared as f32) != actual {
                    return Err(format!(
                        "accessor {index}'s {bound} does not match its data"
                    ));
                }
            }
        }
        data.push((values, width));
    }
    let mut summary = Summary {
        nodes: array("nodes").len(),
        meshes: array("meshes").len(),
        materials: array("materials").len(),
        images: array("images").len(),
        triangles: 0,
    };
    let meshes = array("meshes");
    let mut mesh_triangles = vec![0; meshes.len()];
    for (index, mesh) in meshes.iter().enumerate() {
        for primitive in mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or(format!("mesh {index} has no primitives"))?
        {
            let attribute = |name: &str| {
                primitive
                    .pointer(&format!("/attributes/{name}"))
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .and_then(|at| data.get(at))
            };
            let (positions, 3) =
                attribute("POSITION").ok_or(format!("mesh {index} has no positions"))?
            else {
                return Err(format!("mesh {index}'s positions are not VEC3"));
            };
            if accessors
                .get(
                    field(
                        primitive.get("attributes").unwrap_or(&Value::Null),
                        "POSITION",
                    )
                    .unwrap_or(0),
                )
                .and_then(|accessor| accessor.get("min"))
                .is_none()
            {
                return Err(format!("mesh {index}'s positions have no bounds"));
            }
            let vertices = positions.len() / 3;
            for name in primitive
                .get("attributes")
                .and_then(Value::as_object)
                .map(|attributes| attributes.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
            {
                let (values, width) =
                    attribute(&name).ok_or(format!("mesh {index}'s {name} is missing"))?;
                if values.len() / width != vertices {
                    return Err(format!("mesh {index}'s {name} has the wrong count"));
                }
                if name == "NORMAL" {
                    for normal in values.as_chunks::<3>().0 {
                        let length =
                            (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2])
                                .sqrt();
                        if (length - 1.0).abs() > 5.0e-4 {
                            return Err(format!("mesh {index} has a normal {length} long"));
                        }
                    }
                }
            }
            let (indices, _) = field(primitive, "indices")
                .and_then(|at| data.get(at))
                .ok_or(format!("mesh {index} has no indices"))?;
            if indices.len() % 3 != 0 {
                return Err(format!("mesh {index}'s index count is not a multiple of 3"));
            }
            #[allow(clippy::cast_precision_loss)]
            if indices.iter().any(|&at| at >= vertices as f64) {
                return Err(format!("mesh {index} has an index past its vertices"));
            }
            mesh_triangles[index] += indices.len() / 3;
        }
    }
    for node in array("nodes") {
        if let Some(mesh) = field(&node, "mesh") {
            summary.triangles += mesh_triangles.get(mesh).ok_or("a node names no mesh")?;
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_is_written_and_read_back() {
        let mut document = Document::new();
        let positions = document.attribute(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], 3, true);
        let normals = document.attribute(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], 3, false);
        let indices = document.indices(&[0, 1, 2]);
        let mesh = document.mesh(
            "triangle",
            vec![json!({
                "attributes": { "POSITION": positions, "NORMAL": normals },
                "indices": indices,
            })],
        );
        let node = document.node(json!({ "mesh": mesh }));
        let glb = document.glb(json!({ "version": "2.0" }), &[node], json!({}));
        assert!(glb.len().is_multiple_of(4));
        let summary = check(&glb).expect("a valid file");
        assert_eq!(summary.triangles, 1);
        assert_eq!(summary.nodes, 1);
    }

    #[test]
    fn numbers_keep_their_shortest_form() {
        assert_eq!(number(0.1).to_string(), "0.1");
        assert_eq!(number(-2.5).to_string(), "-2.5");
        assert_eq!(number(f32::INFINITY), Value::Null);
    }

    #[test]
    fn a_broken_file_is_refused() {
        let mut document = Document::new();
        let positions = document.attribute(&[0.0; 9], 3, true);
        let indices = document.indices(&[0, 1, 3]);
        let mesh = document.mesh(
            "bad",
            vec![json!({ "attributes": { "POSITION": positions }, "indices": indices })],
        );
        let node = document.node(json!({ "mesh": mesh }));
        let glb = document.glb(json!({ "version": "2.0" }), &[node], json!({}));
        assert!(check(&glb).unwrap_err().contains("index past"));
        assert!(check(&glb[..glb.len() - 4]).is_err());
    }
}
