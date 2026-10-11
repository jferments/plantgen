//! The two meshopt codecs glTF's `EXT_meshopt_compression` stores buffer
//! views with, written from the extension's bitstream specification
//! (Appendix A): attributes (mode 0) for vertex and instance arrays, and
//! triangles (mode 1) for triangle lists. The same arrays give the same
//! bytes on every machine. The decoders read any stream the specification
//! allows; `crate::gltf` uses them to check the files it writes.
//!
//! The attribute codec stores each byte of an element as the difference
//! from the same byte of the element before, zigzag-encoded, in blocks of
//! up to 256 elements: one byte position at a time, in groups of 16 whose
//! differences take 0, 2, 4 or 8 bits each. Arrays whose neighbouring
//! elements are alike, such as a mesh's quantized vertices, shrink most.
//!
//! The triangle codec stores most triangles of a well-ordered mesh as one
//! byte: an edge of a recent triangle and a vertex that is the next new
//! one or a recent one. It may turn a triangle's vertices (`a b c` as
//! `b c a`), which keeps its winding.

/// Header of an attribute stream, version 0.
const ATTRIBUTES_HEADER: u8 = 0xa0;
/// Header of a triangle stream, version 1.
const TRIANGLES_HEADER: u8 = 0xe1;
/// Elements in a group.
const GROUP: usize = 16;
/// Bytes of differences a block may hold, and elements at most.
const BLOCK_BYTES: usize = 8192;
const BLOCK_ELEMENTS: usize = 256;
/// The tail of an attribute stream: the baseline element, padded in front
/// to this many bytes.
const TAIL: usize = 32;
/// Bytes a decoder may read for one group, which a stream always has left.
const GROUP_LIMIT: usize = 24;
/// Entries in the edge and vertex queues of the triangle codec.
const QUEUE: usize = 16;

/// The triangle codec's table of common vertex pairs (`0xZW`: Z for the
/// second vertex, W for the third; 0 the next new vertex, `n` the `n`th
/// most recent), stored at the end of every stream. Its last two entries
/// must be 0 and no other may hold a nibble of 15; the first, three new
/// vertices, must be there because writing it out in full restarts the
/// count of new vertices.
const PAIRS: [u8; 16] = [
    0x00, 0x76, 0x87, 0x56, 0x67, 0x78, 0xa9, 0x86, 0x65, 0x89, 0x68, 0x98, 0x01, 0x69, 0, 0,
];

/// Elements in a full attribute block of `stride`-byte elements.
fn block_elements(stride: usize) -> usize {
    ((BLOCK_BYTES / stride) & !(GROUP - 1)).min(BLOCK_ELEMENTS)
}

fn zigzag(difference: u8) -> u8 {
    if difference & 0x80 == 0 {
        difference << 1
    } else {
        !(difference << 1)
    }
}

fn unzigzag(value: u8) -> u8 {
    if value & 1 == 0 {
        value >> 1
    } else {
        !(value >> 1)
    }
}

/// `data`, elements of `stride` bytes each, as an attribute stream.
///
/// # Panics
///
/// When `stride` is not a multiple of 4 from 4 to 256 or `data` is not a
/// whole number of elements.
#[must_use]
pub fn encode_attributes(data: &[u8], stride: usize) -> Vec<u8> {
    assert!(
        (4..=256).contains(&stride) && stride.is_multiple_of(4),
        "an attribute stream's elements are 4 to 256 bytes, in fours"
    );
    assert!(
        data.len().is_multiple_of(stride),
        "a whole number of elements"
    );
    let mut out = Vec::with_capacity(1 + data.len() / 2 + TAIL);
    out.push(ATTRIBUTES_HEADER);
    let mut first = vec![0; stride];
    if let Some(element) = data.get(..stride) {
        first.copy_from_slice(element);
    }
    let mut last = first.clone();
    let mut differences = [0u8; BLOCK_ELEMENTS];
    for block in data.chunks(block_elements(stride) * stride) {
        let elements = block.len() / stride;
        let grouped = elements.div_ceil(GROUP) * GROUP;
        for (byte, previous) in last.iter_mut().enumerate() {
            differences.fill(0);
            for (difference, element) in differences.iter_mut().zip(block.chunks_exact(stride)) {
                *difference = zigzag(element[byte].wrapping_sub(*previous));
                *previous = element[byte];
            }
            encode_groups(&differences[..grouped], &mut out);
        }
    }
    out.resize(out.len() + TAIL.saturating_sub(stride), 0);
    out.extend_from_slice(&first);
    out
}

