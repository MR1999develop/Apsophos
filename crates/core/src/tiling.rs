use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

use crate::{num, DenoiseError, Image, ModelError, PixelRect};

/// Neural network that processes square tiles of fixed size.
///
/// The input tile is planar (CHW: all red, then green, then blue),
/// `3 × input_px × input_px` values in 0..=1; the output has the same layout
/// and `scale` times the side: `3 × (scale · input_px)²` values. The
/// `margin_px` input pixels along each edge (`scale · margin_px` output
/// pixels) are discarded, because the network computes them without enough
/// context.
pub trait TileModel {
    /// Side of the input tile, in pixels.
    fn input_px(&self) -> usize;

    /// Input pixels discarded along each edge.
    fn margin_px(&self) -> usize;

    /// How many times larger the output is than the input, per side: 1 for a
    /// denoiser, 2 or 4 for an upscaler.
    fn scale(&self) -> usize {
        1
    }

    /// Processes one tile.
    ///
    /// # Errors
    /// Any error of the inference engine.
    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError>;
}

/// Cancellation and pause of a job, shared between the interface and the worker.
///
/// A paused worker sleeps in [`checkpoint`](Self::checkpoint) until it is
/// resumed or cancelled; no polling.
#[derive(Debug, Default)]
pub struct JobControl {
    cancel: AtomicBool,
    paused: Mutex<bool>,
    wake: Condvar,
}

