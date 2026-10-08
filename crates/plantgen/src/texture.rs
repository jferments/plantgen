//! The drawing kit of the ground's looks.
//!
//! A look is a square texture [`GROUND_LOOK_SIZE`] texels a side that
//! repeats every [`GroundLook::tile_m`] metres, with its albedo in the
//! colour channels and its relief in the alpha channel, drawn from a
//! [`Recipe`]: its name, tile, mean colour and relief, and a function that
//! draws it on a [`Canvas`]. The kit is the canvas (fills, strokes, domes,
//! stamps of organ templates and angular stones, where whatever lies higher
//! hides what lies under it), numbered streams of draws ([`Draws`]),
//! tileable noise ([`value_noise`], [`fbm`], [`ridged`], [`cellular`]), and
//! what a viewer needs from a finished look ([`mip_chain`],
//! [`relief_slopes`], the sRGB conversions). `PlantGen` draws its
//! plant-made looks with it ([`crate::ground`]) and the litters
//! ([`crate::litter`]); a host draws its own looks with it too.
//!
//! Drawing is deterministic, as everything in plant generation is: every
//! random number is a hash of the seed and what it is for, and the
//! transcendental functions go through `libm`. Every stroke and stamp
//! wraps around the texture's edges, so a look tiles without a seam.
//! Finally each look's colours are scaled so that their mean is the look's
//! [`GroundLook::mean`], a linear reflectance: a viewer divides a texel by
//! its look's mean to get how much brighter or darker than the word's
//! colour that spot of ground is. Equations: `docs/developer/PLANTS.md`
//! ("Ground looks").

use crate::looks::{Look, Shape};
use crate::math::{self, PI};
use crate::rng::{hash_words, mix64, unit};
use crate::templates::{TEMPLATE_SIZE, Templates};

/// Texels along each side of a ground look.
pub const GROUND_LOOK_SIZE: usize = 512;

/// One ground look: what one word looks like up close.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundLook {
    pub name: &'static str,
    /// Metres of ground the texture covers before it repeats.
    pub tile_m: f32,
    /// Mean albedo of the texture, linear RGB reflectance: for a living or
    /// organic word its colour (`after_ecology::cover::Ground::colour`),
    /// for a mineral word, whose colour comes from the place's rock, a
    /// neutral one.
    pub mean: [f32; 3],
    /// Height of the highest texel over the lowest, metres: the relief the
    /// alpha channel spans.
    pub relief_m: f32,
    /// [`GROUND_LOOK_SIZE`] squared texels, row by row: sRGB albedo in red,
    /// green and blue, linear relief (0 lowest, 255 highest) in alpha.
    pub rgba: Vec<u8>,
}

/// What each look is drawn with: its tile, its mean colour and its relief.
pub struct Recipe {
    pub name: &'static str,
    pub tile_m: f32,
    pub mean: [f32; 3],
    pub relief_m: f32,
    pub draw: fn(&mut Canvas, u64),
}

/// A recipe, for use in constants.
pub const fn recipe(
    name: &'static str,
    tile_m: f32,
    mean: [f32; 3],
    relief_m: f32,
    draw: fn(&mut Canvas, u64),
) -> Recipe {
    Recipe {
        name,
        tile_m,
        mean,
        relief_m,
        draw,
    }
}

impl Recipe {
    /// Draw the look for `seed`. Each look is drawn on its own from a hash
    /// of the seed and its name, so a viewer can draw looks on several
    /// threads, and a look keeps its pixels whichever list holds its
    /// recipe.
    #[must_use]
    pub fn look(&self, seed: u64) -> GroundLook {
        let mut canvas = Canvas::new(GROUND_LOOK_SIZE);
        (self.draw)(
            &mut canvas,
            hash_words(&[seed, crate::rng::hash_str(self.name)]),
        );
        GroundLook {
            name: self.name,
            tile_m: self.tile_m,
            mean: self.mean,
            relief_m: self.relief_m,
            rgba: canvas.finish(self.mean),
        }
    }

    /// Its tile, mean colour and relief, without drawing it.
    #[must_use]
    pub const fn size(&self) -> (f32, [f32; 3], f32) {
        (self.tile_m, self.mean, self.relief_m)
    }
}

