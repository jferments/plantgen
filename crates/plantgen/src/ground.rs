//! Ground looks: tiling textures of what covers the ground between plants.
//!
//! The terrain draws the ground as shares of twenty-six words
//! (`after_ecology::cover::Ground`: litters, living ground layers, fine
//! mineral surfaces, stones and bedrock), and each word has one look here:
//! a square texture [`GROUND_LOOK_SIZE`] texels a side that repeats every
//! [`GroundLook::tile_m`] metres, with its albedo in the colour channels
//! and its relief in the alpha channel. A look is drawn the way the ground
//! is made: needles, twigs and cones fall on dark duff; leaves of the
//! organ templates the plants wear (`palmate`, `simple`, `lobed`) fall on
//! soil; moss grows in cushions, sphagnum in star-headed carpets and
//! reindeer lichen in pale clumps; desert soil crust rises in dark
//! pinnacles; grass tufts spread their blades and tussocks stand in
//! clumps with bare ground between; mud and salt crack into polygons;
//! gravel is a bed of rounded stones, scree of angular ones, desert
//! pavement a mosaic of varnished pebbles, and frost sorts stones into
//! rings round fine soil; bedrock is massive and jointed, bedded,
//! fluted karst or foliated in bands. Whatever lies higher hides what lies
//! under it, and the relief records how high each texel lies, so a viewer
//! can blend words by height and light their bumps.
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

use crate::looks::{Fruit, Lobed, Look, Palmate, Shape, Simple};
use crate::math::{self, PI};
use crate::rng::{hash_words, mix64, unit};
use crate::templates::{TEMPLATE_SIZE, Templates};

/// Texels along each side of a ground look.
pub const GROUND_LOOK_SIZE: usize = 512;

/// The ground looks, in the order of `after_ecology::cover::Ground::ALL`.
pub const GROUND_LOOK_NAMES: [&str; 26] = [
    "needles", "leaves", "thatch", "twigs", "moss", "sphagnum", "lichen", "crust", "grass",
    "tussock", "cushion", "soil", "peat", "clay", "sand", "ash", "salt", "ice", "gravel", "scree",
    "pavement", "sorted", "massive", "bedded", "karst", "foliated",
];

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
struct Recipe {
    name: &'static str,
    tile_m: f32,
    mean: [f32; 3],
    relief_m: f32,
    draw: fn(&mut Canvas, u64),
}

const fn recipe(
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

const RECIPES: [Recipe; 26] = [
    // Douglas-fir duff: rusty needles over dark humus.
    recipe("needles", 1.5, [0.105, 0.0675, 0.0375], 0.012, draw_needles),
    // Dry broadleaf litter: tan, brown and ochre.
    recipe("leaves", 2.0, [0.145, 0.10, 0.0525], 0.02, draw_leaves),
    // Dead grass lying flat in swirls.
    recipe("thatch", 1.5, [0.30, 0.26, 0.16], 0.02, draw_thatch),
    // Twigs, bark and small leaves under desert shrubs.
    recipe("twigs", 2.0, [0.17, 0.14, 0.11], 0.015, draw_twigs),
    recipe("moss", 1.0, [0.065, 0.11, 0.03], 0.03, draw_moss),
    recipe("sphagnum", 1.0, [0.13, 0.13, 0.05], 0.025, draw_sphagnum),
    recipe("lichen", 1.0, [0.32, 0.33, 0.26], 0.04, draw_lichen),
    recipe("crust", 1.0, [0.11, 0.095, 0.075], 0.02, draw_crust),
    recipe("grass", 1.0, [0.12, 0.175, 0.0725], 0.04, draw_grass),
    recipe("tussock", 2.0, [0.24, 0.21, 0.13], 0.25, draw_tussock),
    recipe("cushion", 1.0, [0.06, 0.13, 0.035], 0.06, draw_cushion),
    recipe("soil", 2.0, [0.18, 0.15, 0.105], 0.012, draw_soil),
    recipe("peat", 1.5, [0.05, 0.04, 0.03], 0.02, draw_peat),
    recipe("clay", 2.0, [0.26, 0.23, 0.19], 0.02, draw_clay),
    recipe("sand", 1.0, [0.29, 0.26, 0.195], 0.008, draw_sand),
    recipe("ash", 1.5, [0.10, 0.10, 0.10], 0.02, draw_ash),
    recipe("salt", 3.0, [0.72, 0.70, 0.66], 0.03, draw_salt),
    recipe("ice", 4.0, [0.70, 0.76, 0.82], 0.05, draw_ice),
    recipe("gravel", 1.0, [0.23, 0.21, 0.175], 0.025, draw_gravel),
    recipe("scree", 2.0, [0.24, 0.23, 0.21], 0.12, draw_scree),
    recipe("pavement", 1.0, [0.16, 0.13, 0.10], 0.012, draw_pavement),
    recipe("sorted", 3.0, [0.22, 0.20, 0.17], 0.08, draw_sorted),
    recipe("massive", 4.0, [0.20, 0.19, 0.175], 0.12, draw_massive),
    recipe("bedded", 4.0, [0.24, 0.22, 0.19], 0.15, draw_bedded),
    recipe("karst", 4.0, [0.40, 0.39, 0.36], 0.15, draw_karst),
    recipe("foliated", 3.0, [0.21, 0.21, 0.20], 0.1, draw_foliated),
];

/// Draw every ground look for `seed`, in [`GROUND_LOOK_NAMES`] order.
#[must_use]
pub fn ground_looks(seed: u64) -> Vec<GroundLook> {
    (0..RECIPES.len())
        .filter_map(|index| ground_look(index, seed))
        .collect()
}

/// Draw look `index` of [`GROUND_LOOK_NAMES`] for `seed`: each look is
/// drawn on its own, so a viewer can draw them on several threads.
#[must_use]
pub fn ground_look(index: usize, seed: u64) -> Option<GroundLook> {
    let recipe = RECIPES.get(index)?;
    let mut canvas = Canvas::new(GROUND_LOOK_SIZE);
    (recipe.draw)(
        &mut canvas,
        hash_words(&[seed, crate::rng::hash_str(recipe.name)]),
    );
    Some(GroundLook {
        name: recipe.name,
        tile_m: recipe.tile_m,
        mean: recipe.mean,
        relief_m: recipe.relief_m,
        rgba: canvas.finish(recipe.mean),
    })
}

/// The tile, mean colour and relief of look `index` without drawing it.
#[must_use]
pub fn ground_look_size(index: usize) -> Option<(f32, [f32; 3], f32)> {
    RECIPES
        .get(index)
        .map(|recipe| (recipe.tile_m, recipe.mean, recipe.relief_m))
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

fn to_byte(value: f64) -> u8 {
    // Clamped to [0, 255] first.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = libm::round(value.clamp(0.0, 1.0) * 255.0) as u8;
    byte
}

pub(crate) fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// A numbered stream of draws: draw `i` of stream `s` is a hash of both.
#[derive(Clone, Copy)]
pub(crate) struct Draws(pub(crate) u64);

impl Draws {
    pub(crate) fn unit(self, index: u64, what: u64) -> f64 {
        unit(mix64(hash_words(&[self.0, index, what])))
    }

    pub(crate) fn range(self, index: u64, what: u64, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit(index, what)
    }

    pub(crate) fn pick<T: Copy>(self, index: u64, what: u64, items: &[T]) -> T {
        let n = items.len().max(1);
        // Below `n`, which is small.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = ((self.unit(index, what) * to_f64(n)) as usize).min(n - 1);
        items[i]
    }

    /// A whole number below `n`, which is small.
    pub(crate) fn below(self, index: u64, what: u64, n: u32) -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (self.unit(index, what) * f64::from(n)) as u64;
        i.min(u64::from(n.max(1) - 1))
    }

    pub(crate) fn stream(self, name: u64) -> Self {
        Self(hash_words(&[self.0, name]))
    }
}

/// Tileable value noise on a grid of `cells` a side over the texture, at
/// texel `(x, y)`.
pub(crate) fn value_noise(
    draws: Draws,
    cells: usize,
    octave: u64,
    x: f64,
    y: f64,
    size: usize,
) -> f64 {
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
pub(crate) fn fbm(draws: Draws, cells: usize, octaves: u32, x: f64, y: f64, size: usize) -> f64 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += weight * value_noise(draws, cells << octave, u64::from(octave), x, y, size);
        total += weight;
        weight *= 0.5;
    }
    sum / total
}

