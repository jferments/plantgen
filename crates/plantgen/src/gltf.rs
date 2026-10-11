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
pub const BYTE: u32 = 5120;
pub const UNSIGNED_BYTE: u32 = 5121;
pub const SHORT: u32 = 5122;
pub const UNSIGNED_SHORT: u32 = 5123;
pub const UNSIGNED_INT: u32 = 5125;
pub const FLOAT: u32 = 5126;

/// Bytes in one component of `kind`, if it is a component type.
#[must_use]
pub fn component_size(kind: u32) -> Option<usize> {
    match kind {
        BYTE | UNSIGNED_BYTE => Some(1),
        SHORT | UNSIGNED_SHORT => Some(2),
        UNSIGNED_INT | FLOAT => Some(4),
        _ => None,
    }
}

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
    extensions_used: Vec<String>,
    extensions_required: Vec<String>,
    /// How each buffer view may be compressed, and whether it is.
    codecs: Vec<Codec>,
    meshopt: bool,
}

/// What a buffer view holds, which says how `EXT_meshopt_compression`
/// may compress it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    /// Bytes stored as they are, such as images.
    None,
    /// Elements of this many bytes: vertex or instance arrays.
    Attributes(usize),
    /// Triangle indices of this many bytes.
    Triangles(usize),
}

