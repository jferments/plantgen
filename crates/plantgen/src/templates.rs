//! Organ card textures.
//!
//! Every organ card samples one square template: coverage, brightness,
//! accent weight and hair weight (`crate::hairs`) per texel. A species' templates are drawn from its looks
//! (see [`crate::looks`]), one per organ type of its program, by a small
//! procedural generator per [`Shape`]: needle shoots, scale sprays,
//! palmate, simple, lobed and compound leaves, leafy sprigs, grass blades,
//! fern fronds, flowers, flower heads, umbels, spikes, panicles and fruit.
//! Simple, lobed, palmate and compound leaves and the leaves of sprigs are
//! grown ([`grow_leaf`]): their teeth and lobes laid down on their margins
//! as the blade grows ([`crate::margins`]; Runions, Tsiantis and
//! Prusinkiewicz 2017), and their veins grown by space colonization
//! ([`crate::venation`]; Runions et al. 2005) from the primary veins their
//! generator draws, one into every tooth, which points the way its vein
//! runs. They are lit along their veins.
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
use crate::divided::{self, Form, Layout};
use crate::looks::{
    Blade, Compound, Flower, Frond, Fruit, Head, Lobed, Look, Needles, Palmate, Panicle, Pattern,
    Scales, Shape, Simple, Spike, Sprig, Umbel,
};
use crate::margins::{self, Drawn, Margin};
use crate::math::{self, PI};
use crate::rng::{mix64, unit};
use crate::spines::{self, View};
use crate::venation::{Growth, KILL, Lamina, Venation};

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
    /// How much of the texel's colour is its hairs', 0 (a bare face) to 1
    /// (a hair of the edge's fringe), which also weighs their sheen
    /// (`crate::hairs`).
    pub hair: f32,
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
    /// Each texel's hair weight ([`Texel::hair`]), and the coverage the
    /// edge's fringe of hairs added, which is no leaf; empty for a
    /// template without hairs.
    pub hair: Vec<f32>,
    pub fringe: Vec<f32>,
    /// The hairs of its look, where it has any (`crate::hairs`).
    pub hairs: Option<crate::looks::Hairs>,
}

impl Template {
    /// Leaf area the template draws on a card one metre long, m²: the
    /// card's width times the share of it covered.
    #[must_use]
    pub fn area(&self) -> f64 {
        let covered: f64 = self.coverage.iter().map(|&value| f64::from(value)).sum();
        let fringe: f64 = self.fringe.iter().map(|&value| f64::from(value)).sum();
        self.aspect * (covered - fringe) / to_f64(self.coverage.len().max(1))
    }
}

/// Leaf area an organ 1 m long draws with `look` and its `template`, m²:
/// the card's covered area, plus the crossing card's for looks that have
/// one. A cluster's card is its own length (`crate::clusters::CardFrame`).
#[must_use]
pub fn drawn_area(look: &Look, template: &Template) -> f64 {
    let area = template.area() * (1.0 + look.cross().unwrap_or(0.0));
    match &look.inflorescence {
        Some(cluster) => area * cluster.card.length * cluster.card.length,
        None => area,
    }
}

/// The share of a card `look` draws solid: its texels at least half
/// covered, which a renderer's cut-out keeps, sampled once per texel of a
/// [`FILL_SAMPLES`]-wide grid. Cluster cards (`crate::mesh`) need it to
/// count how much of a cell overlapping organs fill. Remembered per shape
/// (and cluster), since a plant asks for it at every level of every
/// keyframe.
#[must_use]
pub fn fill_share(look: &Look) -> f64 {
    static SHARES: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String, f64>>> =
        std::sync::OnceLock::new();
    let shape = &look.shape;
    let key = match &look.inflorescence {
        Some(cluster) => format!("{shape:?} {cluster:?}"),
        None => format!("{shape:?} {:?}", look.insertion),
    };
    let shares = SHARES.get_or_init(Default::default);
    if let Some(&share) = shares.lock().ok().as_ref().and_then(|s| s.get(&key)) {
        return share;
    }
    let aspect = look.aspect();
    let grew = grow_leaf(shape);
    let card = look
        .inflorescence
        .as_ref()
        .map(|cluster| ClusterCard::new(cluster, (look.colour, look.accent)));
    let mut kept = 0_usize;
    for y in 0..FILL_SAMPLES {
        for x in 0..FILL_SAMPLES {
            let u = (to_f64(x) + 0.5) / to_f64(FILL_SAMPLES);
            let v = (to_f64(y) + 0.5) / to_f64(FILL_SAMPLES);
            let paint = draw_look(look, card.as_ref(), (u - 0.5) * aspect, v, grew.as_ref());
            if paint.cover >= 0.5 {
                kept += 1;
            }
        }
    }
    let solid = to_f64(kept) / to_f64(FILL_SAMPLES * FILL_SAMPLES);
    if let Ok(mut shares) = shares.lock() {
        shares.insert(key, solid);
    }
    solid
}

