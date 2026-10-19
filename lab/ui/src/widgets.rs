//! Small widgets the lab interfaces share: menu entries, icon, rail and
//! dock buttons, a search field, segmented choices, switches, chips and
//! badges, cards, stats, key caps, popovers, knobs, reading rows, section
//! headings, form rows, key help and the frames of bars,
//! panels and floating pieces.
//!
//! Each control says what the person did and changes nothing itself: a
//! button returns whether it was clicked, a choice the option picked, a knob
//! the value it sends. The caller turns that into its own action (in
//! `WorldLab`, command text), so the value it shows is always read back from
//! the application, never kept here.
use std::f32::consts::PI;
use std::ops::RangeInclusive;
use std::sync::Arc;

use egui::{
    Align, Align2, Button, Color32, CornerRadius, FontId, FontSelection, Frame, Galley, Grid,
    InnerResponse, Key, Label, Layout, Margin, Pos2, Rect, Response, RichText, Sense, Stroke,
    TextFormat, TextStyle, Ui, WidgetInfo, WidgetText, WidgetType, pos2, text::LayoutJob, vec2,
};

use crate::{icons, theme};

/// A menu entry: its label, its shortcut on the right in a weaker colour
/// and, for a switch, its state as a box icon before the label
/// ([`icons::CHECK_SQUARE`] or [`icons::SQUARE`], then `Crosshair      F9`).
/// Returns whether it was clicked; a menu closes on any click.
pub fn menu_item(ui: &mut Ui, label: &str, shortcut: Option<&str>, checked: Option<bool>) -> bool {
    let text = match checked {
        Some(true) => format!("{} {label}", icons::CHECK_SQUARE),
        Some(false) => format!("{} {label}", icons::SQUARE),
        None => label.to_owned(),
    };
    let button = match shortcut {
        Some(shortcut) => Button::new(text).shortcut_text(shortcut),
        None => Button::new(text),
    };
    ui.add(button).clicked()
}

/// A button that shows a switch's state: highlighted while `on`. Returns
/// whether it was clicked.
pub fn switch_button(ui: &mut Ui, label: &str, on: bool) -> bool {
    ui.add(Button::selectable(on, label)).clicked()
}

/// A heading that starts a section of a panel: small, upper-case, spaced
/// out a little and muted.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(theme::GAP_SMALL + 2.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .font(theme::strong_font(ui, theme::SMALL))
            .extra_letter_spacing(SECTION_LETTER_SPACING)
            .color(theme::TEXT_2),
    );
}

/// The space a section heading adds between its letters, points.
pub const SECTION_LETTER_SPACING: f32 = 0.6;

/// A row of readings: its name in the muted colour, then each entry in
/// monospace, wrapping under the name when the panel is narrow
/// (`player   E +12.0 m  N -3.5 m`).
///
/// Name and entries are one paragraph, so a word longer than the row (a
/// hash, a GPU's name) breaks onto the lines below and the row never runs
/// off the panel. Two labels in a wrapping row would start the entries
/// after the name with egui's first-row indent, which egui 0.36 also
/// subtracts from a later word too long for a line: that word, and the
/// rows after it, then started left of the panel and were cut off.
pub fn reading_row(ui: &mut Ui, name: &str, entries: &str) {
    let mut job = LayoutJob::default();
    let style = ui.style().clone();
    RichText::new(name).color(theme::MUTED).append_to(
        &mut job,
        &style,
        FontSelection::Default,
        Align::Center,
    );
    RichText::new(entries).monospace().append_to(
        &mut job,
        &style,
        FontSelection::Default,
        Align::Center,
    );
    if let Some(section) = job.sections.get_mut(1) {
        section.leading_space = style.spacing.item_spacing.x;
    }
    ui.add(Label::new(job).wrap());
}

/// The share of a form row its name and help take; the control has the
/// rest. The name and help never get narrower than [`FIELD_TEXT_MIN`]
/// points.
pub const FIELD_TEXT_SHARE: f32 = 0.5;
pub const FIELD_TEXT_MIN: f32 = 160.0;
/// The gap between form rows, points.
pub const FIELD_GAP: f32 = 4.0;

/// A row of a form, on its own raised band so rows read apart: at the left
/// the name, with its help under it in small muted text, wrapped; at the
/// right what `control` draws, top-aligned with the name. Returns the
/// name's response, for a tooltip, and what `control` returns.
pub fn field<R>(
    ui: &mut Ui,
    name: &str,
    help: &str,
    control: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let inner = Frame::new()
        .fill(theme::SURFACE_2)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_width(width);
            let text_width = (width * FIELD_TEXT_SHARE).max(FIELD_TEXT_MIN).min(width);
            ui.horizontal_top(|ui| {
                let name = ui
                    .allocate_ui_with_layout(
                        vec2(text_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(text_width);
                            let name = ui.add(
                                Label::new(
                                    RichText::new(name).font(theme::strong_font(ui, theme::BODY)),
                                )
                                .wrap(),
                            );
                            if !help.is_empty() {
                                ui.add(
                                    Label::new(RichText::new(help).small().color(theme::MUTED))
                                        .wrap(),
                                );
                            }
                            name
                        },
                    )
                    .inner;
                let value = ui.vertical(control).inner;
                (name, value)
            })
            .inner
        })
        .inner;
    ui.add_space(FIELD_GAP);
    inner
}

/// A key listing: each group's name, then its keys as [`key_caps`] and
/// what they do, in two columns.
pub fn key_help(ui: &mut Ui, groups: &[(&str, Vec<(String, String)>)]) {
    for (group, keys) in groups {
        section(ui, group);
        Grid::new(("lab-ui-key-help", *group))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (key, help) in keys {
                    key_caps(ui, key);
                    ui.label(help);
                    ui.end_row();
                }
            });
    }
}

/// The frame of a bar: the bar colour and a thin margin.
pub fn bar_frame() -> Frame {
    Frame::new()
        .fill(theme::BAR)
        .inner_margin(Margin::symmetric(8, 4))
}

/// The frame of a docked panel's contents.
pub fn panel_frame() -> Frame {
    Frame::new()
        .fill(theme::PANEL)
        .inner_margin(Margin::same(8))
}

// Milestone UX0: the parts of the new look.

/// How large an icon draws in an icon button, points.
pub const ICON_SIZE: f32 = 16.0;

/// A button that shows only an icon (one of [`icons`]), square at the
/// control height, highlighted while `on`, with `tip` as its tooltip and as
/// what a screen reader says. Returns whether it was clicked.
pub fn icon_button(ui: &mut Ui, icon: &str, tip: &str, on: bool) -> bool {
    let side = theme::CONTROL_HEIGHT;
    let response = ui.add(
        Button::selectable(
            on,
            RichText::new(icon).font(theme::icon_font(ui, ICON_SIZE)),
        )
        .min_size(vec2(side, side))
        .frame_when_inactive(on),
    );
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), on, tip));
    response.on_hover_text(tip).clicked()
}

/// A rail entry's width and height, points.
pub const RAIL_WIDTH: f32 = 56.0;
pub const RAIL_ENTRY_HEIGHT: f32 = 48.0;

