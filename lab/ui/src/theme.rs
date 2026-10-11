//! The look: dark grey surfaces in five steps, three text inks, one mint
//! accent, the status colours, an ordered palette for charts, and the type:
//! Inter for text, `JetBrains Mono` for numbers and code, and Phosphor's
//! icons, all built into the crate ([`fonts`]).
//!
//! Call [`apply`] once, when the context exists. The colours are public so
//! an application's own painting matches. The names from before the
//! redesign (`PANEL`, `BAR`, `DEEP`, `BUTTON`, `MUTED`, `ROW`) stay as
//! aliases of the new ones, so code written against them keeps its meaning.
//!
//! Every text ink keeps a contrast of at least 4.5 to 1 on the surfaces
//! text sits on (`inks_keep_their_contrast_on_every_surface`). The chart
//! palette [`SERIES`] was checked for colour-blind separation on
//! [`SURFACE_1`]; its colours are for marks, never for text.
use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle, Visuals, style::HandleShape, vec2,
};

// Surfaces, darkest first.

/// The deepest surface: behind text fields, plots, consoles and the clock.
pub const SURFACE_0: Color32 = Color32::from_rgb(0x0c, 0x0e, 0x12);
/// Bars, docks and panels.
pub const SURFACE_1: Color32 = Color32::from_rgb(0x12, 0x14, 0x19);
/// Windows, menus, popovers and cards.
pub const SURFACE_2: Color32 = Color32::from_rgb(0x18, 0x1b, 0x21);
/// Buttons and form rows at rest.
pub const SURFACE_3: Color32 = Color32::from_rgb(0x1f, 0x23, 0x2b);
/// Hovered buttons, the chosen segment, a switch's track.
pub const SURFACE_4: Color32 = Color32::from_rgb(0x27, 0x2c, 0x36);
/// The surfaces in order, darkest first.
pub const SURFACES: [Color32; 5] = [SURFACE_0, SURFACE_1, SURFACE_2, SURFACE_3, SURFACE_4];

/// Hairlines between areas: 6 % white.
pub const LINE: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);
/// Borders of controls and windows: 10 % white.
pub const LINE_STRONG: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);

// Text inks, strongest first.

/// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xeb, 0xef);
/// Secondary text: names beside values, help, a reading's unit.
pub const TEXT_2: Color32 = Color32::from_rgb(0xa4, 0xab, 0xb7);
/// Faint text: hints, labels on axes, key caps. A step lighter than the
/// mockups' `#6e7581`, which fell under 4.5 to 1 on [`SURFACE_3`].
pub const TEXT_3: Color32 = Color32::from_rgb(0x84, 0x8b, 0x97);

// The accent and the colours that say something.

/// The one accent: what is on, chosen, recording; the caret; a slider's
/// fill.
pub const ACCENT: Color32 = Color32::from_rgb(0x43, 0xd2, 0x9e);
/// Behind something chosen: 13 % of the accent.
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(9, 27, 21, 33);
/// The border of something chosen: 38 % of the accent.
pub const ACCENT_LINE: Color32 = Color32::from_rgba_premultiplied(25, 80, 60, 97);
/// Text on a filled accent, such as a primary button.
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x07, 0x26, 0x1b);
/// A label set on a filled series mark (a bar's part named inside it):
/// dark, so it reads on every series colour but the grey, which takes
/// [`TEXT`] instead.
pub const ON_MARK: Color32 = Color32::from_rgb(0x0d, 0x0f, 0x13);
/// Information and links.
pub const INFO: Color32 = Color32::from_rgb(0x6e, 0xa8, 0xff);
/// Something new.
pub const NEW: Color32 = Color32::from_rgb(0xb1, 0x92, 0xff);
/// Refusals and warnings, as text.
pub const WARNING: Color32 = Color32::from_rgb(0xf1, 0xb7, 0x4c);
/// Errors: a refused command, a traceback, as text.
pub const ERROR: Color32 = Color32::from_rgb(0xf2, 0x6d, 0x6d);

/// The four states a chart's mark or a status dot shows, best first: good,
/// warning, serious, critical. Marks only, each beside an icon or a word,
/// never a series colour; text takes [`WARNING`] and [`ERROR`].
pub const STATUS: [Color32; 4] = [
    Color32::from_rgb(0x0c, 0xa3, 0x0c),
    Color32::from_rgb(0xfa, 0xb2, 0x19),
    Color32::from_rgb(0xec, 0x83, 0x5a),
    Color32::from_rgb(0xd0, 0x3b, 0x3b),
];