/// Samples a side for [`fill_share`].
pub const FILL_SAMPLES: usize = 64;

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
                    hair: Vec::new(),
                    fringe: Vec::new(),
                    hairs: None,
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
                let aspect = look.aspect();
                let grew = grow_leaf(&look.shape);
                let card = look
                    .inflorescence
                    .as_ref()
                    .map(|cluster| ClusterCard::new(cluster, (look.colour, look.accent)));
                for y in 0..TEMPLATE_SIZE {
                    for x in 0..TEMPLATE_SIZE {
                        // Four samples per texel give soft edges.
                        let mut sum = Paint::EMPTY;
                        for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                            let u = (to_f64(x) + dx) / to_f64(TEMPLATE_SIZE);
                            let v = (to_f64(y) + dy) / to_f64(TEMPLATE_SIZE);
                            let paint = draw_look(
                                look,
                                card.as_ref(),
                                (u - 0.5) * aspect,
                                v,
                                grew.as_ref(),
                            );
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
                let painted = look.hairs.as_ref().map(|hairs| {
                    crate::hairs::paint(
                        TEMPLATE_SIZE,
                        aspect,
                        2,
                        hairs,
                        mix64(shape_seed(&look.shape) ^ 0x4841_4952),
                        &mut coverage,
                        &mut brightness,
                        &mut accent,
                    )
                });
                let (hair, fringe) = painted.map_or((Vec::new(), Vec::new()), |painted| {
                    (painted.hair, painted.fringe)
                });
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
                    hair,
                    fringe,
                    hairs: look.hairs.clone(),
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
                hair: 0.0,
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
            hair: template.hair.get(at).copied().unwrap_or(0.0),
        }
    }

    /// Albedo of a card texel: the card's colour, blended toward the look's
    /// accent colour by the texel's accent weight, times its brightness.
    /// The accent colour is darkened as much as the card's colour is
    /// darker than the look's colour, so shaded flowers stay shaded. Its
    /// hairs then move it toward their colour, darkened alike, by the
    /// texel's hair weight.
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
        let hair = f64::from(texel.hair);
        let hairs = template.hairs.as_ref().map_or([0.0; 3], |hairs| hairs.colour);
        std::array::from_fn(|channel| {
            let accent = f64::from(template.accent_colour[channel]) * darkening;
            let face = (colour[channel] + (accent - colour[channel]) * weight) * brightness;
            face + (f64::from(hairs[channel]) * darkening - face) * hair
        })
    }

    /// The templates side by side as an RGBA8 image, the package's organ
    /// atlas: brightness / 2 in red, the accent weight in green, coverage
    /// in alpha, and the hair weight in blue (`crate::hairs`; 0 where a
    /// card has no hairs). Row 0 is the cards' base (`v` = 0).
    #[must_use]
    pub fn atlas_rgba(&self) -> (usize, usize, Vec<u8>) {
        self.image(|index, at| {
            let template = &self.templates[index];
            [
                crate::raster::to_u8(template.brightness[at] * 0.5),
                crate::raster::to_u8(template.accent[at]),
                crate::raster::to_u8(template.hair.get(at).copied().unwrap_or(0.0)),
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
                        hair: template.hair.get(at).copied().unwrap_or(0.0),
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

/// The point `x`, `y` of `look`'s card: its cluster's `card` where it is
/// a cluster, else its shape's.
fn draw_look(
    look: &Look,
    card: Option<&ClusterCard>,
    x: f64,
    y: f64,
    grew: Option<&GrownLeaf>,
) -> Paint {
    match card {
        Some(card) => {
            let (across, along) = drawn_share(&look.shape);
            card.paint(&look.shape, x / across, y / along)
        }
        None => {
            let paint = draw(&look.shape, (look.colour, look.accent), x, y, grew);
            // A leaf drawn on a petiole of its own leaves the petiole,
            // painted in full accent below its blade, off its card.
            match look.stalk() {
                Some(stalk) if y < stalk.petiole + 0.01 && paint.accent >= 0.99 => Paint::EMPTY,
                _ => paint,
            }
        }
    }
}

/// A card's two colours: its colour and its accent.
type Colours = ([f32; 3], [f32; 3]);

/// The point `x`, `y` of a card drawn with `shape` in `colours`, its
/// colour and its accent, its leaf drawn as it `grew` (see [`grow_leaf`])
/// where it is one.
fn draw(shape: &Shape, colours: Colours, x: f64, y: f64, grew: Option<&GrownLeaf>) -> Paint {
    // Shapes are drawn into the card less its border: stretch the point
    // instead.
    let (across, along) = drawn_share(shape);
    let (x, y) = (x / across, y / along);
    let fine = fine(shape);
    let veins = grew.map(|leaf| &leaf.veins);
    let margins = grew.map_or(&[][..], |leaf| &leaf.margins);
    match shape {
        Shape::Needles(s) => needles(s, x, y, fine),
        Shape::Scales(s) => scales(s, x, y, fine),
        Shape::Palmate(s) => palmate(s, x, y, fine, veins, margins),
        Shape::Simple(s) => simple(s, x, y, fine, veins, margins),
        Shape::Lobed(s) => lobed(s, x, y, fine, veins, margins),
        Shape::Compound(s) => {
            let own;
            let laid = if let Some(laid) = grew.and_then(|leaf| leaf.layout.as_ref()) {
                laid
            } else {
                own = divided::layout(s);
                &own
            };
            compound(s, laid, x, y, fine, veins, margins)
        }
        Shape::Sprig(s) => sprig(s, x, y, fine, veins, margins),
        Shape::Blade(s) => blade(s, x, y),
        Shape::Frond(s) => frond(s, x, y, fine),
        Shape::Flower(s) => flower(s, colours, x, y - 0.5),
        Shape::Head(s) => head(s, colours, x, y - 0.5),
        Shape::Umbel(s) => umbel(s, x, y - 0.5),
        Shape::Spike(s) => spike(s, x, y, fine),
        Shape::Panicle(s) => panicle(s, x, y, fine),
        Shape::Spikelet(s) => spikelet(s, x, y, fine),
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

/// A blade's half-width at `t` from base to tip, 1 where it is widest:
/// through its `outline`'s points if it has one, scaled so the widest is
/// 1, else from where it is `widest`, its `base` and its `tip`
/// ([`blade_profile`]).
pub(crate) fn outline_profile(
    outline: Option<&[[f64; 2]]>,
    t: f64,
    widest: f64,
    base: f64,
    tip: f64,
) -> f64 {
    let Some(points) = outline else {
        return blade_profile(t, widest, base, tip);
    };
    if !(0.0..=1.0).contains(&t) {
        return 0.0;
    }
    // The curve never passes its points, so its widest is theirs.
    let most = points.iter().fold(0.0, |most: f64, [_, w]| most.max(*w));
    if most > 0.0 {
        math::monotone_cubic(points, t) / most
    } else {
        0.0
    }
}

/// Where a blade's teeth fade in from its base and out toward its tip.
fn blade_fade(t: f64) -> f64 {
    math::smoothstep(0.04, 0.18, t) * (1.0 - math::smoothstep(0.88, 1.0, t))
}

/// Where a leaflet's teeth fade in and out.
fn leaflet_fade(t: f64) -> f64 {
    math::smoothstep(0.1, 0.25, t) * (1.0 - math::smoothstep(0.85, 1.0, t))
}

/// The share of a blade, along it, that a simple leaf's, a sprig leaf's
/// and a leaflet's teeth and a lobed leaf's lobes form in.
const TEETH: (f64, f64) = (0.08, 0.95);
const LEAFLET_TEETH: (f64, f64) = (0.14, 0.92);
const LOBES: (f64, f64) = (0.06, 0.9);
/// The share of a palmate lobe, along it, that its teeth form in.
const LOBE_TEETH: (f64, f64) = (0.42, 0.92);
/// How teeth and lobes are drawn (see [`Drawn`]): a tooth's sinus moves
/// well toward the tooth below as its vein leans, a lobe's less; teeth
/// differ less in size than lobes.
const TOOTH: Drawn = Drawn {
    round: 0.0,
    skew: 0.4,
    least: 0.6,
};
const LOBE: Drawn = Drawn {
    round: 0.0,
    skew: 0.2,
    least: 0.45,
};
const PALMATE_TOOTH: Drawn = Drawn {
    round: 0.0,
    skew: 0.35,
    least: 0.6,
};
/// The most a blade's teeth cut into it, a share of its half-width.
const DEEPEST: f64 = 0.85;

/// A simple leaf's half-width at `t` along its blade of `length` card
/// units, with the teeth of its side's `margin` if given.
fn simple_width(s: &Simple, length: f64, t: f64, margin: Option<&Margin>) -> f64 {
    let profile = outline_profile(s.outline.as_deref(), t, s.widest, s.base, s.tip);
    let mut width = s.width * length * 0.5 * profile;
    if let Some(margin) = margin {
        let fall = margin.fall(t, TOOTH, |share| margins::tooth_flank(share, 0.0));
        width -= (s.tooth_depth * blade_fade(t)).min(DEEPEST * width) * fall;
    }
    width
}

/// How far a lobe's margin falls back along its flank, from its tip (0)
/// to its sinus (1): pointed lobes (`round` 0) narrow straight, rounded
/// ones swell (`round` 1).
fn lobe_flank(share: f64, round: f64) -> f64 {
    let pointed = 1.0 - share;
    let rounded = math::cos(0.5 * PI * share);
    1.0 - math::pow((pointed + (rounded - pointed) * round).max(0.0), 0.7)
}

/// A pinnately lobed leaf's outline over its lobes' tips, at `t` along
/// its blade: its half-width as a share of its widest.
pub(crate) fn lobed_envelope(t: f64) -> f64 {
    blade_profile(t, 0.55, 0.35, 0.3)
}

/// A pinnately lobed leaf's half-width at `t` along its blade of `length`
/// card units, with the lobes of its side's `margin` if given.
fn lobed_width(s: &Lobed, length: f64, t: f64, margin: Option<&Margin>) -> f64 {
    let envelope = lobed_envelope(t);
    let mut sinus = 1.0;
    if let Some(margin) = margin
        && (LOBES.0..LOBES.1).contains(&t)
    {
        let drawn = Drawn {
            round: s.round,
            ..LOBE
        };
        let fall = margin.fall(t, drawn, |share| lobe_flank(share, s.round));
        // Sinuses shallow out toward the base and the tip.
        let fade = math::sin(PI * (t - LOBES.0) / (LOBES.1 - LOBES.0));
        sinus = 1.0 - s.depth * fall * math::pow(fade, 0.4);
    }
    s.width * length * 0.5 * envelope * sinus
}

/// A leaflet's half-width at `t` along its blade, for a blade `length`
/// card units long of a leaf `s` of `form`, with the teeth of its side's
/// `margin` if given.
fn leaflet_width(s: &Compound, form: &Form, length: f64, t: f64, margin: Option<&Margin>) -> f64 {
    let profile = outline_profile(s.outline.as_deref(), t, form.widest, form.base, form.tip);
    let mut width = s.leaflet_width * length * 0.5 * profile;
    if let Some(margin) = margin {
        let fall = margin.fall(t, TOOTH, |share| margins::tooth_flank(share, 0.0));
        width -= (0.036 * length * leaflet_fade(t)).min(DEEPEST * width) * fall;
    }
    width
}

/// A sprig leaf's half-width at `t` along its blade of `blade` card units,
/// with the teeth of its side's `margin` if given.
fn sprig_width(s: &Sprig, blade: f64, t: f64, margin: Option<&Margin>) -> f64 {
    let profile = outline_profile(s.outline.as_deref(), t, s.widest, s.base, s.tip);
    let mut width = s.width * blade * 0.5 * profile;
    if let Some(margin) = margin {
        let drawn = Drawn {
            round: s.round,
            ..TOOTH
        };
        let fall = margin.fall(t, drawn, |share| margins::tooth_flank(share, s.round));
        width -= (s.tooth_depth * blade * blade_fade(t)).min(DEEPEST * width) * fall;
    }
    width
}

/// The seed a look's veins grow from: a hash of its shape, without an
/// unset `outline` or an unset field of a compound leaf's divisions,
/// which cards had none of before, so a card without them keeps the veins
/// it grew.
fn shape_seed(shape: &Shape) -> u64 {
    // A compound leaf a rule says is pinnate, divided once, with its
    // leaflets apart, is the leaf it was with those fields unset.
    let plain = match shape {
        Shape::Compound(c) => {
            let mut c = c.clone();
            if c.pattern == Some(Pattern::Pinnate) {
                c.pattern = None;
            }
            if c.pinnules == Some(0) {
                c.pinnules = None;
            }
            if c.confluent.is_some_and(|share| share <= 0.0) {
                c.confluent = None;
            }
            Shape::Compound(c)
        }
        _ => shape.clone(),
    };
    // A blade without basal lobes or fanned veins grows the veins it grew
    // before cards had them.
    let mut text = format!("{plain:?}")
        .replace("outline: None, ", "")
        .replace(", sinus: 0.0, fan: 0 }", " }");
    for field in [
        "pattern",
        "pinnules",
        "confluent",
        "angle",
        "pinnule_angle",
        "spread",
        "largest",
        "lowest",
        "terminal_size",
        "outline",
        "widest",
        "tip",
        "base",
        "petiolule",
    ] {
        text = text.replace(&format!(", {field}: None"), "");
    }
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A leaf grown with its veins (spec rework L3): its veins, grown in its
/// blade's frame (see [`crate::venation`]), and its margins' teeth or
/// lobes (see [`crate::margins`]), each fed by a vein. A simple or lobed
/// leaf's, one leaflet's of a compound leaf or one leaf's of a sprig
/// (which every leaflet or leaf draws scaled to its own length) has two
/// margins, its right side's (`a` > 0) then its left's; a palmate leaf's,
/// two a lobe from the lowest on its right, each lobe's left side (looking
/// along it) then its right.
#[derive(Debug, Clone, PartialEq)]
pub struct GrownLeaf {
    pub veins: Venation,
    pub margins: Vec<Margin>,
    /// A compound leaf's leaflets, laid out (`crate::divided`).
    pub layout: Option<Layout>,
}

/// The leaf drawn with `shape`, grown (see [`GrownLeaf`]): a simple or
/// lobed leaf's veins from its midrib, a palmate leaf's from its main
/// veins. `None` for shapes that are not such leaves.
#[must_use]
pub fn grow_leaf(shape: &Shape) -> Option<GrownLeaf> {
    let seed = shape_seed(shape);
    match shape {
        Shape::Simple(s) => {
            let length = 1.0 - junction(s);
            let (below, lobe) = basal_lobes(s, length);
            Some(pinnate_leaf(
                (s.teeth, TEETH),
                &|t| simple_width(s, length, t, None) / length,
                &|t, margin| simple_width(s, length, t, Some(margin)) / length,
                (0.5 * s.width + 0.02, Growth::default()),
                (below, &lobe, s.fan),
                seed,
            ))
        }
        Shape::Lobed(s) => {
            let length = 1.0 - s.petiole;
            let growth = Growth {
                secondary: 0.84 / f64::from(s.lobes.max(1) * 2 + 1),
                ..Growth::default()
            };
            Some(pinnate_leaf(
                (s.lobes, LOBES),
                &|t| lobed_width(s, length, t, None) / length,
                &|t, margin| lobed_width(s, length, t, Some(margin)) / length,
                (0.5 * s.width + 0.02, growth),
                (0.0, &no_lobes, 0),
                seed,
            ))
        }
        Shape::Compound(s) => {
            let form = Form::of(s);
            Some(GrownLeaf {
                layout: Some(divided::layout(s)),
                ..pinnate_leaf(
                    (s.teeth, LEAFLET_TEETH),
                    &|t| leaflet_width(s, &form, 1.0, t, None),
                    &|t, margin| leaflet_width(s, &form, 1.0, t, Some(margin)),
                    (0.5 * s.leaflet_width + 0.02, Growth::default()),
                    (0.0, &no_lobes, 0),
                    seed,
                )
            })
        }
        Shape::Sprig(s) => Some(pinnate_leaf(
            (s.teeth, TEETH),
            &|t| sprig_width(s, 1.0, t, None),
            &|t, margin| sprig_width(s, 1.0, t, Some(margin)),
            (0.5 * s.width + 0.02, Growth::default()),
            (0.0, &no_lobes, 0),
            seed,
        )),
        Shape::Palmate(s) => Some(palmate_leaf(s, fine(shape), seed)),
        _ => None,
    }
}

/// How far a tooth whose vein runs along `heading` leans toward its
/// blade's tip, 0 to 1: the heading's share along the margin's `tangent`
/// there, toward the tip.
fn lean(heading: Option<(f64, f64)>, tangent: (f64, f64)) -> f64 {
    let Some(heading) = heading else { return 0.0 };
    let norm = hypot(tangent.0, tangent.1).max(1e-12);
    ((heading.0 * tangent.0 + heading.1 * tangent.1) / norm).clamp(0.0, 1.0)
}

/// Where a simple card's petiole meets its blade, from the card's base:
/// at the petiole's length, or higher where the blade's basal lobes
/// (`sinus`, a share of the blade's length) reach back farther than the
/// petiole is long, so the lobes still fit on the card, hanging to its
/// foot.
pub(crate) fn junction(s: &Simple) -> f64 {
    let sinus = s.sinus.max(0.0);
    s.petiole.max(sinus / (1.0 + sinus))
}

/// A blade with no basal lobes, for [`pinnate_leaf`].
fn no_lobes(_: (f64, f64)) -> bool {
    false
}

/// A simple blade's basal lobes, in its frame (blade lengths across, and
/// `t` along it from where its petiole meets it): how far back past that
/// point they reach, 0 for none, and whether a point lies in one. Each
/// lobe is a half ellipse hung from the blade's base, from a twenty-fifth
/// of its half-width beside the midrib to its edge, so the sinus between them
/// opens round the petiole and closes at its top.
fn basal_lobes(s: &Simple, length: f64) -> (f64, impl Fn((f64, f64)) -> bool) {
    let length = length.max(1e-6);
    let below = s.sinus.max(0.0);
    let half = simple_width(s, length, 0.0, None) / length;
    let lobe = move |(x, t): (f64, f64)| {
        below > 0.0 && t < 0.0 && t > -below && {
            let reach = (1.0 - (t / below) * (t / below)).max(0.0).sqrt();
            (x.abs() - 0.52 * half).abs() < 0.48 * half * reach
        }
    };
    (below, lobe)
}

/// The main veins a palmately veined blade fans from its midrib's foot,
/// `fan` a side: spread evenly up to 80° from the midrib, or past the
/// perpendicular into its basal lobes where it has them, each run to 0.85
/// of the way to the edge and curving a little toward the tip.
fn fanned(fan: u32, lobed: bool, inside: &dyn Fn((f64, f64)) -> bool) -> Vec<Vec<(f64, f64)>> {
    let spread = math::radians(if lobed { 130.0 } else { 80.0 });
    let mut veins = Vec::new();
    for k in 1..=fan {
        let angle = spread * f64::from(k) / (f64::from(fan) + 0.5);
        for sign in [1.0, -1.0] {
            let ray = |along: f64, angle: f64| {
                (sign * along * math::sin(angle), along * math::cos(angle))
            };
            let mut reach = 0.0;
            let mut along = 0.005;
            while along < 1.5 {
                if inside(ray(along, angle)) {
                    reach = along;
                } else if along > 0.03 {
                    break;
                }
                along += 0.005;
            }
            if reach > 0.05 {
                veins.push(vec![
                    (0.0, 0.0),
                    ray(0.5 * reach, angle),
                    ray(0.85 * reach, 0.88 * angle),
                ]);
            }
        }
    }
    veins
}

/// Grow a pinnately veined blade: `count` teeth or lobes a side in its
/// `zone`, on a blade whose untoothed half-width is `outline(t)` and whose
/// half-width with a side's margin is `half(t, margin)`, both blade lengths;
/// its veins from its midrib with `growth`, in a box `widest` either side
/// of it, one into every tooth's tip, which then leans the way its vein
/// runs.
fn pinnate_leaf(
    (count, zone): (u32, (f64, f64)),
    outline: &dyn Fn(f64) -> f64,
    half: &dyn Fn(f64, &Margin) -> f64,
    (widest, growth): (f64, Growth),
    (below, lobe, fan): (f64, &dyn Fn((f64, f64)) -> bool, u32),
    seed: u64,
) -> GrownLeaf {
    let mut margins: Vec<Margin> = (0..2_u64)
        .map(|side| Margin::grow(count, zone, outline, mix64(seed ^ (side + 1))))
        .collect();
    let sign = |side: usize| if side == 0 { 1.0 } else { -1.0 };
    // A tooth's source lies within reach of its tip, so a vein reaching it
    // runs into the tooth however narrow it is.
    let inset = 0.5 * KILL * growth.secondary;
    let veins = {
        let inside = |p: (f64, f64)| {
            ((0.0..=1.0).contains(&p.1) && p.0.abs() < half(p.1, &margins[usize::from(p.0 < 0.0)]))
                || lobe(p)
        };
        let tips = margins
            .iter()
            .enumerate()
            .flat_map(|(side, margin)| {
                margin.apices.iter().map(move |apex| {
                    let edge = half(apex.t, margin);
                    (sign(side) * (edge - inset).max(0.5 * edge), apex.t)
                })
            })
            .collect();
        let mut primaries = vec![vec![(0.0, 0.0), (0.0, 0.96)]];
        primaries.extend(fanned(fan, below > 0.0, &inside));
        let blade = Lamina {
            inside: &inside,
            bounds: ((-widest, -below), (widest, 1.0)),
            primaries,
            tips,
        };
        Venation::grow(&blade, &growth, seed)
    };
    let mut headings = veins.tip_headings().iter();
    for (side, margin) in margins.iter_mut().enumerate() {
        let leans: Vec<f64> = margin
            .apices
            .iter()
            .map(|apex| {
                let slope = (outline(apex.t + 0.005) - outline(apex.t - 0.005)) / 0.01;
                lean(
                    headings.next().copied().flatten(),
                    (sign(side) * slope, 1.0),
                )
            })
            .collect();
        margin.lean(&leans);
    }
    GrownLeaf {
        veins,
        margins,
        layout: None,
    }
}

/// A palmate lobe's half-width share at `t` along it: straight sides
/// widening from where it leaves the blade to halfway, then drawn in to a
/// long point.
fn lobe_profile(t: f64) -> f64 {
    if t <= 0.5 {
        math::pow(t / 0.5, 0.9)
    } else {
        math::pow((1.0 - t) / 0.5, 1.15)
    }
}

/// How far a palmate lobe's tooth falls back along its flank from its tip
/// (0) to its sinus (1): it narrows slowly from its point, then steeply.
fn palmate_flank(share: f64) -> f64 {
    1.0 - math::pow(1.0 - share, 1.8)
}

/// A palmate leaf grown: each lobe side's teeth, then its veins grown
/// outward from its main veins in the frame of their junction and the
/// blade's radius, inside the painter's own blade less its petiole, one
/// into every tooth, which then leans the way its vein runs.
fn palmate_leaf(s: &Palmate, fine: f64, seed: u64) -> GrownLeaf {
    let (junction, radius) = s.junction();
    let half = i32::try_from(s.lobes / 2).unwrap_or(0);
    let lobes: Vec<(f64, f64)> = (-half..=half).map(|k| s.lobe(k)).collect();
    let outline = |t: f64| s.lobe_width * 0.5 * lobe_profile(t);
    let mut margins: Vec<Margin> = Vec::with_capacity(lobes.len() * 2);
    for index in 0..lobes.len() * 2 {
        margins.push(Margin::grow(
            s.teeth,
            LOBE_TEETH,
            &outline,
            mix64(seed ^ (index as u64 + 1)),
        ));
    }
    // Where a tooth's vein ends in the veins' frame, for side 0 (left of
    // the lobe) or 1, within reach of its tip as on a pinnate leaf, and the
    // margin's tangent there.
    let inset = 0.5 * KILL * Growth::default().secondary * radius;
    let tip = |lobe: usize, side: usize, apex: margins::Apex| {
        let (angle, length) = lobes[lobe];
        let d = (math::sin(angle), math::cos(angle));
        let left = (-d.1, d.0);
        let sign = if side == 0 { 1.0 } else { -1.0 };
        let width = |t: f64| {
            let fade = math::smoothstep(0.42, 0.55, t);
            let size = PALMATE_TOOTH.least + (1.0 - PALMATE_TOOTH.least) * apex.size;
            length * (outline(t) + s.lobe_width * 0.5 * 0.55 * (1.0 - t) * size * fade)
        };
        let (t, edge) = (apex.t, width(apex.t));
        let (along, across) = (t * length, sign * (edge - inset).max(0.5 * edge));
        let slope = (width(t + 0.005) - width(t - 0.005)) / (0.01 * length);
        (
            (
                (along * d.0 + across * left.0) / radius,
                (along * d.1 + across * left.1) / radius,
            ),
            (d.0 + sign * slope * left.0, d.1 + sign * slope * left.1),
        )
    };
    let veins = {
        let inside = |p: (f64, f64)| {
            !(p.1 < 0.02 && p.0.abs() < 0.05)
                && palmate(
                    s,
                    p.0 * radius,
                    junction + p.1 * radius,
                    fine,
                    None,
                    &margins,
                )
                .cover
                    > 0.0
        };
        let mut tips = Vec::new();
        for (index, margin) in margins.iter().enumerate() {
            for apex in &margin.apices {
                tips.push(tip(index / 2, index % 2, *apex).0);
            }
        }
        let primaries = lobes
            .iter()
            .map(|&(angle, length)| {
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
            tips,
        };
        let growth = Growth {
            outward: true,
            ..Growth::default()
        };
        Venation::grow(&blade, &growth, seed)
    };
    let mut headings = veins.tip_headings().iter();
    for (index, margin) in margins.iter_mut().enumerate() {
        let leans: Vec<f64> = margin
            .apices
            .iter()
            .map(|apex| {
                let (_, tangent) = tip(index / 2, index % 2, *apex);
                lean(headings.next().copied().flatten(), tangent)
            })
            .collect();
        margin.lean(&leans);
    }
    GrownLeaf {
        veins,
        margins,
        layout: None,
    }
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
fn palmate(
    s: &Palmate,
    x: f64,
    y: f64,
    fine: f64,
    veins: Option<&Venation>,
    margins: &[Margin],
) -> Paint {
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
        let mut width = length * s.lobe_width * 0.5 * lobe_profile(t);
        // A few coarse teeth on its outer half, so they never close a
        // sinus, each where its margin grew them (`palmate_leaf`).
        let side = usize::try_from(k + half).unwrap_or(0) * 2 + usize::from(across <= 0.0);
        if let Some(margin) = margins.get(side)
            && s.teeth > 0
            && t > LOBE_TEETH.0
            && t < LOBE_TEETH.1
        {
            let tooth = 1.0 - margin.fall(t, PALMATE_TOOTH, palmate_flank);
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
fn simple(
    s: &Simple,
    x: f64,
    y: f64,
    fine: f64,
    veins: Option<&Venation>,
    margins: &[Margin],
) -> Paint {
    let base = junction(s);
    if y < base + 0.01 && x.abs() < fine {
        return Paint::solid(0.9, 1.0);
    }
    let length = 1.0 - base;
    let t = (y - base) / length.max(1e-6);
    if t < 0.0 {
        let (_, lobe) = basal_lobes(s, length);
        if !lobe((x / length.max(1e-6), t)) {
            return Paint::EMPTY;
        }
        return Paint::solid(0.95 + grown(veins, x / length, t, length, || 0.0), 0.0);
    }
    let width = simple_width(s, length, t, margins.get(usize::from(x < 0.0)));
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
fn lobed(
    s: &Lobed,
    x: f64,
    y: f64,
    fine: f64,
    veins: Option<&Venation>,
    margins: &[Margin],
) -> Paint {
    let base = s.petiole;
    if y < base + 0.01 && x.abs() < fine {
        return Paint::solid(0.9, 1.0);
    }
    let length = 1.0 - base;
    let t = (y - base) / length.max(1e-6);
    let width = lobed_width(s, length, t, margins.get(usize::from(x < 0.0)));
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

/// A compound leaf as [`divided::layout`] lays it out: its petiole and
/// rachises in the accent colour, a dissected blade's wings, and each
/// leaflet on its stalk, the first laid out drawn over the rest.
fn compound(
    s: &Compound,
    laid: &Layout,
    x: f64,
    y: f64,
    fine: f64,
    veins: Option<&Venation>,
    margins: &[Margin],
) -> Paint {
    let p = (x, y);
    let mut wing = None;
    for axis in &laid.axes {
        let offset = (axis.to.0 - axis.from.0, axis.to.1 - axis.from.1);
        let length = hypot(offset.0, offset.1);
        if length <= 0.0 {
            continue;
        }
        let (along, across) = local(p, axis.from, (offset.0 / length, offset.1 / length));
        if along < 0.0 || along >= length {
            continue;
        }
        let half = if axis.order == 0 { fine } else { 0.75 * fine };
        if across.abs() < half {
            return Paint::solid(0.9, 1.0);
        }
        let reach = axis.wing * (1.0 - 0.7 * along / length);
        if across.abs() < reach {
            wing = Some(across.abs() / reach);
        }
    }
    let form = &laid.form;
    for leaflet in &laid.leaflets {
        let length = leaflet.length;
        let (along, across) = local(p, leaflet.base, leaflet.direction);
        if !(0.0..=length).contains(&along) {
            continue;
        }
        let stalk = form.petiolule * length;
        if along < stalk {
            if across.abs() < 0.6 * fine {
                return Paint::solid(0.9, 1.0);
            }
            continue;
        }
        let blade = length - stalk;
        let t = (along - stalk) / blade;
        let width = leaflet_width(s, form, blade, t, margins.get(usize::from(across < 0.0)));
        if across.abs() < width {
            // A leaflet too small to show its veins is drawn plain.
            if blade < 12.0 * fine {
                return Paint::solid(0.92 + 0.08 * across.abs() / width.max(1e-6), 0.0);
            }
            if t < 0.95 && across.abs() < fine * 0.6 {
                return Paint::solid(1.15, 0.4);
            }
            return Paint::solid(
                0.92 + 0.08 * across.abs() / width.max(1e-6)
                    + grown(veins, across / blade, t, blade, || {
                        side_veins(along - stalk, across, blade / 7.0, math::radians(55.0))
                    }),
                0.0,
            );
        }
    }
    wing.map_or(Paint::EMPTY, |share| Paint::solid(0.92 + 0.08 * share, 0.0))
}

/// A leafy shoot: a twig in the accent colour with simple leaves
/// alternating along it, each on its petiole at `angle` to the twig, and
/// one leaf continuing the twig at its tip. Side leaves are largest in the
/// shoot's middle; each is turned a little at random and drawn a little
/// lighter or darker, so that overlapping leaves stay apart.
fn sprig(
    s: &Sprig,
    x: f64,
    y: f64,
    fine: f64,
    veins: Option<&Venation>,
    margins: &[Margin],
) -> Paint {
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
        let width = sprig_width(s, blade, t, margins.get(usize::from(across < 0.0)));
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

/// A grass or sedge blade card's half-width at `y` along it, from its foot
/// (0) to its end (1).
pub(crate) fn blade_half_width(s: &Blade, y: f64) -> f64 {
    let mut width = s.aspect * 0.5 * (1.0 - 0.12 * y);
    if s.taper > 0.0 && y > 1.0 - s.taper {
        width *= math::pow(((1.0 - y) / s.taper).max(0.0), 0.85);
    }
    width
}

/// A piece of a grass or sedge blade, or its tapering tip: lighter along
/// the midrib, with fine parallel veins.
fn blade(s: &Blade, x: f64, y: f64) -> Paint {
    let width = blade_half_width(s, y);
    if x.abs() >= width {
        return Paint::EMPTY;
    }
    let across = x.abs() / width.max(1e-6);
    Paint::solid(
        0.86 + 0.24 * (1.0 - across) + 0.04 * math::cos(across * 6.0 * PI),
        0.0,
    )
}

/// A frond card's pinna `k` on its `left` side or its right: where it
/// leaves the rachis, a share of the blade from its foot, and how far its
/// tip reaches out from the rachis, in card units.
pub(crate) fn frond_pinna(s: &Frond, k: u32, left: bool) -> (f64, f64) {
    let offset = if left { 0.3 } else { 0.0 };
    let tk = (f64::from(k) + 0.5 + offset) / (f64::from(s.pinnae) + 0.3);
    let reach = s.width * 0.5 * math::pow(math::sin(PI * math::pow(tk, 0.75)), 0.7);
    (tk, reach)
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
        let (tk, reach) = frond_pinna(s, k, side < 0.0);
        let angle = math::radians(74.0 - 32.0 * tk);
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
/// petal's base tinted toward it; a flower that is not plain as its
/// layout paints it (`crate::flowers::paint`) in `colours`.
fn flower(s: &Flower, colours: Colours, x: f64, y: f64) -> Paint {
    if !s.is_plain() {
        let painted = crate::flowers::paint(s, colours, x / BLOOM, y / BLOOM);
        return Paint {
            cover: painted.cover,
            bright: painted.bright,
            accent: painted.accent,
        };
    }
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

/// A flower head seen face on, `BLOOM` its radius (`crate::capitula`): its
/// florets on the disc, rays or straps round it and the rim of its
/// involucre, in `colours`, its look's colour and accent.
fn head(s: &Head, colours: Colours, x: f64, y: f64) -> Paint {
    let painted = crate::capitula::paint(s, colours, x / BLOOM, y / BLOOM);
    Paint {
        cover: painted.cover,
        bright: painted.bright,
        accent: painted.accent,
    }
}

/// A spikelet seen square to its bracts' plane (`crate::spikelets`), at
/// `(x, y)` in shares of its length.
fn spikelet(s: &crate::looks::Spikelet, x: f64, y: f64, fine: f64) -> Paint {
    let painted = crate::spikelets::paint(s, x, y, fine);
    Paint {
        cover: painted.cover,
        bright: painted.bright,
        accent: painted.accent,
    }
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
        return one_of_a_kind(s, x, y, fine);
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

/// A fruit of a kind beyond the plain berry, or one partly unripe (plant
/// roadmap P4), as `crate::fruit` draws it: filling the card from its
/// foot, as unripe as the template's share, its mean colour, which it
/// blushes from the shaded lower side up.
fn one_of_a_kind(s: &Fruit, x: f64, y: f64, fine: f64) -> Paint {
    let mut paint = one_fruit(s, (0.0, 0.0), (0.0, 1.0), 1.0, x, y, fine);
    if paint.cover <= 0.0 {
        return Paint::EMPTY;
    }
    let (along, across) = local((x, y), (0.0, 0.0), (0.0, 1.0));
    let side = 0.5 * (along + 0.5 * across / (s.aspect * 0.5).max(0.05) + 0.5);
    let green = 1.0 - math::smoothstep(s.unripe - 0.15, s.unripe + 0.15, side);
    paint.accent = paint.accent.max(if s.unripe > 0.0 { green } else { 0.0 });
    paint
}

/// A cluster's card (spec rework L1): the layout its solid is drawn from
/// (`crate::clusters`), unturned, seen as its card faces it, with the
/// frontmost unit or stalk drawn at each point. Stalks and unripe fruit
/// take the accent colour, as a flower's centre and a head's disc do.
/// Each card point looks only at the units and stalks near it, through a
/// grid laid over the card. Units in stages of their own (L1b) are drawn
/// in their stage's look, their colours matched by the card's two
/// (`crate::clusters::Opening::slots`); stalks among flowers or heads take
/// their own colour as nearly as the card's two make it.
pub(crate) struct ClusterCard {
    units: Vec<CardUnit>,
    stalks: Vec<CardStalk>,
    /// Spathes, behind the rest.
    sheets: Vec<CardSheet>,
    /// The looks of units in stages of their own: each its shape, how its
    /// colour and its accent are made of the card's two colours, and those
    /// two colours themselves.
    looks: Vec<Option<StageCard>>,
    /// The look's own colour and accent.
    colours: Colours,
    /// Each unit's pedicel's paint.
    pedicels: Vec<Paint>,
    /// Lowest corner of the grid, its cells' size and how many a side.
    low: (f64, f64),
    cell: (f64, f64),
    side: usize,
    /// Per cell: the stalks, the units, then the spathes, it may show, in
    /// order.
    cells: Vec<Vec<CardItem>>,
    fine: f64,
}

/// A stage's look on a cluster's card (`ClusterCard::looks`).
struct StageCard {
    shape: Shape,
    fit: [f64; 4],
    colours: Colours,
}

/// A unit on a cluster's card, in card units, with its depth toward the
/// viewer (larger in front), and its stage's look (`ClusterCard::looks`)
/// where it is in a stage of its own.
enum CardUnit {
    /// A fruit, a bud or a spikelet: its base, its direction (not
    /// unit-length once projected), length, width over length and how far
    /// it reaches along its direction in its lengths (a spikelet's awns),
    /// its pedicel from `attach`, and whether it is unripe.
    Fruit {
        base: crate::math::Vec3,
        direction: crate::math::Vec3,
        length: f64,
        width: f64,
        reach: f64,
        attach: crate::math::Vec3,
        unripe: bool,
        look: Option<usize>,
    },
    /// A flower or head: its centre, its two radii as seen, and the way
    /// back from the card to them; its pedicel from `attach`.
    Disc {
        centre: crate::math::Vec3,
        a: crate::math::Vec3,
        b: crate::math::Vec3,
        inverse: [f64; 4],
        attach: crate::math::Vec3,
        look: Option<usize>,
    },
    /// A unit that shows nothing that day: its bare pedicel.
    Bare {
        base: crate::math::Vec3,
        attach: crate::math::Vec3,
    },
}

/// A stalk on a cluster's card: from, to, whether it is the cluster's
/// own axis (drawn wider, behind everything), and its paint.
struct CardStalk {
    from: crate::math::Vec3,
    to: crate::math::Vec3,
    axis: bool,
    paint: Paint,
}

/// A spathe on a cluster's card: its foot and tip, its widest width,
/// card units, and its paint.
struct CardSheet {
    foot: crate::math::Vec3,
    tip: crate::math::Vec3,
    width: f64,
    paint: Paint,
}

#[derive(Clone, Copy)]
enum CardItem {
    Stalk(usize),
    Unit(usize),
    Sheet(usize),
}

impl CardUnit {
    fn attach_and_base(&self) -> (crate::math::Vec3, crate::math::Vec3) {
        match self {
            Self::Fruit { attach, base, .. } | Self::Bare { attach, base } => (*attach, *base),
            Self::Disc { attach, centre, .. } => (*attach, *centre),
        }
    }
}

impl ClusterCard {
    /// Cells a side of the grid.
    const SIDE: usize = 48;

    /// The card of `cluster`, drawn in a look's `colours`, its colour and
    /// its accent.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(cluster: &crate::clusters::ClusterLook, colours: Colours) -> Self {
        use crate::math::Vec3;
        let frame = cluster.card;
        let laid = cluster.layout(0.0, 0);
        let length = frame.length;
        let facing = frame.mount == crate::looks::Mount::Facing;
        // Card units: across, along and toward the viewer.
        let project = |p: Vec3| {
            if facing {
                Vec3::new(
                    -p.x / length,
                    0.5 + p.z / length,
                    (p.y - frame.foot) / length,
                )
            } else {
                Vec3::new(p.x / length, (p.y - frame.foot) / length, p.z / length)
            }
        };
        let turn = |d: Vec3| if facing { Vec3::new(-d.x, d.z, d.y) } else { d };
        let opening = cluster.opening.as_deref();
        let slots = opening.map(|opening| opening.slots(&laid));
        // A stalk in the card's accent among fruit, which takes the
        // stalks' colour; or, among flowers or heads, or where the units
        // are in stages of their own and the card's two colours are
        // theirs, in its own colour made of those two.
        let stalk_slots = slots.or_else(|| cluster.unit.faces().then_some(colours));
        let stalk_paint = |colour: [f32; 3]| match stalk_slots {
            None => Paint::solid(0.8, 1.0),
            Some(slots) => {
                let (a, b) = crate::clusters::blend(colour, slots);
                let sum = a + b;
                Paint::solid(0.8 * sum, if sum > 1e-9 { b / sum } else { 0.0 })
            }
        };
        let stalks: Vec<CardStalk> = laid
            .stalks
            .iter()
            .map(|stalk| CardStalk {
                from: project(stalk.from),
                to: project(stalk.to),
                axis: stalk.axis,
                paint: stalk_paint(stalk.colour),
            })
            .collect();
        let pedicels: Vec<Paint> = laid.units.iter().map(|p| stalk_paint(p.stalk)).collect();
        // A spathe in its colour made of the card's two, as a stalk among
        // flowers is.
        let sheets: Vec<CardSheet> = laid
            .sheets
            .iter()
            .map(|sheet| {
                let colour = sheet.colour.unwrap_or(colours.0);
                let (a, b) = crate::clusters::blend(colour, colours);
                let sum = a + b;
                CardSheet {
                    foot: project(sheet.foot),
                    tip: project(sheet.tip()),
                    width: sheet.width / length,
                    paint: Paint::solid(sum.min(1.0), if sum > 1e-9 { b / sum } else { 0.0 }),
                }
            })
            .collect();
        // Each stage's colour and accent made of the card's two.
        let looks = opening
            .zip(slots)
            .map_or_else(Vec::new, |(opening, slots)| {
                opening
                    .looks
                    .iter()
                    .map(|look| {
                        look.as_ref().map(|look| {
                            let (a, b) = crate::clusters::blend(look.colour, slots);
                            let (c, d) = crate::clusters::blend(look.accent, slots);
                            StageCard {
                                shape: look.shape.clone(),
                                fit: [a, b, c, d],
                                colours: (look.colour, look.accent),
                            }
                        })
                    })
                    .collect()
            });
        let units: Vec<CardUnit> = laid
            .units
            .iter()
            .map(|p| {
                let attach = project(p.attach);
                let base = project(p.base);
                // What the unit shows: the look's own unit, or its stage's.
                let (unit, size, unripe, look) = match opening {
                    None => (cluster.unit, 1.0, p.unripe, None),
                    Some(opening) => {
                        let Some((index, unripe)) = opening.shown(p) else {
                            return CardUnit::Bare { base, attach };
                        };
                        let Some(stage) = &opening.looks[index] else {
                            return CardUnit::Bare { base, attach };
                        };
                        let Some(unit) = crate::clusters::Unit::of(&stage.shape) else {
                            return CardUnit::Bare { base, attach };
                        };
                        (unit, stage.size, unripe, Some(index))
                    }
                };
                if !unit.faces() {
                    return CardUnit::Fruit {
                        base,
                        direction: turn(p.direction),
                        length: p.length * size / length,
                        width: unit.across(),
                        reach: unit.reach(),
                        attach,
                        unripe,
                        look,
                    };
                }
                // The disc square to the unit's direction, its radii as
                // the card sees them.
                let radius = 0.5 * p.length * size / length;
                let across = crate::math::any_perpendicular(p.direction);
                let other = p.direction.cross(across);
                let (mut a, mut b) = (turn(across) * radius, turn(other) * radius);
                let det = a.x * b.y - a.y * b.x;
                let (la, lb) = (hypot(a.x, a.y), hypot(b.x, b.y));
                if det.abs() < 0.2 * la * lb || det.abs() < 1e-12 {
                    // Nearly edge on: a narrow ellipse along its longer
                    // radius.
                    let major = if la >= lb { a } else { b };
                    let span = hypot(major.x, major.y).max(1e-9);
                    let minor = 0.2 * span.max(radius);
                    let unit = (major.x / span, major.y / span);
                    a = Vec3::new(
                        unit.0 * span.max(radius),
                        unit.1 * span.max(radius),
                        major.z,
                    );
                    b = Vec3::new(-unit.1 * minor, unit.0 * minor, 0.0);
                }
                let det = a.x * b.y - a.y * b.x;
                CardUnit::Disc {
                    centre: base,
                    a,
                    b,
                    inverse: [b.y / det, -b.x / det, -a.y / det, a.x / det],
                    attach,
                    look,
                }
            })
            .collect();
        let fine = 0.85 * frame.aspect.max(1.0) / to_f64(TEMPLATE_SIZE);
        // Each item's box on the card, widened by what it draws round it.
        let stalk_box = |stalk: &CardStalk| {
            let pad = fine * 1.5;
            (
                (
                    stalk.from.x.min(stalk.to.x) - pad,
                    stalk.from.y.min(stalk.to.y) - pad,
                ),
                (
                    stalk.from.x.max(stalk.to.x) + pad,
                    stalk.from.y.max(stalk.to.y) + pad,
                ),
            )
        };
        let unit_box = |unit: &CardUnit| {
            let (attach, base) = unit.attach_and_base();
            let (low, high) = match unit {
                CardUnit::Fruit {
                    base,
                    direction,
                    length,
                    width,
                    reach,
                    ..
                } => {
                    let tip = *base + *direction * (*length * reach);
                    let pad = (*length * width * 0.5).max(fine) + *length * 0.05 + fine;
                    (
                        (base.x.min(tip.x) - pad, base.y.min(tip.y) - pad),
                        (base.x.max(tip.x) + pad, base.y.max(tip.y) + pad),
                    )
                }
                CardUnit::Disc { centre, a, b, .. } => {
                    let reach = (hypot(a.x, a.y) + hypot(b.x, b.y)) * 1.05 + fine;
                    (
                        (centre.x - reach, centre.y - reach),
                        (centre.x + reach, centre.y + reach),
                    )
                }
                CardUnit::Bare { .. } => ((base.x, base.y), (base.x, base.y)),
            };
            let pad = fine * 1.5;
            (
                (low.0.min(attach.x - pad), low.1.min(attach.y - pad)),
                (high.0.max(attach.x + pad), high.1.max(attach.y + pad)),
            )
        };
        let sheet_box = |sheet: &CardSheet| {
            let pad = 0.5 * sheet.width + fine;
            (
                (
                    sheet.foot.x.min(sheet.tip.x) - pad,
                    sheet.foot.y.min(sheet.tip.y) - fine,
                ),
                (
                    sheet.foot.x.max(sheet.tip.x) + pad,
                    sheet.foot.y.max(sheet.tip.y) + fine,
                ),
            )
        };
        let boxes: Vec<((f64, f64), (f64, f64))> = stalks
            .iter()
            .map(stalk_box)
            .chain(units.iter().map(unit_box))
            .chain(sheets.iter().map(sheet_box))
            .collect();
        let (low, high) = boxes.iter().fold(
            ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN)),
            |(low, high), &(a, b)| {
                (
                    (low.0.min(a.0), low.1.min(a.1)),
                    (high.0.max(b.0), high.1.max(b.1)),
                )
            },
        );
        let side = Self::SIDE;
        let cell = (
            ((high.0 - low.0) / to_f64(side)).max(1e-9),
            ((high.1 - low.1) / to_f64(side)).max(1e-9),
        );
        let mut cells = vec![Vec::new(); side * side];
        let index = |value: f64, from: f64, size: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let at = ((value - from) / size).floor().max(0.0) as usize;
            at.min(side - 1)
        };
        for (number, &(a, b)) in boxes.iter().enumerate() {
            let item = if number < stalks.len() {
                CardItem::Stalk(number)
            } else if number < stalks.len() + units.len() {
                CardItem::Unit(number - stalks.len())
            } else {
                CardItem::Sheet(number - stalks.len() - units.len())
            };
            for row in index(a.1, low.1, cell.1)..=index(b.1, low.1, cell.1) {
                for column in index(a.0, low.0, cell.0)..=index(b.0, low.0, cell.0) {
                    cells[row * side + column].push(item);
                }
            }
        }
        Self {
            units,
            stalks,
            sheets,
            looks,
            colours,
            pedicels,
            low,
            cell,
            side,
            cells,
            fine,
        }
    }

    /// The shape unit `look` is drawn with, `own` for the look's own, and
    /// `paint` of it in the card's two colours.
    #[allow(clippy::float_cmp)]
    fn in_look<'a>(
        &'a self,
        look: Option<usize>,
        own: &'a Shape,
    ) -> (&'a Shape, Colours, impl Fn(Paint) -> Paint) {
        let stage = look.and_then(|index| self.looks.get(index)?.as_ref());
        let fit = stage.map(|stage| stage.fit);
        let refit = move |paint: Paint| match fit {
            Some(fit) if fit != [1.0, 0.0, 0.0, 1.0] => {
                let g = paint.accent;
                let colour = (1.0 - g) * fit[0] + g * fit[2];
                let accent = (1.0 - g) * fit[1] + g * fit[3];
                let sum = colour + accent;
                Paint {
                    cover: paint.cover,
                    bright: paint.bright * sum,
                    accent: if sum > 1e-9 { accent / sum } else { 0.0 },
                }
            }
            _ => paint,
        };
        (
            stage.map_or(own, |stage| &stage.shape),
            stage.map_or(self.colours, |stage| stage.colours),
            refit,
        )
    }

    /// The point `x`, `y` of the card, its units drawn with `shape`.
    #[allow(clippy::too_many_lines)]
    fn paint(&self, shape: &Shape, x: f64, y: f64) -> Paint {
        let (column, row) = (
            (x - self.low.0) / self.cell.0,
            (y - self.low.1) / self.cell.1,
        );
        let side = to_f64(self.side);
        if !(0.0..side).contains(&column) || !(0.0..side).contains(&row) {
            return Paint::EMPTY;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let at =
            (row as usize).min(self.side - 1) * self.side + (column as usize).min(self.side - 1);
        let fine = self.fine;
        let mut best: Option<(f64, Paint)> = None;
        let mut consider = |depth: f64, paint: Paint| {
            if paint.cover > 0.0 && best.is_none_or(|(front, _)| depth > front) {
                best = Some((depth, paint));
            }
        };
        for &item in &self.cells[at] {
            match item {
                CardItem::Sheet(number) => {
                    // Seen from the front, its outline: as wide along its
                    // length as the solid draws it, behind everything.
                    let sheet = &self.sheets[number];
                    let span = sheet.tip.y - sheet.foot.y;
                    if span.abs() > 1e-9 {
                        let u = (y - sheet.foot.y) / span;
                        if (0.0..=1.0).contains(&u) {
                            let centre = sheet.foot.x + (sheet.tip.x - sheet.foot.x) * u;
                            let half = 0.5 * sheet.width * crate::clusters::Sheet::width_at(u);
                            if (x - centre).abs() < half.max(fine * 0.5) {
                                consider(-3.0, sheet.paint);
                            }
                        }
                    }
                }
                CardItem::Stalk(number) => {
                    let stalk = &self.stalks[number];
                    let (distance, _) = segment(
                        (x, y),
                        (stalk.from.x, stalk.from.y),
                        (stalk.to.x, stalk.to.y),
                    );
                    if stalk.axis {
                        if distance < fine * 1.4 {
                            consider(-2.0, stalk.paint);
                        }
                    } else if distance < fine * 1.1 {
                        consider(stalk.from.z.min(stalk.to.z) - 1.0, stalk.paint);
                    }
                }
                CardItem::Unit(number) => {
                    let unit = &self.units[number];
                    let (attach, base) = unit.attach_and_base();
                    if (base - attach).length() > 1e-9 {
                        let (distance, _) = segment((x, y), (attach.x, attach.y), (base.x, base.y));
                        if distance < fine * 1.1 {
                            consider(attach.z.min(base.z) - 1.0, self.pedicels[number]);
                        }
                    }
                    match unit {
                        CardUnit::Fruit {
                            base,
                            direction,
                            length,
                            unripe,
                            look,
                            ..
                        } => {
                            let (shape, _, refit) = self.in_look(*look, shape);
                            let mut paint = match shape {
                                Shape::Fruit(s) => one_fruit(
                                    s,
                                    (base.x, base.y),
                                    (direction.x, direction.y),
                                    *length,
                                    x,
                                    y,
                                    fine,
                                ),
                                Shape::Spikelet(s) => one_spikelet(
                                    s,
                                    (base.x, base.y),
                                    (direction.x, direction.y),
                                    *length,
                                    (x, y),
                                    fine,
                                ),
                                _ => continue,
                            };
                            if *unripe {
                                paint.accent = 1.0;
                            }
                            consider(base.z + direction.z * length * 0.5, refit(paint));
                        }
                        CardUnit::Disc {
                            centre,
                            a,
                            b,
                            inverse,
                            look,
                            ..
                        } => {
                            let (shape, colours, refit) = self.in_look(*look, shape);
                            let (dx, dy) = (x - centre.x, y - centre.y);
                            let (u, v) = (
                                inverse[0] * dx + inverse[1] * dy,
                                inverse[2] * dx + inverse[3] * dy,
                            );
                            if u * u + v * v > 1.1 {
                                continue;
                            }
                            let paint = match shape {
                                Shape::Flower(s) => flower(s, colours, u * BLOOM, v * BLOOM),
                                Shape::Head(s) => head(s, colours, u * BLOOM, v * BLOOM),
                                _ => continue,
                            };
                            consider(centre.z + a.z * u + b.z * v, refit(paint));
                        }
                        CardUnit::Bare { .. } => {}
                    }
                }
            }
        }
        best.map_or(Paint::EMPTY, |(_, paint)| paint)
    }
}

/// One spikelet at card point `p`, its base, direction and length as the
/// card sees them: seen square to its bracts' plane, its length along its
/// projected axis, foreshortened as it leans toward or away from the card.
fn one_spikelet(
    s: &crate::looks::Spikelet,
    base: (f64, f64),
    direction: (f64, f64),
    length: f64,
    p: (f64, f64),
    fine: f64,
) -> Paint {
    let projected = hypot(direction.0, direction.1).max(0.2);
    let (along, across) = (
        (direction.0 / projected, direction.1 / projected),
        (-direction.1 / projected, direction.0 / projected),
    );
    let (dx, dy) = (p.0 - base.0, p.1 - base.1);
    let scale = length.max(1e-9);
    spikelet(
        s,
        (dx * across.0 + dy * across.1) / scale,
        (dx * along.0 + dy * along.1) / (scale * projected),
        fine / scale,
    )
}

/// One fruit at card point `(x, y)`, its base, direction and length as the
/// card sees them: its kind's outline and shading along its projected
/// axis.
#[allow(clippy::too_many_lines)]
fn one_fruit(
    s: &Fruit,
    base: (f64, f64),
    direction: (f64, f64),
    length: f64,
    x: f64,
    y: f64,
    fine: f64,
) -> Paint {
    use crate::looks::FruitKind;
    let half_width = (length * s.aspect * 0.5).max(fine);
    let projected = hypot(direction.0, direction.1);
    let axis = if projected > 0.05 {
        (direction.0 / projected, direction.1 / projected)
    } else {
        (0.0, 1.0)
    };
    // Seen from the side the fruit is its length; seen end on, round.
    let length = (length * projected).max(2.0 * half_width);
    let (along, across) = local((x, y), base, axis);
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

    /// A leaf drawn on a petiole of its own leaves the petiole off its
    /// card; its blade stays as it was drawn.
    #[test]
    fn a_leaf_on_its_own_petiole_leaves_the_petiole_off_its_card() {
        let shape = Shape::Simple(crate::looks::Simple {
            petiole: 0.4,
            ..crate::looks::Simple::default()
        });
        let drawn = |insertion| {
            let own = OrganLook {
                insertion,
                ..serde_json::from_value::<OrganLook>(
                    serde_json::json!({"shape": {"template": "simple"}}),
                )
                .unwrap()
            };
            let own = OrganLook {
                shape: shape.clone(),
                ..own
            };
            let looks = looks::resolve(
                [("leaf", crate::lsys::OrganKind::Leaf)],
                &BTreeMap::from([("leaf".to_string(), own)]),
                [0.2, 0.5, 0.2],
                [0.1, 0.2, 0.1],
            );
            let look = looks[0].clone();
            let petiole = draw_look(&look, None, 0.0, 0.2, None).cover;
            let blade = draw_look(&look, None, 0.0, 0.7, None).cover;
            (petiole, blade)
        };
        assert_eq!(drawn(looks::Insertion::Marginal), (1.0, 1.0));
        assert_eq!(drawn(looks::Insertion::Latent), (0.0, 1.0));
        assert_eq!(drawn(looks::Insertion::Peltate), (0.0, 1.0));
    }
    use crate::lsys::OrganKind;

    fn looks_of(shapes: &[Shape]) -> Vec<Look> {
        let names: Vec<String> = (0..shapes.len()).map(|i| format!("organ{i}")).collect();
        let mut organs = BTreeMap::new();
        for (name, shape) in names.iter().zip(shapes) {
            organs.insert(
                name.clone(),
                OrganLook {
                    hairs: None,
                    roll: 0.0,
                    insertion: crate::looks::Insertion::Marginal,
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
                    inflorescence: None,
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
            let grew = grow_leaf(&shape);
            assert_eq!(grew.is_some(), leaf, "{shape:?}");
            let Some(GrownLeaf { veins, .. }) = grew else {
                continue;
            };
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
    fn a_vein_runs_to_every_tooth_and_the_tooth_leans_its_way() {
        let shapes = [
            Shape::Simple(Simple {
                teeth: 14,
                ..Simple::default()
            }),
            Shape::Sprig(Sprig {
                teeth: 4,
                round: 0.8,
                tooth_depth: 0.2,
                ..Sprig::default()
            }),
            Shape::Lobed(Lobed::default()),
            Shape::Compound(Compound {
                teeth: 6,
                ..Compound::default()
            }),
        ];
        for shape in shapes {
            let leaf = grow_leaf(&shape).unwrap();
            let count = match &shape {
                Shape::Simple(s) => s.teeth,
                Shape::Sprig(s) => s.teeth,
                Shape::Lobed(s) => s.lobes,
                Shape::Compound(s) => s.teeth,
                _ => unreachable!(),
            };
            assert_eq!(leaf.margins.len(), 2);
            let mut tips = 0;
            for margin in &leaf.margins {
                let found = u32::try_from(margin.apices.len()).unwrap();
                assert!(found.abs_diff(count) <= 1, "{shape:?}: {found} of {count}");
                tips += margin.apices.len();
                // Veins run forward into small teeth, on the whole, and
                // out into deep lobes.
                #[allow(clippy::cast_precision_loss)]
                let mean = margin.apices.iter().map(|apex| apex.lean).sum::<f64>()
                    / margin.apices.len().max(1) as f64;
                if matches!(shape, Shape::Simple(_) | Shape::Compound(_)) {
                    assert!(mean > 0.3, "{shape:?}: {mean}");
                }
            }
            // Every tooth's tip has a vein that reached it.
            let headings = leaf.veins.tip_headings();
            assert_eq!(headings.len(), tips);
            assert!(
                headings.iter().all(Option::is_some),
                "{shape:?}: {headings:?}"
            );
        }
        // Each lobe of a palmate leaf has two margins, its teeth fed too.
        let leaf = grow_leaf(&Shape::Palmate(Palmate::default())).unwrap();
        assert_eq!(leaf.margins.len(), 10);
        assert!(leaf.margins.iter().all(|margin| margin.apices.len() == 2));
        let fed = leaf
            .veins
            .tip_headings()
            .iter()
            .filter(|heading| heading.is_some())
            .count();
        assert!(fed >= 18, "{fed} of 20");
    }

    /// A blade with an outline is as wide as its points say, its widest
    /// at the card's width, and its sides parallel where theirs are;
    /// without one, it is drawn as before.
    #[test]
    fn outlines_draw_the_blade_through_their_points() {
        let oblong = vec![
            [0.0, 0.0],
            [0.1, 0.9],
            [0.2, 1.0],
            [0.8, 1.0],
            [0.9, 0.9],
            [1.0, 0.0],
        ];
        let s = Simple {
            width: 0.4,
            outline: Some(oblong.clone()),
            ..Simple::default()
        };
        for [u, w] in &oblong {
            assert!(
                (simple_width(&s, 2.0, *u, None) - 0.4 * w).abs() < 1e-12,
                "{u}"
            );
        }
        for k in 20..=80 {
            let t = f64::from(k) / 100.0;
            assert!((simple_width(&s, 2.0, t, None) - 0.4).abs() < 1e-12, "{t}");
        }
        // Points that never reach 1 are drawn to the card's width.
        let half = Sprig {
            width: 0.4,
            outline: Some(oblong.iter().map(|&[u, w]| [u, w / 2.0]).collect()),
            ..Sprig::default()
        };
        assert!((sprig_width(&half, 2.0, 0.5, None) - 0.4).abs() < 1e-12);
        let plain = Simple::default();
        for k in 0..=10 {
            let t = f64::from(k) / 10.0;
            let drawn = plain.width * 0.5 * blade_profile(t, plain.widest, plain.base, plain.tip);
            assert_eq!(simple_width(&plain, 1.0, t, None), drawn, "{t}");
        }
        // A card without an outline grows the veins it grew before cards
        // had one.
        let before = "Simple(Simple { width: 0.5, widest: 0.4, tip: 0.4, base: 0.5, teeth: 0, tooth_depth: 0.03, petiole: 0.15 })";
        let hash = before
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        assert_eq!(shape_seed(&Shape::Simple(plain)), hash);
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
            hair: 0.0,
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
