//! The vectors an engine checks its card-turning vertex shader against
//! (`shaders/turn_cards.wgsl`): cards of every kind of turn, seen from
//! level, from 45 degrees, from straight above and close up, turned as
//! `raster::turned` turns them in this crate's renders.
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

/// The pictures: one fixed, one facing, four heights (level, 45 degrees,
/// from above, 45 degrees from behind, as `billboards::HEIGHTS` bakes
/// them), four views round.
fn pictures() -> Vec<Texture> {
    let half = 0.5_f32.sqrt();
    let heights = Turn::Heights { first: 2, count: 4 };
    let views = Turn::Views { first: 6, count: 4 };
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
        picture(
            heights,
            [0.0, 2.0, 1.0],
            [0.4, 0.4, 0.35],
            [0.0, half, -half],
        ),
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
    card(&mut mesh, Vec3::new(0.0, 1.5, -1.0), x * 0.7, y * 0.7, 6);
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
            "45 degrees from +z",
            looking(Vec3::new(0.0, 11.5, 10.0), middle, false),
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
        let _ = writeln!(
            out,
            "    {{\"pivot\": {}, \"kind\": {kind}, \"extent\": {}, \"first\": {first}, \"seen_from\": {}, \"count\": {count}}}{}",
            numbers(&p.pivot),
            numbers(&p.extent),
            numbers(&p.seen_from),
            if i + 1 < pictures.len() { "," } else { "" }
        );
    }
    out.push_str("  ],\n  \"vertices\": [\n");
    for i in 0..mesh.positions.len() {
        let _ = writeln!(
            out,
            "    {{\"position\": {}, \"normal\": {}, \"picture\": {}}}{}",
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
        let _ = writeln!(
            out,
            "    {{\"name\": \"{name}\", \"eye\": {}, \"forward\": {}, \"perspective\": {perspective}, \"turned\": [",
            numbers(&camera.eye.to_f32()),
            numbers(&forward.to_f32()),
        );
        for i in 0..turned.positions.len() {
            let _ = writeln!(
                out,
                "      {{\"position\": {}, \"normal\": {}, \"picture\": {}}}{}",
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
        let _ = writeln!(
            out,
            "    ]}}{}",
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

/// The shader itself, run over the vectors on whatever GPU wgpu finds (a
/// software one, such as llvmpipe, does): `cargo test --features gpu
/// --test turn_cards`. Without a GPU it says so and passes.
#[cfg(feature = "gpu")]
#[test]
#[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
fn the_shader_turns_cards_as_the_vectors_say() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            std::thread::yield_now();
        }
    }

    let templates = Templates {
        baked: pictures(),
        ..Templates::default()
    };
    let mesh = cards();
    let mut words: Vec<u32> = Vec::new();
    for p in &templates.baked {
        let (kind, first, count) = match p.turn {
            Turn::Fixed => (0, 0, 0),
            Turn::Facing => (1, 0, 0),
            Turn::Views { first, count } => (2, first, count),
            Turn::Heights { first, count } => (3, first, count),
        };
        for (v, n) in [(p.pivot, kind), (p.extent, first), (p.seen_from, count)] {
            words.extend(v.map(f32::to_bits));
            words.push(u32::try_from(n).unwrap());
        }
    }
    let pictures = words;
    let mut jobs: Vec<u32> = Vec::new();
    let mut expected = Vec::new();
    for (name, camera) in cameras() {
        let forward = (camera.target - camera.eye).normalize_or(-Vec3::Z);
        let perspective = matches!(camera.projection, Projection::Perspective { .. });
        let turned = raster::turned(&mesh, &templates, &camera);
        for i in 0..mesh.positions.len() {
            jobs.extend(mesh.positions[i].map(f32::to_bits));
            jobs.push(mesh.colors[i][3] as u32);
            jobs.extend(mesh.normals[i].map(f32::to_bits));
            jobs.push(0);
            jobs.extend(camera.eye.to_f32().map(f32::to_bits));
            jobs.push(u32::from(perspective));
            jobs.extend(forward.to_f32().map(f32::to_bits));
            jobs.push(0);
            expected.push((
                name,
                turned.positions[i],
                turned.normals[i],
                turned.colors[i][3] as u32,
            ));
        }
    }
    let shader = format!(
        "{}\n{}",
        plantgen::billboards::TURN_CARDS_WGSL,
        r"
struct Job { position: vec3<f32>, picture: u32, normal: vec3<f32>, pad0: u32,
             eye: vec3<f32>, perspective: u32, forward: vec3<f32>, pad1: u32 };
struct Out { position: vec3<f32>, picture: u32, normal: vec3<f32>, pad: u32 };
@group(0) @binding(0) var<storage, read> pictures: array<CardPicture>;
@group(0) @binding(1) var<storage, read> jobs: array<Job>;
@group(0) @binding(2) var<storage, read_write> outs: array<Out>;
fn card_picture(index: u32) -> CardPicture { return pictures[index]; }
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let j = jobs[id.x];
    let t = turn_card(j.position, j.normal, j.picture, j.eye, j.forward, j.perspective != 0u);
    outs[id.x] = Out(t.position, t.picture, t.normal, 0u);
}
"
    );
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("no GPU: the shader is not run");
        return;
    };
    eprintln!("the shader runs on {}", adapter.get_info().name);
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("turn_cards"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let buffer = |data: &[u32], usage| {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (data.len() * 4) as u64,
            usage,
            mapped_at_creation: true,
        });
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(&bytes);
        buffer.unmap();
        buffer
    };
    let pictures = buffer(&pictures, wgpu::BufferUsages::STORAGE);
    let jobs = buffer(&jobs, wgpu::BufferUsages::STORAGE);
    let size = (expected.len() * 32) as u64;
    let outs = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("turn_cards"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: pictures.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: jobs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: outs.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(u32::try_from(expected.len()).unwrap(), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&outs, 0, &read, 0, size);
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes");
    let bytes = read.slice(..).get_mapped_range().to_vec();
    let words: Vec<u32> = bytes
        .chunks(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    for (k, (name, position, normal, picture)) in expected.iter().enumerate() {
        let w = &words[k * 8..k * 8 + 8];
        assert_eq!(w[3], *picture, "{name}, vertex {k}: the picture shown");
        for c in 0..3 {
            let (p, n) = (f32::from_bits(w[c]), f32::from_bits(w[4 + c]));
            assert!(
                (p - position[c]).abs() < 1e-4 && (n - normal[c]).abs() < 1e-4,
                "{name}, vertex {k}: {:?} {:?} on the GPU, {position:?} {normal:?} in PlantGen",
                &w[..3]
                    .iter()
                    .map(|b| f32::from_bits(*b))
                    .collect::<Vec<_>>(),
                &w[4..7]
                    .iter()
                    .map(|b| f32::from_bits(*b))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