/// A chart's series colours in their fixed order. Give each series its
/// slot once and keep it whatever is shown: never cycle the palette or
/// colour by rank. The last, a grey, is for "other"; an eighth series
/// folds into it.
pub const SERIES: [Color32; 7] = [
    Color32::from_rgb(0x39, 0x87, 0xe5),
    Color32::from_rgb(0xd9, 0x59, 0x26),
    Color32::from_rgb(0x19, 0x9e, 0x70),
    Color32::from_rgb(0xc9, 0x85, 0x00),
    Color32::from_rgb(0xd5, 0x51, 0x81),
    Color32::from_rgb(0x90, 0x85, 0xe9),
    Color32::from_rgb(0x4a, 0x50, 0x5b),
];

/// One hue from dark to light, for magnitudes: a heat strip, levels of
/// detail. The first step is the card's own surface.
pub const HEAT: [Color32; 7] = [
    Color32::from_rgb(0x16, 0x19, 0x1f),
    Color32::from_rgb(0x0d, 0x36, 0x6b),
    Color32::from_rgb(0x18, 0x4f, 0x95),
    Color32::from_rgb(0x25, 0x6a, 0xbf),
    Color32::from_rgb(0x39, 0x87, 0xe5),
    Color32::from_rgb(0x6d, 0xa7, 0xec),
    Color32::from_rgb(0x9e, 0xc5, 0xf4),
];

/// A chart's grid lines: 5.5 % white.
pub const GRID: Color32 = Color32::from_rgba_premultiplied(14, 14, 14, 14);
/// A chart's axes and budget lines: 16 % white.
pub const AXIS: Color32 = Color32::from_rgba_premultiplied(41, 41, 41, 41);

// The names from before the redesign.

/// Panel and dock backgrounds ([`SURFACE_1`]).
pub const PANEL: Color32 = SURFACE_1;
/// Bars ([`SURFACE_1`]); windows are [`SURFACE_2`].
pub const BAR: Color32 = SURFACE_1;
/// Behind text fields, plots and consoles ([`SURFACE_0`]).
pub const DEEP: Color32 = SURFACE_0;
/// Buttons at rest ([`SURFACE_3`]).
pub const BUTTON: Color32 = SURFACE_3;
/// Secondary text ([`TEXT_2`]).
pub const MUTED: Color32 = TEXT_2;
/// A raised band a step above a window ([`SURFACE_3`]). Form rows
/// ([`crate::widgets::field`]) now sit on [`SURFACE_2`] inside a hairline.
pub const ROW: Color32 = SURFACE_3;
/// Night and day on a time scale.
pub const NIGHT: Color32 = Color32::from_rgb(18, 28, 52);
pub const DAY: Color32 = Color32::from_rgb(58, 92, 112);

// Shapes and spacing.

/// Corners of key caps and small badges, points.
pub const RADIUS_SMALL: u8 = 4;
/// Corners of buttons, fields and segments.
pub const RADIUS_CONTROL: u8 = 7;
/// Corners of cards.
pub const RADIUS_CARD: u8 = 10;
/// Corners of windows, popovers and floating cards.
pub const RADIUS_WINDOW: u8 = 12;
/// The space between items in a row or a column, points.
pub const GAP: f32 = 8.0;
/// A tight gap: an icon and its label.
pub const GAP_SMALL: f32 = 4.0;
/// Inside a card, points.
pub const CARD_PADDING: i8 = 12;
/// The height of a button, a field or a segment, points.
pub const CONTROL_HEIGHT: f32 = 24.0;

/// Text sizes, points.
pub const SMALL: f32 = 11.5;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 15.0;
pub const MONOSPACE: f32 = 12.0;

// The type.

/// The names of the fonts in [`fonts`], as egui's font definitions key them.
pub const TEXT_FONT: &str = "Inter-Regular";
pub const STRONG_FONT: &str = "Inter-SemiBold";
pub const MONO_FONT: &str = "JetBrainsMono-Regular";
/// Phosphor's regular icons, as `egui-phosphor` names them.
pub const ICON_FONT: &str = "phosphor";
/// The family an icon draws in on its own: Phosphor first. See [`icons`].
pub const ICON_FAMILY: &str = "icons";

const INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

/// The family of strong text: a card's title, a section's name, a form
/// row's name. Draw with [`strong_font`], never with this family alone.
#[must_use]
pub fn strong() -> FontFamily {
    FontFamily::Name(STRONG_FONT.into())
}

/// Strong text at `size` points once the lab's fonts are live, else plain
/// proportional text.
///
/// egui panics on a family no font is bound to, and the fonts [`apply`]
/// sets go live only at the start of the next pass. So the pass that
/// themes the context, and a context never themed (a headless test),
/// draw strong text in the proportional family rather than panic. For the
/// same reason no [`TextStyle`] names the strong family.
#[must_use]
pub fn strong_font(ui: &egui::Ui, size: f32) -> FontId {
    live_or_proportional(ui, strong(), size)
}

/// The family of an icon drawn on its own: Phosphor first, then the text
/// fonts. Inter has glyphs of its own at 315 of Phosphor's code points
/// (`GEAR`, `GEAR_SIX`, `CAMERA`, `CROSSHAIR`, `CHART_LINE_UP` and `INFO`
/// among them), and in the text families, where Inter comes first, those
/// win. So controls draw their icons with [`icon_font`], and an icon put
/// inside a line of text must be one Inter lacks.
#[must_use]
pub fn icons() -> FontFamily {
    FontFamily::Name(ICON_FAMILY.into())
}

/// An icon at `size` points once the lab's fonts are live, else plain
/// proportional text, for the reason [`strong_font`] gives.
#[must_use]
pub fn icon_font(ui: &egui::Ui, size: f32) -> FontId {
    live_or_proportional(ui, icons(), size)
}

/// `family` at `size` if the context's fonts bind it, else proportional.
fn live_or_proportional(ui: &egui::Ui, family: FontFamily, size: f32) -> FontId {
    let live = ui
        .ctx()
        .fonts(|fonts| fonts.definitions().families.contains_key(&family));
    if live {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}

/// egui's fonts with the lab's in front: Inter for proportional text,
/// `JetBrains Mono` for monospace, `Inter SemiBold` as the [`strong`] family,
/// and Phosphor's icons second in each, so an icon (`crate::icons::GEAR`)
/// can sit in any label. egui's own fonts stay behind them for the
/// characters the lab's lack, such as its emoji.
///
/// The icon font goes second, never first: its blank ligature glyphs cover
/// `a` to `z` and would hide the text font's. An icon drawn on its own uses
/// the [`icons`] family, Phosphor first, because Inter has glyphs of its
/// own at some of Phosphor's code points.
#[must_use]
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (TEXT_FONT, INTER_REGULAR),
        (STRONG_FONT, INTER_SEMIBOLD),
        (MONO_FONT, JETBRAINS_MONO),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, TEXT_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, MONO_FONT.to_owned());
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
    monospace.insert(monospace.len().min(1), ICON_FONT.to_owned());
    let mut strong_family = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if let Some(first) = strong_family.first_mut() {
        STRONG_FONT.clone_into(first);
    }
    fonts.families.insert(strong(), strong_family);
    let mut icon_family = vec![ICON_FONT.to_owned()];
    icon_family.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .filter(|name| *name != ICON_FONT)
            .cloned(),
    );
    fonts.families.insert(icons(), icon_family);
    fonts
}

