//! Snapshots and growth animations of the plant in the window.
//!
//! A second camera follows the window's camera and draws into a picture of
//! its own: the scene as the window shows it (ground, scale, measures), or
//! the plant alone on a clear background. It draws only while something
//! is asked of it. A snapshot is saved as a PNG at once; a frame joins the
//! animation, which "Save GIF" writes.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::PrimaryWindow;

use crate::gif;
use crate::render::{target_image, to_rgba8};

/// The window's camera draws this layer: the plant, its ground and scale,
/// the measures.
pub const WINDOW_LAYER: usize = 1;
/// The plant alone, for pictures without the ground.
pub const PLANT_LAYER: usize = 2;

/// Frames to let the recorder draw before reading its picture back.
const SETTLE_FRAMES: u32 = 3;
/// Readings of a blank picture before giving up on it.
const RETRIES: u32 = 8;

/// The camera that takes the pictures.
#[derive(Component)]
pub struct Recorder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    /// A PNG of the plant on screen, saved at once.
    Snapshot,
    /// A frame for the animation.
    Frame,
}

/// What the panel asked for, and what has been taken.
#[derive(Resource)]
pub struct Recording {
    /// Where pictures and animations are saved.
    pub folder: PathBuf,
    /// The pictures' width in pixels; their height follows the window.
    pub width: u32,
    /// Only the plant, on a clear background: no ground, scale or
    /// measures.
    pub plant_only: bool,
    /// Take a frame each time a plant, or a frame of one growing, is shown.
    pub record: bool,
    /// Each frame shows this long, and the last this much longer
    /// (hundredths of a second).
    pub delay: u16,
    pub hold: u16,
    /// Save each frame as a PNG beside the GIF too.
    pub pngs: bool,
    pub frames: Vec<gif::Frame>,
    pub message: String,
    queue: VecDeque<(Take, String)>,
    busy: Option<Busy>,
    done: Arc<Mutex<Vec<Done>>>,
    target: Option<(Handle<Image>, UVec2)>,
}

struct Busy {
    take: Take,
    name: String,
    waited: u32,
    asked: bool,
    tries: u32,
}

struct Done {
    take: Take,
    name: String,
    picture: Option<(usize, usize, Vec<u8>)>,
}

impl Recording {
    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            width: 640,
            plant_only: false,
            record: false,
            delay: 12,
            hold: 150,
            pngs: false,
            frames: Vec::new(),
            message: String::new(),
            queue: VecDeque::new(),
            busy: None,
            done: Arc::default(),
            target: None,
        }
    }

    /// Ask for a picture; `name` is the file name without its extension. A
    /// frame asked for while another waits replaces it: the plant moved on.
    pub fn ask(&mut self, take: Take, name: String) {
        if take == Take::Frame
            && let Some(waiting) = self.queue.iter_mut().find(|(t, _)| *t == Take::Frame)
        {
            waiting.1 = name;
            return;
        }
        self.queue.push_back((take, name));
    }

    /// Something is being taken or waits to be.
    #[must_use]
    pub fn working(&self) -> bool {
        self.busy.is_some() || !self.queue.is_empty()
    }

    /// Write the frames taken as a looping GIF, transparent when any frame
    /// is, and as PNGs too when asked. Returns the GIF's path.
    ///
    /// # Errors
    ///
    /// When there are no frames, or a file cannot be written.
    pub fn save_gif(&self, name: &str) -> Result<PathBuf, String> {
        let mut frames = self.frames.clone();
        let last = frames.len().checked_sub(1).ok_or("no frames taken yet")?;
        for (index, frame) in frames.iter_mut().enumerate() {
            frame.delay = if index == last {
                self.delay.saturating_add(self.hold)
            } else {
                self.delay
            };
        }
        let transparent = frames
            .iter()
            .any(|frame| frame.rgba.as_chunks::<4>().0.iter().any(|p| p[3] < 255));
        let bytes = gif::encode(&frames, transparent)?;
        std::fs::create_dir_all(&self.folder).map_err(|error| error.to_string())?;
        let path = self.folder.join(format!("{name}.gif"));
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        if self.pngs {
            let folder = self.folder.join(name);
            std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
            for (index, frame) in frames.iter().enumerate() {
                write_png(
                    &folder.join(format!("{:04}.png", index + 1)),
                    frame.width,
                    frame.height,
                    &frame.rgba,
                )?;
            }
        }
        Ok(path)
    }
}

/// A name no other picture has: `stem` and the time.
#[must_use]
pub fn stamped(stem: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis());
    format!("{stem}-{millis}")
}

fn write_png(path: &Path, width: usize, height: usize, rgba: &[u8]) -> Result<(), String> {
    let png =
        plantgen::raster::encode_png(width, height, rgba).map_err(|error| error.to_string())?;
    std::fs::write(path, png).map_err(|error| format!("{}: {error}", path.display()))
}

/// The recorder: off, drawing into a placeholder until asked.
pub fn spawn(commands: &mut Commands, images: &mut Assets<Image>, fov: f32) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            Camera {
                is_active: false,
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(images.add(target_image(2, 2)).into()),
            Projection::Perspective(PerspectiveProjection { fov, ..default() }),
            RenderLayers::layer(WINDOW_LAYER),
            Transform::default(),
            Recorder,
        ))
        .id()
}

