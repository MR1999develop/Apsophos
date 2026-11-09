//! Inference of the SCUNet network ("real" variant, trained on camera noise
//! and JPEG compression) with ONNX Runtime.
//!
//! The ONNX model has a fixed `1 × 3 × N × N` input (N = 448 for the default
//! model, read from the file name for the others); the 64 px along each edge
//! of the output are discarded, so a 448 px tile yields 320 × 320 useful
//! pixels. It is generated from the original weights with
//! `tools/export_onnx_model.py`.
//!
//! Two GPU back ends can be compiled in (both are in the default build):
//! * **TensorRT for RTX** (feature `nvrtx`): NVIDIA RTX 30xx and later, needs
//!   only the two TensorRT for RTX DLLs next to the program and the NVIDIA
//!   driver; runs the half-precision (FP16) models
//!   `scunet_color_real_<N>_fp16.onnx` made by `tools/make_fp16_model.py`
//!   when they are present, the normal ones otherwise. It is about 4 times
//!   faster than DirectML on an RTX 3050;
//! * **DirectML** (feature `directml`): any DirectX 12 card, with nothing
//!   extra to install.
//!
//! (A CUDA / TensorRT 10 back end was tried and dropped: it needed several GB
//! of NVIDIA libraries and was slower than both.)
//!
//! If the requested GPU cannot be used, the model falls back to the next
//! back end and finally to the CPU, and [`Scunet::gpu_fallback`] says why.
//!
//! The same back ends run the SwinIR upscaling models ([`Upscaler`]).

pub mod gpu_error;
mod upscaler;

pub use upscaler::{
    card, upscaler_file_kind, upscaler_files, ModelCard, UpscaleSpeed, Upscaler, UpscalerFile, UpscalerTile,
    UPSCALE_MARGIN_PX,
};

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use denoise_core::{ModelError, TileModel, TilePlan};
use ort::environment::Environment;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use gpu_error::GpuProblem;

/// File name of the model looked up in the model folders.
pub const MODEL_FILE: &str = "scunet_color_real_448.onnx";
/// Side of the input tile of the default model.
pub const TILE_INPUT_PX: usize = 448;

/// Side of the input tile of a model file, read from its name
/// (`scunet_color_real_<px>.onnx`, or `…_<px>_fp16.onnx`); the default model
/// is 448 px.
pub fn tile_px_for(path: &Path) -> usize {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("scunet_color_real_"))
        .map(|s| s.strip_suffix(FP16_SUFFIX).unwrap_or(s))
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|&n| n.is_multiple_of(64) && n > 2 * TILE_MARGIN_PX)
        .unwrap_or(TILE_INPUT_PX)
}
/// Pixels discarded along each edge of the output.
pub const TILE_MARGIN_PX: usize = 64;

const FP16_SUFFIX: &str = "_fp16";
/// Timed runs of each tile size in the automatic search; the fastest counts.
const TIMED_PROBES: usize = 2;

/// `true` for a normal (FP32) model file `scunet_color_real_<px>.onnx`: the
/// files listed when looking for the tile sizes. The FP16 variants are found
/// from these with [`fp16_variant`].
pub fn is_model_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "onnx")
        && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
            s.strip_prefix("scunet_color_real_").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// The half-precision file next to a model (`…_448.onnx` → `…_448_fp16.onnx`).
pub fn fp16_variant(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{stem}{FP16_SUFFIX}.onnx"))
}

/// Requested compute device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Device {
    /// The fastest available: TensorRT for RTX, DirectML, then CPU.
    #[default]
    // older settings: "Gpu", and the dropped CUDA back ends
    #[serde(alias = "Gpu", alias = "Cuda", alias = "TensorRt")]
    Auto,
    TensorRtRtx,
    DirectMl,
    Cpu,
}

impl Device {
    /// `true` if this build can use TensorRT for RTX.
    pub const fn rtx_available() -> bool {
        cfg!(feature = "nvrtx")
    }

    /// `true` if this build can use DirectML.
    pub const fn directml_available() -> bool {
        cfg!(feature = "directml")
    }

    /// `true` if this build can use any GPU.
    pub const fn gpu_available() -> bool {
        Self::rtx_available() || Self::directml_available()
    }

