//! Hairs on an organ's face, its indumentum ([`crate::looks::Hairs`]),
//! drawn the three ways a renderer sees them at different distances.
//!
//! - **On the card**, where a hair is far thinner than a texel: a soft
//!   cast of the face's colour toward the hairs' (a felt hides the face,
//!   a few hairs barely tint it; [`cast`]), and a sheen ([`sheen`]):
//!   hairs catch light where the
//!   face is seen edge on and glow where the sun is behind them, as a
//!   velvet does (asperity scattering; Koenderink and Pont, 2003). Both
//!   are the same over the face, so a renderer needs only the template's
//!   hairs (`crate::package::AtlasLayer::hairs`), no texels of their own.
//! - **Along the edge**, a fringe of hairs painted out from the outline
//!   ([`paint`]), pale where they catch the light and show the face's
//!   colour through them, which shows a hairy leaf's soft rim against what
//!   lies behind it. Floras say so for a ciliate edge; every hairy blade has
//!   some. The fringe is no part of the leaf's area, and a renderer leaves
//!   it out where the strands draw the edge's hairs ([`fringe_drawn`]).
//! - **Up close**, where a hair is longer than a couple of pixels, each
//!   hair a strand of its own ([`strand_mesh`]): a thin tapering solid
//!   standing from the face as the hairs' lie and curl set, drawn at least
//!   half a pixel wide, as a camera's blur draws a hair thinner than its
//!   pixel, and lit as a fibre is (Kajiya and Kay, 1989), along its length
//!   rather than across a face.
//!
//! The young stems and the leaf stalks bear the plant's stem hairs
//! (`crate::spec::Appearance::stem_hairs`) the same two ways: a cast of
//! their colour toward the hairs', wearing off as the wood ages
//! ([`youth`]), and up close strands standing from them ([`Tube`]).

// Texel and strand counts are small whole numbers, and texel positions
// small signed ones: casts between them and to `f64` lose nothing.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::looks::Hairs;
use crate::math::{self, Vec3};
use crate::mesh::{Card, Mesh, PlantMesh, Vertex};
use crate::raster::Lighting;
use crate::rng::{mix64, unit};

/// How far a face's hairs move its colour toward theirs at most, under a
/// felt that hides it; a fringe's own texels are all hair. The cast goes
/// with the cube of their density ([`cast`]): a few clear hairs barely
/// tint a face, a felt or a wool whitens it.
pub const CAST: f64 = 0.45;

/// The hair weight of a face whose hairs hide `density` of it (see
/// [`CAST`]).
#[must_use]
pub fn cast(density: f64) -> f64 {
    let density = density.clamp(0.0, 1.0);
    CAST * density * density * density
}

/// How much brighter than the face a fringe's hair is: clear hairs show
/// the face's colour through them and catch the light.
pub const FRINGE_BRIGHTNESS: f32 = 1.5;

/// Strands one plant draws at most ([`strand_mesh`]), each nine
/// triangles: a close-up's budget.
pub const MAX_STRANDS: usize = 60_000;

/// Wood this many years old has lost its stem hairs: a flora's "young
/// stems tomentose" are the shoots of the last year or two, bare once
/// their bark forms (a liana's stems stay thin, so their thickness does
/// not tell). Wood up to half of it keeps them all ([`youth`]).
pub const YOUNG_YEARS: f64 = 2.0;

/// How much of the plant's stem hairs wood `years` old keeps: all up to
/// half of [`YOUNG_YEARS`], none from it on.
#[must_use]
pub fn youth(years: f64) -> f64 {
    (2.0 * (YOUNG_YEARS - years) / YOUNG_YEARS).clamp(0.0, 1.0)
}

/// `colour` of wood keeping `youth` of the plant's stem `hairs`
/// ([`youth`]), cast toward theirs as a blade's is ([`cast`]).
#[must_use]
pub fn cast_wood(colour: [f32; 3], youth: f64, hairs: Option<&Hairs>) -> [f32; 3] {
    let Some(hairs) = hairs else {
        return colour;
    };
    let weight = (cast(hairs.density) * youth) as f32;
    std::array::from_fn(|c| colour[c] + (hairs.colour[c] - colour[c]) * weight)
}

/// A piece of young wood that bears hairs, on the nearest levels
/// ([`crate::mesh::PlantMesh::hair_tubes`]): a stem's piece between two
/// of its rings, or a leaf's stalk. Its hairs are the plant's stem hairs,
/// their density already worn by its age ([`youth`]). Previews draw
/// them as strands up close ([`strand_mesh`]); packages leave them out.
#[derive(Debug, Clone, PartialEq)]
pub struct Tube {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub radius: f32,
    /// The wood's colour, which a clear hair shows through it.
    pub colour: [f32; 3],
    pub born: f32,
    /// Infinity when it is never shed.
    pub shed: f32,
    pub segment: u32,
    pub hairs: Hairs,
}

impl Tube {
    /// A tube from `start` to `end` of wood `radius` thick keeping
    /// `youth` of `hairs` ([`youth`]), or `None` where it keeps none or is
    /// too short to bear any.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        start: Vec3,
        end: Vec3,
        radius: f64,
        colour: [f32; 3],
        (born, shed): (f64, Option<f64>),
        segment: u32,
        hairs: &Hairs,
        youth: f64,
    ) -> Option<Self> {
        let hairs = Hairs {
            edge: None,
            ..hairs.thinned(youth)
        };
        if !hairs.drawn() || (end - start).length() <= 1e-6 {
            return None;
        }
        Some(Self {
            start: start.to_f32(),
            end: end.to_f32(),
            radius: radius as f32,
            colour,
            born: born as f32,
            shed: shed.map_or(f32::INFINITY, |shed| shed as f32),
            segment,
            hairs,
        })
    }
}

/// A number in `[0, 1)` from two small integers and a salt.
fn hash(a: u64, b: u64, salt: u64) -> f64 {
    unit(mix64(mix64(a ^ salt.rotate_left(17)) ^ b.rotate_left(31)))
}

/// Paint `hairs` onto a `size` by `size` template of a card `aspect` wide
/// for its length, in place: a fringe of hairs out from its edge into the
/// texels around it, each a
/// stroke a texel wide standing out from the outline and curving toward
/// the tip (FNA's "marginal hairs perpendicular to margin or curved toward
/// apex", Asarum). Texels within `border` of the template's sides and tip
/// stay clear. Returns the coverage the fringe added to each texel, which
/// the leaf's area leaves out.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn paint(
    size: usize,
    aspect: f64,
    border: usize,
    hairs: &Hairs,
    seed: u64,
    coverage: &mut [f32],
    brightness: &mut [f32],
    accent: &mut [f32],
) -> Vec<f32> {
    let texels = size * size;
    let mut fringe = vec![0.0_f32; texels];
    if !hairs.drawn() || size < 4 {
        return fringe;
    }
    let density = hairs.density.clamp(0.0, 1.0);
    let lie = hairs.lie.clamp(0.0, 1.0);
    let curl = hairs.curl.clamp(0.0, 1.0);
    // The fringe, from the edge texels: covered, beside one that is not.
    let covered = |x: isize, y: isize| -> f64 {
        if x < 0 || y < 0 || x >= size as isize || y >= size as isize {
            return 0.0;
        }
        f64::from(coverage[y as usize * size + x as usize])
    };
    let edge = hairs.edge.unwrap_or(density).clamp(0.0, 1.0);
    // Physical units: x across in card lengths (`aspect` wide), y along.
    let across = aspect / size as f64;
    let along = 1.0 / size as f64;
    // A stroke reaches out one and a half to three texels, longer for
    // longer hairs: a texel is a millimetre or so of a leaf's card.
    let reach = (hairs.length / 0.0015).clamp(0.6, 2.0);
    let mut strokes = Vec::new();
    for y in 1..size as isize - 1 {
        for x in 1..size as isize - 1 {
            if covered(x, y) < 0.5 {
                continue;
            }
            let open = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dy)| covered(x + dx, y + dy) < 0.5);
            if !open {
                continue;
            }
            // About one edge texel in three bears a hair on a fully
            // fringed edge.
            if hash(x as u64, y as u64, seed ^ 0x4652_494e) >= edge * 0.4 {
                continue;
            }
            // The outward normal: down the slope of coverage round it.
            let mut gx = 0.0;
            let mut gy = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let c = covered(x + dx, y + dy);
                    gx += f64::from(dx as i32) * c;
                    gy += f64::from(dy as i32) * c;
                }
            }
            let (nx, ny) = (-gx / across, -gy / along);
            let norm = math::sqrt(nx * nx + ny * ny);
            if norm < 1e-9 {
                continue;
            }
            let (nx, ny) = (nx / norm, ny / norm);
            // Curved toward the tip, more for hairs that lie flat.
            let jitter = hash(x as u64, y as u64, seed ^ 0x4355_524c) - 0.5;
            let lean = (0.25 + 0.5 * (1.0 - lie)) + jitter * (0.4 + curl);
            let (dx, dy) = (nx, ny + lean.max(0.0) * (1.0 - ny.abs()));
            let length = (1.5 + 1.5 * hash(x as u64, y as u64, seed ^ 0x4c45_4e47)) * reach * along;
            let norm = math::sqrt(dx * dx + dy * dy);
            strokes.push((x, y, dx / norm, dy / norm, length));
        }
    }
    let low = border.max(1) as f64;
    let high_x = (size - border.max(1)) as f64 - 1.0;
    let high_y = (size - border.max(1)) as f64 - 1.0;
    for (x, y, dx, dy, length) in strokes {
        let steps = ((length / along) * 4.0).ceil().max(1.0) as usize;
        for step in 1..=steps {
            let t = length * step as f64 / steps as f64;
            let px = x as f64 + dx * t / across;
            let py = y as f64 + dy * t / along;
            if px < low || py < low || px > high_x || py > high_y {
                break;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let at = py.round() as usize * size + px.round() as usize;
            if coverage[at] >= 0.5 {
                continue;
            }
            fringe[at] += 1.0 - coverage[at];
            coverage[at] = 1.0;
            brightness[at] = FRINGE_BRIGHTNESS;
            accent[at] = 0.0;
        }
    }
    fringe
}