impl JobControl {
    pub fn cancel(&self) {
        // under the lock, so a worker about to wait cannot miss the wake-up
        let _guard = self.paused.lock().unwrap_or_else(PoisonError::into_inner);
        self.cancel.store(true, Ordering::Relaxed);
        self.wake.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap_or_else(PoisonError::into_inner) = paused;
        self.wake.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// To be called between units of work: waits while the job is paused.
    ///
    /// # Errors
    /// [`DenoiseError::Cancelled`] if the job was cancelled.
    pub fn checkpoint(&self) -> Result<(), DenoiseError> {
        let paused = self.paused.lock().unwrap_or_else(PoisonError::into_inner);
        drop(self.wake.wait_while(paused, |p| *p && !self.is_cancelled()).unwrap_or_else(PoisonError::into_inner));
        if self.is_cancelled() {
            Err(DenoiseError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Split of an image into tiles.
///
/// Every "useful" tile (the rectangle that receives the result) measures
/// `input_px - 2 × margin_px` per side, except those of the last row and
/// column, which are clipped at the edge. The network receives the useful tile
/// plus `margin_px` of context per side; outside the image the context is the
/// mirrored image, so the result does not depend on the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilePlan {
    width: usize,
    height: usize,
    input_px: usize,
    margin_px: usize,
    core_px: usize,
    cols: usize,
    rows: usize,
}

impl TilePlan {
    /// # Errors
    /// [`DenoiseError::InvalidSize`] if the image is empty or the margin leaves
    /// no useful area in the tile.
    pub fn new(width: usize, height: usize, input_px: usize, margin_px: usize) -> Result<Self, DenoiseError> {
        if width == 0 || height == 0 {
            return Err(DenoiseError::InvalidSize("empty image".into()));
        }
        let core_px = input_px.saturating_sub(margin_px.saturating_mul(2));
        if core_px == 0 {
            return Err(DenoiseError::InvalidSize(format!(
                "tile of {input_px} px too small for a margin of {margin_px} px"
            )));
        }
        Ok(Self {
            width,
            height,
            input_px,
            margin_px,
            core_px,
            cols: width.div_ceil(core_px),
            rows: height.div_ceil(core_px),
        })
    }

    /// Plan with the sizes required by `model`.
    ///
    /// # Errors
    /// As [`TilePlan::new`].
    pub fn for_model(width: usize, height: usize, model: &dyn TileModel) -> Result<Self, DenoiseError> {
        Self::new(width, height, model.input_px(), model.margin_px())
    }

    /// Tiles per row.
    pub fn columns(&self) -> usize {
        self.cols
    }

    /// Rows of tiles.
    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn len(&self) -> usize {
        self.cols * self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Side of the input tile the plan was made for.
    pub fn input_px(&self) -> usize {
        self.input_px
    }

    /// Side of the useful tile.
    pub fn core_px(&self) -> usize {
        self.core_px
    }

    /// Useful rectangle of tile `index` (reading order).
    pub fn rect(&self, index: usize) -> PixelRect {
        let (row, col) = (index / self.cols, index % self.cols);
        let x = col * self.core_px;
        let y = row * self.core_px;
        PixelRect::new(
            x,
            y,
            self.core_px.min(self.width.saturating_sub(x)),
            self.core_px.min(self.height.saturating_sub(y)),
        )
    }

    /// Index of the tile containing pixel `(x, y)`.
    pub fn index_at(&self, x: usize, y: usize) -> Option<usize> {
        (x < self.width && y < self.height).then(|| (y / self.core_px) * self.cols + x / self.core_px)
    }

    /// Indices of the tiles satisfying `pending`, from the nearest to `center`
    /// (image coordinates) to the farthest; without a centre, in reading order.
    pub fn order_from(&self, center: Option<(f32, f32)>, pending: impl Fn(usize) -> bool) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.len()).filter(|&i| pending(i)).collect();
        if let Some((cx, cy)) = center {
            let dist = |i: usize| {
                let r = self.rect(i);
                let dx = r.x as f32 + r.width as f32 / 2.0 - cx;
                let dy = r.y as f32 + r.height as f32 / 2.0 - cy;
                dx * dx + dy * dy
            };
            order.sort_by(|&a, &b| dist(a).total_cmp(&dist(b)));
        }
        order
    }
}

/// A tile just processed.
pub struct TileOutput<'a> {
    pub index: usize,
    /// Tiles completed so far in this job (1 for the first).
    pub done: usize,
    pub total: usize,
    /// The useful rectangle, in input pixels.
    pub rect: PixelRect,
    /// Network output for `rect`: `rect.width × rect.height` times the
    /// model's scale.
    pub pixels: &'a Image,
}

/// Processes the tiles `order` of `plan` and hands each result to `on_tile`.
///
/// Tiles are processed one at a time: the inference engine already uses all
/// cores (or the GPU) for each tile, and peak memory stays that of a single
/// tile. Pause and cancellation are checked between tiles.
///
/// # Errors
/// [`DenoiseError::Cancelled`] if `control` is cancelled,
/// [`DenoiseError::Model`] if the network fails,
/// [`DenoiseError::InvalidSize`] if `plan` does not match `img` or `model`,
/// or the model's scale is 0.
pub fn run_tiles(
    img: &Image,
    model: &mut dyn TileModel,
    plan: &TilePlan,
    order: &[usize],
    on_tile: &mut dyn FnMut(TileOutput<'_>),
    control: &JobControl,
) -> Result<(), DenoiseError> {
    if (plan.width, plan.height) != (img.width(), img.height()) {
        return Err(DenoiseError::InvalidSize("the tile plan does not match the image".into()));
    }
    if (plan.input_px, plan.margin_px) != (model.input_px(), model.margin_px()) {
        return Err(DenoiseError::InvalidSize("the tile plan does not match the model".into()));
    }
    let scale = model.scale();
    if scale == 0 {
        return Err(DenoiseError::InvalidSize("model with a scale of 0".into()));
    }
    let n = plan.input_px;
    let out_n = n.checked_mul(scale).ok_or_else(|| DenoiseError::InvalidSize("tile too large".into()))?;
    let mut input = vec![0.0f32; 3 * n * n];
    let mut output = vec![0.0f32; 3 * out_n * out_n];
    let total = order.len();
    for (k, &index) in order.iter().enumerate() {
        control.checkpoint()?;
        let rect = plan.rect(index);
        gather_planar(img, rect, plan.margin_px, n, &mut input);
        model.run(&input, &mut output)?;
        let scaled = PixelRect::new(rect.x * scale, rect.y * scale, rect.width * scale, rect.height * scale);
        let pixels = scatter_interleaved(&output, out_n, plan.margin_px * scale, scaled);
        on_tile(TileOutput { index, done: k + 1, total, rect, pixels: &pixels });
    }
    control.checkpoint()
}

/// Symmetrically mirrored index (…, 1, 0 | 0, 1, …, n-1 | n-1, n-2, …), valid
/// even when the distance exceeds the size.
pub(crate) fn reflect(i: isize, n: usize) -> usize {
    let n = num::signed(n.max(1));
    let period = 2 * n;
    let m = i.rem_euclid(period);
    let r = if m >= n { period - 1 - m } else { m };
    r.unsigned_abs()
}

/// Copies into the planar buffer the region `rect` extended by `margin` per
/// side, mirroring at the image edges and clamping values to 0..=1.
fn gather_planar(img: &Image, rect: PixelRect, margin: usize, n: usize, out: &mut [f32]) {
    let (w, h) = (img.width(), img.height());
    let x0 = num::signed(rect.x) - num::signed(margin);
    let y0 = num::signed(rect.y) - num::signed(margin);
    let xs: Vec<usize> = (0..n).map(|dx| reflect(x0 + num::signed(dx), w)).collect();
    let plane = n * n;
    for dy in 0..n {
        let row = img.row(reflect(y0 + num::signed(dy), h));
        for (dx, &sx) in xs.iter().enumerate() {
            let o = dy * n + dx;
            for c in 0..3 {
                let v = row.get(sx * 3 + c).copied().unwrap_or(0.0);
                out[c * plane + o] = v.clamp(0.0, 1.0);
            }
        }
    }
}

/// Extracts the useful part of the planar output as an interleaved image.
fn scatter_interleaved(planar: &[f32], n: usize, margin: usize, rect: PixelRect) -> Image {
    let plane = n * n;
    let mut data = Vec::with_capacity(rect.area() * 3);
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            let o = (margin + dy) * n + margin + dx;
            for c in 0..3 {
                data.push(planar.get(c * plane + o).copied().unwrap_or(0.0));
            }
        }
    }
    Image::from_rgb(rect.width, rect.height, data).unwrap_or_default()
}

#[cfg(test)]
#[expect(clippy::needless_range_loop, reason = "the reference filters index pixels as their formulas do")]
mod tests {
    use super::*;

