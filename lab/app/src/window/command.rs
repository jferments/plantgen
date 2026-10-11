//! The window's command path. Every button, menu entry, palette entry and
//! key queues a [`Command`] on the [`Shell`]; [`apply`] carries them out in
//! order, once a frame, and is the only place that changes what grows,
//! what shows and what is saved. The interface itself only reads.
//!
//! A command that changes a saved setting saves that one setting
//! ([`crate::settings`]).

use std::sync::atomic::Ordering;

use bevy::prelude::*;
use plantlab_scene::{Look, View};
use plantlab_ui::icons;
use serde_json::json;

use super::{Lab, picture_name, save_gif};
use crate::record::{self, Recording, Take};
use crate::settings::Key;

/// The panels, each on the rail. [`Panel::Keys`] sits at the rail's foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Plant,
    Grow,
    View,
    Readings,
    Pictures,
    Spec,
    Bark,
    Keys,
}

impl Panel {
    /// The rail, top to bottom; the keys come at its foot.
    pub const RAIL: [Self; 7] = [
        Self::Plant,
        Self::Grow,
        Self::View,
        Self::Readings,
        Self::Pictures,
        Self::Spec,
        Self::Bark,
    ];
    pub const ALL: [Self; 8] = [
        Self::Plant,
        Self::Grow,
        Self::View,
        Self::Readings,
        Self::Pictures,
        Self::Spec,
        Self::Bark,
        Self::Keys,
    ];

    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Plant => icons::LEAF,
            Self::Grow => icons::PLANT,
            Self::View => icons::EYE,
            Self::Readings => icons::CHART_BAR,
            Self::Pictures => icons::IMAGE,
            Self::Spec => icons::BRACKETS_CURLY,
            Self::Bark => icons::TREE,
            Self::Keys => icons::QUESTION,
        }
    }

    /// Its word on the rail.
    #[must_use]
    pub const fn short(self) -> &'static str {
        match self {
            Self::Plant => "Plant",
            Self::Grow => "Grow",
            Self::View => "View",
            Self::Readings => "Readings",
            Self::Pictures => "Pictures",
            Self::Spec => "Spec",
            Self::Bark => "Bark",
            Self::Keys => "Keys",
        }
    }

    /// Its title over the panel.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Plant => "Plant",
            Self::Grow => "Growing",
            Self::View => "View and measures",
            Self::Readings => "Readings",
            Self::Pictures => "Pictures and animations",
            Self::Spec => "Spec",
            Self::Bark => "Bark",
            Self::Keys => "Keys",
        }
    }

    /// What it holds, for tooltips and the palette.
    #[must_use]
    pub const fn about(self) -> &'static str {
        match self {
            Self::Plant => "the species, its age and the day of the year",
            Self::Grow => "grow, stop and pause; showing the plant as it grows",
            Self::View => "level of detail, quality, look, view, grid and ruler",
            Self::Readings => "the plant's measures, parts, levels and file sizes",
            Self::Pictures => "snapshots, recorded growth and GIFs",
            Self::Spec => "the species' spec as the library merges it",
            Self::Bark => "the species' bark pattern and its numbers",
            Self::Keys => "every key",
        }
    }

    /// Its key: 1 to 7 for the rail's panels in order, F1 for the keys.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Plant => "1",
            Self::Grow => "2",
            Self::View => "3",
            Self::Readings => "4",
            Self::Pictures => "5",
            Self::Spec => "6",
            Self::Bark => "7",
            Self::Keys => "F1",
        }
    }

    /// Whether it may take a wide dock: a board of cards.
    #[must_use]
    pub const fn wide(self) -> bool {
        matches!(self, Self::Readings)
    }
}

/// Everything the window can be asked to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Grow this species, by its index in the library's list.
    Species(usize),
    Age(f64),
    Day(f64),
    Level(usize),
    Draft(bool),
    Look(Look),
    View(View),
    Grow,
    Stop,
    Pause(bool),
    GrowOnChange(bool),
    ShowGrowing(bool),
    Every(u32),
    Follow(bool),
    Grid(bool),
    Ruler(bool),
    ResetCamera,
    Snapshot,
    Record(bool),
    SaveGif,
    ClearFrames,
    PlantOnly(bool),
    PictureWidth(u32),
    FrameMs(u32),
    HoldS(f64),
    Pngs(bool),
    /// A rail button: open the panel alone, or `beside` the open ones;
    /// close it when it is open. In the Overlay it opens or closes the
    /// card.
    Panel(Panel, bool),
    Overlay(bool),
    /// Open the palette, its field filled with this.
    Palette(String),
    ClosePalette,
    Settings(bool),
    /// Print this to the console (standard output).
    Print(String),
}

