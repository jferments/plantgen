//! The Studio: the top bar, the rail, the docked panels, the status bar,
//! the command palette and the settings, laid out as WorldLab's redesigned
//! interface is; and the Overlay, which hides the bars and floats one
//! panel at a time as a card over the plant.
//!
//! Everything here reads [`Lab`], [`Recording`] and [`Shell`] and queues
//! [`Command`]s; nothing changes them directly ([`super::command`]).
//!
//! The panels dock beside the 3D view and never cover it: the view's
//! camera draws only into the rectangle left between the bars, the rail
//! and the docks ([`fit_view`]). Docks take widths worked out from the
//! window's width alone, never from what they hold, and always leave the
//! view at least [`MIN_VIEW`] points.

use std::sync::atomic::Ordering;

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};
use egui::{Align, Align2, Layout, RichText, Ui};
use plantlab_scene::View;
use plantlab_ui::palette::{Action, Entry};
use plantlab_ui::{icons, theme, widgets};

use super::command::{Command, Panel, Shell};
use super::{Lab, Orbit, panels};
use crate::record::Recording;

/// The top bar's and the status bar's heights, points.
pub const TOP_BAR: f32 = 44.0;
pub const STATUS_BAR: f32 = 26.0;
/// A docked panel's width, points.
pub const DOCK: f32 = 320.0;
/// The least a dock is drawn at, in a window too narrow for [`DOCK`].
pub const DOCK_LEAST: f32 = 220.0;
/// The least a wide dock (a board of cards) takes, points.
pub const WIDE_LEAST: f32 = 380.0;
/// The 3D view is never narrower than this, points, so it still draws
/// and its GPU time still measures a drawn view.
pub const MIN_VIEW: f32 = 280.0;
/// The rail's width: an entry and its margins.
pub const RAIL: f32 = widgets::RAIL_WIDTH + 4.0;
/// Below this window width the top bar drops its age and day chip and
/// narrows the search field.
pub const NARROW_BAR: f32 = 1_180.0;
/// The Overlay's card, points.
pub const CARD_WIDTH: f32 = 340.0;
const MARGIN: f32 = 16.0;

/// The docks' widths for a window `window` points wide, with `rail` taken
/// by the rail: each panel in `open` in order, the narrow ones [`DOCK`]
/// and a wide one half of what the others leave, all shrunk or left out
/// (the oldest first) so the view keeps [`MIN_VIEW`]. From the window's
/// width alone, so nothing a panel holds can widen it.
#[must_use]
pub fn dock_widths(open: &[Panel], window: f32, rail: f32) -> Vec<(Panel, f32)> {
    let room = (window - rail - MIN_VIEW).max(0.0);
    #[allow(clippy::cast_precision_loss)]
    let others = open.iter().filter(|panel| !panel.wide()).count() as f32 * DOCK;
    // A wide dock takes half of what the other docks leave, so the view
    // is never narrower than the board.
    let wide = ((window - rail - others) * 0.5).max(WIDE_LEAST);
    let want = |panel: Panel| if panel.wide() { wide } else { DOCK };
    // Leave out the oldest until the rest fit at their least.
    let mut shown: Vec<Panel> = open.to_vec();
    #[allow(clippy::cast_precision_loss)]
    while shown.len() > 1 && shown.len() as f32 * DOCK_LEAST > room {
        shown.remove(0);
    }
    let mut widths: Vec<(Panel, f32)> = shown.iter().map(|&panel| (panel, want(panel))).collect();
    let total: f32 = widths.iter().map(|(_, width)| width).sum();
    if total > room {
        // A wide dock gives way first, down to a plain dock; then all
        // shrink alike.
        let narrow: f32 = widths
            .iter()
            .filter(|(panel, _)| !panel.wide())
            .map(|(_, width)| width)
            .sum();
        for (panel, width) in &mut widths {
            if panel.wide() {
                *width = (room - narrow).max(DOCK).min(*width);
            }
        }
        let total: f32 = widths.iter().map(|(_, width)| width).sum();
        if total > room {
            let scale = room / total;
            for (_, width) in &mut widths {
                *width = (*width * scale).max(DOCK_LEAST.min(room));
            }
        }
    }
    widths
}

