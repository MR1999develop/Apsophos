//! Right panel: presets, sliders, the primary action, advanced options,
//! image information and histogram.

use denoise_core::{num, Preset};
use denoise_engine::Device;
use eframe::egui::{self, pos2, vec2, Button, RichText, Sense, Shape, Stroke};

use crate::app::*;
use crate::format::{self, file_name, truncate_middle};
use crate::inspect::BINS;
use crate::text::tr;
use crate::theme::*;

use super::present::Primary;
use super::*;

impl ApsophosApp {
    pub(super) fn ui_params(&mut self, root: &mut egui::Ui) {
        let t = self.tokens;
        let tx = &tr().params;
        let panel = egui::Panel::right("params")
            .resizable(true)
            .default_size(PARAMS_PANEL_WIDTH)
            .size_range(PARAMS_PANEL_RANGE)
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(SPACE_M);
                    ui.label(group_title(tx.title, &t));
                    ui.add_space(SPACE_XS);
                    ui.label(weak(tx.intro, &t));
                    ui.add_space(SPACE_M);
                    self.presets(ui, &t);
                    ui.add_space(SPACE_M);
                    self.sliders(ui, &t);
                    ui.add_space(SPACE_L);
                    self.primary(ui, &t);
                    ui.add_space(SPACE_L);
                    self.advanced(ui, &t);
                    ui.add_space(SPACE_M);
                    ui.separator();
                    ui.add_space(SPACE_M);
                    self.image_info(ui, &t);
                    self.histogram(ui, &t);
                });
            });
        self.panes.area(Pane::Params, panel.response.rect);
    }

    pub(super) fn presets(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tx = &tr().params;
        ui.label(weak(tx.preset, t));
        ui.add_space(SPACE_XS);
        let current = Preset::matching(&self.settings.params);
        ui.horizontal(|ui| {
            for p in Preset::ALL {
                let (name, tip) = match p {
                    Preset::Light => (tx.light, tx.light_tip),
                    Preset::Medium => (tx.medium, tx.medium_tip),
                    Preset::Strong => (tx.strong, tx.strong_tip),
                };
                let shown =
                    if p == Preset::Medium { format!("{name}{}", tx.recommended_mark) } else { name.to_owned() };
                let r = ui.selectable_label(current == Some(p), shown).on_hover_text(tip);
                if p == Preset::ALL[0] {
                    self.panes.land_on(Pane::Params, &r);
                }
                if r.clicked() {
                    self.set_params(p.params(), (tr().history.preset)(name));
                }
            }
        });
    }

    pub(super) fn sliders(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let (tx, h) = (&tr().params, &tr().history);
        let before = self.settings.params;
        let mut p = before;
        let luma = param_slider(ui, t, tx.grain, &mut p.luma, tx.grain_tip);
        ui.add_space(SPACE_M);
        let chroma = param_slider(ui, t, tx.colour, &mut p.chroma, tx.colour_tip);
        self.settings.params = p;
        for (r, what) in [(&luma, (h.grain)(p.luma.get().round())), (&chroma, (h.colour)(p.chroma.get().round()))] {
            if r.drag_started() {
                self.drag_origin = Some(before);
            }
            if r.drag_stopped() {
                if let Some(origin) = self.drag_origin.take() {
                    self.history.push_params(origin, p, what);
                }
            } else if r.changed() && !r.dragged() && self.drag_origin.is_none() {
                self.history.push_params(before, p, what);
            }
        }
    }

    pub(super) fn primary(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tx = &tr().params;
        let full = vec2(ui.available_width(), CONTROL_HEIGHT);
        let primary_button = |ui: &egui::Ui, text: &str| widgets::primary_button(ui, text).min_size(full);
        let wide = widgets::wide_button;
        match self.facts().primary() {
            Primary::Progress => {
                let Job::Running { done, total, started, .. } = self.job else { return };
                ui.add(egui::ProgressBar::new(format::fraction(done, total)).desired_width(full.x));
                ui.label(weak(format::tile_progress(started.elapsed(), done, total), t));
                ui.add_space(SPACE_S);
                // the only Stop control for the open image (also Esc)
                if wide(ui, Button::new(tx.stop).min_size(full))
                    .on_hover_text((tx.stop_image_tip)(&keys(ui, &SC_STOP)))
                    .clicked()
                {
                    self.stop_all();
                }
            }
            Primary::Open => {
                if wide(ui, primary_button(ui, tx.open_image)).on_hover_text(keys(ui, &SC_OPEN)).clicked() {
                    self.open_dialog();
                }
            }
            Primary::Export => {
                if wide(ui, primary_button(ui, tx.export))
                    .on_hover_text((tx.export_tip)(&keys(ui, &SC_EXPORT)))
                    .clicked()
                {
                    self.export();
                }
                ui.add_space(SPACE_XS);
                ui.horizontal(|ui| {
                    if ui.link(tx.export_as).clicked() {
                        self.export_as();
                    }
                    ui.label(weak("·", t));
                    if ui.link(tx.export_settings).clicked() {
                        self.open_export_settings();
                    }
                });
                ui.label(weak(tx.sliders_live, t));
            }
            Primary::Denoise { resume } => {
                let label = if resume { tx.finish_denoising } else { tx.denoise };
                if wide(ui, primary_button(ui, label)).on_hover_text((tx.denoise_tip)(&keys(ui, &SC_DENOISE))).clicked()
                {
                    self.start_denoise();
                }
            }
        }
        if self.exporting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(weak(tx.exporting, t));
            });
        }
        if let Some(h) = &self.hint {
            ui.add_space(SPACE_XS);
            ui.label(RichText::new(symbol::with(symbol::WARNING, h)).color(t.warning));
        }
    }

    pub(super) fn advanced(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let (tx, why) = (&tr().params, &tr().toolbar.why);
        let f = self.facts();
        egui::CollapsingHeader::new(group_title(tx.advanced, t)).id_salt("advanced").default_open(false).show(
            ui,
            |ui| {
                ui.label(weak(tx.device, t));
                ui.horizontal_wrapped(|ui| {
                    let dev = &mut self.settings.engine.device;
                    ui.selectable_value(dev, Device::Auto, tx.auto).on_hover_text(tx.auto_tip);
                    ui.add_enabled_ui(Device::rtx_available(), |ui| {
                        ui.selectable_value(dev, Device::TensorRtRtx, tx.rtx)
                            .on_hover_text(tx.rtx_tip)
                            .on_disabled_hover_text(why.no_rtx);
                    });
                    ui.add_enabled_ui(Device::directml_available(), |ui| {
                        ui.selectable_value(dev, Device::DirectMl, tx.directml)
                            .on_hover_text(tx.directml_tip)
                            .on_disabled_hover_text(why.no_directml);
                    });
                    ui.selectable_value(dev, Device::Cpu, tx.cpu);
                });
                ui.add_space(SPACE_M);
                ui.add_enabled_ui(self.settings.engine.device == Device::Cpu || !Device::gpu_available(), |ui| {
                    ui.label(weak(tx.threads, t));
                    ui.horizontal(|ui| {
                        let max = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
                        let mut auto = self.settings.engine.threads.is_none();
                        if widgets::checkbox(ui, &mut auto, tx.threads_auto)
                            .on_hover_text((tx.threads_auto_tip)(max))
                            .changed()
                        {
                            self.settings.engine.threads = if auto { None } else { std::num::NonZeroUsize::new(max) };
                        }
                        if let Some(n) = self.settings.engine.threads {
                            let mut v = n.get();
                            ui.add(egui::DragValue::new(&mut v).range(1..=max));
                            self.settings.engine.threads = std::num::NonZeroUsize::new(v.clamp(1, max));
                        }
                    });
                })
                .response
                .on_disabled_hover_text(why.threads_cpu_only);
                ui.add_space(SPACE_S);
                ui.label(weak(tx.next_run, t));
                ui.add_space(SPACE_S);
                if add_command(ui, f.redetect_tiles(), Button::new(tx.redetect))
                    .on_hover_text(tx.redetect_tip)
                    .clicked()
                {
                    self.redetect_tiles();
                }
                for note in &self.tile_notes {
                    ui.label(RichText::new(note).size(FONT_SMALL).color(t.text_weak));
                }
                match &self.model {
                    ModelState::Ready(dev) => {
                        ui.label(weak((tx.in_use)(dev), t));
                    }
                    ModelState::Failed(e) => {
                        ui.label(RichText::new(symbol::with(symbol::WARNING, (tx.model_not_loaded)(e))).color(t.error));
                    }
                    ModelState::Loading | ModelState::NotLoaded => {}
                }
            },
        );
    }

    /// Luma histogram: the original filled, the result as a line.
    pub(super) fn histogram(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let params = self.settings.params;
        let Some(doc) = self.doc.as_mut() else { return };
        ui.add_space(SPACE_M);
        let tx = &tr().params;
        ui.label(group_title(tx.histogram, t));
        ui.add_space(SPACE_S);
        let hist = doc.histogram(&params);
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), HISTOGRAM_H), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, f32::from(RADIUS), t.canvas);
        // square-root scale: the few pixels of the shadows and highlights stay visible
        let peak = f64::from(hist.peak().max(1)).sqrt();
        let x_of = |i: usize| rect.left() + rect.width() * i as f32 / (BINS - 1) as f32;
        let y_of = |v: u32| rect.bottom() - rect.height() * num::narrow(f64::from(v).sqrt() / peak);
        let bar = Stroke::new((rect.width() / BINS as f32).max(1.0), t.stroke);
        for (i, &v) in hist.original.iter().enumerate().filter(|(_, v)| **v > 0) {
            painter.line_segment([pos2(x_of(i), rect.bottom()), pos2(x_of(i), y_of(v))], bar);
        }
        let line: Vec<_> = hist.result.iter().enumerate().map(|(i, &v)| pos2(x_of(i), y_of(v))).collect();
        painter.add(Shape::line(line, Stroke::new(1.5, t.text)));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, tx.histogram));
        if let Some((orig, result)) = hist.medians() {
            let text = (tx.histogram_a11y)(&format::dec(orig, 0), &format::dec(result, 0));
            response.ctx.accesskit_node_builder(response.id, |node| node.set_description(text));
        }
        response.on_hover_text(tx.histogram_tip);
        ui.label(weak(tx.histogram_legend, t));
    }

    pub(super) fn image_info(&self, ui: &mut egui::Ui, t: &Tokens) {
        let tx = &tr().info;
        ui.label(group_title(tx.title, t));
        ui.add_space(SPACE_S);
        let Some(doc) = &self.doc else {
            ui.label(weak(tx.no_image, t));
            return;
        };
        let info = &doc.info;
        let model = if info.gray { tx.grey } else { tx.rgb };
        let kept = |present: bool| (if present { tx.kept } else { tx.none }).to_owned();
        let rows = [
            (tx.file, truncate_middle(&file_name(&doc.path), 32)),
            (
                tx.format,
                format!(
                    "{} · {} · {model}{}",
                    info.format.label(),
                    (tr().info.bit_depth)(info.bit_depth),
                    if doc.alpha.is_some() { tx.with_alpha } else { "" }
                ),
            ),
            (
                tx.size,
                format!(
                    "{} · {}",
                    (tr().status.size)(&format::int(doc.width()), &format::int(doc.height())),
                    format::megapixels(doc.width(), doc.height())
                ),
            ),
            (tx.profile, doc.profile_label()),
            (
                tx.display,
                match (&doc.display.transform().problem, doc.display.transform().is_managed()) {
                    (Some(problem), _) => symbol::with(symbol::WARNING, problem),
                    (None, true) => tx.display_converted.into(),
                    (None, false) => tx.display_as_is.into(),
                },
            ),
            (tx.exif, kept(info.exif.is_some())),
            (tx.xmp, kept(info.xmp.is_some())),
            (
                tx.resolution,
                info.dpi.map_or_else(
                    || tx.none.to_owned(),
                    |d| {
                        let (x, y) = (format::dec(num::narrow(d.x), 0), format::dec(num::narrow(d.y), 0));
                        (tx.dpi)(&x, (y != x).then_some(y.as_str()))
                    },
                ),
            ),
        ];
        egui::Grid::new("info").num_columns(2).spacing(vec2(SPACE_M, SPACE_S)).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(weak(k, t));
                ui.add(egui::Label::new(RichText::new(v).color(t.text)).wrap());
                ui.end_row();
            }
        });
        if let Some(q) = info.jpeg_quality.filter(|q| q.is_heavy()) {
            // discreet: an icon and words in the panel, never a colour
            ui.add_space(SPACE_S);
            let text = symbol::with(symbol::INFO, (tx.heavy_jpeg)(q.get()));
            ui.add(egui::Label::new(RichText::new(text).color(t.text)).wrap());
        }
        if doc.done_count() > 0 && !doc.is_complete() && !self.is_running() {
            ui.add_space(SPACE_S);
            ui.label(weak((tx.partial)(&format::int(doc.done_count()), &format::int(doc.plan.len())), t));
        }
    }
}
