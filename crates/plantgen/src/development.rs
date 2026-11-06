//! How a stand develops after a stand-replacing disturbance: which cohorts
//! of which species a stand of a given age holds, and how much of the
//! ground each layer covers (stands plan step 2).
//!
//! The stages are Franklin et al.'s (2002, Forest Ecology and Management
//! 155: 399-423) for the Douglas-fir sere of the Pacific Northwest, the
//! sere Oliver and Larson (1996) generalise: a cohort establishes after
//! the disturbance and its crowns close; competitive exclusion follows,
//! with no new trees and little understorey, until 80-100 years; in
//! maturation, from then for 100-150 years, shade-tolerant trees
//! establish under the pioneers; in vertical diversification, 200-350
//! years, they grow into the canopy beside them; in horizontal
//! diversification, from 300 years or more, the pioneers' deaths open
//! gaps; and the pioneer cohort is lost at 800-1300 years. Its ages are
//! kept here as shares of the pioneers' lifespan (Douglas-fir's 500 years,
//! FEIS), so a sere of shorter-lived pioneers runs through them sooner.
//!
//! Every rule reads a species' niche (its shade tolerance and the layers
//! it grows in) and its lifespan (`growth.lifespan`); none names a
//! species. Which cohorts stand is decided before anything is grown, and
//! each cohort's layer afterwards from its height ([`layer_of`]), so the
//! fill never grows a plant per stand.

use crate::niche::Layer;
use serde::{Deserialize, Serialize};

/// Shade tolerance (a niche's 0 to 1) from which a tree establishes under
/// a closed canopy: western hemlock 0.95, Pacific silver fir 0.8, grand
/// fir 0.7, bigleaf maple 0.6 do; Douglas-fir 0.3 and red alder 0.15,
/// which need gaps or open ground, do not.
pub const TOLERANT: f64 = 0.5;

/// Stage boundaries as shares of the pioneers' lifespan, from Franklin et
/// al. (2002) over Douglas-fir's 500 years. Crowns close by 35 years
/// (McIntosh et al. 2009, PNW-GTR-794: western Oregon's mean canopy cover
/// rises with stand age to 35 years, then holds).
pub const CLOSURE: f64 = 35.0 / 500.0;
/// Competitive exclusion lasts to 80-100 years; shade-tolerant trees
/// establish from then.
pub const REINITIATION: f64 = 100.0 / 500.0;
/// Vertical diversification from 200 years.
pub const VERTICAL: f64 = 200.0 / 500.0;
/// Horizontal diversification, the pioneers' deaths opening gaps, from
/// 300 years.
pub const GAPS: f64 = 300.0 / 500.0;
/// The pioneer cohort is gone between 800 and 1300 years; here linearly
/// from [`GAPS`] to the middle of that span.
pub const PIONEERS_GONE: f64 = 1050.0 / 500.0;

/// How long, as a share of the pioneers' lifespan, a cohort of
/// shade-tolerant trees takes to lose all but a share 1/e of its stems in
/// the understorey. The trees that establish under a canopy die young in
/// numbers, so a stand of them holds many small trees and few large: the
/// reverse-J of all-aged stands (de Liocourt 1898; Meyer 1952).
pub const INGROWTH_DECAY: f64 = 0.25;

/// The share of a released tree's years beneath the canopy that count
/// toward its size once a gap opens over it: it grows on as a tree of its
/// like in a stand of the age its size matches. The model's choice, from
/// its own growth: a hemlock grown 120 years beneath a 60 m canopy is
/// 19.7 m, as tall as one 40 years old in a stand, and one grown 60 years
/// there, 11.9 m, as one 25.
pub const RELEASE_CATCH_UP: f64 = 1.0 / 3.0;

/// The canopy cover of a closed stand, [`Stage::Exclusion`]'s (McIntosh et
/// al. 2009: rarely more than 85%).
pub const CLOSED: f64 = 0.85;

