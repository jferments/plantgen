//! The Readings panel: a board of cards ([`plantlab_ui::board`]), as
//! WorldLab's Diagnostics pages are:
//!
//! - **The plant**: its height, crown, age, triangles and cards;
//! - **Parts**: the triangles and cards of the wood, leaves, flowers,
//!   fruit and the rest, as stacked bars;
//! - **As files**: how big it is packaged and exported, on a log scale;
//! - **Levels**: the triangles, cards and packaged size of each level of
//!   detail, the plant's growth dressed at each on a worker;
//! - **Growth**: its height and crown at each frame shown as it grew;
//! - **Growing time**: each growth step's time, and how they spread;
//! - **Drawing**: the window's frame times over the last ten seconds, and
//!   how they spread.
//!
//! The panel gathers only while it shows ([`gather`]): the frame times, the
//! growth's frames and its steps' times, and the levels. Freeze holds a
//! copy of what it gathered still while you read it, and stops gathering;
//! Dump to the console prints the page's numbers ([`dump`]). Every card's
//! place comes from the panel's width alone.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::egui::{self, Align, Align2, Color32, Layout, Pos2, Rect, RichText, Ui, pos2, vec2};
use plantgen::library::Library;
use plantlab_scene::{Facts, PartCount, PartKind};
use plantlab_ui::board::{self, Arrangement, Arrangements, Head, Place, Width};
use plantlab_ui::canvas::{self, Pen};
use plantlab_ui::figures::{self, Part};
use plantlab_ui::{icons, theme};

use super::command::{Command, Panel, Shell};
use super::{Lab, bytes};

/// How far back the frame times go, seconds.
pub const WINDOW_S: f64 = 10.0;
/// The most step times kept for one growth.
const MOST_STEPS: usize = 20_000;
/// One frame at 60 a second, ms.
const SIXTY_HZ_MS: f32 = 1_000.0 / 60.0;

/// One growth step's time, and whether a frame of the plant was drawn
/// with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepTime {
    pub ms: f32,
    pub framed: bool,
}

/// What the cards draw: gathered live, or a copy held by Freeze.
#[derive(Debug, Clone, Default)]
pub struct Gathered {
    /// Seconds since the window opened, when this was gathered.
    pub now: f64,
    /// The window's frames: when, and how long each took, ms.
    pub frames: VecDeque<[f64; 2]>,
    /// The growth's frames shown: age, height and crown width.
    pub growth: Vec<[f64; 3]>,
    pub steps: Vec<StepTime>,
    /// The plant on screen.
    pub plant: Option<Plant>,
    pub levels: Levels,
}

/// The plant on screen, as the cards read it.
#[derive(Debug, Clone)]
pub struct Plant {
    pub facts: Facts,
    pub parts: Vec<PartCount>,
    pub seconds: f64,
    pub live: bool,
}

/// The plant's levels of detail.
#[derive(Debug, Clone, Default)]
pub enum Levels {
    #[default]
    None,
    /// Being drawn on a worker, for the plant shown `of`.
    Working(u64),
    /// Each level's facts, nearest first, for the plant shown `of`.
    Done(u64, Result<Vec<Facts>, String>),
}

/// Where the levels' worker puts them.
type LevelsSlot = Arc<Mutex<Option<Result<Vec<Facts>, String>>>>;

/// What the panel gathered, and what it is doing.
#[derive(Resource)]
pub struct Readings {
    opened: Instant,
    pub live: Gathered,
    /// The copy Freeze holds.
    pub frozen: Option<Gathered>,
    /// The growth and plant shown the figures were last gathered for.
    growth_of: u64,
    point_of: u64,
    /// The worker drawing the levels and where it puts them.
    worker: Option<(u64, LevelsSlot)>,
}

impl Default for Readings {
    fn default() -> Self {
        Self {
            opened: Instant::now(),
            live: Gathered::default(),
            frozen: None,
            growth_of: 0,
            point_of: 0,
            worker: None,
        }
    }
}

impl Readings {
    /// What the cards draw now.
    #[must_use]
    pub fn shown(&self) -> &Gathered {
        self.frozen.as_ref().unwrap_or(&self.live)
    }

    /// Freezes what the cards show, or follows the readings again.
    pub fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen.then(|| self.live.clone());
    }
}

