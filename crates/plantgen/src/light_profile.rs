//! The light a grown plant stands in, by height: a check of the shade a
//! stand casts against light measured in real canopies.
//!
//! A plant grown in a stand of its like (`crate::conditions::Layout`)
//! stands at the middle of a square cell as wide as its stems stand apart.
//! [`profile`] samples the overcast-sky light at points across that cell,
//! layer by layer from the ground to the top, as the `light@1` tool weighs
//! it for a living tip with no leaves of its own: through the plant's own
//! foliage in voxels, and past its neighbours' crowns and the stand beyond
//! them ([`crate::lsys::tools::sky_light`]). Each layer's mean, median and
//! 10th and 90th percentiles are what a light sensor carried through the
//! canopy reads. Parker, Davis and Chapotin (2002, Tree Physiology 22:
//! 147-157, fig. 3) measured the same profile in five Douglas-fir and
//! western hemlock stands at Wind River from 20 to 500 years, the oldest
//! letting through about 0.9 of the sky's light at two thirds of its
//! height, 0.45 at half, 0.25 at a third and under 0.1 near the ground.
//!
//! `plantc grow ... --light` prints the profile at the last age.

use crate::conditions::{Around, Conditions, Crown};
use crate::graph::{OrganType, PlantGraph};
use crate::lsys::Limits;
use crate::math::Vec3;

/// The light at one height across the cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    /// Height above the ground, metres.
    pub height_m: f64,
    /// Mean light over the layer's points, as a share of the open sky's.
    pub mean: f64,
    pub p10: f64,
    pub median: f64,
    pub p90: f64,
}

/// The plant's cell, metres from its stem to each side: half the distance
/// between stems in its layout at its age, or with no layout its crown's
/// radius and as much again.
#[must_use]
pub fn half_width(graph: &PlantGraph, types: &[OrganType], conditions: &Conditions) -> f64 {
    let crown = crown(graph, types);
    match (conditions.layout(), crown) {
        (Some(layout), Some(crown)) => 0.5 * layout.spacing_m(&crown, graph.age),
        (_, Some(crown)) => 2.0 * crown.radius_m,
        _ => 1.0,
    }
}

fn foliage(graph: &PlantGraph, types: &[OrganType]) -> Vec<(Vec3, f64)> {
    graph
        .organs
        .iter()
        .filter(|organ| organ.shed.is_none_or(|shed| shed > graph.age))
        .map(|organ| {
            let area = types
                .get(usize::from(organ.organ))
                .map_or(0.0, |kind| kind.area);
            (organ.position, area * organ.size * organ.size)
        })
        .filter(|(_, area)| *area > 0.0)
        .collect()
}

fn crown(graph: &PlantGraph, types: &[OrganType]) -> Option<Crown> {
    Crown::of(foliage(graph, types).into_iter())
}