/// One byte position's differences, a whole number of groups: the groups'
/// widths, two bits each, then each group in its width.
fn encode_groups(differences: &[u8], out: &mut Vec<u8>) {
    let header = out.len();
    out.resize(header + (differences.len() / GROUP).div_ceil(4), 0);
    for (group, values) in differences.as_chunks::<GROUP>().0.iter().enumerate() {
        // The smallest width; on a tie, the earlier of whole bytes, none,
        // two bits and four.
        let (code, bits) = [(3u8, 8u32), (0, 0), (1, 2), (2, 4)]
            .into_iter()
            .min_by_key(|&(_, bits)| group_size(values, bits))
            .unwrap_or((3, 8));
        out[header + group / 4] |= code << ((group % 4) * 2);
        match bits {
            0 => {}
            8 => out.extend_from_slice(values),
            _ => {
                // The values packed first to last from the high bits, those
                // that don't fit as the sentinel, all ones, and then written
                // out in bytes.
                let sentinel = (1u8 << bits) - 1;
                for packed in values.chunks_exact(8 / bits as usize) {
                    let byte = packed
                        .iter()
                        .fold(0u8, |byte, &value| (byte << bits) | value.min(sentinel));
                    out.push(byte);
                }
                out.extend(values.iter().filter(|&&value| value >= sentinel));
            }
        }
    }
}

/// Bytes a group of values takes at `bits` each.
fn group_size(values: &[u8], bits: u32) -> usize {
    match bits {
        0 if values.iter().all(|&value| value == 0) => 0,
        0 => usize::MAX,
        8 => GROUP,
        _ => {
            let sentinel = (1u8 << bits) - 1;
            GROUP * bits as usize / 8 + values.iter().filter(|&&value| value >= sentinel).count()
        }
    }
}

/// The `count` elements of `stride` bytes an attribute stream holds.
///
/// # Errors
///
/// When the stream is not version 0, ends early or holds more.
pub fn decode_attributes(stream: &[u8], count: usize, stride: usize) -> Result<Vec<u8>, String> {
    if !(4..=256).contains(&stride) || !stride.is_multiple_of(4) {
        return Err(format!("an attribute stride of {stride} bytes"));
    }
    let tail = TAIL.max(stride);
    if stream.len() < 1 + tail {
        return Err("the attribute stream is too short".into());
    }
    if stream[0] != ATTRIBUTES_HEADER {
        return Err(format!(
            "attribute stream header {:#04x}, not version 0",
            stream[0]
        ));
    }
    // Each block holds at least a byte of group widths for each byte of an
    // element, which bounds what a stream can claim to hold.
    let per_block = block_elements(stride);
    if count.div_ceil(per_block).saturating_mul(stride) > stream.len() {
        return Err("the attribute stream is too short for its elements".into());
    }
    let mut last = stream[stream.len() - stride..].to_vec();
    let mut out = vec![0; count * stride];
    let mut at = 1;
    let mut differences = [0u8; BLOCK_ELEMENTS];
    for block in out.chunks_mut(per_block * stride) {
        let elements = block.len() / stride;
        let grouped = elements.div_ceil(GROUP) * GROUP;
        for (byte, previous) in last.iter_mut().enumerate() {
            at = decode_groups(stream, at, &mut differences[..grouped])?;
            for (difference, element) in differences.iter().zip(block.chunks_exact_mut(stride)) {
                *previous = previous.wrapping_add(unzigzag(*difference));
                element[byte] = *previous;
            }
        }
    }
    if stream.len() - at != tail {
        return Err("the attribute stream holds more than its elements".into());
    }
    Ok(out)
}