/// Gathers what the page draws, while it shows and is not frozen.
#[allow(clippy::needless_pass_by_value)]
pub fn gather(mut readings: ResMut<Readings>, lab: Res<Lab>, shell: Res<Shell>, time: Res<Time>) {
    let showing = shell.shows(Panel::Readings) && readings.frozen.is_none();
    lab.control.gather.store(showing, Ordering::Relaxed);
    if !showing {
        return;
    }
    let readings = &mut *readings;
    let now = readings.opened.elapsed().as_secs_f64();
    let live = &mut readings.live;
    live.now = now;
    live.frames
        .push_back([now, time.delta_secs_f64() * 1_000.0]);
    while live
        .frames
        .front()
        .is_some_and(|[at, _]| now - at > WINDOW_S + 1.0)
    {
        live.frames.pop_front();
    }
    if lab.growths != readings.growth_of {
        readings.growth_of = lab.growths;
        live.growth.clear();
        live.steps.clear();
    }
    if let Ok(mut times) = lab.control.step_times.lock() {
        let room = MOST_STEPS.saturating_sub(live.steps.len());
        live.steps.extend(times.drain(..).take(room));
    }
    let Some(shown) = &lab.shown else {
        return;
    };
    if readings.point_of != lab.shows {
        readings.point_of = lab.shows;
        let facts = &shown.scene.facts;
        live.growth
            .push([facts.age, facts.height_m, facts.crown_width_m]);
        live.plant = Some(Plant {
            facts: facts.clone(),
            parts: shown.scene.parts.clone(),
            seconds: shown.seconds,
            live: shown.live,
        });
    }
    // The levels of a grown plant, drawn on a worker from its growth.
    let of = lab.shows;
    if let Some((worker_of, slot)) = &readings.worker {
        if *worker_of != of {
            readings.worker = None;
        } else if let Some(result) = slot.lock().ok().and_then(|mut slot| slot.take()) {
            live.levels = Levels::Done(of, result);
            readings.worker = None;
        }
    }
    let current = match &live.levels {
        Levels::Working(level_of) | Levels::Done(level_of, _) => *level_of == of,
        Levels::None => false,
    };
    if !current
        && !shown.live
        && let Some(growth) = shown.scene.growth.clone()
    {
        let slot = Arc::new(Mutex::new(None));
        let put = Arc::clone(&slot);
        let asked = shown.shot.clone();
        std::thread::spawn(move || {
            let result = plantlab_scene::levels(&asked, Library::builtin(), &growth);
            if let Ok(mut slot) = put.lock() {
                *slot = Some(result);
            }
        });
        readings.worker = Some((of, slot));
        live.levels = Levels::Working(of);
    }
}

/// The page's numbers as text, for the console.
#[must_use]
pub fn dump(readings: &Readings) -> String {
    let data = readings.shown();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "readings{}",
        if readings.frozen.is_some() {
            " (frozen)"
        } else {
            ""
        }
    );
    if let Some(plant) = &data.plant {
        let facts = &plant.facts;
        let _ = writeln!(
            text,
            "plant {} {:.1} y day {:.0} level {} {}: {:.2} m tall, {:.2} m across, {} triangles, \
             {} cards, grown in {:.1} s",
            facts.species,
            facts.age,
            facts.day,
            facts.level,
            facts.quality,
            facts.height_m,
            facts.crown_width_m,
            facts.triangles,
            facts.cards,
            plant.seconds
        );
        for part in &plant.parts {
            let _ = writeln!(
                text,
                "part {}: {} triangles, {} cards",
                part.kind.name(),
                part.triangles,
                part.cards
            );
        }
        let export = facts.export;
        let _ = writeln!(
            text,
            "files: packaged {}, textures {}, .glb about {}, .obj about {}",
            bytes(export.package_mesh),
            bytes(export.textures_png),
            bytes(export.glb),
            bytes(export.obj)
        );
    }
    if let Levels::Done(_, Ok(levels)) = &data.levels {
        for facts in levels {
            let _ = writeln!(
                text,
                "level {}: {} triangles, {} cards, packaged {}",
                facts.level,
                facts.triangles,
                facts.cards,
                bytes(facts.export.package_mesh)
            );
        }
    }
    for [age, height, crown] in &data.growth {
        let _ = writeln!(
            text,
            "growth {age:.2} y: {height:.2} m tall, {crown:.2} m across"
        );
    }
    let steps: Vec<f32> = data.steps.iter().map(|step| step.ms).collect();
    if let Some((p50, p99)) = spread_of(&steps) {
        let _ = writeln!(
            text,
            "growing: {} steps, p50 {p50:.2} ms, p99 {p99:.2} ms, total {:.2} s",
            steps.len(),
            steps.iter().sum::<f32>() / 1_000.0
        );
    }
    let frames = recent_frames(data);
    if let Some((p50, p99)) = spread_of(&frames) {
        let _ = writeln!(
            text,
            "drawing: {} frames in {WINDOW_S:.0} s, p50 {p50:.2} ms, p99 {p99:.2} ms",
            frames.len()
        );
    }
    text
}

/// The frame times of the last [`WINDOW_S`], ms.
fn recent_frames(data: &Gathered) -> Vec<f32> {
    #[allow(clippy::cast_possible_truncation)]
    data.frames
        .iter()
        .filter(|[at, _]| data.now - at <= WINDOW_S)
        .map(|[_, ms]| *ms as f32)
        .collect()
}

/// The median and the 99th percentile of `values`.
#[must_use]
pub fn spread_of(values: &[f32]) -> Option<(f32, f32)> {
    let mut sorted: Vec<f32> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f32::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let at = |share: f32| sorted[((sorted.len() - 1) as f32 * share).round() as usize];
    Some((at(0.5), at(0.99)))
}

