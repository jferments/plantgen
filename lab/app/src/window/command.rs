//! The window's command path. Every button, menu entry, palette entry and
//! key queues a [`Command`] on the [`Shell`]; [`apply`] carries them out in
//! order, once a frame, and is the only place that changes what grows,
//! what shows and what is saved. The interface itself only reads, apart
//! from its own state: the browser's search, filter and open groups.
//!
//! A command that changes a saved setting saves that one setting
//! ([`crate::settings`]).

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use bevy::prelude::*;
use plantgen::spec::Environment;
use plantlab_scene::{Look, View};
use plantlab_ui::icons;
use serde_json::json;

use super::catalogue::Form;
use super::readings::{self, Readings};
use super::store::Detail;
use super::{Lab, picture_name, save_gif};
use crate::record::{self, Recording, Take};
use crate::settings::Key;

/// The inspector's pages, each a tab at its head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Render,
    Readings,
    Pictures,
    Spec,
    Bark,
    Keys,
}

impl Panel {
    /// The tabs, left to right.
    pub const ALL: [Self; 6] = [
        Self::Render,
        Self::Readings,
        Self::Pictures,
        Self::Spec,
        Self::Bark,
        Self::Keys,
    ];

    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Render => icons::SLIDERS_HORIZONTAL,
            Self::Readings => icons::CHART_BAR,
            Self::Pictures => icons::IMAGE,
            Self::Spec => icons::BRACKETS_CURLY,
            Self::Bark => icons::TREE,
            Self::Keys => icons::KEYBOARD,
        }
    }

    /// Its word on the tab.
    #[must_use]
    pub const fn short(self) -> &'static str {
        match self {
            Self::Render => "Render",
            Self::Readings => "Readings",
            Self::Pictures => "Pictures",
            Self::Spec => "Spec",
            Self::Bark => "Bark",
            Self::Keys => "Keys",
        }
    }

    /// What it holds, for tooltips and the palette.
    #[must_use]
    pub const fn about(self) -> &'static str {
        match self {
            Self::Render => "the plant, its season and detail, and Generate",
            Self::Readings => "the plant's measures, parts, levels and file sizes",
            Self::Pictures => "snapshots, recorded growth and GIFs",
            Self::Spec => "the species' spec as the library merges it",
            Self::Bark => "the species' bark pattern and its numbers",
            Self::Keys => "every key",
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
    /// Choose this species, by id; nothing grows until Generate.
    Choose(String),
    Age(f64),
    Day(f64),
    Detail(Detail),
    Level(usize),
    /// The site it grows on; `None` for its typical one.
    Site(Option<Environment>),
    /// Its variant's seed; `None` for the spec's first.
    Seed(Option<u64>),
    /// How it is lit and finished; changes the plant on screen at once.
    Look(Look),
    /// Where the camera stands; changes the plant on screen at once.
    View(View),
    /// Grow the plant as the settings say, or draw it again when only how
    /// it is drawn changed.
    Generate,
    Stop,
    Pause(bool),
    /// Play the year: draw the plant in each month in turn.
    PlayYear(bool),
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
    /// Show this page of the inspector, opening it.
    Tab(Panel),
    Library(bool),
    Inspector(bool),
    Overlay(bool),
    /// Open the palette, its field filled with this.
    Palette(String),
    ClosePalette,
    Settings(bool),
    /// Print this to the console (standard output).
    Print(String),
    /// Hold the Readings still, or follow them again.
    Freeze(bool),
    /// Print the Readings' numbers to the console.
    DumpReadings,
}

