//! Application state and user actions.
//!
//! `ApsophosApp` is the root of the ownership tree: it owns the document,
//! parameters, history, jobs and queue. The panels (in `ui`) read and modify
//! this state; heavy work goes through the [`Worker`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use denoise_core::{product, JobControl, Params, PixelRect};
use denoise_engine::{Device, TileProfile};
use denoise_io::{ExportOptions, FileFormat};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::document::Document;
use crate::download;
use crate::errors;
use crate::format::{self, file_name};
use crate::history::History;
use crate::logging;
use crate::settings::{Destination, Settings, TileHint, STORAGE_KEY};
use crate::text::{self, tr};
use crate::theme::{self, Tokens};
use crate::viewer::{ImagePos, ResultView, ViewState};
use crate::worker::{Command, ExportJob, ItemStatus, ModelConfig, Msg, UpscaleModels, Worker};

const fn shortcut(command: bool, shift: bool, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers { alt: false, ctrl: false, shift, mac_cmd: false, command }, key)
}

/// Shortcut as shown to the user ("Ctrl+O"), where no egui context is at
/// hand: the same text as `Context::format_shortcut` outside macOS.
pub fn shortcut_text(sc: &KeyboardShortcut) -> String {
    use egui::os::OperatingSystem;
    let is_mac = matches!(OperatingSystem::from_target_os(), OperatingSystem::Mac | OperatingSystem::IOS);
    sc.format(&egui::ModifierNames::NAMES, is_mac)
}

/// Name of the file dialog filter with every supported format.
fn images_filter() -> String {
    let labels: Vec<&str> = FileFormat::ALL.iter().map(|f| f.label()).collect();
    (tr().dialogs.images_filter)(&labels.join(", "))
}

pub const SC_OPEN: KeyboardShortcut = shortcut(true, false, Key::O);
pub const SC_EXPORT: KeyboardShortcut = shortcut(true, false, Key::E);
pub const SC_EXPORT_AS: KeyboardShortcut = shortcut(true, true, Key::E);
pub const SC_CLOSE: KeyboardShortcut = shortcut(true, false, Key::W);
pub const SC_QUIT: KeyboardShortcut = shortcut(true, false, Key::Q);
pub const SC_UNDO: KeyboardShortcut = shortcut(true, false, Key::Z);
pub const SC_REDO: KeyboardShortcut = shortcut(true, false, Key::Y);
pub const SC_REDO_ALT: KeyboardShortcut = shortcut(true, true, Key::Z);
pub const SC_DENOISE: KeyboardShortcut = shortcut(true, false, Key::R);
pub const SC_QUEUE: KeyboardShortcut = shortcut(true, false, Key::B);
// Ctrl with + / − / 0 zooms the photo, as image viewers do (egui's zoom of
// the whole interface is turned off in `theme::apply`)
pub const SC_ZOOM_IN: KeyboardShortcut = shortcut(true, false, Key::Plus);
pub const SC_ZOOM_IN_ALT: KeyboardShortcut = shortcut(true, false, Key::Equals);
pub const SC_ZOOM_OUT: KeyboardShortcut = shortcut(true, false, Key::Minus);
pub const SC_ZOOM_FIT: KeyboardShortcut = shortcut(true, false, Key::Num0);
/// Zoom factor of one step of + / − (and of their Ctrl versions).
const ZOOM_STEP: f32 = 1.25;
// single keys (Shift allowed), active when no text field has the focus
pub const SC_FIT: KeyboardShortcut = shortcut(false, false, Key::F);
pub const SC_ACTUAL: KeyboardShortcut = shortcut(false, false, Key::Num1);
pub const SC_STOP: KeyboardShortcut = shortcut(false, false, Key::Escape);
pub const SC_NOISE: KeyboardShortcut = shortcut(false, false, Key::N);
// the focus between the panes, as in Windows programs: always active
pub const SC_NEXT_PANE: KeyboardShortcut = shortcut(false, false, Key::F6);
pub const SC_PREVIOUS_PANE: KeyboardShortcut = shortcut(false, true, Key::F6);

/// Noise reduction of the open image.
pub enum Job {
    Idle,
    Running {
        doc_id: u64,
        /// Tiles completed in this job.
        done: usize,
        total: usize,
        started: Instant,
        control: Arc<JobControl>,
        order: Vec<usize>,
    },
}

/// State of the model in the worker.
pub enum ModelState {
    NotLoaded,
    Loading,
    Ready(String),
    Failed(String),
}

pub struct QueueItem {
    pub path: PathBuf,
    pub status: ItemStatus,
}

/// The before/after preview of the enlargement.
#[derive(Default)]
pub enum UpscalePreview {
    #[default]
    None,
    Computing,
    Ready {
        upscale: denoise_core::upscale::Upscale,
        preview: crate::upscaling::Preview,
        /// Made by the window on its first frame.
        textures: Option<(egui::TextureHandle, egui::TextureHandle)>,
        /// Where the divider stands, 0..=1 of the width (Catmull-Rom on the left).
        split: f32,
    },
    Failed(String),
}

/// A download of upscaling models, asked for by the user.
pub struct DownloadRun {
    pub control: Arc<JobControl>,
    /// Bytes done and in all.
    pub done: u64,
    pub total: u64,
}

