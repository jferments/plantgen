//! The growth loop: grow a plant once from seed and keep keyframes.
//!
//! Each step draws the current string, runs the environment tools on the
//! drawing, and rewrites the string with rules that read the tools' answers:
//!
//! ```text
//! axiom ─► interpret ─► (keyframe? snapshot) ─► tools ─► derive ─┐
//!              ▲                                                  │
//!              └──────────────────────────────────────────────────┘
//! ```
//!
//! Because segments and organs keep their lineage identities from step to
//! step, every keyframe knows when each of its parts was born and, once
//! growth ends, when it is shed. A renderer can therefore show the plant at
//! any age between keyframes without regrowing it.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use serde::{Deserialize, Serialize};

use crate::conditions::{Around, Conditions};
use crate::graph::{GraphOrgan, GraphSegment, OrganType, PlantGraph};
use crate::lsys::derive::{Clock, Deriver};
use crate::lsys::program::{SymbolKind, ToolKind};
use crate::lsys::tools::{self, ToolState};
use crate::lsys::turtle::{Interpreter, NodeKind, Scene};
use crate::lsys::{GrowthError, Limits, Program};
use crate::math;

/// How to grow one plant variant.
#[derive(Debug, Clone, PartialEq)]
pub struct GrowthSettings {
    /// Variant seed; every random draw depends on it.
    pub seed: u64,
    /// Years per derivation step.
    pub dt: f64,
    /// Total growing time, in years.
    pub years: f64,
    /// Ages at which to keep a [`PlantGraph`], in years.
    pub keyframes: Vec<f64>,
    /// The conditions it grows in; `light@1` reads their neighbours.
    pub conditions: Conditions,
    pub limits: Limits,
    /// The host's wood a climber, epiphyte or parasite grows on
    /// (`host@1`), grown from the spec's `host`.
    pub host: Option<std::sync::Arc<crate::lsys::tools::Host>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GrowthStats {
    pub steps: u32,
    pub peak_modules: usize,
    pub peak_segments: usize,
    pub peak_organs: usize,
}

/// A grown plant: keyframes in age order.
#[derive(Debug, Clone, PartialEq)]
pub struct Growth {
    pub organ_types: Vec<OrganType>,
    /// The program's body types by name, in declaration order: a graph
    /// segment's body, less one, indexes them.
    pub body_types: Vec<String>,
    pub keyframes: Vec<PlantGraph>,
    pub stats: GrowthStats,
    shed: ShedLog,
}

impl Growth {
    /// What the plant shed while it grew: every self-pruned branch as last
    /// seen, and its fallen leaves, cones and fruit counted by type and
    /// year, what a floor under it would hold. No package stores it.
    #[must_use]
    pub fn shed(&self) -> &ShedLog {
        &self.shed
    }
}

/// What a plant shed while it grew: every segment as it was the last step
/// it stood, in id order, and its organs counted by type at the step they
/// fell, in age order. A deciduous tree drops millions of leaves over its
/// life; litter and the nutrients it returns need how many fell when, not
/// each leaf.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShedLog {
    pub segments: Vec<ShedSegment>,
    pub organs: Vec<ShedOrgans>,
}

/// A shed segment of wood or body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShedSegment {
    pub id: u64,
    /// Age it grew at, and age it was shed at, years.
    pub born: f64,
    pub shed: f64,
    /// Its radius and length, metres.
    pub radius: f64,
    pub length: f64,
    pub order: u16,
    /// Height of its middle above the ground, metres.
    pub height: f64,
    /// Its body: 0 for wood, else one more than its body type's index.
    pub body: u8,
}

/// The organs of one type that fell at one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShedOrgans {
    /// Their organ type's index in [`Growth::organ_types`].
    pub organ: u16,
    /// Age they were shed at, years.
    pub shed: f64,
    /// How many fell: an organ that comes back and falls again counts
    /// each time.
    pub count: u32,
    /// The sum of their sizes as last seen, metres.
    pub size: f64,
    /// The sum of their leaf areas (the type's `area` times size squared),
    /// square metres.
    pub area: f64,
}

