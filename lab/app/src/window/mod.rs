//! PlantLab's window: one plant on its ground patch, a camera that orbits
//! it, and around it the Studio ([`shell`]): a top bar, the library
//! browser at the left (every species by group, order, family and genus,
//! with search), the inspector at the right (the render's settings, the
//! readings, pictures, the spec and the bark), a status bar, the command
//! palette and the settings; or the Overlay, where the plant fills the
//! window.
//!
//! Choosing a species grows nothing: the view shows the species' last
//! render, kept on disk ([`store`]), and takes up the settings it was made
//! with. **Generate** grows the plant on a worker thread
//! ([`plantlab_scene::build_watched`], PlantGen's own growth), or, when
//! only the day, the detail or the level changed, draws the plant already
//! grown again ([`plantlab_scene::dress`]), which is much quicker. The
//! view and the look change what is on screen at once. A growth can be
//! paused and stopped, shows its progress, and can show the plant growing:
//! every N steps the worker draws the plant as it stands.
//!
//! The interface only queues commands ([`command`]). Nothing about the
//! plant is computed here. Around it: a measuring grid and a height ruler
//! ([`crate::measure`]), how big the plant would be as a file, and
//! snapshots and growth animations ([`crate::record`]).

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
use plantlab_scene::{Look, Progress, Scene, Shot, Track, View, Viewer};

use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::measure::{self, MeasureGizmos};
use crate::record::{self, PLANT_LAYER, Recorder, Recording, Take, WINDOW_LAYER};
use crate::render::{
    PlantAssets, add_materials, camera_look, exposure, spawn_plant, spawn_sun, to_rgba8,
};
use plantlab_ui::theme;

mod browser;
mod catalogue;
mod command;
mod panels;
mod readings;
mod shell;
mod store;

use crate::settings::{Key, Settings};
use catalogue::Catalogue;
use command::{Command, Panel, Shell};
use store::{Kept, Recipe, Store};

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
    /// The inspector's page at the start (`render`, `readings`, …).
    pub panels: Vec<String>,
    /// Start in the Overlay.
    pub overlay: bool,
    /// The settings file; `None` for the default place.
    pub settings: Option<std::path::PathBuf>,
    /// Where the last renders are kept; `None` for the default place.
    pub renders: Option<std::path::PathBuf>,
    /// Generate the first plant at once, as the window opens.
    pub generate: bool,
    /// Save a picture of the window and close it: once the plant has
    /// grown, or after this many seconds.
    pub capture: Option<(std::path::PathBuf, Option<f64>)>,
}

/// What the inspector chose, and what is on screen.
#[derive(Resource)]
#[allow(clippy::struct_excessive_bools, reason = "the window's switches")]
struct Lab {
    /// The 3D view's width over its height, so framing fits the plant
    /// into the view the docks leave.
    aspect: f64,
    catalogue: Catalogue,
    /// The species chosen, by id: what Generate grows.
    chosen: String,
    /// What the window knows of the chosen species.
    about: About,
    /// The settings the next render is made with.
    recipe: Recipe,
    /// "Generate" was pressed.
    go: bool,
    /// Show the plant growing, drawn every `every` steps.
    live: bool,
    every: u32,
    /// Move the camera with the plant as it grows.
    follow: bool,
    /// The worker growing or drawing the plant: its own thread, since Bevy
    /// compiles shaders on its async task pool and a long growth there
    /// starves it.
    growing: Option<Job>,
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
    /// Frame the plant on screen again now ("Frame the plant again").
    reset_camera: bool,
    /// Look at the plant on screen from the recipe's view now.
    refit: bool,
    /// The camera framed a live frame: fit the grown plant when it comes.
    settle: bool,
    /// Show the plant on screen in the recipe's look now.
    restyle: bool,
    /// The measures on the ground and beside the plant.
    grid: bool,
    ruler: bool,
    /// The last renders, kept on disk.
    store: Store,
    /// Playing the year: the month shown, while it plays.
    year: Option<usize>,
    /// Species chosen lately, newest first.
    recent: Vec<String>,
}

