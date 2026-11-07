//! The icons of the interface, from the Windows icon font Segoe MDL2 Assets
//! (on Windows 10 and 11), so they match the system and each other. Without
//! that font (Linux, or Windows without it) each icon falls back to a
//! Unicode symbol the bundled fonts have. Use only these constants; the
//! tests below check that the fonts have each of them.

use std::sync::atomic::{AtomicBool, Ordering};

/// An icon: its Segoe MDL2 code point and the Unicode symbol used without
/// that font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon {
    mdl2: &'static str,
    plain: &'static str,
}

impl Icon {
    /// The text that draws the icon with the fonts installed.
    pub fn get(self) -> &'static str {
        if MDL2.load(Ordering::Relaxed) {
            self.mdl2
        } else {
            self.plain
        }
    }
}

/// Whether the Segoe MDL2 icon font is installed (set with the fonts).
static MDL2: AtomicBool = AtomicBool::new(false);

pub fn set_icon_font(installed: bool) {
    MDL2.store(installed, Ordering::Relaxed);
}

/// A notice that says what happened.
pub const INFO: Icon = Icon { mdl2: "\u{E946}", plain: "•" };
pub const WARNING: Icon = Icon { mdl2: "\u{E7BA}", plain: "⚠" };
pub const ERROR: Icon = Icon { mdl2: "\u{EA39}", plain: "⚠" };
pub const DONE: Icon = Icon { mdl2: "\u{E73E}", plain: "✓" };
/// Close a notice, remove an item.
pub const REMOVE: Icon = Icon { mdl2: "\u{E711}", plain: "×" };
/// Steps of a run: still to come, in progress (done is [`DONE`]).
pub const PENDING: Icon = Icon { mdl2: "\u{EA3A}", plain: "○" };
pub const CURRENT: Icon = Icon { mdl2: "\u{EA3B}", plain: "●" };

/// Every icon, for the tests that each has a glyph.
#[cfg(test)]
pub const ALL: &[Icon] = &[INFO, WARNING, ERROR, DONE, REMOVE, PENDING, CURRENT];

/// `icon` and `text` with the gap the interface uses between them.
pub fn with(icon: Icon, text: impl std::fmt::Display) -> String {
    format!("{}  {text}", icon.get())
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, FontFamily, FontId};

    fn has_all(ctx: &egui::Context, pick: impl Fn(super::Icon) -> &'static str) {
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let font = FontId::proportional(14.0);
        for &icon in super::ALL {
            let c = pick(icon).chars().next().unwrap_or_default();
            assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "U+{:04X} is missing", u32::from(c));
        }
    }

    #[test]
    fn the_icon_font_has_every_icon() {
        let Some(icons) = crate::platform::ui_fonts().and_then(|f| f.icons) else { return };
        let mut defs = egui::FontDefinitions::empty();
        defs.font_data.insert("icons".into(), std::sync::Arc::new(egui::FontData::from_owned(icons)));
        defs.families.insert(FontFamily::Proportional, vec!["icons".into()]);
        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
        has_all(&ctx, |i| i.mdl2);
    }

    #[test]
    fn the_bundled_fonts_have_every_fallback() {
        let ctx = egui::Context::default();
        crate::theme::install_bundled_fonts(&ctx);
        has_all(&ctx, |i| i.plain);
    }
}