    /// `true` if this build can use this device (`Auto` and `Cpu` always).
    pub const fn available(self) -> bool {
        match self {
            Device::Auto | Device::Cpu => true,
            Device::TensorRtRtx => Self::rtx_available(),
            Device::DirectMl => Self::directml_available(),
        }
    }

    /// GPU back ends to try, in order.
    fn gpu_order(self) -> Vec<Gpu> {
        let mut order = Vec::new();
        if matches!(self, Device::Auto | Device::TensorRtRtx) && Self::rtx_available() {
            order.push(Gpu::Rtx);
        }
        if matches!(self, Device::Auto | Device::DirectMl) && Self::directml_available() {
            order.push(Gpu::DirectMl);
        }
        order
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gpu {
    Rtx,
    DirectMl,
}

impl Gpu {
    fn name(self) -> &'static str {
        match self {
            Gpu::Rtx => "GPU (TensorRT for RTX)",
            Gpu::DirectMl => "GPU (DirectML)",
        }
    }
}

/// Engine options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EngineOptions {
    pub device: Device,
    /// CPU threads; `None` = all cores.
    pub threads: Option<NonZeroUsize>,
}

/// Model loading errors.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("the model file does not exist: {0}")]
    Missing(String),
    #[error("the model could not be loaded ({0}). The file may be damaged or may not be the expected model")]
    Load(String),
}

/// Measured speed of one tile size on the device.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileSpeed {
    pub tile_px: usize,
    /// Seconds to run the network on one tile.
    pub secs_per_tile: f64,
}

impl TileSpeed {
    /// Seconds per megapixel of useful (non-overlap) output.
    pub fn secs_per_mp(&self) -> f64 {
        let core = self.tile_px.saturating_sub(2 * TILE_MARGIN_PX).max(1);
        let px = (core * core) as f64;
        self.secs_per_tile / px * 1e6
    }
}

/// What the tile-size search found on a device: how fast each size that fits
/// runs, and how long it takes to load a model. Lets the program pick the
/// fastest size for each image without searching again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileProfile {
    /// Back end measured (as [`Scunet::device`]): the measures do not apply
    /// to another one, e.g. after TensorRT for RTX is installed.
    pub device: String,
    /// Sizes that fit, smallest first.
    pub speeds: Vec<TileSpeed>,
    /// Seconds to load a model (switching tile size costs this much).
    pub load_secs: f64,
}

impl TileProfile {
    /// Seconds to process an image of `width × height` with `tile_px` tiles,
    /// if that size was measured. Tiles do not shrink at the edges, so large
    /// tiles waste work on images that are not a multiple of them.
    pub fn estimate(&self, tile_px: usize, width: usize, height: usize) -> Option<f64> {
        let speed = self.speeds.iter().find(|s| s.tile_px == tile_px)?;
        let tiles = TilePlan::new(width, height, tile_px, TILE_MARGIN_PX).ok()?.len();
        Some(tiles as f64 * speed.secs_per_tile)
    }

    /// The fastest tile size for an image of `size` (`None`: per useful
    /// pixel, for an image not known yet). With `current`, the size already
    /// loaded, any other size is charged the time to load it.
    pub fn best(&self, size: Option<(usize, usize)>, current: Option<usize>) -> Option<usize> {
        let cost = |s: &TileSpeed| -> f64 {
            let run = match size {
                Some((w, h)) => self.estimate(s.tile_px, w, h).unwrap_or(f64::INFINITY),
                None => s.secs_per_mp(),
            };
            let switch = if current.is_some_and(|c| c != s.tile_px) && size.is_some() { self.load_secs } else { 0.0 };
            run + switch
        };
        self.speeds.iter().min_by(|a, b| cost(a).total_cmp(&cost(b))).map(|s| s.tile_px)
    }
}

/// The files of one model: the normal (FP32) one, its half-precision
/// (FP16) variant, or both.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelFiles {
    pub fp32: Option<PathBuf>,
    pub fp16: Option<PathBuf>,
}

/// A network loaded on a device, the part the denoiser and the upscaler
/// share: the session, the tile it takes and where it runs.
struct Network {
    session: Session,
    tile_px: usize,
    /// Output side over input side: 1 for the denoiser.
    scale: usize,
    on_gpu: bool,
    device: String,
    gpu_fallback: Option<String>,
    notes: Vec<String>,
    load_secs: f64,
}