/// The light a face's hairs add, seen along `view` with its normal
/// `normal` turned
/// toward the eye: a sheen where the face is seen edge on, stronger for
/// hairs pressed flat (silky, sericeous), and a glow where the sun shines
/// through them toward the eye, stronger for hairs standing up (a backlit
/// halo), in `lighting`; `lit` is the sun's share that reaches the point.
#[must_use]
pub fn sheen(hairs: &Hairs, normal: Vec3, view: Vec3, lit: f64, lighting: &Lighting) -> [f64; 3] {
    let (sun, sun_colour, sky) = (lighting.sun, lighting.sun_color, lighting.sky);
    // It goes with the square of how much of the face the hairs cover: a
    // few hairs catch little light, a felt shines.
    let density = hairs.density.clamp(0.0, 1.0);
    let weight = density * density;
    let lie = hairs.lie.clamp(0.0, 1.0);
    let edge_on = 1.0 - normal.dot(view).abs().min(1.0);
    let graze = edge_on * edge_on * edge_on;
    let behind = (-view.dot(sun)).max(0.0);
    let glow = behind * behind * behind * behind;
    let sunlit = lit * (0.5 * (1.0 - 0.5 * lie) * graze + 0.9 * (0.4 + 0.6 * lie) * glow);
    let skylit = 0.25 * graze;
    std::array::from_fn(|channel| {
        f64::from(hairs.colour[channel])
            * weight
            * (f64::from(sun_colour[channel]) * sunlit + f64::from(sky[channel]) * skylit)
    })
}

/// Whether a card draws its edge's painted fringe at a point where a
/// pixel is `pixel` wide: only where its hairs are too short to draw as
/// strands ([`strand_mesh`], at least two pixels long).
#[must_use]
pub fn fringe_drawn(hairs: &Hairs, pixel: f64) -> bool {
    hairs.length < 2.0 * pixel
}

/// The light on a strand with unit tangent `tangent`, seen along `view`
/// toward the eye in `lighting`, `lit` the sun's share reaching it, in a
/// colour `albedo`: a fibre's diffuse light, strongest across its
/// length, its glint, and its glow when the sun shines through it.
#[must_use]
pub fn strand_light(
    albedo: [f64; 3],
    tangent: Vec3,
    view: Vec3,
    lit: f64,
    lighting: &Lighting,
) -> [f64; 3] {
    let (sun, sun_colour, sky, ground) = (
        lighting.sun,
        lighting.sun_color,
        lighting.sky,
        lighting.ground,
    );
    let across = |d: Vec3| math::sqrt((1.0 - tangent.dot(d).powi(2)).max(0.0));
    let diffuse = across(sun) * lit;
    let half = (sun + view).normalize_or(sun);
    let glint = across(half).powi(60) * lit;
    let behind = (-view.dot(sun)).max(0.0);
    let glow = behind.powi(4) * lit;
    std::array::from_fn(|channel| {
        albedo[channel]
            * (f64::from(sun_colour[channel]) * (0.6 * diffuse + 1.2 * glow)
                + 0.75 * f64::from(sky[channel])
                + 0.25 * f64::from(ground[channel]))
            + f64::from(sun_colour[channel]) * 0.2 * glint
    })
}

/// The light on a glandular hair's head with unit `normal`, seen along
/// `view` toward the eye in `lighting`, `lit` the sun's share reaching it,
/// in a colour `albedo`: a clear bead's soft light, its sharp glint where
/// it mirrors the sun, and its glow where the sun shines through it.
#[must_use]
pub fn gland_light(
    albedo: [f64; 3],
    normal: Vec3,
    view: Vec3,
    lit: f64,
    lighting: &Lighting,
) -> [f64; 3] {
    let (sun, sun_colour, sky) = (lighting.sun, lighting.sun_color, lighting.sky);
    let diffuse = normal.dot(sun).max(0.0) * lit;
    let half = (sun + view).normalize_or(sun);
    let glint = normal.dot(half).max(0.0).powi(80) * lit;
    let behind = (-view.dot(sun)).max(0.0);
    let glow = behind.powi(4) * lit;
    std::array::from_fn(|channel| {
        albedo[channel]
            * (f64::from(sun_colour[channel]) * (0.5 * diffuse + 1.0 * glow)
                + 0.6 * f64::from(sky[channel]))
            + f64::from(sun_colour[channel]) * 1.4 * glint
    })
}

/// A bent card's point at `u` across (0 left, 1 right) and `v` along, and
/// its unit normal on the face side: as [`PlantMesh::card_mesh`] lays it.
fn card_point(card: &Card, u: f64, v: f64) -> (Vec3, Vec3) {
    card.bend.point(
        vector(card.base),
        vector(card.heading),
        vector(card.left),
        f64::from(card.width) * 0.5,
        f64::from(card.length),
        2.0 * u - 1.0,
        v,
    )
}

/// A bent card's point at `u` across and `v` along on the surface a
/// renderer draws, its [`crate::bend::ACROSS`] by [`crate::bend::ALONG`]
/// cells of two triangles each, rather than on the curve they stand for:
/// roots set on the curve sink under a cupped card's flat cells.
fn drawn_point(card: &Card, u: f64, v: f64) -> Vec3 {
    let across = f64::from(crate::bend::ACROSS);
    let along = f64::from(crate::bend::ALONG);
    let (su, sv) = (u.clamp(0.0, 1.0) * across, v.clamp(0.0, 1.0) * along);
    let (column, row) = (su.floor().min(across - 1.0), sv.floor().min(along - 1.0));
    let (a, b) = (su - column, sv - row);
    let at = |i: f64, j: f64| card_point(card, (column + i) / across, (row + j) / along).0;
    let (p00, p11) = (at(0.0, 0.0), at(1.0, 1.0));
    // Each cell is cut along its diagonal from (0, 0) to (1, 1), as
    // `crate::bend::corner` lays its triangles.
    if a >= b {
        let p10 = at(1.0, 0.0);
        p00 + (p10 - p00) * a + (p11 - p10) * b
    } else {
        let p01 = at(0.0, 1.0);
        p00 + (p01 - p00) * b + (p11 - p01) * a
    }
}

