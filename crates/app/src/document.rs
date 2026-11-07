//! The open image: original, network output (possibly partial) and display
//! cache.

use std::path::PathBuf;
use std::sync::Arc;

use denoise_core::{DenoiseError, Image, Mixer, Params, PixelRect, TilePlan};
use denoise_engine::{TILE_INPUT_PX, TILE_MARGIN_PX};
use denoise_io::{Picture, SourceInfo};

use crate::color::DisplayTransform;
use crate::inspect::Histogram;
use crate::text::tr;
use crate::viewer::DisplayCache;

pub struct Document {
    pub id: u64,
    pub path: PathBuf,
    pub original: Arc<Image>,
    pub alpha: Option<Arc<Vec<f32>>>,
    pub info: Arc<SourceInfo>,
    /// Network output; where a tile is not ready yet it holds the original.
    pub raw: Arc<Image>,
    pub plan: TilePlan,
    done: Vec<bool>,
    pub display: DisplayCache,
    /// Histogram of the last parameters and progress it was computed for.
    histogram: Option<((Params, usize), Histogram)>,
}

impl Document {
    /// # Errors
    /// [`DenoiseError::InvalidSize`] for an empty image.
    pub fn new(id: u64, path: PathBuf, picture: Picture) -> Result<Self, DenoiseError> {
        let Picture { rgb, alpha, info } = picture;
        let plan = TilePlan::new(rgb.width(), rgb.height(), TILE_INPUT_PX, TILE_MARGIN_PX)?;
        let original = Arc::new(rgb);
        let transform = Arc::new(DisplayTransform::new(info.icc.as_deref()));
        let display = DisplayCache::new(original.width(), original.height(), transform);
        Ok(Self {
            id,
            path,
            raw: Arc::clone(&original),
            original,
            alpha: alpha.map(Arc::new),
            info: Arc::new(info),
            done: vec![false; plan.len()],
            plan,
            display,
            histogram: None,
        })
    }

    /// Luma histograms of the original and of the result with `params`, as
    /// far as the network has got; recomputed only when either changes.
    pub fn histogram(&mut self, params: &Params) -> &Histogram {
        let key = (*params, self.done_count());
        let cached = self.histogram.take().filter(|(k, _)| *k == key).map(|(_, h)| h);
        let h = cached.unwrap_or_else(|| {
            Histogram::compute(&self.original, &self.raw, &Mixer::new(params), |x, y| self.is_pixel_done(x, y))
        });
        &self.histogram.insert((key, h)).1
    }

    pub fn width(&self) -> usize {
        self.original.width()
    }

    pub fn height(&self) -> usize {
        self.original.height()
    }

    pub fn done_count(&self) -> usize {
        self.done.iter().filter(|&&d| d).count()
    }

    pub fn is_complete(&self) -> bool {
        self.done.iter().all(|&d| d)
    }

    pub fn is_tile_done(&self, index: usize) -> bool {
        self.done.get(index).copied().unwrap_or(false)
    }

    /// Is the network tile covering pixel `(x, y)` ready?
    pub fn is_pixel_done(&self, x: usize, y: usize) -> bool {
        self.plan.index_at(x, y).is_some_and(|i| self.is_tile_done(i))
    }

    /// Tiles still to compute, nearest to `center` first.
    pub fn pending_order(&self, center: Option<(f32, f32)>) -> Vec<usize> {
        self.plan.order_from(center, |i| !self.is_tile_done(i))
    }

    /// Records a tile computed by the network.
    pub fn accept_tile(&mut self, index: usize, rect: PixelRect, pixels: &Image) {
        Arc::make_mut(&mut self.raw).paste(pixels, rect.x, rect.y);
        if let Some(d) = self.done.get_mut(index) {
            *d = true;
        }
        self.display.invalidate(rect);
    }

    /// Re-plans the tiles for a model with `tile_px` input tiles. Partial
    /// results computed with another tile size are discarded (the network
    /// output differs slightly between tilings).
    ///
    /// # Errors
    /// [`DenoiseError::InvalidSize`] if the tile size is not usable.
    pub fn use_tile(&mut self, tile_px: usize) -> Result<(), DenoiseError> {
        if self.plan.input_px() == tile_px {
            return Ok(());
        }
        self.plan = TilePlan::new(self.width(), self.height(), tile_px, TILE_MARGIN_PX)?;
        self.done = vec![false; self.plan.len()];
        self.raw = Arc::clone(&self.original);
        self.display.invalidate(PixelRect::new(0, 0, self.width(), self.height()));
        Ok(())
    }

    /// Separate access to the data (read-only) and the cache (writable), to draw
    /// without copying the image.
    pub fn parts(&mut self) -> (&Image, &Image, &TilePlan, &[bool], &mut DisplayCache) {
        (&self.original, &self.raw, &self.plan, &self.done, &mut self.display)
    }

    /// Name of the colour profile, for the information panel.
    pub fn profile_label(&self) -> String {
        match self.info.icc.as_deref() {
            Some(icc) => denoise_io::icc_description(icc).unwrap_or_else(|| tr().info.profile_unnamed.into()),
            None => tr().info.profile_none.into(),
        }
    }
}
