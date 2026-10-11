//! Part meshes (plant roadmap P4): one small solid per organ type, which a
//! renderer draws at each of the type's organs near the camera in place
//! of its card.
//!
//! Small parts repeat: a maple carries thousands of samara clusters, a fir
//! thousands of cones. Drawn into the plant's own mesh they would cost
//! millions of triangles (`crate::blooms::TYPE_TRIANGLES` keeps such a
//! type as cards), so instead each organ type whose look draws a solid (a
//! flower, a fruit or cluster, a cone, a thorn) gets [`VARIANTS`] meshes of
//! one organ, in the frame of the organ's card: a point $`\mathbf p`$ of
//! the organ is stored as
//!
//! ```math
//! (x, y, z) = \big((\mathbf p - \mathbf b)\cdot\mathbf l,\ (\mathbf p - \mathbf b)\cdot\mathbf h,\ (\mathbf p - \mathbf b)\cdot(\mathbf l \times \mathbf h)\big) / \ell
//! ```
//!
//! for the card's base $`\mathbf b`$, heading $`\mathbf h`$, left $`\mathbf l`$
//! and length $`\ell`$, so a renderer puts it back with
//! $`\mathbf p = \mathbf b + \ell\,(x\,\mathbf l + y\,\mathbf h + z\,\mathbf l \times \mathbf h)`$
//! from any card of the type, whichever way the look mounts its card
//! (`crate::mesh::place`; a look that turns its cards to face up keeps
//! them). Variant `v` is the organ drawn with the id `mix64(v + 1)`, so a
//! cluster's turn and which of its fruit are unripe differ from variant to
//! variant; a renderer picks one per organ. Its colours are the look's
//! own, and the renderer shades them as the card's colour is shaded.
//!
//! A part mesh holds at most [`TRIANGLES`] triangles ([`SHOOT_TRIANGLES`]
//! for a needle shoot, `crate::shoots`, and [`CLUSTER_TRIANGLES`] for a
//! cluster, `crate::clusters`, or a flower head, `crate::capitula`, a
//! cluster of florets): a cluster, a head or a shoot that would take more
//! is drawn up to [`coarsest`] steps coarser, and a type that still does
//! not fit keeps its cards.
//!
//! Each type has a span ([`PartMesh::span`]): the share of its card's
//! length whose projection must cover the renderer's least pixels for the
//! part mesh to be drawn. An organ seen whole, as a fruit, has span 1; a
//! needle shoot's is [`SHOOT_SPAN`] times its needles' width, so its
//! needles stay a pixel or more wide where the least is four.

use crate::blooms::{self, Form};
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::{Look, Shape};
use crate::math::Vec3;
use crate::mesh::{Mesh, PlantMesh};
use crate::rng::mix64;

/// Variants of each part mesh.
pub const VARIANTS: usize = 4;

/// Most triangles a part mesh holds.
pub const TRIANGLES: usize = 1024;

/// Most triangles a needle shoot's part mesh holds: a shoot has hundreds
/// of needles.
pub const SHOOT_TRIANGLES: usize = 8192;

/// Most triangles a cluster's or a flower head's part mesh holds: a
/// cluster has dozens to hundreds of units, a head as many florets.
pub const CLUSTER_TRIANGLES: usize = 8192;

/// A needle shoot's span over its needles' width (see the module).
pub const SHOOT_SPAN: f64 = 4.0;

/// One organ type's part meshes.
#[derive(Debug, Clone, PartialEq)]
pub struct PartMesh {
    /// The organ type (template) it stands for, in the program's order.
    pub template: usize,
    /// [`VARIANTS`] meshes, each one organ in its own frame.
    pub variants: Vec<Mesh>,
    /// The share of its card's length that must cover the renderer's
    /// least pixels for it to be drawn: 1, or less for a part whose detail
    /// is finer than the organ (see the module).
    pub span: f64,
}

impl PartMesh {
    /// The most triangles any variant holds.
    #[must_use]
    pub fn triangles(&self) -> usize {
        self.variants
            .iter()
            .map(Mesh::triangle_count)
            .max()
            .unwrap_or(0)
    }
}

