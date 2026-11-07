//! Enlargement by a super-resolution network.
//!
//! The network enlarges tiles of the image like the denoiser cleans them
//! (same plan, same mirrored edges, same overlap discarded); [`Bands`] hands
//! the result over one row of tiles at a time, from the top down, so that a
//! 4× result (16 times the pixels) never has to be in memory as a whole.

use serde::{Deserialize, Serialize};

use crate::{run_tiles, DenoiseError, Image, JobControl, TileModel, TileOutput, TilePlan};

/// How many times larger the result is, per side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Factor {
    X2,
    X4,
}

impl Factor {
    pub const ALL: [Factor; 2] = [Factor::X2, Factor::X4];

    pub const fn value(self) -> usize {
        match self {
            Factor::X2 => 2,
            Factor::X4 => 4,
        }
    }
}

/// What the network was trained for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Detail {
    /// Fidelity to the photo: no detail that was not there.
    Faithful,
    /// Plausible detail, invented where the photo has none (a GAN).
    Generated,
}

/// An enlargement: the factor and the kind of network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Upscale {
    pub factor: Factor,
    pub detail: Detail,
}

/// Size of `width × height` enlarged `factor` times; `None` if it does not
/// fit in memory addresses.
pub fn output_size(width: usize, height: usize, factor: usize) -> Option<(usize, usize)> {
    let (w, h) = (width.checked_mul(factor)?, height.checked_mul(factor)?);
    w.checked_mul(h)?.checked_mul(3)?;
    Some((w, h))
}

/// The enlargement of an image, band by band: each call of
/// [`Bands::next_band`] runs the network on one row of tiles and returns the
/// rows of the result it covers, from the top down; together the bands cover
/// the result exactly once.
pub struct Bands<'a> {
    img: &'a Image,
    plan: TilePlan,
    scale: usize,
    width: usize,
    height: usize,
    row: usize,
}

impl<'a> Bands<'a> {
    /// The enlargement of `img` by `model`, at the model's scale.
    ///
    /// # Errors
    /// [`DenoiseError::InvalidSize`] if the image is empty, the model's tile
    /// has no useful area, or the result would not fit in memory addresses.
    pub fn new(img: &'a Image, model: &dyn TileModel) -> Result<Self, DenoiseError> {
        let scale = model.scale();
        let (width, height) = output_size(img.width(), img.height(), scale)
            .filter(|_| scale > 0)
            .ok_or_else(|| DenoiseError::InvalidSize("the enlarged image would be too large".into()))?;
        let plan = TilePlan::for_model(img.width(), img.height(), model)?;
        Ok(Self { img, plan, scale, width, height, row: 0 })
    }

    /// Size of the result.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Tiles to run in all.
    pub fn total_tiles(&self) -> usize {
        self.plan.len()
    }

    /// The next band, as the index of its first row and its pixels; `None`
    /// after the last. `progress` receives `(done, total)` tiles of the whole
    /// enlargement. `model` must be the one given to [`Bands::new`].
    ///
    /// # Errors
    /// [`DenoiseError::Cancelled`] if `control` is cancelled,
    /// [`DenoiseError::Model`] if the network fails,
    /// [`DenoiseError::InvalidSize`] if `model` is not the one of the plan.
    pub fn next_band(
        &mut self,
        model: &mut dyn TileModel,
        progress: &mut dyn FnMut(usize, usize),
        control: &JobControl,
    ) -> Result<Option<(usize, Image)>, DenoiseError> {
        if self.row >= self.plan.rows() {
            return Ok(None);
        }
        if model.scale() != self.scale {
            return Err(DenoiseError::InvalidSize("the model does not match the enlargement".into()));
        }
        let (cols, total) = (self.plan.columns(), self.plan.len());
        let before = self.row * cols;
        let first = self.plan.rect(before);
        let scale = self.scale;
        let mut band = Image::new(self.width, first.height * scale);
        let order: Vec<usize> = (before..before + cols).collect();
        run_tiles(
            self.img,
            model,
            &self.plan,
            &order,
            &mut |t: TileOutput<'_>| {
                band.paste(t.pixels, t.rect.x * scale, 0);
                progress(before + t.done, total);
            },
            control,
        )?;
        self.row += 1;
        Ok(Some((first.y * scale, band)))
    }
}

