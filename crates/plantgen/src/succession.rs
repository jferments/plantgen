//! Plants that come up by themselves on a paved site, year by year
//! (pavement-cracks plan K3).
//!
//! A host hands PlantGen its pavement as crack tiles ([`crate::pavement`])
//! and the years since it was last kept. This module decides which plants
//! of a pool of species come up where, and which of them are still there
//! in a given year; [`crate::site`] then grows each one in full on the
//! site, where its crack holds it back (soil depth design D1).
//!
//! It is a cheap step for the whole site, with no full growth:
//!
//! 1. **Microsites.** Each patch of the tiles' openings [`PATCH_M`] wide is
//!    one place a plant can stand: its deepest soil, the rooting space a
//!    root reaches from there ([`SiteMesh::rooting`]) and how heavily it
//!    is trodden or driven over ([`Pavement::wear`]).
//! 2. **Arrival.** Each year seeds of every species of the pool land on
//!    the site from outside it, more of those the wind carries; plants on
//!    the site shed seed around them, as far as their seed travels. Seed
//!    that lands stays in the soil for as long as its seed bank lasts.
//! 3. **Establishment.** A share of the seed in an empty microsite
//!    germinates, and a seedling takes hold as often as its soil carries a
//!    year-old plant of its kind ([`SoilNeed::ratio`]), it escapes the
//!    traffic and, under a taller plant's crown, it bears the shade.
//! 4. **Growth.** Each plant grows as its kind does in open ground, held
//!    back by its soil as `soil@1` holds a growing plant: its size is the
//!    open-ground size at its age less the years its soil has cost it, its
//!    lengths times the square root of its supply ratio.
//! 5. **Death.** Annuals die after they seed and biennials in their second
//!    year; others die at a base rate, more where their soil runs short,
//!    where they are trodden on, under a taller crown and wherever the
//!    site is still kept ([`Settings::maintenance`]).
//!
//! The stages of a pavement's colonisation come out of this, not out of a
//! table: annual grass in hairline cracks, perennial herbs where more soil
//! lies, shrubs and young trees in potholes and wide cracks, and bare wheel
//! paths in a lane still driven.
//!
//! Every rate below is a model choice, made by eye to give a plausible
//! site and named so it can be measured and replaced; the species' own
//! facts (life span, dispersal, seed bank, tolerance of trampling and
//! shade, size by age, what its roots ask of the soil) come from the
//! library and its growth.

use crate::conditions::Rooting;
use crate::lsys::tools::SoilNeed;
use crate::math::Vec3;
use crate::pavement::Pavement;
use crate::rng::{hash_words, unit};
use crate::site::SiteMesh;

/// Edge of the square patch of an opening that holds one plant, metres.
pub const PATCH_M: f64 = 0.1;

/// How far over sound pavement seed is blown or washed into the nearest
/// opening, metres: a crack catches the seed that lands on the slab
/// around it, not only what lands in it.
pub const TRAP_M: f64 = 0.5;

/// Seeds of each species of the pool that land on each square metre of the
/// site a year from far off, for a species the wind carries
/// ([`Dispersal::weight`] scales the others). Plants near the site that
/// the host names ([`Source`]) add their own.
pub const SEED_RAIN: f64 = 10.0;

/// Share of the seed in a microsite's soil that germinates each spring.
pub const GERMINATION: f64 = 0.1;

/// How far below the pavement's surface the soil in an opening can lie
/// and still raise a seedling: one in a slit whose grit lies deep down
/// must grow up through the dark on its seed's reserves, and the chance it
/// reaches the light falls as exp(-depth / this). A few centimetres, as
/// small seeds emerge from in soil; an estimate.
pub const EMERGENCE_M: f64 = 0.03;

/// Share of seedlings that live through their first year where the soil
/// fully carries them: most die of slugs, frost and dry spells wherever
/// they come up.
pub const SEEDLING_SURVIVAL: f64 = 0.2;

