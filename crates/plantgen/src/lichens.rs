//! Lichens: fungi that house green algae or cyanobacteria and grow among
//! the plants, on the ground, rock, wood and bark. The library holds them
//! beside the plants (Jay, 2026-11-08) as rank files alone, as it holds
//! the mosses ([`crate::mosses`]): a lichen genus, or a subgenus of one,
//! states its traits, and the rules here, which hold for every lichen,
//! turn them into a [`LichenKind`] and draw it.
//!
//! The first lichens are Cladonia's (Cladoniaceae): a primary thallus of
//! small scales on the ground bearing hollow upright stalks, the podetia.
//! In the cup lichens the podetia widen into cups; in the reindeer
//! lichens (subgenus Cladina) they branch richly into shrublets, packed
//! into mats a few centimetres deep. A podetium grows at its tips and
//! dies back at its base, like a moss's shoot (Munger 2008, USDA Fire
//! Effects Information System, Cladonia spp.), and the reindeer lichens
//! fork once a year, into three or four (Wikipedia, Podetium), so a
//! podetium's segments between forks are its years' growth.
//!
//! [`kinds`] reads the library's lichens; [`draw_mat`] draws a patch of
//! one's mat as a mesh: each podetium a tree of tubes, forking each year,
//! the branch that leads going on to the top and the others stopping
//! sooner, as the reindeer lichens' anisotomic forks do; the base, whose
//! growth is years old, darkened as it dies. Far off, a host tints its
//! ground with [`LichenKind::far_colour`]; how much of a stand's floor
//! lichens cover is [`crate::development::prairie_lichen`].

use serde_json::{Map, Value};

use crate::library::Library;
use crate::math::{PI, Vec3};
use crate::mesh::{Mesh, Vertex};
use crate::rng::{hash_words, unit};

/// A lichen the library describes, as its rules read it.
#[derive(Debug, Clone, PartialEq)]
pub struct LichenKind {
    /// Its name as a reader knows it: the genus, or the genus and its
    /// subgenus (`Cladonia subg. Cladina`).
    pub name: String,
    /// Its genus.
    pub genus: String,
    /// Its rank file.
    pub file: String,
    /// `thallus_form`: `dimorphic` for Cladonia.
    pub thallus_form: String,
    /// `podetium_height_m`, typical.
    pub height_m: f64,
    /// `podetium_width_m`, typical: the main branches' thickness.
    pub width_m: f64,
    /// `podetium_branching`: unbranched, sparse or rich.
    pub branching: String,
    /// `podetium_forks`, typical: branches at a fork.
    pub forks: u32,
    /// `podetium_fork_angle_deg`, typical, in radians.
    pub fork_angle: f64,
    /// `podetium_growth_m_per_year`, typical: a year's growth, and the
    /// length between forks.
    pub growth_m_per_year: f64,
    /// `podetium_cups`.
    pub cups: bool,
    /// `podetium_colour`, linear RGB.
    pub colour: [f32; 3],
    /// `light`: the classes it grows in.
    pub light: Vec<String>,
    /// `moisture`: the classes it grows in.
    pub moisture: Vec<String>,
}

/// The podetia of a reindeer lichen mat in a square metre: an estimated
/// 35,000 in a 5 by 10 m patch of Cladonia stellaris in northern Ontario
/// (Munger 2008, FEIS), 700 a square metre over the patch, its gaps
/// included. A drawn mat packs its podetia by their own heads instead
/// ([`MAT_OVERLAP`]); this is the check on it.
pub const PODETIA_PER_M2: f64 = 700.0;

/// How far a podetium's head reaches over its neighbours' in a mat: the
/// mat closes, its podetia interwoven (Wikipedia, Cladonia portentosa:
/// "compact interwoven mats"), each standing from the next a share of
/// its head's width. This model's choice.
pub const MAT_OVERLAP: f64 = 0.7;

/// A tip's share of its podetium's width: the reindeer lichens' branches
/// stay nearly as thick to their last forks, the tips short spinules
/// (photographs of C. portentosa, imagekit batch inat-2026-11-08). This
/// model's reading of them.
pub const TIP_WIDTH: f64 = 0.6;

