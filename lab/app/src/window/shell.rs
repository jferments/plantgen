//! The Studio: the top bar, the library browser docked at the left, the
//! inspector docked at the right, the status bar, the command palette
//! and the settings; over the 3D view, its toolbar (the views, the
//! measures, the look and pictures), the species' last render while it is
//! not on screen, what changed since the plant on screen was made, and the
//! growth's progress. The Overlay hides the bars and the docks.
//!
//! Everything here reads [`Lab`], [`Recording`] and [`Shell`] and queues
//! [`Command`]s; nothing changes them directly ([`super::command`]).
//!
//! The docks sit beside the 3D view and never cover it: the view's camera
//! draws only into the rectangle left between the bars and the docks
//! ([`fit_view`]). Docks take widths worked out from the window's width
//! alone, never from what they hold, and always leave the view at least
//! [`MIN_VIEW`] points.

use std::sync::atomic::Ordering;

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};
use egui::{Align, Align2, Layout, RichText, Ui};
use plantlab_scene::{Look, View};
use plantlab_ui::palette::{Action, Entry};
use plantlab_ui::{icons, theme, widgets};

use super::command::{Command, Panel, Shell};
use super::{Lab, Orbit, browser, panels, store};
use crate::record::Recording;

/// The top bar's and the status bar's heights, points.
pub const TOP_BAR: f32 = 44.0;
pub const STATUS_BAR: f32 = 26.0;
/// The library's and the inspector's widths, points.
pub const LIBRARY: f32 = 300.0;
pub const INSPECTOR: f32 = 340.0;
/// The least a dock is drawn at, in a window too narrow for its width.
pub const DOCK_LEAST: f32 = 240.0;
/// The least a wide inspector page (a board of cards) takes, points.
pub const WIDE_LEAST: f32 = 380.0;
/// The 3D view is never narrower than this, points, so it still draws
/// and its GPU time still measures a drawn view.
pub const MIN_VIEW: f32 = 300.0;
/// Below this window width the top bar leaves out the species' ranks.
pub const NARROW_BAR: f32 = 1_180.0;
const MARGIN: f32 = 12.0;

/// The views, as the toolbar and the keys 1 to 4 offer them.
/// The view's width from which the toolbar shows its measures, and its
/// looks and picture tools too, points.
const TOOLBAR_MEASURES: f32 = 420.0;
const TOOLBAR_FULL: f32 = 640.0;

pub const VIEWS: [(View, &str, &str); 4] = [
    (
        View::Side,
        "Side",
        "From the side, level with the plant (1)",
    ),
    (
        View::ThreeQuarter,
        "¾",
        "From the south-east, a little above (2)",
    ),
    (
        View::Oblique,
        "45°",
        "From the south-east, 45 degrees down (3)",
    ),
    (View::Top, "Top", "Straight down (4)"),
];

/// The library's and the inspector's widths for a window `window` points
/// wide, each `None` when it does not show: the library [`LIBRARY`], the
/// inspector [`INSPECTOR`] or, for a wide page, half of what the library
/// leaves; both shrunk (the inspector first) and at last the library left
/// out, so the view keeps [`MIN_VIEW`]. From the window's width alone, so
/// nothing a dock holds can widen it.
#[must_use]
pub fn dock_widths(
    window: f32,
    library: bool,
    inspector: Option<Panel>,
) -> (Option<f32>, Option<f32>) {
    let room = (window - MIN_VIEW).max(0.0);
    let mut left = library.then_some(LIBRARY);
    let mut right = inspector.map(|panel| {
        if panel.wide() {
            ((window - left.unwrap_or(0.0)) * 0.5).max(WIDE_LEAST)
        } else {
            INSPECTOR
        }
    });
    let total = |left: Option<f32>, right: Option<f32>| left.unwrap_or(0.0) + right.unwrap_or(0.0);
    if total(left, right) > room
        && let Some(width) = right.as_mut()
    {
        *width = (room - left.unwrap_or(0.0)).max(DOCK_LEAST).min(*width);
    }
    if total(left, right) > room
        && let Some(width) = left.as_mut()
    {
        *width = (room - right.unwrap_or(0.0)).max(DOCK_LEAST).min(*width);
    }
    if total(left, right) > room && right.is_some() {
        left = None;
    }
    if total(left, right) > room {
        // A window narrower than one dock and the view: the dock alone.
        right = right.map(|width| width.min(room.max(DOCK_LEAST.min(window))));
        left = left.map(|width| width.min(room.max(DOCK_LEAST.min(window))));
    }
    (left, right)
}

