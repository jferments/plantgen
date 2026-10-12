//! Painting at exact places: text set at a point and cut to fit a width,
//! filled areas under curves, dashed and solid lines, arcs and rings, the
//! parts an instrument card's figures are made of.
//!
//! A card on a [`crate::board`] paints rather than lays out: each of its
//! figures has a rectangle worked out from the card's, and every word is
//! set at a point with a width it may not pass ([`text_within`] cuts it
//! with an ellipsis). Text that cannot wrap and is laid out in a row
//! pushes its row wider than the room it was given; painted at a point it
//! stays where it was put.
use std::f32::consts::TAU;

use egui::{
    Align, Align2, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Ui,
    epaint::PathStroke,
    pos2,
    text::{LayoutJob, TextFormat, TextWrapping},
    vec2,
};

use crate::theme;

/// The fonts a card writes in. The strong and icon families exist only
/// once the theme's fonts are live (see [`theme::strong_font`]), so a pen
/// is made from the pass's [`Ui`] and falls back to plain text before.
#[derive(Debug, Clone, Copy)]
pub struct Pen {
    strong: bool,
    icons: bool,
}

impl Pen {
    /// The fonts live in `ui`'s context this pass.
    #[must_use]
    pub fn of(ui: &Ui) -> Self {
        ui.ctx().fonts(|fonts| {
            let families = &fonts.definitions().families;
            Self {
                strong: families.contains_key(&theme::strong()),
                icons: families.contains_key(&theme::icons()),
            }
        })
    }

    /// Inter at `size` points.
    #[must_use]
    pub fn regular(self, size: f32) -> FontId {
        FontId::proportional(size)
    }

    /// Inter `SemiBold` at `size` points: titles, headline numbers.
    #[must_use]
    pub fn strong(self, size: f32) -> FontId {
        if self.strong {
            FontId::new(size, theme::strong())
        } else {
            FontId::proportional(size)
        }
    }

    /// `JetBrains Mono` at `size` points: numbers that change while you
    /// watch, whose digits all take one width.
    #[must_use]
    pub fn mono(self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// An icon at `size` points.
    #[must_use]
    pub fn icon(self, size: f32) -> FontId {
        if self.icons {
            FontId::new(size, theme::icons())
        } else {
            FontId::proportional(size)
        }
    }
}

/// An axis label's size, points.
pub const AXIS: f32 = 10.5;
/// A label's size beside a mark, points.
pub const LABEL: f32 = 11.0;
/// A label in bold on a mark, points.
pub const MARK: f32 = 11.5;

/// Sets `text` at `pos`, aligned there by `align`, on one line; returns
/// where it went.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn text(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    colour: Color32,
) -> Rect {
    painter.text(pos, align, text, font, colour)
}

/// Sets `text` at `pos` as [`text`] does, cut with an ellipsis where it
/// would pass `width` points; nothing for a width under a few points.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn text_within(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    colour: Color32,
    width: f32,
) -> Rect {
    if width < 6.0 || text.is_empty() {
        return Rect::from_min_size(pos, egui::Vec2::ZERO);
    }
    let job = one_line(text, font, colour, width, 0.0);
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, colour);
    rect
}

/// Labels that give way to each other: each is set only where it meets no
/// label set before it, so a caller adds the most important first. An
/// axis whose guides crowd together (a budget line next to the floor when
/// the scale is a hundred times larger) then shows the labels that fit
/// instead of stacking them.
#[derive(Debug, Clone, Default)]
pub struct Labels {
    placed: Vec<Rect>,
}

impl Labels {
    /// No labels yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `rect` clear for later labels, as though a label were set there.
    pub fn reserve(&mut self, rect: Rect) {
        self.placed.push(rect);
    }

    /// Sets `text` as [`text`] does unless it would meet a label already
    /// set (with `GAP` points between); returns where it went, if it did.
    pub fn text(
        &mut self,
        painter: &Painter,
        pos: Pos2,
        align: Align2,
        text: &str,
        font: FontId,
        colour: Color32,
    ) -> Option<Rect> {
        const GAP: f32 = 3.0;
        let galley = painter.layout_no_wrap(text.to_owned(), font, colour);
        let rect = align.anchor_size(pos, galley.size());
        if self
            .placed
            .iter()
            .any(|placed| placed.expand(GAP).intersects(rect))
        {
            return None;
        }
        painter.galley(rect.min, galley, colour);
        self.placed.push(rect);
        Some(rect)
    }
}