/// A part's series colour: its fixed slot.
#[must_use]
pub const fn colour(kind: PartKind) -> Color32 {
    match kind {
        PartKind::Wood => theme::SERIES[0],
        PartKind::Fruit => theme::SERIES[1],
        PartKind::Leaves => theme::SERIES[2],
        PartKind::Flowers => theme::SERIES[4],
        PartKind::Other => theme::SERIES[6],
    }
}

/// What every card reads.
struct Page<'a> {
    lab: &'a Lab,
    data: &'a Gathered,
    pen: Pen,
}

type Card = fn(&Ui, &Page, Rect);

const CARDS: [Card; 7] = [plant, parts, files, levels, growth, growing, drawing];

/// Where the cards go at each width.
const ARRANGEMENTS: Arrangements = Arrangements {
    wide: Arrangement {
        rows: &[176.0, 230.0, 250.0],
        places: &[
            Place::at(0, 4, 0),
            Place::at(4, 8, 0),
            Place::at(8, 12, 0),
            Place::at(6, 12, 1),
            Place::at(0, 6, 1),
            Place::at(0, 7, 2),
            Place::at(7, 12, 2),
        ],
    },
    medium: Arrangement {
        rows: &[176.0, 200.0, 230.0, 240.0, 240.0],
        places: &[
            Place::at(0, 6, 0),
            Place::at(0, 12, 1),
            Place::at(6, 12, 0),
            Place::at(6, 12, 2),
            Place::at(0, 6, 2),
            Place::at(0, 12, 3),
            Place::at(0, 12, 4),
        ],
    },
    narrow: Arrangement {
        rows: &[176.0, 200.0, 190.0, 210.0, 240.0, 250.0, 240.0],
        places: &[
            Place::at(0, 12, 0),
            Place::at(0, 12, 1),
            Place::at(0, 12, 2),
            Place::at(0, 12, 3),
            Place::at(0, 12, 4),
            Place::at(0, 12, 5),
            Place::at(0, 12, 6),
        ],
    },
};

/// The panel: its header, then the board.
pub fn panel(ui: &mut Ui, lab: &Lab, readings: &Readings, shell: &mut Shell) {
    header(ui, readings, shell);
    ui.add_space(theme::GAP);
    let width = Width::of(ui.available_width(), shell.readings_width);
    shell.readings_width = width;
    let room = ui.clip_rect().bottom() - ui.cursor().top();
    let page = Page {
        lab,
        data: readings.shown(),
        pen: Pen::of(ui),
    };
    let rects = board::allocate(ui, ARRANGEMENTS.at(width), Some(room));
    for (rect, card) in rects.into_iter().zip(CARDS) {
        if ui.is_rect_visible(rect) {
            card(ui, &page, rect);
        }
    }
}

/// Live or frozen, Freeze or Resume, and Dump to the console.
fn header(ui: &mut Ui, readings: &Readings, shell: &mut Shell) {
    ui.horizontal(|ui| {
        let (dot, words) = match &readings.frozen {
            Some(frozen) => (theme::STATUS[1], format!("Frozen at {:.1} s", frozen.now)),
            None => (theme::STATUS[0], "Live".to_owned()),
        };
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, dot);
        ui.label(
            RichText::new(words)
                .font(theme::strong_font(ui, theme::SMALL))
                .color(theme::TEXT_2),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(super::panels::with_icon(
                    ui,
                    icons::TERMINAL_WINDOW,
                    "Dump to the console",
                    theme::TEXT,
                ))
                .on_hover_text("Print this page's numbers in the console")
                .clicked()
            {
                shell.send(Command::DumpReadings);
            }
            let (icon, label, tip) = if readings.frozen.is_some() {
                (icons::PLAY, "Resume", "Follow the readings again")
            } else {
                (
                    icons::PAUSE,
                    "Freeze",
                    "Hold this page still while you read it; it stops gathering",
                )
            };
            if ui
                .button(super::panels::with_icon(ui, icon, label, theme::TEXT))
                .on_hover_text(tip)
                .clicked()
            {
                shell.send(Command::Freeze(readings.frozen.is_none()));
            }
        });
    });
}

/// A stat painted at `at` (its top left): the value large with its unit
/// beside it, and its name under it.
fn stat_at(painter: &egui::Painter, pen: Pen, at: Pos2, (value, unit): (&str, &str), name: &str) {
    let line = canvas::value_unit(
        painter,
        pos2(at.x, at.y + 12.0),
        value,
        pen.mono(20.0),
        unit,
        pen.regular(11.5),
    );
    canvas::text(
        painter,
        pos2(at.x, line.bottom() + 1.0),
        Align2::LEFT_TOP,
        name,
        pen.regular(11.0),
        theme::TEXT_3,
    );
}

/// A line in the faintest ink at `at`, cut to `width`.
fn note_at(painter: &egui::Painter, pen: Pen, at: Pos2, width: f32, words: &str) {
    canvas::text_within(
        painter,
        at,
        Align2::LEFT_TOP,
        words,
        pen.regular(11.5),
        theme::TEXT_3,
        width,
    );
}