/// Draws the interface and records the view's rectangle.
#[allow(clippy::needless_pass_by_value)]
pub fn draw(
    mut contexts: EguiContexts,
    lab: Res<Lab>,
    recording: Res<Recording>,
    readings: Res<super::readings::Readings>,
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
    } else {
        top_bar(&mut root, &lab, &recording, shell, window.width());
        status_bar(&mut root, &lab, &recording, shell);
        let inspector = shell.inspector.then_some(shell.tab);
        let (left, right) = dock_widths(window.width(), shell.library, inspector);
        if let Some(width) = left {
            egui::Panel::left("plantlab-library")
                .resizable(false)
                .exact_size(width)
                .frame(dock_frame())
                .show(&mut root, |ui| browser::show(ui, &lab, shell));
        }
        if let Some(width) = right {
            egui::Panel::right("plantlab-inspector")
                .resizable(false)
                .exact_size(width)
                .frame(dock_frame())
                .show(&mut root, |ui| {
                    inspector_head(ui, shell);
                    panels::show(ui, shell.tab, &lab, &recording, &readings, shell);
                });
        }
        shell.view = Some(root.available_rect_before_wrap());
    }
    if let Some(view) = shell.view {
        cover(&ctx, &lab, shell, view);
        toolbar(&ctx, &lab, &recording, shell, view);
        changes(&ctx, &lab, shell, view);
        progress(&ctx, &lab, shell, view);
    }
    palette(&ctx, &lab, &recording, shell);
    settings_window(&ctx, &lab, &recording, shell);
}

fn dock_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE_1)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .inner_margin(egui::Margin::symmetric(14, 10))
}

/// The inspector's tabs, then its page's title and what it holds.
fn inspector_head(ui: &mut Ui, shell: &mut Shell) {
    let tabs: Vec<(&str, &str)> = Panel::ALL
        .iter()
        .map(|panel| (panel.icon(), panel.short()))
        .collect();
    let chosen = Panel::ALL
        .iter()
        .position(|panel| *panel == shell.tab)
        .unwrap_or(0);
    if let Some(index) = widgets::tabs(ui, &tabs, chosen) {
        shell.send(Command::Tab(Panel::ALL[index]));
    }
    ui.add_space(theme::GAP);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(shell.tab.short())
                .font(theme::strong_font(ui, theme::HEADING))
                .color(theme::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, "Hide the inspector (I)", false) {
                shell.send(Command::Inspector(false));
            }
        });
    });
    ui.label(
        RichText::new(capitalised(shell.tab.about()))
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
    ui.add_space(theme::GAP_SMALL);
}

/// `text` with its first letter capitalised.
pub fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// An age in years, whole ages without a decimal.
#[must_use]
pub fn years(age: f64) -> String {
    if (age - age.round()).abs() < 1e-6 {
        format!("{age:.0} y")
    } else {
        format!("{age:.1} y")
    }
}

/// The top bar: the menus, the library's switch, the chosen species with
/// its ranks, and at the right the search, the inspector's switch and the
/// settings.
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
                if widgets::icon_button(
                    ui,
                    icons::SIDEBAR_SIMPLE,
                    "Inspector: the render's settings and the plant's pages (I)",
                    shell.inspector,
                ) {
                    let open = shell.inspector;
                    shell.send(Command::Inspector(!open));
                }
                let width = if narrow { 170.0 } else { 240.0 };
                if widgets::search_field(ui, "Search species or commands…", "Ctrl+P", width) {
                    shell.send(Command::Palette(String::new()));
                }
                ui.add_space(theme::GAP);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    menus(ui, lab, recording, shell);
                    if widgets::icon_button(
                        ui,
                        icons::SIDEBAR,
                        "Library: every species by group, order, family and genus (L)",
                        shell.library,
                    ) {
                        let open = shell.library;
                        shell.send(Command::Library(!open));
                    }
                    ui.label(
                        RichText::new("PlantLab")
                            .font(theme::strong_font(ui, theme::BODY))
                            .color(theme::TEXT_2),
                    );
                    ui.separator();
                    title(ui, lab, shell, narrow);
                });
            });
        });
}