/// An index into one of a document's arrays.
pub type Index = usize;

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `bytes`, which hold `codec`'s kind of data, as a buffer view,
    /// four-byte aligned.
    fn view(&mut self, bytes: &[u8], target: Option<u32>, codec: Codec) -> Index {
        self.strided_view(bytes, target, None, codec)
    }

    /// [`Self::view`] whose elements are `stride` bytes apart.
    fn strided_view(
        &mut self,
        bytes: &[u8],
        target: Option<u32>,
        stride: Option<usize>,
        codec: Codec,
    ) -> Index {
        while !self.buffer.len().is_multiple_of(4) {
            self.buffer.push(0);
        }
        let mut view = Map::new();
        view.insert("buffer".into(), json!(0));
        view.insert("byteOffset".into(), json!(self.buffer.len()));
        view.insert("byteLength".into(), json!(bytes.len()));
        if let Some(stride) = stride {
            view.insert("byteStride".into(), json!(stride));
        }
        if let Some(target) = target {
            view.insert("target".into(), json!(target));
        }
        self.buffer.extend_from_slice(bytes);
        self.buffer_views.push(Value::Object(view));
        self.codecs.push(codec);
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
        let view = self.view(&bytes, Some(ARRAY_BUFFER), Codec::Attributes(4 * width));
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

    /// A vertex attribute of `width` integers per vertex, each stored as
    /// `component` (one of [`BYTE`] to [`UNSIGNED_SHORT`]), `normalized`
    /// or not (glTF 2.0, 3.11; `KHR_mesh_quantization`), with each vertex
    /// padded to four bytes as glTF requires; with `bounds`, its minimum
    /// and maximum.
    ///
    /// # Panics
    ///
    /// When `width` is not 1 to 4, `values` is not a whole number of
    /// vertices or `component` is not a byte or short type.
    pub fn integer_attribute(
        &mut self,
        values: &[i32],
        width: usize,
        component: u32,
        normalized: bool,
        bounds: bool,
    ) -> Index {
        assert!(
            (1..=4).contains(&width) && values.len().is_multiple_of(width),
            "an attribute is 1 to 4 components per vertex"
        );
        let size = match component {
            BYTE | UNSIGNED_BYTE => 1,
            SHORT | UNSIGNED_SHORT => 2,
            _ => panic!("an integer attribute is bytes or shorts"),
        };
        let stride = (width * size).div_ceil(4) * 4;
        let mut bytes = Vec::with_capacity(values.len() / width * stride);
        for vertex in values.chunks_exact(width) {
            for &value in vertex {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                match size {
                    1 => bytes.push(value as u8),
                    _ => bytes.extend_from_slice(&(value as u16).to_le_bytes()),
                }
            }
            bytes.resize(bytes.len() + stride - width * size, 0);
        }
        let view = self.strided_view(
            &bytes,
            Some(ARRAY_BUFFER),
            (stride != width * size).then_some(stride),
            Codec::Attributes(stride),
        );
        let mut accessor = Map::new();
        accessor.insert("bufferView".into(), json!(view));
        accessor.insert("componentType".into(), json!(component));
        if normalized {
            accessor.insert("normalized".into(), json!(true));
        }
        accessor.insert("count".into(), json!(values.len() / width));
        accessor.insert(
            "type".into(),
            json!(["SCALAR", "VEC2", "VEC3", "VEC4"][width - 1]),
        );
        if bounds && !values.is_empty() {
            let mut low = vec![i32::MAX; width];
            let mut high = vec![i32::MIN; width];
            for vertex in values.chunks_exact(width) {
                for (axis, &value) in vertex.iter().enumerate() {
                    low[axis] = low[axis].min(value);
                    high[axis] = high[axis].max(value);
                }
            }
            accessor.insert("min".into(), json!(low));
            accessor.insert("max".into(), json!(high));
        }
        self.accessors.push(Value::Object(accessor));
        self.accessors.len() - 1
    }

    /// Per-instance floats for `EXT_mesh_gpu_instancing`: `width` per
    /// instance, in a buffer view with no vertex target.
    ///
    /// # Panics
    ///
    /// When `width` is not 1 to 4 or `values` is not a whole number of
    /// instances.
    pub fn instance_attribute(&mut self, values: &[f32], width: usize) -> Index {
        assert!(
            (1..=4).contains(&width) && values.len().is_multiple_of(width),
            "an instance attribute is 1 to 4 floats per instance"
        );
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let view = self.view(&bytes, None, Codec::Attributes(4 * width));
        let kind = ["SCALAR", "VEC2", "VEC3", "VEC4"][width - 1];
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": FLOAT,
            "count": values.len() / width,
            "type": kind,
        }));
        self.accessors.len() - 1
    }

    /// Declare an extension the document uses; `required` when a viewer
    /// without it cannot draw the document right.
    pub fn extension(&mut self, name: &str, required: bool) {
        if !self.extensions_used.iter().any(|used| used == name) {
            self.extensions_used.push(name.to_string());
        }
        if required && !self.extensions_required.iter().any(|used| used == name) {
            self.extensions_required.push(name.to_string());
        }
    }

    /// Compress the vertex, index and instance arrays with
    /// `EXT_meshopt_compression` ([`crate::meshopt`]), which the file then
    /// requires: their buffer views hold no bytes of their own, in a
    /// fallback buffer with none, and each names its compressed stream in
    /// the binary chunk. Images stay as they are.
    pub fn meshopt(&mut self) {
        self.meshopt = true;
        self.extension("EXT_meshopt_compression", true);
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
        let size = if short { 2 } else { 4 };
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER), Codec::Triangles(size));
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
        let view = self.view(png, None, Codec::None);
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
        if !self.extensions_used.is_empty() {
            root.insert("extensionsUsed".into(), json!(self.extensions_used));
        }
        if !self.extensions_required.is_empty() {
            root.insert("extensionsRequired".into(), json!(self.extensions_required));
        }
        let arrays = [
            ("nodes", self.nodes),
            ("meshes", self.meshes),
            ("materials", self.materials),
            ("textures", self.textures),
            ("samplers", self.samplers),
            ("images", self.images),
            ("accessors", self.accessors),
        ];
        for (name, array) in arrays {
            if !array.is_empty() {
                root.insert(name.into(), Value::Array(array));
            }
        }
        let (views, mut binary, fallback) = if self.meshopt {
            compress(self.buffer_views, &self.codecs, &self.buffer)
        } else {
            (self.buffer_views, self.buffer, 0)
        };
        if !views.is_empty() {
            root.insert("bufferViews".into(), Value::Array(views));
        }
        while !binary.len().is_multiple_of(4) {
            binary.push(0);
        }
        if fallback > 0 {
            root.insert(
                "buffers".into(),
                json!([
                    { "byteLength": binary.len() },
                    {
                        "byteLength": fallback,
                        "extensions": { "EXT_meshopt_compression": { "fallback": true } },
                    },
                ]),
            );
        } else if !binary.is_empty() {
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

/// Buffer views of `buffer` compressed as `codecs` say: the views, the
/// binary chunk holding each compressed stream and every view stored as it
/// is, and the length of the fallback buffer the compressed views lay out
/// their decompressed bytes in.
fn compress(views: Vec<Value>, codecs: &[Codec], buffer: &[u8]) -> (Vec<Value>, Vec<u8>, usize) {
    let mut binary = Vec::new();
    let mut fallback = 0;
    let views = views
        .into_iter()
        .zip(codecs)
        .map(|(mut view, &codec)| {
            let offset = field(&view, "byteOffset").unwrap_or(0);
            let length = field(&view, "byteLength").unwrap_or(0);
            let bytes = &buffer[offset..offset + length];
            while !binary.len().is_multiple_of(4) {
                binary.push(0);
            }
            let (stream, mode, stride) = match codec {
                Codec::None => {
                    view["byteOffset"] = json!(binary.len());
                    binary.extend_from_slice(bytes);
                    return view;
                }
                Codec::Attributes(stride) => (
                    crate::meshopt::encode_attributes(bytes, stride),
                    "ATTRIBUTES",
                    stride,
                ),
                Codec::Triangles(size) => {
                    let indices: Vec<u32> = bytes
                        .chunks_exact(size)
                        .map(|index| match *index {
                            [a, b] => u32::from(u16::from_le_bytes([a, b])),
                            [a, b, c, d] => u32::from_le_bytes([a, b, c, d]),
                            _ => 0,
                        })
                        .collect();
                    (
                        crate::meshopt::encode_triangles(&indices),
                        "TRIANGLES",
                        size,
                    )
                }
            };
            view["buffer"] = json!(1);
            view["byteOffset"] = json!(fallback);
            view["extensions"] = json!({
                "EXT_meshopt_compression": {
                    "buffer": 0,
                    "byteOffset": binary.len(),
                    "byteLength": stream.len(),
                    "byteStride": stride,
                    "mode": mode,
                    "count": length / stride,
                },
            });
            binary.extend_from_slice(&stream);
            fallback += length.div_ceil(4) * 4;
            view
        })
        .collect();
    (views, binary, fallback)
}

/// What a checked file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub nodes: usize,
    pub meshes: usize,
    pub materials: usize,
    pub images: usize,
    /// Meshes drawn: a node's mesh once, or once per instance of a node
    /// with `EXT_mesh_gpu_instancing`.
    pub instances: usize,
    /// Triangles over every primitive of every mesh drawn, as a renderer
    /// draws them.
    pub triangles: usize,
}

