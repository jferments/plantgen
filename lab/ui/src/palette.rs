//! A command palette: a search field over a list of entries that narrows as
//! you type, best match first. Up and Down move the highlight, Enter or a
//! click picks, Tab fills the field with the highlighted entry, Escape or a
//! click outside closes.
//!
//! The match ([`score`]) is a subsequence match: every character of the
//! query, spaces aside, must appear in the entry in order, ignoring case.
//! Characters that follow each other, characters that start a word and an
//! early first character score higher; gaps cost a little. [`rank`] sorts
//! by score, then the shorter entry, then the order the entries came in.
//!
//! The crate draws; what the entries are and what picking one does are the
//! application's. [`Palette`] keeps the search between passes; [`Palette::show`]
//! says what happened ([`Action`]). See Project After's `docs/developer/LAB-UI.md`, section
//! "Command palette".
use std::hash::{DefaultHasher, Hash, Hasher};

use egui::{
    Align2, AtomExt as _, Button, Context, Id, Key, Modifiers, Rect, RichText, ScrollArea,
    TextEdit, TextStyle, pos2, vec2,
};

use crate::theme;

/// The window's width and the list's tallest height, points.
const WIDTH: f32 = 560.0;
const LIST_HEIGHT: f32 = 320.0;
/// In a view too small for [`WIDTH`], the room left at each side of the
/// view and the narrowest the window gets, points.
const SIDE_GAP: f32 = 16.0;
const MIN_WIDTH: f32 = 200.0;
/// The most of a row an entry's label takes; a longer label is cut there.
/// The detail fills what is left and is cut to fit.
const LABEL_SHARE: f32 = 0.7;
/// The gap between the top of the view and the window, points.
const TOP_GAP: f32 = 24.0;
/// Most entries the list draws; the rest are reached by typing more.
pub const MAX_ROWS: usize = 200;
/// Characters of the query, and of an entry, the match reads.
const MAX_QUERY_CHARS: usize = 64;
const MAX_TEXT_CHARS: usize = 160;

/// Score terms: each matched character, one right after the one before, one
/// that starts a word, each character skipped between two matched ones (up
/// to [`MAX_GAP`] of them), and each character before the first match (up
/// to [`MAX_LEAD`]).
pub const MATCH: i32 = 16;
pub const CONSECUTIVE: i32 = 24;
pub const WORD_START: i32 = 20;
pub const GAP: i32 = 2;
pub const MAX_GAP: i32 = 8;
pub const MAX_LEAD: i32 = 16;

/// How well `query` matches `text`, or `None` when it does not: every
/// character of the query but spaces must appear in `text` in order,
/// ignoring case. An empty query matches everything with 0.
///
/// Each place the first character occurs starts a candidate; the rest of
/// the query takes the first occurrence of each character after the one
/// before. The best candidate counts.
#[must_use]
pub fn score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .take(MAX_QUERY_CHARS)
        .collect();
    let Some(&first) = query.first() else {
        return Some(0);
    };
    let text: Vec<char> = text
        .chars()
        .flat_map(char::to_lowercase)
        .take(MAX_TEXT_CHARS)
        .collect();
    let starts_word = |at: usize| at == 0 || !text[at - 1].is_alphanumeric();
    let bonus = |at: usize| MATCH + if starts_word(at) { WORD_START } else { 0 };
    let capped = |count: usize, cap: i32| i32::try_from(count).map_or(cap, |count| count.min(cap));
    let mut best = None;
    for start in (0..text.len()).filter(|&at| text[at] == first) {
        let mut total = bonus(start) - capped(start, MAX_LEAD);
        let mut last = start;
        let mut complete = true;
        for &wanted in &query[1..] {
            let Some(offset) = text[last + 1..].iter().position(|&c| c == wanted) else {
                complete = false;
                break;
            };
            let at = last + 1 + offset;
            total += bonus(at);
            total += if offset == 0 {
                CONSECUTIVE
            } else {
                -GAP * capped(offset, MAX_GAP)
            };
            last = at;
        }
        if !complete {
            // A later start sees less of the text, so it cannot match either.
            break;
        }
        best = best.max(Some(total));
    }
    best
}

/// The indices of the `labels` that `query` matches, best first: by
/// [`score`], then the shorter label, then the order they came in. An empty
/// query keeps every label in its order.
#[must_use]
pub fn rank<'a>(query: &str, labels: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut scored: Vec<(i32, usize, usize)> = labels
        .into_iter()
        .enumerate()
        .filter_map(|(index, label)| {
            score(query, label).map(|score| (score, label.chars().count(), index))
        })
        .collect();
    if query.trim().is_empty() {
        return scored.into_iter().map(|(_, _, index)| index).collect();
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored.into_iter().map(|(_, _, index)| index).collect()
}

