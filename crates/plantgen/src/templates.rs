//! Organ card textures.
//!
//! Every organ card samples one square template: coverage, brightness and
//! accent weight per texel. A species' templates are drawn from its looks
//! (see [`crate::looks`]), one per organ type of its program, by a small
//! procedural generator per [`Shape`]: needle shoots, scale sprays,
//! palmate, simple, lobed and compound leaves, leafy sprigs, grass blades,
//! fern fronds, flowers, flower heads, umbels, spikes, panicles and fruit.
//! Simple, lobed, palmate and compound leaves and the leaves of sprigs are
//! lit along veins grown by space colonization ([`grown_veins`],
//! [`crate::venation`]; Runions et al. 2005) from the primary veins their
//! generator draws.
//!
//! A generator works in card units, card lengths: `x` across the card from
//! its centre line and `y` from its base (0) to its tip (1), so the card
//! spans `x` in `[-aspect / 2, aspect / 2]`. Flowers, heads and umbels are
//! square and drawn about the card's centre.
//!
//! The generators keep the short names of the geometry they draw (`x`, `y`,
//! a point `p`, a parameter `t` along a curve, a radius `r`).
#![allow(clippy::many_single_char_names)]

use crate::body::BodyLook;
use crate::looks::{
    Blade, Compound, Flower, Frond, Fruit, Head, Lobed, Look, Needles, Palmate, Panicle, Scales,
    Shape, Simple, Spike, Sprig, Umbel,
};
use crate::math::{self, PI};
use crate::rng::{mix64, unit};
use crate::spines::{self, View};
use crate::venation::{Growth, Lamina, Venation};

/// Edge length of each template, in texels.
pub const TEMPLATE_SIZE: usize = 128;

/// One texel of a template.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Texel {
    /// 0 (empty) to 1 (covered). Cards are cut out where it is below 0.5.
    pub coverage: f32,
    /// Albedo multiplier, around 1, at most 2.
    pub brightness: f32,
    /// How far the texel's colour moves from the card's colour to the
    /// look's accent colour, 0 to 1: a flower's centre, a petiole.
    pub accent: f32,
}

/// One organ type's template.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    /// The organ's name in the program, or for a spine template its
    /// body's.
    pub organ: String,
    /// The shape's template name, for example `palmate`.
    pub shape: &'static str,
    /// Card width over card length.
    pub aspect: f64,
    /// The look's colour in full light and its accent colour, linear RGB.
    pub colour: [f32; 3],
    pub accent_colour: [f32; 3],
    /// How cards of this template bend on the levels that keep a card per
    /// organ (`crate::bend`).
    pub bend: crate::bend::Bend,
    pub coverage: Vec<f32>,
    pub brightness: Vec<f32>,
    pub accent: Vec<f32>,
}

impl Template {
    /// Leaf area the template draws on a card one metre long, m²: the
    /// card's width times the share of it covered.
    #[must_use]
    pub fn area(&self) -> f64 {
        let covered: f64 = self.coverage.iter().map(|&value| f64::from(value)).sum();
        self.aspect * covered / to_f64(self.coverage.len().max(1))
    }
}

/// Leaf area an organ 1 m long draws with `look` and its `template`, m²:
/// the card's covered area, plus the crossing card's for looks that have
/// one.
#[must_use]
pub fn drawn_area(look: &Look, template: &Template) -> f64 {
    template.area() * (1.0 + look.shape.cross().unwrap_or(0.0))
}

/// One template per organ type, indexed like the program's organ types,
/// then two per body type, its spines seen face on and from the side.
#[derive(Debug, Clone, PartialEq)]
pub struct Templates {
    pub size: usize,
    pub templates: Vec<Template>,
    /// The looks of the program's body types, whose spines the last
    /// templates are drawn from and which a renderer expands tufts with.
    pub bodies: Vec<BodyLook>,
    /// The plant's bark pattern, which previews and impostors draw on its
    /// wood (`crate::bark`); `None` for plain bark.
    pub bark: Option<crate::bark::BarkParams>,
}

/// No templates: for scenes without organ cards. Cards that name a
/// template that does not exist are fully covered and plain.
impl Default for Templates {
    fn default() -> Self {
        Self {
            size: TEMPLATE_SIZE,
            templates: Vec::new(),
            bodies: Vec::new(),
            bark: None,
        }
    }
}

impl Templates {
    /// These templates with the plant's bark pattern (see [`Self::bark`]).
    #[must_use]
    pub fn with_bark(mut self, bark: Option<crate::bark::BarkParams>) -> Self {
        self.bark = bark;
        self
    }

    /// The templates of a plant: one per organ type of `looks`, then a
    /// star and a fan per body type of `bodies`, by name and look in the
    /// program's order (see [`crate::spines::template`]).
    #[must_use]
    pub fn for_plant(looks: &[Look], bodies: &[(&str, &BodyLook)]) -> Self {
        let mut templates = Self::for_looks(looks);
        for &(name, body) in bodies {
            for (view, shape) in [(View::Star, "spine-star"), (View::Fan, "spine-fan")] {
                let drawn = spines::template(body, view, TEMPLATE_SIZE);
                templates.templates.push(Template {
                    organ: name.to_string(),
                    shape,
                    aspect: drawn.aspect,
                    colour: drawn.colour,
                    accent_colour: body.areoles.felt,
                    bend: crate::bend::Bend::FLAT,
                    coverage: drawn.coverage,
                    brightness: drawn.brightness,
                    accent: drawn.accent,
                });
            }
        }
        templates.bodies = bodies.iter().map(|&(_, body)| body.clone()).collect();
        templates
    }

    /// Index of the first spine template: the number of organ templates.
    #[must_use]
    pub fn first_spine_template(&self) -> usize {
        self.templates.len() - 2 * self.bodies.len()
    }

    /// Draw the templates of `looks`, in order.
    #[must_use]
    pub fn for_looks(looks: &[Look]) -> Self {
        let templates = looks
            .iter()
            .map(|look| {
                let texels = TEMPLATE_SIZE * TEMPLATE_SIZE;
                let mut coverage = Vec::with_capacity(texels);
                let mut brightness = Vec::with_capacity(texels);
                let mut accent = Vec::with_capacity(texels);
                let aspect = look.shape.aspect();
                let veins = grown_veins(&look.shape);
                for y in 0..TEMPLATE_SIZE {
                    for x in 0..TEMPLATE_SIZE {
                        // Four samples per texel give soft edges.
                        let mut sum = Paint::EMPTY;
                        for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                            let u = (to_f64(x) + dx) / to_f64(TEMPLATE_SIZE);
                            let v = (to_f64(y) + dy) / to_f64(TEMPLATE_SIZE);
                            let paint = draw(&look.shape, (u - 0.5) * aspect, v, veins.as_ref());
                            sum.cover += paint.cover;
                            // Brightness and accent of the covered samples.
                            sum.bright += paint.bright * paint.cover;
                            sum.accent += paint.accent * paint.cover;
                        }
                        let (bright, tint) = if sum.cover > 0.0 {
                            (sum.bright / sum.cover, sum.accent / sum.cover)
                        } else {
                            (1.0, 0.0)
                        };
                        #[allow(clippy::cast_possible_truncation)]
                        {
                            coverage.push((sum.cover / 4.0) as f32);
                            brightness.push(bright.clamp(0.0, 1.99) as f32);
                            accent.push(tint.clamp(0.0, 1.0) as f32);
                        }
                    }
                }
                Template {
                    organ: look.organ.clone(),
                    shape: look.shape.name(),
                    aspect,
                    colour: look.colour,
                    accent_colour: look.accent,
                    bend: look.bend,
                    coverage,
                    brightness,
                    accent,
                }
            })
            .collect();
        Self {
            size: TEMPLATE_SIZE,
            templates,
            bodies: Vec::new(),
            bark: None,
        }
    }

    /// Nearest-texel lookup. `v` runs from the organ's base (0) to its tip
    /// (1). A template that does not exist is fully covered and plain.
    #[must_use]
    pub fn sample(&self, template: usize, u: f32, v: f32) -> Texel {
        let Some(template) = self.templates.get(template) else {
            return Texel {
                coverage: 1.0,
                brightness: 1.0,
                accent: 0.0,
            };
        };
        let size = self.size;
        let texel = |coordinate: f32| -> usize {
            // Clamped into the texture.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let index = (coordinate.clamp(0.0, 0.999_999) * size as f32) as usize;
            index.min(size - 1)
        };
        let at = texel(v) * size + texel(u);
        Texel {
            coverage: template.coverage[at],
            brightness: template.brightness[at],
            accent: template.accent[at],
        }
    }

    /// Albedo of a card texel: the card's colour, blended toward the look's
    /// accent colour by the texel's accent weight, times its brightness.
    /// The accent colour is darkened as much as the card's colour is
    /// darker than the look's colour, so shaded flowers stay shaded.
    #[must_use]
    pub fn albedo(&self, template: usize, colour: [f64; 3], texel: Texel) -> [f64; 3] {
        let brightness = f64::from(texel.brightness);
        let Some(template) = self.templates.get(template) else {
            return colour.map(|channel| channel * brightness);
        };
        let weight = f64::from(texel.accent);
        let luminance = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let look = luminance(template.colour.map(f64::from));
        let darkening = if look > 1e-6 {
            (luminance(colour) / look).clamp(0.0, 2.0)
        } else {
            1.0
        };
        std::array::from_fn(|channel| {
            let accent = f64::from(template.accent_colour[channel]) * darkening;
            (colour[channel] + (accent - colour[channel]) * weight) * brightness
        })
    }

    /// The templates side by side as an RGBA8 image, the package's organ
    /// atlas: brightness / 2 in red, the accent weight in green, coverage
    /// in alpha. Blue is 0. Row 0 is the cards' base (`v` = 0).
    #[must_use]
    pub fn atlas_rgba(&self) -> (usize, usize, Vec<u8>) {
        self.image(|index, at| {
            let template = &self.templates[index];
            [
                crate::raster::to_u8(template.brightness[at] * 0.5),
                crate::raster::to_u8(template.accent[at]),
                0,
                crate::raster::to_u8(template.coverage[at]),
            ]
        })
    }

    /// The templates side by side in their looks' colours over
    /// `background` (linear RGB), as sRGB RGBA8, for people to look at:
    /// each card at its own width over length, `scale` pixels per texel
    /// along its length, tips up.
    #[must_use]
    pub fn swatches_rgba(&self, background: [f32; 3], scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let gap = 8 * scale;
        let height = self.size * scale + 2 * gap;
        let widths: Vec<usize> = self
            .templates
            .iter()
            .map(|template| {
                // Card widths are small positive multiples of a texel.
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let width = (template.aspect * (self.size * scale) as f64).round() as usize;
                width.max(1)
            })
            .collect();
        let width = widths.iter().map(|w| w + gap).sum::<usize>() + gap;
        let back = background.map(|channel| crate::raster::to_u8(crate::raster::srgb(channel)));
        let mut pixels = Vec::with_capacity(width * height * 4);
        for _ in 0..width * height {
            pixels.extend_from_slice(&[back[0], back[1], back[2], 255]);
        }
        let mut left = gap;
        for (index, template) in self.templates.iter().enumerate() {
            let cell = widths[index];
            for row in 0..self.size * scale {
                // Tips up: the image's top row is the card's tip.
                let v = (self.size * scale - 1 - row) / scale;
                for column in 0..cell {
                    let u = (column * self.size / cell).min(self.size - 1);
                    let at = v * self.size + u;
                    let texel = Texel {
                        coverage: template.coverage[at],
                        brightness: template.brightness[at],
                        accent: template.accent[at],
                    };
                    let colour = self.albedo(index, template.colour.map(f64::from), texel);
                    let cover = texel.coverage.clamp(0.0, 1.0);
                    let target = ((gap + row) * width + left + column) * 4;
                    for channel in 0..3 {
                        #[allow(clippy::cast_possible_truncation)]
                        let lit = colour[channel] as f32;
                        let value = lit * cover + background[channel] * (1.0 - cover);
                        pixels[target + channel] = crate::raster::to_u8(crate::raster::srgb(value));
                    }
                }
            }
            left += cell + gap;
        }
        (width, height, pixels)
    }

    /// The templates side by side, `pixel(template, texel)` giving each
    /// texel's RGBA. Row 0 is the base of the cards.
    fn image(&self, pixel: impl Fn(usize, usize) -> [u8; 4]) -> (usize, usize, Vec<u8>) {
        let size = self.size;
        let width = size * self.templates.len().max(1);
        let mut pixels = vec![0_u8; width * size * 4];
        for index in 0..self.templates.len() {
            for y in 0..size {
                for x in 0..size {
                    let target = (y * width + index * size + x) * 4;
                    pixels[target..target + 4].copy_from_slice(&pixel(index, y * size + x));
                }
            }
        }
        (width, size, pixels)
    }
}

fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// What a generator paints at one point.
#[derive(Debug, Clone, Copy)]
struct Paint {
    cover: f64,
    bright: f64,
    accent: f64,
}

impl Paint {
    const EMPTY: Self = Self {
        cover: 0.0,
        bright: 0.0,
        accent: 0.0,
    };

    fn solid(bright: f64, accent: f64) -> Self {
        Self {
            cover: 1.0,
            bright,
            accent,
        }
    }
}

/// Texels kept clear at the sides and tip of every template, so that a
/// renderer that filters the atlas never bleeds one template into the next.
const BORDER: f64 = 2.0;

/// How much of the card's width and length a shape is drawn into: all of
/// it less [`BORDER`] at the sides and tip. Blade pieces run on into the
/// next card, so only their tips keep clear of the top.
pub(crate) fn drawn_share(shape: &Shape) -> (f64, f64) {
    let size = to_f64(TEMPLATE_SIZE);
    let tiles = matches!(shape, Shape::Blade(blade) if blade.taper < 1.0);
    (
        1.0 - 2.0 * BORDER / size,
        if tiles { 1.0 } else { 1.0 - BORDER / size },
    )
}

/// Half the width of the finest line that survives the cut-out, in units
/// of the card's length: a little under one texel, so lines are almost
/// two texels wide.
pub(crate) fn fine(shape: &Shape) -> f64 {
    0.85 * shape.aspect().max(1.0) / to_f64(TEMPLATE_SIZE)
}

/// The point `x`, `y` of a card drawn with `shape`, its leaves lit along
/// their grown `veins` (see [`grown_veins`]) where it has them.
fn draw(shape: &Shape, x: f64, y: f64, veins: Option<&Venation>) -> Paint {
    // Shapes are drawn into the card less its border: stretch the point
    // instead.
    let (across, along) = drawn_share(shape);
    let (x, y) = (x / across, y / along);
    let fine = fine(shape);
    match shape {
        Shape::Needles(s) => needles(s, x, y, fine),
        Shape::Scales(s) => scales(s, x, y, fine),
        Shape::Palmate(s) => palmate(s, x, y, fine, veins),
        Shape::Simple(s) => simple(s, x, y, fine, veins),
        Shape::Lobed(s) => lobed(s, x, y, fine, veins),
        Shape::Compound(s) => compound(s, x, y, fine, veins),
        Shape::Sprig(s) => sprig(s, x, y, fine, veins),
        Shape::Blade(s) => blade(s, x, y),
        Shape::Frond(s) => frond(s, x, y, fine),
        Shape::Flower(s) => flower(s, x, y - 0.5),
        Shape::Head(s) => head(s, x, y - 0.5),
        Shape::Umbel(s) => umbel(s, x, y - 0.5),
        Shape::Spike(s) => spike(s, x, y, fine),
        Shape::Panicle(s) => panicle(s, x, y, fine),
        Shape::Fruit(s) => fruit(s, x, y, fine),
    }
}

/// How far along `offset` from `from` a line may reach and stay inside a
/// card of half-width `half`, keeping `margin` from its sides and tip: a
/// fraction from 0 to 1.
fn fit(from: (f64, f64), offset: (f64, f64), half: f64, margin: f64) -> f64 {
    let mut t: f64 = 1.0;
    let side = half - margin;
    if offset.0 > 1e-12 {
        t = t.min((side - from.0) / offset.0);
    } else if offset.0 < -1e-12 {
        t = t.min((-side - from.0) / offset.0);
    }
    if offset.1 > 1e-12 {
        t = t.min((1.0 - margin - from.1) / offset.1);
    }
    t.clamp(0.0, 1.0)
}

fn frac(x: f64) -> f64 {
    x - x.floor()
}

fn hypot(x: f64, y: f64) -> f64 {
    math::sqrt(x * x + y * y)
}

/// A number in `[0, 1)` from two small integers and a salt.
fn hash(a: u64, b: u64, salt: u64) -> f64 {
    unit(mix64(mix64(a ^ salt.rotate_left(17)) ^ b.rotate_left(31)))
}

/// Distance from `p` to the segment `a`–`b`, and the position of the
/// nearest point along it from 0 (at `a`) to 1 (at `b`).
fn segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (hypot(p.0 - (a.0 + dx * t), p.1 - (a.1 + dy * t)), t)
}

/// `p` in the frame of an axis from `a` along the unit direction `d`: the
/// distance along it and the signed distance to its left.
fn local(p: (f64, f64), a: (f64, f64), d: (f64, f64)) -> (f64, f64) {
    let (rx, ry) = (p.0 - a.0, p.1 - a.1);
    (rx * d.0 + ry * d.1, ry * d.0 - rx * d.1)
}

/// Half-width profile of a leaf blade from base (`t` = 0) to tip (1), 1
/// where it is widest. `base` and `tip` run from tapered or rounded (0) to
/// rounded or drawn out (1), as in [`Simple`].
fn blade_profile(t: f64, widest: f64, base: f64, tip: f64) -> f64 {
    if !(0.0..=1.0).contains(&t) {
        return 0.0;
    }
    if t <= widest {
        let s = 1.0 - t / widest;
        math::pow((1.0 - s * s).max(0.0), 1.3 - 0.8 * base)
    } else {
        let q = 1.0 - (1.0 - t) / (1.0 - widest);
        math::pow((1.0 - q * q).max(0.0), 0.5 + 1.2 * tip)
    }
}

/// Faint pinnate side veins: brighter along lines leaving a midrib at
/// `angle` (radians) every `spacing`, for a point `along` the midrib and
/// `across` from it. Leaves with grown veins draw those instead
/// ([`grown`]).
fn side_veins(along: f64, across: f64, spacing: f64, angle: f64) -> f64 {
    let u = along - across.abs() * math::cos(angle) / math::sin(angle);
    let nearest = (u / spacing).round() * spacing;
    let distance = (u - nearest).abs() * math::sin(angle);
    if distance < 0.0028 { 0.12 } else { 0.0 }
}

/// How much a grown vein lightens its blade at its middle: the finest by
/// the first, the widest by both.
const VEIN: (f64, f64) = (0.12, 0.1);
/// The half-widths grown veins are drawn at, card units: the finest and
/// the widest (about 0.15 and 0.45 of a texel), true widths being far
/// under a texel; and the soft edge's half-width.
const VEIN_WIDTHS: (f64, f64, f64) = (0.0012, 0.0035, 0.0008);

/// The lightening of grown `veins` at `(a, t)` of a blade's frame, whose
/// blade is `unit` card units long, or the painted `side` veins for a leaf
/// without grown ones.
fn grown(veins: Option<&Venation>, a: f64, t: f64, unit: f64, side: impl FnOnce() -> f64) -> f64 {
    let Some(veins) = veins else {
        return side();
    };
    let (finest, widest, soft) = VEIN_WIDTHS;
    veins
        .near(a, t)
        .map(|(distance, share)| {
            let half = finest + (widest - finest) * share;
            let on = 1.0 - math::smoothstep(half - soft, half + soft, distance * unit);
            (VEIN.0 + VEIN.1 * share) * on
        })
        .fold(0.0, f64::max)
}

/// A simple leaf's half-width at `t` along its blade of `length` card
/// units, with its teeth.
fn simple_width(s: &Simple, length: f64, t: f64) -> f64 {
    let mut width = s.width * length * 0.5 * blade_profile(t, s.widest, s.base, s.tip);
    if s.teeth > 0 {
        let fade = math::smoothstep(0.04, 0.18, t) * (1.0 - math::smoothstep(0.88, 1.0, t));
        let phase = frac(t * f64::from(s.teeth));
        width -= s.tooth_depth * math::pow(1.0 - phase, 1.5) * fade;
    }
    width
}

/// A pinnately lobed leaf's half-width at `t` along its blade of `length`
/// card units, on its `left` side or its right.
fn lobed_width(s: &Lobed, length: f64, t: f64, left: bool) -> f64 {
    let envelope = blade_profile(t, 0.55, 0.35, 0.3);
    let mut sinus = 1.0;
    if (0.06..0.9).contains(&t) {
        let offset = if left { 0.35 } else { 0.0 };
        let phase = frac((t - 0.06) / 0.84 * f64::from(s.lobes) + offset);
        let pointed = 1.0 - (2.0 * phase - 1.0).abs();
        let rounded = math::sin(PI * phase);
        let bump = math::pow(pointed + (rounded - pointed) * s.round, 0.7);
        // Sinuses shallow out toward the base and the tip.
        let fade = math::sin(PI * (t - 0.06) / 0.84);
        sinus = 1.0 - s.depth * (1.0 - bump) * math::pow(fade, 0.4);
    }
    s.width * length * 0.5 * envelope * sinus
}

/// A leaflet's half-width at `t` along it, for a leaflet `length` card
/// units long.
fn leaflet_width(s: &Compound, length: f64, t: f64) -> f64 {
    let mut width = s.leaflet_width * length * 0.5 * blade_profile(t, 0.45, 0.5, 0.45);
    if s.teeth > 0 {
        let fade = math::smoothstep(0.1, 0.25, t) * (1.0 - math::smoothstep(0.85, 1.0, t));
        width -= 0.012 * length * (1.0 - frac(t * f64::from(s.teeth))) * fade * 3.0;
    }
    width
}

/// A sprig leaf's half-width at `t` along its blade of `blade` card units.
fn sprig_width(s: &Sprig, blade: f64, t: f64) -> f64 {
    let mut width = s.width * blade * 0.5 * blade_profile(t, s.widest, s.base, s.tip);
    if s.teeth > 0 {
        let fade = math::smoothstep(0.04, 0.18, t) * (1.0 - math::smoothstep(0.88, 1.0, t));
        let phase = frac(t * f64::from(s.teeth));
        width -= s.tooth_depth * blade * tooth_notch(phase, s.round) * fade;
    }
    width
}