/// Seeds a herb sheds a year for each square metre its crown covers, once
/// it is old enough to seed: a dandelion or a plantain a hand across
/// makes a few thousand.
pub const SEEDS_PER_M2: f64 = 50_000.0;
/// The same for a woody plant, whose crown is deep and spends most of
/// itself on wood.
pub const WOODY_SEEDS_PER_M2: f64 = 2_000.0;

/// Chance each year that a perennial herb dies of age or chance.
pub const HERB_DEATH: f64 = 0.15;
/// The same for a woody plant.
pub const WOODY_DEATH: f64 = 0.05;
/// Chance a year that a plant whose soil carries none of its demand dies
/// of it; it falls as the square of the shortfall.
pub const DROUGHT_DEATH: f64 = 0.3;
/// Chance a year that a plant under a taller plant's crown dies of the
/// shade, if it cannot bear any.
pub const SHADE_DEATH: f64 = 0.5;

/// How a seed travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispersal {
    Wind,
    Water,
    Animal,
    Unassisted,
}

impl Dispersal {
    fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "wind" => Self::Wind,
            "water" => Self::Water,
            "animal" => Self::Animal,
            "unassisted" => Self::Unassisted,
            _ => return None,
        })
    }

    /// How much of [`SEED_RAIN`] reaches the site from outside it: seed the
    /// wind carries comes from far around, seed that only falls comes from
    /// plants at the site's edge.
    #[must_use]
    pub const fn weight(self) -> f64 {
        match self {
            Self::Wind => 1.0,
            Self::Animal => 0.5,
            Self::Water => 0.3,
            Self::Unassisted => 0.1,
        }
    }

    /// How far around a plant on the site its seed lands, metres: run-off
    /// carries seed along the pavement into cracks and gutters.
    #[must_use]
    pub const fn reach(self) -> f64 {
        match self {
            Self::Wind => 20.0,
            Self::Animal => 5.0,
            Self::Water => 2.0,
            Self::Unassisted => 0.5,
        }
    }
}

/// How long seed lives in the soil, in Thompson, Bakker and Bekker's
/// classes (the `seedbank` trait).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedBank {
    Transient,
    ShortTerm,
    LongTerm,
}

impl SeedBank {
    /// Share of the seed in the soil still alive a year on: none of a
    /// transient bank, half of a short-term one (one to five years) and
    /// most of a long-term one (five years or more).
    #[must_use]
    pub const fn kept(self) -> f64 {
        match self {
            Self::Transient => 0.0,
            Self::ShortTerm => 0.5,
            Self::LongTerm => 0.85,
        }
    }
}

/// A tolerance class, of trampling or of shade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tolerance {
    Intolerant,
    Intermediate,
    Tolerant,
}

impl Tolerance {
    fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "intolerant" => Self::Intolerant,
            "intermediate" => Self::Intermediate,
            "tolerant" => Self::Tolerant,
            _ => return None,
        })
    }

    /// Chance a plant of this tolerance of trampling dies in a year where
    /// it is trodden on at load 1.
    #[must_use]
    pub const fn trampling_death(self) -> f64 {
        match self {
            Self::Tolerant => 0.15,
            Self::Intermediate => 0.6,
            Self::Intolerant => 0.95,
        }
    }

    /// Share of what it does in full light that a plant of this tolerance
    /// of shade manages under a taller crown.
    #[must_use]
    pub const fn shade_light(self) -> f64 {
        match self {
            Self::Tolerant => 0.8,
            Self::Intermediate => 0.4,
            Self::Intolerant => 0.1,
        }
    }
}

/// How many years a plant lives and sets seed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    Annual,
    Biennial,
    Perennial,
}

/// A plant's size at an age in open ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub age: f64,
    pub height: f64,
    /// The crown's radius: how far it reaches sideways from its foot.
    pub crown: f64,
}