/// What the window knows of a species before it grows: its keyframes,
/// seeds, typical site and seasons, from its spec.
#[derive(Debug, Clone, Default)]
struct About {
    /// Keyframe ages, oldest last.
    keyframes: Vec<f64>,
    oldest: f64,
    /// The spec's seeds, the first the typical plant's.
    seeds: Vec<u64>,
    /// Its typical site, as a word.
    typical: String,
    /// Each seasonal organ's year.
    year: Vec<Track>,
    /// Why the spec did not load; empty when it did.
    error: String,
}

impl About {
    fn of(id: &str) -> Self {
        let library = Library::builtin();
        match library.spec(id) {
            Ok(spec) => {
                let mut keyframes = spec.growth.keyframes.clone();
                keyframes.sort_by(f64::total_cmp);
                keyframes.dedup();
                let typical = plantgen::drawing::typical_environment(&spec, library)
                    .name()
                    .to_owned();
                Self {
                    oldest: keyframes.last().copied().unwrap_or(1.0),
                    keyframes,
                    seeds: spec.variants.seeds.clone(),
                    typical,
                    year: plantlab_scene::year(&spec),
                    error: String::new(),
                }
            }
            Err(error) => Self {
                oldest: 1.0,
                error: error.to_string(),
                ..Self::default()
            },
        }
    }
}

/// A plant being grown, or drawn again from one already grown.
struct Job {
    handle: std::thread::JoinHandle<Grown>,
    /// Drawn again from the plant on screen, not grown.
    redraw: bool,
}

struct Grown {
    result: Result<Scene, String>,
    shot: Shot,
    recipe: Recipe,
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
    /// The settings it was made with.
    recipe: Recipe,
    entities: Vec<Entity>,
    seconds: f64,
    /// A live frame of a plant still growing.
    live: bool,
    /// When it went on screen.
    at: Instant,
}

impl Lab {
    /// Whether the plant on screen is the chosen species'.
    fn shows_chosen(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|shown| shown.scene.facts.species == self.chosen)
    }

    /// The settings changed since the plant on screen was made, when it is
    /// the chosen species'.
    fn changes(&self) -> Vec<store::Setting> {
        match &self.shown {
            Some(shown) if self.shows_chosen() && !shown.live => self.recipe.changes(&shown.recipe),
            _ => Vec::new(),
        }
    }

    /// Whether Generate would only draw the plant on screen again: the
    /// chosen species, grown as the recipe says, and kept.
    fn redraws(&self) -> bool {
        self.shows_chosen()
            && self.shown.as_ref().is_some_and(|shown| {
                !shown.live
                    && shown.scene.growth.is_some()
                    && !shown
                        .recipe
                        .changes(&self.recipe)
                        .iter()
                        .any(|setting| setting.regrows())
            })
    }

    /// The age the recipe grows: its own, else the oldest keyframe.
    fn age(&self) -> f64 {
        self.recipe.age.unwrap_or(self.about.oldest)
    }

    /// Choose `id`: what the window shows of it, and the settings of its
    /// last render, or the current ones grown at its oldest keyframe on
    /// its typical site.
    fn choose(&mut self, id: &str) {
        if self.chosen == id && !self.about.keyframes.is_empty() {
            return;
        }
        id.clone_into(&mut self.chosen);
        self.about = About::of(id);
        self.recipe = match self.store.kept.get(id) {
            Some(kept) => kept.recipe.clone(),
            None => Recipe {
                age: None,
                environment: None,
                seed: None,
                ..self.recipe.clone()
            },
        };
        if let Some(age) = self.recipe.age {
            self.about.oldest = self.about.oldest.max(age);
        }
        self.recent.retain(|known| known != id);
        self.recent.insert(0, id.to_owned());
        self.recent.truncate(RECENT);
        self.reframe = true;
    }
}

