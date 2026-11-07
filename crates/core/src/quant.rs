//! Conversions between floating-point values 0..=1 and 8/16-bit integers.
//!
//! This is the only place where `as` narrows a type: the value is always
//! clamped before the conversion.

/// 0..=1 → 0..=255, rounded; out-of-range values are clamped.
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the sample range and rounded first"
)]
pub fn to_u8(v: f32) -> u8 {
    if v.is_nan() {
        return 0;
    }
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 0..=1 → 0..=65535, rounded; out-of-range values are clamped.
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the sample range and rounded first"
)]
pub fn to_u16(v: f32) -> u16 {
    if v.is_nan() {
        return 0;
    }
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// 0..=1 → 0..=255 with triangular (TPDF) dither of ±1 level: used when
/// reducing deeper samples to 8 bits, so that smooth gradients (skies, the
/// very areas a denoiser leaves smooth) do not turn into bands. The noise is a
/// fixed function of the sample's position `index`: the same image always
/// gives the same file.
#[inline]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to the sample range and rounded first"
)]
pub fn to_u8_dithered(v: f32, index: usize) -> u8 {
    if v.is_nan() {
        return 0;
    }
    let h = mix(index as u64);
    // two uniform values in 0..1 from the two halves of the hash
    let unit = |bits: u64| (bits & 0xFF_FFFF) as f32 / 16_777_216.0;
    let noise = unit(h) - unit(h >> 32);
    (v.clamp(0.0, 1.0) * 255.0 + noise).round().clamp(0.0, 255.0) as u8
}

/// 8-bit samples of `values`, dithered or simply rounded.
pub fn quantize_u8(values: &[f32], dither: bool) -> Vec<u8> {
    quantize_u8_at(values, 0, dither)
}

/// [`quantize_u8`] for part of an image whose first sample is the
/// `offset`-th: the dither is that of the whole image, so writing it in
/// parts gives the same file.
pub fn quantize_u8_at(values: &[f32], offset: usize, dither: bool) -> Vec<u8> {
    if dither {
        values.iter().enumerate().map(|(i, &v)| to_u8_dithered(v, offset + i)).collect()
    } else {
        values.iter().map(|&v| to_u8(v)).collect()
    }
}

/// SplitMix64 finaliser: a well-mixed 64-bit hash of `x`.
#[inline]
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[inline]
pub fn from_u8(v: u8) -> f32 {
    f32::from(v) / 255.0
}

#[inline]
pub fn from_u16(v: u16) -> f32 {
    f32::from(v) / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_is_exact() {
        for v in 0..=255u8 {
            assert_eq!(to_u8(from_u8(v)), v);
        }
        for v in (0..=65535u16).step_by(97) {
            assert_eq!(to_u16(from_u16(v)), v);
        }
    }

    #[test]
    fn dither_keeps_the_mean_and_breaks_bands() {
        // a level between two 8-bit steps: rounding gives one value everywhere,
        // dithering a mix whose mean is the level itself
        let v = 100.3 / 255.0;
        let values = vec![v; 100_000];
        assert!(quantize_u8(&values, false).iter().all(|&q| q == 100));
        let d = quantize_u8(&values, true);
        let mean = d.iter().map(|&q| f64::from(q)).sum::<f64>() / d.len() as f64;
        assert!((mean - 100.3).abs() < 0.01, "mean {mean}");
        assert!(d.iter().all(|&q| (99..=101).contains(&q)));
        // deterministic, and within range at the ends
        assert_eq!(d, quantize_u8(&values, true));
        // in parts, the same samples
        let parts = [quantize_u8_at(&values[..300], 0, true), quantize_u8_at(&values[300..], 300, true)].concat();
        assert_eq!(parts, d);
        assert!(quantize_u8(&[0.0, 1.0, -3.0, 9.0], true)
            .iter()
            .zip([0u8, 255, 0, 255])
            .all(|(a, b)| a.abs_diff(b) <= 1));
    }

    #[test]
    fn out_of_range_is_clamped() {
        assert_eq!(to_u8(-1.0), 0);
        assert_eq!(to_u8(7.0), 255);
        assert_eq!(to_u16(f32::NAN), 0);
        assert_eq!(to_u16(f32::INFINITY), 65535);
    }
}
