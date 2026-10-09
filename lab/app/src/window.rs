//! PlantLab's window (V3): one plant on its ground patch, a camera that
//! orbits it, and a panel that chooses what grows: the species, its age,
//! the day of the year, the level of detail, the quality and the look.
//!
//! The panel changes only [`Lab`]; a change regrows the plant on a worker
//! thread ([`plantlab_scene::build_watched`], PlantGen's own growth). The
//! panel can start, pause and stop a growth, shows its progress, and can
//! show the plant growing: every N steps the worker draws the plant as it
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

use bevy::asset::embedded_asset;
use bevy::camera::Exposure;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::{DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use plantgen::library::Library;
use plantgen::quality;
use plantlab_scene::{Look, Progress, Scene, Shot, View, Viewer};

use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::measure::{self, MeasureGizmos};
use crate::record::{self, PLANT_LAYER, Recorder, Recording, Take, WINDOW_LAYER};
use crate::render::{CardMaterial, camera_look, exposure, spawn_plant, spawn_sun, to_rgba8};
use crate::theme;

/// How `plantlab open` starts the window.
#[allow(clippy::struct_excessive_bools, reason = "the command's switches")]
pub struct Options {
    pub species: Option<String>,
    /// Grow the first plant to this age, years, instead of its oldest
    /// keyframe.
    pub age: Option<f64>,
    /// Show the plant growing every this many steps; 0 for none.
    pub live: u32,
    pub grid: bool,
    pub ruler: bool,
    /// Where snapshots and animations go.
    pub out: std::path::PathBuf,
    /// Start recording growth, and of the plant alone.
    pub record: bool,
    pub plant_only: bool,
    /// Save a picture of the window and close it: once the plant has
    /// grown, or after this many seconds.
    pub capture: Option<(std::path::PathBuf, Option<f64>)>,
}

/// What the panel chose, and what is on screen.
#[derive(Resource)]
#[allow(clippy::struct_excessive_bools, reason = "the panel's switches")]
struct Lab {
    species: Vec<String>,
    /// Index into `species`.
    chosen: usize,
    filter: String,
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
    shown: Option<Shown>,
    message: String,
    /// The camera should frame the next plant shown.
    reframe: bool,
    /// Frame the plant on screen again now ("Reset camera").
    reset_camera: bool,
    /// The measures on the ground and beside the plant.
    grid: bool,
    ruler: bool,
    /// Where the panel ends, logical pixels from the window's left.
    panel_right: f32,
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
}

/// The worker's side of [`Control`].
struct Watcher(Arc<Control>);

impl Viewer for Watcher {
    fn wants_frame(&mut self, step: u32) -> bool {
        let every = self.0.every.load(Ordering::Relaxed);
        every > 0 && step.is_multiple_of(every) && !self.0.stop.load(Ordering::Relaxed)
    }

    fn step(&mut self, progress: Progress<'_>, frame: Option<Scene>) -> bool {
        self.0.step.store(progress.step, Ordering::Relaxed);
        self.0.steps.store(progress.steps, Ordering::Relaxed);
        self.0.age.store(progress.age.to_bits(), Ordering::Relaxed);
        if let Some(frame) = frame
            && let Ok(mut slot) = self.0.frame.lock()
        {
            *slot = Some(frame);
        }
        while self.0.pause.load(Ordering::Relaxed) && !self.0.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(30));
        }
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
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "PlantLab".into(),
                    resolution: (1400, 900).into(),
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
    embedded_asset!(app, "shaders/card.wgsl");
    let [r, g, b] = plantlab_scene::BACKGROUND;
    app.add_plugins((
        MaterialPlugin::<CardMaterial>::default(),
        EguiPlugin::default(),
    ))
    .init_gizmo_group::<MeasureGizmos>()
    .insert_resource({
        let mut recording = Recording::new(out);
        recording.record = record;
        recording.plant_only = plant_only;
        recording
    })
    .insert_resource(record::Background(Color::linear_rgb(r, g, b)))
    .insert_resource(DirectionalLightShadowMap { size: 4096 })
    .insert_resource(GlobalAmbientLight::NONE)
    .insert_resource(Lab {
        species: ids,
        chosen,
        filter: String::new(),
        age: None,
        asked_age: age,
        oldest: 1.0,
        day: plantgen::package::DEFAULT_DAY,
        level: 0,
        draft: true,
        look: Look::Review,
        view: View::ThreeQuarter,
        dirty: true,
        auto: true,
        go: false,
        live: live > 0,
        every: live.max(1),
        follow: true,
        growing: None,
        control: Arc::default(),
        started: Instant::now(),
        shown: None,
        message: String::new(),
        reframe: true,
        reset_camera: false,
        grid,
        ruler,
        panel_right: 300.0,
    })
    .add_systems(Startup, setup)
    .add_systems(EguiPrimaryContextPass, panel)
    .add_systems(
        Update,
        (
            keys,
            grow,
            live_frame,
            show,
            orbit,
            measures,
            record::follow,
            record::take,
            record::keep,
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

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut gizmos: ResMut<GizmoConfigStore>,
    mut egui: ResMut<EguiGlobalSettings>,
) {
    // The window's camera carries the panel; egui must not give the
    // recorder a second primary context.
    egui.auto_create_primary_context = false;
    measure::configure(&mut gizmos, WINDOW_LAYER);
    let light = plantlab_scene::review_light();
    let [r, g, b] = plantlab_scene::BACKGROUND;
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
            clear_color: ClearColorConfig::Custom(Color::linear_rgb(r, g, b)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection { fov, ..default() }),
        Exposure {
            ev100: exposure(&light),
        },
        RenderLayers::layer(WINDOW_LAYER),
        orbit.transform(),
        orbit,
        // The panel draws on this camera, not on the recorder.
        PrimaryEguiContext,
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
    shot
}

/// The command that renders the shot on screen without the window.
fn command(shot: &Shot) -> String {
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

#[allow(clippy::too_many_lines)]
fn panel(
    mut contexts: EguiContexts,
    mut lab: ResMut<Lab>,
    mut recording: ResMut<Recording>,
    mut themed: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !*themed {
        theme::apply(ctx);
        *themed = true;
    }
    let lab = &mut *lab;
    let recording = &mut *recording;
    let ctx = ctx.clone();
    // As WorldLab does with egui 0.36: panels lie in a root Ui over the
    // whole window.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("plantlab"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let shown = egui::Panel::left("plant")
        .default_size(300.0)
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("PlantLab");
                ui.label(
                    egui::RichText::new("Grown by PlantGen, as plantc grows it")
                        .color(theme::MUTED),
                );
                ui.separator();

                ui.label("Species");
                ui.text_edit_singleline(&mut lab.filter);
                let filter = lab.filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("species")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for index in 0..lab.species.len() {
                            let id = &lab.species[index];
                            if !filter.is_empty() && !id.contains(&filter) {
                                continue;
                            }
                            if ui
                                .selectable_label(index == lab.chosen, id.as_str())
                                .clicked()
                                && index != lab.chosen
                            {
                                lab.chosen = index;
                                lab.age = None;
                                lab.dirty = true;
                                lab.reframe = true;
                            }
                        }
                    });
                ui.separator();

                if let Some(mut age) = lab.age {
                    ui.label("Age, years");
                    let oldest = lab.oldest;
                    let youngest = age.min(1.0);
                    let step = if youngest < 1.0 { 0.0 } else { 1.0 };
                    if ui
                        .add(egui::Slider::new(&mut age, youngest..=oldest).step_by(step))
                        .changed()
                    {
                        lab.age = Some(age);
                        lab.dirty = true;
                    }
                }
                ui.label("Day of the year");
                if ui
                    .add(egui::Slider::new(&mut lab.day, 1.0..=365.0).step_by(1.0))
                    .changed()
                {
                    lab.dirty = true;
                }
                ui.horizontal(|ui| {
                    ui.label("Level of detail");
                    for level in 0..4 {
                        if ui
                            .selectable_label(lab.level == level, level.to_string())
                            .clicked()
                        {
                            lab.level = level;
                            lab.dirty = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Quality");
                    if ui.selectable_label(lab.draft, "draft").clicked() && !lab.draft {
                        lab.draft = true;
                        lab.dirty = true;
                    }
                    if ui.selectable_label(!lab.draft, "standard").clicked() && lab.draft {
                        lab.draft = false;
                        lab.dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Look");
                    for look in [Look::Review, Look::Photo] {
                        if ui.selectable_label(lab.look == look, look.name()).clicked()
                            && lab.look != look
                        {
                            lab.look = look;
                            lab.dirty = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("View");
                    for view in [View::Side, View::ThreeQuarter, View::Top] {
                        if ui.selectable_label(lab.view == view, view.name()).clicked() {
                            lab.view = view;
                            lab.reframe = true;
                            lab.dirty = true;
                        }
                    }
                });
                ui.separator();

                growth_controls(ui, lab);
                if !lab.message.is_empty() {
                    ui.colored_label(theme::ERROR, &lab.message);
                }
                if let Some(shown) = &lab.shown {
                    let facts = &shown.scene.facts;
                    egui::Grid::new("facts")
                        .num_columns(2)
                        .show(ui, |ui| {
                            let mut row = |name: &str, value: String| {
                                ui.label(egui::RichText::new(name).color(theme::MUTED));
                                ui.label(value);
                                ui.end_row();
                            };
                            row("species", facts.species.clone());
                            row("age", format!("{} years", facts.age));
                            row("height", format!("{:.2} m", facts.height_m));
                            row("crown width", format!("{:.2} m", facts.crown_width_m));
                            row("triangles", facts.triangles.to_string());
                            row("cards", facts.cards.to_string());
                            row("conditions", facts.environment.clone());
                            row("seed", facts.seed.to_string());
                            row(
                                "generator",
                                format!("revision {}", facts.generator_revision),
                            );
                            if shown.live {
                                row("drawn", format!("{:.1} s into growing", shown.seconds));
                            } else {
                                row("grown in", format!("{:.1} s", shown.seconds));
                            }
                            let export = facts.export;
                            row("packaged", bytes(export.package_mesh));
                            row("as .glb", format!("about {}", bytes(export.glb)));
                            row("as .obj", format!("about {}", bytes(export.obj)));
                        })
                        .response
                        .on_hover_text(
                            "File sizes of the plant on screen, this level only. \
                     \"packaged\" is exact: this level's mesh as a PlantGen package stores it. \
                     A whole package holds four levels for every keyframe and variant, \
                     plus impostors and textures: `plantc build` gives its size. \
                     The glTF and OBJ sizes are estimates for the drawn triangles \
                     with the card textures as one PNG.",
                        );
                    ui.separator();
                    ui.label(
                        egui::RichText::new("Render it without the window:").color(theme::MUTED),
                    );
                    let mut text = command(&shown.shot);
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(3),
                    );
                }
                ui.separator();
                measure_controls(ui, lab);
                ui.separator();
                picture_controls(ui, lab, recording);
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        "Drag to turn the plant, middle-drag or Shift-drag to move it, \
                     scroll to come closer.",
                    )
                    .color(theme::MUTED),
                );
            });
        });
    lab.panel_right = shown.response.rect.right();
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
    lab.message.clear();
    let control = Arc::new(Control::default());
    control
        .every
        .store(if lab.live { lab.every } else { 0 }, Ordering::Relaxed);
    lab.control = Arc::clone(&control);
    lab.growing = Some(std::thread::spawn(move || {
        let mut viewer = Watcher(control);
        Grown {
            result: plantlab_scene::build_watched(&shot, Library::builtin(), &mut viewer),
            shot,
        }
    }));
}