/// The part meshes of `looks`: one for each organ type whose look draws a
/// solid that fits its [`cap`], in the types' order.
#[must_use]
pub fn part_meshes(looks: &[Look]) -> Vec<PartMesh> {
    part_meshes_within(looks, 1.0)
}

/// [`part_meshes`] with each type's cap ([`cap`]) times `share`, at most
/// 1: an export drawn to a budget (`crate::export`) draws its parts
/// coarser, and a type whose coarsest part mesh no longer fits keeps its
/// cards.
#[must_use]
pub fn part_meshes_within(looks: &[Look], share: f64) -> Vec<PartMesh> {
    (0..looks.len())
        .filter_map(|index| {
            let cap = cap(&looks[index]);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let cap = if share >= 1.0 {
                cap
            } else {
                (cap as f64 * share.max(0.0)) as usize
            };
            part_mesh(looks, index, cap)
        })
        .collect()
}

/// The most triangles a part mesh of `look` may hold.
#[must_use]
pub fn cap(look: &Look) -> usize {
    match look.form {
        Form::Shoot => SHOOT_TRIANGLES,
        _ if look.inflorescence.is_some()
            || matches!(look.shape, Shape::Head(_) | Shape::Spikelet(_)) =>
        {
            CLUSTER_TRIANGLES
        }
        _ => TRIANGLES,
    }
}

/// The most steps coarser a part mesh of `look` is drawn to fit its
/// [`cap`]: `crate::fruit::COARSER`, or fewer for a cluster whose coarsest
/// steps would draw it worse than its card (`crate::clusters::coarsest`).
#[must_use]
pub fn coarsest(look: &Look) -> usize {
    look.inflorescence
        .as_ref()
        .map_or(crate::fruit::COARSER, |cluster| {
            crate::clusters::coarsest(&cluster.cluster)
        })
}

/// The span of a part mesh of `look` (see the module).
#[must_use]
pub fn span(look: &Look) -> f64 {
    match (&look.form, &look.shape) {
        (Form::Shoot, Shape::Needles(needles)) => {
            (SHOOT_SPAN * crate::shoots::needle_width(needles)).min(1.0)
        }
        _ => 1.0,
    }
}

/// Whether organ type `index` of `looks` draws as a part mesh: its look
/// draws a solid on level 0 and its card's frame follows its organ's.
#[must_use]
pub fn drawn_as_part(looks: &[Look], index: usize) -> bool {
    looks
        .get(index)
        .is_some_and(|look| look.solid.is_none() && look.form.solid_at(0) && look.face_up <= 0.0)
}

/// Each organ type of `looks`, whether it has a part mesh in `parts`.
#[must_use]
pub fn types(looks: &[Look], parts: &[PartMesh]) -> Vec<bool> {
    let mut types = vec![false; looks.len()];
    for part in parts {
        if let Some(slot) = types.get_mut(part.template) {
            *slot = true;
        }
    }
    types
}

fn part_mesh(looks: &[Look], index: usize, cap: usize) -> Option<PartMesh> {
    if !drawn_as_part(looks, index) {
        return None;
    }
    let organ = u16::try_from(index).ok()?;
    let mut solid = vec![false; looks.len()];
    solid[index] = true;
    // The frame of a card of this type on the organ drawn.
    let unit = GraphOrgan {
        id: 0,
        organ,
        segment: None,
        position: Vec3::ZERO,
        heading: Vec3::Y,
        left: Vec3::X,
        size: 1.0,
        born: 0.0,
        shed: None,
        light: 1.0,
    };
    let (base, heading, left, length) = crate::mesh::place(&unit, &looks[index]);
    let out = left.cross(heading);
    for coarse in 0..=coarsest(&looks[index]) {
        let variants: Vec<Mesh> = (0..VARIANTS as u64)
            .map(|variant| {
                let graph = PlantGraph {
                    age: 1.0,
                    height: 1.0,
                    segments: Vec::new(),
                    organs: vec![GraphOrgan {
                        id: mix64(variant + 1),
                        organ,
                        segment: None,
                        position: Vec3::ZERO,
                        heading: Vec3::Y,
                        left: Vec3::X,
                        size: 1.0,
                        born: 0.0,
                        shed: None,
                        light: 1.0,
                    }],
                };
                let mut mesh = Mesh::default();
                blooms::build_coarse(
                    &graph,
                    looks,
                    &solid,
                    &|look, _| look.colour,
                    coarse,
                    &mut mesh,
                );
                into_frame(&mut mesh, (base, heading, left, out), length);
                mesh
            })
            .collect();
        if variants.iter().all(|mesh| mesh.triangle_count() > 0)
            && variants.iter().all(|mesh| mesh.triangle_count() <= cap)
        {
            return Some(PartMesh {
                template: index,
                variants,
                span: span(&looks[index]),
            });
        }
    }
    None
}

