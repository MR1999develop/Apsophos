//! The look of the window, in one place: colour tokens, the type scale,
//! spacing and corners. The rest of the interface uses only these values.
//!
//! Sizes, fonts and component shapes follow the Windows 11 style shared with
//! the other desktop programs of this family (Segoe UI, Segoe MDL2 icons, the
//! same spacing and type scale). The colours stay apart on purpose: the
//! interface is dark and strictly greyscale, so that the only colour on
//! screen is the photo, and states are never told apart by colour alone
//! (icons and text carry the meaning).

use std::sync::Arc;

use eframe::egui::{
    self, vec2, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    TextStyle,
};

use crate::{platform, symbol};

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_S: f32 = 8.0;
pub const SPACE_M: f32 = 16.0;
pub const SPACE_L: f32 = 24.0;
/// Between rows of controls, and the height of a control.
pub const ROW_GAP: f32 = 6.0;
pub const CONTROL_HEIGHT: f32 = 24.0;
/// Corners of controls, list items and highlighted cards.
pub const RADIUS: u8 = 4;
/// Corners of windows, menus and popups.
pub const RADIUS_WINDOW: u8 = 8;

/// Body text, captions (hints, details), page titles.
pub const FONT_BODY: f32 = 14.0;
pub const FONT_SMALL: f32 = 12.0;
pub const FONT_TITLE: f32 = 20.0;
const FONT_MONO: f32 = 13.0;

/// Width of the parameters panel, and how far it can be resized.
pub const PARAMS_PANEL_WIDTH: f32 = 300.0;
pub const PARAMS_PANEL_RANGE: std::ops::RangeInclusive<f32> = 260.0..=380.0;
/// Width of the queue panel, and how far it can be resized.
pub const QUEUE_PANEL_WIDTH: f32 = 260.0;
pub const QUEUE_PANEL_RANGE: std::ops::RangeInclusive<f32> = 220.0..=420.0;
/// Width of progress bars in the status bar, and of short text fields.
pub const STATUS_BAR_PROGRESS_W: f32 = 240.0;
pub const SHORT_FIELD_W: f32 = 160.0;
/// Height of the histogram in the side panel.
pub const HISTOGRAM_H: f32 = 72.0;

/// The semibold face: section and page titles.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// The colours of the (only) theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    /// Window and panel background.
    pub bg: Color32,
    /// Raised surfaces: text fields, windows, menus, overlays on the image.
    pub surface: Color32,
    pub text: Color32,
    /// Hints, details, units.
    pub text_weak: Color32,
    /// Borders of fields and controls.
    pub stroke: Color32,
    /// Separators and decorative outlines.
    pub stroke_weak: Color32,
    /// Fill of controls at rest, and of hovered list items.
    pub control: Color32,
    pub control_hover: Color32,
    /// Clickable: links, primary button, focus, ticks; a light grey here.
    pub accent: Color32,
    /// Text on an accent fill.
    pub on_accent: Color32,
    /// Background of the selected list item, and of selected text.
    pub selected: Color32,
    /// Neutral grey behind the image.
    pub canvas: Color32,
    /// Status texts: greys too, always paired with an icon.
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
}

const fn grey(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

impl Tokens {
    pub const DARK: Self = Self {
        bg: grey(0x1E),
        surface: grey(0x26),
        text: grey(0xED),
        text_weak: grey(0xAB),
        stroke: grey(0x7C),
        stroke_weak: grey(0x33),
        control: grey(0x2E),
        control_hover: grey(0x3A),
        accent: grey(0xD6),
        on_accent: grey(0x14),
        selected: grey(0x3E),
        canvas: grey(0x2A),
        error: grey(0xF2),
        warning: grey(0xDA),
        success: grey(0xC4),
    };

    /// The tokens `ui` is drawn with (one theme only).
    pub fn current(_ui: &egui::Ui) -> Self {
        Self::DARK
    }

    fn visuals(self) -> egui::Visuals {
        let mut v = egui::Visuals::dark();
        let control = CornerRadius::same(RADIUS);
        v.panel_fill = self.bg;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.surface;
        v.faint_bg_color = self.control_hover;
        v.code_bg_color = self.control;
        v.text_edit_bg_color = Some(self.surface);
        v.weak_text_color = Some(self.text_weak);
        v.hyperlink_color = self.accent;
        v.error_fg_color = self.error;
        v.warn_fg_color = self.warning;
        v.selection.bg_fill = self.selected;
        v.selection.stroke = Stroke::new(1.0, self.text);
        // slider and progress bar fill: grey, never coloured
        v.slider_trailing_fill = true;
        v.text_cursor.stroke = Stroke::new(2.0, self.text);
        v.window_corner_radius = CornerRadius::same(RADIUS_WINDOW);
        v.menu_corner_radius = CornerRadius::same(RADIUS_WINDOW);
        v.window_stroke = Stroke::new(1.0, self.stroke_weak);
        let shadow = Color32::from_black_alpha(90);
        v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: shadow };
        v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: shadow };
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.bg;
        w.noninteractive.weak_bg_fill = self.bg;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.stroke_weak);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        w.noninteractive.corner_radius = control;
        for (state, fill) in [
            (&mut w.inactive, self.control),
            (&mut w.hovered, self.control_hover),
            (&mut w.active, self.control_hover),
            (&mut w.open, self.control_hover),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, self.stroke_weak);
            state.fg_stroke = Stroke::new(1.0, self.text);
            state.corner_radius = control;
            state.expansion = 0.0;
        }
        // fields and boxes need a visible edge (3:1) to be found
        w.inactive.bg_stroke = Stroke::new(1.0, self.stroke);
        w.hovered.bg_stroke = Stroke::new(1.0, self.stroke);
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        v
    }
}

