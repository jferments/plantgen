//! Drawing scenes with Bevy, without a window: each job's scene is drawn
//! into an image, read back once every pipeline it needs is ready, and
//! written as a PNG with its sidecar. Jobs run one after another in one
//! app, so pipelines compile once.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::{Exposure, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, PipelineCache, ShaderType, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::window::{ExitCondition, WindowPlugin};
use plantgen::library::Library;
use plantlab_scene::{MAX_TEMPLATES, Projection as SceneProjection, Scene, SceneMesh, Shot};

/// One picture to make.
pub struct Job {
    /// The files' stem: `NAME.png` and `NAME.json`.
    pub name: String,
    pub shot: Shot,
}

/// Frames a scene stands before it is read back, once nothing is still
/// compiling: meshes, textures and materials reach the GPU over the
/// first frames after they are made.
const SETTLE_FRAMES: u32 = 4;
/// Frames to wait for pipelines before giving up on a scene.
const PATIENCE_FRAMES: u32 = 2_000;
/// Readbacks of a picture showing nothing but background before giving
/// up on it.
const RETRIES: u32 = 3;

type CardMaterial = ExtendedMaterial<StandardMaterial, CardExtension>;

/// Draws the organ cards of `plantlab-scene` (see `shaders/card.wgsl`).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CardExtension {
    #[uniform(100)]
    accents: CardAccents,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    templates: Handle<Image>,
}

#[derive(ShaderType, Reflect, Debug, Clone)]
struct CardAccents {
    accents: [Vec4; MAX_TEMPLATES],
}

impl MaterialExtension for CardExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://plantlab/shaders/card.wgsl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://plantlab/shaders/card.wgsl".into()
    }
}

/// The jobs still to do and where pictures go.
#[derive(Resource)]
struct Jobs {
    queue: VecDeque<Job>,
    out: PathBuf,
    width: u32,
    height: u32,
    failures: Failures,
    library: &'static Library,
}

/// Pictures that failed, shared with the caller: the app's world is gone
/// once it has run.
#[derive(Clone, Default)]
struct Failures(Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>);

impl Failures {
    /// A job finished, written or failed.
    fn finished(&self) {
        self.1.fetch_add(1, Ordering::Relaxed);
    }

    fn push(&self, error: String) {
        self.finished();
        eprintln!("{error}");
        if let Ok(mut list) = self.0.lock() {
            list.push(error);
        }
    }
}

/// The scene on stage.
#[derive(Resource, Default)]
struct Stage {
    current: Option<Current>,
}

struct Current {
    name: String,
    scene: Scene,
    target: Handle<Image>,
    entities: Vec<Entity>,
    frames: u32,
    settled: u32,
    asked: bool,
    tries: u32,
    done: bool,
}

/// Pipelines the render world is still compiling, counted after each
/// frame's render.
#[derive(Resource, Clone, Default)]
struct Compiling(Arc<AtomicUsize>);

/// Run `jobs`, writing pictures `width` by `height` into `out`.
///
/// # Errors
///
/// When any picture failed; the others are still written.
pub fn run(jobs: Vec<Job>, out: PathBuf, width: u32, height: u32) -> Result<(), String> {
    let compiling = Compiling::default();
    let failures = Failures::default();
    let total = jobs.len();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                // Bevy warns of commands left when the app ends; nothing is lost.
                filter: "wgpu=error,bevy_ecs::world::command_queue=error".into(),
                ..default()
            }),
    );
    if !app.is_plugin_added::<ScheduleRunnerPlugin>() {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO));
    }
    embedded_asset!(app, "shaders/card.wgsl");
    app.add_plugins(MaterialPlugin::<CardMaterial>::default())
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(GlobalAmbientLight::NONE)
        .insert_resource(Jobs {
            queue: jobs.into(),
            out,
            width,
            height,
            failures: failures.clone(),
            library: Library::builtin(),
        })
        .insert_resource(compiling.clone())
        .init_resource::<Stage>()
        .add_systems(Update, (stage_next, settle).chain());
    app.sub_app_mut(RenderApp)
        .insert_resource(compiling)
        .add_systems(Render, count_compiling.in_set(RenderSystems::Cleanup));
    app.run();
    let list = failures
        .0
        .lock()
        .map(|list| list.clone())
        .unwrap_or_default();
    let finished = failures.1.load(Ordering::Relaxed);
    if finished < total {
        // Bevy quits on a rendering error, as if all went well.
        Err(format!(
            "the renderer stopped after {finished} of {total} pictures (see the log above)"
        ))
    } else if list.is_empty() {
        Ok(())
    } else {
        Err(format!("{} of {total} pictures failed", list.len()))
    }
}

