//! Applying the EXIF/TIFF orientation to the pixels.

use denoise_core::Image;

/// Rotates/mirrors `rgb` (and `alpha`) according to the EXIF orientation value
/// 1..=8; unknown values leave the image as it is.
pub fn apply(rgb: Image, alpha: Option<Vec<f32>>, orientation: u16) -> (Image, Option<Vec<f32>>) {
    if !(2..=8).contains(&orientation) {
        return (rgb, alpha);
    }
    let (w, h) = (rgb.width(), rgb.height());
    let (ow, oh) = if orientation >= 5 { (h, w) } else { (w, h) };
    // from output coordinates to source coordinates
    let src = |x: usize, y: usize| -> (usize, usize) {
        match orientation {
            2 => (w - 1 - x, y),
            3 => (w - 1 - x, h - 1 - y),
            4 => (x, h - 1 - y),
            5 => (y, x),
            6 => (y, h - 1 - x),
            7 => (w - 1 - y, h - 1 - x),
            _ => (w - 1 - y, x), // 8
        }
    };
    let input = rgb.as_slice();
    let mut data = Vec::with_capacity(ow * oh * 3);
    let mut alpha_out = alpha.as_ref().map(|_| Vec::with_capacity(ow * oh));
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = src(x, y);
            let i = sy * w + sx;
            data.extend_from_slice(input.get(i * 3..i * 3 + 3).unwrap_or(&[0.0; 3]));
            if let (Some(out), Some(a)) = (alpha_out.as_mut(), alpha.as_ref()) {
                out.push(a.get(i).copied().unwrap_or(1.0));
            }
        }
    }
    match Image::from_rgb(ow, oh, data) {
        Ok(img) => (img, alpha_out),
        Err(_) => (rgb, alpha),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2×1: pixel 0 = 0.0, pixel 1 = 1.0
    fn two_px() -> Image {
        Image::from_rgb(2, 1, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
    }

    #[test]
    fn rotate_90_cw_puts_left_pixel_on_top() {
        let (r, _) = apply(two_px(), None, 6);
        assert_eq!((r.width(), r.height()), (1, 2));
        assert_eq!(r.pixel(0, 0), Some([0.0; 3]));
        assert_eq!(r.pixel(0, 1), Some([1.0; 3]));
    }

    #[test]
    fn rotate_90_ccw_puts_right_pixel_on_top() {
        let (r, a) = apply(two_px(), Some(vec![0.25, 0.75]), 8);
        assert_eq!(r.pixel(0, 0), Some([1.0; 3]));
        assert_eq!(a, Some(vec![0.75, 0.25]));
    }

    #[test]
    fn mirror_and_identity() {
        let (r, _) = apply(two_px(), None, 2);
        assert_eq!(r.pixel(0, 0), Some([1.0; 3]));
        let (r, _) = apply(two_px(), None, 1);
        assert_eq!(r.pixel(0, 0), Some([0.0; 3]));
    }
}