/// Draws the interface and records the view's rectangle.
#[allow(clippy::needless_pass_by_value)]
pub fn draw(
    mut contexts: EguiContexts,
    lab: Res<Lab>,
    recording: Res<Recording>,
    mut shell: ResMut<Shell>,
    mut themed: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !*themed {
        // Once: the fonts are compared and installed for the next pass.
        theme::apply(ctx);
        *themed = true;
    }
    let ctx = ctx.clone();
    let shell = &mut *shell;
    shell.pixels_per_point = ctx.pixels_per_point();
    // As WorldLab does with egui 0.36: panels lie in a root Ui over the
    // whole window.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("plantlab"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let window = ctx.viewport_rect();
    if shell.overlay {
        shell.view = Some(window);
        overlay(&ctx, &lab, &recording, shell, window);
    } else {
        top_bar(&mut root, &lab, &recording, shell, window.width());
        status_bar(&mut root, &lab, shell);
        rail(&mut root, shell);
        for (panel, width) in dock_widths(&shell.open, window.width(), RAIL) {
            egui::Panel::left(egui::Id::new(("plantlab-dock", panel.short())))
                .resizable(false)
                .exact_size(width)
                .frame(
                    egui::Frame::new()
                        .fill(theme::SURFACE_1)
                        .stroke(egui::Stroke::new(1.0, theme::LINE))
                        .inner_margin(egui::Margin::symmetric(14, 12)),
                )
                .show(&mut root, |ui| {
                    panel_head(ui, panel, shell);
                    ui.add_space(theme::GAP);
                    egui::ScrollArea::vertical()
                        .id_salt(("dock", panel.short()))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            panels::show(ui, panel, &lab, &recording, shell);
                        });
                });
        }
        shell.view = Some(root.available_rect_before_wrap());
    }
    palette(&ctx, &lab, &recording, shell);
    settings_window(&ctx, &lab, &recording, shell);
}

/// A panel's head: its icon and title, what it holds, and a close button.
fn panel_head(ui: &mut Ui, panel: Panel, shell: &mut Shell) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(panel.icon())
                .font(theme::icon_font(ui, 16.0))
                .color(theme::TEXT_2),
        );
        ui.label(
            RichText::new(panel.title())
                .font(theme::strong_font(ui, theme::HEADING))
                .color(theme::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, "Close", false) {
                shell.toggle(panel, false);
            }
        });
    });
    ui.label(
        RichText::new(capitalised(panel.about()))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
}

/// `text` with its first letter capitalised.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The top bar: the menus, the species, growing, the age and day, the
/// view, the tools, and at the right the search and the settings.
fn top_bar(root: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell, window: f32) {
    let narrow = window < NARROW_BAR;
    egui::Panel::top("plantlab-top-bar")
        .resizable(false)
        .exact_size(TOP_BAR)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE_1)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .inner_margin(egui::Margin::symmetric(10, 0)),
        )
        .show(root, |ui| {
            // The right group first, so the left one takes what is left.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(
                    ui,
                    icons::GEAR_SIX,
                    "Settings (Ctrl+,)",
                    shell.settings_open,
                ) {
                    let open = shell.settings_open;
                    shell.send(Command::Settings(!open));
                }
                let width = if narrow { 150.0 } else { 220.0 };
                if widgets::search_field(ui, "Search or run…", "Ctrl+P", width) {
                    shell.send(Command::Palette(String::new()));
                }
                ui.add_space(theme::GAP);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    menus(ui, lab, recording, shell);
                    species_chip(ui, lab, shell, if narrow { 150.0 } else { 220.0 });
                    ui.separator();
                    grow_buttons(ui, lab, shell);
                    if !narrow {
                        ui.add_space(theme::GAP_SMALL);
                        age_and_day(ui, lab, shell);
                    }
                    ui.separator();
                    let views = [View::Side, View::ThreeQuarter, View::Top];
                    let chosen = views.iter().position(|view| *view == lab.view).unwrap_or(1);
                    if let Some(index) = widgets::segmented(ui, &["Side", "¾", "Top"], chosen) {
                        shell.send(Command::View(views[index]));
                    }
                    ui.separator();
                    tools(ui, lab, recording, shell);
                });
            });
        });
}

