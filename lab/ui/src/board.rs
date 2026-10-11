//! A board of cards: a page of instrument cards placed on a grid of
//! twelve columns, as a dashboard's are. A page names where each card goes
//! at three widths ([`Arrangements`]: wide, medium and narrow), the board
//! picks the arrangement the width there is takes ([`Width::of`]) and
//! works out each card's rectangle ([`rects`]).
//!
//! Every card's rectangle comes from the width alone, never from what the
//! cards hold, and the board takes exactly that width: what a card draws is
//! clipped to its rectangle ([`card`]) and nothing it holds can widen the
//! page. A page laid out from what its cards held fed back on itself in a
//! dock, whose width follows what it holds: a card a little too wide
//! widened the dock, which gave room for another column, which widened it
//! again, and the cards overlapped, ran off the window's edge and shook as
//! the count of columns flipped from frame to frame.
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2,
    vec2,
};

use crate::{canvas, theme};

/// The grid's columns.
pub const COLUMNS: u8 = 12;
/// The space between cards, points.
pub const GAP: f32 = 12.0;
/// Inside a card: left and right, top, bottom, points.
pub const PADDING: Vec2 = vec2(16.0, 12.0);
/// A card's heading's height, and the space under it, points.
pub const HEAD_HEIGHT: f32 = 24.0;
pub const HEAD_GAP: f32 = 8.0;
/// The size of a card's title and the line beside it, points.
pub const TITLE_SIZE: f32 = 11.5;
/// The space between letters of a card's title, points.
pub const TITLE_SPACING: f32 = 0.8;

/// Where one card goes: its first column and the column after its last
/// (`0..12` is the whole width), and its first row and the row after its
/// last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub columns: (u8, u8),
    pub rows: (u8, u8),
}

impl Place {
    /// A card from column `first` to before `end`, in row `row` alone.
    #[must_use]
    pub const fn at(first: u8, end: u8, row: u8) -> Self {
        Self {
            columns: (first, end),
            rows: (row, row + 1),
        }
    }

    /// A card from row `first` to before `end`.
    #[must_use]
    pub const fn rows(self, first: u8, end: u8) -> Self {
        Self {
            columns: self.columns,
            rows: (first, end),
        }
    }
}

/// One arrangement of a page's cards: each row's height in points, and
/// where each card goes, in the order the page draws them.
#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub rows: &'static [f32],
    pub places: &'static [Place],
}

impl Arrangement {
    /// The board's height: the rows and the gaps between them.
    #[must_use]
    pub fn height(&self) -> f32 {
        #[allow(clippy::cast_precision_loss)] // A few rows.
        let gaps = self.rows.len().saturating_sub(1) as f32 * GAP;
        self.rows.iter().sum::<f32>() + gaps
    }
}

/// A page's arrangements at the three widths.
#[derive(Debug, Clone, Copy)]
pub struct Arrangements {
    pub wide: Arrangement,
    pub medium: Arrangement,
    pub narrow: Arrangement,
}

impl Arrangements {
    /// The arrangement for `width`.
    #[must_use]
    pub const fn at(&self, width: Width) -> &Arrangement {
        match width {
            Width::Wide => &self.wide,
            Width::Medium => &self.medium,
            Width::Narrow => &self.narrow,
        }
    }
}

/// The three widths a page is arranged for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    /// Three cards a row, as the page's design draws it.
    Wide,
    /// Two cards a row, the wide figures a row of their own.
    Medium,
    /// One card a row.
    #[default]
    Narrow,
}

/// A board at least this wide is [`Width::Wide`], points.
pub const WIDE_FROM: f32 = 1_200.0;
/// A board at least this wide is [`Width::Medium`], points.
pub const MEDIUM_FROM: f32 = 740.0;
/// How far past a step the width must go before the arrangement changes
/// back, points: a width resting on a step never flips the arrangement.
pub const HYSTERESIS: f32 = 16.0;

impl Width {
    /// The width class of a board `width` points wide, given the one it
    /// had: it moves up a class only once the width is past the step and
    /// back down only once it is [`HYSTERESIS`] under it.
    #[must_use]
    pub fn of(width: f32, before: Self) -> Self {
        let class = |wide: f32, medium: f32| {
            if width >= wide {
                Self::Wide
            } else if width >= medium {
                Self::Medium
            } else {
                Self::Narrow
            }
        };
        let up = class(WIDE_FROM, MEDIUM_FROM);
        let down = class(WIDE_FROM - HYSTERESIS, MEDIUM_FROM - HYSTERESIS);
        match before {
            // Keep the class the width still holds within the margin.
            Self::Wide if down == Self::Wide => Self::Wide,
            Self::Medium if down != Self::Narrow && up != Self::Wide => Self::Medium,
            _ => up,
        }
    }
}