pub(crate) fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

pub(crate) fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}

/// A square, wrapping canvas of linear colour and height.
pub(crate) struct Canvas {
    pub(crate) size: usize,
    colour: Vec<[f64; 3]>,
    height: Vec<f64>,
}

impl Canvas {
    pub(crate) fn new(size: usize) -> Self {
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
    pub(crate) fn fill(&mut self, paint: impl Fn(f64, f64) -> ([f64; 3], f64)) {
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
    pub(crate) fn lay(&mut self, x: i64, y: i64, colour: [f64; 3], height: f64, cover: f64) {
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
    pub(crate) fn stroke(
        &mut self,
        a: [f64; 2],
        b: [f64; 2],
        width: f64,
        colour: [f64; 3],
        height: f64,
    ) {
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
    pub(crate) fn stroke_with(
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
    pub(crate) fn dome(
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
    pub(crate) fn stamp(
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
    pub(crate) fn stamp_worn(
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
    pub(crate) fn mean_colour(&self) -> [f64; 3] {
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
    pub(crate) fn height_span(&self) -> f64 {
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
    pub(crate) fn finish(&self, mean: [f32; 3]) -> Vec<u8> {
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
pub(crate) fn spot(draws: Draws, index: u64, size: usize) -> [f64; 2] {
    let edge = to_f64(size);
    [draws.unit(index, 1) * edge, draws.unit(index, 2) * edge]
}

/// A leaf, needle shoot or cone template of the plants' own looks.
pub(crate) fn organ_templates(shapes: &[Shape]) -> Templates {
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
        })
        .collect();
    Templates::for_looks(&looks)
}

fn draw_needles(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let humus = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(humus, 8, 4, x, y, size);
        (mix([0.07, 0.045, 0.03], [0.14, 0.09, 0.055], n), 0.15 * n)
    });
    // Fallen needles, 2.5 to 4 cm long at 341 texels a metre.
    let palette = [
        [0.36, 0.19, 0.09],
        [0.42, 0.29, 0.16],
        [0.25, 0.17, 0.11],
        [0.30, 0.22, 0.12],
        [0.27, 0.26, 0.13],
    ];
    let needles = draws.stream(2);
    let count = 14_000_u64;
    for i in 0..count {
        let a = spot(needles, i, size);
        let angle = needles.unit(i, 3) * 2.0 * PI;
        let length = needles.range(i, 4, 8.0, 14.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        let colour = scale(needles.pick(i, 5, &palette), needles.range(i, 6, 0.75, 1.2));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.25 + 0.75 * (i as f64 / count as f64);
        canvas.stroke(a, b, needles.range(i, 7, 1.0, 1.7), colour, layer);
    }
    // Twigs and a few cones.
    let twigs = draws.stream(3);
    for i in 0..10 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 50.0, 130.0);
        let bend = twigs.range(i, 5, -0.4, 0.4);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let colour = scale([0.22, 0.16, 0.11], twigs.range(i, 6, 0.8, 1.2));
        let width = twigs.range(i, 7, 2.0, 3.5);
        canvas.stroke(a, mid, width, colour, 1.15);
        canvas.stroke(mid, end, width * 0.8, colour, 1.15);
    }
    let cones = organ_templates(&[Shape::Fruit(Fruit {
        aspect: 0.45,
        cone: 1.0,
    })]);
    let spots = draws.stream(4);
    for i in 0..4 {
        canvas.stamp(
            &cones,
            0,
            spot(spots, i, size),
            spots.range(i, 3, 22.0, 30.0),
            spots.unit(i, 4) * 2.0 * PI,
            [0.36, 0.22, 0.12],
            1.3,
            0.4,
        );
    }
}

fn draw_leaves(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 8, 4, x, y, size);
        (mix([0.06, 0.04, 0.025], [0.12, 0.08, 0.045], n), 0.1 * n)
    });
    // Bigleaf maple, alder and cottonwood, and Oregon white oak: leaves
    // of 8 to 30 cm at 256 texels a metre.
    let templates = organ_templates(&[
        Shape::Palmate(Palmate::default()),
        Shape::Simple(Simple {
            width: 0.55,
            teeth: 12,
            ..Simple::default()
        }),
        Shape::Simple(Simple {
            width: 0.7,
            widest: 0.35,
            ..Simple::default()
        }),
        Shape::Lobed(Lobed::default()),
    ]);
    let lengths = [[45.0, 75.0], [18.0, 28.0], [22.0, 34.0], [26.0, 40.0]];
    let palette = [
        [0.48, 0.32, 0.14],
        [0.33, 0.19, 0.08],
        [0.52, 0.38, 0.11],
        [0.20, 0.12, 0.06],
        [0.40, 0.25, 0.10],
    ];
    let leaves = draws.stream(2);
    let count = 2_600_u64;
    for i in 0..count {
        let template = leaves.pick(i, 3, &[0_usize, 0, 1, 1, 1, 2, 2, 3]);
        let [low, high] = lengths[template];
        let colour = scale(leaves.pick(i, 5, &palette), leaves.range(i, 6, 0.75, 1.15));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.2 + 0.8 * (i as f64 / count as f64);
        canvas.stamp(
            &templates,
            template,
            spot(leaves, i, size),
            leaves.range(i, 4, low, high),
            leaves.unit(i, 7) * 2.0 * PI,
            colour,
            layer,
            leaves.range(i, 8, 0.0, 0.15),
        );
    }
    let twigs = draws.stream(3);
    for i in 0..6 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 40.0, 110.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        canvas.stroke(a, b, twigs.range(i, 5, 1.5, 3.0), [0.20, 0.15, 0.10], 1.1);
    }
}

fn draw_moss(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 3, x, y, size);
        (mix([0.05, 0.06, 0.02], [0.09, 0.10, 0.04], n), 0.0)
    });
    // Cushions 3 to 10 cm across at 512 texels a metre, each a dome of
    // fine shoots from yellow-green to deep green.
    let cushions = draws.stream(2);
    let shoots = draws.stream(3);
    let greens = [
        [0.13, 0.25, 0.05],
        [0.20, 0.30, 0.06],
        [0.10, 0.19, 0.05],
        [0.17, 0.24, 0.05],
    ];
    for i in 0..420 {
        let c = spot(cushions, i, size);
        let radius = cushions.range(i, 3, 12.0, 32.0);
        let green = cushions.pick(i, 4, &greens);
        let base = cushions.range(i, 5, 0.0, 0.3);
        canvas.dome(
            c,
            radius,
            radius * cushions.range(i, 6, 0.7, 1.0),
            cushions.unit(i, 7) * PI,
            base,
            0.7,
            |u, v, dome| {
                // Shoot tips speckle the cushion; its top catches the light.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 40.0 + 100.0) as u64, (v * 40.0 + 100.0) as u64);
                let speckle = shoots.unit(ix * 1000 + iy, i);
                scale(green, (0.55 + 0.5 * dome) * (0.8 + 0.4 * speckle))
            },
        );
    }
}