/// Species kept in the recent list.
const RECENT: usize = 8;

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
        if let Some(distance) = top_distance(framing) {
            self.distance = distance;
            self.yaw = 0.0;
            self.pitch = TOP_PITCH;
            return;
        }
        self.distance = offset.length().max(0.5);
        self.yaw = offset.x.atan2(offset.z);
        self.pitch = (offset.y / self.distance).clamp(-1.0, 1.0).asin();
    }

    /// Look at a scene's framing from where the camera turns now: its
    /// target and distance, keeping the turn and any pan.
    fn fit(&mut self, framing: &plantlab_scene::Framing) {
        let eye = Vec3::from_array(framing.eye);
        let target = Vec3::from_array(framing.target);
        self.target = target;
        self.distance = top_distance(framing).unwrap_or_else(|| (eye - target).length().max(0.5));
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
        renders,
        generate,
    } = options;
    let library = Library::builtin();
    let catalogue = Catalogue::of(library);
    let mut tab = None;
    for name in &panels {
        let panel = Panel::ALL
            .into_iter()
            .find(|panel| panel.short().eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                let names: Vec<&str> = Panel::ALL.iter().map(|panel| panel.short()).collect();
                format!("no page `{name}`; the pages are {}", names.join(", "))
            })?;
        tab = Some(panel);
    }
    let settings = Settings::load(settings.or_else(Settings::default_path));
    if !settings.message.is_empty() {
        eprintln!("{}", settings.message);
    }
    let known = |id: &str| catalogue.get(id).is_some();
    let chosen = match species.as_deref() {
        Some(id) => {
            let id = library.current(id).unwrap_or(id);
            if !known(id) {
                return Err(format!("`{id}` is not in the library"));
            }
            id.to_owned()
        }
        None => settings
            .string(Key::Species)
            .filter(|id| known(id))
            .map_or_else(|| "acer-macrophyllum".to_owned(), str::to_owned),
    };
    let recent: Vec<String> = settings
        .strings(Key::Recent)
        .into_iter()
        .filter(|id| known(id))
        .take(RECENT)
        .collect();
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
    let store = Store::load(renders.or_else(Store::default_folder));
    let mut recording = Recording::new(out);
    recording.keep_folder.clone_from(&store.folder);
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
    let mut lab = Lab {
        aspect: 1.0,
        catalogue,
        chosen: String::new(),
        about: About::default(),
        recipe: Recipe::default(),
        go: generate,
        live: live > 0,
        every: every.clamp(1, 100),
        follow: settings.bool(Key::Follow).unwrap_or(true),
        growing: None,
        control: Arc::default(),
        started: Instant::now(),
        growths: 0,
        shows: 0,
        shown: None,
        message: String::new(),
        reframe: true,
        reset_camera: false,
        refit: false,
        settle: false,
        restyle: false,
        grid: grid.or(settings.bool(Key::Grid)).unwrap_or(true),
        ruler: ruler.or(settings.bool(Key::Ruler)).unwrap_or(true),
        store,
        year: None,
        recent,
    };
    lab.choose(&chosen);
    if let Some(age) = age {
        lab.recipe.age = Some(age);
        lab.about.oldest = lab.about.oldest.max(age);
    }
    let mut shell = Shell::new(settings);
    if let Some(tab) = tab {
        shell.tab = tab;
        shell.inspector = true;
    }
    shell.reveal = true;
    shell.overlay = overlay;
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
                play_year,
                grow,
                live_frame,
                show,
                orbit,
                measures,
                record::follow,
                record::take,
                record::keep,
                kept,
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