/// How far a tooth's margin falls back, 0 to 1, at `phase` along the
/// tooth: saw teeth (`round` 0) rise slowly to their tip and fall back at
/// once; rounded lobes (`round` 1) swell and narrow evenly.
fn tooth_notch(phase: f64, round: f64) -> f64 {
    let saw = math::pow(1.0 - phase, 1.5);
    if round <= 0.0 {
        return saw;
    }
    let lobe = 1.0 - math::pow(math::sin(PI * phase), 0.6);
    saw + (lobe - saw) * round
}

/// The tips of a blade's `teeth` (each where its margin stands out
/// farthest, for teeth as `round` as [`tooth_notch`] draws them) on both
/// sides, in the blade's frame, for its half-width `half` (blade lengths)
/// at `t` and the share of the blade the teeth fade in by.
fn tooth_tips(
    teeth: u32,
    round: f64,
    half: impl Fn(f64) -> f64,
    fade: impl Fn(f64) -> f64,
) -> Vec<(f64, f64)> {
    // Where a tooth's notch is least, sampled within the tooth short of
    // its end, where a saw tooth meets the next one's notch and a vein
    // could not grow in.
    let tip = (0..200)
        .map(|i| f64::from(i) * 0.94 / 199.0)
        .min_by(|a, b| tooth_notch(*a, round).total_cmp(&tooth_notch(*b, round)))
        .unwrap_or(0.94);
    let mut tips = Vec::new();
    for k in 0..teeth {
        let t = (f64::from(k) + tip) / f64::from(teeth);
        if fade(t) < 0.3 {
            continue;
        }
        let a = 0.96 * half(t);
        tips.push((a, t));
        tips.push((-a, t));
    }
    tips
}

/// The seed a look's veins grow from: a hash of its shape.
fn shape_seed(shape: &Shape) -> u64 {
    format!("{shape:?}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// The veins of a leaf drawn with `shape`, grown in its blade's frame
/// (see [`crate::venation`]): a simple or lobed leaf's from its midrib, a
/// palmate leaf's from its main veins, and one leaflet's of a compound
/// leaf or one leaf's of a sprig, which every leaflet or leaf draws scaled
/// to its own length. `None` for shapes without them.
#[must_use]
pub fn grown_veins(shape: &Shape) -> Option<Venation> {
    let seed = shape_seed(shape);
    let midrib = vec![vec![(0.0, 0.0), (0.0, 0.96)]];
    let simple_fade =
        |t: f64| math::smoothstep(0.04, 0.18, t) * (1.0 - math::smoothstep(0.88, 1.0, t));
    let grow = |inside: &dyn Fn((f64, f64)) -> bool, half: f64, primaries, tips, growth: Growth| {
        let blade = Lamina {
            inside,
            bounds: ((-half, 0.0), (half, 1.0)),
            primaries,
            tips,
        };
        Some(Venation::grow(&blade, &growth, seed))
    };
    match shape {
        Shape::Simple(s) => {
            let length = 1.0 - s.petiole;
            let half = |t: f64| simple_width(s, length, t) / length;
            let inside = |p: (f64, f64)| (0.0..=1.0).contains(&p.1) && p.0.abs() < half(p.1);
            let tips = tooth_tips(s.teeth, 0.0, half, simple_fade);
            grow(
                &inside,
                0.5 * s.width + 0.02,
                midrib,
                tips,
                Growth::default(),
            )
        }
        Shape::Lobed(s) => {
            let length = 1.0 - s.petiole;
            let half = |t: f64, left: bool| lobed_width(s, length, t, left) / length;
            let inside =
                |p: (f64, f64)| (0.0..=1.0).contains(&p.1) && p.0.abs() < half(p.1, p.0 < 0.0);
            // A vein to the tip of every lobe.
            let mut tips = Vec::new();
            for left in [false, true] {
                let offset = if left { 0.35 } else { 0.0 };
                for k in 0..=s.lobes {
                    let t = 0.06 + 0.84 * (f64::from(k) + 0.5 - offset) / f64::from(s.lobes);
                    if (0.1..0.88).contains(&t) {
                        let a = 0.92 * half(t, left);
                        tips.push((if left { -a } else { a }, t));
                    }
                }
            }
            let growth = Growth {
                secondary: 0.84 / f64::from(s.lobes.max(1) * 2 + 1),
                ..Growth::default()
            };
            grow(&inside, 0.5 * s.width + 0.02, midrib, tips, growth)
        }
        Shape::Compound(s) => {
            let half = |t: f64| leaflet_width(s, 1.0, t);
            let inside = |p: (f64, f64)| (0.0..=1.0).contains(&p.1) && p.0.abs() < half(p.1);
            let fade =
                |t: f64| math::smoothstep(0.1, 0.25, t) * (1.0 - math::smoothstep(0.85, 1.0, t));
            let tips = tooth_tips(s.teeth, 0.0, half, fade);
            grow(
                &inside,
                0.5 * s.leaflet_width + 0.02,
                midrib,
                tips,
                Growth::default(),
            )
        }
        Shape::Sprig(s) => {
            let half = |t: f64| sprig_width(s, 1.0, t);
            let inside = |p: (f64, f64)| (0.0..=1.0).contains(&p.1) && p.0.abs() < half(p.1);
            let tips = tooth_tips(s.teeth, s.round, half, simple_fade);
            grow(
                &inside,
                0.5 * s.width + 0.02,
                midrib,
                tips,
                Growth::default(),
            )
        }
        Shape::Palmate(s) => Some(palmate_veins(s, fine(shape), seed)),
        _ => None,
    }
}

/// A palmate leaf's veins, grown outward from its main veins in the frame
/// of their junction and the blade's radius, inside the painter's own
/// blade less its petiole.
fn palmate_veins(s: &Palmate, fine: f64, seed: u64) -> Venation {
    let (junction, radius) = s.junction();
    let inside = |p: (f64, f64)| {
        !(p.1 < 0.02 && p.0.abs() < 0.05)
            && palmate(s, p.0 * radius, junction + p.1 * radius, fine, None).cover > 0.0
    };
    let half = i32::try_from(s.lobes / 2).unwrap_or(0);
    let primaries = (-half..=half)
        .map(|k| {
            let (angle, length) = s.lobe(k);
            let reach = 0.92 * length / radius;
            vec![
                (0.0, 0.0),
                (math::sin(angle) * reach, math::cos(angle) * reach),
            ]
        })
        .collect();
    let blade = Lamina {
        inside: &inside,
        bounds: ((-1.05, -1.05), (1.05, 1.05)),
        primaries,
        tips: Vec::new(),
    };
    let growth = Growth {
        outward: true,
        ..Growth::default()
    };
    Venation::grow(&blade, &growth, seed)
}

/// One axis of a needle spray in its card's frame (units of the card's
/// length, `x` across and `y` along): the shoot, then its side twigs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NeedleAxis {
    pub start: (f64, f64),
    /// Unit direction in the card's plane.
    pub direction: (f64, f64),
    pub length: f64,
    /// The length of its longest needles.
    pub needle: f64,
}

/// The axes of a spray: the shoot, then `twigs` side twigs alternating
/// left and right, reaching no farther than the card holds.
pub(crate) fn needle_axes(s: &Needles) -> Vec<NeedleAxis> {
    let half = s.aspect * 0.5;
    let twig_angle = math::radians(s.twig_angle);
    let reach = ((half - s.needle * 0.75) / math::sin(twig_angle)).clamp(0.04, 0.75);
    let mut axes = Vec::with_capacity(1 + s.twigs as usize);
    axes.push(NeedleAxis {
        start: (0.0, 0.0),
        direction: (0.0, 1.0),
        length: 0.97,
        needle: s.needle,
    });
    for k in 0..s.twigs {
        let at = 0.08 + 0.78 * (f64::from(k) + 0.5) / f64::from(s.twigs);
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let length = (reach * (1.0 - 0.55 * at)).min((0.97 - at) / math::cos(twig_angle));
        axes.push(NeedleAxis {
            start: (0.0, at),
            direction: (side * math::sin(twig_angle), math::cos(twig_angle)),
            length,
            needle: s.needle * 0.8,
        });
    }
    axes
}

/// The spacing of needles along an axis whose needles are `needle` long:
/// close all round a shoot, wider in two ranks.
pub(crate) fn needle_pitch(s: &Needles, needle: f64) -> f64 {
    needle * if s.ranks == 0 { 0.085 } else { 0.22 }
}

/// The needles that leave an axis at one place: a fascicle of `bundle`
/// all round a shoot, or one to each side in two ranks.
pub(crate) fn needles_per_place(s: &Needles) -> usize {
    if s.ranks == 0 {
        s.bundle.max(1) as usize
    } else {
        2
    }
}

/// How far apart the needles of a fascicle fan round the shoot, radians.
const FAN: f64 = 0.25;

/// One needle of a spray, in the frame of its axis: from `along` on the
/// axis, `length` long, leaving at `tilt` from the axis; across the axis
/// (in the card's plane, to its left) by the share `across` of its length
/// and out of the card by `out`, with `along_share` along the axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NeedleDraw {
    pub along: f64,
    pub length: f64,
    pub along_share: f64,
    pub across: f64,
    pub out: f64,
    /// Its own draw in `[0, 1)`, which shades it.
    pub jitter: f64,
}

/// Needle `k` of place `i` (of [`needles_per_place`]) on axis `index` of
/// a spray `s` on a card of half-width `half`, shortened to stay inside
/// the card with `fine` to spare: the card's painter and the solid shoot
/// (`crate::shoots`) both draw it. The painter draws the places' own
/// needles (`between` 0); the solid shoot also those `between` 1 to
/// `steps - 1`, which stand `between / steps` of the pitch farther along
/// (`crate::shoots::fill`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn needle_at(
    s: &Needles,
    index: usize,
    axis: &NeedleAxis,
    i: i64,
    k: usize,
    (between, steps): (usize, usize),
    half: f64,
    fine: f64,
) -> NeedleDraw {
    let lean = math::radians(58.0);
    let pitch = needle_pitch(s, axis.needle);
    let salt = u64::try_from(index).unwrap_or(0);
    let n = u64::try_from(i).unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let shift = between as f64 / steps.max(1) as f64;
    #[allow(clippy::cast_precision_loss)]
    let along = (i as f64 + 0.5 + shift) * pitch;
    let shorten = 1.0 - 0.35 * along / axis.length;
    // A fascicle's needles share their place's draw of its azimuth.
    let per_place = needles_per_place(s);
    let place = n * u64::try_from(per_place.max(2)).unwrap_or(2)
        + (u64::try_from(between).unwrap_or(0) << 40);
    let key = place + u64::try_from(k).unwrap_or(0);
    let jitter = hash(key, salt, 11);
    let full = axis.needle * shorten * (0.85 + 0.3 * jitter);
    let tilt = lean + math::radians(16.0) * (hash(key, salt, 12) - 0.5);
    // A needle at azimuth `phi` round the shoot, a fascicle's fanned; in
    // two ranks, flat in the card's plane, the first to the left.
    let (across, out) = if s.ranks == 0 {
        #[allow(clippy::cast_precision_loss)]
        let fan = (k as f64 - (per_place as f64 - 1.0) * 0.5) * FAN;
        #[allow(clippy::cast_precision_loss)]
        let phi = (i as f64 + shift) * 2.399_963 + hash(place, salt, 13) + fan;
        (
            math::sin(tilt) * math::cos(phi),
            math::sin(tilt) * math::sin(phi),
        )
    } else {
        let side = if k == 0 { 1.0 } else { -1.0 };
        (side * math::sin(tilt), 0.0)
    };
    let along_share = math::cos(tilt);
    // Shorten needles that would leave the card.
    let (d, start) = (axis.direction, axis.start);
    let from = (start.0 + d.0 * along, start.1 + d.1 * along);
    let offset = (
        (d.0 * along_share - d.1 * across) * full,
        (d.1 * along_share + d.0 * across) * full,
    );
    NeedleDraw {
        along,
        length: full * fit(from, offset, half, fine * 1.2),
        along_share,
        across,
        out,
        jitter,
    }
}

