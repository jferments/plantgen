//! PlantGen: procedural plants, grown from species data.
//!
//! A species is data (a [`spec::PlantSpec`]), with a note on how each of
//! its values is known ([`evidence`]); its form comes from a plant
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
//! sand, gravel and rock) are drawn here too ([`ground`]). Beside its spec,
//! a species' record says where it grows ([`niche`]), the site it typically
//! grows on ([`conditions`]) and what falls from it ([`shed`]). The
//! built-in species are compiled in from the library tree ([`library`]);
//! Project After uses all of this through its `after-plants` crate.
//!
//! Everything here is engine-independent and deterministic: the same spec,
//! generator revision and seed give bit-identical output on every machine,
//! because every random draw is a pure function of lineage identities
//! ([`rng`]) and every transcendental function goes through `libm`
//! ([`math`]).

pub mod bark;
pub mod bend;
pub mod blooms;
pub mod body;
pub mod budget;
pub mod capitula;
pub mod clusters;
pub mod conditions;
pub mod cores;
pub mod drawing;
pub mod evidence;
pub mod export;
pub mod form;
pub mod formula;
pub mod fruit;
pub mod gltf;
pub mod graph;
pub mod ground;
pub mod grow;
pub mod impostor;
pub mod inherit;
pub mod json;
pub mod leaves;
pub mod library;
pub mod litter;
pub mod looks;
pub mod lsys;
pub mod math;
pub mod mesh;
pub mod meshopt;
pub mod niche;
pub mod occlusion;
pub mod package;
pub mod parts;
pub mod preview;
pub mod quality;
pub mod raster;
pub mod rng;
pub mod roots;
pub mod shed;
pub mod shoots;
pub mod spec;
pub mod spikelets;
pub mod spines;
pub mod substrate;
pub mod templates;
pub mod texture;
pub mod traits;
pub mod venation;

/// Revision of the generator as a whole: the L-system engine, its tools and
/// the bakers. Bump it whenever output for an unchanged spec changes; it is
/// part of every package key. 3: cluster cards of the far levels gained
/// upright cards, so far crowns show from the side. 4: cluster cards cover
/// what their organs cover, no more, and keep the organs' proportions, so
/// far levels are not denser than near ones. 5: leaf, blade and frond
/// cards bend where they are drawn one per organ (plant forms F2), which
/// changes the impostors and the atlas records. 6: flowers, fruit and cones
/// drawn as solids on the nearest level (F3). 7: every broadleaf package
/// gains the thorn organ type (F5). 8: organ types with part meshes
/// (`crate::parts`, plant roadmap P4) keep their cards on level 0, listed
/// as sites, instead of being drawn solid into its wood mesh, and the
/// package stores their part meshes. 9: needle sprays are solid shoots on
/// the nearest level (`crate::shoots`), and each part mesh stores its span
/// (`APPARTS2`). 10: bark vertices carry the stem's radius in their
/// colour's alpha, for the species' bark pattern (`crate::bark`). 11: leaf
/// templates draw their veins grown by space colonization
/// (`crate::venation`). 12: every level keeps the nearest one's widths
/// (plant leftovers L10 and L12): cluster cards count overlapping organs
/// once, follow their bent organs, never reach past them and share their
/// area in rows; rings widen to the stem's mean width; the thin wood a
/// level drops comes back as sticks; and impostor alpha keeps the plant's
/// coverage at the cut-out. 13: organs take their colour, and leaves their
/// sun or shade form, along the logarithm of their light between
/// `looks::DEEP_SHADE` and `looks::FULL_SUN`, so a dense crown's surface is
/// drawn sunlit; and bark is darkened by the sky it sees through the
/// plant's leaves (`crate::occlusion`). 14: a rosette's blades become a
/// star of its own blades at coarse levels, fewer and wider, with thick
/// leaves' edges (render review S1), instead of a cross of cluster cards.
/// 15: a cluster's cover is counted from 16 directions on a 24-cell grid,
/// not 32 on 48, so coarse levels bake several times faster. 16: a
/// cluster's part mesh may take `parts::CLUSTER_TRIANGLES`, as a needle
/// shoot's does, drawing each flower or head as one flat hexagon a step
/// coarser, and a cluster whose axis bears no branches is never merged
/// into one solid to fit (spec rework L1c). 17: a flower head is laid out
/// from its florets (`crate::capitula`, spec rework P2): its phyllaries in
/// rows, its receptacle, its disc florets opening from the rim inward,
/// rays or straps, and its fruit with their pappus; its solid, the head a
/// cluster draws and its card all come from that layout, and a head's
/// part mesh may take `parts::CLUSTER_TRIANGLES`. 18: a grass's or a
/// sedge's spikelet is laid out from its glumes and florets
/// (`crate::spikelets`, spec rework P3), and a cluster's units may be
/// spikelets; a tapered branch bears fewer units, each as long as on a
/// branch of full length, instead of as many smaller ones; and a
/// cluster's card holds units that reach past its organ's tip, which it
/// cut off.
pub const GENERATOR_REVISION: u32 = 18;
