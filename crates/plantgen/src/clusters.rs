//! Clusters of flowers, heads, spikelets or fruit on one organ (spec
//! rework L1, LANGUAGE.md 5.4.2).
//!
//! An organ whose look has an `inflorescence` is one cluster: the look's
//! `shape` draws one unit of it, a flower, a head or one fruit, and the
//! cluster's [`layout`] places its units, their pedicels and its axes in
//! the organ's frame, with lengths in units of the organ's length, `y`
//! along the organ, `x` across its card and `z` out of it. The solid drawn
//! on the nearest levels ([`draw`]) and the card painted for the farther
//! ones (`crate::templates`) both come from it, so the card is the solid
//! seen face on.
//!
//! With $`n`$ = `flowers`, $`s`$ = `size`, $`\sigma`$ = `spread`, $`p`$ =
//! `pedicel`, $`y_0`$ = `peduncle`, $`\delta`$ = `divergence`, $`m`$ =
//! `per_node`, $`\tau`$ = `taper`, $`K = \lceil n/m \rceil`$ nodes and
//! $`q_k = (k + \tfrac12)/K`$, unit $`j`$ of node $`k`$ is turned
//! $`\psi = \theta + k\delta + 2\pi j/m`$ round the axis, $`\theta`$ the
//! cluster's turn, and scaled by $`t_k = 1 - \tau q_k`$:
//!
//! - a **raceme** sets node $`k`$ at $`y_k = (1 - r)(y_0 + (1 - y_0) q_k)`$
//!   with $`r = \min\big((p + s)\,t_{K-1}\cos\sigma,\ 0.95\big)`$, each unit
//!   on a pedicel $`p\,t_k`$ long leaning $`\sigma`$; a **spike** and a
//!   **catkin** take $`p = 0`$;
//! - a **corymb** sets its nodes at $`y_k = y_0 + (1 - s - y_0) q_k`$, each
//!   pedicel $`\max\big((1 - y_k)/\max(\cos\sigma, 0.2) - s\,t_k,\ 0\big)`$
//!   long, so every unit's tip is level with the organ's;
//! - an **umbel** sets its pedicels from $`y_u = y_0 (1 - s)`$, unit $`i`$
//!   ($`q_i = (i + \tfrac12)/n`$) leaning $`\alpha_i = \sigma\sqrt{q_i}`$,
//!   turned $`\theta + i\delta`$, on a pedicel
//!   $`\max\big((1 - y_u - \tfrac{w}{2}\sin\alpha_i)/\max(\cos\alpha_i, 0.2) - s,\ 0.3\,s\big)`$
//!   long, $`w`$ a unit's width over its length;
//! - a **head** packs its units on a Fibonacci sphere, pointing out, their
//!   bases on a sphere of radius
//!   $`\rho = \max\big(\tfrac{w}{2} s\sqrt{0.35\,n} - 0.3\,s,\ 0.02\big)`$
//!   centred at $`\max(1 - \rho - s,\ \rho)`$, so its top units reach 1;
//! - a **cyme** ends its axis in a unit; $`m`$ branches leave just below
//!   it, each turned $`\delta`$ on from its parent, leaning $`\sigma`$ from
//!   the cluster's axis and $`1 - \tau`$ as long as its parent, each
//!   ending in a unit and forking again, order by order, until $`n`$ units
//!   are placed (those of the last order spread evenly over its places);
//!   a **scorpioid** is a cyme forking once at each node, always to the
//!   same side, each internode turned $`\min(\sigma, \pi/(n - 1))`$ from
//!   the last and 0.9 as long, so its units lie on a coil. Both are scaled
//!   so that their highest unit reaches 1.
//!
//! Where `branches` is above 0 the axis bears that many branches in place
//! of units, placed as units would be, each laid out as `branch` in a frame
//! of its own, $`s`$ long, with its `y` along the branch and its `x` square
//! to it, away from the axis that bears it: a `panicle` is a raceme of
//! branches, a `compound_umbel` an umbel of them. A branch scaled by
//! $`t_k`$ bears that share of its units or branches, rounded and at least
//! one, each as long as on a branch of full length (spec rework P3): a
//! grass's upper panicle branches are shorter, with fewer spikelets, not
//! smaller ones. A cluster nests at most
//! [`LEVELS`] deep and holds at most [`MOST_UNITS`] units. With `droop`
//! $`D`$ an axis bends toward its `x` as it rises, its heading turned
//! $`D(2u - u^2)`$ at the share $`u`$ of its length, and each unit and
//! branch keeps its place relative to the axis where it leaves it. A
//! cluster bent so that it reaches past its organ's tip (a nodding
//! corymb's long lower pedicels turn upright) shrinks to fit its organ.
//!
//! Raceme, umbel and head with fruit units are the fruit clusters of
//! before L1, laid out by the same formulas; a `pair` of then is a raceme
//! with `per_node` 2 and `divergence` 90.
//!
//! Each unit has a rank, when it opens among the cluster's units, from 0
//! (the first) to 1 (the last): on its own axis, $`k/(K - 1)`$ of its
//! node $`k`$ for a raceme, spike, catkin, panicle or corymb (from the
//! bottom, a node's units together), $`1 - i/(n - 1)`$ for an umbel or a
//! compound umbel (from the rim), $`i/(n - 1)`$ for a head (from its
//! foot) and a scorpioid (along the coil), and its order over the last
//! order for a cyme (each axis's unit before its forks'); on a branch,
//! the mean of its ranks on every axis it is on. With a season that
//! opens over days (`crate::looks::Season`), a unit of rank $`r`$ is on
//! day $`d`$ in the stage of day $`d - r \cdot`$ `opening` and is drawn in
//! that stage's look ([`Opening`]): a bud, a flower, or the look's own
//! fruit, as unripe as its stage, or nothing but its pedicel.
//!
//! On the nearest levels each unit is a solid of its shape (a fruit as
//! `crate::fruit` draws one; a flower or a head reduced to its petals or
//! rays at two facets round a centre, with stamens only when it is a
//! centimetre across or more) and the pedicels and axes are thin prisms in
//! the cluster's `stalk` colour. A part mesh (`crate::parts`) that must
//! fit a budget takes up to [`crate::fruit::COARSER`] steps: petals at one
//! facet; flowers and heads as one flat hexagon each, three-sided stalks
//! and fruit at fewer facets; each smallest branch's units merged into one
//! solid round them; every branch merged. A cluster of spikelets draws
//! half its spikelets instead, each longer ([`thinned`]), then each
//! smallest branch as the outline of its spikelets, two kites crossed
//! along it ([`outlined`]). A cluster whose axis bears no branches stops
//! before its units merge ([`coarsest`]).

use crate::blooms::{FlowerForm, Form, Part};
use crate::fruit::{self, Detail, GOLDEN_DEGREES};
use crate::graph::GraphOrgan;
use crate::looks::{ClusterKind, Inflorescence, Look, Mount, Season, Shape, Stage, within};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{self, mix64};

/// Levels a cluster nests to: its own axis, its branches and theirs.
pub const LEVELS: usize = 3;

/// Most units one cluster holds.
pub const MOST_UNITS: u64 = 5000;

/// A cluster's stalks unless it sets a colour: green.
pub const STALK: [f32; 3] = [0.3, 0.38, 0.14];

/// Units of a cluster not laid along its axis (an umbel, a head, a cyme)
/// whose taxa do not count them.
pub const UNITS: u32 = 12;

/// The coarse step (`crate::parts`) from which a flower or head is one
/// flat hexagon and stalks are three-sided.
pub(crate) const FLAT: usize = 2;

/// The coarse step (`crate::parts`) from which units are merged into one
/// solid per smallest branch; the next merges every branch.
const MERGED: usize = 3;

/// Whether a part mesh of `cluster` drawn `coarse` steps coarser draws
/// half its units, each larger ([`THINNED_GROWTH`]): a cluster of
/// spikelets at [`MERGED`].
#[must_use]
pub fn thinned(cluster: &ClusterLook, coarse: usize) -> bool {
    matches!(cluster.unit, Unit::Spikelet { .. }) && coarse == MERGED
}

/// Whether a part mesh of `cluster` drawn `coarse` steps coarser draws
/// its smallest branches' outlines ([`Outline`]): a cluster of spikelets
/// beyond [`MERGED`].
#[must_use]
pub fn outlined(cluster: &ClusterLook, coarse: usize) -> bool {
    matches!(cluster.unit, Unit::Spikelet { .. }) && coarse > MERGED
}

/// How much longer a thinned cluster's units are drawn: as much as makes
/// up the area of those it leaves out.
const THINNED_GROWTH: f64 = std::f64::consts::SQRT_2;

/// Whether the unit at `index` of `organ`'s cluster is drawn when the
/// cluster is thinned: half of them, picked by hash.
fn kept(organ: &GraphOrgan, index: usize) -> bool {
    rng::unit(mix64(
        organ.id ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15),
    )) < 0.5
}

/// The coarsest step (`crate::parts`) a part mesh of `cluster` takes:
/// [`crate::fruit::COARSER`], or before [`MERGED`] for a cluster whose axis
/// bears no branches, whose units would merge into one solid that draws
/// it worse than its card.
#[must_use]
pub fn coarsest(cluster: &Cluster) -> usize {
    if cluster.branch.is_some() {
        fruit::COARSER
    } else {
        MERGED - 1
    }
}

impl ClusterKind {
    /// Whether the kind may bear branches in place of units.
    fn bears_branches(self) -> bool {
        !matches!(
            self,
            Self::Cyme | Self::Scorpioid | Self::Head | Self::Catkin
        )
    }

    /// Branches the kind bears unless set.
    fn default_branches(self) -> u32 {
        match self {
            Self::Panicle => 8,
            Self::CompoundUmbel => 12,
            _ => 0,
        }
    }

    /// Whether its units stand one node after another up its axis, so
    /// that how many fit depends on their size.
    fn along_its_axis(self) -> bool {
        matches!(
            self,
            Self::Raceme | Self::Spike | Self::Catkin | Self::Corymb
        )
    }

    /// The kind of its branches unless set.
    fn branch_kind(self) -> Self {
        match self {
            Self::Panicle => Self::Raceme,
            Self::CompoundUmbel => Self::Umbel,
            kind => kind,
        }
    }
}

/// An inflorescence with every default filled in (see
/// [`Inflorescence`] for the keys).
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    pub kind: ClusterKind,
    pub flowers: u32,
    pub branches: u32,
    /// How each branch is laid out: `Some` exactly when `branches` is
    /// above 0.
    pub branch: Option<Box<Cluster>>,
    pub per_node: u32,
    /// Degrees.
    pub divergence: f64,
    /// Degrees.
    pub spread: f64,
    pub pedicel: f64,
    pub peduncle: f64,
    pub size: f64,
    pub taper: f64,
    /// Degrees.
    pub droop: f64,
    pub stalk: [f32; 3],
    /// The spathe's length, a share of the axis; 0 where there is none
    /// (always on a branch).
    pub spathe: f64,
    pub spathe_width: f64,
    /// Degrees.
    pub spathe_hood: f64,
    pub spathe_cup: f64,
    /// The look's colour where unset.
    pub spathe_colour: Option<[f32; 3]>,
}

impl Cluster {
    /// The cluster `spec` describes, its defaults filled in.
    #[must_use]
    pub fn of(spec: &Inflorescence) -> Self {
        Self::filled(spec, ClusterKind::Raceme, None, STALK, 1)
    }

    /// `spec` at nesting `level`, taking `kind`, `flowers` and `stalk`
    /// unless it sets its own. Units left uncounted are counted from their
    /// size ([`Self::packed`]), and a raceme's flower whose stalk is left
    /// unset stands on one as long as itself.
    fn filled(
        spec: &Inflorescence,
        kind: ClusterKind,
        flowers: Option<u32>,
        stalk: [f32; 3],
        level: usize,
    ) -> Self {
        let kind = spec.kind.unwrap_or(kind);
        let set_flowers = spec.flowers.or(flowers);
        let stalk = spec.stalk.unwrap_or(stalk);
        let branches = if kind.bears_branches() && level < LEVELS {
            spec.branches.unwrap_or_else(|| kind.default_branches())
        } else {
            0
        };
        let empty = Inflorescence::default();
        let branch = (branches > 0).then(|| {
            Box::new(Self::filled(
                spec.branch.as_deref().unwrap_or(&empty),
                kind.branch_kind(),
                set_flowers,
                stalk,
                level + 1,
            ))
        });
        let bearing = branches > 0;
        let per_node = spec
            .per_node
            .unwrap_or(if kind == ClusterKind::Cyme { 2 } else { 1 });
        let peduncle = spec.peduncle.unwrap_or(
            if matches!(kind, ClusterKind::Umbel | ClusterKind::CompoundUmbel) {
                0.3
            } else {
                0.1
            },
        );
        let size = spec.size.unwrap_or(if bearing { 0.4 } else { 0.15 });
        let along = !bearing && kind.along_its_axis();
        let flowers = set_flowers.unwrap_or_else(|| {
            if along {
                Self::packed(size, peduncle, per_node)
            } else {
                UNITS
            }
        });
        Self {
            kind,
            flowers,
            branches,
            branch,
            per_node,
            divergence: spec.divergence.unwrap_or(GOLDEN_DEGREES),
            spread: spec.spread.unwrap_or(35.0),
            pedicel: spec.pedicel.unwrap_or(if bearing {
                0.0
            } else if along && kind == ClusterKind::Raceme {
                size
            } else {
                0.15
            }),
            peduncle,
            size,
            taper: spec.taper.unwrap_or(0.0),
            droop: spec.droop.unwrap_or(0.0),
            stalk,
            spathe: if level == 1 {
                spec.spathe.unwrap_or(0.0)
            } else {
                0.0
            },
            spathe_width: spec.spathe_width.unwrap_or(0.5),
            spathe_hood: spec.spathe_hood.unwrap_or(20.0),
            spathe_cup: spec.spathe_cup.unwrap_or(0.5),
            spathe_colour: spec.spathe_colour,
        }
    }

    /// The units of a cluster laid along its axis whose taxa do not count
    /// them: as many as stand half a unit's length apart over the axis
    /// above its peduncle, `per_node` at each node, 1 to 200. A unit 0.15
    /// of the axis above a peduncle of 0.1 gives 12, the count a cluster
    /// with neither set has always drawn; a raceme as long as its flora
    /// measures it, with flowers as wide as it measures them, is as
    /// crowded as that one, not 12 flowers spread along it.
    #[must_use]
    pub fn packed(size: f64, peduncle: f64, per_node: u32) -> u32 {
        let nodes = (2.0 * (1.0 - peduncle.clamp(0.0, 0.95)) / size.max(0.005)).round();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let nodes = nodes.max(1.0) as u32;
        (nodes * per_node.clamp(1, 8)).clamp(1, 200)
    }

