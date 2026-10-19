//! What each inspector page holds, the menus, the palette's entries and
//! the settings. Each control reads [`Lab`] and [`Recording`] and queues a
//! [`Command`]; none changes them.
//!
//! The Render page holds what a render is made with, in the order a
//! render is thought through: which plant (its age, site and variant),
//! when in the year (the day, with each seasonal organ's year drawn under
//! it), and how much detail; then Generate, always in reach at its foot.

use std::sync::atomic::Ordering;

use bevy_egui::egui::{self, Align2, Color32, RichText, Sense, Ui, vec2};
use plantgen::library::Library;
use plantgen::spec::Environment;
use plantlab_scene::{Look, Phase, TRACK_STEP};
use plantlab_ui::palette::Entry;
use plantlab_ui::widgets::{self, SliderLook};
use plantlab_ui::{icons, theme};

use super::command::{Command, Panel, Shell};
use super::shell::{VIEWS, years};
use super::store::{Detail, Recipe};
use super::{About, Lab, store, thumbs_command};
use crate::record::Recording;

/// Draws `panel`'s contents: the Render page with Generate kept at its
/// foot, the others scrolling.
pub fn show(
    ui: &mut Ui,
    panel: Panel,
    lab: &Lab,
    recording: &Recording,
    readings: &super::readings::Readings,
    shell: &mut Shell,
) {
    if panel == Panel::Render {
        egui::Panel::bottom("plantlab-generate")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE_1)
                    .inner_margin(egui::Margin::symmetric(0, 10)),
            )
            .show(ui, |ui| generate(ui, lab, shell));
    }
    egui::ScrollArea::vertical()
        .id_salt(("inspector", panel.short()))
        .auto_shrink([false, false])
        .show(ui, |ui| match panel {
            Panel::Render => render(ui, lab, shell),
            Panel::Readings => super::readings::panel(ui, lab, readings, shell),
            Panel::Pictures => pictures(ui, lab, recording, shell),
            Panel::Spec => spec(ui, lab, shell),
            Panel::Bark => bark(ui, lab, shell),
            Panel::Keys => keys(ui),
        });
}

/// The month names, January first.
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The sites a plant can grow on, as the Render page offers them.
const SITES: [(Option<Environment>, &str); 5] = [
    (None, "Typical"),
    (Some(Environment::Open), "Open"),
    (Some(Environment::Edge), "Edge"),
    (Some(Environment::Interior), "Interior"),
    (Some(Environment::Suppressed), "Suppressed"),
];

/// The days the season chips jump to: the middle of April, July,
/// October and January.
const SEASONS: [(&str, f64); 4] = [
    ("Spring", 105.0),
    ("Summer", 196.0),
    ("Autumn", 288.0),
    ("Winter", 15.0),
];

/// The Render page: the species, then the plant, the season and the
/// detail.
#[allow(clippy::too_many_lines)]
fn render(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    species_card(ui, lab);
    if !lab.about.error.is_empty() {
        ui.label(RichText::new(&lab.about.error).color(theme::ERROR));
        return;
    }
    let recipe = &lab.recipe;

    widgets::section(ui, "Plant");
    let age = lab.age();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Age").color(theme::TEXT_2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let words = if recipe.age.is_none() {
                format!("{}, its oldest keyframe", years(age))
            } else {
                years(age)
            };
            ui.label(RichText::new(words).size(theme::SMALL).color(theme::TEXT_3));
        });
    });
    let youngest = lab
        .about
        .keyframes
        .first()
        .copied()
        .unwrap_or(1.0)
        .min(age)
        .min(1.0);
    let step = if youngest < 1.0 { 0.0 } else { 1.0 };
    let look = SliderLook {
        step,
        suffix: " y",
        decimals: (step >= 1.0).then_some(0),
        ..SliderLook::default()
    };
    // The slider takes the row, leaving room for its value.
    ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(80.0);
    let salt = ("age", &lab.chosen);
    if let Some(age) = widgets::slider(ui, salt, age, youngest..=lab.about.oldest, look) {
        shell.send(Command::Age(age));
    }
    if !lab.about.keyframes.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for &keyframe in &lab.about.keyframes {
                let on = (keyframe - age).abs() < 1e-6;
                if widgets::chip(ui, &years(keyframe), on) && !on {
                    shell.send(Command::Age(keyframe));
                }
            }
        });
        ui.label(
            RichText::new("The keyframes: the ages its spec checks and packages hold")
                .size(theme::SMALL)
                .color(theme::TEXT_3),
        );
    }
    ui.add_space(theme::GAP_SMALL);
    ui.label(RichText::new("Site").color(theme::TEXT_2));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (site, word) in SITES {
            let on = recipe.environment == site;
            let text = if site.is_none() {
                format!("{word} ({})", lab.about.typical)
            } else {
                word.to_owned()
            };
            if widgets::chip(ui, &text, on) && !on {
                shell.send(Command::Site(site));
            }
        }
    });
    ui.label(
        RichText::new(site_words(recipe.environment, &lab.about.typical))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
    ui.add_space(theme::GAP_SMALL);
    variant(ui, lab, shell);

    widgets::section(ui, "Season");
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(date(recipe.day))
                .font(theme::strong_font(ui, theme::BODY))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(format!("day {:.0}", recipe.day))
                .font(egui::FontId::monospace(theme::SMALL))
                .color(theme::TEXT_3),
        );
    });
    if let Some(day) = season_strip(ui, &lab.about, recipe.day) {
        shell.send(Command::Day(day));
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (season, day) in SEASONS {
            let on = (recipe.day - day).abs() < 0.5;
            if widgets::chip(ui, season, on) && !on {
                shell.send(Command::Day(day));
            }
        }
        let playing = lab.year.is_some();
        let (icon, text) = if playing {
            (icons::STOP, "Stop the year")
        } else {
            (icons::PLAY_CIRCLE, "Play the year")
        };
        if ui
            .button(with_icon(ui, icon, text, theme::TEXT))
            .on_hover_text(
                "Draw the plant in the middle of each month in turn, from the plant grown once; \
                 record growth to make a GIF of its year",
            )
            .clicked()
        {
            shell.send(Command::PlayYear(!playing));
        }
    });
    if lab.about.year.is_empty() {
        ui.label(
            RichText::new("Its spec gives it no seasons: it looks the same all year.")
                .size(theme::SMALL)
                .color(theme::TEXT_3),
        );
    }

    widgets::section(ui, "Detail");
    let chosen = Detail::ALL
        .iter()
        .position(|detail| *detail == recipe.detail)
        .unwrap_or(0);
    let names: Vec<&str> = Detail::ALL.iter().map(|detail| detail.label()).collect();
    if let Some(index) = widgets::segmented(ui, &names, chosen) {
        shell.send(Command::Detail(Detail::ALL[index]));
    }
    ui.label(
        RichText::new(match recipe.detail {
            Detail::Draft => "Draft meshes: quick, for trying settings.",
            Detail::Standard => "Standard meshes, as the library's packages hold them.",
            Detail::Full => {
                "Standard meshes with every flower, fruit, cone and needle shoot as its part \
                 mesh, as full-quality exports draw them up close."
            }
        })
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    ui.add_space(theme::GAP_SMALL);
    ui.label(RichText::new("Level of detail").color(theme::TEXT_2));
    let level = if recipe.detail.parts() {
        0
    } else {
        recipe.level
    };
    if let Some(level) = widgets::segmented(ui, &["0 nearest", "1", "2", "3 farthest"], level) {
        shell.send(Command::Level(level));
    }

    widgets::section(ui, "While it grows");
    if widgets::switch(ui, "Draw the plant as it grows", lab.live) {
        shell.send(Command::ShowGrowing(!lab.live));
    }
    ui.add_enabled_ui(lab.live, |ui| {
        let look = SliderLook {
            step: 1.0,
            suffix: " steps",
            logarithmic: true,
            decimals: Some(0),
        };
        if let Some(every) = widgets::slider(ui, "every", f64::from(lab.every), 1.0..=100.0, look) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            shell.send(Command::Every(every.round() as u32));
        }
    });
    if widgets::switch(ui, "Move the camera with the plant", lab.follow) {
        shell.send(Command::Follow(!lab.follow));
    }
    ui.add_space(theme::GAP);
}

