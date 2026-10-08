//! A plant grown and dressed for drawing: what `plantc render` draws,
//! as a library call, so every renderer (PlantLab, `plantc`, a host's
//! review tool) draws the same plant from the same inputs.
//!
//! [`draw`] grows one variant of a spec to one age, gives each organ type
//! its look on a day of the year (organs gone that day left out, the others
//! at their stage's size), and meshes one level of detail with the
//! templates that cut its cards out and the bark pattern of its wood. A
//! guest (plant forms F7) is drawn standing on its host unless asked
//! alone. Nothing here changes what a spec grows into: these are the steps
//! `plantc` takes, in its order.

use std::borrow::Cow;

use crate::body::BodyLook;
use crate::conditions::Conditions;
use crate::graph::PlantGraph;
use crate::grow::{Growth, GrowthSettings, grow};
use crate::library::Library;
use crate::looks::{self, Look};
use crate::lsys::{Limits, Neighbourhood};
use crate::mesh::{self, PlantMesh};
use crate::quality::Quality;
use crate::spec::{Environment, PlantSpec};
use crate::templates::Templates;

/// The environment of the species' typical site (its `conditions.json`)
/// when its variants grow in it, else its first.
#[must_use]
pub fn typical_environment(spec: &PlantSpec, library: &Library) -> Environment {
    library
        .entry(&spec.id)
        .and_then(|entry| entry.conditions().ok().flatten())
        .and_then(|conditions| conditions.preset)
        .filter(|preset| spec.variants.environments.contains(preset))
        .unwrap_or(spec.variants.environments[0])
}

/// The neighbourhood a spec's variants grow in for `environment`: the
/// spec's own when it overrides the preset, else the preset's.
#[must_use]
pub fn neighbourhood(spec: &PlantSpec, environment: Environment) -> Neighbourhood {
    spec.variant_list()
        .into_iter()
        .find(|variant| variant.environment == environment)
        .map_or_else(
            || environment.neighbourhood(),
            |variant| variant.neighbourhood,
        )
}

/// Grow one variant of `spec` for `years`, keeping its graph at each of
/// `keyframes`. `program` replaces the spec's own program when given.
///
/// # Errors
///
/// When the program does not load or compile, the host does not grow, or
/// growth fails.
pub fn grow_variant(
    spec: &PlantSpec,
    library: &Library,
    program: Option<&str>,
    environment: Environment,
    seed: u64,
    keyframes: Vec<f64>,
    years: f64,
) -> Result<Growth, String> {
    let (program, params) = match program {
        Some(source) => spec.program_from(source),
        None => spec.program_in(library),
    }
    .map_err(|error| error.to_string())?;
    let settings = GrowthSettings {
        seed,
        dt: spec.growth.step,
        years,
        keyframes,
        conditions: Conditions::in_neighbourhood(environment, neighbourhood(spec, environment)),
        limits: Limits::default(),
        host: spec
            .host_geometry_in(library)
            .map_err(|error| error.to_string())?,
    };
    grow(&program, &params, &settings).map_err(|error| format!("{}: {error}", spec.id))
}

/// Each organ type's look on `day`, and its size that day as a share of
/// its size in fruit (0 when it is gone).
#[must_use]
pub fn looks_of(spec: &PlantSpec, growth: &Growth, day: f64) -> (Vec<Look>, Vec<f64>) {
    spec.appearance.looks_on(
        growth
            .organ_types
            .iter()
            .map(|organ| (organ.name.as_str(), organ.kind)),
        Some(day),
    )
}

/// `graph` as drawn with its organ types' `sizes` on a day: organs gone
/// left out, the others at their stage's size (`looks::staged`).
#[must_use]
pub fn drawn<'a>(graph: &'a PlantGraph, sizes: &[f64]) -> Cow<'a, PlantGraph> {
    looks::staged(graph, sizes).map_or(Cow::Borrowed(graph), Cow::Owned)
}

/// One body look per body type of a grown plant.
#[must_use]
pub fn bodies_of(spec: &PlantSpec, growth: &Growth) -> Vec<BodyLook> {
    spec.appearance
        .body_looks(growth.body_types.iter().map(String::as_str))
}

/// A grown plant's card templates: its organs', then its bodies' spines,
/// with the bark pattern of `spec` for its wood.
#[must_use]
pub fn templates_of(
    spec: &PlantSpec,
    looks: &[Look],
    bodies: &[BodyLook],
    growth: &Growth,
) -> Templates {
    let named: Vec<(&str, &BodyLook)> = growth
        .body_types
        .iter()
        .map(String::as_str)
        .zip(bodies)
        .collect();
    Templates::for_plant(looks, &named).with_bark(spec.appearance.bark_params())
}

/// A guest's mesh standing on its host's: the host's wood and cards, then
/// the guest's, its cards' templates shifted past the host's
/// `host_types` organ types.
#[must_use]
pub fn on_host(host: PlantMesh, guest: PlantMesh, host_types: usize) -> PlantMesh {
    let mut plant = host;
    let first = u32::try_from(plant.wood.positions.len()).unwrap_or(u32::MAX);
    let wood = guest.wood;
    plant.wood.positions.extend(wood.positions);
    plant.wood.normals.extend(wood.normals);
    plant.wood.uvs.extend(wood.uvs);
    plant.wood.colors.extend(wood.colors);
    plant.wood.births.extend(wood.births);
    plant.wood.sheds.extend(wood.sheds);
    plant.wood.levels.extend(wood.levels);
    plant
        .wood
        .indices
        .extend(wood.indices.iter().map(|index| index + first));
    let shift = u8::try_from(host_types).unwrap_or(u8::MAX);
    plant.cards.extend(guest.cards.into_iter().map(|mut card| {
        card.template = card.template.saturating_add(shift);
        card
    }));
    plant
}