/// The whole enlarged image, in memory: for small images, the preview and
/// tests.
///
/// # Errors
/// As [`Bands::new`] and [`Bands::next_band`].
pub fn upscale(
    img: &Image,
    model: &mut dyn TileModel,
    progress: &mut dyn FnMut(usize, usize),
    control: &JobControl,
) -> Result<Image, DenoiseError> {
    let mut bands = Bands::new(img, model)?;
    let (w, h) = bands.size();
    let mut out = Image::new(w, h);
    while let Some((y, band)) = bands.next_band(model, progress, control)? {
        out.paste(&band, 0, y);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelError;

    /// Test "network": enlarges by repeating each pixel `scale` times.
    struct Nearest {
        n: usize,
        margin: usize,
        scale: usize,
    }

    impl TileModel for Nearest {
        fn input_px(&self) -> usize {
            self.n
        }
        fn margin_px(&self) -> usize {
            self.margin
        }
        fn scale(&self) -> usize {
            self.scale
        }
        fn run(&mut self, src: &[f32], dst: &mut [f32]) -> Result<(), ModelError> {
            let (n, s) = (self.n, self.scale);
            let big = n * s;
            for c in 0..3 {
                for y in 0..big {
                    for x in 0..big {
                        dst[c * big * big + y * big + x] = src[c * n * n + (y / s) * n + x / s];
                    }
                }
            }
            Ok(())
        }
    }

    fn pseudo_random(w: usize, h: usize) -> Image {
        let data = (0..w * h * 3).map(|i| ((i * 7919 + 13) % 251) as f32 / 250.0).collect();
        Image::from_rgb(w, h, data).unwrap()
    }

    fn nearest_whole(img: &Image, s: usize) -> Image {
        let mut out = Image::new(img.width() * s, img.height() * s);
        let data = out.as_mut_slice();
        for y in 0..img.height() * s {
            for x in 0..img.width() * s {
                let p = img.pixel(x / s, y / s).unwrap();
                data[(y * img.width() * s + x) * 3..][..3].copy_from_slice(&p);
            }
        }
        out
    }

    #[test]
    fn tiled_enlargement_equals_the_whole() {
        let img = pseudo_random(37, 23);
        for scale in [2, 4] {
            let mut model = Nearest { n: 16, margin: 3, scale };
            let out = upscale(&img, &mut model, &mut |_, _| {}, &JobControl::default()).unwrap();
            assert_eq!(out, nearest_whole(&img, scale), "scale {scale}");
        }
    }

    #[test]
    fn bands_come_in_order_and_cover_the_result() {
        let img = pseudo_random(30, 25);
        let mut model = Nearest { n: 12, margin: 2, scale: 2 };
        let mut bands = Bands::new(&img, &model).unwrap();
        assert_eq!(bands.size(), (60, 50));
        let (mut next, mut last_progress) = (0, (0, 0));
        let ctrl = JobControl::default();
        while let Some((y, band)) = bands.next_band(&mut model, &mut |d, t| last_progress = (d, t), &ctrl).unwrap() {
            assert_eq!((y, band.width()), (next, 60));
            next += band.height();
        }
        assert_eq!(next, 50);
        assert_eq!(last_progress, (bands.total_tiles(), bands.total_tiles()));
        assert!(bands.next_band(&mut model, &mut |_, _| {}, &ctrl).unwrap().is_none());
    }

    #[test]
    fn another_model_is_refused() {
        let img = pseudo_random(30, 25);
        let model = Nearest { n: 12, margin: 2, scale: 2 };
        let mut other = Nearest { n: 12, margin: 2, scale: 4 };
        let mut bands = Bands::new(&img, &model).unwrap();
        assert!(bands.next_band(&mut other, &mut |_, _| {}, &JobControl::default()).is_err());
    }

    #[test]
    fn sizes_that_overflow_are_refused() {
        assert_eq!(output_size(100, 50, 4), Some((400, 200)));
        assert_eq!(output_size(usize::MAX / 2, 3, 4), None);
    }
}