    /// Units in the whole cluster.
    #[must_use]
    pub fn units(&self) -> u64 {
        match &self.branch {
            Some(branch) => self
                .shares()
                .into_iter()
                .map(|share| branch.shortened(share).units())
                .sum(),
            None => u64::from(self.flowers),
        }
    }

    /// The length of each unit or branch on the cluster's own axis, in
    /// order, as a share of `size`: $`t = 1 - \tau q`$ (see the module).
    fn shares(&self) -> Vec<f64> {
        let n = if self.branch.is_some() {
            self.branches
        } else {
            self.flowers
        }
        .max(1);
        let tau = self.taper.clamp(0.0, 1.0);
        let m = self.per_node.clamp(1, 8);
        let nodes = n.div_ceil(m);
        (0..n)
            .map(|i| {
                let q = match self.kind {
                    ClusterKind::Umbel | ClusterKind::CompoundUmbel => {
                        (f64::from(i) + 0.5) / f64::from(n)
                    }
                    _ => (f64::from(i / m) + 0.5) / f64::from(nodes),
                };
                1.0 - tau * q
            })
            .collect()
    }

    /// The cluster as a branch `share` as long as the longest (a tapered
    /// branch, spec rework P3): that share of its units or branches, at
    /// least one, each as long as on the longest branch.
    fn shortened(&self, share: f64) -> std::borrow::Cow<'_, Self> {
        if share >= 1.0 - 1e-9 {
            return std::borrow::Cow::Borrowed(self);
        }
        let share = share.max(1e-3);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let fewer = |n: u32| ((f64::from(n) * share).round() as u32).clamp(1, n.max(1));
        let mut short = self.clone();
        if short.branch.is_some() {
            short.branches = fewer(self.branches);
        } else {
            short.flowers = fewer(self.flowers);
        }
        short.size = (self.size / share).min(1.0);
        std::borrow::Cow::Owned(short)
    }

    /// Check an inflorescence as a spec gives it.
    ///
    /// # Errors
    ///
    /// Describes the first value outside its range, branches on a kind
    /// that bears none (or none on a panicle or compound umbel), nesting
    /// deeper than [`LEVELS`], or more than [`MOST_UNITS`] units.
    pub fn check(spec: &Inflorescence) -> Result<(), String> {
        Self::check_level(spec, ClusterKind::Raceme, 1)?;
        let units = Self::of(spec).units();
        if units > MOST_UNITS {
            return Err(format!(
                "an inflorescence holds at most {MOST_UNITS} units, found {units}"
            ));
        }
        Ok(())
    }

    fn check_level(spec: &Inflorescence, kind: ClusterKind, level: usize) -> Result<(), String> {
        let kind = spec.kind.unwrap_or(kind);
        let counts = [
            ("flowers", spec.flowers, 1.0, 200.0),
            ("branches", spec.branches, 0.0, 60.0),
            ("per_node", spec.per_node, 1.0, 8.0),
        ];
        for (key, value, low, high) in counts {
            if let Some(value) = value {
                within(&format!("inflorescence {key}"), f64::from(value), low, high)?;
            }
        }
        let values = [
            ("divergence", spec.divergence, 0.0, 180.0),
            ("spread", spec.spread, 0.0, 90.0),
            ("pedicel", spec.pedicel, 0.0, 1.0),
            ("peduncle", spec.peduncle, 0.0, 1.0),
            ("size", spec.size, 0.005, 1.0),
            ("taper", spec.taper, 0.0, 1.0),
            ("droop", spec.droop, 0.0, 180.0),
            ("spathe", spec.spathe, 0.0, 3.0),
            ("spathe_width", spec.spathe_width, 0.1, 1.5),
            ("spathe_hood", spec.spathe_hood, 0.0, 120.0),
            ("spathe_cup", spec.spathe_cup, 0.0, 1.0),
        ];
        for (key, value, low, high) in values {
            if let Some(value) = value {
                within(&format!("inflorescence {key}"), value, low, high)?;
            }
        }
        for channel in spec.stalk.into_iter().flatten() {
            within("inflorescence stalk", f64::from(channel), 0.0, 1.0)?;
        }
        for channel in spec.spathe_colour.into_iter().flatten() {
            within("inflorescence spathe_colour", f64::from(channel), 0.0, 1.0)?;
        }
        if level > 1 && spec.spathe.is_some_and(|spathe| spathe > 0.0) {
            return Err("only an inflorescence's own axis bears a spathe".to_string());
        }
        let branches = spec.branches.unwrap_or_else(|| kind.default_branches());
        if branches > 0 && !kind.bears_branches() {
            return Err(format!(
                "a {} bears no branches, found {branches}",
                kind.name()
            ));
        }
        if branches == 0 {
            if matches!(kind, ClusterKind::Panicle | ClusterKind::CompoundUmbel) {
                return Err(format!("a {} bears branches, 1 to 60", kind.name()));
            }
            if spec.branch.is_some() {
                return Err(
                    "inflorescence `branch` lays out branches, and it bears none".to_string(),
                );
            }
            return Ok(());
        }
        if level >= LEVELS {
            return Err(format!(
                "an inflorescence nests at most {LEVELS} levels deep: its axis, its branches \
                 and theirs"
            ));
        }
        let empty = Inflorescence::default();
        Self::check_level(
            spec.branch.as_deref().unwrap_or(&empty),
            kind.branch_kind(),
            level + 1,
        )
    }
}

/// What one unit of a cluster is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    /// A flower; a unit's length is its width.
    Flower,
    /// A head (a capitulum); a unit's length is its width.
    Head,
    /// One fruit or cone, `aspect` its width over its length, `unripe` the
    /// share of a cluster's fruit still unripe.
    Fruit { aspect: f64, unripe: f64 },
    /// A grass or sedge spikelet (`crate::spikelets`), `aspect` the width
    /// of its glumes and lemmas over its length, its awns reaching `beyond`
    /// its tip and spanning `across` its width, in its lengths.
    Spikelet {
        aspect: f64,
        beyond: f64,
        across: f64,
    },
}

impl Unit {
    /// The unit a look of `shape` draws, if it can be a cluster's.
    #[must_use]
    pub fn of(shape: &Shape) -> Option<Self> {
        match shape {
            Shape::Flower(_) => Some(Self::Flower),
            Shape::Head(_) => Some(Self::Head),
            Shape::Fruit(fruit) => Some(Self::Fruit {
                aspect: fruit.aspect,
                unripe: fruit.unripe,
            }),
            Shape::Spikelet(spikelet) => {
                let laid = crate::spikelets::layout(spikelet);
                let aspect = (2.0 * laid.body()).max(spikelet.aspect);
                Some(Self::Spikelet {
                    aspect,
                    beyond: laid.height() - 1.0,
                    across: (2.0 * laid.reach()).max(aspect),
                })
            }
            _ => None,
        }
    }

    /// One unit's width over its length: a spikelet's without its awns,
    /// which sets how thick the stalks that bear it are.
    #[must_use]
    pub fn width(self) -> f64 {
        match self {
            Self::Flower | Self::Head => 1.0,
            Self::Fruit { aspect, .. } | Self::Spikelet { aspect, .. } => aspect.clamp(0.05, 2.0),
        }
    }

    /// How thick, over its length, a unit draws the pedicel that bears it,
    /// as a share of its [`Self::width`]: a flower's pedicel is far
    /// thinner than the flower is wide (a brodiaea's or a rhododendron's 1
    /// to 2 mm under a flower 3 to 4 cm long), so it is drawn as for a
    /// unit 0.35 as wide; a head, a fruit and a spikelet are as wide as
    /// they are.
    #[must_use]
    pub fn stalk_width(self) -> f64 {
        match self {
            Self::Flower => 0.35,
            unit => unit.width(),
        }
    }

    /// How far across the unit spans over its length, a spikelet's awns
    /// included: what a card must hold.
    #[must_use]
    pub fn across(self) -> f64 {
        match self {
            Self::Spikelet { across, .. } => across.clamp(0.05, 4.0),
            unit => unit.width(),
        }
    }

    /// How far the unit reaches along its direction, in its lengths: past
    /// 1 where a spikelet's awns reach beyond its tip.
    #[must_use]
    pub fn reach(self) -> f64 {
        match self {
            Self::Spikelet { beyond, .. } => 1.0 + beyond.max(0.0),
            _ => 1.0,
        }
    }

    fn unripe(self) -> f64 {
        match self {
            Self::Fruit { unripe, .. } => unripe,
            Self::Flower | Self::Head | Self::Spikelet { .. } => 0.0,
        }
    }

    /// Whether the unit is seen face on: a flower or a head.
    #[must_use]
    pub fn faces(self) -> bool {
        matches!(self, Self::Flower | Self::Head)
    }
}

/// A look's cluster: its layout's keys, its unit and its card, and on a
/// day its units are in stages of their own, how each is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterLook {
    pub cluster: Cluster,
    pub unit: Unit,
    pub card: CardFrame,
    /// Its units in their own stages (spec rework L1b); `None` when every
    /// unit is drawn with the look's own shape and colours.
    pub opening: Option<Box<Opening>>,
}

impl ClusterLook {
    /// `cluster` of units drawn with `shape`, its card holding them and
    /// the units of `stages`, each a unit and its length as a share of the
    /// own unit's (a bud's, a flower's), that it shows on other days;
    /// `None` unless the shape is a flower, a head, a spikelet or a fruit.
    #[must_use]
    pub fn new(cluster: Cluster, shape: &Shape, stages: &[(Unit, f64)]) -> Option<Self> {
        let unit = Unit::of(shape)?;
        let card = CardFrame::of(&cluster, unit, stages);
        Some(Self {
            cluster,
            unit,
            card,
            opening: None,
        })
    }

    /// The cluster laid out turned `turn` radians round its axis, its
    /// unripe fruit chosen by `seed`.
    #[must_use]
    pub fn layout(&self, turn: f64, seed: u64) -> Layout {
        layout(&self.cluster, self.unit, turn, seed)
    }

    /// The cluster laid out as it is drawn on `organ`: turned, and its
    /// unripe fruit chosen, by the organ's id.
    #[must_use]
    pub fn laid_on(&self, organ: &GraphOrgan) -> Layout {
        let turn = rng::unit(mix64(organ.id ^ 0x00F2_0175)) * std::f64::consts::TAU;
        self.layout(turn, organ.id)
    }
}

/// A unit's look in one stage of its season (spec rework L1b): a bud's,
/// a flower's, or the look's own in fruit.
#[derive(Debug, Clone, PartialEq)]
pub struct StageUnit {
    pub shape: Shape,
    pub form: Form,
    pub colour: [f32; 3],
    pub accent: [f32; 3],
    /// Its length as a share of the look's own unit's.
    pub size: f64,
}

/// A cluster's units each in a stage of its own on one day (spec rework
/// L1b): a unit of rank `r` is in the stage its season gives on the day
/// less `r` times the season's `opening` ([`Season::unit_stage`]), and is
/// drawn in that stage's look.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    /// The organ's season, without its stage looks.
    pub season: Season,
    /// The day of the year.
    pub day: f64,
    /// A unit's look in bud, in flower and in fruit; `None` in a stage it
    /// shows nothing in, where its pedicel stays bare.
    pub looks: [Option<StageUnit>; 3],
}

impl Opening {
    /// The look unit `p` shows, and whether it is unripe fruit; `None` if
    /// it shows nothing.
    #[must_use]
    pub fn showing(&self, p: &Placed) -> Option<(&StageUnit, bool)> {
        let (look, unripe) = self.shown(p)?;
        self.looks[look].as_ref().map(|look| (look, unripe))
    }

    /// Which of [`Self::looks`] unit `p` shows, and whether it is unripe
    /// fruit; `None` if it shows nothing.
    #[must_use]
    pub fn shown(&self, p: &Placed) -> Option<(usize, bool)> {
        let (look, unripe) = match self.season.unit_stage(self.day, p.rank) {
            Stage::Gone => return None,
            Stage::Bud => (0, false),
            Stage::Flower => (1, false),
            Stage::Fruit { unripe } => (2, p.chance < unripe),
        };
        self.looks[look].is_some().then_some((look, unripe))
    }

    /// The two colours a card of `laid`'s units is painted in, a card
    /// carrying one colour and one accent: where every unit that shows is
    /// in one stage, that stage's colour and accent; else the colour most
    /// of their area shows, unripe fruit their accent, and the next. Each
    /// unit's colours are then matched as nearly as these two make them
    /// ([`blend`]).
    #[must_use]
    // Colours are told apart exactly: one stage's, or another's.
    #[allow(clippy::float_cmp)]
    pub fn slots(&self, laid: &Layout) -> ([f32; 3], [f32; 3]) {
        let mut stages = [false; 3];
        let mut areas: Vec<([f32; 3], f64)> = Vec::new();
        for p in &laid.units {
            let Some((stage, unripe)) = self.shown(p) else {
                continue;
            };
            stages[stage] = true;
            let Some(look) = &self.looks[stage] else {
                continue;
            };
            let colour = if unripe { look.accent } else { look.colour };
            let area = (p.length * look.size).powi(2);
            match areas.iter_mut().find(|(shown, _)| *shown == colour) {
                Some((_, sum)) => *sum += area,
                None => areas.push((colour, area)),
            }
        }
        let only = stages.iter().filter(|&&shown| shown).count() <= 1;
        let first = stages.iter().position(|&shown| shown).unwrap_or(2);
        if only && let Some(look) = &self.looks[first] {
            return (look.colour, look.accent);
        }
        // The largest first; the earliest of equals.
        let mut order: Vec<usize> = (0..areas.len()).collect();
        order.sort_by(|&a, &b| areas[b].1.total_cmp(&areas[a].1).then(a.cmp(&b)));
        match order.as_slice() {
            [] => ([0.5; 3], [0.5; 3]),
            [one] => (areas[*one].0, areas[*one].0),
            [one, two, ..] => (areas[*one].0, areas[*two].0),
        }
    }
}

/// How nearly `colour` is `a` times the first of `slots` plus `b` times
/// the second, `a` and `b` at least 0, in least squares: `(a, b)`.
#[must_use]
#[allow(clippy::float_cmp, clippy::many_single_char_names)]
pub fn blend(colour: [f32; 3], slots: ([f32; 3], [f32; 3])) -> (f64, f64) {
    if colour == slots.0 {
        return (1.0, 0.0);
    }
    if colour == slots.1 {
        return (0.0, 1.0);
    }
    let v = |c: [f32; 3]| Vec3::new(f64::from(c[0]), f64::from(c[1]), f64::from(c[2]));
    let (x, c, a) = (v(colour), v(slots.0), v(slots.1));
    let one = |basis: Vec3| {
        let norm = basis.dot(basis);
        if norm > 1e-12 {
            (x.dot(basis) / norm).max(0.0)
        } else {
            0.0
        }
    };
    let error = |p: f64, q: f64| (x - c * p - a * q).length();
    let (cc, aa, ca) = (c.dot(c), a.dot(a), c.dot(a));
    let det = cc * aa - ca * ca;
    if det > 1e-9 * cc.max(aa).max(1e-12) {
        let p = (x.dot(c) * aa - x.dot(a) * ca) / det;
        let q = (x.dot(a) * cc - x.dot(c) * ca) / det;
        if p >= 0.0 && q >= 0.0 {
            return (p, q);
        }
    }
    let (p, q) = (one(c), one(a));
    if error(p, 0.0) <= error(0.0, q) {
        (p, 0.0)
    } else {
        (0.0, q)
    }
}