/// The chosen species' names, form and ranks, and its last render.
fn species_card(ui: &mut Ui, lab: &Lab) {
    let Some(item) = lab.catalogue.get(&lab.chosen) else {
        return;
    };
    widgets::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(item.title())
                .font(theme::strong_font(ui, theme::HEADING))
                .color(theme::TEXT),
        );
        if !item.common.is_empty() {
            ui.label(
                RichText::new(&item.scientific)
                    .italics()
                    .color(theme::TEXT_2),
            );
        }
        ui.label(
            RichText::new(format!(
                "{} · {} · {} · {}",
                item.form.one(),
                item.genus,
                item.family,
                item.order
            ))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
        );
        let line = match lab.store.kept.get(&lab.chosen) {
            Some(kept) => format!(
                "Last render {}, in {:.1} s",
                store::ago(kept.saved, store::now()),
                kept.seconds
            ),
            None => "Not rendered yet".to_owned(),
        };
        ui.label(RichText::new(line).size(theme::SMALL).color(theme::TEXT_2));
    });
}

/// What a site means for the plant.
fn site_words(site: Option<Environment>, typical: &str) -> String {
    match site {
        None => format!("Where its spec says it usually grows: {typical}."),
        Some(Environment::Open) => "In the open, with no neighbours shading it.".into(),
        Some(Environment::Edge) => "At a stand's edge: open on one side.".into(),
        Some(Environment::Interior) => "Inside a stand, crowded by its neighbours.".into(),
        Some(Environment::Suppressed) => "Under a canopy, in deep shade.".into(),
    }
}

/// The variant: the spec's seeds, the first the typical plant.
fn variant(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let seeds = &lab.about.seeds;
    if seeds.is_empty() {
        return;
    }
    let seed = lab.recipe.seed.unwrap_or(seeds[0]);
    let index = seeds.iter().position(|known| *known == seed);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Variant").color(theme::TEXT_2));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = index.map_or(0, |index| (index + 1) % seeds.len());
            if widgets::icon_button(ui, icons::CARET_RIGHT, "The next variant", false) {
                shell.send(Command::Seed((next != 0).then_some(seeds[next])));
            }
            let words = match index {
                Some(index) => format!("{} of {} · seed {seed}", index + 1, seeds.len()),
                None => format!("seed {seed}"),
            };
            ui.label(
                RichText::new(words)
                    .font(egui::FontId::monospace(theme::SMALL))
                    .color(theme::TEXT),
            );
            let back = index.map_or(0, |index| (index + seeds.len() - 1) % seeds.len());
            if widgets::icon_button(ui, icons::CARET_LEFT, "The variant before", false) {
                shell.send(Command::Seed((back != 0).then_some(seeds[back])));
            }
        });
    });
    ui.label(
        RichText::new(
            "Each seed grows another plant of the species; the first is the typical one.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
}

/// Each phase's colour on the season strip.
fn phase_colour(phase: Phase) -> Option<Color32> {
    Some(match phase {
        Phase::Gone => return None,
        Phase::Bud => Color32::from_rgb(0xd9, 0xa8, 0xc4),
        Phase::Flower => Color32::from_rgb(0xf0, 0x6f, 0xb4),
        Phase::Unripe => Color32::from_rgb(0xb8, 0xc9, 0x5a),
        Phase::Ripe => Color32::from_rgb(0xe0, 0x7a, 0x3a),
        Phase::Unfolding => Color32::from_rgb(0x9f, 0xd8, 0x6b),
        Phase::Leaf => Color32::from_rgb(0x4c, 0xa0, 0x5a),
        Phase::Turning => Color32::from_rgb(0xd8, 0xa1, 0x38),
    })
}

/// Each phase's word in the strip's legend.
fn phase_word(phase: Phase) -> &'static str {
    match phase {
        Phase::Gone => "",
        Phase::Bud => "bud",
        Phase::Flower => "flower",
        Phase::Unripe => "unripe",
        Phase::Ripe => "ripe",
        Phase::Unfolding => "leaves unfold",
        Phase::Leaf => "full leaf",
        Phase::Turning => "turning",
    }
}

