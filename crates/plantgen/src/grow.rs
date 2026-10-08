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

use std::collections::HashMap;

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
    /// Every segment and organ shed while the plant grew, as last seen:
    /// the self-pruned branches, fallen leaves, cones and fruit a floor
    /// under it would hold. No package stores it.
    #[must_use]
    pub fn shed(&self) -> &ShedLog {
        &self.shed
    }
}

/// What a plant shed while it grew, each part as it was the last step it
/// stood, in id order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShedLog {
    pub segments: Vec<ShedSegment>,
    pub organs: Vec<ShedOrgan>,
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

/// A shed organ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShedOrgan {
    pub id: u64,
    /// Its organ type's index in [`Growth::organ_types`].
    pub organ: u16,
    /// Age it grew at, and age it was shed at, years.
    pub born: f64,
    pub shed: f64,
    pub size: f64,
    /// Height above the ground, metres.
    pub height: f64,
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

/// Girth bookkeeping across steps: the annual rings laid down on each
/// segment and the widest radius it has had.
#[derive(Default)]
struct Girth {
    widest: HashMap<u64, f64>,
    ring_area: HashMap<u64, f64>,
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
    let (steps, keyframe_steps) = schedule(settings)?;
    let organ_area = tools::organ_areas(program, params)?;
    let (organ_types, organ_index) = organ_types(program, &organ_area);

    let limits = settings.limits;
    let mut deriver = Deriver::new(program, params, &limits);
    let mut interpreter = Interpreter::new(program, params, &limits);
    let mut tool_state = ToolState::default();
    let neighbourhood = settings.conditions.neighbourhood();
    let around = Around {
        neighbourhood: &neighbourhood,
        host: settings.host.as_deref(),
    };
    let mut string = deriver.axiom(settings.seed, settings.dt)?;
    let mut stats = GrowthStats {
        steps,
        ..GrowthStats::default()
    };
    let mut girth = Girth::default();
    let mut last_seen: HashMap<u64, u32> = HashMap::new();
    // Each part as it was the last step it stood, for the shed log.
    let mut last_segments: HashMap<u64, (u32, ShedSegment)> = HashMap::new();
    let mut last_organs: HashMap<u64, (u32, ShedOrgan)> = HashMap::new();
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
        for segment in &scene.segments {
            last_seen.insert(segment.id, step);
        }
        for organ in &scene.organs {
            last_seen.insert(organ.id, step);
        }
        for (segment, radius) in scene.segments.iter().zip(&radii) {
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
            last_segments.insert(segment.id, (step, part));
        }
        for organ in &scene.organs {
            let part = ShedOrgan {
                id: organ.id,
                organ: organ_index[usize::from(organ.symbol)],
                born: organ.born,
                shed: 0.0,
                size: organ.size,
                height: organ.position.y,
            };
            last_organs.insert(organ.id, (step, part));
        }

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
            keyframes.push(snapshot(
                &scene,
                &radii,
                &output.organ_light,
                &organ_index,
                clock.t,
            ));
            next_keyframe += 1;
        }
        if step == steps {
            break;
        }
        string = deriver.derive(&string, &output.env, clock)?;
    }

    let shed_at = |id: u64| -> Option<f64> {
        let seen = *last_seen.get(&id)?;
        (seen < steps).then(|| f64::from(seen + 1) * settings.dt)
    };
    for graph in &mut keyframes {
        for segment in &mut graph.segments {
            segment.shed = shed_at(segment.id);
        }
        for organ in &mut graph.organs {
            organ.shed = shed_at(organ.id);
        }
    }
    let shed_age = |seen: u32| (seen < steps).then(|| f64::from(seen + 1) * settings.dt);
    let mut shed = ShedLog {
        segments: last_segments
            .into_values()
            .filter_map(|(seen, part)| shed_age(seen).map(|shed| ShedSegment { shed, ..part }))
            .collect(),
        organs: last_organs
            .into_values()
            .filter_map(|(seen, part)| shed_age(seen).map(|shed| ShedOrgan { shed, ..part }))
            .collect(),
    };
    shed.segments.sort_unstable_by_key(|part| part.id);
    shed.organs.sort_unstable_by_key(|part| part.id);
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
    use std::collections::BTreeMap;

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
        let organs: HashMap<u64, &ShedOrgan> =
            log.organs.iter().map(|part| (part.id, part)).collect();
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
                assert_eq!(
                    organ.shed,
                    organs.get(&organ.id).map(|part| part.shed),
                    "organ {}",
                    organ.id
                );
                if let Some(part) = organs.get(&organ.id) {
                    assert_eq!((part.organ, part.born), (organ.organ, organ.born));
                }
            }
        }
        for part in &log.segments {
            assert!(part.shed > part.born && part.radius > 0.0 && part.length > 0.0);
        }
        assert!(log.segments.windows(2).all(|pair| pair[0].id < pair[1].id));
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
}