/// Read one byte position's groups from `stream` at `at` into
/// `differences`; where they end.
fn decode_groups(stream: &[u8], mut at: usize, differences: &mut [u8]) -> Result<usize, String> {
    let groups = differences.len() / GROUP;
    let header = at;
    at += groups.div_ceil(4);
    for (group, values) in differences
        .as_chunks_mut::<GROUP>()
        .0
        .iter_mut()
        .enumerate()
    {
        // Every group is followed by at least the tail, so a stream that
        // ends sooner is cut short.
        if stream.len() < at + GROUP_LIMIT {
            return Err("the attribute stream ends early".into());
        }
        match (stream[header + group / 4] >> ((group % 4) * 2)) & 3 {
            0 => values.fill(0),
            3 => {
                values.copy_from_slice(&stream[at..at + GROUP]);
                at += GROUP;
            }
            code => {
                let bits = if code == 1 { 2 } else { 4 };
                let sentinel = (1u8 << bits) - 1;
                let per_byte = 8 / bits;
                let mut extra = at + GROUP / per_byte;
                for (index, value) in values.iter_mut().enumerate() {
                    let shift = 8 - bits * (index % per_byte + 1);
                    let packed = (stream[at + index / per_byte] >> shift) & sentinel;
                    *value = if packed == sentinel {
                        extra += 1;
                        stream[extra - 1]
                    } else {
                        packed
                    };
                }
                at = extra;
            }
        }
    }
    Ok(at)
}

/// The triangle codec's state, the same in the encoder and the decoder:
/// the next new vertex, the last vertex written out in full, and queues of
/// recent edges and vertices, the most recent last.
#[derive(Debug, Default)]
struct Queues {
    next: u32,
    last: u32,
    edges: [[u32; 2]; QUEUE],
    edges_at: usize,
    edges_held: usize,
    vertices: [u32; QUEUE],
    vertices_at: usize,
    vertices_held: usize,
}

impl Queues {
    /// Edge `index` back from the most recent.
    fn edge(&self, index: usize) -> Option<[u32; 2]> {
        (index < self.edges_held).then(|| self.edges[(self.edges_at + QUEUE - 1 - index) % QUEUE])
    }

    /// Vertex `index` back from the most recent.
    fn vertex(&self, index: usize) -> Option<u32> {
        (index < self.vertices_held)
            .then(|| self.vertices[(self.vertices_at + QUEUE - 1 - index) % QUEUE])
    }

    fn push_edge(&mut self, a: u32, b: u32) {
        self.edges[self.edges_at] = [a, b];
        self.edges_at = (self.edges_at + 1) % QUEUE;
        self.edges_held = (self.edges_held + 1).min(QUEUE);
    }

    fn push_vertex(&mut self, vertex: u32) {
        self.vertices[self.vertices_at] = vertex;
        self.vertices_at = (self.vertices_at + 1) % QUEUE;
        self.vertices_held = (self.vertices_held + 1).min(QUEUE);
    }

    /// How far back `vertex` is among the first `within` recent vertices.
    fn find_vertex(&self, vertex: u32, within: usize) -> Option<usize> {
        (0..within).find(|&index| self.vertex(index) == Some(vertex))
    }

    /// `index` written out in full: its zigzag-encoded difference from the
    /// last one so written, as an unsigned LEB128 number.
    fn write_index(&mut self, index: u32, data: &mut Vec<u8>) {
        let difference = index.wrapping_sub(self.last);
        let mut value = (difference << 1) ^ (difference.cast_signed() >> 31).cast_unsigned();
        self.last = index;
        loop {
            #[allow(clippy::cast_possible_truncation)]
            let low = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                data.push(low);
                return;
            }
            data.push(low | 0x80);
        }
    }

    /// An index written out in full, read from `data` at `at`.
    fn read_index(&mut self, data: &[u8], at: &mut usize) -> Result<u32, String> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = *data.get(*at).ok_or("the triangle stream ends early")?;
            *at += 1;
            value |= u32::from(byte & 0x7f).wrapping_shl(shift);
            if byte & 0x80 == 0 {
                let difference = (value >> 1) ^ (value & 1).wrapping_neg();
                self.last = self.last.wrapping_add(difference);
                return Ok(self.last);
            }
        }
        Err("an index runs past five bytes".into())
    }
}