/// The species on screen; a click opens the palette on the species.
fn species_chip(ui: &mut Ui, lab: &Lab, shell: &mut Shell, width: f32) {
    let name = &lab.species[lab.chosen];
    let response = ui
        .scope(|ui| {
            ui.set_max_width(width);
            ui.add(
                egui::Button::new(panels::with_icon(ui, icons::LEAF, name, theme::TEXT)).truncate(),
            )
        })
        .inner;
    if response
        .on_hover_text("Choose a species (type to find it)")
        .clicked()
    {
        shell.send(Command::Palette("species ".into()));
    }
}

/// Grow, pause or resume, and stop.
fn grow_buttons(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let growing = lab.growing.is_some();
    let paused = lab.control.pause.load(Ordering::Relaxed);
    ui.add_enabled_ui(!growing, |ui| {
        if widgets::icon_button(ui, icons::PLAY, "Grow (Enter)", false) {
            shell.send(Command::Grow);
        }
    });
    ui.add_enabled_ui(growing, |ui| {
        let (icon, tip) = if paused {
            (icons::PLAY_PAUSE, "Resume (Space)")
        } else {
            (icons::PAUSE, "Pause (Space)")
        };
        if widgets::icon_button(ui, icon, tip, paused) {
            shell.send(Command::Pause(!paused));
        }
        if widgets::icon_button(ui, icons::STOP, "Stop; the last frame stays (Esc)", false) {
            shell.send(Command::Stop);
        }
    });
}

/// The age and the day; a click opens the Plant panel.
fn age_and_day(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let age = lab.age.map_or_else(|| "…".to_owned(), years);
    let text = RichText::new(format!("{age}  ·  day {:.0}", lab.day))
        .font(egui::FontId::monospace(theme::MONOSPACE))
        .color(theme::TEXT_2);
    if ui
        .add(egui::Button::new(text).frame(false))
        .on_hover_text("The plant's age and the day of the year: open Plant")
        .clicked()
        && !shell.shows(Panel::Plant)
    {
        shell.toggle(Panel::Plant, false);
    }
}

/// An age in years, short.
pub fn years(age: f64) -> String {
    if (age - age.round()).abs() < 1e-6 {
        format!("{age:.0} y")
    } else {
        format!("{age:.1} y")
    }
}

/// The tools: reset the camera, the grid, the ruler, a snapshot, recording
/// growth, saving the GIF and the Overlay.
fn tools(ui: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let shown = lab.shown.is_some();
    if widgets::icon_button(ui, icons::FRAME_CORNERS, "Frame the plant again (F)", false) {
        shell.send(Command::ResetCamera);
    }
    if widgets::icon_button(ui, icons::GRID_FOUR, "Grid on the ground (G)", lab.grid) {
        shell.send(Command::Grid(!lab.grid));
    }
    if widgets::icon_button(ui, icons::RULER, "Height ruler (R)", lab.ruler) {
        shell.send(Command::Ruler(!lab.ruler));
    }
    ui.add_enabled_ui(shown, |ui| {
        if widgets::icon_button(ui, icons::CAMERA, "Snapshot: save a PNG (P)", false) {
            shell.send(Command::Snapshot);
        }
    });
    if widgets::icon_button(
        ui,
        icons::RECORD,
        "Record growth: a frame each time a plant or a frame of its growth shows",
        recording.record,
    ) {
        shell.send(Command::Record(!recording.record));
    }
    ui.add_enabled_ui(!recording.frames.is_empty(), |ui| {
        let tip = format!("Save the {} frames as a GIF (S)", recording.frames.len());
        if widgets::icon_button(ui, icons::FILM_STRIP, &tip, false) {
            shell.send(Command::SaveGif);
        }
    });
    if widgets::icon_button(
        ui,
        icons::CORNERS_OUT,
        "Overlay: the plant fills the window, one panel floats (O)",
        false,
    ) {
        shell.send(Command::Overlay(true));
    }
}