/// A species of the pool, with what this step reads of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Colonist {
    pub id: String,
    pub life: Life,
    pub woody: bool,
    pub dispersal: Vec<Dispersal>,
    pub seedbank: SeedBank,
    pub trampling: Tolerance,
    pub shade: Tolerance,
    /// What its roots ask of the soil; `None` for a program without
    /// `soil@1`, which no soil holds back.
    pub soil: Option<SoilNeed>,
    /// Its size by age in open ground, ages rising.
    pub sizes: Vec<Size>,
}

/// The facts a species leaves out, and what this step takes for them:
/// seed with no way to travel, a seed bank that does not last, a herb that
/// bears some trampling and a woody plant that bears none, and some shade.
impl Colonist {
    /// A colonist from the traits a species states (as
    /// [`crate::library::Library::traits_of`] gives them) and whether it is
    /// woody, its soil need and its sizes.
    #[must_use]
    pub fn new(
        id: &str,
        traits: &serde_json::Map<String, serde_json::Value>,
        woody: bool,
        soil: Option<SoilNeed>,
        sizes: Vec<Size>,
    ) -> Self {
        let words = |key: &str| -> Vec<String> { trait_words(traits.get(key)) };
        let first = |key: &str| words(key).into_iter().next();
        let life = match first("life_span").as_deref() {
            Some("annual") => Life::Annual,
            Some("biennial") => Life::Biennial,
            _ => Life::Perennial,
        };
        let mut dispersal: Vec<Dispersal> = words("seed_dispersal")
            .iter()
            .filter_map(|word| Dispersal::from_word(word))
            .collect();
        if dispersal.is_empty() {
            dispersal.push(Dispersal::Unassisted);
        }
        let seedbank = match first("seedbank").as_deref() {
            Some("long_term") => SeedBank::LongTerm,
            Some("short_term") => SeedBank::ShortTerm,
            _ => SeedBank::Transient,
        };
        let trampling = first("trampling_tolerance")
            .and_then(|word| Tolerance::from_word(&word))
            .unwrap_or(if woody {
                Tolerance::Intolerant
            } else {
                Tolerance::Intermediate
            });
        let shade = first("shade_tolerance")
            .and_then(|word| Tolerance::from_word(&word))
            .unwrap_or(Tolerance::Intermediate);
        // A plant with no crown yet (a shrub's first year, all below
        // ground) is grown from nothing to its first size with one.
        let sizes = sizes
            .into_iter()
            .filter(|size| size.height > 0.0 && size.crown > 0.0)
            .collect();
        Self {
            id: id.to_owned(),
            life,
            woody,
            dispersal,
            seedbank,
            trampling,
            shade,
            soil,
            sizes,
        }
    }

    /// Its size in open ground at `age`, read off its sizes linearly, and
    /// from nothing before the first.
    #[must_use]
    pub fn size_at(&self, age: f64) -> (f64, f64) {
        let Some(first) = self.sizes.first() else {
            return (0.0, 0.0);
        };
        if age <= first.age {
            let t = (age / first.age.max(1.0e-9)).clamp(0.0, 1.0);
            return (first.height * t, first.crown * t);
        }
        for pair in self.sizes.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if age <= b.age {
                let t = (age - a.age) / (b.age - a.age).max(1.0e-9);
                return (
                    a.height + (b.height - a.height) * t,
                    a.crown + (b.crown - a.crown) * t,
                );
            }
        }
        let last = self.sizes[self.sizes.len() - 1];
        (last.height, last.crown)
    }

    fn seeds_per_m2(&self) -> f64 {
        if self.woody {
            WOODY_SEEDS_PER_M2
        } else {
            SEEDS_PER_M2
        }
    }

    /// Effective years of growth before it first seeds: in its first year
    /// for an annual, its second for a biennial or a perennial herb, its
    /// third for a shrub and its sixth for a tree-sized woody plant.
    fn first_seed(&self) -> f64 {
        match (self.life, self.woody) {
            (Life::Annual, _) => 1.0,
            (_, false) => 2.0,
            (_, true) => {
                let tallest = self.sizes.last().map_or(0.0, |size| size.height);
                if tallest > 5.0 { 6.0 } else { 3.0 }
            }
        }
    }

    /// The share of supply to demand at a microsite for a plant `age`
    /// effective years old, its lengths held to the square root of the
    /// share as `soil@1` holds them: a few rounds toward the fixed point,
    /// starting from `guess`.
    #[must_use]
    pub fn supply(&self, site: &Microsite, age: f64, guess: f64) -> (f64, f64, f64) {
        let (height, crown) = self.size_at(age.max(0.0));
        let Some(need) = self.soil else {
            return (1.0, height, crown);
        };
        let mut rs = guess.clamp(0.0, 1.0);
        for _ in 0..4 {
            let scale = rs.sqrt();
            rs = need.ratio(crown * scale, height * scale, |reach, depth| {
                Some(site.rooting.volume(reach, depth))
            });
        }
        let scale = rs.sqrt();
        (rs, height * scale, crown * scale)
    }
}