/// A track's band and the gap between bands, points; the most tracks
/// drawn; the room for the tracks' names.
const BAND: f32 = 9.0;
const BAND_GAP: f32 = 3.0;
const MOST_TRACKS: usize = 6;
const NAMES: f32 = 64.0;

/// The year as a strip: the months along it, each seasonal organ's year
/// as a band of its phases, and the day chosen as a line across. A click
/// or a drag along it chooses the day, sent when let go.
#[allow(clippy::too_many_lines)]
fn season_strip(ui: &mut Ui, about: &About, day: f64) -> Option<f64> {
    let tracks: Vec<_> = about.year.iter().take(MOST_TRACKS).collect();
    #[allow(clippy::cast_precision_loss)]
    let bands = tracks.len() as f32 * (BAND + BAND_GAP);
    let height = 16.0 + bands.max(BAND + BAND_GAP) + 4.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    let strip = egui::Rect::from_min_max(egui::pos2(rect.left() + NAMES, rect.top()), rect.max);
    let x_of = |day: f64| {
        #[allow(clippy::cast_possible_truncation)]
        let share = ((day - 1.0) / 364.0) as f32;
        strip.left() + share * strip.width()
    };
    let day_at = |x: f32| {
        let share = ((x - strip.left()) / strip.width()).clamp(0.0, 1.0);
        (1.0 + f64::from(share) * 364.0).round()
    };
    let painter = ui.painter_at(rect);
    let id = response.id.with("held");
    let held: Option<f64> = ui.data(|data| data.get_temp(id));
    // The months: a faint band each other month, and their initials.
    let mut start = 1.0;
    for (month, days) in [
        31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0,
    ]
    .iter()
    .enumerate()
    {
        let (x0, x1) = (x_of(start), x_of(start + days - 1.0));
        if month % 2 == 0 {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
                0.0,
                theme::SURFACE_2,
            );
        }
        painter.text(
            egui::pos2(f32::midpoint(x0, x1), rect.top() + 7.0),
            Align2::CENTER_CENTER,
            &MONTHS[month][..1],
            egui::FontId::proportional(theme::SMALL - 1.0),
            theme::TEXT_3,
        );
        start += days;
    }
    // Each track's band.
    #[allow(clippy::cast_precision_loss)]
    let step = TRACK_STEP as f64;
    for (row, track) in tracks.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let top = rect.top() + 16.0 + row as f32 * (BAND + BAND_GAP);
        painter.text(
            egui::pos2(rect.left(), top + BAND / 2.0),
            Align2::LEFT_CENTER,
            organ_word(&track.organ, track.leaves),
            egui::FontId::proportional(theme::SMALL - 1.0),
            theme::TEXT_2,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(strip.left(), top),
                egui::pos2(strip.right(), top + BAND),
            ),
            2.0,
            theme::SURFACE_3,
        );
        for (index, phase) in track.phases.iter().enumerate() {
            let Some(colour) = phase_colour(*phase) else {
                continue;
            };
            #[allow(clippy::cast_precision_loss)]
            let first = 1.0 + index as f64 * step;
            let x0 = x_of(first);
            let x1 = x_of((first + step).min(365.0)).max(x0 + 1.0);
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x0, top), egui::pos2(x1, top + BAND)),
                0.0,
                colour,
            );
        }
    }
    // The day: the chosen one, or the one under the pointer while held.
    let under = response
        .interact_pointer_pos()
        .map(|position| day_at(position.x));
    let shown_day = under.or(held).unwrap_or(day);
    let x = x_of(shown_day);
    painter.line_segment(
        [
            egui::pos2(x, rect.top() + 13.0),
            egui::pos2(x, rect.bottom()),
        ],
        egui::Stroke::new(2.0, theme::TEXT),
    );
    painter.circle_filled(egui::pos2(x, rect.top() + 13.0), 3.0, theme::TEXT);
    let mut sent = None;
    if response.dragged() || response.is_pointer_button_down_on() {
        ui.data_mut(|data| data.insert_temp(id, shown_day));
    }
    if (response.drag_stopped() || response.clicked())
        && let Some(day) = under.or(held)
    {
        sent = Some(day);
        ui.data_mut(|data| data.remove::<f64>(id));
    }
    let response = response.on_hover_text(date(shown_day));
    drop(response);
    // The legend: the phases the strip shows.
    let mut seen: Vec<Phase> = Vec::new();
    for track in &tracks {
        for phase in &track.phases {
            if *phase != Phase::Gone && !seen.contains(phase) {
                seen.push(*phase);
            }
        }
    }
    let order = [
        Phase::Unfolding,
        Phase::Leaf,
        Phase::Turning,
        Phase::Bud,
        Phase::Flower,
        Phase::Unripe,
        Phase::Ripe,
    ];
    if !seen.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
            for phase in order.iter().filter(|phase| seen.contains(phase)) {
                // One label each, a square of its colour then its word, so
                // the row wraps between phases.
                let font = egui::FontId::proportional(theme::SMALL - 1.0);
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    "■",
                    0.0,
                    egui::TextFormat::simple(
                        font.clone(),
                        phase_colour(*phase).unwrap_or(theme::TEXT_3),
                    ),
                );
                job.append(
                    phase_word(*phase),
                    3.0,
                    egui::TextFormat::simple(font, theme::TEXT_3),
                );
                ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
    }
    sent
}

