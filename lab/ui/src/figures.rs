//! Figures: what an instrument card draws, each painted into a rectangle
//! the card works out for it ([`crate::board`]). Stacked bars and share
//! bars, donuts, radars, heat ramps, log scales, columns of recent values,
//! and the histories and histograms they are drawn from.
//!
//! A figure paints and keeps nothing: the caller reads its numbers from its
//! own application each pass and gives each figure its rectangle, and what
//! a figure paints stays inside the painter's clip, so nothing it holds can
//! widen a page. Colours come from [`theme`]: a figure's series take
//! [`theme::SERIES`] in its fixed order (the caller gives each part its
//! slot), magnitudes take [`theme::HEAT`] ([`heat`]), and text stays in a
//! text ink, never a series colour, except a part's name set on the part
//! itself ([`theme::ON_MARK`]). Parts sit [`PART_GAP`] apart, so
//! neighbours never touch. [`hover_parts`] names the part under the
//! pointer.
use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui,
    pos2, vec2,
};

use crate::{canvas, theme};

/// One part of a whole: its name, its value, its colour and the value as
/// text for a tooltip or a legend (`3.1 ms`).
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub value: f32,
    pub colour: Color32,
    pub text: String,
}

impl Part {
    pub fn new(
        name: impl Into<String>,
        value: f32,
        colour: Color32,
        text: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value,
            colour,
            text: text.into(),
        }
    }
}

/// Points of surface left between two parts.
pub const PART_GAP: f32 = 2.0;
/// A stacked bar's height, points.
pub const BAR_HEIGHT: f32 = 10.0;

/// The value a scale's top edge takes for `value`: the next of 1, 2 or 5
/// times a power of ten at or above it; 1 for nothing to show.
#[must_use]
pub fn nice_top(value: f32) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        return 1.0;
    }
    let power = 10_f32.powf(value.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|step| step * power)
        .find(|top| *top >= value * 0.999_9)
        .unwrap_or(10.0 * power)
}

/// The parts of a stacked bar across `width` points from `left`: each
/// part's left and right edge, scaled so `total` (or the parts' sum, if
/// larger) fills the width, with [`PART_GAP`] after every part but the
/// last drawn. Parts of no width are `None`.
#[must_use]
pub fn bar_spans(parts: &[Part], total: f32, left: f32, width: f32) -> Vec<Option<(f32, f32)>> {
    let sum: f32 = parts.iter().map(|part| part.value.max(0.0)).sum();
    let scale = total.max(sum);
    if scale <= 0.0 {
        return vec![None; parts.len()];
    }
    let last = parts.iter().rposition(|part| part.value > 0.0);
    let mut x = left;
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let span = part.value.max(0.0) / scale * width;
            let start = x;
            x += span;
            if span <= 0.0 {
                return None;
            }
            let gap = if Some(index) == last { 0.0 } else { PART_GAP };
            Some((start, (x - gap).max(start + 1.0)))
        })
        .collect()
}

/// A rounding for a mark `height` points high: rounded data ends, at most
/// three points.
#[must_use]
pub fn round_for(height: f32) -> f32 {
    (height / 2.0).min(3.0)
}

/// A bar in `rect`: `parts` left to right, scaled so `total` (or their sum,
/// if larger) fills it, each rounded and [`PART_GAP`] from the next, over
/// `track` when there is one. With `names`, a part wide enough sets its
/// name and value inside it. Returns each part's span for
/// [`hover_parts`].
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the spans.
pub fn stacked_bar(
    painter: &Painter,
    rect: Rect,
    parts: &[Part],
    total: f32,
    track: Option<Color32>,
    names: Option<&FontId>,
) -> Vec<Option<(f32, f32)>> {
    let spans = bar_spans(parts, total, rect.left(), rect.width());
    let round = round_for(rect.height());
    if let Some(track) = track {
        canvas::block(painter, rect, round, track);
    }
    for (part, span) in parts.iter().zip(&spans) {
        let Some((x0, x1)) = *span else { continue };
        let mark = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom()));
        canvas::block(painter, mark, round, part.colour);
        let Some(font) = names else { continue };
        let label = if part.text.is_empty() {
            part.name.clone()
        } else {
            format!("{} {}", part.name, part.text)
        };
        let ink = on_mark(part.colour);
        let room = mark.width() - 10.0;
        let full = canvas::width_of(painter, &label, font.clone());
        if full <= room {
            canvas::text(
                painter,
                pos2(mark.left() + 5.0, mark.center().y),
                Align2::LEFT_CENTER,
                &label,
                font.clone(),
                ink,
            );
        } else if canvas::width_of(painter, &part.text, font.clone()) <= room {
            canvas::text(
                painter,
                pos2(mark.left() + 5.0, mark.center().y),
                Align2::LEFT_CENTER,
                &part.text,
                font.clone(),
                ink,
            );
        }
    }
    spans
}