fn trait_words(value: Option<&serde_json::Value>) -> Vec<String> {
    use serde_json::Value;
    const FREQUENCY: [&str; 5] = ["rarely", "sometimes", "often", "usually", "always"];
    match value {
        Some(Value::String(word)) => vec![word.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Some(Value::Object(map)) => {
            // A frequency table: the commonest first.
            let mut words: Vec<(usize, &String)> = map
                .iter()
                .map(|(word, how)| {
                    let rank = how
                        .as_str()
                        .and_then(|how| FREQUENCY.iter().position(|f| *f == how))
                        .unwrap_or(0);
                    (rank, word)
                })
                .collect();
            words.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            words.into_iter().map(|(_, word)| word.clone()).collect()
        }
        _ => Vec::new(),
    }
}

/// A place a plant can stand.
#[derive(Debug, Clone, PartialEq)]
pub struct Microsite {
    /// The pavement it is in, an index into the site's pavements.
    pub tile: usize,
    pub x: f64,
    pub z: f64,
    /// Height of the soil's surface.
    pub y: f64,
    /// Depth of soil in the opening, metres.
    pub soil_depth: f64,
    /// Width of the opening, metres.
    pub width: f64,
    /// How far its soil lies below the pavement's surface, metres.
    pub sunk: f64,
    /// How heavily a low plant there is trodden or driven over, 0 to 1.
    pub wear: f64,
    /// The same for a plant taller than a vehicle's clearance.
    pub wear_tall: f64,
    /// The ground whose seed ends up here, m²: its own patch and the
    /// sound pavement within [`TRAP_M`] nearer it than any other opening.
    pub catch_m2: f64,
    pub rooting: Rooting,
}

/// The microsites of every pavement on `site`, one for each patch of its
/// openings [`PATCH_M`] wide, in the tiles' order. Their rooting tables
/// are made on `threads` threads.
#[must_use]
pub fn microsites(site: &SiteMesh, threads: usize) -> Vec<Microsite> {
    let mut found: Vec<(usize, &Pavement, crate::pavement::Opening)> = Vec::new();
    for (index, tile) in site.pavements().iter().enumerate() {
        for opening in tile.openings(PATCH_M) {
            found.push((index, tile, opening));
        }
    }
    let threads = threads.max(1);
    let chunk = found.len().div_ceil(threads).max(1);
    let mut sites: Vec<Option<Microsite>> = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = found
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|(index, tile, opening)| {
                            let foot = Vec3::new(opening.x, opening.soil_top, opening.z);
                            site.rooting(foot).map(|rooting| Microsite {
                                tile: *index,
                                x: opening.x,
                                z: opening.z,
                                y: opening.soil_top,
                                soil_depth: opening.soil_depth,
                                width: opening.width,
                                sunk: (tile.top() - opening.soil_top).max(0.0),
                                wear: tile.wear(opening.x, opening.z, 0.0),
                                wear_tall: tile.wear(opening.x, opening.z, f64::INFINITY),
                                catch_m2: 0.0,
                                rooting,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            sites.extend(handle.join().unwrap_or_default());
        }
    });
    let mut sites: Vec<Microsite> = sites.into_iter().flatten().collect();
    catchments(site, &mut sites);
    sites
}

/// Each microsite's catchment: the tiles sampled every [`CATCH_STEP_M`],
/// each sample going to the nearest microsite within [`TRAP_M`].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
fn catchments(site: &SiteMesh, sites: &mut [Microsite]) {
    use std::collections::HashMap;
    let key = |x: f64, z: f64| ((x / TRAP_M).floor() as i64, (z / TRAP_M).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (index, place) in sites.iter().enumerate() {
        grid.entry(key(place.x, place.z)).or_default().push(index);
    }
    let area = CATCH_STEP_M * CATCH_STEP_M;
    for tile in site.pavements() {
        let (low, high) = tile.extent();
        let nx = ((high[0] - low[0]) / CATCH_STEP_M).round() as i64;
        let nz = ((high[1] - low[1]) / CATCH_STEP_M).round() as i64;
        for k in 0..nz {
            for i in 0..nx {
                let x = low[0] + (i as f64 + 0.5) * CATCH_STEP_M;
                let z = low[1] + (k as f64 + 0.5) * CATCH_STEP_M;
                let (gx, gz) = key(x, z);
                let mut best: Option<(f64, usize)> = None;
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        for &index in grid.get(&(gx + dx, gz + dz)).into_iter().flatten() {
                            let gap = (sites[index].x - x).hypot(sites[index].z - z);
                            if gap <= TRAP_M && best.is_none_or(|(g, _)| gap < g) {
                                best = Some((gap, index));
                            }
                        }
                    }
                }
                if let Some((_, index)) = best {
                    sites[index].catch_m2 += area;
                }
            }
        }
    }
    for place in sites.iter_mut() {
        place.catch_m2 = place.catch_m2.max(PATCH_M * PATCH_M);
    }
}

