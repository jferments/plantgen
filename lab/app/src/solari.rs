//! The Solari spike (Joshi, 2026-10-08): one plant lit by Bevy 0.19.1's
//! experimental hardware ray tracing, for him to judge on his RTX 4090.
//! Built only with the `solari` feature; lavapipe and most cloud GPUs
//! lack ray queries, and then Bevy leaves Solari out with a warning.
//!
//! What Solari 0.19.1 takes, read from its source, and what this does
//! about it:
//! - every triangle is opaque (`AccelerationStructureGeometryFlags::OPAQUE`)
//!   and materials have no alpha: so leaf cards are cut into triangles
//!   along their outline ([`plantlab_scene::cut_cards`]);
//! - meshes need exactly POSITION, NORMAL, UV_0 and TANGENT with 32-bit
//!   indices, and a `StandardMaterial`, no vertex colours: so every colour
//!   goes into one palette texture, each vertex's UV pointing at its
//!   colour's texel;
//! - light comes from directional lights and emissive meshes: the sun, and
//!   a dome of sky around the plant.
//!
//! `--mode realtime` uses Solari's real-time lighting (ReSTIR, temporal
//! accumulation); `--mode pathtrace` its reference path tracer, which
//! converges to a clean picture over many frames.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::RenderAssetUsages;
use bevy::camera::{CameraMainTextureUsages, Exposure, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::light::GlobalAmbientLight;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshAllocatorSettings;
use bevy::render::render_resource::{
    BufferUsages, Extent3d, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::solari::pathtracer::{Pathtracer, PathtracingPlugin};
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};
use bevy::window::{ExitCondition, WindowPlugin};
use plantgen::library::Library;
use plantlab_scene::{Scene, SceneMesh, Shot};

use crate::render::{Compiling, count_compiling, target_image, to_rgba8};

/// How Solari lights the plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Realtime,
    Pathtrace,
}

/// One ray-traced picture.
pub struct Photo {
    pub shot: Shot,
    pub out: PathBuf,
    pub size: u32,
    pub mode: Mode,
    /// Frames drawn before the picture is taken, so lighting accumulates.
    pub frames: u32,
}

#[derive(Resource)]
struct Job {
    scene: Scene,
    out: PathBuf,
    name: String,
    target: Handle<Image>,
    frames: u32,
    drawn: u32,
    asked: bool,
    done: Arc<Mutex<Option<Result<String, String>>>>,
}

/// Grow, light and ray-trace one plant.
///
/// # Errors
///
/// When the plant does not grow, or the picture is not written (for
/// example on a GPU without ray queries, where Bevy leaves Solari out).
pub fn run(photo: Photo) -> Result<(), String> {
    let scene = plantlab_scene::build(&photo.shot, Library::builtin())?;
    let name = format!("{}-solari", scene.facts.species);
    let done = Arc::new(Mutex::new(None));
    let compiling = Compiling::default();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                filter: "wgpu=error,bevy_ecs::world::command_queue=error".into(),
                ..default()
            }),
        SolariPlugins,
    ));
    if photo.mode == Mode::Pathtrace {
        app.add_plugins(PathtracingPlugin);
    }
    if !app.is_plugin_added::<ScheduleRunnerPlugin>() {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO));
    }
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(target_image(photo.size, photo.size));
    app.insert_resource(GlobalAmbientLight::NONE)
        .insert_resource(compiling.clone())
        .insert_resource(Job {
            scene,
            out: photo.out,
            name,
            target,
            frames: photo.frames,
            drawn: 0,
            asked: false,
            done: done.clone(),
        })
        .insert_resource(PhotoMode(photo.mode))
        .add_systems(Startup, setup)
        .add_systems(Update, take);
    // Solari asks the mesh allocator for buffers it can trace (storage and
    // acceleration-structure input) in its `finish`, after the allocator has
    // read its settings, so the buffers lack them; ask before either.
    app.sub_app_mut(RenderApp)
        .insert_resource(MeshAllocatorSettings {
            extra_buffer_usages: BufferUsages::BLAS_INPUT | BufferUsages::STORAGE,
            ..default()
        })
        .insert_resource(compiling)
        .add_systems(Render, count_compiling.in_set(RenderSystems::Cleanup));
    app.run();
    let result = done.lock().ok().and_then(|mut done| done.take());
    match result {
        Some(Ok(path)) => {
            println!("wrote {path}");
            Ok(())
        }
        Some(Err(error)) => Err(error),
        None => Err(
            "the renderer stopped before the picture was taken (see the log above; \
                     Solari needs a GPU with ray queries)"
                .into(),
        ),
    }
}