/// egui's dark visuals in the theme's colours and shapes.
#[must_use]
pub fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = SURFACE_1;
    visuals.window_fill = SURFACE_2;
    visuals.extreme_bg_color = SURFACE_0;
    visuals.code_bg_color = SURFACE_0;
    visuals.faint_bg_color = SURFACE_2;
    visuals.hyperlink_color = INFO;
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = ERROR;
    visuals.window_stroke = Stroke::new(1.0, LINE_STRONG);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_WINDOW);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_CARD);
    visuals.window_shadow = Shadow {
        offset: [0, 10],
        blur: 36,
        spread: 0,
        color: Color32::from_black_alpha(107),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0x43, 0xd2, 0x9e, 90);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = HandleShape::Circle;

    let control = CornerRadius::same(RADIUS_CONTROL);
    let widgets = &mut visuals.widgets;
    // Labels, separators and frames that take no clicks.
    widgets.noninteractive.bg_fill = SURFACE_1;
    widgets.noninteractive.weak_bg_fill = SURFACE_1;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = control;
    // Buttons, fields and handles at rest, hovered, pressed and open.
    widgets.inactive.bg_fill = SURFACE_3;
    widgets.inactive.weak_bg_fill = SURFACE_3;
    widgets.inactive.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = control;
    widgets.inactive.expansion = 0.0;
    widgets.hovered.bg_fill = SURFACE_4;
    widgets.hovered.weak_bg_fill = SURFACE_4;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(40));
    widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    widgets.hovered.corner_radius = control;
    widgets.hovered.expansion = 0.0;
    widgets.active.bg_fill = Color32::from_rgb(0x2f, 0x35, 0x41);
    widgets.active.weak_bg_fill = Color32::from_rgb(0x2f, 0x35, 0x41);
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT_LINE);
    widgets.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    widgets.active.corner_radius = control;
    widgets.active.expansion = 0.0;
    widgets.open.bg_fill = SURFACE_3;
    widgets.open.weak_bg_fill = SURFACE_4;
    widgets.open.bg_stroke = Stroke::new(1.0, LINE_STRONG);
    widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.open.corner_radius = control;
    visuals
}