/// The chosen species: its ranks, each a link that finds it in the
/// library, then its common and scientific names.
fn title(ui: &mut Ui, lab: &Lab, shell: &mut Shell, narrow: bool) {
    let Some(item) = lab.catalogue.get(&lab.chosen) else {
        return;
    };
    if !narrow {
        for rank in [&item.order, &item.family] {
            let response = ui.add(
                egui::Label::new(RichText::new(rank).size(theme::SMALL).color(theme::TEXT_3))
                    .sense(egui::Sense::click()),
            );
            if response.on_hover_text("Find it in the library").clicked() {
                shell.filter.clone_from(rank);
                shell.send(Command::Library(true));
            }
            ui.label(
                RichText::new(icons::CARET_RIGHT)
                    .font(theme::icon_font(ui, 10.0))
                    .color(theme::TEXT_3),
            );
        }
    }
    ui.label(
        RichText::new(item.title())
            .font(theme::strong_font(ui, theme::BODY))
            .color(theme::TEXT),
    );
    if !item.common.is_empty() {
        ui.label(
            RichText::new(&item.scientific)
                .italics()
                .size(theme::SMALL)
                .color(theme::TEXT_2),
        );
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

/// The status bar: what is growing or why it stopped, the plant's size,
/// the generator's revision, a setting that could not be saved and what
/// was last saved.
fn status_bar(root: &mut Ui, lab: &Lab, recording: &Recording, shell: &Shell) {
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
                if !recording.message.is_empty() {
                    ui.separator();
                    ui.add(
                        egui::Label::new(
                            RichText::new(&recording.message)
                                .size(theme::SMALL)
                                .color(theme::TEXT_2),
                        )
                        .truncate(),
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

/// The toolbar floating at the top of the view: the views, framing, the
/// measures, the look, pictures, and the Overlay.
fn toolbar(
    ctx: &egui::Context,
    lab: &Lab,
    recording: &Recording,
    shell: &mut Shell,
    view: egui::Rect,
) {
    // A narrow view keeps the views and the Overlay; the rest are in the
    // View and Pictures menus.
    let measures = view.width() >= TOOLBAR_MEASURES;
    let full = view.width() >= TOOLBAR_FULL;
    egui::Area::new(egui::Id::new("plantlab-toolbar"))
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(view.center().x, view.top() + MARGIN))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            widgets::floating_frame(ui).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let chosen = VIEWS
                        .iter()
                        .position(|(view, _, _)| *view == lab.recipe.view)
                        .unwrap_or(1);
                    let names: Vec<&str> = VIEWS.iter().map(|(_, name, _)| *name).collect();
                    let picked = widgets::segmented(ui, &names, chosen);
                    if let Some(index) = picked {
                        shell.send(Command::View(VIEWS[index].0));
                    }
                    if widgets::icon_button(
                        ui,
                        icons::FRAME_CORNERS,
                        "Frame the plant again (F)",
                        false,
                    ) {
                        shell.send(Command::ResetCamera);
                    }
                    if measures {
                        ui.separator();
                        if widgets::icon_button(
                            ui,
                            icons::GRID_FOUR,
                            "Grid on the ground (G)",
                            lab.grid,
                        ) {
                            shell.send(Command::Grid(!lab.grid));
                        }
                        if widgets::icon_button(ui, icons::RULER, "Height ruler (R)", lab.ruler) {
                            shell.send(Command::Ruler(!lab.ruler));
                        }
                    }
                    if full {
                        ui.separator();
                        looks_and_pictures(ui, lab, recording, shell);
                    }
                    let (icon, tip) = if shell.overlay {
                        (icons::CORNERS_IN, "Leave the Overlay (O or Esc)")
                    } else {
                        (
                            icons::CORNERS_OUT,
                            "Overlay: the plant fills the window (O)",
                        )
                    };
                    if widgets::icon_button(ui, icon, tip, shell.overlay) {
                        let on = shell.overlay;
                        shell.send(Command::Overlay(!on));
                    }
                });
            });
        });
}

