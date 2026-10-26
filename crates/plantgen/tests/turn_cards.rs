//! The vectors an engine checks its card-turning vertex shader against
//! (`shaders/turn_cards.wgsl`): cards of every kind of turn, seen from
//! level, from 45 degrees, from straight above and close up, turned as
//! `raster::turned` turns them in PlantGen's renders.
//!
//! When the turning changes, `PLANTGEN_WRITE_VECTORS=1 cargo test --test
//! turn_cards` writes the file again.

use std::fmt::Write;
use std::path::Path;

use plantgen::math::Vec3;
use plantgen::mesh::{Mesh, Vertex};
use plantgen::raster::{self, Camera, Projection, Texture, Turn};
use plantgen::templates::Templates;

const VECTORS: &str = "shaders/turn_cards.json";

fn picture(turn: Turn, pivot: [f32; 3], extent: [f32; 3], seen_from: [f32; 3]) -> Texture {
    Texture {
        width: 1,
        height: 1,
        color: vec![[1.0; 4]],
        normal: vec![[0.0, 0.0, 1.0]],
        sun: Vec::new(),
        turn,
        pivot,
        extent,
        seen_from,
    }
}

/// The pictures: one fixed, one facing, three heights (level, 45 degrees,
/// from above, as `billboards::HEIGHTS` bakes them), four views round.
fn pictures() -> Vec<Texture> {
    let half = 0.5_f32.sqrt();
    let heights = Turn::Heights { first: 2, count: 3 };
    let views = Turn::Views { first: 5, count: 4 };
    vec![
        picture(Turn::Fixed, [0.0, 1.0, 0.0], [0.3; 3], [0.0, 0.0, 1.0]),
        picture(
            Turn::Facing,
            [1.0, 1.0, 0.0],
            [0.3, 0.3, 0.1],
            [0.0, 0.0, 1.0],
        ),
        picture(heights, [0.0, 2.0, 1.0], [0.4, 0.5, 0.2], [0.0, 0.0, 1.0]),
        picture(
            heights,
            [0.0, 2.0, 1.0],
            [0.4, 0.45, 0.3],
            [0.0, half, half],
        ),
        picture(heights, [0.0, 2.0, 1.0], [0.4, 0.35, 0.5], [0.0, 1.0, 0.0]),
        picture(views, [0.0, 1.5, -1.0], [0.6; 3], [1.0, 0.0, 0.0]),
        picture(views, [0.0, 1.5, -1.0], [0.6; 3], [0.0, 0.0, 1.0]),
        picture(views, [0.0, 1.5, -1.0], [0.6; 3], [-1.0, 0.0, 0.0]),
        picture(views, [0.0, 1.5, -1.0], [0.6; 3], [0.0, 0.0, -1.0]),
    ]
}

/// A card about `centre`, `right` and `up` its half sides, facing `normal`,
/// showing `picture`: the four corners of a quad.
fn card(mesh: &mut Mesh, centre: Vec3, right: Vec3, up: Vec3, picture: usize) {
    let normal = right.cross(up).normalize_or(Vec3::Z);
    #[allow(clippy::cast_precision_loss)]
    let index = picture as f32;
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        mesh.push(Vertex {
            position: centre + right * x + up * y,
            normal,
            uv: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, index],
            born: 0.0,
            shed: None,
            level: 3,
            segment: plantgen::mesh::NO_SEGMENT,
        });
    }
}

fn cards() -> Mesh {
    let mut mesh = Mesh::default();
    let (x, y, z) = (Vec3::X, Vec3::Y, Vec3::Z);
    card(&mut mesh, Vec3::new(0.0, 1.0, 0.0), x * 0.3, y * 0.3, 0);
    // Facing, a little off its pivot and leaning.
    card(
        &mut mesh,
        Vec3::new(1.1, 1.05, 0.0),
        (x + z * 0.3) * 0.3,
        y * 0.3,
        1,
    );
    // A crown's card, laid facing level as `billboards` lays it.
    card(&mut mesh, Vec3::new(0.0, 2.0, 1.0), x * 0.5, y * 0.6, 2);
    // An impostor's card.
    card(&mut mesh, Vec3::new(0.0, 1.5, -1.0), x * 0.7, y * 0.7, 5);
    mesh
}

