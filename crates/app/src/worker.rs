//! Background work: the interface never does I/O or computation.
//!
//! * A "model" thread owns the ONNX sessions (expensive to load) and runs, in
//!   order, noise reduction of the open image, its enlarged exports and the
//!   queue. It keeps one network at a time in memory, the denoiser or an
//!   upscaler, so the two never share a GPU's memory.
//! * Opening and exporting a file run in separate short-lived threads, so
//!   another image can be opened while the network is working.
//!
//! Results come back as [`Msg`]s on a channel; every send asks egui to
//! repaint. A panic in a job becomes an error message instead of bringing the
//! application down.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use denoise_core::upscale::Upscale;
use denoise_core::{
    blend, run_tiles, DenoiseError, Image, JobControl, Params, PixelRect, TileModel, TileOutput, TilePlan,
};
use denoise_engine::{EngineOptions, Scunet, TileProfile, Upscaler};
use denoise_io::{ExportOptions, Picture, ReadLimits, SourceInfo};
use eframe::egui;

use crate::download;
use crate::errors;
use crate::network_space::with_network_space;
use crate::resources;
use crate::settings::ExportSettings;
use crate::text::tr;
use crate::upscaling::{self, ExportError};

/// Model to use: if it changes, the worker reloads it.
#[derive(Clone, Debug)]
pub struct ModelConfig {
    /// The same network with different tile sizes; the fastest that fits on
    /// the device is used.
    pub models: Vec<PathBuf>,
    pub engine: EngineOptions,
    /// Tile speeds measured by an earlier search, to skip searching again.
    /// Not part of the identity of the configuration.
    pub profile: Option<TileProfile>,
    /// Size of the image about to be processed: the worker loads the tile
    /// size that is fastest for it (switching size if worthwhile). `None`:
    /// keep the size already loaded. Not part of the identity either.
    pub image_px: Option<(usize, usize)>,
}

impl ModelConfig {
    /// The same configuration, for an image of `size`.
    pub fn for_image(&self, size: (usize, usize)) -> Self {
        Self { image_px: Some(size), ..self.clone() }
    }

    /// The tile size the worker will want for this configuration's image,
    /// given the size `current` already loaded with `profile`.
    pub fn wanted_tile(&self, current: usize, profile: Option<&TileProfile>) -> usize {
        match (self.image_px, profile) {
            (Some(size), Some(p)) => p.best(Some(size), Some(current)).unwrap_or(current),
            _ => current,
        }
    }
}

impl PartialEq for ModelConfig {
    fn eq(&self, other: &Self) -> bool {
        self.models == other.models && self.engine == other.engine
    }
}