/// How wide a legend of square swatches for `entries` is at the right
/// end of a card's heading, or 0 when it would crowd the title.
fn legend_width(ui: &Ui, pen: Pen, rect: Rect, entries: &[(Color32, &str)]) -> f32 {
    let painter = ui.painter();
    let font = pen.regular(canvas::LABEL);
    let width: f32 = entries
        .iter()
        .map(|(_, name)| canvas::legend_width(painter, name, "", &font) + 12.0)
        .sum();
    let head = rect.width() - 2.0 * board::PADDING.x;
    if width > head * 0.55 { 0.0 } else { width }
}

/// A card with `entries` as a legend at the right of its heading: the
/// card first, so the legend lies on it.
fn card_with_legend(
    ui: &Ui,
    pen: Pen,
    rect: Rect,
    head: &Head,
    entries: &[(Color32, &str)],
) -> board::Room {
    let width = legend_width(ui, pen, rect, entries);
    let room = board::card(ui, rect, head, width);
    if width > 0.0 {
        let head = Rect::from_min_size(
            rect.min + board::PADDING,
            vec2(rect.width() - 2.0 * board::PADDING.x, board::HEAD_HEIGHT),
        );
        let painter = board::painter_in(ui, head);
        let font = pen.regular(canvas::LABEL);
        let mut x = head.right() - width;
        for (colour, name) in entries {
            x = canvas::legend_entry(
                &painter,
                pos2(x, head.center().y),
                *colour,
                name,
                "",
                font.clone(),
            ) + 12.0;
        }
    }
    room
}

/// The card's body with nothing to show yet: one line saying why.
fn waiting(ui: &Ui, pen: Pen, body: Rect, words: &str) {
    let painter = board::painter_in(ui, body);
    note_at(
        &painter,
        pen,
        body.min + vec2(0.0, 4.0),
        body.width(),
        words,
    );
}

/// A count with thin spaces between thousands.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push('\u{2009}');
        }
        text.push(digit);
    }
    text
}

fn plant(ui: &Ui, page: &Page, rect: Rect) {
    let room = board::card(
        ui,
        rect,
        &Head {
            icon: icons::PLANT,
            title: "The plant",
            about: "as drawn on screen",
        },
        0.0,
    );
    let body = room.body;
    let Some(plant) = &page.data.plant else {
        waiting(ui, page.pen, body, "Nothing grown yet.");
        return;
    };
    let painter = board::painter_in(ui, body);
    let facts = &plant.facts;
    let height = format!("{:.2}", facts.height_m);
    let crown = format!("{:.2}", facts.crown_width_m);
    let age = format!("{:.1}", facts.age);
    let triangles = thousands(facts.triangles);
    let cards = thousands(facts.cards);
    let grown = format!("{:.1}", plant.seconds);
    let stats: [((&str, &str), &str); 6] = [
        ((&height, "m"), "tall"),
        ((&crown, "m"), "across"),
        ((&age, "y"), "old"),
        ((&triangles, ""), "triangles"),
        ((&cards, ""), "cards"),
        (
            (&grown, "s"),
            if plant.live { "growing" } else { "to grow" },
        ),
    ];
    let columns = if body.width() >= 300.0 { 3 } else { 2 };
    #[allow(clippy::cast_precision_loss)]
    let step = body.width() / columns as f32;
    for (index, (value, name)) in stats.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let at = pos2(
            body.left() + (index % columns) as f32 * step,
            body.top() + (index / columns) as f32 * 46.0,
        );
        stat_at(&painter, page.pen, at, *value, name);
    }
    note_at(
        &painter,
        page.pen,
        pos2(body.left(), body.bottom() - 14.0),
        body.width(),
        &format!(
            "level {} · {} · day {:.0} · generator revision {}",
            facts.level, facts.quality, facts.day, facts.generator_revision
        ),
    );
}

