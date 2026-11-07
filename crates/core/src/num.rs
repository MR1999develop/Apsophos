//! Conversions between floating-point and integer values, with their rule
//! written once: round, floor or ceil as named, then clamp to the target
//! range; NaN goes to the lower bound. The program converts floats to
//! integers (and sizes to signed offsets) only through these functions and
//! the pixel quantization of [`crate::quant`].

/// `v` rounded to the nearest `u16`, clamped to `lo..=u16::MAX`.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to the target range first")]
pub fn round_to_u16(v: f64, lo: u16) -> u16 {
    clamp_f64(v.round(), f64::from(lo), f64::from(u16::MAX)) as u16
}

/// `v` rounded to the nearest `u32`, clamped to `lo..=u32::MAX`.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to the target range first")]
pub fn round_to_u32(v: f64, lo: u32) -> u32 {
    clamp_f64(v.round(), f64::from(lo), f64::from(u32::MAX)) as u32
}

/// `v` rounded to the nearest `usize`, clamped to `0..=max`.
pub fn round_to_usize(v: f32, max: usize) -> usize {
    to_usize(v.round(), max)
}

/// The largest `usize` not above `v`, clamped to `0..=max`.
pub fn floor_to_usize(v: f32, max: usize) -> usize {
    to_usize(v.floor(), max)
}

/// The smallest `usize` not below `v`, clamped to `0..=max`.
pub fn ceil_to_usize(v: f32, max: usize) -> usize {
    to_usize(v.ceil(), max)
}

/// `v` as `f32`: rounds to the nearest representable value, which is all a
/// narrowing between floats can lose (out-of-range values become infinite).
#[expect(clippy::cast_possible_truncation, reason = "f64 to f32 only rounds, which is the intent")]
pub fn narrow(v: f64) -> f32 {
    v as f32
}

/// A size or index as a signed offset: sizes of things in memory are below
/// `isize::MAX` (no allocation can be larger), so this never saturates in
/// practice.
pub fn signed(v: usize) -> isize {
    isize::try_from(v).unwrap_or(isize::MAX)
}

/// A signed value as an index, negative values clamped to 0.
pub fn unsigned(v: isize) -> usize {
    usize::try_from(v).unwrap_or(0)
}

/// `v` (already rounded) as `usize`, clamped to `0..=max`; NaN gives 0.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=max first")]
fn to_usize(v: f32, max: usize) -> usize {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= max as f32 {
        return max;
    }
    (v as usize).min(max)
}

/// `v` clamped to `lo..=hi`, NaN to `lo`.
fn clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        lo
    } else {
        v.clamp(lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_limits() {
        assert_eq!(round_to_u16(299.6, 1), 300);
        assert_eq!(round_to_u16(0.2, 1), 1);
        assert_eq!(round_to_u16(1e9, 1), u16::MAX);
        assert_eq!(round_to_u16(f64::NAN, 1), 1);
        assert_eq!(round_to_u32(-5.0, 0), 0);
        assert_eq!(round_to_u32(f64::INFINITY, 0), u32::MAX);
    }

    #[test]
    fn float_to_index() {
        assert_eq!(floor_to_usize(3.9, 10), 3);
        assert_eq!(ceil_to_usize(3.1, 10), 4);
        assert_eq!(round_to_usize(3.5, 10), 4);
        assert_eq!(floor_to_usize(-2.0, 10), 0);
        assert_eq!(ceil_to_usize(25.0, 10), 10);
        assert_eq!(floor_to_usize(f32::NAN, 10), 0);
        assert_eq!(floor_to_usize(f32::INFINITY, 10), 10);
    }

    #[test]
    fn signed_offsets() {
        assert_eq!(signed(7), 7);
        assert_eq!(signed(usize::MAX), isize::MAX);
        assert_eq!(unsigned(-3), 0);
        assert_eq!(unsigned(5), 5);
        assert!((narrow(0.1) - 0.1).abs() < 1e-7);
    }
}