/// The menus' button: every menu, each entry a command the palette finds
/// too.
fn menus(ui: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let icon = RichText::new(icons::LIST).font(theme::icon_font(ui, 18.0));
    ui.menu_button(icon, |ui| {
        for (menu, entries) in super::panels::menus(lab, recording, shell) {
            ui.menu_button(menu, |ui| {
                for item in entries {
                    if widgets::menu_item(ui, &item.label, item.key.as_deref(), item.checked) {
                        shell.send(item.command);
                    }
                }
            });
        }
    })
    .response
    .on_hover_text("Menus");
}

/// The rail: the panels' buttons, the keys at its foot.
fn rail(root: &mut Ui, shell: &mut Shell) {
    egui::Panel::left("plantlab-rail")
        .resizable(false)
        .exact_size(RAIL)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE_1)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .inner_margin(egui::Margin::symmetric(2, 6)),
        )
        .show(root, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for panel in Panel::RAIL {
                rail_entry(ui, panel, shell);
            }
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                rail_entry(ui, Panel::Keys, shell);
            });
        });
}

fn rail_entry(ui: &mut Ui, panel: Panel, shell: &mut Shell) {
    let open = shell.shows(panel);
    let tip = format!("{}: {} ({})", panel.title(), panel.about(), panel.key());
    let tip = if open {
        format!("{tip}\nClick to close")
    } else {
        format!("{tip}\nShift+click opens it beside the others")
    };
    if widgets::rail_button(ui, panel.icon(), panel.short(), &tip, open) {
        let shift = ui.input(|input| input.modifiers.shift);
        shell.toggle(panel, shift);
    }
}

/// The status bar: what is growing or why it stopped, the plant's size,
/// the generator's revision, and a setting that could not be saved.
fn status_bar(root: &mut Ui, lab: &Lab, shell: &Shell) {
    egui::Panel::bottom("plantlab-status-bar")
        .resizable(false)
        .exact_size(STATUS_BAR)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE_1)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .inner_margin(egui::Margin::symmetric(10, 0)),
        )
        .show(root, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "generator revision {}",
                        plantgen::GENERATOR_REVISION
                    ))
                    .size(theme::SMALL)
                    .color(theme::TEXT_3),
                );
                if !shell.settings.message.is_empty() {
                    ui.separator();
                    ui.label(
                        RichText::new(&shell.settings.message)
                            .size(theme::SMALL)
                            .color(theme::WARNING),
                    )
                    .on_hover_text("The settings file");
                }
                if let Some(shown) = &lab.shown {
                    let facts = &shown.scene.facts;
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{:.2} m tall · {:.2} m across · {} triangles",
                            facts.height_m, facts.crown_width_m, facts.triangles
                        ))
                        .font(egui::FontId::monospace(theme::SMALL))
                        .color(theme::TEXT_2),
                    );
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let (dot, text) = super::panels::state_line(lab);
                    ui.label(RichText::new("●").size(theme::SMALL).color(dot));
                    ui.add(
                        egui::Label::new(
                            RichText::new(text).size(theme::SMALL).color(theme::TEXT_2),
                        )
                        .truncate(),
                    );
                });
            });
        });
}