/// The toolbar's looks, snapshot, recording and GIF.
fn looks_and_pictures(ui: &mut Ui, lab: &Lab, recording: &Recording, shell: &mut Shell) {
    let looks = [Look::Review, Look::Photo];
    let chosen = looks
        .iter()
        .position(|look| *look == lab.recipe.look)
        .unwrap_or(0);
    if let Some(index) = widgets::segmented(ui, &["Review", "Photo"], chosen) {
        shell.send(Command::Look(looks[index]));
    }
    ui.separator();
    let shown = lab.shown.is_some();
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
    if !recording.frames.is_empty() {
        let tip = format!("Save the {} frames as a GIF (S)", recording.frames.len());
        if widgets::icon_button(ui, icons::FILM_STRIP, &tip, false) {
            shell.send(Command::SaveGif);
        }
    }
}

/// Over the view while the chosen species is not on screen: its last
/// render and the settings it was made with, or, never rendered, what to
/// do; and Generate.
fn cover(ctx: &egui::Context, lab: &Lab, shell: &mut Shell, view: egui::Rect) {
    if lab.shows_chosen() {
        return;
    }
    let Some(item) = lab.catalogue.get(&lab.chosen) else {
        return;
    };
    let kept = lab.store.kept.get(&lab.chosen);
    let picture = kept.and_then(|_| browser::picture(ctx, &lab.store, shell, &lab.chosen));
    let growing = lab.growing.is_some();
    egui::Area::new(egui::Id::new("plantlab-cover"))
        .fixed_pos(view.min)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(view.size(), egui::Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, theme::SURFACE_0);
            let card_height = 118.0;
            let room = egui::Rect::from_min_max(
                rect.min + egui::vec2(MARGIN * 2.0, 60.0),
                rect.max - egui::vec2(MARGIN * 2.0, card_height + MARGIN * 3.0),
            );
            if let Some(texture) = &picture {
                let size = texture.size_vec2();
                let scale = (room.width() / size.x).min(room.height() / size.y).min(1.5);
                let shown = egui::Rect::from_center_size(room.center(), size * scale);
                ui.painter().image(
                    texture.id(),
                    shown,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.painter().text(
                    room.center() - egui::vec2(0.0, 24.0),
                    Align2::CENTER_CENTER,
                    icons::PLANT,
                    theme::icon_font(ui, 64.0),
                    theme::SURFACE_4,
                );
            }
            let card = egui::Rect::from_center_size(
                egui::pos2(
                    rect.center().x,
                    rect.bottom() - MARGIN * 2.0 - card_height / 2.0,
                ),
                egui::vec2(rect.width().min(460.0) - MARGIN * 2.0, card_height),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                widgets::floating_frame(ui)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(card.width() - 28.0);
                        ui.horizontal(|ui| {
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
                        });
                        let line = match kept {
                            Some(kept) => format!(
                                "Last render, {}: {}",
                                store::ago(kept.saved, store::now()),
                                panels::recipe_words(&kept.recipe, kept.age, &lab.about)
                            ),
                            None => "Not rendered yet. Set the age, season and detail at the \
                                     right, then Generate."
                                .to_owned(),
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(line).size(theme::SMALL).color(theme::TEXT_2),
                            )
                            .wrap(),
                        );
                        ui.add_space(theme::GAP_SMALL);
                        let text = if growing { "Growing…" } else { "Generate" };
                        if widgets::primary_button(ui, icons::PLAY, text, Some("Enter"), !growing)
                            .clicked()
                        {
                            shell.send(Command::Generate);
                        }
                    });
            });
        });
}