/// One entry of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the search reads and the list shows first, such as
    /// `camera toggle`.
    pub label: String,
    /// What it does, after the label in the muted colour.
    pub detail: String,
    /// The key chord bound to it, at the right.
    pub chord: Option<String>,
}

/// What happened in a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Enter: the highlighted entry, if any matches. The application runs
    /// it, or the typed text.
    Enter(Option<usize>),
    /// A click on an entry.
    Pick(usize),
    /// Tab: fill the field from the highlighted entry.
    Fill(usize),
    /// Escape, or a click outside the window.
    Close,
}

/// The palette's search between passes.
#[derive(Debug, Default)]
pub struct Palette {
    /// The text typed.
    pub query: String,
    /// The highlighted row of the ranked list.
    selected: usize,
    /// Whether the last pass drew the window, and its rectangle.
    open: bool,
    rect: Option<Rect>,
    /// Move the caret to the end of the field on the next pass.
    caret_to_end: bool,
    /// The ranked list, and the query and entries it was ranked for.
    ranked: Vec<usize>,
    ranked_for: Option<(String, u64)>,
    /// A line under the field, such as why Enter did nothing.
    pub note: String,
}

impl Palette {
    /// Whether the last pass drew the window.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Puts `text` in the field, the caret at its end, the first entry
    /// highlighted.
    pub fn fill(&mut self, text: &str) {
        text.clone_into(&mut self.query);
        self.selected = 0;
        self.caret_to_end = true;
        self.note.clear();
    }

    /// The indices of the entries the query matches, best first, as the
    /// last pass ranked them.
    #[must_use]
    pub fn ranked(&self) -> &[usize] {
        &self.ranked
    }

    /// Ranks `entries` for the query, unless they and the query are what
    /// the list was ranked for.
    fn rerank(&mut self, entries: &[Entry]) {
        let mut hasher = DefaultHasher::new();
        for entry in entries {
            entry.label.hash(&mut hasher);
        }
        let key = (self.query.clone(), hasher.finish());
        if self.ranked_for.as_ref() != Some(&key) {
            self.ranked = rank(
                &self.query,
                entries.iter().map(|entry| entry.label.as_str()),
            );
            self.ranked_for = Some(key);
        }
        self.selected = self.selected.min(self.ranked.len().saturating_sub(1));
    }