/// The light at each height from `step_m` / 2 to the plant's top, every
/// `step_m` metres, sampled on a square grid of `step_m` across the
/// plant's cell ([`half_width`]), with the light tool's voxel `cell` and
/// `extinction` (pinidae's `light@1`: 0.4 m and 0.35).
///
/// # Errors
///
/// Fails when the light's voxels pass the growth limit.
pub fn profile(
    graph: &PlantGraph,
    types: &[OrganType],
    conditions: &Conditions,
    step_m: f64,
    (cell, extinction): (f64, f64),
) -> Result<Vec<Layer>, String> {
    let step_m = step_m.max(0.1);
    let foliage = foliage(graph, types);
    let neighbourhood = conditions.neighbourhood();
    let mut envelopes = conditions.envelopes().to_vec();
    let mut far = None;
    if let (Some(layout), Some(crown)) = (conditions.layout(), Crown::of(foliage.iter().copied())) {
        envelopes.extend(layout.envelopes(&crown, graph.age));
        far = layout.far(&crown, graph.age);
    }
    let around = Around {
        neighbourhood: &neighbourhood,
        envelopes: &envelopes,
        far: far.as_ref(),
        host: None,
        substrate: None,
        sun: None,
        soil: None,
    };
    let half = half_width(graph, types, conditions);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let across = ((2.0 * half / step_m).floor() as usize).max(1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let layers = ((graph.height / step_m).ceil() as usize).max(1);
    let mut points = Vec::with_capacity(across * across * layers);
    #[allow(clippy::cast_precision_loss)]
    let at = |i: usize| -half + (i as f64 + 0.5) * (2.0 * half / across as f64);
    for layer in 0..layers {
        #[allow(clippy::cast_precision_loss)]
        let y = (layer as f64 + 0.5) * step_m;
        for i in 0..across {
            for k in 0..across {
                points.push(Vec3::new(at(i), y, at(k)));
            }
        }
    }
    let limits = Limits {
        max_voxels: 50_000_000,
        ..Limits::default()
    };
    let light = crate::lsys::tools::sky_light(
        &points,
        &foliage,
        (cell, extinction),
        graph.height,
        &around,
        &limits,
    )
    .map_err(|error| error.to_string())?;
    Ok(light
        .chunks(across * across)
        .enumerate()
        .map(|(layer, values)| {
            let mut sorted = values.to_vec();
            sorted.sort_by(f64::total_cmp);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let quantile = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
            #[allow(clippy::cast_precision_loss)]
            Layer {
                height_m: (layer as f64 + 0.5) * step_m,
                mean: values.iter().sum::<f64>() / values.len() as f64,
                p10: quantile(0.1),
                median: quantile(0.5),
                p90: quantile(0.9),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphOrgan;
    use crate::lsys::program::OrganKind;
    use crate::spec::Environment;

    fn leafy(height: f64) -> (PlantGraph, Vec<OrganType>) {
        let mut organs = Vec::new();
        let mut id = 0;
        // A crown 2 m across, one organ in each 0.4 m voxel, its top
        // tenth of the height deep.
        let at = |i: i32| f64::from(i) * 0.4;
        for y in (0..3).map(|i| height * 0.9 + at(i)) {
            for x in (-2..=2).map(at) {
                for z in (-2..=2).map(at) {
                    id += 1;
                    organs.push(GraphOrgan {
                        id,
                        organ: 0,
                        segment: None,
                        position: Vec3::new(x, y, z),
                        heading: Vec3::new(0.0, 1.0, 0.0),
                        left: Vec3::new(1.0, 0.0, 0.0),
                        size: 1.0,
                        born: 0.0,
                        shed: None,
                        light: 1.0,
                        within: None,
                        adult: None,
                    });
                }
            }
        }
        let graph = PlantGraph {
            age: 30.0,
            height,
            segments: Vec::new(),
            organs,
            structures: Vec::new(),
        };
        let types = vec![OrganType {
            name: "leaf".into(),
            kind: OrganKind::Leaf,
            area: 2.0,
            term: None,
        }];
        (graph, types)
    }

    #[test]
    fn open_ground_is_shaded_under_the_crown_and_open_beside_it() {
        let (graph, types) = leafy(10.0);
        let open = Conditions::preset(Environment::Open);
        let layers = profile(&graph, &types, &open, 1.0, (0.4, 0.35)).unwrap();
        assert_eq!(layers.len(), 10);
        // Just under the crown (8.5 m): dark under it, the open sky beside.
        let under = layers[8];
        assert!(under.p10 < 0.5 && under.p90 > 0.8, "{under:?}");
        assert!(layers.iter().all(|l| (0.0..=1.0).contains(&l.mean)));
    }

    #[test]
    fn a_stand_is_darker_than_open_ground() {
        let (graph, types) = leafy(10.0);
        let open = profile(
            &graph,
            &types,
            &Conditions::preset(Environment::Open),
            1.0,
            (0.4, 0.35),
        )
        .unwrap();
        let stand = profile(
            &graph,
            &types,
            &Conditions::preset(Environment::Interior),
            1.0,
            (0.4, 0.35),
        )
        .unwrap();
        assert!(stand[2].mean < open[2].mean, "{:?} {:?}", stand[2], open[2]);
    }
}
