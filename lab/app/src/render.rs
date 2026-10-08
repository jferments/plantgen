//! Drawing sheets with Bevy, without a window. A job is a sheet: one or
//! more tiles, each a plant drawn by its own camera into its rectangle of
//! one image, and labels drawn over them. The image is read back once every
//! pipeline it needs is ready and written as a PNG with its sidecar. Jobs
//! run one after another in one app, so pipelines compile once.
//!
//! Tiles stand [`TILE_SPACING`] metres apart and on their own render
//! layers, so no tile sees, lights or shades another's plant. One sun
//! lights them all: Bevy holds at most ten directional lights.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::Viewport;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::system::EntityCommands;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::light::{
    CascadeShadowConfigBuilder, DirectionalLightShadowMap, EnvironmentMapLight, GlobalAmbientLight,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, ScreenSpaceAmbientOcclusion};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, PipelineCache, ShaderType, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::text::FontSize;
use bevy::window::{ExitCondition, WindowPlugin};
use plantgen::library::Library;
use plantlab_scene::{
    Facts, Look, MAX_TEMPLATES, Projection as SceneProjection, Scene, SceneMesh, Sheet,
};

/// One picture to make.
pub struct Job {
    /// The files' stem: `NAME.png` and `NAME.json`.
    pub name: String,
    pub sheet: Sheet,
    /// What decides the picture, recorded in its sidecar so a later run
    /// can skip it when nothing changed.
    pub key: String,
}

/// Metres between tiles' plants.
const TILE_SPACING: f32 = 2_000.0;

/// Frames a scene stands before it is read back, once nothing is still
/// compiling: meshes, textures and materials reach the GPU over the
/// first frames after they are made.
const SETTLE_FRAMES: u32 = 4;
/// Frames to wait for pipelines before giving up on a scene.
const PATIENCE_FRAMES: u32 = 2_000;
/// Readbacks of a picture showing nothing but background before giving
/// up on it.
const RETRIES: u32 = 3;

pub(crate) type CardMaterial = ExtendedMaterial<StandardMaterial, CardExtension>;

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
    key: String,
    sheet: Sheet,
    facts: Vec<Facts>,
    /// Times the sheet is drawn larger, per side.
    supersample: u32,
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
pub(crate) struct Compiling(pub(crate) Arc<AtomicUsize>);