/// An enlarged export of the open image, running on the model thread.
pub struct UpscaleRun {
    pub control: Arc<JobControl>,
    pub started: Instant,
    /// Tiles done and in all (0 of 0 while the model loads).
    pub done: usize,
    pub total: usize,
}

pub struct QueueRun {
    pub control: Arc<JobControl>,
    pub started: Instant,
    pub total: usize,
}

#[derive(Default)]
pub struct Queue {
    pub items: Vec<QueueItem>,
    pub running: Option<QueueRun>,
    /// Last clearing, for "Restore".
    pub cleared: Option<Vec<QueueItem>>,
}

impl Queue {
    pub fn pending(&self) -> usize {
        self.items.iter().filter(|i| matches!(i.status, ItemStatus::Queued | ItemStatus::Failed(_))).count()
    }

    pub fn finished(&self) -> usize {
        self.items.iter().filter(|i| matches!(i.status, ItemStatus::Done(_) | ItemStatus::Failed(_))).count()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Success,
    Error,
}

/// Remedies offered next to a notice.
#[derive(Clone, PartialEq, Eq)]
pub enum NoticeAction {
    OpenFile,
    Reveal(PathBuf),
    ChooseModel,
    RetryFailed,
    RestoreQueue,
    /// Downloads the faithful upscaling models.
    DownloadUpscalers,
    /// Never offers the upscaling models at start again.
    NoUpscalerOffer,
}

impl NoticeAction {
    pub fn label(&self) -> &'static str {
        let n = &tr().notices;
        match self {
            NoticeAction::OpenFile => n.open_another,
            NoticeAction::Reveal(_) => n.show_in_folder,
            NoticeAction::ChooseModel => n.choose_model,
            NoticeAction::RetryFailed => n.retry_failed,
            NoticeAction::RestoreQueue => n.restore,
            NoticeAction::DownloadUpscalers => n.download_models,
            NoticeAction::NoUpscalerOffer => n.no_offer,
        }
    }
}

/// Non-modal notice at the top of the image area.
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
    pub actions: Vec<NoticeAction>,
}

#[derive(Default)]
pub struct Windows {
    pub preferences: bool,
    pub export: bool,
    pub upscale_preview: bool,
    pub shortcuts: bool,
    pub about: bool,
}

/// What the worker has loaded, as reported by its last `ModelReady`.
struct LoadedModel {
    config: ModelConfig,
    tile_px: usize,
    /// Measured tile speeds (on a GPU), to pick the size for each image.
    profile: Option<TileProfile>,
}

/// Export requested while the network had not finished yet.
enum PendingExport {
    Auto,
    To(PathBuf, ExportOptions),
}

pub struct ApsophosApp {
    pub settings: Settings,
    pub tokens: Tokens,
    pub history: History,
    /// Parameters at the start of a slider drag.
    pub drag_origin: Option<Params>,
    pub doc: Option<Document>,
    next_doc_id: u64,
    pub opening: Option<PathBuf>,
    pub job: Job,
    pub model: ModelState,
    /// Model loaded in the worker.
    loaded: Option<LoadedModel>,
    /// Denoising requested while the model was still being prepared.
    pub denoise_waiting: bool,
    /// Result of the last tile-size search, shown in Advanced.
    pub tile_notes: Vec<String>,
    pub queue: Queue,
    pub worker: Worker,
    pub view: ViewState,
    pub notice: Option<Notice>,
    pub windows: Windows,
    pending_export: Option<PendingExport>,
    pub exporting: bool,
    /// Set while an enlarged export runs (with `exporting`).
    pub upscaling: Option<UpscaleRun>,
    pub preview: UpscalePreview,
    /// The upscaling models on disk, read when the export settings open
    /// (never on every frame).
    pub upscaler_files: Option<Vec<PathBuf>>,
    pub download: Option<DownloadRun>,
    /// Folders being read for the queue.
    pub scanning: usize,
    pub hover: Option<(usize, usize)>,
    pub show_original: bool,
    /// What the "after" side of the viewer shows.
    pub result_view: ResultView,
    /// Explanation under the primary button (e.g. what is missing).
    pub hint: Option<String>,
    title: String,
    /// Where F6 moves the focus.
    pub panes: crate::ui::PaneTargets,
    /// The networks and their files, read when the About window opens.
    pub model_list: Option<Vec<crate::model_list::ListedModel>>,
    /// A click on the photo will choose the point of the enlargement preview.
    pub picking_preview_point: bool,
    /// Where the last enlargement preview was asked for; `None`: the centre
    /// of the view.
    pub preview_point: Option<ImagePos>,
}