/// Grow, stop, pause and resume; live frames; the growth's progress.
fn growth_controls(ui: &mut egui::Ui, lab: &mut Lab) {
    let growing = lab.growing.is_some();
    let paused = lab.control.pause.load(Ordering::Relaxed);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!growing, egui::Button::new("Grow"))
            .on_hover_text("Grow the plant with these settings")
            .clicked()
        {
            lab.go = true;
        }
        if ui
            .add_enabled(growing, egui::Button::new("Stop"))
            .on_hover_text("Stop growing; the last frame shown stays")
            .clicked()
        {
            lab.control.stop.store(true, Ordering::Relaxed);
            lab.dirty = false;
        }
        let pause = if paused { "Resume" } else { "Pause" };
        if ui.add_enabled(growing, egui::Button::new(pause)).clicked() {
            lab.control.pause.store(!paused, Ordering::Relaxed);
        }
    });
    ui.checkbox(&mut lab.auto, "Grow when the settings change");
    if !lab.auto && lab.dirty && !growing {
        ui.label(egui::RichText::new("Settings changed: press Grow").color(theme::MUTED));
    }
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut lab.live, "Show it growing, every")
            .changed()
            && growing
        {
            let every = if lab.live { lab.every } else { 0 };
            lab.control.every.store(every, Ordering::Relaxed);
        }
        if ui
            .add_enabled(
                lab.live,
                egui::Slider::new(&mut lab.every, 1..=100)
                    .logarithmic(true)
                    .suffix(" steps"),
            )
            .changed()
            && growing
        {
            lab.control.every.store(lab.every, Ordering::Relaxed);
        }
    });
    ui.checkbox(&mut lab.follow, "Move the camera with the plant");
    if growing {
        let step = lab.control.step.load(Ordering::Relaxed);
        let steps = lab.control.steps.load(Ordering::Relaxed);
        let age = f64::from_bits(lab.control.age.load(Ordering::Relaxed));
        let seconds = lab.started.elapsed().as_secs_f64();
        let (share, text) = if steps == 0 {
            (0.0, format!("starting, {seconds:.1} s"))
        } else if step >= steps {
            (1.0, format!("grown, meshing, {seconds:.1} s"))
        } else {
            (
                f64::from(step) / f64::from(steps),
                format!("step {step} of {steps}, {age:.1} years, {seconds:.1} s"),
            )
        };
        #[allow(clippy::cast_possible_truncation)]
        let bar = egui::ProgressBar::new(share as f32).text(if paused {
            format!("paused at {text}")
        } else {
            text
        });
        ui.add(bar);
    }
}