/// Status of a queued file.
/// The upscaling models on disk and the device to run them on.
#[derive(Clone, Debug, PartialEq)]
pub struct UpscaleModels {
    /// The files of the model folder; the engine picks those it needs.
    pub files: Vec<PathBuf>,
    pub engine: EngineOptions,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemStatus {
    Queued,
    Running { done: usize, total: usize },
    Done(PathBuf),
    Failed(String),
}

pub enum Command {
    /// Loads the model (choosing the tile size) without processing anything.
    Prepare { model: ModelConfig },
    /// Frees the model, so that the next job loads it (and searches the tile
    /// size) again.
    Unload,
    /// Computes the network output for the tiles `order` of the open image.
    Denoise {
        doc_id: u64,
        model: ModelConfig,
        image: Arc<Image>,
        /// The image's file information: its colour profile decides what
        /// the network sees.
        info: Arc<SourceInfo>,
        plan: TilePlan,
        order: Vec<usize>,
        control: Arc<JobControl>,
    },
    /// Exports the open image enlarged: the upscaler runs here, one row of
    /// tiles at a time, straight into the file.
    ExportUpscaled { job: ExportJob, upscale: Upscale, models: UpscaleModels, control: Arc<JobControl> },
    /// Enlarges the tile of the export that holds the pixel `at` of the open
    /// image, for the before/after preview.
    PreviewUpscale { doc_id: u64, job: ExportJob, at: (usize, usize), upscale: Upscale, models: UpscaleModels },
    /// Processes and exports a series of files (enlarged if `export` says so).
    Batch {
        model: ModelConfig,
        upscalers: UpscaleModels,
        items: Vec<(usize, PathBuf)>,
        params: Params,
        export: ExportSettings,
        control: Arc<JobControl>,
    },
}

pub enum Msg {
    Opened {
        path: PathBuf,
        result: Result<Picture, String>,
    },
    /// Images found among files and folders added to the queue.
    Scanned {
        images: Vec<PathBuf>,
        skipped: usize,
    },
    ModelLoading,
    ModelReady {
        config: ModelConfig,
        device: String,
        /// Side of the input tile chosen for this device.
        tile_px: usize,
        /// On the GPU: the tile sizes tried, with speeds or rejection reasons.
        tile_notes: Vec<String>,
        /// On the GPU: the measured speed of each tile size, to remember.
        profile: Option<TileProfile>,
        on_gpu: bool,
        /// Why the GPU was requested but not used.
        gpu_fallback: Option<String>,
    },
    ModelFailed(String),
    Tile {
        doc_id: u64,
        index: usize,
        done: usize,
        total: usize,
        rect: PixelRect,
        pixels: Image,
    },
    /// End of noise reduction: `Ok(())`, cancelled (`Err(None)`) or failed.
    Denoised {
        doc_id: u64,
        result: Result<(), Option<String>>,
    },
    /// An upscaler is loaded: where it runs, and why not on the GPU asked
    /// for, if that happened.
    UpscalerReady {
        device: String,
        gpu_fallback: Option<String>,
    },
    /// The before/after preview of an enlargement, or why it failed.
    UpscalePreview {
        doc_id: u64,
        upscale: Upscale,
        result: Result<upscaling::Preview, String>,
    },
    /// Progress of an enlarged export, in tiles.
    ExportProgress {
        done: usize,
        total: usize,
    },
    /// Bytes of the upscaling models downloaded so far, and in all.
    DownloadProgress {
        done: u64,
        total: u64,
    },
    /// End of the download: done, cancelled (`Err(None)`) or failed.
    Downloaded(Result<(), Option<String>>),
    /// End of an export: the file, cancelled (`Err(None)`) or failed.
    Exported(Result<PathBuf, Option<String>>),
    BatchItem {
        index: usize,
        status: ItemStatus,
    },
    BatchFinished {
        cancelled: bool,
    },
    /// Unexpected internal error (panic) in a job.
    Crashed(String),
}

/// Channel to the interface that wakes it up on every message.
#[derive(Clone)]
struct Outbox {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Outbox {
    fn send(&self, msg: Msg) {
        // if the interface is closed there is nobody to tell
        let _ = self.tx.send(msg);
        self.ctx.request_repaint();
    }
}

// What crosses the worker's channels, and the errors carried inside, must be
// `Send`: if a field ever makes one of them `!Send`, the build stops here
// instead of in a distant refactoring.
const _: () = {
    const fn sendable<T: Send + 'static>() {}
    sendable::<Command>();
    sendable::<Msg>();
    sendable::<denoise_io::IoError>();
    sendable::<denoise_core::DenoiseError>();
    sendable::<denoise_engine::EngineError>();
};

pub struct Worker {
    commands: Sender<Command>,
    outbox: Outbox,
    inbox: Receiver<Msg>,
}

impl Worker {
    pub fn spawn(ctx: &egui::Context) -> Self {
        let (tx, inbox) = channel();
        let outbox = Outbox { tx, ctx: ctx.clone() };
        let (commands, rx) = channel::<Command>();
        let out = outbox.clone();
        std::thread::Builder::new()
            .name("model".into())
            .spawn(move || model_thread(&rx, &out))
            .map_err(|e| outbox.send(Msg::Crashed(format!("could not start the compute thread: {e}"))))
            .ok();
        Self { commands, outbox, inbox }
    }

    pub fn send(&self, cmd: Command) {
        if self.commands.send(cmd).is_err() {
            self.outbox.send(Msg::Crashed(tr().notices.compute_thread_stopped.into()));
        }
    }

    /// Messages received since the last frame.
    pub fn drain(&self) -> Vec<Msg> {
        self.inbox.try_iter().collect()
    }

    /// Opens a file in a separate thread.
    pub fn open(&self, path: PathBuf) {
        let out = self.outbox.clone();
        spawn_guarded(&self.outbox, "opening", move || {
            let result = denoise_io::read(&path, &read_limits()).map_err(|e| errors::io(&e));
            match &result {
                Ok(p) => tracing::info!("opened {} ({} × {})", path.display(), p.rgb.width(), p.rgb.height()),
                Err(e) => tracing::warn!("cannot open {}: {e}", path.display()),
            }
            out.send(Msg::Opened { path, result });
        });
    }

