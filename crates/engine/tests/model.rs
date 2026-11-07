//! Test with the real model (when present in `models/`): loads SCUNet with
//! ONNX Runtime, on CPU and on the compiled GPU back ends, and checks that it reduces the noise of a
//! synthetic image. A CPU test also guards the quality: a minimum PSNR, the
//! presets in order, and no seams between tiles. Run with `--nocapture` to
//! see timings and the measured values.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use denoise_core::{blend, denoise, network_output, Image, JobControl, ModelError, PixelRect, Preset, TileModel};
use denoise_engine::{Device, EngineOptions, Scunet, MODEL_FILE};

fn model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models").join(MODEL_FILE)
}

/// Gradients and sharp edges plus reproducible pseudo-random noise.
fn scene(w: usize, h: usize) -> (Image, Image) {
    let mut state = 7u32;
    let mut noise = || {
        // sum of uniforms ≈ Gaussian, standard deviation ≈ 0.06
        let mut s = 0.0f32;
        for _ in 0..4 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s += (state >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
        }
        s * 0.104
    };
    let mut clean = Vec::with_capacity(w * h * 3);
    let mut noisy = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let base = 0.2 + 0.6 * x as f32 / w as f32;
            let square = if (x / 40 + y / 40).is_multiple_of(2) { 0.1 } else { -0.1 };
            for c in 0..3 {
                let v = (base + square + 0.05 * c as f32).clamp(0.0, 1.0);
                clean.push(v);
                noisy.push((v + noise()).clamp(0.0, 1.0));
            }
        }
    }
    (Image::from_rgb(w, h, clean).unwrap(), Image::from_rgb(w, h, noisy).unwrap())
}

fn rmse(a: &Image, b: &Image) -> f32 {
    let n = a.as_slice().len() as f32;
    (a.as_slice().iter().zip(b.as_slice()).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / n).sqrt()
}

/// Measures how much of a run is the network itself: the rest is preparing
/// and placing the tiles, which [`denoise_core::run_tiles`] overlaps with the
/// network where it can.
struct Timed<'a> {
    inner: &'a mut Scunet,
    network: Duration,
}

impl TileModel for Timed<'_> {
    fn input_px(&self) -> usize {
        self.inner.input_px()
    }

    fn margin_px(&self) -> usize {
        self.inner.margin_px()
    }

    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
        let t = Instant::now();
        let r = self.inner.run(planar_in, planar_out);
        self.network += t.elapsed();
        r
    }
}

fn check(device: Device) {
    let path = model_path();
    if !path.is_file() {
        eprintln!("model missing ({}): test skipped", path.display());
        return;
    }
    let t0 = Instant::now();
    let mut model = Scunet::load(&path, &EngineOptions { device, threads: None }).expect("model loading");
    eprintln!("[{device:?}] model loaded in {:.1} s on {}", t0.elapsed().as_secs_f32(), model.device());
    if let Some(why) = model.gpu_fallback() {
        eprintln!("[{device:?}] GPU not used: {why}");
    }

    // 2×2 useful tiles of 320 px: also checks the seams. The first run on a
    // GPU includes shader compilation, so it is timed separately.
    let (clean, noisy) = scene(600, 500);
    let ctrl = JobControl::default();
    let t1 = Instant::now();
    let out = denoise(&noisy, &Preset::Strong.params(), &mut model, &mut |_, _| {}, &ctrl).expect("denoising");
    let first = t1.elapsed().as_secs_f32();
    let mut timed = Timed { inner: &mut model, network: Duration::ZERO };
    let t2 = Instant::now();
    let _ = denoise(&noisy, &Preset::Strong.params(), &mut timed, &mut |_, _| {}, &ctrl).expect("denoising");
    let second = t2.elapsed().as_secs_f32();
    let network = timed.network.as_secs_f32();
    let (before, after) = (rmse(&noisy, &clean), rmse(&out, &clean));
    eprintln!(
        "[{device:?}] RMSE {before:.4} -> {after:.4}; 4 tiles: first run {first:.2} s, then {second:.3} s ({:.2} s/MP), \
         of which network {network:.3} s ({:.0} %)",
        second / 0.3,
        network / second * 100.0
    );
    assert!(after < before * 0.5, "noise not reduced enough: {before} -> {after}");
}