impl Network {
    /// Loads the model at `path` (input tile `tile_px`, output `scale` times
    /// larger), on the GPU if requested and possible, else on the CPU.
    fn load(path: &Path, tile_px: usize, scale: usize, options: &EngineOptions) -> Result<Self, EngineError> {
        if !path.is_file() {
            return Err(EngineError::Missing(path.display().to_string()));
        }
        let fp16 = fp16_variant(path);
        let files = ModelFiles { fp32: Some(path.to_path_buf()), fp16: fp16.is_file().then_some(fp16) };
        Self::load_files(&files, tile_px, scale, options)
    }

    /// As [`Network::load`], from the files of one model: either precision
    /// may be missing. The GPU takes FP16 when there is one, the CPU FP32.
    fn load_files(
        files: &ModelFiles,
        tile_px: usize,
        scale: usize,
        options: &EngineOptions,
    ) -> Result<Self, EngineError> {
        let (fp32, fp16) = (files.fp32.as_deref(), files.fp16.as_deref());
        let Some(any) = fp32.or(fp16) else {
            return Err(EngineError::Missing("model file".into()));
        };
        let started = std::time::Instant::now();
        let threads =
            options.threads.or_else(|| std::thread::available_parallelism().ok()).map_or(1, NonZeroUsize::get);
        let mut failures = Vec::new();
        for gpu in options.device.gpu_order() {
            // Half precision from the FP16 model when there is one: TensorRT
            // for RTX only runs FP16 that way, and DirectML runs it about 1.7
            // times faster with the same result and half the GPU memory.
            let first = match fp16 {
                Some(p) => (p, ", FP16"),
                None => (any, ", FP32"),
            };
            let mut notes = Vec::new();
            let mut result = session(first.0, threads, Some(gpu)).map(|s| (s, first.1));
            // a DirectML GPU that cannot run FP16 gets the FP32 model there
            // rather than the CPU; not when memory ran out (FP32 needs twice
            // as much), nor on TensorRT for RTX (another engine build, ~15 s)
            if let (Err(e), Some(_), Some(full), Gpu::DirectMl) = (&result, fp16, fp32, gpu) {
                if GpuProblem::from_text(e) != Some(GpuProblem::OutOfMemory) {
                    notes.push(format!("{}: FP16 model not usable ({}), FP32 used", gpu.name(), gpu_error::plain(e)));
                    result = session(full, threads, Some(gpu)).map(|s| (s, ", FP32"));
                }
            }
            match result {
                Ok((session, precision)) => {
                    let gpu_fallback = (!failures.is_empty()).then(|| failures.join("; "));
                    let name = gpu.name();
                    let device = match name.strip_suffix(')') {
                        Some(head) => format!("{head}{precision})"),
                        None => name.to_owned(),
                    };
                    return Ok(Self {
                        session,
                        tile_px,
                        scale,
                        on_gpu: true,
                        device,
                        gpu_fallback,
                        notes,
                        load_secs: started.elapsed().as_secs_f64(),
                    });
                }
                Err(e) => failures.push(format!("{}: {}", gpu.name(), gpu_error::plain(&e))),
            }
        }
        if !matches!(options.device, Device::Auto | Device::Cpu) && failures.is_empty() {
            failures.push(format!("this build has no {:?} support", options.device));
        }
        // the CPU runs FP32; an FP16 model only when there is no other (it
        // works, about twice as slowly)
        let session = session(any, threads, None).map_err(EngineError::Load)?;
        let gpu_fallback = (!failures.is_empty()).then(|| failures.join("; "));
        let precision = if fp32.is_none() { ", FP16" } else { "" };
        Ok(Self {
            session,
            tile_px,
            scale,
            on_gpu: false,
            device: format!("CPU, {threads} threads{precision}"),
            gpu_fallback,
            notes: Vec::new(),
            load_secs: started.elapsed().as_secs_f64(),
        })
    }