/// Herb cover where no tree shades the ground: a meadow, or a stand just
/// after its disturbance. Grassland herbs cover most of the ground, and
/// restored Willamette Valley wet prairies reached more than 100% total
/// cover (layers counted apart) within three to five years (Conservation
/// Evidence study 9161); the early-successional stage of a forest site is
/// herb-rich before its trees close (Swanson et al. 2011, Frontiers in
/// Ecology and the Environment 9: 117-125). The value itself is this
/// model's choice.
pub const OPEN_HERB: f64 = 0.9;

/// A stage of the sere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The new cohort before its crowns close.
    Establishment,
    /// Closed, single-layered, with little understorey.
    Exclusion,
    /// Shade-tolerant trees establish under the pioneers.
    Maturation,
    /// Tolerant trees reach the canopy beside the pioneers.
    VerticalDiversification,
    /// The pioneers' deaths open gaps.
    HorizontalDiversification,
    /// The pioneers are gone.
    PioneerLoss,
}

impl Stage {
    /// Franklin et al.'s name for it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Establishment => "cohort establishment",
            Self::Exclusion => "competitive exclusion",
            Self::Maturation => "maturation",
            Self::VerticalDiversification => "vertical diversification",
            Self::HorizontalDiversification => "horizontal diversification",
            Self::PioneerLoss => "pioneer cohort loss",
        }
    }

    /// The stage of a stand `age` years after the disturbance whose
    /// pioneers commonly live `lifespan` years.
    #[must_use]
    pub fn at(age: f64, lifespan: f64) -> Self {
        let share = age / lifespan.max(1.0);
        if share < CLOSURE {
            Self::Establishment
        } else if share < REINITIATION {
            Self::Exclusion
        } else if share < VERTICAL {
            Self::Maturation
        } else if share < GAPS {
            Self::VerticalDiversification
        } else if share < PIONEERS_GONE * 0.8 {
            Self::HorizontalDiversification
        } else {
            Self::PioneerLoss
        }
    }

    /// Ground covered by each layer's crowns, canopy, subcanopy, shrub
    /// and herb ([`Layer::ALL`]), seen from above and counting each crown
    /// whole. After closure the canopy holds 60-96% and rarely more than
    /// 85% (McIntosh et al. 2009); old growth about 70% (horizontal
    /// diversification's gaps); the middle tree layer grows with stand
    /// age and the lowest exceeds 20% only past 200 years (same); shrub
    /// cover shows no trend with stand age (same), and forbs are fewest
    /// under the closed young stand (same, at 25-35 years). The shrub and
    /// herb shares themselves are this model's choices, pending measured
    /// ones.
    #[must_use]
    pub fn cover(self, age: f64, lifespan: f64) -> [f64; 4] {
        match self {
            Self::Establishment => {
                let closing = (age / (CLOSURE * lifespan.max(1.0))).clamp(0.0, 1.0);
                [CLOSED * closing, 0.0, 0.35, 0.5]
            }
            Self::Exclusion => [CLOSED, 0.05, 0.3, 0.15],
            Self::Maturation => [0.8, 0.15, 0.3, 0.3],
            Self::VerticalDiversification => [0.75, 0.3, 0.3, 0.35],
            Self::HorizontalDiversification | Self::PioneerLoss => [0.7, 0.35, 0.3, 0.4],
        }
    }
}

/// A species the stand may hold.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Its niche's shade tolerance, 0 to 1.
    pub shade_tolerance: f64,
    /// Its niche's abundance in each layer, canopy, subcanopy, shrub and
    /// herb.
    pub layers: [f64; 4],
    /// The age its old plants commonly reach, years.
    pub lifespan: f64,
    /// The ages it can be drawn at, ascending (its keyframes and old
    /// ages).
    pub ages: Vec<f64>,
    /// How well the site suits it, 0 to 1 (its niche's
    /// [`crate::niche::Niche::site_response`]): it scales the species'
    /// share of the stand, never how full the stand is.
    pub suitability: f64,
}