/// Keep the recorder where the window's camera is, drawing what was asked,
/// and only while it is needed.
#[allow(clippy::type_complexity)]
pub fn follow(
    recording: Res<Recording>,
    background: Res<Background>,
    window_camera: Query<&Transform, (With<Camera3d>, Without<Recorder>)>,
    mut recorder: Query<(&mut Camera, &mut Transform, &mut RenderLayers), With<Recorder>>,
) {
    let Ok((mut camera, mut transform, mut layers)) = recorder.single_mut() else {
        return;
    };
    let active = recording.record || recording.working();
    if camera.is_active != active {
        camera.is_active = active;
    }
    if !active {
        return;
    }
    if let Some(seen) = window_camera.iter().next() {
        *transform = *seen;
    }
    let (layer, clear) = if recording.plant_only {
        (PLANT_LAYER, Color::NONE)
    } else {
        (WINDOW_LAYER, background.0)
    };
    if !layers.intersects(&RenderLayers::layer(layer)) || layers.iter().count() != 1 {
        *layers = RenderLayers::layer(layer);
    }
    camera.clear_color = ClearColorConfig::Custom(clear);
}

/// The window's background colour, for pictures of the whole scene.
#[derive(Resource, Clone, Copy)]
pub struct Background(pub Color);

/// Take what was asked: size the recorder's picture to the window, let it
/// draw a few frames, then read it back.
#[allow(clippy::needless_pass_by_value)]
pub fn take(
    mut commands: Commands,
    mut recording: ResMut<Recording>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut recorder: Query<&mut RenderTarget, With<Recorder>>,
) {
    let recording = &mut *recording;
    if recording.busy.is_none() {
        let Some((take, name)) = recording.queue.pop_front() else {
            return;
        };
        let aspect = windows
            .iter()
            .next()
            .map_or(1.0, |window| window.height() / window.width().max(1.0));
        let width = recording.width.clamp(64, 4096);
        // Even sizes; the window's shape.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let height = ((width as f32 * aspect / 2.0).round() as u32 * 2).max(2);
        let size = UVec2::new(width, height);
        if recording
            .target
            .as_ref()
            .is_none_or(|(_, had)| *had != size)
        {
            let handle = images.add(target_image(size.x, size.y));
            if let Ok(mut target) = recorder.single_mut() {
                *target = RenderTarget::Image(handle.clone().into());
            }
            recording.target = Some((handle, size));
        }
        recording.busy = Some(Busy {
            take,
            name,
            waited: 0,
            asked: false,
            tries: 0,
        });
        return;
    }
    let Some(busy) = recording.busy.as_mut() else {
        return;
    };
    if busy.asked {
        return;
    }
    busy.waited += 1;
    if busy.waited < SETTLE_FRAMES {
        return;
    }
    let Some((handle, _)) = recording.target.clone() else {
        return;
    };
    busy.asked = true;
    let done = Arc::clone(&recording.done);
    let (take, name) = (busy.take, busy.name.clone());
    commands
        .spawn(Screenshot::image(handle))
        .observe(move |event: On<ScreenshotCaptured>| {
            let image = &event.image;
            let picture = image.data.as_ref().map(|data| {
                (
                    image.width() as usize,
                    image.height() as usize,
                    to_rgba8(data, image.texture_descriptor.format),
                )
            });
            if let Ok(mut done) = done.lock() {
                done.push(Done {
                    take,
                    name: name.clone(),
                    picture,
                });
            }
        });
}

/// Keep the pictures read back: save a snapshot, or add a frame.
pub fn keep(mut recording: ResMut<Recording>) {
    let done: Vec<Done> = match recording.done.lock() {
        Ok(mut done) => done.drain(..).collect(),
        Err(_) => return,
    };
    for done in done {
        let Some((width, height, mut rgba)) = done.picture else {
            recording.message = "the picture could not be read back".into();
            recording.busy = None;
            continue;
        };
        if blank(&rgba)
            && let Some(busy) = recording.busy.as_mut()
            && busy.tries < RETRIES
        {
            // Not drawn yet: read it again after a few more frames.
            busy.tries += 1;
            busy.waited = 0;
            busy.asked = false;
            continue;
        }
        recording.busy = None;
        if recording.plant_only {
            gif::unpremultiply(&mut rgba);
        }
        match done.take {
            Take::Snapshot => {
                let folder = recording.folder.clone();
                let path = folder.join(format!("{}.png", done.name));
                recording.message = std::fs::create_dir_all(&folder)
                    .map_err(|error| error.to_string())
                    .and_then(|()| write_png(&path, width, height, &rgba))
                    .map_or_else(|error| error, |()| format!("Saved {}", path.display()));
            }
            Take::Frame => {
                let delay = recording.delay;
                recording.frames.push(gif::Frame {
                    width,
                    height,
                    rgba,
                    delay,
                });
            }
        }
    }
}

/// Every pixel the same: nothing drawn yet.
fn blank(rgba: &[u8]) -> bool {
    let pixels = rgba.as_chunks::<4>().0;
    pixels
        .first()
        .is_none_or(|first| pixels.iter().all(|pixel| pixel == first))
}
