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
//! A part mesh holds at most [`TRIANGLES`] triangles: a cluster that would
//! take more is drawn up to `crate::fruit::COARSER` steps coarser, and a
//! type that still does not fit keeps its cards.

use crate::blooms;
use crate::graph::{GraphOrgan, PlantGraph};
use crate::looks::Look;
use crate::math::Vec3;
use crate::mesh::Mesh;
use crate::rng::mix64;

/// Variants of each part mesh.
pub const VARIANTS: usize = 4;

/// Most triangles a part mesh holds.
pub const TRIANGLES: usize = 1024;

/// One organ type's part meshes.
#[derive(Debug, Clone, PartialEq)]
pub struct PartMesh {
    /// The organ type (template) it stands for, in the program's order.
    pub template: usize,
    /// [`VARIANTS`] meshes, each one organ in its own frame.
    pub variants: Vec<Mesh>,
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
/// solid that fits [`TRIANGLES`], in the types' order.
#[must_use]
pub fn part_meshes(looks: &[Look]) -> Vec<PartMesh> {
    (0..looks.len())
        .filter_map(|index| part_mesh(looks, index))
        .collect()
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

fn part_mesh(looks: &[Look], index: usize) -> Option<PartMesh> {
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
    let (base, heading, left) = crate::mesh::place(&unit, &looks[index]);
    let out = left.cross(heading);
    for coarse in 0..=crate::fruit::COARSER {
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
                into_frame(&mut mesh, base, heading, left, out);
                mesh
            })
            .collect();
        if variants.iter().all(|mesh| mesh.triangle_count() > 0)
            && variants
                .iter()
                .all(|mesh| mesh.triangle_count() <= TRIANGLES)
        {
            return Some(PartMesh {
                template: index,
                variants,
            });
        }
    }
    None
}

/// Express `mesh`, drawn in the organ's frame, in the frame of its card:
/// base `base`, heading `heading`, left `left` and `out` = left × heading,
/// one unit long.
#[allow(clippy::cast_possible_truncation)] // meshes hold f32
fn into_frame(mesh: &mut Mesh, base: Vec3, heading: Vec3, left: Vec3, out: Vec3) {
    let to_card = |p: Vec3| [p.dot(left), p.dot(heading), p.dot(out)];
    for position in &mut mesh.positions {
        let p = Vec3::new(
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        ) - base;
        *position = to_card(p).map(|value| value as f32);
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
    use crate::blooms::Form;
    use crate::looks::{Cluster, Flower, Fruit, FruitKind, Shape, Simple};

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
        }
    }

    #[test]
    fn flowers_fruit_and_clusters_get_part_meshes_and_leaves_do_not() {
        let looks = vec![
            look(Shape::Simple(Simple::default())),
            look(Shape::Flower(Flower::default())),
            look(Shape::Fruit(Fruit {
                kind: FruitKind::Drupelets,
                count: 8,
                cluster: Cluster::Raceme,
                unripe: 0.5,
                ..Fruit::default()
            })),
        ];
        let parts = part_meshes(&looks);
        assert_eq!(
            parts.iter().map(|p| p.template).collect::<Vec<_>>(),
            vec![1, 2]
        );
        for part in &parts {
            assert_eq!(part.variants.len(), VARIANTS);
            assert!(part.triangles() <= TRIANGLES, "{}", part.triangles());
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
        // (Mount::Facing); a fruit's stands on it (Mount::Along). Placed
        // back by a card of a turned and moved organ, the part
        // mesh is the organ drawn there.
        for shape in [
            Shape::Flower(Flower::default()),
            Shape::Fruit(Fruit::default()),
        ] {
            let looks = vec![look(shape)];
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
            let (base, heading, left) = crate::mesh::place(&organ, &looks[0]);
            let out = left.cross(heading);
            let size = organ.size;
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
                        * size
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
        let fruit = Fruit {
            kind: FruitKind::Drupelets,
            count: 8,
            ..Fruit::default()
        };
        assert!(
            crate::fruit::triangles(&blooms::FruitForm::default(), &fruit) > TRIANGLES,
            "this cluster needs coarsening"
        );
        let parts = part_meshes(&[look(Shape::Fruit(fruit))]);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].triangles() <= TRIANGLES);
    }

    #[test]
    fn part_meshes_are_the_same_every_time() {
        let looks = vec![look(Shape::Fruit(Fruit {
            count: 12,
            cluster: Cluster::Umbel,
            unripe: 0.3,
            ..Fruit::default()
        }))];
        assert_eq!(part_meshes(&looks), part_meshes(&looks));
    }
}
