//! Measures of a grown plant's form (growth plan G1): the crown's
//! outline, its width against the stem, and the size of its leaves.
//!
//! These measure what a species' references (`allometry`) do not: a crown
//! that grows into the same ball whatever the species, a stem too thin for
//! its crown, leaves too small for the tree. Nothing here changes growth.
//!
//! The crown is the plant's living organs. Its height runs from the lowest
//! organ $`y_0`$ to the highest $`y_1`$, cut into [`BANDS`] equal bands; a
//! band's radius $`r_k`$ is the [`OUTLINE_SHARE`] quantile of its organs'
//! distances from the stem axis, so a few far organs do not set it. With
//! $`R = \max_k r_k`$ and the band's middle at $`h_k \in (0, 1)`$ up the
//! crown, the outline's departure from an ellipse of the same height and
//! width is
//!
//! ```math
//! d = \sqrt{\frac{1}{K} \sum_k \left(\frac{r_k}{R} - \sqrt{1 - (2 h_k - 1)^2}\right)^2}
//! ```
//!
//! over the $`K`$ bands that hold organs: 0 for an orb, about 0.3 for a
//! cone or a vase. Round the stem, the widest band's organs fall in
//! [`SECTORS`] sectors of azimuth; the sectors' radii (the same quantile)
//! vary by their coefficient of variation, the crown's lobing: 0 for a
//! crown round as a coin seen from above.

use crate::graph::{OrganType, PlantGraph};
use crate::lsys::program::OrganKind;
use crate::math;

/// Horizontal bands the crown is cut into.
pub const BANDS: usize = 10;

/// Sectors of azimuth round the stem for the lobing.
pub const SECTORS: usize = 8;

/// Quantile of organ distances that makes a band's or sector's radius.
pub const OUTLINE_SHARE: f64 = 0.95;

/// The form of one grown plant at one age.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Form {
    /// Height of the plant, m.
    pub height: f64,
    /// Lowest and highest living organ, m.
    pub crown_low: f64,
    pub crown_high: f64,
    /// Widest band's radius, m ($`R`$).
    pub crown_radius: f64,
    /// Crown width over crown depth: $`2R / (y_1 - y_0)`$.
    pub width_to_depth: f64,
    /// Where the widest band lies, as a share of the crown's depth from
    /// its foot.
    pub widest_at: f64,
    /// Departure from an ellipse, $`d`$ (see the module).
    pub ellipse_departure: f64,
    /// Coefficient of variation of the sectors' radii round the widest
    /// band.
    pub lobing: f64,
    /// Diameter at breast height, m, where the plant has a stem there.
    pub dbh: Option<f64>,
    /// Crown width over dbh, where there is a dbh.
    pub width_to_dbh: Option<f64>,
    /// Median card length of the leaves (organs of kind `leaf`), m.
    pub leaf_length: Option<f64>,
    /// Living leaves.
    pub leaves: usize,
}

