//! Classical enlargement by a whole factor, with Catmull-Rom interpolation
//! (a = −0.5): the baseline the upscaling network is compared with, and the
//! enlargement of what the network does not see (the alpha channel, and the
//! part of a wide-gamut photo outside sRGB).

use crate::tiling::reflect;
use crate::{num, Image};

/// Catmull-Rom weights of the four input samples around an output sample
/// that lies `t` (0..1) past the second of them.
fn weights(t: f32) -> [f32; 4] {
    [
        ((-0.5 * t + 1.0) * t - 0.5) * t,
        (1.5 * t - 2.5) * t * t + 1.0,
        ((-1.5 * t + 2.0) * t + 0.5) * t,
        (0.5 * t - 0.5) * t * t,
    ]
}

/// For each phase of the output (its index modulo `factor`): the offset of
/// the second input sample from `index / factor` (0 or −1) and the weights.
/// Pixel centres are aligned: output pixel `o` lies at input position
/// `(o + ½) / factor − ½`.
fn phases(factor: usize) -> Vec<(isize, [f32; 4])> {
    (0..factor)
        .map(|phase| {
            #[expect(clippy::cast_precision_loss, reason = "factors are small")]
            let pos = (phase as f32 + 0.5) / factor as f32 - 0.5;
            if pos < 0.0 {
                (-1, weights(pos + 1.0))
            } else {
                (0, weights(pos))
            }
        })
        .collect()
}

/// Enlarges interleaved samples (`channels` per pixel, `width × height`
/// pixels) by `factor` with Catmull-Rom interpolation, edges mirrored. Near
/// sharp edges the result overshoots a little, as Catmull-Rom does; nothing
/// is clamped. Returns an empty vector if `values` does not hold the pixels
/// or a size is 0.
pub fn enlarge(values: &[f32], width: usize, height: usize, channels: usize, factor: usize) -> Vec<f32> {
    let sizes_ok = width > 0 && height > 0 && channels > 0 && factor > 0;
    if !sizes_ok || width.checked_mul(height).and_then(|p| p.checked_mul(channels)) != Some(values.len()) {
        return Vec::new();
    }
    let taps = phases(factor);
    let (out_w, out_h) = (width * factor, height * factor);
    let source = |i: usize, off: isize, k: isize, n: usize| reflect(num::signed(i) + off + k - 1, n);
    // across the rows: width × factor, height unchanged
    let mut wide = vec![0.0f32; out_w * height * channels];
    for y in 0..height {
        let row = &values[y * width * channels..(y + 1) * width * channels];
        let out = &mut wide[y * out_w * channels..(y + 1) * out_w * channels];
        for ox in 0..out_w {
            let (off, w) = taps[ox % factor];
            for (k, wk) in (0..4).zip(w) {
                let sx = source(ox / factor, off, k, width);
                for c in 0..channels {
                    out[ox * channels + c] += wk * row[sx * channels + c];
                }
            }
        }
    }
    // down the columns: height × factor
    let stride = out_w * channels;
    let mut out = vec![0.0f32; stride * out_h];
    for oy in 0..out_h {
        let (off, w) = taps[oy % factor];
        let dst = &mut out[oy * stride..(oy + 1) * stride];
        for (k, wk) in (0..4).zip(w) {
            let sy = source(oy / factor, off, k, height);
            for (d, s) in dst.iter_mut().zip(&wide[sy * stride..(sy + 1) * stride]) {
                *d += wk * s;
            }
        }
    }
    out
}

/// [`enlarge`] for an RGB image.
pub fn enlarge_image(img: &Image, factor: usize) -> Image {
    let data = enlarge(img.as_slice(), img.width(), img.height(), 3, factor);
    Image::from_rgb(img.width() * factor, img.height() * factor, data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one_and_hit_the_samples() {
        for t in [0.0, 0.125, 0.25, 0.5, 0.75, 0.875] {
            assert!((weights(t).iter().sum::<f32>() - 1.0).abs() < 1e-6);
        }
        assert_eq!(weights(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn factor_one_is_the_identity() {
        let v: Vec<f32> = (0..30).map(|i| (i as f32 * 0.37).sin()).collect();
        assert_eq!(enlarge(&v, 5, 3, 2, 1), v);
    }

    #[test]
    fn constants_stay_and_ramps_are_exact_inside() {
        let flat = enlarge(&[0.4; 7 * 5 * 3], 7, 5, 3, 4);
        assert!(flat.iter().all(|&v| (v - 0.4).abs() < 1e-6));
        // a horizontal ramp: Catmull-Rom reproduces straight lines, away
        // from the mirrored edges
        let ramp: Vec<f32> = (0..10).map(|x| x as f32 / 10.0).collect();
        let out = enlarge(&ramp, 10, 1, 1, 2);
        for (ox, v) in out.iter().enumerate().take(16).skip(4) {
            let expected = ((ox as f32 + 0.5) / 2.0 - 0.5) / 10.0;
            assert!((v - expected).abs() < 1e-6, "{ox}: {v} vs {expected}");
        }
    }

    #[test]
    fn mismatched_sizes_give_nothing() {
        assert!(enlarge(&[0.0; 5], 2, 2, 1, 2).is_empty());
        assert!(enlarge(&[], 0, 0, 3, 2).is_empty());
        assert_eq!(enlarge_image(&Image::new(3, 2), 4).width(), 12);
    }
}
