//! What each panel holds, the menus, the palette's entries and the
//! settings. Each control reads [`Lab`] and [`Recording`] and queues a
//! [`Command`]; none changes them.

use std::sync::atomic::Ordering;

use bevy_egui::egui::{self, Color32, RichText, Ui};
use plantgen::library::Library;
use plantlab_scene::{Look, View};
use plantlab_ui::palette::Entry;
use plantlab_ui::widgets::{self, SliderLook};
use plantlab_ui::{icons, theme};

use super::command::{Command, Panel, Shell};
use super::shell::years;
use super::{Lab, bytes, keyframes, thumbs_command};
use crate::measure;
use crate::record::Recording;

/// Draws `panel`'s contents.
pub fn show(ui: &mut Ui, panel: Panel, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    match panel {
        Panel::Plant => plant(ui, lab, shell),
        Panel::Grow => grow(ui, lab, shell),
        Panel::View => view(ui, lab, shell),
        Panel::Readings => readings(ui, lab, shell),
        Panel::Pictures => pictures(ui, lab, recording, shell),
        Panel::Spec => spec(ui, lab, shell),
        Panel::Bark => bark(ui, lab, shell),
        Panel::Keys => keys(ui),
    }
}

/// The species, its age and the day of the year.
fn plant(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    widgets::section(ui, "Species");
    ui.add(
        egui::TextEdit::singleline(&mut shell.filter)
            .hint_text(format!("Find one of {} species", lab.species.len()))
            .desired_width(f32::INFINITY),
    );
    let filter = shell.filter.to_lowercase();
    egui::ScrollArea::vertical()
        .id_salt("species")
        .max_height(240.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, id) in lab.species.iter().enumerate() {
                if !filter.is_empty() && !id.contains(&filter) {
                    continue;
                }
                if ui
                    .selectable_label(index == lab.chosen, id.as_str())
                    .clicked()
                {
                    shell.send(Command::Species(index));
                }
            }
        });

    widgets::section(ui, "Age and season");
    if let Some(age) = lab.age {
        let youngest = age.min(1.0);
        let step = if youngest < 1.0 { 0.0 } else { 1.0 };
        ui.label(RichText::new("Age, years").color(theme::TEXT_2));
        let look = SliderLook {
            step,
            ..SliderLook::default()
        };
        let salt = ("age", lab.chosen);
        if let Some(age) = widgets::slider(ui, salt, age, youngest..=lab.oldest, look) {
            shell.send(Command::Age(age));
        }
        let ages = keyframes(&lab.species[lab.chosen]);
        if !ages.is_empty() {
            let text: Vec<String> = ages.iter().map(|age| years(*age)).collect();
            ui.label(
                RichText::new(format!("Keyframes: {}", text.join(", ")))
                    .size(theme::SMALL)
                    .color(theme::TEXT_3),
            );
        }
    }
    ui.label(RichText::new("Day of the year").color(theme::TEXT_2));
    let look = SliderLook {
        step: 1.0,
        decimals: Some(0),
        ..SliderLook::default()
    };
    if let Some(day) = widgets::slider(ui, "day", lab.day, 1.0..=365.0, look) {
        shell.send(Command::Day(day));
    }
    ui.label(
        RichText::new(date(lab.day))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );

    if let Some(shown) = &lab.shown {
        let facts = &shown.scene.facts;
        widgets::section(ui, "Grown under");
        widgets::reading_row(ui, "conditions", &facts.environment);
        widgets::reading_row(ui, "seed", &facts.seed.to_string());
        if let Some(entry) = Library::builtin().entry(&facts.species) {
            widgets::reading_row(ui, "spec", &entry.file());
        }
    }
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

/// What is growing, or why it stopped, and how far it got.
#[must_use]
pub fn state_line(lab: &Lab) -> (Color32, String) {
    if lab.growing.is_some() {
        let (_, text) = progress(lab);
        let paused = lab.control.pause.load(Ordering::Relaxed);
        let colour = if paused {
            theme::STATUS[1]
        } else {
            theme::ACCENT
        };
        let word = if paused { "Paused" } else { "Growing" };
        return (colour, format!("{word}: {text}"));
    }
    if !lab.message.is_empty() {
        return (theme::STATUS[2], lab.message.clone());
    }
    match &lab.shown {
        Some(shown) => (
            theme::STATUS[0],
            format!(
                "{}, {}, grown in {:.1} s",
                shown.scene.facts.species,
                years(shown.scene.facts.age),
                shown.seconds
            ),
        ),
        None => (theme::TEXT_3, "Nothing grown yet".into()),
    }
}