/// A GLB file split into its two chunks.
#[derive(Debug, Clone)]
pub struct Glb<'a> {
    /// The JSON chunk: the document.
    pub json: Value,
    /// The binary chunk, empty when the file has none.
    pub binary: &'a [u8],
    /// Each buffer past the binary chunk, a fallback holding the views
    /// `EXT_meshopt_compression` decompresses into it.
    fallbacks: Vec<Vec<u8>>,
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
        let fallbacks = decompress(&json, binary)?;
        Ok(Self {
            json,
            binary,
            fallbacks,
        })
    }

    /// Buffer `index`'s bytes: the binary chunk's, or a fallback buffer's as
    /// decompressed.
    fn buffer(&self, index: usize) -> Option<&[u8]> {
        match index {
            0 => Some(self.binary),
            _ => self.fallbacks.get(index - 1).map(Vec::as_slice),
        }
    }

    /// Accessor `index`'s components as doubles, normalized integers read
    /// as glTF reads them, and how many make one element (1 for `SCALAR` to
    /// 4 for `VEC4`).
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
        let kind = field(accessor, "componentType")
            .and_then(|kind| u32::try_from(kind).ok())
            .unwrap_or(0);
        let size = component_size(kind)
            .ok_or_else(|| format!("accessor {index} has an unknown component type"))?;
        let normalized = accessor
            .get("normalized")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let count = field(accessor, "count").ok_or("an accessor has no count")?;
        let element = width * size;
        let stride = field(view, "byteStride").unwrap_or(element);
        if stride < element {
            return Err(format!(
                "accessor {index}'s stride is shorter than an element"
            ));
        }
        let view_start = field(view, "byteOffset").unwrap_or(0);
        let view_end = view_start + field(view, "byteLength").unwrap_or(0);
        let start = view_start + field(accessor, "byteOffset").unwrap_or(0);
        let end = if count == 0 {
            start
        } else {
            start + (count - 1) * stride + element
        };
        if end > view_end {
            return Err(format!("accessor {index} runs past its buffer view"));
        }
        let bytes = self
            .buffer(field(view, "buffer").unwrap_or(0))
            .and_then(|buffer| buffer.get(start..end))
            .ok_or(format!("accessor {index} is outside the buffer"))?;
        let mut values = Vec::with_capacity(count * width);
        for item in 0..count {
            let at = item * stride;
            for component in bytes[at..at + element].chunks_exact(size) {
                let value = match (kind, component) {
                    (FLOAT, &[a, b, c, d]) => f64::from(f32::from_le_bytes([a, b, c, d])),
                    (UNSIGNED_INT, &[a, b, c, d]) => f64::from(u32::from_le_bytes([a, b, c, d])),
                    (SHORT, &[a, b]) => f64::from(i16::from_le_bytes([a, b])),
                    (UNSIGNED_SHORT, &[a, b]) => f64::from(u16::from_le_bytes([a, b])),
                    (BYTE, &[a]) => f64::from(a.cast_signed()),
                    (UNSIGNED_BYTE, &[a]) => f64::from(a),
                    _ => f64::NAN,
                };
                values.push(normalize(value, kind, normalized));
            }
        }
        Ok((values, width))
    }
}