fn steps_for(years: f64, dt: f64) -> Result<u32, GrowthError> {
    let steps = (years / dt).round();
    if !(0.0..=f64::from(u32::MAX)).contains(&steps) {
        return Err(GrowthError::Settings(format!(
            "{years} years is out of range"
        )));
    }
    // Checked above: a whole number inside the u32 range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(steps as u32)
}

/// The number of steps and the sorted, distinct keyframe steps.
fn schedule(settings: &GrowthSettings) -> Result<(u32, Vec<u32>), GrowthError> {
    if !(settings.dt > 0.0 && settings.dt.is_finite()) {
        return Err(GrowthError::Settings(format!(
            "the step must be a positive number of years, found {}",
            settings.dt
        )));
    }
    if !(settings.years >= 0.0 && settings.years.is_finite()) {
        return Err(GrowthError::Settings(format!(
            "growing time must be zero or more years, found {}",
            settings.years
        )));
    }
    let steps = steps_for(settings.years, settings.dt)?;
    if steps > settings.limits.max_steps {
        return Err(GrowthError::Limit {
            what: "growth steps",
            limit: u64::from(settings.limits.max_steps),
        });
    }
    let mut keyframe_steps = Vec::new();
    for age in &settings.keyframes {
        let step = steps_for(*age, settings.dt)?;
        if step > steps {
            return Err(GrowthError::Settings(format!(
                "keyframe age {age} is after the end of growth at {} years",
                settings.years
            )));
        }
        keyframe_steps.push(step);
    }
    keyframe_steps.sort_unstable();
    keyframe_steps.dedup();
    Ok((steps, keyframe_steps))
}

/// The program's organ types, and for every symbol its index among them
/// (`u16::MAX` for symbols that are not organs).
fn organ_types(program: &Program, organ_area: &[f64]) -> (Vec<OrganType>, Vec<u16>) {
    let mut organ_index = vec![u16::MAX; program.symbols.len()];
    let mut types = Vec::new();
    for (symbol, info) in program.symbols.iter().enumerate() {
        if let SymbolKind::Organ { kind, .. } = info.kind {
            organ_index[symbol] = u16::try_from(types.len()).unwrap_or(u16::MAX);
            types.push(OrganType {
                name: info.name.clone(),
                kind,
                area: organ_area[symbol],
            });
        }
    }
    (types, organ_index)
}

/// A map keyed by a part's id. Ids are the engine's own, never read from
/// outside, so one multiply mixes them enough and the maps a growth updates
/// for every part at every step need not pay for `SipHash`.
type IdMap<V> = HashMap<u64, V, BuildHasherDefault<IdHasher>>;
type IdSet = HashSet<u64, BuildHasherDefault<IdHasher>>;

/// The hasher of [`IdMap`]: Fx hashing (as in rustc), one rotate, xor and
/// multiply per word.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// The parts that stood at the last step, and every part gone before it,
/// each as it was the last step it stood: the shed log in the making. Only
/// standing parts are looked up, so the work follows the plant as it is,
/// not every leaf it has ever shed.
struct Parts<P> {
    standing: IdMap<(u32, P)>,
    now: IdMap<(u32, P)>,
    gone: Vec<(u64, u32, P)>,
}

impl<P> Default for Parts<P> {
    fn default() -> Self {
        Self {
            standing: IdMap::default(),
            now: IdMap::default(),
            gone: Vec::new(),
        }
    }
}

impl<P: Copy> Parts<P> {
    /// This step's parts, by id; a part that stood at the last step and
    /// does not now has gone.
    fn step(&mut self, parts: impl Iterator<Item = (u64, u32, P)>) {
        self.now.clear();
        for (id, step, part) in parts {
            self.now.insert(id, (step, part));
        }
        for (id, (seen, part)) in self.standing.drain() {
            if !self.now.contains_key(&id) {
                self.gone.push((id, seen, part));
            }
        }
        std::mem::swap(&mut self.standing, &mut self.now);
    }