/// One site of a nearest level drawn as its type's part mesh: which part
/// and variant, the frame it is put back in (`p = base + length (x left +
/// y heading + z out)`, see the module) and the cards it replaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The organ's card in the level's cards, and the crossing card of a
    /// crossed pair; a renderer drawing the part leaves both out.
    pub card: usize,
    pub cross: Option<usize>,
    /// The part mesh, as an index into the `parts` placed, and its variant.
    pub part: usize,
    pub variant: usize,
    pub base: Vec3,
    /// Unit vectors: the card's heading, its left made square to it, and
    /// `left × heading`.
    pub heading: Vec3,
    pub left: Vec3,
    pub out: Vec3,
    pub length: f64,
    /// What the part's colours are multiplied by: as much darker as its
    /// card's colour is than its look's, 0 to 2.
    pub darkening: f32,
}

/// Where each site of `plant`, a nearest level built for part meshes
/// (`crate::mesh::build_with`), is drawn as one of its type's `parts`, in
/// the order of its sites; sites whose type has no part mesh are left out.
/// Site `n` draws the variant `mix64(n)` picks.
#[must_use]
pub fn placements(plant: &PlantMesh, looks: &[Look], parts: &[PartMesh]) -> Vec<Placement> {
    let to_vec = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let mut out = Vec::with_capacity(plant.sites.len());
    for (n, &site) in plant.sites.iter().enumerate() {
        let index = site as usize;
        let Some(&card) = plant.cards.get(index) else {
            continue;
        };
        let template = usize::from(card.template);
        let (Some(part), Some(look)) = (
            parts.iter().position(|part| part.template == template),
            looks.get(template),
        ) else {
            continue;
        };
        let variant = usize::try_from(mix64(n as u64) % VARIANTS as u64).unwrap_or(0);
        if variant >= parts[part].variants.len() {
            continue;
        }
        let heading = to_vec(card.heading).normalize_or(Vec3::Y);
        let along = to_vec(card.left);
        let left = (along - heading * along.dot(heading))
            .normalize_or(crate::math::any_perpendicular(heading));
        // The crossing card, pushed just before the organ's own with a copy
        // of its base and heading.
        #[allow(clippy::float_cmp)]
        let cross = index.checked_sub(1).filter(|&cross| {
            plant.cards.get(cross).is_some_and(|other| {
                other.template == card.template
                    && other.base == card.base
                    && other.heading == card.heading
            })
        });
        out.push(Placement {
            card: index,
            cross,
            part,
            variant,
            base: to_vec(card.base),
            heading,
            left,
            out: left.cross(heading),
            length: f64::from(card.length),
            darkening: (luminance(card.color) / luminance(look.colour).max(1.0e-6)).clamp(0.0, 2.0),
        });
    }
    out
}