/// An entry of a rail, the column of tools at a window's edge: its icon
/// over a short label, the open one on a soft accent band with a bar at its
/// left edge, and `tip` as its tooltip (the tool's whole name and key).
/// Keep the label to a word: it is cut at the rail's width. Returns whether
/// it was clicked; read the click's modifiers from the input.
pub fn rail_button(ui: &mut Ui, icon: &str, label: &str, tip: &str, active: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(RAIL_WIDTH, RAIL_ENTRY_HEIGHT), Sense::click());
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), active, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let band = rect.shrink2(vec2(6.0, 2.0));
        let fill = if active {
            theme::ACCENT_SOFT
        } else if response.hovered() {
            theme::SURFACE_3
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(band, theme::RADIUS_CONTROL, fill);
        if active {
            let marker = Rect::from_center_size(
                pos2(rect.left() + 1.5, rect.center().y),
                vec2(3.0, RAIL_ENTRY_HEIGHT * 0.45),
            );
            painter.rect_filled(marker, CornerRadius::same(2), theme::ACCENT);
        }
        let ink = if active {
            theme::ACCENT
        } else if response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_2
        };
        painter.text(
            pos2(rect.center().x, rect.top() + 17.0),
            Align2::CENTER_CENTER,
            icon,
            theme::icon_font(ui, ICON_SIZE + 2.0),
            ink,
        );
        painter.text(
            pos2(rect.center().x, rect.bottom() - 11.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(RAIL_LABEL_SIZE),
            ink,
        );
    }
    response.on_hover_text(tip).clicked()
}

/// A rail label's text size, points.
const RAIL_LABEL_SIZE: f32 = 10.5;

/// A dock button's side and its icon's size, points.
pub const DOCK_BUTTON: f32 = 40.0;
pub const DOCK_ICON_SIZE: f32 = 20.0;

/// A button of a floating dock, the row of tools over a full-window view:
/// a large icon on a rounded square, the open tool's on a soft accent band
/// with a hairline of accent and a dot under the icon, and `tip` as its
/// tooltip. Returns whether it was clicked.
pub fn dock_button(ui: &mut Ui, icon: &str, tip: &str, active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(DOCK_BUTTON, DOCK_BUTTON), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), active, tip));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let (fill, stroke, ink) = if active {
            (
                theme::ACCENT_SOFT,
                Stroke::new(1.0, theme::ACCENT_LINE),
                theme::ACCENT,
            )
        } else if response.hovered() {
            (theme::SURFACE_4, Stroke::NONE, theme::TEXT)
        } else {
            (Color32::TRANSPARENT, Stroke::NONE, theme::TEXT_2)
        };
        painter.rect(
            rect,
            theme::RADIUS_CARD,
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            theme::icon_font(ui, DOCK_ICON_SIZE),
            ink,
        );
        if active {
            painter.circle_filled(pos2(rect.center().x, rect.bottom() - 4.0), 2.0, ink);
        }
    }
    response.on_hover_text(tip).clicked()
}

/// A button drawn as a search field: a magnifying glass, `hint` in the
/// faint ink and the keys that open the search at its right, `width`
/// points wide. It opens a search kept elsewhere (a command palette);
/// returns whether it was clicked.
pub fn search_field(ui: &mut Ui, hint: &str, keys: &str, width: f32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT + 4.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), hint));
    if ui.is_rect_visible(rect) {
        let stroke = if response.hovered() {
            theme::ACCENT_LINE
        } else {
            theme::LINE_STRONG
        };
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            theme::RADIUS_CONTROL,
            theme::SURFACE_0,
            Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        // Painted, not laid out as labels, so no part of it takes the click.
        let font = FontId::proportional(theme::BODY);
        let key_font = FontId::monospace(theme::SMALL);
        let mut right = rect.right() - theme::GAP;
        for key in key_parts(keys).iter().rev() {
            let galley = painter.layout_no_wrap((*key).to_owned(), key_font.clone(), theme::TEXT_2);
            let cap = Rect::from_min_max(
                pos2(right - galley.size().x - 10.0, rect.center().y - 9.0),
                pos2(right, rect.center().y + 9.0),
            );
            painter.rect(
                cap,
                theme::RADIUS_SMALL,
                theme::SURFACE_3,
                Stroke::new(1.0, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
            painter.galley(cap.center() - galley.size() / 2.0, galley, theme::TEXT_2);
            right = cap.left() - 3.0;
        }
        let glass = painter.layout_no_wrap(
            icons::MAGNIFYING_GLASS.to_owned(),
            theme::icon_font(ui, theme::BODY),
            theme::TEXT_3,
        );
        let left = rect.left() + theme::GAP;
        painter.galley(
            pos2(left, rect.center().y - glass.size().y / 2.0),
            glass.clone(),
            theme::TEXT_3,
        );
        let text_left = left + glass.size().x + theme::GAP_SMALL + 2.0;
        let room = (right - theme::GAP_SMALL - text_left).max(0.0);
        let mut job = LayoutJob::simple_singleline(hint.to_owned(), font, theme::TEXT_3);
        job.wrap.max_width = room;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let text = ui.fonts_mut(|fonts| fonts.layout_job(job));
        painter.galley(
            pos2(text_left, rect.center().y - text.size().y / 2.0),
            text,
            theme::TEXT_3,
        );
    }
    response.clicked()
}

/// A row of options in one track, the chosen one raised (`Day | Night`).
/// Returns the option clicked when it is not the chosen one, so the caller
/// acts only on a change.
pub fn segmented(ui: &mut Ui, options: &[&str], chosen: usize) -> Option<usize> {
    let mut picked = None;
    Frame::new()
        .fill(theme::SURFACE_0)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // A row laid out right to left adds the last option first,
                // so the options still read left to right.
                let order: Vec<_> = if ui.layout().main_dir() == egui::Direction::RightToLeft {
                    options.iter().enumerate().rev().collect()
                } else {
                    options.iter().enumerate().collect()
                };
                for (index, option) in order {
                    let on = index == chosen;
                    if segment(ui, option, on).clicked() && !on {
                        picked = Some(index);
                    }
                }
            });
        });
    picked
}

/// One option of [`segmented`].
fn segment(ui: &mut Ui, text: &str, on: bool) -> Response {
    let galley = WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    let size = vec2(
        galley.size().x + 2.0 * SEGMENT_PADDING,
        theme::CONTROL_HEIGHT - 4.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), on, text));
    if ui.is_rect_visible(rect) {
        let fill = if on {
            theme::SURFACE_4
        } else if response.hovered() {
            theme::SURFACE_2
        } else {
            Color32::TRANSPARENT
        };
        let ink = if on || response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_2
        };
        let painter = ui.painter();
        painter.rect_filled(rect, theme::RADIUS_CONTROL - 2, fill);
        let at = rect.center() - galley.size() / 2.0;
        painter.galley(at, galley, ink);
    }
    response
}

/// The room either side of a segment's text, points.
const SEGMENT_PADDING: f32 = 10.0;

/// A switch's track, points, and the rounding that makes its ends round.
pub const SWITCH_TRACK: egui::Vec2 = egui::Vec2::new(30.0, 17.0);
const SWITCH_ROUND: u8 = 9;