/// The ink for a label set on a mark of `colour`: [`theme::ON_MARK`] on a
/// series colour, the text ink on the grey or a dark mark.
#[must_use]
pub fn on_mark(colour: Color32) -> Color32 {
    let [r, g, b, _] = colour.to_array();
    let light = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
    if light >= 100.0 {
        theme::ON_MARK
    } else {
        theme::TEXT
    }
}

/// Names the part under the pointer in a tooltip: `spans` are each part's
/// left and right edge across `rect`, as [`stacked_bar`] returns them.
pub fn hover_parts(ui: &Ui, rect: Rect, id: Id, spans: &[Option<(f32, f32)>], parts: &[Part]) {
    let response = ui.interact(rect, id, Sense::hover());
    let Some(point) = response.hover_pos() else {
        return;
    };
    let hovered = spans
        .iter()
        .position(|span| span.is_some_and(|(x0, x1)| point.x >= x0 && point.x <= x1 + PART_GAP));
    if let Some(index) = hovered {
        let part = &parts[index];
        response.on_hover_text_at_pointer(format!("{}  {}", part.name, part.text));
    }
}

/// Shows `text` in a tooltip while the pointer is over `rect`.
pub fn hover_text(ui: &Ui, rect: Rect, id: Id, text: impl Into<egui::WidgetText>) {
    let response = ui.interact(rect, id, Sense::hover());
    if response.hovered() {
        response.on_hover_text_at_pointer(text);
    }
}

/// A bar in `rect` filled by `share` (0 to 1) in `colour` over its track.
pub fn share_bar(painter: &Painter, rect: Rect, share: f32, colour: Color32) {
    let round = round_for(rect.height());
    canvas::block(painter, rect, round, theme::SURFACE_3);
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if share > 0.0 {
        let mut fill = rect;
        fill.set_width((rect.width() * share).max(rect.height().min(rect.width())));
        canvas::block(painter, fill, round, colour);
    }
}

/// A ring round `centre` between the `inner` and `outer` radii: `parts`
/// clockwise from the top, a gap between each, over a track. With nothing
/// to show it is the track alone.
pub fn donut(painter: &Painter, centre: Pos2, (inner, outer): (f32, f32), parts: &[Part]) {
    canvas::ring(
        painter,
        centre,
        (inner, outer),
        (0.0, 359.99),
        theme::SURFACE_3,
    );
    let sum: f32 = parts.iter().map(|part| part.value.max(0.0)).sum();
    if sum <= 0.0 {
        return;
    }
    let drawn = parts.iter().filter(|part| part.value > 0.0).count();
    // A gap of PART_GAP points at the middle of the ring.
    let gap = if drawn > 1 {
        PART_GAP / f32::midpoint(inner, outer) * 180.0 / std::f32::consts::PI
    } else {
        0.0
    };
    let mut angle = 0.0;
    for part in parts.iter().filter(|part| part.value > 0.0) {
        let sweep = part.value / sum * 360.0;
        let end = angle + sweep;
        if sweep > gap {
            canvas::ring(
                painter,
                centre,
                (inner, outer),
                (angle, end - gap),
                part.colour,
            );
        }
        angle = end;
    }
}