    /// Every part gone by the end, by id, as it was the last step it stood:
    /// a part that went more than once is as it went last, and one that
    /// came back and still stands has not gone.
    fn gone(mut self) -> Vec<(u64, u32, P)> {
        self.gone.sort_unstable_by_key(|(id, seen, _)| (*id, *seen));
        let mut last: Vec<(u64, u32, P)> = Vec::with_capacity(self.gone.len());
        for record in self.gone {
            match last.last_mut() {
                Some(previous) if previous.0 == record.0 => *previous = record,
                _ => last.push(record),
            }
        }
        last.retain(|(id, _, _)| !self.standing.contains_key(id));
        last
    }
}

/// The last step part `id` stood, if it is among the `gone` (by id).
fn last_seen<P>(gone: &[(u64, u32, P)], id: u64) -> Option<u32> {
    let at = gone.binary_search_by_key(&id, |(gone, _, _)| *gone).ok()?;
    Some(gone[at].1)
}

/// The organs that stood at the last step, in the order they were drawn,
/// and the litter so far: organs counted by type at the step they fell.
/// Only organs a keyframe holds are followed by id, for the age their
/// keyframes show them shed at.
#[derive(Default)]
struct Litter {
    /// Last step's organs: id, symbol and size.
    standing: Vec<(u64, u16, f64)>,
    /// This step's organ ids.
    now: IdSet,
    spare: Vec<(u64, u16, f64)>,
    fallen: Vec<ShedOrgans>,
    /// Organs a keyframe holds, with the last step each stood before it
    /// last went.
    kept: IdMap<Option<u32>>,
}

impl Litter {
    /// This step's organs (id, symbol, size): one that stood at the last
    /// step and does not now fell this step, at age `step · dt`. Each
    /// type's sums add in the order the organs were drawn.
    fn step(
        &mut self,
        step: u32,
        dt: f64,
        organs: impl Iterator<Item = (u64, u16, f64)>,
        types: &[OrganType],
        organ_index: &[u16],
    ) {
        let mut current = std::mem::take(&mut self.spare);
        current.clear();
        current.extend(organs);
        self.now.clear();
        for (id, _, _) in &current {
            self.now.insert(*id);
        }
        let mut fell = vec![(0_u32, 0.0_f64, 0.0_f64); types.len()];
        for &(id, symbol, size) in &self.standing {
            if self.now.contains(&id) {
                continue;
            }
            let organ = usize::from(organ_index[usize::from(symbol)]);
            let (count, sizes, areas) = &mut fell[organ];
            *count += 1;
            *sizes += size;
            *areas += types[organ].area * size * size;
            if let Some(last) = self.kept.get_mut(&id) {
                *last = Some(step.saturating_sub(1));
            }
        }
        for (organ, (count, size, area)) in fell.into_iter().enumerate() {
            if count > 0 {
                self.fallen.push(ShedOrgans {
                    organ: u16::try_from(organ).unwrap_or(u16::MAX),
                    shed: f64::from(step) * dt,
                    count,
                    size,
                    area,
                });
            }
        }
        self.spare = std::mem::replace(&mut self.standing, current);
    }

    /// Follow the organs a keyframe holds.
    fn keep(&mut self, scene: &Scene) {
        for organ in &scene.organs {
            self.kept.entry(organ.id).or_insert(None);
        }
    }

    /// For an organ a keyframe holds: the last step it stood before it
    /// went for good, if it does not stand at the end.
    fn last_seen(&self, id: u64) -> Option<u32> {
        if self.now.contains(&id) {
            return None;
        }
        self.kept.get(&id).copied().flatten()
    }
}

/// Girth bookkeeping across steps: the annual rings laid down on each
/// segment and the widest radius it has had.
#[derive(Default)]
struct Girth {
    widest: IdMap<f64>,
    ring_area: IdMap<f64>,
}