/// What putting a plant on screen touches.
#[derive(SystemParam)]
struct Stage<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    standard: ResMut<'w, Assets<StandardMaterial>>,
    cards: ResMut<'w, Assets<CardMaterial>>,
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
        (
            &mut stage.images,
            &mut stage.meshes,
            &mut stage.standard,
            &mut stage.cards,
        ),
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
            &mut stage.images,
        );
    }
    if let Ok((entity, mut orbit)) = stage.camera.single_mut() {
        camera_look(
            &mut commands.entity(entity),
            scene.look,
            &scene.light,
            &mut stage.images,
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
    mut camera: Query<(&mut Orbit, &mut Transform), Without<Recorder>>,
) {
    let over_panel = contexts
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui());
    let Ok((mut orbit, mut transform)) = camera.single_mut() else {
        return;
    };
    if lab.reset_camera {
        lab.reset_camera = false;
        if let Some(shown) = &lab.shown {
            orbit.frame(&shown.scene.framing);
        }
    }
    if !over_panel {
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        if buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Left)) {
            let pixels = windows.iter().next().map_or(900.0, Window::height);
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

/// Keys, unless a text field has them: G the grid, R the ruler, F frames
/// the plant again, P takes a snapshot, S saves the GIF.
fn keys(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut lab: ResMut<Lab>,
    mut recording: ResMut<Recording>,
) {
    if contexts
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_keyboard_input())
    {
        return;
    }
    if keys.just_pressed(KeyCode::KeyG) {
        lab.grid = !lab.grid;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.ruler = !lab.ruler;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        lab.reset_camera = true;
    }
    if keys.just_pressed(KeyCode::KeyP)
        && let Some(shown) = &lab.shown
    {
        recording.ask(Take::Snapshot, record::stamped(&picture_name(&shown.scene)));
    }
    if keys.just_pressed(KeyCode::KeyS)
        && let Some(shown) = &lab.shown
    {
        save_gif(&mut recording, &shown.scene);
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
    let screen = windows
        .iter()
        .next()
        .map_or(Vec2::new(1400.0, 900.0), |window| {
            Vec2::new(window.width(), window.height())
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
                    seen.x > lab.panel_right + 30.0
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

/// The measures and the camera.
fn measure_controls(ui: &mut egui::Ui, lab: &mut Lab) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut lab.grid, "Grid (G)");
        ui.checkbox(&mut lab.ruler, "Height ruler (R)");
        if ui.button("Reset camera (F)").clicked() {
            lab.reset_camera = true;
        }
    });
    if let Some(shown) = &lab.shown
        && lab.grid
    {
        let step = measure::grid_step(shown.scene.ground_radius);
        ui.label(
            egui::RichText::new(format!(
                "Grid squares {}, bold lines every {}",
                measure::length(step, step),
                measure::length(step * 5.0, step * 5.0)
            ))
            .color(theme::MUTED),
        );
    }
}