#[derive(Resource, Clone, Copy)]
struct PhotoMode(Mode);

/// Every colour of the scene in one texture, a texel each.
struct Palette {
    side: u32,
    texels: Vec<[u8; 4]>,
    index: HashMap<[u8; 4], u32>,
}

impl Palette {
    fn new() -> Self {
        Self {
            side: 0,
            texels: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// The texel of a linear colour, sRGB-encoded to 8 bits.
    fn texel(&mut self, linear: [f32; 4]) -> u32 {
        let key = [
            plantgen::raster::to_u8(plantgen::raster::srgb(linear[0])),
            plantgen::raster::to_u8(plantgen::raster::srgb(linear[1])),
            plantgen::raster::to_u8(plantgen::raster::srgb(linear[2])),
            255,
        ];
        let next = u32::try_from(self.texels.len()).unwrap_or(u32::MAX);
        *self.index.entry(key).or_insert_with(|| {
            self.texels.push(key);
            next
        })
    }

    /// Freeze the palette's size; UVs are made after this.
    fn finish(&mut self) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let side = (self.texels.len().max(1) as f64).sqrt().ceil() as u32;
        self.side = side.max(1);
    }

    #[allow(clippy::cast_precision_loss)]
    fn uv(&self, texel: u32) -> [f32; 2] {
        let side = self.side as f32;
        let (x, y) = (texel % self.side, texel / self.side);
        [(x as f32 + 0.5) / side, (y as f32 + 0.5) / side]
    }

    fn image(&self) -> Image {
        let count = (self.side * self.side) as usize;
        let mut data = Vec::with_capacity(count * 4);
        for at in 0..count {
            data.extend(self.texels.get(at).copied().unwrap_or([0, 0, 0, 255]));
        }
        let mut image = Image::new(
            Extent3d {
                width: self.side,
                height: self.side,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
        image
    }
}

/// A mesh as Solari takes it: positions, normals, palette UVs, tangents,
/// 32-bit indices.
fn traced_mesh(mesh: &SceneMesh, texels: &[u32], palette: &Palette) -> Option<Mesh> {
    if mesh.is_empty() {
        return None;
    }
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone());
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone());
    let uvs: Vec<[f32; 2]> = texels.iter().map(|&t| palette.uv(t)).collect();
    out.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    out.insert_indices(Indices::U32(mesh.indices.clone()));
    let mut out = out.with_generated_tangents().ok()?;
    out.enable_raytracing = true;
    Some(out)
}

fn texels(mesh: &SceneMesh, palette: &mut Palette) -> Vec<u32> {
    mesh.colors.iter().map(|&c| palette.texel(c)).collect()
}

/// A disc of ground, `radius` metres, as Solari takes it.
fn ground(radius: f32, texel: u32, palette: &Palette) -> Option<Mesh> {
    let mut mesh = Mesh::from(Circle::new(radius));
    let uv = palette.uv(texel);
    let count = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![uv; count]);
    if let Some(Indices::U16(indices)) = mesh.indices() {
        let wide: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();
        mesh.insert_indices(Indices::U32(wide));
    }
    // Circle meshes lie in XY facing +Z; lay it down facing up.
    if let Some(VertexAttributeValues::Float32x3(points)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in points.iter_mut() {
            *p = [p[0], 0.0, -p[1]];
        }
    }
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        for n in normals.iter_mut() {
            *n = [0.0, 1.0, 0.0];
        }
    }
    let mut mesh = mesh.with_generated_tangents().ok()?;
    mesh.enable_raytracing = true;
    Some(mesh)
}

/// A dome of sky around the plant, facing in, that lights it.
fn sky_dome(radius: f32) -> Option<Mesh> {
    let mut mesh = Sphere::new(radius).mesh().uv(48, 24);
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        for n in normals.iter_mut() {
            *n = [-n[0], -n[1], -n[2]];
        }
    }
    let indices: Vec<u32> = match mesh.indices() {
        Some(Indices::U16(i)) => i.iter().map(|&i| u32::from(i)).collect(),
        Some(Indices::U32(i)) => i.clone(),
        None => return None,
    };
    // Reverse each triangle so it faces in.
    let flipped: Vec<u32> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|t| [t[0], t[2], t[1]])
        .collect();
    mesh.insert_indices(Indices::U32(flipped));
    let mut mesh = mesh.with_generated_tangents().ok()?;
    mesh.enable_raytracing = true;
    Some(mesh)
}

