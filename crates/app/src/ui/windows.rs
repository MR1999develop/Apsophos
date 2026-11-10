//! Windows: preferences, export settings, keyboard shortcuts, about.

use denoise_core::product;
use denoise_io::ExportFormat;
use eframe::egui::{self, vec2, KeyboardShortcut, RichText, Slider};

use crate::app::*;
use crate::format::truncate_middle;
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
            self.model_list.get_or_insert_with(|| crate::model_list::list(&settings.denoiser_files())).clone()
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
