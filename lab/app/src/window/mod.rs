//! PlantLab's window: one plant on its ground patch, a camera that orbits
//! it, and around it the Studio ([`shell`]): a top bar, a rail of panels
//! that dock beside the view, a status bar, the command palette and the
//! settings, or the Overlay, where the plant fills the window.
//!
//! The interface only queues commands ([`command`]); a change of what
//! grows regrows the plant on a worker thread
//! ([`plantlab_scene::build_watched`], PlantGen's own growth). A growth
//! can be started, paused and stopped, shows its progress, and can show
//! the plant growing: every N steps the worker draws the plant as it
//! stands and the window puts it on screen. Otherwise the old plant stays
//! until the new one is ready. Nothing about the plant is computed here.
//!
//! Around it: a measuring grid and a height ruler ([`crate::measure`]),
//! how big the plant would be as a file, and snapshots and growth
//! animations ([`crate::record`]).

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::camera::CameraUpdateSystems;
use bevy::camera::Exposure;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::{DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPostUpdateSet, EguiPrimaryContextPass,
    PrimaryEguiContext,
};
use plantgen::library::Library;
use plantgen::quality;
use plantlab_scene::{Look, Progress, Scene, Shot, View, Viewer};

use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::measure::{self, MeasureGizmos};
use crate::record::{self, PLANT_LAYER, Recorder, Recording, Take, WINDOW_LAYER};
use crate::render::{
    PlantAssets, add_materials, camera_look, exposure, spawn_plant, spawn_sun, to_rgba8,
};
use plantlab_ui::theme;

mod command;
mod panels;
mod readings;
mod shell;

use crate::settings::{Key, Settings};
use command::{Command, Panel, Shell};

/// How `plantlab open` starts the window.
#[allow(clippy::struct_excessive_bools, reason = "the command's switches")]
pub struct Options {
    pub species: Option<String>,
    /// Grow the first plant to this age, years, instead of its oldest
    /// keyframe.
    pub age: Option<f64>,
    /// Show the plant growing every this many steps; 0 for none. `None`
    /// and the other `Option`s here: as the settings file says.
    pub live: Option<u32>,
    pub grid: Option<bool>,
    pub ruler: Option<bool>,
    /// Where snapshots and animations go.
    pub out: std::path::PathBuf,
    /// Start recording growth, and of the plant alone.
    pub record: bool,
    pub plant_only: Option<bool>,
    /// The window's size, logical pixels.
    pub size: (u32, u32),
    /// The panels open at the start, left to right.
    pub panels: Vec<String>,
    /// Start in the Overlay.
    pub overlay: bool,
    /// The settings file; `None` for the default place.
    pub settings: Option<std::path::PathBuf>,
    /// Save a picture of the window and close it: once the plant has
    /// grown, or after this many seconds.
    pub capture: Option<(std::path::PathBuf, Option<f64>)>,
}

/// What the panel chose, and what is on screen.
#[derive(Resource)]
#[allow(clippy::struct_excessive_bools, reason = "the panel's switches")]
struct Lab {
    /// The 3D view's width over its height, so framing fits the plant
    /// into the view the docks leave.
    aspect: f64,
    species: Vec<String>,
    /// Index into `species`.
    chosen: usize,
    /// Plant age, years; `None` until the species' keyframes are known.
    age: Option<f64>,
    /// The age `--age` asked for, used once.
    asked_age: Option<f64>,
    oldest: f64,
    day: f64,
    level: usize,
    draft: bool,
    look: Look,
    view: View,
    /// The settings changed since the last growth started.
    dirty: bool,
    /// Grow again whenever the settings change; otherwise only on "Grow".
    auto: bool,
    /// "Grow" was pressed.
    go: bool,
    /// Show the plant growing, drawn every `every` steps.
    live: bool,
    every: u32,
    /// Move the camera with the plant as it grows.
    follow: bool,
    /// The worker growing the plant: its own thread, since Bevy compiles
    /// shaders on its async task pool and a long growth there starves it.
    growing: Option<std::thread::JoinHandle<Grown>>,
    /// Shared with the growing worker.
    control: Arc<Control>,
    started: Instant,
    /// Growths started, and plants (or live frames) put on screen: the
    /// readings start a growth's figures and a plant's levels afresh
    /// when these change.
    growths: u64,
    shows: u64,
    shown: Option<Shown>,
    message: String,
    /// The camera should frame the next plant shown.
    reframe: bool,
    /// Frame the plant on screen again now ("Reset camera").
    reset_camera: bool,
    /// The measures on the ground and beside the plant.
    grid: bool,
    ruler: bool,
}

