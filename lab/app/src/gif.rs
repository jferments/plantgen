//! A GIF89a writer for PlantLab's growth animations: one palette for every
//! frame, so colours do not flicker between them, chosen by median cut over
//! the frames' pixels; an optional transparent colour; LZW; and an endless
//! loop. Written here rather than added as a dependency (`AGENTS.md`).

use std::collections::HashMap;

/// One picture of an animation: straight (not premultiplied) RGBA rows.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// How long it shows, hundredths of a second.
    pub delay: u16,
}

/// Pixels with less alpha than this are transparent in a transparent GIF.
const OPAQUE: u8 = 128;

/// At most this many pixels feed the palette.
const SAMPLES: usize = 400_000;

/// Encode `frames` as a looping GIF. With `transparent`, pixels under half
/// alpha are clear and each frame replaces the last; otherwise alpha is
/// ignored.
///
/// # Errors
///
/// When there are no frames, they differ in size, or a side is over 65535.
pub fn encode(frames: &[Frame], transparent: bool) -> Result<Vec<u8>, String> {
    let first = frames.first().ok_or("an animation needs a frame")?;
    let (width, height) = (first.width, first.height);
    if frames
        .iter()
        .any(|frame| frame.width != width || frame.height != height)
    {
        return Err("every frame of an animation must be the same size".into());
    }
    if frame_rgba_wrong(frames) {
        return Err("a frame's pixels do not match its size".into());
    }
    let (w, h) = (
        u16::try_from(width).map_err(|_| "the animation is too wide")?,
        u16::try_from(height).map_err(|_| "the animation is too tall")?,
    );
    let colours = if transparent { 255 } else { 256 };
    let palette = median_cut(&samples(frames, transparent), colours);
    let clear = u8::try_from(palette.len()).unwrap_or(u8::MAX);
    let mut nearest = Nearest::new(&palette);

    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    // A global table of 256 colours, 8 bits of colour resolution.
    out.extend_from_slice(&[0xF7, 0, 0]);
    for index in 0..256 {
        out.extend_from_slice(&palette.get(index).copied().unwrap_or([0, 0, 0]));
    }
    // Loop forever.
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[3, 1, 0, 0, 0]);
    for frame in frames {
        // Graphic control: a transparent frame clears to the background
        // (2) so the one before does not show through; else keep it (1).
        let packed = if transparent { (2 << 2) | 1 } else { 1 << 2 };
        out.extend_from_slice(&[0x21, 0xF9, 4, packed]);
        out.extend_from_slice(&frame.delay.to_le_bytes());
        out.extend_from_slice(&[if transparent { clear } else { 0 }, 0]);
        out.push(0x2C);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.push(0);
        let indices: Vec<u8> = frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| {
                if transparent && pixel[3] < OPAQUE {
                    clear
                } else {
                    nearest.index([pixel[0], pixel[1], pixel[2]])
                }
            })
            .collect();
        out.push(8);
        for block in lzw(&indices).chunks(255) {
            // A chunk is at most 255 bytes.
            #[allow(clippy::cast_possible_truncation)]
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }
    out.push(0x3B);
    Ok(out)
}

fn frame_rgba_wrong(frames: &[Frame]) -> bool {
    frames
        .iter()
        .any(|frame| frame.rgba.len() != frame.width * frame.height * 4)
}

/// Undo the premultiplied alpha a render leaves where an edge meets a
/// clear background, so edge pixels keep their colour.
pub fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                // At most 255 by the min.
                #[allow(clippy::cast_possible_truncation)]
                let straight = (u32::from(*channel) * 255 / alpha).min(255) as u8;
                *channel = straight;
            }
        }
    }
}

/// The colours the palette is chosen from: an even sample of every
/// frame's pixels that will show.
fn samples(frames: &[Frame], transparent: bool) -> Vec<[u8; 3]> {
    let total: usize = frames.iter().map(|frame| frame.width * frame.height).sum();
    let every = total.div_ceil(SAMPLES).max(1);
    frames
        .iter()
        .flat_map(|frame| frame.rgba.as_chunks::<4>().0.iter())
        .step_by(every)
        .filter(|pixel| !transparent || pixel[3] >= OPAQUE)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect()
}