/// Sets `text` left-aligned at `pos` (its middle at `pos.y`) with `spacing`
/// points between letters, cut to `width`; returns where it went.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn spaced(
    painter: &Painter,
    pos: Pos2,
    text: &str,
    font: FontId,
    colour: Color32,
    spacing: f32,
    width: f32,
) -> Rect {
    if width < 6.0 {
        return Rect::from_min_size(pos, egui::Vec2::ZERO);
    }
    let job = one_line(text, font, colour, width, spacing);
    let galley = painter.layout_job(job);
    let rect = Align2::LEFT_CENTER.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, colour);
    rect
}

/// How wide `text` sets in `font`, points.
#[must_use]
pub fn width_of(painter: &Painter, text: &str, font: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
}

/// One line of `text`, cut with an ellipsis at `width`.
fn one_line(text: &str, font: FontId, colour: Color32, width: f32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color: colour,
            extra_letter_spacing: spacing,
            valign: Align::Center,
            ..TextFormat::default()
        },
    );
    job.wrap = TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    job
}

/// Sets a value and its unit on one baseline from `pos` (the value's left
/// end, the line's middle): the value in `font`, the unit after it,
/// smaller and fainter. Returns where both went.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn value_unit(
    painter: &Painter,
    pos: Pos2,
    value: &str,
    font: FontId,
    unit: &str,
    unit_font: FontId,
) -> Rect {
    let value_galley = painter.layout_no_wrap(value.to_owned(), font, theme::TEXT);
    let value_size = value_galley.size();
    let top = pos.y - value_size.y / 2.0;
    let ascent = value_galley.rows.first().map_or(value_size.y, |row| {
        row.glyphs
            .first()
            .map_or(value_size.y * 0.8, |glyph| glyph.font_ascent)
    });
    painter.galley(pos2(pos.x, top), value_galley, theme::TEXT);
    let mut rect = Rect::from_min_size(pos2(pos.x, top), value_size);
    if !unit.is_empty() {
        let unit_galley = painter.layout_no_wrap(unit.to_owned(), unit_font, theme::TEXT_2);
        let unit_ascent = unit_galley
            .rows
            .first()
            .map_or(unit_galley.size().y, |row| {
                row.glyphs
                    .first()
                    .map_or(unit_galley.size().y * 0.8, |glyph| glyph.font_ascent)
            });
        // Same baseline: the unit's top sits lower by the difference of
        // the two ascents.
        let at = pos2(rect.right() + 4.0, top + ascent - unit_ascent);
        let unit_rect = Rect::from_min_size(at, unit_galley.size());
        painter.galley(at, unit_galley, theme::TEXT_2);
        rect = rect.union(unit_rect);
    }
    rect
}

/// A straight line.
pub fn line(painter: &Painter, from: Pos2, to: Pos2, width: f32, colour: Color32) {
    painter.line_segment([from, to], Stroke::new(width, colour));
}

/// A dashed line of `dash` points on, `gap` points off.
pub fn dashed(
    painter: &Painter,
    from: Pos2,
    to: Pos2,
    width: f32,
    colour: Color32,
    dash: f32,
    gap: f32,
) {
    painter.extend(Shape::dashed_line(
        &[from, to],
        Stroke::new(width, colour),
        dash,
        gap,
    ));
}

/// A dashed path through `points`.
pub fn dashed_path(
    painter: &Painter,
    points: &[Pos2],
    width: f32,
    colour: Color32,
    dash: f32,
    gap: f32,
) {
    if points.len() >= 2 {
        painter.extend(Shape::dashed_line(
            points,
            Stroke::new(width, colour),
            dash,
            gap,
        ));
    }
}

/// A line through `points`.
pub fn polyline(painter: &Painter, points: Vec<Pos2>, width: f32, colour: Color32) {
    if points.len() >= 2 {
        painter.add(Shape::line(points, PathStroke::new(width, colour)));
    }
}