/// Draw each site of `plant`, a nearest level built for part meshes
/// (`crate::mesh::build_with`), as one of its type's `parts`, into its wood
/// mesh, and leave out the site's cards (both of a crossed pair): what a
/// renderer that draws part meshes shows near the camera, for previews.
/// Each site draws the variant its index picks, in the part mesh's
/// colours darkened with its card as the renderer darkens them.
pub fn place(plant: &mut PlantMesh, looks: &[Look], parts: &[PartMesh]) {
    let to_vec = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut dropped = vec![false; plant.cards.len()];
    for placed in placements(plant, looks, parts) {
        let mesh = &parts[placed.part].variants[placed.variant];
        let card = plant.cards[placed.card];
        let (base, left, heading, out) = (placed.base, placed.left, placed.heading, placed.out);
        let darkening = placed.darkening;
        let first = u32::try_from(plant.wood.positions.len()).unwrap_or(u32::MAX);
        for ((position, normal), colour) in
            mesh.positions.iter().zip(&mesh.normals).zip(&mesh.colors)
        {
            let p = to_vec(*position);
            let n = to_vec(*normal);
            plant
                .wood
                .positions
                .push((base + (left * p.x + heading * p.y + out * p.z) * placed.length).to_f32());
            plant
                .wood
                .normals
                .push((left * n.x + heading * n.y + out * n.z).to_f32());
            plant.wood.uvs.push([0.0, 0.0]);
            plant.wood.colors.push([
                colour[0] * darkening,
                colour[1] * darkening,
                colour[2] * darkening,
                1.0,
            ]);
            plant.wood.births.push(card.born);
            plant.wood.sheds.push(card.shed);
            plant.wood.levels.push(3);
        }
        plant
            .wood
            .indices
            .extend(mesh.indices.iter().map(|&i| first + i));
        dropped[placed.card] = true;
        if let Some(cross) = placed.cross {
            dropped[cross] = true;
        }
    }
    let mut kept = dropped.iter();
    plant
        .cards
        .retain(|_| !kept.next().copied().unwrap_or(false));
    plant.sites.clear();
}