/// What to draw: one variant of a spec at one age, day and level.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub spec: &'a PlantSpec,
    pub library: &'a Library,
    /// Replaces the spec's program when given (`plantc --program`).
    pub program: Option<&'a str>,
    pub environment: Environment,
    pub seed: u64,
    /// Plant age, years.
    pub age: f64,
    /// Day of the year, 1 to 365, that seasoned organs show.
    pub day: f64,
    /// Level of detail, 0 (nearest) to 3.
    pub level: usize,
    pub quality: &'a Quality,
    /// On level 0, draw organ types with part meshes as them, as a
    /// renderer does near the camera (`crate::parts`).
    pub parts: bool,
    /// Draw a guest without its host.
    pub alone: bool,
}

impl<'a> Request<'a> {
    /// The spec's typical site, its first seed and oldest keyframe, the
    /// package's day, level 0, without part meshes.
    #[must_use]
    pub fn typical(spec: &'a PlantSpec, library: &'a Library, quality: &'a Quality) -> Self {
        Self {
            spec,
            library,
            program: None,
            environment: typical_environment(spec, library),
            seed: spec.variants.seeds[0],
            age: spec.growth.keyframes.iter().copied().fold(0.0, f64::max),
            day: crate::package::DEFAULT_DAY,
            level: 0,
            quality,
            parts: false,
            alone: false,
        }
    }
}

/// A plant ready to draw.
#[derive(Debug, Clone)]
pub struct Drawing {
    /// The grown plant as drawn on the day (organs gone left out).
    pub graph: PlantGraph,
    pub growth: Growth,
    pub plant: PlantMesh,
    pub templates: Templates,
    /// The looks the cards' templates index: the host's first when the
    /// plant stands on one.
    pub looks: Vec<Look>,
    /// Whether the plant stands on its host.
    pub on_host: bool,
}

/// Grow and mesh `request`, as `plantc render` does.
///
/// # Errors
///
/// When the level is out of range, the plant or its host does not grow, or
/// the host is not in the library.
pub fn draw(request: &Request<'_>) -> Result<Drawing, String> {
    let spec = request.spec;
    let library = request.library;
    let lod = request
        .quality
        .lods
        .get(request.level)
        .ok_or("the level of detail must be 0 to 3")?;
    if request.parts && request.level != 0 {
        return Err("part meshes are drawn on the nearest level only".into());
    }
    let growth = grow_variant(
        spec,
        library,
        request.program,
        request.environment,
        request.seed,
        vec![request.age],
        request.age,
    )?;
    let (mut looks, sizes) = looks_of(spec, &growth, request.day);
    let graph = drawn(&growth.keyframes[0], &sizes).into_owned();
    let bodies = bodies_of(spec, &growth);
    let mut plant = if request.parts {
        let parts = crate::parts::part_meshes(&looks);
        let types = crate::parts::types(&looks, &parts);
        let mut plant = mesh::build_with(
            &graph,
            &looks,
            &bodies,
            &spec.appearance,
            &lod.for_height(graph.height),
            request.level,
            Some(&types),
        );
        crate::parts::place(&mut plant, &looks, &parts);
        plant
    } else {
        mesh::build(
            &graph,
            &looks,
            &bodies,
            &spec.appearance,
            &lod.for_height(graph.height),
            request.level,
        )
    };
    // A guest is drawn on its host (plant forms F7), unless alone.
    // Neither has fleshy bodies, so the templates are the organs' alone.
    let mut on = false;
    if let (Some(host), false, true) = (&spec.host, request.alone, bodies.is_empty()) {
        let host_spec = library
            .spec(&host.species)
            .map_err(|error| error.to_string())?;
        let host_growth = grow_variant(
            &host_spec,
            library,
            None,
            host.environment,
            host.seed,
            vec![host.age],
            host.age,
        )?;
        let (host_looks, host_sizes) = looks_of(&host_spec, &host_growth, request.day);
        let host_graph = &*drawn(&host_growth.keyframes[0], &host_sizes);
        let host_plant = mesh::build(
            host_graph,
            &host_looks,
            &[],
            &host_spec.appearance,
            &lod.for_height(host_graph.height),
            request.level,
        );
        plant = on_host(host_plant, plant, host_looks.len());
        let mut both = host_looks;
        both.append(&mut looks);
        looks = both;
        on = true;
    }
    let templates = if on {
        // The host's trunk carries its own bark.
        let host_bark = spec
            .host
            .as_ref()
            .and_then(|host| library.spec(&host.species).ok())
            .and_then(|host| host.appearance.bark_params());
        Templates::for_plant(&looks, &[]).with_bark(host_bark)
    } else {
        templates_of(spec, &looks, &bodies, &growth)
    };
    Ok(Drawing {
        graph,
        growth,
        plant,
        templates,
        looks,
        on_host: on,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_is_the_same_twice_and_has_wood_and_cards() {
        let library = Library::builtin();
        let spec = library.spec("acer-macrophyllum").expect("bigleaf maple");
        let quality = crate::quality::DRAFT;
        let mut request = Request::typical(&spec, library, &quality);
        request.age = 6.0;
        let first = draw(&request).expect("draws");
        let second = draw(&request).expect("draws");
        assert_eq!(first.plant, second.plant);
        assert!(first.plant.wood.triangle_count() > 0);
        assert!(!first.plant.cards.is_empty());
        assert!(!first.on_host);
    }
}