impl Girth {
    /// This step's radii: the larger of the pipe model and the rings laid
    /// down so far, never shrinking, so a trunk keeps the girth it reached
    /// before its lower branches were shed.
    fn radii(&mut self, scene: &Scene, pipes: Option<&tools::Pipes>, dt: f64) -> Vec<f64> {
        scene
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let radius = match pipes {
                    Some(pipes) => {
                        let rings = self.ring_area.entry(segment.id).or_insert(0.0);
                        let radius = pipes.radii[index].max(math::sqrt(*rings / math::PI));
                        // This step's ring shows from the next step on.
                        *rings += pipes.ring_rate * dt * pipes.leaf_area[index];
                        radius
                    }
                    None => segment.width,
                };
                let widest = self.widest.entry(segment.id).or_insert(0.0);
                *widest = widest.max(radius);
                *widest
            })
            .collect()
    }
}

/// The pipe tool's settings at this step, if the program configures it.
fn pipe_settings(
    program: &Program,
    params: &[f64],
    clock: Clock,
    stack: &mut Vec<f64>,
) -> Option<Vec<f64>> {
    let config = program.tool(ToolKind::Pipe)?;
    let scope = crate::lsys::expr::Scope {
        globals: params,
        locals: &[],
        env: &crate::lsys::expr::NO_ENV,
        t: clock.t,
        dt: clock.dt,
        age: 0.0,
        step: f64::from(clock.step),
        key: 0,
    };
    Some(
        config
            .settings
            .iter()
            .map(|code| crate::lsys::expr::eval(code, &scope, stack))
            .collect(),
    )
}

/// The conditions' substrate, decoded once for the whole growth (G3).
fn substrate_field(
    settings: &GrowthSettings,
) -> Result<Option<crate::substrate::SubstrateField>, GrowthError> {
    settings
        .conditions
        .substrate
        .as_ref()
        .map(crate::substrate::Substrate::field)
        .transpose()
        .map_err(|message| GrowthError::Tool {
            tool: "substrate",
            message,
        })
}

/// What a [`Watch`] sees after a growth step.
#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    /// The step just taken, from 0, and the last step.
    pub step: u32,
    pub steps: u32,
    /// The plant's age at this step, years.
    pub age: f64,
    /// The plant as it stands at this step, as one keyframe, when the
    /// watch asked for it ([`Watch::wants_frame`]). Nothing in it is shed.
    pub frame: Option<&'a Growth>,
}

/// Watches a growth step by step: a viewer showing it grow, a progress
/// bar, a stop button. Watching never changes what grows.
pub trait Watch {
    /// Whether to pass the plant as it stands at `step` to [`Watch::step`].
    fn wants_frame(&mut self, _step: u32) -> bool {
        false
    }