/// Each card's rectangle in an arrangement `width` points wide whose top
/// left corner is `origin`.
#[must_use]
pub fn rects(origin: Pos2, width: f32, arrangement: &Arrangement) -> Vec<Rect> {
    rects_stretched(origin, width, arrangement, 1.0)
}

/// Each card's rectangle as [`rects`] gives it, with every row `stretch`
/// times its height.
#[must_use]
pub fn rects_stretched(
    origin: Pos2,
    width: f32,
    arrangement: &Arrangement,
    stretch: f32,
) -> Vec<Rect> {
    let column = (width - GAP * f32::from(COLUMNS - 1)) / f32::from(COLUMNS);
    let x = |line: u8| origin.x + f32::from(line) * (column + GAP);
    let mut tops = Vec::with_capacity(arrangement.rows.len() + 1);
    let mut y = origin.y;
    for height in arrangement.rows {
        tops.push(y);
        y += height * stretch + GAP;
    }
    tops.push(y);
    arrangement
        .places
        .iter()
        .map(|place| {
            let (first, end) = place.columns;
            let (top, bottom) = place.rows;
            let row = |line: u8| tops[usize::from(line).min(tops.len() - 1)];
            Rect::from_min_max(
                pos2(x(first), row(top)),
                pos2(x(end) - GAP, row(bottom) - GAP),
            )
        })
        .collect()
}

/// The most a board's rows stretch to fill the room under them.
pub const MOST_STRETCH: f32 = 1.4;

/// How much `arrangement`'s rows stretch to fill `room` points of height:
/// once at least, [`MOST_STRETCH`] at most, and never so much that the
/// board passes the room it fits in. `room` is the page's viewport, which
/// scrolling does not change, so the board never feeds back on itself.
#[must_use]
pub fn stretch_for(arrangement: &Arrangement, room: f32) -> f32 {
    let rows: f32 = arrangement.rows.iter().sum();
    let gaps = arrangement.height() - rows;
    if rows <= 0.0 || !room.is_finite() {
        return 1.0;
    }
    ((room - gaps) / rows).clamp(1.0, MOST_STRETCH)
}

/// Takes the whole width there is for `arrangement` and returns each card's
/// rectangle; the page then draws each card with [`card`]. With `room`, the
/// height the board has down to the page's foot, its rows stretch to fill
/// it ([`stretch_for`]).
pub fn allocate(ui: &mut Ui, arrangement: &Arrangement, room: Option<f32>) -> Vec<Rect> {
    let width = ui.available_width().max(1.0);
    let stretch = room.map_or(1.0, |room| stretch_for(arrangement, room));
    let rows: f32 = arrangement.rows.iter().sum();
    let height = arrangement.height() + rows * (stretch - 1.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    rects_stretched(rect.min, width, arrangement, stretch)
}

/// A card's heading: its icon, its title (drawn in capitals) and a line
/// saying what it shows.
#[derive(Debug, Clone, Copy)]
pub struct Head<'a> {
    pub icon: &'a str,
    pub title: &'a str,
    pub about: &'a str,
}

/// What a card's [`card`] gives back: the room under the heading for its
/// figures, and the heading's room right of its title and line, for a
/// legend or a button.
#[derive(Debug, Clone, Copy)]
pub struct Room {
    pub body: Rect,
    /// The heading's right end, from the end of the title (the line
    /// beside it gives way to what is drawn here).
    pub head: Rect,
}

/// Draws a card's surface, border and heading in `rect` and returns the
/// room inside it. `right` is how wide the caller will draw at the
/// heading's right end: the line beside the title is cut short of it.
#[must_use]
pub fn card(ui: &Ui, rect: Rect, head: &Head, right: f32) -> Room {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS_WINDOW),
        theme::SURFACE_1,
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(PADDING);
    let head_rect = Rect::from_min_size(inner.min, vec2(inner.width(), HEAD_HEIGHT));
    let middle = head_rect.center().y;
    let pen = canvas::Pen::of(ui);
    let mut x = head_rect.left();
    if !head.icon.is_empty() {
        let icon = canvas::text(
            &painter,
            pos2(x, middle),
            Align2::LEFT_CENTER,
            head.icon,
            pen.icon(15.0),
            theme::TEXT_3,
        );
        x = icon.right() + 7.0;
    }
    let title = canvas::spaced(
        &painter,
        pos2(x, middle),
        &head.title.to_uppercase(),
        pen.strong(TITLE_SIZE),
        theme::TEXT_2,
        TITLE_SPACING,
        head_rect.right() - x,
    );
    let after_title = title.right() + 10.0;
    let right_edge = head_rect.right() - right.max(0.0);
    if right_edge - after_title > 40.0 {
        canvas::text_within(
            &painter,
            pos2(after_title, middle),
            Align2::LEFT_CENTER,
            head.about,
            FontId::proportional(TITLE_SIZE),
            theme::TEXT_3,
            right_edge - after_title - 8.0,
        );
    }
    let body = Rect::from_min_max(pos2(inner.left(), head_rect.bottom() + HEAD_GAP), inner.max);
    Room {
        body,
        head: Rect::from_min_max(
            pos2(after_title.min(head_rect.right()), head_rect.top()),
            head_rect.max,
        ),
    }
}