/// Run `jobs`, writing pictures into `out`, on GPU `gpu` (an index of
/// `plantlab gpus`), else the one Bevy chooses.
///
/// # Errors
///
/// When the GPU cannot be opened or any picture failed; the others are
/// still written.
pub fn run(jobs: Vec<Job>, out: PathBuf, gpu: Option<usize>) -> Result<(), String> {
    let compiling = Compiling::default();
    let failures = Failures::default();
    let total = jobs.len();
    let shadow_map = if jobs
        .iter()
        .flat_map(|job| &job.sheet.tiles)
        .any(|tile| tile.shot.look == Look::Photo)
    {
        PHOTO_SHADOW_MAP
    } else {
        REVIEW_SHADOW_MAP
    };
    let render = match gpu {
        Some(index) => RenderPlugin {
            render_creation: crate::gpu::creation(index)?,
            ..default()
        },
        None => RenderPlugin::default(),
    };
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(render)
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            // No window: drawing goes to images, and the loop is ours.
            .disable::<bevy::winit::WinitPlugin>()
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
        .insert_resource(DirectionalLightShadowMap { size: shadow_map })
        .insert_resource(GlobalAmbientLight::NONE)
        .insert_resource(Jobs {
            queue: jobs.into(),
            out,
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

pub(crate) fn count_compiling(cache: Res<PipelineCache>, compiling: Res<Compiling>) {
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
    let mut scenes = Vec::with_capacity(job.sheet.tiles.len());
    for tile in &job.sheet.tiles {
        match plantlab_scene::build(&tile.shot, jobs.library) {
            Ok(scene) if scene.templates.layers > MAX_TEMPLATES => {
                jobs.failures.push(format!(
                    "{}: {} templates, more than PlantLab draws ({MAX_TEMPLATES})",
                    job.name, scene.templates.layers
                ));
                return;
            }
            Ok(scene) => scenes.push(scene),
            Err(error) => {
                jobs.failures.push(format!("{}: {error}", job.name));
                return;
            }
        }
    }
    let k = scenes.first().map_or(1, |scene| scene.look.supersample());
    let sheet = job.sheet;
    let target = images.add(target_image(sheet.width * k, sheet.height * k));
    let mut entities = vec![spawn_sun(&mut commands, &scenes)];
    for (index, (scene, tile)) in scenes.iter().zip(&sheet.tiles).enumerate() {
        entities.extend(spawn_scene(
            &mut commands,
            scene,
            &target,
            (&mut images, &mut meshes, &mut standard, &mut cards),
            Placement {
                index,
                rect: tile.rect,
                supersample: k,
            },
        ));
    }
    entities.extend(spawn_labels(&mut commands, &sheet, &target, k));
    stage.current = Some(Current {
        name: job.name,
        key: job.key,
        facts: scenes.into_iter().map(|scene| scene.facts).collect(),
        sheet,
        supersample: k,
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
    let k = current.supersample;
    let rgba = downsample(
        &to_rgba8(data, image.texture_descriptor.format),
        width,
        height,
        k,
    );
    let (width, height) = (width / k, height / k);
    if shows_only_background(&rgba) && current.tries < RETRIES {
        // Not drawn yet: read it back again after a few more frames.
        current.tries += 1;
        current.asked = false;
        current.settled = 0;
        return;
    }
    let result = write(&jobs.out, current, width, height, &rgba);
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
    current: &Current,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<String, String> {
    let name = &current.name;
    let picture = format!("{name}.png");
    let png = plantgen::raster::encode_png(width as usize, height as usize, rgba)
        .map_err(|error| format!("{name}: cannot encode PNG: {error}"))?;
    let path = out.join(&picture);
    std::fs::write(&path, png)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    let sidecar = match current.facts.as_slice() {
        [facts] if current.sheet.labels.is_empty() => {
            plantlab_scene::sidecar(facts, &picture, width, height)
        }
        facts => plantlab_scene::sheet_sidecar(&current.sheet, &picture, facts),
    };
    // The key first, so a later run finds it at once.
    let sidecar = sidecar.replacen("{\n", &format!("{{\n  \"key\": \"{}\",\n", current.key), 1);
    let json = out.join(format!("{name}.json"));
    std::fs::write(&json, sidecar)
        .map_err(|error| format!("cannot write {}: {error}", json.display()))?;
    Ok(path.display().to_string())
}

/// The picture as sRGB RGBA8, whatever the readback's format.
pub(crate) fn to_rgba8(data: &[u8], format: TextureFormat) -> Vec<u8> {
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

/// Average each `k` by `k` block of an sRGB picture, in linear light.
fn downsample(rgba: &[u8], width: u32, height: u32, k: u32) -> Vec<u8> {
    if k <= 1 {
        return rgba.to_vec();
    }
    let (w, h, k) = (width as usize, height as usize, k as usize);
    let (ow, oh) = (w / k, h / k);
    let decode: Vec<f32> = (0..=255_u8)
        .map(|v| {
            let c = f32::from(v) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect();
    let mut out = Vec::with_capacity(ow * oh * 4);
    #[allow(clippy::cast_precision_loss)]
    let n = (k * k) as f32;
    for y in 0..oh {
        for x in 0..ow {
            let mut sum = [0.0_f32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let at = ((y * k + dy) * w + x * k + dx) * 4;
                    for c in 0..3 {
                        sum[c] += decode[usize::from(rgba[at + c])];
                    }
                    sum[3] += f32::from(rgba[at + 3]);
                }
            }
            for channel in &sum[..3] {
                out.push(plantgen::raster::to_u8(plantgen::raster::srgb(channel / n)));
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            out.push((sum[3] / n).round() as u8);
        }
    }
    out
}

/// Whether every pixel is the background's: nothing drawn yet.
fn shows_only_background(rgba: &[u8]) -> bool {
    let Some(first) = rgba.get(..4) else {
        return true;
    };
    rgba.as_chunks::<4>().0.iter().all(|p| p[..] == *first)
}

pub(crate) fn target_image(width: u32, height: u32) -> Image {
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

/// The sheet's title, headings and captions, and a thin frame around each
/// tile, drawn last over the tiles by a UI camera.
#[allow(clippy::many_single_char_names)]
fn spawn_labels(
    commands: &mut Commands,
    sheet: &Sheet,
    target: &Handle<Image>,
    k: u32,
) -> Vec<Entity> {
    if sheet.labels.is_empty() {
        return Vec::new();
    }
    #[allow(clippy::cast_precision_loss)]
    let k = k as f32;
    #[allow(clippy::cast_precision_loss)]
    let px = |v: u32| Val::Px(v as f32 * k);
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 1_000,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            // Nothing in the world: only the UI.
            RenderLayers::layer(31),
        ))
        .id();
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();
    for label in &sheet.labels {
        let text = commands
            .spawn((
                Text::new(label.text.clone()),
                TextFont {
                    font_size: FontSize::Px(label.size * k),
                    ..default()
                },
                TextColor(Color::srgb(0.93, 0.93, 0.9)),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(label.x),
                    top: px(label.y),
                    ..default()
                },
            ))
            .id();
        commands.entity(root).add_child(text);
    }
    for tile in &sheet.tiles {
        let [x, y, w, h] = tile.rect;
        let frame = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x),
                    top: px(y),
                    width: px(w),
                    height: px(h),
                    border: UiRect::all(Val::Px(k)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.45, 0.45, 0.45)),
            ))
            .id();
        commands.entity(root).add_child(frame);
    }
    vec![camera, root]
}

/// The photo look's soft shadow size, in the units Bevy's percentage-closer
/// soft shadows take: a penumbra grows as `(z_blocker - z) · size / z` in
/// shadow-map texels, with depths in the light's 0 to 1 range. A sun of
/// angular diameter θ casts a penumbra θ · d wide at a distance d behind its
/// blocker, so at mid depth (z = ½) `size = θ · range / (2 · texel)`, for a
/// cascade `range` metres deep and texels `texel` metres wide. An
/// approximation: exact at mid depth only. `None` (hard shadows) for the
/// review look.
fn soft_shadows(scene: &Scene, reach: f32) -> Option<f32> {
    // (The cascade covers `reach` metres of the camera's view.)
    if scene.look != Look::Photo {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let texel = reach / PHOTO_SHADOW_MAP as f32;
    Some(plantlab_scene::SUN_ANGULAR_DIAMETER * reach / (2.0 * texel))
}

/// Shadow map side, texels, for each look.
const REVIEW_SHADOW_MAP: usize = 4096;
const PHOTO_SHADOW_MAP: usize = 8192;

/// Where a tile is drawn: its index among the sheet's tiles, its
/// rectangle in pixels and how much larger the sheet is drawn.
struct Placement {
    index: usize,
    rect: [u32; 4],
    supersample: u32,
}

impl Placement {
    /// The tile's place in the world: far from every other tile's.
    #[allow(clippy::cast_precision_loss)]
    fn offset(&self) -> Vec3 {
        Vec3::new(self.index as f32 * TILE_SPACING, 0.0, 0.0)
    }

    /// The tile's own render layer; layer 0 is left to nothing.
    fn layer(&self) -> RenderLayers {
        RenderLayers::layer(self.index + 1)
    }
}

/// The sun, shared by every tile of a sheet and seeing all their layers.
pub(crate) fn spawn_sun(commands: &mut Commands, scenes: &[Scene]) -> Entity {
    let Some(scene) = scenes.first() else {
        return commands.spawn_empty().id();
    };
    let light = scene.light;
    // Room for the farthest-reaching tile's shadows.
    let reach = scenes
        .iter()
        .map(|scene| {
            let (low, high) = scene.shadow_bounds;
            (vec3(high) - vec3(low)).length() + vec3(scene.framing.eye).length()
        })
        .fold(0.0_f32, f32::max);
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
                soft_shadow_size: soft_shadows(scene, reach),
                ..default()
            },
            CascadeShadowConfigBuilder {
                num_cascades: 1,
                minimum_distance: 0.1,
                maximum_distance: reach,
                ..default()
            }
            .build(),
            RenderLayers::from_layers(&(0..=scenes.len()).collect::<Vec<_>>()),
            Transform::from_translation(vec3(light.sun)).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id()
}

/// Spawn a scene's camera and meshes, drawing into its tile of `target`.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
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
    place: Placement,
) -> Vec<Entity> {
    let mut entities = Vec::new();
    let framing = scene.framing;
    let [x, y, width, height] = place.rect;
    let k = place.supersample;
    let offset = place.offset();
    let layer = place.layer();
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
    let ev100 = exposure(&scene.light);
    let [r, g, b] = plantlab_scene::BACKGROUND;
    entities.push(
        commands
            .spawn((
                Camera3d::default(),
                Camera {
                    // A clear clears the whole image, viewport or not: only
                    // the first tile's camera clears it.
                    clear_color: if place.index == 0 {
                        ClearColorConfig::Custom(Color::linear_rgb(r, g, b))
                    } else {
                        ClearColorConfig::None
                    },
                    viewport: Some(Viewport {
                        physical_position: UVec2::new(x * k, y * k),
                        physical_size: UVec2::new(width * k, height * k),
                        ..default()
                    }),
                    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                    order: place.index as isize,
                    ..default()
                },
                layer.clone(),
                RenderTarget::Image(target.clone().into()),
                projection,
                Exposure { ev100 },
                Transform::from_translation(vec3(framing.eye) + offset)
                    .looking_at(vec3(framing.target) + offset, vec3(framing.up)),
            ))
            .id(),
    );
    camera_look(
        &mut commands.entity(entities[0]),
        scene.look,
        &scene.light,
        images,
    );
    entities.extend(spawn_plant(
        commands,
        scene,
        (images, meshes, standard, cards),
        offset,
        &layer,
    ));
    entities
}

/// Exposure that shows a white surface facing the sun as white: Bevy's
/// diffuse is albedo / π times the illuminance, and its exposure is
/// 1 / (1.2 · 2^EV100).
pub(crate) fn exposure(light: &plantlab_scene::Light) -> f32 {
    (light.sun_lux / (1.2 * std::f32::consts::PI)).log2()
}

/// The parts of a camera that make a look: tone map, sky light,
/// multisampling or ambient occlusion.
pub(crate) fn camera_look(
    camera: &mut EntityCommands,
    look: Look,
    light: &plantlab_scene::Light,
    images: &mut Assets<Image>,
) {
    let sky = Color::linear_rgb(light.sky_color[0], light.sky_color[1], light.sky_color[2]);
    match look {
        Look::Review => {
            camera.remove::<(EnvironmentMapLight, ScreenSpaceAmbientOcclusion)>();
            camera.insert((
                Tonemapping::None,
                AmbientLight {
                    color: sky,
                    brightness: light.sky_brightness,
                    affects_lightmapped_meshes: true,
                },
                Msaa::Sample4,
            ));
        }
        Look::Photo => {
            // Sky light from every direction: the sky's colour above, a
            // paler horizon, and light bounced off the ground below.
            let [gr, gg, gb] = plantlab_scene::GROUND;
            let mut environment = EnvironmentMapLight::hemispherical_gradient(
                images,
                sky,
                Color::linear_rgb(0.85, 0.88, 0.92),
                Color::linear_rgb(gr * 2.0, gg * 2.0, gb * 2.0),
            );
            environment.intensity = light.sky_brightness;
            camera.remove::<AmbientLight>();
            camera.insert((
                // Filmic, and needs no lookup table.
                Tonemapping::AcesFitted,
                Exposure {
                    ev100: ev100 - plantlab_scene::PHOTO_EXPOSURE_BOOST_EV,
                },
                environment,
                // Ambient occlusion reads the depth and normal prepasses,
                // which take no multisampling: supersampling smooths edges.
                Msaa::Off,
                ScreenSpaceAmbientOcclusion::default(),
            ));
        }
    }
}

/// Spawn a scene's plant, its scale and its ground at `offset` on `layer`.
pub(crate) fn spawn_plant(
    commands: &mut Commands,
    scene: &Scene,
    (images, meshes, standard, cards): (
        &mut Assets<Image>,
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &mut Assets<CardMaterial>,
    ),
    offset: Vec3,
    layer: &RenderLayers,
) -> Vec<Entity> {
    let mut entities = Vec::new();
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
                Transform::from_translation(offset)
                    .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                layer.clone(),
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
                    Transform::from_translation(offset),
                    layer.clone(),
                ))
                .id(),
        );
    }
    if !scene.cut_cards.is_empty() {
        // Cards cut into triangles: two-sided solids in their colours.
        let cut = standard.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            reflectance: 0.3,
            double_sided: true,
            cull_mode: None,
            diffuse_transmission: 0.35,
            ..default()
        });
        entities.push(
            commands
                .spawn((
                    Mesh3d(meshes.add(bevy_mesh(&scene.cut_cards))),
                    MeshMaterial3d(cut),
                    Transform::from_translation(offset),
                    layer.clone(),
                ))
                .id(),
        );
    } else if !scene.cards.is_empty() {
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
                    Transform::from_translation(offset),
                    layer.clone(),
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
        let mut shot = plantlab_scene::Shot::thumbnail("polystichum-munitum");
        shot.quality = plantgen::quality::DRAFT;
        let jobs = vec![Job {
            name: "fern".into(),
            sheet: plantlab_scene::single(shot, 128, 128),
            key: "test".into(),
        }];
        run(jobs, out.clone(), None).expect("renders");
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