/// The interface's own state and the commands it queued.
#[derive(Resource)]
#[allow(clippy::struct_excessive_bools, reason = "what shows")]
pub struct Shell {
    /// The inspector's page.
    pub tab: Panel,
    /// The library at the left and the inspector at the right show.
    pub library: bool,
    pub inspector: bool,
    /// The Overlay: no bars or docks, the plant fills the window.
    pub overlay: bool,
    pub palette: plantlab_ui::palette::Palette,
    pub palette_open: bool,
    pub palette_fill: Option<String>,
    pub settings_open: bool,
    pub settings: crate::settings::Settings,
    /// The browser's search.
    pub filter: String,
    /// The browser's growth form filter; `None` for every form.
    pub form: Option<Form>,
    /// The browser's open groups, orders, families and genera, as
    /// `rank:name`.
    pub expanded: BTreeSet<String>,
    /// Open the chosen species' groups in the browser and scroll to it.
    pub reveal: bool,
    /// The 3D view: the window's rectangle left between the bars and the
    /// docks, logical points; and the points per pixel.
    pub view: Option<bevy_egui::egui::Rect>,
    pub pixels_per_point: f32,
    /// The bark swatch drawn for a species.
    pub bark: Option<(String, bevy_egui::egui::TextureHandle)>,
    /// The kept renders' pictures loaded, by species: the write they
    /// show, and the picture (`None` where it does not load).
    pub pictures: std::collections::HashMap<String, (u64, Option<bevy_egui::egui::TextureHandle>)>,
    /// The Readings board's width class, kept for its hysteresis.
    pub readings_width: plantlab_ui::board::Width,
    pub queue: Vec<Command>,
}

impl Shell {
    #[must_use]
    pub fn new(settings: crate::settings::Settings) -> Self {
        let library = settings.bool(Key::LibraryOpen).unwrap_or(true);
        Self {
            tab: Panel::Render,
            library,
            inspector: true,
            overlay: false,
            palette: plantlab_ui::palette::Palette::default(),
            palette_open: false,
            palette_fill: None,
            settings_open: false,
            settings,
            filter: String::new(),
            form: None,
            expanded: BTreeSet::new(),
            reveal: false,
            view: None,
            pixels_per_point: 1.0,
            bark: None,
            pictures: std::collections::HashMap::new(),
            readings_width: plantlab_ui::board::Width::default(),
            queue: Vec::new(),
        }
    }

    /// Queue `command`.
    pub fn send(&mut self, command: Command) {
        self.queue.push(command);
    }

    /// Whether `panel` shows: the inspector's page, outside the Overlay.
    #[must_use]
    pub fn shows(&self, panel: Panel) -> bool {
        !self.overlay && self.inspector && self.tab == panel
    }
}

/// Carry out the queued commands, in order.
pub fn apply(
    mut shell: ResMut<Shell>,
    mut lab: ResMut<Lab>,
    mut recording: ResMut<Recording>,
    mut readings: ResMut<Readings>,
) {
    let queue = std::mem::take(&mut shell.queue);
    for command in queue {
        match command {
            Command::Freeze(frozen) => readings.freeze(frozen),
            Command::DumpReadings => print!("{}", readings::dump(&readings)),
            command => run(&mut shell, &mut lab, &mut recording, command),
        }
    }
}