/// One strand, before it is meshed: its root, its two directions (the
/// second for its far half), its length, its root's radius, the normal of
/// the face or the wood it stands on, how far it is lifted off the face
/// (zero for an edge's or a stalk's), and the kind of hair it draws.
struct Strand {
    root: Vec3,
    first: Vec3,
    second: Vec3,
    length: f64,
    radius: f64,
    normal: Vec3,
    rest: Vec3,
    form: Form,
}

/// An edge texel: its `u`, `v` and its outward normal across and along
/// the card, in card lengths.
type Edge = (f64, f64, f64, f64);

/// The edge texels of a template with their outward normals, in the
/// card's `u`, `v` (each from 0 to 1) and in card lengths: where the fringe
/// starts, and up close where edge strands do.
fn edges(templates: &crate::templates::Templates, template: usize) -> Vec<Edge> {
    let Some(drawn) = templates.templates.get(template) else {
        return Vec::new();
    };
    let size = templates.size;
    let solid = |x: isize, y: isize| -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < size
            && (y as usize) < size
            && drawn.coverage[y as usize * size + x as usize] >= 0.5
            && drawn
                .fringe
                .get(y as usize * size + x as usize)
                .copied()
                .unwrap_or(0.0)
                <= 0.0
    };
    let mut out = Vec::new();
    for y in 0..size as isize {
        for x in 0..size as isize {
            if !solid(x, y) {
                continue;
            }
            let mut gx = 0.0;
            let mut gy = 0.0;
            let mut open = false;
            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    let s = solid(x + dx, y + dy);
                    if !s && (dx == 0 || dy == 0) {
                        open = true;
                    }
                    let c = if s { 1.0 } else { 0.0 };
                    gx += dx as f64 * c;
                    gy += dy as f64 * c;
                }
            }
            if !open {
                continue;
            }
            let (nx, ny) = (-gx / drawn.aspect, -gy);
            let norm = math::sqrt(nx * nx + ny * ny);
            if norm < 1e-9 {
                continue;
            }
            out.push((
                (x as f64 + 0.5) / size as f64,
                (y as f64 + 0.5) / size as f64,
                nx / norm,
                ny / norm,
            ));
        }
    }
    out
}

/// How one hair is drawn: a plain strand, a star or a little tree of
/// them (`crate::looks::Branching`), a strand ending in a round head of
/// resin (a glandular hair), or a stinging hair, a glassy needle on a
/// swollen green foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Plain,
    Wool,
    Stellate,
    Dendritic,
    Gland,
    Sting,
}

impl Form {
    /// The strands one hair of this form is drawn with, which the budget
    /// counts ([`MAX_STRANDS`]).
    fn pieces(self) -> f64 {
        match self {
            Self::Plain => 1.0,
            Self::Wool => 4.0,
            Self::Stellate => 7.0,
            Self::Dendritic => 13.0,
            Self::Gland => 2.0,
            Self::Sting => 1.5,
        }
    }
}

/// Each kind of hair `hairs` holds that has any to draw, with the form
/// it is drawn in: the hairs themselves, then the glandular and the
/// stinging hairs among them.
/// The curl from which a simple hair is drawn as wool ([`Form::Wool`]).
const WOOL_CURL: f64 = 0.85;

fn kinds(hairs: &Hairs) -> Vec<(&Hairs, Form)> {
    let mut out = Vec::new();
    if hairs.density > 0.0 && hairs.length > 0.0 {
        let form = match hairs.branching {
            // Hairs curled through more than a turn (lanate, woolly,
            // crisped) wind as they go, which two bends can't draw.
            crate::looks::Branching::Simple if hairs.curl >= WOOL_CURL => Form::Wool,
            crate::looks::Branching::Simple => Form::Plain,
            crate::looks::Branching::Stellate => Form::Stellate,
            crate::looks::Branching::Dendritic => Form::Dendritic,
        };
        out.push((hairs, form));
    }
    for (among, form) in [(&hairs.glands, Form::Gland), (&hairs.stings, Form::Sting)] {
        if let Some(among) = among.as_deref().filter(|among| among.own_drawn()) {
            out.push((among, form));
        }
    }
    out
}

/// How many hairs of one kind stand on `area` square metres, each about
/// its length from the next where they hide the whole face, and a felt's
/// or a wool's in layers over one another: up to five deep where they
/// hide it all (`density` 1), one where they hide six tenths of it or
/// less.
fn hairs_on(hairs: &Hairs, form: Form, area: f64) -> f64 {
    // Wool lies along itself, its hairs a third of their length apart.
    let apart = if form == Form::Wool { 0.35 } else { 0.9 };
    let spacing = (hairs.length * apart).max(1e-4);
    let layers = 1.0 + 4.0 * ((hairs.density - 0.6) / 0.4).clamp(0.0, 1.0);
    hairs.density * layers * area / (spacing * spacing)
}

/// The share of its hairs each card or tube draws, from the strands it
/// would draw (`wanted`) and how many pixels long its longest hairs are
/// in view (`scale`): all of them while [`MAX_STRANDS`] holds them, else
/// all of them on the ones nearest the eye, falling off by the square of
/// the scale below a threshold found so they spend the budget: the near
/// hairs drawn as thick as they stand, the far ones left to the card's
/// own shading ([`cast`]).
fn budget_shares(wanted: impl Iterator<Item = (f64, f64)>) -> Vec<f64> {
    let wanted: Vec<(f64, f64)> = wanted.collect();
    let budget = MAX_STRANDS as f64;
    let spent = |threshold: f64| -> f64 {
        wanted
            .iter()
            .map(|&(count, scale)| count * (scale / threshold).powi(2).min(1.0))
            .sum()
    };
    let total: f64 = wanted.iter().map(|&(count, _)| count).sum();
    if total <= budget {
        return vec![1.0; wanted.len()];
    }
    // The threshold rises until the budget holds, over orders of
    // magnitude: bisect its logarithm.
    let (mut low, mut high) = (1e-3_f64, 1e7_f64);
    for _ in 0..60 {
        let middle = math::sqrt(low * high);
        if spent(middle) > budget {
            low = middle;
        } else {
            high = middle;
        }
    }
    wanted
        .iter()
        .map(|&(_, scale)| (scale / high).powi(2).min(1.0))
        .collect()
}

/// A pixel's width, where `pixel` gives it (infinite out of view), at
/// each of a grid of points over `card`, for [`seen_share`].
fn view_pixels(card: &Card, pixel: &dyn Fn(Vec3) -> f64) -> Vec<f64> {
    const GRID: usize = 15;
    let mut out = Vec::with_capacity(GRID * GRID);
    for y in 0..GRID {
        for x in 0..GRID {
            let (u, v) = (x as f64 / (GRID - 1) as f64, y as f64 / (GRID - 1) as f64);
            out.push(pixel(card_point(card, u, v).0));
        }
    }
    out
}

/// The share of a card where hairs `length` long are drawn one by one,
/// from its [`view_pixels`]: the points in view where they are at least
/// two pixels long ([`fringe_drawn`]), at least one where its `finest`
/// pixel is that fine. Only those hairs are drawn, so only they spend
/// the budget.
fn seen_share(pixels: &[f64], finest: f64, length: f64) -> f64 {
    if !finest.is_finite() || 2.0 * finest > length {
        return 0.0;
    }
    let seen = pixels
        .iter()
        .filter(|&&p| p.is_finite() && 2.0 * p <= length)
        .count();
    seen.max(1) as f64 / pixels.len().max(1) as f64
}