/// Group title: semibold, strong colour.
pub fn group_title(text: &str, t: &Tokens) -> egui::RichText {
    egui::RichText::new(text).family(semibold()).size(FONT_BODY).color(t.text)
}

/// Secondary text.
pub fn weak(text: impl Into<String>, t: &Tokens) -> egui::RichText {
    egui::RichText::new(text.into()).color(t.text_weak)
}

/// The interface fonts: Segoe UI and the Segoe MDL2 icons from Windows when
/// it has them; the Inter shipped with the program behind them (for the
/// symbols Segoe UI lacks, and in its place elsewhere), then egui's own
/// fonts for other scripts.
pub fn install_fonts(ctx: &Context) {
    let system = platform::ui_fonts();
    symbol::set_icon_font(system.as_ref().is_some_and(|f| f.icons.is_some()));
    ctx.set_fonts(font_definitions(system));
}

/// The fonts without the system ones, as on Linux: for the test that the
/// icons' fallback symbols can be drawn.
#[cfg(test)]
pub fn install_bundled_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions(None));
}

fn font_definitions(system: Option<platform::UiFonts>) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let egui_fonts = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let (mut regular, mut bold) = (Vec::new(), Vec::new());
    if let Some(fonts) = system {
        defs.font_data.insert("ui".into(), Arc::new(FontData::from_owned(fonts.regular)));
        defs.font_data.insert("ui-semibold".into(), Arc::new(FontData::from_owned(fonts.semibold)));
        regular.push("ui".to_owned());
        bold.push("ui-semibold".to_owned());
        if let Some(icons) = fonts.icons {
            defs.font_data.insert("icons".into(), Arc::new(FontData::from_owned(icons)));
            regular.push("icons".to_owned());
            bold.push("icons".to_owned());
        }
    }
    defs.font_data
        .insert("inter".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    defs.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))),
    );
    regular.push("inter".to_owned());
    bold.push("inter-semibold".to_owned());
    regular.extend(egui_fonts.iter().cloned());
    bold.extend(egui_fonts);
    defs.families.insert(FontFamily::Proportional, regular);
    defs.families.insert(semibold(), bold);
    defs
}

/// Applies colours, the type scale and spacing.
pub fn apply(ctx: &Context, t: &Tokens) {
    // egui follows the system theme by default and keeps one style per theme:
    // with Windows in light mode it would show its light style instead of
    // ours. The interface is always dark, so force it and give both slots the
    // same style.
    ctx.set_theme(egui::ThemePreference::Dark);
    // and the window title bar (on Windows it follows the system otherwise)
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::viewport::SystemTheme::Dark));
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(FONT_SMALL, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(FONT_BODY, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(FONT_BODY, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(FONT_TITLE, semibold())),
        (TextStyle::Monospace, FontId::new(FONT_MONO, FontFamily::Monospace)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = vec2(SPACE_S, ROW_GAP);
    s.button_padding = vec2(12.0, 3.0);
    s.interact_size.y = CONTROL_HEIGHT;
    s.slider_width = 170.0;
    s.window_margin = Margin::same(16);
    s.menu_margin = Margin::same(6);
    s.indent = 20.0;
    style.visuals = t.visuals();
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
    // Ctrl with + / − / 0 zooms the photo, not the whole interface
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 contrast ratio of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let s = f32::from(v) / 255.0;
                if s <= 0.040_45 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_and_controls_keep_their_contrast() {
        let t = Tokens::DARK;
        for bg in [t.bg, t.surface, t.control, t.control_hover, t.selected] {
            assert!(contrast(t.text, bg) >= 4.5, "text on {bg:?}: {}", contrast(t.text, bg));
            assert!(contrast(t.text_weak, bg) >= 4.5, "weak text on {bg:?}: {}", contrast(t.text_weak, bg));
        }
        for bg in [t.bg, t.surface] {
            for c in [t.accent, t.error, t.warning, t.success] {
                assert!(contrast(c, bg) >= 4.5, "{c:?} on {bg:?}: {}", contrast(c, bg));
            }
            assert!(contrast(t.stroke, bg) >= 3.0, "field borders on {bg:?}: {}", contrast(t.stroke, bg));
        }
        assert!(contrast(t.on_accent, t.accent) >= 4.5, "primary button text");
    }

    #[test]
    fn palette_is_greyscale() {
        let t = Tokens::DARK;
        for c in [
            t.bg,
            t.surface,
            t.text,
            t.text_weak,
            t.stroke,
            t.stroke_weak,
            t.control,
            t.control_hover,
            t.accent,
            t.on_accent,
            t.selected,
            t.canvas,
            t.error,
            t.warning,
            t.success,
        ] {
            assert!(c.r() == c.g() && c.g() == c.b(), "{c:?} is not grey");
        }
    }
}