/// A radar of shares round `centre`: each axis, clockwise from the top, a
/// value from 0 to 1 and its name set outside the rim; rings at a third,
/// two thirds and the whole; the shape filled faintly in `colour` and
/// outlined in it, a dot at each corner.
pub fn radar(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    axes: &[(&str, f32)],
    colour: Color32,
    font: &FontId,
) {
    let count = axes.len();
    if count < 3 {
        return;
    }
    #[allow(clippy::cast_precision_loss)] // A few axes.
    let angle_of = |index: usize| index as f32 * 360.0 / count as f32;
    for ring in [1.0_f32 / 3.0, 2.0 / 3.0, 1.0] {
        let mut outline: Vec<Pos2> = (0..count)
            .map(|index| canvas::polar(centre, radius * ring, angle_of(index)))
            .collect();
        outline.push(outline[0]);
        let stroke = if ring >= 1.0 {
            theme::AXIS
        } else {
            theme::GRID
        };
        painter.add(Shape::line(outline, Stroke::new(1.0, stroke)));
    }
    for index in 0..count {
        canvas::line(
            painter,
            centre,
            canvas::polar(centre, radius, angle_of(index)),
            1.0,
            theme::GRID,
        );
    }
    let corners: Vec<Pos2> = axes
        .iter()
        .enumerate()
        .map(|(index, (_, value))| {
            let value = if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            };
            canvas::polar(centre, radius * value.max(0.02), angle_of(index))
        })
        .collect();
    canvas::star(
        painter,
        &corners,
        colour.gamma_multiply(0.35),
        Stroke::new(1.5, colour),
    );
    for corner in &corners {
        painter.circle_filled(*corner, 2.5, colour);
    }
    for (index, (name, _)) in axes.iter().enumerate() {
        let angle = angle_of(index);
        let at = canvas::polar(centre, radius + 10.0, angle);
        let align = match angle {
            a if a < 10.0 || a > 350.0 => Align2::CENTER_BOTTOM,
            a if (170.0..=190.0).contains(&a) => Align2::CENTER_TOP,
            a if a < 180.0 => Align2::LEFT_CENTER,
            _ => Align2::RIGHT_CENTER,
        };
        canvas::text(painter, at, align, name, font.clone(), theme::TEXT_3);
    }
}

/// The colour `t` (0 to 1) of the way along [`theme::HEAT`]'s ramp, from
/// the card's surface to its lightest blue.
#[must_use]
pub fn heat(t: f32) -> Color32 {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)] // Seven stops.
    let place = t * (theme::HEAT.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Within the stops.
    let index = (place.floor() as usize).min(theme::HEAT.len() - 2);
    #[allow(clippy::cast_precision_loss)] // Seven stops.
    let within = place - index as f32;
    lerp_colour(theme::HEAT[index], theme::HEAT[index + 1], within)
}