/// Every hair on the bent cards of `plant` (the levels that keep a card
/// per organ) whose look has hairs, as a strand at least two pixels long
/// where `pixel` gives a pixel's width at a point (infinite where the
/// point is out of view, which draws none there and spends none of the
/// budget on it), and every hair on the plant's young wood and stalks
/// ([`PlantMesh::hair_tubes`]): each strand a tapering three-sided solid,
/// its vertex normals its tangent (for [`strand_light`]), in its hairs'
/// colour; a branched hair a star or a little tree of them, a stinging
/// hair a needle on a swollen foot, a glandular hair a stalk whose round
/// head goes in [`Strands::heads`]. At most about [`MAX_STRANDS`], the
/// nearest cards' kept whole where there would be more ([`budget_shares`]).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn strand_mesh(
    plant: &PlantMesh,
    templates: &crate::templates::Templates,
    pixel: &dyn Fn(Vec3) -> f64,
) -> Strands {
    let bent = plant.bent_cards();
    let mut edge_lists: Vec<Option<Vec<Edge>>> = vec![None; templates.templates.len()];
    // Count first, to share the budget out.
    let mut vein_maps: Vec<Option<Vec<f32>>> = vec![None; templates.templates.len()];
    let card_area = |card: &Card, template: &crate::templates::Template| {
        f64::from(card.length) * f64::from(card.length) * template.area()
    };
    let wanted: Vec<(f64, f64)> = plant.cards[..bent]
        .iter()
        .map(|card| {
            let Some(template) = templates.templates.get(usize::from(card.template)) else {
                return (0.0, 0.0);
            };
            if (template.hairs.is_none() && template.hairs_beneath.is_none())
                || !nearest_pixel(card, pixel).is_finite()
            {
                return (0.0, 0.0);
            }
            let area = card_area(card, template);
            let pixels = view_pixels(card, pixel);
            let finest = nearest_pixel(card, pixel);
            let seen = |length: f64| seen_share(&pixels, finest, length);
            let on = |hairs: Option<&Hairs>| {
                hairs.map_or(0.0, |hairs| {
                    kinds(hairs)
                        .iter()
                        .map(|(kind, form)| {
                            seen(kind.length) * hairs_on(kind, *form, area) * form.pieces()
                        })
                        .sum::<f64>()
                })
            };
            let longest = [template.hairs.as_ref(), template.hairs_beneath.as_ref()]
                .into_iter()
                .flatten()
                .flat_map(kinds)
                .map(|(kind, _)| kind.length)
                .fold(0.0, f64::max);
            (
                on(template.hairs.as_ref())
                    + on(template.hairs_beneath.as_ref())
                    + template
                        .hairs
                        .as_ref()
                        .map_or(0.0, |hairs| seen(hairs.length))
                        * edge_hairs(template, area),
                longest / finest,
            )
        })
        .collect();
    let tube_wanted: Vec<(f64, f64)> = plant
        .hair_tubes
        .iter()
        .map(|tube| {
            let (start, end) = (vector(tube.start), vector(tube.end));
            let middle = pixel((start + end) * 0.5);
            if !middle.is_finite() {
                return (0.0, 0.0);
            }
            let area = 2.0 * math::PI * f64::from(tube.radius) * (end - start).length();
            let kinds = kinds(&tube.hairs);
            (
                kinds
                    .iter()
                    .map(|(kind, form)| hairs_on(kind, *form, area) * form.pieces())
                    .sum(),
                kinds
                    .iter()
                    .map(|(kind, _)| kind.length)
                    .fold(0.0, f64::max)
                    / middle,
            )
        })
        .collect();
    let shares = budget_shares(wanted.iter().chain(&tube_wanted).copied());
    let (card_shares, tube_shares) = shares.split_at(wanted.len());
    let mut out = Strands::default();
    let mut strands = Vec::new();
    for (index, card) in plant.cards[..bent].iter().enumerate() {
        let share = card_shares[index];
        if wanted[index].0 <= 0.0 || share <= 0.0 {
            continue;
        }
        let template = usize::from(card.template);
        let drawn = &templates.templates[template];
        let area = card_area(card, drawn);
        // The finest pixel over the card's part in view.
        let finest = nearest_pixel(card, pixel);
        let seed = mix64(index as u64 ^ 0x5354_5241_4e44);
        let turn = |k: u64, salt: u64| hash(k, salt, seed) - 0.5;
        // The upper face is the side the card's normal points to unless
        // the card is flipped (`crate::templates::card_flipped`).
        let upward = if crate::templates::card_flipped(card.heading, card.left) {
            -1.0
        } else {
            1.0
        };
        strands.clear();
        for lower in [false, true] {
            let Some(face) = drawn.hairs_on(lower) else {
                continue;
            };
            let side_sign = if lower { -upward } else { upward };
            for (kind_index, (hairs, form)) in kinds(face).into_iter().enumerate() {
                // Too short to see: none drawn one by one.
                if fringe_drawn(hairs, finest) {
                    continue;
                }
                let lie = hairs.lie.clamp(0.0, 1.0);
                let curl = hairs.curl.clamp(0.0, 1.0);
                // The glandular and stinging hairs among a face's hairs
                // stand where they please, not on its veins.
                let gathered = if form == Form::Gland || form == Form::Sting {
                    0.0
                } else {
                    face.veins.unwrap_or(0.0).clamp(0.0, 1.0)
                };
                let veins = if gathered > 0.0 {
                    Some(
                        vein_maps[template]
                            .get_or_insert_with(|| vein_map(drawn, templates.size))
                            .as_slice(),
                    )
                } else {
                    None
                };
                let salt = (if lower { 0x42_454e_4541_5448 } else { 0 })
                    ^ (kind_index as u64).wrapping_mul(0x4b49_4e44_0000);
                // Hairs on the face, at random points the template covers,
                // gathered on the veins as much as they are said to be.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let on_face = (hairs_on(hairs, form, area) * share).round() as u64;
                let mut tries = 0_u64;
                let mut placed = 0_u64;
                while placed < on_face && tries < on_face * 8 + 16 {
                    tries += 1;
                    let k = tries ^ salt;
                    let (u, v) = (hash(k, 2, seed), hash(k, 3, seed));
                    #[allow(clippy::cast_possible_truncation)]
                    let texel = templates.sample(template, u as f32, v as f32);
                    if texel.coverage < 0.5 || texel.fringe {
                        continue;
                    }
                    if let Some(veins) = veins {
                        let on_vein = f64::from(veins[texel_index(templates.size, u, v)]);
                        if hash(k, 10, seed) >= 1.0 - gathered * (1.0 - on_vein) {
                            continue;
                        }
                    }
                    placed += 1;
                    let (_, normal) = card_point(card, u, v);
                    let root = drawn_point(card, u, v);
                    let normal = normal * side_sign;
                    let (ahead, _) = card_point(card, u, (v + 0.01).min(1.0));
                    let (behind, _) = card_point(card, u, (v - 0.01).max(0.0));
                    let tipward = (ahead - behind).normalize_or(math::any_perpendicular(normal));
                    let (first, second) = lean(normal, tipward, lie, curl, &|salt| turn(k, salt));
                    strands.push(Strand {
                        root,
                        first,
                        second,
                        length: hairs.length * (0.6 + 0.8 * hash(k, 1, seed)),
                        radius: 0.0,
                        normal,
                        rest: normal,
                        form,
                    });
                }
            }
        }
        // Hairs along the edge, standing out of it in the face's plane:
        // plain ones, whatever branching the face's have.
        if let Some(hairs) = drawn
            .hairs
            .as_ref()
            .filter(|hairs| !fringe_drawn(hairs, finest))
        {
            let lie = hairs.lie.clamp(0.0, 1.0);
            let curl = hairs.curl.clamp(0.0, 1.0);
            let list = edge_lists[template].get_or_insert_with(|| edges(templates, template));
            if !list.is_empty() {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let on_edge = (edge_hairs(drawn, area) * share).round() as u64;
                for k in 0..on_edge {
                    let pick = hash(k, 7, seed) * list.len() as f64;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let (u, v, nx, ny) = list[(pick as usize).min(list.len() - 1)];
                    let (_, normal) = card_point(card, u, v);
                    let root = drawn_point(card, u, v);
                    let (ahead, _) = card_point(card, u, (v + 0.01).min(1.0));
                    let (behind, _) = card_point(card, u, (v - 0.01).max(0.0));
                    let (right, _) = card_point(card, (u + 0.01).min(1.0), v);
                    let (left, _) = card_point(card, (u - 0.01).max(0.0), v);
                    let tipward = (ahead - behind).normalize_or(math::any_perpendicular(normal));
                    let rightward = (right - left).normalize_or(math::any_perpendicular(tipward));
                    let out_of = (rightward * nx + tipward * ny).normalize_or(rightward);
                    let bend = 0.25 + 0.5 * (1.0 - lie) + turn(k, 8) * (0.4 + curl);
                    let first = (out_of + tipward * bend.max(0.0) + normal * (0.15 * lie))
                        .normalize_or(out_of);
                    let second =
                        (first + tipward * (0.3 * (1.0 - lie)) + normal * turn(k, 9) * curl)
                            .normalize_or(first);
                    strands.push(Strand {
                        root,
                        first,
                        second,
                        length: hairs.length * (0.6 + 0.8 * hash(k + 1_000_003, 1, seed)),
                        radius: 0.0,
                        normal,
                        rest: Vec3::ZERO,
                        form: Form::Plain,
                    });
                }
            }
        }
        let lifetime = (
            f64::from(card.born),
            card.shed.is_finite().then_some(f64::from(card.shed)),
        );
        let face = drawn.hairs.as_ref().or(drawn.hairs_beneath.as_ref());
        for (n, strand) in strands.iter_mut().enumerate() {
            // Out of view, or too short there to see: placed, so the rest
            // stand where they would, but not drawn.
            let here = pixel(strand.root);
            if !here.is_finite() || strand.length < 2.0 * here {
                continue;
            }
            // Real hairs are a fiftieth of their length thick or less;
            // narrower than half a pixel, a camera blurs them to that.
            strand.radius = (strand.length * 0.012).max(0.5 * here);
            // A hair pressed flat lies on the face, not in it.
            strand.root += strand.rest * (strand.radius * 1.2 + 1e-5);
            let colours = colours_of(face, strand.form, card.color);
            push_hair(
                &mut out,
                strand,
                &colours,
                lifetime,
                card.segment,
                mix64(seed ^ n as u64),
            );
        }
    }
    // Hairs on the young stems and the stalks, standing out of the wood
    // and leaning toward its tip as a blade's lean toward its own.
    for (index, tube) in plant.hair_tubes.iter().enumerate() {
        let share = tube_shares[index];
        if tube_wanted[index].0 <= 0.0 || share <= 0.0 {
            continue;
        }
        let (start, end) = (vector(tube.start), vector(tube.end));
        let seed = mix64(index as u64 ^ 0x5455_4245);
        let turn = |k: u64, salt: u64| hash(k, salt, seed) - 0.5;
        let tipward = (end - start).normalize_or(Vec3::Y);
        let across = math::any_perpendicular(tipward);
        let other = tipward.cross(across);
        let lifetime = (
            f64::from(tube.born),
            tube.shed.is_finite().then_some(f64::from(tube.shed)),
        );
        let area = 2.0 * math::PI * f64::from(tube.radius) * (end - start).length();
        for (kind_index, (hairs, form)) in kinds(&tube.hairs).into_iter().enumerate() {
            if fringe_drawn(hairs, pixel((start + end) * 0.5)) {
                continue;
            }
            let lie = hairs.lie.clamp(0.0, 1.0);
            let curl = hairs.curl.clamp(0.0, 1.0);
            let colours = colours_of(Some(&tube.hairs), form, tube.colour);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let count = (hairs_on(hairs, form, area) * share).round() as u64;
            let salt = (kind_index as u64).wrapping_mul(0x4b49_4e44_0000);
            for k in (0..count).map(|k| k ^ salt) {
                let along = hash(k, 2, seed);
                let angle = 2.0 * math::PI * hash(k, 3, seed);
                let normal = across * math::cos(angle) + other * math::sin(angle);
                let (first, second) = lean(normal, tipward, lie, curl, &|salt| turn(k, salt));
                let length = hairs.length * (0.6 + 0.8 * hash(k, 1, seed));
                let root = start + (end - start) * along + normal * f64::from(tube.radius);
                let strand = Strand {
                    root,
                    first,
                    second,
                    length,
                    radius: (length * 0.012).max(0.5 * pixel(root)),
                    normal,
                    rest: Vec3::ZERO,
                    form,
                };
                push_hair(
                    &mut out,
                    &strand,
                    &colours,
                    lifetime,
                    tube.segment,
                    mix64(seed ^ k),
                );
            }
        }
    }
    out
}

