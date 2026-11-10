//! Status bar: image size, pixel values, zoom, device and progress.

use denoise_io::BitDepth;
use eframe::egui::{self, Align, Layout};

use crate::app::*;
use crate::format;
use crate::text::tr;
use crate::theme::*;

impl ApsophosApp {
    pub(super) fn ui_status(&mut self, root: &mut egui::Ui) {
        let t = self.tokens;
        let tx = &tr().status;
        egui::Panel::bottom("status").show(root, |ui| {
            ui.add_space(SPACE_XS);
            ui.horizontal(|ui| {
                if let Some(doc) = &self.doc {
                    let size = (tx.size)(&format::int(doc.width()), &format::int(doc.height()));
                    ui.label(weak(format!("{size} · {}", (tr().info.bit_depth)(doc.info.bit_depth)), &t));
                    if let Some((x, y)) = self.hover {
                        let scale = doc.info.bit_depth.scale();
                        let src = if doc.is_pixel_done(x, y) && !self.show_original { &doc.raw } else { &doc.original };
                        if let Some(p) = src.pixel(x, y) {
                            let v = |c: f32| {
                                if doc.info.bit_depth == BitDepth::F32 {
                                    format::dec(c, 3)
                                } else {
                                    format!("{:.0}", (c * scale).clamp(0.0, scale))
                                }
                            };
                            ui.separator();
                            ui.monospace(format!("x {x} y {y}  R {} G {} B {}", v(p[0]), v(p[1]), v(p[2])));
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.doc.is_some() {
                        let mut pct = self.view.zoom_percent();
                        let r = ui
                            .add(
                                egui::DragValue::new(&mut pct)
                                    .range(1.0..=3200.0)
                                    .suffix(" %")
                                    .max_decimals(0)
                                    .speed(1.0),
                            )
                            .on_hover_text(tx.zoom_tip);
                        if r.changed() {
                            self.view.set_zoom_percent(pct);
                        }
                        ui.label(weak(tx.zoom, &t));
                        ui.separator();
                    }
                    match &self.model {
                        ModelState::Loading => {
                            ui.label(weak(tx.loading_model, &t));
                            ui.spinner();
                        }
                        ModelState::Ready(dev) => {
                            ui.label(weak(dev.clone(), &t));
                        }
                        _ => {}
                    }
                    // progress only: the Stop control lives in the side panels
                    if let Job::Running { done, total, started, .. } = self.job {
                        let mut text = (tx.tile)(done, total);
                        if let Some(e) = format::eta(started.elapsed(), done, total) {
                            text.push_str(" · ");
                            text.push_str(&e);
                        }
                        ui.add(
                            egui::ProgressBar::new(format::fraction(done, total))
                                .desired_width(STATUS_BAR_PROGRESS_W)
                                .text(text),
                        );
                    } else if let Some(run) = &self.queue.running {
                        let finished = self.queue.finished().min(run.total);
                        ui.add(
                            egui::ProgressBar::new(format::fraction(finished, run.total))
                                .desired_width(STATUS_BAR_PROGRESS_W)
                                .text((tx.queue)(finished, run.total)),
                        );
                    }
                });
            });
            ui.add_space(SPACE_XS);
        });
    }
}