/// Carry out one command.
#[allow(clippy::too_many_lines)]
fn run(shell: &mut Shell, lab: &mut Lab, recording: &mut Recording, command: Command) {
    let growing = lab.growing.is_some();
    match command {
        Command::Choose(id) => {
            if lab.catalogue.get(&id).is_some() && id != lab.chosen {
                lab.year = None;
                lab.choose(&id);
                shell.reveal = true;
                save(shell, Key::Species, json!(id));
                let recent = json!(lab.recent);
                save(shell, Key::Recent, recent);
            }
        }
        Command::Age(age) => {
            if age > 0.0 && age.is_finite() {
                lab.recipe.age = Some(age);
            }
        }
        Command::Day(day) => {
            lab.recipe.day = day.clamp(1.0, 365.0).round();
        }
        Command::Detail(detail) => {
            lab.recipe.detail = detail;
            if detail.parts() {
                lab.recipe.level = 0;
            }
        }
        Command::Level(level) => {
            lab.recipe.level = level.min(3);
            if level > 0 && lab.recipe.detail.parts() {
                lab.recipe.detail = Detail::Standard;
            }
        }
        Command::Site(site) => lab.recipe.environment = site,
        Command::Seed(seed) => lab.recipe.seed = seed,
        Command::Look(look) => {
            lab.recipe.look = look;
            lab.restyle = lab.shown.is_some();
        }
        Command::View(view) => {
            lab.recipe.view = view;
            lab.refit = true;
            if let Some(shown) = &mut lab.shown {
                shown.recipe.view = view;
                shown.shot.view = view;
            }
        }
        Command::Generate => {
            if !growing {
                lab.go = true;
            }
        }
        Command::Stop => {
            lab.control.stop.store(true, Ordering::Relaxed);
            lab.year = None;
        }
        Command::Pause(pause) => lab.control.pause.store(pause, Ordering::Relaxed),
        Command::PlayYear(on) => {
            lab.year = (on && lab.about.error.is_empty()).then_some(0);
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
        Command::Tab(panel) => {
            shell.tab = panel;
            shell.inspector = true;
            shell.overlay = false;
        }
        Command::Library(on) => {
            shell.library = on;
            if on {
                shell.overlay = false;
                shell.reveal = true;
            }
            save(shell, Key::LibraryOpen, json!(on));
        }
        Command::Inspector(on) => {
            shell.inspector = on;
            if on {
                shell.overlay = false;
            }
        }
        Command::Overlay(on) => shell.overlay = on,
        Command::Palette(fill) => {
            shell.palette_open = true;
            shell.palette_fill = Some(fill);
        }
        Command::ClosePalette => shell.palette_open = false,
        Command::Settings(open) => shell.settings_open = open,
        Command::Print(text) => println!("{text}"),
        // Carried out by `apply`, which holds the readings.
        Command::Freeze(_) | Command::DumpReadings => {}
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

    fn shell() -> Shell {
        Shell::new(crate::settings::Settings::default())
    }

    #[test]
    fn tabs_open_the_inspector_and_the_overlay_hides_it() {
        let mut shell = shell();
        let mut lab = super::super::tests::lab();
        let mut recording = Recording::new("pictures".into());
        let mut send = |shell: &mut Shell, command| run(shell, &mut lab, &mut recording, command);
        assert!(shell.shows(Panel::Render));
        send(&mut shell, Command::Inspector(false));
        assert!(!shell.shows(Panel::Render));
        send(&mut shell, Command::Tab(Panel::Readings));
        assert!(shell.inspector && shell.shows(Panel::Readings));
        send(&mut shell, Command::Overlay(true));
        assert!(!shell.shows(Panel::Readings));
        send(&mut shell, Command::Library(true));
        assert!(!shell.overlay && shell.library);
    }

    #[test]
    fn choosing_and_settings_never_grow_until_generate() {
        let mut shell = shell();
        let mut lab = super::super::tests::lab();
        let mut recording = Recording::new("pictures".into());
        for command in [
            Command::Choose("rosa-nutkana".into()),
            Command::Day(400.0),
            Command::Level(9),
            Command::Seed(Some(4)),
            Command::Site(Some(Environment::Edge)),
            Command::Every(0),
            Command::PictureWidth(10),
            Command::FrameMs(5),
            Command::View(View::Top),
        ] {
            run(&mut shell, &mut lab, &mut recording, command);
        }
        assert!(!lab.go && lab.growing.is_none());
        assert_eq!(lab.chosen, "rosa-nutkana");
        assert!((lab.recipe.day - 365.0).abs() < f64::EPSILON);
        assert_eq!(lab.recipe.level, 3);
        assert_eq!(lab.recipe.seed, Some(4));
        assert_eq!(lab.recipe.environment, Some(Environment::Edge));
        assert_eq!(lab.recipe.view, View::Top);
        assert!(lab.refit, "the view changes at once");
        assert_eq!(lab.every, 1);
        assert_eq!(recording.width, 128);
        assert_eq!(recording.delay, 2);
        run(&mut shell, &mut lab, &mut recording, Command::Generate);
        assert!(lab.go);
    }

    #[test]
    fn full_detail_is_the_nearest_level() {
        let mut shell = shell();
        let mut lab = super::super::tests::lab();
        let mut recording = Recording::new("pictures".into());
        run(&mut shell, &mut lab, &mut recording, Command::Level(2));
        run(
            &mut shell,
            &mut lab,
            &mut recording,
            Command::Detail(Detail::Full),
        );
        assert_eq!(lab.recipe.level, 0);
        run(&mut shell, &mut lab, &mut recording, Command::Level(1));
        assert_eq!(lab.recipe.detail, Detail::Standard);
    }
}