/// The strands of [`strand_mesh`]: the hairs, lit as fibres, and the
/// round heads of the glandular ones, lit as drops of resin
/// (`crate::raster::Material::Gland`).
#[derive(Debug, Clone, Default)]
pub struct Strands {
    pub hairs: Mesh,
    pub heads: Mesh,
}

/// How many hairs fringe the edge of a card of `template` with `area`
/// square metres of face.
fn edge_hairs(template: &crate::templates::Template, area: f64) -> f64 {
    template.hairs.as_ref().map_or(0.0, |hairs| {
        let spacing = (hairs.length * 0.9).max(1e-4);
        let perimeter = 3.5 * math::sqrt(area);
        hairs.edge.unwrap_or(hairs.density) * perimeter / (spacing * 0.7)
    })
}

/// A hair's two directions standing from a face with unit `normal`,
/// leaning toward `tipward`, as its `lie` and `curl` set: risen from the
/// face by the lie, swung aside by the curl, its far half bowed over
/// where it is soft and turned on where it is curled; `turn` gives a
/// number in `[-0.5, 0.5)` for a salt.
fn lean(
    normal: Vec3,
    tipward: Vec3,
    lie: f64,
    curl: f64,
    turn: &dyn Fn(u64) -> f64,
) -> (Vec3, Vec3) {
    let tipward =
        (tipward - normal * tipward.dot(normal)).normalize_or(math::any_perpendicular(normal));
    let side = normal
        .cross(tipward)
        .normalize_or(math::any_perpendicular(tipward));
    let rise = math::radians((lie * 80.0 + turn(4) * 24.0).clamp(3.0, 88.0));
    let swing = math::radians(turn(5) * (40.0 + curl * 120.0));
    let first = (tipward * (math::cos(rise) * math::cos(swing))
        + side * (math::cos(rise) * math::sin(swing))
        + normal * math::sin(rise))
    .normalize_or(normal);
    // Soft, long hairs bow over; curled ones turn on.
    let bow = (1.0 - lie) * 0.3 + curl * 0.8;
    let second = (first + side * (turn(6) * 2.0 * curl) + (tipward - normal * 0.5) * (bow * 0.5))
        .normalize_or(first);
    (first, second)
}

/// The colours a hair of `form` is drawn in on an organ coloured `organ`
/// whose hairs are `face`: its strand's, and its foot's or its head's.
/// A living hair is clear: it shows the organ's colour through it; a
/// branched hair or wool is dead and white with air. A stinging
/// hair's needle is glass, its foot the organ's own green flesh; a
/// glandular hair's head is a drop of resin, in its hairs' colour.
fn colours_of(face: Option<&Hairs>, form: Form, organ: [f32; 3]) -> [[f32; 3]; 2] {
    let of = |hairs: Option<&Hairs>| hairs.map_or([0.8, 0.8, 0.76], |hairs| hairs.colour);
    let kind = match form {
        Form::Gland => face.and_then(|face| face.glands.as_deref()),
        Form::Sting => face.and_then(|face| face.stings.as_deref()),
        _ => face,
    };
    let colour = of(kind);
    let organ = organ.map(|c| c.max(0.0));
    let mix = |weight: f32| std::array::from_fn(|c| weight * colour[c] + (1.0 - weight) * organ[c]);
    match form {
        Form::Sting => [mix(0.55), organ.map(|c| c * 1.08)],
        Form::Gland => [mix(0.45), mix(0.7)],
        // Branched hairs and wool are dead and full of air: white, not
        // clear.
        Form::Stellate | Form::Dendritic | Form::Wool => [mix(0.8), mix(0.8)],
        Form::Plain => [mix(0.45), mix(0.45)],
    }
}

