//! Clusters of flowers, heads or fruit on one organ (spec rework L1,
//! LANGUAGE.md 5.4.2).
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
//! branches, a `compound_umbel` an umbel of them. A cluster nests at most
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
//! On the nearest levels each unit is a solid of its shape (a fruit as
//! `crate::fruit` draws one; a flower or a head reduced to its petals or
//! rays at two facets round a centre, with stamens only when it is a
//! centimetre across or more) and the pedicels and axes are thin prisms in
//! the cluster's `stalk` colour. A part mesh (`crate::parts`) that must
//! fit a budget takes [`crate::fruit::COARSER`] steps: petals at one
//! facet; no stamens and fruit at fewer facets; each smallest branch's
//! units merged into one solid round them; every branch merged.

use crate::blooms::{FlowerForm, Form, Part};
use crate::fruit::{self, Detail, GOLDEN_DEGREES};
use crate::graph::GraphOrgan;
use crate::looks::{ClusterKind, Inflorescence, Look, Mount, Shape, within};
use crate::math::{self, Vec3, any_perpendicular};
use crate::mesh::Mesh;
use crate::rng::{self, mix64};

/// Levels a cluster nests to: its own axis, its branches and theirs.
pub const LEVELS: usize = 3;

/// Most units one cluster holds.
pub const MOST_UNITS: u64 = 5000;

/// A cluster's stalks unless it sets a colour: green.
pub const STALK: [f32; 3] = [0.3, 0.38, 0.14];

/// The coarse step (`crate::parts`) from which units are merged into one
/// solid per smallest branch; the next merges every branch.
const MERGED: usize = 3;

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
}

impl Cluster {
    /// The cluster `spec` describes, its defaults filled in.
    #[must_use]
    pub fn of(spec: &Inflorescence) -> Self {
        Self::filled(spec, ClusterKind::Raceme, 12, STALK, 1)
    }

    /// `spec` at nesting `level`, taking `kind`, `flowers` and `stalk`
    /// unless it sets its own.
    fn filled(
        spec: &Inflorescence,
        kind: ClusterKind,
        flowers: u32,
        stalk: [f32; 3],
        level: usize,
    ) -> Self {
        let kind = spec.kind.unwrap_or(kind);
        let flowers = spec.flowers.unwrap_or(flowers);
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
                flowers,
                stalk,
                level + 1,
            ))
        });
        let bearing = branches > 0;
        Self {
            kind,
            flowers,
            branches,
            branch,
            per_node: spec
                .per_node
                .unwrap_or(if kind == ClusterKind::Cyme { 2 } else { 1 }),
            divergence: spec.divergence.unwrap_or(GOLDEN_DEGREES),
            spread: spec.spread.unwrap_or(35.0),
            pedicel: spec.pedicel.unwrap_or(if bearing { 0.0 } else { 0.15 }),
            peduncle: spec.peduncle.unwrap_or(
                if matches!(kind, ClusterKind::Umbel | ClusterKind::CompoundUmbel) {
                    0.3
                } else {
                    0.1
                },
            ),
            size: spec.size.unwrap_or(if bearing { 0.4 } else { 0.15 }),
            taper: spec.taper.unwrap_or(0.0),
            droop: spec.droop.unwrap_or(0.0),
            stalk,
        }
    }

    /// Units in the whole cluster.
    #[must_use]
    pub fn units(&self) -> u64 {
        match &self.branch {
            Some(branch) => u64::from(self.branches) * branch.units(),
            None => u64::from(self.flowers),
        }
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
        ];
        for (key, value, low, high) in values {
            if let Some(value) = value {
                within(&format!("inflorescence {key}"), value, low, high)?;
            }
        }
        for channel in spec.stalk.into_iter().flatten() {
            within("inflorescence stalk", f64::from(channel), 0.0, 1.0)?;
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
            _ => None,
        }
    }

    /// One unit's width over its length.
    #[must_use]
    pub fn width(self) -> f64 {
        match self {
            Self::Flower | Self::Head => 1.0,
            Self::Fruit { aspect, .. } => aspect.clamp(0.05, 2.0),
        }
    }

    fn unripe(self) -> f64 {
        match self {
            Self::Fruit { unripe, .. } => unripe,
            Self::Flower | Self::Head => 0.0,
        }
    }

    /// Whether the unit is seen face on: a flower or a head.
    #[must_use]
    pub fn faces(self) -> bool {
        matches!(self, Self::Flower | Self::Head)
    }
}

