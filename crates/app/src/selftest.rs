//! `apsophos --self-test [report.txt]`: benchmarks the network without
//! opening the window. For every model in the models folder
//! (`scunet_color_real_<tile>.onnx`) and every compiled device it denoises a
//! synthetic noisy image and writes quality and speed to the report. It runs
//! from the real executable, so it uses the same libraries (DirectML.dll and
//! the TensorRT for RTX DLLs next to the program) as the application. On a
//! GPU it stops at the first tile size that fails: the larger ones would need
//! even more memory.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use denoise_core::{denoise, Image, JobControl, Preset, TileModel};
use denoise_engine::{tile_px_for, Device, EngineOptions, Scunet, TileProfile};

use crate::settings::Settings;

/// Gradients and squares plus reproducible noise (σ ≈ 0.06); returns
/// `(clean, noisy)`.
fn scene(w: usize, h: usize) -> (Image, Image) {
    let mut state = 7u32;
    let mut noise = || {
        let mut s = 0.0f32;
        for _ in 0..4 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = (state >> 8) as f32 / (1u32 << 24) as f32;
            s += u - 0.5;
        }
        s * 0.104
    };
    let mut clean = Vec::with_capacity(w * h * 3);
    let mut noisy = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let base = 0.2 + 0.6 * x as f32 / w as f32;
            let square = if (x / 40 + y / 40).is_multiple_of(2) { 0.1 } else { -0.1 };
            for c in [0.0, 0.05, 0.1] {
                let v: f32 = (base + square + c).clamp(0.0, 1.0);
                clean.push(v);
                noisy.push((v + noise()).clamp(0.0, 1.0));
            }
        }
    }
    (Image::from_rgb(w, h, clean).unwrap_or_default(), Image::from_rgb(w, h, noisy).unwrap_or_default())
}

fn rmse(a: &Image, b: &Image) -> f32 {
    let n = a.as_slice().len().max(1) as f32;
    (a.as_slice().iter().zip(b.as_slice()).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n).sqrt()
}

/// All models in the folder of `default_model`, smallest tile first.
fn models_next_to(default_model: &Path) -> Vec<PathBuf> {
    let dir = default_model.parent().unwrap_or_else(|| Path::new("."));
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| denoise_engine::is_model_file(p)).collect())
        .unwrap_or_default();
    found.sort_by_key(|p| tile_px_for(p));
    found
}

/// Denoises a synthetic image with one model on one device and writes the
/// result; `false` if it failed or did not run on that device.
fn check(out: &mut String, model_path: &Path, device: Device, scene_px: (usize, usize)) -> bool {
    let tile = tile_px_for(model_path);
    let tag = format!("[{device:?}, tile {tile}]");
    let t0 = Instant::now();
    let mut model = match Scunet::load(model_path, &EngineOptions { device, threads: None }) {
        Ok(m) => m,
        Err(e) => {
            let _ = writeln!(out, "{tag} model not loaded: {e}");
            return false;
        }
    };
    let load_s = t0.elapsed().as_secs_f32();
    if let Some(why) = model.gpu_fallback() {
        let _ = writeln!(out, "{tag} GPU not used: {why}");
        return false;
    }
    let ctrl = JobControl::default();
    let params = Preset::Strong.params();
    // warm-up on one tile: the first run on a GPU compiles shaders
    let (_, small) = scene(tile, tile);
    let t1 = Instant::now();
    if let Err(e) = denoise(&small, &params, &mut model, &mut |_, _| {}, &ctrl) {
        let _ = writeln!(out, "{tag} failed: {e}");
        return false;
    }
    let warm_s = t1.elapsed().as_secs_f32();
    let (w, h) = scene_px;
    let (clean, noisy) = scene(w, h);
    let t2 = Instant::now();
    let mut tiles = 0;
    match denoise(&noisy, &params, &mut model, &mut |_, total| tiles = total, &ctrl) {
        Ok(img) => {
            let secs = t2.elapsed().as_secs_f32();
            let mp = (w * h) as f32 / 1e6;
            let (before, after) = (rmse(&noisy, &clean), rmse(&img, &clean));
            let ok = if after < before * 0.5 { "OK" } else { "NOISE NOT REDUCED" };
            let _ = writeln!(
                out,
                "{tag} {ok} on {}: {:.2} s per megapixel ({tiles} tiles in {secs:.1} s, {:.2} s/tile); \
                 RMSE {before:.4} -> {after:.4}; load {load_s:.1} s, warm-up {warm_s:.1} s",
                model.device(),
                secs / mp,
                secs / tiles.max(1) as f32,
            );
            true
        }
        Err(e) => {
            let _ = writeln!(out, "{tag} failed: {e}");
            false
        }
    }
}