/// Up to `count` colours for `colours`: split the box with the most
/// spread along its widest channel at its median, then average each box.
fn median_cut(colours: &[[u8; 3]], count: usize) -> Vec<[u8; 3]> {
    if colours.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes = vec![colours.to_vec()];
    while boxes.len() < count {
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, colours)| colours.len() > 1)
            .map(|(index, colours)| {
                let (channel, range) = widest_channel(colours);
                (index, channel, range)
            })
            .filter(|(_, _, range)| *range > 0)
            .max_by_key(|(index, _, range)| (*range, boxes[*index].len()));
        let Some((index, channel, _)) = widest else {
            break;
        };
        let mut split = boxes.swap_remove(index);
        split.sort_unstable_by_key(|colour| colour[channel]);
        let upper = split.split_off(split.len() / 2);
        boxes.push(split);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|colours| {
            let mut sum = [0_u64; 3];
            for colour in colours {
                for (total, channel) in sum.iter_mut().zip(colour) {
                    *total += u64::from(*channel);
                }
            }
            let n = colours.len().max(1) as u64;
            // An average of bytes is a byte.
            #[allow(clippy::cast_possible_truncation)]
            sum.map(|total| ((total + n / 2) / n) as u8)
        })
        .collect()
}

fn widest_channel(colours: &[[u8; 3]]) -> (usize, u8) {
    let mut low = [u8::MAX; 3];
    let mut high = [0_u8; 3];
    for colour in colours {
        for channel in 0..3 {
            low[channel] = low[channel].min(colour[channel]);
            high[channel] = high[channel].max(colour[channel]);
        }
    }
    (0..3)
        .map(|channel| (channel, high[channel] - low[channel]))
        .max_by_key(|(_, range)| *range)
        .unwrap_or((0, 0))
}

/// The nearest palette colour, remembered by 5-bit cell.
struct Nearest<'a> {
    palette: &'a [[u8; 3]],
    cells: HashMap<u16, u8>,
}

impl<'a> Nearest<'a> {
    fn new(palette: &'a [[u8; 3]]) -> Self {
        Self {
            palette,
            cells: HashMap::new(),
        }
    }

    fn index(&mut self, colour: [u8; 3]) -> u8 {
        let cell = (u16::from(colour[0] >> 3) << 10)
            | (u16::from(colour[1] >> 3) << 5)
            | u16::from(colour[2] >> 3);
        let palette = self.palette;
        *self.cells.entry(cell).or_insert_with(|| {
            let centre = colour.map(|channel| i32::from(channel | 4));
            let best = palette
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| {
                    (0..3)
                        .map(|c| (i32::from(entry[c]) - centre[c]).pow(2))
                        .sum::<i32>()
                })
                .map_or(0, |(index, _)| index);
            u8::try_from(best).unwrap_or(u8::MAX)
        })
    }
}

/// GIF's variable-width LZW of 8-bit `indices`, bytes packed low bit
/// first, as `gifenc` writes it: the code width grows before the entry
/// that needs it, and a full table of 4096 codes starts again.
fn lzw(indices: &[u8]) -> Vec<u8> {
    const CLEAR: u16 = 256;
    const END: u16 = 257;
    let mut bits = Bits::default();
    let mut width = 9;
    // children[code * 256 + byte]: the code for `code` then `byte`, or 0.
    let mut children = vec![0_u16; 4096 * 256];
    let mut next: u16 = 258;
    bits.put(CLEAR, width);
    let Some((&first, rest)) = indices.split_first() else {
        bits.put(END, width);
        return bits.finish();
    };
    let mut code = u16::from(first);
    for &byte in rest {
        let slot = usize::from(code) * 256 + usize::from(byte);
        if children[slot] != 0 {
            code = children[slot];
            continue;
        }
        bits.put(code, width);
        if next < 4096 {
            if u32::from(next) == 1 << width {
                width += 1;
            }
            children[slot] = next;
            next += 1;
        } else {
            bits.put(CLEAR, width);
            children.fill(0);
            next = 258;
            width = 9;
        }
        code = u16::from(byte);
    }
    bits.put(code, width);
    bits.put(END, width);
    bits.finish()
}

#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    buffer: u32,
    count: u32,
}