/// `strand` as its form draws it, into `out`: `colours` its strand's and
/// its foot's or head's, `seed` the hair's own for its arms.
#[allow(clippy::too_many_lines)]
fn push_hair(
    out: &mut Strands,
    strand: &Strand,
    colours: &[[f32; 3]; 2],
    lifetime: (f64, Option<f64>),
    segment: u32,
    seed: u64,
) {
    let [colour, other] = *colours;
    let arm = |k: u64, salt: u64| hash(k, salt, seed);
    match strand.form {
        Form::Plain => push_strand(&mut out.hairs, strand, colour, lifetime, segment),
        Form::Wool => {
            // Seven rings down a wandering path: the hair's own lean, with
            // a crimp that turns about it once and a quarter as it goes,
            // settling back toward the face as wool mats down.
            let along = strand.first.normalize_or(strand.normal);
            let a = math::any_perpendicular(along);
            let b = along.cross(a);
            let phase = 2.0 * math::PI * arm(0, 6);
            let crimp = strand.length * 0.1;
            let settle = strand.normal.normalize_or(Vec3::ZERO) * (strand.length * 0.22);
            let at = |t: f64| -> Vec3 {
                let bent = strand.first * (t.min(0.5)) + strand.second * (t - 0.5).max(0.0);
                let angle = phase + 2.5 * math::PI * t;
                strand.root + bent * strand.length - settle * (t * t)
                    + (a * math::cos(angle) + b * math::sin(angle))
                        * (crimp * math::sin(math::PI * t.min(0.9) / 0.9))
            };
            let rings: Vec<(Vec3, Vec3, f64, [f32; 3])> = (0..7)
                .map(|k| {
                    let t = f64::from(k) / 7.0;
                    let here = at(t);
                    let next = at(t + 0.07);
                    (
                        here,
                        (next - here).normalize_or(along),
                        strand.radius * (1.0 - 0.6 * t),
                        colour,
                    )
                })
                .collect();
            let tip = at(1.0);
            let tip_along = (tip - at(0.93)).normalize_or(strand.second);
            push_profile(
                &mut out.hairs,
                &rings,
                (tip, tip_along, colour),
                lifetime,
                segment,
            );
        }
        Form::Stellate => {
            // A short stalk, then five to eight arms spread over the face
            // from its top, rising a little from it.
            let normal = strand.normal.normalize_or(strand.first);
            let top = strand.root + normal * (strand.length * 0.08);
            push_strand(
                &mut out.hairs,
                &Strand {
                    root: strand.root,
                    first: normal,
                    second: normal,
                    length: strand.length * 0.08,
                    form: Form::Plain,
                    ..*strand
                },
                colour,
                lifetime,
                segment,
            );
            let a = math::any_perpendicular(normal);
            let b = normal.cross(a);
            let arms = 5 + (arm(0, 1) * 4.0) as u64;
            let offset = 2.0 * math::PI * arm(0, 2);
            for k in 0..arms {
                let angle =
                    offset + 2.0 * math::PI * (k as f64 + 0.3 * (arm(k, 3) - 0.5)) / arms as f64;
                let rise = math::radians(8.0 + 22.0 * arm(k, 4));
                let out_of = a * math::cos(angle) + b * math::sin(angle);
                let first =
                    (out_of * math::cos(rise) + normal * math::sin(rise)).normalize_or(out_of);
                let second = (first - normal * 0.25).normalize_or(first);
                push_strand(
                    &mut out.hairs,
                    &Strand {
                        root: top,
                        first,
                        second,
                        length: strand.length * 0.5 * (0.7 + 0.3 * arm(k, 5)),
                        radius: strand.radius * 0.8,
                        form: Form::Plain,
                        ..*strand
                    },
                    colour,
                    lifetime,
                    segment,
                );
            }
        }
        Form::Dendritic => {
            // A stalk standing well up from the face with whorls of four
            // arms at three heights up it, the arms spreading nearly
            // square to it: a candelabrum.
            let axis = (strand.first + strand.normal * 1.5).normalize_or(strand.first);
            push_strand(
                &mut out.hairs,
                &Strand {
                    first: axis,
                    second: axis,
                    form: Form::Plain,
                    ..*strand
                },
                colour,
                lifetime,
                segment,
            );
            let a = math::any_perpendicular(axis);
            let b = axis.cross(a);
            for (tier, at) in [0.4_f64, 0.7, 0.97].into_iter().enumerate() {
                let centre = strand.root + axis * (strand.length * at);
                let offset = 2.0 * math::PI * arm(tier as u64, 6);
                for k in 0..4_u64 {
                    let angle = offset + math::PI * 0.5 * (k as f64 + 0.3 * (arm(k, 7) - 0.5));
                    let out_of = a * math::cos(angle) + b * math::sin(angle);
                    let first = (out_of + axis * (0.35 + 0.3 * arm(k + 4 * tier as u64, 8)))
                        .normalize_or(out_of);
                    push_strand(
                        &mut out.hairs,
                        &Strand {
                            root: centre,
                            first,
                            second: first,
                            length: strand.length * 0.3 * (0.7 + 0.3 * arm(k, 9)),
                            radius: strand.radius * 0.8,
                            form: Form::Plain,
                            ..*strand
                        },
                        colour,
                        lifetime,
                        segment,
                    );
                }
            }
        }
        Form::Gland => {
            push_strand(&mut out.hairs, strand, colour, lifetime, segment);
            let half = strand.length * 0.5;
            let tip = strand.root + strand.first * half + strand.second * half;
            // A tomato's capitate heads are about a fifth of their hair's
            // height across (Bergau et al. 2015: 57 µm on 0.2-0.3 mm).
            let radius = (strand.length * 0.11).max(strand.radius * 1.8);
            push_head(
                &mut out.heads,
                tip + strand.second * radius * 0.8,
                radius,
                other,
                lifetime,
                segment,
            );
        }
        Form::Sting => {
            // A swollen green foot a quarter of its height (Ensikat et
            // al. 2021: a pedestal of many cells), then a glass needle a
            // tenth of a millimetre thick at its foot for a hair 1.4 mm
            // long, straight, thinning fast to a fine shaft and a point.
            let axis = strand.first;
            let l = strand.length;
            let r = strand.radius;
            push_profile(
                &mut out.hairs,
                &[
                    (strand.root, axis, (0.075 * l).max(2.5 * r), other),
                    (
                        strand.root + axis * (0.16 * l),
                        axis,
                        (0.06 * l).max(2.0 * r),
                        other,
                    ),
                    (
                        strand.root + axis * (0.25 * l),
                        axis,
                        (0.035 * l).max(1.2 * r),
                        colour,
                    ),
                    (
                        strand.root + axis * (0.55 * l),
                        axis,
                        (0.014 * l).max(0.6 * r),
                        colour,
                    ),
                ],
                (strand.root + axis * l, axis, colour),
                lifetime,
                segment,
            );
        }
    }
}

/// The width of a pixel at the nearest of a few points over `card` (its
/// corners, its middle and the middles of its sides), infinite where none
/// is in view.
fn nearest_pixel(card: &Card, pixel: &dyn Fn(Vec3) -> f64) -> f64 {
    [
        (0.5, 0.5),
        (0.0, 0.0),
        (1.0, 0.0),
        (0.0, 1.0),
        (1.0, 1.0),
        (0.5, 0.0),
        (0.5, 1.0),
        (0.0, 0.5),
        (1.0, 0.5),
    ]
    .iter()
    .map(|&(u, v)| pixel(card_point(card, u, v).0))
    .fold(f64::INFINITY, f64::min)
}

/// The texel of a `size` by `size` template under `u`, `v`.
fn texel_index(size: usize, u: f64, v: f64) -> usize {
    let at = |c: f64| ((c.clamp(0.0, 0.999_999) * size as f64) as usize).min(size - 1);
    at(v) * size + at(u)
}

/// How much each texel of `template` lies on one of its main veins, 0 to
/// 1: how much brighter it is than the face round it, as its veins are
/// drawn lighter (`crate::templates`), full at a main vein's lightening.
fn vein_map(template: &crate::templates::Template, size: usize) -> Vec<f32> {
    let reach = 3_isize;
    let mut out = vec![0.0_f32; size * size];
    for y in 0..size as isize {
        for x in 0..size as isize {
            let at = y as usize * size + x as usize;
            if template.coverage[at] < 0.5 {
                continue;
            }
            let (mut sum, mut count) = (0.0_f64, 0.0_f64);
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= size as isize || ny >= size as isize {
                        continue;
                    }
                    let near = ny as usize * size + nx as usize;
                    if template.coverage[near] >= 0.5 {
                        sum += f64::from(template.brightness[near]);
                        count += 1.0;
                    }
                }
            }
            let above = f64::from(template.brightness[at]) - sum / count.max(1.0);
            out[at] = (above / 0.06).clamp(0.0, 1.0) as f32;
        }
    }
    out
}

/// A point of a [`Tube`] or [`Card`], in double precision.
fn vector(v: [f32; 3]) -> Vec3 {
    Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]))
}