/// Triangle list `indices` as a triangle stream.
///
/// # Panics
///
/// When `indices` is not a whole number of triangles.
#[must_use]
pub fn encode_triangles(indices: &[u32]) -> Vec<u8> {
    assert!(
        indices.len().is_multiple_of(3),
        "a whole number of triangles"
    );
    let mut codes = Vec::with_capacity(indices.len() / 3);
    let mut data = Vec::new();
    let mut queues = Queues::default();
    for &[i, j, k] in indices.as_chunks::<3>().0 {
        // The triangle turned so that an edge of a recent one, the same
        // way round, comes first.
        let edge = (0..QUEUE - 1).find_map(|index| {
            let edge = queues.edge(index)?;
            let turned = if edge == [i, j] {
                [i, j, k]
            } else if edge == [j, k] {
                [j, k, i]
            } else if edge == [k, i] {
                [k, i, j]
            } else {
                return None;
            };
            Some((index, turned))
        });
        let code = match edge {
            Some((index, turned)) => queues.encode_on_edge(index, turned, &mut data),
            None => queues.encode_free([i, j, k], &mut data),
        };
        codes.push(code);
    }
    let mut out = Vec::with_capacity(1 + codes.len() + data.len() + PAIRS.len());
    out.push(TRIANGLES_HEADER);
    out.extend_from_slice(&codes);
    out.extend_from_slice(&data);
    out.extend_from_slice(&PAIRS);
    out
}

impl Queues {
    /// The code of triangle `a b c`, whose edge `a b` is edge `index` back
    /// in the queue, with what it writes out in full put in `data`.
    fn encode_on_edge(&mut self, index: usize, [a, b, c]: [u32; 3], data: &mut Vec<u8>) -> u8 {
        // A recent vertex, but not the most recent, which the code for the
        // next new vertex takes.
        let third = if let Some(back) = (1..13).find(|&back| self.vertex(back) == Some(c)) {
            back
        } else if c == self.next {
            self.next += 1;
            0
        } else if c.wrapping_add(1) == self.last {
            13
        } else if c == self.last.wrapping_add(1) {
            14
        } else {
            15
        };
        match third {
            15 => self.write_index(c, data),
            13 | 14 => self.last = c,
            _ => {}
        }
        if third == 0 || third >= 13 {
            self.push_vertex(c);
        }
        self.push_edge(c, b);
        self.push_edge(a, c);
        #[allow(clippy::cast_possible_truncation)]
        let code = ((index as u8) << 4) | third as u8;
        code
    }

    /// The code of a triangle that shares no recent edge, with what it
    /// writes out in full put in `data`.
    fn encode_free(&mut self, triangle: [u32; 3], data: &mut Vec<u8>) -> u8 {
        // The next new vertex first, if it is one of the three.
        let turn = triangle[1..]
            .iter()
            .position(|&vertex| vertex == self.next)
            .map_or(0, |at| at + 1);
        let [a, b, c] = std::array::from_fn(|at| triangle[(at + turn) % 3]);
        let first_new = a == self.next;
        if first_new {
            self.next += 1;
        }
        let second = self.free_vertex(b);
        let third = self.free_vertex(c);
        let pair = (second << 4) | third;
        let code = match PAIRS[..14].iter().position(|&entry| entry == pair) {
            #[allow(clippy::cast_possible_truncation)]
            Some(entry) if first_new => 0xf0 | entry as u8,
            _ => {
                // Written out in full, where a pair of zeros would restart
                // the count of new vertices; it never is, since a first
                // vertex that is not the next new one leaves none of them
                // new.
                debug_assert!(pair != 0);
                data.push(pair);
                if first_new { 0xfe } else { 0xff }
            }
        };
        if !first_new {
            self.write_index(a, data);
        }
        if second == 15 {
            self.write_index(b, data);
        }
        if third == 15 {
            self.write_index(c, data);
        }
        self.push_vertex(a);
        if second == 0 || second == 15 {
            self.push_vertex(b);
        }
        if third == 0 || third == 15 {
            self.push_vertex(c);
        }
        self.push_edge(b, a);
        self.push_edge(c, b);
        self.push_edge(a, c);
        code
    }

    /// The nibble naming `vertex` in a triangle that shares no recent
    /// edge: a recent vertex, the next new one, or 15 for one written out.
    fn free_vertex(&mut self, vertex: u32) -> u8 {
        if let Some(back) = self.find_vertex(vertex, 14) {
            #[allow(clippy::cast_possible_truncation)]
            return back as u8 + 1;
        }
        if vertex == self.next {
            self.next += 1;
            return 0;
        }
        15
    }