fn draw_grass(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let thatch = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(thatch, 8, 4, x, y, size);
        (mix([0.10, 0.09, 0.05], [0.24, 0.20, 0.10], n), 0.1 * n)
    });
    // Tufts seen from above: blades 3 to 8 cm long at 512 texels a metre
    // spreading from each tuft's crown, green to straw.
    let tufts = draws.stream(2);
    let blades = draws.stream(3);
    let colours = [
        [0.16, 0.32, 0.08],
        [0.22, 0.38, 0.10],
        [0.30, 0.36, 0.13],
        [0.42, 0.38, 0.18],
        [0.12, 0.26, 0.07],
    ];
    let mut blade = 0_u64;
    for i in 0..260 {
        let c = spot(tufts, i, size);
        let count = 18 + tufts.below(i, 3, 26);
        let lean = tufts.unit(i, 4) * 2.0 * PI;
        for _ in 0..count {
            blade += 1;
            let angle = lean + blades.range(blade, 1, -1.6, 1.6) + blades.unit(blade, 6) * PI;
            let length = blades.range(blade, 2, 15.0, 42.0);
            let start = [
                c[0] + blades.range(blade, 3, -4.0, 4.0),
                c[1] + blades.range(blade, 4, -4.0, 4.0),
            ];
            let end = [
                start[0] + length * math::cos(angle),
                start[1] + length * math::sin(angle),
            ];
            let colour = scale(
                blades.pick(blade, 5, &colours),
                blades.range(blade, 7, 0.8, 1.2),
            );
            // Blades rise from the crown: higher near it.
            let mid = [
                f64::midpoint(start[0], end[0]),
                f64::midpoint(start[1], end[1]),
            ];
            canvas.stroke(start, mid, 2.2, colour, 0.95);
            canvas.stroke(mid, end, 1.6, colour, 0.7);
        }
    }
}

fn draw_soil(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    let grain = draws.stream(2);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 5, x, y, size);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let colour = mix([0.24, 0.18, 0.12], [0.40, 0.33, 0.24], n);
        (scale(colour, 0.85 + 0.3 * speck), 0.4 * n + 0.05 * speck)
    });
    // Crumbs and clods 1 to 3 cm across at 256 texels a metre, a few
    // pebbles, and dark bits of rotting wood and roots.
    let clods = draws.stream(3);
    for i in 0..900 {
        let c = spot(clods, i, size);
        let radius = clods.range(i, 3, 1.5, 4.5);
        let colour = scale([0.36, 0.29, 0.20], clods.range(i, 4, 0.8, 1.2));
        canvas.dome(
            c,
            radius,
            radius * 0.8,
            clods.unit(i, 5) * PI,
            0.4,
            0.4,
            |_, _, dome| scale(colour, 0.75 + 0.4 * dome),
        );
    }
    let pebbles = draws.stream(4);
    for i in 0..40 {
        let c = spot(pebbles, i, size);
        let radius = pebbles.range(i, 3, 2.5, 6.0);
        let colour = scale([0.42, 0.40, 0.36], pebbles.range(i, 4, 0.7, 1.2));
        canvas.dome(
            c,
            radius,
            radius * 0.75,
            pebbles.unit(i, 5) * PI,
            0.45,
            0.5,
            |_, _, dome| scale(colour, 0.7 + 0.45 * dome),
        );
    }
    let bits = draws.stream(5);
    for i in 0..60 {
        let a = spot(bits, i, size);
        let angle = bits.unit(i, 3) * 2.0 * PI;
        let length = bits.range(i, 4, 4.0, 14.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        canvas.stroke(a, b, bits.range(i, 5, 1.0, 2.0), [0.10, 0.07, 0.05], 0.9);
    }
}

fn draw_sand(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let warp = draws.stream(1);
    let grain = draws.stream(2);
    let tone = draws.stream(3);
    // Wind or water ripples about 8 cm apart at 512 texels a metre,
    // wandering with the noise, over grains of every shade.
    canvas.fill(|x, y| {
        let w = fbm(warp, 4, 3, x, y, size);
        let phase = (x + 0.25 * y) / to_f64(size) * 12.0 + 3.0 * w;
        let ripple = 0.5 + 0.5 * math::sin(2.0 * PI * phase);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let shade = fbm(tone, 6, 3, x, y, size);
        let colour = mix([0.50, 0.44, 0.32], [0.64, 0.58, 0.44], shade);
        let grainy = if speck > 0.97 {
            [0.20, 0.18, 0.16]
        } else {
            scale(colour, 0.88 + 0.24 * speck)
        };
        (
            scale(grainy, 0.92 + 0.12 * ripple),
            0.6 * ripple + 0.1 * speck,
        )
    });
    let shells = draws.stream(4);
    for i in 0..25 {
        let c = spot(shells, i, size);
        let radius = shells.range(i, 3, 2.0, 5.0);
        canvas.dome(
            c,
            radius,
            radius * 0.6,
            shells.unit(i, 4) * PI,
            0.6,
            0.3,
            |_, _, dome| scale([0.75, 0.72, 0.66], 0.8 + 0.25 * dome),
        );
    }
}

fn draw_gravel(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let sand = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(sand, 8, 3, x, y, size);
        (mix([0.18, 0.16, 0.13], [0.30, 0.27, 0.22], n), 0.0)
    });
    // Rounded stones 1 to 5 cm across at 512 texels a metre, of mixed
    // rock: grey basalt, brown sandstone, pale quartz and rusty stones.
    let stones = draws.stream(2);
    let colours = [
        [0.38, 0.37, 0.35],
        [0.30, 0.29, 0.28],
        [0.50, 0.44, 0.34],
        [0.62, 0.60, 0.56],
        [0.46, 0.33, 0.22],
        [0.24, 0.24, 0.23],
    ];
    for i in 0..2_600 {
        let c = spot(stones, i, size);
        let radius = stones.range(i, 3, 3.0, 4.0) * (1.0 + 2.2 * libm::pow(stones.unit(i, 8), 3.0));
        let colour = scale(stones.pick(i, 4, &colours), stones.range(i, 5, 0.8, 1.15));
        let speckled = stones.unit(i, 9) > 0.6;
        canvas.dome(
            c,
            radius,
            radius * stones.range(i, 6, 0.6, 0.95),
            stones.unit(i, 7) * PI,
            0.1,
            0.5 + 0.08 * radius,
            |u, v, dome| {
                let speck = if speckled {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let cell = ((u * 6.0 + 10.0) as u64) * 100 + (v * 6.0 + 10.0) as u64;
                    0.85 + 0.3 * stones.unit(cell, i)
                } else {
                    1.0
                };
                scale(colour, (0.6 + 0.5 * dome) * speck)
            },
        );
    }
}

fn draw_massive(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let body = draws.stream(1);
    let strata = draws.stream(2);
    let cracks = draws.stream(3);
    let lichen = draws.stream(4);
    // Weathered rock at 128 texels a metre: broad tones, faint layering,
    // a net of cracks and crusts of lichen.
    canvas.fill(|x, y| {
        let n = fbm(body, 4, 6, x, y, size);
        let layer = fbm(strata, 3, 3, x, y, size);
        let bands = 0.5 + 0.5 * math::sin(2.0 * PI * (y / to_f64(size) * 9.0 + 2.5 * layer));
        // Cracks: where the ridged noise is near its fold.
        let ridge = 1.0 - (2.0 * fbm(cracks, 6, 4, x, y, size) - 1.0).abs();
        let crack = ((ridge - 0.9) / 0.1).clamp(0.0, 1.0);
        let base = mix([0.30, 0.29, 0.27], [0.50, 0.48, 0.44], n);
        let mut colour = scale(base, 0.9 + 0.15 * bands);
        colour = mix(colour, [0.12, 0.12, 0.11], crack * 0.8);
        let crust = fbm(lichen, 10, 4, x, y, size);
        if crust > 0.62 {
            let pale = if fbm(lichen, 5, 2, y, x, size) > 0.5 {
                [0.55, 0.57, 0.48]
            } else {
                [0.52, 0.42, 0.24]
            };
            colour = mix(colour, pale, ((crust - 0.62) / 0.08).clamp(0.0, 1.0) * 0.7);
        }
        (colour, 0.7 * n + 0.15 * bands - 0.5 * crack)
    });
}