/// A seasonal organ's name on the strip: its name in the program, made
/// plain (`male_cone` to `male cone`), and `leaves` for the leafing year.
fn organ_word(organ: &str, leaves: bool) -> String {
    if leaves {
        return "leaves".into();
    }
    organ.replace('_', " ")
}

/// Generate, at the Render page's foot: what it will do and how long it
/// took last; Pause and Stop while it grows.
fn generate(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.max_rect().top(),
        egui::Stroke::new(1.0, theme::LINE_STRONG),
    );
    ui.add_space(2.0);
    if let Some(job) = &lab.growing {
        let (share, text) = progress(lab);
        let paused = lab.control.pause.load(Ordering::Relaxed);
        let label = if job.redraw {
            "Drawing"
        } else if paused {
            "Paused"
        } else {
            "Growing"
        };
        widgets::progress(ui, share, label, &text);
        ui.add_space(theme::GAP_SMALL);
        ui.horizontal(|ui| {
            if !job.redraw {
                let pause = if paused { "Resume" } else { "Pause" };
                if ui.button(pause).on_hover_text("Space").clicked() {
                    shell.send(Command::Pause(!paused));
                }
            }
            if ui
                .button("Stop")
                .on_hover_text("Stop growing; the last frame shown stays (Esc)")
                .clicked()
            {
                shell.send(Command::Stop);
            }
        });
        return;
    }
    let changes = lab.changes();
    let shows = lab.shows_chosen();
    let text = if shows && changes.is_empty() {
        "Generate again"
    } else {
        "Generate"
    };
    if widgets::primary_button(
        ui,
        icons::PLAY,
        text,
        Some("Enter"),
        lab.about.error.is_empty(),
    )
    .on_hover_text("Grow the plant with these settings")
    .clicked()
    {
        shell.send(Command::Generate);
    }
    let note = if lab.redraws() && !changes.is_empty() {
        let words: Vec<&str> = changes.iter().map(|setting| setting.word()).collect();
        format!(
            "Draws the grown plant again for its new {}: no growing.",
            words.join(" and ")
        )
    } else if shows && changes.is_empty() {
        "On screen as these settings make it.".to_owned()
    } else {
        match lab.store.kept.get(&lab.chosen) {
            Some(kept) if !shows => format!(
                "Grows the plant. Its last render took {:.1} s.",
                kept.seconds
            ),
            _ => "Grows the plant from seed to its age.".to_owned(),
        }
    };
    ui.add_space(4.0);
    ui.label(RichText::new(note).size(theme::SMALL).color(theme::TEXT_3));
}

/// A recipe in words, for the last render's caption: the age, day,
/// detail, site and variant.
pub fn recipe_words(recipe: &Recipe, age: f64, about: &About) -> String {
    let mut words = vec![
        years(age),
        date(recipe.day),
        recipe.detail.name().to_owned(),
    ];
    if !recipe.detail.parts() && recipe.level > 0 {
        words.push(format!("level {}", recipe.level));
    }
    words.push(recipe.environment.map_or_else(
        || format!("{} site", about.typical),
        |site| format!("{} site", site.name()),
    ));
    if let Some(seed) = recipe.seed {
        words.push(format!("seed {seed}"));
    }
    words.join(", ")
}

/// A day of the year as a date in a year of 365 days.
#[must_use]
pub fn date(day: f64) -> String {
    const MONTHS: [(&str, u32); 12] = [
        ("January", 31),
        ("February", 28),
        ("March", 31),
        ("April", 30),
        ("May", 31),
        ("June", 30),
        ("July", 31),
        ("August", 31),
        ("September", 30),
        ("October", 31),
        ("November", 30),
        ("December", 31),
    ];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut left = day.clamp(1.0, 365.0).round() as u32;
    for (month, days) in MONTHS {
        if left <= days {
            return format!("{left} {month}");
        }
        left -= days;
    }
    "31 December".into()
}

/// What is growing, or why it stopped, and what is on screen.
#[must_use]
pub fn state_line(lab: &Lab) -> (Color32, String) {
    if let Some(job) = &lab.growing {
        let (_, text) = progress(lab);
        let paused = lab.control.pause.load(Ordering::Relaxed);
        let colour = if paused {
            theme::STATUS[1]
        } else {
            theme::ACCENT
        };
        let word = if job.redraw {
            "Drawing"
        } else if paused {
            "Paused"
        } else {
            "Growing"
        };
        return (colour, format!("{word}: {text}"));
    }
    if !lab.message.is_empty() {
        return (theme::STATUS[2], lab.message.clone());
    }
    match &lab.shown {
        Some(shown) => {
            let name = lab
                .catalogue
                .get(&shown.scene.facts.species)
                .map_or(shown.scene.facts.species.as_str(), |item| item.title());
            (
                theme::STATUS[0],
                format!(
                    "{name}, {}, {}, made in {:.1} s",
                    years(shown.scene.facts.age),
                    date(shown.recipe.day),
                    shown.seconds
                ),
            )
        }
        None => (theme::TEXT_3, "Choose the settings, then Generate".into()),
    }
}

