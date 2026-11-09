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

/// The share of an overtopped cohort ([`Cohort::overtopped`]) that falls
/// into the suppressed crown class, the lower tree layer, the rest
/// standing in the middle layer as intermediate trees. Western Oregon's
/// wet conifer plots hold a lower layer a quarter to two fifths as wide
/// as the middle one from 15 to 105 years (McIntosh et al. 2009: 5% under
/// 19% in unthinned 15-45-year Douglas-fir, table 4; 10% under 26% at 65
/// years, 17% under 44% at 85), and a suppressed tree's crown is narrower
/// than an intermediate's, so somewhat fewer than half its stems. The
/// value is this model's choice.
pub const SUPPRESSED_SHARE: f64 = 0.4;

/// The share of the stand's age at which a suppressed tree is grown, with
/// the overtopped preset ([`crate::spec::Environment::Suppressed`]): it
/// fell behind early and grows as an overtopped tree of half its years.
/// The plots' lower layer stands about a fifth as high as their upper
/// (4.2 m under 20.5 m in table 4 of McIntosh et al. 2009), as such a
/// Douglas-fir does in this model's growth (about 4 m at 15 years under
/// a 30-year stand's 20 m). The value is this model's choice.
pub const SUPPRESSED_AGE: f64 = 0.5;

/// Herb cover where no tree suits the ground: a meadow. Grassland herbs
/// cover most of the ground, and restored Willamette Valley wet prairies
/// reached more than 100% total cover (layers counted apart) within three
/// to five years (Conservation Evidence study 9161). The value itself is
/// this model's choice. A forest site just after its disturbance holds
/// less ([`Stage::cover`]).
pub const OPEN_HERB: f64 = 0.9;

/// Each layer's cover in a closed stand, [`Stage::Exclusion`]'s: canopy,
/// subcanopy, shrub and herb, from western Oregon's wet conifer stands of
/// 35 to 65 years (McIntosh, Gray and Garman 2009, PNW-GTR-794, figs. 2
/// and 9, 298 inventory plots): the upper tree layer 68-74%, the middle
/// 19-26% (the trees the dominants overtop, most of them Douglas-firs:
/// shade-tolerant trees are 3.5 of its 19.4 points in their table 4) and
/// the lower 6-10%, shrubs 41-48% and forbs, ferns and grasses 22-33%.
/// The subcanopy here is the middle and lower tree layers together, the
/// lower trees standing under the middle's crowns as often as elsewhere
/// (22% and 6%, 27% together). Capitol Forest's 65-year Douglas-fir
/// stands beside Olympia measured shrubs 40% and overstory 77% (Evans,
/// Halpern and McKenzie 2012, The Bryologist 115: 278-294).
pub const CLOSED_LAYERS: [f64; 4] = [0.70, 0.27, 0.45, 0.28];

/// Each layer's cover in old growth, [`Stage::HorizontalDiversification`]'s
/// and after: the upper tree layer falls to 36-47% as the pioneers die,
/// the middle rises to 34-42% and the lower trees to 35-51% (McIntosh et
/// al. 2009, stands 200 to over 300 years, few plots), 63% together as
/// [`CLOSED_LAYERS`] adds them; the understorey covers about half the
/// ground and herbs 6% (Spies and Franklin 1991, PNW-GTR-285: 52% and
/// 6.4% in old growth against 46% and about 3% in young stands), here
/// herbs 15% for the plots' 11-14% of forbs and ferns.
pub const OLD_LAYERS: [f64; 4] = [0.45, 0.63, 0.50, 0.15];

/// Each layer's cover in a closed broadleaf stand, as [`CLOSED_LAYERS`]
/// gives a conifer one's: western Oregon's wet hardwood stands of 35 to 65
/// years (McIntosh, Gray and Garman 2009, PNW-GTR-794, figs. 2 and 9: 82
/// inventory plots, red alder dominant in 99 of the group's 137 and
/// bigleaf maple in 25). Their upper tree layer covers 54-69% (64 here),
/// the middle 27-31% over a lower 4-8% (35 together), as many trees as
/// beneath the conifers, but shrubs cover 46-59% (50) and forbs, ferns and
/// grasses 53-56% (54) where the conifers' hold 22-33%: under the alders'
/// lighter crowns the herb layer is twice as full, and it does not thin as
/// the trees close (forb cover rose with tree cover in the hardwood plots,
/// r = 0.12, and fell with it under conifers, r = -0.34, their table 3).
/// Few hardwood plots are older than 65 years (3 of 137), so a broadleaf
/// stand holds these covers in every later stage too, this model's choice;
/// as its pioneers die and conifers come up beneath them, its covers move
/// toward the conifers' by their share of the trees ([`hardwood_share`]).
pub const HARDWOOD_LAYERS: [f64; 4] = [0.64, 0.35, 0.50, 0.54];

/// The share of a closed conifer stand's forest floor mosses cover where
/// nothing grows over them, on moist ground: fitted, with
/// [`MOSS_UNDER_PLANTS`], to the Puget Trough's second-growth Douglas-fir
/// beside Olympia (Thysell and Carey 2001, PNW-GTR-488, table 5: mosses
/// 59% of the floor of dense unthinned 55-year stands whose vascular
/// understorey covered 34%, 30% in thinned stands whose understorey
/// covered 88%). Capitol Forest's 65-year stands held 14.5%, from 1 to 53%
/// by plot (Evans, Halpern and McKenzie 2012); the moss mat that harvest
/// strips takes decades to come back (under 5% after harvest on Vancouver
/// Island, Trofymow et al. 2003).
pub const FLOOR_MOSS: f64 = 0.72;

/// How much of the moss a full understorey of shrubs and herbs shades
/// out (see [`FLOOR_MOSS`]).
pub const MOSS_UNDER_PLANTS: f64 = 0.55;

/// The share of the moss a floor keeps under broadleaf trees' litter:
/// each autumn's fallen leaves bury and leach forest-floor feather mosses
/// (Startsev, Lieffers and Landhäusser 2008, Journal of Vegetation
/// Science 19: 253-260: aspen leaf litter and its extracts killed feather
/// mosses, pine needles did not), so under maples and alders moss keeps
/// to logs, stones and stem bases. The share is this model's choice.
pub const MOSS_UNDER_LEAVES: f64 = 0.3;

/// The tree cover from which a stand's floor is shaded and damp enough for
/// a full moss mat; an open stand's is drier and sunnier. This model's
/// choice.
pub const CLOSED_TREE_COVER: f64 = 0.8;

/// The tree cover from which litter covers all of a floor that moss and
/// logs leave; under thinner crowns some of the ground lies bare. This
/// model's choice.
pub const LITTER_CLOSURE: f64 = 0.5;