    /// Looks for the images among `paths` in a separate thread: a large
    /// folder on a network drive must not freeze the window.
    pub fn scan(&self, paths: Vec<PathBuf>) {
        let out = self.outbox.clone();
        spawn_guarded(&self.outbox, "scanning", move || {
            let (images, skipped) = crate::files::scan(&paths, |p| denoise_io::detect(p).is_some());
            out.send(Msg::Scanned { images, skipped });
        });
    }

    /// Downloads the upscaling models `entries` into `dir` in a separate
    /// thread (asked for by the user with a button).
    pub fn download(&self, entries: Vec<download::Entry>, dir: PathBuf, control: Arc<JobControl>) {
        let out = self.outbox.clone();
        spawn_guarded(&self.outbox, "download", move || {
            // at most a few progress messages a second
            let mut last: Option<std::time::Instant> = None;
            let mut progress = |done, total| {
                if done == total || last.is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(100)) {
                    last = Some(std::time::Instant::now());
                    out.send(Msg::DownloadProgress { done, total });
                }
            };
            let result = download::download(&entries, &dir, &mut progress, &control).map_err(|e| match e {
                download::DownloadError::Cancelled => None,
                download::DownloadError::Failed(why) => Some(why),
            });
            match &result {
                Ok(()) => tracing::info!("upscaling models downloaded to {}", dir.display()),
                Err(None) => tracing::info!("download of the upscaling models cancelled"),
                Err(Some(e)) => tracing::warn!("download of the upscaling models failed: {e}"),
            }
            out.send(Msg::Downloaded(result));
        });
    }

    /// Blends and exports in a separate thread.
    pub fn export(&self, job: ExportJob) {
        let out = self.outbox.clone();
        spawn_guarded(&self.outbox, "export", move || {
            let blended = blend(&job.original, &job.raw, &job.params);
            let result = denoise_io::write(
                &job.path,
                &blended,
                job.alpha.as_deref().map(Vec::as_slice),
                &job.info,
                &job.options,
            )
            .map(|()| job.path.clone())
            .map_err(|e| Some(errors::io(&e)));
            log_export(&job.path, &result);
            out.send(Msg::Exported(result));
        });
    }
}

fn log_export(path: &Path, result: &Result<PathBuf, Option<String>>) {
    match result {
        Ok(p) => tracing::info!("exported {}", p.display()),
        Err(None) => tracing::info!("export to {} cancelled", path.display()),
        Err(Some(e)) => tracing::warn!("export to {} failed: {e}", path.display()),
    }
}

/// Everything needed to export the open image.
pub struct ExportJob {
    pub path: PathBuf,
    pub original: Arc<Image>,
    pub raw: Arc<Image>,
    pub alpha: Option<Arc<Vec<f32>>>,
    pub info: Arc<SourceInfo>,
    pub params: Params,
    pub options: ExportOptions,
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".into())
}

fn spawn_guarded(out: &Outbox, what: &'static str, f: impl FnOnce() + Send + 'static) {
    let out2 = out.clone();
    let spawned = std::thread::Builder::new().name(what.into()).spawn(move || {
        if let Err(p) = catch_unwind(AssertUnwindSafe(f)) {
            let text = format!("internal error during {what}: {}", panic_text(&*p));
            tracing::error!("{text}");
            out2.send(Msg::Crashed(text));
        }
    });
    if let Err(e) = spawned {
        tracing::error!("could not start {what}: {e}");
        out.send(Msg::Crashed(format!("could not start {what}: {e}")));
    }
}

/// Loaded model together with the configuration that produced it.
struct Loaded {
    config: ModelConfig,
    model: Scunet,
}

fn ready(config: &ModelConfig, model: &Scunet) -> Msg {
    Msg::ModelReady {
        config: config.clone(),
        tile_px: model.input_px(),
        tile_notes: model.tile_notes().to_vec(),
        profile: model.profile().cloned(),
        on_gpu: model.on_gpu(),
        device: model.device().to_owned(),
        gpu_fallback: model.gpu_fallback().map(str::to_owned),
    }
}

fn ensure_model<'a>(slot: &'a mut Option<Loaded>, config: &ModelConfig, out: &Outbox) -> Option<&'a mut Scunet> {
    let reload = slot.as_ref().is_none_or(|l| l.config != *config);
    // same model, but another tile size may be faster for this image
    let switch = match slot.as_ref() {
        Some(l) if !reload => {
            let current = l.model.input_px();
            let want = config.wanted_tile(current, l.model.profile());
            (want != current).then(|| (want, l.model.profile().cloned()))
        }
        _ => None,
    };
    if reload || switch.is_some() {
        // free the GPU memory before loading another size
        *slot = None;
        out.send(Msg::ModelLoading);
        let result = match &switch {
            Some((tile, Some(profile))) => Scunet::load_tile(&config.models, &config.engine, *tile, profile)
                .or_else(|_| Scunet::load_adaptive(&config.models, &config.engine, Some(profile), config.image_px)),
            _ => Scunet::load_adaptive(&config.models, &config.engine, config.profile.as_ref(), config.image_px),
        };
        match result {
            Ok(model) => {
                tracing::info!(
                    "model ready: {} px tiles on {}{}",
                    model.input_px(),
                    model.device(),
                    model.gpu_fallback().map(|f| format!(" (GPU not used: {f})")).unwrap_or_default()
                );
                for note in model.tile_notes() {
                    tracing::info!("tile search: {note}");
                }
                out.send(ready(config, &model));
                *slot = Some(Loaded { config: config.clone(), model });
            }
            Err(e) => {
                tracing::error!("model not loaded: {e}");
                out.send(Msg::ModelFailed(errors::engine(&e)));
                return None;
            }
        }
    }
    slot.as_mut().map(|l| &mut l.model)
}

