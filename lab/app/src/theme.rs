//! The look: a dark theme in deep blue-greys with one accent colour, and
//! text sizes that fit narrow docked panels, on egui's default fonts
//! (Ubuntu Light, Hack for monospace, and egui's emoji and icon fonts).
//!
//! Copied from Project After's `lab-ui` (`crates/lab-ui/src/theme.rs` at
//! `jferments/after` `597354d`, MIT), so PlantLab looks like WorldLab and
//! AssetLab. PlantLab owns this copy and never syncs it (Joshi,
//! 2026-10-08: no shared UI crate).
//!
//! Call [`apply`] once, when the context exists. The colours are public so
//! an application's own painting matches.
use bevy_egui::egui::{self, Color32, FontId, TextStyle, Visuals};

/// Panel and dock backgrounds.
pub const PANEL: Color32 = Color32::from_rgb(10, 15, 20);
/// Bars and windows, a shade lighter than the panels.
pub const BAR: Color32 = Color32::from_rgb(16, 24, 31);
/// Behind text fields, plots and consoles.
pub const DEEP: Color32 = Color32::from_rgb(6, 9, 12);
/// Buttons at rest.
pub const BUTTON: Color32 = Color32::from_rgb(31, 56, 71);
/// Errors: a refused command, a traceback.
pub const ERROR: Color32 = Color32::from_rgb(232, 112, 104);
/// Secondary text.
pub const MUTED: Color32 = Color32::from_gray(150);

/// Text sizes, points.
pub const SMALL: f32 = 11.0;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 15.0;
pub const MONOSPACE: f32 = 12.5;

/// egui's dark visuals in the theme's colours.
#[must_use]
pub fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = BAR;
    visuals.extreme_bg_color = DEEP;
    visuals.faint_bg_color = Color32::from_rgb(18, 26, 33);
    visuals.widgets.inactive.weak_bg_fill = BUTTON;
    visuals.widgets.inactive.bg_fill = BUTTON;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 80, 100);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(56, 104, 130);
    visuals.selection.bg_fill = Color32::from_rgb(40, 96, 124);
    visuals
}

/// Sets the theme on both of egui's styles (dark and light), so the look
/// does not follow the desktop's light or dark preference.
pub fn apply(ctx: &egui::Context) {
    let visuals = visuals();
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(SMALL)),
            (TextStyle::Body, FontId::proportional(BODY)),
            (TextStyle::Button, FontId::proportional(BODY)),
            (TextStyle::Heading, FontId::proportional(HEADING)),
            (TextStyle::Monospace, FontId::monospace(MONOSPACE)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sets_the_colours_and_sizes_in_both_styles() {
        let ctx = egui::Context::default();
        apply(&ctx);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let style = ctx.style_of(theme);
            assert_eq!(style.visuals.panel_fill, PANEL);
            let size = |text_style| style.text_styles[&text_style].size;
            assert!((size(TextStyle::Body) - BODY).abs() < f32::EPSILON);
            assert!((size(TextStyle::Monospace) - MONOSPACE).abs() < f32::EPSILON);
        }
        assert_eq!(ctx.theme(), egui::Theme::Dark);
    }
}
