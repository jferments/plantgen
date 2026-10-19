//! The library browser: every species in a tree of its group, order,
//! family and genus, with a search over its names and ranks, a filter by
//! growth form and the species chosen lately. A click chooses a species
//! (nothing grows until Generate); a double click chooses and generates
//! it. A dot marks the species with a kept render, whose picture shows
//! when the pointer rests on it.

use bevy_egui::egui::{self, Align2, Color32, RichText, Sense, Ui, vec2};
use plantlab_ui::{icons, theme, widgets};

use super::catalogue::{Form, Item, Node};
use super::command::{Command, Shell};
use super::{Lab, store};

/// A row's height, points, and how far each rank indents.
const ROW: f32 = 24.0;
const SPECIES_ROW: f32 = 34.0;
const INDENT: f32 = 14.0;

/// Draws the browser.
pub fn show(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Library")
                .font(theme::strong_font(ui, theme::HEADING))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(format!("{} species", lab.catalogue.items.len()))
                .size(theme::SMALL)
                .color(theme::TEXT_3),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icons::X, "Hide the library (L)", false) {
                shell.send(Command::Library(false));
            }
        });
    });
    ui.add_space(theme::GAP_SMALL);
    search(ui, shell);
    ui.add_space(theme::GAP_SMALL);
    forms(ui, lab, shell);
    ui.add_space(theme::GAP_SMALL);
    let query = shell.filter.trim().to_lowercase();
    let form = shell.form;
    let keep = |item: &Item| form.is_none_or(|form| item.form == form) && item.matches(&query);
    egui::ScrollArea::vertical()
        .id_salt("library")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if query.is_empty() && form.is_none() && !lab.recent.is_empty() {
                widgets::section(ui, "Recent");
                for id in lab.recent.iter().take(5) {
                    if let Some(index) = lab.catalogue.items.iter().position(|item| item.id == *id)
                    {
                        species_row(ui, lab, shell, index, 0.0);
                    }
                }
            }
            let tree = lab.catalogue.tree(keep);
            if tree.is_empty() {
                ui.add_space(theme::GAP);
                ui.label(
                    RichText::new(
                        "No species matches. Try a common or scientific name, a genus, \
                                   a family or an order.",
                    )
                    .color(theme::TEXT_3),
                );
                return;
            }
            if shell.reveal
                && let Some(item) = lab.catalogue.get(&lab.chosen)
            {
                for key in [
                    key("group", &item.group),
                    key("order", &item.order),
                    key("family", &item.family),
                    key("genus", &item.genus),
                ] {
                    shell.expanded.insert(key);
                }
            }
            // While searching, every match shows with its ranks open.
            let open_all = !query.is_empty();
            for group in &tree {
                node(ui, lab, shell, group, 0, open_all);
            }
        });
    shell.reveal = false;
}

/// The search field, with a button that clears it.
fn search(ui: &mut Ui, shell: &mut Shell) {
    egui::Frame::new()
        .fill(theme::SURFACE_0)
        .stroke(egui::Stroke::new(1.0, theme::LINE_STRONG))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icons::MAGNIFYING_GLASS)
                        .font(theme::icon_font(ui, 14.0))
                        .color(theme::TEXT_3),
                );
                let clear = !shell.filter.is_empty();
                let width = ui.available_width() - if clear { 26.0 } else { 0.0 };
                ui.add(
                    egui::TextEdit::singleline(&mut shell.filter)
                        .hint_text("Name, genus, family or order")
                        .frame(egui::Frame::NONE)
                        .desired_width(width),
                );
                if clear && widgets::icon_button(ui, icons::X, "Clear the search", false) {
                    shell.filter.clear();
                    shell.reveal = true;
                }
            });
        });
}

/// The growth form filter: a chip a form, with how many it has.
fn forms(ui: &mut Ui, lab: &Lab, shell: &mut Shell) {
    let counts = lab.catalogue.forms();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        if widgets::chip(ui, "All", shell.form.is_none()) {
            shell.form = None;
            shell.reveal = true;
        }
        for form in Form::ALL {
            let Some(count) = counts.get(&form) else {
                continue;
            };
            let on = shell.form == Some(form);
            if widgets::chip(ui, &format!("{} {count}", form.plural()), on) {
                shell.form = if on { None } else { Some(form) };
                shell.reveal = true;
            }
        }
    });
}

