use crate::DenoiseError;

/// Rectangle in image pixel coordinates (not screen coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PixelRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl PixelRect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self { x, y, width, height }
    }

    pub const fn right(&self) -> usize {
        self.x + self.width
    }

    pub const fn bottom(&self) -> usize {
        self.y + self.height
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn area(&self) -> usize {
        self.width * self.height
    }

    /// Intersection; empty (width or height 0) if the rectangles do not touch.
    pub fn intersect(&self, other: &PixelRect) -> PixelRect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        PixelRect::new(x, y, r.saturating_sub(x), b.saturating_sub(y))
    }

    pub const fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

/// Floating-point RGB image with interleaved samples `[r, g, b, …]`.
///
/// Nominal values are 0..=1 and stay encoded as in the source file (with its
/// gamma curve and colour profile): noise reduction needs no colour-space
/// conversion.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Image {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Image {
    /// Black image.
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![0.0; width * height * 3] }
    }

    /// Builds an image from interleaved RGB samples.
    ///
    /// # Errors
    /// [`DenoiseError::InvalidSize`] if `data` does not hold `width × height × 3` values.
    pub fn from_rgb(width: usize, height: usize, data: Vec<f32>) -> Result<Self, DenoiseError> {
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(3));
        if expected != Some(data.len()) {
            return Err(DenoiseError::InvalidSize(format!(
                "{width}×{height} needs {} values, got {}",
                width.saturating_mul(height).saturating_mul(3),
                data.len()
            )));
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn rect(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width, self.height)
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.data
    }

    pub fn into_vec(self) -> Vec<f32> {
        self.data
    }

    /// One row of pixels (`width × 3` values); empty if `y` is outside the image.
    pub fn row(&self, y: usize) -> &[f32] {
        let w3 = self.width * 3;
        self.data.get(y * w3..(y + 1) * w3).unwrap_or(&[])
    }

    /// Pixel at `(x, y)`, `None` if outside the image.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[f32; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) * 3;
        match self.data.get(i..i + 3) {
            Some(&[r, g, b]) => Some([r, g, b]),
            _ => None,
        }
    }

    /// Copy of the region `rect`, clipped to the image.
    pub fn crop(&self, rect: PixelRect) -> Image {
        let r = rect.intersect(&self.rect());
        let mut out = Image::new(r.width, r.height);
        let w3 = r.width * 3;
        for (dy, dst) in out.data.chunks_exact_mut(w3.max(1)).enumerate().take(r.height) {
            let src = &self.row(r.y + dy)[r.x * 3..r.right() * 3];
            dst.copy_from_slice(src);
        }
        out
    }

    /// Copies `src` with its top-left corner at `(x, y)`; the part outside the
    /// image is ignored.
    pub fn paste(&mut self, src: &Image, x: usize, y: usize) {
        let target = PixelRect::new(x, y, src.width, src.height).intersect(&self.rect());
        let w3 = self.width * 3;
        for dy in 0..target.height {
            let s = &src.row(dy)[..target.width * 3];
            let start = (target.y + dy) * w3 + target.x * 3;
            if let Some(d) = self.data.get_mut(start..start + s.len()) {
                d.copy_from_slice(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: usize, h: usize) -> Image {
        let data = (0..w * h * 3).map(|i| i as f32).collect();
        Image::from_rgb(w, h, data).unwrap()
    }

    #[test]
    fn from_rgb_rejects_wrong_length() {
        assert!(Image::from_rgb(2, 2, vec![0.0; 11]).is_err());
    }

    #[test]
    fn crop_and_paste_roundtrip() {
        let img = ramp(7, 5);
        let part = img.crop(PixelRect::new(2, 1, 3, 3));
        assert_eq!(part.pixel(0, 0), img.pixel(2, 1));
        let mut blank = Image::new(7, 5);
        blank.paste(&part, 2, 1);
        assert_eq!(blank.pixel(4, 3), img.pixel(4, 3));
        assert_eq!(blank.pixel(0, 0), Some([0.0; 3]));
    }

    #[test]
    fn crop_is_clamped() {
        let img = ramp(4, 4);
        let part = img.crop(PixelRect::new(3, 3, 10, 10));
        assert_eq!((part.width(), part.height()), (1, 1));
    }

    #[test]
    fn intersect_disjoint_is_empty() {
        let a = PixelRect::new(0, 0, 2, 2);
        let b = PixelRect::new(5, 5, 2, 2);
        assert!(a.intersect(&b).is_empty());
    }
}