/// How far a tube's normals lean up, out of the mat: a mat of fine
/// branches scatters light through itself, as foliage does, and a game
/// lights such a mass by normals bent toward the outside of the canopy
/// rather than each twig's own (the same trick as foliage normals); with
/// each branch's own normals alone, a mat of matte tubes draws far darker
/// than the pale lichen photographed. This model's choice.
pub const NORMAL_LIFT: f64 = 1.0;

/// How strongly a growing branch turns upward, a share of its length a
/// year: podetia stand upright in their mat. This model's choice.
pub const UP_PULL: f64 = 0.12;

/// The share of its colour a podetium keeps at its base, where its growth
/// of many years ago is dying back (Munger 2008: podetia die at the base
/// as they grow at the tips): this model's choice, as greyish brown as
/// the dead bases of mats look in photographs.
pub const DEAD_BASE: f32 = 0.45;

/// A side branch's share of the years left to the branch it forks from:
/// the reindeer lichens fork unequally (anisotomic, Wikipedia, Podetium),
/// the leading branch going on and the others stopping sooner. This
/// model's choice.
pub const SIDE_YEARS: f64 = 0.8;

fn usual(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Object(states) => {
            let weight = |word: &Value| match word.as_str() {
                Some("always") => 5,
                Some("usually") => 4,
                Some("often") => 3,
                Some("sometimes") => 2,
                Some("rarely") => 1,
                _ => 0,
            };
            let mut best: Option<(&String, i32)> = None;
            for (state, word) in states {
                let w = weight(word);
                if best.is_none_or(|(_, b)| w > b) {
                    best = Some((state, w));
                }
            }
            best.map(|(state, _)| state.clone())
        }
        _ => None,
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Object(range) => range.get("typical").and_then(Value::as_f64).or_else(|| {
            let low = range.get("min").and_then(Value::as_f64)?;
            let high = range.get("max").and_then(Value::as_f64)?;
            Some(f64::midpoint(low, high))
        }),
        _ => None,
    }
}