/// Spacing of the samples a catchment is counted on, metres.
const CATCH_STEP_M: f64 = 0.05;

/// How the site has been used since it was last kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub seed: u64,
    /// Scales every traffic path's load, 0 to 1: 0 for a site no longer
    /// driven or walked on.
    pub traffic: f64,
    /// Chance each year that a plant is weeded out or sprayed, 0 to 1: 0
    /// for a site no one keeps.
    pub maintenance: f64,
}

/// A plant near the site that sheds seed onto it each year: a verge's
/// weeds, a tree across the road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    /// Its species, an index into the pool.
    pub colonist: usize,
    pub x: f64,
    pub z: f64,
    /// Its age, years.
    pub age: f64,
}

impl Source {
    /// The seed it sheds a year: as a plant of its kind that size on the
    /// site does, once it is old enough.
    fn seeds(&self, pool: &[Colonist]) -> f64 {
        let colonist = &pool[self.colonist];
        if self.age + 1.0e-9 < colonist.first_seed() {
            return 0.0;
        }
        let (_, crown) = colonist.size_at(self.age);
        colonist.seeds_per_m2() * std::f64::consts::PI * crown * crown
    }
}

/// A plant standing on the site.
#[derive(Debug, Clone, PartialEq)]
pub struct Established {
    /// Its species, an index into the pool.
    pub colonist: usize,
    /// Its microsite, an index into the microsites.
    pub microsite: usize,
    /// The year it came up.
    pub born: u32,
    /// Years since it came up.
    pub age: f64,
    /// Years of growth its soil has cost it.
    pub lag: f64,
    /// Its supply ratio last year.
    pub rs: f64,
    pub height: f64,
    pub crown: f64,
}