/// A switch with its label after it: a pill track that is accent while
/// `on`, its knob sliding to the right. Returns whether it was clicked.
pub fn switch(ui: &mut Ui, label: &str, on: bool) -> bool {
    let galley = WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    let gap = if label.is_empty() { 0.0 } else { theme::GAP };
    let size = vec2(
        SWITCH_TRACK.x + gap + galley.size().x,
        theme::CONTROL_HEIGHT.max(galley.size().y),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), on, label));
    if ui.is_rect_visible(rect) {
        let shift = ui.ctx().animate_bool(response.id, on);
        let track = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - SWITCH_TRACK.y / 2.0),
            SWITCH_TRACK,
        );
        let painter = ui.painter();
        let (fill, stroke) = if on {
            (theme::ACCENT, Stroke::NONE)
        } else {
            (theme::SURFACE_4, Stroke::new(1.0, theme::LINE_STRONG))
        };
        painter.rect(
            track,
            CornerRadius::same(SWITCH_ROUND),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        let radius = SWITCH_TRACK.y / 2.0 - 2.5;
        let travel = SWITCH_TRACK.x - SWITCH_TRACK.y;
        let centre = pos2(
            track.left() + SWITCH_TRACK.y / 2.0 + travel * shift,
            track.center().y,
        );
        let knob = if on { theme::ON_ACCENT } else { theme::TEXT_2 };
        painter.circle_filled(centre, radius, knob);
        let ink = if response.hovered() {
            theme::TEXT
        } else {
            ui.visuals().text_color()
        };
        let at = pos2(track.right() + gap, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, ink);
    }
    response.clicked()
}

/// A state a mark shows, with its colour and its icon, so it never reads by
/// colour alone. The colours are [`theme::STATUS`]'s and [`theme::INFO`],
/// never a chart's series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    /// Something to know.
    Info,
    /// Fine.
    Good,
    /// Worth a look.
    Warning,
    /// Hurting.
    Serious,
    /// Broken.
    Critical,
}

impl Level {
    /// Every level, mildest first.
    pub const ALL: [Self; 5] = [
        Self::Info,
        Self::Good,
        Self::Warning,
        Self::Serious,
        Self::Critical,
    ];

    /// The level's colour, for its icon and marks; text stays in a text ink.
    #[must_use]
    pub fn colour(self) -> Color32 {
        match self {
            Self::Info => theme::INFO,
            Self::Good => theme::STATUS[0],
            Self::Warning => theme::STATUS[1],
            Self::Serious => theme::STATUS[2],
            Self::Critical => theme::STATUS[3],
        }
    }

    /// The level's icon.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => icons::INFO,
            Self::Good => icons::CHECK_CIRCLE,
            Self::Warning => icons::WARNING,
            Self::Serious => icons::WARNING_OCTAGON,
            Self::Critical => icons::X_CIRCLE,
        }
    }
}

/// A pill: an optional icon in its own colour, then the text, on `fill`
/// with `stroke` round it.
fn pill(
    ui: &mut Ui,
    icon: Option<(&str, Color32)>,
    text: &str,
    colours: (Color32, Stroke, Color32),
    sense: Sense,
) -> Response {
    let (fill, stroke, ink) = colours;
    let font = FontId::proportional(theme::SMALL);
    let painter = ui.painter().clone();
    let text_galley = painter.layout_no_wrap(text.to_owned(), font.clone(), ink);
    let icon_galley = icon.map(|(icon, colour)| {
        painter.layout_no_wrap(icon.to_owned(), theme::icon_font(ui, theme::SMALL), colour)
    });
    let icon_width = icon_galley
        .as_ref()
        .map_or(0.0, |galley| galley.size().x + theme::GAP_SMALL);
    let size = vec2(
        icon_width + text_galley.size().x + 2.0 * PILL_PADDING,
        PILL_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        painter.rect(
            rect,
            CornerRadius::same(PILL_ROUND),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left() + PILL_PADDING;
        if let Some(galley) = icon_galley {
            let width = galley.size().x;
            let y = rect.center().y - galley.size().y / 2.0;
            painter.galley(pos2(x, y), galley, ink);
            x += width + theme::GAP_SMALL;
        }
        let y = rect.center().y - text_galley.size().y / 2.0;
        painter.galley(pos2(x, y), text_galley, ink);
    }
    response
}

/// A pill's height and the room either side of its contents, points.
pub const PILL_HEIGHT: f32 = 20.0;
const PILL_PADDING: f32 = 8.0;
/// Half the pill's height, so its ends are round.
const PILL_ROUND: u8 = 10;

/// A filter chip: a pill that is on (accent, outlined) or off. Returns
/// whether it was clicked.
pub fn chip(ui: &mut Ui, text: &str, on: bool) -> bool {
    let colours = if on {
        (
            theme::ACCENT_SOFT,
            Stroke::new(1.0, theme::ACCENT_LINE),
            theme::TEXT,
        )
    } else {
        (
            theme::SURFACE_3,
            Stroke::new(1.0, theme::LINE),
            theme::TEXT_2,
        )
    };
    let response = pill(ui, None, text, colours, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), on, text));
    response.clicked()
}

/// A badge: a state's icon in its colour, then `text` in the text ink, on a
/// faint wash of the colour (`✓ ready`). It takes no clicks.
pub fn badge(ui: &mut Ui, level: Level, text: &str) -> Response {
    let colour = level.colour();
    let response = pill(
        ui,
        Some((level.icon(), colour)),
        text,
        (colour.gamma_multiply(BADGE_WASH), Stroke::NONE, theme::TEXT),
        Sense::hover(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
    response
}

/// How strongly a badge's colour washes its pill.
pub const BADGE_WASH: f32 = 0.16;

/// The frame of a card: a raised surface with a hairline and rounded
/// corners, holding one figure or one group of controls.
pub fn card_frame() -> Frame {
    Frame::new()
        .fill(theme::SURFACE_2)
        .stroke(Stroke::new(1.0, theme::LINE))
        .corner_radius(theme::RADIUS_CARD)
        .inner_margin(Margin::same(theme::CARD_PADDING))
}

/// How opaque a floating piece's tint is: the view shows through a little,
/// never blurred.
pub const FLOATING_TINT: f32 = 0.94;

/// The frame of a piece that floats over a full-window view (a dock, a
/// clock, a card opened from the dock): the window surface, tinted so the
/// view shows through a little, with a strong hairline, the window's
/// rounding and its shadow.
pub fn floating_frame(ui: &Ui) -> Frame {
    Frame::new()
        .fill(theme::SURFACE_1.gamma_multiply(FLOATING_TINT))
        .stroke(Stroke::new(1.0, theme::LINE_STRONG))
        .corner_radius(theme::RADIUS_WINDOW)
        .shadow(ui.style().visuals.popup_shadow)
        .inner_margin(Margin::same(6))
}

/// A card: its title, when it has one, then what `add` draws.
pub fn card<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame().show(ui, |ui| {
        if !title.is_empty() {
            ui.label(
                RichText::new(title)
                    .font(theme::strong_font(ui, theme::BODY))
                    .color(theme::TEXT),
            );
            ui.add_space(theme::GAP_SMALL);
        }
        add(ui)
    })
}

/// A stat's number size, points.
pub const STAT_SIZE: f32 = 22.0;

/// A stat: its name, small and muted, over its value, large in monospace,
/// with the unit small beside it (`frame p95` over `16.4 ms`).
pub fn stat(ui: &mut Ui, name: &str, value: &str, unit: &str) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(name).size(theme::SMALL).color(theme::TEXT_2));
        let mut job = LayoutJob::default();
        job.append(
            value,
            0.0,
            TextFormat::simple(FontId::monospace(STAT_SIZE), theme::TEXT),
        );
        if !unit.is_empty() {
            job.append(
                unit,
                3.0,
                TextFormat::simple(FontId::proportional(theme::SMALL), theme::TEXT_2),
            );
        }
        ui.add(Label::new(job).extend());
    })
    .response
}