    /// Runs the network on one planar tile.
    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
        let err = |e: &dyn std::fmt::Display| ModelError(e.to_string());
        let n = self.tile_px;
        let input = Tensor::from_array(([1usize, 3, n, n], planar_in.to_vec())).map_err(|e| err(&e))?;
        // the GPU's own errors in words: they are what a user sees most often
        let outputs =
            self.session.run(ort::inputs![input]).map_err(|e| ModelError(gpu_error::plain(&e.to_string())))?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(|e| err(&e))?;
        if data.len() != planar_out.len() {
            return Err(ModelError(format!(
                "output has {} values instead of {}: this is not the expected model",
                data.len(),
                planar_out.len()
            )));
        }
        planar_out.copy_from_slice(data);
        Ok(())
    }

    /// Runs the network once on a blank tile: fails if the device cannot
    /// handle this tile size (typically: not enough GPU memory).
    fn probe(&mut self) -> Result<(), ModelError> {
        let n = self.tile_px;
        let input = vec![0.5f32; 3 * n * n];
        let mut output = vec![0.0f32; 3 * n * n * self.scale * self.scale];
        self.run(&input, &mut output)
    }
}

/// SCUNet model, loaded and ready.
pub struct Scunet {
    net: Network,
    profile: Option<TileProfile>,
}

impl Scunet {
    /// Loads the model from `path`, on the GPU if requested and possible.
    ///
    /// # Errors
    /// [`EngineError`] if the file is missing or is not a valid model.
    pub fn load(path: &Path, options: &EngineOptions) -> Result<Self, EngineError> {
        Network::load(path, tile_px_for(path), 1, options).map(|net| Self { net, profile: None })
    }

    /// Loads the model with the tile size best suited to an image of `size`
    /// (`None`: not known yet) on the device.
    ///
    /// `models` are files of the same network with different tile sizes
    /// (`scunet_color_real_<px>.onnx`). On a GPU larger tiles waste less work
    /// on the overlap but need more memory, and larger is not always faster;
    /// they also waste work at the edges of the image. So, unless `known`
    /// already says how fast each size is on this device, the sizes are tried
    /// from the smallest up: each is run on a blank tile and timed; the search
    /// stops at the first size that does not fit in memory or is not faster
    /// per useful pixel than the previous one. The measures make up a
    /// [`TileProfile`], kept in the model ([`Scunet::profile`]) to be
    /// remembered, and the size that processes `size` fastest is loaded.
    ///
    /// On the CPU the default 448 px model is used, which is the fastest there.
    ///
    /// # Errors
    /// [`EngineError`] if no model can be loaded at all.
    pub fn load_adaptive(
        models: &[PathBuf],
        options: &EngineOptions,
        known: Option<&TileProfile>,
        size: Option<(usize, usize)>,
    ) -> Result<Self, EngineError> {
        let mut sorted = models.to_vec();
        sorted.sort_by_key(|p| tile_px_for(p));
        sorted.dedup_by_key(|p| tile_px_for(p));
        let default = sorted
            .iter()
            .find(|p| tile_px_for(p) == TILE_INPUT_PX)
            .or(sorted.first())
            .cloned()
            .ok_or_else(|| EngineError::Missing("no model file".into()))?;
        let cpu = EngineOptions { device: Device::Cpu, ..*options };
        if options.device == Device::Cpu || !Device::gpu_available() {
            return Self::load(&default, &cpu);
        }

        // fast path: the measures of an earlier search
        if let Some(profile) = known.filter(|p| !p.speeds.is_empty()) {
            let path = profile.best(size, None).and_then(|t| sorted.iter().find(|p| tile_px_for(p) == t));
            if let Some(path) = path {
                if let Ok(mut m) = Self::load(path, options) {
                    if m.net.on_gpu && m.net.device == profile.device && m.probe().is_ok() {
                        m.net.notes.push(format!("{} px tiles, from the measures of an earlier search", m.net.tile_px));
                        let mut profile = profile.clone();
                        profile.load_secs = m.net.load_secs;
                        m.profile = Some(profile);
                        return Ok(m);
                    }
                }
            }
        }

        let mut notes = Vec::new();
        let mut profile = TileProfile::default();
        let mut load_secs = Vec::new();
        // only one model at a time is kept in GPU memory, so that a larger one
        // gets all of it; the last one is kept in case it is the one to use
        let mut last: Option<Scunet> = None;
        for path in &sorted {
            let tile = tile_px_for(path);
            drop(last.take());
            let mut model = match Self::load(path, options) {
                Ok(m) => m,
                Err(e) => {
                    notes.push(format!("{tile} px tiles: {e}"));
                    continue;
                }
            };
            if !model.net.on_gpu {
                // no usable GPU at all: the CPU is fastest with the default tile
                let reason = model.net.gpu_fallback.take();
                drop(model);
                let mut m = Self::load(&default, &cpu)?;
                m.net.gpu_fallback = reason;
                return Ok(m);
            }
            load_secs.push(model.net.load_secs);
            profile.device.clone_from(&model.net.device);
            // first run: compiles the GPU kernels and checks that the tile fits
            if let Err(e) = model.probe() {
                notes.push(format!("{tile} px tiles skipped: {e}"));
                break;
            }
            // the faster of two timed runs: a single one can be slowed down by
            // anything else the GPU or the system is doing at that moment
            let mut secs = f64::INFINITY;
            let mut failed = None;
            for _ in 0..TIMED_PROBES {
                let t = std::time::Instant::now();
                if let Err(e) = model.probe() {
                    failed = Some(e);
                    break;
                }
                secs = secs.min(t.elapsed().as_secs_f64());
            }
            if let Some(e) = failed {
                notes.push(format!("{tile} px tiles skipped: {e}"));
                break;
            }
            let speed = TileSpeed { tile_px: tile, secs_per_tile: secs };
            notes.push(format!("{tile} px tiles: {:.2} s per megapixel", speed.secs_per_mp()));
            let previous = profile.speeds.last().map(TileSpeed::secs_per_mp);
            profile.speeds.push(speed);
            last = Some(model);
            if previous.is_some_and(|p| speed.secs_per_mp() >= p * 0.97) {
                break; // not faster: stop here
            }
        }
        if profile.speeds.is_empty() {
            drop(last);
            let mut m = Self::load(&default, &cpu)?;
            m.net.gpu_fallback = Some(notes.join("; "));
            return Ok(m);
        }
        // the fastest load seen: the first load of a size can include one-off
        // work (TensorRT for RTX compiles each size once)
        profile.load_secs = load_secs.iter().copied().fold(f64::INFINITY, f64::min);
        let best = profile.best(size, None).unwrap_or(TILE_INPUT_PX);
        let mut m = match last {
            Some(m) if m.net.tile_px == best => m,
            other => {
                drop(other);
                let path = sorted.iter().find(|p| tile_px_for(p) == best).unwrap_or(&default);
                let mut m = Self::load(path, options)?;
                // warm up again, so the first real tile is not slower
                let _ = m.probe();
                m
            }
        };
        // the search's notes first, then the loading's (e.g. FP16 not usable)
        let loading = std::mem::take(&mut m.net.notes);
        m.net.notes = notes.into_iter().chain(loading).collect();
        m.profile = Some(profile);
        Ok(m)
    }