/// Tileable cellular noise over `cells` by `cells` cells, each with one
/// point at a random place: at texel `(x, y)`, the distances in texels to
/// the nearest point and the second nearest, and the nearest point's
/// cell. Half the difference of the two distances is about the distance
/// to the border between two cells: the cracks of mud, the ridges of a
/// salt crust, the rings of sorted stones.
#[allow(clippy::many_single_char_names)] // Points of the plane.
fn cellular(draws: Draws, cells: usize, x: f64, y: f64, size: usize) -> (f64, f64, u64) {
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
fn ridged(draws: Draws, cells: usize, octaves: u32, x: f64, y: f64, size: usize) -> f64 {
    1.0 - (2.0 * fbm(draws, cells, octaves, x, y, size) - 1.0).abs()
}

impl Canvas {
    /// An angular stone centred at `c`: a polygon of `sides` corners
    /// about `radius` texels out, turned by `angle`, whose top is a low
    /// pyramid of facets `height` above `base` sloping to its edges, each
    /// facet lit by light from the north-west and tinted by a number of
    /// its own.
    #[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
    fn block(
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
pub(crate) fn power_size(
    draws: Draws,
    index: u64,
    what: u64,
    low: f64,
    high: f64,
    exponent: f64,
) -> f64 {
    let u = draws.unit(index, what);
    (low * libm::pow(1.0 - 0.98 * u, -1.0 / exponent)).min(high)
}

fn draw_thatch(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 8, 4, x, y, size);
        (mix([0.09, 0.075, 0.05], [0.18, 0.15, 0.10], n), 0.1 * n)
    });
    // Blades 10 to 25 cm long at 341 texels a metre, laid flat by snow
    // and wind in swirls that follow a slowly turning direction.
    let flow = draws.stream(2);
    let blades = draws.stream(3);
    let colours = [
        [0.62, 0.55, 0.36],
        [0.55, 0.47, 0.30],
        [0.70, 0.64, 0.46],
        [0.46, 0.40, 0.28],
        [0.40, 0.42, 0.26],
    ];
    let count = 5_200_u64;
    for i in 0..count {
        let a = spot(blades, i, size);
        let swirl = fbm(flow, 3, 2, a[0], a[1], size) * 4.0 * PI;
        let angle = swirl + blades.range(i, 3, -0.5, 0.5);
        let length = blades.range(i, 4, 34.0, 85.0);
        let bend = blades.range(i, 5, -0.25, 0.25);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let colour = scale(blades.pick(i, 6, &colours), blades.range(i, 7, 0.8, 1.15));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.3 + 0.7 * (i as f64 / count as f64);
        let width = blades.range(i, 8, 1.4, 2.6);
        canvas.stroke(a, mid, width, colour, layer);
        canvas.stroke(mid, end, width * 0.8, colour, layer);
    }
}

fn draw_twigs(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let dust = draws.stream(1);
    let grain = draws.stream(2);
    canvas.fill(|x, y| {
        let n = fbm(dust, 6, 4, x, y, size);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let colour = mix([0.36, 0.31, 0.24], [0.50, 0.44, 0.35], n);
        (scale(colour, 0.9 + 0.2 * speck), 0.2 * n)
    });
    // Small grey leaves of sagebrush and blackbrush, 1 to 3 cm at 256
    // texels a metre, flakes of shredded bark and twigs.
    let leaves = organ_templates(&[Shape::Simple(Simple {
        width: 0.4,
        ..Simple::default()
    })]);
    let fall = draws.stream(3);
    for i in 0..700 {
        canvas.stamp(
            &leaves,
            0,
            spot(fall, i, size),
            fall.range(i, 3, 3.0, 8.0),
            fall.unit(i, 4) * 2.0 * PI,
            scale([0.42, 0.42, 0.34], fall.range(i, 5, 0.7, 1.15)),
            fall.range(i, 6, 0.3, 0.8),
            0.05,
        );
    }
    let bark = draws.stream(4);
    for i in 0..260 {
        let c = spot(bark, i, size);
        let radius = bark.range(i, 3, 2.0, 6.0);
        let colour = scale([0.30, 0.24, 0.18], bark.range(i, 4, 0.7, 1.2));
        canvas.dome(
            c,
            radius,
            radius * 0.35,
            bark.unit(i, 5) * PI,
            0.35,
            0.25,
            |_, _, dome| scale(colour, 0.75 + 0.35 * dome),
        );
    }
    let twigs = draws.stream(5);
    for i in 0..380 {
        let a = spot(twigs, i, size);
        let angle = twigs.unit(i, 3) * 2.0 * PI;
        let length = twigs.range(i, 4, 12.0, 60.0);
        let bend = twigs.range(i, 5, -0.6, 0.6);
        let mid = [
            a[0] + 0.5 * length * math::cos(angle),
            a[1] + 0.5 * length * math::sin(angle),
        ];
        let end = [
            mid[0] + 0.5 * length * math::cos(angle + bend),
            mid[1] + 0.5 * length * math::sin(angle + bend),
        ];
        let grey = twigs.pick(
            i,
            6,
            &[[0.36, 0.33, 0.28], [0.26, 0.21, 0.16], [0.45, 0.42, 0.37]],
        );
        let colour = scale(grey, twigs.range(i, 7, 0.8, 1.2));
        let width = twigs.range(i, 8, 1.2, 2.8);
        canvas.stroke(a, mid, width, colour, 1.0);
        canvas.stroke(mid, end, width * 0.75, colour, 1.0);
    }
}

fn draw_sphagnum(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let water = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(water, 6, 3, x, y, size);
        (mix([0.03, 0.04, 0.02], [0.07, 0.07, 0.03], n), 0.0)
    });
    // Capitula, the star-shaped heads of the shoots, 1 to 2 cm across at
    // 512 texels a metre, packed into a carpet whose colour wanders from
    // green to ochre and red.
    let heads = draws.stream(2);
    let hue = draws.stream(3);
    let greens = [0.20, 0.30, 0.08];
    let ochres = [0.42, 0.34, 0.10];
    let reds = [0.40, 0.14, 0.10];
    for i in 0..5_200 {
        let c = spot(heads, i, size);
        let radius = heads.range(i, 3, 4.0, 9.0);
        let h = fbm(hue, 3, 3, c[0], c[1], size);
        let base = if h < 0.45 {
            mix(greens, ochres, h / 0.45)
        } else {
            mix(ochres, reds, ((h - 0.45) / 0.4).min(1.0))
        };
        let colour = scale(base, heads.range(i, 4, 0.75, 1.2));
        let turn = heads.unit(i, 5) * 2.0 * PI;
        canvas.dome(
            c,
            radius,
            radius,
            0.0,
            heads.range(i, 6, 0.0, 0.4),
            0.6,
            |u, v, dome| {
                // Five branch tufts radiate from the head's bud.
                let star = 0.5 + 0.5 * math::cos(5.0 * (libm::atan2(v, u) + turn));
                scale(colour, (0.55 + 0.45 * dome) * (0.75 + 0.35 * star))
            },
        );
    }
}

