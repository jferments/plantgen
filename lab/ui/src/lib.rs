//! PlantLab's interface kit: the theme and its fonts, the controls, boards
//! of instrument cards, painting at exact places and the figures cards
//! draw. It draws and nothing else: each function takes an
//! [`egui::Context`] or an [`egui::Ui`] and says what was clicked or the
//! rectangle it used; the window decides what a click means.
//!
//! The design is WorldLab's redesigned interface (Joshi, 2026-10-11),
//! copied from Project After's `lab-ui` (`crates/lab-ui/src/`, MIT, the
//! same owner) at `jferments/after` `be2518b`. PlantGen never depends on
//! After, so this is a copy that PlantLab owns and never syncs.
//!
//! - [`theme`]: colours, chart colours, fonts, sizes and [`theme::apply`];
//! - [`icons`]: Phosphor's regular icons, each a `&str`, drawn alone in
//!   [`theme::icon_font`];
//! - [`widgets`]: icon, rail and dock buttons, the search field, segmented
//!   choices, switches, chips, badges, cards, stats, progress bars, key
//!   caps, popovers, knobs, section headings, form rows and frames;
//! - [`board`]: a page of cards on a grid of twelve columns, each card's
//!   rectangle worked out from the width alone, at three widths;
//! - [`canvas`]: text cut to fit, labels that give way, lines, areas, arcs
//!   and legends;
//! - [`figures`]: stacked and share bars, donuts, log scales, columns of
//!   recent values, histograms and spreads;
//! - [`palette`]: the command palette, a search field over entries that
//!   narrows as you type, best match first.
pub use egui;

/// Phosphor's regular icons (MIT), from `egui-phosphor`: each is a
/// one-glyph `&str`. Inter has glyphs of its own at some of their code
/// points, so an icon drawn alone takes [`theme::icon_font`], and an icon
/// inside a line of text must be one Inter lacks (see [`theme::icons`]).
pub use egui_phosphor::regular as icons;

pub mod board;
pub mod canvas;
pub mod figures;
pub mod palette;
pub mod theme;
pub mod widgets;

/// Runs one egui pass for a test and drops its output.
#[cfg(test)]
pub(crate) fn test_pass(
    ctx: &egui::Context,
    input: egui::RawInput,
    add: impl FnMut(&mut egui::Ui),
) {
    ctx.run_ui(input, add).drop_without_applying_deltas();
}