    /// Loads the size `tile_px` of the same network, keeping `profile`; used
    /// to switch tile size for an image of another size.
    ///
    /// # Errors
    /// [`EngineError`] if that size is not among `models` or cannot be loaded.
    pub fn load_tile(
        models: &[PathBuf],
        options: &EngineOptions,
        tile_px: usize,
        profile: &TileProfile,
    ) -> Result<Self, EngineError> {
        let path = models
            .iter()
            .find(|p| tile_px_for(p) == tile_px)
            .ok_or_else(|| EngineError::Missing(format!("{tile_px} px model")))?;
        let mut m = Self::load(path, options)?;
        m.probe().map_err(|e| EngineError::Load(e.0))?;
        let mut profile = profile.clone();
        profile.load_secs = m.net.load_secs;
        m.net.notes.push(format!("{tile_px} px tiles, the fastest for this image size"));
        m.profile = Some(profile);
        Ok(m)
    }

    /// Tile sizes tried by [`Scunet::load_adaptive`], with their speed or the
    /// reason they were rejected.
    pub fn tile_notes(&self) -> &[String] {
        &self.net.notes
    }

    /// Measured speeds of the tile sizes (on a GPU, after
    /// [`Scunet::load_adaptive`] or [`Scunet::load_tile`]).
    pub fn profile(&self) -> Option<&TileProfile> {
        self.profile.as_ref()
    }

    fn probe(&mut self) -> Result<(), ModelError> {
        self.net.probe()
    }

    /// `true` if the network runs on a GPU.
    pub fn on_gpu(&self) -> bool {
        self.net.on_gpu
    }

    /// Device in use, for the interface (e.g. "GPU (DirectML)").
    pub fn device(&self) -> &str {
        &self.net.device
    }