/// The share of a forest floor under moss ([`FLOOR_MOSS`]): less under
/// thin crowns ([`CLOSED_TREE_COVER`]), under shrubs and herbs
/// (`understorey`, the share of the ground under either,
/// [`MOSS_UNDER_PLANTS`]), on dry ground (none at moisture 0.1, all from
/// 0.35, a niche's moisture scale) and under broadleaf litter
/// (`leaf_litter`, the share of the crowns that shed broad leaves,
/// [`MOSS_UNDER_LEAVES`]).
#[must_use]
pub fn ground_moss(tree_cover: f64, understorey: f64, moisture: f64, leaf_litter: f64) -> f64 {
    let closure = (tree_cover / CLOSED_TREE_COVER).clamp(0.0, 1.0);
    let wet = ((moisture - 0.1) / 0.25).clamp(0.0, 1.0);
    let leaves = 1.0 - leaf_litter.clamp(0.0, 1.0) * (1.0 - MOSS_UNDER_LEAVES);
    let shaded = 1.0 - MOSS_UNDER_PLANTS * understorey.clamp(0.0, 1.0);
    (FLOOR_MOSS * closure * wet * leaves * shaded).clamp(0.0, 1.0)
}

/// The light on a forest floor, on [`crate::mosses::Habitat`]'s scale (0
/// deep shade, 1 open): open ground's 1, falling to 0.15 under closed
/// crowns ([`CLOSED_TREE_COVER`]), in the shade class where a closed
/// conifer stand's floor gets a few per cent of full sun. This model's
/// choice.
#[must_use]
pub fn floor_light(tree_cover: f64) -> f64 {
    1.0 - 0.85 * (tree_cover / CLOSED_TREE_COVER).clamp(0.0, 1.0)
}

/// How many years after a disturbance a forest floor's moss takes to pass
/// from the mosses that spread fastest to those that build the deepest
/// wefts. Hylocomium splendens, whose stair-step wefts add one frond a
/// year (Goffinet and Shaw, Bryophyte Biology, 2009, p. 424), is a moss
/// of stable late stages and very shade tolerant; 10 to 30 years after a
/// fire it replaces the first mosses, and in Finland it was not back to
/// its cover 50 years on (USDA Fire Effects Information System,
/// Hylocomium splendens, after Johnson 1981 and Ahlgren 1974). The years
/// are this model's choice.
pub const FLOOR_MOSS_BUILD_YEARS: f64 = 80.0;

/// Mosses whose fit to a floor is under this share of the best one's are
/// left out. This model's choice.
const FLOOR_MOSS_LEAST_FIT: f64 = 0.25;

/// The moss genera of a forest floor and each one's share of its moss,
/// from `kinds` (the library's mosses, [`crate::mosses::kinds`]): each
/// genus's fit to the floor ([`crate::mosses::MossKind::fit`] on soil and
/// humus, in [`floor_light`] under `tree_cover`, as wet as the ground's
/// `moisture`), weighted early by how fast it spreads
/// (`colony_spread_m_per_year`) and late by how deep its colonies grow
/// (`height_typical_m`), the two blended over
/// [`FLOOR_MOSS_BUILD_YEARS`] since the disturbance (`years`). At most
/// `most` genera, each at least a tenth, largest first; none where no
/// moss fits.
#[must_use]
pub fn floor_mosses(
    kinds: &[crate::mosses::MossKind],
    tree_cover: f64,
    moisture: f64,
    years: f64,
    most: usize,
) -> Vec<(usize, f64)> {
    let habitat = crate::mosses::Habitat {
        light: floor_light(tree_cover),
        wetness: moisture.clamp(0.0, 1.0),
        material: crate::substrate::SOIL,
        // The floor's reaction is not modelled yet: neither acid nor
        // basic, so no genus is kept off it for its reaction.
        reaction: 0,
        soil: true,
        hold: 1.0,
    };
    let fits: Vec<f64> = kinds.iter().map(|kind| kind.fit(&habitat)).collect();
    let best = fits.iter().copied().fold(0.0, f64::max);
    if best <= 1.0e-6 {
        return Vec::new();
    }
    let fitting: Vec<usize> = (0..kinds.len())
        .filter(|&k| fits[k] >= FLOOR_MOSS_LEAST_FIT * best)
        .collect();
    let shares = |weight: &dyn Fn(usize) -> f64| -> Vec<f64> {
        let total: f64 = fitting.iter().map(|&k| weight(k)).sum();
        fitting
            .iter()
            .map(|&k| if total > 0.0 { weight(k) / total } else { 0.0 })
            .collect()
    };
    let early = shares(&|k| fits[k] * kinds[k].spread_m);
    let late = shares(&|k| fits[k] * kinds[k].height_m);
    let built = 1.0 - (-years.max(0.0) / FLOOR_MOSS_BUILD_YEARS).exp();
    let mut mix: Vec<(usize, f64)> = fitting
        .iter()
        .enumerate()
        .map(|(n, &k)| (k, (1.0 - built) * early[n] + built * late[n]))
        .collect();
    mix.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    mix.retain(|(_, share)| *share >= 0.1);
    mix.truncate(most);
    let total: f64 = mix.iter().map(|(_, share)| share).sum();
    for (_, share) in &mut mix {
        *share /= total;
    }
    mix
}

/// The day of the year a stand's covers stand for: inventory crews measure
/// in the growing season, so the plots' layers ([`CLOSED_LAYERS`],
/// [`OLD_LAYERS`]) are the summer's, as a package's day
/// ([`crate::package::DEFAULT_DAY`], mid-July) is.
pub const PLOT_DAY: f64 = crate::package::DEFAULT_DAY;

/// The light a crown in full leaf lets through, as a share of the light
/// above it: [`crate::stand_tile::Rules`]'s default.
pub const LEAFY_CROWN_LIGHT: f64 = 0.1;

/// The light a bare deciduous crown lets through, as a share of the light
/// above it. Under a Maryland beech and tulip-poplar canopy (23 m, leaf
/// area index 5.1) the light at 1.75 m was a median 1.3% of the open
/// sky's in full summer leaf and 31% in the leafless winter, 39% in the
/// leafless spring (Hudson et al. 2017, PLOS ONE 12: e0185894, table 1),
/// the trees' and shrubs' woody frames stopping up to half of it. The
/// crowns that let 1.3% through at [`LEAFY_CROWN_LIGHT`] each are 1.9
/// crowns deep (0.1^1.9 = 0.013), and 1.9 bare crowns let 35% through at
/// 0.57 each.
pub const BARE_CROWN_LIGHT: f64 = 0.57;

/// The light a crown lets through when its leaves are `leafy` of their
/// full size ([`crate::looks::Leafing::size`]: 0 bare, 1 in full leaf),
/// between [`BARE_CROWN_LIGHT`] and `in_leaf`, in step with its leaves'
/// size. The steps between are this model's choice.
#[must_use]
pub fn crown_light(leafy: f64, in_leaf: f64) -> f64 {
    BARE_CROWN_LIGHT + (in_leaf - BARE_CROWN_LIGHT) * leafy.clamp(0.0, 1.0)
}

/// A herb's year above ground: its shoots come up on day `up` (1 to 365)
/// and are down by day `down`, a `down` past 365 in the next year, as a
/// leaf's year is counted ([`crate::looks::Leafing`]). A herb without one
/// stands all year.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shoots {
    pub up: f64,
    pub down: f64,
}