/// How many places along an axis needles leave it at.
pub(crate) fn needle_count(s: &Needles, axis: &NeedleAxis) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let count = (axis.length / needle_pitch(s, axis.needle))
        .floor()
        .max(0.0) as i64;
    count
}

/// A conifer shoot: a central axis and side twigs, set with needles in two
/// flat ranks or all round, which seen from the side radiate at every angle.
fn needles(s: &Needles, x: f64, y: f64, fine: f64) -> Paint {
    let half = s.aspect * 0.5;
    let p = (x, y);
    let mut best = Paint::EMPTY;
    for (index, axis) in needle_axes(s).iter().enumerate() {
        let (along, across) = local(p, axis.start, axis.direction);
        if along < -axis.needle || along > axis.length + axis.needle {
            continue;
        }
        // The twig itself.
        let wood = if index == 0 { fine * 1.3 } else { fine };
        if (0.0..=axis.length).contains(&along) && across.abs() < wood {
            return Paint::solid(0.75, 1.0);
        }
        let pitch = needle_pitch(s, axis.needle);
        let count = needle_count(s, axis);
        #[allow(clippy::cast_possible_truncation)]
        let first = (((along - axis.needle) / pitch).floor() as i64).max(0);
        #[allow(clippy::cast_possible_truncation)]
        let last = (((along + 0.01) / pitch).ceil() as i64).min(count - 1);
        for i in first..=last {
            for k in 0..needles_per_place(s) {
                let needle = needle_at(s, index, axis, i, k, (0, 1), half, fine);
                let (fa, fc) = (needle.along_share, needle.across);
                let shown = hypot(fa, fc);
                let tip_along = needle.along + fa * needle.length;
                let tip_across = fc * needle.length;
                let (distance, t) = segment(
                    (along, across),
                    (needle.along, 0.0),
                    (tip_along, tip_across),
                );
                if distance < fine && shown > 0.05 {
                    // Prefer the needle drawn on top: the later one.
                    best = Paint::solid(0.78 + 0.32 * t + 0.1 * (needle.jitter - 0.5), 0.0);
                }
            }
        }
    }
    best
}

/// A flat, branched spray of scale leaves: cords of overlapping scales,
/// branching twice, narrowing to the tips.
fn scales(s: &Scales, x: f64, y: f64, fine: f64) -> Paint {
    let half = s.aspect * 0.5;
    let angle = math::radians(s.angle);
    let p = (x, y);
    // One cord: a segment whose width tapers and swells at every scale.
    let cord = |a: (f64, f64), b: (f64, f64), width: f64| -> Option<Paint> {
        let (distance, t) = segment(p, a, b);
        let length = hypot(b.0 - a.0, b.1 - a.1);
        let period = (width * 2.6).max(0.004);
        let along = t * length;
        let swell = 1.0 + 0.22 * math::sin(PI * frac(along / period)).abs();
        let w = (width * (1.0 - 0.45 * t)).max(fine) * swell;
        (distance < w).then(|| {
            let round = 1.0 - distance / w;
            Paint::solid(
                0.8 + 0.28 * round - 0.08 * (1.0 - math::sin(PI * frac(along / period))),
                0.0,
            )
        })
    };
    // Unit-reach geometry first, to fit the spray in the card.
    let branchlets = s.branchlets * 2;
    let sub_angle = math::radians(40.0);
    let mut widest: f64 = 0.0;
    for k in 0..branchlets {
        let at = 0.05 + 0.85 * (f64::from(k) + 0.5) / f64::from(branchlets);
        let length = 1.0 - 0.75 * math::pow(at, 1.3);
        widest = widest.max(length * math::sin(angle));
        widest = widest.max(
            length * 0.5 * math::sin(angle) + 0.36 * length * math::sin(angle + sub_angle).abs(),
        );
    }
    let reach = ((half - s.cord * 1.6) / widest.max(1e-3)).min(0.75);
    if let Some(paint) = cord((0.0, 0.0), (0.0, 0.98), s.cord * 1.2) {
        return paint;
    }
    for k in 0..branchlets {
        let at = 0.05 + 0.85 * (f64::from(k) + 0.5) / f64::from(branchlets);
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let length =
            (reach * (1.0 - 0.75 * math::pow(at, 1.3))).min((0.98 - at) / math::cos(angle));
        let direction = (side * math::sin(angle), math::cos(angle));
        let start = (0.0, at);
        let end = (
            start.0 + direction.0 * length,
            start.1 + direction.1 * length,
        );
        // Skip branchlets far from the point.
        if segment(p, start, end).0 > length * 0.45 + s.cord * 2.0 {
            continue;
        }
        if let Some(paint) = cord(start, end, s.cord * 0.9) {
            return paint;
        }
        for m in 0..5 {
            let position = 0.16 + 0.16 * f64::from(m);
            let sub_side = if (m + k) % 2 == 0 { 1.0 } else { -1.0 };
            let turn = sub_angle * sub_side * -side;
            let (sin, cos) = (math::sin(turn), math::cos(turn));
            let sub = (
                direction.0 * cos - direction.1 * sin,
                direction.0 * sin + direction.1 * cos,
            );
            let sub_length = (0.36 * length * (1.0 - 0.6 * position)).min(
                // Keep the tip inside the card.
                (0.98 - (start.1 + direction.1 * length * position)) / sub.1.max(0.05),
            );
            let from = (
                start.0 + direction.0 * length * position,
                start.1 + direction.1 * length * position,
            );
            let to = (from.0 + sub.0 * sub_length, from.1 + sub.1 * sub_length);
            if let Some(paint) = cord(from, to, s.cord * 0.72) {
                return paint;
            }
        }
    }
    Paint::EMPTY
}

/// A palmately lobed leaf on its petiole: lobes radiating from where the
/// main veins meet, each widest near its middle and drawn out to a point,
/// with a few large teeth, deep sinuses between them, and a heart-shaped
/// base round the petiole.
fn palmate(s: &Palmate, x: f64, y: f64, fine: f64, veins: Option<&Venation>) -> Paint {
    let (junction, radius) = s.junction();
    let p = (x, y - junction);
    if y < junction + 0.01 {
        let width = fine * (1.0 + 0.4 * y / junction.max(1e-3));
        if x.abs() < width {
            return Paint::solid(0.9, 1.0);
        }
    }
    let r = hypot(p.0, p.1);
    if r > radius * 1.05 {
        return Paint::EMPTY;
    }
    // Angle from the tip direction; the base sinus sits at pi.
    let phi = math::atan2(p.0, p.1);
    let floor = radius * (1.0 - s.depth);
    let notch = math::cos(phi).min(0.0);
    let mut inside = r < floor * (1.0 - 0.75 * notch * notch);
    let half = i32::try_from(s.lobes / 2).unwrap_or(0);
    let mut vein: f64 = 0.0;
    let mut accent: f64 = 0.0;
    for k in -half..=half {
        let (angle, length) = s.lobe(k);
        let direction = (math::sin(angle), math::cos(angle));
        let (along, across) = local(p, (0.0, 0.0), direction);
        if along < 0.0 || along > length {
            continue;
        }
        let t = along / length;
        // Main vein to near the lobe's tip.
        if t < 0.92 && across.abs() < 0.0045 * (1.0 - 0.5 * t) {
            vein = vein.max(0.32);
            accent = accent.max(0.6);
        }
        // Straight sides widening from where the lobe leaves the blade to
        // halfway, then drawn in to a long point.
        let profile = if t <= 0.5 {
            math::pow(t / 0.5, 0.9)
        } else {
            math::pow((1.0 - t) / 0.5, 1.15)
        };
        let mut width = length * s.lobe_width * 0.5 * profile;
        if s.teeth > 0 && t > 0.42 && t < 0.92 {
            // A few coarse teeth pointing toward the lobe's tip, on its
            // outer half so they never close a sinus: the margin widens
            // slowly along each tooth and falls back steeply at its point.
            let phase = frac((t - 0.42) / 0.5 * f64::from(s.teeth));
            let tooth = if phase < 0.82 {
                math::pow(phase / 0.82, 1.8)
            } else {
                (1.0 - phase) / 0.18
            };
            let fade = math::smoothstep(0.42, 0.55, t);
            width += length * s.lobe_width * 0.5 * 0.55 * (1.0 - t) * tooth * fade;
        }
        // Keep clear of the line halfway to the neighbouring lobe on this
        // side, so the sinus between them runs down to the blade's floor.
        // `across` is positive toward lower lobe numbers; the outermost
        // lobes' neighbour across the petiole is the opposite outermost lobe.
        let neighbour = if across > 0.0 { k - 1 } else { k + 1 };
        let gap = if neighbour.abs() > half {
            2.0 * PI - 2.0 * angle.abs()
        } else {
            (s.lobe(neighbour).0 - angle).abs()
        };
        let half_gap = (gap * 0.5).min(math::radians(80.0));
        // The sinus opens a little toward the margin.
        let sinus = along * math::tan(half_gap) - radius * (0.018 + 0.05 * along / radius);
        if across.abs() < width.min(sinus) {
            inside = true;
            if veins.is_none() {
                vein = vein.max(side_veins(along, across, length / 6.0, math::radians(48.0)));
            }
        }
    }
    if !inside {
        return Paint::EMPTY;
    }
    if veins.is_some() {
        vein = vein.max(grown(veins, p.0 / radius, p.1 / radius, radius, || 0.0));
    }
    Paint {
        cover: 1.0,
        bright: 0.9 + 0.1 * r / radius + vein,
        accent,
    }
}

/// An undivided leaf: a blade from ovate to lanceolate, with an optional
/// toothed margin, a midrib, faint side veins and a petiole.
fn simple(s: &Simple, x: f64, y: f64, fine: f64, veins: Option<&Venation>) -> Paint {
    let base = s.petiole;
    if y < base + 0.01 && x.abs() < fine {
        return Paint::solid(0.9, 1.0);
    }
    let length = 1.0 - base;
    let t = (y - base) / length.max(1e-6);
    let width = simple_width(s, length, t);
    if x.abs() >= width {
        return Paint::EMPTY;
    }
    if t < 0.97 && x.abs() < fine * 0.7 {
        return Paint::solid(1.2, 0.5);
    }
    let along = y - base;
    Paint::solid(
        0.92 + 0.08 * x.abs() / width.max(1e-6)
            + grown(veins, x / length, t, length, || {
                side_veins(along, x, length / 9.0, math::radians(52.0))
            }),
        0.0,
    )
}