impl Candidate {
    /// Whether it grows into a tree: it has a canopy or subcanopy layer.
    #[must_use]
    pub fn tree(&self) -> bool {
        self.layers[0] + self.layers[1] > 0.0
    }
}

/// One cohort: a candidate at one of its ages, and its weight among the
/// stand's members.
#[derive(Debug, Clone, PartialEq)]
pub struct Cohort {
    pub candidate: usize,
    pub age: f64,
    pub weight: f64,
    /// Of the cohort that established after the disturbance.
    pub pioneer: bool,
    /// Ingrowth that a gap in the canopy opened over: it grows as in a
    /// stand of its like, at the age its size matches
    /// ([`RELEASE_CATCH_UP`]), where the rest of its cohort grows beneath
    /// the canopy.
    pub released: bool,
}

/// A stand of one age: its stage, its cohorts and its layers' cover.
#[derive(Debug, Clone, PartialEq)]
pub struct Development {
    pub stage: Stage,
    /// The lifespan of its pioneers, years.
    pub lifespan: f64,
    pub cohorts: Vec<Cohort>,
    pub cover: [f64; 4],
}

/// The cohorts of a stand `age` years after a stand-replacing
/// disturbance, from `candidates`:
///
/// 1. Every tree establishes in the first cohort, in proportion to its
///    canopy abundance and its intolerance of shade (Douglas-fir
///    dominates the cohort that follows a fire; hemlock joins it a little),
///    drawn at the age nearest the stand's. Past [`GAPS`] of its lifespan
///    its share falls to nothing at [`PIONEERS_GONE`].
/// 2. From [`REINITIATION`] of the pioneers' lifespan each shade-tolerant
///    tree ([`TOLERANT`]) establishes under the canopy year by year, so it
///    stands at every age it can be drawn at up to the years since then,
///    fewer of the older ([`INGROWTH_DECAY`]). From [`GAPS`] the
///    pioneers' deaths open gaps, the canopy's share open in step with the
///    pioneers lost, and the share of a cohort that a gap opened over
///    since it came up is released ([`Cohort::released`]): it has stood
///    in the gap for half the years since then, on average, and grows on
///    as in a stand of its like at the age its size matches
///    ([`RELEASE_CATCH_UP`]), the nearest it can be drawn at.
/// 3. Shrubs and herbs stand at the oldest age they can be drawn at, up to
///    the stand's, in proportion to their abundance in their layer.
///
/// Each weight is scaled by the candidate's suitability to the site, so
/// the site picks which species stand and in what shares; how full each
/// layer is comes from the stage alone, unless no tree suits the site.
/// Light keeps out what cannot grow under the crowns above it when the
/// stand is laid out ([`crate::stand_tile`]).
#[must_use]
pub fn develop(candidates: &[Candidate], age: f64) -> Development {
    develop_as(candidates, age, Origin::Natural)
}

/// How a stand came to be, which sets who stands in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// It came up by itself after a disturbance ([`develop`]).
    #[default]
    Natural,
    /// It was planted or kept: a yard's, a street's or a park's trees.
    /// Each tree stands in proportion to its abundance in the canopy and
    /// subcanopy, whatever its tolerance of shade, from the first cohort;
    /// nothing comes up beneath it, since a kept yard is mown and weeded.
    /// Its trees still fade as they pass their lifespan.
    Planted,
}