/// How a cluster's card sits on its organ, in lengths of the organ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardFrame {
    /// A flat-topped cluster of flowers or heads (an umbel, a compound
    /// umbel, a corymb or a head) faces along its organ; every other
    /// stands along it with a crossing card.
    pub mount: Mount,
    /// Along the organ: where a standing card starts (0, or below the
    /// organ's foot where units hang lower), or where a facing card's
    /// centre is (its highest unit's base).
    pub foot: f64,
    /// The card's length: 1 (or the highest awn's tip, where spikelets'
    /// awns reach past the organ's) less `foot` for a standing card, twice the
    /// farthest any unit reaches from the axis for a facing one, × 1.04.
    pub length: f64,
    /// Width over length: 1 for a facing card, else twice the farthest any
    /// unit reaches across it, × 1.04, from 0.1 to 2.
    pub aspect: f64,
    /// The crossing card's width, relative: 1 for a standing card.
    pub cross: Option<f64>,
}

impl CardFrame {
    /// The card of `cluster`, laid out unturned, holding its units drawn
    /// as `unit` and as each of `stages` (see [`ClusterLook::new`]).
    #[must_use]
    pub fn of(cluster: &Cluster, unit: Unit, stages: &[(Unit, f64)]) -> Self {
        let laid = layout(cluster, unit, 0.0, 0);
        let flat_topped = matches!(
            cluster.kind,
            ClusterKind::Umbel
                | ClusterKind::CompoundUmbel
                | ClusterKind::Corymb
                | ClusterKind::Head
        );
        if unit.faces() && flat_topped {
            let top = laid
                .units
                .iter()
                .map(|p| p.base.y)
                .fold(f64::NEG_INFINITY, f64::max);
            let largest = stages.iter().map(|&(_, size)| size).fold(1.0, f64::max);
            let reach = laid
                .units
                .iter()
                .map(|p| {
                    math::sqrt(p.base.x * p.base.x + p.base.z * p.base.z) + 0.5 * p.length * largest
                })
                .fold(0.0, f64::max);
            return Self {
                mount: Mount::Facing,
                foot: if top.is_finite() { top } else { 1.0 },
                length: (2.0 * reach * 1.04).clamp(0.1, 4.0),
                aspect: 1.0,
                cross: None,
            };
        }
        let width = unit.across();
        let (mut low, mut high, mut reach) = (0.0_f64, 1.0_f64, 0.0_f64);
        for p in &laid.units {
            let tip = p.base + p.direction * (p.length * unit.reach());
            let half = p.length * width * 0.5;
            reach = reach.max(p.base.x.abs().max(tip.x.abs()) + half);
            low = low.min(p.base.y.min(tip.y) - half);
            high = high.max(tip.y);
            // As it shows on other days: a flower or a head round its
            // centre, a bud or fruit from its base.
            for &(other, size) in stages {
                let length = p.length * size;
                let tip = if other.faces() {
                    p.base
                } else {
                    p.base + p.direction * (length * other.reach())
                };
                let half = length * other.across() * 0.5;
                reach = reach.max(p.base.x.abs().max(tip.x.abs()) + half);
                low = low.min(p.base.y.min(tip.y) - half);
                high = high.max(tip.y);
            }
        }
        for stalk in &laid.stalks {
            reach = reach.max(stalk.from.x.abs().max(stalk.to.x.abs()));
            low = low.min(stalk.from.y.min(stalk.to.y));
        }
        for sheet in &laid.sheets {
            reach = reach.max(0.5 * sheet.width);
            low = low.min(sheet.foot.y);
            high = high.max(sheet.tip().y);
        }
        // Awns may reach past the organ's tip.
        let length = high - low;
        Self {
            mount: Mount::Along,
            foot: low,
            length,
            aspect: (2.0 * reach * 1.04 / length).clamp(0.1, 2.0),
            cross: Some(1.0),
        }
    }
}

/// One unit of a cluster, in the organ's frame (see the module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Where its pedicel leaves its axis: its base, if it has none.
    pub attach: Vec3,
    /// Its base: a fruit's, or a flower's or head's centre.
    pub base: Vec3,
    /// Unit vector from its base toward a fruit's tip, or the way a
    /// flower or head faces.
    pub direction: Vec3,
    /// Its length: a fruit's, or a flower's or head's width.
    pub length: f64,
    /// Whether it is still unripe.
    pub unripe: bool,
    /// When it opens among the cluster's units, from 0 (the first) to 1:
    /// the mean of its ranks on each axis it is on (see the module).
    pub rank: f64,
    /// Its draw from 0 to 1: it is unripe while the share unripe is above
    /// it.
    pub chance: f64,
    /// The smallest branch it is on: 0 for the cluster's own axis.
    pub group: u32,
    /// The branch of the cluster's own axis it is on: 0 for that axis.
    pub top: u32,
    /// Its pedicel's colour.
    pub stalk: [f32; 3],
    /// The heading of the axis that bears it, where it leaves that axis:
    /// a zygomorphic flower's adaxial side is toward it.
    pub bearing: Vec3,
}

/// An axis, a ray or a cyme's branch: a thin prism from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stalk {
    pub from: Vec3,
    pub to: Vec3,
    pub radius: f64,
    pub colour: [f32; 3],
    /// A piece of the cluster's own axis.
    pub axis: bool,
}

/// A bract at the foot of a cluster (spec rework E6b): a spathe, one
/// blade from `foot` up along `up`, behind the units on the side `back`
/// points to, cupped round them and arching over them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheet {
    pub foot: Vec3,
    pub up: Vec3,
    pub back: Vec3,
    pub length: f64,
    /// Its widest width.
    pub width: f64,
    /// Degrees its blade arches forward from foot to tip.
    pub hood: f64,
    /// How deeply it cups, 0 to 1.
    pub cup: f64,
    /// The look's colour where `None`.
    pub colour: Option<[f32; 3]>,
}

impl Sheet {
    /// Width along the sheet, a share of its widest, `u` from foot (0) to
    /// tip (1): narrow where it sheathes the axis, widest a little below
    /// its middle, drawn out to a point.
    #[must_use]
    pub fn width_at(u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        if u < SHEET_WIDEST {
            SHEET_BASE
                + (1.0 - SHEET_BASE) * math::sin(0.5 * std::f64::consts::PI * u / SHEET_WIDEST)
        } else {
            math::pow((1.0 - u) / (1.0 - SHEET_WIDEST), SHEET_TAPER).sqrt()
        }
    }

    /// The solid it is drawn with ([`crate::leaves::Leaf::draw`]).
    #[must_use]
    pub fn solid(&self) -> crate::leaves::SolidLeaf {
        crate::leaves::SolidLeaf {
            section: crate::leaves::Section::Channelled,
            thickness: 0.03,
            fold: self.cup,
            widest: SHEET_WIDEST,
            base: SHEET_BASE,
            taper: SHEET_TAPER,
            tip: 0.0,
            // The face toward the units, so a forward arch bends against
            // the leaf's own.
            arch: -self.hood,
            twist: 0.0,
            margin: None,
            teeth: None,
            spine: None,
            levels: 2,
        }
    }

    /// Where its tip is, the arch followed.
    #[must_use]
    pub fn tip(&self) -> Vec3 {
        let forward = -self.back;
        let hood = math::radians(self.hood);
        if hood < 1e-6 {
            return self.foot + self.up * self.length;
        }
        // A circular arc of length `length` turning `hood`.
        let radius = self.length / hood;
        self.foot
            + self.up * (radius * math::sin(hood))
            + forward * (radius * (1.0 - math::cos(hood)))
    }
}

/// Where a spathe is widest, a share of its length from its foot.
const SHEET_WIDEST: f64 = 0.45;
/// A spathe's width at its foot, a share of its widest.
const SHEET_BASE: f64 = 0.35;
/// How long its point is drawn out (see `crate::leaves::SolidLeaf::taper`).
const SHEET_TAPER: f64 = 1.4;
/// How far below its lowest unit a spathe's foot sheathes the axis, a
/// share of the spathe's length.
const SHEATH: f64 = 0.15;

/// A cluster's units, stalks and bracts, in the organ's frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub units: Vec<Placed>,
    pub stalks: Vec<Stalk>,
    pub sheets: Vec<Sheet>,
}

impl Layout {
    /// The cluster's length as `inflorescence_length_m` measures it (spec
    /// rework M8B), a share of its organ: along the organ's axis from its
    /// lowest unit or branch (where its pedicel or branch leaves an axis)
    /// to its farthest tip, a flower's or head's half its width beyond its
    /// centre, a fruit's or spikelet's its tip.
    #[must_use]
    pub fn length(&self, unit: Unit) -> f64 {
        let low = self.lowest();
        let high = self
            .units
            .iter()
            .map(|p| match unit {
                Unit::Flower | Unit::Head => p.base.y + 0.5 * p.length,
                Unit::Fruit { .. } | Unit::Spikelet { .. } => {
                    p.base.y.max((p.base + p.direction * p.length).y)
                }
            })
            .fold(f64::NEG_INFINITY, f64::max);
        if low.is_finite() && high.is_finite() {
            (high - low).max(0.0)
        } else {
            0.0
        }
    }

    /// The cluster's peduncle as `peduncle_length_m` measures it, a share
    /// of its organ: from the organ's base to where its lowest unit's
    /// pedicel or lowest branch leaves an axis; 0 for a cluster with none.
    #[must_use]
    pub fn peduncle(&self) -> f64 {
        let low = self.lowest();
        if low.is_finite() { low.max(0.0) } else { 0.0 }
    }

    /// The share of its organ where its lowest unit or branch leaves an
    /// axis; infinite for a cluster with none.
    fn lowest(&self) -> f64 {
        self.units
            .iter()
            .map(|p| p.attach.y)
            .chain(
                self.stalks
                    .iter()
                    .filter(|stalk| !stalk.axis)
                    .map(|stalk| stalk.from.y),
            )
            .fold(f64::INFINITY, f64::min)
    }
}

/// Where a cluster's own frame sits in the organ's: a point `p` of it is
/// `origin + (x p.x + y p.y + z p.z) scale`.
#[derive(Debug, Clone, Copy)]
struct Frame {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
    scale: f64,
    /// The organ's own frame, which leaves points as they are.
    own: bool,
}

impl Frame {
    const ORGAN: Self = Self {
        origin: Vec3::ZERO,
        x: Vec3::X,
        y: Vec3::Y,
        z: Vec3::Z,
        scale: 1.0,
        own: true,
    };

    fn point(&self, p: Vec3) -> Vec3 {
        if self.own {
            return p;
        }
        self.origin + (self.x * p.x + self.y * p.y + self.z * p.z) * self.scale
    }

    fn direction(&self, d: Vec3) -> Vec3 {
        if self.own {
            return d;
        }
        (self.x * d.x + self.y * d.y + self.z * d.z).normalize_or(self.y)
    }
}

/// A unit or a branch on an axis, in the axis's own frame.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// How far up the axis it leaves it.
    anchor: f64,
    attach: Vec3,
    base: Vec3,
    direction: Vec3,
    length: f64,
    /// Outward from the axis, square to it, the way it is turned.
    radial: Vec3,
    /// When it opens among the axis's slots, from 0 (first) to 1.
    rank: f64,
}

/// The slots of one axis, the stalks between them that are not pedicels
/// (a cyme's branches, a scorpioid's coil), each with the height its
/// group leaves the axis at, and the height the axis runs to.
struct Axis {
    slots: Vec<Slot>,
    stalks: Vec<(f64, Vec3, Vec3)>,
    top: f64,
}

/// The direction `angle` from the axis, turned `around` it.
fn lean(angle: f64, around: f64) -> Vec3 {
    let radial = Vec3::new(math::cos(around), 0.0, math::sin(around));
    (Vec3::Y * math::cos(angle) + radial * math::sin(angle)).normalize_or(Vec3::Y)
}

fn radial(around: f64) -> Vec3 {
    Vec3::new(math::cos(around), 0.0, math::sin(around))
}

/// `v` turned `angle` from the axis toward `x`, about `z`.
fn bend(v: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = (math::sin(angle), math::cos(angle));
    Vec3::new(v.x * cos + v.y * sin, v.y * cos - v.x * sin, v.z)
}

/// An axis drooping `droop` radians by its tip: where its point a share
/// `u` of its length up lies, and how far it has turned there.
struct Droop {
    droop: f64,
    points: Vec<Vec3>,
}

impl Droop {
    const STEPS: usize = 64;

    fn new(droop: f64) -> Self {
        let steps = to_f64(Self::STEPS);
        let mut points = vec![Vec3::ZERO];
        let mut at = Vec3::ZERO;
        for i in 0..Self::STEPS {
            let u = (to_f64(i) + 0.5) / steps;
            at += bend(Vec3::Y, Self::angle_of(droop, u)) * (1.0 / steps);
            points.push(at);
        }
        Self { droop, points }
    }

    fn angle_of(droop: f64, u: f64) -> f64 {
        droop * (2.0 * u - u * u)
    }

    fn angle(&self, u: f64) -> f64 {
        Self::angle_of(self.droop, u.clamp(0.0, 1.0))
    }

    fn at(&self, u: f64) -> Vec3 {
        let scaled = u.clamp(0.0, 1.0) * to_f64(Self::STEPS);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (scaled.floor() as usize).min(Self::STEPS - 1);
        let f = scaled - to_f64(i);
        self.points[i] + (self.points[i + 1] - self.points[i]) * f
    }

    /// `p`, laid out on the straight axis about the point `anchor` up it,
    /// carried to the bent axis.
    fn carry(&self, anchor: f64, p: Vec3) -> Vec3 {
        self.at(anchor) + bend(p - Vec3::Y * anchor, self.angle(anchor))
    }
}

fn to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn to_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The `k`th of `n` in order as a share from 0 to 1; 0 if it is the only
/// one.
fn share(k: u32, n: u32) -> f64 {
    if n > 1 {
        f64::from(k) / f64::from(n - 1)
    } else {
        0.0
    }
}