fn draw_lichen(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(ground, 8, 3, x, y, size);
        (mix([0.05, 0.05, 0.035], [0.12, 0.11, 0.08], n), 0.0)
    });
    // Reindeer lichen: clumps 2 to 6 cm across of fine, hollow branches,
    // pale grey-green, some yellowish, with dark gaps between the tips.
    let clumps = draws.stream(2);
    let tips = draws.stream(3);
    let palette = [
        [0.62, 0.64, 0.52],
        [0.70, 0.70, 0.56],
        [0.58, 0.62, 0.44],
        [0.75, 0.74, 0.62],
    ];
    for i in 0..420 {
        let c = spot(clumps, i, size);
        let radius = clumps.range(i, 3, 10.0, 30.0);
        let colour = scale(clumps.pick(i, 4, &palette), clumps.range(i, 5, 0.85, 1.1));
        canvas.dome(
            c,
            radius,
            radius * clumps.range(i, 6, 0.75, 1.0),
            clumps.unit(i, 7) * PI,
            clumps.range(i, 8, 0.0, 0.3),
            0.7,
            |u, v, dome| {
                // Branch tips: a fine lattice of light and shadow.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 6.0 + 50.0) as u64, (v * 6.0 + 50.0) as u64);
                let tip = tips.unit(ix * 1000 + iy, i);
                let gap = if tip < 0.12 { 0.55 } else { 1.0 };
                scale(colour, (0.6 + 0.4 * dome) * gap * (0.85 + 0.3 * tip))
            },
        );
    }
}

fn draw_crust(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    let patches = draws.stream(2);
    let grain = draws.stream(3);
    canvas.fill(|x, y| {
        let n = fbm(soil, 8, 3, x, y, size);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let colour = mix([0.42, 0.35, 0.26], [0.55, 0.46, 0.34], n);
        (scale(colour, 0.9 + 0.2 * speck), 0.1 * n)
    });
    // Pinnacles of cyanobacteria, lichen and moss 4 to 12 mm across at
    // 512 texels a metre, black and dark brown, gathered in patches with
    // pale soil between, a few green moss tufts and orange lichen.
    let knobs = draws.stream(4);
    let colours = [
        [0.07, 0.06, 0.05],
        [0.14, 0.11, 0.08],
        [0.10, 0.09, 0.07],
        [0.20, 0.16, 0.11],
    ];
    let mut placed = 0_u64;
    let mut i = 0_u64;
    while placed < 9_000 && i < 40_000 {
        i += 1;
        let c = spot(knobs, i, size);
        if fbm(patches, 5, 3, c[0], c[1], size) < knobs.unit(i, 9) * 0.75 {
            continue;
        }
        placed += 1;
        let radius = knobs.range(i, 3, 2.0, 6.0);
        let lichen = knobs.unit(i, 4);
        let colour = if lichen > 0.97 {
            [0.62, 0.36, 0.10]
        } else if lichen > 0.93 {
            [0.16, 0.24, 0.07]
        } else {
            scale(knobs.pick(i, 5, &colours), knobs.range(i, 6, 0.8, 1.2))
        };
        canvas.dome(
            c,
            radius,
            radius * knobs.range(i, 7, 0.7, 1.0),
            knobs.unit(i, 8) * PI,
            0.2,
            0.8,
            |_, _, dome| scale(colour, 0.6 + 0.6 * dome),
        );
    }
}

fn draw_tussock(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let soil = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(soil, 6, 4, x, y, size);
        (mix([0.10, 0.08, 0.055], [0.22, 0.18, 0.12], n), 0.05 * n)
    });
    // Bunchgrass and sedge tussocks 20 to 45 cm across at 256 texels a
    // metre: dense crowns of fine blades, straw outside and green at the
    // heart, with bare ground and a little thatch between.
    let litter = draws.stream(2);
    for i in 0..900 {
        let a = spot(litter, i, size);
        let angle = litter.unit(i, 3) * 2.0 * PI;
        let length = litter.range(i, 4, 10.0, 30.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        canvas.stroke(
            a,
            b,
            1.2,
            scale([0.48, 0.42, 0.28], litter.range(i, 5, 0.7, 1.1)),
            0.1,
        );
    }
    let tussocks = draws.stream(3);
    let blades = draws.stream(4);
    let straw = [[0.62, 0.55, 0.36], [0.55, 0.48, 0.32], [0.70, 0.62, 0.42]];
    let mut blade = 0_u64;
    for i in 0..30 {
        let c = spot(tussocks, i, size);
        let radius = tussocks.range(i, 3, 26.0, 58.0);
        let green = tussocks.range(i, 4, 0.0, 0.5);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = (radius * 6.0) as u64;
        for _ in 0..count {
            blade += 1;
            let angle = blades.unit(blade, 1) * 2.0 * PI;
            let start_r = blades.range(blade, 2, 0.0, 0.35) * radius;
            let start = [
                c[0] + start_r * math::cos(angle),
                c[1] + start_r * math::sin(angle),
            ];
            let length = radius * blades.range(blade, 3, 0.7, 1.25);
            let lean = angle + blades.range(blade, 4, -0.3, 0.3);
            let end = [
                start[0] + length * math::cos(lean),
                start[1] + length * math::sin(lean),
            ];
            let heart = 1.0 - start_r / radius;
            let base = blades.pick(blade, 5, &straw);
            let colour = mix(base, [0.20, 0.32, 0.10], green * heart);
            let colour = scale(colour, blades.range(blade, 6, 0.75, 1.15));
            let mid = [
                f64::midpoint(start[0], end[0]),
                f64::midpoint(start[1], end[1]),
            ];
            canvas.stroke(start, mid, 1.6, colour, 0.6 + 0.4 * heart);
            canvas.stroke(mid, end, 1.2, colour, 0.35 + 0.3 * heart);
        }
    }
}

fn draw_cushion(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 3, x, y, size);
        (mix([0.05, 0.045, 0.03], [0.12, 0.10, 0.07], n), 0.0)
    });
    // Cushion plants packed into firm mounds 3 to 12 cm across at 512
    // texels a metre: tiny rosettes of leaves, bright green, starred
    // with flowers here and there.
    let mounds = draws.stream(2);
    let rosettes = draws.stream(3);
    let greens = [
        [0.10, 0.26, 0.06],
        [0.14, 0.30, 0.07],
        [0.09, 0.22, 0.06],
        [0.16, 0.28, 0.09],
    ];
    for i in 0..700 {
        let c = spot(mounds, i, size);
        let radius = mounds.range(i, 3, 15.0, 60.0);
        let green = scale(mounds.pick(i, 4, &greens), mounds.range(i, 5, 0.8, 1.15));
        let flowers = mounds.unit(i, 6) > 0.8;
        canvas.dome(
            c,
            radius,
            radius * mounds.range(i, 7, 0.8, 1.0),
            mounds.unit(i, 8) * PI,
            0.0,
            1.0,
            |u, v, dome| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 14.0 + 50.0) as u64, (v * 14.0 + 50.0) as u64);
                let rosette = rosettes.unit(ix * 1000 + iy, i);
                if flowers && rosette > 0.93 {
                    return [0.85, 0.80, 0.86];
                }
                scale(green, (0.55 + 0.5 * dome) * (0.8 + 0.35 * rosette))
            },
        );
    }
}