/// The colour `t` of the way from `a` to `b`.
#[must_use]
pub fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 255.
        let value = (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
            .round()
            .clamp(0.0, 255.0) as u8;
        value
    };
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Where `value` falls on a log scale from `low` to `high`, 0 to 1 (held
/// at the ends).
#[must_use]
pub fn log_scale(value: f32, low: f32, high: f32) -> f32 {
    if !(value > 0.0 && low > 0.0 && high > low) {
        return 0.0;
    }
    ((value / low).ln() / (high / low).ln()).clamp(0.0, 1.0)
}

/// Where a distance `d` lands on a disc of `radius` whose scale is a log of
/// one plus the distance over `near`, the rim at `far`: close things keep
/// room and the far ring still fits.
#[must_use]
pub fn log_radius(d: f32, near: f32, far: f32, radius: f32) -> f32 {
    if !(d > 0.0 && near > 0.0 && far > 0.0) {
        return 0.0;
    }
    radius * (1.0 + d / near).ln() / (1.0 + far / near).ln()
}

/// Each column's largest value of `samples` over `columns` equal spans of
/// the `window_s` seconds before `now`, oldest first; `None` where no
/// sample falls.
#[must_use]
pub fn column_peaks(
    samples: &[[f64; 2]],
    now: f64,
    window_s: f64,
    columns: usize,
) -> Vec<Option<f32>> {
    let mut peaks = vec![None; columns];
    if columns == 0 || window_s <= 0.0 {
        return peaks;
    }
    let start = now - window_s;
    #[allow(clippy::cast_precision_loss)] // A few hundred columns.
    let per_column = window_s / columns as f64;
    for &[t, value] in samples {
        if t < start || t > now || !value.is_finite() {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let column = (((t - start) / per_column) as usize).min(columns - 1);
        #[allow(clippy::cast_possible_truncation)] // Milliseconds and the like.
        let value = value as f32;
        let peak: &mut Option<f32> = &mut peaks[column];
        *peak = Some(peak.map_or(value, |peak| peak.max(value)));
    }
    peaks
}

/// `peaks` with each gap between two samples filled by the sample after
/// it: a sample holds back to the one before it, as a frame's time lasts
/// from the frame before it ended. Columns before the first sample and
/// after the last stay empty.
#[must_use]
pub fn held(mut peaks: Vec<Option<f32>>) -> Vec<Option<f32>> {
    let Some(first) = peaks.iter().position(Option::is_some) else {
        return peaks;
    };
    let mut later = None;
    for peak in peaks[first..].iter_mut().rev() {
        match peak {
            Some(value) => later = Some(*value),
            None => *peak = later,
        }
    }
    peaks
}

/// Columns standing on `plot`'s floor, one per value across its width:
/// `layers` stacked in the order given, each value's height against `top`
/// (cut at the top edge). A column with no value in a layer adds nothing
/// there.
pub fn stacked_columns(
    painter: &Painter,
    plot: Rect,
    layers: &[(&[Option<f32>], Color32)],
    top: f32,
) {
    let count = layers
        .iter()
        .map(|(values, _)| values.len())
        .max()
        .unwrap_or(0);
    if count == 0 || top <= 0.0 {
        return;
    }
    #[allow(clippy::cast_precision_loss)] // A few hundred columns.
    let width = plot.width() / count as f32;
    let mut base = vec![0.0_f32; count];
    let mut mesh = Mesh::default();
    for (values, colour) in layers {
        for (column, value) in values.iter().enumerate() {
            let Some(value) = value.filter(|value| value.is_finite() && *value > 0.0) else {
                continue;
            };
            let low = base[column];
            let high = low + value;
            base[column] = high;
            let y0 = plot.bottom() - (low / top).min(1.0) * plot.height();
            let y1 = plot.bottom() - (high / top).min(1.0) * plot.height();
            if y0 - y1 < 0.05 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)] // A few hundred columns.
            let x = plot.left() + column as f32 * width;
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(x, y1), pos2(x + width + 0.05, y0)),
                *colour,
            );
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A line through `values`, one per column across `plot`, against `top`;
/// it breaks where a value is missing.
pub fn line_through(
    painter: &Painter,
    plot: Rect,
    values: &[Option<f32>],
    top: f32,
    width: f32,
    colour: Color32,
) {
    if values.is_empty() || top <= 0.0 {
        return;
    }
    #[allow(clippy::cast_precision_loss)] // A few hundred columns.
    let step = plot.width() / values.len() as f32;
    let mut run: Vec<Pos2> = Vec::new();
    for (column, value) in values.iter().enumerate() {
        match value.filter(|value| value.is_finite()) {
            Some(value) => {
                #[allow(clippy::cast_precision_loss)] // A few hundred columns.
                let x = plot.left() + (column as f32 + 0.5) * step;
                let y = plot.bottom() - (value / top).clamp(0.0, 1.0) * plot.height();
                run.push(pos2(x, y));
            }
            None => canvas::polyline(painter, std::mem::take(&mut run), width, colour),
        }
    }
    canvas::polyline(painter, run, width, colour);
}

/// How many of `values` fall in each of `bins` equal spans from 0 to
/// `top`, lowest first; values above `top` count in the last.
#[must_use]
pub fn histogram(values: &[f32], top: f32, bins: usize) -> Vec<u32> {
    let mut counts = vec![0_u32; bins];
    if bins == 0 || top <= 0.0 {
        return counts;
    }
    for value in values.iter().filter(|value| value.is_finite()) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let bin = ((value.max(0.0) / top * bins as f32) as usize).min(bins - 1);
        counts[bin] += 1;
    }
    counts
}

/// How `values` spread from 0 (`plot`'s floor) to `top`: a bar from the
/// left per bin of `bin` points' height, as long as its count against the
/// largest. Returns the values above `top`.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the count.
pub fn spread(painter: &Painter, plot: Rect, values: &[f32], top: f32, bin: f32) -> usize {
    if top <= 0.0 || plot.height() < bin {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Screen rows.
    let bins = (plot.height() / bin).floor().max(1.0) as usize;
    let within: Vec<f32> = values
        .iter()
        .copied()
        .filter(|value| *value <= top)
        .collect();
    let counts = histogram(&within, top, bins);
    let largest = counts.iter().copied().max().unwrap_or(0);
    if largest > 0 {
        let mut mesh = Mesh::default();
        for (index, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)] // Counts and rows.
            let (length, bottom) = (
                plot.width() * count as f32 / largest as f32,
                plot.bottom() - index as f32 * bin,
            );
            mesh.add_colored_rect(
                Rect::from_min_max(
                    pos2(plot.left(), bottom - bin + 0.5),
                    pos2(plot.left() + length.max(1.0), bottom),
                ),
                theme::TEXT_3,
            );
        }
        painter.add(Shape::mesh(mesh));
    }
    values.len() - within.len()
}