    /// Test "network": 3×3 mean with replicated border inside the tile. With a
    /// margin ≥ 1 the tiled result must equal the mean over the whole mirrored
    /// image.
    struct BoxBlur {
        n: usize,
        margin: usize,
        calls: usize,
    }

    impl TileModel for BoxBlur {
        fn input_px(&self) -> usize {
            self.n
        }
        fn margin_px(&self) -> usize {
            self.margin
        }
        fn run(&mut self, src: &[f32], dst: &mut [f32]) -> Result<(), ModelError> {
            self.calls += 1;
            let n = self.n;
            for c in 0..3 {
                for y in 0..n {
                    for x in 0..n {
                        let mut s = 0.0;
                        for (yy, xx) in (-1..=1).flat_map(|a| (-1..=1).map(move |b| (a, b))) {
                            let sy = num::unsigned((num::signed(y) + yy).clamp(0, num::signed(n) - 1));
                            let sx = num::unsigned((num::signed(x) + xx).clamp(0, num::signed(n) - 1));
                            s += src[c * n * n + sy * n + sx];
                        }
                        dst[c * n * n + y * n + x] = s / 9.0;
                    }
                }
            }
            Ok(())
        }
    }

    fn reference_blur(img: &Image) -> Image {
        let (w, h) = (img.width(), img.height());
        let mut out = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut s = [0.0f32; 3];
                for yy in -1..=1isize {
                    for xx in -1..=1isize {
                        let p = img.pixel(reflect(num::signed(x) + xx, w), reflect(num::signed(y) + yy, h)).unwrap();
                        for c in 0..3 {
                            s[c] += p[c];
                        }
                    }
                }
                let i = (y * w + x) * 3;
                for c in 0..3 {
                    out.as_mut_slice()[i + c] = s[c] / 9.0;
                }
            }
        }
        out
    }

    fn pseudo_random(w: usize, h: usize) -> Image {
        let mut state = 12345u32;
        let data = (0..w * h * 3)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (state >> 8) as f32 / (1u32 << 24) as f32
            })
            .collect();
        Image::from_rgb(w, h, data).unwrap()
    }

    #[test]
    fn reflect_matches_symmetric_padding() {
        let got: Vec<usize> = (-4..8).map(|i| reflect(i, 3)).collect();
        assert_eq!(got, vec![2, 2, 1, 0, 0, 1, 2, 2, 1, 0, 0, 1]);
        assert_eq!(reflect(5, 1), 0);
    }

    #[test]
    fn tiled_result_equals_whole_image() {
        // sizes that are not multiples of the tile, smaller and larger
        for (w, h) in [(37, 23), (5, 3), (64, 40)] {
            let img = pseudo_random(w, h);
            let mut model = BoxBlur { n: 16, margin: 2, calls: 0 };
            let ctrl = JobControl::default();
            let raw = crate::network_output(&img, &mut model, &mut |_, _| {}, &ctrl).unwrap();
            let reference = reference_blur(&img);
            let max_err =
                raw.as_slice().iter().zip(reference.as_slice()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(max_err < 1e-5, "{w}×{h}: error {max_err}");
            assert_eq!(model.calls, TilePlan::new(w, h, 16, 2).unwrap().len());
        }
    }

    #[test]
    fn order_starts_near_center_and_skips_done() {
        let plan = TilePlan::new(100, 100, 30, 5).unwrap(); // useful tiles of 20 px, 5×5
        let order = plan.order_from(Some((50.0, 50.0)), |i| i != 12);
        assert_eq!(order.len(), 24);
        // the central tile (12) is already done: start from one of its 4 neighbours
        assert!([7, 11, 13, 17].contains(&order[0]));
        assert!(!order.contains(&12));
        assert!([0, 4, 20, 24].contains(&order[23]));
        assert_eq!(plan.index_at(99, 99), Some(24));
    }

    #[test]
    fn cancel_stops_before_next_tile() {
        let img = pseudo_random(40, 40);
        let mut model = BoxBlur { n: 16, margin: 2, calls: 0 };
        let ctrl = JobControl::default();
        let plan = TilePlan::for_model(40, 40, &model).unwrap();
        let order: Vec<usize> = (0..plan.len()).collect();
        let res = run_tiles(
            &img,
            &mut model,
            &plan,
            &order,
            &mut |t| {
                if t.done == 2 {
                    ctrl.cancel();
                }
            },
            &ctrl,
        );
        assert!(matches!(res, Err(DenoiseError::Cancelled)));
        assert_eq!(model.calls, 2);
    }

    #[test]
    fn paused_checkpoint_waits_for_resume_or_cancel() {
        use std::sync::Arc;
        use std::time::Duration;
        for cancel in [false, true] {
            let ctrl = Arc::new(JobControl::default());
            ctrl.set_paused(true);
            let worker = {
                let ctrl = Arc::clone(&ctrl);
                std::thread::spawn(move || ctrl.checkpoint())
            };
            std::thread::sleep(Duration::from_millis(50));
            assert!(!worker.is_finished(), "a paused checkpoint must wait");
            if cancel {
                ctrl.cancel();
            } else {
                ctrl.set_paused(false);
            }
            let res = worker.join().unwrap();
            assert_eq!(res.is_err(), cancel);
        }
    }

    #[test]
    fn plan_rejects_margin_without_core() {
        assert!(TilePlan::new(10, 10, 8, 4).is_err());
        assert!(TilePlan::new(0, 10, 8, 1).is_err());
    }
}