/// A strand's solid: a ring of three at its root, a narrower ring at its
/// middle, and its tip; nine triangles.
fn push_strand(
    mesh: &mut Mesh,
    strand: &Strand,
    colour: [f32; 3],
    lifetime: (f64, Option<f64>),
    segment: u32,
) {
    let half = strand.length * 0.5;
    let middle = strand.root + strand.first * half;
    let tip = middle + strand.second * half;
    push_profile(
        mesh,
        &[
            (strand.root, strand.first, strand.radius, colour),
            (
                middle,
                (strand.first + strand.second).normalize_or(strand.first),
                strand.radius * 0.7,
                colour,
            ),
        ],
        (tip, strand.second, colour),
        lifetime,
        segment,
    );
}

/// A tapering solid through `rings` (each a centre, the direction along
/// the solid there, a radius and a colour), three vertices round each,
/// closed at `tip`: six triangles between two rings, three to the tip,
/// its vertex normals its tangent.
fn push_profile(
    mesh: &mut Mesh,
    rings: &[(Vec3, Vec3, f64, [f32; 3])],
    (tip, tip_along, tip_colour): (Vec3, Vec3, [f32; 3]),
    (born, shed): (f64, Option<f64>),
    segment: u32,
) {
    let ring = |centre: Vec3, along: Vec3, radius: f64| -> [Vec3; 3] {
        let a = math::any_perpendicular(along);
        let b = along.cross(a);
        std::array::from_fn(|k| {
            let angle = 2.0 * math::PI * k as f64 / 3.0;
            centre + (a * math::cos(angle) + b * math::sin(angle)) * radius
        })
    };
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    let mut push = |position: Vec3, tangent: Vec3, colour: [f32; 3]| {
        mesh.push(Vertex {
            position,
            normal: tangent,
            uv: [0.0, 0.0],
            color: [colour[0], colour[1], colour[2], 1.0],
            born,
            shed,
            level: 3,
            segment,
        });
    };
    // Every ring is turned as the first is, so its sides meet the next's.
    let along = rings.first().map_or(tip_along, |ring| ring.1);
    for &(centre, tangent, radius, colour) in rings {
        for p in ring(centre, along, radius) {
            push(p, tangent, colour);
        }
    }
    push(tip, tip_along, tip_colour);
    let count = u32::try_from(rings.len()).unwrap_or(0);
    let apex = first + 3 * count;
    for r in 0..count {
        let base = first + 3 * r;
        for k in 0..3_u32 {
            let next = (k + 1) % 3;
            if r + 1 < count {
                mesh.indices.extend_from_slice(&[
                    base + k,
                    base + next,
                    base + 3 + next,
                    base + k,
                    base + 3 + next,
                    base + 3 + k,
                ]);
            } else {
                mesh.indices
                    .extend_from_slice(&[base + k, base + next, apex]);
            }
        }
    }
}

/// A glandular hair's head: a round drop `radius` across at `centre`,
/// eight triangles, its vertex normals pointing out of it.
fn push_head(
    mesh: &mut Mesh,
    centre: Vec3,
    radius: f64,
    colour: [f32; 3],
    (born, shed): (f64, Option<f64>),
    segment: u32,
) {
    let first = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
    for direction in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        mesh.push(Vertex {
            position: centre + direction * radius,
            normal: direction,
            uv: [0.0, 0.0],
            color: [colour[0], colour[1], colour[2], 1.0],
            born,
            shed,
            level: 3,
            segment,
        });
    }
    for (a, b) in [(0, 2), (2, 1), (1, 3), (3, 0)] {
        mesh.indices
            .extend_from_slice(&[first + a, first + b, first + 4]);
        mesh.indices
            .extend_from_slice(&[first + b, first + a, first + 5]);
    }
}