/// Where a body at `azimuth_deg` and `elevation_deg` stands on a sky dome
/// of `radius` round `centre`: the zenith in the middle, the horizon at
/// the rim (below it, past the rim), north up and east to the right.
#[must_use]
pub fn sky_point(centre: Pos2, radius: f32, azimuth_deg: f32, elevation_deg: f32) -> Pos2 {
    let r = radius * (90.0 - elevation_deg.clamp(-90.0, 90.0)) / 90.0;
    let azimuth = azimuth_deg.to_radians();
    centre + r * vec2(azimuth.sin(), -azimuth.cos())
}

/// A grid of cells from `origin`, `columns` across, each `cell` square
/// and `gap` apart, coloured by `colour_of(index)`; returns the grid's
/// rectangle.
pub fn cells(
    painter: &Painter,
    origin: Pos2,
    count: usize,
    columns: usize,
    (cell, gap): (egui::Vec2, f32),
    colour_of: impl Fn(usize) -> Color32,
) -> Rect {
    let columns = columns.max(1);
    let mut rect = Rect::from_min_size(origin, egui::Vec2::ZERO);
    let mut mesh = Mesh::default();
    for index in 0..count {
        #[allow(clippy::cast_precision_loss)] // A few hundred cells.
        let (column, row) = ((index % columns) as f32, (index / columns) as f32);
        let at = origin + vec2(column * (cell.x + gap), row * (cell.y + gap));
        let square = Rect::from_min_size(at, cell);
        mesh.add_colored_rect(square, colour_of(index));
        rect = rect.union(square);
    }
    painter.add(Shape::mesh(mesh));
    rect
}

