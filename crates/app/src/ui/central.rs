//! Central panel: the viewer, the notice bar, the empty state and the drop
//! overlay.

use eframe::egui::{self, vec2, Align, Button, Layout, RichText};

use crate::app::*;
use crate::format::{file_name, truncate_middle};
use crate::text::tr;
use crate::theme::*;
use crate::viewer::{self, ViewerInput};

use super::*;

impl ApsophosApp {
    pub(super) fn ui_central(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let t = self.tokens;
        let tx = &tr().central;
        let frame = egui::Frame::new().fill(t.canvas);
        egui::CentralPanel::default().frame(frame).show(root, |ui| {
            self.notice_bar(ui, &t);
            let input = ViewerInput {
                tokens: &t,
                params: &self.settings.params,
                result_view: self.result_view,
                show_original: self.show_original,
                reduce_motion: self.settings.reduce_motion,
                working_on: self.working_on(),
            };
            if let Some(doc) = self.doc.as_mut() {
                let out = viewer::show(ui, &mut self.view, doc, &input);
                self.hover = out.hover;
            } else {
                self.hover = None;
                self.empty_state(ui, &t);
            }
            if let Some(path) = &self.opening {
                let rect = ui.max_rect();
                let p = ui.painter();
                let text = (tx.opening)(&truncate_middle(&file_name(path), 40));
                let galley = p.layout_no_wrap(text, egui::FontId::proportional(FONT_BODY), t.text);
                let box_rect =
                    egui::Rect::from_center_size(rect.center(), galley.size() + vec2(SPACE_L * 2.0, SPACE_M * 2.0));
                p.rect_filled(box_rect, f32::from(RADIUS_WINDOW), t.surface);
                p.galley(box_rect.min + vec2(SPACE_L, SPACE_M), galley, t.text);
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
            if hovering_files {
                let rect = ui.max_rect().shrink(SPACE_M);
                let p = ui.painter();
                p.rect_filled(rect, f32::from(RADIUS_WINDOW), t.surface.gamma_multiply(0.9));
                p.rect_stroke(
                    rect,
                    f32::from(RADIUS_WINDOW),
                    egui::Stroke::new(2.0, t.accent),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tx.drop_here,
                    egui::FontId::proportional(FONT_BODY),
                    t.text,
                );
            }
        });
    }

    pub(super) fn notice_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let Some(notice) = &self.notice else { return };
        let (icon, color) = match notice.kind {
            NoticeKind::Info => (symbol::INFO, t.text),
            NoticeKind::Success => (symbol::DONE, t.success),
            NoticeKind::Error => (symbol::WARNING, t.error),
        };
        let mut clicked = None;
        let mut dismiss = false;
        let dismiss_text = tr().central.dismiss;
        egui::Frame::new()
            .fill(t.surface)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .stroke(egui::Stroke::new(1.0, t.stroke_weak))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon.get()).color(color).size(FONT_BODY));
                    ui.add(egui::Label::new(RichText::new(&notice.text).color(t.text)).wrap());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::icon_button(ui, symbol::REMOVE.get(), dismiss_text).clicked() {
                            dismiss = true;
                        }
                        for a in notice.actions.iter().rev() {
                            if ui.button(a.label()).clicked() {
                                clicked = Some(a.clone());
                            }
                        }
                    });
                });
            });
        if dismiss {
            self.notice = None;
        }
        if let Some(a) = clicked {
            self.run_notice_action(a);
        }
    }

    pub(super) fn empty_state(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tx = &tr().central;
        let model_missing = self.settings.find_model().is_none();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.28).max(SPACE_L));
            ui.label(
                RichText::new((tx.empty_title)(&keys(ui, &SC_OPEN))).size(FONT_TITLE).family(semibold()).color(t.text),
            );
            ui.add_space(SPACE_S);
            ui.label(RichText::new(tx.empty_formats).color(t.text));
            ui.add_space(SPACE_M);
            if ui.add(Button::new(tx.open_image).min_size(vec2(SHORT_FIELD_W, CONTROL_HEIGHT))).clicked() {
                self.open_dialog();
            }
            if model_missing {
                ui.add_space(SPACE_L);
                ui.label(
                    RichText::new(symbol::with(symbol::WARNING, tx.model_missing)).color(t.warning).family(semibold()),
                );
                ui.label(RichText::new(tx.model_missing_detail).color(t.text));
                if ui.button(tx.choose_model).clicked() {
                    self.choose_model();
                }
            }
            if !self.settings.recent.is_empty() {
                ui.add_space(SPACE_L);
                ui.label(RichText::new(tx.recent).family(semibold()).color(t.text));
                ui.add_space(SPACE_XS);
                let mut chosen = None;
                for p in self.settings.recent.iter().take(6) {
                    if ui.link(truncate_middle(&file_name(p), 50)).on_hover_text(p.display().to_string()).clicked() {
                        chosen = Some(p.clone());
                    }
                }
                if let Some(p) = chosen {
                    self.open_path(p);
                }
            }
        });
    }
}