impl Shoots {
    /// Day `day` within its year: a day before `up` is in the next year
    /// where its `down` comes then.
    fn on(&self, day: f64) -> f64 {
        if day < self.up && day + 365.0 < self.down {
            day + 365.0
        } else {
            day
        }
    }

    /// Whether its shoots stand on day `day` of the year.
    #[must_use]
    pub fn stand_on(&self, day: f64) -> bool {
        (self.up..self.down).contains(&self.on(day))
    }

    /// The middle of its year above ground, 1 to 365: the day the light it
    /// grows in is judged on, so a spring herb is judged under a canopy
    /// still bare.
    #[must_use]
    pub fn middle(&self) -> f64 {
        let middle = f64::midpoint(self.up, self.down);
        if middle > 365.0 {
            middle - 365.0
        } else {
            middle
        }
    }

    /// A herb's shoots' year from its traits, by the rules a deciduous
    /// woody plant's leaves keep theirs (the angiosperms' rank file,
    /// `appearance.organs.leaf.leafing`): a deciduous herb, or an annual,
    /// comes up a third of the way into the first month floras say its
    /// leaves unfold in (`leaf_out_months`), or into April, and no later
    /// than the start of the first month it flowers in, since a herb
    /// flowers on its shoots; it is down ten days before the last month
    /// its leaves are said to fall in ends (`leaf_fall_months`), or
    /// November, those months in the next year where they come before its
    /// leaves'. A drought-deciduous herb stands only in the months floras
    /// give for its leaves. `None` for a woody plant (its leaves keep a
    /// [`crate::looks::Leafing`]), an evergreen herb (by its most usual
    /// habit, where floras say how often each occurs), a biennial's
    /// overwintering rosette, and a herb whose habit floras do not give.
    #[must_use]
    pub fn of_herb(traits: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        use crate::formula::month_start;
        // Its most usual states, as the rank files' rules read a trait
        // stated with how often each occurs: a bracken `{"deciduous":
        // "usually", "evergreen": "rarely"}` dies back.
        let states = |name: &str| -> Vec<String> {
            match traits.get(name) {
                Some(serde_json::Value::Object(stated)) => {
                    let rank = |frequency: &serde_json::Value| {
                        crate::inherit::FREQUENCIES
                            .iter()
                            .position(|known| frequency.as_str() == Some(known))
                    };
                    let best = stated.values().filter_map(rank).min();
                    stated
                        .iter()
                        .filter(|(_, frequency)| best.is_some() && rank(frequency) == best)
                        .map(|(state, _)| state.clone())
                        .collect()
                }
                Some(value) => crate::traits::states_of(value),
                None => Vec::new(),
            }
        };
        let months = |name: &str| -> Vec<f64> {
            traits
                .get(name)
                .and_then(serde_json::Value::as_array)
                .map(|months| {
                    months
                        .iter()
                        .filter_map(serde_json::Value::as_f64)
                        .collect()
                })
                .unwrap_or_default()
        };
        if states("woodiness").iter().any(|state| state == "woody") {
            return None;
        }
        let habit = states("leaf_habit");
        let has = |state: &str| habit.iter().any(|own| own == state);
        if has("evergreen") || has("semi") {
            return None;
        }
        let annual = states("life_span").iter().any(|state| state == "annual");
        let (out, fall, flowers) = (
            months("leaf_out_months"),
            months("leaf_fall_months"),
            months("flowering_months"),
        );
        let first = |months: &[f64]| months.iter().copied().reduce(f64::min);
        let last = |months: &[f64]| months.iter().copied().reduce(f64::max);
        let late = |out: f64, fall: f64| if fall < out { 365.0 } else { 0.0 };
        if has("drought_deciduous") {
            let up = month_start(first(&out)?) + 10.0;
            let down = last(&fall).map_or(365.0, |fall| {
                month_start(fall + 1.0) - 10.0 + late(first(&out).unwrap_or(1.0), fall)
            });
            return Some(Self { up, down });
        }
        if !has("deciduous") && !annual {
            return None;
        }
        let out_month = first(&out).unwrap_or(4.0);
        let mut up = month_start(out_month) + 10.0;
        if let Some(flowering) = first(&flowers) {
            up = up.min(month_start(flowering));
        }
        let fall_first = first(&fall).unwrap_or(10.0);
        let down =
            month_start(last(&fall).unwrap_or(11.0) + 1.0) - 10.0 + late(out_month, fall_first);
        Some(Self { up, down })
    }
}

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

    /// Ground covered by each layer's crowns, canopy, subcanopy (the
    /// middle and lower tree layers), shrub and herb ([`Layer::ALL`]),
    /// seen from above, each layer's crowns counted once where they
    /// overlap, as inventory crews measure them (McIntosh et al. 2009,
    /// figs. 2 and 9, western Oregon's wet conifer stands). Before closure
    /// the tree layers close in step with the years (upper 7, 32 and 62%
    /// at 5, 15 and 25 years; middle 1, 11 and 17%), shrubs already cover
    /// 36-53% and herbs fall from 47% to 27% as the crowns close. A closed
    /// stand holds [`CLOSED_LAYERS`]; in maturation the upper layer thins
    /// and the lower layers, shrubs and herbs grow (upper 47-64%, middle
    /// 22-30% over lower 10-17%, together 37% as [`CLOSED_LAYERS`] adds
    /// them, shrubs 46-57%, herbs 26-47% from 75 to 115 years); old growth
    /// holds [`OLD_LAYERS`], vertical diversification halfway to it.
    ///
    /// A broadleaf stand holds [`HARDWOOD_LAYERS`] once closed instead, and
    /// one of both kinds of trees covers in between by `hardwood`, the
    /// hardwoods' share of its trees ([`hardwood_share`]).
    #[must_use]
    pub fn cover(self, age: f64, lifespan: f64, hardwood: f64) -> [f64; 4] {
        let conifer = self.conifer_cover(age, lifespan);
        let hardwood = hardwood.clamp(0.0, 1.0);
        if hardwood <= 0.0 {
            return conifer;
        }
        let broadleaf = match self {
            Self::Establishment => {
                let closing = (age / (CLOSURE * lifespan.max(1.0))).clamp(0.0, 1.0);
                let [canopy, subcanopy, shrub, herb] = HARDWOOD_LAYERS;
                [
                    canopy * closing,
                    subcanopy * closing,
                    shrub,
                    0.47 + (herb - 0.47) * closing,
                ]
            }
            _ => HARDWOOD_LAYERS,
        };
        std::array::from_fn(|i| conifer[i] + (broadleaf[i] - conifer[i]) * hardwood)
    }

    /// [`Stage::cover`] of a conifer stand.
    fn conifer_cover(self, age: f64, lifespan: f64) -> [f64; 4] {
        let [canopy, subcanopy, shrub, herb] = CLOSED_LAYERS;
        let between = |share: f64| -> [f64; 4] {
            std::array::from_fn(|i| CLOSED_LAYERS[i] + (OLD_LAYERS[i] - CLOSED_LAYERS[i]) * share)
        };
        match self {
            Self::Establishment => {
                let closing = (age / (CLOSURE * lifespan.max(1.0))).clamp(0.0, 1.0);
                [
                    canopy * closing,
                    subcanopy * closing,
                    shrub,
                    0.47 + (herb - 0.47) * closing,
                ]
            }
            Self::Exclusion => CLOSED_LAYERS,
            Self::Maturation => [0.6, 0.37, 0.52, 0.36],
            Self::VerticalDiversification => between(0.5),
            Self::HorizontalDiversification | Self::PioneerLoss => OLD_LAYERS,
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
    /// Whether its wood is a hardwood (the `wood_class` trait: every
    /// angiosperm's, from the Wood Handbook's division), so a stand of it
    /// takes a broadleaf stand's covers ([`HARDWOOD_LAYERS`]).
    pub hardwood: bool,
}

impl Candidate {
    /// Whether a species' traits give it a hardwood (`wood_class`
    /// `"hardwood"`), as [`Candidate::hardwood`] reads it.
    #[must_use]
    pub fn hardwood_in(traits: &serde_json::Map<String, serde_json::Value>) -> bool {
        traits
            .get("wood_class")
            .is_some_and(|class| class.as_str() == Some("hardwood"))
    }

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
    /// Pioneers their own cohort's dominants overtop once the crowns
    /// close: the intermediate and suppressed crown classes of an
    /// even-aged stand. They grow as overtopped trees grow
    /// ([`crate::spec::Environment::Suppressed`]) and stand under the
    /// canopy, in the subcanopy or lower.
    pub overtopped: bool,
    /// Old trees a fire-kept stand's surface fires have passed
    /// ([`develop_kept`]): they grew up in the open and stand in it.
    pub veteran: bool,
}

/// A stand of one age: its stage, its cohorts and its layers' cover.
#[derive(Debug, Clone, PartialEq)]
pub struct Development {
    pub stage: Stage,
    /// The lifespan of its pioneers, years.
    pub lifespan: f64,
    pub cohorts: Vec<Cohort>,
    pub cover: [f64; 4],
    /// Whether its trees still grow as in the open: until half its years
    /// to closure ([`CLOSURE`]), when its tree layers cover about half the
    /// ground and neighbouring crowns meet (44% at 15 years in western
    /// Oregon's wet conifer plots, McIntosh et al. 2009). From then they
    /// grow as in a stand, crowded, as a young natural stand's thousands
    /// of stems a hectare do.
    pub open_grown: bool,
}

/// The cohorts of a stand `age` years after a stand-replacing
/// disturbance, from `candidates`:
///
/// 1. Every tree establishes in the first cohort, in proportion to its
///    abundance in the canopy and subcanopy and its intolerance of shade
///    (Douglas-fir dominates the cohort that follows a fire; hemlock
///    joins it a little), drawn at the stand's age (or the oldest it can
///    be drawn at). Past [`GAPS`] of its lifespan its share falls to
///    nothing at [`PIONEERS_GONE`]. Once the crowns close, the cohort's
///    dominants overtop part of it ([`Cohort::overtopped`]), in the same
///    shares: the middle layer of a young stand is mostly these trees
///    (McIntosh et al. 2009; [`CLOSED_LAYERS`]), and its lower layer the
///    suppressed among them ([`SUPPRESSED_SHARE`], grown as at
///    [`SUPPRESSED_AGE`] of the stand's years). Overtopped trees
///    intolerant of shade die out through maturation, from
///    [`REINITIATION`] to [`VERTICAL`] of the pioneers' lifespan, as
///    young stands' many small Douglas-firs do (Spies and Franklin 1991:
///    490 shade-intolerant trees a hectare in young stands, 243 in
///    mature, 63 in old growth); tolerant ones stay.
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
    /// It came up by itself on ground people cleared or built on: a vacant
    /// lot, a building site, a railway's edge. Its stand develops as a
    /// natural one does, but no forest stood there before it, so nothing
    /// lies on its floor that one left: its only logs are those its own
    /// trees drop ([`crate::deadwood::DownWood::at`]).
    Cleared,
}