fn capture(
    mut commands: Commands,
    lab: Res<Lab>,
    recording: Res<Recording>,
    mut capture: ResMut<Capture>,
) {
    if capture.asked {
        return;
    }
    match capture.after {
        // The window as it is then, the plant perhaps still growing.
        Some(after) if capture.opened.elapsed().as_secs_f64() < after => return,
        Some(_) => capture.frames = 30,
        // The grown plant, not a live frame of it, once its kept picture
        // is written; or the window as it opens when nothing grows.
        None if lab.growing.is_some() || lab.go || recording.working() => return,
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

/// The shot the recipe describes for the chosen species.
fn shot(lab: &Lab) -> Shot {
    let recipe = &lab.recipe;
    let mut shot = Shot::thumbnail(&lab.chosen);
    shot.age = Some(lab.age());
    shot.day = recipe.day;
    shot.quality = recipe.detail.quality();
    shot.parts = recipe.detail.parts();
    shot.level = if shot.parts { 0 } else { recipe.level };
    shot.environment = recipe.environment;
    shot.seed = recipe.seed;
    shot.look = recipe.look;
    shot.view = recipe.view;
    shot.aspect = lab.aspect;
    shot
}

/// The command that renders the shot on screen without the window.
fn thumbs_command(shot: &Shot) -> String {
    let mut text = format!("plantlab thumbs {} --out pictures", shot.species);
    if let Some(age) = shot.age {
        let _ = write!(text, " --age {age}");
    }
    if let Some(environment) = shot.environment {
        let _ = write!(text, " --env {}", environment.name());
    }
    if let Some(seed) = shot.seed {
        let _ = write!(text, " --seed {seed}");
    }
    let quality = if shot.parts {
        "full"
    } else {
        shot.quality.name
    };
    let _ = write!(
        text,
        " --day {} --view {} --quality {quality} --look {}",
        shot.day,
        shot.view.name(),
        shot.look.name()
    );
    text
}

/// Step through the year while it plays: once the month's plant has
/// stood a moment, draw the next month's.
fn play_year(mut lab: ResMut<Lab>) {
    let Some(month) = lab.year else {
        return;
    };
    if lab.growing.is_some() || lab.go {
        return;
    }
    let ready = lab.shows_chosen()
        && lab.shown.as_ref().is_some_and(|shown| {
            (shown.recipe.day - MONTH_DAYS[month]).abs() < 0.5
                && shown.at.elapsed().as_secs_f64() >= YEAR_HOLD_S
        });
    if !ready {
        // The first month: draw it.
        if lab
            .shown
            .as_ref()
            .is_none_or(|shown| (shown.recipe.day - MONTH_DAYS[month]).abs() >= 0.5)
        {
            lab.recipe.day = MONTH_DAYS[month];
            lab.go = true;
        }
        return;
    }
    if month + 1 >= MONTH_DAYS.len() {
        lab.year = None;
        return;
    }
    lab.year = Some(month + 1);
    lab.recipe.day = MONTH_DAYS[month + 1];
    lab.go = true;
}

/// The middle of each month, as days of the year.
pub const MONTH_DAYS: [f64; 12] = [
    15.0, 46.0, 74.0, 105.0, 135.0, 166.0, 196.0, 227.0, 258.0, 288.0, 319.0, 349.0,
];

/// How long each month stands while the year plays, seconds.
const YEAR_HOLD_S: f64 = 0.8;

/// Start on "Generate": grow the plant, or draw the one on screen again
/// when only how it is drawn changed.
fn grow(mut lab: ResMut<Lab>) {
    if !lab.go || lab.growing.is_some() {
        return;
    }
    lab.go = false;
    if !lab.about.error.is_empty() {
        lab.message = lab.about.error.clone();
        return;
    }
    let shot = shot(&lab);
    let recipe = lab.recipe.clone();
    let redraw = lab.redraws();
    lab.started = Instant::now();
    lab.message.clear();
    let control = Arc::new(Control::default());
    control
        .every
        .store(if lab.live { lab.every } else { 0 }, Ordering::Relaxed);
    lab.control = Arc::clone(&control);
    let growth = redraw
        .then(|| {
            lab.shown
                .as_ref()
                .and_then(|shown| shown.scene.growth.clone())
        })
        .flatten();
    let handle = if let Some(growth) = growth {
        std::thread::spawn(move || Grown {
            result: plantlab_scene::dress(&shot, Library::builtin(), &growth),
            shot,
            recipe,
        })
    } else {
        lab.growths += 1;
        std::thread::spawn(move || {
            let mut viewer = Watcher(control, Instant::now());
            Grown {
                result: plantlab_scene::build_watched(&shot, Library::builtin(), &mut viewer),
                shot,
                recipe,
            }
        })
    };
    lab.growing = Some(Job { handle, redraw });
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
    let recipe = lab.recipe.clone();
    let seconds = lab.started.elapsed().as_secs_f64();
    put_on_screen(&mut stage, &mut lab, scene, (shot, recipe), (true, seconds));
}

/// Put a newly grown plant on screen in place of the old one, and keep it
/// as the species' last render; or show the plant on screen in a new look.
fn show(mut lab: ResMut<Lab>, mut stage: Stage) {
    if lab.restyle {
        lab.restyle = false;
        if let Some(old) = lab.shown.take() {
            for entity in &old.entities {
                stage.commands.entity(*entity).despawn();
            }
            let mut scene = old.scene;
            scene.look = lab.recipe.look;
            let (mut shot, mut recipe) = (old.shot, old.recipe);
            shot.look = scene.look;
            recipe.look = scene.look;
            put_on_screen(
                &mut stage,
                &mut lab,
                scene,
                (shot, recipe),
                (old.live, old.seconds),
            );
        }
    }
    if !lab
        .growing
        .as_ref()
        .is_some_and(|job| job.handle.is_finished())
    {
        return;
    }
    let Some(Ok(grown)) = lab.growing.take().map(|job| job.handle.join()) else {
        lab.message = "the growing thread panicked".into();
        return;
    };
    let scene = match grown.result {
        Ok(scene) => scene,
        Err(error) => {
            if lab.control.stop.load(Ordering::Relaxed) {
                let step = lab.control.step.load(Ordering::Relaxed);
                let steps = lab.control.steps.load(Ordering::Relaxed);
                let age = f64::from_bits(lab.control.age.load(Ordering::Relaxed));
                lab.message = format!("Stopped at step {step} of {steps}, {age:.1} years.");
                lab.year = None;
            } else {
                lab.message = error;
            }
            return;
        }
    };
    lab.message.clear();
    let seconds = lab.started.elapsed().as_secs_f64();
    let species = scene.facts.species.clone();
    let kept = Kept {
        recipe: grown.recipe.clone(),
        saved: store::now(),
        age: scene.facts.age,
        height_m: scene.facts.height_m,
        crown_width_m: scene.facts.crown_width_m,
        triangles: scene.facts.triangles,
        seconds,
        generator_revision: scene.facts.generator_revision,
    };
    put_on_screen(
        &mut stage,
        &mut lab,
        scene,
        (grown.shot, grown.recipe),
        (false, seconds),
    );
    // While the year plays, its months are not the species' last render.
    if lab.year.is_none() {
        match lab.store.keep(&species, kept) {
            Ok(()) => stage.recording.ask(Take::Keep, species),
            Err(error) => lab.message = format!("The render was not kept: {error}"),
        }
    }
}

/// Note the kept pictures written, so the window shows them anew.
fn kept(mut lab: ResMut<Lab>, mut recording: ResMut<Recording>) {
    for id in std::mem::take(&mut recording.kept) {
        lab.store.wrote(&id);
    }
}

/// Replace the plant on screen with `scene`: a grown plant, or a `live`
/// frame of one still growing, `seconds` after it started. The plant goes
/// on the window's layer and the plant-only layer, its ground and scale on
/// the window's alone.
fn put_on_screen(
    stage: &mut Stage,
    lab: &mut Lab,
    scene: Scene,
    (shot, recipe): (Shot, Recipe),
    (live, seconds): (bool, f64),
) {
    // A kept render not yet taken was of the plant going off screen; the
    // pictures take the view's shape.
    stage.recording.drop_keeps();
    #[allow(clippy::cast_possible_truncation)]
    let aspect = (1.0 / lab.aspect.max(0.1)) as f32;
    stage.recording.aspect = Some(aspect);
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
            orbit.frame(&scene.framing_for(lab.recipe.view, lab.aspect));
            lab.reframe = false;
            // Framed on a seedling: frame the grown plant too.
            lab.settle = live;
        } else if (lab.follow && live) || (!live && lab.settle) {
            // Keep the way the camera turns and any pan; frame the plant's
            // new size.
            orbit.fit(&scene.framing_for(lab.recipe.view, lab.aspect));
        }
        if !live {
            lab.settle = false;
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
        recipe,
        entities,
        seconds,
        live,
        at: Instant::now(),
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

/// A picture's name: the species, its age and the day.
fn picture_name(scene: &Scene) -> String {
    format!(
        "{}-{:.1}y-day{:.0}",
        scene.facts.species, scene.facts.age, scene.facts.day
    )
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
    if lab.reset_camera || lab.refit {
        lab.reset_camera = false;
        lab.refit = false;
        if let Some(shown) = &lab.shown {
            orbit.frame(&shown.scene.framing_for(lab.recipe.view, lab.aspect));
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
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-0.2, TOP_PITCH);
        }
        if scroll.delta.y != 0.0 {
            orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.1)).clamp(0.05, 2000.0);
        }
    }
    *transform = orbit.transform();
}