/// The Overlay: the dock of the rail's panels floating at the foot of the
/// view and one panel as a card over it.
fn overlay(
    ctx: &egui::Context,
    lab: &Lab,
    recording: &Recording,
    shell: &mut Shell,
    window: egui::Rect,
) {
    egui::Area::new(egui::Id::new("plantlab-overlay-dock"))
        .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -MARGIN))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            widgets::floating_frame(ui).show(ui, |ui| {
                ui.horizontal(|ui| {
                    for panel in Panel::RAIL {
                        let tip = format!("{}: {} ({})", panel.title(), panel.about(), panel.key());
                        if widgets::dock_button(ui, panel.icon(), &tip, shell.card == Some(panel)) {
                            shell.toggle(panel, false);
                        }
                    }
                    ui.separator();
                    let running = lab.growing.is_some();
                    let (icon, tip, command) = if running {
                        (icons::STOP, "Stop (Esc)", Command::Stop)
                    } else {
                        (icons::PLAY, "Grow (Enter)", Command::Grow)
                    };
                    if widgets::dock_button(ui, icon, tip, false) {
                        shell.send(command);
                    }
                    if widgets::dock_button(ui, icons::CAMERA, "Snapshot (P)", false) {
                        shell.send(Command::Snapshot);
                    }
                    if widgets::dock_button(
                        ui,
                        icons::MAGNIFYING_GLASS,
                        "Search or run (Ctrl+P)",
                        false,
                    ) {
                        shell.send(Command::Palette(String::new()));
                    }
                    if widgets::dock_button(
                        ui,
                        icons::CORNERS_IN,
                        "Leave the Overlay (O or Esc)",
                        true,
                    ) {
                        shell.send(Command::Overlay(false));
                    }
                });
            });
        });
    if let Some(panel) = shell.card {
        let height =
            (window.height() - 2.0 * MARGIN - widgets::DOCK_BUTTON - 3.0 * MARGIN).max(160.0);
        egui::Area::new(egui::Id::new("plantlab-overlay-card"))
            .anchor(Align2::LEFT_TOP, egui::vec2(MARGIN, MARGIN))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                widgets::floating_frame(ui)
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.set_width(CARD_WIDTH);
                        panel_head(ui, panel, shell);
                        ui.add_space(theme::GAP);
                        egui::ScrollArea::vertical()
                            .id_salt(("card", panel.short()))
                            .max_height(height)
                            .show(ui, |ui| {
                                panels::show(ui, panel, lab, recording, shell);
                            });
                    });
            });
    }
}

/// The command palette: every command and every species, best match
/// first.
fn palette(ctx: &egui::Context, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let entries = super::panels::palette_entries(lab, recording, shell);
    // Opening clears the field, so a fill goes in once the palette has
    // drawn this frame.
    let fill = shell.palette_fill.take();
    let labels: Vec<Entry> = entries.iter().map(|(entry, _)| entry.clone()).collect();
    let within = shell.view.unwrap_or_else(|| ctx.viewport_rect());
    let open = shell.palette_open;
    match shell.palette.show(ctx, open, &labels, within) {
        Some(Action::Enter(Some(index)) | Action::Pick(index)) => {
            if let Some((_, command)) = entries.get(index) {
                shell.send(command.clone());
            }
            shell.send(Command::ClosePalette);
        }
        Some(Action::Enter(None)) => {
            shell.palette.note =
                "Nothing matches: try a species' name or a word such as grid".into();
        }
        Some(Action::Fill(index)) => {
            if let Some((entry, _)) = entries.get(index) {
                let label = entry.label.clone();
                shell.palette.fill(&label);
            }
        }
        Some(Action::Close) => shell.send(Command::ClosePalette),
        None => {}
    }
    if open && let Some(fill) = fill {
        shell.palette.fill(&fill);
    }
}

/// The settings: what is saved between runs, each control saving its own
/// setting.
fn settings_window(ctx: &egui::Context, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    if !shell.settings_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(Align2::RIGHT_TOP, egui::vec2(-MARGIN, TOP_BAR + MARGIN))
        .show(ctx, |ui| {
            panels::settings(ui, lab, recording, shell);
        });
    if !open {
        shell.send(Command::Settings(false));
    }
}