/// The networks of the model thread. At most one is loaded at a time: the
/// denoiser and an upscaler would not both fit in a small GPU's memory.
#[derive(Default)]
struct Networks {
    denoiser: Option<Loaded>,
    upscaler: Option<(UpscaleModels, Upscaler)>,
}

impl Networks {
    /// The denoiser for `config`, loading it if needed (and freeing the
    /// upscaler first).
    fn denoiser(&mut self, config: &ModelConfig, out: &Outbox) -> Option<&mut Scunet> {
        self.upscaler = None;
        ensure_model(&mut self.denoiser, config, out)
    }

    /// The upscaler for `upscale`, loading it if needed (and freeing the
    /// denoiser first).
    ///
    /// # Errors
    /// The reason it cannot be loaded, in the user's words.
    fn upscaler(&mut self, upscale: Upscale, models: &UpscaleModels, out: &Outbox) -> Result<&mut Upscaler, String> {
        let current = self.upscaler.as_ref().is_some_and(|(m, u)| m == models && u.upscale() == upscale);
        if !current {
            self.denoiser = None;
            self.upscaler = None;
            let u = Upscaler::load(&models.files, upscale, &models.engine).map_err(|e| {
                tracing::error!("upscaler not loaded: {e}");
                errors::engine(&e)
            })?;
            tracing::info!(
                "upscaler {upscale:?} ready on {}{}",
                u.device(),
                u.gpu_fallback().map(|f| format!(" (GPU not used: {f})")).unwrap_or_default()
            );
            out.send(Msg::UpscalerReady {
                device: u.device().to_owned(),
                gpu_fallback: u.gpu_fallback().map(str::to_owned),
            });
            self.upscaler = Some((models.clone(), u));
        }
        self.upscaler.as_mut().map(|(_, u)| u).ok_or_else(|| tr().notices.model_not_available.to_owned())
    }
}

/// Exports `job` enlarged by `upscale`; `Err(None)` if cancelled.
fn export_upscaled(
    nets: &mut Networks,
    job: &ExportJob,
    upscale: Upscale,
    models: &UpscaleModels,
    control: &JobControl,
    out: &Outbox,
) -> Result<PathBuf, Option<String>> {
    // what could stop the export late is checked before loading anything
    let dir = job.path.parent().unwrap_or_else(|| Path::new("."));
    let size = (job.original.width(), job.original.height());
    let scale = upscale.factor.value();
    upscaling::preflight(size, scale, job.alpha.is_some(), &job.info, &job.options, dir).map_err(Some)?;
    let model = nets.upscaler(upscale, models, out).map_err(Some)?;
    let blended = blend(&job.original, &job.raw, &job.params);
    let alpha = job.alpha.as_deref().map(Vec::as_slice);
    let mut progress = |done, total| out.send(Msg::ExportProgress { done, total });
    let target = upscaling::Output { path: &job.path, info: &job.info, opts: &job.options };
    upscaling::write_enlarged(&target, &blended, alpha, model, &mut progress, control).map_err(|e| match e {
        ExportError::Cancelled => None,
        ExportError::Failed(why) => Some(why),
    })?;
    Ok(job.path.clone())
}