/// The buffers after the binary chunk, which hold no bytes of their own,
/// with every buffer view that `EXT_meshopt_compression` stores in the
/// binary chunk decompressed into its place in them.
fn decompress(json: &Value, binary: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let empty = Vec::new();
    let buffers = json
        .get("buffers")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    // A compressed stream holds at most 64 bytes for each of its own (16
    // zeros per two bits of group widths), which bounds what a fallback
    // buffer can need.
    let most = binary.len().saturating_mul(64).saturating_add(256);
    let mut fallbacks: Vec<Vec<u8>> = buffers
        .iter()
        .skip(1)
        .map(|buffer| {
            let length = field(buffer, "byteLength").unwrap_or(0);
            if buffer.get("uri").is_some() || length > most {
                return Err("a buffer outside the file".to_string());
            }
            Ok(vec![0; length])
        })
        .collect::<Result<_, _>>()?;
    let views = json
        .get("bufferViews")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for (index, view) in views.iter().enumerate() {
        let Some(compressed) = view.pointer("/extensions/EXT_meshopt_compression") else {
            continue;
        };
        let error = |what: &str| format!("buffer view {index}'s compression {what}");
        if field(compressed, "buffer") != Some(0) {
            return Err(error("is not in the binary chunk"));
        }
        let at = field(compressed, "byteOffset").unwrap_or(0);
        let stream = field(compressed, "byteLength")
            .and_then(|length| binary.get(at..at + length))
            .ok_or_else(|| error("runs past the binary chunk"))?;
        let (count, stride) = (
            field(compressed, "count").ok_or_else(|| error("has no count"))?,
            field(compressed, "byteStride").ok_or_else(|| error("has no stride"))?,
        );
        if !matches!(
            compressed.get("filter").and_then(Value::as_str),
            None | Some("NONE")
        ) {
            return Err(error("uses a filter this reader lacks"));
        }
        let bytes = match compressed.get("mode").and_then(Value::as_str) {
            Some("ATTRIBUTES") => crate::meshopt::decode_attributes(stream, count, stride)?,
            Some("TRIANGLES") if stride == 2 || stride == 4 => {
                crate::meshopt::decode_triangles(stream, count)?
                    .into_iter()
                    .flat_map(|index| index.to_le_bytes().into_iter().take(stride))
                    .collect()
            }
            _ => return Err(error("has a mode this reader lacks")),
        };
        let offset = field(view, "byteOffset").unwrap_or(0);
        let target = field(view, "buffer")
            .and_then(|buffer| buffer.checked_sub(1))
            .and_then(|buffer| fallbacks.get_mut(buffer))
            .and_then(|buffer| buffer.get_mut(offset..offset + bytes.len()))
            .ok_or_else(|| error("does not fit its fallback buffer"))?;
        if field(view, "byteLength") != Some(bytes.len()) {
            return Err(error("decompresses to another length"));
        }
        target.copy_from_slice(&bytes);
    }
    Ok(fallbacks)
}