/// A pinnately lobed leaf: rounded or pointed lobes along both sides of a
/// midrib, alternating a little between the sides.
fn lobed(s: &Lobed, x: f64, y: f64, fine: f64, veins: Option<&Venation>) -> Paint {
    let base = s.petiole;
    if y < base + 0.01 && x.abs() < fine {
        return Paint::solid(0.9, 1.0);
    }
    let length = 1.0 - base;
    let t = (y - base) / length.max(1e-6);
    let width = lobed_width(s, length, t, x < 0.0);
    if x.abs() >= width {
        return Paint::EMPTY;
    }
    if t < 0.97 && x.abs() < fine * 0.7 {
        return Paint::solid(1.2, 0.5);
    }
    Paint::solid(
        0.92 + 0.08 * x.abs() / width.max(1e-6)
            + grown(veins, x / length, t, length, || {
                side_veins(
                    y - base,
                    x,
                    length / f64::from(s.lobes * 2 + 1),
                    math::radians(55.0),
                )
            }),
        0.0,
    )
}

/// A pinnately compound leaf: a rachis with opposite pairs of leaflets and
/// a terminal one.
fn compound(s: &Compound, x: f64, y: f64, fine: f64, veins: Option<&Venation>) -> Paint {
    let leaflet = s.leaflet_length();
    let pairs = s.leaflets / 2;
    let top = 1.0 - leaflet;
    if y < top + 0.02 && y >= 0.0 && x.abs() < fine {
        return Paint::solid(0.9, 1.0);
    }
    let p = (x, y);
    // (base, unit direction, length)
    let mut leaflets = Vec::with_capacity(usize::try_from(pairs).unwrap_or(0) * 2 + 1);
    leaflets.push(((0.0, top), (0.0, 1.0), leaflet * 0.98));
    let angle = math::radians(62.0);
    for j in 0..pairs {
        let at = s.petiole + (top - s.petiole) * (f64::from(j) + 0.5) / f64::from(pairs);
        let length = leaflet * (0.82 + 0.18 * (f64::from(j) + 0.5) / f64::from(pairs));
        for side in [1.0, -1.0] {
            leaflets.push((
                (0.0, at),
                (side * math::sin(angle), math::cos(angle)),
                length,
            ));
        }
    }
    for (start, direction, length) in leaflets {
        let (along, across) = local(p, start, direction);
        let t = along / length;
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let width = leaflet_width(s, length, t);
        if across.abs() < width {
            if t < 0.95 && across.abs() < fine * 0.6 {
                return Paint::solid(1.15, 0.4);
            }
            return Paint::solid(
                0.92 + 0.08 * across.abs() / width.max(1e-6)
                    + grown(veins, across / length, t, length, || {
                        side_veins(along, across, length / 7.0, math::radians(55.0))
                    }),
                0.0,
            );
        }
    }
    Paint::EMPTY
}

/// A leafy shoot: a twig in the accent colour with simple leaves
/// alternating along it, each on its petiole at `angle` to the twig, and
/// one leaf continuing the twig at its tip. Side leaves are largest in the
/// shoot's middle; each is turned a little at random and drawn a little
/// lighter or darker, so that overlapping leaves stay apart.
fn sprig(s: &Sprig, x: f64, y: f64, fine: f64, veins: Option<&Venation>) -> Paint {
    let half = s.aspect() * 0.5;
    let longest = s.leaf_length();
    let top = 1.0 - longest;
    let side = s.leaves.saturating_sub(1);
    let p = (x, y);
    let mut best = Paint::EMPTY;
    // The twig, thinning toward the tip; leaves drawn later cover it.
    if (0.0..=top + 0.02).contains(&y) && x.abs() < fine * (1.3 - 0.4 * y) {
        best = Paint::solid(0.8, 1.0);
    }
    // (base, unit direction, length, brightness) of each leaf, lowest
    // first, so that a higher leaf covers the one below it.
    let mut leaves = Vec::with_capacity(usize::try_from(s.leaves).unwrap_or(0));
    for j in 0..side {
        let n = u64::from(j);
        let at = (f64::from(j) + 0.5) / f64::from(side);
        let base = (0.0, s.stalk + (top - s.stalk) * at);
        let turn = if j % 2 == 0 { 1.0 } else { -1.0 };
        let angle = math::radians(s.angle + 12.0 * (hash(n, 0, 21) - 0.5));
        let direction = (turn * math::sin(angle), math::cos(angle));
        let length = longest * (0.78 + 0.22 * math::sin(PI * at));
        // Shorten leaves that would leave the card.
        let offset = (direction.0 * length, direction.1 * length);
        let length = length * fit(base, offset, half, fine * 1.2);
        leaves.push((base, direction, length, 0.86 + 0.12 * hash(n, 1, 22)));
    }
    leaves.push(((0.0, top), (0.0, 1.0), longest * 0.97, 0.92));
    for (start, direction, length, bright) in leaves {
        let (along, across) = local(p, start, direction);
        if along < 0.0 || along > length {
            continue;
        }
        let stalk = s.petiole * length;
        if along < stalk {
            if across.abs() < fine * 0.8 {
                best = Paint::solid(0.9, 1.0);
            }
            continue;
        }
        let blade = length - stalk;
        let t = (along - stalk) / blade.max(1e-6);
        let width = sprig_width(s, blade, t);
        if across.abs() >= width {
            continue;
        }
        best = if t < 0.95 && across.abs() < fine * 0.6 {
            Paint::solid(bright + 0.25, 0.4)
        } else {
            Paint::solid(
                bright
                    + 0.08 * across.abs() / width.max(1e-6)
                    + grown(veins, across / blade, t, blade, || {
                        side_veins(along - stalk, across, blade / 8.0, math::radians(52.0))
                    }),
                0.0,
            )
        };
    }
    best
}

/// A piece of a grass or sedge blade, or its tapering tip: lighter along
/// the midrib, with fine parallel veins.
fn blade(s: &Blade, x: f64, y: f64) -> Paint {
    let mut width = s.aspect * 0.5 * (1.0 - 0.12 * y);
    if s.taper > 0.0 && y > 1.0 - s.taper {
        width *= math::pow(((1.0 - y) / s.taper).max(0.0), 0.85);
    }
    if x.abs() >= width {
        return Paint::EMPTY;
    }
    let across = x.abs() / width.max(1e-6);
    Paint::solid(
        0.86 + 0.24 * (1.0 - across) + 0.04 * math::cos(across * 6.0 * PI),
        0.0,
    )
}

/// A fern frond: a bare stipe, then a rachis with pinnae along both sides
/// in a lance-shaped outline, each pinna whole or deeply divided.
fn frond(s: &Frond, x: f64, y: f64, fine: f64) -> Paint {
    if y < s.stipe {
        return if x.abs() < fine * 1.2 {
            Paint::solid(0.85, 1.0)
        } else {
            Paint::EMPTY
        };
    }
    let length = 1.0 - s.stipe;
    let t = (y - s.stipe) / length.max(1e-6);
    if t < 0.98 && x.abs() < fine * (1.1 - 0.3 * t) {
        return Paint::solid(0.9, 0.7);
    }
    let p = (x, y);
    let count = f64::from(s.pinnae);
    let spacing = length / count;
    let side = if x < 0.0 { -1.0 } else { 1.0 };
    for k in 0..s.pinnae {
        let offset = if side < 0.0 { 0.3 } else { 0.0 };
        let tk = (f64::from(k) + 0.5 + offset) / (count + 0.3);
        let angle = math::radians(74.0 - 32.0 * tk);
        let reach = s.width * 0.5 * math::pow(math::sin(PI * math::pow(tk, 0.75)), 0.7);
        let pinna = reach / math::sin(angle);
        if pinna < 0.004 {
            continue;
        }
        let start = (0.0, s.stipe + length * tk);
        let direction = (side * math::sin(angle), math::cos(angle));
        let (along, across) = local(p, start, direction);
        let u = along / pinna;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let mut width =
            (0.3 * pinna).min(0.5 * spacing * math::sin(angle)) * blade_profile(u, 0.25, 0.8, 0.6);
        if s.divided > 0.0 && (0.04..0.92).contains(&u) {
            let bump = math::pow(math::sin(PI * frac(u * 7.0)), 0.5);
            width *= 1.0 - 0.75 * s.divided * (1.0 - bump);
        }
        if across.abs() < width {
            return Paint::solid(
                0.9 + 0.12 * (1.0 - across.abs() / width.max(1e-6))
                    + if across.abs() < 0.0025 { 0.15 } else { 0.0 },
                0.0,
            );
        }
    }
    Paint::EMPTY
}

/// Radius of a face-on flower, head or umbel in card units.
const BLOOM: f64 = 0.47;

/// The petal or ray that a point lies in front of: the angle of the point
/// from the tip direction, and its distance along and across that
/// petal's axis.
fn petal_frame(x: f64, y: f64, count: u32) -> (f64, f64) {
    let share = 2.0 * PI / f64::from(count.max(1));
    let phi = math::atan2(x, y);
    let delta = phi - (phi / share).round() * share;
    let r = hypot(x, y);
    (r * math::cos(delta), r * math::sin(delta))
}

/// Dots packed in a sunflower spiral over a disc of radius `radius`: the
/// distance from `(x, y)` to the nearest of `count` dots, over the dots'
/// spacing.
fn spiral_dots(x: f64, y: f64, radius: f64, count: u32) -> f64 {
    let golden = 2.399_963;
    let spacing = radius / math::sqrt(f64::from(count.max(1)));
    let mut nearest = f64::MAX;
    for i in 0..count {
        let fi = f64::from(i);
        let r = radius * math::sqrt((fi + 0.5) / f64::from(count));
        let (dx, dy) = (
            x - r * math::sin(fi * golden),
            y - r * math::cos(fi * golden),
        );
        nearest = nearest.min(dx * dx + dy * dy);
    }
    math::sqrt(nearest) / spacing
}

/// A single flower seen face on: rounded or pointed petals, separate at
/// their bases, round a centre of stamens in the accent colour, each
/// petal's base tinted toward it.
fn flower(s: &Flower, x: f64, y: f64) -> Paint {
    let r = hypot(x, y);
    let centre = s.centre * BLOOM;
    if r < centre {
        let dot = spiral_dots(x, y, centre, 24);
        return Paint::solid(if dot < 0.38 { 1.25 } else { 0.8 }, 1.0);
    }
    let (along, across) = petal_frame(x, y, s.petals);
    // Half-width of a petal where it is widest, as a share of the circle
    // at that radius.
    let widest = BLOOM * 0.66 * math::tan(PI / f64::from(s.petals)) * s.petal_width;
    let notch = s.notch * 0.4 * (1.0 - across.abs() / (0.55 * widest)).max(0.0);
    let reach = BLOOM * (1.0 - notch);
    let u = along / reach;
    if !(0.0..=1.0).contains(&u) {
        return Paint::EMPTY;
    }
    // Narrow at the base (the claw), widest two thirds out, round or
    // pointed at the tip.
    let k = 0.35 + 0.9 * s.point;
    let peak = 1.1 / (1.1 + k);
    let norm = math::pow(peak, 1.1) * math::pow(1.0 - peak, k);
    let width = widest * math::pow(u, 1.1) * math::pow(1.0 - u, k) / norm.max(1e-9);
    if across.abs() >= width {
        return Paint::EMPTY;
    }
    let vein = 0.03 * math::cos(across / width.max(1e-6) * 3.0 * PI) * u;
    Paint::solid(0.88 + 0.16 * u + vein, (1.0 - u / 0.35).max(0.0) * 0.5)
}