/// The pioneers' lifespan among `candidates`, years: the longest-lived
/// tree's that is intolerant of shade, else any tree's, at least 1.
#[must_use]
pub fn pioneer_lifespan(candidates: &[Candidate]) -> f64 {
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
    lifespan.max(1.0)
}

/// [`develop`] for a stand of `origin`.
#[must_use]
pub fn develop_as(candidates: &[Candidate], age: f64, origin: Origin) -> Development {
    let planted = origin == Origin::Planted;
    let lifespan = pioneer_lifespan(candidates);
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
            let trees = candidate.layers[0] + candidate.layers[1];
            let weight = if planted {
                suits * trees * survival
            } else {
                suits * trees * (1.0 - candidate.shade_tolerance) * survival
            };
            let drawn = oldest_at(&candidate.ages, age);
            if weight > 0.0 {
                cohorts.push(Cohort {
                    candidate: index,
                    age: drawn,
                    weight,
                    pioneer: true,
                    released: false,
                    overtopped: false,
                    veteran: false,
                });
            }
            // Its overtopped share, once the crowns close: intermediate
            // trees at its age, suppressed ones grown as at a younger.
            let persists = if candidate.shade_tolerance >= TOLERANT {
                1.0
            } else {
                1.0 - smoothstep(REINITIATION, VERTICAL, age / lifespan)
            };
            if !planted && stage != Stage::Establishment && weight * persists > 0.0 {
                for (share, at) in [
                    (1.0 - SUPPRESSED_SHARE, drawn),
                    (SUPPRESSED_SHARE, drawn * SUPPRESSED_AGE),
                ] {
                    cohorts.push(Cohort {
                        candidate: index,
                        age: at,
                        weight: weight * persists * share,
                        pioneer: true,
                        released: false,
                        overtopped: true,
                        veteran: false,
                    });
                }
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
                    overtopped: false,
                    veteran: false,
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
    let mut cover = stage.cover(age, lifespan, hardwood_share(candidates, &cohorts));
    let woodland = smoothstep(0.0, 0.5, best);
    cover[0] *= woodland;
    cover[1] *= woodland;
    // Herbs take the ground no tree can hold, up to a meadow's cover where
    // none suits the site. A stage's own herb cover is the least it holds.
    cover[3] = cover[3].max(OPEN_HERB * (1.0 - woodland));
    Development {
        stage,
        lifespan,
        cohorts,
        cover,
        open_grown: age < 0.5 * CLOSURE * lifespan,
    }
}

/// The share of the ground a fire-kept stand's old trees cover, seen from
/// above, however recent its last fire: in the Willamette Valley and Puget
/// Trough's prairies and savannas, fires every 3 to 10 years kept the
/// trees few, and "savanna conditions are created when an occasional tree
/// survives the frequent fires". The open classes, grassland with isolated
/// trees under 20% canopy cover and savanna of large oaks under 20%, held
/// 88% of the landscape in LANDFIRE's model of it (Biophysical Setting
/// 11200, Willamette Valley Upland Prairie and Savanna, 2020: classes C
/// and D, 20% and 68%). The value, half that bound, is this model's
/// choice.
pub const VETERAN_COVER: f64 = 0.1;

/// The share of its lifespan a fire-kept stand's old tree is drawn at (or
/// the oldest it can be drawn at): the savanna's "occasional giant Oregon
/// white oak" (BpS 11200, its class D in the largest size class, over 33
/// inches, 84 cm, at breast height). An oak of 500-year life (FEIS, Gucker
/// 2007) is drawn at 100 years, when an open-grown one is about 18 m tall
/// and 90-99 cm through in this model's growth (88 cm at 80 years, 99 at
/// 120). The share is this model's choice.
pub const VETERAN_AGE: f64 = 0.2;

/// The fire-free years after which a fire-kept grassland has become
/// woodland: LANDFIRE's model moves its grassland (class A, bunchgrass and
/// forbs with no shrub named) into the closed class (B, oak over common
/// snowberry and poison oak, "grasslands that are actively becoming
/// invaded by trees") 28 years after the last fire (BpS 11200, 2020), and
/// Douglas-fir took Fort Lewis's grasslands to forest in under 30 years
/// (Foster and Shaff 2003). A fire-kept stand's shrubs reach the cover
/// [`develop_as`] gives them only then, in step with the years since the
/// fire before ([`develop_kept`]), this model's reading of the classes.
pub const FIRE_FREE_CLOSURE: f64 = 28.0;

/// Trees' cover from above at which a stand is closed: LANDFIRE's mapping
/// rules put trees 5 m tall and more over more than 30% of the ground in
/// the closed classes (B and E), over less in the open ones (C and D;
/// BpS 11200, 2020).
pub const LANDFIRE_CLOSED: f64 = 0.3;

/// The age `since` years after its last fire a fire-kept stand's trees
/// stand at, as [`develop_as`] grows a stand from its disturbance. Trees
/// come into a grassland's sod more slowly than onto a clearing: western
/// Oregon's young conifer stands cover 30% of the ground at about 11
/// years ([`Stage::cover`]: [`LANDFIRE_CLOSED`] of the closed stand's
/// [`CLOSED_LAYERS`] trees, [`CLOSURE`] of the pioneers' `lifespan`), where
/// the grassland closes only after [`FIRE_FREE_CLOSURE`] years. So until
/// then its trees' years run that much slower, and after it they keep
/// their lag: a prairie unburnt for 80 years holds a forest of 63.
#[must_use]
pub fn encroached_age(since: f64, lifespan: f64) -> f64 {
    let since = since.max(0.0);
    let closed =
        LANDFIRE_CLOSED / (CLOSED_LAYERS[0] + CLOSED_LAYERS[1]) * CLOSURE * lifespan.max(1.0);
    if closed >= FIRE_FREE_CLOSURE {
        since
    } else if since <= FIRE_FREE_CLOSURE {
        since * closed / FIRE_FREE_CLOSURE
    } else {
        since - (FIRE_FREE_CLOSURE - closed)
    }
}

/// The share of a fire-kept prairie's ground under moss, long after its
/// last fire. The south Puget Sound's native prairies are "dominated by
/// Idaho fescue (Festuca idahoensis var. roemeri) and mosses (especially
/// Rhacomitrium canescens)" (Chappell, Washington Natural Heritage
/// Program, Native Vegetation of the South Puget Sound Prairie Landscape:
/// Idaho fescue "generally occupying 30-70 percent cover"). On five of
/// them (Glacial Heritage, Joint Base Lewis-McChord, Mima Mounds, Scatter
/// Creek, West Rocky), plots unburnt for at least 20 years had 54 ± 23% of
/// their ground under mosses and lichens, the lichens 26% of that
/// (Calabria et al. 2016, Northwest Science 90(4): 470-483, table 3), so
/// mosses 40%, the value here; Racomitrium (now Niphotrichum) is the
/// commonest of them (7.4% of the ground on Glacial Heritage and Mima
/// Mounds; Calabria et al. 2020, Plant and Soil 456: 307-321).
pub const PRAIRIE_MOSS: f64 = 0.40;

/// The share of its moss cover a prairie keeps just after a fire: the
/// same prairies' plots burnt one to three times in ten years, last 8 to
/// 20 months before they were read, had 33% of their ground under mosses,
/// with only traces of lichen (Calabria et al. 2016, table 3), against
/// the unburnt plots' 40% ([`PRAIRIE_MOSS`]).
pub const MOSS_AFTER_FIRE: f64 = 0.83;

/// The years a prairie's moss takes to grow back after a fire: Calabria et
/// al. (2020) took plots over 10 years unburnt as undisturbed. The moss
/// returns over them as `1 - exp(-3 since / years)`, this model's curve.
pub const PRAIRIE_MOSS_YEARS: f64 = 10.0;

/// The share of a fire-kept prairie's ground under moss `since` years
/// after its last fire: [`MOSS_AFTER_FIRE`] of [`PRAIRIE_MOSS`] just
/// after, all of it by [`PRAIRIE_MOSS_YEARS`].
#[must_use]
pub fn prairie_moss(since: f64) -> f64 {
    let back = 1.0 - (-3.0 * since.max(0.0) / PRAIRIE_MOSS_YEARS).exp();
    PRAIRIE_MOSS * (MOSS_AFTER_FIRE + (1.0 - MOSS_AFTER_FIRE) * back)
}

/// The years after a fire a grassland's litter takes to build back: in
/// central Saskatchewan's Fescue Prairie, burnt in early spring and
/// followed for 11 years, the standing crop rose until about the eighth
/// year and the litter's mass levelled off about the eleventh (Pylypec and
/// Romo 2003, Journal of Range Management 56), while the grasses
/// themselves were back within 3 to 4.
pub const THATCH_YEARS: f64 = 11.0;

/// The share of its full thatch a fire-kept grassland's floor holds
/// `since` years after its last fire: none just after, 95% by
/// [`THATCH_YEARS`], rising as `1 - exp(-3 since / THATCH_YEARS)`, this
/// model's curve through the plateau.
#[must_use]
pub fn thatch_built(since: f64) -> f64 {
    1.0 - (-3.0 * since.max(0.0) / THATCH_YEARS).exp()
}

/// [`develop_as`] for a stand that surface fires keep open, `since` years
/// after its last fire ([`crate::fill::Community::fire_years`]): a prairie
/// or an oak savanna. What came up since the fire develops as a stand
/// does from its disturbance, its trees at [`encroached_age`] (they come
/// into the sod more slowly than onto a clearing), its shrubs, which
/// resprout, at `since`: where fire stays away they close into woodland
/// and then forest, as LANDFIRE's model has the grassland close after 28
/// fire-free years, and as Douglas-fir converted Fort Lewis's grasslands
/// to forest within 30 years (Foster and Shaff 2003, Northwest Science
/// 77(4)). Beside it:
///
/// 1. Old trees the fires passed ([`Cohort::veteran`]): each tree
///    intolerant of shade, as the thick-barked oaks, pines and
///    Douglas-firs of the savannas are, in proportion to its abundance in
///    the canopy and subcanopy, drawn at [`VETERAN_AGE`] of its lifespan,
///    covering [`VETERAN_COVER`] of the ground at least. Where firs come
///    up and overtop them, they stand under the firs, as the valley's
///    large oaks now die under closed conifer canopies (BpS 11200).
/// 2. The grass sod: surface fires leave it (prescribed fire had no effect
///    on Idaho fescue's frequency on Fort Lewis's prairies, which it kept
///    dominating; Tveten and Fonda 1999, Northwest Science 73: 145-158),
///    so herbs hold a meadow's cover ([`OPEN_HERB`]) on the ground the
///    trees leave open, however recent the fire, each herb grown to the
///    oldest age it can be drawn at up to [`FIRE_FREE_CLOSURE`]. Shrubs, top-killed, resprout and
///    stand at the years since the fire, their cover building with the
///    trees' to [`FIRE_FREE_CLOSURE`].
#[must_use]
pub fn develop_kept(candidates: &[Candidate], since: f64, origin: Origin) -> Development {
    let mut stand = develop_as(
        candidates,
        encroached_age(since, pioneer_lifespan(candidates)),
        origin,
    );
    // The sod's plants outlive the fires: each herb stands at the oldest
    // age it can be drawn at up to the grassland's fire-free span
    // ([`FIRE_FREE_CLOSURE`]), not at the years since the fire (a clonal
    // fern's oldest drawn age is centuries).
    // A shrub, top-killed, resprouts from the fire: it stands at the
    // years since it.
    let sod = since.max(FIRE_FREE_CLOSURE);
    for cohort in &mut stand.cohorts {
        let candidate = &candidates[cohort.candidate];
        if candidate.tree() {
            continue;
        }
        let up_to = if candidate.layers[2] <= 0.0 && candidate.layers[3] > 0.0 {
            sod
        } else {
            since
        };
        cohort.age = candidate
            .ages
            .iter()
            .copied()
            .filter(|a| *a <= up_to)
            .fold(cohort.age, f64::max);
    }
    let mut any = false;
    for (index, candidate) in candidates.iter().enumerate() {
        if !candidate.tree()
            || candidate.shade_tolerance >= TOLERANT
            || candidate.ages.is_empty()
            || candidate.suitability <= 0.0
        {
            continue;
        }
        let weight = candidate.suitability
            * (candidate.layers[0] + candidate.layers[1])
            * (1.0 - candidate.shade_tolerance);
        if weight <= 0.0 {
            continue;
        }
        let age = oldest_at(&candidate.ages, VETERAN_AGE * candidate.lifespan.max(1.0));
        if age <= since {
            continue;
        }
        any = true;
        stand.cohorts.push(Cohort {
            candidate: index,
            age,
            weight,
            pioneer: true,
            released: false,
            overtopped: false,
            veteran: true,
        });
    }
    if any {
        stand.cover[0] = stand.cover[0].max(VETERAN_COVER);
    }
    // Shrubs come in with the trees: the model's grassland after a fire
    // names no shrub, and snowberry and poison oak first stand in the
    // grassland that trees are invading.
    stand.cover[2] *= (since / FIRE_FREE_CLOSURE).clamp(0.0, 1.0);
    let trees = (stand.cover[0] + stand.cover[1]).min(1.0);
    stand.cover[3] = stand.cover[3].max(OPEN_HERB * (1.0 - trees));
    stand
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
                overtopped: false,
                veteran: false,
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
                    overtopped: false,
                    veteran: false,
                });
            }
        }
    }
}

