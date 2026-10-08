//! Quality profiles: how much detail each level of a package keeps and how
//! large its impostors are. The profile is part of the package key.

use serde::Serialize;

use crate::mesh::LodSpec;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Quality {
    pub name: &'static str,
    /// LOD0 (closest) to LOD3 (farthest mesh).
    pub lods: [LodSpec; 4],
    /// Views along each side of the hemi-octahedral impostor grid.
    pub impostor_views: usize,
    /// Pixels along each side of one impostor view.
    pub impostor_size: usize,
}

const fn lod(min_radius: f64, ring_edge: f64, max_sides: u32, bend: f64, cluster: f64) -> LodSpec {
    LodSpec {
        min_radius,
        ring_edge,
        min_sides: 3,
        max_sides,
        bend,
        cluster,
    }
}

/// Fast builds for iterating on a species.
pub const DRAFT: Quality = Quality {
    name: "draft",
    lods: [
        lod(0.0, 0.08, 10, 0.0, 0.0),
        lod(0.006, 0.16, 6, 10.0, 0.0),
        lod(0.02, 0.35, 5, 18.0, 0.8),
        lod(0.05, 0.6, 4, 28.0, 1.8),
    ],
    impostor_views: 6,
    impostor_size: 64,
};

/// The default for library builds.
pub const STANDARD: Quality = Quality {
    name: "standard",
    lods: [
        lod(0.0, 0.04, 16, 0.0, 0.0),
        lod(0.004, 0.1, 8, 8.0, 0.0),
        lod(0.015, 0.25, 6, 15.0, 0.6),
        lod(0.04, 0.5, 4, 25.0, 1.5),
    ],
    impostor_views: 8,
    impostor_size: 128,
};

pub const PROFILES: [Quality; 2] = [DRAFT, STANDARD];

#[must_use]
pub fn profile(name: &str) -> Option<Quality> {
    PROFILES.into_iter().find(|quality| quality.name == name)
}