/// Express `mesh`, drawn in the organ's frame, in the frame of its card:
/// base `base`, heading `heading`, left `left` and `out` = left × heading,
/// `length` long (1, or a cluster's card length).
#[allow(clippy::cast_possible_truncation)] // meshes hold f32
fn into_frame(mesh: &mut Mesh, (base, heading, left, out): (Vec3, Vec3, Vec3, Vec3), length: f64) {
    let to_card = |p: Vec3| [p.dot(left), p.dot(heading), p.dot(out)];
    for position in &mut mesh.positions {
        let p = Vec3::new(
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        ) - base;
        *position = to_card(p).map(|value| (value / length) as f32);
    }
    for normal in &mut mesh.normals {
        let n = Vec3::new(
            f64::from(normal[0]),
            f64::from(normal[1]),
            f64::from(normal[2]),
        );
        *normal = to_card(n).map(|value| value as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clusters::{Cluster, ClusterLook};
    use crate::looks::{ClusterKind, Flower, Fruit, FruitKind, Inflorescence, Needles, Simple};

    fn look(shape: Shape) -> Look {
        Look {
            organ: "bloom".to_string(),
            form: Form::default_for(&shape),
            bend: crate::bend::Bend::FLAT,
            shape,
            colour: [0.7, 0.1, 0.1],
            shade: [0.2, 0.05, 0.05],
            accent: [0.3, 0.5, 0.1],
            face_up: 0.0,
            solid: None,
            forms: crate::looks::Forms::default(),
            inflorescence: None,
        }
    }

    /// `shape`'s units in a cluster of `flowers` of `kind`.
    fn clustered(shape: Shape, kind: ClusterKind, flowers: u32) -> Look {
        let mut look = look(shape);
        let spec = Inflorescence {
            kind: Some(kind),
            flowers: Some(flowers),
            ..Inflorescence::default()
        };
        look.inflorescence = ClusterLook::new(Cluster::of(&spec), &look.shape, &[]);
        assert!(look.inflorescence.is_some());
        look
    }

    #[test]
    fn flowers_fruit_and_clusters_get_part_meshes_and_leaves_do_not() {
        let looks = vec![
            look(Shape::Simple(Simple::default())),
            look(Shape::Flower(Flower::default())),
            clustered(
                Shape::Fruit(Fruit {
                    kind: FruitKind::Drupelets,
                    unripe: 0.5,
                    ..Fruit::default()
                }),
                ClusterKind::Raceme,
                8,
            ),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Umbel, 12),
        ];
        let parts = part_meshes(&looks);
        assert_eq!(
            parts.iter().map(|p| p.template).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        for part in &parts {
            assert_eq!(part.variants.len(), VARIANTS);
            assert!(
                part.triangles() <= cap(&looks[part.template]),
                "{}",
                part.triangles()
            );
            for mesh in &part.variants {
                for p in &mesh.positions {
                    let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                    assert!(length <= 1.3, "within the organ's length: {p:?}");
                }
            }
        }
        // Variants differ: the cluster turns and its unripe fruit change.
        assert_ne!(
            parts[1].variants[0].positions,
            parts[1].variants[1].positions
        );
    }

    #[test]
    fn a_part_put_back_by_its_card_lands_where_the_organ_is() {
        // A flower's card is centred on it and faces along its heading
        // (Mount::Facing); a fruit's stands on it (Mount::Along); a
        // cluster's card is its card frame's. Placed back by a card of a
        // turned and moved organ, the part mesh is the organ drawn there.
        for shown in [
            look(Shape::Flower(Flower::default())),
            look(Shape::Fruit(Fruit::default())),
            look(Shape::Needles(Needles::default())),
            look(Shape::Needles(Needles {
                ranks: 0,
                ..Needles::default()
            })),
            clustered(Shape::Fruit(Fruit::default()), ClusterKind::Raceme, 3),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Umbel, 4),
            clustered(Shape::Flower(Flower::default()), ClusterKind::Cyme, 3),
        ] {
            // Drawn whole: a part mesh at its finest.
            assert!(
                shown.inflorescence.is_none() || crate::clusters::triangles(&shown) <= cap(&shown),
                "{}",
                crate::clusters::triangles(&shown)
            );
            let looks = vec![shown];
            let parts = part_meshes(&looks);
            let organ = GraphOrgan {
                id: crate::rng::mix64(1),
                organ: 0,
                segment: None,
                position: Vec3::new(1.0, 2.0, -3.0),
                heading: Vec3::new(0.0, 0.6, 0.8),
                left: Vec3::X,
                // Leaf-like parts take detail from their size in metres:
                // the same size as the part mesh, so the two match.
                size: 1.0,
                born: 0.0,
                shed: None,
                light: 1.0,
            };
            let (base, heading, left, length) = crate::mesh::place(&organ, &looks[0]);
            let out = left.cross(heading);
            let graph = PlantGraph {
                age: 1.0,
                height: 1.0,
                segments: Vec::new(),
                organs: vec![organ],
            };
            let mut drawn = Mesh::default();
            blooms::build(&graph, &looks, &[true], &|look, _| look.colour, &mut drawn);
            let back: Vec<Vec3> = parts[0].variants[0]
                .positions
                .iter()
                .map(|p| {
                    base + (left * f64::from(p[0])
                        + heading * f64::from(p[1])
                        + out * f64::from(p[2]))
                        * length
                })
                .collect();
            assert_eq!(back.len(), drawn.positions.len());
            for (b, d) in back.iter().zip(&drawn.positions) {
                let d = Vec3::new(f64::from(d[0]), f64::from(d[1]), f64::from(d[2]));
                assert!((*b - d).length() < 1e-5, "{b:?} {d:?}");
            }
        }
    }

    #[test]
    fn a_large_cluster_is_drawn_coarser_to_fit() {
        // A cluster takes more than a single fruit may.
        let fruit = Fruit {
            kind: FruitKind::Drupelets,
            ..Fruit::default()
        };
        let small = clustered(Shape::Fruit(fruit.clone()), ClusterKind::Raceme, 8);
        assert!(crate::clusters::triangles(&small) > TRIANGLES);
        let parts = part_meshes(&[small]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() > TRIANGLES);
        // One that takes more than a cluster may is drawn coarser, each of
        // its flowers still drawn.
        let flower = Flower {
            petals: 10,
            ..Flower::default()
        };
        let large = clustered(Shape::Flower(flower), ClusterKind::Raceme, 200);
        assert!(
            crate::clusters::triangles(&large) > CLUSTER_TRIANGLES,
            "this cluster needs coarsening: {}",
            crate::clusters::triangles(&large)
        );
        let parts = part_meshes(&[large]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() <= CLUSTER_TRIANGLES);
        assert!(parts[0].triangles() >= 200 * 8, "{}", parts[0].triangles());
    }

    #[test]
    fn only_a_compound_cluster_is_merged_to_fit() {
        // Too many drupelets to draw one by one: as a raceme the cluster
        // keeps its cards rather than become one solid; as a panicle each
        // of its racemes is merged into one.
        let fruit = Shape::Fruit(Fruit {
            kind: FruitKind::Drupelets,
            ..Fruit::default()
        });
        let raceme = clustered(fruit.clone(), ClusterKind::Raceme, 200);
        assert!(part_meshes(&[raceme]).is_empty());
        let mut panicle = look(fruit);
        let spec = Inflorescence {
            kind: Some(ClusterKind::Panicle),
            branches: Some(8),
            flowers: Some(25),
            ..Inflorescence::default()
        };
        panicle.inflorescence = ClusterLook::new(Cluster::of(&spec), &panicle.shape, &[]);
        let parts = part_meshes(&[panicle]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() <= CLUSTER_TRIANGLES);
    }

    #[test]
    fn needle_shoots_are_part_meshes_spanning_their_needles_width() {
        let needles = Needles::default();
        let looks = vec![
            look(Shape::Needles(needles.clone())),
            look(Shape::Fruit(Fruit::default())),
        ];
        let parts = part_meshes(&looks);
        assert_eq!(
            parts.iter().map(|p| p.template).collect::<Vec<_>>(),
            vec![0, 1]
        );
        let width = needles.width * needles.needle;
        assert!((parts[0].span - SHOOT_SPAN * width).abs() < 1e-12);
        assert!((parts[1].span - 1.0).abs() < 1e-12);
        assert!(parts[0].triangles() <= SHOOT_TRIANGLES);
        // A dense spray of short needles takes more than a fruit may.
        let dense = Needles {
            needle: 0.035,
            twigs: 5,
            aspect: 0.7,
            ..Needles::default()
        };
        let parts_dense = part_meshes(&[look(Shape::Needles(dense))]);
        assert_eq!(parts_dense.len(), 1);
        assert!(parts_dense[0].triangles() > TRIANGLES);
        assert!(parts_dense[0].triangles() <= SHOOT_TRIANGLES);
        // The variants differ out of the card's plane alone.
        assert_ne!(
            parts[0].variants[0].positions,
            parts[0].variants[1].positions
        );
        let flat = |mesh: &Mesh| -> Vec<[f32; 2]> {
            mesh.positions.iter().map(|p| [p[0], p[1]]).collect()
        };
        assert_eq!(
            flat(&parts[0].variants[0]).len(),
            flat(&parts[0].variants[1]).len()
        );
    }

    #[test]
    fn placed_at_their_sites_parts_replace_both_cards() {
        // Three fruit, each a crossed pair of cards, the second its site.
        let looks = vec![look(Shape::Fruit(Fruit::default()))];
        let parts = part_meshes(&looks);
        let mut plant = PlantMesh::default();
        for k in 0..3_u8 {
            let card = crate::mesh::Card {
                base: [f32::from(k), 1.0, 0.0],
                heading: [0.0, 1.0, 0.0],
                left: [1.0, 0.0, 0.0],
                length: 0.1,
                width: 0.05,
                color: looks[0].colour,
                template: 0,
                born: 0.0,
                shed: f32::INFINITY,
                bend: crate::bend::Bend::FLAT,
            };
            plant.cards.push(crate::mesh::Card {
                left: [0.0, 0.0, 1.0],
                ..card
            });
            plant.cards.push(card);
            plant.sites.push(u32::from(k) * 2 + 1);
        }
        place(&mut plant, &looks, &parts);
        assert!(plant.cards.is_empty() && plant.sites.is_empty());
        let per_site: Vec<usize> = (0..3)
            .map(|n: u64| {
                parts[0].variants[usize::try_from(mix64(n) % VARIANTS as u64).unwrap()]
                    .triangle_count()
            })
            .collect();
        assert_eq!(plant.wood.triangle_count(), per_site.iter().sum::<usize>());
        // Each lies within its organ's length of its card's base.
        for p in &plant.wood.positions {
            let nearest = (0..3_u8).map(|k| {
                let dx = p[0] - f32::from(k);
                (dx * dx + (p[1] - 1.0).powi(2) + p[2] * p[2]).sqrt()
            });
            assert!(nearest.fold(f32::INFINITY, f32::min) <= 0.13, "{p:?}");
        }
    }

    #[test]
    fn part_meshes_are_the_same_every_time() {
        let looks = vec![clustered(
            Shape::Fruit(Fruit {
                unripe: 0.3,
                ..Fruit::default()
            }),
            ClusterKind::Umbel,
            12,
        )];
        assert_eq!(part_meshes(&looks), part_meshes(&looks));
    }
}