/// What [`layout`] has laid so far.
struct Laid {
    layout: Layout,
    seed: u64,
    unripe: f64,
    groups: u32,
}

/// Where the units of `cluster` sit, their shape `unit`, the cluster
/// turned `turn` radians round its axis and its unripe fruit chosen by
/// `seed` (see the module).
#[must_use]
pub fn layout(cluster: &Cluster, unit: Unit, turn: f64, seed: u64) -> Layout {
    let mut laid = Laid {
        layout: Layout::default(),
        seed,
        unripe: unit.unripe(),
        groups: 0,
    };
    lay(
        cluster,
        unit,
        turn,
        Frame::ORGAN,
        Vec3::Y,
        (0, 0, 0.0),
        1,
        &mut laid,
    );
    // A spathe: its foot round the axis a little below the lowest unit,
    // its blade behind the units.
    let mut layout = laid.layout;
    if cluster.spathe > 0.0 {
        let length = cluster.spathe;
        let lowest = layout
            .units
            .iter()
            .map(|p| p.attach.y)
            .fold(cluster.peduncle, f64::min);
        let radius = axis_radius(cluster, unit);
        layout.sheets.push(Sheet {
            foot: Vec3::new(0.0, (lowest - SHEATH * length).max(0.0), -radius),
            up: Vec3::Y,
            back: -Vec3::Z,
            length,
            width: length * cluster.spathe_width,
            hood: cluster.spathe_hood,
            cup: cluster.spathe_cup,
            colour: cluster.spathe_colour,
        });
    }
    // A bent cluster can reach past its organ's tip (a nodding corymb's
    // long lower pedicels turn upright), and a spathe past its units: it
    // shrinks to fit its organ.
    let high = layout
        .units
        .iter()
        .map(|p| p.base.y.max((p.base + p.direction * p.length).y))
        .chain(
            layout
                .stalks
                .iter()
                .map(|stalk| stalk.from.y.max(stalk.to.y)),
        )
        .chain(
            layout
                .sheets
                .iter()
                .map(|sheet| sheet.foot.y + sheet.length),
        )
        .fold(1.0, f64::max);
    if high > 1.0 + 1e-9 {
        let shrink = 1.0 / high;
        for p in &mut layout.units {
            p.attach = p.attach * shrink;
            p.base = p.base * shrink;
            p.length *= shrink;
        }
        for stalk in &mut layout.stalks {
            stalk.from = stalk.from * shrink;
            stalk.to = stalk.to * shrink;
            stalk.radius *= shrink;
        }
        for sheet in &mut layout.sheets {
            sheet.foot = sheet.foot * shrink;
            sheet.length *= shrink;
            sheet.width *= shrink;
        }
    }
    layout
}

/// Lay out `cluster` in `frame`, its axis heading along `heading` in the
/// organ's frame where it starts, its units in `group` of top branch
/// `top`, at nesting `level`, `above` the sum of its branch's ranks on the
/// axes that bear it.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lay(
    cluster: &Cluster,
    unit: Unit,
    turn: f64,
    frame: Frame,
    heading: Vec3,
    (group, top, above): (u32, u32, f64),
    level: usize,
    laid: &mut Laid,
) {
    let mut axis = axis(cluster, unit, turn);
    let radius = axis_radius(cluster, unit);
    let droop = (cluster.droop > 0.0).then(|| Droop::new(math::radians(cluster.droop.min(180.0))));
    // The axis itself: straight, or bent in eight pieces.
    if axis.top > 0.0 {
        let pieces: Vec<Vec3> = match &droop {
            Some(droop) => (0..=8)
                .map(|i| droop.at(axis.top * f64::from(i) / 8.0))
                .collect(),
            None => vec![Vec3::ZERO, Vec3::Y * axis.top],
        };
        for pair in pieces.windows(2) {
            laid.layout.stalks.push(Stalk {
                from: frame.point(pair[0]),
                to: frame.point(pair[1]),
                radius: radius * frame.scale,
                colour: cluster.stalk,
                axis: level == 1,
            });
        }
    }
    if let Some(droop) = &droop {
        for slot in &mut axis.slots {
            let angle = droop.angle(slot.anchor);
            slot.attach = droop.carry(slot.anchor, slot.attach);
            slot.base = droop.carry(slot.anchor, slot.base);
            slot.direction = bend(slot.direction, angle);
            slot.radial = bend(slot.radial, angle);
        }
        for (anchor, from, to) in &mut axis.stalks {
            *from = droop.carry(*anchor, *from);
            *to = droop.carry(*anchor, *to);
        }
    }
    let width = if cluster.branch.is_some() {
        1.0
    } else {
        unit.stalk_width()
    };
    for &(_, from, to) in &axis.stalks {
        laid.layout.stalks.push(Stalk {
            from: frame.point(from),
            to: frame.point(to),
            radius: 0.08 * cluster.size * width * frame.scale,
            colour: cluster.stalk,
            axis: false,
        });
    }
    let Some(branch) = &cluster.branch else {
        for slot in &axis.slots {
            let index = u64::try_from(laid.layout.units.len()).unwrap_or(u64::MAX);
            let chance = rng::unit(mix64(
                laid.seed ^ (index + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            ));
            laid.layout.units.push(Placed {
                attach: frame.point(slot.attach),
                base: frame.point(slot.base),
                direction: frame.direction(slot.direction),
                length: slot.length * frame.scale,
                unripe: chance < laid.unripe,
                rank: (above + slot.rank) / to_f64(level),
                chance,
                group,
                top,
                stalk: cluster.stalk,
                bearing: frame.direction(match &droop {
                    Some(droop) => bend(Vec3::Y, droop.angle(slot.anchor)),
                    None => Vec3::Y,
                }),
            });
        }
        return;
    };
    // Each branch: its ray or stalk, then the branch in its own frame,
    // shortened by the taper.
    let branch_radius = axis_radius(branch, unit) * cluster.size;
    let golden = math::radians(GOLDEN_DEGREES);
    let shares = cluster.shares();
    for (k, slot) in axis.slots.iter().enumerate() {
        let branch = branch.shortened(shares.get(k).copied().unwrap_or(1.0));
        let attach = frame.point(slot.attach);
        let origin = frame.point(slot.base);
        if (slot.base - slot.attach).length() > 1e-9 {
            laid.layout.stalks.push(Stalk {
                from: attach,
                to: origin,
                radius: branch_radius * frame.scale,
                colour: branch.stalk,
                axis: false,
            });
        }
        let y = frame.direction(slot.direction);
        // The bearing axis's heading where the branch leaves it.
        let along = match &droop {
            Some(droop) => frame.direction(bend(Vec3::Y, droop.angle(slot.anchor))),
            None => heading,
        };
        let x = (y * y.dot(along) - along).normalize_or(frame.direction(slot.radial));
        let x = (x - y * x.dot(y)).normalize_or(any_perpendicular(y));
        laid.groups += 1;
        let group = laid.groups;
        let top = if level == 1 { group } else { top };
        lay(
            &branch,
            unit,
            golden * to_f64(k + 1),
            Frame {
                origin,
                x,
                y,
                z: x.cross(y),
                scale: frame.scale * slot.length,
                own: false,
            },
            y,
            (group, top, above + slot.rank),
            level + 1,
            laid,
        );
    }
}

/// An axis's radius over its length: a tenth of the width of the units it
/// bears, or half again its branches' at their length.
fn axis_radius(cluster: &Cluster, unit: Unit) -> f64 {
    match &cluster.branch {
        Some(branch) => 1.5 * axis_radius(branch, unit) * cluster.size,
        None => cluster.size * unit.width() * 0.1,
    }
}

/// The slots of `cluster`'s own axis, unbent, turned `turn` (see the
/// module's formulas, whose names these follow).
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn axis(cluster: &Cluster, unit: Unit, turn: f64) -> Axis {
    let bearing = cluster.branch.is_some();
    let n = if bearing {
        cluster.branches
    } else {
        cluster.flowers
    }
    .max(1);
    let s = cluster.size.clamp(0.005, 1.0);
    let sigma = math::radians(cluster.spread.clamp(0.0, 90.0));
    let delta = math::radians(cluster.divergence.clamp(0.0, 180.0));
    let tau = cluster.taper.clamp(0.0, 1.0);
    let y0 = cluster.peduncle.clamp(0.0, 1.0);
    let w = if bearing { 1.0 } else { unit.width() };
    let m = cluster.per_node.clamp(1, 8);
    let nodes = n.div_ceil(m);
    let q = |k: u32| (f64::from(k) + 0.5) / f64::from(nodes);
    // Turned the node's divergence and, beyond the node's first, its
    // share of a turn.
    let around = |i: u32| {
        let (k, j) = (i / m, i % m);
        let mut around = turn + delta * f64::from(k);
        if j > 0 {
            around += std::f64::consts::TAU * f64::from(j) / f64::from(m);
        }
        around
    };
    let mut slots = Vec::with_capacity(to_usize(n));
    match cluster.kind {
        ClusterKind::Raceme | ClusterKind::Spike | ClusterKind::Catkin | ClusterKind::Panicle => {
            let p = if matches!(cluster.kind, ClusterKind::Spike | ClusterKind::Catkin) {
                0.0
            } else {
                cluster.pedicel.clamp(0.0, 1.0)
            };
            // The axis stops where the top unit, on its pedicel, reaches
            // the organ's length.
            let reach = ((p + s) * (1.0 - tau * q(nodes - 1)) * math::cos(sigma)).min(0.95);
            for i in 0..n {
                let qk = q(i / m);
                let t = 1.0 - tau * qk;
                let around = around(i);
                let attach = Vec3::Y * ((1.0 - reach) * (y0 + (1.0 - y0) * qk));
                let direction = lean(sigma, around);
                slots.push(Slot {
                    anchor: attach.y,
                    attach,
                    base: attach + direction * (p * t),
                    direction,
                    length: s * t,
                    radial: radial(around),
                    rank: share(i / m, nodes),
                });
            }
        }
        ClusterKind::Corymb => {
            for i in 0..n {
                let qk = q(i / m);
                let t = 1.0 - tau * qk;
                let around = around(i);
                let y = y0 + (1.0 - s - y0) * qk;
                // Long enough that every unit's tip is level with the
                // organ's.
                let stalk = ((1.0 - y) / math::cos(sigma).max(0.2) - s * t).max(0.0);
                let attach = Vec3::Y * y;
                let direction = lean(sigma, around);
                slots.push(Slot {
                    anchor: y,
                    attach,
                    base: attach + direction * stalk,
                    direction,
                    length: s * t,
                    radial: radial(around),
                    rank: share(i / m, nodes),
                });
            }
        }
        ClusterKind::Umbel | ClusterKind::CompoundUmbel => {
            let foot = y0 * (1.0 - s);
            for i in 0..n {
                let k = f64::from(i);
                let q = (k + 0.5) / f64::from(n);
                let length = s * (1.0 - tau * q);
                let angle = sigma * math::sqrt(q);
                // Long enough that the unit's far side reaches the organ's
                // length.
                let side = 0.5 * length * w * math::sin(angle);
                let stalk =
                    ((1.0 - foot - side) / math::cos(angle).max(0.2) - length).max(0.3 * length);
                let around = turn + delta * k;
                let attach = Vec3::Y * foot;
                let direction = lean(angle, around);
                slots.push(Slot {
                    anchor: foot,
                    attach,
                    base: attach + direction * stalk,
                    direction,
                    length,
                    radial: radial(around),
                    // The outermost first.
                    rank: 1.0 - share(i, n),
                });
            }
        }
        ClusterKind::Head => {
            let inner = (s * w * 0.5 * math::sqrt(0.35 * f64::from(n)) - 0.3 * s).max(0.02);
            let centre = Vec3::Y * (1.0 - inner - s).max(inner);
            for i in 0..n {
                let k = f64::from(i);
                let q = (k + 0.5) / f64::from(n);
                // From just above the stalk (y = -0.6) round to the top.
                let y = -0.6 + 1.6 * q;
                let ring = math::sqrt((1.0 - y * y).max(0.0));
                let around = turn + delta * k;
                let direction = Vec3::new(ring * math::cos(around), y, ring * math::sin(around));
                let base = centre + direction * inner;
                slots.push(Slot {
                    anchor: centre.y,
                    attach: base,
                    base,
                    direction,
                    length: s * (1.0 - tau * q),
                    radial: radial(around),
                    // From the foot of the head, its rim, to its top.
                    rank: share(i, n),
                });
            }
            return Axis {
                slots,
                stalks: Vec::new(),
                top: centre.y,
            };
        }
        ClusterKind::Cyme => return cyme(cluster, turn),
        ClusterKind::Scorpioid => return scorpioid(cluster, turn),
    }
    let top = slots.iter().map(|slot| slot.attach.y).fold(0.0, f64::max);
    Axis {
        slots,
        stalks: Vec::new(),
        top,
    }
}

/// Scale a cyme or scorpioid laid out with its first internode 1 long so
/// that its highest unit's tip reaches the organ's length: the share each
/// raw length is drawn at.
fn fit(units: &[(Vec3, Vec3, f64)], y0: f64) -> f64 {
    let room = 1.0 - y0;
    let scale = units
        .iter()
        .filter(|(at, ..)| at.y > 1e-9)
        .map(|&(at, direction, length)| (room - length * direction.y.max(0.0)) / at.y)
        .fold(f64::INFINITY, f64::min);
    if scale.is_finite() {
        scale.max(0.05 * room)
    } else {
        room
    }
}

/// A node of a cyme as first laid out, its first internode 1 long: its
/// place, the way its branch leans, its turn, its parent and its scale.
struct Node {
    at: Vec3,
    direction: Vec3,
    around: f64,
    parent: usize,
    scale: f64,
    order: u32,
}

/// A cyme (see the module): its units at the ends of its branches, which
/// are its stalks.
#[allow(clippy::many_single_char_names)]
fn cyme(cluster: &Cluster, turn: f64) -> Axis {
    let n = cluster.flowers.max(1);
    let s = cluster.size.clamp(0.005, 1.0);
    let sigma = math::radians(cluster.spread.clamp(0.0, 90.0));
    let delta = math::radians(cluster.divergence.clamp(0.0, 180.0));
    let keep = 1.0 - cluster.taper.clamp(0.0, 0.95);
    let y0 = cluster.peduncle.clamp(0.0, 1.0);
    let m = cluster.per_node.clamp(1, 8);
    let mut nodes = vec![Node {
        at: Vec3::Y,
        direction: Vec3::Y,
        around: turn,
        parent: 0,
        scale: 1.0,
        order: 0,
    }];
    let (mut first, mut count) = (0_usize, 1_usize);
    let (mut scale, mut order) = (1.0, 0);
    let (n, m) = (to_usize(n), to_usize(m));
    while nodes.len() < n {
        order += 1;
        let possible = count * m;
        let placed = possible.min(n - nodes.len());
        scale *= keep;
        let start = nodes.len();
        for c in 0..placed {
            // The last order's units spread evenly over its places.
            let place = if placed == possible {
                c
            } else {
                (2 * c + 1) * possible / (2 * placed)
            };
            let parent = first + place / m;
            let around = nodes[parent].around
                + delta
                + std::f64::consts::TAU * to_f64(place % m) / to_f64(m);
            let direction = lean(sigma, around);
            nodes.push(Node {
                at: nodes[parent].at + direction * scale,
                direction,
                around,
                parent,
                scale,
                order,
            });
        }
        first = start;
        count = placed;
    }
    let units: Vec<(Vec3, Vec3, f64)> = nodes
        .iter()
        .map(|node| (node.at, node.direction, s * node.scale))
        .collect();
    let f = fit(&units, y0);
    let place = |at: Vec3| Vec3::Y * y0 + at * f;
    let anchor = y0 + f;
    // Each axis's own unit before its forks': order by order.
    let orders = nodes.last().map_or(0, |node| node.order) + 1;
    let slots = nodes
        .iter()
        .map(|node| {
            let at = place(node.at);
            Slot {
                anchor,
                attach: at,
                base: at,
                direction: node.direction,
                length: s * node.scale,
                radial: radial(node.around),
                rank: share(node.order, orders),
            }
        })
        .collect();
    let stalks = nodes
        .iter()
        .skip(1)
        .map(|node| (anchor, place(nodes[node.parent].at), place(node.at)))
        .collect();
    Axis {
        slots,
        stalks,
        top: anchor,
    }
}

/// A scorpioid cyme (see the module): its units along a coil, which is
/// its stalk.
fn scorpioid(cluster: &Cluster, turn: f64) -> Axis {
    let n = cluster.flowers.max(1);
    let s = cluster.size.clamp(0.005, 1.0);
    let sigma = math::radians(cluster.spread.clamp(0.0, 90.0));
    let tau = cluster.taper.clamp(0.0, 1.0);
    let y0 = cluster.peduncle.clamp(0.0, 1.0);
    let turning = sigma.min(math::PI / f64::from((n - 1).max(1)));
    let out = radial(turn);
    let mut raw = Vec::with_capacity(to_usize(n));
    let (mut at, mut heading, mut step) = (Vec3::ZERO, 0.0_f64, 1.0);
    for k in 0..n {
        let along = out * math::sin(heading) + Vec3::Y * math::cos(heading);
        at += along * step;
        // On the coil's outer side, leaning the spread from it.
        let outward = Vec3::Y * math::sin(heading) - out * math::cos(heading);
        let direction = (along * math::cos(sigma) + outward * math::sin(sigma)).normalize_or(along);
        let q = (f64::from(k) + 0.5) / f64::from(n);
        raw.push((at, direction, s * (1.0 - tau * q)));
        heading += turning;
        step *= 0.9;
    }
    let f = fit(&raw, y0);
    let place = |at: Vec3| Vec3::Y * y0 + at * f;
    let anchor = place(raw[0].0).y;
    // Along the coil, from its foot.
    let slots = raw
        .iter()
        .zip(0..n)
        .map(|(&(at, direction, length), k)| {
            let at = place(at);
            Slot {
                anchor,
                attach: at,
                base: at,
                direction,
                length,
                radial: out,
                rank: share(k, n),
            }
        })
        .collect();
    let stalks = raw
        .windows(2)
        .map(|pair| (anchor, place(pair[0].0), place(pair[1].0)))
        .collect();
    Axis {
        slots,
        stalks,
        top: anchor,
    }
}

/// Draw the cluster `cluster` of `look` on `organ`, its `axis` and `side`
/// unit vectors, into `mesh`, if its form draws a solid: ripe units in
/// `colour`, unripe fruit, cups and husks and flower centres in the
/// look's accent darkened with the organ as its colour is, stalks in the
/// cluster's stalk colour; the part's `coarse` steps (up to
/// [`crate::fruit::COARSER`]) coarser.
pub(crate) fn draw(
    organ: &GraphOrgan,
    (axis, side): (Vec3, Vec3),
    look: &Look,
    cluster: &ClusterLook,
    colour: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    if organ.size <= 0.0 || !look.form.draws_on(&look.shape) || matches!(look.form, Form::Card) {
        return;
    }
    let (first, first_index) = (mesh.positions.len(), mesh.indices.len());
    let accent = crate::blooms::with_shading(look.accent, look.colour, colour);
    draw_laid(
        organ,
        (axis, side),
        look,
        cluster,
        &cluster.laid_on(organ),
        (colour, accent),
        part,
        mesh,
    );
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// One unit as it is drawn: its look's shape and form, its colours as
/// painted on the organ, its length as a share of the unit's own, and
/// whether it is unripe fruit.
struct Shown<'a> {
    shape: &'a Shape,
    form: &'a Form,
    colour: [f32; 3],
    accent: [f32; 3],
    size: f64,
    unripe: bool,
}

impl Shown<'_> {
    /// Its width over its length, and whether it is seen face on.
    fn extent(&self) -> (f64, bool) {
        Unit::of(self.shape).map_or((1.0, false), |unit| (unit.width(), unit.faces()))
    }
}