fn setup(
    mut commands: Commands,
    job: Res<Job>,
    mode: Res<PhotoMode>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let scene = &job.scene;
    let mut palette = Palette::new();
    let [gr, gg, gb] = plantlab_scene::GROUND;
    let ground_texel = palette.texel([gr, gg, gb, 1.0]);
    let parts: Vec<(&SceneMesh, Vec<u32>)> = [&scene.wood, &scene.solids, &scene.cut_cards]
        .into_iter()
        .map(|mesh| (mesh, texels(mesh, &mut palette)))
        .collect();
    palette.finish();
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(images.add(palette.image())),
        perceptual_roughness: 0.8,
        reflectance: 0.3,
        ..default()
    });
    for (mesh, texels) in &parts {
        if let Some(traced) = traced_mesh(mesh, texels, &palette) {
            commands.spawn((
                RaytracingMesh3d(meshes.add(traced)),
                MeshMaterial3d(material.clone()),
            ));
        }
    }
    if let Some(disc) = ground(scene.ground_radius, ground_texel, &palette) {
        commands.spawn((
            RaytracingMesh3d(meshes.add(disc)),
            MeshMaterial3d(material.clone()),
        ));
    }
    let light = scene.light;
    let sky = LinearRgba::rgb(light.sky_color[0], light.sky_color[1], light.sky_color[2]);
    if let Some(dome) = sky_dome(scene.ground_radius * 20.0 + 100.0) {
        commands.spawn((
            RaytracingMesh3d(meshes.add(dome)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::BLACK,
                // Emission in cd/m², the sky's luminance.
                emissive: sky * light.sky_brightness,
                ..default()
            })),
        ));
    }
    commands.spawn((
        DirectionalLight {
            color: Color::linear_rgb(light.sun_color[0], light.sun_color[1], light.sun_color[2]),
            illuminance: light.sun_lux,
            // Solari traces the sun's shadows itself.
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(Vec3::from_array(light.sun)).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let framing = scene.framing;
    let ev100 = (light.sun_lux / (1.2 * std::f32::consts::PI)).log2();
    let mut camera = commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(job.target.clone().into()),
        Exposure { ev100 },
        Tonemapping::AcesFitted,
        // Solari writes the main texture from compute passes.
        CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
        Msaa::Off,
        Transform::from_translation(Vec3::from_array(framing.eye)).looking_at(
            Vec3::from_array(framing.target),
            Vec3::from_array(framing.up),
        ),
    ));
    if let plantlab_scene::Projection::Perspective { fov_y_deg } = framing.projection {
        camera.insert(Projection::Perspective(PerspectiveProjection {
            fov: fov_y_deg.to_radians(),
            ..default()
        }));
    }
    match mode.0 {
        Mode::Realtime => camera.insert(SolariLighting::default()),
        Mode::Pathtrace => camera.insert(Pathtracer::default()),
    };
}

/// After the pipelines are ready and `frames` more frames, take the
/// picture.
fn take(mut commands: Commands, mut job: ResMut<Job>, compiling: Res<Compiling>) {
    if job.asked {
        return;
    }
    if compiling.0.load(Ordering::Relaxed) > 0 {
        job.drawn = 0;
        return;
    }
    job.drawn += 1;
    if job.drawn < job.frames {
        return;
    }
    job.asked = true;
    commands
        .spawn(Screenshot::image(job.target.clone()))
        .observe(captured);
}

fn captured(event: On<ScreenshotCaptured>, job: Res<Job>, mut exit: MessageWriter<AppExit>) {
    let image = &event.image;
    let result = image
        .data
        .as_ref()
        .ok_or_else(|| "the picture came back empty".to_string())
        .and_then(|data| {
            let rgba = to_rgba8(data, image.texture_descriptor.format);
            let png = plantgen::raster::encode_png(
                image.width() as usize,
                image.height() as usize,
                &rgba,
            )
            .map_err(|error| format!("cannot encode PNG: {error}"))?;
            std::fs::create_dir_all(&job.out)
                .map_err(|error| format!("cannot create {}: {error}", job.out.display()))?;
            let path = job.out.join(format!("{}.png", job.name));
            std::fs::write(&path, png)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            Ok(path.display().to_string())
        });
    if let Ok(mut done) = job.done.lock() {
        *done = Some(result);
    }
    exit.write(AppExit::Success);
}