    /// Why the GPU was requested but not used, if that happened.
    pub fn gpu_fallback(&self) -> Option<&str> {
        self.net.gpu_fallback.as_deref()
    }
}

/// ONNX Runtime errors (whose types vary between builder steps) as text.
fn text<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The ONNX Runtime environment, created once: `ort` allows a single one per
/// process, and it must outlive every session. Telemetry off: ONNX Runtime
/// would otherwise report usage to Windows (ETW) from a program that
/// promises to send nothing.
fn environment() -> Result<&'static Environment, String> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name(denoise_core::product::ID).with_telemetry(false).build().map_err(text))
        .as_ref()
        .map_err(Clone::clone)
}

fn session(path: &Path, threads: usize, gpu: Option<Gpu>) -> Result<Session, String> {
    let mut b = Session::builder(environment()?)
        .map_err(text)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(text)?;
    b = match gpu {
        Some(Gpu::Rtx) => with_rtx(quiet_threads(b)?)?,
        Some(Gpu::DirectMl) => with_directml(quiet_threads(b)?)?,
        // on the CPU the threads hand work to each other all the time:
        // spinning keeps them awake for it (measured: 27 s/MP with spinning,
        // 42 s/MP without)
        None => b.with_intra_threads(threads).map_err(text)?,
    };
    if gpu == Some(Gpu::Rtx) {
        return commit_compiled(b, path);
    }
    b.commit_from_file(path).map_err(text)
}

/// With the network on the GPU, the CPU threads only wait for it: they sleep
/// instead of spinning, which would keep the cores busy (and the fans on)
/// for nothing.
fn quiet_threads(b: SessionBuilder) -> Result<SessionBuilder, String> {
    b.with_intra_op_spinning(false).map_err(text)?.with_inter_op_spinning(false).map_err(text)
}

/// Where the engine TensorRT for RTX builds from `path` is saved. The name
/// carries the size and date of the model and the ONNX Runtime version, so a
/// changed model or library never reuses an old engine.
fn compiled_path(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let stem = path.file_stem()?.to_string_lossy();
    Some(rtx_cache_dir().join(format!("{stem}-{:x}-{modified:x}-ort{}.onnx", meta.len(), ort::MINOR_VERSION)))
}

/// TensorRT for RTX needs about 15 s to build its engine from the ONNX model;
/// the first time the engine is saved as a compiled model (in
/// [`rtx_cache_dir`]) and later sessions load that in a moment. A compiled
/// model that no longer loads (new driver or library) is rebuilt; if building
/// fails, the original model is used directly.
fn commit_compiled(b: SessionBuilder, path: &Path) -> Result<Session, String> {
    if let Some(compiled) = compiled_path(path) {
        if compiled.is_file() {
            match b.clone().commit_from_file(&compiled) {
                Ok(session) => return Ok(session),
                Err(_) => {
                    let _ = std::fs::remove_file(&compiled);
                }
            }
        }
        let partial = compiled.with_extension("partial.onnx");
        let built = std::fs::create_dir_all(rtx_cache_dir()).map_err(text).and_then(|()| {
            ort::compiler::ModelCompiler::new(b.clone())
                .and_then(|c| c.with_model_from_file(path))
                .and_then(|c| c.with_embed_ep_context())
                .and_then(|c| c.compile_to_file(&partial))
                .map_err(text)
        });
        if built.is_ok() && std::fs::rename(&partial, &compiled).is_ok() {
            if let Ok(session) = b.clone().commit_from_file(&compiled) {
                return Ok(session);
            }
            let _ = std::fs::remove_file(&compiled);
        }
        let _ = std::fs::remove_file(&partial);
    }
    let mut b = b;
    b.commit_from_file(path).map_err(text)
}

// `error_on_failure` turns a GPU that cannot be used into an error (instead of
// a silent CPU fallback inside ONNX Runtime), so that the interface can tell
// the truth about the device.

#[cfg(feature = "directml")]
fn with_directml(b: SessionBuilder) -> Result<SessionBuilder, String> {
    // DirectML requires sequential execution without memory patterns
    let b = b.with_memory_pattern(false).map_err(text)?;
    let b = b.with_parallel_execution(false).map_err(text)?;
    b.with_execution_providers([ort::ep::DirectML::default().build().error_on_failure()]).map_err(text)
}