fn states(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Some(Value::String(text)) => vec![text.clone()],
        _ => Vec::new(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn colour(value: &Value) -> Option<[f32; 3]> {
    let items = value.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let mut out = [0.0_f32; 3];
    for (channel, item) in out.iter_mut().zip(items) {
        let byte = u8::try_from(item.as_u64()?.min(255)).ok()?;
        *channel = crate::texture::srgb_to_linear(byte) as f32;
    }
    Some(out)
}

impl LichenKind {
    /// A lichen from its traits, or `None` where they do not say enough
    /// to draw it (a genus whose groups differ, each stating its own
    /// podetia).
    ///
    /// # Errors
    ///
    /// On a trait the rules cannot read.
    pub fn from_traits(
        name: &str,
        genus: &str,
        file: &str,
        traits: &Map<String, Value>,
    ) -> Result<Option<Self>, String> {
        let thallus_form = traits
            .get("thallus_form")
            .and_then(usual)
            .ok_or_else(|| format!("{name}: a lichen states its `thallus_form`"))?;
        let Some(height_m) = traits.get("podetium_height_m").and_then(number) else {
            return Ok(None);
        };
        let Some(width_m) = traits.get("podetium_width_m").and_then(number) else {
            return Ok(None);
        };
        let colour = traits
            .get("podetium_colour")
            .and_then(colour)
            .ok_or_else(|| format!("{name}: its podetia state `podetium_colour`"))?;
        let branching = traits
            .get("podetium_branching")
            .and_then(usual)
            .unwrap_or_else(|| "unbranched".into());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let forks = traits
            .get("podetium_forks")
            .and_then(number)
            .map_or(1, |forks| forks.round().max(1.0) as u32);
        let fork_angle = traits
            .get("podetium_fork_angle_deg")
            .and_then(number)
            .unwrap_or(0.0)
            .to_radians();
        let growth_m_per_year = traits
            .get("podetium_growth_m_per_year")
            .and_then(number)
            .ok_or_else(|| format!("{name}: its podetia state `podetium_growth_m_per_year`"))?;
        let cups = traits
            .get("podetium_cups")
            .and_then(usual)
            .is_some_and(|state| state == "true");
        Ok(Some(Self {
            name: name.to_string(),
            genus: genus.to_string(),
            file: file.to_string(),
            thallus_form,
            height_m,
            width_m,
            branching,
            forks,
            fork_angle,
            growth_m_per_year,
            cups,
            colour,
            light: states(traits.get("light")),
            moisture: states(traits.get("moisture")),
        }))
    }

    /// The years a podetium takes to grow to its height at its yearly
    /// growth: the number of its forks from base to top.
    #[must_use]
    pub fn years(&self) -> f64 {
        (self.height_m / self.growth_m_per_year).max(1.0)
    }

    /// Its colour from afar, over a mat: the live tips' colour over the
    /// shade between the branches and the darker dying bases seen through
    /// them. A mat's top shows mostly tips; this model takes the colour
    /// a photograph of a mat gives (the podetia's `podetium_colour` is
    /// measured so), unchanged.
    #[must_use]
    pub fn far_colour(&self) -> [f32; 3] {
        self.colour
    }
}

/// Every lichen in `library` that states enough to be drawn: each genus
/// or subgenus whose `body_plan` is `lichen` and whose traits give its
/// podetia's height and width. A genus whose groups differ (Cladonia, its
/// cup lichens and its reindeer lichens) is drawn by its subgenera.
///
/// # Errors
///
/// On a lichen whose traits the rules cannot read.
pub fn kinds(library: &Library) -> Result<Vec<LichenKind>, String> {
    let mut found = Vec::new();
    for rank in library.ranks() {
        if rank.rank != "genus" && rank.rank != "subgenus" {
            continue;
        }
        let traits = library.rank_traits(&rank.file)?;
        if traits.get("body_plan").and_then(Value::as_str) != Some("lichen") {
            continue;
        }
        let genus = if rank.rank == "genus" {
            rank.name.clone()
        } else {
            rank.parent
                .as_deref()
                .and_then(|parent| library.rank(parent))
                .map_or_else(String::new, |genus| genus.name.clone())
        };
        let name = if rank.rank == "genus" {
            rank.name.clone()
        } else {
            format!("{genus} subg. {}", rank.name)
        };
        if let Some(kind) = LichenKind::from_traits(&name, &genus, &rank.file, &traits)? {
            found.push(kind);
        }
    }
    Ok(found)
}

/// A unit vector across `axis`.
fn across(axis: Vec3) -> Vec3 {
    let pick = if axis.y.abs() < 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    axis.cross(pick).normalize_or(Vec3::new(1.0, 0.0, 0.0))
}

/// `v` turned by `angle` about the unit axis `axis` (Rodrigues).
fn turned(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = (libm::sin(angle), libm::cos(angle));
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// One tube from `from` to `to`, radii `r0` and `r1`, colours `c0` and
/// `c1`, as `sides` faces, its tip closed when `cap`.
#[allow(clippy::too_many_arguments)]
fn tube(
    mesh: &mut Mesh,
    from: Vec3,
    to: Vec3,
    (r0, r1): (f64, f64),
    (c0, c1): ([f32; 3], [f32; 3]),
    sides: u32,
    cap: bool,
) {
    let axis = (to - from).normalize_or(Vec3::new(0.0, 1.0, 0.0));
    let first_side = across(axis);
    let second_side = axis.cross(first_side);
    let mut ring = |centre: Vec3, radius: f64, colour: [f32; 3]| -> u32 {
        let mut first = 0;
        for corner in 0..sides {
            let angle = 2.0 * PI * f64::from(corner) / f64::from(sides);
            let out = first_side * libm::cos(angle) + second_side * libm::sin(angle);
            let normal = (out + Vec3::new(0.0, NORMAL_LIFT, 0.0)).normalize_or(out);
            let index = mesh.push(Vertex::plain(centre + out * radius, normal, colour));
            if corner == 0 {
                first = index;
            }
        }
        first
    };
    let base = ring(from, r0, c0);
    let top = ring(to, r1, c1);
    for corner in 0..sides {
        let next = (corner + 1) % sides;
        mesh.indices.extend([
            base + corner,
            top + corner,
            base + next,
            base + next,
            top + corner,
            top + next,
        ]);
    }
    if cap {
        let tip = mesh.push(Vertex::plain(to + axis * r1, axis, c1));
        for corner in 0..sides {
            let next = (corner + 1) % sides;
            mesh.indices.extend([top + corner, tip, top + next]);
        }
    }
}

/// How a mat is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatOptions {
    /// The side of the square patch drawn, metres, its centre at the
    /// origin on the ground (y up).
    pub side_m: f64,
    /// Faces round each tube.
    pub sides: u32,
    /// The patch's seed.
    pub seed: u64,
}

impl Default for MatOptions {
    fn default() -> Self {
        Self {
            side_m: 0.2,
            sides: 4,
            seed: 1,
        }
    }
}

struct Grower<'a> {
    kind: &'a LichenKind,
    years: f64,
    mesh: Mesh,
    sides: u32,
}

impl Grower<'_> {
    /// The colour of a podetium's part grown `age` years before its tips:
    /// the live colour near the tips, darkening into the dying base.
    #[allow(clippy::cast_possible_truncation)]
    fn colour_at(&self, age: f64) -> [f32; 3] {
        let dying = (age / self.years).clamp(0.0, 1.0) as f32;
        let k = 1.0 - (1.0 - DEAD_BASE) * dying * dying;
        self.kind.colour.map(|c| c * k)
    }

    /// A branch from `at` along `dir`, `left` years of growth to come.
    fn branch(&mut self, at: Vec3, dir: Vec3, left: f64, salt: u64) {
        let kind = self.kind;
        if left <= 0.0 {
            return;
        }
        let step = kind.growth_m_per_year.min(left * kind.growth_m_per_year);
        let years_here = step / kind.growth_m_per_year;
        // The radius thins toward the tips: full at the base of the
        // oldest growth, [`TIP_WIDTH`] of it at a tip.
        let radius = |left: f64| {
            0.5 * kind.width_m * (TIP_WIDTH + (1.0 - TIP_WIDTH) * (left / self.years).min(1.0))
        };
        // A little wander, so no branch is ruled straight.
        let wobble = across(dir) * (0.25 * (unit(hash_words(&[salt, 1])) - 0.5))
            + dir.cross(across(dir)) * (0.25 * (unit(hash_words(&[salt, 2])) - 0.5));
        let up_pull = Vec3::new(0.0, UP_PULL, 0.0);
        let dir = (dir + wobble + up_pull).normalize_or(dir);
        let to = at + dir * step;
        let next = left - years_here;
        let tip = next <= 0.0 || kind.forks <= 1;
        let colours = (self.colour_at(left), self.colour_at(next.max(0.0)));
        tube(
            &mut self.mesh,
            at,
            to,
            (radius(left), radius(next.max(0.0))),
            colours,
            self.sides,
            tip && next <= 0.0,
        );
        if next <= 0.0 {
            return;
        }
        if kind.forks <= 1 {
            self.branch(to, dir, next, hash_words(&[salt, 3]));
            return;
        }
        // A fork: the leading branch goes on; the others lean out at the
        // fork angle around it, each with a share of the years left.
        let turn = 2.0 * PI * unit(hash_words(&[salt, 4]));
        let side = turned(across(dir), dir, turn);
        let lead = turned(dir, side, 0.4 * kind.fork_angle);
        self.branch(to, lead, next, hash_words(&[salt, 5]));
        for k in 1..kind.forks {
            let around = turn + 2.0 * PI * f64::from(k) / f64::from(kind.forks);
            let out = turned(across(dir), dir, around);
            let bent = turned(dir, out, -kind.fork_angle);
            self.branch(
                to,
                bent,
                next * SIDE_YEARS,
                hash_words(&[salt, 6, u64::from(k)]),
            );
        }
    }
}