struct Grown {
    result: Result<Scene, String>,
    shot: Shot,
}

/// What the window and a growing worker share: the buttons one way, the
/// progress and the latest live frame the other.
#[derive(Default)]
struct Control {
    stop: AtomicBool,
    pause: AtomicBool,
    /// Draw a live frame every this many steps; 0 for none.
    every: AtomicU32,
    step: AtomicU32,
    steps: AtomicU32,
    /// The age at `step`, years, as `f64` bits.
    age: AtomicU64,
    frame: Mutex<Option<Scene>>,
    /// Time each step while the Readings panel shows.
    gather: AtomicBool,
    /// Each step's time since the last, gathered while asked.
    step_times: Mutex<Vec<readings::StepTime>>,
}

/// The worker's side of [`Control`], and when its last step ended.
struct Watcher(Arc<Control>, Instant);

impl Viewer for Watcher {
    fn wants_frame(&mut self, step: u32) -> bool {
        let every = self.0.every.load(Ordering::Relaxed);
        every > 0 && step.is_multiple_of(every) && !self.0.stop.load(Ordering::Relaxed)
    }

    fn step(&mut self, progress: Progress<'_>, frame: Option<Scene>) -> bool {
        self.0.step.store(progress.step, Ordering::Relaxed);
        self.0.steps.store(progress.steps, Ordering::Relaxed);
        self.0.age.store(progress.age.to_bits(), Ordering::Relaxed);
        if self.0.gather.load(Ordering::Relaxed)
            && let Ok(mut times) = self.0.step_times.lock()
        {
            #[allow(clippy::cast_possible_truncation)]
            times.push(readings::StepTime {
                ms: (self.1.elapsed().as_secs_f64() * 1_000.0) as f32,
                framed: frame.is_some(),
            });
        }
        if let Some(frame) = frame
            && let Ok(mut slot) = self.0.frame.lock()
        {
            *slot = Some(frame);
        }
        while self.0.pause.load(Ordering::Relaxed) && !self.0.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(30));
        }
        // A pause is not the next step's time.
        self.1 = Instant::now();
        !self.0.stop.load(Ordering::Relaxed)
    }
}

/// The plant on screen.
struct Shown {
    scene: Scene,
    shot: Shot,
    entities: Vec<Entity>,
    seconds: f64,
    /// A live frame of a plant still growing.
    live: bool,
}

/// The orbiting camera: around `target` moved by `pan`, at `distance`,
/// turned by `yaw` and raised by `pitch` (radians). Following a growing
/// plant moves `target`; panning moves `pan`, which stays.
#[derive(Component)]
struct Orbit {
    target: Vec3,
    pan: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

impl Orbit {
    fn transform(&self) -> Transform {
        let direction = Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        );
        let centre = self.target + self.pan;
        Transform::from_translation(centre + direction * self.distance).looking_at(centre, Vec3::Y)
    }

    /// Look at a scene as its framing does, unpanned.
    fn frame(&mut self, framing: &plantlab_scene::Framing) {
        let eye = Vec3::from_array(framing.eye);
        let target = Vec3::from_array(framing.target);
        let offset = eye - target;
        self.target = target;
        self.pan = Vec3::ZERO;
        self.distance = offset.length().max(0.5);
        self.yaw = offset.x.atan2(offset.z);
        self.pitch = (offset.y / self.distance).clamp(-1.0, 1.0).asin();
    }