/// The interface's own state and the commands it queued.
#[derive(Resource)]
pub struct Shell {
    /// The docked panels, left to right.
    pub open: Vec<Panel>,
    /// The Overlay: no bars, the plant fills the window.
    pub overlay: bool,
    /// The Overlay's floating card.
    pub card: Option<Panel>,
    pub palette: plantlab_ui::palette::Palette,
    pub palette_open: bool,
    pub palette_fill: Option<String>,
    pub settings_open: bool,
    pub settings: crate::settings::Settings,
    /// The species list's filter.
    pub filter: String,
    /// The 3D view: the window's rectangle left between the bars, the rail
    /// and the docks, logical points; and the points per pixel.
    pub view: Option<bevy_egui::egui::Rect>,
    pub pixels_per_point: f32,
    /// The bark swatch drawn for a species.
    pub bark: Option<(String, bevy_egui::egui::TextureHandle)>,
    pub queue: Vec<Command>,
}

impl Shell {
    #[must_use]
    pub fn new(settings: crate::settings::Settings) -> Self {
        Self {
            open: vec![Panel::Plant],
            overlay: false,
            card: None,
            palette: plantlab_ui::palette::Palette::default(),
            palette_open: false,
            palette_fill: None,
            settings_open: false,
            settings,
            filter: String::new(),
            view: None,
            pixels_per_point: 1.0,
            bark: None,
            queue: Vec::new(),
        }
    }

    /// Queue `command`.
    pub fn send(&mut self, command: Command) {
        self.queue.push(command);
    }

    /// Whether `panel` shows: docked, or the Overlay's card.
    #[must_use]
    pub fn shows(&self, panel: Panel) -> bool {
        if self.overlay {
            self.card == Some(panel)
        } else {
            self.open.contains(&panel)
        }
    }

    /// The rail button's command for `panel`: close it while it shows,
    /// else open it, beside the others with Shift held.
    pub fn toggle(&mut self, panel: Panel, beside: bool) {
        self.send(Command::Panel(panel, beside));
    }
}

/// Carry out the queued commands, in order.
pub fn apply(mut shell: ResMut<Shell>, mut lab: ResMut<Lab>, mut recording: ResMut<Recording>) {
    let queue = std::mem::take(&mut shell.queue);
    for command in queue {
        run(&mut shell, &mut lab, &mut recording, command);
    }
}

/// Carry out one command.
#[allow(clippy::too_many_lines)]
fn run(shell: &mut Shell, lab: &mut Lab, recording: &mut Recording, command: Command) {
    let growing = lab.growing.is_some();
    match command {
        Command::Species(index) => {
            if index < lab.species.len() && index != lab.chosen {
                lab.chosen = index;
                lab.age = None;
                lab.dirty = true;
                lab.reframe = true;
            }
        }
        Command::Age(age) => {
            if age > 0.0 && age.is_finite() {
                lab.age = Some(age);
                lab.dirty = true;
            }
        }
        Command::Day(day) => {
            lab.day = day.clamp(1.0, 365.0).round();
            lab.dirty = true;
        }
        Command::Level(level) => {
            lab.level = level.min(3);
            lab.dirty = true;
        }
        Command::Draft(draft) => {
            lab.draft = draft;
            lab.dirty = true;
        }
        Command::Look(look) => {
            lab.look = look;
            lab.dirty = true;
        }
        Command::View(view) => {
            lab.view = view;
            lab.reframe = true;
            lab.dirty = true;
        }
        Command::Grow => {
            if !growing {
                lab.go = true;
            }
        }
        Command::Stop => {
            lab.control.stop.store(true, Ordering::Relaxed);
            lab.dirty = false;
        }
        Command::Pause(pause) => lab.control.pause.store(pause, Ordering::Relaxed),
        Command::GrowOnChange(on) => {
            lab.auto = on;
            save(shell, Key::GrowOnChange, json!(on));
        }
        Command::ShowGrowing(on) => {
            lab.live = on;
            if growing {
                let every = if on { lab.every } else { 0 };
                lab.control.every.store(every, Ordering::Relaxed);
            }
            save(shell, Key::ShowGrowing, json!(on));
        }
        Command::Every(every) => {
            lab.every = every.clamp(1, 100);
            if growing && lab.live {
                lab.control.every.store(lab.every, Ordering::Relaxed);
            }
            save(shell, Key::Every, json!(lab.every));
        }
        Command::Follow(on) => {
            lab.follow = on;
            save(shell, Key::Follow, json!(on));
        }
        Command::Grid(on) => {
            lab.grid = on;
            save(shell, Key::Grid, json!(on));
        }
        Command::Ruler(on) => {
            lab.ruler = on;
            save(shell, Key::Ruler, json!(on));
        }
        Command::ResetCamera => lab.reset_camera = true,
        Command::Snapshot => {
            if let Some(shown) = &lab.shown {
                recording.ask(Take::Snapshot, record::stamped(&picture_name(&shown.scene)));
            }
        }
        Command::Record(on) => {
            recording.record = on;
            if on && let Some(shown) = &lab.shown {
                recording.ask(Take::Frame, picture_name(&shown.scene));
            }
        }
        Command::SaveGif => {
            if let Some(shown) = &lab.shown
                && !recording.frames.is_empty()
            {
                save_gif(recording, &shown.scene);
            }
        }
        Command::ClearFrames => recording.frames.clear(),
        Command::PlantOnly(on) => {
            recording.plant_only = on;
            save(shell, Key::PlantOnly, json!(on));
        }
        Command::PictureWidth(width) => {
            recording.width = width.clamp(128, 1920);
            save(shell, Key::PictureWidth, json!(recording.width));
        }
        Command::FrameMs(ms) => {
            let ms = ms.clamp(20, 2000);
            recording.delay = u16::try_from(ms / 10).unwrap_or(u16::MAX).max(2);
            save(shell, Key::FrameMs, json!(ms));
        }
        Command::HoldS(seconds) => {
            let seconds = seconds.clamp(0.0, 10.0);
            // At most 1000 hundredths.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let hundredths = (seconds * 100.0).round() as u16;
            recording.hold = hundredths;
            save(shell, Key::HoldS, json!(f64::from(hundredths) / 100.0));
        }
        Command::Pngs(on) => {
            recording.pngs = on;
            save(shell, Key::Pngs, json!(on));
        }
        Command::Panel(panel, beside) => {
            if shell.overlay {
                shell.card = if shell.card == Some(panel) {
                    None
                } else {
                    Some(panel)
                };
            } else if shell.open.contains(&panel) {
                shell.open.retain(|open| *open != panel);
            } else if beside {
                shell.open.push(panel);
            } else {
                shell.open = vec![panel];
            }
        }
        Command::Overlay(on) => {
            if on && !shell.overlay {
                shell.card = None;
            }
            shell.overlay = on;
        }
        Command::Palette(fill) => {
            shell.palette_open = true;
            shell.palette_fill = Some(fill);
        }
        Command::ClosePalette => shell.palette_open = false,
        Command::Settings(open) => shell.settings_open = open,
        Command::Print(text) => println!("{text}"),
    }
}