fn model_thread(rx: &Receiver<Command>, out: &Outbox) {
    let mut nets = Networks::default();
    for cmd in rx {
        let result = catch_unwind(AssertUnwindSafe(|| match cmd {
            Command::Prepare { model } => {
                let before = nets.denoiser.as_ref().map(|l| (l.config == model, l.model.input_px()));
                if let Some(m) = nets.denoiser(&model, out) {
                    // nothing to load: the interface still waits for an answer
                    if before == Some((true, m.input_px())) {
                        out.send(ready(&model, m));
                    }
                }
            }
            Command::Unload => nets = Networks::default(),
            Command::Denoise { doc_id, model, image, info, plan, order, control } => {
                let Some(m) = nets.denoiser(&model, out) else {
                    out.send(Msg::Denoised { doc_id, result: Err(Some(tr().notices.model_not_available.into())) });
                    return;
                };
                if m.input_px() != plan.input_px() {
                    // the interface planned for another tile size: it asks again
                    out.send(Msg::Denoised { doc_id, result: Err(Some(tr().notices.tile_size_changed.into())) });
                    return;
                }
                let res = with_network_space(m, info.icc.as_deref(), |m| {
                    run_tiles(
                        &image,
                        m,
                        &plan,
                        &order,
                        &mut |t: TileOutput<'_>| {
                            out.send(Msg::Tile {
                                doc_id,
                                index: t.index,
                                done: t.done,
                                total: t.total,
                                rect: t.rect,
                                pixels: t.pixels.clone(),
                            });
                        },
                        &control,
                    )
                });
                let result = match res {
                    Ok(()) => Ok(()),
                    Err(DenoiseError::Cancelled) => Err(None),
                    Err(e) => Err(Some(errors::denoise(&e))),
                };
                out.send(Msg::Denoised { doc_id, result });
            }
            Command::ExportUpscaled { job, upscale, models, control } => {
                let result = export_upscaled(&mut nets, &job, upscale, &models, &control, out);
                log_export(&job.path, &result);
                out.send(Msg::Exported(result));
            }
            Command::PreviewUpscale { doc_id, job, at, upscale, models } => {
                let result = nets.upscaler(upscale, &models, out).and_then(|m| {
                    let blended = blend(&job.original, &job.raw, &job.params);
                    upscaling::preview(&blended, &job.info, m, at, &JobControl::default()).map_err(|e| match e {
                        ExportError::Cancelled => tr().errors.cancelled.to_owned(),
                        ExportError::Failed(why) => why,
                    })
                });
                out.send(Msg::UpscalePreview { doc_id, upscale, result });
            }
            Command::Batch { model, upscalers, items, params, export, control } => {
                let batch = Batch { config: &model, upscalers: &upscalers, params: &params, export: &export };
                let cancelled = run_batch(&mut nets, &batch, &items, &control, out);
                out.send(Msg::BatchFinished { cancelled });
            }
        }));
        if let Err(p) = result {
            // the sessions may be in an inconsistent state: reload them
            nets = Networks::default();
            out.send(Msg::Crashed(format!("internal error during processing: {}", panic_text(&*p))));
        }
    }
}

/// Processes the queue; returns `true` if it was cancelled. Each file is
/// read first, so that the tile size can be chosen for its size.
/// What every file of a queue run is processed with.
struct Batch<'a> {
    config: &'a ModelConfig,
    upscalers: &'a UpscaleModels,
    params: &'a Params,
    export: &'a ExportSettings,
}