    /// The triangle of code `0xXY`, `X` below 15: on edge `X` back.
    fn decode_on_edge(
        &mut self,
        code: u8,
        data: &[u8],
        at: &mut usize,
    ) -> Result<[u32; 3], String> {
        let [a, b] = self.edge(usize::from(code >> 4)).ok_or_else(missing)?;
        let third = code & 15;
        let c = match third {
            0 => {
                self.next += 1;
                self.next - 1
            }
            1..=12 => self.vertex(usize::from(third)).ok_or_else(missing)?,
            13 | 14 => {
                let c = if third == 13 {
                    self.last.wrapping_sub(1)
                } else {
                    self.last.wrapping_add(1)
                };
                self.last = c;
                c
            }
            _ => self.read_index(data, at)?,
        };
        if third == 0 || third >= 13 {
            self.push_vertex(c);
        }
        self.push_edge(c, b);
        self.push_edge(a, c);
        Ok([a, b, c])
    }

    /// The triangle of code `0xfY`: three vertices on their own.
    fn decode_free(
        &mut self,
        code: u8,
        pairs: &[u8],
        data: &[u8],
        at: &mut usize,
    ) -> Result<[u32; 3], String> {
        let low = code & 15;
        let (first_new, pair) = if low < 14 {
            (true, pairs[usize::from(low)])
        } else {
            let pair = *data.get(*at).ok_or("the triangle stream ends early")?;
            *at += 1;
            if pair == 0 {
                self.next = 0;
            }
            (low == 14, pair)
        };
        let (second, third) = (pair >> 4, pair & 15);
        let a = first_new.then(|| {
            self.next += 1;
            self.next - 1
        });
        // Vertices from the queue or new are read before any written out.
        let b = self.early_vertex(second)?;
        let c = self.early_vertex(third)?;
        let mut written = |vertex: Option<u32>| match vertex {
            Some(vertex) => Ok(vertex),
            None => self.read_index(data, at),
        };
        let (a, b, c) = (written(a)?, written(b)?, written(c)?);
        self.push_vertex(a);
        if second == 0 || second == 15 {
            self.push_vertex(b);
        }
        if third == 0 || third == 15 {
            self.push_vertex(c);
        }
        self.push_edge(b, a);
        self.push_edge(c, b);
        self.push_edge(a, c);
        Ok([a, b, c])
    }

    /// The vertex nibble `code` names in a triangle of its own, unless it
    /// is written out (15).
    fn early_vertex(&mut self, code: u8) -> Result<Option<u32>, String> {
        match code {
            0 => {
                self.next += 1;
                Ok(Some(self.next - 1))
            }
            15 => Ok(None),
            _ => self
                .vertex(usize::from(code) - 1)
                .map(Some)
                .ok_or_else(missing),
        }
    }
}

fn missing() -> String {
    "the triangle stream names a vertex its queues don't hold".into()
}