    /// Move the centre by a drag of `delta` pixels in a view `pixels` tall
    /// with vertical field of view `fov`, so the plant follows the pointer.
    fn drag(&mut self, delta: Vec2, pixels: f32, fov: f32) {
        let metres = 2.0 * self.distance * (fov * 0.5).tan() / pixels.max(1.0);
        let rotation = self.transform().rotation;
        self.pan +=
            (rotation * Vec3::NEG_X) * delta.x * metres + (rotation * Vec3::Y) * delta.y * metres;
    }
}

/// With `--capture FILE`: a picture of the window, taken once the first
/// plant has stood for a few frames, then the window closes. A smoke test
/// of the window, and a way to show it.
#[derive(Resource)]
struct Capture {
    path: std::path::PathBuf,
    /// Take it this many seconds after opening, grown or not.
    after: Option<f64>,
    opened: Instant,
    frames: u32,
    asked: bool,
}

/// The sun of the plant on screen.
#[derive(Component)]
struct Sun;

/// Open the window as `options` say.
#[allow(clippy::too_many_lines)]
///
/// # Errors
///
/// When the species is not in the library.
pub fn run(options: Options) -> Result<(), String> {
    let Options {
        species,
        age,
        live,
        grid,
        ruler,
        out,
        record,
        plant_only,
        capture: picture,
        size,
        panels,
        overlay,
        settings,
    } = options;
    let library = Library::builtin();
    let ids: Vec<String> = library
        .species()
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    let chosen = match species.as_deref() {
        Some(id) => ids
            .iter()
            .position(|known| known == id)
            .ok_or_else(|| format!("`{id}` is not in the library"))?,
        None => ids
            .iter()
            .position(|id| id == "acer-macrophyllum")
            .unwrap_or(0),
    };
    let mut open = Vec::new();
    for name in &panels {
        let panel = Panel::ALL
            .into_iter()
            .find(|panel| panel.short().eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                let names: Vec<&str> = Panel::ALL.iter().map(|panel| panel.short()).collect();
                format!("no panel `{name}`; the panels are {}", names.join(", "))
            })?;
        open.push(panel);
    }
    let settings = Settings::load(settings.or_else(Settings::default_path));
    if !settings.message.is_empty() {
        eprintln!("{}", settings.message);
    }
    // The flags, else the settings file, else the defaults.
    let live = live.unwrap_or_else(|| {
        if settings.bool(Key::ShowGrowing).unwrap_or(true) {
            settings.u32(Key::Every).unwrap_or(10)
        } else {
            0
        }
    });
    let every = if live > 0 {
        live
    } else {
        settings.u32(Key::Every).unwrap_or(10)
    };
    let mut recording = Recording::new(out);
    recording.record = record;
    recording.plant_only = plant_only
        .or(settings.bool(Key::PlantOnly))
        .unwrap_or(false);
    if let Some(width) = settings.u32(Key::PictureWidth) {
        recording.width = width;
    }
    if let Some(ms) = settings.u32(Key::FrameMs) {
        recording.delay = u16::try_from(ms / 10).unwrap_or(u16::MAX).max(2);
    }
    if let Some(seconds) = settings.f64(Key::HoldS) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let hundredths = (seconds * 100.0).round() as u16;
        recording.hold = hundredths;
    }
    if let Some(pngs) = settings.bool(Key::Pngs) {
        recording.pngs = pngs;
    }
    let lab = Lab {
        aspect: 1.0,
        growths: 0,
        shows: 0,
        species: ids,
        chosen,
        age: None,
        asked_age: age,
        oldest: 1.0,
        day: plantgen::package::DEFAULT_DAY,
        level: 0,
        draft: true,
        look: Look::Review,
        view: View::ThreeQuarter,
        dirty: true,
        auto: settings.bool(Key::GrowOnChange).unwrap_or(true),
        go: false,
        live: live > 0,
        every: every.clamp(1, 100),
        follow: settings.bool(Key::Follow).unwrap_or(true),
        growing: None,
        control: Arc::default(),
        started: Instant::now(),
        shown: None,
        message: String::new(),
        reframe: true,
        reset_camera: false,
        grid: grid.or(settings.bool(Key::Grid)).unwrap_or(true),
        ruler: ruler.or(settings.bool(Key::Ruler)).unwrap_or(true),
    };
    let mut shell = Shell::new(settings);
    if !open.is_empty() {
        shell.open = open;
    }
    if overlay {
        shell.overlay = true;
        shell.card = shell.open.first().copied();
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "PlantLab".into(),
                    resolution: size.into(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                filter: "wgpu=error,bevy_ecs::world::command_queue=error".into(),
                ..default()
            }),
    );
    add_materials(&mut app);
    app.add_plugins(EguiPlugin::default())
        .init_gizmo_group::<MeasureGizmos>()
        .insert_resource(recording)
        .insert_resource(record::Background(backdrop()))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(GlobalAmbientLight::NONE)
        .insert_resource(lab)
        .insert_resource(shell)
        .init_resource::<readings::Readings>()
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, shell::draw)
        .add_systems(
            PostUpdate,
            shell::fit_view
                .after(EguiPostUpdateSet::EndPass)
                .before(CameraUpdateSystems),
        )
        .add_systems(
            Update,
            (
                keys,
                command::apply,
                grow,
                live_frame,
                show,
                orbit,
                measures,
                record::follow,
                record::take,
                record::keep,
                readings::gather,
            )
                .chain(),
        );
    if let Some((path, after)) = picture {
        app.insert_resource(Capture {
            path,
            after,
            opened: Instant::now(),
            frames: 0,
            asked: false,
        })
        .add_systems(Update, capture.after(show));
    }
    app.run();
    Ok(())
}