/// The radius of a podetium's head: the mean, over a few podetia, of the
/// farthest any of its branches reaches from its base, across.
#[must_use]
pub fn head_radius(kind: &LichenKind) -> f64 {
    let years = kind.years();
    let mut sum = 0.0;
    for seed in 0..8_u64 {
        let mut grower = Grower {
            kind,
            years,
            mesh: Mesh::default(),
            sides: 3,
        };
        grower.branch(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            years,
            seed,
        );
        sum += grower
            .mesh
            .positions
            .iter()
            .map(|p| f64::from(p[0]).hypot(f64::from(p[2])))
            .fold(0.0_f64, f64::max);
    }
    sum / 8.0
}

/// How far apart a mat's podetia stand: each head's width less
/// [`MAT_OVERLAP`] of it.
#[must_use]
pub fn podetium_spacing(kind: &LichenKind) -> f64 {
    (2.0 * head_radius(kind) * (1.0 - MAT_OVERLAP)).max(kind.width_m)
}

/// A square patch of `kind`'s mat (`options.side_m` on a side, centred on
/// the origin, on the ground y = 0): podetia [`podetium_spacing`] apart, each
/// grown for [`LichenKind::years`] from a point jittered within its cell,
/// leaning a little at random, the mat's height set by its kind.
#[must_use]
pub fn draw_mat(kind: &LichenKind, options: &MatOptions) -> Mesh {
    let spacing = podetium_spacing(kind);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (options.side_m / spacing).ceil().max(1.0) as u64;
    let years = kind.years();
    let mut grower = Grower {
        kind,
        years,
        mesh: Mesh::default(),
        sides: options.sides.max(3),
    };
    let half = 0.5 * options.side_m;
    for i in 0..count {
        for j in 0..count {
            let salt = hash_words(&[options.seed, i, j]);
            #[allow(clippy::cast_precision_loss)]
            let (x, z) = (
                -half + (i as f64 + unit(hash_words(&[salt, 10]))) * spacing,
                -half + (j as f64 + unit(hash_words(&[salt, 11]))) * spacing,
            );
            if x.abs() > half || z.abs() > half {
                continue;
            }
            // Each podetium a little shorter or taller than the kind's
            // typical: up to a fifth either way. This model's choice.
            let own = years * (0.8 + 0.4 * unit(hash_words(&[salt, 12])));
            let lean = 0.35 * unit(hash_words(&[salt, 13]));
            let azimuth = 2.0 * PI * unit(hash_words(&[salt, 14]));
            let dir = Vec3::new(
                libm::sin(lean) * libm::cos(azimuth),
                libm::cos(lean),
                libm::sin(lean) * libm::sin(azimuth),
            );
            grower.branch(Vec3::new(x, 0.0, z), dir, own, salt);
        }
    }
    grower.mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cladina() -> LichenKind {
        kinds(Library::builtin())
            .expect("lichens")
            .into_iter()
            .find(|kind| kind.name == "Cladonia subg. Cladina")
            .expect("the reindeer lichens")
    }

    #[test]
    fn the_reindeer_lichens_come_from_their_subgenus_over_their_genus() {
        let kind = cladina();
        assert_eq!(kind.genus, "Cladonia");
        assert_eq!(kind.thallus_form, "dimorphic");
        assert_eq!(kind.branching, "rich");
        assert_eq!(kind.forks, 3);
        assert!(!kind.cups);
        assert!((kind.height_m - 0.06).abs() < 1e-12);
        // About a fork a year, a dozen years to its height.
        assert!((kind.years() - 12.0).abs() < 1e-9);
        // The genus alone, whose cup lichens and reindeer lichens differ,
        // states no podetia to draw.
        assert!(
            kinds(Library::builtin())
                .unwrap()
                .iter()
                .all(|kind| kind.name != "Cladonia")
        );
    }

    #[test]
    fn a_mat_stands_as_tall_as_its_podetia() {
        let kind = cladina();
        let mesh = draw_mat(
            &kind,
            &MatOptions {
                side_m: 0.08,
                ..MatOptions::default()
            },
        );
        assert!(mesh.triangle_count() > 1000);
        let top = mesh
            .positions
            .iter()
            .map(|p| f64::from(p[1]))
            .fold(0.0_f64, f64::max);
        assert!(
            top > 0.6 * kind.height_m && top < 1.5 * kind.height_m,
            "top {top}"
        );
        assert!(mesh.positions.iter().all(|p| p[1] > -0.01));
    }

    #[test]
    fn a_closed_mat_holds_about_as_many_podetia_as_a_measured_one() {
        // Packed by their heads, the reindeer lichens stand within a
        // factor of a few of the 700 a square metre counted over a
        // C. stellaris patch, gaps and all (FEIS).
        let spacing = podetium_spacing(&cladina());
        let per_m2 = 1.0 / (spacing * spacing);
        assert!(
            per_m2 > 0.5 * PODETIA_PER_M2 && per_m2 < 6.0 * PODETIA_PER_M2,
            "{per_m2}"
        );
    }
}