#[cfg(not(feature = "directml"))]
fn with_directml(_b: SessionBuilder) -> Result<SessionBuilder, String> {
    Err("this build has no DirectML support".into())
}

/// Folder where TensorRT for RTX keeps the engines it builds and the GPU
/// kernels it compiles on the first run, so later runs start faster.
pub fn rtx_cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(denoise_core::product::DATA_DIR)
        .join("tensorrt-rtx-cache")
}

/// TensorRT for RTX: builds the engine in seconds and compiles the kernels for
/// this GPU on the first run. The precision is the model's own (FP16 for the
/// `_fp16` models).
#[cfg(feature = "nvrtx")]
fn with_rtx(b: SessionBuilder) -> Result<SessionBuilder, String> {
    let cache = rtx_cache_dir();
    std::fs::create_dir_all(&cache).map_err(text)?;
    let rtx = ort::ep::NVRTX::default().with_runtime_cache_path(cache.to_string_lossy()).build().error_on_failure();
    b.with_execution_providers([rtx]).map_err(text)
}

#[cfg(not(feature = "nvrtx"))]
fn with_rtx(_b: SessionBuilder) -> Result<SessionBuilder, String> {
    Err("this build has no TensorRT for RTX support".into())
}

impl TileModel for Scunet {
    fn input_px(&self) -> usize {
        self.net.tile_px
    }

    fn margin_px(&self) -> usize {
        TILE_MARGIN_PX
    }

    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
        self.net.run(planar_in, planar_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_size_from_file_name() {
        assert_eq!(tile_px_for(Path::new("models/scunet_color_real_448.onnx")), 448);
        assert_eq!(tile_px_for(Path::new("scunet_color_real_960.onnx")), 960);
        assert_eq!(tile_px_for(Path::new("scunet_color_real_1000.onnx")), 448, "not a multiple of 64");
        assert_eq!(tile_px_for(Path::new("other.onnx")), 448);
        assert_eq!(tile_px_for(Path::new("scunet_color_real_960_fp16.onnx")), 960);
    }

    /// Speeds measured with TensorRT for RTX on an RTX 3050 laptop GPU.
    fn rtx_3050() -> TileProfile {
        let speed = |tile_px: usize, s_per_mp: f64| {
            let core = (tile_px - 2 * TILE_MARGIN_PX) as f64;
            TileSpeed { tile_px, secs_per_tile: s_per_mp * core * core / 1e6 }
        };
        TileProfile {
            device: "GPU".into(),
            speeds: vec![speed(448, 0.81), speed(704, 0.58), speed(960, 0.50), speed(1216, 0.47), speed(1600, 0.45)],
            load_secs: 15.0,
        }
    }

    #[test]
    fn best_tile_depends_on_the_image() {
        let p = rtx_3050();
        // per useful pixel the largest tile wins...
        assert_eq!(p.best(None, None), Some(1600));
        // ...but on a 24 MP photo 1600 px tiles waste too much at the edges
        assert_eq!(p.best(Some((6000, 4000)), None), Some(1216));
        // a single tile covers a small image
        assert_eq!(p.best(Some((1000, 800)), None), Some(1216));
        assert_eq!(p.best(Some((300, 200)), None), Some(448));
    }

    #[test]
    fn switching_tile_size_must_pay_for_the_load() {
        let p = rtx_3050();
        // 1216 saves about 1 s on 24 MP, less than the 15 s of a new load
        assert_eq!(p.best(Some((6000, 4000)), Some(1600)), Some(1600));
        let cheap = TileProfile { load_secs: 0.2, ..p };
        assert_eq!(cheap.best(Some((6000, 4000)), Some(1600)), Some(1216));
        assert_eq!(TileProfile::default().best(Some((6000, 4000)), None), None);
    }

    #[test]
    fn fp16_files_are_variants_not_models() {
        assert!(is_model_file(Path::new("m/scunet_color_real_704.onnx")));
        assert!(!is_model_file(Path::new("m/scunet_color_real_704_fp16.onnx")));
        assert!(!is_model_file(Path::new("m/scunet_color_real_.onnx")));
        assert!(!is_model_file(Path::new("m/scunet_color_real_704.onnx.tmp")));
        assert_eq!(
            fp16_variant(Path::new("m/scunet_color_real_704.onnx")),
            Path::new("m/scunet_color_real_704_fp16.onnx")
        );
    }
}
