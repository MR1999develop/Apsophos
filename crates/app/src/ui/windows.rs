//! Windows: preferences, export settings, keyboard shortcuts, about.

use std::path::PathBuf;

use denoise_core::product;
use denoise_core::upscale::{Detail, Factor, Upscale};
use denoise_io::ExportFormat;
use eframe::egui::{self, vec2, KeyboardShortcut, RichText, Slider};

use crate::app::*;
use crate::download::{self, Pack};
use crate::format::{self, truncate_middle};
use crate::settings::Destination;
use crate::text::{self, tr, Language};
use crate::theme::*;

use super::*;

impl ApsophosApp {
    pub(super) fn ui_windows(&mut self, ctx: &egui::Context) {
        let t = self.tokens;
        let (tp, te, ts, ta) = (&tr().preferences, &tr().export, &tr().shortcuts, &tr().about);
        let mut open = self.windows.preferences;
        widgets::dialog(ctx, tp.title).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(group_title(tp.appearance, &t));
            widgets::checkbox(ui, &mut self.settings.reduce_motion, tp.reduce_motion);
            ui.add_space(SPACE_M);
            ui.label(group_title(tp.language, &t));
            // applies at once: every text is read from the catalog each frame
            let mut choice = self.settings.language;
            ui.radio_value(&mut choice, None, (tp.language_auto)(Language::system().name()));
            for language in Language::ALL {
                ui.radio_value(&mut choice, Some(language), language.name());
            }
            if choice != self.settings.language {
                self.settings.language = choice;
                text::set_language(choice);
            }
            ui.add_space(SPACE_M);
            ui.label(group_title(tp.keyboard, &t));
            let mut single_keys = !self.settings.single_keys_off;
            if widgets::checkbox(ui, &mut single_keys, tp.single_keys).on_hover_text(tp.single_keys_tip).changed() {
                self.settings.single_keys_off = !single_keys;
            }
            ui.add_space(SPACE_M);
            ui.label(group_title(tp.model, &t));
            let found = self.settings.find_model();
            let text = found
                .as_ref()
                .map_or_else(|| tp.not_found.to_owned(), |p| truncate_middle(&p.display().to_string(), 56));
            ui.label(
                RichText::new(if found.is_some() { text } else { symbol::with(symbol::WARNING, &text) }).color(t.text),
            );
            ui.horizontal(|ui| {
                if ui.button(tp.change).clicked() {
                    self.choose_model();
                }
                if self.settings.model_path.is_some() && ui.link(tp.automatic_search).clicked() {
                    self.settings.model_path = None;
                }
            });
            ui.label(weak(tp.model_where, &t));
        });
        self.windows.preferences = open;

        let mut open = self.windows.export;
        // the models on disk decide what can be chosen (read again at each
        // opening and after a download)
        let files = if open { self.upscaler_files_now() } else { Vec::new() };
        let denoised = self.doc.as_ref().is_some_and(crate::document::Document::is_complete);
        let mut want_preview = false;
        let running = self.download.as_ref().map(|d| (d.done, d.total));
        // the estimate needs the settings while the export part of them is borrowed
        let timings = self.settings.upscale_timings.clone();
        let device = self.settings.engine.device;
        let mut download_action = None;
        widgets::dialog(ctx, te.title).open(&mut open).resizable(false).show(ctx, |ui| {
            let e = &mut self.settings.export;
            ui.label(weak(te.format, &t));
            egui::ComboBox::from_id_salt("format")
                .selected_text(match e.options.format {
                    ExportFormat::SameAsSource => te.same_as_original,
                    ExportFormat::Tiff16 => te.tiff16,
                    ExportFormat::Jpeg => te.jpeg,
                    ExportFormat::Png16 => te.png16,
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut e.options.format, ExportFormat::SameAsSource, te.same_as_original)
                        .on_hover_text(te.same_as_original_tip);
                    ui.selectable_value(&mut e.options.format, ExportFormat::Tiff16, te.tiff16);
                    ui.selectable_value(&mut e.options.format, ExportFormat::Jpeg, te.jpeg);
                    ui.selectable_value(&mut e.options.format, ExportFormat::Png16, te.png16);
                });
            ui.add_space(SPACE_M);
            ui.label(weak(te.jpeg_quality, &t));
            let mut q = e.options.jpeg_quality;
            ui.add(Slider::new(&mut q, 50..=100)).on_hover_text(te.jpeg_quality_tip);
            e.options.jpeg_quality = q;
            ui.add_space(SPACE_M);
            ui.label(weak(te.suffix, &t));
            ui.add(egui::TextEdit::singleline(&mut e.suffix).desired_width(SHORT_FIELD_W));
            ui.label(weak((te.suffix_example)(&e.suffix), &t));
            ui.add_space(SPACE_M);
            let size = self.doc.as_ref().map(|d| (d.width(), d.height()));
            let speed = |u| crate::settings::upscale_speed(&timings, u, device);
            upscale_choice(ui, &t, &mut e.upscale, size, &files, &speed);
            download_action = download_block(ui, &t, &files, running);
            if e.upscale.is_some() && size.is_some() {
                let r = ui.add_enabled(denoised, egui::Button::new(te.preview));
                let r = r.on_hover_text(te.preview_tip).on_disabled_hover_text(te.preview_needs_denoise);
                want_preview |= r.clicked();
            }
            ui.add_space(SPACE_M);
            ui.label(weak(te.destination, &t));
            let mut same = e.destination == Destination::SameAsSource;
            if ui.radio_value(&mut same, true, te.next_to_original).clicked() {
                e.destination = Destination::SameAsSource;
            }
            ui.horizontal(|ui| {
                let label = match &e.destination {
                    Destination::Folder(d) => (te.folder)(&truncate_middle(&d.display().to_string(), 40)),
                    Destination::SameAsSource => te.a_folder.to_owned(),
                };
                if ui.radio(!same, label).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().set_title(tr().dialogs.destination_folder).pick_folder() {
                        e.destination = Destination::Folder(dir);
                    }
                }
            });
        });
        self.windows.export = open;
        if want_preview {
            // from the export settings: the centre of the view, the default
            self.preview_upscale(None);
        }
        match download_action {
            Some(DownloadAction::Start(pack)) => self.start_download(pack),
            Some(DownloadAction::Cancel) => self.cancel_download(),
            None => {}
        }
        self.ui_upscale_preview(ctx);

        let mut open = self.windows.shortcuts;
        let single_keys_off = self.settings.single_keys_off;
        widgets::dialog(ctx, ts.title).open(&mut open).resizable(false).show(ctx, |ui| {
            let k = |sc: &KeyboardShortcut| keys(ui, sc);
            // what a single key does, marked when those keys are turned off
            let single = |what: &str| if single_keys_off { format!("{what}{}", ts.off) } else { what.to_owned() };
            let rows: [(String, String); 17] = [
                (k(&SC_OPEN), ts.open.into()),
                (k(&SC_EXPORT), ts.export.into()),
                (k(&SC_EXPORT_AS), ts.export_as.into()),
                (k(&SC_DENOISE), ts.denoise.into()),
                (k(&SC_QUEUE), ts.process_queue.into()),
                (k(&SC_STOP), ts.stop.into()),
                (format!("{} / {}", k(&SC_UNDO), k(&SC_REDO)), ts.undo_redo.into()),
                (k(&SC_CLOSE), ts.close_image.into()),
                (k(&SC_FIT), single(ts.fit)),
                (k(&SC_ACTUAL), single(ts.actual_size)),
                (k(&SC_NOISE), single(ts.toggle_noise)),
                (ts.zoom_keys.into(), single(ts.zoom)),
                (ts.double_click.into(), ts.double_click_does.into()),
                (ts.original_keys.into(), ts.original.into()),
                (ts.arrows.into(), ts.move_view.into()),
                (ts.drag.into(), ts.drag_does.into()),
                (format!("{} / {}", k(&SC_NEXT_PANE), k(&SC_PREVIOUS_PANE)), ts.panes.into()),
            ];
            egui::Grid::new("shortcuts").num_columns(2).spacing(vec2(SPACE_L, SPACE_S)).show(ui, |ui| {
                for (k, v) in rows {
                    ui.monospace(k);
                    ui.label(v);
                    ui.end_row();
                }
            });
        });
        self.windows.shortcuts = open;

        let mut open = self.windows.about;
        // the model folders are read once per opening, not on every frame
        let models = if open {
            let settings = &self.settings;
            self.model_list
                .get_or_insert_with(|| crate::model_list::list(&settings.denoiser_files(), &settings.upscaler_files()))
                .clone()
        } else {
            self.model_list = None;
            Vec::new()
        };
        let title = (tr().menu.about)(product::NAME);
        widgets::dialog(ctx, title).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(product::NAME).size(FONT_TITLE).family(semibold()).color(t.text));
            ui.label(weak(ta.meaning, &t));
            ui.label(weak((ta.version)(env!("CARGO_PKG_VERSION")), &t));
            ui.add_space(SPACE_M);
            ui.label(ta.network);
            ui.label(ta.credits);
            ui.add_space(SPACE_S);
            egui::CollapsingHeader::new(group_title(ta.models, &t))
                .id_salt("about-models")
                .default_open(false)
                .show(ui, |ui| model_cards(ui, &t, &models));
        });
        self.windows.about = open;
    }
}