/// The tile size the program would use for some typical image sizes.
fn report_profile(out: &mut String, profile: &TileProfile) {
    for (w, h, what) in [(6000, 4000, "24 MP photo"), (4032, 3024, "12 MP phone photo"), (2048, 1536, "3 MP image")] {
        if let Some(tile) = profile.best(Some((w, h)), None) {
            let secs = profile.estimate(tile, w, h).unwrap_or(0.0);
            let _ = writeln!(out, "    for a {what} ({w}x{h}): {tile} px tiles, about {secs:.1} s");
        }
    }
}

/// Runs the benchmark and writes the report to `report`.
///
/// # Errors
/// If the report cannot be written.
pub fn run(report: &Path) -> std::io::Result<()> {
    let mut out = String::new();
    let yes = |b: bool| if b { "yes" } else { "no" };
    let _ = writeln!(out, "{} {} self-test", denoise_core::product::NAME, env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "Built with TensorRT for RTX: {}, DirectML: {}",
        yes(Device::rtx_available()),
        yes(Device::directml_available())
    );
    let Some(default_model) = Settings::default().find_model() else {
        let _ = writeln!(out, "Model not found.");
        return std::fs::write(report, out);
    };
    let models = models_next_to(&default_model);
    let _ =
        writeln!(out, "Models: {}", models.iter().map(|m| tile_px_for(m).to_string()).collect::<Vec<_>>().join(", "));
    let gpus: Vec<Device> = [Device::TensorRtRtx, Device::DirectMl].into_iter().filter(|d| d.available()).collect();
    // GPU: a 3.1 MP image; CPU: a smaller one and only the default and 960 px
    // tiles, to keep the test short
    for &device in &gpus {
        for (i, m) in models.iter().enumerate() {
            if !check(&mut out, m, device, (2048, 1536)) {
                // a larger tile needs more memory still: with DirectML on a
                // laptop GPU it can spill into the shared system memory and
                // freeze the whole computer instead of failing
                for larger in &models[i + 1..] {
                    let _ = writeln!(out, "[{device:?}, tile {}] skipped after the failure above", tile_px_for(larger));
                }
                std::fs::write(report, &out)?;
                break;
            }
            // written as it goes, so a crash still leaves the results so far
            std::fs::write(report, &out)?;
        }
    }
    // the choice the application makes: the measured speed of each tile size
    // that fits, then the fastest size for some typical images
    for &device in &gpus {
        let all: Vec<PathBuf> = Settings::default().model_set();
        let options = EngineOptions { device, threads: None };
        match Scunet::load_adaptive(&all, &options, None, None) {
            Ok(m) => {
                let _ = writeln!(out, "[{device:?}, automatic] chose {} px tiles on {}", m.input_px(), m.device());
                if let Some(why) = m.gpu_fallback() {
                    let _ = writeln!(out, "    GPU not used: {why}");
                }
                for note in m.tile_notes() {
                    let _ = writeln!(out, "    {note}");
                }
                if let Some(profile) = m.profile().cloned() {
                    report_profile(&mut out, &profile);
                    // loading again shows what the saved engines and kernels save
                    let tile = m.input_px();
                    drop(m);
                    let t = Instant::now();
                    match Scunet::load_tile(&all, &options, tile, &profile) {
                        Ok(_) => {
                            let _ = writeln!(
                                out,
                                "    loading {tile} px tiles again, with warm-up: {:.1} s (fastest load during the search: {:.1} s)",
                                t.elapsed().as_secs_f32(),
                                profile.load_secs
                            );
                        }
                        Err(e) => {
                            let _ = writeln!(out, "    loading {tile} px tiles again failed: {e}");
                        }
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(out, "[{device:?}, automatic] failed: {e}");
            }
        }
        std::fs::write(report, &out)?;
    }
    for m in models.iter().filter(|m| matches!(tile_px_for(m), 448 | 960)) {
        check(&mut out, m, Device::Cpu, (1280, 960));
        std::fs::write(report, &out)?;
    }
    let _ = writeln!(out, "Finished.");
    std::fs::write(report, out)
}