/// A head of ray and disc flowers seen face on: a disc in the accent colour
/// ringed by strap-shaped rays.
fn head(s: &Head, x: f64, y: f64) -> Paint {
    let r = hypot(x, y);
    let disc = s.disc * BLOOM;
    if r < disc {
        // Disc florets packed in a sunflower spiral.
        let dot = spiral_dots(x, y, disc, 60);
        return Paint::solid(
            if dot < 0.4 { 1.1 } else { 0.78 } * (0.85 + 0.15 * r / disc),
            1.0,
        );
    }
    if s.rays == 0 {
        return Paint::EMPTY;
    }
    let (along, across) = petal_frame(x, y, s.rays);
    let start = disc * 0.85;
    let u = (along - start) / (BLOOM - start);
    if !(0.0..=1.0).contains(&u) {
        return Paint::EMPTY;
    }
    let mut width = BLOOM * math::sin(PI / f64::from(s.rays)) * s.ray_width * (1.0 - 0.2 * u);
    if u > 0.85 {
        width *= math::sqrt((1.0 - ((u - 0.85) / 0.15).powi(2)).max(0.0));
    }
    if across.abs() >= width {
        return Paint::EMPTY;
    }
    let groove = if across.abs() < width * 0.12 {
        -0.06
    } else {
        0.0
    };
    Paint::solid(
        0.94 + 0.1 * (1.0 - across.abs() / width.max(1e-6)) + groove,
        0.0,
    )
}

/// A flat-topped cluster of small flowers seen from above: florets packed
/// in clusters, the clusters packed in the umbel, both in sunflower
/// spirals.
fn umbel(s: &Umbel, x: f64, y: f64) -> Paint {
    let golden = 2.399_963;
    let clusters = f64::from(s.clusters);
    let cluster_radius = (BLOOM * 1.05 / math::sqrt(clusters)).min(BLOOM);
    let floret_radius = cluster_radius * 0.95 / math::sqrt(f64::from(s.florets)) * 1.1;
    let mut best = Paint::EMPTY;
    for j in 0..s.clusters {
        let jf = f64::from(j);
        let spread = (BLOOM - cluster_radius) * math::sqrt((jf + 0.5) / clusters);
        let centre = (
            spread * math::sin(jf * golden),
            spread * math::cos(jf * golden),
        );
        if hypot(x - centre.0, y - centre.1) > cluster_radius + floret_radius {
            continue;
        }
        for i in 0..s.florets {
            let fi = f64::from(i);
            let place =
                (cluster_radius - floret_radius) * math::sqrt((fi + 0.5) / f64::from(s.florets));
            let floret = (
                centre.0 + place * math::sin(fi * golden + jf),
                centre.1 + place * math::cos(fi * golden + jf),
            );
            let distance = hypot(x - floret.0, y - floret.1);
            if distance < floret_radius {
                let d = distance / floret_radius;
                best = Paint::solid(0.9 + 0.2 * (1.0 - d), if d < 0.3 { 1.0 } else { 0.0 });
            }
        }
    }
    best
}

/// A spike, raceme or plume seen from the side: a stalk, then flowers or
/// spikelets along an axis, the lower ones larger.
fn spike(s: &Spike, x: f64, y: f64, fine: f64) -> Paint {
    let half = s.aspect * 0.5;
    if y < s.stalk {
        return if x.abs() < fine * 1.2 {
            Paint::solid(0.85, 1.0)
        } else {
            Paint::EMPTY
        };
    }
    let length = 1.0 - s.stalk;
    if y < 0.97 && x.abs() < fine * (1.0 - 0.3 * (y - s.stalk) / length) {
        return Paint::solid(0.85, 1.0);
    }
    let count = f64::from(s.florets);
    let mut best = Paint::EMPTY;
    if s.plume >= 0.5 {
        // Fine spikelets angled up from the axis on alternate sides.
        let strokes = s.florets * 3;
        for i in 0..strokes {
            let n = u64::from(i);
            let t = (f64::from(i) + 0.5) / f64::from(strokes);
            let envelope = math::sin(PI * math::pow(t, 0.8)).max(0.15);
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            let angle = math::radians(22.0 + 18.0 * hash(n, 1, 21));
            let reach = half * envelope * (0.6 + 0.4 * hash(n, 2, 21));
            let start = (0.0, s.stalk + length * t * 0.97);
            let offset = (side * reach, reach / math::sin(angle) * math::cos(angle));
            let keep = fit(start, offset, half, fine * 1.2);
            let end = (start.0 + offset.0 * keep, start.1 + offset.1 * keep);
            let (distance, along) = segment((x, y), start, end);
            if distance < fine {
                best = Paint::solid(0.85 + 0.3 * along, 0.0);
            }
        }
        return best;
    }
    for i in 0..s.florets {
        let n = u64::from(i);
        let t = (f64::from(i) + 0.5) / count;
        // Racemes open from the bottom: the upper flowers are smaller buds.
        let envelope = 1.0 - 0.55 * t;
        let lateral = match i % 3 {
            0 => 0.38,
            1 => -0.38,
            _ => 0.0,
        };
        let radius = half * 0.5 * envelope * (0.88 + 0.24 * hash(n, 3, 23));
        let centre = (
            half * lateral * envelope,
            (s.stalk + length * (0.03 + 0.94 * t)).min(1.0 - radius * 0.8),
        );
        let (dx, dy) = ((x - centre.0) / radius, (y - centre.1) / (radius * 0.8));
        let d = dx * dx + dy * dy;
        if d < 1.0 {
            best = Paint::solid(0.82 + 0.3 * (1.0 - d), 0.0);
        }
    }
    best
}

/// `d` turned by `angle` radians, anticlockwise.
fn turn(d: (f64, f64), angle: f64) -> (f64, f64) {
    let (sin, cos) = (math::sin(angle), math::cos(angle));
    (d.0 * cos - d.1 * sin, d.0 * sin + d.1 * cos)
}

/// `d` scaled to unit length.
fn unit_vector(d: (f64, f64)) -> (f64, f64) {
    let length = hypot(d.0, d.1).max(1e-12);
    (d.0 / length, d.1 / length)
}

/// Spikelet length and half-width on a panicle, in card lengths.
const SPIKELET: (f64, f64) = (0.03, 0.0065);

/// One branch of a [`Panicle`] in card units: two straight pieces from the
/// axis, the outer turned further out, and two branchlets.
struct PanicleBranch {
    start: (f64, f64),
    knee: (f64, f64),
    end: (f64, f64),
    /// Each branchlet's base and tip.
    twigs: [((f64, f64), (f64, f64)); 2],
    /// Where spikelets cluster: the branch's tip and its branchlets' tips,
    /// with the way each points.
    tips: [((f64, f64), (f64, f64)); 3],
    /// How far the branchlets and spikelets reach past the pieces.
    pad: f64,
    key: u64,
}

impl PanicleBranch {
    /// Branch `i` of whorl `k`, shrunk to stay `margin` inside a card
    /// `half` wide on each side.
    fn new(s: &Panicle, k: u32, i: u32, half: f64, margin: f64) -> Self {
        let spread = math::radians(s.spread);
        // The lowest branches reach the card's sides.
        let longest = half / math::sin(spread).max(0.2);
        let u = (f64::from(k) + 0.3) / f64::from(s.whorls);
        let start = (0.0, s.stalk + (1.0 - s.stalk) * u * 0.9);
        let key = u64::from(k * 16 + i);
        let azimuth = 2.0 * PI * (f64::from(i) + hash(key, 1, 31)) / f64::from(s.branches)
            + f64::from(k) * 2.399_963;
        let side = math::cos(azimuth);
        let reach = longest * (1.0 - 0.75 * u) * (0.7 + 0.3 * hash(key, 2, 31));
        let a0 = spread * (0.8 + 0.4 * hash(key, 3, 31));
        let a1 = (a0 + 0.5 * spread).min(math::radians(80.0));
        let piece = 0.5 * reach;
        let first = (piece * math::sin(a0) * side, piece * math::cos(a0));
        let second = (piece * math::sin(a1) * side, piece * math::cos(a1));
        let wide = (first.0 + second.0).abs().max(first.0.abs());
        let high = first.1 + second.1;
        let mut keep: f64 = 1.0;
        if wide > 1e-12 {
            keep = keep.min((half - margin) / wide);
        }
        if high > 1e-12 {
            keep = keep.min((1.0 - margin - start.1) / high);
        }
        let keep = keep.clamp(0.0, 1.0);
        let first = (first.0 * keep, first.1 * keep);
        let second = (second.0 * keep, second.1 * keep);
        let knee = (start.0 + first.0, start.1 + first.1);
        let end = (knee.0 + second.0, knee.1 + second.1);
        let mut twigs = [(start, start); 2];
        let mut tips = [(end, unit_vector(second)); 3];
        for j in 0..2_u32 {
            let t = 0.4 + 0.45 * (f64::from(j) + hash(key, 4 + u64::from(j), 31)) / 2.0;
            let (from, direction) = if t < 0.5 {
                (
                    (start.0 + first.0 * t * 2.0, start.1 + first.1 * t * 2.0),
                    unit_vector(first),
                )
            } else {
                (
                    (
                        knee.0 + second.0 * (t - 0.5) * 2.0,
                        knee.1 + second.1 * (t - 0.5) * 2.0,
                    ),
                    unit_vector(second),
                )
            };
            let sign = if j == 0 { 1.0 } else { -1.0 };
            let direction = turn(
                direction,
                sign * math::radians(25.0 + 15.0 * hash(key, 6 + u64::from(j), 31)),
            );
            let twig = 0.3 * reach * keep * (1.0 - 0.4 * t);
            let tip = (from.0 + direction.0 * twig, from.1 + direction.1 * twig);
            twigs[j as usize] = (from, tip);
            tips[j as usize + 1] = (tip, direction);
        }
        Self {
            start,
            knee,
            end,
            twigs,
            tips,
            pad: 0.3 * reach * keep + SPIKELET.0,
            key,
        }
    }

    /// Whether `p` is far from every part of the branch.
    fn far(&self, p: (f64, f64)) -> bool {
        p.0 < self.start.0.min(self.end.0) - self.pad
            || p.0 > self.start.0.max(self.end.0) + self.pad
            || p.1 < self.start.1 - self.pad
            || p.1 > self.end.1.max(self.knee.1) + self.pad
    }

    /// Whether `p` lies on the branch or a branchlet, lines `fine` wide.
    fn line(&self, p: (f64, f64), fine: f64) -> bool {
        segment(p, self.start, self.knee).0 < fine * 0.7
            || segment(p, self.knee, self.end).0 < fine * 0.7
            || self
                .twigs
                .iter()
                .any(|&(from, tip)| segment(p, from, tip).0 < fine * 0.6)
    }