/// The window's camera keeps its perspective for the top view: straight
/// down, as near as the orbit turns, from as far above the plant's top as
/// shows the orthographic framing's height there. `None` for a framing in
/// perspective.
fn top_distance(framing: &plantlab_scene::Framing) -> Option<f32> {
    let plantlab_scene::Projection::Orthographic { half_height } = framing.projection else {
        return None;
    };
    #[allow(clippy::cast_possible_truncation)]
    let above = plantlab_scene::TOP_EYE_ABOVE as f32;
    let top = (framing.eye[1] - framing.target[1] - above).max(0.0);
    Some((top + half_height / (fov() * 0.5).tan()).max(0.5))
}

/// The steepest the camera looks down, radians: just short of straight
/// down, where turning it would have no meaning.
const TOP_PITCH: f32 = 1.55;

/// The camera's vertical field of view, radians.
fn fov() -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let fov = plantlab_scene::FOV_Y_DEG.to_radians() as f32;
    fov
}

/// Keys, unless a text field has them, each as a command: Enter
/// generates, Space pauses or resumes, Esc stops (or leaves the Overlay),
/// 1 to 4 the views, F frames the plant again, G the grid, R the ruler, O
/// the Overlay, P a snapshot, S saves the GIF, L the library, I the
/// inspector, F1 the keys, Ctrl+P the palette, Ctrl+, the settings.
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
    let growing = lab.growing.is_some();
    let (overlay, settings_open) = (shell.overlay, shell.settings_open);
    let (library, inspector) = (shell.library, shell.inspector);
    let mut send = |command| shell.queue.push(command);
    if control {
        if keys.just_pressed(KeyCode::KeyP) || keys.just_pressed(KeyCode::KeyK) {
            send(Command::Palette(String::new()));
        }
        if keys.just_pressed(KeyCode::Comma) {
            send(Command::Settings(true));
        }
        return;
    }
    let pressed = |key| keys.just_pressed(key);
    if pressed(KeyCode::Enter) || pressed(KeyCode::NumpadEnter) {
        send(Command::Generate);
    }
    if pressed(KeyCode::Space) && growing {
        send(Command::Pause(!lab.control.pause.load(Ordering::Relaxed)));
    }
    if pressed(KeyCode::Escape) {
        if growing || lab.year.is_some() {
            send(Command::Stop);
        } else if overlay {
            send(Command::Overlay(false));
        } else if settings_open {
            send(Command::Settings(false));
        }
    }
    for (key, view) in [
        (KeyCode::Digit1, View::Side),
        (KeyCode::Digit2, View::ThreeQuarter),
        (KeyCode::Digit3, View::Oblique),
        (KeyCode::Digit4, View::Top),
    ] {
        if pressed(key) {
            send(Command::View(view));
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
    if pressed(KeyCode::KeyL) {
        send(Command::Library(!library));
    }
    if pressed(KeyCode::KeyI) {
        send(Command::Inspector(!inspector));
    }
    if pressed(KeyCode::F1) {
        send(Command::Tab(Panel::Keys));
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
    use super::store::Detail;
    use super::*;

    /// A lab as the window starts it, on the bigleaf maple, keeping no
    /// renders.
    pub(super) fn lab() -> Lab {
        let mut lab = Lab {
            aspect: 1.0,
            catalogue: Catalogue::of(Library::builtin()),
            chosen: String::new(),
            about: About::default(),
            recipe: Recipe::default(),
            go: false,
            live: true,
            every: 10,
            follow: true,
            growing: None,
            control: Arc::default(),
            started: Instant::now(),
            growths: 0,
            shows: 0,
            shown: None,
            message: String::new(),
            reframe: false,
            reset_camera: false,
            refit: false,
            settle: false,
            restyle: false,
            grid: true,
            ruler: true,
            store: Store::default(),
            year: None,
            recent: Vec::new(),
        };
        lab.choose("acer-macrophyllum");
        lab
    }

    #[test]
    fn the_command_renders_what_the_window_shows() {
        let mut shot = Shot::thumbnail("acer-macrophyllum");
        shot.age = Some(30.0);
        shot.view = View::Side;
        shot.parts = true;
        shot.seed = Some(9);
        let text = thumbs_command(&shot);
        assert!(text.starts_with("plantlab thumbs acer-macrophyllum"));
        assert!(text.contains("--age 30") && text.contains("--view side"));
        assert!(text.contains("--quality full") && text.contains("--seed 9"));
    }

    #[test]
    fn choosing_a_species_grows_nothing_and_takes_up_its_last_recipe() {
        let mut lab = lab();
        assert!(!lab.go && lab.growing.is_none());
        assert_eq!(lab.chosen, "acer-macrophyllum");
        assert!(!lab.about.keyframes.is_empty());
        assert!((lab.age() - lab.about.oldest).abs() < 1e-9);
        let kept = Recipe {
            age: Some(4.0),
            day: 300.0,
            detail: Detail::Full,
            ..Recipe::default()
        };
        lab.store.kept.insert(
            "rosa-nutkana".into(),
            Kept {
                recipe: kept.clone(),
                saved: 0,
                age: 4.0,
                height_m: 1.0,
                crown_width_m: 1.0,
                triangles: 1,
                seconds: 1.0,
                generator_revision: 1,
            },
        );
        lab.choose("rosa-nutkana");
        assert_eq!(lab.recipe, kept);
        assert_eq!(lab.recent, ["rosa-nutkana", "acer-macrophyllum"]);
        let shot = shot(&lab);
        assert!(shot.parts && shot.level == 0 && shot.age == Some(4.0));
        // A species without a kept render keeps the day and the detail, and
        // grows the typical plant at its oldest keyframe.
        lab.choose("pseudotsuga-menziesii");
        assert_eq!(lab.recipe.age, None);
        assert!((lab.recipe.day - 300.0).abs() < 1e-9);
        assert_eq!(lab.recipe.detail, Detail::Full);
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
    fn the_top_view_looks_down_without_turning_over() {
        let mut orbit = Orbit {
            pan: Vec3::ZERO,
            target: Vec3::ZERO,
            distance: 10.0,
            yaw: 0.7,
            pitch: 0.4,
        };
        orbit.frame(&plantlab_scene::Framing {
            eye: [0.0, 60.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, -1.0],
            projection: plantlab_scene::Projection::Orthographic { half_height: 5.0 },
        });
        let transform = orbit.transform();
        assert!(transform.forward().y < -0.99, "looks down");
        assert!(transform.translation.is_finite());
        assert!(orbit.distance > 5.0);
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