/// [`develop`] for a stand of `origin`.
#[must_use]
pub fn develop_as(candidates: &[Candidate], age: f64, origin: Origin) -> Development {
    let planted = origin == Origin::Planted;
    // The pioneers' lifespan: the longest-lived intolerant tree's, else
    // any tree's.
    let longest = |pick: &dyn Fn(&Candidate) -> bool| {
        candidates
            .iter()
            .filter(|candidate| pick(candidate))
            .map(|candidate| candidate.lifespan)
            .fold(0.0_f64, f64::max)
    };
    let mut lifespan = longest(&|c| c.tree() && c.shade_tolerance < TOLERANT);
    if lifespan <= 0.0 {
        lifespan = longest(&Candidate::tree);
    }
    let lifespan = lifespan.max(1.0);
    let stage = Stage::at(age, lifespan);
    let mut cohorts = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.ages.is_empty() || candidate.suitability <= 0.0 {
            continue;
        }
        let suits = candidate.suitability;
        if candidate.tree() {
            // The first cohort.
            let own = candidate.lifespan.max(1.0);
            let survival = 1.0 - ((age / own - GAPS) / (PIONEERS_GONE - GAPS)).clamp(0.0, 1.0);
            let weight = if planted {
                suits * (candidate.layers[0] + candidate.layers[1]) * survival
            } else {
                suits * candidate.layers[0] * (1.0 - candidate.shade_tolerance) * survival
            };
            if weight > 0.0 {
                cohorts.push(Cohort {
                    candidate: index,
                    age: nearest(&candidate.ages, age),
                    weight,
                    pioneer: true,
                    released: false,
                });
            }
            // Ingrowth under the canopy.
            if !planted && candidate.shade_tolerance >= TOLERANT {
                ingrowth(&mut cohorts, index, candidate, age, lifespan);
            }
        } else {
            let abundance = candidate.layers[2] + candidate.layers[3];
            if abundance > 0.0 {
                let a = candidate
                    .ages
                    .iter()
                    .copied()
                    .filter(|a| *a <= age.max(candidate.ages[0]))
                    .fold(candidate.ages[0], f64::max);
                cohorts.push(Cohort {
                    candidate: index,
                    age: a,
                    weight: suits * abundance,
                    pioneer: false,
                    released: false,
                });
            }
        }
    }
    // Where some tree suits the site, its trees close their canopy
    // whatever the others' fit; only where none suits well does the stand
    // open toward woodland, fully at half the best suitability. The
    // threshold is this model's choice.
    let best = candidates
        .iter()
        .filter(|candidate| candidate.tree())
        .map(|candidate| candidate.suitability)
        .fold(0.0_f64, f64::max);
    let mut cover = stage.cover(age, lifespan);
    let woodland = smoothstep(0.0, 0.5, best);
    cover[0] *= woodland;
    cover[1] *= woodland;
    // Herbs take the ground the canopy leaves open, up to a meadow's
    // cover where no tree shades it, falling in proportion to the canopy's
    // share of a closed one. A stage's own herb cover is the least it
    // holds.
    let open = (1.0 - cover[0] / CLOSED).clamp(0.0, 1.0);
    cover[3] = cover[3].max(OPEN_HERB * open);
    Development {
        stage,
        lifespan,
        cohorts,
        cover,
    }
}

/// A shade-tolerant tree's cohorts under the canopy (rule 2 of
/// [`develop`]), pushed onto `cohorts`: one per age it can be drawn at up
/// to the years since [`REINITIATION`], and from [`GAPS`] the share of
/// each that a gap released, at the age its size matches.
fn ingrowth(
    cohorts: &mut Vec<Cohort>,
    index: usize,
    candidate: &Candidate,
    age: f64,
    lifespan: f64,
) {
    let suits = candidate.suitability;
    let since = age - REINITIATION * lifespan;
    if since > 0.0 {
        let abundance = candidate.layers[0] + candidate.layers[1];
        let ages: Vec<f64> = candidate
            .ages
            .iter()
            .copied()
            .filter(|a| *a > 0.0 && *a <= since)
            .collect();
        // Gaps open as the pioneers die, from GAPS of their lifespan, the
        // canopy's share open in step with their loss; a cohort's share
        // released is that of the gaps opened since it came up.
        let gaps_since = age - GAPS * lifespan;
        let opened = ((age / lifespan - GAPS) / (PIONEERS_GONE - GAPS)).clamp(0.0, 1.0);
        // Each age stands for the years from the one before.
        let mut before = 0.0;
        for a in ages {
            let span = a - before;
            before = a;
            let weight = suits
                * abundance
                * candidate.shade_tolerance
                * span
                * libm::exp(-a / (INGROWTH_DECAY * lifespan))
                / (INGROWTH_DECAY * lifespan);
            let share = if gaps_since > 0.0 {
                opened * (a / gaps_since).min(1.0)
            } else {
                0.0
            };
            cohorts.push(Cohort {
                candidate: index,
                age: a,
                weight: weight * (1.0 - share),
                pioneer: false,
                released: false,
            });
            if share > 0.0 {
                // On average it has stood in its gap half the years since
                // gaps could open over it.
                let free = 0.5 * a.min(gaps_since);
                let effective = free + (a - free) * RELEASE_CATCH_UP;
                cohorts.push(Cohort {
                    candidate: index,
                    age: nearest(&candidate.ages, effective),
                    weight: weight * share,
                    pioneer: false,
                    released: true,
                });
            }
        }
    }
}