    /// Draws the palette at the top of `within` while `open`, the field
    /// focused; says what was picked, filled or closed. Opening clears the
    /// field.
    pub fn show(
        &mut self,
        ctx: &Context,
        open: bool,
        entries: &[Entry],
        within: Rect,
    ) -> Option<Action> {
        if !open {
            self.open = false;
            self.rect = None;
            return None;
        }
        if !self.open {
            self.fill("");
        }
        self.open = true;
        // A press outside the window closes it; the window takes its own.
        let pressed_outside = ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| self.rect.is_some_and(|rect| !rect.contains(at)))
        });
        if pressed_outside {
            return Some(Action::Close);
        }
        let mut action = None;
        // From the view, not from the window: an egui window that sizes
        // itself to its contents offers them only its last size.
        let width = WIDTH.min(within.width() - 2.0 * SIDE_GAP).max(MIN_WIDTH);
        let window = egui::Window::new("Command palette")
            .id(Id::new("lab-ui-palette"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(pos2(within.center().x, within.top() + TOP_GAP))
            .constrain_to(within)
            .show(ctx, |ui| {
                action = self.contents(ui, entries, width);
            });
        self.rect = window.map(|window| window.response.rect);
        action
    }

    /// The field, the note and the list, `width` wide.
    fn contents(&mut self, ui: &mut egui::Ui, entries: &[Entry], width: f32) -> Option<Action> {
        ui.set_width(width);
        // The keys the list needs, before the field can take them.
        let (up, down, enter, tab, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Tab),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let field_id = Id::new("lab-ui-palette-field");
        let field = ui.add(
            TextEdit::singleline(&mut self.query)
                .id(field_id)
                .hint_text("Type a command, a setting, a panel or a bookmark")
                .font(TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
        if !field.has_focus() {
            field.request_focus();
        }
        if self.caret_to_end {
            if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), field_id) {
                let end = egui::text::CCursor::new(self.query.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), field_id);
            }
            self.caret_to_end = false;
        }
        if field.changed() {
            self.selected = 0;
            self.note.clear();
        }
        self.rerank(entries);
        let moved = up || down;
        if down {
            self.selected = (self.selected + 1).min(self.ranked.len().saturating_sub(1));
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if !self.note.is_empty() {
            ui.label(RichText::new(&self.note).small().color(theme::WARNING));
        }
        let mut action = None;
        if self.ranked.is_empty() {
            ui.label(
                RichText::new("Nothing matches; Enter runs what you typed if it is a command")
                    .small()
                    .color(theme::MUTED),
            );
        }
        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let width = ui.available_width();
                for (row, &index) in self.ranked.iter().take(MAX_ROWS).enumerate() {
                    let Some(entry) = entries.get(index) else {
                        continue;
                    };
                    let selected = row == self.selected;
                    // egui lets one atom of a button shrink, by default the
                    // first text. The detail is that atom here, so a long
                    // one is cut instead of widening the window; the label
                    // is cut only past its share of the row.
                    let mut button = Button::selectable(
                        selected,
                        (
                            RichText::new(&entry.label)
                                .monospace()
                                .atom_max_width(LABEL_SHARE * width),
                            RichText::new(format!("  {}", entry.detail))
                                .small()
                                .color(theme::MUTED)
                                .atom_shrink(true),
                        ),
                    )
                    .truncate()
                    .min_size(vec2(width, 0.0));
                    if let Some(chord) = &entry.chord {
                        button = button.shortcut_text(chord.as_str());
                    }
                    let response = ui.add(button);
                    if selected && moved {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        action = Some(Action::Pick(index));
                    }
                }
            });
        let highlighted = self.ranked.get(self.selected).copied();
        if enter {
            action = Some(Action::Enter(highlighted));
        } else if tab {
            action = highlighted.map(Action::Fill).or(action);
        } else if escape {
            action = Some(Action::Close);
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[&str], query: &str) -> Vec<String> {
        rank(query, entries.iter().copied())
            .into_iter()
            .map(|index| entries[index].to_owned())
            .collect()
    }

    #[test]
    fn a_subsequence_matches_in_order_and_ignores_case_and_spaces() {
        assert!(score("ctog", "camera toggle").is_some());
        assert!(score("CAM TOG", "camera toggle").is_some());
        assert!(score("togcam", "camera toggle").is_none());
        assert!(score("cameras", "camera toggle").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("  ", "anything"), Some(0));
        // Exactly: three word starts, consecutive runs, no lead.
        let exact = score("map", "map toggle").unwrap();
        assert_eq!(exact, 3 * MATCH + WORD_START + 2 * CONSECUTIVE);
        // The best start wins: `map` in `camera map` is the word.
        assert_eq!(
            score("map", "camera map").unwrap(),
            3 * MATCH + WORD_START + 2 * CONSECUTIVE - 7
        );
    }

    #[test]
    fn word_starts_and_runs_rank_first_then_shorter_then_given_order() {
        let entries = [
            "view haze toggle",
            "camera toggle",
            "panel help toggle",
            "map toggle",
            "camera ortho",
        ];
        assert_eq!(labels(&entries, "cam tog"), ["camera toggle"]);
        // Equal scores: the shorter first.
        assert_eq!(labels(&entries, "cam"), ["camera ortho", "camera toggle"]);
        // Equal but for the lead: the earliest first.
        assert_eq!(
            labels(&entries, "tog"),
            [
                "map toggle",
                "camera toggle",
                "view haze toggle",
                "panel help toggle"
            ]
        );
        // A run beats word starts with gaps; a shorter gap beats a later
        // start.
        assert_eq!(score("ho", "camera ortho"), Some(46));
        assert_eq!(score("ho", "view haze toggle"), Some(37));
        assert_eq!(score("ho", "panel help toggle"), Some(36));
        assert_eq!(
            labels(&entries, "ho"),
            ["camera ortho", "view haze toggle", "panel help toggle"]
        );
        // Nothing typed keeps the given order.
        assert_eq!(labels(&entries, ""), entries);
        // Ties: the shorter first, then the given order.
        assert_eq!(
            labels(&["ab x", "ab", "ab y"], "ab"),
            ["ab", "ab x", "ab y"]
        );
    }

    fn entries() -> Vec<Entry> {
        ["camera toggle", "camera ortho", "map toggle"]
            .into_iter()
            .map(|label| Entry {
                label: label.to_owned(),
                detail: "a view".to_owned(),
                chord: Some("V".to_owned()),
            })
            .collect()
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    /// A 1280 by 720 screen.
    fn screen() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0))
    }

    /// One pass of `palette` over `entries` with `events`, drawn in
    /// `within` on [`screen`].
    fn pass_within(
        ctx: &Context,
        palette: &mut Palette,
        entries: &[Entry],
        events: Vec<egui::Event>,
        within: Rect,
    ) -> Option<Action> {
        let input = egui::RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        };
        let mut action = None;
        crate::test_pass(ctx, input, |ui| {
            action = palette.show(ui.ctx(), true, entries, within);
        });
        action
    }

    /// One pass of `palette` over `entries` with `events`, on the whole
    /// screen.
    fn pass(
        ctx: &Context,
        palette: &mut Palette,
        entries: &[Entry],
        events: Vec<egui::Event>,
    ) -> Option<Action> {
        pass_within(ctx, palette, entries, events, screen())
    }

    #[test]
    fn keys_move_the_highlight_pick_fill_and_close() {
        let ctx = Context::default();
        crate::theme::apply(&ctx);
        let entries = entries();
        let mut palette = Palette::default();
        assert_eq!(pass(&ctx, &mut palette, &entries, Vec::new()), None);
        assert!(palette.is_open());
        assert_eq!(palette.ranked(), [0, 1, 2]);
        // Typing narrows the list; Down moves the highlight; Enter says
        // which entry it is.
        let typed = vec![egui::Event::Text("cam".into())];
        assert_eq!(pass(&ctx, &mut palette, &entries, typed), None);
        assert_eq!(palette.query, "cam");
        assert_eq!(palette.ranked(), [1, 0]);
        let down_enter = vec![key(Key::ArrowDown), key(Key::Enter)];
        assert_eq!(
            pass(&ctx, &mut palette, &entries, down_enter),
            Some(Action::Enter(Some(0)))
        );
        assert_eq!(
            pass(&ctx, &mut palette, &entries, vec![key(Key::Tab)]),
            Some(Action::Fill(0))
        );
        palette.fill("zzz");
        assert_eq!(
            pass(&ctx, &mut palette, &entries, vec![key(Key::Enter)]),
            Some(Action::Enter(None))
        );
        assert_eq!(
            pass(&ctx, &mut palette, &entries, vec![key(Key::Escape)]),
            Some(Action::Close)
        );
        // Closing and opening again clears the field.
        crate::test_pass(&ctx, egui::RawInput::default(), |ui| {
            assert_eq!(palette.show(ui.ctx(), false, &entries, screen()), None);
        });
        assert!(!palette.is_open());
        pass(&ctx, &mut palette, &entries, Vec::new());
        assert_eq!(palette.query, "");
    }

    /// Entries whose label or detail is wider than the window.
    fn long_entries() -> Vec<Entry> {
        vec![
            Entry {
                label: "camera pose third YAW PITCH ZOOM".to_owned(),
                detail: "degrees and metres of the view ".repeat(12),
                chord: None,
            },
            Entry {
                label: format!("open {}", "after://LAT,LON?t=TIME&cam=VIEW ".repeat(4)),
                detail: "go to a place link's place, time, camera and weather".to_owned(),
                chord: Some("Ctrl+Shift+O".to_owned()),
            },
            Entry {
                label: "map toggle".to_owned(),
                detail: "the map".to_owned(),
                chord: Some("M".to_owned()),
            },
        ]
    }

    /// The palette's window after a few passes over `entries` in `within`,
    /// in a fresh context.
    fn settled(entries: &[Entry], within: Rect) -> Rect {
        let ctx = Context::default();
        crate::theme::apply(&ctx);
        let mut palette = Palette::default();
        for _ in 0..3 {
            pass_within(&ctx, &mut palette, entries, Vec::new(), within);
        }
        palette.rect.expect("the window is drawn")
    }

    #[test]
    fn the_window_keeps_its_width_and_cuts_long_entries() {
        // A detail wider than the window used to widen the window, even
        // past the view: by default egui lets only a button's first text,
        // the label, shrink. Short entries left the window at the first
        // size egui gave it, narrower than the field should be.
        for list in [long_entries(), entries()] {
            let width = settled(&list, screen()).width();
            assert!(
                (WIDTH..=WIDTH + 40.0).contains(&width),
                "the window is {width} points wide"
            );
        }
    }

    #[test]
    fn a_small_view_narrows_the_window() {
        let small = Rect::from_min_size(pos2(100.0, 0.0), vec2(400.0, 720.0));
        let rect = settled(&long_entries(), small);
        assert!(
            small.contains_rect(rect),
            "{rect:?} is not inside {small:?}"
        );
        assert!(rect.width() >= small.width() - 2.0 * SIDE_GAP, "{rect:?}");
    }
}