/// A look's cluster: its layout's keys, its unit and its card.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterLook {
    pub cluster: Cluster,
    pub unit: Unit,
    pub card: CardFrame,
}

impl ClusterLook {
    /// `cluster` of units drawn with `shape`; `None` unless the shape is a
    /// flower, a head or a fruit.
    #[must_use]
    pub fn new(cluster: Cluster, shape: &Shape) -> Option<Self> {
        let unit = Unit::of(shape)?;
        let card = CardFrame::of(&cluster, unit);
        Some(Self {
            cluster,
            unit,
            card,
        })
    }

    /// The cluster laid out turned `turn` radians round its axis, its
    /// unripe fruit chosen by `seed`.
    #[must_use]
    pub fn layout(&self, turn: f64, seed: u64) -> Layout {
        layout(&self.cluster, self.unit, turn, seed)
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
    /// The card's length: 1 less `foot` for a standing card, twice the
    /// farthest any unit reaches from the axis for a facing one, × 1.04.
    pub length: f64,
    /// Width over length: 1 for a facing card, else twice the farthest any
    /// unit reaches across it, × 1.04, from 0.1 to 2.
    pub aspect: f64,
    /// The crossing card's width, relative: 1 for a standing card.
    pub cross: Option<f64>,
}

impl CardFrame {
    /// The card of `cluster`, laid out unturned.
    #[must_use]
    pub fn of(cluster: &Cluster, unit: Unit) -> Self {
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
            let reach = laid
                .units
                .iter()
                .map(|p| math::sqrt(p.base.x * p.base.x + p.base.z * p.base.z) + 0.5 * p.length)
                .fold(0.0, f64::max);
            return Self {
                mount: Mount::Facing,
                foot: if top.is_finite() { top } else { 1.0 },
                length: (2.0 * reach * 1.04).clamp(0.1, 4.0),
                aspect: 1.0,
                cross: None,
            };
        }
        let width = unit.width();
        let (mut low, mut reach) = (0.0_f64, 0.0_f64);
        for p in &laid.units {
            let tip = p.base + p.direction * p.length;
            let half = p.length * width * 0.5;
            reach = reach.max(p.base.x.abs().max(tip.x.abs()) + half);
            low = low.min(p.base.y.min(tip.y) - half);
        }
        for stalk in &laid.stalks {
            reach = reach.max(stalk.from.x.abs().max(stalk.to.x.abs()));
            low = low.min(stalk.from.y.min(stalk.to.y));
        }
        let length = 1.0 - low;
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
    /// The smallest branch it is on: 0 for the cluster's own axis.
    pub group: u32,
    /// The branch of the cluster's own axis it is on: 0 for that axis.
    pub top: u32,
    /// Its pedicel's colour.
    pub stalk: [f32; 3],
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

/// A cluster's units and stalks, in the organ's frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub units: Vec<Placed>,
    pub stalks: Vec<Stalk>,
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
        (0, 0),
        1,
        &mut laid,
    );
    // A bent cluster can reach past its organ's tip (a nodding corymb's
    // long lower pedicels turn upright): it shrinks to fit its organ.
    let mut layout = laid.layout;
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
    }
    layout
}