    /// The paint of the spikelet `p` lies in, if any: `count` spikelets in
    /// turn at the three tips, each further back from its tip than the
    /// last, fanned a little either way and tinged with up to 0.6 of the
    /// accent.
    fn spikelet(&self, p: (f64, f64), count: u32) -> Option<Paint> {
        let (length, thin) = SPIKELET;
        let mut paint = None;
        for m in 0..count {
            let (tip, direction) = self.tips[(m % 3) as usize];
            let back = f64::from(m / 3) * length * 0.55;
            let along = turn(
                direction,
                (hash(self.key, 10 + u64::from(m), 31) - 0.5) * 0.9,
            );
            let centre = (
                tip.0 - direction.0 * back + along.0 * length * 0.3,
                tip.1 - direction.1 * back + along.1 * length * 0.3,
            );
            let (a, b) = local(p, centre, along);
            let d = (a / (length * 0.5)).powi(2) + (b / thin).powi(2);
            if d < 1.0 {
                let tint = 0.6 * hash(self.key, 20 + u64::from(m), 31);
                paint = Some(Paint::solid(0.95 + 0.25 * (1.0 - d), tint));
            }
        }
        paint
    }
}

/// An open grass panicle seen from the side: a stalk, an axis, and whorls
/// of hair-fine branches, shorter toward the tip. Each branch leaves the
/// axis at about `spread` and turns further out halfway along, and forks
/// into two branchlets; the spikelets cluster at the tips of the branch
/// and its branchlets, so they hang in a loose cloud rather than in rows.
/// The branches of a whorl point different ways round the axis, so seen
/// from the side some reach out further than others. Stalk, axis and
/// branches are a little dull; spikelets are bright.
fn panicle(s: &Panicle, x: f64, y: f64, fine: f64) -> Paint {
    let wood = Paint::solid(0.8, 0.2);
    if y < s.stalk {
        return if x.abs() < fine * 1.2 {
            wood
        } else {
            Paint::EMPTY
        };
    }
    let p = (x, y);
    let mut lines = y < 0.97 && x.abs() < fine * (1.0 - 0.4 * (y - s.stalk) / (1.0 - s.stalk));
    let mut spikes = None;
    for k in 0..s.whorls {
        for i in 0..s.branches {
            let branch = PanicleBranch::new(s, k, i, s.aspect * 0.5, SPIKELET.0 + fine);
            if branch.far(p) {
                continue;
            }
            lines = lines || branch.line(p, fine);
            spikes = branch.spikelet(p, s.spikelets).or(spikes);
        }
    }
    spikes.unwrap_or(if lines { wood } else { Paint::EMPTY })
}

/// A berry, fruit or cone seen from the side, on a short stalk in the
/// accent colour.
fn fruit(s: &Fruit, x: f64, y: f64, fine: f64) -> Paint {
    if !crate::fruit::is_single(s) {
        return fruit_cluster(s, x, y, fine);
    }
    if y < 0.08 {
        return if x.abs() < fine * 1.2 {
            Paint::solid(0.85, 1.0)
        } else {
            Paint::EMPTY
        };
    }
    let across = s.aspect * 0.5 * 0.94;
    let (dx, dy) = (x / across, (y - 0.53) / 0.45);
    let mut d = dx * dx + dy * dy;
    if s.cone > 0.0 {
        // Scales in crossing spiral rows, each lighter toward its exposed
        // tip; the tips scallop the outline.
        let rows = 7.0;
        let a = (y - 0.08) / 0.9 * rows + x / across * 2.0;
        let b = (y - 0.08) / 0.9 * rows - x / across * 2.0;
        let (fa, fb) = (frac(a), frac(b));
        let tip = fa.min(fb);
        let scallop = 0.1 * s.cone * (1.0 - math::pow((fa - fb).abs(), 0.5));
        d *= 1.0 - scallop * (dx * dx).min(1.0);
        if d >= 1.0 {
            return Paint::EMPTY;
        }
        let body = 0.7 + 0.4 * math::sqrt(1.0 - d);
        let scales = 0.55 + 0.75 * tip;
        return Paint::solid(body * (1.0 - s.cone) + scales * body * s.cone, 0.0);
    }
    if d >= 1.0 {
        return Paint::EMPTY;
    }
    // A round body lit from the upper left, with a highlight.
    let highlight = math::exp(-((dx + 0.35).powi(2) + (dy - 0.4).powi(2)) * 9.0);
    Paint::solid(0.68 + 0.4 * math::sqrt(1.0 - d) + 0.5 * highlight, 0.0)
}

/// A cluster, or a fruit of a kind beyond the plain berry (plant roadmap
/// P4): the layout the solid is drawn from (`crate::fruit`), seen face on
/// and unturned, with the frontmost fruit or stalk drawn at each point.
/// Stalks and unripe fruit take the accent colour; a single fruit is as
/// unripe as the template's share, its mean colour.
fn fruit_cluster(s: &Fruit, x: f64, y: f64, fine: f64) -> Paint {
    let placed = crate::fruit::layout(s, 0.0, 0);
    let mut best: Option<(f64, Paint)> = None;
    let mut consider = |depth: f64, paint: Paint| {
        if paint.cover > 0.0 && best.is_none_or(|(front, _)| depth > front) {
            best = Some((depth, paint));
        }
    };
    let single = placed.len() == 1;
    let reach = crate::fruit::axis_length(s, &placed);
    if reach > 0.0 {
        // The cluster's own axis.
        let (distance, _) = segment((x, y), (0.0, 0.0), (0.0, reach));
        if distance < fine * 1.4 {
            consider(-2.0, Paint::solid(0.8, 1.0));
        }
    }
    for p in &placed {
        if (p.base - p.attach).length() > 1e-9 {
            let (distance, _) = segment((x, y), (p.attach.x, p.attach.y), (p.base.x, p.base.y));
            if distance < fine * 1.1 {
                consider(p.attach.z.min(p.base.z) - 1.0, Paint::solid(0.8, 1.0));
            }
        }
        let mut paint = one_fruit(s, p, x, y, fine);
        if single {
            // A ripening fruit blushes: its share still unripe in the
            // accent colour, from the shaded lower side up.
            let (along, across) = local((x, y), (p.base.x, p.base.y), (0.0, 1.0));
            let side = 0.5 * (along + 0.5 * across / (s.aspect * 0.5).max(0.05) + 0.5);
            let green = 1.0 - math::smoothstep(s.unripe - 0.15, s.unripe + 0.15, side);
            paint.accent = paint.accent.max(if s.unripe > 0.0 { green } else { 0.0 });
        } else if p.unripe {
            paint.accent = 1.0;
        }
        consider(p.base.z + p.direction.z * p.length * 0.5, paint);
    }
    best.map_or(Paint::EMPTY, |(_, paint)| paint)
}