fn cameras() -> Vec<(&'static str, Camera)> {
    let looking = |eye: Vec3, target: Vec3, perspective: bool| Camera {
        eye,
        target,
        up: Vec3::Y,
        projection: if perspective {
            Projection::Perspective { fov_y: 50.0 }
        } else {
            Projection::Orthographic { half_height: 3.0 }
        },
    };
    let middle = Vec3::new(0.0, 1.5, 0.0);
    vec![
        (
            "level from +x",
            looking(Vec3::new(10.0, 1.5, 0.0), middle, false),
        ),
        (
            "45 degrees from -z",
            looking(Vec3::new(0.0, 11.5, -10.0), middle, false),
        ),
        (
            "straight above",
            looking(Vec3::new(0.001, 20.0, 0.0), middle, false),
        ),
        (
            "close, perspective",
            looking(Vec3::new(2.5, 3.0, 3.5), middle, true),
        ),
        (
            "far below the horizon, perspective",
            looking(Vec3::new(-6.0, -2.0, 4.0), middle, true),
        ),
    ]
}

fn numbers(values: &[f32]) -> String {
    let parts: Vec<String> = values.iter().map(|v| format!("{v:.6}")).collect();
    format!("[{}]", parts.join(", "))
}

/// The vectors as JSON: the pictures, the vertices, and for each camera
/// each vertex turned.
fn vectors() -> String {
    let pictures = pictures();
    let templates = Templates {
        baked: pictures.clone(),
        ..Templates::default()
    };
    let mesh = cards();
    let mut out = String::from("{\n  \"pictures\": [\n");
    for (i, p) in pictures.iter().enumerate() {
        let (kind, first, count) = match p.turn {
            Turn::Fixed => (0, 0, 0),
            Turn::Facing => (1, 0, 0),
            Turn::Views { first, count } => (2, first, count),
            Turn::Heights { first, count } => (3, first, count),
        };
        let _ = write!(
            out,
            "    {{\"pivot\": {}, \"kind\": {kind}, \"extent\": {}, \"first\": {first}, \"seen_from\": {}, \"count\": {count}}}{}\n",
            numbers(&p.pivot),
            numbers(&p.extent),
            numbers(&p.seen_from),
            if i + 1 < pictures.len() { "," } else { "" }
        );
    }
    out.push_str("  ],\n  \"vertices\": [\n");
    for i in 0..mesh.positions.len() {
        let _ = write!(
            out,
            "    {{\"position\": {}, \"normal\": {}, \"picture\": {}}}{}\n",
            numbers(&mesh.positions[i]),
            numbers(&mesh.normals[i]),
            mesh.colors[i][3],
            if i + 1 < mesh.positions.len() {
                ","
            } else {
                ""
            }
        );
    }
    out.push_str("  ],\n  \"cameras\": [\n");
    let cameras = cameras();
    for (k, (name, camera)) in cameras.iter().enumerate() {
        let forward = (camera.target - camera.eye).normalize_or(-Vec3::Z);
        let perspective = matches!(camera.projection, Projection::Perspective { .. });
        let turned = raster::turned(&mesh, &templates, camera);
        let _ = write!(
            out,
            "    {{\"name\": \"{name}\", \"eye\": {}, \"forward\": {}, \"perspective\": {perspective}, \"turned\": [\n",
            numbers(&camera.eye.to_f32()),
            numbers(&forward.to_f32()),
        );
        for i in 0..turned.positions.len() {
            let _ = write!(
                out,
                "      {{\"position\": {}, \"normal\": {}, \"picture\": {}}}{}\n",
                numbers(&turned.positions[i]),
                numbers(&turned.normals[i]),
                turned.colors[i][3],
                if i + 1 < turned.positions.len() {
                    ","
                } else {
                    ""
                }
            );
        }
        let _ = write!(
            out,
            "    ]}}{}\n",
            if k + 1 < cameras.len() { "," } else { "" }
        );
    }
    out.push_str("  ]\n}\n");
    out
}

#[test]
fn the_shaders_vectors_are_the_turning_of_plantgens_renders() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(VECTORS);
    let now = vectors();
    if std::env::var_os("PLANTGEN_WRITE_VECTORS").is_some() {
        std::fs::write(&path, &now).expect("writes the vectors");
    }
    let held = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        held == now,
        "{VECTORS} is not what raster::turned does now; if the change is \
         meant, run PLANTGEN_WRITE_VECTORS=1 cargo test --test turn_cards and \
         change shaders/turn_cards.wgsl to match"
    );
}

#[test]
fn every_kind_of_turn_moves_its_cards() {
    let templates = Templates {
        baked: pictures(),
        ..Templates::default()
    };
    let mesh = cards();
    for (name, camera) in cameras() {
        let turned = raster::turned(&mesh, &templates, &camera);
        // The fixed card stays; every other card faces the camera.
        assert_eq!(turned.positions[..4], mesh.positions[..4], "{name}");
        for i in 4..mesh.positions.len() {
            let n = Vec3::new(
                f64::from(turned.normals[i][0]),
                f64::from(turned.normals[i][1]),
                f64::from(turned.normals[i][2]),
            );
            assert!((n.length() - 1.0).abs() < 1e-5, "{name}: {n:?}");
        }
    }
}
