//! Export with enlargement: the upscaling network runs (through the photo's
//! colour space) one row of tiles at a time, straight into the file; the
//! alpha channel is enlarged classically; what could stop the export late
//! (format limits, memory, disk space) is checked before any work.

use std::path::Path;

use denoise_core::resample::{enlarge, enlarge_image};
use denoise_core::upscale::{output_size, Bands};
use denoise_core::{run_tiles, DenoiseError, Image, JobControl, PixelRect, TileModel, TileOutput, TilePlan};
use denoise_io::{Band, BandSource, ExportOptions, FileFormat, IoError, SourceInfo};

use crate::errors;
use crate::format;
use crate::network_space::with_network_space;
use crate::resources;
use crate::text::tr;

/// Why an enlarged export ended without a file.
#[derive(Debug)]
pub enum ExportError {
    Cancelled,
    /// The message for the user.
    Failed(String),
}

impl From<IoError> for ExportError {
    fn from(e: IoError) -> Self {
        Self::Failed(errors::io(&e))
    }
}

impl From<DenoiseError> for ExportError {
    fn from(e: DenoiseError) -> Self {
        match e {
            DenoiseError::Cancelled => Self::Cancelled,
            other => Self::Failed(errors::denoise(&other)),
        }
    }
}

/// The file to write and how: the source's metadata and the options.
pub struct Output<'a> {
    pub path: &'a Path,
    pub info: &'a SourceInfo,
    pub opts: &'a ExportOptions,
}

/// Writes `img` (with `alpha`, one sample per pixel) enlarged by `model` to
/// `out`. `progress` receives `(done, total)` tiles.
///
/// # Errors
/// [`ExportError::Cancelled`] if `control` is cancelled, otherwise the
/// failure in the user's words; the target is then left untouched.
pub fn write_enlarged(
    out: &Output<'_>,
    img: &Image,
    alpha: Option<&[f32]>,
    model: &mut dyn TileModel,
    progress: &mut dyn FnMut(usize, usize),
    control: &JobControl,
) -> Result<(), ExportError> {
    let alpha = alpha.filter(|a| a.len() == img.width() * img.height());
    let Output { path, info, opts } = *out;
    with_network_space(model, info.icc.as_deref(), |m| {
        let bands = Bands::new(img, m)?;
        let mut source = Enlargement {
            bands,
            model: m,
            alpha,
            width: img.width(),
            height: img.height(),
            band: Image::default(),
            band_alpha: Vec::new(),
            progress,
            control,
        };
        denoise_io::write_from(path, &mut source, info, opts)
    })
}

/// One tile of the enlargement, before and after.
pub struct Preview {
    /// The tile's area in the photo.
    pub rect: PixelRect,
    /// The area enlarged with Catmull-Rom.
    pub classical: Image,
    /// The area enlarged by the network: the very pixels the export writes.
    pub network: Image,
}

