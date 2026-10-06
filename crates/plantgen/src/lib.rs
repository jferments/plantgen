//! Procedural plants for Project After.
//!
//! A species is data (a [`spec::PlantSpec`]); its form comes from a plant
//! program written in an open L-system language ([`lsys`]). The compiler
//! grows each variant once, from seed to old age, inside a synthetic
//! neighbourhood, and keeps a [`graph::PlantGraph`] at each keyframe age.
//! Meshes, level-of-detail chains and impostors are baked from those graphs
//! ([`mesh`], [`raster`], [`impostor`]), with organ cards cut out by
//! templates drawn from the species' looks ([`looks`], [`templates`]), into
//! a content-addressed `.afterplant` package ([`package`]). Fleshy bodies,
//! the stems of cacti and other succulents, are meshed by [`body`], and
//! their spines grow per areole from a pattern per species ([`spines`]).
//! The textures of the ground between plants (litter, moss, grass, soil,
//! sand, gravel and rock) are drawn here too ([`ground`]).
//!
//! Everything here is engine-independent and deterministic: the same spec,
//! generator revision and seed give bit-identical output on every machine,
//! because every random draw is a pure function of lineage identities
//! ([`rng`]) and every transcendental function goes through `libm`
//! ([`math`]).

pub mod body;
pub mod graph;
pub mod ground;
pub mod grow;
pub mod impostor;
pub mod json;
pub mod litter;
pub mod looks;
pub mod lsys;
pub mod math;
pub mod mesh;
pub mod package;
pub mod preview;
pub mod quality;
pub mod raster;
pub mod rng;
pub mod spec;
pub mod spines;
pub mod templates;

/// Revision of the generator as a whole: the L-system engine, its tools and
/// the bakers. Bump it whenever output for an unchanged spec changes; it is
/// part of every package key. 3: cluster cards of the far levels gained
/// upright cards, so far crowns show from the side. 4: cluster cards cover
/// what their organs cover, no more, and keep the organs' proportions, so
/// far levels are not denser than near ones.
pub const GENERATOR_REVISION: u32 = 4;
