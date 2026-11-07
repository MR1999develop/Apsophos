//! Tools to judge the result: the "removed noise" view and the histogram.
//!
//! Both work on the values the viewer shows: the blended result where the
//! network has finished, the original elsewhere.

use denoise_core::{luma, num, Image, Mixer};

/// Amplification of the removed noise in its view: typical photo noise is a
/// few hundredths, invisible at ×1.
pub const NOISE_GAIN: f32 = 4.0;
/// Histogram bins (one per 8-bit level).
pub const BINS: usize = 256;
/// About this many pixels are sampled for the histogram, so that it can be
/// recomputed while a slider moves.
const HISTOGRAM_SAMPLES: usize = 250_000;

/// What was taken out of a pixel, around mid grey: grey where nothing changed,
/// lighter or darker (and coloured, for colour noise) where noise was removed.
#[inline]
pub fn removed_noise(original: [f32; 3], result: [f32; 3]) -> [f32; 3] {
    let v = |c: usize| (0.5 + NOISE_GAIN * (original[c] - result[c])).clamp(0.0, 1.0);
    [v(0), v(1), v(2)]
}

/// Luma histograms of the original and of the result.
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub original: Vec<u32>,
    pub result: Vec<u32>,
}

impl Histogram {
    /// Samples a regular grid of about [`HISTOGRAM_SAMPLES`] pixels;
    /// `done(x, y)` tells where the network output is ready.
    pub fn compute(original: &Image, raw: &Image, mixer: &Mixer, done: impl Fn(usize, usize) -> bool) -> Self {
        let (w, h) = (original.width(), original.height());
        let step = sample_step(w * h);
        let mut out = Self { original: vec![0; BINS], result: vec![0; BINS] };
        for y in (0..h).step_by(step) {
            let (o, n) = (original.row(y), raw.row(y));
            for x in (0..w).step_by(step) {
                let i = x * 3;
                let (Some(po), Some(pn)) = (o.get(i..i + 3), n.get(i..i + 3)) else { continue };
                let po = [po[0], po[1], po[2]];
                let pr = if done(x, y) { mixer.apply(po, [pn[0], pn[1], pn[2]]) } else { po };
                out.original[bin(luma(po))] += 1;
                out.result[bin(luma(pr))] += 1;
            }
        }
        out
    }

    /// The largest bin of both, to scale the drawing.
    pub fn peak(&self) -> u32 {
        self.original.iter().chain(&self.result).copied().max().unwrap_or(0)
    }

    /// Median brightness of the original and of the result, 0..=100 %: what
    /// a screen reader says of the drawing. `None` without samples.
    pub fn medians(&self) -> Option<(f32, f32)> {
        Some((median_percent(&self.original)?, median_percent(&self.result)?))
    }
}

/// Bin where half of the samples are reached, as a percentage of the range.
fn median_percent(bins: &[u32]) -> Option<f32> {
    let total: u64 = bins.iter().map(|&n| u64::from(n)).sum();
    if total == 0 {
        return None;
    }
    let mut seen = 0u64;
    let i = bins.iter().position(|&n| {
        seen += u64::from(n);
        2 * seen >= total
    })?;
    Some(100.0 * i as f32 / (bins.len().max(2) - 1) as f32)
}

/// Grid step giving about [`HISTOGRAM_SAMPLES`] samples out of `pixels`.
fn sample_step(pixels: usize) -> usize {
    num::floor_to_usize(num::narrow((pixels as f64 / HISTOGRAM_SAMPLES as f64).sqrt()), pixels).max(1)
}

/// The histogram bin of a value in 0..=1 (outside: the end bins; NaN: 0).
#[inline]
fn bin(v: f32) -> usize {
    num::round_to_usize(v * (BINS - 1) as f32, BINS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use denoise_core::{Params, Percent};

    #[test]
    fn unchanged_pixels_are_mid_grey() {
        assert_eq!(removed_noise([0.3, 0.6, 0.9], [0.3, 0.6, 0.9]), [0.5; 3]);
        let v = removed_noise([0.3, 0.3, 0.3], [0.25, 0.3, 0.35]);
        assert!((v[0] - 0.7).abs() < 1e-6 && (v[1] - 0.5).abs() < 1e-6 && (v[2] - 0.3).abs() < 1e-6);
        assert_eq!(removed_noise([1.0; 3], [0.0; 3]), [1.0; 3]);
    }

    #[test]
    fn histogram_counts_samples_and_follows_the_result() {
        // left half dark, right half light; the "network" turns all to mid grey
        let (w, h) = (100, 40);
        let data = (0..w * h).flat_map(|i| if i % w < w / 2 { [0.1; 3] } else { [0.9; 3] }).collect();
        let original = Image::from_rgb(w, h, data).unwrap();
        let raw = Image::from_rgb(w, h, vec![0.5; w * h * 3]).unwrap();
        let full = Mixer::new(&Params { luma: Percent::clamped(100.0), chroma: Percent::clamped(100.0) });
        let hist = Histogram::compute(&original, &raw, &full, |_, _| true);
        assert_eq!(hist.original.iter().sum::<u32>(), u32::try_from(w * h).unwrap());
        assert_eq!(hist.original[bin(0.1)] + hist.original[bin(0.9)], u32::try_from(w * h).unwrap());
        assert_eq!(hist.result[bin(0.5)], u32::try_from(w * h).unwrap());
        // where the network has not finished, the result is the original
        let none = Histogram::compute(&original, &raw, &full, |_, _| false);
        assert_eq!(none.result, none.original);
        assert_eq!(hist.peak(), u32::try_from(w * h).unwrap());
        // half dark, half light: the median is the dark half's top, the result mid grey
        let (orig, result) = hist.medians().unwrap();
        assert!((orig - 10.2).abs() < 0.5 && (result - 50.2).abs() < 0.5, "{orig} {result}");
    }

    #[test]
    fn median_of_nothing() {
        let empty = Histogram { original: vec![0; BINS], result: vec![0; BINS] };
        assert_eq!(empty.medians(), None);
        let mut one = empty.clone();
        one.original[BINS - 1] = 1;
        one.result[0] = 3;
        assert_eq!(one.medians(), Some((100.0, 0.0)));
    }

    #[test]
    fn large_images_are_sampled() {
        assert_eq!(sample_step(1000), 1);
        let step = sample_step(24_000_000);
        let samples = (6000usize.div_ceil(step)) * (4000usize.div_ceil(step));
        assert!((200_000..=320_000).contains(&samples), "{samples}");
    }
}