/// A component's value as glTF reads it: a normalized integer as a share
/// of its type's range (glTF 2.0, 3.11), anything else as it is.
fn normalize(value: f64, kind: u32, normalized: bool) -> f64 {
    match (normalized, kind) {
        (true, BYTE) => (value / 127.0).max(-1.0),
        (true, UNSIGNED_BYTE) => value / 255.0,
        (true, SHORT) => (value / 32767.0).max(-1.0),
        (true, UNSIGNED_SHORT) => value / 65535.0,
        _ => value,
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
        let size = glb
            .buffer(field(view, "buffer").unwrap_or(0))
            .map_or(0, <[u8]>::len);
        if offset + length > size || !offset.is_multiple_of(4) {
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
        check_bounds(index, accessor, &values, width)?;
        data.push((values, width));
    }
    let mut summary = Summary {
        nodes: array("nodes").len(),
        meshes: array("meshes").len(),
        materials: array("materials").len(),
        images: array("images").len(),
        instances: 0,
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
            mesh_triangles[index] += primitive_triangles(index, primitive, &accessors, &data)?;
        }
    }
    for (index, node) in array("nodes").iter().enumerate() {
        let Some(mesh) = field(node, "mesh") else {
            continue;
        };
        let triangles = *mesh_triangles.get(mesh).ok_or("a node names no mesh")?;
        // A node with `EXT_mesh_gpu_instancing` draws its mesh once for each
        // instance, each placed by its own translation, rotation and scale.
        let Some(attributes) = node
            .pointer("/extensions/EXT_mesh_gpu_instancing/attributes")
            .and_then(Value::as_object)
        else {
            summary.instances += 1;
            summary.triangles += triangles;
            continue;
        };
        let count = instance_count(index, attributes, &data)?;
        summary.instances += count;
        summary.triangles += triangles * count;
    }
    Ok(summary)
}

