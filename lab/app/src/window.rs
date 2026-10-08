//! PlantLab's window (V3): one plant on its ground patch, a camera that
//! orbits it, and a panel that chooses what grows: the species, its age,
//! the day of the year, the level of detail, the quality and the look.
//!
//! The panel changes only [`Lab`]; a change regrows the plant on a worker
//! thread ([`plantlab_scene::build`], PlantGen's own growth), and the old
//! plant stays on screen until the new one is ready. Nothing about the
//! plant is computed here.

use std::fmt::Write as _;
use std::time::Instant;

use bevy::asset::embedded_asset;
use bevy::camera::Exposure;
use bevy::camera::visibility::RenderLayers;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::{DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use plantgen::library::Library;
use plantgen::quality;
use plantlab_scene::{Look, Scene, Shot, View};

use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::render::{CardMaterial, camera_look, exposure, spawn_plant, spawn_sun, to_rgba8};
use crate::theme;

/// What the panel chose, and what is on screen.
#[derive(Resource)]
struct Lab {
    species: Vec<String>,
    /// Index into `species`.
    chosen: usize,
    filter: String,
    /// Plant age, years; `None` until the species' keyframes are known.
    age: Option<f64>,
    oldest: f64,
    day: f64,
    level: usize,
    draft: bool,
    look: Look,
    view: View,
    /// The settings changed since the last growth started.
    dirty: bool,
    growing: Option<Task<Grown>>,
    started: Instant,
    shown: Option<Shown>,
    message: String,
    /// The camera should frame the next plant shown.
    reframe: bool,
}

struct Grown {
    result: Result<Scene, String>,
    shot: Shot,
}

/// The plant on screen.
struct Shown {
    scene: Scene,
    shot: Shot,
    entities: Vec<Entity>,
    seconds: f64,
}

/// The orbiting camera: around `target`, at `distance`, turned by `yaw`
/// and raised by `pitch` (radians).
#[derive(Component)]
struct Orbit {
    target: Vec3,
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
        Transform::from_translation(self.target + direction * self.distance)
            .looking_at(self.target, Vec3::Y)
    }
}

/// With `--capture FILE`: a picture of the window, taken once the first
/// plant has stood for a few frames, then the window closes. A smoke test
/// of the window, and a way to show it.
#[derive(Resource)]
struct Capture {
    path: std::path::PathBuf,
    frames: u32,
    asked: bool,
}

/// The sun of the plant on screen.
#[derive(Component)]
struct Sun;

/// Open the window, on `species` if given.
///
/// # Errors
///
/// When `species` is not in the library.
pub fn run(species: Option<&str>, picture: Option<std::path::PathBuf>) -> Result<(), String> {
    let library = Library::builtin();
    let ids: Vec<String> = library
        .species()
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    let chosen = match species {
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
    app.add_plugins((
        MaterialPlugin::<CardMaterial>::default(),
        EguiPlugin::default(),
    ))
    .insert_resource(DirectionalLightShadowMap { size: 4096 })
    .insert_resource(GlobalAmbientLight::NONE)
    .insert_resource(Lab {
        species: ids,
        chosen,
        filter: String::new(),
        age: None,
        oldest: 1.0,
        day: plantgen::package::DEFAULT_DAY,
        level: 0,
        draft: true,
        look: Look::Review,
        view: View::ThreeQuarter,
        dirty: true,
        growing: None,
        started: Instant::now(),
        shown: None,
        message: String::new(),
        reframe: true,
    })
    .add_systems(Startup, setup)
    .add_systems(EguiPrimaryContextPass, panel)
    .add_systems(Update, (grow, show, orbit).chain());
    if let Some(path) = picture {
        app.insert_resource(Capture {
            path,
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

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let light = plantlab_scene::review_light();
    let [r, g, b] = plantlab_scene::BACKGROUND;
    let orbit = Orbit {
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
        Projection::Perspective(PerspectiveProjection {
            #[allow(clippy::cast_possible_truncation)]
            fov: plantlab_scene::FOV_Y_DEG.to_radians() as f32,
            ..default()
        }),
        Exposure {
            ev100: exposure(&light),
        },
        RenderLayers::layer(1),
        orbit.transform(),
        orbit,
    ));
    camera_look(&mut camera, Look::Review, &light, &mut images);
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
fn panel(mut contexts: EguiContexts, mut lab: ResMut<Lab>, mut themed: Local<bool>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !*themed {
        theme::apply(ctx);
        *themed = true;
    }
    let lab = &mut *lab;
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
    egui::Panel::left("plant")
        .default_size(300.0)
        .show(&mut root, |ui| {
            ui.heading("PlantLab");
            ui.label(
                egui::RichText::new("Grown by PlantGen, as plantc grows it").color(theme::MUTED),
            );
            ui.separator();

            ui.label("Species");
            ui.text_edit_singleline(&mut lab.filter);
            let filter = lab.filter.to_lowercase();
            egui::ScrollArea::vertical()
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
                if ui
                    .add(egui::Slider::new(&mut age, 1.0..=oldest).step_by(1.0))
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

            if lab.growing.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!(
                        "growing… {:.1} s",
                        lab.started.elapsed().as_secs_f64()
                    ));
                });
            }
            if !lab.message.is_empty() {
                ui.colored_label(theme::ERROR, &lab.message);
            }
            if let Some(shown) = &lab.shown {
                let facts = &shown.scene.facts;
                egui::Grid::new("facts").num_columns(2).show(ui, |ui| {
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
                    row("grown in", format!("{:.1} s", shown.seconds));
                });
                ui.separator();
                ui.label(egui::RichText::new("Render it without the window:").color(theme::MUTED));
                let mut text = command(&shown.shot);
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(3),
                );
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Drag to turn the plant, scroll to come closer.")
                    .color(theme::MUTED),
            );
        });
}