/// The `count` indices, a triangle list, a triangle stream holds.
///
/// # Errors
///
/// When the stream is not version 1, names a vertex its queues don't hold,
/// ends early or holds more.
pub fn decode_triangles(stream: &[u8], count: usize) -> Result<Vec<u32>, String> {
    if !count.is_multiple_of(3) {
        return Err("a triangle stream holds whole triangles".into());
    }
    let triangles = count / 3;
    if stream.len() < 1 + triangles + PAIRS.len() {
        return Err("the triangle stream is too short".into());
    }
    if stream[0] != TRIANGLES_HEADER {
        return Err(format!(
            "triangle stream header {:#04x}, not version 1",
            stream[0]
        ));
    }
    let end = stream.len() - PAIRS.len();
    let (data, pairs) = stream.split_at(end);
    let mut at = 1 + triangles;
    let mut queues = Queues::default();
    let mut out = Vec::with_capacity(count);
    for &code in &stream[1..=triangles] {
        let triangle = if code < 0xf0 {
            queues.decode_on_edge(code, data, &mut at)?
        } else {
            queues.decode_free(code, pairs, data, &mut at)?
        };
        out.extend_from_slice(&triangle);
    }
    if at != end {
        return Err("the triangle stream holds more than its triangles".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator for test data.
    fn noise(seed: &mut u64) -> u64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *seed >> 33
    }

    #[test]
    fn the_specifications_example_group_decodes() {
        // Appendix A: sixteen 4-bit differences, two of them in bytes.
        let mut stream = vec![ATTRIBUTES_HEADER, 0b10];
        stream.extend_from_slice(&[0x17, 0x5f, 0xf0, 0xbc, 0x77, 0xa9, 0x21, 0x00, 0x34, 0xb5]);
        stream.resize(stream.len() + TAIL, 0);
        let mut differences = [0u8; GROUP];
        let end = decode_groups(&stream, 1, &mut differences).unwrap();
        assert_eq!(end, 12);
        let decoded: Vec<i8> = differences
            .iter()
            .map(|&value| unzigzag(value).cast_signed())
            .collect();
        assert_eq!(
            decoded,
            [-1, -4, -3, 26, -91, 0, -6, 6, -4, -4, 5, -5, 1, -1, 0, 0]
        );
    }

    #[test]
    fn attributes_come_back_as_they_were() {
        let mut seed = 7;
        for (count, stride) in [
            (0, 4),
            (1, 4),
            (15, 8),
            (16, 12),
            (17, 16),
            (700, 12),
            (1000, 36),
            (300, 256),
        ] {
            // Smooth runs, steps and noise, as vertices hold.
            let data: Vec<u8> = (0..count * stride)
                .map(|at| {
                    let (element, byte) = (at / stride, at % stride);
                    #[allow(clippy::cast_possible_truncation)]
                    match byte % 4 {
                        0 => (element / 3) as u8,
                        1 => (element * 7 + byte) as u8,
                        2 => noise(&mut seed) as u8,
                        _ => 0,
                    }
                })
                .collect();
            let stream = encode_attributes(&data, stride);
            assert_eq!(decode_attributes(&stream, count, stride).unwrap(), data);
            assert_eq!(stream, encode_attributes(&data, stride));
            assert!(decode_attributes(&stream[..stream.len() - 1], count, stride).is_err());
        }
        // Alike neighbours shrink.
        let alike: Vec<u8> = (0u32..4000)
            .flat_map(|value| (value / 8).to_le_bytes())
            .collect();
        assert!(encode_attributes(&alike, 4).len() * 4 < alike.len());
    }

    #[test]
    fn triangles_come_back_turned_at_most() {
        // A grid of quads, as a bent card's, and a fan of scattered ones.
        let mut grid = Vec::new();
        for row in 0..20u32 {
            for column in 0..30u32 {
                let at = row * 31 + column;
                grid.extend_from_slice(&[at, at + 1, at + 31, at + 1, at + 32, at + 31]);
            }
        }
        let mut seed = 3;
        #[allow(clippy::cast_possible_truncation)]
        let scattered: Vec<u32> = (0..3000)
            .map(|_| (noise(&mut seed) % 5000) as u32)
            .collect();
        let mut repeated = grid.clone();
        repeated.extend_from_slice(&grid);
        for indices in [
            Vec::new(),
            vec![0, 1, 2],
            vec![5, 5, 5, 0, 1, 2],
            grid.clone(),
            scattered,
            repeated,
        ] {
            let stream = encode_triangles(&indices);
            let decoded = decode_triangles(&stream, indices.len()).unwrap();
            for (got, want) in decoded
                .as_chunks::<3>()
                .0
                .iter()
                .zip(indices.as_chunks::<3>().0)
            {
                let turns = [
                    [want[0], want[1], want[2]],
                    [want[1], want[2], want[0]],
                    [want[2], want[0], want[1]],
                ];
                assert!(
                    turns.contains(&[got[0], got[1], got[2]]),
                    "{got:?} {want:?}"
                );
            }
            assert_eq!(decoded.len(), indices.len());
            assert!(decode_triangles(&stream[..stream.len() - 1], indices.len()).is_err());
        }
        // A grid's triangles take about a byte each.
        assert!(encode_triangles(&grid).len() < grid.len() / 3 * 3 / 2);
    }
}
