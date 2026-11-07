//! Panels, menus and windows. Each panel is a view that directly modifies the
//! state of [`ApsophosApp`]; the actions live in `app.rs`.
//!
//! One module per panel; the helpers shared by the panels are here.

mod central;
mod menus;
mod params;
mod present;
mod queue;
mod status;
mod widgets;
mod windows;

use denoise_core::Percent;
use denoise_engine::Device;
use eframe::egui::{self, Button, KeyboardShortcut, Slider, Widget};

use crate::app::ApsophosApp;
use crate::symbol;
use crate::theme::*;
use present::{Availability, Facts};

/// Menu item with shortcut; when disabled it explains why.
fn menu_item(ui: &mut egui::Ui, enabled: bool, label: &str, sc: Option<&KeyboardShortcut>, why_disabled: &str) -> bool {
    let mut b = Button::new(label);
    if let Some(sc) = sc {
        b = b.shortcut_text(ui.ctx().format_shortcut(sc));
    }
    let r = ui.add_enabled(enabled, b).on_disabled_hover_text(why_disabled);
    if r.clicked() {
        ui.close();
        true
    } else {
        false
    }
}

/// Menu item for a command that may be unavailable, with the reason as its
/// tooltip.
fn command(ui: &mut egui::Ui, available: Availability, label: &str, sc: Option<&KeyboardShortcut>) -> bool {
    menu_item(ui, available.is_ok(), label, sc, available.err().unwrap_or_default())
}

/// A control for a command that may be unavailable, with the reason as its
/// tooltip when disabled.
fn add_command(ui: &mut egui::Ui, available: Availability, widget: impl Widget) -> egui::Response {
    ui.add_enabled(available.is_ok(), widget).on_disabled_hover_text(available.err().unwrap_or_default())
}

/// Slider for a percentage parameter, with the label above and a hint.
fn param_slider(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut Percent, hint: &str) -> egui::Response {
    ui.label(weak(label, t));
    ui.add_space(SPACE_XS);
    let mut v = value.get();
    let r = ui.add(Slider::new(&mut v, 0.0..=100.0).suffix(" %").fixed_decimals(0)).on_hover_text(hint);
    *value = Percent::clamped(v);
    r
}

/// Shortcut as shown to the user ("Ctrl+O").
fn keys(ui: &egui::Ui, sc: &KeyboardShortcut) -> String {
    ui.ctx().format_shortcut(sc)
}

impl ApsophosApp {
    /// What the commands depend on, for [`present`].
    fn facts(&self) -> Facts {
        Facts {
            has_image: self.doc.is_some(),
            image_complete: self.doc.as_ref().is_some_and(|d| d.is_complete()),
            tiles_done: self.doc.as_ref().map_or(0, |d| d.done_count()),
            denoising: self.is_running(),
            queue_running: self.queue_running(),
            queue_pending: self.queue.pending(),
            exporting: self.exporting,
            gpu_build: Device::gpu_available(),
        }
    }

    /// The whole interface, in the root `Ui` that eframe gives each frame.
    pub fn draw(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        self.ui_top(root);
        self.ui_status(root);
        self.ui_params(root);
        if self.settings.show_queue {
            self.ui_queue(root);
        }
        self.ui_central(root);
        self.ui_windows(&ctx);
    }
}