/// A progress bar's track, points high.
pub const PROGRESS_TRACK: f32 = 6.0;

/// A progress bar: `label` at the left and `value` at the right, both
/// small and muted, over a thin rounded track the accent fills by `share`
/// (0 to 1) across the width there is (`World build`, `about 1 min left`).
pub fn progress(ui: &mut Ui, share: f32, label: &str, value: &str) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(theme::SMALL).color(theme::TEXT_2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(value)
                        .monospace()
                        .size(theme::SMALL)
                        .color(theme::TEXT_2),
                );
            });
        });
        let (track, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), PROGRESS_TRACK), Sense::hover());
        let round = CornerRadius::same(PROGRESS_ROUND);
        ui.painter().rect_filled(track, round, theme::SURFACE_3);
        let filled = progress_filled(share, track.width());
        if filled > 0.0 {
            let bar = Rect::from_min_size(track.min, vec2(filled, track.height()));
            ui.painter().rect_filled(bar, round, theme::ACCENT);
        }
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::ProgressIndicator, ui.is_enabled(), label)
        });
    })
    .response
}

/// Half the track's height, so its ends are round.
const PROGRESS_ROUND: u8 = 3;

/// How much of a `width` track `share` fills: nothing for no share (or
/// none known), at least a round dot once started, all of it at 1.
fn progress_filled(share: f32, width: f32) -> f32 {
    if share.is_nan() || share <= 0.0 {
        return 0.0;
    }
    (share.min(1.0) * width).max(PROGRESS_TRACK).min(width)
}

/// A primary button's height, points.
pub const PRIMARY_HEIGHT: f32 = 36.0;

/// The one button that does a page's main work (`Generate`): as wide as
/// the room, filled with the accent, `icon` (one of [`icons`]) before its
/// text in the on-accent ink, and `key` faint at its right when given.
/// Greyed and inert unless `enabled`. Returns whether it was clicked.
pub fn primary_button(
    ui: &mut Ui,
    icon: &str,
    text: &str,
    key: Option<&str>,
    enabled: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), PRIMARY_HEIGHT), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let (fill, ink) = if !enabled {
            (theme::SURFACE_3, theme::TEXT_3)
        } else if response.is_pointer_button_down_on() {
            (theme::ACCENT.gamma_multiply(0.85), theme::ON_ACCENT)
        } else if response.hovered() {
            (theme::ACCENT.gamma_multiply(1.08), theme::ON_ACCENT)
        } else {
            (theme::ACCENT, theme::ON_ACCENT)
        };
        painter.rect_filled(rect, theme::RADIUS_CONTROL, fill);
        let label = text_galley(ui, text, theme::strong_font(ui, theme::BODY + 1.0), ink);
        let icon_font = theme::icon_font(ui, ICON_SIZE + 2.0);
        let icon_width = ICON_SIZE + 2.0;
        let gap = 8.0;
        let total = icon_width + gap + label.size().x;
        let left = rect.center().x - total / 2.0;
        painter.text(
            pos2(left + icon_width / 2.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon,
            icon_font,
            ink,
        );
        let at = pos2(
            left + icon_width + gap,
            rect.center().y - label.size().y / 2.0,
        );
        painter.galley(at, label, ink);
        if let Some(key) = key {
            painter.text(
                pos2(rect.right() - 10.0, rect.center().y),
                Align2::RIGHT_CENTER,
                key,
                FontId::monospace(theme::SMALL),
                ink.gamma_multiply(0.7),
            );
        }
    }
    response
}

/// `text` laid out on one line in `font` and `ink`.
fn text_galley(ui: &Ui, text: &str, font: FontId, ink: Color32) -> Arc<Galley> {
    ui.painter().layout_no_wrap(text.to_owned(), font, ink)
}

/// A tab strip's height, points.
pub const TAB_HEIGHT: f32 = 34.0;

/// A row of tabs across the room, each an icon (one of [`icons`]) with
/// its name as the tooltip, the chosen one in the text ink over an accent
/// underline, the rest faint. Returns the tab clicked, when it is not the
/// chosen one.
pub fn tabs(ui: &mut Ui, tabs: &[(&str, &str)], chosen: usize) -> Option<usize> {
    let mut picked = None;
    #[allow(clippy::cast_precision_loss)]
    let width = ui.available_width() / tabs.len().max(1) as f32;
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    ui.painter().hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        Stroke::new(1.0, theme::LINE_STRONG),
    );
    for (index, (icon, tip)) in tabs.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let rect = Rect::from_min_size(
            pos2(strip.left() + width * index as f32, strip.top()),
            vec2(width, TAB_HEIGHT),
        );
        let response = ui.interact(rect, ui.id().with(("lab-ui-tab", index)), Sense::click());
        let on = index == chosen;
        response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, *tip));
        let ink = if on {
            theme::TEXT
        } else if response.hovered() {
            theme::TEXT_2
        } else {
            theme::TEXT_3
        };
        if response.hovered() && !on {
            ui.painter()
                .rect_filled(rect.shrink(3.0), theme::RADIUS_SMALL, theme::SURFACE_2);
        }
        ui.painter().text(
            rect.center() - vec2(0.0, 1.0),
            Align2::CENTER_CENTER,
            *icon,
            theme::icon_font(ui, ICON_SIZE + 1.0),
            ink,
        );
        if on {
            let bar = Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.bottom() - 2.0),
                pos2(rect.right() - 8.0, rect.bottom()),
            );
            ui.painter()
                .rect_filled(bar, CornerRadius::same(1), theme::ACCENT);
        }
        if response.on_hover_text(*tip).clicked() && !on {
            picked = Some(index);
        }
    }
    picked
}

/// A key cap: one key's name in a small raised box (`Ctrl`).
pub fn key_cap(ui: &mut Ui, key: &str) -> Response {
    Frame::new()
        .fill(theme::SURFACE_3)
        .stroke(Stroke::new(1.0, theme::LINE_STRONG))
        .corner_radius(theme::RADIUS_SMALL)
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(key)
                    .monospace()
                    .size(theme::SMALL)
                    .color(theme::TEXT),
            )
        })
        .response
}

/// Keys pressed together, each in its [`key_cap`] (`Ctrl` `Shift` `C` for
/// `Ctrl+Shift+C`; `Ctrl++` is `Ctrl` and `+`).
pub fn key_caps(ui: &mut Ui, keys: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for key in key_parts(keys) {
            key_cap(ui, key);
        }
    })
    .response
}

/// The keys of a `+` joined chord, a `+` key kept.
fn key_parts(keys: &str) -> Vec<&str> {
    if keys == "+" {
        return vec!["+"];
    }
    if let Some(head) = keys.strip_suffix("++") {
        let mut parts = key_parts(head);
        parts.push("+");
        return parts;
    }
    keys.split('+').filter(|part| !part.is_empty()).collect()
}