/// One fruit of a cluster at card point `(x, y)`: its kind's outline and
/// shading along its projected axis.
#[allow(clippy::too_many_lines)]
fn one_fruit(s: &Fruit, p: &crate::fruit::Placed, x: f64, y: f64, fine: f64) -> Paint {
    use crate::looks::FruitKind;
    let half_width = (p.length * s.aspect * 0.5).max(fine);
    let projected = hypot(p.direction.x, p.direction.y);
    let axis = if projected > 0.05 {
        (p.direction.x / projected, p.direction.y / projected)
    } else {
        (0.0, 1.0)
    };
    // Seen from the side the fruit is its length; seen end on, round.
    let length = (p.length * projected).max(2.0 * half_width);
    let (along, across) = local((x, y), (p.base.x, p.base.y), axis);
    let t = along / length;
    let v = across / half_width;
    if !(-0.05..=1.05).contains(&t) {
        return Paint::EMPTY;
    }
    // An ellipse centred at `middle` along the fruit, `reach` long each
    // way, `wide` of the half-width: its d², under 1 inside.
    let ellipse = |middle: f64, reach: f64, wide: f64| {
        let dt = (t - middle) / reach;
        let dv = v / wide;
        dt * dt + dv * dv
    };
    let ball = |d: f64, middle: f64, reach: f64| {
        let dt = (t - middle) / reach;
        let highlight = math::exp(-((v + 0.35).powi(2) + (dt - 0.4).powi(2)) * 9.0);
        0.68 + 0.4 * math::sqrt((1.0 - d).max(0.0)) + 0.5 * highlight
    };
    if s.cone >= 0.5 {
        let d = ellipse(0.5, 0.5, 1.0);
        if d >= 1.0 {
            return Paint::EMPTY;
        }
        let rows = 7.0;
        let tip = frac(t * rows + v).min(frac(t * rows - v));
        return Paint::solid(ball(d, 0.5, 0.5) * (0.55 + 0.75 * tip) * 0.85, 0.0);
    }
    match s.kind {
        FruitKind::Berry => {
            let d = ellipse(0.5, 0.5, 1.0);
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            Paint::solid(ball(d, 0.5, 0.5), 0.0)
        }
        FruitKind::Crowned | FruitKind::Pome => {
            let wide = if s.kind == FruitKind::Pome { 1.08 } else { 1.0 };
            let d = ellipse(0.5, 0.5, wide);
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            // The calyx's dark eye at the tip.
            let eye = ellipse(0.93, 0.07, 0.28);
            let bright = if eye < 1.0 { 0.4 } else { ball(d, 0.5, 0.5) };
            Paint::solid(bright, 0.0)
        }
        FruitKind::Drupelets => {
            // Drupelets in offset rows scallop the outline.
            let cell = 0.32;
            let row = (t * 0.5 / cell * (length / half_width)).floor();
            let offset = if row % 2.0 == 0.0 { 0.0 } else { 0.5 };
            let fu = frac(t * 0.5 / cell * (length / half_width));
            let fv = frac(v / (2.0 * cell) + offset);
            let bump = (1.0 - ((fu - 0.5).powi(2) + (fv - 0.5).powi(2)) * 4.0).clamp(0.0, 1.0);
            let d = ellipse(0.5, 0.5, 1.0) * (1.0 + 0.08 * (1.0 - bump));
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            Paint::solid(0.5 + 0.55 * bump + 0.25 * math::sqrt(1.0 - d), 0.0)
        }
        FruitKind::Hip => {
            let d = ellipse(0.4, 0.4, 1.0);
            if d < 1.0 {
                return Paint::solid(ball(d, 0.4, 0.4), 0.0);
            }
            // Three of the five sepals spreading from the mouth.
            for k in [-1.0, 0.0, 1.0] {
                let angle = math::radians(32.0) * k;
                let reach = 0.2;
                let end = (
                    0.78 + reach * math::cos(angle),
                    math::sin(angle) * reach * length / half_width,
                );
                let (distance, _) = segment((t, v), (0.78, 0.0), end);
                if distance * half_width < fine * 1.2 {
                    return Paint::solid(0.75, 1.0);
                }
            }
            Paint::EMPTY
        }
        FruitKind::Samara => {
            // The seed in the first fifth.
            if ellipse(0.11, 0.11, 0.34) < 1.0 {
                return Paint::solid(0.72, 0.0);
            }
            // The wing beyond it, widest past its middle, a little to one
            // side, with veins fanning from the seed.
            let w = ((t - 0.12) / 0.88).clamp(0.0, 1.0);
            if t < 0.12 {
                return Paint::EMPTY;
            }
            let half = math::pow(math::sin(math::PI * math::pow(w, 0.8)).max(0.0), 0.7);
            let shifted = v + 0.35 * w * (1.0 - w);
            if shifted.abs() >= half {
                return Paint::EMPTY;
            }
            let veins = 0.5 + 0.5 * math::cos(28.0 * (w + 0.35 * shifted));
            Paint::solid(0.82 + 0.18 * veins - 0.15 * w * w, 0.0)
        }
        FruitKind::Acorn => {
            // The cup over the nut's first third, in the accent colour.
            let cup = ellipse(0.18, 0.2, 1.06);
            if cup < 1.0 && t < 0.36 {
                // Overlapping scales in offset rows.
                let row = (t * 26.0).floor();
                let scale = frac(v * 3.0 + if row % 2.0 == 0.0 { 0.0 } else { 0.5 });
                let edge = frac(t * 26.0);
                return Paint::solid(
                    0.55 + 0.25 * edge + 0.15 * (1.0 - (scale - 0.5).abs() * 2.0),
                    1.0,
                );
            }
            let d = ellipse(0.55, 0.45, 0.94);
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            Paint::solid(ball(d, 0.55, 0.45), 0.0)
        }
        FruitKind::Husked => {
            // A husk round the nut, drawn out into a beak; all accent.
            let bulb = ellipse(0.22, 0.22, 1.0);
            // The husk's half-width over the fruit's: 0.84 where the beak
            // starts, 0.2 at its end.
            let beak = 0.84 - 0.64 * math::sqrt(((t - 0.42) / 0.58).clamp(0.0, 1.0));
            if bulb < 1.0 || ((0.3..=1.0).contains(&t) && v.abs() < beak) {
                let bristle = hash(
                    (t * 90.0).floor().to_bits(),
                    (v * 12.0).floor().to_bits(),
                    0x4A2E,
                );
                let round = if bulb < 1.0 {
                    0.25 * math::sqrt(1.0 - bulb)
                } else {
                    0.0
                };
                return Paint::solid(0.68 + 0.12 * bristle + round, 1.0);
            }
            Paint::EMPTY
        }
        FruitKind::Pod => {
            let bulge = 1.0 + 0.1 * math::cos(std::f64::consts::TAU * 5.0 * t);
            let d = ellipse(0.5, 0.5, bulge);
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            let seeds = (math::cos(std::f64::consts::TAU * 5.0 * t)).max(0.0);
            Paint::solid(0.65 + 0.25 * seeds + 0.2 * math::sqrt(1.0 - d), 0.0)
        }
        FruitKind::Capsule => {
            let d = ellipse(0.5, 0.5, 1.0);
            if d >= 1.0 {
                return Paint::EMPTY;
            }
            let rib = [-0.6, 0.0, 0.6]
                .iter()
                .any(|r| (v - r).abs() < 0.07 + fine / half_width);
            Paint::solid(if rib { 0.5 } else { ball(d, 0.5, 0.5) }, 0.0)
        }
    }
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::looks::{self, OrganLook};
    use crate::lsys::OrganKind;

    fn looks_of(shapes: &[Shape]) -> Vec<Look> {
        let names: Vec<String> = (0..shapes.len()).map(|i| format!("organ{i}")).collect();
        let mut organs = BTreeMap::new();
        for (name, shape) in names.iter().zip(shapes) {
            organs.insert(
                name.clone(),
                OrganLook {
                    shape: shape.clone(),
                    colour: Some([0.2, 0.4, 0.1]),
                    shade: None,
                    accent: Some([0.8, 0.7, 0.1]),
                    face_up: 0.0,
                    solid: None,
                    bend: None,
                    form: None,
                    season: None,
                    families: None,
                },
            );
        }
        looks::resolve(
            names.iter().map(|name| (name.as_str(), OrganKind::Leaf)),
            &organs,
            [0.1, 0.3, 0.1],
            [0.05, 0.1, 0.05],
        )
    }

    /// An organ a program declares without an area shades with about the
    /// area its kind's default look draws.
    #[test]
    fn default_areas_match_default_looks() {
        for kind in [
            OrganKind::Foliage,
            OrganKind::Leaf,
            OrganKind::Flower,
            OrganKind::Fruit,
            OrganKind::Cone,
        ] {
            let looks = looks::resolve(
                [("organ", kind)],
                &BTreeMap::new(),
                [0.1, 0.3, 0.1],
                [0.05, 0.1, 0.05],
            );
            let templates = Templates::for_looks(&looks);
            let drawn = drawn_area(&looks[0], &templates.templates[0]);
            let shaded = kind.default_area();
            assert!(
                (shaded - drawn).abs() <= 0.05 * drawn,
                "{kind:?}: the default look draws {drawn:.3} m² but shades with {shaded:.3} m²"
            );
        }
    }

    fn every_shape() -> Vec<Shape> {
        vec![
            Shape::Needles(Needles::default()),
            Shape::Needles(Needles {
                ranks: 0,
                ..Needles::default()
            }),
            Shape::Scales(Scales::default()),
            Shape::Palmate(Palmate::default()),
            Shape::Simple(Simple::default()),
            Shape::Simple(Simple {
                teeth: 14,
                ..Simple::default()
            }),
            Shape::Lobed(Lobed::default()),
            Shape::Compound(Compound::default()),
            Shape::Sprig(Sprig::default()),
            Shape::Sprig(Sprig {
                leaves: 11,
                angle: 70.0,
                teeth: 16,
                ..Sprig::default()
            }),
            Shape::Blade(Blade::default()),
            Shape::Blade(Blade {
                taper: 1.0,
                ..Blade::default()
            }),
            Shape::Frond(Frond::default()),
            Shape::Flower(Flower::default()),
            Shape::Head(Head::default()),
            Shape::Umbel(Umbel::default()),
            Shape::Spike(Spike::default()),
            Shape::Spike(Spike {
                plume: 1.0,
                ..Spike::default()
            }),
            Shape::Panicle(Panicle::default()),
            Shape::Fruit(Fruit::default()),
            Shape::Fruit(Fruit {
                aspect: 0.5,
                cone: 1.0,
                ..Fruit::default()
            }),
        ]
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn leaves_are_lit_along_their_grown_veins() {
        // Every leaf shape grows its veins and no other shape does; on a
        // simple, lobed, palmate or compound leaf they lighten a share of
        // the blade off its midrib (whose texels carry the accent).
        for shape in every_shape() {
            let leaf = matches!(
                shape,
                Shape::Simple(_)
                    | Shape::Lobed(_)
                    | Shape::Palmate(_)
                    | Shape::Compound(_)
                    | Shape::Sprig(_)
            );
            let veins = grown_veins(&shape);
            assert_eq!(veins.is_some(), leaf, "{shape:?}");
            let Some(veins) = veins else { continue };
            assert!(
                veins.segments().len() > 40,
                "{shape:?}: {}",
                veins.segments().len()
            );
            if matches!(shape, Shape::Sprig(_)) {
                continue;
            }
            let looks = looks_of(std::slice::from_ref(&shape));
            let template = &Templates::for_looks(&looks).templates[0];
            let blade = (0..template.coverage.len())
                .filter(|&i| template.coverage[i] > 0.5 && template.accent[i] == 0.0)
                .collect::<Vec<_>>();
            let lit = blade
                .iter()
                .filter(|&&i| template.brightness[i] > 1.025)
                .count();
            let share = lit as f64 / blade.len() as f64;
            assert!((0.02..0.4).contains(&share), "{shape:?}: {share}");
        }
    }

    #[test]
    fn a_vein_runs_to_every_tooth() {
        let s = Simple {
            teeth: 14,
            ..Simple::default()
        };
        let length = 1.0 - s.petiole;
        let fade =
            |t: f64| math::smoothstep(0.04, 0.18, t) * (1.0 - math::smoothstep(0.88, 1.0, t));
        let tips = tooth_tips(s.teeth, 0.0, |t| simple_width(&s, length, t) / length, fade);
        assert!(tips.len() >= 16, "{}", tips.len());
        let segments = grown_veins(&Shape::Simple(s)).unwrap().segments();
        for tip in tips {
            let reach = segments
                .iter()
                .map(|&(_, b, _)| (b.0 - tip.0).hypot(b.1 - tip.1))
                .fold(f64::INFINITY, f64::min);
            assert!(reach < 0.07, "{tip:?}: {reach}");
        }
    }

    #[test]
    fn every_template_covers_a_plausible_share_of_its_card() {
        let shapes = every_shape();
        let templates = Templates::for_looks(&looks_of(&shapes));
        let size = templates.size;
        let mut problems = Vec::new();
        for (index, (shape, template)) in shapes.iter().zip(&templates.templates).enumerate() {
            let name = format!("{index} {}", shape.name());
            let covered = template
                .coverage
                .iter()
                .filter(|coverage| **coverage >= 0.5)
                .count();
            #[allow(clippy::cast_precision_loss)]
            let fraction = covered as f32 / template.coverage.len() as f32;
            // Blade pieces fill their card, less its border.
            let tiles = matches!(shape, Shape::Blade(blade) if blade.taper < 1.0);
            let most = if tiles { 0.95 } else { 0.9 };
            if !(0.03..most).contains(&fraction) {
                problems.push(format!("{name} covers {fraction}"));
            }
            // Nothing is cut off at the card's sides or tip: the outermost
            // columns and the top row stay clear.
            let sides = (0..size)
                .filter(|row| {
                    [0, size - 1]
                        .iter()
                        .any(|column| template.coverage[row * size + column] >= 0.5)
                })
                .count();
            if sides > 0 {
                problems.push(format!(
                    "{name} touches the sides of its card in {sides} rows"
                ));
            }
            let tip = (0..size)
                .filter(|column| template.coverage[(size - 1) * size + column] >= 0.5)
                .count();
            if tip > 0 && !tiles {
                problems.push(format!(
                    "{name} touches the tip of its card in {tip} columns"
                ));
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn leaves_hang_from_their_petioles_and_flowers_show_their_centres() {
        let shapes = [
            Shape::Palmate(Palmate::default()),
            Shape::Flower(Flower::default()),
        ];
        let templates = Templates::for_looks(&looks_of(&shapes));
        // The petiole is at the base, centred, in the accent colour.
        let petiole = templates.sample(0, 0.5, 0.05);
        assert_eq!(petiole.coverage, 1.0);
        assert!(petiole.accent > 0.9);
        assert_eq!(templates.sample(0, 0.05, 0.05).coverage, 0.0);
        // The flower's centre is accent; a petal halfway out is not.
        let centre = templates.sample(1, 0.5, 0.5);
        assert_eq!(centre.coverage, 1.0);
        assert!(centre.accent > 0.9);
        let petal = templates.sample(1, 0.5, 0.82);
        assert_eq!(petal.coverage, 1.0);
        assert!(petal.accent < 0.2);
    }

    #[test]
    fn accents_take_the_card_s_shading() {
        let templates = Templates::for_looks(&looks_of(&[Shape::Flower(Flower::default())]));
        let accent = Texel {
            coverage: 1.0,
            brightness: 1.0,
            accent: 1.0,
        };
        let lit = templates.albedo(0, [0.2, 0.4, 0.1], accent);
        let shaded = templates.albedo(0, [0.1, 0.2, 0.05], accent);
        // In full light the accent is the accent colour; at half the
        // brightness, half of it.
        for channel in 0..3 {
            let expected = f64::from([0.8_f32, 0.7, 0.1][channel]);
            assert!((lit[channel] - expected).abs() < 1e-6);
            assert!((shaded[channel] - expected * 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn templates_are_the_same_every_time() {
        let shapes = every_shape();
        let looks = looks_of(&shapes);
        assert_eq!(Templates::for_looks(&looks), Templates::for_looks(&looks));
    }
}