/// The tile of the export's enlargement that holds the pixel `at` of `img`:
/// the same plan, network and colour handling as [`write_enlarged`], so the
/// pixels are those of the file.
///
/// # Errors
/// As [`write_enlarged`].
pub fn preview(
    img: &Image,
    info: &SourceInfo,
    model: &mut dyn TileModel,
    at: (usize, usize),
    control: &JobControl,
) -> Result<Preview, ExportError> {
    with_network_space(model, info.icc.as_deref(), |m| {
        let plan = TilePlan::for_model(img.width(), img.height(), m)?;
        let x = at.0.min(img.width().saturating_sub(1));
        let y = at.1.min(img.height().saturating_sub(1));
        let index = plan.index_at(x, y).ok_or_else(|| DenoiseError::InvalidSize("point outside the photo".into()))?;
        let mut found = None;
        run_tiles(img, m, &plan, &[index], &mut |t: TileOutput<'_>| found = Some((t.rect, t.pixels.clone())), control)?;
        let (rect, network) = found.ok_or_else(|| DenoiseError::InvalidSize("no tile".into()))?;
        let scale = m.scale();
        // the classical enlargement of the same area, with two pixels of
        // context so that its edges match the whole photo's
        let context = PixelRect::new(
            rect.x.saturating_sub(2),
            rect.y.saturating_sub(2),
            (rect.right() + 2).min(img.width()) - rect.x.saturating_sub(2),
            (rect.bottom() + 2).min(img.height()) - rect.y.saturating_sub(2),
        );
        let big = enlarge_image(&img.crop(context), scale);
        let inner = PixelRect::new(
            (rect.x - context.x) * scale,
            (rect.y - context.y) * scale,
            rect.width * scale,
            rect.height * scale,
        );
        Ok(Preview { rect, classical: big.crop(inner), network })
    })
}

/// The tiles of the export's enlargement over `area` of `img`, from the
/// nearest to its centre, leaving out `have`: each handed to `on_tile` (its
/// index, its area in the photo, the enlarged pixels) as soon as it is
/// done. The same plan, network and colour handling as [`write_enlarged`],
/// so the pixels are those of the file.
///
/// # Errors
/// As [`write_enlarged`].
pub fn enlarge_area(
    img: &Image,
    info: &SourceInfo,
    model: &mut dyn TileModel,
    area: PixelRect,
    have: &[usize],
    control: &JobControl,
    on_tile: &mut dyn FnMut(usize, PixelRect, &Image),
) -> Result<(), ExportError> {
    with_network_space(model, info.icc.as_deref(), |m| {
        let plan = TilePlan::for_model(img.width(), img.height(), m)?;
        #[expect(clippy::cast_precision_loss, reason = "a centre for sorting")]
        let centre = (area.x as f32 + area.width as f32 / 2.0, area.y as f32 + area.height as f32 / 2.0);
        let order = plan.order_from(Some(centre), |i| !have.contains(&i) && !plan.rect(i).intersect(&area).is_empty());
        run_tiles(img, m, &plan, &order, &mut |t: TileOutput<'_>| on_tile(t.index, t.rect, t.pixels), control)?;
        Ok(())
    })
}

/// The enlargement of a photo, as bands for the writer.
struct Enlargement<'a> {
    bands: Bands<'a>,
    model: &'a mut dyn TileModel,
    alpha: Option<&'a [f32]>,
    /// Size of the photo before the enlargement.
    width: usize,
    height: usize,
    band: Image,
    band_alpha: Vec<f32>,
    progress: &'a mut dyn FnMut(usize, usize),
    control: &'a JobControl,
}

impl BandSource for Enlargement<'_> {
    type Error = ExportError;

    fn width(&self) -> usize {
        self.bands.size().0
    }

    fn height(&self) -> usize {
        self.bands.size().1
    }

    fn has_alpha(&self) -> bool {
        self.alpha.is_some()
    }

    fn next_band(&mut self) -> Result<Option<Band<'_>>, ExportError> {
        let Some((y, band)) = self.bands.next_band(self.model, self.progress, self.control)? else {
            return Ok(None);
        };
        let scale = self.bands.size().0 / self.width.max(1);
        if let Some(a) = self.alpha {
            self.band_alpha = enlarge_rows(a, self.width, self.height, scale, y / scale, band.height() / scale);
        }
        self.band = band;
        Ok(Some(Band { rgb: &self.band, alpha: self.alpha.map(|_| self.band_alpha.as_slice()) }))
    }
}

/// Rows `first .. first + count` of a plane of `width × height` samples,
/// enlarged `scale` times, exactly as in the enlargement of the whole plane:
/// Catmull-Rom reads two rows around each one, which are taken along.
fn enlarge_rows(plane: &[f32], width: usize, height: usize, scale: usize, first: usize, count: usize) -> Vec<f32> {
    let top = first.saturating_sub(2);
    let bottom = (first + count + 2).min(height);
    let part = enlarge(plane.get(top * width..bottom * width).unwrap_or_default(), width, bottom - top, 1, scale);
    let row = width * scale;
    let skip = (first - top) * scale * row;
    part.get(skip..skip + count * scale * row).map(<[f32]>::to_vec).unwrap_or_default()
}