/// The area between two curves sampled at the same `xs`: from `low` up to
/// `high` (screen y of each), filled. The curves need not be straight or
/// convex: the area is drawn as a strip of quads.
pub fn band(painter: &Painter, xs: &[f32], low: &[f32], high: &[f32], colour: Color32) {
    let count = xs.len().min(low.len()).min(high.len());
    if count < 2 {
        return;
    }
    let mut mesh = Mesh::default();
    for index in 0..count {
        mesh.colored_vertex(pos2(xs[index], high[index]), colour);
        mesh.colored_vertex(pos2(xs[index], low[index]), colour);
    }
    for index in 0..count - 1 {
        #[allow(clippy::cast_possible_truncation)] // Meshes stay well under 2^32 vertices.
        let base = (index * 2) as u32;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// A filled rectangle with even corners.
pub fn block(painter: &Painter, rect: Rect, round: f32, colour: Color32) {
    if rect.width() > 0.0 && rect.height() > 0.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few points.
        let round = round
            .clamp(0.0, rect.width().min(rect.height()) / 2.0)
            .round() as u8;
        painter.rect_filled(rect, round, colour);
    }
}

/// A point at `angle_deg` (clockwise from up, so north on a map) and
/// `radius` from `centre`.
#[must_use]
pub fn polar(centre: Pos2, radius: f32, angle_deg: f32) -> Pos2 {
    let angle = (angle_deg - 90.0).to_radians();
    pos2(
        centre.x + radius * angle.cos(),
        centre.y + radius * angle.sin(),
    )
}

/// Points along an arc from `from_deg` to `to_deg` (clockwise from up).
#[must_use]
pub fn arc_points(centre: Pos2, radius: f32, from_deg: f32, to_deg: f32) -> Vec<Pos2> {
    let sweep = to_deg - from_deg;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // A few dozen steps.
    let steps = ((sweep.abs() / 360.0 * 120.0).ceil() as usize).max(2);
    #[allow(clippy::cast_precision_loss)] // A few dozen steps.
    (0..=steps)
        .map(|step| {
            polar(
                centre,
                radius,
                from_deg + sweep * step as f32 / steps as f32,
            )
        })
        .collect()
}

/// An arc stroke from `from_deg` to `to_deg`, `width` points wide, its
/// ends rounded when `round` is set.
pub fn arc(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    (from_deg, to_deg): (f32, f32),
    width: f32,
    colour: Color32,
    round: bool,
) {
    if (to_deg - from_deg).abs() < 0.05 {
        return;
    }
    let points = arc_points(centre, radius, from_deg, to_deg);
    if round {
        for end in [points[0], points[points.len() - 1]] {
            painter.circle_filled(end, width / 2.0, colour);
        }
    }
    painter.add(Shape::line(points, PathStroke::new(width, colour)));
}

/// A ring segment (a thick arc drawn as a filled strip), from `from_deg`
/// to `to_deg`, between `inner` and `outer` radii.
pub fn ring(
    painter: &Painter,
    centre: Pos2,
    (inner, outer): (f32, f32),
    (from_deg, to_deg): (f32, f32),
    colour: Color32,
) {
    if (to_deg - from_deg).abs() < 0.05 || outer <= inner {
        return;
    }
    let inside = arc_points(centre, inner, from_deg, to_deg);
    let outside = arc_points(centre, outer, from_deg, to_deg);
    let mut mesh = Mesh::default();
    for (a, b) in outside.iter().zip(&inside) {
        mesh.colored_vertex(*a, colour);
        mesh.colored_vertex(*b, colour);
    }
    for index in 0..outside.len() - 1 {
        #[allow(clippy::cast_possible_truncation)] // A few hundred vertices.
        let base = (index * 2) as u32;
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// A closed outline through `points`, filled with `fill` and stroked with
/// `stroke`. The fill is a fan from the outline's middle, right for the
/// star-shaped outlines a radar or a range draws.
pub fn star(painter: &Painter, points: &[Pos2], fill: Color32, stroke: Stroke) {
    if points.len() < 3 {
        return;
    }
    #[allow(clippy::cast_precision_loss)] // A few dozen points.
    let middle = points
        .iter()
        .fold(egui::Vec2::ZERO, |sum, point| sum + point.to_vec2())
        / points.len() as f32;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(middle.to_pos2(), fill);
    for point in points {
        mesh.colored_vertex(*point, fill);
    }
    #[allow(clippy::cast_possible_truncation)] // A few dozen points.
    let count = points.len() as u32;
    for index in 0..count {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % count);
    }
    painter.add(Shape::mesh(mesh));
    let mut outline = points.to_vec();
    outline.push(points[0]);
    painter.add(Shape::line(
        outline,
        PathStroke::new(stroke.width, stroke.color),
    ));
}

/// A regular polygon's corners, `sides` of them, the first straight up.
#[must_use]
pub fn polygon(centre: Pos2, radius: f32, sides: usize) -> Vec<Pos2> {
    #[allow(clippy::cast_precision_loss)] // A few sides.
    (0..sides)
        .map(|side| polar(centre, radius, side as f32 * 360.0 / sides as f32))
        .collect()
}

/// A small arrow pointing along `to - from`: a line and two barbs.
pub fn arrow(painter: &Painter, from: Pos2, to: Pos2, barb: f32, width: f32, colour: Color32) {
    let stroke = Stroke::new(width, colour);
    painter.line_segment([from, to], stroke);
    let direction = (to - from).normalized();
    if direction.length() < 0.5 {
        return;
    }
    let angle = direction.angle();
    for side in [-1.0_f32, 1.0] {
        let barb_angle = angle + std::f32::consts::PI + side * 0.5;
        let end = to + vec2(barb_angle.cos(), barb_angle.sin()) * barb;
        painter.line_segment([to, end], stroke);
    }
}

/// A circle's outline, dashed.
pub fn dashed_circle(painter: &Painter, centre: Pos2, radius: f32, width: f32, colour: Color32) {
    let steps = 96;
    #[allow(clippy::cast_precision_loss)] // A few dozen steps.
    let mut points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let angle = step as f32 / steps as f32 * TAU;
            pos2(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
            )
        })
        .collect();
    if points.len() > 1 {
        points.dedup();
        dashed_path(painter, &points, width, colour, 4.0, 3.0);
    }
}