#[test]
fn scunet_reduces_noise_on_cpu() {
    check(Device::Cpu);
}

#[test]
fn scunet_reduces_noise_on_directml() {
    if Device::directml_available() {
        check(Device::DirectMl);
    }
}

#[test]
fn scunet_reduces_noise_on_tensorrt_rtx() {
    if Device::rtx_available() {
        check(Device::TensorRtRtx);
    }
}

fn psnr(a: &Image, b: &Image) -> f32 {
    20.0 * (1.0 / rmse(a, b)).log10()
}

/// Largest mean difference between `a` and `b` over the columns from `x0`
/// on: where seams would show, as stripes of a few columns.
fn worst_column_difference(a: &Image, b: &Image, x0: usize) -> f32 {
    (x0..a.width())
        .map(|x| {
            let d: f32 = (0..a.height())
                .map(|y| {
                    let (p, q) = (a.pixel(x, y).unwrap(), b.pixel(x, y).unwrap());
                    (0..3).map(|c| (p[c] - q[c]).abs()).sum::<f32>() / 3.0
                })
                .sum();
            d / a.height() as f32
        })
        .fold(0.0, f32::max)
}

/// Quality guard on the CPU, where results are deterministic. The bounds sit
/// a little below the values measured when the test was written (PSNR 48.1 dB
/// from 24.5; column difference after shifting the grid 0.0013): a change to the tiling, the
/// blend or the model that makes them worse fails here.
#[test]
fn quality_holds_on_cpu() {
    let path = model_path();
    if !path.is_file() {
        eprintln!("model missing ({}): test skipped", path.display());
        return;
    }
    let mut model = Scunet::load(&path, &EngineOptions { device: Device::Cpu, threads: None }).expect("model loading");
    // 3 × 2 useful tiles of 320 px
    let (clean, noisy) = scene(960, 640);
    let ctrl = JobControl::default();
    let raw = network_output(&noisy, &mut model, &mut |_, _| {}, &ctrl).expect("denoising");

    // minimum PSNR with the network's full correction
    let strong = blend(&noisy, &raw, &Preset::Strong.params());
    let (before, after) = (psnr(&noisy, &clean), psnr(&strong, &clean));
    eprintln!("PSNR {before:.2} dB -> {after:.2} dB");
    assert!(after >= 47.0, "PSNR {after:.2} dB: below the 48.1 dB measured when the test was written");

    // the presets remove more noise in order: Light < Medium < Strong
    let errors =
        [Preset::Light, Preset::Medium, Preset::Strong].map(|p| rmse(&blend(&noisy, &raw, &p.params()), &clean));
    eprintln!("RMSE light {:.4}, medium {:.4}, strong {:.4}", errors[0], errors[1], errors[2]);
    assert!(errors[0] > errors[1] && errors[1] > errors[2], "presets out of order: {errors:?}");

    // no seams: half a tile to the right, the grid falls elsewhere, and the
    // result must not change where the two images overlap (away from the new
    // left edge, whose context is mirrored)
    let shift = 160;
    let shifted_in = noisy.crop(PixelRect::new(shift, 0, 960 - shift, 640));
    let shifted = network_output(&shifted_in, &mut model, &mut |_, _| {}, &ctrl).expect("denoising");
    let same_area = raw.crop(PixelRect::new(shift, 0, 960 - shift, 640));
    let worst = worst_column_difference(&shifted, &same_area, 64);
    eprintln!("largest column difference after shifting the grid: {worst:.5}");
    assert!(worst < 0.003, "the result depends on the tile grid: column difference {worst:.5}");
}
