//! The grown plant: one [`PlantGraph`] per keyframe age.
//!
//! A graph stores the plant's structure, not its triangles: segments with
//! radii and lineage-derived identities, and organ attachments. Meshes,
//! impostors and colliders are all derived from it (see [`crate::mesh`]).
//! Positions are metres in the plant's own frame: +Y up, origin at the base
//! of the stem.

use serde::{Deserialize, Serialize};

use crate::lsys::OrganKind;
use crate::math::Vec3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphSegment {
    /// Stable identity: the same branch keeps it at every age.
    pub id: u64,
    /// Index of the segment this one grows from.
    pub parent: Option<u32>,
    /// True when the segment starts a new axis (a branch) rather than
    /// continuing its parent's.
    pub lateral: bool,
    /// Branch order: 0 for the stem.
    pub order: u16,
    pub start: Vec3,
    pub end: Vec3,
    /// Radius at the start of the segment, in metres.
    pub radius: f64,
    /// Plant age at which the segment appeared, in years.
    pub born: f64,
    /// Plant age at which the segment is shed, if it is shed before the end
    /// of growth.
    pub shed: Option<f64>,
    /// 0 for wood; for a segment drawn with one of the program's bodies,
    /// that body's index among them plus one (see
    /// [`crate::lsys::Program::bodies`]).
    #[serde(default)]
    pub body: u8,
    /// The turtle's left vector where a body segment was drawn: the wide
    /// direction of a flattened body such as a pad. Zero for wood.
    #[serde(default)]
    pub left: Vec3,
    /// The structure it was drawn in ([`PlantGraph::structures`]), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<u32>,
    /// How young the program drew it, 0 grown to 1 just emerged (the
    /// L-system's `young`). Package graphs do not record it; it is drawn
    /// into the wood's colour.
    #[serde(default, skip_serializing_if = "grown")]
    pub young: f32,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn grown(young: &f32) -> bool {
    *young == 0.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphOrgan {
    pub id: u64,
    /// Index into [`OrganType`]s of the growth this graph belongs to.
    pub organ: u16,
    pub segment: Option<u32>,
    pub position: Vec3,
    /// Turtle heading and left vector where the organ was placed.
    pub heading: Vec3,
    pub left: Vec3,
    pub size: f64,
    /// Plant age it was born at: an organ a rule turned into another
    /// keeps the birth (and the id) it had (organ phases, spec rework M7).
    pub born: f64,
    /// Plant age at which it is shed, or becomes another organ type (a
    /// blade turned to thatch), if before the end of growth.
    pub shed: Option<f64>,
    /// Light exposure from 0 (deep shade) to 1 (open sky).
    pub light: f64,
    /// The structure it was placed in ([`PlantGraph::structures`]), if
    /// any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<u32>,
    /// The adultness of the shoot it was placed on, from 0 (juvenile) to
    /// 1 (adult), if the program set one (spec rework P6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adult: Option<f32>,
}

/// A structure of the grown plant (spec rework M7): a fern's frond, a
/// grass leaf. What is drawn in it, segments, organs and structures, is
/// within it; it is within another, or stands on the axis that bears it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphStructure {
    /// Stable identity: the same structure keeps it at every age.
    pub id: u64,
    /// Index into [`StructureType`]s of the growth this graph belongs to.
    pub structure: u16,
    pub within: Option<u32>,
    /// Plant age at which it appeared, in years.
    pub born: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrganType {
    pub name: String,
    pub kind: OrganKind,
    /// Shading area of a size-1 organ, m².
    pub area: f64,
    /// What the organ is: one of the vocabulary's organs, or `piece`
    /// (`crate::organs`). A grown plant knows its program's term only; a
    /// package gives every type's, its looks' included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
}

/// A structure type a program declares: its name and what it is, one of
/// the vocabulary's organs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureType {
    pub name: String,
    pub term: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlantGraph {
    /// Plant age at this keyframe, in years.
    pub age: f64,
    pub height: f64,
    pub segments: Vec<GraphSegment>,
    pub organs: Vec<GraphOrgan>,
    /// Its structures, each before the structures within it; none for a
    /// program that declares none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structures: Vec<GraphStructure>,
}

impl PlantGraph {
    /// One-sided leaf area of every organ, m².
    #[must_use]
    pub fn leaf_area(&self, types: &[OrganType]) -> f64 {
        // A fold from +0: float `Sum` starts at -0, which a plant without
        // organs would print and record as `-0`.
        self.organs
            .iter()
            .map(|organ| {
                types
                    .get(usize::from(organ.organ))
                    .map_or(0.0, |kind| kind.area)
                    * organ.size
                    * organ.size
            })
            .fold(0.0, |total, area| total + area)
    }

    /// Mean and lowest-decile light of the organs, or `None` without
    /// organs: how deep the foliage is in its own shade.
    #[must_use]
    pub fn organ_light(&self) -> Option<(f64, f64)> {
        let mut lights: Vec<f64> = self.organs.iter().map(|organ| organ.light).collect();
        if lights.is_empty() {
            return None;
        }
        lights.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss)]
        let mean = lights.iter().sum::<f64>() / lights.len() as f64;
        Some((mean, lights[lights.len() / 10]))
    }

    /// Axis-aligned bounds of segments and organs, or `None` when empty.
    #[must_use]
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self
            .segments
            .iter()
            .flat_map(|segment| {
                let pad = Vec3::new(segment.radius, segment.radius, segment.radius);
                [
                    segment.start - pad,
                    segment.start + pad,
                    segment.end - pad,
                    segment.end + pad,
                ]
            })
            .chain(self.organs.iter().map(|organ| organ.position));
        let first = points.next()?;
        Some(points.fold((first, first), |(low, high), point| {
            (low.min(point), high.max(point))
        }))
    }

    /// For each segment, the child that continues its axis, if any.
    #[must_use]
    pub fn continuations(&self) -> Vec<Option<u32>> {
        let mut next = vec![None; self.segments.len()];
        for (index, segment) in self.segments.iter().enumerate() {
            if let Some(parent) = segment.parent
                && !segment.lateral
                && next[parent as usize].is_none()
            {
                next[parent as usize] = u32::try_from(index).ok();
            }
        }
        next
    }

    /// Radius at the end of each segment: the start radius of its
    /// continuation, or its own radius at a tip.
    #[must_use]
    pub fn end_radii(&self) -> Vec<f64> {
        self.continuations()
            .iter()
            .zip(&self.segments)
            .map(|(next, segment)| match next {
                Some(index) => self.segments[*index as usize].radius.min(segment.radius),
                None => segment.radius * 0.6,
            })
            .collect()
    }

    /// Diameter of the stem at 1.3 m (breast height), in metres.
    #[must_use]
    pub fn diameter_at_breast_height(&self) -> Option<f64> {
        self.segments
            .iter()
            .filter(|segment| segment.order == 0)
            .find(|segment| {
                let (low, high) = if segment.start.y <= segment.end.y {
                    (segment.start.y, segment.end.y)
                } else {
                    (segment.end.y, segment.start.y)
                };
                (low..=high).contains(&1.3)
            })
            .map(|segment| segment.radius * 2.0)
    }

    /// Crown base: where the lowest branch that still bears organs leaves
    /// the stem (or the lowest organ on the stem itself). Dead and bare
    /// branches below it do not count. `None` without organs, or without a
    /// stem (no segment of order 0), as in grasses and herbs, whose shoots
    /// all rise from the ground.
    #[must_use]
    pub fn crown_base(&self) -> Option<f64> {
        if !self.segments.iter().any(|segment| segment.order == 0) {
            return None;
        }
        // Height at which each segment's first-order branch leaves the
        // stem; parents always come before their children.
        let mut branch_base: Vec<Option<f64>> = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            let base = if segment.order == 0 {
                None
            } else if segment.order == 1 && segment.lateral {
                Some(segment.start.y)
            } else {
                segment
                    .parent
                    .and_then(|parent| branch_base.get(parent as usize).copied().flatten())
            };
            branch_base.push(base);
        }
        self.organs
            .iter()
            .map(|organ| {
                organ
                    .segment
                    .and_then(|segment| branch_base.get(segment as usize).copied().flatten())
                    .unwrap_or(organ.position.y)
            })
            .min_by(f64::total_cmp)
    }

    /// Widest horizontal distance of any organ from the stem axis, or of any
    /// segment when there are no organs.
    #[must_use]
    pub fn crown_radius(&self) -> f64 {
        let radial = |point: Vec3| (point.x * point.x + point.z * point.z).sqrt();
        if self.organs.is_empty() {
            self.segments
                .iter()
                .map(|segment| radial(segment.end))
                .fold(0.0, f64::max)
        } else {
            self.organs
                .iter()
                .map(|organ| radial(organ.position))
                .fold(0.0, f64::max)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plant_that_shed_everything_measures_positive_zero() {
        let graph = PlantGraph {
            age: 30.0,
            height: 0.0,
            segments: Vec::new(),
            organs: Vec::new(),
            structures: Vec::new(),
        };
        assert_eq!(graph.leaf_area(&[]).to_bits(), 0.0_f64.to_bits());
        assert_eq!(graph.crown_radius().to_bits(), 0.0_f64.to_bits());
        assert_eq!(graph.crown_base(), None);
        assert_eq!(graph.organ_light(), None);
        assert_eq!(graph.diameter_at_breast_height(), None);
    }

    /// A clump of shoots from the ground, every one in brackets, has no
    /// stem and so no crown base.
    #[test]
    fn a_plant_without_a_stem_has_no_crown_base() {
        let shoot = GraphSegment {
            id: 1,
            parent: None,
            lateral: true,
            order: 1,
            start: Vec3::ZERO,
            end: Vec3::new(0.0, 0.5, 0.0),
            radius: 0.003,
            born: 1.0,
            shed: None,
            body: 0,
            left: Vec3::ZERO,
            within: None,
            young: 0.0,
        };
        let leaf = GraphOrgan {
            id: 2,
            organ: 0,
            segment: Some(0),
            position: Vec3::new(0.0, 0.3, 0.0),
            heading: Vec3::X,
            left: Vec3::Z,
            size: 0.1,
            born: 1.0,
            shed: None,
            light: 1.0,
            within: None,
            adult: None,
        };
        let mut graph = PlantGraph {
            age: 1.0,
            height: 0.5,
            segments: vec![shoot],
            organs: vec![leaf],
            structures: Vec::new(),
        };
        assert_eq!(graph.crown_base(), None);
        // The same shoot drawn outside brackets is a stem.
        graph.segments[0].order = 0;
        assert_eq!(graph.crown_base(), Some(0.3));
    }
}
