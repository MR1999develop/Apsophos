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
pub use present::Pane;
use present::{Availability, Facts};

/// Where F6 can put the keyboard focus, recorded while the panes are drawn:
/// each pane's area and the control that takes the focus in it.
#[derive(Default)]
pub struct PaneTargets {
    rects: [Option<egui::Rect>; 3],
    landing: [Option<egui::Id>; 3],
    /// The pane the focus was last seen in, for the menu command (the menu
    /// itself holds the focus while it is open).
    last: Option<Pane>,
    /// Asked for from the menu, done on the next frame (`Some(back)`): in
    /// the frame of the click egui takes the focus away from every widget.
    pub requested: Option<bool>,
}

impl PaneTargets {
    /// Clears what the last frame recorded: a pane not drawn is not a target.
    fn begin_frame(&mut self) {
        self.rects = [None; 3];
        self.landing = [None; 3];
    }

    /// Records `pane`'s area.
    fn area(&mut self, pane: Pane, rect: egui::Rect) {
        self.rects[pane.index()] = Some(rect);
    }

    /// Records the control that takes the focus in `pane`.
    fn land_on(&mut self, pane: Pane, response: &egui::Response) {
        self.landing[pane.index()] = Some(response.id);
    }

    /// Whether the photo pane's own control (the image area, or the open
    /// button when no image is open) has the focus: the view's keys
    /// (arrows, Space) work there as when nothing has the focus.
    pub fn photo_focused(&self, ctx: &egui::Context) -> bool {
        self.landing[Pane::Photo.index()].is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
    }

    /// The pane holding the focused widget, if it is in one (a widget in a
    /// window or a menu is in none).
    fn focused_pane(&self, ctx: &egui::Context) -> Option<Pane> {
        let id = ctx.memory(egui::Memory::focused)?;
        let r = ctx.read_response(id)?;
        if r.layer_id.order != egui::Order::Background {
            return None;
        }
        let at = r.rect.center();
        Pane::ALL.into_iter().find(|p| self.rects[p.index()].is_some_and(|rect| rect.contains(at)))
    }
}

impl ApsophosApp {
    /// F6 / Shift+F6 (`back`) and the View menu: the focus goes to the next
    /// pane shown, on its first control.
    pub fn focus_next_pane(&mut self, ctx: &egui::Context, back: bool) {
        let panes = &mut self.panes;
        let current = panes.focused_pane(ctx).or(panes.last);
        let next = present::next_pane(current, self.settings.show_queue, back);
        if let Some(id) = panes.landing[next.index()] {
            ctx.memory_mut(|m| m.request_focus(id));
            panes.last = Some(next);
        }
    }
}

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
            reduce_noise: self.settings.export.reduce_noise,
            changes_the_photo: self.settings.export.changes_the_photo(),
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
        // the menu bar acts on what the last frame recorded
        self.ui_top(root);
        self.panes.begin_frame();
        self.ui_status(root);
        self.ui_params(root);
        if self.settings.show_queue {
            self.ui_queue(root);
        }
        self.ui_central(root);
        self.ui_windows(&ctx);
        if let Some(pane) = self.panes.focused_pane(&ctx) {
            self.panes.last = Some(pane);
        }
    }
}