/// Save `key`, keeping the rest of the file; a refusal shows in the
/// status bar.
fn save(shell: &mut Shell, key: Key, value: serde_json::Value) {
    if shell.settings.path.is_some() {
        let _ = shell.settings.save(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_buttons_open_alone_beside_and_close() {
        let mut shell = Shell::new(crate::settings::Settings::default());
        let mut lab = super::super::tests::lab();
        let mut recording = Recording::new("pictures".into());
        let mut send = |shell: &mut Shell, command| run(shell, &mut lab, &mut recording, command);
        assert_eq!(shell.open, [Panel::Plant]);
        send(&mut shell, Command::Panel(Panel::View, false));
        assert_eq!(shell.open, [Panel::View]);
        send(&mut shell, Command::Panel(Panel::Readings, true));
        assert_eq!(shell.open, [Panel::View, Panel::Readings]);
        send(&mut shell, Command::Panel(Panel::View, false));
        assert_eq!(shell.open, [Panel::Readings]);
        // The Overlay keeps one card at a time and leaves the docks be.
        send(&mut shell, Command::Overlay(true));
        send(&mut shell, Command::Panel(Panel::Grow, true));
        assert_eq!(shell.card, Some(Panel::Grow));
        send(&mut shell, Command::Panel(Panel::Pictures, false));
        assert_eq!(shell.card, Some(Panel::Pictures));
        assert!(shell.shows(Panel::Pictures) && !shell.shows(Panel::Readings));
        send(&mut shell, Command::Overlay(false));
        assert!(shell.shows(Panel::Readings));
    }

    #[test]
    fn commands_change_what_grows_and_keep_values_in_range() {
        let mut shell = Shell::new(crate::settings::Settings::default());
        let mut lab = super::super::tests::lab();
        let mut recording = Recording::new("pictures".into());
        lab.dirty = false;
        for command in [
            Command::Day(400.0),
            Command::Level(9),
            Command::Every(0),
            Command::PictureWidth(10),
            Command::FrameMs(5),
        ] {
            run(&mut shell, &mut lab, &mut recording, command);
        }
        assert!((lab.day - 365.0).abs() < f64::EPSILON);
        assert_eq!(lab.level, 3);
        assert_eq!(lab.every, 1);
        assert_eq!(recording.width, 128);
        assert_eq!(recording.delay, 2);
        assert!(lab.dirty);
        run(&mut shell, &mut lab, &mut recording, Command::Species(1));
        assert_eq!(lab.chosen, 1);
        assert!(lab.age.is_none() && lab.reframe);
    }
}