/// The window's backdrop behind the ground: the deepest surface of the
/// interface. Headless pictures keep the review grey
/// ([`plantlab_scene::BACKGROUND`]), so species compare fairly.
fn backdrop() -> Color {
    let [r, g, b, _] = theme::SURFACE_0.to_array();
    Color::srgb_u8(r, g, b)
}

fn capture(mut commands: Commands, lab: Res<Lab>, mut capture: ResMut<Capture>) {
    if capture.asked || lab.shown.is_none() {
        return;
    }
    match capture.after {
        // The window as it is then, the plant perhaps still growing.
        Some(after) if capture.opened.elapsed().as_secs_f64() < after => return,
        Some(_) => capture.frames = 30,
        // The grown plant, not a live frame of it.
        None if lab.growing.is_some() => return,
        None => {}
    }
    capture.frames += 1;
    if capture.frames < 30 {
        return;
    }
    capture.asked = true;
    let path = capture.path.clone();
    commands.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let image = &event.image;
            if let Some(data) = image.data.as_ref() {
                let rgba = to_rgba8(data, image.texture_descriptor.format);
                match plantgen::raster::encode_png(
                    image.width() as usize,
                    image.height() as usize,
                    &rgba,
                ) {
                    Ok(png) => match std::fs::write(&path, png) {
                        Ok(()) => println!("wrote {}", path.display()),
                        Err(error) => eprintln!("cannot write {}: {error}", path.display()),
                    },
                    Err(error) => eprintln!("cannot encode PNG: {error}"),
                }
            }
            exit.write(AppExit::Success);
        },
    );
}

/// The interface camera's layer: nothing in the scene is on it.
const INTERFACE_LAYER: usize = 3;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut gizmos: ResMut<GizmoConfigStore>,
    mut egui: ResMut<EguiGlobalSettings>,
) {
    // The interface has its own camera over the whole window, drawn after
    // the plant's, whose viewport is the view the docks leave; egui must not
    // give the recorder a second primary context.
    egui.auto_create_primary_context = false;
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(INTERFACE_LAYER),
        PrimaryEguiContext,
    ));
    measure::configure(&mut gizmos, WINDOW_LAYER);
    let light = plantlab_scene::review_light();
    #[allow(clippy::cast_possible_truncation)]
    let fov = plantlab_scene::FOV_Y_DEG.to_radians() as f32;
    let orbit = Orbit {
        pan: Vec3::ZERO,
        target: Vec3::new(0.0, 5.0, 0.0),
        distance: 30.0,
        yaw: std::f32::consts::FRAC_PI_4,
        pitch: 0.3,
    };
    let mut camera = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(backdrop()),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection { fov, ..default() }),
        Exposure {
            ev100: exposure(&light),
        },
        RenderLayers::layer(WINDOW_LAYER),
        orbit.transform(),
        orbit,
    ));
    camera_look(&mut camera, Look::Review, &light, &mut images);
    let recorder = record::spawn(&mut commands, &mut images, fov);
    camera_look(
        &mut commands.entity(recorder),
        Look::Review,
        &light,
        &mut images,
    );
}