fn smoothstep(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The age of `ages` nearest `age`, by ratio.
fn nearest(ages: &[f64], age: f64) -> f64 {
    ages.iter()
        .copied()
        .filter(|a| *a > 0.0)
        .min_by(|a, b| {
            libm::fabs(libm::log(a / age.max(1e-9)))
                .total_cmp(&libm::fabs(libm::log(b / age.max(1e-9))))
        })
        .unwrap_or(ages[0])
}

/// The layer a grown plant `height_m` tall stands in, in a stand whose
/// tallest trees are `top_m` tall: a tree in the canopy from two thirds of
/// that height, under it from a third, below as a sapling among the
/// shrubs; a shrub or herb in its niche's own layer. McIntosh et al.
/// (2009) layer a stand's trees relative to its own height.
#[must_use]
pub fn layer_of(candidate: &Candidate, height_m: f64, top_m: f64) -> Layer {
    if candidate.tree() {
        if height_m >= top_m * 2.0 / 3.0 {
            Layer::Canopy
        } else if height_m >= top_m / 3.0 {
            Layer::Subcanopy
        } else {
            Layer::Shrub
        }
    } else if candidate.layers[2] >= candidate.layers[3] {
        Layer::Shrub
    } else {
        Layer::Herb
    }
}

/// The light a plant needs from above to stand in a stand: where its
/// niche's light response ([`crate::niche::Niche::light_response`])
/// reaches half, so an intolerant sapling stands only in gaps.
#[must_use]
pub fn light_needed(niche: &crate::niche::Niche) -> f64 {
    let mut low = 0.0;
    let mut high = 1.0;
    if niche.light_response(0.0) >= 0.5 {
        return 0.0;
    }
    for _ in 0..30 {
        let middle = f64::midpoint(low, high);
        if niche.light_response(middle) >= 0.5 {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fir() -> Candidate {
        Candidate {
            shade_tolerance: 0.3,
            layers: [1.0, 0.5, 0.6, 0.0],
            lifespan: 500.0,
            ages: vec![5.0, 15.0, 30.0, 60.0, 120.0, 240.0, 480.0],
            suitability: 1.0,
        }
    }

    fn hemlock() -> Candidate {
        Candidate {
            shade_tolerance: 0.95,
            layers: [0.6, 1.0, 0.9, 0.0],
            lifespan: 400.0,
            ages: vec![5.0, 15.0, 30.0, 60.0, 120.0, 240.0],
            suitability: 1.0,
        }
    }

    fn fern() -> Candidate {
        Candidate {
            shade_tolerance: 0.9,
            layers: [0.0, 0.0, 0.0, 2.0],
            lifespan: 0.0,
            ages: vec![1.0, 3.0, 8.0],
            suitability: 1.0,
        }
    }

    #[test]
    fn stages_follow_franklins_ages_for_douglas_fir() {
        let at = |age| Stage::at(age, 500.0);
        assert_eq!(at(10.0), Stage::Establishment);
        assert_eq!(at(60.0), Stage::Exclusion);
        assert_eq!(at(150.0), Stage::Maturation);
        assert_eq!(at(250.0), Stage::VerticalDiversification);
        assert_eq!(at(500.0), Stage::HorizontalDiversification);
        assert_eq!(at(1000.0), Stage::PioneerLoss);
        // Cover closes by 35 years and the canopy opens in old growth.
        assert!(at(10.0).cover(10.0, 500.0)[0] < 0.5);
        assert!(at(500.0).cover(500.0, 500.0)[0] < at(60.0).cover(60.0, 500.0)[0]);
    }

    #[test]
    fn a_young_stand_is_its_pioneers_and_an_old_one_gains_tolerant_ingrowth() {
        let pool = [fir(), hemlock(), fern()];
        let young = develop(&pool, 60.0);
        assert_eq!(young.stage, Stage::Exclusion);
        // Only the first cohort's trees, both at 60 years, mostly fir.
        let trees: Vec<&Cohort> = young.cohorts.iter().filter(|c| c.candidate < 2).collect();
        assert!(trees.iter().all(|c| c.pioneer && c.age == 60.0));
        let weight = |d: &Development, i: usize| -> f64 {
            d.cohorts
                .iter()
                .filter(|c| c.candidate == i)
                .map(|c| c.weight)
                .sum()
        };
        assert!(weight(&young, 0) > 5.0 * weight(&young, 1));
        let old = develop(&pool, 500.0);
        assert_eq!(old.stage, Stage::HorizontalDiversification);
        // The fir pioneers are drawn at 480 years and thinning; hemlocks
        // stand at every age up to 400 years, the younger more often.
        assert!(
            old.cohorts
                .iter()
                .any(|c| c.candidate == 0 && c.pioneer && c.age == 480.0)
        );
        let hemlocks: Vec<&Cohort> = old
            .cohorts
            .iter()
            .filter(|c| c.candidate == 1 && !c.pioneer && !c.released)
            .collect();
        assert_eq!(
            hemlocks.iter().map(|c| c.age).collect::<Vec<_>>(),
            vec![5.0, 15.0, 30.0, 60.0, 120.0, 240.0]
        );
        let per_year = |c: &Cohort, before: f64| c.weight / (c.age - before);
        assert!(per_year(hemlocks[1], 5.0) > per_year(hemlocks[5], 120.0));
        assert!(weight(&old, 1) > weight(&old, 0));
        // No Douglas-fir establishes under the canopy.
        assert!(old.cohorts.iter().all(|c| c.candidate != 0 || c.pioneer));
        // The fern stands at its oldest age.
        assert!(old.cohorts.iter().any(|c| c.candidate == 2 && c.age == 8.0));
    }

    #[test]
    fn gaps_release_a_share_of_the_ingrowth_that_came_up_before_them() {
        let pool = [fir(), hemlock(), fern()];
        // Before the pioneers start to die, no gap releases any.
        assert!(develop(&pool, 280.0).cohorts.iter().all(|c| !c.released));
        let old = develop(&pool, 500.0);
        let hemlock = |pick: &dyn Fn(&Cohort) -> bool| -> f64 {
            old.cohorts
                .iter()
                .filter(|c| c.candidate == 1 && !c.pioneer && pick(c))
                .map(|c| c.weight)
                .sum()
        };
        // At 500 years the gaps are (1 - 0.6) / (2.1 - 0.6) of the fir
        // canopy; the younger cohorts saw only some of them open.
        let opened = (1.0 - GAPS) / (PIONEERS_GONE - GAPS);
        let share = hemlock(&|c| c.released) / hemlock(&|_| true);
        assert!(share > 0.05 && share < opened, "{share}");
        // The 240-year cohort has stood in its gaps 100 years on average,
        // so it grows on as a 120-year tree in a stand (100 + 140 / 3,
        // the nearest age it can be drawn at); none grows at its own age.
        let released: Vec<f64> = old
            .cohorts
            .iter()
            .filter(|c| c.candidate == 1 && c.released)
            .map(|c| c.age)
            .collect();
        assert!(
            released.contains(&120.0) && !released.contains(&240.0),
            "{released:?}"
        );
        assert!(old.cohorts.iter().all(|c| !(c.released && c.pioneer)));
    }

    #[test]
    fn the_site_picks_the_species_but_not_how_full_the_stand_is() {
        let mut drier = hemlock();
        drier.suitability = 0.2;
        let pool = [fir(), drier, fern()];
        let full = develop(&[fir(), hemlock(), fern()], 500.0);
        let dry = develop(&pool, 500.0);
        assert_eq!(full.cover, dry.cover);
        let hemlocks = |d: &Development| -> f64 {
            d.cohorts
                .iter()
                .filter(|c| c.candidate == 1)
                .map(|c| c.weight)
                .sum()
        };
        assert!((hemlocks(&dry) - 0.2 * hemlocks(&full)).abs() < 1e-9);
        // With no tree suiting the site well, the stand opens.
        let mut poor = [fir(), hemlock(), fern()];
        poor[0].suitability = 0.1;
        poor[1].suitability = 0.1;
        assert!(develop(&poor, 500.0).cover[0] < 0.5 * full.cover[0]);
    }

    #[test]
    fn planted_trees_stand_by_their_abundance_whatever_their_shade_tolerance() {
        let pool = [fir(), hemlock(), fern()];
        let weight = |d: &Development, candidate: usize| -> f64 {
            d.cohorts
                .iter()
                .filter(|c| c.candidate == candidate)
                .map(|c| c.weight)
                .sum()
        };
        // After a fire the tolerant hemlock barely joins the first cohort;
        // planted, each tree stands as its layers' abundance gives.
        let natural = develop(&pool, 80.0);
        let planted = develop_as(&pool, 80.0, Origin::Planted);
        assert!(weight(&natural, 1) < 0.2 * weight(&natural, 0));
        let ratio = weight(&planted, 1) / weight(&planted, 0);
        let layers = |c: &Candidate| c.layers[0] + c.layers[1];
        assert!((ratio - layers(&hemlock()) / layers(&fir())).abs() < 1e-9);
        // Nothing comes up beneath a kept yard's trees.
        assert!(
            planted
                .cohorts
                .iter()
                .all(|c| pool[c.candidate].tree() == c.pioneer)
        );
    }

    #[test]
    fn herbs_take_the_ground_the_canopy_leaves_open() {
        // A meadow: no tree, so herbs cover the ground as a meadow's do.
        let meadow = develop(&[fern()], 40.0);
        assert!((meadow.cover[3] - OPEN_HERB).abs() < 1e-9);
        // Just after a disturbance the stand is herb-rich; once its crowns
        // close, herbs keep only the closed stand's own share.
        let pool = [fir(), hemlock(), fern()];
        let young = develop(&pool, 5.0);
        let closed = develop(&pool, 60.0);
        assert!(young.cover[3] > 0.75, "{:?}", young.cover);
        assert!((closed.cover[3] - 0.15).abs() < 1e-9, "{:?}", closed.cover);
        // A woodland where no tree suits the site well lies between.
        let mut sparse = pool.clone();
        sparse[0].suitability = 0.15;
        sparse[1].suitability = 0.15;
        let woodland = develop(&sparse, 500.0).cover[3];
        assert!(woodland > develop(&pool, 500.0).cover[3] && woodland < OPEN_HERB);
    }

    #[test]
    fn layers_follow_height_in_the_stand() {
        let tree = hemlock();
        assert_eq!(layer_of(&tree, 50.0, 65.0), Layer::Canopy);
        assert_eq!(layer_of(&tree, 28.0, 65.0), Layer::Subcanopy);
        assert_eq!(layer_of(&tree, 8.0, 65.0), Layer::Shrub);
        assert_eq!(layer_of(&fern(), 1.0, 65.0), Layer::Herb);
    }
}