/// Under the toolbar, while the plant on screen is the chosen species'
/// and its settings changed since: what changed, and Generate.
fn changes(ctx: &egui::Context, lab: &Lab, shell: &mut Shell, view: egui::Rect) {
    let changed = lab.changes();
    if changed.is_empty() || lab.growing.is_some() {
        return;
    }
    let words: Vec<&str> = changed.iter().map(|setting| setting.word()).collect();
    let action = if lab.redraws() {
        "Generate draws it again"
    } else {
        "Generate grows it again"
    };
    egui::Area::new(egui::Id::new("plantlab-changes"))
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(view.center().x, view.top() + MARGIN + 52.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            widgets::floating_frame(ui).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(theme::WARNING));
                    ui.label(
                        RichText::new(format!("Changed: {}", words.join(", "))).color(theme::TEXT),
                    );
                    ui.label(
                        RichText::new(action)
                            .size(theme::SMALL)
                            .color(theme::TEXT_3),
                    );
                    if ui
                        .button(panels::with_icon(ui, icons::PLAY, "Generate", theme::TEXT))
                        .on_hover_text("Enter")
                        .clicked()
                    {
                        shell.send(Command::Generate);
                    }
                });
            });
        });
}

/// At the foot of the view while a plant grows or is drawn: its progress,
/// Pause and Stop; and while the year plays, its month.
fn progress(ctx: &egui::Context, lab: &Lab, shell: &mut Shell, view: egui::Rect) {
    let Some(job) = &lab.growing else {
        return;
    };
    let paused = lab.control.pause.load(Ordering::Relaxed);
    egui::Area::new(egui::Id::new("plantlab-progress"))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(view.center().x, view.bottom() - MARGIN))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            widgets::floating_frame(ui)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(360.0_f32.min(view.width() - 4.0 * MARGIN));
                    let (share, text) = panels::progress(lab);
                    let label = if job.redraw {
                        "Drawing".to_owned()
                    } else if paused {
                        "Paused".to_owned()
                    } else {
                        "Growing".to_owned()
                    };
                    let label = match lab.year {
                        Some(month) => format!("{label} · {}", panels::MONTHS[month]),
                        None => label,
                    };
                    widgets::progress(ui, share, &label, &text);
                    ui.add_space(theme::GAP_SMALL);
                    ui.horizontal(|ui| {
                        if !job.redraw {
                            let (icon, tip) = if paused {
                                (icons::PLAY_PAUSE, "Resume (Space)")
                            } else {
                                (icons::PAUSE, "Pause (Space)")
                            };
                            if widgets::icon_button(ui, icon, tip, paused) {
                                shell.send(Command::Pause(!paused));
                            }
                        }
                        if widgets::icon_button(
                            ui,
                            icons::STOP,
                            "Stop; the last frame stays (Esc)",
                            false,
                        ) {
                            shell.send(Command::Stop);
                        }
                    });
                });
        });
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
                "Nothing matches: try a species' name, a family or a word such as grid".into();
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
        .default_width(380.0)
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
        for window in [1_000.0_f32, 1_200.0, 1_500.0, 2_400.0] {
            for library in [false, true] {
                for inspector in [None, Some(Panel::Render), Some(Panel::Readings)] {
                    let (left, right) = dock_widths(window, library, inspector);
                    let used = left.unwrap_or(0.0) + right.unwrap_or(0.0);
                    assert!(
                        window - used >= MIN_VIEW - 0.5,
                        "{window} {library} {inspector:?} {left:?} {right:?}"
                    );
                    // The inspector always shows when asked.
                    assert_eq!(right.is_some(), inspector.is_some());
                }
            }
        }
        // A wide window gives a board half of what the library leaves, and
        // keeps the library.
        let (left, right) = dock_widths(2_400.0, true, Some(Panel::Readings));
        assert_eq!(left, Some(LIBRARY));
        assert!((right.unwrap() - (2_400.0 - LIBRARY) * 0.5).abs() < 0.5);
        // A narrow one leaves the library out before the view gives way.
        let (left, right) = dock_widths(700.0, true, Some(Panel::Render));
        assert!(left.is_none() && right.is_some());
    }

    #[test]
    fn docks_follow_the_window_alone() {
        assert_eq!(
            dock_widths(1_234.0, true, Some(Panel::Readings)),
            dock_widths(1_234.0, true, Some(Panel::Readings))
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