    /// Called after each step. Returning false stops the growth with
    /// [`GrowthError::Stopped`]. May block, to pause it.
    fn step(&mut self, progress: Progress<'_>) -> bool;
}

/// Grow a plant from its axiom.
///
/// # Errors
///
/// Fails on invalid settings, a rule or tool error, or a sandbox limit.
pub fn grow(
    program: &Program,
    params: &[f64],
    settings: &GrowthSettings,
) -> Result<Growth, GrowthError> {
    grow_watched(program, params, settings, None)
}

/// [`grow`], telling `watch` after each step.
///
/// # Errors
///
/// As [`grow`], and [`GrowthError::Stopped`] when the watch stops it.
pub fn grow_watched(
    program: &Program,
    params: &[f64],
    settings: &GrowthSettings,
    mut watch: Option<&mut dyn Watch>,
) -> Result<Growth, GrowthError> {
    let (steps, keyframe_steps) = schedule(settings)?;
    let organ_area = tools::organ_areas(program, params)?;
    let (organ_types, organ_index) = organ_types(program, &organ_area);

    let limits = settings.limits;
    let mut deriver = Deriver::new(program, params, &limits);
    let mut interpreter = Interpreter::new(program, params, &limits);
    let mut tool_state = ToolState::default();
    let neighbourhood = settings.conditions.neighbourhood();
    let substrate = substrate_field(settings)?;
    let around = Around {
        neighbourhood: &neighbourhood,
        host: settings.host.as_deref(),
        substrate: substrate.as_ref(),
    };
    let mut string = deriver.axiom(settings.seed, settings.dt)?;
    let mut stats = GrowthStats {
        steps,
        ..GrowthStats::default()
    };
    let mut girth = Girth::default();
    // Each segment as it was the last step it stood, and the organs that
    // fell, for the shed log.
    let mut segments: Parts<ShedSegment> = Parts::default();
    let mut litter = Litter::default();
    let mut keyframes = Vec::with_capacity(keyframe_steps.len());
    let mut next_keyframe = 0;
    let mut stack = Vec::new();

    for step in 0..=steps {
        let clock = Clock {
            step,
            t: f64::from(step) * settings.dt,
            dt: settings.dt,
        };
        let scene = interpreter.interpret(&string, clock)?;
        stats.peak_modules = stats.peak_modules.max(string.len());
        stats.peak_segments = stats.peak_segments.max(scene.segments.len());
        stats.peak_organs = stats.peak_organs.max(scene.organs.len());

        let pipes = match pipe_settings(program, params, clock, &mut stack) {
            Some(values) => Some(tools::pipe(&scene, &organ_area, &values)?),
            None => None,
        };
        let radii = girth.radii(&scene, pipes.as_ref(), settings.dt);
        segments.step(scene.segments.iter().zip(&radii).map(|(segment, radius)| {
            let part = ShedSegment {
                id: segment.id,
                born: segment.born,
                shed: 0.0,
                radius: *radius,
                length: segment.start.distance(segment.end),
                order: segment.order,
                height: 0.5 * (segment.start.y + segment.end.y),
                body: segment.body,
            };
            (segment.id, step, part)
        }));
        litter.step(
            step,
            settings.dt,
            scene
                .organs
                .iter()
                .map(|organ| (organ.id, organ.symbol, organ.size)),
            &organ_types,
            &organ_index,
        );

        let keyframe = keyframe_steps.get(next_keyframe) == Some(&step);
        if step == steps && !keyframe {
            break;
        }
        let output = tools::run(
            program,
            params,
            &organ_area,
            clock,
            &scene,
            &around,
            &mut tool_state,
            &limits,
            settings.seed,
        )?;
        if keyframe {
            litter.keep(&scene);
            keyframes.push(snapshot(
                &scene,
                &radii,
                &output.organ_light,
                &organ_index,
                clock.t,
            ));
            next_keyframe += 1;
        }
        if let Some(watch) = watch.as_deref_mut() {
            let frame = watch.wants_frame(step).then(|| Growth {
                organ_types: organ_types.clone(),
                body_types: program.bodies().map(str::to_string).collect(),
                keyframes: vec![snapshot(
                    &scene,
                    &radii,
                    &output.organ_light,
                    &organ_index,
                    clock.t,
                )],
                stats,
                shed: ShedLog::default(),
            });
            let progress = Progress {
                step,
                steps,
                age: clock.t,
                frame: frame.as_ref(),
            };
            if !watch.step(progress) {
                return Err(GrowthError::Stopped);
            }
        }
        if step == steps {
            break;
        }
        string = deriver.derive(&string, &output.env, clock)?;
    }

    // Every part stood at some step; one gone before the last was shed the
    // step after it was last seen.
    let shed_age = |seen: u32| (seen < steps).then(|| f64::from(seen + 1) * settings.dt);
    let gone_segments = segments.gone();
    for graph in &mut keyframes {
        for segment in &mut graph.segments {
            segment.shed = last_seen(&gone_segments, segment.id).and_then(shed_age);
        }
        for organ in &mut graph.organs {
            organ.shed = litter.last_seen(organ.id).and_then(shed_age);
        }
    }
    // Segments by id, as the gone list is; organs by age.
    let shed = ShedLog {
        segments: gone_segments
            .into_iter()
            .filter_map(|(_, seen, part)| shed_age(seen).map(|shed| ShedSegment { shed, ..part }))
            .collect(),
        organs: litter.fallen,
    };
    Ok(Growth {
        organ_types,
        body_types: program.bodies().map(str::to_string).collect(),
        keyframes,
        stats,
        shed,
    })
}

fn snapshot(
    scene: &Scene,
    radii: &[f64],
    organ_light: &[f64],
    organ_index: &[u16],
    age: f64,
) -> PlantGraph {
    let segments = scene
        .segments
        .iter()
        .zip(radii)
        .map(|(segment, radius)| {
            // Walk up through query modules to the segment this one grows from.
            let node = scene.nodes[segment.node as usize];
            let mut lateral = node.lateral;
            let mut parent = node.parent;
            while let Some(index) = parent {
                let up = scene.nodes[index as usize];
                match up.kind {
                    NodeKind::Segment(_) => break,
                    NodeKind::Query(_) => {
                        lateral |= up.lateral;
                        parent = up.parent;
                    }
                }
            }
            let parent = parent.and_then(|index| match scene.nodes[index as usize].kind {
                NodeKind::Segment(segment) => Some(segment),
                NodeKind::Query(_) => None,
            });
            GraphSegment {
                id: segment.id,
                parent,
                lateral: lateral || parent.is_none(),
                order: segment.order,
                start: segment.start,
                end: segment.end,
                radius: *radius,
                born: segment.born,
                shed: None,
                body: segment.body,
                left: segment.left,
            }
        })
        .collect();
    let organs = scene
        .organs
        .iter()
        .enumerate()
        .map(|(index, organ)| GraphOrgan {
            id: organ.id,
            organ: organ_index[usize::from(organ.symbol)],
            segment: organ.segment,
            position: organ.position,
            heading: organ.frame.h,
            left: organ.frame.l,
            size: organ.size,
            born: organ.born,
            shed: None,
            light: organ_light.get(index).copied().unwrap_or(1.0),
        })
        .collect();
    PlantGraph {
        age,
        height: scene.height,
        segments,
        organs,
    }
}

#[cfg(test)]
// Tests check exact results: clamped, integral and copied values.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::lsys::program::OrganKind;
    use crate::lsys::turtle::OrganInstance;
    use crate::math::Vec3;
    use std::collections::{BTreeMap, BTreeSet};