/// The form of `graph`, or `None` with fewer than [`BANDS`] living
/// organs.
// Band and sector counts are small; ages and shares are in [0, 1].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[must_use]
pub fn measure(graph: &PlantGraph, types: &[OrganType]) -> Option<Form> {
    let organs: Vec<_> = graph
        .organs
        .iter()
        .filter(|organ| organ.shed.is_none())
        .collect();
    if organs.len() < BANDS {
        return None;
    }
    let low = organs
        .iter()
        .map(|organ| organ.position.y)
        .fold(f64::INFINITY, f64::min);
    let high = organs
        .iter()
        .map(|organ| organ.position.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let depth = (high - low).max(1e-6);
    let radial = |x: f64, z: f64| math::sqrt(x * x + z * z);
    let mut bands: Vec<Vec<f64>> = vec![Vec::new(); BANDS];
    for organ in &organs {
        let share = ((organ.position.y - low) / depth).clamp(0.0, 1.0);
        let band = ((share * BANDS as f64) as usize).min(BANDS - 1);
        bands[band].push(radial(organ.position.x, organ.position.z));
    }
    let radii: Vec<Option<f64>> = bands.iter_mut().map(|band| quantile(band)).collect();
    let (widest, radius) = radii
        .iter()
        .enumerate()
        .filter_map(|(k, r)| r.map(|r| (k, r)))
        .fold(
            (0, 0.0),
            |best, (k, r)| if r > best.1 { (k, r) } else { best },
        );
    let radius = radius.max(1e-6);
    let mut sum = 0.0;
    let mut count = 0.0;
    for (k, r) in radii.iter().enumerate() {
        if let Some(r) = r {
            let h = (k as f64 + 0.5) / BANDS as f64;
            let ellipse = math::sqrt((1.0 - (2.0 * h - 1.0) * (2.0 * h - 1.0)).max(0.0));
            sum += (r / radius - ellipse).powi(2);
            count += 1.0;
        }
    }
    let mut sectors: Vec<Vec<f64>> = vec![Vec::new(); SECTORS];
    for organ in &organs {
        let share = ((organ.position.y - low) / depth).clamp(0.0, 1.0);
        if ((share * BANDS as f64) as usize).min(BANDS - 1) != widest {
            continue;
        }
        let angle = math::atan2(organ.position.z, organ.position.x) + std::f64::consts::PI;
        let sector = ((angle / std::f64::consts::TAU * SECTORS as f64) as usize).min(SECTORS - 1);
        sectors[sector].push(radial(organ.position.x, organ.position.z));
    }
    let sector_radii: Vec<f64> = sectors
        .iter_mut()
        .map(|s| quantile(s).unwrap_or(0.0))
        .collect();
    let mean = sector_radii.iter().sum::<f64>() / SECTORS as f64;
    let variance = sector_radii.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / SECTORS as f64;
    let dbh = graph.diameter_at_breast_height();
    let mut leaf_sizes: Vec<f64> = organs
        .iter()
        .filter(|organ| {
            types
                .get(usize::from(organ.organ))
                .is_some_and(|kind| kind.kind == OrganKind::Leaf)
        })
        .map(|organ| organ.size.abs())
        .collect();
    leaf_sizes.sort_by(f64::total_cmp);
    Some(Form {
        height: graph.height,
        crown_low: low,
        crown_high: high,
        crown_radius: radius,
        width_to_depth: 2.0 * radius / depth,
        widest_at: (widest as f64 + 0.5) / BANDS as f64,
        ellipse_departure: math::sqrt(sum / count),
        lobing: if mean > 0.0 {
            math::sqrt(variance) / mean
        } else {
            0.0
        },
        dbh,
        width_to_dbh: dbh.filter(|d| *d > 0.0).map(|d| 2.0 * radius / d),
        leaf_length: leaf_sizes.get(leaf_sizes.len() / 2).copied(),
        leaves: leaf_sizes.len(),
    })
}

/// The [`OUTLINE_SHARE`] quantile of `values` (sorted in place), or `None`
/// when empty.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn quantile(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * OUTLINE_SHARE).round() as usize;
    values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphOrgan;
    use crate::math::Vec3;

    fn crown(profile: impl Fn(f64) -> f64) -> PlantGraph {
        let mut organs = Vec::new();
        for i in 0..200_u32 {
            let h = (f64::from(i) + 0.5) / 200.0;
            for j in 0..16_u32 {
                let angle = f64::from(j) / 16.0 * std::f64::consts::TAU;
                let r = profile(h);
                organs.push(GraphOrgan {
                    id: u64::from(i * 16 + j),
                    organ: 0,
                    segment: None,
                    position: Vec3 {
                        x: r * math::cos(angle),
                        y: 2.0 + 10.0 * h,
                        z: r * math::sin(angle),
                    },
                    heading: Vec3 {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                    left: Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    size: 0.2,
                    born: 0.0,
                    shed: None,
                    light: 1.0,
                });
            }
        }
        PlantGraph {
            age: 10.0,
            height: 12.0,
            segments: Vec::new(),
            organs,
        }
    }

    #[test]
    fn an_orb_departs_little_from_an_ellipse_and_a_cone_much() {
        let types = [OrganType {
            name: "leaf".into(),
            kind: OrganKind::Leaf,
            area: 0.3,
        }];
        let orb = measure(
            &crown(|h| 5.0 * math::sqrt(1.0 - (2.0 * h - 1.0).powi(2))),
            &types,
        )
        .unwrap();
        let cone = measure(&crown(|h| 5.0 * (1.0 - h)), &types).unwrap();
        assert!(orb.ellipse_departure < 0.08, "{orb:?}");
        assert!(cone.ellipse_departure > 0.2, "{cone:?}");
        assert!(cone.widest_at < 0.15);
        assert!(orb.lobing < 0.01);
        assert_eq!(orb.leaves, 3200);
        assert!((orb.leaf_length.unwrap() - 0.2).abs() < 1e-12);
    }
}