fn count_compiling(cache: Res<PipelineCache>, compiling: Res<Compiling>) {
    compiling
        .0
        .store(cache.waiting_pipelines().count(), Ordering::Relaxed);
}

/// Put the next job's scene on stage, or exit when none is left.
#[allow(clippy::too_many_arguments)]
fn stage_next(
    mut commands: Commands,
    mut jobs: ResMut<Jobs>,
    mut stage: ResMut<Stage>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut cards: ResMut<Assets<CardMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(current) = &stage.current {
        if !current.done {
            return;
        }
        for &entity in &current.entities {
            commands.entity(entity).despawn();
        }
        stage.current = None;
    }
    let Some(job) = jobs.queue.pop_front() else {
        exit.write(AppExit::Success);
        return;
    };
    let scene = match plantlab_scene::build(&job.shot, jobs.library) {
        Ok(scene) => scene,
        Err(error) => {
            jobs.failures.push(format!("{}: {error}", job.name));
            return;
        }
    };
    if scene.templates.layers > MAX_TEMPLATES {
        let error = format!(
            "{} templates, more than PlantLab draws ({MAX_TEMPLATES})",
            scene.templates.layers
        );
        jobs.failures.push(format!("{}: {error}", job.name));
        return;
    }
    let target = images.add(target_image(jobs.width, jobs.height));
    let entities = spawn_scene(
        &mut commands,
        &scene,
        &target,
        (&mut images, &mut meshes, &mut standard, &mut cards),
        jobs.width,
        jobs.height,
    );
    stage.current = Some(Current {
        name: job.name,
        scene,
        target,
        entities,
        frames: 0,
        settled: 0,
        asked: false,
        tries: 0,
        done: false,
    });
}

/// Wait for the stage to settle, then read it back.
fn settle(
    mut commands: Commands,
    mut stage: ResMut<Stage>,
    jobs: Res<Jobs>,
    compiling: Res<Compiling>,
) {
    let Some(current) = stage.current.as_mut() else {
        return;
    };
    if current.done || current.asked {
        return;
    }
    current.frames += 1;
    if compiling.0.load(Ordering::Relaxed) == 0 {
        current.settled += 1;
    } else {
        current.settled = 0;
    }
    if current.frames > PATIENCE_FRAMES {
        jobs.failures.push(format!(
            "{}: pipelines never finished compiling",
            current.name
        ));
        current.done = true;
        return;
    }
    if current.settled < SETTLE_FRAMES {
        return;
    }
    current.asked = true;
    commands
        .spawn(Screenshot::image(current.target.clone()))
        .observe(captured);
}

fn captured(event: On<ScreenshotCaptured>, mut stage: ResMut<Stage>, jobs: Res<Jobs>) {
    let Some(current) = stage.current.as_mut() else {
        return;
    };
    let image = &event.image;
    let (width, height) = (image.width(), image.height());
    let Some(data) = image.data.as_ref() else {
        current.asked = false;
        return;
    };
    let rgba = to_rgba8(data, image.texture_descriptor.format);
    if shows_only_background(&rgba) && current.tries < RETRIES {
        // Not drawn yet: read it back again after a few more frames.
        current.tries += 1;
        current.asked = false;
        current.settled = 0;
        return;
    }
    let result = write(
        &jobs.out,
        &current.name,
        &current.scene,
        width,
        height,
        &rgba,
    );
    match result {
        Ok(path) => {
            jobs.failures.finished();
            println!("wrote {path}");
        }
        Err(error) => jobs.failures.push(error),
    }
    current.done = true;
}