/// The species' keyframe ages, oldest last.
fn keyframes(id: &str) -> Vec<f64> {
    let mut ages = Library::builtin()
        .spec(id)
        .map(|spec| spec.growth.keyframes.clone())
        .unwrap_or_default();
    ages.sort_by(f64::total_cmp);
    ages.dedup();
    ages
}

/// The shot the panel's settings describe.
fn shot(lab: &Lab) -> Shot {
    let mut shot = Shot::thumbnail(&lab.species[lab.chosen]);
    shot.age = lab.age;
    shot.day = lab.day;
    shot.level = lab.level;
    shot.quality = if lab.draft {
        quality::DRAFT
    } else {
        quality::STANDARD
    };
    shot.look = lab.look;
    shot.view = lab.view;
    shot.aspect = lab.aspect;
    shot
}

/// The command that renders the shot on screen without the window.
fn thumbs_command(shot: &Shot) -> String {
    let mut text = format!("plantlab thumbs {} --out pictures", shot.species);
    if let Some(age) = shot.age {
        let _ = write!(text, " --age {age}");
    }
    let _ = write!(
        text,
        " --day {} --view {} --quality {} --look {}",
        shot.day,
        shot.view.name(),
        shot.quality.name,
        shot.look.name()
    );
    text
}

/// Start growing on "Grow", or when the settings changed and growing
/// follows them. A change while growing stops the growth under way first.
fn grow(mut lab: ResMut<Lab>) {
    let wanted = lab.go || (lab.auto && lab.dirty);
    if !wanted {
        return;
    }
    if lab.growing.is_some() {
        if lab.dirty && lab.auto {
            lab.control.stop.store(true, Ordering::Relaxed);
        }
        lab.go = false;
        return;
    }
    lab.dirty = false;
    lab.go = false;
    if lab.age.is_none() {
        let ages = keyframes(&lab.species[lab.chosen]);
        let oldest = ages.last().copied().unwrap_or(1.0);
        let age = lab.asked_age.take().unwrap_or(oldest);
        lab.oldest = oldest.max(age);
        lab.age = Some(age);
    }
    let shot = shot(&lab);
    lab.started = Instant::now();
    lab.growths += 1;
    lab.message.clear();
    let control = Arc::new(Control::default());
    control
        .every
        .store(if lab.live { lab.every } else { 0 }, Ordering::Relaxed);
    lab.control = Arc::clone(&control);
    lab.growing = Some(std::thread::spawn(move || {
        let mut viewer = Watcher(control, Instant::now());
        Grown {
            result: plantlab_scene::build_watched(&shot, Library::builtin(), &mut viewer),
            shot,
        }
    }));
}