/// Check one primitive of mesh `index`: positions with bounds, every
/// attribute one a vertex, normals of unit length, and indices in threes
/// inside its vertices; its triangles.
fn primitive_triangles(
    index: usize,
    primitive: &Value,
    accessors: &[Value],
    data: &[(Vec<f64>, usize)],
) -> Result<usize, String> {
    let attribute = |name: &str| {
        primitive
            .pointer(&format!("/attributes/{name}"))
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|at| data.get(at))
    };
    let (positions, 3) = attribute("POSITION").ok_or(format!("mesh {index} has no positions"))?
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
            let kind = primitive
                .pointer("/attributes/NORMAL")
                .and_then(Value::as_u64)
                .and_then(|at| accessors.get(usize::try_from(at).ok()?))
                .and_then(|accessor| field(accessor, "componentType"))
                .and_then(|kind| u32::try_from(kind).ok());
            check_normals(index, kind, values)?;
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
    Ok(indices.len() / 3)
}

/// Check accessor `index`'s declared bounds, if any, against its `values`.
fn check_bounds(
    index: usize,
    accessor: &Value,
    values: &[f64],
    width: usize,
) -> Result<(), String> {
    let kind = field(accessor, "componentType")
        .and_then(|kind| u32::try_from(kind).ok())
        .unwrap_or(FLOAT);
    let normalized = accessor
        .get("normalized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
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
            // Declared bounds are in the data's own type: single precision,
            // or integers before normalizing.
            #[allow(clippy::cast_possible_truncation)]
            let declared = if kind == FLOAT {
                f64::from(declared as f32)
            } else {
                normalize(declared, kind, normalized)
            };
            #[allow(clippy::float_cmp)]
            if declared != actual {
                return Err(format!(
                    "accessor {index}'s {bound} does not match its data"
                ));
            }
        }
    }
    Ok(())
}

/// Check that mesh `index`'s normals, stored as `kind`, are unit length to
/// within what the type can hold.
fn check_normals(index: usize, kind: Option<u32>, values: &[f64]) -> Result<(), String> {
    let tolerance = match kind {
        Some(BYTE) => 0.01,
        Some(SHORT) => 1.0e-3,
        _ => 5.0e-4,
    };
    for normal in values.as_chunks::<3>().0 {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if (length - 1.0).abs() > tolerance {
            return Err(format!("mesh {index} has a normal {length} long"));
        }
    }
    Ok(())
}