/// The mip chain of a look's texels, finest first, down to one texel:
/// each level averages four texels of the one above, colour in linear
/// light and relief as stored.
#[must_use]
pub fn mip_chain(rgba: &[u8], size: usize) -> Vec<Vec<u8>> {
    let mut levels = vec![rgba.to_vec()];
    let mut edge = size;
    while edge > 1 {
        let above = levels.last().map_or(&[][..], Vec::as_slice);
        let half = edge / 2;
        let mut level = Vec::with_capacity(half * half * 4);
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0_f64; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = ((2 * y + dy) * edge + 2 * x + dx) * 4;
                    for channel in 0..3 {
                        sum[channel] += srgb_to_linear(above[at + channel]);
                    }
                    sum[3] += f64::from(above[at + 3]);
                }
                for total in &sum[..3] {
                    level.push(linear_to_srgb(total / 4.0));
                }
                level.push(to_byte(sum[3] / 4.0 / 255.0));
            }
        }
        levels.push(level);
        edge = half;
    }
    levels
}

/// The slopes of a look's relief, finest first, down to one texel. On a
/// level of the look's [`mip_chain`] `size` texels a side, each texel's
/// slope is how much its relief changes over one tile along a row (the
/// look's `u`) and down the rows (`v`), in fractions of the look's relief
/// per tile: a central difference that wraps around the edges as the look
/// does. Each coarser level averages four texels of the one above. A
/// viewer filters these slopes between texels to light the relief: the
/// slope of filtered relief would change at every texel's edge and light
/// the ground in blocks.
#[must_use]
pub fn relief_slopes(rgba: &[u8], size: usize) -> Vec<Vec<[f32; 2]>> {
    let relief =
        |x: usize, y: usize| f64::from(rgba[((y % size) * size + x % size) * 4 + 3]) / 255.0;
    let per_tile = to_f64(size) / 2.0;
    let mut finest = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let u = (relief(x + 1, y) - relief(x + size - 1, y)) * per_tile;
            let v = (relief(x, y + 1) - relief(x, y + size - 1)) * per_tile;
            // A few hundred at most: well inside f32.
            #[allow(clippy::cast_possible_truncation)]
            finest.push([u as f32, v as f32]);
        }
    }
    let mut levels = vec![finest];
    let mut edge = size;
    while edge > 1 {
        let above = levels.last().map_or(&[][..], Vec::as_slice);
        let half = edge / 2;
        let mut level = Vec::with_capacity(half * half);
        for y in 0..half {
            for x in 0..half {
                let mut sum = [0.0_f32; 2];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let slope = above[(2 * y + dy) * edge + 2 * x + dx];
                    sum[0] += slope[0];
                    sum[1] += slope[1];
                }
                level.push(sum.map(|total| total / 4.0));
            }
        }
        levels.push(level);
        edge = half;
    }
    levels
}

/// sRGB byte to linear light.
#[must_use]
pub fn srgb_to_linear(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        math::pow((c + 0.055) / 1.055, 2.4)
    }
}

/// Linear light to an sRGB byte.
#[must_use]
pub fn linear_to_srgb(value: f64) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let encoded = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * math::pow(c, 1.0 / 2.4) - 0.055
    };
    to_byte(encoded)
}

/// A value from 0 to 1 as a byte, rounded; outside that range, clamped.
#[must_use]
pub fn to_byte(value: f64) -> u8 {
    // Clamped to [0, 255] first.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = libm::round(value.clamp(0.0, 1.0) * 255.0) as u8;
    byte
}

/// A count or index as a float.
#[must_use]
pub fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// A numbered stream of draws: draw `i` of stream `s` is a hash of both.
#[derive(Clone, Copy)]
pub struct Draws(pub u64);

impl Draws {
    /// Draw `index` for `what`, 0 to 1.
    #[must_use]
    pub fn unit(self, index: u64, what: u64) -> f64 {
        unit(mix64(hash_words(&[self.0, index, what])))
    }

    /// Draw `index` for `what`, from `low` to `high`.
    #[must_use]
    pub fn range(self, index: u64, what: u64, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit(index, what)
    }