impl ApsophosApp {
    /// The before/after preview: the export's tile at 1:1, Catmull-Rom left
    /// of the divider and the network right of it.
    fn ui_upscale_preview(&mut self, ctx: &egui::Context) {
        let te = &tr().export;
        let t = self.tokens;
        let mut open = self.windows.upscale_preview;
        let transform = self.doc.as_ref().map(|d| d.display.shared_transform());
        let denoised = self.doc.as_ref().is_some_and(crate::document::Document::is_complete);
        let (picking, at_point) = (self.picking_preview_point, self.preview_point.is_some());
        let mut pick = false;
        widgets::dialog(ctx, te.preview_title).open(&mut open).resizable(true).show(ctx, |ui| {
            // the point: the centre of the view by default, or one clicked
            let r = ui.add_enabled(denoised, egui::Button::new(te.preview_pick).selected(picking));
            pick = r.on_hover_text(te.preview_pick_tip).on_disabled_hover_text(te.preview_needs_denoise).clicked();
            ui.add_space(SPACE_S);
            match &mut self.preview {
                UpscalePreview::None => {}
                UpscalePreview::Computing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        let what = if at_point { te.preview_computing_point } else { te.preview_computing };
                        ui.label(weak(what, &t));
                    });
                }
                UpscalePreview::Failed(e) => {
                    ui.label(RichText::new(symbol::with(symbol::WARNING, (te.preview_failed)(e))).color(t.warning));
                }
                UpscalePreview::Ready { upscale, preview, textures, split } => {
                    let Some(transform) = &transform else { return };
                    let (before, after) = textures.get_or_insert_with(|| {
                        let load = |name: &str, img| {
                            ui.ctx().load_texture(name, crate::viewer::color_image(img, transform), Default::default())
                        };
                        (load("preview-before", &preview.classical), load("preview-after", &preview.network))
                    });
                    let factor = format!("{}×", upscale.factor.value());
                    let r = preview.rect;
                    let area = format!("x {}–{}, y {}–{}", r.x, r.right(), r.y, r.bottom());
                    ui.label(weak(format!("{} ← | → {} · {factor} · {area}", te.preview_before, te.preview_after), &t));
                    ui.add(Slider::new(split, 0.0..=1.0).show_value(false));
                    egui::ScrollArea::both().max_height(ui.ctx().content_rect().height() * 0.7).show(ui, |ui| {
                        let size = after.size_vec2();
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        ui.painter().image(after.id(), rect, uv, egui::Color32::WHITE);
                        let mut left = rect;
                        left.set_right(rect.left() + rect.width() * *split);
                        let left_uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(*split, 1.0));
                        ui.painter().image(before.id(), left, left_uv, egui::Color32::WHITE);
                        ui.painter().vline(left.right(), rect.y_range(), egui::Stroke::new(1.0, t.accent));
                    });
                }
            }
        });
        if pick {
            // the button again leaves the choice, as Esc does
            if picking {
                self.picking_preview_point = false;
            } else {
                self.pick_preview_point();
            }
        }
        if !open {
            // closed: the textures and the choice of a point go with it
            self.preview = UpscalePreview::None;
            self.picking_preview_point = false;
        }
        self.windows.upscale_preview = open;
    }
}