/// The instances node `index` draws with `EXT_mesh_gpu_instancing`'s
/// `attributes`: one translation, rotation (a unit quaternion) and scale
/// each, as far as it has them.
fn instance_count(
    index: usize,
    attributes: &Map<String, Value>,
    data: &[(Vec<f64>, usize)],
) -> Result<usize, String> {
    let mut count = None;
    for (name, width) in [("TRANSLATION", 3), ("ROTATION", 4), ("SCALE", 3)] {
        let Some(at) = attributes.get(name) else {
            continue;
        };
        let (values, found) = at
            .as_u64()
            .and_then(|at| data.get(usize::try_from(at).ok()?))
            .ok_or(format!("node {index}'s {name} names no accessor"))?;
        if *found != width || count.is_some_and(|count| count != values.len() / width) {
            return Err(format!(
                "node {index}'s {name} is not one {width}-vector an instance"
            ));
        }
        count = Some(values.len() / width);
        if name == "ROTATION" {
            for rotation in values.as_chunks::<4>().0 {
                let length = rotation
                    .iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt();
                if (length - 1.0).abs() > 1.0e-3 {
                    return Err(format!(
                        "node {index} has an instance rotation {length} long"
                    ));
                }
            }
        }
    }
    count.ok_or(format!("node {index}'s instancing names no attribute"))
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
    fn integers_and_instances_are_read_back() {
        let mut document = Document::new();
        document.extension("KHR_mesh_quantization", true);
        document.extension("EXT_mesh_gpu_instancing", true);
        let positions = document.integer_attribute(
            &[0, 0, 0, 65535, 0, 0, 0, 65535, 7],
            3,
            UNSIGNED_SHORT,
            false,
            true,
        );
        let normals =
            document.integer_attribute(&[0, 0, 127, 0, 0, 127, 0, -127, 0], 3, BYTE, true, false);
        let uvs =
            document.integer_attribute(&[0, 0, 65535, 0, 0, 65535], 2, UNSIGNED_SHORT, true, false);
        let indices = document.indices(&[0, 1, 2]);
        let mesh = document.mesh(
            "triangle",
            vec![json!({
                "attributes": { "POSITION": positions, "NORMAL": normals, "TEXCOORD_0": uvs },
                "indices": indices,
            })],
        );
        let translations = document.instance_attribute(&[0.0, 0.0, 0.0, 1.0, 2.0, 3.0], 3);
        let rotations = document.instance_attribute(&[0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0], 4);
        let node = document.node(json!({
            "mesh": mesh,
            "extensions": { "EXT_mesh_gpu_instancing": { "attributes": {
                "TRANSLATION": translations,
                "ROTATION": rotations,
            } } },
        }));
        let glb = document.glb(json!({ "version": "2.0" }), &[node], json!({}));
        let summary = check(&glb).expect("a valid file");
        assert_eq!(summary.instances, 2);
        assert_eq!(summary.triangles, 2);
        let read = Glb::read(&glb).unwrap();
        assert_eq!(
            read.json["extensionsRequired"],
            json!(["KHR_mesh_quantization", "EXT_mesh_gpu_instancing"])
        );
        // Three shorts and three bytes a vertex are padded to eight and four.
        assert_eq!(read.json["bufferViews"][0]["byteStride"], 8);
        assert_eq!(read.json["bufferViews"][1]["byteStride"], 4);
        assert!(read.json["bufferViews"][2].get("byteStride").is_none());
        assert_eq!(read.json["accessors"][0]["max"], json!([65535, 65535, 7]));
        assert_eq!(
            read.accessor(0).unwrap(),
            (vec![0.0, 0.0, 0.0, 65535.0, 0.0, 0.0, 0.0, 65535.0, 7.0], 3)
        );
        assert_eq!(
            read.accessor(1).unwrap(),
            (vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0], 3)
        );
        assert_eq!(
            read.accessor(2).unwrap(),
            (vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0], 2)
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn compressed_arrays_are_read_back() {
        let mut document = Document::new();
        document.meshopt();
        let positions: Vec<f32> = (0u16..300).map(|at| f32::from(at % 17) * 0.25).collect();
        let positions = document.attribute(&positions, 3, true);
        let indices: Vec<u32> = (0..96).map(|at| (at * 7) % 100).collect();
        let indices = document.indices(&indices);
        let image = document.png("image", &[1, 2, 3, 4, 5]);
        let mesh = document.mesh(
            "points",
            vec![json!({ "attributes": { "POSITION": positions }, "indices": indices })],
        );
        let node = document.node(json!({ "mesh": mesh }));
        let glb = document.glb(
            json!({ "version": "2.0" }),
            &[node],
            json!({ "image": image }),
        );
        assert_eq!(check(&glb).expect("a valid file").triangles, 32);
        let read = Glb::read(&glb).unwrap();
        assert_eq!(
            read.json["extensionsRequired"],
            json!(["EXT_meshopt_compression"])
        );
        let views = read.json["bufferViews"].as_array().unwrap();
        assert_eq!(
            views[0]["extensions"]["EXT_meshopt_compression"]["mode"],
            "ATTRIBUTES"
        );
        assert_eq!(
            views[1]["extensions"]["EXT_meshopt_compression"]["mode"],
            "TRIANGLES"
        );
        // The image is stored as it is, in the binary chunk.
        assert_eq!(views[2]["buffer"], 0);
        assert!(views[2].get("extensions").is_none());
        let (values, width) = read.accessor(positions).unwrap();
        assert_eq!(width, 3);
        assert_eq!(values.len(), 300);
        assert!(
            values
                .iter()
                .enumerate()
                .all(|(at, &value)| value == f64::from(u16::try_from(at).unwrap() % 17) * 0.25)
        );
        assert_eq!(read.accessor(indices).unwrap().0.len(), 96);
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