#[cfg(test)]
// Tests check exact results: copied and cleared values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::looks::{Look, Shape, Simple};
    use crate::templates::Templates;

    fn leaf(hairs: Option<Hairs>) -> Look {
        let mut look = crate::looks::resolve(
            [("leaf", crate::lsys::OrganKind::Leaf)],
            &std::collections::BTreeMap::new(),
            [0.1, 0.2, 0.05],
            [0.03, 0.06, 0.02],
        )
        .remove(0);
        look.shape = Shape::Simple(Simple::default());
        look.hairs = hairs;
        look
    }

    fn villous() -> Hairs {
        Hairs {
            density: 0.6,
            length: 0.0025,
            lie: 0.5,
            curl: 0.45,
            ..Hairs::default()
        }
    }

    #[test]
    fn hairs_tint_the_face_and_fringe_the_edge_but_add_no_leaf() {
        let bare = Templates::for_looks(&[leaf(None)]);
        let hairy = Templates::for_looks(&[leaf(Some(villous()))]);
        let (bare_one, hairy_one) = (&bare.templates[0], &hairy.templates[0]);
        assert!(bare_one.fringe.is_empty() && bare_one.hair() == 0.0);
        // The fringe covers texels the bare blade leaves open, and only
        // those: the blade's own texels are the same.
        let fringed = hairy_one.fringe.iter().filter(|&&f| f > 0.0).count();
        assert!(fringed > 20, "{fringed} fringe texels");
        for at in 0..bare_one.coverage.len() {
            if bare_one.coverage[at] >= 0.5 {
                assert_eq!(hairy_one.coverage[at], bare_one.coverage[at]);
                assert_eq!(hairy_one.fringe[at], 0.0);
            } else if hairy_one.fringe[at] > 0.0 {
                assert_eq!(hairy_one.brightness[at], FRINGE_BRIGHTNESS);
            }
        }
        assert!((hairy_one.area() - bare_one.area()).abs() < 1e-9);
        let weight = hairy_one.hair();
        assert!(weight > 0.05 && f64::from(weight) <= CAST);
        // A hairy face is paler: its colour moves toward the hairs'.
        let middle = hairy.sample(0, 0.5, 0.5);
        assert!(middle.coverage >= 0.5 && !middle.fringe && middle.hair == weight);
        let tinted = hairy.albedo(0, [0.1, 0.2, 0.05], middle);
        let plain = bare.albedo(0, [0.1, 0.2, 0.05], bare.sample(0, 0.5, 0.5));
        assert!(tinted[0] > plain[0] && tinted[2] > plain[2]);
    }

    #[test]
    fn a_hairy_face_shines_edge_on_and_glows_against_the_sun() {
        let hairs = villous();
        let sun = Vec3::new(0.0, 1.0, 0.0);
        let up = Vec3::new(0.0, 1.0, 0.0);
        let lighting = Lighting {
            sun,
            sun_color: [1.0; 3],
            sky: [0.3; 3],
            ground: [0.1; 3],
        };
        let face_on = sheen(&hairs, up, up, 1.0, &lighting);
        let edge_on = sheen(
            &hairs,
            up,
            Vec3::new(1.0, 0.05, 0.0).normalize_or(up),
            1.0,
            &lighting,
        );
        let backlit = sheen(
            &hairs,
            up,
            Vec3::new(0.3, -1.0, 0.0).normalize_or(up),
            1.0,
            &lighting,
        );
        assert!(face_on[1] < 1e-3);
        assert!(edge_on[1] > 0.05 && backlit[1] > 0.05);
        let bare = Hairs {
            density: 0.0,
            ..villous()
        };
        assert_eq!(sheen(&bare, up, up, 1.0, &lighting), [0.0; 3]);
    }

    #[test]
    fn strands_stand_up_close_and_not_far_away() {
        use crate::mesh::Card;
        let templates = Templates::for_looks(&[leaf(Some(villous()))]);
        let card = Card {
            base: [0.0; 3],
            heading: [0.0, 0.0, 1.0],
            left: [1.0, 0.0, 0.0],
            length: 0.1,
            width: 0.06,
            color: [0.1, 0.2, 0.05],
            template: 0,
            born: 0.0,
            shed: f32::INFINITY,
            bend: crate::bend::Bend {
                cup: 0.1,
                ..crate::bend::Bend::FLAT
            },
            segment: crate::mesh::NO_SEGMENT,
            seed: 0.5,
        };
        let plant = PlantMesh {
            cards: vec![card],
            ..PlantMesh::default()
        };
        assert_eq!(plant.bent_cards(), 1);
        let near = strand_mesh(&plant, &templates, &|_| 0.0001).hairs;
        let far = strand_mesh(&plant, &templates, &|_| 0.01).hairs;
        assert!(far.indices.is_empty());
        let strands = near.indices.len() / 27;
        assert!(strands > 100, "{strands} strands");
        assert_eq!(near.indices.len() % 27, 0);
        assert!(strands <= MAX_STRANDS);
        // Every hair stands on the face, the side `heading × left` points
        // to (up), within its length of the card, which cups a few
        // millimetres.
        let reach = villous().length * 1.4 + 0.004;
        for p in &near.positions {
            let y = f64::from(p[1]);
            assert!(y <= reach && y >= -1e-3, "{p:?}");
        }
    }

    #[test]
    fn each_face_wears_its_own_hairs_whichever_way_a_card_turns() {
        use crate::mesh::Card;
        let flat = |left: [f32; 3]| Card {
            base: [0.0; 3],
            heading: [0.0, 0.0, 1.0],
            left,
            length: 0.1,
            width: 0.06,
            color: [0.1, 0.2, 0.05],
            template: 0,
            born: 0.0,
            shed: f32::INFINITY,
            bend: crate::bend::Bend {
                cup: 0.0001,
                ..crate::bend::Bend::FLAT
            },
            segment: crate::mesh::NO_SEGMENT,
            seed: 0.5,
        };
        let bare_edge = Hairs {
            edge: Some(0.0),
            ..villous()
        };
        let mut above = leaf(Some(bare_edge.clone()));
        let mut below = leaf(None);
        below.hairs_beneath = Some(bare_edge);
        above.hairs_beneath = None;
        // Its normal up (`heading × left`), then down: the upper face is
        // the one toward the sky either way.
        for left in [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]] {
            for (look, up) in [(&above, true), (&below, false)] {
                let templates = Templates::for_looks(std::slice::from_ref(look));
                let plant = PlantMesh {
                    cards: vec![flat(left)],
                    ..PlantMesh::default()
                };
                let near = strand_mesh(&plant, &templates, &|_| 0.0001).hairs;
                assert!(near.indices.len() / 27 > 100);
                for p in &near.positions {
                    let y = f64::from(p[1]);
                    assert!(
                        if up { y >= -1e-4 } else { y <= 1e-4 },
                        "{left:?} {up} {p:?}"
                    );
                }
            }
        }
        // A lower face paints its own hairs' cast, or none.
        let templates = Templates::for_looks(&[above.clone()]);
        let texel = templates.sample(0, 0.5, 0.5);
        let lower = crate::templates::Face {
            lower: true,
            seed: 0.5,
        };
        let colour = [0.1, 0.2, 0.05];
        let top = templates.paint(0, colour, texel, crate::templates::Face::UPPER);
        let bottom = templates.paint(0, colour, texel, lower);
        assert!(
            top[0] > bottom[0] && top[2] > bottom[2],
            "{top:?} {bottom:?}"
        );
    }

    #[test]
    fn hairs_said_to_lie_along_the_veins_gather_there() {
        use crate::mesh::Card;
        let gathered = Hairs {
            veins: Some(1.0),
            edge: Some(0.0),
            ..villous()
        };
        let mut look = leaf(Some(gathered));
        look.shape = Shape::Simple(Simple::default());
        let templates = Templates::for_looks(&[look]);
        let veins = vein_map(&templates.templates[0], templates.size);
        assert!(veins.iter().any(|&v| v > 0.5), "the blade draws veins");
        let card = Card {
            base: [0.0; 3],
            heading: [0.0, 0.0, 1.0],
            left: [1.0, 0.0, 0.0],
            length: 0.1,
            width: 0.06,
            color: [0.1, 0.2, 0.05],
            template: 0,
            born: 0.0,
            shed: f32::INFINITY,
            bend: crate::bend::Bend {
                cup: 0.0001,
                ..crate::bend::Bend::FLAT
            },
            segment: crate::mesh::NO_SEGMENT,
            seed: 0.5,
        };
        let covered: Vec<f32> = (0..veins.len())
            .filter(|&at| templates.templates[0].coverage[at] >= 0.5)
            .map(|at| veins[at])
            .collect();
        let mean = covered.iter().sum::<f32>() / covered.len() as f32;
        let plant = PlantMesh {
            cards: vec![card],
            ..PlantMesh::default()
        };
        let near = strand_mesh(&plant, &templates, &|_| 0.0001).hairs;
        // Each strand's root is its first vertex; the card lies in x, z.
        let mut on = 0.0_f32;
        let mut count = 0.0_f32;
        for strand in near.positions.chunks(7) {
            let root = strand[0];
            let u = f64::from(root[0]) / 0.06 + 0.5;
            let v = f64::from(root[2]) / 0.1;
            on += veins[texel_index(templates.size, u, v)];
            count += 1.0;
        }
        assert!(count > 50.0);
        assert!(
            on / count > 2.0 * mean,
            "{} on veins, {mean} over the face",
            on / count
        );
    }

    #[test]
    fn a_species_stating_its_hairs_draws_them() {
        let spec = crate::spec::PlantSpec::builtin("asarum-canadense").unwrap();
        let hairs = spec.appearance.organs["leaf"].hairs.as_ref().unwrap();
        // Strigose (FNA: appressed-hirsute), and ciliate.
        assert!((hairs.density - 0.45).abs() < 1e-9 && hairs.lie < 0.1);
        assert_eq!(hairs.edge, Some(0.8));
        // Its leaf stalks are crisped-hirsute (FNA): curled hairs.
        let stems = spec.appearance.stem_hairs.as_ref().unwrap();
        assert!(stems.drawn() && stems.curl > 0.8, "{stems:?}");
    }

    /// Stem hairs keep on the first year's wood, wear off in the second
    /// and cast young wood toward their colour as they do a blade.
    #[test]
    fn stem_hairs_stay_on_young_wood() {
        assert_eq!(youth(0.5), 1.0);
        assert_eq!(youth(YOUNG_YEARS), 0.0);
        assert!(youth(1.5) > 0.0 && youth(1.5) < 1.0);
        let felt = Hairs {
            density: 0.85,
            colour: [0.8, 0.8, 0.76],
            ..Hairs::default()
        };
        let bark = [0.2, 0.15, 0.1];
        assert_eq!(cast_wood(bark, 1.0, None), bark);
        assert_eq!(cast_wood(bark, 0.0, Some(&felt)), bark);
        let young = cast_wood(bark, 1.0, Some(&felt));
        assert!(young[0] > bark[0] + 0.1, "{young:?}");
        // A tube keeps the hairs its age leaves, and none past it.
        let tube = |youth| {
            Tube::new(
                Vec3::ZERO,
                Vec3::new(0.0, 0.05, 0.0),
                0.001,
                bark,
                (0.0, None),
                0,
                &felt,
                youth,
            )
        };
        assert!(tube(0.0).is_none());
        assert!((tube(0.5).unwrap().hairs.density - 0.425).abs() < 1e-9);
    }

    /// Up close a hairy stalk's hairs stand out of it as strands; far
    /// away, or out of view, it draws none.
    #[test]
    fn a_hairy_stalk_draws_strands_up_close() {
        let tube = Tube::new(
            Vec3::ZERO,
            Vec3::new(0.0, 0.05, 0.0),
            0.001,
            [0.3, 0.2, 0.2],
            (0.0, None),
            0,
            &villous(),
            1.0,
        )
        .unwrap();
        let plant = PlantMesh {
            hair_tubes: vec![tube],
            ..PlantMesh::default()
        };
        let templates = Templates::for_looks(&[]);
        let near = strand_mesh(&plant, &templates, &|_| 0.0001).hairs;
        // About a hair for each of its spacings squared it covers: 37.
        assert!(near.indices.len() / 27 > 20, "{}", near.indices.len() / 27);
        assert!(
            strand_mesh(&plant, &templates, &|_| 0.01)
                .hairs
                .indices
                .is_empty()
        );
        assert!(
            strand_mesh(&plant, &templates, &|_| f64::INFINITY)
                .hairs
                .indices
                .is_empty()
        );
        // Every hair roots on the stalk's surface and reaches out of it.
        for p in &near.positions {
            let out = f64::from(p[0]).hypot(f64::from(p[2]));
            assert!(
                out >= 0.0009 && out <= 0.001 + villous().length * 1.5,
                "{p:?}"
            );
        }
    }
}