fn write(
    out: &std::path::Path,
    name: &str,
    scene: &Scene,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<String, String> {
    let picture = format!("{name}.png");
    let png = plantgen::raster::encode_png(width as usize, height as usize, rgba)
        .map_err(|error| format!("{name}: cannot encode PNG: {error}"))?;
    let path = out.join(&picture);
    std::fs::write(&path, png)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    let sidecar = plantlab_scene::sidecar(&scene.facts, &picture, width, height);
    let json = out.join(format!("{name}.json"));
    std::fs::write(&json, sidecar)
        .map_err(|error| format!("cannot write {}: {error}", json.display()))?;
    Ok(path.display().to_string())
}

/// The picture as sRGB RGBA8, whatever the readback's format.
fn to_rgba8(data: &[u8], format: TextureFormat) -> Vec<u8> {
    match format {
        TextureFormat::Bgra8UnormSrgb | TextureFormat::Bgra8Unorm => data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect(),
        _ => data.to_vec(),
    }
}

/// Whether every pixel is the background's: nothing drawn yet.
fn shows_only_background(rgba: &[u8]) -> bool {
    let Some(first) = rgba.get(..4) else {
        return true;
    };
    rgba.as_chunks::<4>().0.iter().all(|p| p[..] == *first)
}

fn target_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    image
}

fn bevy_mesh(mesh: &SceneMesh) -> Mesh {
    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone());
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone());
    out.insert_attribute(Mesh::ATTRIBUTE_UV_0, mesh.uvs.clone());
    out.insert_attribute(Mesh::ATTRIBUTE_COLOR, mesh.colors.clone());
    if !mesh.darkening.is_empty() {
        let uv_b: Vec<[f32; 2]> = mesh.darkening.iter().map(|&d| [d, 0.0]).collect();
        out.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv_b);
    }
    out.insert_indices(Indices::U32(mesh.indices.clone()));
    out
}

fn templates_image(scene: &Scene) -> Image {
    let layers = &scene.templates;
    let side = u32::try_from(layers.size).unwrap_or(128);
    let count = u32::try_from(layers.layers).unwrap_or(1);
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: count,
        },
        TextureDimension::D2,
        layers.rgba.clone(),
        // The templates are data, not colours: never decoded from sRGB.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    // An array even with one layer, as the shader declares it.
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image
}

fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::from_array(v)
}