    const BUSH: &str = "
        lsystem bush 1;
        param growth = 0.4;
        module A queries light, vigour;
        module S queries vigour;
        organ leaf foliage area 0.02;
        tool light@1 { cell = 0.3 };
        tool vigour@1 { lambda = 0.6 };
        tool pipe@1 { exponent = 2.5, tip = 0.004 };
        axiom A;
        rule A : vigour > 0.2 -> F(growth) [ &(50) S A ] /(137.5) [ leaf ] A;
        rule S : age > 2 && qsum < 0.3 -> %;
        rule leaf(s) : age > 2 -> ;
    ";

    fn settings(years: f64, keyframes: Vec<f64>) -> GrowthSettings {
        GrowthSettings {
            seed: 11,
            dt: 1.0,
            years,
            keyframes,
            conditions: Conditions::preset(crate::spec::Environment::Open),
            limits: Limits::default(),
            host: None,
        }
    }

    fn bush(seed: u64) -> Growth {
        let program = Program::compile(BUSH).unwrap();
        let params = program.resolve_params(&BTreeMap::new()).unwrap();
        grow(
            &program,
            &params,
            &GrowthSettings {
                seed,
                ..settings(8.0, vec![4.0, 8.0])
            },
        )
        .unwrap()
    }

    #[test]
    fn growth_is_reproducible_and_seed_dependent() {
        let a = bush(11);
        let b = bush(11);
        assert_eq!(a, b);
        assert_eq!(a.keyframes.len(), 2);
        assert!(a.keyframes[1].segments.len() > a.keyframes[0].segments.len());
        let other = bush(12);
        // The bush has no random draws, so a seed changes identities only.
        assert_ne!(
            a.keyframes[1].segments[0].id,
            other.keyframes[1].segments[0].id
        );
    }