/// A popover: a card under `anchor` that a click on the anchor opens and
/// closes, and Escape or a click outside closes. It may hold controls;
/// clicking them keeps it open. Returns what `add` returns while it is open.
pub fn popover<R>(anchor: &Response, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let shadow = anchor.ctx.global_style().visuals.popup_shadow;
    egui::Popup::from_toggle_button_response(anchor)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(
            card_frame()
                .stroke(Stroke::new(1.0, theme::LINE_STRONG))
                .shadow(shadow),
        )
        .gap(theme::GAP_SMALL)
        .show(add)
        .map(|inner| inner.inner)
}

/// A knob's dial, points across.
pub const KNOB_SIZE: f32 = 44.0;
/// The drag, points up or right, that turns a knob through its whole range.
pub const KNOB_TRAVEL: f32 = 160.0;
/// How long a knob keeps showing the value it sent while the application's
/// reading has not caught up, seconds (as `WorldLab`'s weather sliders do).
pub const KNOB_HOLD_S: f64 = 1.0;
/// How many presses of an arrow key turn a focused knob through its range.
pub const KNOB_KEY_STEPS: f32 = 20.0;
/// Where the dial's sweep starts, radians clockwise from the right on the
/// screen (lower left), and how far it sweeps (to lower right).
const KNOB_START: f32 = 0.75 * PI;
const KNOB_SWEEP: f32 = 1.5 * PI;

/// What a knob remembers between passes.
#[derive(Debug, Clone, Copy, Default)]
struct KnobMemory {
    /// The value while it is dragged.
    edit: Option<f32>,
    /// The value sent and when, shown until the reading has it.
    sent: Option<(f32, f64)>,
}

/// A knob: a dial that turns through `range`, an accent arc to its value,
/// the value as `text` gives it under the dial and `label` under that.
///
/// `value` is the application's reading. Dragging up or right turns it;
/// the knob shows the turned value while dragged and sends it once, when
/// released: the return is that value on that pass and `None` otherwise.
/// While focused, each arrow key press sends a step of
/// 1/[`KNOB_KEY_STEPS`] of the range. A sent value stays shown until the
/// reading has it or [`KNOB_HOLD_S`] passes, so the knob never jumps back
/// while the application catches up. Knobs are told apart by `label`.
pub fn knob(
    ui: &mut Ui,
    label: &str,
    value: f32,
    range: RangeInclusive<f32>,
    text: impl Fn(f32) -> String,
) -> Option<f32> {
    knob_sized(ui, label, value, range, text, KNOB_SIZE)
}

/// A [`knob`] whose dial is `size` points across (at least
/// [`KNOB_SIZE`]), for the one control a group leads with, such as a
/// master volume.
pub fn knob_sized(
    ui: &mut Ui,
    label: &str,
    value: f32,
    range: RangeInclusive<f32>,
    text: impl Fn(f32) -> String,
    size: f32,
) -> Option<f32> {
    let dial = size.max(KNOB_SIZE);
    let (low, high) = (*range.start(), *range.end());
    let span = (high - low).max(f32::EPSILON);
    let id = ui.make_persistent_id(("lab-ui-knob", label));
    let now = ui.input(|input| input.time);
    let mut memory: KnobMemory = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    if let Some((sent, at)) = memory.sent
        && ((value - sent).abs() <= span * 1e-4 || now - at >= KNOB_HOLD_S)
    {
        memory.sent = None;
    }
    let shown = |memory: &KnobMemory| {
        memory
            .edit
            .or(memory.sent.map(|(sent, _)| sent))
            .unwrap_or(value)
            .clamp(low, high)
    };
    let font = FontId::monospace(theme::SMALL);
    let label_font = FontId::proportional(theme::SMALL);
    let label_galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), label_font, theme::TEXT_2);
    let line = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let extent = vec2(
        dial.max(label_galley.size().x),
        dial + 2.0 + line + label_galley.size().y,
    );
    let (rect, response) = ui.allocate_exact_size(extent, Sense::click_and_drag());

    let mut sent = None;
    if response.dragged() {
        let delta = response.drag_delta();
        let turned = shown(&memory) + (delta.x - delta.y) / KNOB_TRAVEL * span;
        memory.edit = Some(turned.clamp(low, high));
    }
    if response.drag_stopped()
        && let Some(edit) = memory.edit.take()
    {
        sent = Some(edit);
    }
    if response.has_focus() {
        let steps = ui.input(|input| {
            let presses =
                |key| f32::from(u16::try_from(input.num_presses(key)).unwrap_or(u16::MAX));
            presses(Key::ArrowUp) + presses(Key::ArrowRight)
                - presses(Key::ArrowDown)
                - presses(Key::ArrowLeft)
        });
        if steps != 0.0 {
            sent = Some((shown(&memory) + steps * span / KNOB_KEY_STEPS).clamp(low, high));
        }
    }
    if let Some(edit) = sent {
        memory.sent = Some((edit, now));
    }
    let showing = shown(&memory);
    if memory.edit.is_some() || memory.sent.is_some() {
        ui.data_mut(|data| data.insert_temp(id, memory));
        if memory.sent.is_some() {
            ui.ctx().request_repaint();
        }
    } else {
        ui.data_mut(|data| data.remove::<KnobMemory>(id));
    }
    response.widget_info(|| WidgetInfo::slider(ui.is_enabled(), f64::from(showing), label));

    if ui.is_rect_visible(rect) {
        paint_knob(
            ui,
            &response,
            dial,
            (showing - low) / span,
            &text(showing),
            label_galley,
        );
    }
    sent
}

/// Draws a knob `dial` points across turned to `share` of its sweep,
/// `value` and its label.
fn paint_knob(
    ui: &Ui,
    response: &Response,
    dial: f32,
    share: f32,
    value: &str,
    label: Arc<Galley>,
) {
    let rect = response.rect;
    let painter = ui.painter();
    let centre = pos2(rect.center().x, rect.top() + dial / 2.0);
    let radius = dial / 2.0 - 3.0;
    painter.add(egui::Shape::line(
        arc(centre, radius, 0.0, 1.0),
        Stroke::new(3.0, theme::SURFACE_4),
    ));
    if share > 0.0 {
        painter.add(egui::Shape::line(
            arc(centre, radius, 0.0, share),
            Stroke::new(3.0, theme::ACCENT),
        ));
    }
    let fill = if response.hovered() || response.dragged() {
        theme::SURFACE_4
    } else {
        theme::SURFACE_3
    };
    let inner = radius - 6.0;
    painter.circle(centre, inner, fill, Stroke::new(1.0, theme::LINE_STRONG));
    let angle = KNOB_START + KNOB_SWEEP * share;
    let tip = centre + inner * 0.75 * vec2(angle.cos(), angle.sin());
    let foot = centre + inner * 0.2 * vec2(angle.cos(), angle.sin());
    painter.line_segment([foot, tip], Stroke::new(2.0, theme::TEXT));
    if response.has_focus() {
        painter.circle_stroke(centre, radius + 2.5, Stroke::new(1.0, theme::ACCENT_LINE));
    }
    painter.text(
        pos2(rect.center().x, rect.top() + dial + 2.0),
        Align2::CENTER_TOP,
        value,
        FontId::monospace(theme::SMALL),
        theme::TEXT,
    );
    let at = pos2(
        rect.center().x - label.size().x / 2.0,
        rect.bottom() - label.size().y,
    );
    painter.galley(at, label, theme::TEXT_2);
}

