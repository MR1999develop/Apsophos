//! Left panel: the processing queue.

use eframe::egui::{self, vec2, Button, RichText};

use crate::app::*;
use crate::format::{self, file_name, truncate_middle};
use crate::settings::Destination;
use crate::text::tr;
use crate::theme::*;
use crate::worker::ItemStatus;

use super::*;

impl ApsophosApp {
    pub(super) fn ui_queue(&mut self, root: &mut egui::Ui) {
        let t = self.tokens;
        let tx = &tr().queue;
        let f = self.facts();
        let panel = egui::Panel::left("queue")
            .resizable(true)
            .default_size(QUEUE_PANEL_WIDTH)
            .size_range(QUEUE_PANEL_RANGE)
            .show(root, |ui| {
                ui.add_space(SPACE_M);
                ui.horizontal(|ui| {
                    ui.label(group_title(tx.title, &t));
                    ui.label(weak((tr().numbers.files)(self.queue.items.len()), &t));
                    if self.scanning > 0 {
                        ui.spinner();
                        ui.label(weak(tx.adding, &t));
                    }
                });
                ui.add_space(SPACE_S);
                ui.horizontal(|ui| {
                    let add = add_command(ui, f.edit_queue(), Button::new(tx.add_files));
                    self.panes.land_on(Pane::Queue, &add);
                    if add.clicked() {
                        self.add_files_dialog();
                    }
                    if add_command(ui, f.edit_queue(), Button::new(tx.add_folder))
                        .on_hover_text(tx.add_folder_tip)
                        .clicked()
                    {
                        self.add_folder_dialog();
                    }
                });
                ui.add_space(SPACE_M);

                // commands at the bottom, list in the remaining space
                egui::Panel::bottom("queue-commands").show_separator_line(false).show(ui, |ui| {
                    self.queue_controls(ui, &t);
                });
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.queue.items.is_empty() {
                        ui.label(weak(tx.empty, &t));
                    }
                    let mut remove = None;
                    let mut open = None;
                    let open_now = self.doc.as_ref().map(|d| d.path.clone());
                    for (n, item) in self.queue.items.iter().enumerate() {
                        let (icon, color, status, tip) = match &item.status {
                            ItemStatus::Queued => (symbol::PENDING, t.text_weak, tx.queued.to_owned(), String::new()),
                            ItemStatus::Running { done, total } => {
                                (symbol::CURRENT, t.text, format::processing(*done, *total), String::new())
                            }
                            ItemStatus::Done(p) => (
                                symbol::DONE,
                                t.success,
                                (tx.done)(&truncate_middle(&file_name(p), 26)),
                                p.display().to_string(),
                            ),
                            ItemStatus::Failed(e) => (symbol::ERROR, t.error, tx.failed.to_owned(), e.clone()),
                        };
                        // the item open in the viewer is the selected one
                        let selected = open_now.as_ref() == Some(&item.path);
                        let name = truncate_middle(&file_name(&item.path), 30);
                        let r = widgets::list_item(ui, selected, Some((icon.get(), color)), &name, Some(&status));
                        let r = r.on_hover_text(if tip.is_empty() { item.path.display().to_string() } else { tip });
                        if r.double_clicked() {
                            open = Some(item.path.clone());
                        }
                        r.context_menu(|ui| {
                            if ui.button(tx.open_in_viewer).clicked() {
                                open = Some(item.path.clone());
                                ui.close();
                            }
                            if let ItemStatus::Done(p) = &item.status {
                                if ui.button(tx.show_result).clicked() {
                                    ApsophosApp::reveal(p);
                                    ui.close();
                                }
                            }
                            if add_command(ui, f.edit_queue(), Button::new(tx.remove)).clicked() {
                                remove = Some(n);
                                ui.close();
                            }
                        });
                    }
                    if let Some(n) = remove {
                        self.remove_from_queue(n);
                    }
                    if let Some(p) = open {
                        self.open_path(p);
                    }
                });
            });
        self.panes.area(Pane::Queue, panel.response.rect);
    }

    pub(super) fn queue_controls(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tx = &tr().queue;
        let f = self.facts();
        ui.add_space(SPACE_S);
        ui.label(weak(tx.destination, t));
        let dest = match &self.settings.export.destination {
            Destination::SameAsSource => tx.next_to_originals.to_owned(),
            Destination::Folder(d) => truncate_middle(&d.display().to_string(), 34),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(dest).color(t.text));
            if ui.link(tx.change).clicked() {
                self.open_export_settings();
            }
        });
        ui.add_space(SPACE_S);
        let full = vec2(ui.available_width(), CONTROL_HEIGHT);
        if let Some(run) = &self.queue.running {
            let (started, total, paused) = (run.started, run.total, run.control.is_paused());
            let done_now = self.queue.finished().min(total);
            ui.add(
                egui::ProgressBar::new(format::fraction(done_now, total))
                    .desired_width(full.x)
                    .text((tx.progress)(done_now, total)),
            );
            if let Some(e) = format::eta(started.elapsed(), done_now, total) {
                ui.label(weak(e, t));
            }
            ui.horizontal(|ui| {
                if ui.button(if paused { tx.resume } else { tx.pause }).on_hover_text(tx.pause_tip).clicked() {
                    self.toggle_queue_pause();
                }
                // the only Stop control for the queue (also Esc)
                if ui.button(tx.stop).on_hover_text((tx.stop_tip)(&keys(ui, &SC_STOP))).clicked() {
                    self.stop_all();
                }
            });
        } else {
            let pending = f.queue_pending;
            let label = if pending > 0 { (tx.process_n)(pending) } else { tx.process.to_owned() };
            // as wide as the panel, text centred
            let justified = egui::Layout::top_down_justified(egui::Align::Center);
            ui.with_layout(justified, |ui| add_command(ui, f.process_queue(), Button::new(label).min_size(full)))
                .inner
                .on_hover_text((tx.process_tip)(&keys(ui, &SC_QUEUE)))
                .clicked()
                .then(|| self.start_queue());
            if !self.queue.items.is_empty() && ui.link(tx.clear).clicked() {
                self.clear_queue();
            }
        }
        ui.add_space(SPACE_S);
    }
}