/// The Models section of the About window: for each network its card and
/// where its files are.
fn model_cards(ui: &mut egui::Ui, t: &Tokens, models: &[crate::model_list::ListedModel]) {
    let ta = &tr().about;
    for (n, m) in models.iter().enumerate() {
        if n > 0 {
            ui.add_space(SPACE_M);
        }
        ui.label(group_title(m.card.network, t));
        let files = if m.folders.is_empty() {
            ta.model_not_installed.to_owned()
        } else {
            m.folders
                .iter()
                .map(|(dir, names)| (ta.model_folder)(&dir.display().to_string(), &names.join(", ")))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let rows = [
            (ta.model_source, m.card.source.to_owned()),
            (ta.model_licence, m.card.licence.to_owned()),
            (ta.model_training, m.card.training_data.to_owned()),
            (ta.model_files, files),
        ];
        egui::Grid::new(("about-model", n)).num_columns(2).spacing(vec2(SPACE_M, SPACE_XS)).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(weak(k, t));
                ui.add(egui::Label::new(RichText::new(v).color(t.text)).wrap());
                ui.end_row();
            }
        });
    }
}

/// What the download part of the export settings asks for.
pub(super) enum DownloadAction {
    Start(Pack),
    Cancel,
}

/// The upscaling models still to download: a button with the size for the
/// faithful ones, and, once those are there, one for the generated-detail
/// ones (never both at once: generated detail is a choice of its own).
/// While a download runs: its progress and Cancel.
pub(super) fn download_block(
    ui: &mut egui::Ui,
    t: &Tokens,
    files: &[PathBuf],
    running: Option<(u64, u64)>,
) -> Option<DownloadAction> {
    let te = &tr().export;
    if let Some((done, total)) = running {
        ui.label(weak(te.downloading, t));
        #[expect(clippy::cast_precision_loss, reason = "a fraction for the bar")]
        let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        let text = (tr().status.download)(&format::bytes(done), &format::bytes(total));
        ui.add(egui::ProgressBar::new(fraction).text(text));
        return ui.button(te.cancel_download).clicked().then_some(DownloadAction::Cancel);
    }
    let missing = |pack| download::missing(pack, files).iter().map(|e| e.size).sum::<u64>();
    let (faithful, generated) = (missing(Pack::Faithful), missing(Pack::Generated));
    let (pack, label) = if faithful > 0 {
        (Pack::Faithful, (te.download_faithful)(&format::bytes(faithful)))
    } else if generated > 0 {
        (Pack::Generated, (te.download_generated)(&format::bytes(generated)))
    } else {
        return None;
    };
    let clicked = ui.button(label).clicked();
    ui.label(weak(te.download_note, t));
    clicked.then_some(DownloadAction::Start(pack))
}