/// What putting a plant on screen touches.
#[derive(SystemParam)]
struct Stage<'w, 's> {
    commands: Commands<'w, 's>,
    assets: PlantAssets<'w>,
    camera: Query<'w, 's, (Entity, &'static mut Orbit)>,
    recorder: Query<'w, 's, Entity, With<Recorder>>,
    suns: Query<'w, 's, Entity, With<Sun>>,
    recording: ResMut<'w, Recording>,
}

/// Put the latest live frame on screen.
fn live_frame(mut lab: ResMut<Lab>, mut stage: Stage) {
    if lab.growing.is_none() {
        return;
    }
    let frame = lab
        .control
        .frame
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    let Some(scene) = frame else {
        return;
    };
    let shot = shot(&lab);
    put_on_screen(&mut stage, &mut lab, scene, shot, true);
}

/// Put a newly grown plant on screen in place of the old one.
fn show(mut lab: ResMut<Lab>, mut stage: Stage) {
    if !lab
        .growing
        .as_ref()
        .is_some_and(std::thread::JoinHandle::is_finished)
    {
        return;
    }
    let Some(Ok(grown)) = lab.growing.take().map(std::thread::JoinHandle::join) else {
        lab.message = "the growing thread panicked".into();
        return;
    };
    let scene = match grown.result {
        Ok(scene) => scene,
        Err(error) => {
            if lab.control.stop.load(Ordering::Relaxed) {
                // Stopped: by the button, or for new settings.
                if !lab.dirty {
                    let step = lab.control.step.load(Ordering::Relaxed);
                    let steps = lab.control.steps.load(Ordering::Relaxed);
                    let age = f64::from_bits(lab.control.age.load(Ordering::Relaxed));
                    lab.message = format!("Stopped at step {step} of {steps}, {age:.1} years.");
                }
            } else {
                lab.message = error;
            }
            return;
        }
    };
    lab.message.clear();
    put_on_screen(&mut stage, &mut lab, scene, grown.shot, false);
}

/// Replace the plant on screen with `scene`: a grown plant, or a `live`
/// frame of one still growing. The plant goes on the window's layer and
/// the plant-only layer, its ground and scale on the window's alone.
fn put_on_screen(stage: &mut Stage, lab: &mut Lab, scene: Scene, shot: Shot, live: bool) {
    let commands = &mut stage.commands;
    if let Some(old) = lab.shown.take() {
        for entity in old.entities {
            commands.entity(entity).despawn();
        }
    }
    for sun in &stage.suns {
        commands.entity(sun).despawn();
    }
    let sun = spawn_sun(commands, std::slice::from_ref(&scene));
    commands.entity(sun).insert((
        Sun,
        RenderLayers::from_layers(&[0, WINDOW_LAYER, PLANT_LAYER]),
    ));
    let entities = spawn_plant(
        commands,
        &scene,
        &mut stage.assets,
        Vec3::ZERO,
        (
            &RenderLayers::from_layers(&[WINDOW_LAYER, PLANT_LAYER]),
            &RenderLayers::layer(WINDOW_LAYER),
        ),
    );
    for recorder in &stage.recorder {
        camera_look(
            &mut commands.entity(recorder),
            scene.look,
            &scene.light,
            &mut stage.assets.images,
        );
    }
    if let Ok((entity, mut orbit)) = stage.camera.single_mut() {
        camera_look(
            &mut commands.entity(entity),
            scene.look,
            &scene.light,
            &mut stage.assets.images,
        );
        if lab.reframe {
            orbit.frame(&scene.framing);
            lab.reframe = false;
        } else if lab.follow {
            // Keep the way the camera turns and any pan; frame the plant's
            // new size.
            let eye = Vec3::from_array(scene.framing.eye);
            let target = Vec3::from_array(scene.framing.target);
            orbit.target = target;
            orbit.distance = (eye - target).length().max(0.5);
        }
    }
    if stage.recording.record {
        let name = picture_name(&scene);
        stage.recording.ask(Take::Frame, name);
    }
    lab.shows += 1;
    lab.shown = Some(Shown {
        scene,
        shot,
        entities,
        seconds: lab.started.elapsed().as_secs_f64(),
        live,
    });
}

/// Save the frames taken as a GIF named for the plant.
fn save_gif(recording: &mut Recording, scene: &Scene) {
    let name = record::stamped(&format!("{}-growth", scene.facts.species));
    recording.message = match recording.save_gif(&name) {
        Ok(path) => format!("Saved {}", path.display()),
        Err(error) => error,
    };
}

/// A picture's name: the species and its age.
fn picture_name(scene: &Scene) -> String {
    format!("{}-{:.1}y", scene.facts.species, scene.facts.age)
}

/// Drag to turn around the plant, middle-drag (or Shift and drag) to pan,
/// scroll to come closer.
#[allow(clippy::too_many_arguments)]
fn orbit(
    mut contexts: EguiContexts,
    mut lab: ResMut<Lab>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Orbit, &mut Transform, &Camera), Without<Recorder>>,
) {
    let over_panel = contexts
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui());
    let Ok((mut orbit, mut transform, view)) = camera.single_mut() else {
        return;
    };
    let view_height = view.logical_viewport_size().map(|size| size.y);
    if lab.reset_camera {
        lab.reset_camera = false;
        if let Some(shown) = &lab.shown {
            orbit.frame(&shown.scene.framing);
        }
    }
    if !over_panel {
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Left)) {
            let pixels =
                view_height.unwrap_or_else(|| windows.iter().next().map_or(900.0, Window::height));
            orbit.drag(motion.delta, pixels, fov());
        } else if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right) {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-0.2, 1.5);
        }
        if scroll.delta.y != 0.0 {
            orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.1)).clamp(0.05, 2000.0);
        }
    }
    *transform = orbit.transform();
}