    /// Draw one of `items`.
    pub fn pick<T: Copy>(self, index: u64, what: u64, items: &[T]) -> T {
        let n = items.len().max(1);
        // Below `n`, which is small.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = ((self.unit(index, what) * to_f64(n)) as usize).min(n - 1);
        items[i]
    }

    /// A whole number below `n`, which is small.
    #[must_use]
    pub fn below(self, index: u64, what: u64, n: u32) -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (self.unit(index, what) * f64::from(n)) as u64;
        i.min(u64::from(n.max(1) - 1))
    }

    /// Another stream, numbered by `name`, independent of this one.
    #[must_use]
    pub fn stream(self, name: u64) -> Self {
        Self(hash_words(&[self.0, name]))
    }
}

/// Tileable value noise on a grid of `cells` a side over the texture, at
/// texel `(x, y)`.
#[must_use]
pub fn value_noise(draws: Draws, cells: usize, octave: u64, x: f64, y: f64, size: usize) -> f64 {
    let scale = to_f64(cells) / to_f64(size);
    let (gx, gy) = (x * scale, y * scale);
    let (fx, fy) = (libm::floor(gx), libm::floor(gy));
    let cell = |i: f64, j: f64| {
        // Grid indices are small and wrapped.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            clippy::cast_sign_loss
        )]
        let (i, j) = (
            (i as i64).rem_euclid(cells as i64) as u64,
            (j as i64).rem_euclid(cells as i64) as u64,
        );
        draws.unit(i * 65_537 + j, octave)
    };
    let blend = |t: f64| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (blend(gx - fx), blend(gy - fy));
    let top = cell(fx, fy) + (cell(fx + 1.0, fy) - cell(fx, fy)) * sx;
    let bottom = cell(fx, fy + 1.0) + (cell(fx + 1.0, fy + 1.0) - cell(fx, fy + 1.0)) * sx;
    top + (bottom - top) * sy
}

/// Fractal sum of `octaves` of tileable noise, the first on `cells` cells,
/// each with twice the cells and half the weight; 0 to 1.
#[must_use]
pub fn fbm(draws: Draws, cells: usize, octaves: u32, x: f64, y: f64, size: usize) -> f64 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += weight * value_noise(draws, cells << octave, u64::from(octave), x, y, size);
        total += weight;
        weight *= 0.5;
    }
    sum / total
}

/// `a` blended towards `b` by `t`, channel by channel.
#[must_use]
pub fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// `a` with every channel times `k`.
#[must_use]
pub fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}

/// A square, wrapping canvas of linear colour and height.
pub struct Canvas {
    pub size: usize,
    colour: Vec<[f64; 3]>,
    height: Vec<f64>,
}