/// A node's key in [`Shell::expanded`].
fn key(rank: &str, name: &str) -> String {
    format!("{rank}:{name}")
}

/// A rank's row and, open, what it holds. A genus with one species, in a
/// family of one genus, shows its species straight under the family.
fn node(ui: &mut Ui, lab: &Lab, shell: &mut Shell, node: &Node, depth: usize, open_all: bool) {
    let key = key(node.rank, &node.name);
    let open = open_all || shell.expanded.contains(&key);
    #[allow(clippy::cast_precision_loss)]
    let indent = depth as f32 * INDENT;
    let holds_chosen = node
        .items
        .iter()
        .any(|&index| lab.catalogue.items[index].id == lab.chosen);
    if rank_row(ui, node, indent, open, holds_chosen).clicked() {
        if open {
            shell.expanded.remove(&key);
        } else {
            shell.expanded.insert(key);
        }
    }
    if !open {
        return;
    }
    if node.rank == "family" && node.children.len() == 1 {
        for &index in &node.children[0].items {
            species_row(ui, lab, shell, index, indent + INDENT);
        }
        return;
    }
    if node.rank == "genus" {
        for &index in &node.items {
            species_row(ui, lab, shell, index, indent + INDENT);
        }
        return;
    }
    for child in &node.children {
        self::node(ui, lab, shell, child, depth + 1, open_all);
    }
}

/// A rank's row: a caret, its name (a group's in capitals) and how many
/// species it holds.
fn rank_row(
    ui: &mut Ui,
    node: &Node,
    indent: f32,
    open: bool,
    holds_chosen: bool,
) -> egui::Response {
    let group = node.rank == "group";
    let height = if group { ROW + 6.0 } else { ROW };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, theme::RADIUS_SMALL, theme::SURFACE_2);
    }
    let caret = if open {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    };
    let left = rect.left() + indent + 4.0;
    let mid = if group {
        rect.bottom() - ROW / 2.0
    } else {
        rect.center().y
    };
    painter.text(
        egui::pos2(left + 5.0, mid),
        Align2::CENTER_CENTER,
        caret,
        theme::icon_font(ui, 11.0),
        theme::TEXT_3,
    );
    let (text, font, ink) = if group {
        (
            node.name.to_uppercase(),
            theme::strong_font(ui, theme::SMALL),
            theme::TEXT_2,
        )
    } else {
        let ink = if holds_chosen {
            theme::TEXT
        } else {
            theme::TEXT_2
        };
        let font = if node.rank == "order" {
            theme::strong_font(ui, theme::BODY)
        } else {
            egui::FontId::proportional(theme::BODY)
        };
        (node.name.clone(), font, ink)
    };
    let rank = match node.rank {
        "order" => "order",
        "family" => "family",
        "genus" => "genus",
        _ => "",
    };
    let galley = painter.layout_no_wrap(text, font, ink);
    let name_at = egui::pos2(left + 16.0, mid - galley.size().y / 2.0);
    let name_width = galley.size().x;
    painter.galley(name_at, galley, ink);
    if !rank.is_empty() {
        painter.text(
            egui::pos2(name_at.x + name_width + 6.0, mid),
            Align2::LEFT_CENTER,
            rank,
            egui::FontId::proportional(theme::SMALL - 1.0),
            theme::TEXT_3.gamma_multiply(0.8),
        );
    }
    painter.text(
        egui::pos2(rect.right() - 6.0, mid),
        Align2::RIGHT_CENTER,
        node.items.len().to_string(),
        egui::FontId::monospace(theme::SMALL),
        theme::TEXT_3,
    );
    response
}