fn draw_peat(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    let hollows = draws.stream(3);
    let wet = |x: f64, y: f64| ((fbm(hollows, 5, 3, x, y, size) - 0.6) / 0.1).clamp(0.0, 1.0);
    canvas.fill(|x, y| {
        let n = fbm(ground, 6, 4, x, y, size);
        let colour = mix([0.04, 0.03, 0.022], [0.09, 0.065, 0.045], n);
        (mix(colour, [0.025, 0.02, 0.016], 0.7 * wet(x, y)), 0.3 * n)
    });
    // Fibres of half-rotted sedge and moss, 1 to 6 cm at 341 texels a
    // metre, dark brown to rust, fewer in the wet hollows.
    let fibres = draws.stream(2);
    let colours = [
        [0.12, 0.08, 0.05],
        [0.18, 0.10, 0.06],
        [0.08, 0.06, 0.04],
        [0.22, 0.15, 0.08],
    ];
    for i in 0..6_000_u64 {
        let a = spot(fibres, i, size);
        if wet(a[0], a[1]) > fibres.unit(i, 9) {
            continue;
        }
        let angle = fibres.unit(i, 3) * 2.0 * PI;
        let length = fibres.range(i, 4, 4.0, 20.0);
        let b = [
            a[0] + length * math::cos(angle),
            a[1] + length * math::sin(angle),
        ];
        let colour = scale(fibres.pick(i, 5, &colours), fibres.range(i, 6, 0.7, 1.3));
        #[allow(clippy::cast_precision_loss)]
        let layer = 0.35 + 0.6 * (i as f64 / 6_000.0);
        canvas.stroke(a, b, fibres.range(i, 7, 0.8, 1.6), colour, layer);
    }
}

fn draw_clay(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let cells = draws.stream(1);
    let fine = draws.stream(2);
    let tone = draws.stream(3);
    // Mud cracked as it dried into polygons 25 to 40 cm across at 256
    // texels a metre, each split again by finer cracks, their edges
    // curling up and paler where they dried first.
    canvas.fill(|x, y| {
        let (d1, d2, cell) = cellular(cells, 6, x, y, size);
        let edge = 0.5 * (d2 - d1);
        let (e1, e2, _) = cellular(fine, 15, x, y, size);
        let fine_edge = 0.5 * (e2 - e1);
        let crack = 1.0 - ((edge - 1.2) / 1.5).clamp(0.0, 1.0);
        let fine_crack = 0.6 * (1.0 - ((fine_edge - 0.6) / 1.0).clamp(0.0, 1.0));
        let curl = 1.0 - (edge / 18.0).clamp(0.0, 1.0);
        let shade = cells.unit(cell, 5);
        let n = fbm(tone, 8, 3, x, y, size);
        let plate = mix(
            [0.42, 0.37, 0.30],
            [0.55, 0.50, 0.42],
            0.6 * n + 0.4 * shade,
        );
        let plate = scale(plate, 1.0 + 0.12 * curl);
        let colour = mix(plate, [0.12, 0.10, 0.08], crack.max(fine_crack));
        let _ = d1;
        (colour, 0.6 + 0.3 * curl - 0.9 * crack - 0.3 * fine_crack)
    });
}

fn draw_ash(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let ground = draws.stream(1);
    let grain = draws.stream(2);
    canvas.fill(|x, y| {
        let n = fbm(ground, 8, 4, x, y, size);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let colour = mix([0.10, 0.10, 0.10], [0.20, 0.19, 0.18], n);
        (scale(colour, 0.75 + 0.5 * speck), 0.2 * n + 0.1 * speck)
    });
    // Cinders 1 to 6 cm at 341 texels a metre: porous, black and
    // oxidised red-brown, their pores dark.
    let cinders = draws.stream(3);
    let pores = draws.stream(4);
    let colours = [
        [0.08, 0.075, 0.07],
        [0.22, 0.12, 0.08],
        [0.14, 0.12, 0.11],
        [0.30, 0.16, 0.10],
    ];
    for i in 0..1_600_u64 {
        let c = spot(cinders, i, size);
        let radius = power_size(cinders, i, 3, 2.0, 10.0, 1.8);
        let colour = scale(cinders.pick(i, 4, &colours), cinders.range(i, 5, 0.8, 1.2));
        canvas.dome(
            c,
            radius,
            radius * cinders.range(i, 6, 0.7, 1.0),
            cinders.unit(i, 7) * PI,
            0.3,
            0.5 + 0.05 * radius,
            |u, v, dome| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (ix, iy) = ((u * 5.0 + 20.0) as u64, (v * 5.0 + 20.0) as u64);
                let pore = if pores.unit(ix * 100 + iy, i) > 0.7 {
                    0.45
                } else {
                    1.0
                };
                scale(colour, (0.6 + 0.5 * dome) * pore)
            },
        );
    }
}

fn draw_salt(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let cells = draws.stream(1);
    let dirt = draws.stream(2);
    let grain = draws.stream(3);
    // Salt crust at 171 texels a metre: polygons about 60 cm across whose
    // edges thrust up into ridges as the salt grows, brown dust in the
    // hollows and a glitter of crystals.
    canvas.fill(|x, y| {
        let (d1, d2, cell) = cellular(cells, 5, x, y, size);
        let edge = 0.5 * (d2 - d1);
        let ridge = 1.0 - (edge / 6.0).clamp(0.0, 1.0);
        let hollow = (d1 / 40.0).clamp(0.0, 1.0);
        let dust = fbm(dirt, 6, 4, x, y, size) * (1.0 - ridge) * hollow;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let speck = grain.unit((x as u64) * 1024 + y as u64, 0);
        let white = scale([0.86, 0.84, 0.80], 0.94 + 0.06 * cells.unit(cell, 4));
        let colour = mix(white, [0.55, 0.47, 0.38], 0.55 * dust);
        let colour = scale(
            colour,
            if speck > 0.985 {
                1.12
            } else {
                0.97 + 0.05 * speck
            },
        );
        (colour, 0.3 + 0.7 * ridge * ridge - 0.2 * dust)
    });
}

fn draw_ice(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let body = draws.stream(1);
    let cracks = draws.stream(2);
    let bands = draws.stream(3);
    let snow = draws.stream(4);
    // Glacier ice at 128 texels a metre: blue-white, crossed by cracks,
    // with bands of dust and patches of firn.
    canvas.fill(|x, y| {
        let n = fbm(body, 4, 5, x, y, size);
        let crack = ((ridged(cracks, 4, 4, x, y, size) - 0.93) / 0.07).clamp(0.0, 1.0);
        let warp = fbm(bands, 3, 3, x, y, size);
        let band = math::sin(2.0 * PI * (y / to_f64(size) * 3.0 + 1.5 * warp));
        let dust = ((band - 0.92) / 0.08).clamp(0.0, 1.0);
        let firn = ((fbm(snow, 5, 4, x, y, size) - 0.55) / 0.15).clamp(0.0, 1.0);
        let blue = mix([0.60, 0.72, 0.82], [0.80, 0.86, 0.90], n);
        let mut colour = mix(blue, [0.35, 0.33, 0.30], 0.6 * dust);
        colour = mix(colour, [0.40, 0.55, 0.70], 0.7 * crack);
        colour = mix(colour, [0.92, 0.93, 0.94], firn);
        (colour, 0.5 * n - 0.6 * crack + 0.4 * firn)
    });
    let bubbles = draws.stream(5);
    for i in 0..300 {
        let c = spot(bubbles, i, size);
        let radius = bubbles.range(i, 3, 0.8, 2.5);
        canvas.dome(c, radius, radius, 0.0, 0.6, 0.1, |_, _, dome| {
            scale([0.92, 0.95, 0.97], 0.9 + 0.1 * dome)
        });
    }
}

fn draw_scree(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let grit = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(grit, 10, 3, x, y, size);
        (mix([0.10, 0.10, 0.095], [0.20, 0.19, 0.18], n), 0.0)
    });
    // Angular rock fragments 3 to 25 cm across at 256 texels a metre,
    // jumbled together, the bigger ones on top, of one rock with a few
    // weathered and lichen-stained.
    let stones = draws.stream(2);
    let greys = [
        [0.42, 0.41, 0.39],
        [0.50, 0.48, 0.45],
        [0.36, 0.35, 0.34],
        [0.56, 0.54, 0.50],
    ];
    let count = 2_400_u64;
    for i in 0..count {
        let c = spot(stones, i, size);
        let radius = power_size(stones, i, 3, 4.0, 32.0, 1.4);
        let stain = stones.unit(i, 4);
        let colour = if stain > 0.94 {
            [0.55, 0.50, 0.32]
        } else {
            scale(stones.pick(i, 5, &greys), stones.range(i, 6, 0.8, 1.15))
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sides = 4 + (stones.unit(i, 7) * 4.0) as u64;
        canvas.block(
            stones,
            i,
            c,
            radius,
            sides,
            stones.unit(i, 8) * PI,
            0.05 + 0.6 * radius / 32.0,
            0.25 + 0.4 * radius / 32.0,
            colour,
        );
    }
}