impl Canvas {
    /// A blank canvas `size` texels a side: black, and nothing laid.
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            size,
            colour: vec![[0.0; 3]; size * size],
            height: vec![0.0; size * size],
        }
    }

    fn index(&self, x: i64, y: i64) -> usize {
        // Wrapped into the canvas.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            clippy::cast_sign_loss
        )]
        let (x, y) = (
            x.rem_euclid(self.size as i64) as usize,
            y.rem_euclid(self.size as i64) as usize,
        );
        y * self.size + x
    }

    /// Fill every texel from `paint(x, y) -> (colour, height)`.
    pub fn fill(&mut self, paint: impl Fn(f64, f64) -> ([f64; 3], f64)) {
        for y in 0..self.size {
            for x in 0..self.size {
                let (colour, height) = paint(to_f64(x) + 0.5, to_f64(y) + 0.5);
                self.colour[y * self.size + x] = colour;
                self.height[y * self.size + x] = height;
            }
        }
    }

    /// Lay something at texel `(x, y)` whose surface stands `height` high
    /// and covers `cover` of the texel: it shows where it lies above what
    /// is there.
    pub fn lay(&mut self, x: i64, y: i64, colour: [f64; 3], height: f64, cover: f64) {
        let at = self.index(x, y);
        if cover <= 0.0 || height <= self.height[at] {
            return;
        }
        let cover = cover.min(1.0);
        self.colour[at] = mix(self.colour[at], colour, cover);
        self.height[at] += (height - self.height[at]) * cover;
    }

    /// A straight stroke from `a` to `b`, `width` texels wide, lying at
    /// `height` and rounded across: a needle, a blade, a twig.
    pub fn stroke(&mut self, a: [f64; 2], b: [f64; 2], width: f64, colour: [f64; 3], height: f64) {
        // Lit along its crest, darker at its sides.
        self.stroke_with(a, b, width, |round| {
            (
                scale(colour, 0.8 + 0.3 * round),
                height * (0.6 + 0.4 * round),
            )
        });
    }

    /// A straight stroke from `a` to `b`, `width` texels wide, its colour
    /// and height at each texel `paint(round)`: `round` is 1 along its
    /// middle and falls to 0 at its sides as a rod's top does.
    #[allow(clippy::many_single_char_names)] // Points and vectors of the plane.
    pub fn stroke_with(
        &mut self,
        a: [f64; 2],
        b: [f64; 2],
        width: f64,
        paint: impl Fn(f64) -> ([f64; 3], f64),
    ) {
        let half = 0.5 * width;
        let (lo_x, hi_x) = (a[0].min(b[0]) - half - 1.0, a[0].max(b[0]) + half + 1.0);
        let (lo_y, hi_y) = (a[1].min(b[1]) - half - 1.0, a[1].max(b[1]) + half + 1.0);
        let d = [b[0] - a[0], b[1] - a[1]];
        let length2 = (d[0] * d[0] + d[1] * d[1]).max(1.0e-9);
        // Bounding boxes are a few hundred texels at most.
        #[allow(clippy::cast_possible_truncation)]
        for y in (libm::floor(lo_y) as i64)..=(libm::ceil(hi_y) as i64) {
            #[allow(clippy::cast_possible_truncation)]
            for x in (libm::floor(lo_x) as i64)..=(libm::ceil(hi_x) as i64) {
                #[allow(clippy::cast_precision_loss)]
                let p = [x as f64 + 0.5 - a[0], y as f64 + 0.5 - a[1]];
                let t = ((p[0] * d[0] + p[1] * d[1]) / length2).clamp(0.0, 1.0);
                let q = [p[0] - t * d[0], p[1] - t * d[1]];
                let distance = libm::sqrt(q[0] * q[0] + q[1] * q[1]);
                let cover = (half + 0.5 - distance).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let across = (distance / half.max(0.5)).min(1.0);
                    let round = libm::sqrt(1.0 - across * across);
                    let (colour, height) = paint(round);
                    self.lay(x, y, colour, height, cover);
                }
            }
        }
    }

    /// An ellipse centred at `c` with half-axes `a` along `angle` and `b`
    /// across it, domed `height` high over `base`, its colour from
    /// `paint(u, v, dome)` in the ellipse's own coordinates.
    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    pub fn dome(
        &mut self,
        c: [f64; 2],
        a: f64,
        b: f64,
        angle: f64,
        base: f64,
        height: f64,
        paint: impl Fn(f64, f64, f64) -> [f64; 3],
    ) {
        let reach = a.max(b) + 1.0;
        let (cos, sin) = (math::cos(angle), math::sin(angle));
        #[allow(clippy::cast_possible_truncation)]
        for y in (libm::floor(c[1] - reach) as i64)..=(libm::ceil(c[1] + reach) as i64) {
            #[allow(clippy::cast_possible_truncation)]
            for x in (libm::floor(c[0] - reach) as i64)..=(libm::ceil(c[0] + reach) as i64) {
                #[allow(clippy::cast_precision_loss)]
                let p = [x as f64 + 0.5 - c[0], y as f64 + 0.5 - c[1]];
                let u = (p[0] * cos + p[1] * sin) / a;
                let v = (-p[0] * sin + p[1] * cos) / b;
                let r = libm::sqrt(u * u + v * v);
                let cover = ((1.0 - r) * b.min(a) + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let dome = libm::sqrt((1.0 - r * r).max(0.0));
                    self.lay(x, y, paint(u, v, dome), base + height * dome, cover);
                }
            }
        }
    }

    /// One organ template stamped flat on the ground: centred at `c`,
    /// `length` texels from base to tip along `angle`, in `colour` times
    /// the template's brightness, lying at `height` and curling up by
    /// `curl` toward its edges.
    #[allow(clippy::too_many_arguments)]
    pub fn stamp(
        &mut self,
        templates: &Templates,
        template: usize,
        c: [f64; 2],
        length: f64,
        angle: f64,
        colour: [f64; 3],
        height: f64,
        curl: f64,
    ) {
        self.stamp_worn(
            templates,
            template,
            c,
            [length, angle],
            colour,
            height,
            curl,
            |_, _| true,
        );
    }

    /// [`Self::stamp`] of a template `size` = `[length, angle]`, keeping
    /// only the texels at `(u, v)` (across the organ and along it from its
    /// base, 0 to 1) where `keep(u, v)` holds: a leaf broken or eaten
    /// through as it decays.
    #[allow(clippy::too_many_arguments)]
    pub fn stamp_worn(
        &mut self,
        templates: &Templates,
        template: usize,
        c: [f64; 2],
        size: [f64; 2],
        colour: [f64; 3],
        height: f64,
        curl: f64,
        keep: impl Fn(f64, f64) -> bool,
    ) {
        let [length, angle] = size;
        let Some(shape) = templates.templates.get(template) else {
            return;
        };
        let aspect = shape.aspect;
        let width = length * aspect;
        let reach = 0.5 * libm::sqrt(length * length + width * width) + 1.0;
        let (cos, sin) = (math::cos(angle), math::sin(angle));
        #[allow(clippy::cast_possible_truncation)]
        for y in (libm::floor(c[1] - reach) as i64)..=(libm::ceil(c[1] + reach) as i64) {
            #[allow(clippy::cast_possible_truncation)]
            for x in (libm::floor(c[0] - reach) as i64)..=(libm::ceil(c[0] + reach) as i64) {
                #[allow(clippy::cast_precision_loss)]
                let p = [x as f64 + 0.5 - c[0], y as f64 + 0.5 - c[1]];
                // Along the leaf from its base (0) to its tip (1), and
                // across it from one edge (0) to the other (1).
                let v = (p[0] * cos + p[1] * sin) / length + 0.5;
                let u = (-p[0] * sin + p[1] * cos) / width + 0.5;
                if !((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) && keep(u, v)) {
                    continue;
                }
                // Inside the template's square.
                #[allow(clippy::cast_possible_truncation)]
                let texel = templates.sample(template, u as f32, v as f32);
                if texel.coverage <= 0.02 {
                    continue;
                }
                let edge = (2.0 * u - 1.0).abs();
                let tint = scale(colour, f64::from(texel.brightness).clamp(0.6, 1.3));
                self.lay(
                    x,
                    y,
                    tint,
                    height + curl * edge * edge,
                    f64::from(texel.coverage),
                );
            }
        }
        let _ = TEMPLATE_SIZE;
    }

    /// The height laid at texel `(x, y)`.
    #[cfg(test)]
    pub(crate) fn height_at(&self, x: usize, y: usize) -> f64 {
        self.height[y * self.size + x]
    }

    /// The colour laid at texel `(x, y)`.
    #[cfg(test)]
    pub(crate) fn colour_at(&self, x: usize, y: usize) -> [f64; 3] {
        self.colour[y * self.size + x]
    }

    /// The mean of the canvas's colours as drawn, before [`Self::finish`]
    /// scales them.
    #[must_use]
    pub fn mean_colour(&self) -> [f64; 3] {
        let n = to_f64(self.colour.len().max(1));
        let mut sum = [0.0_f64; 3];
        for colour in &self.colour {
            for channel in 0..3 {
                sum[channel] += colour[channel];
            }
        }
        sum.map(|total| total / n)
    }

    /// The height of the highest texel over the lowest, in the units the
    /// pieces were laid in.
    #[must_use]
    pub fn height_span(&self) -> f64 {
        let (low, high) = self
            .height
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &h| {
                (lo.min(h), hi.max(h))
            });
        (high - low).max(0.0)
    }

    /// The canvas as RGBA bytes, its colours scaled so their mean is
    /// `mean` and its heights spread over the alpha channel.
    #[must_use]
    pub fn finish(&self, mean: [f32; 3]) -> Vec<u8> {
        let n = to_f64(self.colour.len().max(1));
        let mut sum = [0.0_f64; 3];
        for colour in &self.colour {
            for channel in 0..3 {
                sum[channel] += colour[channel];
            }
        }
        let gain: [f64; 3] = std::array::from_fn(|i| f64::from(mean[i]) / (sum[i] / n).max(1.0e-6));
        let (low, high) = self
            .height
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &h| {
                (lo.min(h), hi.max(h))
            });
        let span = (high - low).max(1.0e-9);
        let mut rgba = Vec::with_capacity(self.colour.len() * 4);
        for (colour, height) in self.colour.iter().zip(&self.height) {
            for channel in 0..3 {
                rgba.push(linear_to_srgb(colour[channel] * gain[channel]));
            }
            rgba.push(to_byte((height - low) / span));
        }
        rgba
    }
}