fn smoothstep(low: f64, high: f64, value: f64) -> f64 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `age` itself, or the oldest of `ages` where it is older.
fn oldest_at(ages: &[f64], age: f64) -> f64 {
    let oldest = ages.iter().copied().fold(0.0_f64, f64::max);
    if oldest > 0.0 { age.min(oldest) } else { age }
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

/// The hardwoods' share of a stand's trees, 0 to 1: of the weight of its
/// tree cohorts that are not overtopped (the canopy's, and the ingrowth
/// beneath it), the share whose candidates are hardwoods
/// ([`Candidate::hardwood`]). Zero where it holds no trees.
#[must_use]
pub fn hardwood_share(candidates: &[Candidate], cohorts: &[Cohort]) -> f64 {
    let (mut hard, mut all) = (0.0, 0.0);
    for cohort in cohorts {
        let candidate = &candidates[cohort.candidate];
        if !candidate.tree() || cohort.overtopped {
            continue;
        }
        all += cohort.weight;
        if candidate.hardwood {
            hard += cohort.weight;
        }
    }
    if all > 0.0 { hard / all } else { 0.0 }
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
            hardwood: false,
        }
    }

    fn hemlock() -> Candidate {
        Candidate {
            shade_tolerance: 0.95,
            layers: [0.6, 1.0, 0.9, 0.0],
            lifespan: 400.0,
            ages: vec![5.0, 15.0, 30.0, 60.0, 120.0, 240.0],
            suitability: 1.0,
            hardwood: false,
        }
    }

    fn fern() -> Candidate {
        Candidate {
            shade_tolerance: 0.9,
            layers: [0.0, 0.0, 0.0, 2.0],
            lifespan: 0.0,
            ages: vec![1.0, 3.0, 8.0],
            suitability: 1.0,
            hardwood: false,
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
        assert!(at(10.0).cover(10.0, 500.0, 0.0)[0] < 0.5);
        assert!(at(500.0).cover(500.0, 500.0, 0.0)[0] < at(60.0).cover(60.0, 500.0, 0.0)[0]);
    }

    #[test]
    fn a_young_stand_is_its_pioneers_and_an_old_one_gains_tolerant_ingrowth() {
        let pool = [fir(), hemlock(), fern()];
        let young = develop(&pool, 60.0);
        assert_eq!(young.stage, Stage::Exclusion);
        // Only the first cohort's trees, both at 60 years (their
        // suppressed grown as at 30), mostly fir.
        let trees: Vec<&Cohort> = young.cohorts.iter().filter(|c| c.candidate < 2).collect();
        assert!(
            trees
                .iter()
                .all(|c| c.pioneer && (c.age == 60.0 || (c.overtopped && c.age == 30.0)))
        );
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
        // Just after a disturbance a forest site is herb-rich (47% at 5
        // years in western Oregon's wet conifer plots); once its crowns
        // close, herbs keep the closed stand's share.
        let pool = [fir(), hemlock(), fern()];
        let young = develop(&pool, 5.0);
        let closed = develop(&pool, 60.0);
        assert!((0.4..0.5).contains(&young.cover[3]), "{:?}", young.cover);
        assert!(
            (closed.cover[3] - CLOSED_LAYERS[3]).abs() < 1e-9,
            "{:?}",
            closed.cover
        );
        // A woodland where no tree suits the site well lies between.
        let mut sparse = pool.clone();
        sparse[0].suitability = 0.15;
        sparse[1].suitability = 0.15;
        let woodland = develop(&sparse, 500.0).cover[3];
        assert!(woodland > develop(&pool, 500.0).cover[3] && woodland < OPEN_HERB);
    }

    #[test]
    fn layers_cover_the_ground_as_inventory_plots_find() {
        // Western Oregon's wet conifer stands (McIntosh et al. 2009): a
        // closed young stand has an upper layer of about 70% over a middle
        // of about 22%, shrubs over 40% and herbs under a third; old
        // growth's upper layer thins as its middle and lower fill.
        let at = |age: f64| Stage::at(age, 500.0).cover(age, 500.0, 0.0);
        let young = at(50.0);
        assert!((0.65..0.75).contains(&young[0]) && (0.2..0.3).contains(&young[1]));
        assert!(young[2] > 0.4 && young[3] < 0.34);
        let old = at(500.0);
        assert!(old[0] < young[0] && old[1] > young[1] && old[2] > young[2]);
        // The tree layers close in step with the years.
        assert!(at(5.0)[0] < 0.15 && at(25.0)[0] > 0.45);
    }

    #[test]
    fn a_closed_cohort_overtops_part_of_itself_and_the_intolerant_part_dies_out() {
        let pool = [fir(), hemlock(), fern()];
        let overtopped = |age: f64, candidate: usize| {
            develop(&pool, age)
                .cohorts
                .iter()
                .filter(|c| c.overtopped && c.candidate == candidate)
                .map(|c| c.weight)
                .sum::<f64>()
        };
        // None before the crowns close; then as many as its dominants.
        assert!(overtopped(20.0, 0) == 0.0);
        let dominants: f64 = develop(&pool, 60.0)
            .cohorts
            .iter()
            .filter(|c| c.pioneer && !c.overtopped && c.candidate == 0)
            .map(|c| c.weight)
            .sum();
        assert!((overtopped(60.0, 0) - dominants).abs() < 1e-9);
        // Some of them suppressed, grown as at half the stand's years.
        let suppressed: f64 = develop(&pool, 60.0)
            .cohorts
            .iter()
            .filter(|c| c.overtopped && c.candidate == 0 && c.age < 60.0)
            .map(|c| c.weight)
            .sum();
        assert!((suppressed - SUPPRESSED_SHARE * dominants).abs() < 1e-9);
        // Overtopped Douglas-firs are gone by vertical diversification;
        // overtopped hemlocks stay.
        assert!(overtopped(150.0, 0) < overtopped(60.0, 0));
        assert!(overtopped(250.0, 0) == 0.0 && overtopped(250.0, 1) > 0.0);
        // Pioneers stand at the stand's own age, not the nearest they can
        // be drawn at.
        assert!(
            develop(&pool, 80.0)
                .cohorts
                .iter()
                .filter(|c| c.pioneer && !c.overtopped)
                .all(|c| c.age == 80.0)
        );
    }

    #[test]
    fn layers_follow_height_in_the_stand() {
        let tree = hemlock();
        assert_eq!(layer_of(&tree, 50.0, 65.0), Layer::Canopy);
        assert_eq!(layer_of(&tree, 28.0, 65.0), Layer::Subcanopy);
        assert_eq!(layer_of(&tree, 8.0, 65.0), Layer::Shrub);
        assert_eq!(layer_of(&fern(), 1.0, 65.0), Layer::Herb);
    }

    #[test]
    fn floor_mosses_spread_fast_early_and_build_deep_wefts_late() {
        let kinds = crate::mosses::kinds(crate::library::Library::builtin()).expect("mosses");
        let named = |mix: &[(usize, f64)]| -> Vec<(String, f64)> {
            mix.iter()
                .map(|(k, share)| (kinds[*k].genus.clone(), *share))
                .collect()
        };
        let young = named(&floor_mosses(&kinds, 0.9, 0.5, 20.0, 3));
        let old = named(&floor_mosses(&kinds, 0.9, 0.5, 400.0, 3));
        assert_eq!(young[0].0, "Kindbergia", "{young:?}");
        assert_eq!(old[0].0, "Hylocomium", "{old:?}");
        for mix in [&young, &old] {
            let total: f64 = mix.iter().map(|(_, share)| share).sum();
            assert!((total - 1.0).abs() < 1e-9);
            // Wall and roof mosses keep off the forest floor.
            assert!(
                mix.iter()
                    .all(|(genus, _)| genus != "Grimmia" && genus != "Tortula")
            );
        }
        // A sunlit, dry floor fits none of the forest's mosses.
        let open = named(&floor_mosses(&kinds, 0.0, 0.15, 400.0, 3));
        assert!(
            open.iter().all(|(genus, _)| genus != "Rhytidiadelphus"),
            "{open:?}"
        );
    }

    #[test]
    fn floor_moss_grows_in_shade_on_moist_ground_under_needles() {
        // The Puget Trough's dense and thinned second growth (Thysell and
        // Carey 2001): 59% and 30%.
        let dense = ground_moss(0.9, 0.34, 0.5, 0.0);
        assert!((dense - 0.59).abs() < 0.03, "{dense}");
        let thinned = ground_moss(0.65, 0.88, 0.5, 0.0);
        assert!((thinned - 0.30).abs() < 0.04, "{thinned}");
        // Less in the open, on dry ground and under broad leaves.
        assert!(ground_moss(0.1, 0.34, 0.5, 0.0) < 0.1);
        assert!(ground_moss(0.9, 0.34, 0.12, 0.0) < 0.1);
        assert!(ground_moss(0.9, 0.34, 0.5, 1.0) < 0.5 * dense);
    }

    fn traits(json: &str) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_herbs_shoots_keep_the_year_its_traits_give() {
        // A deciduous herb with no months comes up in April and is down in
        // November, as a deciduous tree's leaves are by default.
        let summer = Shoots::of_herb(&traits(
            r#"{"woodiness": "herbaceous", "leaf_habit": {"deciduous": "usually"}}"#,
        ))
        .unwrap();
        assert_eq!(
            summer,
            Shoots {
                up: 101.0,
                down: 325.0
            }
        );
        assert!(summer.stand_on(PLOT_DAY) && !summer.stand_on(15.0) && !summer.stand_on(340.0));
        // A spring ephemeral is up from March to May, judged in April.
        let spring = Shoots::of_herb(&traits(
            r#"{"woodiness": "herbaceous", "leaf_habit": "deciduous",
                "leaf_out_months": [3], "leaf_fall_months": [5], "flowering_months": [3, 4]}"#,
        ))
        .unwrap();
        assert_eq!(
            spring,
            Shoots {
                up: 60.0,
                down: 142.0
            }
        );
        assert!(spring.stand_on(100.0) && !spring.stand_on(PLOT_DAY));
        assert!((spring.middle() - 101.0).abs() < 1e-9);
        // A winter-green herb of a dry summer's climate: up in November,
        // down in May of the next year.
        let winter = Shoots::of_herb(&traits(
            r#"{"woodiness": "herbaceous", "leaf_habit": "deciduous",
                "leaf_out_months": [11], "leaf_fall_months": [5]}"#,
        ))
        .unwrap();
        assert!(winter.stand_on(15.0) && winter.stand_on(320.0) && !winter.stand_on(PLOT_DAY));
        assert!(winter.middle() < 60.0, "{}", winter.middle());
        // Its most usual habit counts: a bracken dies back.
        let bracken = Shoots::of_herb(&traits(
            r#"{"woodiness": "herbaceous", "leaf_habit": {"deciduous": "usually", "evergreen": "rarely"}}"#,
        ));
        assert_eq!(bracken, Some(summer));
        // Evergreen herbs, woody plants and herbs whose habit is not given
        // stand all year.
        for json in [
            r#"{"woodiness": "herbaceous", "leaf_habit": {"deciduous": "usually", "evergreen": "usually"}}"#,
            r#"{"woodiness": "herbaceous", "leaf_habit": "evergreen"}"#,
            r#"{"woodiness": "woody", "leaf_habit": "deciduous"}"#,
            r#"{"woodiness": "herbaceous", "life_span": "perennial"}"#,
        ] {
            assert_eq!(Shoots::of_herb(&traits(json)), None, "{json}");
        }
        // An annual comes up in spring and is gone after it seeds.
        assert!(Shoots::of_herb(&traits(r#"{"life_span": "annual"}"#)).is_some());
    }

    #[test]
    fn a_bare_crown_lets_through_what_a_leafless_forest_does() {
        assert!((crown_light(1.0, LEAFY_CROWN_LIGHT) - LEAFY_CROWN_LIGHT).abs() < 1e-12);
        assert!((crown_light(0.0, LEAFY_CROWN_LIGHT) - BARE_CROWN_LIGHT).abs() < 1e-12);
        // Hudson et al. 2017: 1.3% beneath the canopy in full leaf, 31-39%
        // leafless; as deep a canopy of bare crowns lets that through.
        let depth = 0.013_f64.ln() / LEAFY_CROWN_LIGHT.ln();
        let bare = BARE_CROWN_LIGHT.powf(depth);
        assert!((0.31..0.40).contains(&bare), "{bare}");
    }

    #[test]
    fn a_broadleaf_stand_takes_the_hardwood_plots_covers() {
        let alder = Candidate {
            shade_tolerance: 0.2,
            layers: [1.0, 0.3, 0.0, 0.0],
            lifespan: 100.0,
            ages: vec![5.0, 15.0, 30.0, 60.0, 90.0],
            suitability: 1.0,
            hardwood: true,
        };
        // Closed at 30 years: the wet hardwood plots' covers, herbs twice
        // the conifers'.
        let alders = develop(std::slice::from_ref(&alder), 30.0);
        assert_eq!(alders.cover, HARDWOOD_LAYERS);
        assert!(alders.cover[3] > 1.5 * CLOSED_LAYERS[3]);
        // Conifers alone keep theirs.
        let firs = develop(&[fir()], 60.0);
        assert_eq!(firs.cover, CLOSED_LAYERS);
        // Both together cover in between, by the hardwoods' share.
        let both = [fir(), alder.clone()];
        let mixed = develop(&both, 60.0);
        let share = hardwood_share(&both, &mixed.cohorts);
        assert!((0.0..1.0).contains(&share) && share > 0.0, "{share}");
        let at = Stage::at(60.0, mixed.lifespan);
        assert_eq!(mixed.cover, at.cover(60.0, mixed.lifespan, share));
        assert!(mixed.cover[3] > at.cover(60.0, mixed.lifespan, 0.0)[3]);
        // Overtopped trees do not count toward the share.
        let overtopped: Vec<Cohort> = mixed
            .cohorts
            .iter()
            .filter(|cohort| cohort.overtopped)
            .cloned()
            .collect();
        assert_eq!(hardwood_share(&both, &overtopped), 0.0);
    }

    #[test]
    fn fires_keep_a_prairie_open_under_old_trees() {
        let oak = Candidate {
            shade_tolerance: 0.2,
            layers: [0.8, 0.6, 0.4, 0.0],
            lifespan: 500.0,
            ages: vec![8.0, 25.0, 50.0, 80.0, 120.0],
            suitability: 1.0,
            hardwood: true,
        };
        let pool = [oak, fir(), hemlock(), fern()];
        // Three years after a fire: old oaks and firs over the sod, and
        // no hemlock among them, which fire kills.
        let kept = develop_kept(&pool, 3.0, Origin::Natural);
        let veterans: Vec<&Cohort> = kept.cohorts.iter().filter(|c| c.veteran).collect();
        assert_eq!(veterans.len(), 2);
        assert!(veterans.iter().all(|c| c.candidate < 2 && c.age >= 80.0));
        assert!(kept.cover[0] >= VETERAN_COVER);
        let trees = kept.cover[0] + kept.cover[1];
        assert!(kept.cover[3] >= OPEN_HERB * (1.0 - trees) - 1e-9 && trees < 0.2);
        let burnt = develop_as(&pool, 3.0, Origin::Natural);
        assert!(
            kept.cover[3] > 1.5 * burnt.cover[3],
            "{:?} {:?}",
            kept.cover,
            burnt.cover
        );
        // Trees come into the sod more slowly than onto a clearing: still
        // open at 20 fire-free years, closed at 28 (LANDFIRE), where a
        // clearing's young conifers close at about 11.
        let trees_at = |since: f64| {
            let stand = develop_kept(&pool, since, Origin::Natural);
            stand.cover[0] + stand.cover[1]
        };
        let young = |since: f64| {
            let stand = develop_as(&pool, since, Origin::Natural);
            stand.cover[0] + stand.cover[1]
        };
        assert!(young(12.0) >= LANDFIRE_CLOSED, "{}", young(12.0));
        assert!(trees_at(20.0) < LANDFIRE_CLOSED, "{}", trees_at(20.0));
        assert!(trees_at(FIRE_FREE_CLOSURE) >= LANDFIRE_CLOSED - 1e-9);
        // Unburnt for 80 years it has closed as a stand 17 years younger
        // does, the old trees still in it.
        let closed = develop_kept(&pool, 80.0, Origin::Natural);
        let lag = 80.0 - encroached_age(80.0, pioneer_lifespan(&pool));
        assert!((lag - 17.2).abs() < 0.1, "{lag}");
        let forest = develop_as(&pool, 80.0 - lag, Origin::Natural);
        assert_eq!(closed.cover[0], forest.cover[0]);
        assert!(closed.cohorts.iter().any(|c| c.veteran));
        // Thatch builds back over about 11 years.
        assert_eq!(thatch_built(0.0), 0.0);
        assert!(thatch_built(3.0) > 0.5 && thatch_built(3.0) < 0.6);
        assert!(thatch_built(THATCH_YEARS) > 0.95);
        // Moss: 83% of a prairie's just after a fire, all of it ten years on.
        assert!((prairie_moss(0.0) - MOSS_AFTER_FIRE * PRAIRIE_MOSS).abs() < 1e-12);
        assert!(prairie_moss(PRAIRIE_MOSS_YEARS) > 0.95 * PRAIRIE_MOSS);
    }
}