/// Gives the 3D camera the view the interface left, before Bevy computes
/// its projection: a viewport in physical pixels, or the whole window.
pub fn fit_view(
    shell: Res<Shell>,
    mut lab: ResMut<Lab>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<Orbit>>,
) {
    let (Ok(window), Some(view)) = (windows.single(), shell.view) else {
        return;
    };
    let size = UVec2::new(window.physical_width(), window.physical_height());
    let physical = viewport(view, window.scale_factor(), size);
    if view.height() > 1.0 {
        let aspect = f64::from(view.width() / view.height());
        if (aspect - lab.aspect).abs() > 0.01 {
            lab.aspect = aspect;
        }
    }
    for mut camera in &mut cameras {
        let current = camera
            .viewport
            .as_ref()
            .map(|viewport| (viewport.physical_position, viewport.physical_size));
        if current != physical {
            camera.viewport = physical.map(|(physical_position, physical_size)| Viewport {
                physical_position,
                physical_size,
                ..default()
            });
        }
    }
}

/// `view` (logical points) as a camera viewport inside a window of
/// `window` physical pixels: `None` when it is the whole window.
#[must_use]
pub fn viewport(view: egui::Rect, pixels_per_point: f32, window: UVec2) -> Option<(UVec2, UVec2)> {
    if window.x == 0 || window.y == 0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let clamp =
        |value: f32, limit: u32| (value * pixels_per_point).round().clamp(0.0, limit as f32) as u32;
    let (x0, y0) = (
        clamp(view.min.x, window.x - 1),
        clamp(view.min.y, window.y - 1),
    );
    let (x1, y1) = (clamp(view.max.x, window.x), clamp(view.max.y, window.y));
    let size = UVec2::new(x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
    let position = UVec2::new(x0, y0);
    (position != UVec2::ZERO || size != window).then_some((position, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docks_leave_the_view_its_least_width() {
        for window in [700.0_f32, 1_000.0, 1_500.0, 2_400.0] {
            for open in [
                vec![Panel::Plant],
                vec![Panel::Readings],
                vec![Panel::Plant, Panel::Readings],
                vec![Panel::Plant, Panel::Grow, Panel::View, Panel::Readings],
            ] {
                let widths = dock_widths(&open, window, RAIL);
                let used: f32 = widths.iter().map(|(_, width)| width).sum();
                assert!(
                    window - RAIL - used >= MIN_VIEW - 0.5,
                    "{window} {open:?} {widths:?}"
                );
                assert!(!widths.is_empty());
                // The newest panel always shows.
                assert_eq!(widths.last().map(|(panel, _)| *panel), open.last().copied());
            }
        }
        // A wide window gives a board half of what the other docks leave.
        for open in [
            vec![Panel::Readings],
            vec![Panel::Plant, Panel::Bark, Panel::Readings],
        ] {
            let widths = dock_widths(&open, 2_400.0, RAIL);
            let used: f32 = widths.iter().map(|(_, width)| width).sum();
            let board = widths.last().map_or(0.0, |(_, width)| *width);
            assert!(2_400.0 - RAIL - used >= board - 0.5, "{widths:?}");
        }
    }

    #[test]
    fn docks_follow_the_window_alone() {
        // The same width gives the same docks, whatever happened before.
        let open = [Panel::Plant, Panel::Readings];
        assert_eq!(
            dock_widths(&open, 1_234.0, RAIL),
            dock_widths(&open, 1_234.0, RAIL)
        );
    }

    #[test]
    fn the_view_becomes_a_clamped_viewport_or_the_whole_window() {
        let window = UVec2::new(1600, 900);
        let rect =
            |x0, y0, x1, y1| egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
        assert_eq!(viewport(rect(0.0, 0.0, 1600.0, 900.0), 1.0, window), None);
        assert_eq!(
            viewport(rect(380.0, 44.0, 1600.0, 874.0), 1.0, window),
            Some((UVec2::new(380, 44), UVec2::new(1220, 830)))
        );
        assert_eq!(
            viewport(rect(380.0, 44.0, 1600.0, 874.0), 0.5, window),
            Some((UVec2::new(190, 22), UVec2::new(610, 415)))
        );
    }
}