/// Start growing when the settings changed and nothing is growing.
fn grow(mut lab: ResMut<Lab>) {
    if !lab.dirty || lab.growing.is_some() {
        return;
    }
    lab.dirty = false;
    if lab.age.is_none() {
        let ages = keyframes(&lab.species[lab.chosen]);
        lab.oldest = ages.last().copied().unwrap_or(1.0);
        lab.age = Some(lab.oldest);
    }
    let shot = shot(&lab);
    lab.started = Instant::now();
    lab.growing = Some(AsyncComputeTaskPool::get().spawn(async move {
        Grown {
            result: plantlab_scene::build(&shot, Library::builtin()),
            shot,
        }
    }));
}

/// Put a newly grown plant on screen in place of the old one.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut lab: ResMut<Lab>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut cards: ResMut<Assets<CardMaterial>>,
    mut camera: Query<(Entity, &mut Orbit)>,
    suns: Query<Entity, With<Sun>>,
) {
    let Some(task) = lab.growing.as_mut() else {
        return;
    };
    let Some(grown) = block_on(poll_once(task)) else {
        return;
    };
    lab.growing = None;
    let seconds = lab.started.elapsed().as_secs_f64();
    let scene = match grown.result {
        Ok(scene) => scene,
        Err(error) => {
            lab.message = error;
            return;
        }
    };
    lab.message.clear();
    if let Some(old) = lab.shown.take() {
        for entity in old.entities {
            commands.entity(entity).despawn();
        }
    }
    for sun in &suns {
        commands.entity(sun).despawn();
    }
    let sun = spawn_sun(&mut commands, std::slice::from_ref(&scene));
    commands.entity(sun).insert(Sun);
    let entities = spawn_plant(
        &mut commands,
        &scene,
        (&mut images, &mut meshes, &mut standard, &mut cards),
        Vec3::ZERO,
        &RenderLayers::layer(1),
    );
    if let Ok((entity, mut orbit)) = camera.single_mut() {
        camera_look(
            &mut commands.entity(entity),
            scene.look,
            &scene.light,
            &mut images,
        );
        if lab.reframe {
            let framing = scene.framing;
            let eye = Vec3::from_array(framing.eye);
            let target = Vec3::from_array(framing.target);
            let offset = eye - target;
            orbit.target = target;
            orbit.distance = offset.length().max(0.5);
            orbit.yaw = offset.x.atan2(offset.z);
            orbit.pitch = (offset.y / orbit.distance).clamp(-1.0, 1.0).asin();
            lab.reframe = false;
        }
    }
    lab.shown = Some(Shown {
        scene,
        shot: grown.shot,
        entities,
        seconds,
    });
}

/// Drag to turn around the plant, scroll to come closer.
fn orbit(
    mut contexts: EguiContexts,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
) {
    let over_panel = contexts
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_pointer_input() || ctx.is_pointer_over_egui());
    let Ok((mut orbit, mut transform)) = camera.single_mut() else {
        return;
    };
    if !over_panel {
        if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right) {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-0.2, 1.5);
        }
        if scroll.delta.y != 0.0 {
            orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.1)).clamp(0.2, 500.0);
        }
    }
    *transform = orbit.transform();
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
}