fn draw_pavement(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let silt = draws.stream(1);
    canvas.fill(|x, y| {
        let n = fbm(silt, 8, 3, x, y, size);
        (mix([0.40, 0.35, 0.28], [0.52, 0.46, 0.37], n), 0.0)
    });
    // Desert pavement at 512 texels a metre: pebbles 1 to 3 cm across
    // fitted together over the silt they rose from, varnished brown and
    // black on top, with a glint of their polish.
    let pebbles = draws.stream(2);
    let colours = [
        [0.16, 0.10, 0.07],
        [0.10, 0.08, 0.07],
        [0.24, 0.15, 0.09],
        [0.30, 0.22, 0.15],
    ];
    for i in 0..5_500_u64 {
        let c = spot(pebbles, i, size);
        let radius =
            pebbles.range(i, 3, 4.0, 9.0) * (1.0 + 0.6 * libm::pow(pebbles.unit(i, 4), 3.0));
        let colour = scale(pebbles.pick(i, 5, &colours), pebbles.range(i, 6, 0.8, 1.2));
        canvas.dome(
            c,
            radius,
            radius * pebbles.range(i, 7, 0.65, 0.95),
            pebbles.unit(i, 8) * PI,
            0.2,
            0.6,
            |u, v, dome| {
                // A highlight toward the north-west on the polished tops.
                let glint = (1.0 - libm::hypot(u + 0.35, v + 0.35) / 0.35).clamp(0.0, 1.0);
                mix(
                    scale(colour, 0.65 + 0.45 * dome),
                    [0.55, 0.50, 0.44],
                    0.5 * glint,
                )
            },
        );
    }
}

fn draw_sorted(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let cells = draws.stream(1);
    let soil = draws.stream(2);
    // Frost-sorted circles at 171 texels a metre: frost heave pushes the
    // stones out of rings of fine soil about a metre across into borders
    // of gravel and cobbles. The soil domes up in the middle.
    canvas.fill(|x, y| {
        let (d1, d2, _) = cellular(cells, 3, x, y, size);
        let edge = 0.5 * (d2 - d1);
        let middle = (edge / 40.0).clamp(0.0, 1.0);
        let n = fbm(soil, 8, 4, x, y, size);
        let colour = mix([0.24, 0.20, 0.15], [0.36, 0.31, 0.24], n);
        let _ = d1;
        (colour, 0.25 * middle)
    });
    let stones = draws.stream(3);
    let greys = [
        [0.44, 0.42, 0.39],
        [0.36, 0.35, 0.33],
        [0.52, 0.50, 0.46],
        [0.40, 0.36, 0.30],
    ];
    let mut placed = 0_u64;
    let mut i = 0_u64;
    while placed < 2_600 && i < 30_000 {
        i += 1;
        let c = spot(stones, i, size);
        let (d1, d2, _) = cellular(cells, 3, c[0], c[1], size);
        let edge = 0.5 * (d2 - d1);
        // Stones gather where the border is, few in the middle.
        let keep = libm::exp(-edge / 9.0) + 0.03;
        if stones.unit(i, 9) > keep {
            continue;
        }
        placed += 1;
        let radius = power_size(stones, i, 3, 3.0, 14.0, 1.6);
        let colour = scale(stones.pick(i, 4, &greys), stones.range(i, 5, 0.8, 1.2));
        let lichen = stones.unit(i, 6) > 0.85;
        canvas.dome(
            c,
            radius,
            radius * stones.range(i, 7, 0.65, 0.95),
            stones.unit(i, 8) * PI,
            0.2,
            0.5 + 0.04 * radius,
            |_, _, dome| {
                let stone = scale(colour, 0.6 + 0.5 * dome);
                if lichen && dome > 0.6 {
                    mix(stone, [0.62, 0.62, 0.50], 0.6)
                } else {
                    stone
                }
            },
        );
    }
}

fn draw_bedded(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let warp = draws.stream(1);
    let beds = draws.stream(2);
    let laminae = draws.stream(3);
    let pits = draws.stream(4);
    let joints = draws.stream(5);
    let tones = [
        [0.52, 0.46, 0.36],
        [0.44, 0.36, 0.28],
        [0.58, 0.54, 0.46],
        [0.40, 0.38, 0.35],
        [0.50, 0.40, 0.30],
    ];
    // Sandstone and shale at 128 texels a metre: beds 10 to 60 cm thick of
    // their own tone and hardness, cross-bedded laminae in some, pits
    // where the cement weathers out and a few joints across.
    canvas.fill(|x, y| {
        let w = fbm(warp, 3, 3, x, y, size);
        let depth = y + 40.0 * (w - 0.5);
        // Beds of varying thickness: each 64-texel band split by the hash.
        #[allow(clippy::cast_possible_truncation)]
        let band = libm::floor(depth / 32.0) as i64;
        #[allow(clippy::cast_sign_loss)]
        let id = band.rem_euclid(16) as u64;
        let within = depth / 32.0 - libm::floor(depth / 32.0);
        let tone = beds.pick(id, 1, &tones);
        let hard = beds.unit(id, 2);
        let cross = beds.unit(id, 3) > 0.5;
        // Laminae with whole numbers of cycles across the texture, so they
        // wrap: slanting in cross-bedded beds, level in the rest.
        let edge = to_f64(size);
        let lamina = if cross {
            let along = (34.0 * x + 80.0 * depth) / edge + 3.0 * fbm(laminae, 4, 2, x, y, size);
            0.5 + 0.5 * math::sin(2.0 * PI * along)
        } else {
            0.5 + 0.5 * math::sin(2.0 * PI * 102.0 * depth / edge)
        };
        let parting = 1.0 - (within / 0.06).clamp(0.0, 1.0);
        let pit = ((fbm(pits, 12, 2, x, y, size) - 0.72) / 0.08).clamp(0.0, 1.0) * (1.0 - hard);
        let joint =
            ((ridged(joints, 4, 2, x + 8.0 * (w - 0.5), 0.0, size) - 0.97) / 0.03).clamp(0.0, 1.0);
        let mut colour = scale(tone, 0.88 + 0.16 * lamina);
        colour = mix(colour, [0.16, 0.13, 0.10], 0.7 * parting.max(joint));
        colour = mix(colour, [0.20, 0.17, 0.14], 0.6 * pit);
        (
            colour,
            0.5 + 0.4 * hard - 0.5 * parting - 0.4 * pit - 0.5 * joint + 0.05 * lamina,
        )
    });
}