/// Draw `laid`, a layout of `cluster`, on `organ` (see [`draw`]).
#[allow(clippy::too_many_arguments, clippy::too_many_lines, clippy::float_cmp)]
fn draw_laid(
    organ: &GraphOrgan,
    (axis, side): (Vec3, Vec3),
    look: &Look,
    cluster: &ClusterLook,
    laid: &Layout,
    (colour, accent): ([f32; 3], [f32; 3]),
    part: Part,
    mesh: &mut Mesh,
) {
    let length = organ.size;
    let other = axis.cross(side);
    let to_world = |p: Vec3| {
        organ.position + side * (p.x * length) + axis * (p.y * length) + other * (p.z * length)
    };
    let to_direction = |d: Vec3| (side * d.x + axis * d.y + other * d.z).normalize_or(axis);
    // A stage's colours darkened with the organ as the look's colour is.
    let painted = |stage: [f32; 3]| {
        if stage == look.colour {
            colour
        } else {
            crate::blooms::with_shading(stage, look.colour, colour)
        }
    };
    let shown: Vec<Option<Shown>> = laid
        .units
        .iter()
        .map(|p| match &cluster.opening {
            None => Some(Shown {
                shape: &look.shape,
                form: &look.form,
                colour,
                accent,
                size: 1.0,
                unripe: p.unripe,
            }),
            Some(opening) => opening.showing(p).map(|(stage, unripe)| Shown {
                shape: &stage.shape,
                form: &stage.form,
                colour: painted(stage.colour),
                accent: painted(stage.accent),
                size: stage.size,
                unripe,
            }),
        })
        .collect();
    for stalk in &laid.stalks {
        let reach = (stalk.to - stalk.from).length() * length;
        if reach > 1e-9 {
            let start = to_world(stalk.from);
            let along = (to_world(stalk.to) - start).normalize_or(axis);
            fruit::stalk_between(
                start,
                along,
                reach,
                stalk.radius * length,
                stalk.colour,
                part,
                mesh,
            );
        }
    }
    // A spathe, at every step: it is what shows of the cluster from afar.
    for sheet in &laid.sheets {
        let leaf = part.leaf(
            to_world(sheet.foot),
            to_direction(sheet.up),
            // So the leaf's face (heading × left) is toward the units.
            to_direction(sheet.back.cross(sheet.up)),
            sheet.length * length,
            sheet.width * length,
            sheet.colour.map_or(colour, painted),
        );
        let (stations, across) = crate::leaves::detail_of(
            part.coarse.min(1),
            sheet.length * length,
            sheet.width * length,
        );
        let (stations, across) = if part.coarse >= FLAT {
            (stations.min(4), 1)
        } else {
            (stations, across)
        };
        leaf.draw(&sheet.solid(), stations, across, mesh);
    }
    let thinned = thinned(cluster, part.coarse);
    if outlined(cluster, part.coarse) {
        outlines(laid, &shown, part, &to_world, colour, mesh);
        return;
    }
    if part.coarse >= MERGED && !thinned {
        merged(laid, &shown, part, &to_world, (axis, side), colour, mesh);
        return;
    }
    // A thinned cluster draws the units it keeps as one step coarser
    // draws them, each longer for those it leaves out; their stalks as
    // its own step does.
    let stalk_part = part;
    let (part, growth) = if thinned {
        (Part { coarse: 1, ..part }, THINNED_GROWTH)
    } else {
        (part, 1.0)
    };
    let width = cluster.unit.stalk_width();
    let detail = Detail::for_count(
        u32::try_from(laid.units.len()).unwrap_or(u32::MAX),
        part.coarse,
    );
    for (index, (p, shown)) in laid.units.iter().zip(&shown).enumerate() {
        if thinned && !kept(organ, index) {
            continue;
        }
        let base = to_world(p.base);
        let direction = to_direction(p.direction);
        let unit_length = p.length * length * growth;
        let stalk = (p.base - p.attach).length() * length;
        if stalk > 1e-6 {
            let start = to_world(p.attach);
            let along = (base - start).normalize_or(direction);
            fruit::stalk_between(
                start,
                along,
                stalk,
                unit_length * width * 0.07,
                p.stalk,
                stalk_part,
                mesh,
            );
        }
        let Some(shown) = shown else {
            continue;
        };
        let unit_length = unit_length * shown.size;
        // Each unit turned its own way about its direction.
        let spin = rng::unit(mix64(
            organ.id ^ p.base.x.to_bits() ^ p.base.z.to_bits().rotate_left(17),
        )) * std::f64::consts::TAU;
        match (shown.shape, shown.form) {
            (Shape::Fruit(shape), Form::Fruit(form)) => fruit::draw_unit(
                organ,
                shape,
                form,
                (base, direction, unit_length),
                shown.unripe,
                detail,
                shown.colour,
                shown.accent,
                part,
                mesh,
            ),
            (Shape::Flower(flower), Form::Flower(form)) if !flower.is_plain() => {
                // Laid out whorl by whorl (`crate::flowers`), facing the
                // axis that bears it where it is zygomorphic.
                let first = mesh.positions.len();
                let first_index = mesh.indices.len();
                crate::flowers::draw_reduced(
                    &crate::flowers::layout(flower, form, unit_length * 0.5, spin),
                    crate::flowers::Frame::new(
                        base,
                        direction,
                        to_direction(p.bearing),
                        any_perpendicular(direction),
                        flower.is_zygomorphic(),
                    ),
                    crate::flowers::Colours {
                        petal: shown.colour,
                        calyx: crate::blooms::with_shading(
                            flower.calyx.unwrap_or(crate::flowers::CALYX),
                            look.colour,
                            colour,
                        ),
                        accent: shown.accent,
                        filament: flower.filament.unwrap_or(shown.accent),
                        ovary: flower.pistil.unwrap_or(shown.accent),
                    },
                    part,
                    mesh,
                );
                crate::leaves::smooth_normals(mesh, first, first_index);
            }
            (Shape::Flower(flower), Form::Flower(form)) => floret(
                &Petals {
                    count: flower.petals.max(1),
                    width: flower.petal_share(),
                    centre: flower.centre,
                    start: 0.6,
                },
                form,
                (base, direction, unit_length, spin),
                shown.colour,
                shown.accent,
                part,
                mesh,
            ),
            // A head reduced (`crate::capitula`), or from the FLAT step
            // one hexagon as a flower is.
            (Shape::Head(head), Form::Flower(form)) if part.coarse < FLAT => {
                crate::capitula::draw_reduced(
                    head,
                    form,
                    crate::capitula::Frame {
                        foot: base,
                        axis: direction,
                        side: any_perpendicular(direction),
                        radius: unit_length * 0.5,
                    },
                    spin,
                    crate::capitula::Colours::of(
                        head,
                        shown.colour,
                        shown.accent,
                        (look.colour, colour),
                    ),
                    part,
                    mesh,
                );
            }
            (Shape::Spikelet(spikelet), Form::Spikelet(_)) => crate::spikelets::draw(
                spikelet,
                crate::capitula::Frame {
                    foot: base,
                    axis: direction,
                    side: any_perpendicular(direction).rotate_about(direction, spin),
                    radius: unit_length,
                },
                crate::spikelets::Colours::of(shown.colour, shown.accent),
                part,
                mesh,
            ),
            (Shape::Head(head), Form::Flower(form)) => floret(
                &Petals {
                    count: head.rays,
                    width: head.ray_width,
                    centre: head.disc,
                    start: 0.85,
                },
                form,
                (base, direction, unit_length, spin),
                shown.colour,
                shown.accent,
                part,
                mesh,
            ),
            _ => {}
        }
    }
}

/// A floret's petals, or a head's rays, round its centre.
struct Petals {
    count: u32,
    /// Petal width over the width of its share of the circle.
    width: f64,
    /// The centre's radius over the floret's.
    centre: f64,
    /// Where petals start, as a share of the centre's radius.
    start: f64,
}

