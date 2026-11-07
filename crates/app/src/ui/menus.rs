//! Menu bar, Help menu and toolbar.

use denoise_core::{product, Params};
use eframe::egui::{self, vec2, Button};

use crate::app::*;
use crate::format::{file_name, truncate_middle};
use crate::text::tr;
use crate::theme::*;
use crate::viewer::ResultView;

use super::*;

impl ApsophosApp {
    pub(super) fn ui_top(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.menu_file(ui);
                self.menu_edit(ui);
                self.menu_view(ui);
                self.menu_process(ui);
                let m = &tr().menu;
                ui.menu_button(m.help, |ui| {
                    if menu_item(ui, true, m.keyboard_shortcuts, None, "") {
                        self.windows.shortcuts = true;
                    }
                    if menu_item(ui, true, m.open_log_folder, None, "") {
                        ApsophosApp::open_log_folder();
                    }
                    if menu_item(ui, true, &(m.about)(product::NAME), None, "") {
                        self.windows.about = true;
                    }
                });
            });
            ui.add_space(SPACE_XS);
            self.toolbar(ui);
            ui.add_space(SPACE_XS);
        });
    }

    pub(super) fn menu_file(&mut self, ui: &mut egui::Ui) {
        let f = self.facts();
        let m = &tr().menu;
        ui.menu_button(m.file, |ui| {
            if menu_item(ui, true, m.open, Some(&SC_OPEN), "") {
                self.open_dialog();
            }
            ui.menu_button(m.open_recent, |ui| {
                if self.settings.recent.is_empty() {
                    ui.add_enabled(false, Button::new(m.no_recent));
                }
                let mut chosen = None;
                for p in &self.settings.recent {
                    if ui.button(truncate_middle(&file_name(p), 48)).on_hover_text(p.display().to_string()).clicked() {
                        chosen = Some(p.clone());
                        ui.close();
                    }
                }
                if !self.settings.recent.is_empty() {
                    ui.separator();
                    if menu_item(ui, true, m.clear_list, None, "") {
                        self.settings.recent.clear();
                    }
                }
                if let Some(p) = chosen {
                    self.open_path(p);
                }
            });
            ui.separator();
            if command(ui, f.edit_queue(), m.add_files, None) {
                self.add_files_dialog();
            }
            if command(ui, f.edit_queue(), m.add_folder, None) {
                self.add_folder_dialog();
            }
            ui.separator();
            if command(ui, f.export(), m.export, Some(&SC_EXPORT)) {
                self.export();
            }
            if command(ui, f.export(), m.export_as, Some(&SC_EXPORT_AS)) {
                self.export_as();
            }
            if menu_item(ui, true, m.export_settings, None, "") {
                self.windows.export = true;
            }
            ui.separator();
            if command(ui, f.view(), m.close_image, Some(&SC_CLOSE)) {
                self.close_image();
            }
            if menu_item(ui, true, m.quit, Some(&SC_QUIT), "") {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    pub(super) fn menu_edit(&mut self, ui: &mut egui::Ui) {
        let (m, why) = (&tr().menu, &tr().toolbar.why);
        ui.menu_button(m.edit, |ui| {
            let undo = self.history.undo_label();
            let label = undo.as_deref().map_or_else(|| m.undo.to_owned(), m.undo_action);
            if menu_item(ui, undo.is_some(), &label, Some(&SC_UNDO), why.nothing_to_undo) {
                self.undo();
            }
            let redo = self.history.redo_label();
            let label = redo.as_deref().map_or_else(|| m.redo.to_owned(), m.redo_action);
            if menu_item(ui, redo.is_some(), &label, Some(&SC_REDO), why.nothing_to_redo) {
                self.redo();
            }
            ui.separator();
            let is_default = self.settings.params == Params::default();
            if menu_item(ui, !is_default, m.reset, None, why.already_recommended) {
                self.set_params(Params::default(), tr().history.recommended);
            }
            ui.separator();
            if menu_item(ui, true, m.preferences, None, "") {
                self.windows.preferences = true;
            }
        });
    }

    pub(super) fn menu_view(&mut self, ui: &mut egui::Ui) {
        let f = self.facts();
        let m = &tr().menu;
        ui.menu_button(m.view, |ui| {
            if command(ui, f.view(), m.fit, self.single_key(&SC_FIT)) {
                self.view.request_fit();
            }
            if command(ui, f.view(), m.actual_size, self.single_key(&SC_ACTUAL)) {
                self.view.actual_size(None);
            }
            ui.separator();
            let label = match self.result_view {
                ResultView::Denoised => m.show_removed_noise,
                ResultView::RemovedNoise => m.show_result,
            };
            if command(ui, f.view(), label, self.single_key(&SC_NOISE)) {
                self.toggle_removed_noise();
            }
            ui.separator();
            widgets::checkbox(ui, &mut self.settings.show_queue, m.show_queue);
        });
    }

    pub(super) fn menu_process(&mut self, ui: &mut egui::Ui) {
        let f = self.facts();
        let m = &tr().menu;
        ui.menu_button(m.process, |ui| {
            if command(ui, f.denoise(), m.denoise, Some(&SC_DENOISE)) {
                self.start_denoise();
            }
            if command(ui, f.stop(), m.stop, Some(&SC_STOP)) {
                self.stop_all();
            }
            ui.separator();
            if command(ui, f.process_queue(), m.process_queue, Some(&SC_QUEUE)) {
                self.start_queue();
            }
            let paused = self.queue.running.as_ref().is_some_and(|r| r.control.is_paused());
            let label = if paused { m.resume_queue } else { m.pause_queue };
            if command(ui, f.pause_queue(), label, None) {
                self.toggle_queue_pause();
            }
        });
    }

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        let f = self.facts();
        let tb = &tr().toolbar;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            let h = CONTROL_HEIGHT;
            if ui
                .add(Button::new(tb.open).min_size(vec2(0.0, h)))
                .on_hover_text((tb.open_tip)(&keys(ui, &SC_OPEN)))
                .clicked()
            {
                self.open_dialog();
            }
            if add_command(ui, f.export(), Button::new(tb.export).min_size(vec2(0.0, h)))
                .on_hover_text((tb.export_tip)(&keys(ui, &SC_EXPORT)))
                .clicked()
            {
                self.export();
            }
            ui.separator();
            if add_command(ui, f.view(), Button::new(tb.fit).min_size(vec2(0.0, h)))
                .on_hover_text((tb.fit_tip)(&keys(ui, &SC_FIT)))
                .clicked()
            {
                self.view.request_fit();
            }
            if add_command(ui, f.view(), Button::new(tb.actual).min_size(vec2(0.0, h)))
                .on_hover_text((tb.actual_tip)(&keys(ui, &SC_ACTUAL)))
                .clicked()
            {
                self.view.actual_size(None);
            }
            ui.separator();
            if ui
                .add(Button::new(tb.queue).selected(self.settings.show_queue).min_size(vec2(0.0, h)))
                .on_hover_text(tb.queue_tip)
                .clicked()
            {
                self.settings.show_queue = !self.settings.show_queue;
            }
        });
    }
}