/// The growth's share done and its words.
fn progress(lab: &Lab) -> (f32, String) {
    let step = lab.control.step.load(Ordering::Relaxed);
    let steps = lab.control.steps.load(Ordering::Relaxed);
    let age = f64::from_bits(lab.control.age.load(Ordering::Relaxed));
    let seconds = lab.started.elapsed().as_secs_f64();
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

/// Grow, stop and pause, and showing the plant as it grows.
fn grow(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let growing = lab.growing.is_some();
    let paused = lab.control.pause.load(Ordering::Relaxed);
    widgets::section(ui, "Growth");
    if growing {
        let (share, text) = progress(lab);
        let label = if paused { "Paused" } else { "Growing" };
        widgets::progress(ui, share, label, &text);
    } else {
        let (_, text) = state_line(lab);
        ui.label(RichText::new(text).color(theme::TEXT_2));
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!growing, egui::Button::new("Grow"))
            .on_hover_text("Grow the plant with these settings (Enter)")
            .clicked()
        {
            shell.send(Command::Grow);
        }
        let pause = if paused { "Resume" } else { "Pause" };
        if ui
            .add_enabled(growing, egui::Button::new(pause))
            .on_hover_text("Space")
            .clicked()
        {
            shell.send(Command::Pause(!paused));
        }
        if ui
            .add_enabled(growing, egui::Button::new("Stop"))
            .on_hover_text("Stop growing; the last frame shown stays (Esc)")
            .clicked()
        {
            shell.send(Command::Stop);
        }
    });

    widgets::section(ui, "When to grow");
    if widgets::switch(ui, "Grow when the settings change", lab.auto) {
        shell.send(Command::GrowOnChange(!lab.auto));
    }
    if !lab.auto && lab.dirty && !growing {
        ui.label(RichText::new("Settings changed: press Grow").color(theme::WARNING));
    }

    widgets::section(ui, "Show it growing");
    if widgets::switch(ui, "Draw the plant as it grows", lab.live) {
        shell.send(Command::ShowGrowing(!lab.live));
    }
    ui.add_enabled_ui(lab.live, |ui| {
        ui.label(RichText::new("Every").color(theme::TEXT_2));
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
}

/// The level of detail, quality, look and view, and the measures.
fn view(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    widgets::section(ui, "Level of detail");
    if let Some(level) = widgets::segmented(ui, &["0", "1", "2", "3"], lab.level) {
        shell.send(Command::Level(level));
    }
    widgets::section(ui, "Quality");
    if let Some(index) = widgets::segmented(ui, &["Draft", "Standard"], usize::from(!lab.draft)) {
        shell.send(Command::Draft(index == 0));
    }
    widgets::section(ui, "Look");
    let looks = [Look::Review, Look::Photo];
    let chosen = looks.iter().position(|look| *look == lab.look).unwrap_or(0);
    if let Some(index) = widgets::segmented(ui, &["Review", "Photo"], chosen) {
        shell.send(Command::Look(looks[index]));
    }
    ui.label(
        RichText::new(
            "Review is fixed, so species compare fairly; photo adds soft shadows, sky light \
             and a filmic tone map.",
        )
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    widgets::section(ui, "Camera");
    let views = [View::Side, View::ThreeQuarter, View::Top];
    let chosen = views.iter().position(|view| *view == lab.view).unwrap_or(1);
    if let Some(index) = widgets::segmented(ui, &["Side", "Three-quarter", "Top"], chosen) {
        shell.send(Command::View(views[index]));
    }
    if ui.button("Frame the plant again (F)").clicked() {
        shell.send(Command::ResetCamera);
    }
    ui.label(
        RichText::new("Drag to turn, middle-drag or Shift-drag to move, scroll to come closer.")
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
    widgets::section(ui, "Measures");
    if widgets::switch(ui, "Grid on the ground (G)", lab.grid) {
        shell.send(Command::Grid(!lab.grid));
    }
    if let Some(shown) = &lab.shown
        && lab.grid
    {
        let step = measure::grid_step(shown.scene.ground_radius);
        ui.label(
            RichText::new(format!(
                "Squares {}, bold lines every {}",
                measure::length(step, step),
                measure::length(step * 5.0, step * 5.0)
            ))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
        );
    }
    if widgets::switch(ui, "Height ruler (R)", lab.ruler) {
        shell.send(Command::Ruler(!lab.ruler));
    }
}

/// The plant's measures and file sizes.
fn readings(ui: &mut Ui, lab: &Lab, _shell: &mut Shell) {
    let Some(shown) = &lab.shown else {
        ui.label(RichText::new("Nothing grown yet").color(theme::TEXT_3));
        return;
    };
    let facts = &shown.scene.facts;
    widgets::section(ui, "The plant");
    egui::Grid::new("readings-stats")
        .num_columns(2)
        .spacing([24.0, 10.0])
        .show(ui, |ui| {
            widgets::stat(ui, "Height", &format!("{:.2}", facts.height_m), "m");
            widgets::stat(
                ui,
                "Crown width",
                &format!("{:.2}", facts.crown_width_m),
                "m",
            );
            ui.end_row();
            widgets::stat(ui, "Age", &format!("{}", facts.age), "years");
            widgets::stat(ui, "Level", &facts.level.to_string(), "");
            ui.end_row();
            widgets::stat(ui, "Triangles", &facts.triangles.to_string(), "");
            widgets::stat(ui, "Cards", &facts.cards.to_string(), "");
            ui.end_row();
        });
    widgets::section(ui, "As files");
    let export = facts.export;
    widgets::reading_row(ui, "packaged", &bytes(export.package_mesh));
    widgets::reading_row(ui, "as .glb", &format!("about {}", bytes(export.glb)));
    widgets::reading_row(ui, "as .obj", &format!("about {}", bytes(export.obj)));
    widgets::section(ui, "Grown");
    let grown = if shown.live {
        format!("{:.1} s into growing", shown.seconds)
    } else {
        format!("in {:.1} s", shown.seconds)
    };
    widgets::reading_row(ui, "grown", &grown);
    widgets::reading_row(
        ui,
        "generator",
        &format!("revision {}", facts.generator_revision),
    );
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
    let id = &lab.species[lab.chosen];
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

/// Every key, in groups.
fn keys(ui: &mut Ui) {
    let pair = |key: &str, help: &str| (key.to_owned(), help.to_owned());
    let mut panels: Vec<(String, String)> = Panel::RAIL
        .iter()
        .map(|panel| pair(panel.key(), panel.title()))
        .collect();
    panels.push(pair("Shift+1", "a panel beside the open ones (any digit)"));
    panels.push(pair("F1", "these keys"));
    widgets::key_help(
        ui,
        &[
            (
                "Growing",
                vec![
                    pair("Enter", "grow"),
                    pair("Space", "pause or resume"),
                    pair("Esc", "stop growing"),
                ],
            ),
            (
                "View",
                vec![
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
            ("Panels", panels),
            (
                "Anywhere",
                vec![pair("Ctrl+P", "search or run"), pair("Ctrl+,", "settings")],
            ),
        ],
    );
    ui.label(
        RichText::new("Drag to turn, middle-drag or Shift-drag to move, scroll to come closer.")
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
    let growing = lab.growing.is_some();
    let paused = lab.control.pause.load(Ordering::Relaxed);
    let mut plant = vec![
        item(
            "Choose a species…",
            None,
            None,
            Command::Palette("species ".into()),
        ),
        item("Grow", Some("Enter"), None, Command::Grow),
    ];
    if growing {
        plant.push(item(
            if paused { "Resume" } else { "Pause" },
            Some("Space"),
            None,
            Command::Pause(!paused),
        ));
        plant.push(item("Stop", Some("Esc"), None, Command::Stop));
    }
    plant.extend([
        item(
            "Grow when the settings change",
            None,
            Some(lab.auto),
            Command::GrowOnChange(!lab.auto),
        ),
        item(
            "Show it growing",
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
    let mut view = Vec::new();
    for (name, value) in [
        ("Side", View::Side),
        ("Three-quarter", View::ThreeQuarter),
        ("Top", View::Top),
    ] {
        view.push(item(
            &format!("View from the {}", name.to_lowercase()),
            None,
            Some(lab.view == value),
            Command::View(value),
        ));
    }
    for level in 0..4 {
        view.push(item(
            &format!("Level of detail {level}"),
            None,
            Some(lab.level == level),
            Command::Level(level),
        ));
    }
    view.extend([
        item("Draft quality", None, Some(lab.draft), Command::Draft(true)),
        item(
            "Standard quality",
            None,
            Some(!lab.draft),
            Command::Draft(false),
        ),
        item(
            "Review look",
            None,
            Some(lab.look == Look::Review),
            Command::Look(Look::Review),
        ),
        item(
            "Photo look",
            None,
            Some(lab.look == Look::Photo),
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
    let panels = Panel::ALL
        .iter()
        .map(|panel| {
            item(
                panel.title(),
                Some(panel.key()),
                Some(shell.shows(*panel)),
                Command::Panel(*panel, false),
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
        ("Panels", panels),
        ("Help", help),
    ]
}

/// The palette's entries: every menu entry and every species.
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
    for (index, id) in lab.species.iter().enumerate() {
        entries.push((
            Entry {
                label: format!("species {id}"),
                detail: "grow this species".into(),
                chord: None,
            },
            Command::Species(index),
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
    if widgets::switch(ui, "Grow when the settings change", lab.auto) {
        shell.send(Command::GrowOnChange(!lab.auto));
    }
    if widgets::switch(ui, "Draw the plant as it grows", lab.live) {
        shell.send(Command::ShowGrowing(!lab.live));
    }
    if widgets::switch(ui, "Move the camera with the plant", lab.follow) {
        shell.send(Command::Follow(!lab.follow));
    }
    widgets::section(ui, "Measures");
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
    if !shell.settings.message.is_empty() {
        ui.label(RichText::new(&shell.settings.message).color(theme::WARNING));
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
        assert_eq!(entries.len(), menu_entries + lab.species.len());
        assert!(entries.iter().any(|(entry, _)| entry.label == "grid"));
        assert!(entries.iter().any(
            |(entry, command)| entry.label == "species acer-macrophyllum"
                && matches!(command, Command::Species(_))
        ));
    }
}