/// Spawn a scene's camera, light and meshes, drawing into `target`.
#[allow(clippy::too_many_lines)]
fn spawn_scene(
    commands: &mut Commands,
    scene: &Scene,
    target: &Handle<Image>,
    (images, meshes, standard, cards): (
        &mut Assets<Image>,
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &mut Assets<CardMaterial>,
    ),
    width: u32,
    height: u32,
) -> Vec<Entity> {
    let mut entities = Vec::new();
    let framing = scene.framing;
    let projection = match framing.projection {
        SceneProjection::Perspective { fov_y_deg } => {
            Projection::Perspective(PerspectiveProjection {
                fov: fov_y_deg.to_radians(),
                #[allow(clippy::cast_precision_loss)]
                aspect_ratio: width as f32 / height as f32,
                ..default()
            })
        }
        SceneProjection::Orthographic { half_height } => {
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 2.0 * half_height,
                },
                ..OrthographicProjection::default_3d()
            })
        }
    };
    let light = scene.light;
    // Exposure that shows a white surface facing the sun as white: Bevy's
    // diffuse is albedo / π times the illuminance, and its exposure is
    // 1 / (1.2 · 2^EV100).
    let ev100 = (light.sun_lux / (1.2 * std::f32::consts::PI)).log2();
    let [r, g, b] = plantlab_scene::BACKGROUND;
    entities.push(
        commands
            .spawn((
                Camera3d::default(),
                Camera {
                    clear_color: ClearColorConfig::Custom(Color::linear_rgb(r, g, b)),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                projection,
                Tonemapping::None,
                Exposure { ev100 },
                AmbientLight {
                    color: Color::linear_rgb(
                        light.sky_color[0],
                        light.sky_color[1],
                        light.sky_color[2],
                    ),
                    brightness: light.sky_brightness,
                    affects_lightmapped_meshes: true,
                },
                Msaa::Sample4,
                Transform::from_translation(vec3(framing.eye))
                    .looking_at(vec3(framing.target), vec3(framing.up)),
            ))
            .id(),
    );
    let (low, high) = scene.shadow_bounds;
    let reach = (vec3(high) - vec3(low)).length();
    entities.push(
        commands
            .spawn((
                DirectionalLight {
                    color: Color::linear_rgb(
                        light.sun_color[0],
                        light.sun_color[1],
                        light.sun_color[2],
                    ),
                    illuminance: light.sun_lux,
                    shadow_maps_enabled: true,
                    ..default()
                },
                CascadeShadowConfigBuilder {
                    num_cascades: 1,
                    minimum_distance: 0.1,
                    maximum_distance: reach + vec3(framing.eye).length(),
                    ..default()
                }
                .build(),
                Transform::from_translation(vec3(light.sun)).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id(),
    );
    let ground = Circle::new(scene.ground_radius);
    let [gr, gg, gb] = plantlab_scene::GROUND;
    entities.push(
        commands
            .spawn((
                Mesh3d(meshes.add(ground)),
                MeshMaterial3d(standard.add(StandardMaterial {
                    base_color: Color::linear_rgb(gr, gg, gb),
                    perceptual_roughness: 1.0,
                    reflectance: 0.2,
                    ..default()
                })),
                Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            ))
            .id(),
    );
    let solid = standard.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        ..default()
    });
    for mesh in [&scene.wood, &scene.solids] {
        if mesh.is_empty() {
            continue;
        }
        entities.push(
            commands
                .spawn((
                    Mesh3d(meshes.add(bevy_mesh(mesh))),
                    MeshMaterial3d(solid.clone()),
                ))
                .id(),
        );
    }
    if !scene.cards.is_empty() {
        let mut accents = [Vec4::ZERO; MAX_TEMPLATES];
        for (slot, accent) in accents.iter_mut().zip(&scene.template_accents) {
            *slot = Vec3::from_array(*accent).extend(1.0);
        }
        let material = cards.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.8,
                reflectance: 0.3,
                double_sided: true,
                cull_mode: None,
                alpha_mode: AlphaMode::Mask(0.5),
                diffuse_transmission: 0.35,
                ..default()
            },
            extension: CardExtension {
                accents: CardAccents { accents },
                templates: images.add(templates_image(scene)),
            },
        });
        entities.push(
            commands
                .spawn((
                    Mesh3d(meshes.add(bevy_mesh(&scene.cards))),
                    MeshMaterial3d(material),
                ))
                .id(),
        );
    }
    entities
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws one plant on whatever GPU there is (lavapipe in a cloud
    /// session): `cargo test -p plantlab -- --ignored`.
    #[test]
    #[ignore = "needs a GPU or lavapipe"]
    fn a_thumbnail_is_drawn_and_described() {
        let out = std::env::temp_dir().join(format!("plantlab-test-{}", std::process::id()));
        std::fs::create_dir_all(&out).expect("a temporary folder");
        let mut shot = Shot::thumbnail("polystichum-munitum");
        shot.quality = plantgen::quality::DRAFT;
        let jobs = vec![Job {
            name: "fern".into(),
            shot,
        }];
        run(jobs, out.clone(), 128, 128).expect("renders");
        let png = std::fs::read(out.join("fern.png")).expect("the picture");
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().expect("a PNG");
        let mut rgba = vec![0; reader.output_buffer_size().expect("a size")];
        reader.next_frame(&mut rgba).expect("its pixels");
        // The plant and its ground cover a good share of the picture.
        let background = rgba[..4].to_vec();
        let drawn = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..] != background[..])
            .count();
        assert!(drawn * 10 > 128 * 128, "only {drawn} pixels drawn");
        let sidecar = std::fs::read_to_string(out.join("fern.json")).expect("the sidecar");
        assert!(sidecar.contains("\"species\": \"polystichum-munitum\""));
        let _ = std::fs::remove_dir_all(&out);
    }
}