/// A rounded box outlined in `colour`, as a legend draws for an outline
/// mark.
pub fn outline_swatch(painter: &Painter, rect: Rect, colour: Color32) {
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.5, colour),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(value: f32) -> Part {
        Part::new("a", value, Color32::RED, format!("{value}"))
    }

    #[test]
    fn a_scale_tops_out_at_one_two_or_five() {
        for (value, top) in [
            (0.0, 1.0),
            (16.7, 20.0),
            (20.0, 20.0),
            (31.2, 50.0),
            (51.0, 100.0),
            (0.37, 0.5),
        ] {
            assert!(
                (nice_top(value) - top).abs() < 1e-5,
                "{value}: {}",
                nice_top(value)
            );
        }
    }

    #[test]
    fn a_bars_parts_fill_their_share_with_a_gap_between() {
        let spans = bar_spans(&[part(1.0), part(0.0), part(3.0)], 8.0, 0.0, 80.0);
        assert_eq!(spans[0], Some((0.0, 10.0 - PART_GAP)));
        assert_eq!(spans[1], None);
        // The last part keeps its whole span; the rest of the total is track.
        assert_eq!(spans[2], Some((10.0, 40.0)));
        // Parts larger than the total fill the bar.
        let spans = bar_spans(&[part(2.0), part(2.0)], 1.0, 0.0, 40.0);
        assert_eq!(spans[1], Some((20.0, 40.0)));
        assert!(bar_spans(&[part(0.0)], 0.0, 0.0, 40.0)[0].is_none());
    }

    #[test]
    fn columns_keep_each_spans_peak() {
        let samples = [[0.5, 3.0], [1.2, 5.0], [1.8, 4.0], [3.9, 1.0], [-1.0, 9.0]];
        let peaks = column_peaks(&samples, 4.0, 4.0, 4);
        assert_eq!(peaks, [Some(3.0), Some(5.0), None, Some(1.0)]);
        assert_eq!(column_peaks(&samples, 4.0, 0.0, 4), [None; 4]);
    }

    #[test]
    fn a_sample_holds_back_to_the_one_before_it() {
        // A slow frame's time fills the columns it lasted, back to the
        // frame before; nothing before the first sample or after the last.
        assert_eq!(
            held(vec![None, Some(3.0), None, None, Some(9.0), None]),
            [None, Some(3.0), Some(9.0), Some(9.0), Some(9.0), None]
        );
        assert_eq!(held(vec![None, None]), [None, None]);
    }

    #[test]
    fn a_histogram_counts_each_span_and_the_overflow_in_the_last() {
        assert_eq!(
            histogram(&[1.0, 2.5, 2.9, 7.0, 40.0], 10.0, 5),
            [1, 2, 0, 1, 1]
        );
        assert_eq!(histogram(&[1.0], 0.0, 3), [0, 0, 0]);
    }

    #[test]
    fn the_sky_puts_the_zenith_in_the_middle_and_east_to_the_right() {
        let centre = pos2(100.0, 100.0);
        assert_eq!(sky_point(centre, 90.0, 0.0, 90.0), centre);
        let east = sky_point(centre, 90.0, 90.0, 0.0);
        assert!((east.x - 190.0).abs() < 1e-3 && (east.y - 100.0).abs() < 1e-3);
        let north_high = sky_point(centre, 90.0, 0.0, 60.0);
        assert!((north_high.y - 70.0).abs() < 1e-3);
        // Below the horizon is past the rim.
        let twilight = sky_point(centre, 90.0, 180.0, -18.0);
        assert!((twilight.y - 208.0).abs() < 1e-3);
    }

    #[test]
    fn the_heat_ramp_runs_from_the_surface_to_the_lightest_blue() {
        assert_eq!(heat(0.0), theme::HEAT[0]);
        assert_eq!(heat(1.0), theme::HEAT[6]);
        assert_eq!(heat(0.5), theme::HEAT[3]);
        assert_eq!(heat(f32::NAN), theme::HEAT[0]);
    }

    #[test]
    fn log_scales_keep_room_near_and_end_at_the_rim() {
        assert!((log_scale(10.0, 1.0, 100.0) - 0.5).abs() < 1e-6);
        assert!(log_scale(0.1, 1.0, 100.0).abs() < 1e-6);
        assert!((log_scale(1e6, 1.0, 100.0) - 1.0).abs() < 1e-6);
        assert!((log_radius(4_000.0, 25.0, 4_000.0, 200.0) - 200.0).abs() < 1e-3);
        assert!(log_radius(0.0, 25.0, 4_000.0, 200.0).abs() < 1e-6);
        // A 40 m reach takes far more than its 1 % share of a 4 km rim.
        assert!(log_radius(40.0, 25.0, 4_000.0, 200.0) > 30.0);
    }

    #[test]
    fn a_label_on_a_mark_takes_the_dark_ink_on_colours_and_light_on_grey() {
        for colour in &theme::SERIES[..6] {
            assert_eq!(on_mark(*colour), theme::ON_MARK);
        }
        assert_eq!(on_mark(theme::SERIES[6]), theme::TEXT);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // A few hundred values.
    fn every_figure_paints_inside_its_clip() {
        let ctx = egui::Context::default();
        let parts = [
            Part::new("render", 3.1, theme::SERIES[0], "3.1 ms"),
            Part::new("terrain", 2.4, theme::SERIES[1], "2.4 ms"),
        ];
        let values: Vec<Option<f32>> = (0..200_u16).map(|i| Some(8.0 + f32::from(i % 7))).collect();
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
            let clip = Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0, 200.0));
            let painter = ui.painter().with_clip_rect(clip);
            let font = FontId::proportional(11.0);
            let bar = Rect::from_min_size(clip.min, vec2(280.0, 18.0));
            let spans = stacked_bar(
                &painter,
                bar,
                &parts,
                16.7,
                Some(theme::SURFACE_3),
                Some(&font),
            );
            hover_parts(ui, bar, Id::new("bar"), &spans, &parts);
            share_bar(
                &painter,
                bar.translate(vec2(0.0, 24.0)),
                0.4,
                theme::SERIES[2],
            );
            donut(&painter, clip.center(), (40.0, 54.0), &parts);
            donut(&painter, clip.center(), (40.0, 54.0), &[]);
            radar(
                &painter,
                clip.center(),
                60.0,
                &[
                    ("moisture", 0.6),
                    ("heat", 0.4),
                    ("rise", 0.2),
                    ("light", f32::NAN),
                ],
                theme::SERIES[2],
                &font,
            );
            stacked_columns(
                &painter,
                clip,
                &[(&values, theme::ACCENT), (&values, theme::SERIES[6])],
                40.0,
            );
            line_through(&painter, clip, &values, 40.0, 1.5, theme::TEXT);
            spread(&painter, clip, &[1.0, 2.0, 50.0], 20.0, 2.0);
            cells(&painter, clip.min, 60, 20, (vec2(8.0, 8.0), 2.0), |index| {
                heat(index as f32 / 60.0)
            });
            outline_swatch(
                &painter,
                Rect::from_min_size(clip.min, vec2(9.0, 9.0)),
                theme::ACCENT,
            );
        });
    }
}