/// The segments of a dial's arc.
const ARC_STEPS: u16 = 48;

/// Points along a dial's arc from share `from` to share `to` of its sweep.
fn arc(centre: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    (0..=ARC_STEPS)
        .map(|step| {
            let share = from + (to - from) * f32::from(step) / f32::from(ARC_STEPS);
            let angle = KNOB_START + KNOB_SWEEP * share;
            centre + radius * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

// PlantLab: a slider that sends one value when it is let go.

/// What a [`slider`] remembers between passes.
#[derive(Debug, Clone, Copy, Default)]
struct SliderMemory {
    /// The value while it is dragged.
    edit: Option<f64>,
    /// The value sent and when, shown until the reading has it.
    sent: Option<(f64, f64)>,
}

/// How a [`slider`] steps and shows its value.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliderLook<'a> {
    /// Values snap to multiples of this; 0 for none.
    pub step: f64,
    /// After the value, such as `" years"`.
    pub suffix: &'a str,
    /// Spread the range on a log scale.
    pub logarithmic: bool,
    /// Decimals shown; `None` for egui's choice.
    pub decimals: Option<usize>,
}

/// A slider over `range` that sends one value when it is let go, as a
/// [`knob`] does, never a stream while it is dragged; a click on the track,
/// an arrow key or a value typed into its field sends at once.
///
/// `value` is the application's reading; `salt` tells sliders apart. The
/// value sent stays shown until the reading is within $`10^{-4}`$ of the
/// range of it or [`KNOB_HOLD_S`] passes, so the slider never jumps back
/// while the application catches up. Returns the value sent, on the pass
/// it is sent.
pub fn slider(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: f64,
    range: RangeInclusive<f64>,
    look: SliderLook,
) -> Option<f64> {
    let (low, high) = (*range.start(), *range.end());
    let span = (high - low).abs().max(f64::EPSILON);
    let id = ui.make_persistent_id(("plantlab-slider", salt));
    let now = ui.input(|input| input.time);
    let mut memory: SliderMemory = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    if let Some((sent, at)) = memory.sent
        && ((value - sent).abs() <= span * 1e-4 || now - at >= KNOB_HOLD_S)
    {
        memory.sent = None;
    }
    let mut shown = memory
        .edit
        .or(memory.sent.map(|(sent, _)| sent))
        .unwrap_or(value);
    let before = shown;
    let mut widget = egui::Slider::new(&mut shown, range)
        .step_by(look.step)
        .suffix(look.suffix)
        .logarithmic(look.logarithmic);
    if let Some(decimals) = look.decimals {
        widget = widget.fixed_decimals(decimals);
    }
    let response = ui.add(widget);
    let mut sent = None;
    if response.dragged() {
        memory.edit = Some(shown);
    } else if let Some(edit) = memory.edit.take() {
        // Let go: the value dragged to, if it moved.
        if (edit - value).abs() > f64::EPSILON {
            sent = Some(edit);
        }
    } else if response.changed() && (shown - before).abs() > f64::EPSILON {
        sent = Some(shown);
    }
    if let Some(edit) = sent {
        memory.sent = Some((edit, now));
    }
    if memory.edit.is_some() || memory.sent.is_some() {
        ui.data_mut(|data| data.insert_temp(id, memory));
        if memory.sent.is_some() {
            ui.ctx().request_repaint();
        }
    } else {
        ui.data_mut(|data| data.remove::<SliderMemory>(id));
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_rows_put_the_help_under_the_name_and_the_control_beside_it() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                vec2(600.0, 400.0),
            )),
            ..Default::default()
        };
        let mut rects = Vec::new();
        crate::test_pass(&ctx, input(), |ui| {
            for help in [
                "",
                "A long help text that wraps over more than one line in half a row.",
            ] {
                let top = ui.cursor().top();
                let (name, control) =
                    field(ui, "Capture folder", help, |ui| ui.button("Browse…").rect);
                rects.push((top, name.rect, control, ui.cursor().top()));
            }
        });
        let [(top, name, control, bottom), (next_top, _, _, next_bottom)] = rects[..] else {
            panic!("two rows");
        };
        // The control sits to the right of the name, from the same top.
        assert!(control.left() > name.right(), "{control:?} {name:?}");
        assert!(
            (control.top() - name.top()).abs() < 1.0,
            "{control:?} {name:?}"
        );
        // Rows follow each other with a gap; a row with help is taller.
        assert!(next_top >= bottom - 0.5 && name.top() > top);
        assert!(next_bottom - next_top > bottom - top, "{rects:?}");
    }

    /// Runs `add` in one pass of a fresh context and returns the context.
    fn pass(add: impl FnMut(&mut Ui)) -> egui::Context {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut add = add;
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| add(ui));
        ctx
    }

    /// Draws every control once, clicking none.
    fn draw_every_control(ui: &mut Ui) {
        assert!(!menu_item(ui, "Crosshair", Some("F9"), Some(true)));
        assert!(!menu_item(ui, "Quit", None, None));
        assert!(!switch_button(ui, "Grid", false));
        section(ui, "Movement");
        reading_row(ui, "player", "E +12.0 m  N -3.5 m");
        let (name, picked) = field(ui, "Walk speed", "How fast you walk.", |ui| {
            ui.label("1.4 m/s");
            7
        });
        assert_eq!(picked, 7);
        assert!(!name.clicked());
        assert!(!icon_button(ui, icons::GEAR_SIX, "Settings", false));
        assert!(!rail_button(
            ui,
            icons::GAUGE,
            "Diag",
            "Diagnostics (F3)",
            true
        ));
        assert!(!dock_button(
            ui,
            icons::USERS_THREE,
            "People and zombies",
            true
        ));
        assert!(!search_field(ui, "Search or run…", "Ctrl+P", 200.0));
        floating_frame(ui).show(ui, |ui| ui.label("12:00"));
        assert_eq!(segmented(ui, &["Day", "Night"], 0), None);
        assert!(!switch(ui, "Grid", true));
        assert!(!chip(ui, "render", true));
        for level in Level::ALL {
            badge(ui, level, "ready");
        }
        card(ui, "Frame time", |ui| stat(ui, "frame p95", "16.4", "ms"));
        progress(ui, 0.4, "World build", "about 1 min left");
        key_caps(ui, "Ctrl+Shift+C");
        assert_eq!(
            knob(ui, "Volume", 0.8, 0.0..=1.0, |v| format!("{v:.2}")),
            None
        );
        key_help(
            ui,
            &[("View", vec![("F9".to_owned(), "crosshair".to_owned())])],
        );
    }

    #[test]
    fn widgets_draw_without_clicks() {
        pass(draw_every_control);
    }

    /// The pass that themes a context, and a context never themed, draw
    /// every control and a heading without the lab's fonts: egui panics on
    /// a family no font is bound to, and the fonts go live a pass later.
    #[test]
    fn widgets_draw_before_the_fonts_are_live() {
        let never = egui::Context::default();
        crate::test_pass(&never, egui::RawInput::default(), draw_every_control);
        let themed = egui::Context::default();
        let mut families = Vec::new();
        for _ in 0..2 {
            crate::test_pass(&themed, egui::RawInput::default(), |ui| {
                theme::apply(ui.ctx());
                draw_every_control(ui);
                ui.heading("Settings");
                families.push((
                    theme::strong_font(ui, theme::BODY).family,
                    theme::icon_font(ui, ICON_SIZE).family,
                ));
            });
        }
        // egui may run a call's pass twice (a sizing pass), so read the
        // first pass and the last.
        let proportional = egui::FontFamily::Proportional;
        assert_eq!(
            families.first(),
            Some(&(proportional.clone(), proportional))
        );
        assert_eq!(families.last(), Some(&(theme::strong(), theme::icons())));
    }

    /// A pass's input on a 600 by 400 point screen at `time` seconds.
    fn screen(events: Vec<egui::Event>, time: f64) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
            events,
            time: Some(time),
            ..egui::RawInput::default()
        }
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// Runs a pass of `add` for each list of events, a twentieth of a second
    /// apart from `start`, and returns what each pass returned.
    fn run<T>(
        ctx: &egui::Context,
        start: f64,
        steps: Vec<Vec<egui::Event>>,
        mut add: impl FnMut(&mut Ui) -> T,
    ) -> Vec<T> {
        let mut results = Vec::new();
        for (index, events) in (0u16..).zip(steps) {
            let mut result = None;
            ctx.run_ui(screen(events, start + f64::from(index) * 0.05), |ui| {
                result = Some(add(ui));
            })
            .drop_without_applying_deltas();
            results.push(result.expect("a pass"));
        }
        results
    }

    /// Moves to `at`, presses, lets go and waits a pass.
    fn click(at: Pos2) -> Vec<Vec<egui::Event>> {
        vec![
            vec![egui::Event::PointerMoved(at)],
            vec![button(at, true)],
            vec![button(at, false)],
            Vec::new(),
        ]
    }

    /// Each control, clicked `offset` from where it starts, says so on the
    /// pass the button is let go and on no other.
    #[test]
    fn each_control_reports_its_click_once() {
        type Control = fn(&mut Ui) -> bool;
        let controls: [(&str, Control, egui::Vec2); 8] = [
            (
                "icon button",
                |ui| icon_button(ui, icons::GEAR_SIX, "Settings", false),
                vec2(12.0, 12.0),
            ),
            (
                "rail button",
                |ui| rail_button(ui, icons::GAUGE, "Diag", "Diagnostics", false),
                vec2(28.0, 24.0),
            ),
            (
                "dock button",
                |ui| dock_button(ui, icons::USERS_THREE, "People and zombies", false),
                vec2(20.0, 20.0),
            ),
            (
                "search field",
                |ui| search_field(ui, "Search or run…", "Ctrl+P", 200.0),
                vec2(40.0, 12.0),
            ),
            ("switch", |ui| switch(ui, "Grid", false), vec2(10.0, 12.0)),
            ("chip", |ui| chip(ui, "render", false), vec2(8.0, 10.0)),
            (
                "segment, another",
                |ui| segmented(ui, &["Day", "Night"], 1) == Some(0),
                vec2(8.0, 8.0),
            ),
            (
                "segment, the chosen",
                |ui| segmented(ui, &["Day", "Night"], 0).is_some(),
                vec2(8.0, 8.0),
            ),
        ];
        for (name, control, offset) in controls {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let mut origin = Pos2::ZERO;
            run(&ctx, 0.0, vec![Vec::new()], |ui| {
                origin = ui.cursor().min;
                control(ui)
            });
            let clicks = run(&ctx, 1.0, click(origin + offset), control);
            let expected = if name == "segment, the chosen" {
                [false; 4]
            } else {
                [false, false, true, false]
            };
            assert_eq!(clicks, expected, "{name}");
        }
    }

    #[test]
    fn levels_have_their_own_colours_and_icons() {
        for (index, level) in Level::ALL.iter().enumerate() {
            for other in &Level::ALL[index + 1..] {
                assert_ne!(level.colour(), other.colour(), "{level:?} {other:?}");
                assert_ne!(level.icon(), other.icon(), "{level:?} {other:?}");
            }
            assert!(!theme::SERIES.contains(&level.colour()), "{level:?}");
        }
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exact by construction: halves and whole points.
    fn a_progress_bar_fills_its_share_and_shows_a_start() {
        assert_eq!(progress_filled(0.0, 300.0), 0.0);
        assert_eq!(progress_filled(f32::NAN, 300.0), 0.0);
        assert_eq!(progress_filled(-0.5, 300.0), 0.0);
        assert_eq!(progress_filled(0.5, 300.0), 150.0);
        assert_eq!(progress_filled(2.0, 300.0), 300.0);
        // A step just started is a round dot, never a sliver.
        assert_eq!(progress_filled(0.001, 300.0), PROGRESS_TRACK);
        assert_eq!(progress_filled(0.001, 4.0), 4.0);
        assert_eq!(f32::from(PROGRESS_ROUND) * 2.0, PROGRESS_TRACK);
    }

    #[test]
    fn a_chord_splits_into_its_keys() {
        assert_eq!(key_parts("Ctrl+Shift+C"), ["Ctrl", "Shift", "C"]);
        assert_eq!(key_parts("F9"), ["F9"]);
        assert_eq!(key_parts("+"), ["+"]);
        assert_eq!(key_parts("Ctrl++"), ["Ctrl", "+"]);
        assert_eq!(key_parts("Alt+Shift++"), ["Alt", "Shift", "+"]);
    }

    /// A press at `centre`, a drag `rise` up in eight moves, the release and
    /// a pass after it.
    fn knob_drag(centre: Pos2, rise: f32) -> Vec<Vec<egui::Event>> {
        let mut steps = vec![
            vec![egui::Event::PointerMoved(centre)],
            vec![button(centre, true)],
        ];
        for step in 1..=8u8 {
            let at = centre - vec2(0.0, rise * f32::from(step) / 8.0);
            steps.push(vec![egui::Event::PointerMoved(at)]);
        }
        steps.push(vec![button(centre - vec2(0.0, rise), false)]);
        steps.push(Vec::new());
        steps
    }

    /// A knob dragged half its travel up sends half its range once, when
    /// let go; it shows that value until the reading has it or the hold
    /// passes, then the reading.
    #[test]
    fn a_knob_sends_one_value_when_let_go() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let label = "Vol";
        let mut centre = Pos2::ZERO;
        run(&ctx, 0.0, vec![Vec::new()], |ui| {
            centre = ui.cursor().min + vec2(KNOB_SIZE / 2.0, KNOB_SIZE / 2.0);
            knob(ui, label, 0.0, 0.0..=1.0, |v| format!("{v:.2}"))
        });
        let rise = KNOB_TRAVEL / 2.0;
        let steps = knob_drag(centre, rise);
        let release = steps.len() - 2;
        let sent = run(&ctx, 1.0, steps, |ui| {
            knob(ui, label, 0.0, 0.0..=1.0, |v| format!("{v:.2}"))
        });
        for (index, value) in sent.iter().enumerate() {
            if index == release {
                let value = value.expect("sent when let go");
                assert!((0.4..=0.5).contains(&value), "{value}");
            } else {
                assert_eq!(*value, None, "pass {index}");
            }
        }
        let value = sent[release].unwrap_or_default();
        let memory = |ctx: &egui::Context, reading: f32, time: f64| {
            let mut kept = None;
            run(ctx, time, vec![Vec::new()], |ui| {
                knob(ui, label, reading, 0.0..=1.0, |v| format!("{v:.2}"));
                let id = ui.make_persistent_id(("lab-ui-knob", label));
                kept = ui.data(|data| data.get_temp::<KnobMemory>(id));
            });
            kept
        };
        let released = 1.0 + f64::from(u16::try_from(release).expect("a few passes")) * 0.05;
        // Still waiting for the reading: the sent value stays.
        let held = memory(&ctx, 0.0, released + KNOB_HOLD_S / 2.0).and_then(|memory| memory.sent);
        assert_eq!(held.map(|(held, _)| held), Some(value));
        // The reading has it: the knob follows the reading again.
        assert!(memory(&ctx, value, released + KNOB_HOLD_S * 0.75).is_none());
        // A reading that never catches up is shown once the hold passes.
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let sent = run(&ctx, 1.0, knob_drag(centre, rise), |ui| {
            knob(ui, label, 0.0, 0.0..=1.0, |v| format!("{v:.2}"))
        });
        assert_eq!(sent.iter().flatten().count(), 1);
        assert!(memory(&ctx, 0.0, released + KNOB_HOLD_S * 1.5).is_none());
    }

    /// A larger knob takes its size, and a size under the plain knob's is
    /// the plain knob's.
    #[test]
    fn a_knob_takes_the_size_it_is_given() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let widths = run(&ctx, 0.0, vec![Vec::new(), Vec::new()], |ui| {
            let mut width = |size: f32| {
                ui.scope(|ui| knob_sized(ui, "V", 0.5, 0.0..=1.0, |v| format!("{v:.1}"), size))
                    .response
                    .rect
                    .width()
            };
            (width(72.0), width(10.0))
        });
        let (large, small) = widths[1];
        assert!((large - 72.0).abs() < 0.5, "{large}");
        assert!((small - KNOB_SIZE).abs() < 0.5, "{small}");
    }

    /// A popover opens when its anchor is clicked, stays open for clicks
    /// inside it and closes on a click outside.
    #[test]
    fn a_popover_opens_on_its_anchor_and_closes_outside() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let anchor = std::cell::Cell::new(Pos2::ZERO);
        let inside = std::cell::Cell::new(Pos2::ZERO);
        let draw = |ui: &mut Ui| {
            let response = ui.button("Volume");
            anchor.set(response.rect.center());
            popover(&response, |ui| {
                inside.set(ui.label("Master").rect.center());
            })
            .is_some()
        };
        assert_eq!(run(&ctx, 0.0, vec![Vec::new()], draw), [false]);
        let opened = run(&ctx, 1.0, click(anchor.get()), draw);
        assert_eq!(opened, [false, false, true, true]);
        let kept = run(&ctx, 2.0, click(inside.get()), draw);
        assert_eq!(kept, [true; 4]);
        let closed = run(&ctx, 3.0, click(pos2(580.0, 380.0)), draw);
        assert_eq!(closed.last(), Some(&false), "{closed:?}");
    }

    /// The Diagnostics panel's rows (`apps/worldlab/src/shell/diagnostics.rs`
    /// `row_ui`): a reading with a word longer than its row, such as a
    /// source's hash, wraps inside the row, and the rows after it start where
    /// the rows before it did.
    #[test]
    fn a_long_reading_wraps_inside_its_row() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let hash = "a676a4d0ad2bc6df01e9297ba63670892be1d4b19352efd7ceb59fdd3fda9f";
        let rows = [
            (
                "origin",
                "47.037900 deg  -122.900700 deg  datum 8.8 m".to_owned(),
            ),
            ("source", format!("registered EGM96/geoid surface {hash}")),
            (
                "source cache",
                "31.9 MiB  pinned 0.0 MiB  blocks 2878".to_owned(),
            ),
        ];
        let panel = egui::Rect::from_min_size(egui::pos2(600.0, 0.0), vec2(330.0, 600.0));
        let mut lefts = Vec::new();
        let mut room = 0.0;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                egui::ScrollArea::new([false, true]).show(ui, |ui| {
                    for (name, entries) in &rows {
                        let row = ui.horizontal(|ui| {
                            room = ui.available_width() - 56.0;
                            ui.allocate_ui(vec2(room, 0.0), |ui| reading_row(ui, name, entries));
                        });
                        lefts.push(row.response.rect.left());
                    }
                });
            });
        });
        assert!(
            lefts.iter().all(|left| (left - lefts[0]).abs() < 0.01),
            "{lefts:?}"
        );
        let mut lines = 0;
        for shape in &output.shapes {
            let egui::epaint::Shape::Text(text) = &shape.shape else {
                continue;
            };
            for row in &text.galley.rows {
                for glyph in &row.glyphs {
                    let x = text.pos.x + row.pos.x + glyph.pos.x;
                    assert!(
                        x >= lefts[0] - 0.5 && x + glyph.advance_width <= lefts[0] + room + 0.5,
                        "{:?} at {x} outside {} to {}",
                        glyph.chr,
                        lefts[0],
                        lefts[0] + room
                    );
                }
                lines += usize::from(!row.glyphs.is_empty());
            }
        }
        // The hash broke over two lines under the name's.
        assert!(lines >= 6, "{lines}");
        output.drop_without_applying_deltas();
    }

    /// A slider dragged along its track sends nothing while dragged and
    /// one value when let go, then shows it until the reading has it.
    #[test]
    fn a_slider_sends_one_value_when_let_go() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let look = SliderLook {
            step: 1.0,
            ..SliderLook::default()
        };
        let mut track = Rect::NOTHING;
        run(&ctx, 0.0, vec![Vec::new(), Vec::new()], |ui| {
            let start = ui.cursor().min;
            slider(ui, "day", 10.0, 1.0..=365.0, look);
            track = Rect::from_min_size(start, vec2(ui.spacing().slider_width, 18.0));
        });
        let from = pos2(track.left() + 4.0, track.center().y);
        let mut steps = vec![
            vec![egui::Event::PointerMoved(from)],
            vec![button(from, true)],
        ];
        for step in 1..=8u8 {
            let at = from + vec2(track.width() * 0.6 * f32::from(step) / 8.0, 0.0);
            steps.push(vec![egui::Event::PointerMoved(at)]);
        }
        let end = from + vec2(track.width() * 0.6, 0.0);
        steps.push(vec![button(end, false)]);
        steps.push(Vec::new());
        let release = steps.len() - 2;
        let sent = run(&ctx, 1.0, steps, |ui| {
            slider(ui, "day", 10.0, 1.0..=365.0, look)
        });
        for (index, value) in sent.iter().enumerate() {
            if index == release {
                let value = value.expect("sent when let go");
                assert!(value > 100.0 && value < 300.0, "{value}");
            } else {
                assert_eq!(*value, None, "pass {index}");
            }
        }
    }
}