fn draw_karst(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let body = draws.stream(1);
    let grikes = draws.stream(2);
    let flutes = draws.stream(3);
    let pits = draws.stream(4);
    let lichen = draws.stream(5);
    // Limestone pavement at 128 texels a metre: pale clints between deep
    // grikes, fluted by runnels where the rain dissolves it, pitted with
    // solution pans and spotted with dark lichen.
    canvas.fill(|x, y| {
        let n = fbm(body, 4, 5, x, y, size);
        let (d1, d2, cell) = cellular(grikes, 3, x, y, size);
        let edge = 0.5 * (d2 - d1);
        let grike = 1.0 - ((edge - 2.0) / 3.0).clamp(0.0, 1.0);
        // Runnels about 14 texels apart, turned with each clint, with a
        // whole number of them across the texture so they wrap.
        let [kx, ky] = grikes.pick(
            cell,
            7,
            &[
                [36.0, 0.0],
                [0.0, 36.0],
                [26.0, 26.0],
                [26.0, -26.0],
                [32.0, 16.0],
                [16.0, -32.0],
            ],
        );
        let along = (kx * x + ky * y) / to_f64(size);
        let flute = 0.5 + 0.5 * math::sin(2.0 * PI * (along + 2.0 * fbm(flutes, 4, 2, x, y, size)));
        let (p1, _, _) = cellular(pits, 9, x, y, size);
        let pit = 1.0 - (p1 / 5.0).clamp(0.0, 1.0);
        let spots = ((fbm(lichen, 16, 2, x, y, size) - 0.7) / 0.06).clamp(0.0, 1.0);
        let mut colour = mix([0.55, 0.54, 0.50], [0.68, 0.67, 0.63], n);
        colour = scale(colour, 0.9 + 0.12 * flute);
        colour = mix(colour, [0.12, 0.12, 0.11], 0.85 * grike);
        colour = mix(colour, [0.30, 0.29, 0.27], 0.6 * pit);
        colour = mix(colour, [0.10, 0.10, 0.10], 0.7 * spots);
        let _ = d1;
        (
            colour,
            0.7 + 0.1 * flute - 1.0 * grike - 0.3 * pit + 0.1 * n,
        )
    });
}

fn draw_foliated(canvas: &mut Canvas, seed: u64) {
    let draws = Draws(seed);
    let size = canvas.size;
    let warp = draws.stream(1);
    let bands = draws.stream(2);
    let mica = draws.stream(3);
    let veins = draws.stream(4);
    let fractures = draws.stream(5);
    // Schist and gneiss at 171 texels a metre: light and dark bands that
    // wander and fold, glinting mica, quartz veins along the foliation and
    // fractures across it.
    canvas.fill(|x, y| {
        let w = fbm(warp, 3, 4, x, y, size);
        let depth = y + 70.0 * (w - 0.5) + 14.0 * math::sin(2.0 * PI * x / to_f64(size) * 2.0);
        // Bands of every width: noise along the depth alone, thresholded
        // softly into light felsic and dark mafic layers.
        let stripe = fbm(bands, 16, 4, 0.0, depth, size);
        let banding = ((stripe - 0.42) / 0.16).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let glint = mica.unit((x as u64) * 1024 + y as u64, 0);
        let vein = ((fbm(veins, 2, 3, x, depth * 4.0, size) - 0.8) / 0.05).clamp(0.0, 1.0);
        // Joints across the foliation: nearly straight, wandering a little.
        let fracture = ((ridged(fractures, 5, 2, x + 10.0 * (w - 0.5), 0.0, size) - 0.975) / 0.025)
            .clamp(0.0, 1.0);
        let mut colour = mix([0.22, 0.22, 0.21], [0.52, 0.50, 0.46], banding);
        if glint > 0.97 {
            colour = scale(colour, 1.35);
        }
        colour = mix(colour, [0.75, 0.74, 0.70], vein);
        colour = mix(colour, [0.10, 0.10, 0.10], 0.75 * fracture);
        (colour, 0.5 + 0.25 * banding + 0.2 * vein - 0.6 * fracture)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_deterministic_and_average_to_their_means() {
        let looks = ground_looks(1);
        assert_eq!(looks.len(), GROUND_LOOK_NAMES.len());
        assert_eq!(looks, ground_looks(1));
        assert_ne!(looks[0].rgba, ground_looks(2)[0].rgba);
        for (look, name) in looks.iter().zip(GROUND_LOOK_NAMES) {
            assert_eq!(look.name, name);
            assert_eq!(look.rgba.len(), GROUND_LOOK_SIZE * GROUND_LOOK_SIZE * 4);
            let texels = mip_chain(&look.rgba, GROUND_LOOK_SIZE);
            assert_eq!(texels.len(), 10);
            // The one texel of the last level is the texture's mean.
            let last = texels.last().unwrap();
            for (channel, &byte) in last.iter().enumerate().take(3) {
                let mean = srgb_to_linear(byte);
                let want = f64::from(look.mean[channel]);
                assert!(
                    (mean - want).abs() < 0.02 + 0.06 * want,
                    "{name}: channel {channel} averages {mean:.3}, wants {want:.3}"
                );
            }
            // Relief spans the alpha channel.
            let (texels, _) = look.rgba.as_chunks::<4>();
            let (low, high) = texels.iter().fold((255, 0), |(lo, hi), texel| {
                (lo.min(texel[3]), hi.max(texel[3]))
            });
            assert_eq!((low, high), (0, 255), "{name}");
        }
    }

    #[test]
    fn relief_slopes_are_how_the_relief_changes_over_a_tile() {
        // Relief rising and falling once along every row: half its span
        // times 2π cos along the rows, nothing down them.
        let size = 64;
        let mut rgba = vec![0_u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let phase = 2.0 * PI * to_f64(x) / to_f64(size);
                rgba[(y * size + x) * 4 + 3] = to_byte(0.5 + 0.5 * math::sin(phase));
            }
        }
        let levels = relief_slopes(&rgba, size);
        assert_eq!(levels.len(), 7);
        for (level, slopes) in levels.iter().enumerate() {
            assert_eq!(slopes.len(), (size >> level) * (size >> level));
        }
        for y in [0, 17, size - 1] {
            for x in 0..size {
                let want = PI * math::cos(2.0 * PI * to_f64(x) / to_f64(size));
                let [u, v] = levels[0][y * size + x];
                // A byte of relief is 1/255 of it; a step of one over two
                // texels is 0.125 per tile here.
                assert!((f64::from(u) - want).abs() < 0.15, "{x}: {u} wants {want}");
                assert!(v.abs() < 1.0e-6);
            }
        }
        // A coarser level is the mean of the finer.
        let [u, _] = levels[1][3];
        let finer = [6, 7, size + 6, size + 7].map(|at| levels[0][at][0]);
        assert!((u - finer.iter().sum::<f32>() / 4.0).abs() < 1.0e-6);
        // The looks wrap, so their slopes average to nothing.
        for look in ground_looks(5).iter().take(4) {
            let levels = relief_slopes(&look.rgba, GROUND_LOOK_SIZE);
            assert_eq!(levels.len(), 10);
            let [u, v] = levels[9][0];
            assert!(
                u.abs() < 1.0e-3 && v.abs() < 1.0e-3,
                "{}: {u} {v}",
                look.name
            );
        }
    }

    #[test]
    fn looks_tile_without_a_seam() {
        // Across the wrap, neighbouring texels differ no more than they do
        // anywhere else.
        for look in ground_looks(3) {
            let size = GROUND_LOOK_SIZE;
            let texel = |x: usize, y: usize| {
                let at = (y * size + x) * 4;
                [0, 1, 2, 3].map(|c| f64::from(look.rgba[at + c]))
            };
            // The mean change between column (or row) `i` and `j`.
            let step = |i: usize, j: usize, rows: bool| {
                (0..size)
                    .map(|k| {
                        let (a, b) = if rows {
                            (texel(k, i), texel(k, j))
                        } else {
                            (texel(i, k), texel(j, k))
                        };
                        (0..4).map(|c| (a[c] - b[c]).abs()).sum::<f64>()
                    })
                    .sum::<f64>()
                    / to_f64(size)
            };
            for (rows, edge) in [(false, "left and right"), (true, "top and bottom")] {
                let inside: f64 = (1..9).map(|k| step(k * 50, k * 50 + 1, rows)).sum::<f64>() / 8.0;
                let across = step(size - 1, 0, rows);
                assert!(
                    across < 2.0 * inside + 4.0,
                    "{}: {across:.1} across the {edge} edges, {inside:.1} inside",
                    look.name
                );
            }
        }
    }
}