/// A rounded box with text in it, as a label set on a figure: its left
/// end (or right, by `align`) at `pos`, its middle at `pos.y`.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn tag(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    colour: Color32,
    fill: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), font, colour);
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = align.anchor_size(pos, size);
    painter.rect_filled(rect, 4, fill);
    painter.galley(rect.min + vec2(5.0, 2.0), galley, colour);
    rect
}

/// A legend's swatch and its name: the swatch, then the name in faint
/// text and, when there is one, the value after it in the text ink.
/// Returns where it ended.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn legend_entry(
    painter: &Painter,
    pos: Pos2,
    colour: Color32,
    name: &str,
    value: &str,
    font: FontId,
) -> f32 {
    let swatch = Rect::from_center_size(pos2(pos.x + 4.5, pos.y), vec2(9.0, 9.0));
    painter.rect_filled(swatch, 2, colour);
    let name_rect = painter.text(
        pos2(swatch.right() + 5.0, pos.y),
        Align2::LEFT_CENTER,
        name,
        font.clone(),
        theme::TEXT_2,
    );
    if value.is_empty() {
        return name_rect.right();
    }
    painter
        .text(
            pos2(name_rect.right() + 4.0, pos.y),
            Align2::LEFT_CENTER,
            value,
            font,
            theme::TEXT,
        )
        .right()
}

/// How wide [`legend_entry`] draws.
#[must_use]
pub fn legend_width(painter: &Painter, name: &str, value: &str, font: &FontId) -> f32 {
    let mut width = 14.0 + width_of(painter, name, font.clone());
    if !value.is_empty() {
        width += 4.0 + width_of(painter, value, font.clone());
    }
    width
}

/// Lays legend entries left to right from `pos`, wrapping onto the next
/// line `line` points down when one would pass `right`; returns the
/// bottom of the last line.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn legend_row(
    painter: &Painter,
    pos: Pos2,
    right: f32,
    line: f32,
    entries: &[(Color32, String, String)],
    font: &FontId,
) -> f32 {
    let mut at = pos;
    for (colour, name, value) in entries {
        let width = legend_width(painter, name, value, font);
        if at.x > pos.x && at.x + width > right {
            at = pos2(pos.x, at.y + line);
        }
        legend_entry(painter, at, *colour, name, value, font.clone());
        at.x += width + 10.0;
    }
    at.y + line / 2.0
}

#[cfg(test)]
mod tests {
    use egui::{Context, RawInput, pos2};

    use super::*;

    #[test]
    fn labels_that_would_meet_give_way_to_the_first() {
        let ctx = Context::default();
        crate::test_pass(&ctx, RawInput::default(), |ui| {
            let painter = ui.painter();
            let font = FontId::proportional(AXIS);
            let mut labels = Labels::new();
            let first = labels.text(
                painter,
                pos2(100.0, 100.0),
                Align2::RIGHT_CENTER,
                "16.7",
                font.clone(),
                Color32::WHITE,
            );
            let crowded = labels.text(
                painter,
                pos2(100.0, 104.0),
                Align2::RIGHT_CENTER,
                "8.3",
                font.clone(),
                Color32::WHITE,
            );
            let clear = labels.text(
                painter,
                pos2(100.0, 140.0),
                Align2::RIGHT_CENTER,
                "0",
                font.clone(),
                Color32::WHITE,
            );
            assert!(first.is_some());
            assert!(crowded.is_none());
            assert!(clear.is_some());
            labels.reserve(Rect::from_min_size(
                pos2(0.0, 200.0),
                egui::vec2(200.0, 20.0),
            ));
            assert!(
                labels
                    .text(
                        painter,
                        pos2(50.0, 210.0),
                        Align2::CENTER_CENTER,
                        "now",
                        font,
                        Color32::WHITE
                    )
                    .is_none()
            );
        });
    }
}