/// The growth's share done and its words.
#[must_use]
pub fn progress(lab: &Lab) -> (f32, String) {
    let seconds = lab.started.elapsed().as_secs_f64();
    if lab.growing.as_ref().is_some_and(|job| job.redraw) {
        return (0.0, format!("drawing, {seconds:.1} s"));
    }
    let step = lab.control.step.load(Ordering::Relaxed);
    let steps = lab.control.steps.load(Ordering::Relaxed);
    let age = f64::from_bits(lab.control.age.load(Ordering::Relaxed));
    if steps == 0 {
        (0.0, format!("starting, {seconds:.1} s"))
    } else if step >= steps {
        (1.0, format!("grown, meshing, {seconds:.1} s"))
    } else {
        #[allow(clippy::cast_possible_truncation)]
        let share = (f64::from(step) / f64::from(steps)) as f32;
        (
            share,
            format!("step {step} of {steps}, {age:.1} years, {seconds:.1} s"),
        )
    }
}

/// Every key, in groups.
fn keys(ui: &mut Ui) {
    let pair = |key: &str, help: &str| (key.to_owned(), help.to_owned());
    widgets::key_help(
        ui,
        &[
            (
                "Rendering",
                vec![
                    pair("Enter", "generate"),
                    pair("Space", "pause or resume"),
                    pair("Esc", "stop, or leave the Overlay"),
                ],
            ),
            (
                "View",
                vec![
                    pair("1", "from the side"),
                    pair("2", "three-quarter"),
                    pair("3", "45 degrees down"),
                    pair("4", "from the top"),
                    pair("F", "frame the plant again"),
                    pair("G", "grid"),
                    pair("R", "ruler"),
                    pair("O", "Overlay, and back"),
                ],
            ),
            (
                "Pictures",
                vec![pair("P", "snapshot"), pair("S", "save the GIF")],
            ),
            (
                "Window",
                vec![
                    pair("L", "the library"),
                    pair("I", "the inspector"),
                    pair("F1", "these keys"),
                    pair("Ctrl+K", "find a species or a command"),
                    pair("Ctrl+,", "settings"),
                ],
            ),
        ],
    );
    ui.label(
        RichText::new(
            "Drag to turn, middle-drag or Shift-drag to move, scroll to come closer. \
             In the library, double-click a species to choose it and generate.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
}

/// One entry of a menu, which the palette lists too.
pub struct MenuItem {
    pub label: String,
    pub key: Option<String>,
    /// A switch's state; `None` for an action.
    pub checked: Option<bool>,
    pub command: Command,
}

fn item(label: &str, key: Option<&str>, checked: Option<bool>, command: Command) -> MenuItem {
    MenuItem {
        label: label.to_owned(),
        key: key.map(str::to_owned),
        checked,
        command,
    }
}

/// The menus, each a list of entries.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn menus(
    lab: &Lab,
    recording: &Recording,
    shell: &Shell,
) -> Vec<(&'static str, Vec<MenuItem>)> {
    let paused = lab.control.pause.load(Ordering::Relaxed);
    let mut plant = vec![
        item("Generate", Some("Enter"), None, Command::Generate),
        item(
            "Find a species…",
            Some("Ctrl+K"),
            None,
            Command::Palette("species ".into()),
        ),
    ];
    if lab.growing.is_some() {
        plant.push(item(
            if paused { "Resume" } else { "Pause" },
            Some("Space"),
            None,
            Command::Pause(!paused),
        ));
        plant.push(item("Stop", Some("Esc"), None, Command::Stop));
    }
    plant.push(item(
        "Play the year",
        None,
        Some(lab.year.is_some()),
        Command::PlayYear(lab.year.is_none()),
    ));
    for detail in Detail::ALL {
        plant.push(item(
            &format!("{} detail", detail.label()),
            None,
            Some(lab.recipe.detail == detail),
            Command::Detail(detail),
        ));
    }
    plant.extend([
        item(
            "Draw the plant as it grows",
            None,
            Some(lab.live),
            Command::ShowGrowing(!lab.live),
        ),
        item(
            "Move the camera with the plant",
            None,
            Some(lab.follow),
            Command::Follow(!lab.follow),
        ),
    ]);
    let mut view: Vec<MenuItem> = VIEWS
        .iter()
        .enumerate()
        .map(|(index, (value, name, _))| {
            item(
                &format!("{name} view"),
                Some(&(index + 1).to_string()),
                Some(lab.recipe.view == *value),
                Command::View(*value),
            )
        })
        .collect();
    view.extend([
        item(
            "Review look",
            None,
            Some(lab.recipe.look == Look::Review),
            Command::Look(Look::Review),
        ),
        item(
            "Photo look",
            None,
            Some(lab.recipe.look == Look::Photo),
            Command::Look(Look::Photo),
        ),
        item("Grid", Some("G"), Some(lab.grid), Command::Grid(!lab.grid)),
        item(
            "Height ruler",
            Some("R"),
            Some(lab.ruler),
            Command::Ruler(!lab.ruler),
        ),
        item(
            "Frame the plant again",
            Some("F"),
            None,
            Command::ResetCamera,
        ),
        item(
            "Overlay",
            Some("O"),
            Some(shell.overlay),
            Command::Overlay(!shell.overlay),
        ),
        item(
            "Library",
            Some("L"),
            Some(shell.library),
            Command::Library(!shell.library),
        ),
        item(
            "Inspector",
            Some("I"),
            Some(shell.inspector),
            Command::Inspector(!shell.inspector),
        ),
    ]);
    let pictures = vec![
        item("Snapshot", Some("P"), None, Command::Snapshot),
        item(
            "Record growth",
            None,
            Some(recording.record),
            Command::Record(!recording.record),
        ),
        item("Save GIF", Some("S"), None, Command::SaveGif),
        item("Clear the frames", None, None, Command::ClearFrames),
        item(
            "Pictures of the plant alone",
            None,
            Some(recording.plant_only),
            Command::PlantOnly(!recording.plant_only),
        ),
    ];
    let pages = Panel::ALL
        .iter()
        .map(|panel| {
            item(
                panel.short(),
                (*panel == Panel::Keys).then_some("F1"),
                Some(shell.shows(*panel)),
                Command::Tab(*panel),
            )
        })
        .collect();
    let mut help = vec![item(
        "Settings",
        Some("Ctrl+,"),
        None,
        Command::Settings(true),
    )];
    if let Some(shown) = &lab.shown {
        help.push(item(
            "Print the thumbs command",
            None,
            None,
            Command::Print(thumbs_command(&shown.shot)),
        ));
    }
    vec![
        ("Plant", plant),
        ("View", view),
        ("Pictures", pictures),
        ("Pages", pages),
        ("Help", help),
    ]
}

/// The palette's entries: every menu entry and every species, found by
/// any of its names or ranks.
#[must_use]
pub fn palette_entries(lab: &Lab, recording: &Recording, shell: &Shell) -> Vec<(Entry, Command)> {
    let mut entries = Vec::new();
    for (menu, items) in menus(lab, recording, shell) {
        for item in items {
            let detail = match item.checked {
                Some(true) => format!("{menu} · on"),
                Some(false) => format!("{menu} · off"),
                None => menu.to_owned(),
            };
            entries.push((
                Entry {
                    label: item.label.to_lowercase(),
                    detail,
                    chord: item.key,
                },
                item.command,
            ));
        }
    }
    for item in &lab.catalogue.items {
        let label = if item.common.is_empty() {
            format!("species {}", item.scientific.to_lowercase())
        } else {
            format!(
                "species {} {}",
                item.common.to_lowercase(),
                item.scientific.to_lowercase()
            )
        };
        entries.push((
            Entry {
                label,
                detail: format!("{} · {} · {}", item.form.one(), item.family, item.order),
                chord: None,
            },
            Command::Choose(item.id.clone()),
        ));
    }
    entries
}

/// The settings saved between runs: each control saves its own.
pub fn settings(ui: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let path = shell.settings.path.as_ref().map_or_else(
        || "no folder known".to_owned(),
        |path| path.display().to_string(),
    );
    widgets::reading_row(ui, "saved in", &path);
    ui.label(
        RichText::new("Each change is saved at once, alone. Flags on the command line win.")
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
    widgets::section(ui, "Growing");
    if widgets::switch(ui, "Draw the plant as it grows", lab.live) {
        shell.send(Command::ShowGrowing(!lab.live));
    }
    if widgets::switch(ui, "Move the camera with the plant", lab.follow) {
        shell.send(Command::Follow(!lab.follow));
    }
    widgets::section(ui, "Window");
    if widgets::switch(ui, "Open with the library showing", shell.library) {
        shell.send(Command::Library(!shell.library));
    }
    if widgets::switch(ui, "Grid on the ground", lab.grid) {
        shell.send(Command::Grid(!lab.grid));
    }
    if widgets::switch(ui, "Height ruler", lab.ruler) {
        shell.send(Command::Ruler(!lab.ruler));
    }
    widgets::section(ui, "Pictures");
    if widgets::switch(ui, "Pictures of the plant alone", recording.plant_only) {
        shell.send(Command::PlantOnly(!recording.plant_only));
    }
    if widgets::switch(ui, "Save each frame as a PNG too", recording.pngs) {
        shell.send(Command::Pngs(!recording.pngs));
    }
    widgets::section(ui, "Last renders");
    let folder = lab.store.folder.as_ref().map_or_else(
        || "not kept: no cache folder known".to_owned(),
        |path| path.display().to_string(),
    );
    widgets::reading_row(ui, "kept in", &folder);
    widgets::reading_row(ui, "species kept", &lab.store.kept.len().to_string());
    ui.label(
        RichText::new(
            "Each species' last render is kept there, a picture and its settings, \
             and shown when it is chosen again. Delete the folder to forget them.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    if !shell.settings.message.is_empty() {
        ui.label(RichText::new(&shell.settings.message).color(theme::WARNING));
    }
}

/// Snapshots, recorded growth and GIFs, and the command for the same
/// picture without the window.
#[allow(clippy::too_many_lines)]
fn pictures(ui: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let shown = lab.shown.is_some();
    widgets::section(ui, "Pictures of");
    if let Some(index) = widgets::segmented(
        ui,
        &["The scene", "The plant alone"],
        usize::from(recording.plant_only),
    ) {
        shell.send(Command::PlantOnly(index == 1));
    }
    ui.label(
        RichText::new(if recording.plant_only {
            "On a clear background: transparent PNGs and GIFs"
        } else {
            "As the view shows it: ground, scale and measures"
        })
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    ui.label(RichText::new("Width").color(theme::TEXT_2));
    let look = SliderLook {
        step: 1.0,
        suffix: " px",
        decimals: Some(0),
        ..SliderLook::default()
    };
    if let Some(width) = widgets::slider(
        ui,
        "picture-width",
        f64::from(recording.width),
        128.0..=1920.0,
        look,
    ) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        shell.send(Command::PictureWidth(width.round() as u32));
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                shown,
                egui::Button::new(with_icon(ui, icons::CAMERA, "Snapshot", theme::TEXT)),
            )
            .on_hover_text("Save a PNG of the plant on screen (P)")
            .clicked()
        {
            shell.send(Command::Snapshot);
        }
    });

    widgets::section(ui, "Growth animations");
    if widgets::switch(ui, "Record growth", recording.record) {
        shell.send(Command::Record(!recording.record));
    }
    ui.label(
        RichText::new(
            "A frame each time a plant or a frame of its growth shows. Turn off \"Move the \
             camera with the plant\" to keep the camera still.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    ui.label(RichText::new("Each frame").color(theme::TEXT_2));
    let look = SliderLook {
        step: 10.0,
        suffix: " ms",
        logarithmic: true,
        decimals: Some(0),
    };
    let milliseconds = f64::from(recording.delay) * 10.0;
    if let Some(ms) = widgets::slider(ui, "frame-ms", milliseconds, 20.0..=2000.0, look) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        shell.send(Command::FrameMs(ms.round() as u32));
    }
    ui.label(RichText::new("Hold the last frame").color(theme::TEXT_2));
    let look = SliderLook {
        step: 0.1,
        suffix: " s",
        decimals: Some(1),
        ..SliderLook::default()
    };
    if let Some(seconds) = widgets::slider(
        ui,
        "hold",
        f64::from(recording.hold) / 100.0,
        0.0..=10.0,
        look,
    ) {
        shell.send(Command::HoldS(seconds));
    }
    if widgets::switch(ui, "Save each frame as a PNG too", recording.pngs) {
        shell.send(Command::Pngs(!recording.pngs));
    }
    let frames = recording.frames.len();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                frames > 0,
                egui::Button::new(with_icon(ui, icons::FILM_STRIP, "Save GIF", theme::TEXT)),
            )
            .on_hover_text("S")
            .clicked()
        {
            shell.send(Command::SaveGif);
        }
        if ui
            .add_enabled(frames > 0, egui::Button::new("Clear"))
            .clicked()
        {
            shell.send(Command::ClearFrames);
        }
        ui.label(
            RichText::new(format!("{frames} frames"))
                .font(egui::FontId::monospace(theme::MONOSPACE))
                .color(theme::TEXT_2),
        );
    });
    widgets::reading_row(ui, "saved in", &recording.folder.display().to_string());
    if !recording.message.is_empty() {
        ui.label(RichText::new(&recording.message).color(theme::TEXT_2));
    }

    if let Some(shown) = &lab.shown {
        widgets::section(ui, "Without the window");
        let text = thumbs_command(&shown.shot);
        command_text(ui, &text, shell);
    }
}