/// The plants standing on the site in one year.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub year: u32,
    pub plants: Vec<Established>,
}

const SALT_GERMINATE: u64 = 0x6765_726d;
const SALT_ORDER: u64 = 0x6f72_6465;
const SALT_DEATH: u64 = 0x6465_6174;

/// Runs the site from bare pavement for the largest of `years`, with seed
/// from far off and from `sources`, and keeps the plants standing at the
/// end of each of them (0 is the bare site).
/// The same microsites, pool and settings give the same plants.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]
pub fn run(
    microsites: &[Microsite],
    pool: &[Colonist],
    sources: &[Source],
    settings: &Settings,
    years: &[u32],
) -> Vec<Snapshot> {
    let last = years.iter().copied().max().unwrap_or(0);
    let species = pool.len();
    // Seed alive in each microsite's soil, by species.
    let mut bank = vec![0.0_f64; microsites.len() * species];
    let mut plants: Vec<Established> = Vec::new();
    let mut taken = vec![false; microsites.len()];
    // Seed each plant shed last year: (colonist, x, z, seeds).
    let mut shed: Vec<(usize, f64, f64, f64)> = Vec::new();
    let from_sources: Vec<(usize, f64, f64, f64)> = sources
        .iter()
        .map(|source| (source.colonist, source.x, source.z, source.seeds(pool)))
        .filter(|shed| shed.3 > 0.0)
        .collect();
    let mut snapshots = Vec::new();
    if years.contains(&0) {
        snapshots.push(Snapshot {
            year: 0,
            plants: Vec::new(),
        });
    }
    let roll = |words: &[u64]| unit(hash_words(words));
    for year in 1..=last {
        let y = u64::from(year);
        // Arrival: from outside, then from the site's own plants.
        for (s, colonist) in pool.iter().enumerate() {
            let kept = colonist.seedbank.kept();
            let rain: f64 = colonist
                .dispersal
                .iter()
                .map(|d| d.weight())
                .fold(0.0, f64::max)
                * SEED_RAIN;
            for (m, site) in microsites.iter().enumerate() {
                let at = m * species + s;
                bank[at] = bank[at] * kept + rain * site.catch_m2;
            }
        }
        for &(s, x, z, seeds) in shed.iter().chain(&from_sources) {
            let colonist = &pool[s];
            let share = seeds / colonist.dispersal.len() as f64;
            for d in &colonist.dispersal {
                let reach = d.reach();
                let density = share / (std::f64::consts::PI * reach * reach);
                for (m, site) in microsites.iter().enumerate() {
                    if (site.x - x).hypot(site.z - z) <= reach {
                        bank[m * species + s] += density * site.catch_m2;
                    }
                }
            }
        }
        // Establishment, in empty microsites.
        for m in 0..microsites.len() {
            if taken[m] {
                continue;
            }
            let site = &microsites[m];
            let wear = site.wear * settings.traffic;
            let over = tallest_over(&plants, microsites, site, None);
            let first = (roll(&[settings.seed, y, m as u64, SALT_ORDER]) * species as f64) as usize;
            for k in 0..species {
                let s = (first + k) % species;
                let colonist = &pool[s];
                let at = m * species + s;
                let germinants = bank[at] * GERMINATION;
                if germinants <= 0.0 {
                    continue;
                }
                bank[at] -= germinants;
                let (rs, height, _) = colonist.supply(site, 1.0, 1.0);
                let light = if over > height {
                    colonist.shade.shade_light()
                } else {
                    1.0
                };
                let survive = SEEDLING_SURVIVAL
                    * (-site.sunk / EMERGENCE_M).exp()
                    * rs
                    * light
                    * (1.0 - wear * colonist.trampling.trampling_death());
                let chance = 1.0 - (-germinants * survive).exp();
                if roll(&[settings.seed, y, m as u64, s as u64, SALT_GERMINATE]) < chance {
                    taken[m] = true;
                    plants.push(Established {
                        colonist: s,
                        microsite: m,
                        born: year,
                        age: 0.0,
                        lag: 0.0,
                        rs: 1.0,
                        height: 0.0,
                        crown: 0.0,
                    });
                    break;
                }
            }
        }
        // Growth through the year.
        for plant in &mut plants {
            let colonist = &pool[plant.colonist];
            let site = &microsites[plant.microsite];
            plant.age += 1.0;
            let (rs, height, crown) = colonist.supply(site, plant.age - plant.lag, plant.rs);
            plant.lag += 1.0 - rs;
            plant.rs = rs;
            let (height, crown) = {
                // Its size at the years of growth it has had.
                let (_, h, c) = colonist.supply(site, plant.age - plant.lag, rs);
                if h > 0.0 { (h, c) } else { (height, crown) }
            };
            plant.height = height;
            plant.crown = crown;
        }
        // Seed.
        shed.clear();
        for plant in &plants {
            let colonist = &pool[plant.colonist];
            if plant.age - plant.lag + 1.0e-9 >= colonist.first_seed() {
                let area = std::f64::consts::PI * plant.crown * plant.crown;
                let site = &microsites[plant.microsite];
                shed.push((
                    plant.colonist,
                    site.x,
                    site.z,
                    colonist.seeds_per_m2() * area,
                ));
            }
        }
        if years.contains(&year) {
            snapshots.push(Snapshot {
                year,
                plants: plants.clone(),
            });
        }
        // Death.
        let before = plants.clone();
        let mut index = 0;
        plants.retain(|plant| {
            let me = index;
            index += 1;
            let colonist = &pool[plant.colonist];
            let site = &microsites[plant.microsite];
            let ends = match colonist.life {
                Life::Annual => true,
                Life::Biennial => plant.age >= 2.0,
                Life::Perennial => false,
            };
            if ends {
                return false;
            }
            let base = if colonist.woody {
                WOODY_DEATH
            } else {
                HERB_DEATH
            };
            let drought = DROUGHT_DEATH * (1.0 - plant.rs).powi(2);
            let wear = if plant.height > crate::pavement::VEHICLE_CLEARANCE_M {
                site.wear_tall
            } else {
                site.wear
            };
            let trodden = wear * settings.traffic * colonist.trampling.trampling_death();
            let over = tallest_over(&before, microsites, site, Some(me));
            let shaded = if over >= plant.height && over > 0.0 {
                SHADE_DEATH * (1.0 - colonist.shade.shade_light())
            } else {
                0.0
            };
            let live = (1.0 - base)
                * (1.0 - drought)
                * (1.0 - trodden)
                * (1.0 - shaded)
                * (1.0 - settings.maintenance);
            roll(&[settings.seed, y, plant.microsite as u64, SALT_DEATH]) < live
        });
        taken.iter_mut().for_each(|t| *t = false);
        for plant in &plants {
            taken[plant.microsite] = true;
        }
    }
    snapshots
}

/// The height of the tallest plant whose crown covers `site`, other than
/// one standing on it. For the plant `me` of `plants`, a plant as tall as
/// it counts only if it came first, so of two equal crowns over each
/// other's feet one is crowded out, not both.
fn tallest_over(
    plants: &[Established],
    microsites: &[Microsite],
    site: &Microsite,
    me: Option<usize>,
) -> f64 {
    let own = me.map_or(f64::INFINITY, |me| plants[me].height);
    plants
        .iter()
        .enumerate()
        .filter(|(index, plant)| {
            let at = &microsites[plant.microsite];
            let gap = (at.x - site.x).hypot(at.z - site.z);
            let first = me.is_none_or(|me| plant.height > own || *index < me);
            gap > 1.0e-9 && gap <= plant.crown && first
        })
        .map(|(_, plant)| plant.height)
        .fold(0.0, f64::max)
}