    /// The shed log holds every part a keyframe records as shed, at the
    /// same age, and nothing that stood to the end.
    #[test]
    fn the_shed_log_agrees_with_the_keyframes() {
        let growth = bush(11);
        let log = growth.shed();
        // Its leaves live two years, so it sheds leaves.
        assert!(!log.organs.is_empty());
        let segments: HashMap<u64, &ShedSegment> =
            log.segments.iter().map(|part| (part.id, part)).collect();
        // How many organs a keyframe shows falling at each age, by type:
        // the log counts at least those.
        let mut shown: HashMap<(u16, u64), BTreeSet<u64>> = HashMap::new();
        for graph in &growth.keyframes {
            for segment in &graph.segments {
                assert_eq!(
                    segment.shed,
                    segments.get(&segment.id).map(|part| part.shed),
                    "segment {}",
                    segment.id
                );
            }
            for organ in &graph.organs {
                if let Some(shed) = organ.shed {
                    assert!(shed > organ.born, "organ {}", organ.id);
                    shown
                        .entry((organ.organ, shed.to_bits()))
                        .or_default()
                        .insert(organ.id);
                }
            }
        }
        for ((organ, shed), ids) in &shown {
            let fallen = log
                .organs
                .iter()
                .find(|fallen| fallen.organ == *organ && fallen.shed.to_bits() == *shed)
                .expect("a keyframe's shed organ is in the log");
            assert!(fallen.count as usize >= ids.len());
        }
        for fallen in &log.organs {
            assert!(fallen.count > 0 && fallen.size > 0.0 && fallen.area >= 0.0);
        }
        assert!(
            log.organs
                .windows(2)
                .all(|pair| { (pair[0].shed, pair[0].organ) < (pair[1].shed, pair[1].organ) })
        );
        for part in &log.segments {
            assert!(part.shed > part.born && part.radius > 0.0 && part.length > 0.0);
        }
        assert!(log.segments.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    /// Organs fall the step they are gone, counted by type each time they
    /// go; a kept organ's shed age is its last going, unless it stands at
    /// the end.
    #[test]
    fn litter_counts_organs_as_they_fall() {
        let types = vec![
            OrganType {
                name: "leaf".into(),
                kind: OrganKind::Leaf,
                area: 2.0,
            },
            OrganType {
                name: "fruit".into(),
                kind: OrganKind::Fruit,
                area: 0.0,
            },
        ];
        // Symbols 0 and 1 are the two types.
        let index = [0_u16, 1];
        let mut litter = Litter::default();
        let steps: [&[(u64, u16, f64)]; 5] = [
            &[(1, 0, 0.5), (2, 0, 0.25), (3, 1, 1.0)],
            &[(1, 0, 0.5), (3, 1, 1.0), (4, 0, 1.0)],
            &[(3, 1, 1.0)],
            &[(1, 0, 0.5), (3, 1, 1.0)],
            &[(3, 1, 1.0)],
        ];
        let mut scene = Scene::default();
        for (step, organs) in steps.iter().enumerate() {
            let step = u32::try_from(step).unwrap();
            litter.step(step, 0.5, organs.iter().copied(), &types, &index);
            if step == 0 {
                scene.organs = organs
                    .iter()
                    .map(|&(id, symbol, size)| OrganInstance {
                        id,
                        symbol,
                        position: Vec3::ZERO,
                        frame: math::Frame::UPRIGHT,
                        size,
                        born: 0.0,
                        segment: None,
                        node: None,
                    })
                    .collect();
                litter.keep(&scene);
            }
        }
        let fallen: Vec<(u16, f64, u32, f64, f64)> = litter
            .fallen
            .iter()
            .map(|f| (f.organ, f.shed, f.count, f.size, f.area))
            .collect();
        assert_eq!(
            fallen,
            [
                (0, 0.5, 1, 0.25, 0.125),
                (0, 1.0, 2, 1.5, 2.5),
                (0, 2.0, 1, 0.5, 0.5),
            ]
        );
        // Organ 1 went at step 2 and again at step 4: its last going.
        assert_eq!(litter.last_seen(1), Some(3));
        assert_eq!(litter.last_seen(2), Some(0));
        // Organ 3 stands at the end.
        assert_eq!(litter.last_seen(3), None);
    }

    #[test]
    fn identities_persist_between_keyframes_and_record_shedding() {
        let growth = bush(11);
        let (young, old) = (&growth.keyframes[0], &growth.keyframes[1]);
        for segment in &young.segments {
            if let Some(later) = old.segments.iter().find(|other| other.id == segment.id) {
                assert_eq!(later.born, segment.born);
                assert!(later.radius >= segment.radius, "radii never shrink");
                assert!(segment.shed.is_none() || segment.shed > Some(8.0));
            } else {
                let shed = segment.shed.expect("a vanished segment has a shed age");
                assert!(shed > young.age && shed <= old.age);
            }
        }
        // Leaves live two years, so some leaves of the young bush are shed.
        assert!(young.organs.iter().any(|organ| organ.shed.is_some()));
    }

    #[test]
    fn pipe_radii_decrease_toward_the_tips() {
        let growth = bush(11);
        let graph = &growth.keyframes[1];
        for segment in &graph.segments {
            if let Some(parent) = segment.parent {
                assert!(
                    graph.segments[parent as usize].radius + 1e-12 >= segment.radius,
                    "a child is never thicker than its parent"
                );
            }
        }
        assert!(graph.segments[0].radius > 0.004);
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let program = Program::compile(BUSH).unwrap();
        let params = program.resolve_params(&BTreeMap::new()).unwrap();
        let mut bad = settings(5.0, vec![6.0]);
        assert!(grow(&program, &params, &bad).is_err());
        bad.keyframes = vec![2.0];
        bad.dt = 0.0;
        assert!(grow(&program, &params, &bad).is_err());
    }

    /// A shoot that creeps west at ankle height and stops short of
    /// whatever it meets (`substrate@1`, G3).
    fn creeper(substrate: Option<crate::substrate::SubstratePreset>) -> f64 {
        let program = Program::compile(
            "lsystem creeper 1;
             module A queries substrate;
             tool substrate@1 {};
             axiom f(0.1) steer(-1, 0, 0, 1) A;
             rule A : sd > 0.06 -> F(0.05) A;",
        )
        .unwrap();
        let params = program.resolve_params(&BTreeMap::new()).unwrap();
        let mut conditions = Conditions::preset(crate::spec::Environment::Open);
        conditions.substrate = substrate.map(crate::substrate::Substrate::preset);
        let growth = grow(
            &program,
            &params,
            &GrowthSettings {
                conditions,
                ..settings(12.0, vec![12.0])
            },
        )
        .unwrap();
        growth.keyframes[0]
            .segments
            .iter()
            .map(|segment| segment.end.x)
            .fold(0.0, f64::min)
    }

    #[test]
    fn a_shoot_feels_the_substrate_it_grows_against() {
        // On open ground it creeps its full 0.6 m; in a cleft it stops at
        // the west block's face, 0.25 m out.
        let open = creeper(None);
        assert!((open + 0.6).abs() < 1.0e-9, "open ground: {open}");
        let cleft = creeper(Some(crate::substrate::SubstratePreset::Cleft));
        assert!((-0.25..-0.15).contains(&cleft), "cleft: {cleft}");
    }
}