impl Bits {
    fn put(&mut self, code: u16, width: u32) {
        self.buffer |= u32::from(code) << self.count;
        self.count += width;
        while self.count >= 8 {
            // The low byte.
            #[allow(clippy::cast_possible_truncation)]
            self.bytes.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            #[allow(clippy::cast_possible_truncation)]
            self.bytes.push(self.buffer as u8);
        }
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GIF's LZW decoder, as a reader implements it.
    fn unlzw(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut at, mut width) = (0_usize, 9_u32);
        let read = |at: &mut usize, width: u32| -> u16 {
            let mut code = 0_u32;
            for bit in 0..width {
                let index = *at + bit as usize;
                if bytes[index / 8] >> (index % 8) & 1 == 1 {
                    code |= 1 << bit;
                }
            }
            *at += width as usize;
            u16::try_from(code).expect("a code")
        };
        let mut table: Vec<Vec<u8>> = Vec::new();
        let reset = |table: &mut Vec<Vec<u8>>| {
            table.clear();
            table.extend((0..=255_u8).map(|byte| vec![byte]));
            table.push(Vec::new());
            table.push(Vec::new());
        };
        reset(&mut table);
        let mut previous: Option<Vec<u8>> = None;
        loop {
            let code = read(&mut at, width);
            match code {
                256 => {
                    reset(&mut table);
                    width = 9;
                    previous = None;
                }
                257 => return out,
                _ => {
                    let entry = if usize::from(code) < table.len() {
                        table[usize::from(code)].clone()
                    } else {
                        let mut entry = previous.clone().expect("a previous string");
                        entry.push(entry[0]);
                        entry
                    };
                    out.extend_from_slice(&entry);
                    if let Some(mut grown) = previous.take() {
                        grown.push(entry[0]);
                        if table.len() < 4096 {
                            table.push(grown);
                        }
                    }
                    if table.len() == 1 << width && width < 12 {
                        width += 1;
                    }
                    previous = Some(entry);
                }
            }
        }
    }

    #[test]
    fn lzw_round_trips_through_a_full_table() {
        // Repetitive and noisy runs, long enough to fill the table twice.
        let mut indices = Vec::new();
        let mut state = 7_u32;
        for run in 0..3_000_u32 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let byte = state.to_le_bytes()[2];
            let length = 1 + run % 13;
            indices.extend(std::iter::repeat_n(byte, length as usize));
        }
        assert_eq!(unlzw(&lzw(&indices)), indices);
        assert_eq!(unlzw(&lzw(&[5])), vec![5]);
        assert_eq!(unlzw(&lzw(&[])), Vec::<u8>::new());
    }

    #[test]
    fn a_palette_keeps_few_colours_exactly() {
        let colours = vec![[10, 20, 30], [200, 100, 0], [0, 255, 0], [10, 20, 30]];
        let mut palette = median_cut(&colours, 255);
        palette.sort_unstable();
        palette.dedup();
        assert_eq!(palette, vec![[0, 255, 0], [10, 20, 30], [200, 100, 0]]);
    }

    #[test]
    fn an_animation_has_its_frames_and_transparency() {
        let frame = |shade: u8| Frame {
            width: 4,
            height: 2,
            rgba: [[shade, 0, 0, 255], [0, 0, 0, 0]].repeat(4).concat(),
            delay: 10,
        };
        let gif = encode(&[frame(50), frame(200)], true).expect("encodes");
        assert!(gif.starts_with(b"GIF89a") && gif.ends_with(&[0x3B]));
        let images = gif.windows(2).filter(|pair| pair == &[0x00, 0x2C]).count();
        assert_eq!(images, 2, "two image descriptors");
        assert!(
            encode(
                &[
                    frame(1),
                    Frame {
                        width: 2,
                        ..frame(1)
                    }
                ],
                false
            )
            .is_err()
        );
    }

    #[test]
    fn unpremultiplying_restores_an_edge_colour() {
        let mut rgba = vec![100, 50, 25, 128, 9, 9, 9, 255, 0, 0, 0, 0];
        unpremultiply(&mut rgba);
        assert_eq!(rgba, vec![199, 99, 49, 128, 9, 9, 9, 255, 0, 0, 0, 0]);
    }
}