fn run_batch(
    nets: &mut Networks,
    batch: &Batch<'_>,
    items: &[(usize, PathBuf)],
    control: &JobControl,
    out: &Outbox,
) -> bool {
    for (index, path) in items {
        if control.checkpoint().is_err() {
            return true;
        }
        let index = *index;
        out.send(Msg::BatchItem { index, status: ItemStatus::Running { done: 0, total: 0 } });
        let picture = match denoise_io::read(path, &read_limits()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("queue: cannot open {}: {e}", path.display());
                out.send(Msg::BatchItem { index, status: ItemStatus::Failed(errors::io(&e)) });
                continue;
            }
        };
        let mut progress =
            |done, total| out.send(Msg::BatchItem { index, status: ItemStatus::Running { done, total } });
        let status = match process_file(nets, batch, path, &picture, control, &mut progress, out) {
            Ok(written) => ItemStatus::Done(written),
            Err(None) => {
                out.send(Msg::BatchItem { index, status: ItemStatus::Queued });
                return true;
            }
            Err(Some(e)) => {
                tracing::warn!("queue: {} failed: {e}", path.display());
                ItemStatus::Failed(e)
            }
        };
        out.send(Msg::BatchItem { index, status });
    }
    false
}

/// Processes and exports one file already read (noise reduction, then the
/// enlargement if the export asks for one); `Err(None)` if cancelled,
/// otherwise the error message for the user.
fn process_file(
    nets: &mut Networks,
    batch: &Batch<'_>,
    path: &Path,
    picture: &Picture,
    control: &JobControl,
    progress: &mut dyn FnMut(usize, usize),
    out: &Outbox,
) -> Result<PathBuf, Option<String>> {
    let export = batch.export;
    let (format, _) = denoise_io::resolve(&picture.info, &export.options);
    let dir = export.dir_for(path);
    let target = denoise_io::output_path(path, &dir, &export.full_suffix(), format.extension());
    let size = (picture.rgb.width(), picture.rgb.height());
    let alpha = picture.alpha.as_deref();
    if let Some(u) = export.upscale {
        upscaling::preflight(size, u.factor.value(), alpha.is_some(), &picture.info, &export.options, &dir)
            .map_err(Some)?;
    }
    let Some(model) = nets.denoiser(&batch.config.for_image(size), out) else {
        return Err(Some(tr().notices.model_not_available.into()));
    };
    let result = with_network_space(model, picture.info.icc.as_deref(), |m| {
        denoise_core::denoise(&picture.rgb, batch.params, m, progress, control)
    })
    .map_err(|e| match e {
        DenoiseError::Cancelled => None,
        other => Some(errors::denoise(&other)),
    })?;
    match export.upscale {
        None => denoise_io::write(&target, &result, alpha, &picture.info, &export.options)
            .map_err(|e| Some(errors::io(&e)))?,
        Some(u) => {
            let model = nets.upscaler(u, batch.upscalers, out).map_err(Some)?;
            let output = upscaling::Output { path: &target, info: &picture.info, opts: &export.options };
            upscaling::write_enlarged(&output, &result, alpha, model, progress, control).map_err(|e| match e {
                ExportError::Cancelled => None,
                ExportError::Failed(why) => Some(why),
            })?;
        }
    }
    Ok(target)
}

/// Memory an open image takes per pixel: original and network result as
/// floating-point RGB (12 + 12 bytes), the blended copy made on export (12),
/// the two viewer textures (8) and the decoder's buffers.
const BYTES_PER_PIXEL: u64 = 48;

/// The largest image that fits in `available` bytes of memory, never more
/// than [`ReadLimits::MAX_PIXELS`]; no limit beyond that one if the amount is
/// unknown.
fn limits_for_memory(available: Option<u64>) -> ReadLimits {
    let fit = available.map_or(ReadLimits::MAX_PIXELS, |bytes| bytes / BYTES_PER_PIXEL);
    ReadLimits { max_pixels: fit.min(ReadLimits::MAX_PIXELS) }
}

/// Read limits for the memory available right now.
fn read_limits() -> ReadLimits {
    limits_for_memory(resources::available_memory())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_limits_follow_the_memory() {
        // 16 GB free: about 330 megapixels
        assert_eq!(limits_for_memory(Some(16_000_000_000)).max_pixels, 16_000_000_000 / 48);
        assert_eq!(limits_for_memory(None).max_pixels, ReadLimits::MAX_PIXELS);
        assert_eq!(limits_for_memory(Some(u64::MAX)).max_pixels, ReadLimits::MAX_PIXELS);
    }
}