/// The camera's vertical field of view, radians.
fn fov() -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let fov = plantlab_scene::FOV_Y_DEG.to_radians() as f32;
    fov
}

/// Keys, unless a text field has them, each as a command: Enter grows,
/// Space pauses or resumes, Esc stops (or leaves the Overlay), F frames
/// the plant again, G the grid, R the ruler, O the Overlay, P a snapshot,
/// S saves the GIF, 1 to 7 the panels (Shift beside the others), F1 the
/// keys, Ctrl+P the palette, Ctrl+, the settings.
fn keys(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    lab: Res<Lab>,
    recording: Res<Recording>,
    mut shell: ResMut<Shell>,
) {
    if contexts
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_keyboard_input())
    {
        return;
    }
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let growing = lab.growing.is_some();
    let (overlay, settings_open) = (shell.overlay, shell.settings_open);
    let mut send = |command| shell.queue.push(command);
    if control {
        if keys.just_pressed(KeyCode::KeyP) {
            send(Command::Palette(String::new()));
        }
        if keys.just_pressed(KeyCode::Comma) {
            send(Command::Settings(true));
        }
        return;
    }
    let pressed = |key| keys.just_pressed(key);
    if pressed(KeyCode::Enter) || pressed(KeyCode::NumpadEnter) {
        send(Command::Grow);
    }
    if pressed(KeyCode::Space) && growing {
        send(Command::Pause(!lab.control.pause.load(Ordering::Relaxed)));
    }
    if pressed(KeyCode::Escape) {
        if growing {
            send(Command::Stop);
        } else if overlay {
            send(Command::Overlay(false));
        } else if settings_open {
            send(Command::Settings(false));
        }
    }
    if pressed(KeyCode::KeyF) {
        send(Command::ResetCamera);
    }
    if pressed(KeyCode::KeyG) {
        send(Command::Grid(!lab.grid));
    }
    if pressed(KeyCode::KeyR) {
        send(Command::Ruler(!lab.ruler));
    }
    if pressed(KeyCode::KeyO) {
        send(Command::Overlay(!overlay));
    }
    if pressed(KeyCode::KeyP) {
        send(Command::Snapshot);
    }
    if pressed(KeyCode::KeyS) && !recording.frames.is_empty() {
        send(Command::SaveGif);
    }
    if pressed(KeyCode::F1) {
        send(Command::Panel(Panel::Keys, shift));
    }
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
    ];
    for (key, panel) in digits.into_iter().zip(Panel::RAIL) {
        if pressed(key) {
            send(Command::Panel(panel, shift));
        }
    }
}