/// A command in monospace that can be selected, and a button that prints
/// it to the console.
fn command_text(ui: &mut Ui, text: &str, shell: &mut Shell) {
    egui::Frame::new()
        .fill(theme::SURFACE_0)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .font(egui::FontId::monospace(theme::MONOSPACE))
                        .color(theme::TEXT),
                )
                .selectable(true)
                .wrap(),
            );
        });
    if ui
        .button(with_icon(
            ui,
            icons::TERMINAL_WINDOW,
            "Print to the console",
            theme::TEXT,
        ))
        .clicked()
    {
        shell.send(Command::Print(text.to_owned()));
    }
}

/// The species' spec, as the library merges it.
fn spec(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let id = &lab.chosen;
    let Some(entry) = Library::builtin().entry(id) else {
        ui.label(RichText::new("Not in the library").color(theme::ERROR));
        return;
    };
    widgets::reading_row(ui, "file", &entry.file());
    ui.label(
        RichText::new(
            "Merged onto the rank files above it. Read-only: the spec's format is being \
             reworked.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    let source = entry.source();
    if ui
        .button(with_icon(
            ui,
            icons::TERMINAL_WINDOW,
            "Print to the console",
            theme::TEXT,
        ))
        .clicked()
    {
        shell.send(Command::Print(source.to_owned()));
    }
    egui::Frame::new()
        .fill(theme::SURFACE_0)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(source)
                        .font(egui::FontId::monospace(theme::SMALL))
                        .color(theme::TEXT_2),
                )
                .selectable(true)
                .wrap(),
            );
        });
}