fn parts(ui: &Ui, page: &Page, rect: Rect) {
    let shown: Vec<PartCount> = page
        .data
        .plant
        .as_ref()
        .map(|plant| {
            plant
                .parts
                .iter()
                .copied()
                .filter(|part| part.triangles > 0 || part.cards > 0)
                .collect()
        })
        .unwrap_or_default();
    let legend: Vec<(Color32, &str)> = shown
        .iter()
        .map(|part| (colour(part.kind), part.kind.name()))
        .collect();
    let room = card_with_legend(
        ui,
        page.pen,
        rect,
        &Head {
            icon: icons::LEAF,
            title: "Parts",
            about: "the triangles and cards of each part",
        },
        &legend,
    );
    let body = room.body;
    if shown.is_empty() {
        waiting(ui, page.pen, body, "Nothing grown yet.");
        return;
    }
    let painter = board::painter_in(ui, body);
    let pen = page.pen;
    let font = pen.regular(canvas::MARK);
    let label_width = 72.0;
    let bar_height = 22.0;
    #[allow(clippy::type_complexity)]
    let rows: [(&str, fn(&PartCount) -> usize); 2] = [
        ("triangles", |part| part.triangles),
        ("cards", |part| part.cards),
    ];
    for (row, (name, count)) in rows.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let top = body.top() + 6.0 + row as f32 * (bar_height + 30.0);
        let total: usize = shown.iter().map(count).sum();
        canvas::text(
            &painter,
            pos2(body.left(), top + bar_height / 2.0),
            Align2::LEFT_CENTER,
            name,
            pen.regular(11.5),
            theme::TEXT_2,
        );
        let bar = Rect::from_min_max(
            pos2(body.left() + label_width, top),
            pos2(body.right(), top + bar_height),
        );
        #[allow(clippy::cast_precision_loss)]
        let marks: Vec<Part> = shown
            .iter()
            .map(|part| {
                Part::new(
                    part.kind.name(),
                    count(part) as f32,
                    colour(part.kind),
                    thousands(count(part)),
                )
            })
            .collect();
        let spans = figures::stacked_bar(&painter, bar, &marks, 0.0, None, Some(&font));
        figures::hover_parts(ui, bar, egui::Id::new(("parts", row)), &spans, &marks);
        canvas::text(
            &painter,
            pos2(bar.left(), bar.bottom() + 4.0),
            Align2::LEFT_TOP,
            &format!("{} in all", thousands(total)),
            pen.regular(11.0),
            theme::TEXT_3,
        );
    }
}

/// The files' scale: 10 KiB to 1 GiB.
const FILE_LOW: f32 = 10.0 * 1_024.0;
const FILE_HIGH: f32 = 1_024.0 * 1_024.0 * 1_024.0;