/// A painter for a card's body: drawing outside `rect` is cut off.
#[must_use]
pub fn painter_in(ui: &Ui, rect: Rect) -> egui::Painter {
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()))
}

/// A small tag in a card's heading saying the figure reads something new,
/// drawn at the right end of `head`; returns its width.
#[allow(clippy::must_use_candidate)] // Callers that only draw leave the width.
pub fn new_tag(ui: &Ui, head: Rect, text: &str) -> f32 {
    let painter = painter_in(ui, head);
    let pen = canvas::Pen::of(ui);
    let galley = painter.layout_no_wrap(text.to_owned(), pen.strong(10.0), theme::NEW);
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = Rect::from_min_size(
        pos2(head.right() - size.x, head.center().y - size.y / 2.0),
        size,
    );
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_SMALL),
        theme::NEW.gamma_multiply(0.14),
    );
    painter.galley(rect.min + vec2(5.0, 2.0), galley, theme::NEW);
    size.x
}

/// The colour a status dot takes: good or busy.
#[must_use]
pub const fn dot(good: bool) -> Color32 {
    if good {
        theme::STATUS[0]
    } else {
        theme::STATUS[1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREE: Arrangement = Arrangement {
        rows: &[100.0, 50.0],
        places: &[
            Place::at(0, 8, 0),
            Place::at(8, 12, 0).rows(0, 2),
            Place::at(0, 4, 1),
            Place::at(4, 8, 1),
        ],
    };

    #[test]
    fn cards_tile_the_width_with_a_gap_between() {
        let rects = rects(Pos2::ZERO, 1_200.0, &THREE);
        // The first card spans eight of twelve columns.
        let column = (1_200.0 - GAP * 11.0) / 12.0;
        assert!((rects[0].width() - (8.0 * column + 7.0 * GAP)).abs() < 1e-3);
        // Neighbours sit a gap apart and the last ends at the edge.
        assert!((rects[1].left() - rects[0].right() - GAP).abs() < 1e-3);
        assert!((rects[1].right() - 1_200.0).abs() < 1e-3);
        // A card two rows tall spans both and the gap between.
        assert!((rects[1].height() - (150.0 + GAP)).abs() < 1e-3);
        assert!((rects[2].top() - (100.0 + GAP)).abs() < 1e-3);
        assert!((THREE.height() - 162.0).abs() < 1e-3);
        // No two overlap.
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn rows_stretch_to_fill_the_room_and_no_further() {
        // 150 points of rows and one gap.
        assert!((stretch_for(&THREE, 100.0) - 1.0).abs() < 1e-6);
        let stretch = stretch_for(&THREE, 192.0);
        assert!((stretch - 1.2).abs() < 1e-4, "{stretch}");
        let rects = rects_stretched(Pos2::ZERO, 1_200.0, &THREE, stretch);
        assert!((rects[2].bottom() - 192.0).abs() < 1e-3);
        assert!((stretch_for(&THREE, 10_000.0) - MOST_STRETCH).abs() < 1e-6);
        assert!((stretch_for(&THREE, f32::INFINITY) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_width_class_holds_on_a_step_until_it_is_well_past() {
        assert_eq!(Width::of(1_300.0, Width::Narrow), Width::Wide);
        assert_eq!(Width::of(800.0, Width::Narrow), Width::Medium);
        assert_eq!(Width::of(500.0, Width::Wide), Width::Narrow);
        // Just under a step keeps the class it had ...
        assert_eq!(Width::of(WIDE_FROM - 4.0, Width::Wide), Width::Wide);
        assert_eq!(Width::of(MEDIUM_FROM - 4.0, Width::Medium), Width::Medium);
        // ... and goes down once past the margin.
        assert_eq!(
            Width::of(WIDE_FROM - HYSTERESIS - 1.0, Width::Wide),
            Width::Medium
        );
        assert_eq!(
            Width::of(MEDIUM_FROM - HYSTERESIS - 1.0, Width::Medium),
            Width::Narrow
        );
        // Going up needs the step itself.
        assert_eq!(Width::of(WIDE_FROM - 1.0, Width::Medium), Width::Medium);
        // Every width settles: the class of a width given its own class is
        // itself.
        for width in (200..2_000).step_by(7) {
            #[allow(clippy::cast_precision_loss)] // Small widths.
            let width = width as f32;
            let class = Width::of(width, Width::Narrow);
            assert_eq!(Width::of(width, class), class, "{width}");
        }
    }
}