/// A plain flower ([`crate::looks::Flower::is_plain`]) or a head reduced
/// for a cluster: its petals as kites (one facet each from the first
/// coarse step), seen from both sides, round a low pyramid of a centre in
/// the accent colour; from the [`FLAT`] step one flat hexagon, seen from
/// both sides. A flower with stamens is drawn by
/// [`crate::flowers::draw_reduced`].
#[allow(clippy::too_many_arguments)]
fn floret(
    petals: &Petals,
    form: &FlowerForm,
    (base, direction, width, spin): (Vec3, Vec3, f64, f64),
    colour: [f32; 3],
    accent: [f32; 3],
    part: Part,
    mesh: &mut Mesh,
) {
    let radius = width * 0.5;
    if radius <= 0.0 {
        return;
    }
    let first = mesh.positions.len();
    let first_index = mesh.indices.len();
    let centre = (petals.centre * radius).clamp(0.0, radius * 0.9);
    let start = petals.start * centre;
    let length = (radius - start).max(radius * 0.1);
    let n = f64::from(petals.count.max(1));
    let petal_width =
        (petals.width * std::f64::consts::PI * (start + radius) / n).clamp(length * 0.08, length);
    let cup = math::radians(form.cup);
    let across = any_perpendicular(direction);
    let other = direction.cross(across);
    let leaf = part.leaf(base, direction, across, 0.0, 0.0, colour);
    let both = |corners: &[Vec3], colour: [f32; 3], mesh: &mut Mesh| {
        // A flat face seen from both sides: each side its own vertices,
        // so their normals are not averaged away.
        for flip in [false, true] {
            let at = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for &corner in corners {
                mesh.push(leaf.vertex(corner, colour));
            }
            for k in 1..u32::try_from(corners.len()).unwrap_or(2) - 1 {
                if flip {
                    mesh.indices.extend_from_slice(&[at, at + k + 1, at + k]);
                } else {
                    mesh.indices.extend_from_slice(&[at, at + k, at + k + 1]);
                }
            }
        }
    };
    if part.coarse >= FLAT {
        // One hexagon round the tips of its petals, or round its centre
        // where it has none.
        let (ring, rise, paint) = if petals.count == 0 {
            (centre, 0.0, accent)
        } else {
            let reach = length * 0.85;
            (
                start + reach * math::cos(cup),
                reach * math::sin(cup),
                colour,
            )
        };
        let corners: Vec<Vec3> = (0..6)
            .map(|k| {
                let angle = spin + std::f64::consts::TAU * f64::from(k) / 6.0;
                base + (across * math::cos(angle) + other * math::sin(angle)) * ring
                    + direction * rise
            })
            .collect();
        if ring > 0.0 {
            both(&corners, paint, mesh);
        }
        crate::leaves::smooth_normals(mesh, first, first_index);
        return;
    }
    for k in 0..petals.count {
        let angle = spin + std::f64::consts::TAU * f64::from(k) / n;
        let radial = across * math::cos(angle) + other * math::sin(angle);
        let heading = (radial * math::cos(cup) + direction * math::sin(cup)).normalize_or(radial);
        let left = direction.cross(radial).normalize_or(across) * (petal_width * 0.5);
        let foot = base + radial * start;
        if part.coarse == 0 {
            let widest = foot + heading * (length * 0.55);
            both(
                &[foot, widest + left, foot + heading * length, widest - left],
                colour,
                mesh,
            );
        } else {
            let end = foot + heading * (length * 0.85);
            both(&[foot, end + left, end - left], colour, mesh);
        }
    }
    if centre > 0.0 {
        // A low six-sided pyramid.
        let apex = mesh.push(leaf.vertex(base + direction * (centre * 0.5), accent));
        let ring = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        for k in 0..6 {
            let angle = spin + std::f64::consts::TAU * f64::from(k) / 6.0;
            let radial = across * math::cos(angle) + other * math::sin(angle);
            mesh.push(leaf.vertex(base + radial * centre, accent));
        }
        for k in 0..6 {
            mesh.indices
                .extend_from_slice(&[apex, ring + k, ring + (k + 1) % 6]);
        }
    }
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// The coarsest steps but a cluster of spikelets': each smallest branch's units (at [`MERGED`]) or
/// each branch of the cluster's own axis (beyond it) merged into one
/// ellipsoid round the units that show, in the colour most of them are,
/// the organ's `colour` where as many are in another.
#[allow(clippy::float_cmp)]
fn merged(
    laid: &Layout,
    shown: &[Option<Shown>],
    part: Part,
    to_world: &dyn Fn(Vec3) -> Vec3,
    (axis, side): (Vec3, Vec3),
    colour: [f32; 3],
    mesh: &mut Mesh,
) {
    type Group = (u32, Vec3, Vec3, Vec<([f32; 3], usize)>);
    let mut groups: Vec<Group> = Vec::new();
    for (p, shown) in laid.units.iter().zip(shown) {
        let Some(shown) = shown else {
            continue;
        };
        let key = if part.coarse > MERGED { p.top } else { p.group };
        let (width, faces) = shown.extent();
        let length = p.length * shown.size;
        let tip = if faces {
            p.base
        } else {
            p.base + p.direction * length
        };
        let half = length * width * 0.5;
        let reach = Vec3::new(half, half, half);
        let (low, high) = (p.base.min(tip) - reach, p.base.max(tip) + reach);
        let painted = if shown.unripe {
            shown.accent
        } else {
            shown.colour
        };
        let at = groups
            .iter()
            .position(|group| group.0 == key)
            .unwrap_or_else(|| {
                groups.push((key, low, high, vec![(colour, 0)]));
                groups.len() - 1
            });
        let group = &mut groups[at];
        group.1 = group.1.min(low);
        group.2 = group.2.max(high);
        if let Some((_, count)) = group.3.iter_mut().find(|(seen, _)| *seen == painted) {
            *count += 1;
        } else {
            group.3.push((painted, 1));
        }
    }
    let length = (to_world(Vec3::Y) - to_world(Vec3::ZERO)).length();
    for (_, low, high, colours) in groups {
        let half = (high - low) * 0.5;
        // The most; the organ's colour, listed first, of equals.
        let most = colours.iter().fold(
            colours[0],
            |most, &next| if next.1 > most.1 { next } else { most },
        );
        fruit::spheroid(
            to_world((low + high) * 0.5),
            (half.y * length, half.x * length, half.z * length),
            Detail::merged(),
            axis,
            side,
            most.0,
            part,
            mesh,
        );
    }
}

/// Share of a spikelet's length times its width it covers: a lance,
/// widest below the middle.
const SPIKELET_FILL: f64 = 0.7;

/// The coarsest steps of a cluster of spikelets: each smallest branch's
/// spikelets drawn as their [`Outline`].
#[allow(clippy::float_cmp)]
fn outlines(
    laid: &Layout,
    shown: &[Option<Shown>],
    part: Part,
    to_world: &dyn Fn(Vec3) -> Vec3,
    colour: [f32; 3],
    mesh: &mut Mesh,
) {
    let mut outlines: Vec<Outline> = Vec::new();
    for (p, shown) in laid.units.iter().zip(shown) {
        let Some(shown) = shown else {
            continue;
        };
        let key = p.group;
        let (width, _) = shown.extent();
        let length = p.length * shown.size;
        let painted = if shown.unripe {
            shown.accent
        } else {
            shown.colour
        };
        let at = outlines
            .iter()
            .position(|outline| outline.key == key)
            .unwrap_or_else(|| {
                outlines.push(Outline {
                    key,
                    ends: Vec::new(),
                    along: Vec3::ZERO,
                    area: 0.0,
                    widest: 0.0,
                    colours: vec![(colour, 0)],
                });
                outlines.len() - 1
            });
        let outline = &mut outlines[at];
        outline.ends.push(p.base);
        outline.ends.push(p.base + p.direction * length);
        outline.along += p.direction * length;
        outline.area += length * length * width * SPIKELET_FILL;
        outline.widest = outline.widest.max(length * width);
        if let Some((_, count)) = outline
            .colours
            .iter_mut()
            .find(|(seen, _)| *seen == painted)
        {
            *count += 1;
        } else {
            outline.colours.push((painted, 1));
        }
    }
    for outline in &outlines {
        outline.draw(part, to_world, mesh);
    }
}

/// The outline of a branch's spikelets: two kites crossed along the
/// branch, one across the way they spread most and one across the way
/// they spread least, each as wide as covers its share of the spikelets'
/// own area and no wider than they spread, in the colour most of them are.
struct Outline {
    key: u32,
    /// Each spikelet's base and tip.
    ends: Vec<Vec3>,
    /// The spikelets' lengths along their directions, summed.
    along: Vec3,
    /// The spikelets' area, and the widest.
    area: f64,
    widest: f64,
    colours: Vec<([f32; 3], usize)>,
}

impl Outline {
    fn draw(&self, part: Part, to_world: &dyn Fn(Vec3) -> Vec3, mesh: &mut Mesh) {
        let along = self.along.normalize_or(Vec3::Y);
        let count = crate::texture::to_f64(self.ends.len());
        let centre = self.ends.iter().fold(Vec3::ZERO, |sum, &end| sum + end) * (1.0 / count);
        // The spikelets' span along the branch, and the moments of their
        // spread across it, in a plane square to it.
        let first = any_perpendicular(along);
        let second = along.cross(first);
        let (mut low, mut high) = (0.0_f64, 0.0_f64);
        let (mut firsts, mut seconds, mut both) = (0.0, 0.0, 0.0);
        for &end in &self.ends {
            let offset = end - centre;
            let height = offset.dot(along);
            low = low.min(height);
            high = high.max(height);
            let (out, over) = (offset.dot(first), offset.dot(second));
            firsts += out * out;
            seconds += over * over;
            both += out * over;
        }
        let span = high - low;
        if span <= 1e-9 {
            return;
        }
        // The ways the spikelets spread most and least, and how far.
        let turn = 0.5 * libm::atan2(2.0 * both, firsts - seconds);
        let wide = first * libm::cos(turn) + second * libm::sin(turn);
        let narrow = along.cross(wide);
        let reach = |axis: Vec3| {
            self.ends
                .iter()
                .map(|&end| (end - centre).dot(axis).abs())
                .fold(0.0, f64::max)
        };
        let spreads = [reach(wide), reach(narrow)];
        let total = spreads[0] + spreads[1] + self.widest;
        let most =
            self.colours.iter().fold(
                self.colours[0],
                |most, &next| {
                    if next.1 > most.1 { next } else { most }
                },
            );
        let foot = centre + along * low;
        let tip = centre + along * high;
        let middle = centre + along * f64::midpoint(low, high);
        for (axis, spread) in [wide, narrow].into_iter().zip(spreads) {
            // Each kite's share of the spikelets' area as they spread.
            let share = (spread + 0.5 * self.widest) / total;
            let width = (2.0 * self.area * share / span).min(2.0 * spread + self.widest);
            let half = axis * (0.5 * width);
            let corners = [foot, middle + half, tip, middle - half].map(to_world);
            crate::capitula::both(part, &corners, most.0, mesh);
        }
    }
}

/// Triangles one organ of `look`, a cluster's, takes drawn as a solid:
/// counted by drawing one; 0 for a look that is not a cluster.
#[must_use]
pub fn triangles(look: &Look) -> usize {
    let Some(cluster) = &look.inflorescence else {
        return 0;
    };
    let organ = GraphOrgan {
        id: 1,
        organ: 0,
        segment: None,
        position: Vec3::ZERO,
        heading: Vec3::Y,
        left: Vec3::X,
        size: 1.0,
        born: 0.0,
        shed: None,
        light: 1.0,
        within: None,
        adult: None,
    };
    let mut mesh = Mesh::default();
    draw(
        &organ,
        (Vec3::Y, Vec3::X),
        look,
        cluster,
        [0.5; 3],
        Part {
            born: 0.0,
            shed: None,
            coarse: 0,
        },
        &mut mesh,
    );
    mesh.triangle_count()
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    /// A cluster that leaves its units uncounted holds as many as its size
    /// fits along its axis: the old 12 at the old size, more for smaller
    /// units, and a raceme's flowers stand on stalks as long as they are.
    #[test]
    fn uncounted_units_fill_their_axis_by_their_size() {
        let plain = Cluster::of(&Inflorescence::default());
        assert_eq!(plain.flowers, 12);
        assert_eq!(plain.pedicel, 0.15);
        // A lupine's raceme: flowers 13 mm in a raceme 37.5 cm long.
        let lupine = Cluster::of(&Inflorescence {
            size: Some(0.035),
            ..Inflorescence::default()
        });
        assert_eq!(lupine.flowers, 51);
        assert_eq!(lupine.pedicel, 0.035);
        let whorled = Cluster::of(&Inflorescence {
            size: Some(0.035),
            per_node: Some(3),
            ..Inflorescence::default()
        });
        assert_eq!(whorled.flowers, 153);
        // A count the taxa give stands, and a branch takes its cluster's.
        let counted = Cluster::of(&Inflorescence {
            size: Some(0.035),
            flowers: Some(20),
            ..Inflorescence::default()
        });
        assert_eq!(counted.flowers, 20);
        let panicle = Cluster::of(&Inflorescence {
            kind: Some(ClusterKind::Panicle),
            flowers: Some(5),
            branch: Some(Box::new(Inflorescence {
                size: Some(0.05),
                ..Inflorescence::default()
            })),
            ..Inflorescence::default()
        });
        assert_eq!(panicle.branch.unwrap().flowers, 5);
        // An umbel's units are not laid along an axis.
        let umbel = Cluster::of(&Inflorescence {
            kind: Some(ClusterKind::Umbel),
            size: Some(0.05),
            ..Inflorescence::default()
        });
        assert_eq!(umbel.flowers, 12);
    }
    use crate::looks::{Flower, Fruit, FruitKind, Head};

    fn spec(kind: ClusterKind, flowers: u32) -> Inflorescence {
        Inflorescence {
            kind: Some(kind),
            flowers: Some(flowers),
            ..Inflorescence::default()
        }
    }

    fn berries() -> Unit {
        Unit::Fruit {
            aspect: 0.8,
            unripe: 0.4,
        }
    }

    /// Today's fruit layouts as `crate::fruit` laid them out before L1.
    #[allow(clippy::many_single_char_names)]
    fn before(kind: &str, n: u32, s: f64, spread: f64, aspect: f64, turn: f64) -> Vec<Placed> {
        let sigma = math::radians(spread);
        let golden = math::radians(GOLDEN_DEGREES);
        let lean = |angle: f64, around: f64| -> Vec3 {
            let radial = Vec3::new(math::cos(around), 0.0, math::sin(around));
            (Vec3::Y * math::cos(angle) + radial * math::sin(angle)).normalize_or(Vec3::Y)
        };
        (0..n)
            .map(|i| {
                let k = f64::from(i);
                let q = (k + 0.5) / f64::from(n);
                let (attach, direction, stalk) = match kind {
                    "raceme" => {
                        let reach = (1.9 * s * math::cos(sigma)).min(0.9);
                        (
                            Vec3::Y * ((1.0 - reach) * (0.1 + 0.9 * q)),
                            lean(sigma, turn + golden * k),
                            s * 0.9,
                        )
                    }
                    "umbel" => {
                        let foot = 0.3 * (1.0 - s);
                        let angle = sigma * math::sqrt(q);
                        let side = 0.5 * s * aspect.clamp(0.05, 2.0) * math::sin(angle);
                        let stalk =
                            ((1.0 - foot - side) / math::cos(angle).max(0.2) - s).max(0.3 * s);
                        (Vec3::Y * foot, lean(angle, turn + golden * k), stalk)
                    }
                    "head" => {
                        let width = s * aspect.clamp(0.05, 2.0);
                        let inner =
                            (width * 0.5 * math::sqrt(0.35 * f64::from(n)) - 0.3 * s).max(0.02);
                        let centre = Vec3::Y * (1.0 - inner - s).max(inner);
                        let y = -0.6 + 1.6 * q;
                        let ring = math::sqrt((1.0 - y * y).max(0.0));
                        let around = turn + golden * k;
                        let direction =
                            Vec3::new(ring * math::cos(around), y, ring * math::sin(around));
                        (centre + direction * inner, direction, 0.0)
                    }
                    _ => {
                        let pairs = n.div_ceil(2);
                        let pair = f64::from(i / 2);
                        let other = if i % 2 == 0 { 0.0 } else { math::PI };
                        let reach = (1.25 * s * math::cos(sigma)).min(0.95);
                        (
                            Vec3::Y * ((1.0 - reach) * (pair + 1.0) / f64::from(pairs)),
                            lean(sigma, turn + std::f64::consts::FRAC_PI_2 * pair + other),
                            s * 0.25,
                        )
                    }
                };
                Placed {
                    attach,
                    base: attach + direction * stalk,
                    direction,
                    length: s,
                    unripe: false,
                    rank: 0.0,
                    chance: 0.0,
                    group: 0,
                    top: 0,
                    stalk: STALK,
                    bearing: Vec3::Y,
                }
            })
            .collect()
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn the_fruit_clusters_of_before_lay_out_as_they_did() {
        let unit = Unit::Fruit {
            aspect: 0.95,
            unripe: 0.0,
        };
        let cases = [
            (
                "raceme",
                Inflorescence {
                    kind: Some(ClusterKind::Raceme),
                    flowers: Some(14),
                    size: Some(0.15),
                    spread: Some(40.0),
                    pedicel: Some(0.135),
                    ..Inflorescence::default()
                },
                14,
                0.15,
                40.0,
            ),
            (
                "umbel",
                Inflorescence {
                    kind: Some(ClusterKind::Umbel),
                    flowers: Some(16),
                    size: Some(0.28),
                    spread: Some(55.0),
                    ..Inflorescence::default()
                },
                16,
                0.28,
                55.0,
            ),
            (
                "head",
                Inflorescence {
                    kind: Some(ClusterKind::Head),
                    flowers: Some(30),
                    size: Some(0.25),
                    ..Inflorescence::default()
                },
                30,
                0.25,
                35.0,
            ),
            (
                "pair",
                Inflorescence {
                    kind: Some(ClusterKind::Raceme),
                    flowers: Some(2),
                    per_node: Some(2),
                    divergence: Some(90.0),
                    pedicel: Some(0.2),
                    peduncle: Some(1.0),
                    size: Some(0.8),
                    spread: Some(20.0),
                    ..Inflorescence::default()
                },
                2,
                0.8,
                20.0,
            ),
        ];
        for (kind, spec, n, s, spread) in cases {
            for turn in [0.0, 1.3] {
                let now = layout(&Cluster::of(&spec), unit, turn, 5);
                let then = before(kind, n, s, spread, 0.95, turn);
                assert_eq!(now.units.len(), then.len(), "{kind}");
                for (a, b) in now.units.iter().zip(&then) {
                    assert!(close(a.attach, b.attach), "{kind}: {a:?} {b:?}");
                    assert!(close(a.base, b.base), "{kind}: {a:?} {b:?}");
                    assert!(close(a.direction, b.direction), "{kind}: {a:?} {b:?}");
                    assert!((a.length - b.length).abs() < 1e-12, "{kind}");
                }
            }
        }
    }

    #[test]
    fn a_longer_peduncle_raises_a_clusters_lowest_unit() {
        let at = |peduncle| {
            let spec = Inflorescence {
                kind: Some(ClusterKind::Raceme),
                flowers: Some(10),
                size: Some(0.1),
                spread: Some(30.0),
                pedicel: Some(0.1),
                peduncle: Some(peduncle),
                ..Inflorescence::default()
            };
            layout(&Cluster::of(&spec), Unit::Flower, 0.0, 5)
        };
        let (short, long) = (at(0.1), at(0.5));
        assert!(
            long.peduncle() > short.peduncle() + 0.2,
            "{} {}",
            short.peduncle(),
            long.peduncle()
        );
        for cluster in [&short, &long] {
            assert!(cluster.peduncle() + cluster.length(Unit::Flower) <= 1.0 + 1e-9);
        }
    }

    #[test]
    fn every_kind_places_its_units_inside_its_organ() {
        for kind in ClusterKind::ALL {
            let compound = matches!(kind, ClusterKind::Panicle | ClusterKind::CompoundUmbel);
            for (flowers, droop) in [(1, 0.0), (7, 0.0), (40, 0.0), (12, 90.0), (12, 180.0)] {
                let cluster = Cluster::of(&Inflorescence {
                    droop: Some(droop),
                    ..spec(kind, flowers)
                });
                for unit in [berries(), Unit::Flower] {
                    let laid = layout(&cluster, unit, 0.7, 3);
                    let expected = if compound {
                        u64::from(cluster.branches) * u64::from(flowers)
                    } else {
                        u64::from(flowers)
                    };
                    assert_eq!(laid.units.len() as u64, expected, "{kind:?} x {flowers}");
                    assert_eq!(cluster.units(), expected);
                    for p in &laid.units {
                        let tip = p.base + p.direction * p.length;
                        for point in [p.attach, p.base, tip] {
                            assert!(point.is_finite(), "{kind:?}: {p:?}");
                            assert!(
                                point.y <= 1.06 && point.y >= -1.06,
                                "{kind:?} x {flowers}, droop {droop}: {p:?}"
                            );
                            assert!(
                                point.x.abs() <= 1.5 && point.z.abs() <= 1.5,
                                "{kind:?} x {flowers}, droop {droop}: {p:?}"
                            );
                        }
                        assert!((p.direction.length() - 1.0).abs() < 1e-9, "{kind:?}");
                        assert!(p.length > 0.0);
                    }
                    if droop == 0.0 && flowers > 1 && !matches!(kind, ClusterKind::Head) {
                        // Upright clusters reach about the organ's length.
                        let top = laid
                            .units
                            .iter()
                            .map(|p| p.base.y.max((p.base + p.direction * p.length).y))
                            .fold(0.0, f64::max);
                        assert!(top > 0.7, "{kind:?} x {flowers}: reaches {top}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_corymbs_tops_are_level() {
        let cluster = Cluster::of(&Inflorescence {
            size: Some(0.12),
            ..spec(ClusterKind::Corymb, 15)
        });
        let laid = layout(&cluster, berries(), 0.0, 0);
        let tops: Vec<f64> = laid
            .units
            .iter()
            .map(|p| (p.base + p.direction * p.length).y)
            .collect();
        let (low, high) = tops
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &t| (l.min(t), h.max(t)));
        assert!(high - low < 0.1 * high, "{low}..{high}");
    }

    #[test]
    fn racemes_turn_by_the_golden_angle_and_spikes_are_sessile() {
        let raceme = layout(
            &Cluster::of(&spec(ClusterKind::Raceme, 5)),
            berries(),
            0.0,
            0,
        );
        let azimuth = |p: &Placed| math::atan2(p.direction.z, p.direction.x);
        let turned = (azimuth(&raceme.units[1]) - azimuth(&raceme.units[0]))
            .rem_euclid(std::f64::consts::TAU);
        assert!((turned - math::radians(GOLDEN_DEGREES)).abs() < 1e-9);
        let spike = layout(
            &Cluster::of(&spec(ClusterKind::Spike, 5)),
            berries(),
            0.0,
            0,
        );
        assert!(spike.units.iter().all(|p| p.attach == p.base));
        // Pairs: two units a node, the next pair crossing them.
        let pairs = layout(
            &Cluster::of(&Inflorescence {
                per_node: Some(2),
                divergence: Some(90.0),
                ..spec(ClusterKind::Raceme, 4)
            }),
            berries(),
            0.0,
            0,
        );
        assert_eq!(pairs.units[0].attach, pairs.units[1].attach);
        assert!(pairs.units[0].direction.dot(pairs.units[1].direction) < 0.5);
        assert!(pairs.units[2].attach.y > pairs.units[0].attach.y);
    }

    #[test]
    fn a_tapered_panicles_upper_branches_bear_fewer_units_as_large() {
        // Spec rework P3: a grass's upper branches are shorter, with fewer
        // spikelets of the same size.
        let cluster = Cluster::of(&Inflorescence {
            branches: Some(4),
            taper: Some(0.6),
            size: Some(0.4),
            branch: Some(Box::new(Inflorescence {
                flowers: Some(10),
                size: Some(0.1),
                ..Inflorescence::default()
            })),
            ..spec(ClusterKind::Panicle, 10)
        });
        // Branch k is 1 - 0.6 (k + 0.5) / 4 long: 0.925, 0.775, 0.625 and
        // 0.475 of the longest, bearing that share of its 10 units.
        assert_eq!(cluster.units(), 9 + 8 + 6 + 5);
        let laid = layout(&cluster, Unit::Flower, 0.0, 0);
        assert_eq!(laid.units.len(), 28);
        for top in 1..=4 {
            let on: Vec<&Placed> = laid.units.iter().filter(|p| p.top == top).collect();
            assert_eq!(on.len(), [9, 8, 6, 5][top as usize - 1]);
            // Every unit 0.1 of a full branch, 0.4 of the organ, long.
            for p in on {
                assert!((p.length - 0.04).abs() < 1e-12, "{p:?}");
            }
        }
        // A branch too short for its units' size bears one, as long as the
        // branch.
        let short = Cluster::of(&Inflorescence {
            branches: Some(2),
            taper: Some(1.0),
            branch: Some(Box::new(Inflorescence {
                flowers: Some(3),
                size: Some(0.5),
                ..Inflorescence::default()
            })),
            ..spec(ClusterKind::Panicle, 3)
        });
        assert_eq!(short.units(), 2 + 1);
    }

    #[test]
    fn compound_clusters_nest_their_branches() {
        let cluster = Cluster::of(&Inflorescence {
            branches: Some(5),
            ..spec(ClusterKind::CompoundUmbel, 9)
        });
        assert_eq!(
            cluster.branch.as_ref().map(|b| b.kind),
            Some(ClusterKind::Umbel)
        );
        let laid = layout(&cluster, Unit::Flower, 0.2, 0);
        assert_eq!(laid.units.len(), 45);
        let groups: std::collections::BTreeSet<u32> = laid.units.iter().map(|p| p.group).collect();
        assert_eq!(groups.len(), 5, "an umbellet a ray");
        assert!(laid.units.iter().all(|p| p.group == p.top));
        // A panicle of cymes: a thyrse, three levels at most.
        let thyrse = Inflorescence {
            branches: Some(4),
            branch: Some(Box::new(Inflorescence {
                kind: Some(ClusterKind::Cyme),
                ..Inflorescence::default()
            })),
            ..spec(ClusterKind::Panicle, 7)
        };
        assert!(Cluster::check(&thyrse).is_ok());
        assert_eq!(Cluster::of(&thyrse).units(), 28);
        let too_deep = Inflorescence {
            branch: Some(Box::new(Inflorescence {
                kind: Some(ClusterKind::Panicle),
                branch: Some(Box::new(Inflorescence {
                    kind: Some(ClusterKind::Panicle),
                    ..Inflorescence::default()
                })),
                ..Inflorescence::default()
            })),
            ..spec(ClusterKind::Panicle, 3)
        };
        assert!(
            Cluster::check(&too_deep)
                .unwrap_err()
                .contains("nests at most 3")
        );
    }

    #[test]
    fn checks_name_what_is_wrong() {
        let wrong = |spec: Inflorescence| Cluster::check(&spec).unwrap_err();
        assert!(wrong(spec(ClusterKind::Raceme, 0)).contains("flowers"));
        assert!(
            wrong(Inflorescence {
                branches: Some(3),
                ..spec(ClusterKind::Cyme, 5)
            })
            .contains("a cyme bears no branches")
        );
        assert!(
            wrong(Inflorescence {
                branches: Some(0),
                ..spec(ClusterKind::Panicle, 5)
            })
            .contains("a panicle bears branches")
        );
        assert!(
            wrong(Inflorescence {
                branch: Some(Box::default()),
                ..spec(ClusterKind::Raceme, 5)
            })
            .contains("`branch`")
        );
        assert!(
            wrong(Inflorescence {
                branches: Some(60),
                ..spec(ClusterKind::Panicle, 200)
            })
            .contains("at most 5000 units")
        );
        assert!(
            wrong(Inflorescence {
                spread: Some(120.0),
                ..Inflorescence::default()
            })
            .contains("spread")
        );
        assert!(Cluster::check(&Inflorescence::default()).is_ok());
        let raceme = Cluster::of(&Inflorescence::default());
        assert_eq!(
            (raceme.kind, raceme.flowers, raceme.size),
            (ClusterKind::Raceme, 12, 0.15)
        );
    }

    #[test]
    fn cymes_fork_order_by_order_and_scorpioids_coil() {
        let cyme = layout(
            &Cluster::of(&spec(ClusterKind::Cyme, 7)),
            Unit::Flower,
            0.0,
            0,
        );
        assert_eq!(cyme.units.len(), 7);
        // The first unit ends the axis; two branches fork below each unit.
        assert!(cyme.units[0].base.x.abs() < 1e-12);
        assert_eq!(cyme.stalks.len(), 1 + 6);
        let coil = layout(
            &Cluster::of(&Inflorescence {
                spread: Some(40.0),
                ..spec(ClusterKind::Scorpioid, 9)
            }),
            Unit::Flower,
            0.0,
            0,
        );
        let xs: Vec<f64> = coil.units.iter().map(|p| p.base.x).collect();
        assert!(
            xs.windows(2).all(|pair| pair[1] >= pair[0] - 1e-12),
            "{xs:?}"
        );
    }

    #[test]
    fn units_open_in_their_kinds_order() {
        let laid = |kind: ClusterKind, flowers: u32, unit: Unit| {
            layout(&Cluster::of(&spec(kind, flowers)), unit, 0.3, 0)
        };
        let ranks = |layout: &Layout| layout.units.iter().map(|p| p.rank).collect::<Vec<_>>();
        // From the bottom: a raceme's lowest first, its top last.
        let raceme = laid(ClusterKind::Raceme, 9, berries());
        for pair in raceme.units.windows(2) {
            assert!(pair[1].attach.y > pair[0].attach.y && pair[1].rank > pair[0].rank);
        }
        assert_eq!(ranks(&raceme).first(), Some(&0.0));
        assert_eq!(ranks(&raceme).last(), Some(&1.0));
        // Units at one node open together.
        let pairs = layout(
            &Cluster::of(&Inflorescence {
                per_node: Some(2),
                ..spec(ClusterKind::Raceme, 6)
            }),
            berries(),
            0.0,
            0,
        );
        for pair in pairs.units.chunks(2) {
            assert_eq!(pair[0].rank, pair[1].rank);
        }
        // From the rim: an umbel's outermost first, a head's from its foot.
        let umbel = laid(ClusterKind::Umbel, 8, Unit::Flower);
        let out = |p: &Placed| p.direction.y;
        for pair in umbel.units.windows(2) {
            assert!(out(&pair[1]) < out(&pair[0]) && pair[1].rank < pair[0].rank);
        }
        let head = laid(ClusterKind::Head, 20, Unit::Flower);
        for pair in head.units.windows(2) {
            assert!(pair[1].direction.y > pair[0].direction.y && pair[1].rank > pair[0].rank);
        }
        // From the centre: a cyme's own unit before its forks', order by
        // order; along the coil, a scorpioid's.
        let cyme = laid(ClusterKind::Cyme, 7, Unit::Flower);
        assert_eq!(ranks(&cyme), vec![0.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0]);
        let coil = laid(ClusterKind::Scorpioid, 5, Unit::Flower);
        assert_eq!(ranks(&coil), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        // Nested: the mean of its ranks on each axis, so a panicle's lowest
        // branch's foot opens first and its top branch's tip last.
        let panicle = layout(
            &Cluster::of(&Inflorescence {
                branches: Some(4),
                ..spec(ClusterKind::Panicle, 5)
            }),
            berries(),
            0.0,
            0,
        );
        let ranks = ranks(&panicle);
        assert_eq!(ranks.len(), 20);
        assert_eq!(ranks[0], 0.0);
        assert_eq!(ranks[19], 1.0);
        assert!((ranks[4] - 0.5).abs() < 1e-12 && (ranks[15] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn two_colours_match_a_stage_as_nearly_as_they_can() {
        let (white, purple) = ([0.9, 0.9, 0.85], [0.2, 0.05, 0.25]);
        assert_eq!(blend(white, (white, purple)), (1.0, 0.0));
        assert_eq!(blend(purple, (white, purple)), (0.0, 1.0));
        // A mix of the two is found again.
        let mix: [f32; 3] = std::array::from_fn(|c| 0.3 * white[c] + 0.5 * purple[c]);
        let (a, b) = blend(mix, (white, purple));
        assert!((a - 0.3).abs() < 1e-5 && (b - 0.5).abs() < 1e-5, "{a} {b}");
        // One outside them takes the nearer, never less than none of
        // either.
        let (a, b) = blend([0.1, 0.6, 0.1], (white, purple));
        assert!(a >= 0.0 && b >= 0.0);
        assert_eq!(blend([0.5; 3], ([0.0; 3], [0.0; 3])), (0.0, 0.0));
    }

    #[test]
    fn droop_bends_the_axis_over() {
        let upright = Cluster::of(&spec(ClusterKind::Catkin, 20));
        let hanging = Cluster::of(&Inflorescence {
            droop: Some(180.0),
            ..spec(ClusterKind::Catkin, 20)
        });
        let top = |cluster: &Cluster| {
            let laid = layout(cluster, berries(), 0.0, 0);
            laid.units[laid.units.len() - 1].base
        };
        assert!(top(&upright).y > 0.8);
        let hung = top(&hanging);
        assert!(hung.y < top(&upright).y - 0.3 && hung.x > 0.2, "{hung:?}");
        let card = CardFrame::of(&hanging, berries(), &[]);
        assert_eq!(card.mount, Mount::Along);
        assert!(card.aspect > CardFrame::of(&upright, berries(), &[]).aspect);
    }

    #[test]
    fn flat_topped_clusters_of_flowers_face_along_their_organ() {
        let flower = Shape::Flower(Flower::default());
        let umbel = ClusterLook::new(Cluster::of(&spec(ClusterKind::Umbel, 20)), &flower, &[])
            .expect("a flower is a unit");
        assert_eq!(umbel.card.mount, Mount::Facing);
        assert!(umbel.card.foot > 0.5 && umbel.card.cross.is_none());
        let raceme = ClusterLook::new(Cluster::of(&spec(ClusterKind::Raceme, 20)), &flower, &[])
            .expect("a flower is a unit");
        assert_eq!(raceme.card.mount, Mount::Along);
        assert_eq!(raceme.card.cross, Some(1.0));
        let fruit = Shape::Fruit(Fruit::default());
        let berries = ClusterLook::new(Cluster::of(&spec(ClusterKind::Umbel, 20)), &fruit, &[])
            .expect("a fruit is a unit");
        assert_eq!(
            berries.card.mount,
            Mount::Along,
            "fruit hang along their organ"
        );
        assert!(
            ClusterLook::new(
                Cluster::of(&Inflorescence::default()),
                &Shape::default_for(crate::lsys::OrganKind::Leaf),
                &[]
            )
            .is_none()
        );
        let heads = ClusterLook::new(
            Cluster::of(&spec(ClusterKind::Corymb, 30)),
            &Shape::Head(Head::default()),
            &[],
        )
        .expect("a head is a unit");
        assert_eq!(heads.card.mount, Mount::Facing);
    }

    #[test]
    fn standing_cards_hold_every_unit() {
        for kind in [
            ClusterKind::Raceme,
            ClusterKind::Umbel,
            ClusterKind::Head,
            ClusterKind::Panicle,
            ClusterKind::Cyme,
        ] {
            let cluster = Cluster::of(&spec(kind, 9));
            let card = CardFrame::of(&cluster, berries(), &[]);
            let half = card.aspect * card.length * 0.5;
            for p in layout(&cluster, berries(), 0.0, 0).units {
                let tip = p.base + p.direction * p.length;
                let reach = p.base.x.abs().max(tip.x.abs()) + p.length * 0.8 * 0.5;
                assert!(reach <= half + 1e-9 || card.aspect >= 2.0, "{kind:?}");
            }
        }
    }

    #[test]
    fn about_the_unripe_share_is_unripe_and_layouts_repeat() {
        let cluster = Cluster::of(&spec(ClusterKind::Raceme, 60));
        let unit = Unit::Fruit {
            aspect: 0.7,
            unripe: 0.3,
        };
        let unripe = (0..20)
            .flat_map(|seed| layout(&cluster, unit, 0.0, seed).units)
            .filter(|p| p.unripe)
            .count();
        let share = unripe as f64 / 1200.0;
        assert!((share - 0.3).abs() < 0.05, "{share}");
        assert_eq!(
            layout(&cluster, unit, 0.4, 9),
            layout(&cluster, unit, 0.4, 9)
        );
    }

    #[test]
    fn clusters_draw_within_their_organ_and_count_their_triangles() {
        let looks = crate::looks::resolve(
            [("bloom", crate::lsys::OrganKind::Flower)],
            &std::collections::BTreeMap::from([(
                "bloom".to_string(),
                crate::looks::OrganLook {
                    hairs: None,
                    hairs_beneath: None,
                    underside: None,
                    variegation: None,
                    sori: None,
                    roll: 0.0,
                    insertion: crate::looks::Insertion::Marginal,
                    shape: Shape::Fruit(Fruit {
                        kind: FruitKind::Crowned,
                        ..Fruit::default()
                    }),
                    inflorescence: Some(spec(ClusterKind::Raceme, 9)),
                    colour: None,
                    shade: None,
                    accent: None,
                    face_up: 0.0,
                    solid: None,
                    bend: None,
                    form: None,
                    season: None,
                    families: None,
                },
            )]),
            [0.3; 3],
            [0.1; 3],
        );
        let look = &looks[0];
        let cluster = look.inflorescence.as_ref().expect("a cluster");
        let organ = GraphOrgan {
            id: 11,
            organ: 0,
            segment: None,
            position: Vec3::new(1.0, 2.0, 3.0),
            heading: Vec3::Y,
            left: Vec3::X,
            size: 0.1,
            born: 1.0,
            shed: Some(2.0),
            light: 1.0,
            within: None,
            adult: None,
        };
        let mut counts = Vec::new();
        for coarse in 0..=fruit::COARSER {
            let mut mesh = Mesh::default();
            let part = Part {
                born: 1.0,
                shed: Some(2.0),
                coarse,
            };
            draw(
                &organ,
                (Vec3::Y, Vec3::X),
                look,
                cluster,
                [0.6, 0.1, 0.1],
                part,
                &mut mesh,
            );
            assert!(mesh.triangle_count() > 0);
            for position in &mesh.positions {
                let p = Vec3::new(
                    f64::from(position[0]),
                    f64::from(position[1]),
                    f64::from(position[2]),
                ) - organ.position;
                assert!(p.length() <= 0.1 * 1.3, "{p:?}");
            }
            counts.push(mesh.triangle_count());
        }
        assert_eq!(counts[0], triangles(look));
        assert!(
            counts.windows(2).all(|pair| pair[1] <= pair[0]),
            "{counts:?}"
        );
    }

    /// The look of `shape` in `cluster`, as a species resolves it.
    /// A cluster of `shape` on `day` of `season`, if given.
    fn clustered_on(
        shape: Shape,
        cluster: Inflorescence,
        season: Option<(crate::looks::Season, f64)>,
    ) -> Look {
        let day = season.as_ref().map(|&(_, day)| day);
        crate::looks::resolve_on(
            [("bloom", crate::lsys::OrganKind::Flower)],
            &std::collections::BTreeMap::from([(
                "bloom".to_string(),
                crate::looks::OrganLook {
                    hairs: None,
                    hairs_beneath: None,
                    underside: None,
                    variegation: None,
                    sori: None,
                    roll: 0.0,
                    insertion: crate::looks::Insertion::Marginal,
                    shape,
                    inflorescence: Some(cluster),
                    colour: None,
                    shade: None,
                    accent: None,
                    face_up: 0.0,
                    solid: None,
                    bend: None,
                    form: None,
                    season: season.map(|(season, _)| season),
                    families: None,
                },
            )]),
            [0.3; 3],
            [0.1; 3],
            day,
        )
        .0
        .remove(0)
    }

    /// A season whose units open over 40 days, in flower with a white
    /// flower `size` of their fruit.
    fn opening(size: f64) -> crate::looks::Season {
        crate::looks::Season {
            bud: 95.0,
            flower: 110.0,
            fruit: 135.0,
            ripe: 175.0,
            ripening: 20.0,
            fall: 245.0,
            opening: 40.0,
            fruit_size: 1.0,
            bud_look: None,
            flower_look: Some(crate::looks::StageLook {
                shape: Shape::Flower(Flower::default()),
                colour: Some([0.95, 0.95, 0.9]),
                accent: Some([0.9, 0.8, 0.2]),
                size,
                form: None,
                whole: false,
            }),
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_cluster_card_covers_what_its_solid_shows() {
        let fruit = Shape::Fruit(Fruit::default());
        let flower = Shape::Flower(Flower::default());
        let cases = [
            (fruit.clone(), spec(ClusterKind::Raceme, 12), None),
            (fruit.clone(), spec(ClusterKind::Umbel, 8), None),
            (fruit.clone(), spec(ClusterKind::Head, 20), None),
            (fruit.clone(), spec(ClusterKind::Panicle, 5), None),
            (flower.clone(), spec(ClusterKind::Raceme, 9), None),
            (flower.clone(), spec(ClusterKind::Umbel, 14), None),
            (flower, spec(ClusterKind::Corymb, 10), None),
            (
                Shape::Head(Head::default()),
                spec(ClusterKind::Corymb, 7),
                None,
            ),
            // Fruit, flowers and buds on one raceme; an umbel's flowers,
            // larger than its fruit, and its first fruit.
            (
                fruit.clone(),
                spec(ClusterKind::Raceme, 12),
                Some((opening(1.0), 140.0)),
            ),
            (
                fruit,
                spec(ClusterKind::Umbel, 8),
                Some((opening(1.4), 150.0)),
            ),
        ];
        for (shape, inflorescence, season) in cases {
            let staged = season.is_some();
            let look = clustered_on(shape, inflorescence, season);
            let cluster = look.inflorescence.as_ref().expect("a cluster");
            assert_eq!(cluster.opening.is_some(), staged);
            let card = cluster.card;
            // The solid at the card's turn, its organ 1 long from the
            // origin, heading y with its left x.
            let organ = GraphOrgan {
                id: 0,
                organ: 0,
                segment: None,
                position: Vec3::ZERO,
                heading: Vec3::Y,
                left: Vec3::X,
                size: 1.0,
                born: 0.0,
                shed: None,
                light: 1.0,
                within: None,
                adult: None,
            };
            let mut mesh = Mesh::default();
            let part = Part {
                born: 0.0,
                shed: None,
                coarse: 0,
            };
            draw_laid(
                &organ,
                (Vec3::Y, Vec3::X),
                &look,
                cluster,
                &cluster.layout(0.0, 0),
                ([0.6, 0.1, 0.1], [0.2, 0.5, 0.1]),
                part,
                &mut mesh,
            );
            // Each vertex on the card, as (u, v) of its template.
            let on_card = |p: [f32; 3]| {
                let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), -f64::from(p[2]));
                let (across, along) = match card.mount {
                    Mount::Along => (x / card.length, (y - card.foot) / card.length),
                    Mount::Facing => (-x / card.length, 0.5 + z / card.length),
                };
                (across / card.aspect + 0.5, along)
            };
            let triangles: Vec<[(f64, f64); 3]> = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| t.map(|k| on_card(mesh.positions[usize::try_from(k).unwrap()])))
                .collect();
            let inside = |(u, v): (f64, f64)| {
                triangles.iter().any(|&[a, b, c]| {
                    let side = |p: (f64, f64), q: (f64, f64)| {
                        (q.0 - p.0) * (v - p.1) - (q.1 - p.1) * (u - p.0)
                    };
                    let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
                    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
                })
            };
            let templates = crate::templates::Templates::for_looks(std::slice::from_ref(&look));
            // Which cells of a grid over the card each covers.
            let grid = 48_usize;
            let centre = |k: usize| (to_f64(k) + 0.5) / to_f64(grid);
            let mut painted = vec![false; grid * grid];
            let mut shown = vec![false; grid * grid];
            for row in 0..grid {
                for column in 0..grid {
                    let (u, v) = (centre(column), centre(row));
                    #[allow(clippy::cast_possible_truncation)]
                    let texel = templates.sample(0, u as f32, v as f32);
                    painted[row * grid + column] = texel.coverage >= 0.5;
                    shown[row * grid + column] = inside((u, v));
                }
            }
            // The share of one's cells with one of the other's within a
            // cell: the card paints each unit from its own template, so
            // they agree on where, not on every petal.
            let near = |of: &[bool], to: &[bool]| {
                let (mut met, mut all) = (0_u32, 0_u32);
                for row in 0..grid {
                    for column in 0..grid {
                        if !of[row * grid + column] {
                            continue;
                        }
                        all += 1;
                        let close = (row.saturating_sub(1)..=(row + 1).min(grid - 1)).any(|r| {
                            (column.saturating_sub(1)..=(column + 1).min(grid - 1))
                                .any(|c| to[r * grid + c])
                        });
                        met += u32::from(close);
                    }
                }
                f64::from(met) / f64::from(all.max(1))
            };
            let (card_on_solid, solid_on_card) = (near(&painted, &shown), near(&shown, &painted));
            // A facing card mirrored either way scores below these (the
            // umbel's card, 0.74 or 0.67 on its solid).
            assert!(
                card_on_solid > 0.8 && solid_on_card > 0.9,
                "{:?} of {:?}: {card_on_solid:.2} of the card lies on the solid, \
                 {solid_on_card:.2} of the solid on the card",
                cluster.cluster.kind,
                cluster.unit
            );
        }
    }
}