/// Snapshots and animations.
fn picture_controls(ui: &mut egui::Ui, lab: &Lab, recording: &mut Recording) {
    ui.horizontal(|ui| {
        ui.label("Pictures of");
        if ui
            .selectable_label(!recording.plant_only, "the scene")
            .on_hover_text("As the window shows it: ground, scale and measures")
            .clicked()
        {
            recording.plant_only = false;
        }
        if ui
            .selectable_label(recording.plant_only, "the plant alone")
            .on_hover_text("On a clear background: transparent PNGs and GIFs")
            .clicked()
        {
            recording.plant_only = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Width");
        ui.add(egui::Slider::new(&mut recording.width, 128..=1920).suffix(" px"));
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(lab.shown.is_some(), egui::Button::new("Snapshot (P)"))
            .on_hover_text("Save a PNG of the plant on screen")
            .clicked()
            && let Some(shown) = &lab.shown
        {
            recording.ask(Take::Snapshot, record::stamped(&picture_name(&shown.scene)));
        }
        if ui
            .checkbox(&mut recording.record, "Record growth")
            .on_hover_text(
                "Take a frame each time a plant or a frame of its growth is shown. \
                 Turn off \"Move the camera with the plant\" to keep the camera still.",
            )
            .changed()
            && recording.record
            && let Some(shown) = &lab.shown
        {
            recording.ask(Take::Frame, picture_name(&shown.scene));
        }
    });
    ui.horizontal(|ui| {
        let mut milliseconds = u32::from(recording.delay) * 10;
        ui.label("Frame");
        if ui
            .add(
                egui::Slider::new(&mut milliseconds, 20..=2000)
                    .logarithmic(true)
                    .suffix(" ms"),
            )
            .changed()
        {
            recording.delay = u16::try_from(milliseconds / 10).unwrap_or(u16::MAX).max(2);
        }
    });
    ui.horizontal(|ui| {
        let mut seconds = f32::from(recording.hold) / 100.0;
        ui.label("Hold the last");
        if ui
            .add(egui::Slider::new(&mut seconds, 0.0..=10.0).suffix(" s"))
            .changed()
        {
            // At most 1000 hundredths.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let hundredths = (seconds * 100.0).round() as u16;
            recording.hold = hundredths;
        }
    });
    ui.horizontal(|ui| {
        let frames = recording.frames.len();
        ui.label(format!("{frames} frames"));
        if ui
            .add_enabled(frames > 0, egui::Button::new("Save GIF (S)"))
            .clicked()
            && let Some(shown) = &lab.shown
        {
            save_gif(recording, &shown.scene);
        }
        if ui
            .add_enabled(frames > 0, egui::Button::new("Clear"))
            .clicked()
        {
            recording.frames.clear();
        }
        ui.checkbox(&mut recording.pngs, "and PNGs");
    });
    ui.label(
        egui::RichText::new(format!("Saved in {}", recording.folder.display())).color(theme::MUTED),
    );
    if !recording.message.is_empty() {
        ui.label(egui::RichText::new(&recording.message).color(theme::MUTED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_renders_what_the_window_shows() {
        let mut shot = Shot::thumbnail("acer-macrophyllum");
        shot.age = Some(30.0);
        shot.view = View::Side;
        let text = command(&shot);
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
        let mut watcher = Watcher(Arc::clone(&control));
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