/// Lay out `cluster` in `frame`, its axis heading along `heading` in the
/// organ's frame where it starts, its units in `group` of top branch
/// `top`, at nesting `level`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lay(
    cluster: &Cluster,
    unit: Unit,
    turn: f64,
    frame: Frame,
    heading: Vec3,
    (group, top): (u32, u32),
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
        unit.width()
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
            let unripe = rng::unit(mix64(
                laid.seed ^ (index + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            )) < laid.unripe;
            laid.layout.units.push(Placed {
                attach: frame.point(slot.attach),
                base: frame.point(slot.base),
                direction: frame.direction(slot.direction),
                length: slot.length * frame.scale,
                unripe,
                group,
                top,
                stalk: cluster.stalk,
            });
        }
        return;
    };
    // Each branch: its ray or stalk, then the branch in its own frame.
    let branch_radius = axis_radius(branch, unit) * cluster.size;
    let golden = math::radians(GOLDEN_DEGREES);
    for (k, slot) in axis.slots.iter().enumerate() {
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
            branch,
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
            (group, top),
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
    }];
    let (mut first, mut count) = (0_usize, 1_usize);
    let mut scale = 1.0;
    let (n, m) = (to_usize(n), to_usize(m));
    while nodes.len() < n {
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
    let slots = raw
        .iter()
        .map(|&(at, direction, length)| {
            let at = place(at);
            Slot {
                anchor,
                attach: at,
                base: at,
                direction,
                length,
                radial: out,
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
    let turn = rng::unit(mix64(organ.id ^ 0x00F2_0175)) * std::f64::consts::TAU;
    draw_laid(
        organ,
        (axis, side),
        look,
        cluster,
        &cluster.layout(turn, organ.id),
        (colour, accent),
        part,
        mesh,
    );
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// Draw `laid`, a layout of `cluster`, on `organ` (see [`draw`]).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
    if part.coarse >= MERGED {
        merged(
            laid,
            cluster.unit,
            part,
            &to_world,
            axis,
            side,
            colour,
            accent,
            mesh,
        );
        return;
    }
    let width = cluster.unit.width();
    let detail = Detail::for_count(
        u32::try_from(laid.units.len()).unwrap_or(u32::MAX),
        part.coarse,
    );
    for p in &laid.units {
        let base = to_world(p.base);
        let direction = to_direction(p.direction);
        let unit_length = p.length * length;
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
                part,
                mesh,
            );
        }
        // Each unit turned its own way about its direction.
        let spin = rng::unit(mix64(
            organ.id ^ p.base.x.to_bits() ^ p.base.z.to_bits().rotate_left(17),
        )) * std::f64::consts::TAU;
        match (&look.shape, &look.form) {
            (Shape::Fruit(shape), Form::Fruit(form)) => fruit::draw_unit(
                organ,
                shape,
                form,
                (base, direction, unit_length),
                p.unripe,
                detail,
                colour,
                accent,
                part,
                mesh,
            ),
            (Shape::Flower(flower), Form::Flower(form)) => floret(
                &Petals {
                    count: flower.petals.max(1),
                    width: flower.petal_width,
                    centre: flower.centre,
                    start: 0.6,
                },
                form,
                (base, direction, unit_length, spin),
                colour,
                accent,
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
                colour,
                accent,
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

/// A flower or head reduced for a cluster: its petals as kites (one facet
/// each from the first coarse step), seen from both sides, round a low
/// pyramid of a centre in the accent colour, and stamens while it is a
/// centimetre across or more and the part is drawn fine.
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
    if part.coarse <= 1 && width >= 0.01 && form.stamens > 0 {
        let stamens = form.stamens.min(12);
        let reach = form.stamen_length * radius;
        for k in 0..stamens {
            let angle = spin + std::f64::consts::TAU * (f64::from(k) + 0.5) / f64::from(stamens);
            let radial = across * math::cos(angle) + other * math::sin(angle);
            let foot = base + radial * (centre * 0.5);
            let tip = foot + (direction + radial * 0.35).normalize_or(direction) * reach;
            let side = direction.cross(radial).normalize_or(across) * (reach * 0.06);
            both(&[foot - side, foot + side, tip], accent, mesh);
        }
    }
    crate::leaves::smooth_normals(mesh, first, first_index);
}

/// The coarsest steps: each smallest branch's units (at [`MERGED`]) or
/// each branch of the cluster's own axis (beyond it) merged into one
/// ellipsoid round them, in the colour most of them are.
#[allow(clippy::too_many_arguments)]
fn merged(
    laid: &Layout,
    unit: Unit,
    part: Part,
    to_world: &dyn Fn(Vec3) -> Vec3,
    axis: Vec3,
    side: Vec3,
    colour: [f32; 3],
    accent: [f32; 3],
    mesh: &mut Mesh,
) {
    let width = unit.width();
    let mut groups: Vec<(u32, Vec3, Vec3, usize, usize)> = Vec::new();
    for p in &laid.units {
        let key = if part.coarse > MERGED { p.top } else { p.group };
        let tip = if unit.faces() {
            p.base
        } else {
            p.base + p.direction * p.length
        };
        let half = p.length * width * 0.5;
        let reach = Vec3::new(half, half, half);
        let (low, high) = (p.base.min(tip) - reach, p.base.max(tip) + reach);
        match groups.iter_mut().find(|group| group.0 == key) {
            Some(group) => {
                group.1 = group.1.min(low);
                group.2 = group.2.max(high);
                group.3 += 1;
                group.4 += usize::from(p.unripe);
            }
            None => groups.push((key, low, high, 1, usize::from(p.unripe))),
        }
    }
    let length = (to_world(Vec3::Y) - to_world(Vec3::ZERO)).length();
    for (_, low, high, count, unripe) in groups {
        let half = (high - low) * 0.5;
        fruit::spheroid(
            to_world((low + high) * 0.5),
            (half.y * length, half.x * length, half.z * length),
            Detail::merged(),
            axis,
            side,
            if 2 * unripe > count { accent } else { colour },
            part,
            mesh,
        );
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
                    group: 0,
                    top: 0,
                    stalk: STALK,
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
        let card = CardFrame::of(&hanging, berries());
        assert_eq!(card.mount, Mount::Along);
        assert!(card.aspect > CardFrame::of(&upright, berries()).aspect);
    }

    #[test]
    fn flat_topped_clusters_of_flowers_face_along_their_organ() {
        let flower = Shape::Flower(Flower::default());
        let umbel = ClusterLook::new(Cluster::of(&spec(ClusterKind::Umbel, 20)), &flower)
            .expect("a flower is a unit");
        assert_eq!(umbel.card.mount, Mount::Facing);
        assert!(umbel.card.foot > 0.5 && umbel.card.cross.is_none());
        let raceme = ClusterLook::new(Cluster::of(&spec(ClusterKind::Raceme, 20)), &flower)
            .expect("a flower is a unit");
        assert_eq!(raceme.card.mount, Mount::Along);
        assert_eq!(raceme.card.cross, Some(1.0));
        let fruit = Shape::Fruit(Fruit::default());
        let berries = ClusterLook::new(Cluster::of(&spec(ClusterKind::Umbel, 20)), &fruit)
            .expect("a fruit is a unit");
        assert_eq!(
            berries.card.mount,
            Mount::Along,
            "fruit hang along their organ"
        );
        assert!(
            ClusterLook::new(
                Cluster::of(&Inflorescence::default()),
                &Shape::default_for(crate::lsys::OrganKind::Leaf)
            )
            .is_none()
        );
        let heads = ClusterLook::new(
            Cluster::of(&spec(ClusterKind::Corymb, 30)),
            &Shape::Head(Head::default()),
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
            let card = CardFrame::of(&cluster, berries());
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
    fn clustered(shape: Shape, cluster: Inflorescence) -> Look {
        crate::looks::resolve(
            [("bloom", crate::lsys::OrganKind::Flower)],
            &std::collections::BTreeMap::from([(
                "bloom".to_string(),
                crate::looks::OrganLook {
                    shape,
                    inflorescence: Some(cluster),
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
        )
        .remove(0)
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_cluster_card_covers_what_its_solid_shows() {
        let fruit = Shape::Fruit(Fruit::default());
        let flower = Shape::Flower(Flower::default());
        let cases = [
            (fruit.clone(), spec(ClusterKind::Raceme, 12)),
            (fruit.clone(), spec(ClusterKind::Umbel, 8)),
            (fruit.clone(), spec(ClusterKind::Head, 20)),
            (fruit, spec(ClusterKind::Panicle, 5)),
            (flower.clone(), spec(ClusterKind::Raceme, 9)),
            (flower.clone(), spec(ClusterKind::Umbel, 14)),
            (flower, spec(ClusterKind::Corymb, 10)),
            (Shape::Head(Head::default()), spec(ClusterKind::Corymb, 7)),
        ];
        for (shape, inflorescence) in cases {
            let look = clustered(shape, inflorescence);
            let cluster = look.inflorescence.as_ref().expect("a cluster");
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
