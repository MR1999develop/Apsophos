use rayon::prelude::*;

use crate::{Image, Params, PixelRect};

/// BT.709 luma weights: the images are gamma-encoded RGB with sRGB (= BT.709)
/// primaries, so their luma Y′ is this weighted sum of R′G′B′.
const LUMA_WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Luma Y′ of a gamma-encoded RGB pixel (BT.709 weights).
#[inline]
pub fn luma(p: [f32; 3]) -> f32 {
    p[0] * LUMA_WEIGHTS[0] + p[1] * LUMA_WEIGHTS[1] + p[2] * LUMA_WEIGHTS[2]
}

/// Applies the network's correction to a pixel in proportion to the parameters.
///
/// The difference `network − original` is split into a luma part (equal on
/// the three channels: the change of Y′, the grain) and a colour part (the rest), weighted by `luma` and
/// `chroma` respectively. With both at 1 the result is the network output;
/// with both at 0 it is the original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mixer {
    luma: f32,
    chroma: f32,
}

impl Mixer {
    pub fn new(params: &Params) -> Self {
        Self { luma: params.luma.fraction(), chroma: params.chroma.fraction() }
    }

    #[inline]
    pub fn apply(&self, original: [f32; 3], network: [f32; 3]) -> [f32; 3] {
        // values outside 0..=1 (floating-point TIFF highlights) were never
        // seen by the network: keep the pixel as it was
        if original.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return original;
        }
        let d = [network[0] - original[0], network[1] - original[1], network[2] - original[2]];
        let dy = luma(d);
        let mut out = [0.0; 3];
        for c in 0..3 {
            out[c] = original[c] + self.luma * dy + self.chroma * (d[c] - dy);
        }
        out
    }
}

/// Blends `original` and `raw` (network output) over the whole image.
pub fn blend(original: &Image, raw: &Image, params: &Params) -> Image {
    let mut out = original.clone();
    blend_region(original, raw, params, original.rect(), &mut out);
    out
}

/// Blends only the region `rect` (clipped to the image) and writes it into
/// `out`.
///
/// # Panics
/// If `raw` or `out` do not have the size of `original`: they always come
/// from the same document, so a mismatch is a bug, and writing a blend of two
/// different images would silently corrupt the result.
pub fn blend_region(original: &Image, raw: &Image, params: &Params, rect: PixelRect, out: &mut Image) {
    let same = |i: &Image| (i.width(), i.height()) == (original.width(), original.height());
    assert!(same(raw) && same(out), "blend_region: images of different sizes");
    let r = rect.intersect(&original.rect());
    if r.is_empty() {
        return;
    }
    let mixer = Mixer::new(params);
    let w3 = original.width() * 3;
    let (x0, x1) = (r.x * 3, r.right() * 3);
    out.as_mut_slice().par_chunks_exact_mut(w3).enumerate().skip(r.y).take(r.height).for_each(|(y, row_out)| {
        let row_o = &original.row(y)[x0..x1];
        let row_r = &raw.row(y)[x0..x1];
        let dst = &mut row_out[x0..x1];
        let pixels = dst.as_chunks_mut::<3>().0.iter_mut().zip(row_o.as_chunks::<3>().0).zip(row_r.as_chunks::<3>().0);
        for ((d, o), n) in pixels {
            *d = mixer.apply(*o, *n);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Percent, Preset};

    fn params(luma: f32, chroma: f32) -> Params {
        Params { luma: Percent::clamped(luma), chroma: Percent::clamped(chroma) }
    }

    #[test]
    fn extremes_give_original_and_network() {
        let o = [0.2, 0.5, 0.7];
        let n = [0.3, 0.4, 0.75];
        assert_eq!(Mixer::new(&params(0.0, 0.0)).apply(o, n), o);
        let full = Mixer::new(&Preset::Strong.params()).apply(o, n);
        for c in 0..3 {
            assert!((full[c] - n[c]).abs() < 1e-6);
        }
    }

    #[test]
    fn luma_only_keeps_chroma_of_original() {
        let o = [0.2, 0.5, 0.7];
        let n = [0.4, 0.4, 0.4]; // grey: changes both luminance and colour
        let m = Mixer::new(&params(100.0, 0.0)).apply(o, n);
        // differences between channels (colour) unchanged
        assert!(((m[0] - m[1]) - (o[0] - o[1])).abs() < 1e-6);
        assert!(((m[2] - m[1]) - (o[2] - o[1])).abs() < 1e-6);
        // luma equal to the network's
        assert!((luma(m) - luma(n)).abs() < 1e-6);
        assert!((luma([1.0; 3]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn region_blend_touches_only_region() {
        let o = Image::from_rgb(4, 2, vec![0.5; 24]).unwrap();
        let n = Image::from_rgb(4, 2, vec![0.1; 24]).unwrap();
        let mut out = o.clone();
        blend_region(&o, &n, &Preset::Strong.params(), PixelRect::new(1, 0, 2, 1), &mut out);
        assert_eq!(out.pixel(0, 0), Some([0.5; 3]));
        assert!((out.pixel(1, 0).unwrap()[0] - 0.1).abs() < 1e-6);
        assert_eq!(out.pixel(1, 1), Some([0.5; 3]));
    }

    #[test]
    fn out_of_range_pixels_are_kept() {
        let o = [1.5, 0.5, 0.5];
        assert_eq!(Mixer::new(&Preset::Strong.params()).apply(o, [1.0, 0.4, 0.4]), o);
    }
}