/// Checks, before any work, that `width × height` enlarged `scale` times
/// (with alpha or not) can be written with `opts` into `dir`: the format's
/// limits, the memory its writer gathers (the converted samples for TIFF
/// and JPEG) and a lower bound of the file's size against the free space.
///
/// # Errors
/// What stands in the way, in the user's words.
pub fn preflight(
    (width, height): (usize, usize),
    scale: usize,
    has_alpha: bool,
    info: &SourceInfo,
    opts: &ExportOptions,
    dir: &Path,
) -> Result<(), String> {
    let t = &tr().errors;
    let (w, h) = output_size(width, height, scale).ok_or_else(|| (t.invalid_size)("enlargement"))?;
    denoise_io::check_output(w, h, has_alpha, info, opts).map_err(|e| errors::io(&e))?;
    let (file, depth) = denoise_io::resolve(info, opts);
    let channels: u64 = match (file, info.gray, has_alpha) {
        (FileFormat::Jpeg, true, _) => 1,
        (FileFormat::Jpeg, false, _) | (_, false, false) => 3,
        (_, true, false) => 1,
        (_, _, true) => 4,
    };
    let samples = (w as u64).saturating_mul(h as u64).saturating_mul(channels).saturating_mul(depth.bytes());
    let gathered = if file == FileFormat::Png { 0 } else { samples };
    if let Some(available) = resources::available_memory().filter(|&available| gathered > available) {
        return Err((t.not_enough_memory)(&format::bytes(gathered), &format::bytes(available)));
    }
    // compressed photos rarely take less than this
    let least = if file == FileFormat::Jpeg { samples / 15 } else { samples / 3 };
    if let Some(free) = resources::free_space(dir).filter(|&free| least > free) {
        return Err((t.not_enough_disk)(&format::bytes(least), &format::bytes(free)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use denoise_core::ModelError;
    use denoise_io::{BitDepth, ExportFormat};

    #[test]
    fn rows_of_the_alpha_plane_match_the_whole() {
        let (w, h, s) = (7, 11, 4);
        let plane: Vec<f32> = (0..w * h).map(|i| ((i * 37) % 17) as f32 / 16.0).collect();
        let whole = enlarge(&plane, w, h, 1, s);
        let mut parts = Vec::new();
        for (first, count) in [(0, 3), (3, 3), (6, 4), (10, 1)] {
            parts.extend(enlarge_rows(&plane, w, h, s, first, count));
        }
        assert_eq!(parts.len(), whole.len());
        for (a, b) in parts.iter().zip(&whole) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    /// Enlarges by repeating each pixel twice.
    struct Nearest;

    impl TileModel for Nearest {
        fn input_px(&self) -> usize {
            16
        }
        fn margin_px(&self) -> usize {
            2
        }
        fn scale(&self) -> usize {
            2
        }
        fn run(&mut self, src: &[f32], dst: &mut [f32]) -> Result<(), ModelError> {
            for c in 0..3 {
                for y in 0..32 {
                    for x in 0..32 {
                        dst[c * 1024 + y * 32 + x] = src[c * 256 + (y / 2) * 16 + x / 2];
                    }
                }
            }
            Ok(())
        }
    }

    fn info(format: FileFormat) -> SourceInfo {
        SourceInfo {
            format,
            bit_depth: BitDepth::U8,
            gray: false,
            icc: None,
            exif: None,
            xmp: None,
            xmp_extended: None,
            dpi: None,
            notes: vec![],
            jpeg_quality: None,
        }
    }

    #[test]
    fn the_enlarged_photo_reaches_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let img = Image::from_rgb(30, 20, (0..1800).map(|i| (i % 255) as f32 / 255.0).collect()).unwrap();
        let alpha = vec![0.5; 600];
        let path = dir.path().join("big.png");
        let opts = ExportOptions { format: ExportFormat::SameAsSource, jpeg_quality: 90 };
        let (mut last, ctrl) = ((0, 0), JobControl::default());
        let src = info(FileFormat::Png);
        let out = Output { path: &path, info: &src, opts: &opts };
        write_enlarged(&out, &img, Some(&alpha), &mut Nearest, &mut |d, t| last = (d, t), &ctrl).unwrap();
        let back = denoise_io::read(&path, &denoise_io::ReadLimits::default()).unwrap();
        assert_eq!((back.rgb.width(), back.rgb.height()), (60, 40));
        assert!(back.alpha.is_some());
        assert_eq!(last.0, last.1);
        // cancelled: no file
        ctrl.cancel();
        let other = dir.path().join("other.png");
        let out = Output { path: &other, info: &src, opts: &opts };
        let r = write_enlarged(&out, &img, None, &mut Nearest, &mut |_, _| {}, &ctrl);
        assert!(matches!(r, Err(ExportError::Cancelled)));
        assert!(!other.exists());
    }

    #[test]
    fn the_preview_is_the_exported_tile() {
        let img = Image::from_rgb(40, 30, (0..3600).map(|i| (i % 97) as f32 / 96.0).collect()).unwrap();
        let src = info(FileFormat::Png);
        let p = preview(&img, &src, &mut Nearest, (20, 15), &JobControl::default()).unwrap();
        let whole = denoise_core::upscale::upscale(&img, &mut Nearest, &mut |_, _| {}, &JobControl::default()).unwrap();
        let r = p.rect;
        assert!(r.x <= 20 && 20 < r.right() && r.y <= 15 && 15 < r.bottom());
        assert_eq!(p.network, whole.crop(PixelRect::new(r.x * 2, r.y * 2, r.width * 2, r.height * 2)));
        assert_eq!((p.classical.width(), p.classical.height()), (r.width * 2, r.height * 2));
    }

    #[test]
    fn the_view_gets_the_exported_tiles_over_its_area() {
        let img = Image::from_rgb(40, 30, (0..3600).map(|i| (i % 97) as f32 / 96.0).collect()).unwrap();
        let src = info(FileFormat::Png);
        let whole = denoise_core::upscale::upscale(&img, &mut Nearest, &mut |_, _| {}, &JobControl::default()).unwrap();
        let area = PixelRect::new(14, 10, 6, 5);
        let mut got = Vec::new();
        enlarge_area(&img, &src, &mut Nearest, area, &[], &JobControl::default(), &mut |i, r, px| {
            got.push((i, r));
            assert_eq!(*px, whole.crop(PixelRect::new(r.x * 2, r.y * 2, r.width * 2, r.height * 2)));
        })
        .unwrap();
        // every tile touches the area, and together they cover it
        assert!(got.iter().all(|(_, r)| !r.intersect(&area).is_empty()));
        let covered: usize = got.iter().map(|(_, r)| r.intersect(&area).area()).sum();
        assert_eq!(covered, area.area());
        // the nearest to the centre first; tiles already there are left out
        let (first, _) = got[0];
        let mut again = Vec::new();
        enlarge_area(&img, &src, &mut Nearest, area, &[first], &JobControl::default(), &mut |i, _, _| again.push(i))
            .unwrap();
        assert_eq!(again.len(), got.len() - 1);
        assert!(!again.contains(&first));
    }

    #[test]
    fn limits_are_checked_before_the_work() {
        let dir = tempfile::tempdir().unwrap();
        let opts = ExportOptions { format: ExportFormat::Jpeg, jpeg_quality: 90 };
        let jpeg = info(FileFormat::Jpeg);
        assert!(preflight((6000, 4000), 4, false, &jpeg, &opts, dir.path()).is_ok());
        assert!(preflight((20_000, 1000), 4, false, &jpeg, &opts, dir.path()).is_err());
    }
}