/// A species' row: its common name over its scientific one (the genus's
/// name in it where its genus has no row of its own), a dot when it has a
/// kept render, raised and marked when chosen. A click chooses it; a
/// double click generates it too.
fn species_row(ui: &mut Ui, lab: &Lab, shell: &mut Shell, index: usize, indent: f32) {
    let item = &lab.catalogue.items[index];
    let chosen = item.id == lab.chosen;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), SPECIES_ROW), Sense::click());
    if chosen && shell.reveal {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let band = rect.shrink2(vec2(0.0, 1.0));
        if chosen {
            painter.rect_filled(band, theme::RADIUS_SMALL, theme::ACCENT_SOFT);
            let marker = egui::Rect::from_min_size(
                egui::pos2(band.left(), band.top() + 6.0),
                vec2(3.0, band.height() - 12.0),
            );
            painter.rect_filled(marker, 1.0, theme::ACCENT);
        } else if response.hovered() {
            painter.rect_filled(band, theme::RADIUS_SMALL, theme::SURFACE_2);
        }
        let left = rect.left() + indent + 20.0;
        let kept = lab.store.kept.contains_key(&item.id);
        if kept {
            painter.circle_filled(
                egui::pos2(left - 9.0, rect.top() + 11.0),
                3.0,
                if chosen { theme::ACCENT } else { theme::TEXT_3 },
            );
        }
        let width = rect.right() - left - 6.0;
        let title = one_line(ui, item.title(), theme::BODY, theme::TEXT, false, width);
        painter.galley(egui::pos2(left, rect.top() + 3.0), title, theme::TEXT);
        let second = if item.common.is_empty() {
            item.form.one()
        } else {
            &item.scientific
        };
        let italics = !item.common.is_empty();
        let galley = one_line(ui, second, theme::SMALL, theme::TEXT_3, italics, width);
        painter.galley(egui::pos2(left, rect.top() + 19.0), galley, theme::TEXT_3);
    }
    let response = response.on_hover_ui(|ui| hover(ui, lab, shell, item));
    if response.double_clicked() {
        shell.send(Command::Choose(item.id.clone()));
        shell.send(Command::Generate);
    } else if response.clicked() {
        shell.send(Command::Choose(item.id.clone()));
    }
}

/// `text` on one line at most `width` wide, cut with an ellipsis.
fn one_line(
    ui: &Ui,
    text: &str,
    size: f32,
    colour: Color32,
    italics: bool,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color: colour,
            italics,
            ..Default::default()
        },
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// What shows while the pointer rests on a species: its names and ranks,
/// and its kept render with when it was made.
fn hover(ui: &mut Ui, lab: &Lab, shell: &mut Shell, item: &Item) {
    ui.set_max_width(300.0);
    ui.label(
        RichText::new(item.title())
            .font(theme::strong_font(ui, theme::BODY))
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
            "{} · {} · {}",
            item.form.one(),
            item.family,
            item.order
        ))
        .size(theme::SMALL)
        .color(theme::TEXT_3),
    );
    match lab.store.kept.get(&item.id) {
        Some(kept) => {
            if let Some(texture) = picture(ui.ctx(), &lab.store, shell, &item.id) {
                let size = texture.size_vec2();
                let scale = (280.0 / size.x).min(200.0 / size.y);
                ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
            }
            ui.label(
                RichText::new(format!(
                    "Last render {}, {:.1} m tall",
                    store::ago(kept.saved, store::now()),
                    kept.height_m
                ))
                .size(theme::SMALL)
                .color(theme::TEXT_2),
            );
        }
        None => {
            ui.label(
                RichText::new("Not rendered yet")
                    .size(theme::SMALL)
                    .color(theme::TEXT_3),
            );
        }
    }
    ui.label(
        RichText::new("Click to choose, double-click to generate")
            .size(theme::SMALL)
            .color(theme::TEXT_3),
    );
}

/// `id`'s kept picture as a texture, loaded once for each time it was
/// written; `None` where there is none or it does not load.
pub fn picture(
    ctx: &egui::Context,
    store: &store::Store,
    shell: &mut Shell,
    id: &str,
) -> Option<egui::TextureHandle> {
    let written = store.written.get(id).copied().unwrap_or(0);
    if let Some((loaded, texture)) = shell.pictures.get(id)
        && *loaded == written
    {
        return texture.clone();
    }
    let texture = store
        .picture(id)
        .and_then(|path| store::read_png(&path))
        .map(|(width, height, rgba)| {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            ctx.load_texture(
                format!("plantlab-kept-{id}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
    shell
        .pictures
        .insert(id.to_owned(), (written, texture.clone()));
    texture
}