impl ApsophosApp {
    /// `files` are the paths given on the command line (or dropped onto the
    /// executable): one file is opened, several go to the queue.
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)).unwrap_or_default();
        text::set_language(settings.language);
        let tokens = Tokens::DARK;
        theme::apply(&cc.egui_ctx, &tokens);
        let mut app = Self {
            settings,
            tokens,
            history: History::default(),
            drag_origin: None,
            doc: None,
            next_doc_id: 1,
            opening: None,
            job: Job::Idle,
            model: ModelState::NotLoaded,
            loaded: None,
            denoise_waiting: false,
            tile_notes: Vec::new(),
            queue: Queue::default(),
            worker: Worker::spawn(&cc.egui_ctx),
            view: ViewState::default(),
            notice: None,
            windows: Windows::default(),
            pending_export: None,
            exporting: false,
            upscaling: None,
            preview: UpscalePreview::None,
            upscaler_files: None,
            download: None,
            scanning: 0,
            hover: None,
            show_original: false,
            result_view: ResultView::Denoised,
            hint: None,
            title: String::new(),
            panes: crate::ui::PaneTargets::default(),
            model_list: None,
            picking_preview_point: false,
            preview_point: None,
        };
        app.offer_upscalers();
        match files.as_slice() {
            [] => {}
            [one] if one.is_file() => app.open_path(one.clone()),
            _ => app.add_to_queue(files),
        }
        app
    }

    /// At start, when no upscaling model is installed: a notice that says
    /// what they are for and offers the button that downloads them (nothing
    /// is downloaded without it), and one to stop offering them.
    fn offer_upscalers(&mut self) {
        if self.settings.no_upscaler_offer
            || !download::none_installed(download::Pack::Faithful, &self.settings.upscaler_files())
        {
            return;
        }
        let size = format::bytes(download::pack_bytes(download::Pack::Faithful));
        let actions = vec![NoticeAction::DownloadUpscalers, NoticeAction::NoUpscalerOffer];
        self.notify(NoticeKind::Info, (tr().notices.upscalers_offer)(&size), actions);
    }

    /// Downloads the models of `pack` that are missing, in the background.
    pub fn start_download(&mut self, pack: download::Pack) {
        if self.download.is_some() {
            return;
        }
        let entries = download::missing(pack, &self.settings.upscaler_files());
        if entries.is_empty() {
            self.notify(NoticeKind::Success, tr().notices.models_present, vec![]);
            return;
        }
        let Some(dir) = download::download_dir() else {
            let why = tr().errors.not_found;
            self.notify(NoticeKind::Error, (tr().notices.download_failed)(why), vec![]);
            return;
        };
        let control = Arc::new(JobControl::default());
        let total = entries.iter().map(|e| e.size).sum();
        self.download = Some(DownloadRun { control: Arc::clone(&control), done: 0, total });
        self.worker.download(entries, dir, control);
    }

    pub fn cancel_download(&mut self) {
        if let Some(run) = &self.download {
            run.control.cancel();
        }
    }

    /// `sc` if it is a single-key shortcut that is turned on, for showing
    /// it next to a command; `None` when single keys are off.
    pub fn single_key(&self, sc: &'static KeyboardShortcut) -> Option<&'static KeyboardShortcut> {
        (!self.settings.single_keys_off).then_some(sc)
    }

    pub fn is_running(&self) -> bool {
        matches!(self.job, Job::Running { .. })
    }

    pub fn queue_running(&self) -> bool {
        self.queue.running.is_some()
    }

    fn notify(&mut self, kind: NoticeKind, text: impl Into<String>, actions: Vec<NoticeAction>) {
        self.notice = Some(Notice { kind, text: text.into(), actions });
    }

    // ---------------------------------------------------------- opening

    pub fn open_dialog(&mut self) {
        let d = &tr().dialogs;
        let mut dlg = rfd::FileDialog::new()
            .set_title(d.open_image)
            .add_filter(images_filter(), &FileFormat::all_extensions())
            .add_filter(d.all_files, &["*"]);
        if let Some(dir) = &self.settings.last_open_dir {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.open_path(path);
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        if denoise_io::detect(&path).is_none() {
            let n = &tr().notices;
            let why = if path.exists() { n.not_an_image } else { n.file_gone };
            self.notify(NoticeKind::Error, (n.cannot_open)(&file_name(&path), why), vec![NoticeAction::OpenFile]);
            self.settings.recent.retain(|p| p != &path);
            return;
        }
        self.opening = Some(path.clone());
        self.worker.open(path);
    }

    pub fn close_image(&mut self) {
        self.cancel_denoise();
        self.doc = None;
        self.pending_export = None;
        self.hover = None;
        self.picking_preview_point = false;
    }

    // --------------------------------------------------------- processing

    fn model_config(&mut self) -> Option<ModelConfig> {
        let models = self.settings.model_set();
        match models {
            m if !m.is_empty() => {
                let engine = self.settings.engine;
                let profile = self
                    .settings
                    .tile_hint
                    .as_ref()
                    .filter(|h| h.device == engine.device && h.models == m)
                    .and_then(|h| h.profile.clone());
                Some(ModelConfig { models: m, engine, profile, image_px: None })
            }
            _ => {
                self.notify(NoticeKind::Error, tr().notices.model_missing, vec![NoticeAction::ChooseModel]);
                None
            }
        }
    }

    /// Starts (or completes) noise reduction of the open image.
    pub fn start_denoise(&mut self) {
        self.start_denoise_with(false);
    }

    /// With `accept_loaded`, uses the model just loaded whatever its tile
    /// size, so that a disagreement with the worker cannot loop.
    fn start_denoise_with(&mut self, accept_loaded: bool) {
        self.hint = None;
        if self.is_running() {
            return;
        }
        if let Some(why) = format::denoise_blocker(self.queue_running(), self.doc.is_some(), &shortcut_text(&SC_OPEN)) {
            self.hint = Some(why);
            return;
        }
        let Some(model) = self.model_config() else { return };
        let size = self.doc.as_ref().map(|d| (d.width(), d.height()));
        let for_image = size.map_or_else(|| model.clone(), |s| model.for_image(s));
        // the tile size depends on the device (its speed and free memory) and
        // on the image size: the worker decides it while loading the model,
        // then denoising restarts
        let tile_px = match &self.loaded {
            Some(l)
                if l.config == model
                    && (accept_loaded || for_image.wanted_tile(l.tile_px, l.profile.as_ref()) == l.tile_px) =>
            {
                l.tile_px
            }
            _ => {
                self.worker.send(Command::Prepare { model: for_image });
                self.model = ModelState::Loading;
                self.denoise_waiting = true;
                return;
            }
        };
        if let Some(Err(e)) = self.doc.as_mut().map(|d| d.use_tile(tile_px)) {
            self.notify(NoticeKind::Error, (tr().notices.denoise_failed)(&errors::denoise(&e)), vec![]);
            return;
        }
        let Some(doc) = &self.doc else { return };
        let center = self.view.center();
        let order = doc.pending_order(Some((center.x, center.y)));
        if order.is_empty() {
            return;
        }
        let (doc_id, image, info, plan) = (doc.id, Arc::clone(&doc.original), Arc::clone(&doc.info), doc.plan.clone());
        let control = Arc::new(JobControl::default());
        self.worker.send(Command::Denoise {
            doc_id,
            model,
            image,
            info,
            plan,
            order: order.clone(),
            control: Arc::clone(&control),
        });
        self.job = Job::Running { doc_id, done: 0, total: order.len(), started: Instant::now(), control, order };
    }

    pub fn cancel_denoise(&mut self) {
        if let Job::Running { control, .. } = &self.job {
            control.cancel();
        }
        self.pending_export = None;
    }

    /// Esc / Stop: stops whatever is running.
    pub fn stop_all(&mut self) {
        self.cancel_denoise();
        self.cancel_export();
        self.cancel_download();
        if let Some(run) = &self.queue.running {
            run.control.cancel();
        }
    }

    /// Network tile being processed, to highlight in the view.
    pub fn working_on(&self) -> Option<PixelRect> {
        match (&self.job, &self.doc) {
            (Job::Running { doc_id, done, order, .. }, Some(doc)) if *doc_id == doc.id => {
                order.get(*done).map(|&i| doc.plan.rect(i))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ export

    /// Stops an enlarged export: no file is written.
    pub fn cancel_export(&mut self) {
        if let Some(run) = &self.upscaling {
            run.control.cancel();
        }
    }

    /// Enlarges the export's tile at `point`, or at the centre of the view
    /// when `None`, for the before/after preview (the photo must be
    /// denoised: the preview shows it as it will be exported).
    pub fn preview_upscale(&mut self, point: Option<ImagePos>) {
        self.picking_preview_point = false;
        self.preview_point = point;
        let Some(upscale) = self.settings.export.upscale else { return };
        let models = self.upscale_models();
        let Some(doc) = self.doc.as_ref().filter(|d| d.is_complete()) else { return };
        let at = point.unwrap_or_else(|| self.view.center()).pixel(doc.width(), doc.height());
        let job = ExportJob {
            path: doc.path.clone(),
            original: Arc::clone(&doc.original),
            raw: Arc::clone(&doc.raw),
            alpha: None,
            info: Arc::clone(&doc.info),
            params: self.settings.params,
            options: self.settings.export.options,
        };
        self.preview = UpscalePreview::Computing;
        self.windows.upscale_preview = true;
        self.worker.send(Command::PreviewUpscale { doc_id: doc.id, job, at, upscale, models });
    }

    /// The next click on the photo chooses where the enlargement preview is
    /// taken (Esc cancels).
    pub fn pick_preview_point(&mut self) {
        self.picking_preview_point = self.doc.is_some();
    }

    /// The upscaling models and the device, for the worker.
    fn upscale_models(&self) -> UpscaleModels {
        UpscaleModels { files: self.settings.upscaler_files(), engine: self.settings.engine }
    }

    /// Exports with the last settings into the remembered folder (Ctrl+E).
    pub fn export(&mut self) {
        self.export_to(None);
    }

    fn export_to(&mut self, target: Option<(PathBuf, ExportOptions)>) {
        self.hint = None;
        let Some(complete) = self.doc.as_ref().map(Document::is_complete) else {
            self.hint = Some((tr().notices.open_first)(&shortcut_text(&SC_OPEN)));
            return;
        };
        if self.exporting {
            return;
        }
        if !complete {
            self.pending_export = Some(match target {
                Some((p, o)) => PendingExport::To(p, o),
                None => PendingExport::Auto,
            });
            if !self.is_running() && !self.denoise_waiting {
                self.start_denoise();
            }
            if self.is_running() || self.denoise_waiting {
                self.hint = Some(tr().notices.export_after_denoise.into());
            } else {
                // noise reduction did not start (missing model, queue running…)
                self.pending_export = None;
            }
            return;
        }
        let upscale = self.settings.export.upscale;
        let models = upscale.map(|_| self.upscale_models());
        if let (Some(u), Some(m)) = (upscale, &models) {
            if denoise_engine::upscaler_files(&m.files, u).is_empty() {
                self.hint = Some(tr().export.upscale_missing.into());
                return;
            }
        }
        let Some(doc) = &self.doc else { return };
        let export = &self.settings.export;
        let (path, options) = target.unwrap_or_else(|| {
            let (format, _) = denoise_io::resolve(&doc.info, &export.options);
            let dir = export.dir_for(&doc.path);
            (denoise_io::output_path(&doc.path, &dir, &export.full_suffix(), format.extension()), export.options)
        });
        let job = ExportJob {
            path,
            original: Arc::clone(&doc.original),
            raw: Arc::clone(&doc.raw),
            alpha: doc.alpha.clone(),
            info: Arc::clone(&doc.info),
            params: self.settings.params,
            options,
        };
        self.exporting = true;
        match (upscale, models) {
            (Some(upscale), Some(models)) => {
                let control = Arc::new(JobControl::default());
                self.upscaling =
                    Some(UpscaleRun { control: Arc::clone(&control), started: Instant::now(), done: 0, total: 0 });
                self.worker.send(Command::ExportUpscaled { job, upscale, models, control });
            }
            _ => self.worker.export(job),
        }
    }

    /// Exports choosing name, folder and format (Ctrl+Shift+E).
    pub fn export_as(&mut self) {
        let Some(doc) = &self.doc else {
            self.hint = Some((tr().notices.open_first)(&shortcut_text(&SC_OPEN)));
            return;
        };
        let export = &self.settings.export;
        let (format, _) = denoise_io::resolve(&doc.info, &export.options);
        let dir = export.dir_for(&doc.path);
        let suggested = denoise_io::output_path(&doc.path, &dir, &export.full_suffix(), format.extension());
        let mut dlg = rfd::FileDialog::new()
            .set_title(tr().dialogs.export_image)
            .set_directory(&dir)
            .set_file_name(file_name(&suggested));
        for f in FileFormat::ALL {
            dlg = dlg.add_filter(f.label(), f.extensions());
        }
        let Some(path) = dlg.save_file() else {
            return;
        };
        let (path, format) = denoise_io::export_choice(&path, doc.info.format);
        let options = ExportOptions { format, ..self.settings.export.options };
        if let Some(parent) = path.parent() {
            self.settings.export.destination = Destination::Folder(parent.to_path_buf());
        }
        self.export_to(Some((path, options)));
    }

    // ------------------------------------------------------------- queue

    /// Adds files and folders (not recursively) to the queue; the folders
    /// are read by the worker, the files arrive with [`Msg::Scanned`].
    pub fn add_to_queue(&mut self, paths: Vec<PathBuf>) {
        self.settings.show_queue = true;
        self.scanning += 1;
        self.worker.scan(paths);
    }

    fn queue_scanned(&mut self, images: Vec<PathBuf>, skipped: usize) {
        self.scanning = self.scanning.saturating_sub(1);
        let (mut added, mut already) = (0, 0);
        for f in images {
            if self.queue.items.iter().any(|i| i.path == f) {
                already += 1;
            } else {
                self.queue.items.push(QueueItem { path: f, status: ItemStatus::Queued });
                added += 1;
            }
        }
        if let Some(text) = (tr().notices.added_to_queue)(added, already, skipped) {
            self.notify(NoticeKind::Info, text, vec![]);
        }
    }

    pub fn add_files_dialog(&mut self) {
        let d = &tr().dialogs;
        let mut dlg =
            rfd::FileDialog::new().set_title(d.add_to_queue).add_filter(images_filter(), &FileFormat::all_extensions());
        if let Some(dir) = &self.settings.last_open_dir {
            dlg = dlg.set_directory(dir);
        }
        if let Some(files) = dlg.pick_files() {
            self.add_to_queue(files);
        }
    }

    pub fn add_folder_dialog(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().set_title(tr().dialogs.add_folder).pick_folder() {
            self.add_to_queue(vec![dir]);
        }
    }

    pub fn start_queue(&mut self) {
        self.hint = None;
        if self.queue_running() {
            return;
        }
        if self.is_running() {
            self.notify(NoticeKind::Info, tr().notices.queue_wait, vec![]);
            return;
        }
        let items: Vec<(usize, PathBuf)> = self
            .queue
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i.status, ItemStatus::Queued | ItemStatus::Failed(_)))
            .map(|(n, i)| (n, i.path.clone()))
            .collect();
        if items.is_empty() {
            return;
        }
        let Some(model) = self.model_config() else { return };
        for (n, _) in &items {
            self.queue.items[*n].status = ItemStatus::Queued;
        }
        let control = Arc::new(JobControl::default());
        self.queue.running =
            Some(QueueRun { control: Arc::clone(&control), started: Instant::now(), total: items.len() });
        let upscalers = self.upscale_models();
        self.worker.send(Command::Batch {
            model,
            upscalers,
            items,
            params: self.settings.params,
            export: self.settings.export.clone(),
            control,
        });
    }

    pub fn toggle_queue_pause(&mut self) {
        if let Some(run) = &self.queue.running {
            run.control.set_paused(!run.control.is_paused());
        }
    }

    pub fn clear_queue(&mut self) {
        if self.queue_running() || self.queue.items.is_empty() {
            return;
        }
        let n = self.queue.items.len();
        self.queue.cleared = Some(std::mem::take(&mut self.queue.items));
        self.notify(NoticeKind::Info, (tr().notices.queue_cleared)(n), vec![NoticeAction::RestoreQueue]);
    }

    pub fn remove_from_queue(&mut self, index: usize) {
        if !self.queue_running() && index < self.queue.items.len() {
            self.queue.items.remove(index);
        }
    }

    // ------------------------------------------------------------- other

    pub fn choose_model(&mut self) {
        let d = &tr().dialogs;
        if let Some(path) =
            rfd::FileDialog::new().set_title(d.choose_model).add_filter(d.model_filter, &["onnx"]).pick_file()
        {
            self.settings.model_path = Some(path);
            self.model = ModelState::NotLoaded;
            self.loaded = None;
            self.notice = None;
        }
    }

    /// Switches the "after" side between the result and the removed noise.
    pub fn toggle_removed_noise(&mut self) {
        self.result_view = match self.result_view {
            ResultView::Denoised => ResultView::RemovedNoise,
            ResultView::RemovedNoise => ResultView::Denoised,
        };
    }

    /// Opens the folder of the log files.
    pub fn open_log_folder() {
        if let Some(dir) = logging::folder() {
            // nothing useful to do if the file manager does not open
            let _ = opener::open(dir);
        }
    }

    /// Forgets the remembered tile size: the next job searches it again.
    pub fn redetect_tiles(&mut self) {
        self.settings.tile_hint = None;
        self.loaded = None;
        self.tile_notes.clear();
        self.model = ModelState::NotLoaded;
        self.worker.send(Command::Unload);
    }

    pub fn undo(&mut self) {
        self.history.undo(&mut self.settings.params);
    }

    pub fn redo(&mut self) {
        self.history.redo(&mut self.settings.params);
    }

    /// Changes the parameters, recording the action in the history.
    pub fn set_params(&mut self, params: Params, what: impl Into<String>) {
        self.history.push_params(self.settings.params, params, what);
        self.settings.params = params;
    }

    pub fn reveal(path: &Path) {
        // if the file manager does not open there is nothing useful to do
        let _ = opener::reveal(path);
    }

    pub fn run_notice_action(&mut self, action: NoticeAction) {
        self.notice = None;
        match action {
            NoticeAction::OpenFile => self.open_dialog(),
            NoticeAction::Reveal(p) => Self::reveal(&p),
            NoticeAction::ChooseModel => self.choose_model(),
            NoticeAction::RetryFailed => self.start_queue(),
            NoticeAction::DownloadUpscalers => self.start_download(download::Pack::Faithful),
            NoticeAction::NoUpscalerOffer => self.settings.no_upscaler_offer = true,
            NoticeAction::RestoreQueue => {
                if let Some(mut items) = self.queue.cleared.take() {
                    items.append(&mut self.queue.items);
                    self.queue.items = items;
                }
            }
        }
    }

    // ---------------------------------------------------------- messages

    fn on_message(&mut self, msg: Msg) {
        match msg {
            Msg::Opened { path, result } => {
                if self.opening.as_ref() == Some(&path) {
                    self.opening = None;
                }
                let opened = result.and_then(|picture| {
                    let notes = picture.info.notes.join(" ");
                    Document::new(self.next_doc_id, path.clone(), picture)
                        .map(|doc| (doc, notes))
                        .map_err(|e| errors::denoise(&e))
                });
                match opened {
                    Ok((doc, notes)) => {
                        self.cancel_denoise();
                        self.next_doc_id += 1;
                        self.doc = Some(doc);
                        self.view.request_fit();
                        self.settings.push_recent(&path);
                        self.notice = (!notes.is_empty()).then(|| Notice {
                            kind: NoticeKind::Info,
                            text: notes,
                            actions: vec![],
                        });
                    }
                    Err(e) => self.notify(
                        NoticeKind::Error,
                        (tr().notices.cannot_open)(&file_name(&path), &e),
                        vec![NoticeAction::OpenFile],
                    ),
                }
            }
            Msg::ModelLoading => self.model = ModelState::Loading,
            Msg::ModelReady { config, device, tile_px, tile_notes, profile, on_gpu, gpu_fallback } => {
                self.model = ModelState::Ready((tr().params.model_ready)(&device, tile_px));
                if on_gpu && profile.is_some() {
                    self.settings.tile_hint = Some(TileHint {
                        device: config.engine.device,
                        models: config.models.clone(),
                        profile: profile.clone(),
                    });
                }
                self.tile_notes = tile_notes;
                // a reply to Prepare with nothing reloaded repeats no notice
                let reloaded = self.loaded.as_ref().is_none_or(|l| l.config != config || l.tile_px != tile_px);
                let device_choice = config.engine.device;
                self.loaded = Some(LoadedModel { config, tile_px, profile });
                if std::mem::take(&mut self.denoise_waiting) {
                    self.start_denoise_with(true);
                }
                match gpu_fallback {
                    Some(why) if reloaded && !on_gpu => {
                        self.notify(NoticeKind::Info, (tr().notices.gpu_unused)(&errors::gpu(&why)), vec![])
                    }
                    // Auto quietly takes the next GPU back end (shown in Advanced);
                    // a back end chosen by hand that fails is worth saying
                    Some(why) if reloaded && device_choice != Device::Auto => {
                        self.notify(NoticeKind::Info, (tr().notices.device_unused)(&errors::gpu(&why), &device), vec![])
                    }
                    _ => {}
                }
            }
            Msg::ModelFailed(e) => {
                self.model = ModelState::Failed(e.clone());
                self.loaded = None;
                self.denoise_waiting = false;
                self.pending_export = None;
                self.notify(NoticeKind::Error, (tr().notices.model_failed)(&e), vec![NoticeAction::ChooseModel]);
            }
            Msg::Tile { doc_id, index, done, total, rect, pixels } => {
                if let Some(doc) = self.doc.as_mut().filter(|d| d.id == doc_id) {
                    doc.accept_tile(index, rect, &pixels);
                }
                if let Job::Running { doc_id: id, done: d, total: t, .. } = &mut self.job {
                    if *id == doc_id {
                        *d = done;
                        *t = total;
                    }
                }
            }
            Msg::Denoised { doc_id, result } => {
                if matches!(&self.job, Job::Running { doc_id: id, .. } if *id == doc_id) {
                    self.job = Job::Idle;
                }
                let current = self.doc.as_ref().is_some_and(|d| d.id == doc_id);
                match result {
                    Ok(()) if current => {
                        self.hint = None;
                        match self.pending_export.take() {
                            Some(PendingExport::Auto) => self.export_to(None),
                            Some(PendingExport::To(p, o)) => self.export_to(Some((p, o))),
                            None => {}
                        }
                    }
                    Ok(()) | Err(None) => self.pending_export = None,
                    Err(Some(e)) => {
                        self.pending_export = None;
                        self.notify(NoticeKind::Error, (tr().notices.denoise_failed)(&e), vec![]);
                    }
                }
            }
            Msg::UpscalerReady { device, gpu_fallback } => {
                tracing::info!("upscaler on {device}");
                // never a silent fallback: the CPU is many times slower
                if let Some(why) = gpu_fallback.filter(|_| self.settings.engine.device != Device::Cpu) {
                    self.notify(NoticeKind::Info, (tr().notices.upscaler_on_cpu)(&errors::gpu(&why)), vec![]);
                }
            }
            Msg::UpscalePreview { doc_id, upscale, result } => {
                if self.doc.as_ref().is_some_and(|d| d.id == doc_id) {
                    self.preview = match result {
                        Ok(preview) => UpscalePreview::Ready { upscale, preview, textures: None, split: 0.5 },
                        Err(e) => UpscalePreview::Failed(e),
                    };
                }
            }
            Msg::DownloadProgress { done, total } => {
                if let Some(run) = &mut self.download {
                    (run.done, run.total) = (done, total);
                }
            }
            Msg::Downloaded(result) => {
                self.download = None;
                // read the model folders again: the choices change
                self.upscaler_files = None;
                match result {
                    Ok(()) => self.notify(NoticeKind::Success, tr().notices.models_downloaded, vec![]),
                    Err(None) => self.notify(NoticeKind::Info, tr().notices.download_cancelled, vec![]),
                    Err(Some(e)) => self.notify(
                        NoticeKind::Error,
                        (tr().notices.download_failed)(&e),
                        vec![NoticeAction::DownloadUpscalers],
                    ),
                }
            }
            Msg::UpscaleMeasured { upscale, device, speed } => {
                self.settings.remember_upscale_speed(upscale, device, speed);
            }
            Msg::ExportProgress { done, total } => {
                if let Some(run) = &mut self.upscaling {
                    (run.done, run.total) = (done, total);
                }
            }
            Msg::Exported(result) => {
                self.exporting = false;
                self.upscaling = None;
                self.hint = None;
                match result {
                    Ok(path) => self.notify(
                        NoticeKind::Success,
                        (tr().notices.exported)(&file_name(&path)),
                        vec![NoticeAction::Reveal(path)],
                    ),
                    Err(None) => self.notify(NoticeKind::Info, tr().notices.export_cancelled, vec![]),
                    Err(Some(e)) => self.notify(NoticeKind::Error, (tr().notices.export_failed)(&e), vec![]),
                }
            }
            Msg::BatchItem { index, status } => {
                if let Some(item) = self.queue.items.get_mut(index) {
                    item.status = status;
                }
            }
            Msg::Scanned { images, skipped } => self.queue_scanned(images, skipped),
            Msg::BatchFinished { cancelled } => {
                self.queue.running = None;
                self.batch_summary(cancelled);
            }
            Msg::Crashed(e) => {
                self.scanning = 0;
                self.job = Job::Idle;
                self.loaded = None;
                self.denoise_waiting = false;
                self.queue.running = None;
                self.exporting = false;
                self.upscaling = None;
                self.opening = None;
                self.notify(NoticeKind::Error, (tr().notices.internal_error)(&e), vec![]);
            }
        }
    }

    fn batch_summary(&mut self, cancelled: bool) {
        let ok: Vec<&PathBuf> = self
            .queue
            .items
            .iter()
            .filter_map(|i| if let ItemStatus::Done(p) = &i.status { Some(p) } else { None })
            .collect();
        let failed = self.queue.items.iter().filter(|i| matches!(i.status, ItemStatus::Failed(_))).count();
        let last = ok.last().map(|p| (*p).clone());
        let text = (tr().notices.queue_summary)(cancelled, ok.len(), failed);
        let mut actions = Vec::new();
        if let Some(p) = last {
            actions.push(NoticeAction::Reveal(p));
        }
        if failed > 0 {
            actions.push(NoticeAction::RetryFailed);
        }
        let kind = if failed > 0 { NoticeKind::Error } else { NoticeKind::Success };
        self.notify(kind, text, actions);
    }

    // ---------------------------------------------------------- keyboard

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |sc: &KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(sc));
        // Shift combinations first: Ctrl+E also matches Ctrl+Shift+E
        if pressed(&SC_EXPORT_AS) {
            self.export_as();
        }
        if pressed(&SC_REDO_ALT) || pressed(&SC_REDO) {
            self.redo();
        }
        if pressed(&SC_OPEN) {
            self.open_dialog();
        }
        if pressed(&SC_EXPORT) {
            self.export();
        }
        if pressed(&SC_CLOSE) {
            self.close_image();
        }
        if pressed(&SC_QUIT) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if pressed(&SC_UNDO) {
            self.undo();
        }
        if pressed(&SC_DENOISE) {
            self.start_denoise();
        }
        if pressed(&SC_QUEUE) {
            self.start_queue();
        }
        let (zoom_in, zoom_out) = (pressed(&SC_ZOOM_IN) || pressed(&SC_ZOOM_IN_ALT), pressed(&SC_ZOOM_OUT));
        if zoom_in || zoom_out {
            let factor = if zoom_in { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
            self.view.zoom_by(factor, self.view.canvas_center());
        }
        if pressed(&SC_ZOOM_FIT) {
            self.view.request_fit();
        }
        // Shift+F6 first: F6 also matches it
        if pressed(&SC_PREVIOUS_PANE) {
            self.focus_next_pane(ctx, true);
        }
        if pressed(&SC_NEXT_PANE) {
            self.focus_next_pane(ctx, false);
        }
        if let Some(back) = self.panes.requested.take() {
            self.focus_next_pane(ctx, back);
        }

        // single keys: only when not typing in a field
        if ctx.egui_wants_keyboard_input() {
            self.show_original = false;
            return;
        }
        let nothing_focused = ctx.memory(|m| m.focused().is_none()) || self.panes.photo_focused(ctx);
        let single_keys = !self.settings.single_keys_off;
        let (keys, hold, shift) = ctx.input(|i| {
            let plain = i.modifiers.is_none() || i.modifiers == Modifiers::SHIFT;
            let k = |key| single_keys && plain && i.key_pressed(key);
            (
                [
                    k(SC_FIT.logical_key),
                    k(SC_ACTUAL.logical_key),
                    k(Key::Plus) || k(Key::Equals),
                    k(Key::Minus),
                    i.key_pressed(SC_STOP.logical_key),
                    k(SC_NOISE.logical_key),
                ],
                (single_keys && i.key_down(Key::Backslash)) || (nothing_focused && i.key_down(Key::Space)),
                i.modifiers.shift,
            )
        });
        let [fit, actual, zoom_in, zoom_out, escape, noise] = keys;
        let center = self.view.canvas_center();
        if fit {
            self.view.request_fit();
        }
        if actual {
            self.view.actual_size(None);
        }
        if zoom_in {
            self.view.zoom_by(ZOOM_STEP, center);
        }
        if zoom_out {
            self.view.zoom_by(1.0 / ZOOM_STEP, center);
        }
        if escape {
            // Esc leaves the choice of a point first, then stops the work
            if self.picking_preview_point {
                self.picking_preview_point = false;
            } else {
                self.stop_all();
            }
        }
        if noise && self.doc.is_some() {
            self.toggle_removed_noise();
        }
        self.show_original = hold && self.doc.is_some();

        let step = if shift { 200.0 } else { 20.0 };
        let arrows =
            ctx.input(|i| [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown].map(|k| i.key_pressed(k)));
        if nothing_focused {
            let delta = egui::vec2(
                (f32::from(u8::from(arrows[0])) - f32::from(u8::from(arrows[1]))) * step,
                (f32::from(u8::from(arrows[2])) - f32::from(u8::from(arrows[3]))) * step,
            );
            if delta != egui::Vec2::ZERO {
                self.view.pan(delta);
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        match dropped.as_slice() {
            [] => {}
            [one] if one.is_file() && !self.queue_running() => self.open_path(one.clone()),
            _ => self.add_to_queue(dropped),
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.doc {
            Some(d) => format!("{} — {}", file_name(&d.path), product::NAME),
            None => product::NAME.to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for ApsophosApp {
    /// Worker messages, keys, dropped files and the title: everything that
    /// changes the state before the frame is drawn.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for msg in self.worker.drain() {
            self.on_message(msg);
        }
        self.handle_shortcuts(ctx);
        self.handle_drops(ctx);
        self.update_title(ctx);
        // while processing, the time estimate changes even without messages
        if self.is_running() || self.queue_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.settings);
    }
}