/// Texel position of a draw: uniform over the canvas.
#[must_use]
pub fn spot(draws: Draws, index: u64, size: usize) -> [f64; 2] {
    let edge = to_f64(size);
    [draws.unit(index, 1) * edge, draws.unit(index, 2) * edge]
}

/// A leaf, needle shoot or cone template of the plants' own looks.
#[must_use]
pub fn organ_templates(shapes: &[Shape]) -> Templates {
    let looks: Vec<Look> = shapes
        .iter()
        .map(|shape| Look {
            organ: shape.name().to_owned(),
            shape: shape.clone(),
            colour: [1.0; 3],
            shade: [1.0; 3],
            accent: [1.0; 3],
            face_up: 0.0,
            solid: None,
            // Litter lies flat: its pieces are drawn into the ground's
            // textures, never as bent cards.
            bend: crate::bend::Bend::FLAT,
            form: crate::blooms::Form::Card,
            forms: crate::looks::Forms::default(),
        })
        .collect();
    Templates::for_looks(&looks)
}

/// Tileable cellular noise over `cells` by `cells` cells, each with one
/// point at a random place: at texel `(x, y)`, the distances in texels to
/// the nearest point and the second nearest, and the nearest point's
/// cell. Half the difference of the two distances is about the distance
/// to the border between two cells: the cracks of mud, the ridges of a
/// salt crust, the rings of sorted stones.
#[allow(clippy::many_single_char_names)] // Points of the plane.
#[must_use]
pub fn cellular(draws: Draws, cells: usize, x: f64, y: f64, size: usize) -> (f64, f64, u64) {
    let cell = to_f64(size) / to_f64(cells);
    let (gx, gy) = (libm::floor(x / cell), libm::floor(y / cell));
    let wrap = i64::try_from(cells).unwrap_or(1).max(1);
    let (mut first, mut second, mut nearest) = (f64::INFINITY, f64::INFINITY, 0);
    for dy in -1..=1_i64 {
        for dx in -1..=1_i64 {
            // Cells of a texture: a few hundred at most.
            #[allow(clippy::cast_possible_truncation)]
            let (i, j) = (gx as i64 + dx, gy as i64 + dy);
            #[allow(clippy::cast_sign_loss)]
            let id = (i.rem_euclid(wrap) as u64) * 65_537 + j.rem_euclid(wrap) as u64;
            #[allow(clippy::cast_precision_loss)]
            let (px, py) = (
                (i as f64 + draws.unit(id, 1)) * cell,
                (j as f64 + draws.unit(id, 2)) * cell,
            );
            let d = libm::hypot(px - x, py - y);
            if d < first {
                second = first;
                first = d;
                nearest = id;
            } else if d < second {
                second = d;
            }
        }
    }
    (first, second, nearest)
}