/// The swatch's size, pixels, and the stem's radius it is drawn round,
/// metres.
const SWATCH: usize = 192;
const SWATCH_RADIUS: f32 = 0.25;

/// The species' bark pattern: a stretch of stem unrolled, and its numbers.
fn bark(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let Some(shown) = &lab.shown else {
        ui.label(RichText::new("Nothing grown yet").color(theme::TEXT_3));
        return;
    };
    let scene = &shown.scene;
    let Some(params) = scene.bark else {
        ui.label(RichText::new("This species' bark is plain: no pattern.").color(theme::TEXT_2));
        return;
    };
    let species = scene.facts.species.clone();
    if shell
        .bark
        .as_ref()
        .is_none_or(|(for_species, _)| *for_species != species)
    {
        let image = swatch(&params, wood_colour(scene));
        let texture = ui
            .ctx()
            .load_texture("plantlab-bark", image, egui::TextureOptions::LINEAR);
        shell.bark = Some((species, texture));
    }
    if let Some((_, texture)) = &shell.bark {
        let side = ui.available_width().min(260.0);
        ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(side, side)));
    }
    ui.label(
        RichText::new(format!(
            "A stem {:.0} cm across, unrolled: once round it left to right, as far along it \
             top to bottom; drawn by plantgen::bark::sample in the wood's mean colour.",
            SWATCH_RADIUS * 200.0
        ))
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    widgets::section(ui, "Pattern");
    let kind = match params.kind {
        1 => "furrowed",
        2 => "plated",
        3 => "fibrous",
        4 => "scaly",
        5 => "smooth",
        6 => "peeling",
        _ => "none",
    };
    widgets::reading_row(ui, "kind", kind);
    widgets::reading_row(ui, "cell width", &format!("{:.1} cm", params.scale * 100.0));
    widgets::reading_row(ui, "elongation", &format!("{:.2}", params.elongation));
    widgets::reading_row(ui, "fissures", &format!("{:.2} of a cell", params.fissure));
    widgets::reading_row(ui, "depth", &format!("{:.2}", params.depth));
    widgets::reading_row(ui, "contrast", &format!("{:.2}", params.contrast));
    widgets::reading_row(ui, "variation", &format!("{:.2}", params.variation));
    widgets::reading_row(
        ui,
        "smooth below",
        &format!("{:.1} cm radius", params.smooth_below * 100.0),
    );
    widgets::reading_row(ui, "lenticels", &format!("{:.2}", params.lenticels));
    widgets::reading_row(ui, "peeled", &format!("{:.2}", params.peeled));
    widgets::reading_row(ui, "roughness", &format!("{:.2}", params.roughness));
    ui.label(
        RichText::new("Editing waits on the bark work's decisions.")
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
}

/// The wood's mean colour where it carries bark, linear RGB.
fn wood_colour(scene: &plantlab_scene::Scene) -> [f32; 3] {
    let wood = &scene.wood;
    let mut sum = [0.0_f64; 3];
    let mut count = 0.0_f64;
    for (index, colour) in wood.colors.iter().enumerate() {
        if wood.bark_radius.get(index).is_some_and(|r| *r > 0.0) {
            for (total, channel) in sum.iter_mut().zip(colour) {
                *total += f64::from(*channel);
            }
            count += 1.0;
        }
    }
    if count == 0.0 {
        return [0.2, 0.15, 0.1];
    }
    #[allow(clippy::cast_possible_truncation)]
    sum.map(|total| (total / count) as f32)
}

/// The pattern round a stem of [`SWATCH_RADIUS`], one turn across and as
/// far along it, in `wood` shaded and turned to the peeled colour as
/// `plantgen::bark::sample` says.
fn swatch(params: &plantgen::bark::BarkParams, wood: [f32; 3]) -> egui::ColorImage {
    let along = std::f32::consts::TAU * SWATCH_RADIUS;
    #[allow(clippy::cast_precision_loss)]
    let footprint = along / SWATCH as f32;
    let mut pixels = Vec::with_capacity(SWATCH * SWATCH);
    for y in 0..SWATCH {
        for x in 0..SWATCH {
            #[allow(clippy::cast_precision_loss)]
            let (u, v) = (
                (x as f32 + 0.5) / SWATCH as f32,
                (y as f32 + 0.5) * footprint,
            );
            let sample = plantgen::bark::sample(params, u, v, SWATCH_RADIUS, footprint);
            let colour: [f32; 3] = std::array::from_fn(|channel| {
                let base = wood[channel] + (params.inner[channel] - wood[channel]) * sample.inner;
                (base * sample.shade).clamp(0.0, 1.0)
            });
            pixels.push(Color32::from_rgb(
                plantgen::raster::to_u8(linear_to_srgb(colour[0])),
                plantgen::raster::to_u8(linear_to_srgb(colour[1])),
                plantgen::raster::to_u8(linear_to_srgb(colour[2])),
            ));
        }
    }
    egui::ColorImage::new([SWATCH, SWATCH], pixels)
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// `icon` then `text` on one line, the icon in the icon font: Inter has
/// glyphs of its own where Phosphor keeps its icons, so an icon inside a
/// line of plain text would come out as Inter's glyph.
pub fn with_icon(ui: &Ui, icon: &str, text: &str, colour: egui::Color32) -> egui::text::LayoutJob {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon,
        0.0,
        egui::TextFormat::simple(theme::icon_font(ui, body.size), colour),
    );
    job.append(text, 8.0, egui::TextFormat::simple(body, colour));
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_become_dates() {
        assert_eq!(date(1.0), "1 January");
        assert_eq!(date(59.0), "28 February");
        assert_eq!(date(196.0), "15 July");
        assert_eq!(date(365.0), "31 December");
    }

    #[test]
    fn the_palette_lists_every_menu_entry_and_every_species() {
        let lab = super::super::tests::lab();
        let recording = Recording::new("pictures".into());
        let shell = Shell::new(crate::settings::Settings::default());
        let entries = palette_entries(&lab, &recording, &shell);
        let menu_entries: usize = menus(&lab, &recording, &shell)
            .iter()
            .map(|(_, items)| items.len())
            .sum();
        assert_eq!(entries.len(), menu_entries + lab.catalogue.items.len());
        assert!(entries.iter().any(|(entry, _)| entry.label == "grid"));
        assert!(entries.iter().any(|(entry, command)| entry.label
            == "species bigleaf maple acer macrophyllum"
            && *command == Command::Choose("acer-macrophyllum".into())));
    }
}
