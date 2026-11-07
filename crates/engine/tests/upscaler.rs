//! Test with the real upscaling models (when present in `models/`), on the
//! GPU back ends compiled in: the faithful 4× model must enlarge a reduced
//! photo-like image (with a camera's blur) better than Catmull-Rom, and the
//! faithful 2× model must run and stay close to it (its gain is small, see
//! docs/decisions/0001-ai-upscaling.md). A tile takes about a minute on the
//! CPU, so the CPU is left to the converter's checks. Run with `--nocapture`
//! to see timings and the measured values.
// test arithmetic on small, known sizes
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::path::PathBuf;
use std::time::Instant;

use denoise_core::resample::enlarge_image;
use denoise_core::upscale::{upscale, Detail, Factor, Upscale};
use denoise_core::{Image, JobControl};
use denoise_engine::{upscaler_files, Device, EngineOptions, Upscaler};

/// The model folder's files, if it has the model for `upscale`.
/// With `fp16_only`, only the half-precision files: the set the program
/// downloads.
fn model_files(upscale: Upscale, fp16_only: bool) -> Option<Vec<PathBuf>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| !fp16_only || p.to_string_lossy().ends_with("_fp16.onnx"))
        .collect();
    (!upscaler_files(&files, upscale).is_empty()).then_some(files)
}

/// Gradients, a checkerboard of hard edges and a disc, `w × h`.
fn scene(w: usize, h: usize) -> Image {
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let check = if (x / 48 + y / 48).is_multiple_of(2) { 0.15 } else { -0.15 };
            let disc = (fx - 0.6).powi(2) + (fy - 0.4).powi(2) < 0.04;
            let px = if disc { [0.85, 0.25, 0.15] } else { [0.3 + 0.4 * fx + check, 0.35 + 0.3 * fy, 0.5 + check] };
            data.extend(px);
        }
    }
    Image::from_rgb(w, h, data).unwrap()
}

/// Separable Gaussian blur with clamped edges.
fn blur(img: &Image, sigma: f32) -> Image {
    let r = (3.0 * sigma).ceil() as isize;
    let k: Vec<f32> = (-r..=r).map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp()).collect();
    let sum: f32 = k.iter().sum();
    let (w, h) = (img.width() as isize, img.height() as isize);
    let pass = |src: &Image, dx: isize, dy: isize| {
        let mut out = Image::new(src.width(), src.height());
        let data = out.as_mut_slice();
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 3];
                for (i, kw) in (-r..=r).zip(&k) {
                    let (sx, sy) = ((x + i * dx).clamp(0, w - 1), (y + i * dy).clamp(0, h - 1));
                    let p = src.pixel(sx as usize, sy as usize).unwrap();
                    for c in 0..3 {
                        acc[c] += kw / sum * p[c];
                    }
                }
                data[((y * w + x) * 3) as usize..][..3].copy_from_slice(&acc);
            }
        }
        out
    };
    pass(&pass(img, 1, 0), 0, 1)
}

/// Mean of each `f × f` block.
fn reduce(img: &Image, f: usize) -> Image {
    let (w, h) = (img.width() / f, img.height() / f);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for (dy, dx) in (0..f).flat_map(|a| (0..f).map(move |b| (a, b))) {
                let p = img.pixel(x * f + dx, y * f + dy).unwrap();
                for c in 0..3 {
                    acc[c] += p[c] / (f * f) as f32;
                }
            }
            data.extend(acc);
        }
    }
    Image::from_rgb(w, h, data).unwrap()
}

/// PSNR of `a` against `b`, without a border of `border` pixels.
fn psnr(a: &Image, b: &Image, border: usize) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0usize);
    for y in border..a.height() - border {
        for (p, q) in a.row(y).iter().zip(b.row(y)).skip(border * 3).take((a.width() - 2 * border) * 3) {
            sum += f64::from(p - q).powi(2);
            n += 1;
        }
    }
    (10.0 * (n as f64 / sum).log10()) as f32
}

/// PSNR of the network and of Catmull-Rom on the test image, for `factor`.
fn measure(device: Device, upscale_kind: Upscale) -> Option<(f32, f32)> {
    // 4×: the downloaded set (FP16 only); 2×: every file of the folder
    let files = model_files(upscale_kind, upscale_kind.factor == Factor::X4)?;
    let t0 = Instant::now();
    let mut m = Upscaler::load(&files, upscale_kind, &EngineOptions { device, threads: None }).expect("model loading");
    eprintln!("[{device:?}] {upscale_kind:?} loaded in {:.1} s on {}", t0.elapsed().as_secs_f32(), m.device());
    if !m.on_gpu() {
        eprintln!("[{device:?}] GPU not used ({:?}): test skipped", m.gpu_fallback());
        return None;
    }
    let f = upscale_kind.factor.value();
    let hr = scene(100 * f, 80 * f);
    let lr = reduce(&blur(&hr, 0.5 * f as f32), f);
    let t1 = Instant::now();
    let out = upscale(&lr, &mut m, &mut |_, _| {}, &JobControl::default()).expect("upscaling");
    eprintln!("[{device:?}] {f}x of {}x{} in {:.2} s", lr.width(), lr.height(), t1.elapsed().as_secs_f32());
    assert_eq!((out.width(), out.height()), (hr.width(), hr.height()));
    assert!(out.as_slice().iter().all(|v| v.is_finite()));
    let border = 4 * f;
    let (net, base) = (psnr(&out, &hr, border), psnr(&enlarge_image(&lr, f), &hr, border));
    eprintln!("[{device:?}] {f}x PSNR: network {net:.2} dB, Catmull-Rom {base:.2} dB");
    Some((net, base))
}

fn check(device: Device) {
    let faithful = |factor| Upscale { factor, detail: Detail::Faithful };
    if let Some((net, base)) = measure(device, faithful(Factor::X4)) {
        assert!(net > base + 1.0, "4x not better than Catmull-Rom: {net} vs {base}");
    }
    if let Some((net, base)) = measure(device, faithful(Factor::X2)) {
        assert!(net > base - 1.0, "2x far from Catmull-Rom: {net} vs {base}");
    }
}

#[test]
fn upscaler_beats_catmull_rom_on_directml() {
    if Device::directml_available() {
        check(Device::DirectMl);
    }
}

#[test]
fn upscaler_beats_catmull_rom_on_tensorrt_rtx() {
    if Device::rtx_available() {
        check(Device::TensorRtRtx);
    }
}