fn files(ui: &Ui, page: &Page, rect: Rect) {
    let room = board::card(
        ui,
        rect,
        &Head {
            icon: icons::FILE,
            title: "As files",
            about: "this level packaged and exported, log scale",
        },
        0.0,
    );
    let body = room.body;
    let Some(plant) = &page.data.plant else {
        waiting(ui, page.pen, body, "Nothing grown yet.");
        return;
    };
    let painter = board::painter_in(ui, body);
    let pen = page.pen;
    let export = plant.facts.export;
    let rows = [
        ("packaged", export.package_mesh, ""),
        ("textures", export.textures_png, ""),
        (".glb", export.glb, "about "),
        (".obj", export.obj, "about "),
    ];
    let label_width = 64.0;
    let value_width = 88.0;
    let plot = Rect::from_min_max(
        pos2(body.left() + label_width, body.top()),
        pos2(body.right() - value_width, body.bottom() - 16.0),
    );
    if plot.width() < 30.0 {
        return;
    }
    // Every power of ten bytes from 10 KiB, as the axis.
    for (index, tick) in ["10 KiB", "100 KiB", "1 MiB", "10 MiB", "100 MiB", "1 GiB"]
        .iter()
        .enumerate()
    {
        #[allow(clippy::cast_precision_loss)]
        let x = plot.left() + plot.width() * index as f32 / 5.0;
        canvas::line(
            &painter,
            pos2(x, plot.top()),
            pos2(x, plot.bottom()),
            1.0,
            theme::GRID,
        );
        if plot.width() >= 260.0 || index % 2 == 0 {
            canvas::text(
                &painter,
                pos2(x, plot.bottom() + 3.0),
                match index {
                    0 => Align2::LEFT_TOP,
                    5 => Align2::RIGHT_TOP,
                    _ => Align2::CENTER_TOP,
                },
                tick,
                pen.regular(canvas::AXIS),
                theme::TEXT_3,
            );
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let line = plot.height() / rows.len() as f32;
    for (index, (name, size, about)) in rows.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let middle = plot.top() + line * (index as f32 + 0.5);
        canvas::text(
            &painter,
            pos2(body.left(), middle),
            Align2::LEFT_CENTER,
            name,
            pen.regular(11.5),
            theme::TEXT_2,
        );
        #[allow(clippy::cast_precision_loss)]
        let share = figures::log_scale(*size as f32, FILE_LOW / 10.0, FILE_HIGH);
        // The bar starts a decade under the axis's first tick so the
        // smallest sizes still show.
        let start = plot.left() - plot.width() / 5.0;
        let end = (start + share * plot.width() * 1.2).max(plot.left() + 2.0);
        let bar = Rect::from_min_max(
            pos2(plot.left(), middle - 5.0),
            pos2(end.min(plot.right()), middle + 5.0),
        );
        canvas::block(&painter, bar, figures::round_for(10.0), theme::HEAT[4]);
        canvas::text(
            &painter,
            pos2(body.right(), middle),
            Align2::RIGHT_CENTER,
            &format!("{about}{}", bytes(*size)),
            pen.mono(11.5),
            theme::TEXT,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn levels(ui: &Ui, page: &Page, rect: Rect) {
    let room = board::card(
        ui,
        rect,
        &Head {
            icon: icons::STACK,
            title: "Levels",
            about: "the plant at each level of detail, as a package holds it",
        },
        0.0,
    );
    let body = room.body;
    let pen = page.pen;
    let rows = match &page.data.levels {
        Levels::None => {
            waiting(
                ui,
                pen,
                body,
                "The levels are drawn once a plant has grown.",
            );
            return;
        }
        Levels::Working(_) => {
            waiting(ui, pen, body, "Drawing the plant at each level…");
            return;
        }
        Levels::Done(_, Err(error)) => {
            waiting(ui, pen, body, &format!("The levels did not draw: {error}"));
            return;
        }
        Levels::Done(_, Ok(rows)) => rows,
    };
    let painter = board::painter_in(ui, body);
    let wide = body.width() >= 380.0;
    let columns: &[(&str, f32)] = if wide {
        &[("triangles", 76.0), ("cards", 60.0), ("packaged", 76.0)]
    } else {
        &[("triangles", 76.0), ("packaged", 76.0)]
    };
    let numbers: f32 = columns.iter().map(|(_, width)| width).sum();
    let label_width = 56.0;
    let bar_left = body.left() + label_width;
    let bar_right = body.right() - numbers - 8.0;
    // The columns' names.
    let mut x = body.right();
    for (name, width) in columns.iter().rev() {
        canvas::text(
            &painter,
            pos2(x, body.top()),
            Align2::RIGHT_TOP,
            name,
            pen.regular(canvas::AXIS),
            theme::TEXT_3,
        );
        x -= width;
    }
    let nearest = rows.first().map_or(1, |facts| facts.triangles.max(1));
    #[allow(clippy::cast_precision_loss)]
    let line = ((body.height() - 20.0) / rows.len().max(1) as f32).min(30.0);
    for (index, facts) in rows.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let middle = body.top() + 20.0 + line * (index as f32 + 0.5);
        let name = match index {
            0 => "0 near".to_owned(),
            3 => "3 far".to_owned(),
            _ => index.to_string(),
        };
        canvas::text(
            &painter,
            pos2(body.left(), middle),
            Align2::LEFT_CENTER,
            &name,
            pen.regular(11.5),
            if facts.level
                == page
                    .data
                    .plant
                    .as_ref()
                    .map_or(usize::MAX, |p| p.facts.level)
            {
                theme::TEXT
            } else {
                theme::TEXT_2
            },
        );
        if bar_right - bar_left > 20.0 {
            #[allow(clippy::cast_precision_loss)]
            let share = facts.triangles as f32 / nearest as f32;
            figures::share_bar(
                &painter,
                Rect::from_min_max(pos2(bar_left, middle - 4.0), pos2(bar_right, middle + 4.0)),
                share,
                theme::HEAT[6 - index.min(3)],
            );
        }
        let mut x = body.right();
        for (name, width) in columns.iter().rev() {
            let value = match *name {
                "triangles" => thousands(facts.triangles),
                "cards" => thousands(facts.cards),
                _ => bytes(facts.export.package_mesh),
            };
            canvas::text(
                &painter,
                pos2(x, middle),
                Align2::RIGHT_CENTER,
                &value,
                pen.mono(11.5),
                theme::TEXT,
            );
            x -= width;
        }
    }
}

fn growth(ui: &Ui, page: &Page, rect: Rect) {
    let room = card_with_legend(
        ui,
        page.pen,
        rect,
        &Head {
            icon: icons::TREND_UP,
            title: "Growth",
            about: "height and crown at each frame shown as it grew",
        },
        &[(theme::TEXT, "height"), (theme::SERIES[2], "crown")],
    );
    let body = room.body;
    let pen = page.pen;
    let points = &page.data.growth;
    if points.len() < 2 {
        waiting(
            ui,
            pen,
            body,
            "Grow the plant with this panel open and \"Draw the plant as it grows\" on.",
        );
        return;
    }
    let painter = board::painter_in(ui, body);
    let plot = Rect::from_min_max(
        pos2(body.left() + 40.0, body.top() + 6.0),
        pos2(body.right() - 6.0, body.bottom() - 18.0),
    );
    if plot.width() < 40.0 || plot.height() < 30.0 {
        return;
    }
    let oldest = points
        .iter()
        .map(|[age, _, _]| *age)
        .fold(page.lab.age(), f64::max)
        .max(0.1);
    #[allow(clippy::cast_possible_truncation)]
    let top = figures::nice_top(
        points
            .iter()
            .map(|[_, height, crown]| height.max(*crown))
            .fold(0.0, f64::max) as f32,
    );
    #[allow(clippy::cast_possible_truncation)]
    let at = |age: f64, value: f64| {
        pos2(
            plot.left() + (age / oldest) as f32 * plot.width(),
            plot.bottom() - (value as f32 / top).clamp(0.0, 1.0) * plot.height(),
        )
    };
    for share in [0.0_f32, 0.5, 1.0] {
        let y = plot.bottom() - share * plot.height();
        canvas::line(
            &painter,
            pos2(plot.left(), y),
            pos2(plot.right(), y),
            1.0,
            if share == 0.0 {
                theme::AXIS
            } else {
                theme::GRID
            },
        );
        canvas::text(
            &painter,
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            &metres(f64::from(top * share)),
            pen.regular(canvas::AXIS),
            theme::TEXT_3,
        );
    }
    canvas::text(
        &painter,
        pos2(plot.left(), plot.bottom() + 3.0),
        Align2::LEFT_TOP,
        "0 y",
        pen.regular(canvas::AXIS),
        theme::TEXT_3,
    );
    canvas::text(
        &painter,
        pos2(plot.right(), plot.bottom() + 3.0),
        Align2::RIGHT_TOP,
        &format!("{oldest:.1} y"),
        pen.regular(canvas::AXIS),
        theme::TEXT_3,
    );
    let crown: Vec<Pos2> = points.iter().map(|[age, _, c]| at(*age, *c)).collect();
    let height: Vec<Pos2> = points.iter().map(|[age, h, _]| at(*age, *h)).collect();
    canvas::polyline(&painter, crown, 1.5, theme::SERIES[2]);
    canvas::polyline(&painter, height.clone(), 2.0, theme::TEXT);
    if let Some(last) = height.last() {
        painter.circle_filled(*last, 3.0, theme::TEXT);
    }
}

/// `value` metres as the axis says it.
fn metres(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if value < 1.0 {
        format!("{:.0} cm", value * 100.0)
    } else {
        format!("{value:.0} m")
    }
}

/// Columns over `plot` against `top`, the 50th and 99th percentiles as
/// dashed lines labelled at the right, and the values' spread beside it.
fn spread_lines(painter: &egui::Painter, pen: Pen, plot: Rect, values: &[f32], top: f32) {
    let Some((p50, p99)) = spread_of(values) else {
        return;
    };
    let spread = Rect::from_min_max(
        pos2(plot.right() + 8.0, plot.top()),
        pos2(plot.right() + 44.0, plot.bottom()),
    );
    figures::spread(painter, spread, values, top, 3.0);
    let mut labels = canvas::Labels::new();
    for (value, name) in [(p99, "p99"), (p50, "p50")] {
        if value > top {
            continue;
        }
        let y = plot.bottom() - value / top * plot.height();
        canvas::dashed(
            painter,
            pos2(plot.left(), y),
            pos2(spread.right(), y),
            1.0,
            theme::TEXT_3,
            4.0,
            4.0,
        );
        labels.text(
            painter,
            pos2(spread.right() + 4.0, y),
            Align2::LEFT_CENTER,
            &format!("{name} {value:.1} ms"),
            pen.regular(canvas::AXIS),
            theme::TEXT_2,
        );
    }
}

/// The plot's floor and top labels in ms.
fn ms_axis(painter: &egui::Painter, pen: Pen, plot: Rect, top: f32) {
    canvas::line(
        painter,
        pos2(plot.left(), plot.bottom()),
        pos2(plot.right(), plot.bottom()),
        1.0,
        theme::AXIS,
    );
    canvas::line(
        painter,
        pos2(plot.left(), plot.top()),
        pos2(plot.right(), plot.top()),
        1.0,
        theme::GRID,
    );
    for (value, y) in [(top, plot.top()), (0.0, plot.bottom())] {
        canvas::text(
            painter,
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            &if value == 0.0 {
                "0".to_owned()
            } else if value >= 1_000.0 {
                format!("{} s", value / 1_000.0)
            } else {
                format!("{value} ms")
            },
            pen.regular(canvas::AXIS),
            theme::TEXT_3,
        );
    }
}

/// The plot of a timing card: room for the ms labels at the left and the
/// spread and its labels at the right.
fn timing_plot(body: Rect) -> Rect {
    Rect::from_min_max(
        pos2(body.left() + 44.0, body.top() + 22.0),
        pos2(body.right() - 128.0, body.bottom() - 18.0),
    )
}

const STEP_COLOUR: Color32 = theme::HEAT[5];
const FRAMED_COLOUR: Color32 = theme::HEAT[2];

fn growing(ui: &Ui, page: &Page, rect: Rect) {
    let room = card_with_legend(
        ui,
        page.pen,
        rect,
        &Head {
            icon: icons::TIMER,
            title: "Growing time",
            about: "each growth step, oldest at the left",
        },
        &[
            (STEP_COLOUR, "a step"),
            (FRAMED_COLOUR, "with a frame drawn"),
        ],
    );
    let body = room.body;
    let pen = page.pen;
    let steps = &page.data.steps;
    if steps.is_empty() {
        waiting(
            ui,
            pen,
            body,
            "Grow the plant with this panel open to time its steps.",
        );
        return;
    }
    let painter = board::painter_in(ui, body);
    let values: Vec<f32> = steps.iter().map(|step| step.ms).collect();
    let total: f32 = values.iter().sum();
    let (p50, p99) = spread_of(&values).unwrap_or_default();
    note_at(
        &painter,
        pen,
        body.min,
        body.width(),
        &format!(
            "{} steps · {:.2} s in all · p50 {p50:.2} ms · p99 {p99:.2} ms",
            steps.len(),
            total / 1_000.0
        ),
    );
    let plot = timing_plot(body);
    if plot.width() < 40.0 || plot.height() < 30.0 {
        return;
    }
    let top = figures::nice_top(p99.max(0.1) * 1.25);
    ms_axis(&painter, pen, plot, top);
    #[allow(clippy::cast_precision_loss)]
    let count = steps.len() as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let columns = ((plot.width() / 2.0).floor() as usize).clamp(1, steps.len());
    let samples = |framed: Option<bool>| -> Vec<[f64; 2]> {
        #[allow(clippy::cast_precision_loss)]
        steps
            .iter()
            .enumerate()
            .filter(|(_, step)| framed.is_none_or(|framed| step.framed == framed))
            .map(|(index, step)| [index as f64 + 0.5, f64::from(step.ms)])
            .collect()
    };
    let all = figures::column_peaks(&samples(None), count, count, columns);
    let plain = figures::column_peaks(&samples(Some(false)), count, count, columns);
    figures::stacked_columns(&painter, plot, &[(&all, FRAMED_COLOUR)], top);
    figures::stacked_columns(&painter, plot, &[(&plain, STEP_COLOUR)], top);
    spread_lines(&painter, pen, plot, &values, top);
    canvas::text(
        &painter,
        pos2(plot.left(), plot.bottom() + 3.0),
        Align2::LEFT_TOP,
        "step 1",
        pen.regular(canvas::AXIS),
        theme::TEXT_3,
    );
    canvas::text(
        &painter,
        pos2(plot.right(), plot.bottom() + 3.0),
        Align2::RIGHT_TOP,
        &format!("step {}", steps.len()),
        pen.regular(canvas::AXIS),
        theme::TEXT_3,
    );
}

fn drawing(ui: &Ui, page: &Page, rect: Rect) {
    let room = board::card(
        ui,
        rect,
        &Head {
            icon: icons::MONITOR,
            title: "Drawing",
            about: "the window's frames, last 10 s",
        },
        0.0,
    );
    let body = room.body;
    let pen = page.pen;
    let data = page.data;
    let values = recent_frames(data);
    if values.is_empty() {
        waiting(ui, pen, body, "Frame times start with the next frame.");
        return;
    }
    let painter = board::painter_in(ui, body);
    let (p50, p99) = spread_of(&values).unwrap_or_default();
    note_at(
        &painter,
        pen,
        body.min,
        body.width(),
        &format!(
            "{:.0} frames a second · p50 {p50:.1} ms · p99 {p99:.1} ms",
            1_000.0 / p50.max(0.1)
        ),
    );
    let plot = timing_plot(body);
    if plot.width() < 40.0 || plot.height() < 30.0 {
        return;
    }
    let top = figures::nice_top(p99.max(SIXTY_HZ_MS) * 1.25);
    ms_axis(&painter, pen, plot, top);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let columns = (plot.width() / 2.0).floor().max(1.0) as usize;
    let frames: Vec<[f64; 2]> = data.frames.iter().copied().collect();
    let peaks = figures::held(figures::column_peaks(&frames, data.now, WINDOW_S, columns));
    figures::stacked_columns(&painter, plot, &[(&peaks, theme::HEAT[4])], top);
    spread_lines(&painter, pen, plot, &values, top);
    for step in 0..=5 {
        let seconds = 10 - 2 * step;
        #[allow(clippy::cast_precision_loss)]
        let x = plot.left() + plot.width() * step as f32 / 5.0;
        let label = if seconds == 0 {
            "now".to_owned()
        } else {
            format!("−{seconds} s")
        };
        if plot.width() < 240.0 && step % 5 != 0 {
            continue;
        }
        canvas::text(
            &painter,
            pos2(x, plot.bottom() + 3.0),
            match step {
                0 => Align2::LEFT_TOP,
                5 => Align2::RIGHT_TOP,
                _ => Align2::CENTER_TOP,
            },
            &label,
            pen.regular(canvas::AXIS),
            theme::TEXT_3,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spread_is_the_median_and_the_99th_percentile() {
        let values: Vec<f32> = (1..=100_u8).map(f32::from).collect();
        assert_eq!(spread_of(&values), Some((51.0, 99.0)));
        assert_eq!(spread_of(&[]), None);
        assert_eq!(thousands(1_234_567), "1\u{2009}234\u{2009}567");
    }

    #[test]
    fn every_arrangement_places_every_card() {
        for width in [Width::Wide, Width::Medium, Width::Narrow] {
            let arrangement = ARRANGEMENTS.at(width);
            assert_eq!(arrangement.places.len(), CARDS.len());
            for place in arrangement.places {
                assert!(usize::from(place.rows.1) <= arrangement.rows.len());
                assert!(place.columns.1 <= board::COLUMNS);
            }
        }
    }
}