/// Draw the grid and the ruler the panel turned on. The ruler stands at
/// the ground's edge, or nearer the plant where the edge is off screen.
fn measures(
    lab: Res<Lab>,
    mut gizmos: Gizmos<MeasureGizmos>,
    camera: Query<(&Transform, &Camera, &GlobalTransform), With<Orbit>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(shown) = &lab.shown else {
        return;
    };
    if !lab.grid && !lab.ruler {
        return;
    }
    let Ok((transform, camera, global)) = camera.single() else {
        return;
    };
    // The view the interface leaves: the camera's viewport.
    let screen = camera.logical_viewport_size().unwrap_or_else(|| {
        windows
            .iter()
            .next()
            .map_or(Vec2::new(1400.0, 900.0), |window| {
                Vec2::new(window.width(), window.height())
            })
    });
    let view = measure::View {
        eye: transform.translation,
        rotation: transform.rotation,
        fov_y: fov(),
        pixels: screen.y,
    };
    let radius = shown.scene.ground_radius;
    if lab.grid {
        measure::grid(&mut gizmos, &view, radius);
    }
    if lab.ruler {
        #[allow(clippy::cast_possible_truncation)]
        let (height, reach) = (
            shown.scene.facts.height_m as f32,
            (shown.scene.facts.crown_width_m * 0.5) as f32,
        );
        let top = measure::ruler_top(height);
        let side = measure::side(&view);
        let on_screen = |at: f32| {
            [side * at, side * at + Vec3::Y * top].iter().all(|point| {
                camera.world_to_viewport(global, *point).is_ok_and(|seen| {
                    seen.x > 30.0
                        && seen.x < screen.x - 80.0
                        && seen.y > 10.0
                        && seen.y < screen.y - 10.0
                })
            })
        };
        let nearest = (reach * 1.15).min(radius);
        let at = (0..=24)
            .map(|k| {
                #[allow(clippy::cast_precision_loss)]
                let share = k as f32 / 24.0;
                radius - (radius - nearest) * share
            })
            .find(|at| on_screen(*at))
            .unwrap_or(nearest);
        measure::ruler(&mut gizmos, &view, height, at);
    }
}

/// A file size, in the binary units `plantc build` reports.
fn bytes(count: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let count = count as f64;
    if count < 1024.0 * 1024.0 {
        format!("{:.0} KiB", count / 1024.0)
    } else if count < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MiB", count / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", count / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lab as the window starts it, with the library's species.
    pub(super) fn lab() -> Lab {
        Lab {
            aspect: 1.0,
            growths: 0,
            shows: 0,
            species: Library::builtin()
                .species()
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
            chosen: 0,
            age: Some(5.0),
            asked_age: None,
            oldest: 10.0,
            day: 196.0,
            level: 0,
            draft: true,
            look: Look::Review,
            view: View::ThreeQuarter,
            dirty: true,
            auto: true,
            go: false,
            live: true,
            every: 10,
            follow: true,
            growing: None,
            control: Arc::default(),
            started: Instant::now(),
            shown: None,
            message: String::new(),
            reframe: false,
            reset_camera: false,
            grid: true,
            ruler: true,
        }
    }

    #[test]
    fn the_command_renders_what_the_window_shows() {
        let mut shot = Shot::thumbnail("acer-macrophyllum");
        shot.age = Some(30.0);
        shot.view = View::Side;
        let text = thumbs_command(&shot);
        assert!(text.starts_with("plantlab thumbs acer-macrophyllum"));
        assert!(text.contains("--age 30") && text.contains("--view side"));
    }

    #[test]
    fn an_orbit_looks_at_its_target_from_its_distance() {
        let orbit = Orbit {
            pan: Vec3::ZERO,
            target: Vec3::new(1.0, 2.0, 3.0),
            distance: 10.0,
            yaw: 0.7,
            pitch: 0.4,
        };
        let transform = orbit.transform();
        assert!((transform.translation.distance(orbit.target) - 10.0).abs() < 1e-4);
        let forward = transform.forward();
        let toward = (orbit.target - transform.translation).normalize();
        assert!(forward.dot(toward) > 0.9999);
    }

    #[test]
    fn the_watcher_draws_every_n_steps_and_stops_on_the_button() {
        let control = Arc::new(Control::default());
        let mut watcher = Watcher(Arc::clone(&control), Instant::now());
        assert!(!watcher.wants_frame(0), "no live frames unless asked");
        control.every.store(5, Ordering::Relaxed);
        assert!(watcher.wants_frame(10) && !watcher.wants_frame(11));
        let progress = Progress {
            step: 10,
            steps: 40,
            age: 2.5,
            frame: None,
        };
        assert!(watcher.step(progress, None));
        assert_eq!(control.step.load(Ordering::Relaxed), 10);
        assert_eq!(control.age.load(Ordering::Relaxed), 2.5_f64.to_bits());
        control.stop.store(true, Ordering::Relaxed);
        assert!(!watcher.wants_frame(10), "no frames once stopped");
        assert!(!watcher.step(progress, None));
    }
}