/// The enlargement in the export settings: none, 2× or 4×, and faithful or
/// generated detail; what has no model on disk is disabled, with the reason.
/// Below: the size of the result for the open photo, how long it should take
/// at the speed `speed` measured for the choice, and the model's card.
fn upscale_choice(
    ui: &mut egui::Ui,
    t: &Tokens,
    choice: &mut Option<Upscale>,
    size: Option<(usize, usize)>,
    files: &[PathBuf],
    speed: &dyn Fn(Upscale) -> Option<denoise_engine::UpscaleSpeed>,
) {
    let te = &tr().export;
    let have = |u: Upscale| !denoise_engine::upscaler_files(files, u).is_empty();
    let detail = choice.map_or(Detail::Faithful, |u| u.detail);
    ui.label(weak(te.upscale, t));
    let mut factor = choice.map(|u| u.factor);
    ui.radio_value(&mut factor, None, te.upscale_none);
    for (f, label) in [(Factor::X2, te.upscale_x2), (Factor::X4, te.upscale_x4)] {
        let available = have(Upscale { factor: f, detail: Detail::Faithful });
        let r = ui.add_enabled(available, egui::RadioButton::new(factor == Some(f), label));
        let r = if available { r } else { r.on_disabled_hover_text(te.upscale_missing) };
        let r = if f == Factor::X4 { r.on_hover_text(te.upscale_x4_tip) } else { r };
        if r.clicked() {
            factor = Some(f);
        }
    }
    *choice = factor.map(|factor| Upscale { factor, detail });
    let Some(mut u) = *choice else { return };
    ui.horizontal(|ui| {
        ui.add_space(SPACE_M);
        ui.radio_value(&mut u.detail, Detail::Faithful, te.detail_faithful).on_hover_text(te.detail_faithful_tip);
        let generated = Upscale { detail: Detail::Generated, ..u };
        let r =
            ui.add_enabled(have(generated), egui::RadioButton::new(u.detail == Detail::Generated, te.detail_generated));
        if r.on_hover_text(te.detail_generated_tip).on_disabled_hover_text(te.upscale_missing).clicked() {
            u.detail = Detail::Generated;
        }
    });
    *choice = Some(u);
    if u.detail == Detail::Generated {
        ui.label(RichText::new(symbol::with(symbol::WARNING, te.detail_generated_tip)).color(t.warning));
    }
    if let Some((w, h)) = size {
        let f = u.factor.value();
        let text = format!("{} × {} px ({})", format::int(w * f), format::int(h * f), format::megapixels(w * f, h * f));
        ui.label(weak((te.upscale_result)(&text), t));
        let secs = speed(u).and_then(|s| s.estimate(w, h));
        ui.label(weak(format::upscale_estimate(secs), t));
    }
    let card = denoise_engine::card(u);
    ui.label(weak((te.upscale_model)(card.network, card.licence, card.training_data), t));
}