/// Sets the theme and the fonts on both of egui's styles (dark and light),
/// so the look does not follow the desktop's light or dark preference.
/// The fonts take effect at the start of the next pass; until then text
/// draws in egui's own, and [`strong_font`] falls back to them.
pub fn apply(ctx: &egui::Context) {
    let visuals = visuals();
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_fonts(fonts());
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
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(GAP, 5.0);
        spacing.button_padding = vec2(9.0, 3.0);
        spacing.interact_size.y = CONTROL_HEIGHT;
        spacing.menu_margin = Margin::same(6);
        spacing.window_margin = Margin::same(CARD_PADDING);
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
            assert_eq!(style.visuals.window_fill, SURFACE_2);
            let size = |text_style| style.text_styles[&text_style].size;
            assert!((size(TextStyle::Body) - BODY).abs() < f32::EPSILON);
            assert!((size(TextStyle::Monospace) - MONOSPACE).abs() < f32::EPSILON);
            // No text style names the strong family: egui would panic on it
            // in the pass before the fonts go live.
            assert!(
                style
                    .text_styles
                    .values()
                    .all(|font| font.family != strong())
            );
        }
        assert_eq!(ctx.theme(), egui::Theme::Dark);
    }

    /// WCAG's relative luminance of an opaque sRGB colour.
    fn luminance(color: Color32) -> f64 {
        let channel = |value: u8| {
            let c = f64::from(value) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Text and secondary text read on every surface; faint text and the
    /// coloured inks on the four text sits on at rest (hovered and chosen
    /// controls draw their text in [`TEXT`]); text on the accent reads too.
    #[test]
    fn inks_keep_their_contrast_on_every_surface() {
        let mut low = Vec::new();
        for (name, ink, surfaces) in [
            ("TEXT", TEXT, &SURFACES[..]),
            ("TEXT_2", TEXT_2, &SURFACES[..]),
            ("TEXT_3", TEXT_3, &SURFACES[..4]),
            ("ACCENT", ACCENT, &SURFACES[..4]),
            ("INFO", INFO, &SURFACES[..4]),
            ("NEW", NEW, &SURFACES[..4]),
            ("WARNING", WARNING, &SURFACES[..4]),
            ("ERROR", ERROR, &SURFACES[..4]),
        ] {
            for (step, surface) in surfaces.iter().enumerate() {
                let ratio = contrast(ink, *surface);
                if ratio < 4.5 {
                    low.push(format!("{name} on SURFACE_{step}: {ratio:.2}"));
                }
            }
        }
        assert!(contrast(ON_ACCENT, ACCENT) >= 4.5);
        // A label on a series mark: dark on the six colours, the text ink
        // on the grey.
        for (slot, series) in SERIES[..6].iter().enumerate() {
            let ratio = contrast(ON_MARK, *series);
            if ratio < 4.5 {
                low.push(format!("ON_MARK on SERIES[{slot}]: {ratio:.2}"));
            }
        }
        assert!(contrast(TEXT, SERIES[6]) >= 4.5);
        assert!(low.is_empty(), "{low:?}");
    }

    /// The chart palette is the set checked for colour-blind separation;
    /// a change needs the check again (Project After's `docs/developer/LAB-UI.md`).
    #[test]
    fn the_series_are_the_checked_palette() {
        let hex: Vec<String> = SERIES
            .iter()
            .map(|c| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b()))
            .collect();
        assert_eq!(
            hex,
            [
                "#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#9085e9", "#4a505b"
            ]
        );
        // No series colour doubles as a status colour.
        assert!(SERIES.iter().all(|series| !STATUS.contains(series)));
    }

    /// The fonts load and resolve: text from Inter, monospace from
    /// `JetBrains Mono` with every digit as wide as the others, icons from
    /// Phosphor in proportional, monospace and strong text alike.
    #[test]
    fn the_lab_fonts_draw_text_numbers_and_icons() {
        let ctx = egui::Context::default();
        apply(&ctx);
        crate::test_pass(&ctx, egui::RawInput::default(), |_| {});
        let definitions = fonts();
        assert_eq!(
            definitions.families[&FontFamily::Proportional][..2],
            [TEXT_FONT, ICON_FONT]
        );
        assert_eq!(
            definitions.families[&FontFamily::Monospace][..2],
            [MONO_FONT, ICON_FONT]
        );
        assert_eq!(
            definitions.families[&strong()][..2],
            [STRONG_FONT, ICON_FONT]
        );
        assert_eq!(definitions.families[&icons()][..2], [ICON_FONT, TEXT_FONT]);
        ctx.fonts_mut(|fonts| {
            let mono = FontId::monospace(MONOSPACE);
            let widths: Vec<f32> = "0123456789"
                .chars()
                .map(|digit| fonts.glyph_width(&mono, digit))
                .collect();
            assert!(
                widths.iter().all(|width| (width - widths[0]).abs() < 1e-3),
                "{widths:?}"
            );
            for font in [
                FontId::proportional(BODY),
                FontId::monospace(MONOSPACE),
                FontId::new(HEADING, strong()),
            ] {
                assert!(fonts.has_glyphs(&font, "Ag°µ"), "{font:?}");
                assert!(fonts.has_glyphs(&font, crate::icons::GEAR_SIX), "{font:?}");
            }
            // SemiBold is wider than Regular at the same size.
            let mut word = |font: &FontId| {
                "Diagnostics"
                    .chars()
                    .map(|c| fonts.glyph_width(font, c))
                    .sum::<f32>()
            };
            let strong = word(&FontId::new(BODY, strong()));
            assert!(strong > word(&FontId::proportional(BODY)));
        });
    }

    /// Inter has a glyph of its own at `GEAR_SIX`'s code point, narrower
    /// than Phosphor's gear (egui's `has_glyphs` does not count it, but
    /// layout draws it), which wins in the text families; the icon family
    /// draws Phosphor's gear there.
    #[test]
    fn an_icon_alone_draws_phosphors_glyph_where_inter_has_one_too() {
        let ctx = egui::Context::default();
        let mut definitions = fonts();
        let inter = FontFamily::Name("Inter alone".into());
        let phosphor = FontFamily::Name("Phosphor alone".into());
        definitions
            .families
            .insert(inter.clone(), vec![TEXT_FONT.to_owned()]);
        definitions
            .families
            .insert(phosphor.clone(), vec![ICON_FONT.to_owned()]);
        ctx.set_fonts(definitions);
        crate::test_pass(&ctx, egui::RawInput::default(), |_| {});
        let gear = crate::icons::GEAR_SIX;
        ctx.fonts_mut(|fonts| {
            let mut width = |family: &FontFamily| {
                let galley = fonts.layout_no_wrap(
                    gear.to_owned(),
                    FontId::new(BODY, family.clone()),
                    Color32::WHITE,
                );
                galley.size().x
            };
            let (inter, phosphor) = (width(&inter), width(&phosphor));
            assert!((inter - phosphor).abs() > 0.5, "{inter} {phosphor}");
            assert!((width(&icons()) - phosphor).abs() < 1e-3);
            assert!((width(&FontFamily::Proportional) - inter).abs() < 1e-3);
        });
    }
}