/// Ridged fractal noise, 0 to 1: 1 along the folds of the noise, where
/// cracks and flutes run.
#[must_use]
pub fn ridged(draws: Draws, cells: usize, octaves: u32, x: f64, y: f64, size: usize) -> f64 {
    1.0 - (2.0 * fbm(draws, cells, octaves, x, y, size) - 1.0).abs()
}

impl Canvas {
    /// An angular stone centred at `c`: a polygon of `sides` corners
    /// about `radius` texels out, turned by `angle`, whose top is a low
    /// pyramid of facets `height` above `base` sloping to its edges, each
    /// facet lit by light from the north-west and tinted by a number of
    /// its own.
    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    pub fn block(
        &mut self,
        draws: Draws,
        index: u64,
        c: [f64; 2],
        radius: f64,
        sides: u64,
        angle: f64,
        base: f64,
        height: f64,
        colour: [f64; 3],
    ) {
        let corners: Vec<[f64; 2]> = (0..sides)
            .map(|k| {
                let n = index * 16 + k;
                #[allow(clippy::cast_precision_loss)]
                let a = angle
                    + 2.0 * PI * (k as f64 + 0.35 * (draws.unit(n, 21) - 0.5))
                        / to_f64(usize::try_from(sides).unwrap_or(1));
                let r = radius * draws.range(n, 22, 0.65, 1.0);
                [r * math::cos(a), r * math::sin(a)]
            })
            .collect();
        // Each edge's inward unit normal and offset: a point lies inside
        // where n·p - offset is positive for every edge.
        let edges: Vec<([f64; 2], f64, f64)> = (0..corners.len())
            .map(|k| {
                let (p, q) = (corners[k], corners[(k + 1) % corners.len()]);
                let d = [q[0] - p[0], q[1] - p[1]];
                let length = libm::hypot(d[0], d[1]).max(1.0e-9);
                let mut n = [-d[1] / length, d[0] / length];
                if n[0] * p[0] + n[1] * p[1] > 0.0 {
                    n = [-n[0], -n[1]];
                }
                #[allow(clippy::cast_precision_loss)]
                let tint = draws.range(index * 16 + k as u64, 23, 0.88, 1.1);
                (n, n[0] * p[0] + n[1] * p[1], tint)
            })
            .collect();
        let slope = height / (0.45 * radius).max(1.0);
        // Light from the north-west and above, texel y running south.
        let light = [-0.45, -0.45, 0.77];
        let flat = light[2];
        let reach = radius + 1.0;
        #[allow(clippy::cast_possible_truncation)]
        for y in (libm::floor(c[1] - reach) as i64)..=(libm::ceil(c[1] + reach) as i64) {
            #[allow(clippy::cast_possible_truncation)]
            for x in (libm::floor(c[0] - reach) as i64)..=(libm::ceil(c[0] + reach) as i64) {
                #[allow(clippy::cast_precision_loss)]
                let p = [x as f64 + 0.5 - c[0], y as f64 + 0.5 - c[1]];
                let mut inside = f64::INFINITY;
                let mut facet = 0;
                for (k, (n, offset, _)) in edges.iter().enumerate() {
                    let d = n[0] * p[0] + n[1] * p[1] - offset;
                    if d < inside {
                        inside = d;
                        facet = k;
                    }
                }
                let cover = (inside + 0.5).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let (n, _, tint) = edges[facet];
                let top = inside >= 0.45 * radius;
                // The facet falls outward, against its inward normal.
                let normal = if top {
                    [0.0, 0.0, 1.0]
                } else {
                    let length = libm::sqrt(1.0 + slope * slope);
                    [n[0] * slope / length, n[1] * slope / length, 1.0 / length]
                };
                let lit = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2])
                    .max(0.0)
                    / flat;
                let shade = (0.45 + 0.55 * lit) * tint;
                let rise = (inside / (0.45 * radius).max(1.0)).clamp(0.0, 1.0);
                self.lay(x, y, scale(colour, shade), base + height * rise, cover);
            }
        }
    }
}

/// A size from a power law: mostly `low`, now and then up to `high`.
#[must_use]
pub fn power_size(draws: Draws, index: u64, what: u64, low: f64, high: f64, exponent: f64) -> f64 {
    let u = draws.unit(index, what);
    (low * libm::pow(1.0 - 0.98 * u, -1.0 / exponent)).min(high)
}
