//! Reading and writing images.
//!
//! Each format is read **and** written by the same module, so its rules (bit
//! depth, channels, metadata) live in one place:
//!
//! * TIFF ([`tiff_io`]): 8, 16, 32-bit integer or floating point; grey, RGB,
//!   with or without alpha; ICC profile, EXIF, GPS, XMP, descriptive tags and
//!   print resolution; orientation applied to the pixels.
//! * JPEG and PNG ([`raster`]): ICC profile, EXIF, XMP and print resolution,
//!   with the orientation applied to the pixels and reset on save; for JPEG
//!   also the "extended XMP" of packets larger than one segment
//!   ([`jpeg_app`]).
//!
//! The format is recognised from the file content, not the extension. The
//! image size is checked against [`ReadLimits`] from the header, before any
//! pixel is decoded. Writing is atomic: temporary file in the same folder,
//! then rename. Deeper sources written to 8 bits are dithered.

mod error;
mod exif;
#[cfg(test)]
mod hostile_input;
mod icc;
mod jpeg_app;
mod meta;
mod orient;
mod raster;
mod tiff_io;

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use denoise_core::Image;
use serde::{Deserialize, Serialize};

pub use error::IoError;
pub use icc::icc_description;

/// Supported file formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFormat {
    Tiff,
    Jpeg,
    Png,
}

impl FileFormat {
    /// Every supported format: the file dialogs and the drop handling are
    /// built from this list.
    pub const ALL: [FileFormat; 3] = [FileFormat::Tiff, FileFormat::Jpeg, FileFormat::Png];

    /// File extensions of the format, the preferred one first.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileFormat::Tiff => &["tif", "tiff"],
            FileFormat::Jpeg => &["jpg", "jpeg"],
            FileFormat::Png => &["png"],
        }
    }

    /// The format whose extension is `ext` (any case).
    pub fn from_extension(ext: &str) -> Option<FileFormat> {
        let ext = ext.to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Extensions of all supported formats.
    pub fn all_extensions() -> Vec<&'static str> {
        Self::ALL.iter().flat_map(|f| f.extensions().iter().copied()).collect()
    }

    /// Preferred extension, for new files.
    pub fn extension(self) -> &'static str {
        self.extensions()[0]
    }

    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Tiff => "TIFF",
            FileFormat::Jpeg => "JPEG",
            FileFormat::Png => "PNG",
        }
    }
}

/// Sample depth in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitDepth {
    U8,
    U16,
    F32,
}

impl BitDepth {
    pub fn label(self) -> &'static str {
        match self {
            BitDepth::U8 => "8 bit",
            BitDepth::U16 => "16 bit",
            BitDepth::F32 => "32-bit float",
        }
    }

    /// Bytes per sample.
    pub fn bytes(self) -> u64 {
        match self {
            BitDepth::U8 => 1,
            BitDepth::U16 => 2,
            BitDepth::F32 => 4,
        }
    }

    /// Maximum value for showing samples on the file's scale (1 for floating
    /// point).
    pub fn scale(self) -> f32 {
        match self {
            BitDepth::U8 => 255.0,
            BitDepth::U16 => 65535.0,
            BitDepth::F32 => 1.0,
        }
    }
}

/// Print resolution, in pixels per inch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dpi {
    pub x: f64,
    pub y: f64,
}

impl Dpi {
    /// `None` unless both values are plausible (1 to 100,000 per inch).
    pub fn new(x: f64, y: f64) -> Option<Self> {
        let ok = |v: f64| (1.0..=100_000.0).contains(&v);
        (ok(x) && ok(y)).then_some(Self { x, y })
    }
}

/// How the source file was made: needed to write it back faithfully.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub format: FileFormat,
    pub bit_depth: BitDepth,
    /// Greyscale image (processed as RGB and written back as grey).
    pub gray: bool,
    pub icc: Option<Vec<u8>>,
    /// EXIF block (TIFF structure, without the `Exif\0\0` header); for TIFF
    /// sources, built from the file's descriptive tags and EXIF/GPS sub-IFDs.
    pub exif: Option<Vec<u8>>,
    /// XMP packet, without the JPEG namespace header.
    pub xmp: Option<Vec<u8>>,
    /// The rest of a JPEG's XMP that did not fit in one segment ("extended
    /// XMP"), named by its MD5 in the main packet; kept as it is.
    pub xmp_extended: Option<Vec<u8>>,
    /// Print resolution, when the source gives one.
    pub dpi: Option<Dpi>,
    /// Notes to show the user (e.g. ignored pages).
    pub notes: Vec<String>,
}

/// Image read from a file.
#[derive(Clone, Debug)]
pub struct Picture {
    pub rgb: Image,
    /// Alpha channel, one value per pixel, if present.
    pub alpha: Option<Vec<f32>>,
    pub info: SourceInfo,
}

/// Format requested on export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportFormat {
    /// Same format and bit depth as the original.
    #[default]
    SameAsSource,
    Tiff16,
    Jpeg,
    Png16,
}

/// Write settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// JPEG quality 1..=100.
    pub jpeg_quality: u8,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { format: ExportFormat::SameAsSource, jpeg_quality: 95 }
    }
}

/// Largest image accepted when reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadLimits {
    /// Width × height.
    pub max_pixels: u64,
}

impl ReadLimits {
    /// The absolute ceiling: one gigapixel.
    pub const MAX_PIXELS: u64 = 1 << 30;

    /// Rejects `width × height` beyond the limit.
    ///
    /// # Errors
    /// [`IoError::TooManyPixels`].
    pub fn check(&self, width: u32, height: u32) -> Result<(), IoError> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.max_pixels {
            Err(IoError::TooManyPixels { width, height, max_mpx: self.max_pixels / 1_000_000 })
        } else {
            Ok(())
        }
    }
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self { max_pixels: Self::MAX_PIXELS }
    }
}

/// Recognises the format from the first bytes of the file.
pub fn sniff(head: &[u8]) -> Option<FileFormat> {
    match head {
        [b'I', b'I', 42, 0, ..] | [b'M', b'M', 0, 42, ..] => Some(FileFormat::Tiff),
        [0xFF, 0xD8, 0xFF, ..] => Some(FileFormat::Jpeg),
        [0x89, b'P', b'N', b'G', ..] => Some(FileFormat::Png),
        _ => None,
    }
}

/// Format of the file at `path`, read from its content; `None` if unsupported
/// or unreadable.
pub fn detect(path: &Path) -> Option<FileFormat> {
    let mut head = [0u8; 8];
    let n = File::open(path).and_then(|mut f| f.read(&mut head)).ok()?;
    sniff(head.get(..n)?)
}

/// Reads an image.
///
/// # Errors
/// [`IoError::UnknownFormat`] if the file is not TIFF, JPEG or PNG;
/// [`IoError::TooManyPixels`] if it is larger than `limits`; other
/// [`IoError`]s if it is damaged or uses unsupported variants.
pub fn read(path: &Path, limits: &ReadLimits) -> Result<Picture, IoError> {
    match detect(path) {
        Some(FileFormat::Tiff) => tiff_io::read(path, limits),
        Some(f @ (FileFormat::Jpeg | FileFormat::Png)) => raster::read(path, f, limits),
        None => {
            // tell "does not exist" from "unknown format"
            File::open(path)?;
            Err(IoError::UnknownFormat)
        }
    }
}

/// Format and bit depth a file will be written with.
pub fn resolve(info: &SourceInfo, opts: &ExportOptions) -> (FileFormat, BitDepth) {
    match opts.format {
        ExportFormat::SameAsSource => match info.format {
            FileFormat::Jpeg => (FileFormat::Jpeg, BitDepth::U8),
            FileFormat::Png => {
                (FileFormat::Png, if info.bit_depth == BitDepth::U8 { BitDepth::U8 } else { BitDepth::U16 })
            }
            FileFormat::Tiff => (FileFormat::Tiff, info.bit_depth),
        },
        ExportFormat::Tiff16 => (FileFormat::Tiff, BitDepth::U16),
        ExportFormat::Jpeg => (FileFormat::Jpeg, BitDepth::U8),
        ExportFormat::Png16 => (FileFormat::Png, BitDepth::U16),
    }
}

/// Export format and final path for a name chosen in a save dialog: the
/// format follows the extension (none or an unknown one: TIFF, with its
/// extension added); the source's own format keeps its bit depth.
pub fn export_choice(chosen: &Path, source: FileFormat) -> (PathBuf, ExportFormat) {
    let known = chosen.extension().and_then(|e| FileFormat::from_extension(&e.to_string_lossy()));
    let format = known.unwrap_or(FileFormat::Tiff);
    let export = match format {
        f if f == source => ExportFormat::SameAsSource,
        FileFormat::Jpeg => ExportFormat::Jpeg,
        FileFormat::Png => ExportFormat::Png16,
        FileFormat::Tiff => ExportFormat::Tiff16,
    };
    let path = if known.is_some() { chosen.to_path_buf() } else { chosen.with_extension(format.extension()) };
    (path, export)
}

/// Writes `rgb` (and `alpha`) to `path` with the metadata of `info`.
///
/// Writing is atomic: if it fails, `path` is left untouched.
///
/// # Errors
/// [`IoError`] if encoding or the disk fails.
pub fn write(
    path: &Path,
    rgb: &Image,
    alpha: Option<&[f32]>,
    info: &SourceInfo,
    opts: &ExportOptions,
) -> Result<(), IoError> {
    let alpha = alpha.filter(|a| a.len() == rgb.width() * rgb.height());
    write_from(path, &mut WholeImage { rgb, alpha, given: false }, info, opts)
}

/// The rows of an image to write, from the top: the whole image at once, or
/// band by band when it is produced that way (an enlargement, which may not
/// fit in memory as a whole).
pub trait BandSource {
    /// What producing a band can fail with; writing errors convert into it.
    type Error: From<IoError>;

    fn width(&self) -> usize;

    fn height(&self) -> usize;

    /// `true` if every band comes with its alpha samples.
    fn has_alpha(&self) -> bool;

    /// The next band of whole rows, from the top; `None` after the last.
    ///
    /// # Errors
    /// Whatever stops the production (cancellation, a failed network).
    fn next_band(&mut self) -> Result<Option<Band<'_>>, Self::Error>;
}

/// Some whole rows of an image: RGB, and alpha (one sample per pixel) when
/// the source has it.
pub struct Band<'a> {
    pub rgb: &'a Image,
    pub alpha: Option<&'a [f32]>,
}

/// An image in memory, as a single band.
struct WholeImage<'a> {
    rgb: &'a Image,
    alpha: Option<&'a [f32]>,
    given: bool,
}

impl BandSource for WholeImage<'_> {
    type Error = IoError;

    fn width(&self) -> usize {
        self.rgb.width()
    }

    fn height(&self) -> usize {
        self.rgb.height()
    }

    fn has_alpha(&self) -> bool {
        self.alpha.is_some()
    }

    fn next_band(&mut self) -> Result<Option<Band<'_>>, IoError> {
        if std::mem::replace(&mut self.given, true) {
            return Ok(None);
        }
        Ok(Some(Band { rgb: self.rgb, alpha: self.alpha }))
    }
}

/// Writes the rows of `source` to `path` with the metadata of `info`, as
/// they are produced: only the bands being encoded are in memory (for JPEG,
/// whose encoder takes the whole image, its 8-bit samples). The limits of
/// the format are checked first ([`check_output`]).
///
/// Writing is atomic: if it fails or the source stops, `path` is left
/// untouched.
///
/// # Errors
/// The source's error, or an [`IoError`] (limits, encoding, disk).
pub fn write_from<S: BandSource>(
    path: &Path,
    source: &mut S,
    info: &SourceInfo,
    opts: &ExportOptions,
) -> Result<(), S::Error> {
    let (format, depth) = resolve(info, opts);
    check_output(source.width(), source.height(), source.has_alpha(), info, opts)?;
    atomic_write(path, |w| match format {
        FileFormat::Tiff => tiff_io::write(w, source, info, depth),
        FileFormat::Jpeg => raster::write_jpeg(w, source, info, opts.jpeg_quality),
        FileFormat::Png => raster::write_png(w, source, info, depth),
    })
}

/// Whether an image of `width × height` (with alpha or not) can be written
/// with these options, checked before any work: JPEG takes at most 65,535 px
/// per side; a TIFF file must stay under 4 GB, so its uncompressed samples
/// must too.
///
/// # Errors
/// [`IoError::TooLarge`] or [`IoError::FileTooLarge`].
pub fn check_output(
    width: usize,
    height: usize,
    has_alpha: bool,
    info: &SourceInfo,
    opts: &ExportOptions,
) -> Result<(), IoError> {
    let (format, depth) = resolve(info, opts);
    let side = |max: u32| usize::try_from(max).map_or(true, |m| width <= m && height <= m);
    match format {
        FileFormat::Jpeg if !side(raster::MAX_JPEG_PX) => {
            Err(IoError::TooLarge { format: "JPEG", max: raster::MAX_JPEG_PX })
        }
        FileFormat::Png if !side(u32::MAX) => Err(IoError::TooLarge { format: "PNG", max: u32::MAX }),
        FileFormat::Tiff => {
            let channels: u64 = if info.gray && !has_alpha {
                1
            } else if has_alpha {
                4
            } else {
                3
            };
            let bytes = u64::try_from(width)
                .ok()
                .zip(u64::try_from(height).ok())
                .and_then(|(w, h)| w.checked_mul(h)?.checked_mul(channels)?.checked_mul(depth.bytes()));
            if bytes.is_none_or(|b| b >= TIFF_MAX_BYTES) || !side(u32::MAX) {
                return Err(IoError::FileTooLarge { format: "TIFF", max_gb: 4 });
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Uncompressed samples a TIFF may hold: under 4 GiB, with room for the
/// metadata (classic TIFF addresses its content with 32 bits).
const TIFF_MAX_BYTES: u64 = (1 << 32) - (64 << 20);

/// Output path that never overwrites anything: `dir/photo<suffix>.ext`,
/// then `dir/photo<suffix> 2.ext`, `… 3.ext` and so on.
pub fn output_path(source: &Path, dir: &Path, suffix: &str, extension: &str) -> PathBuf {
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
    let mut candidate = dir.join(format!("{stem}{suffix}.{extension}"));
    let mut n = 2u32;
    while candidate.exists() || candidate == source {
        candidate = dir.join(format!("{stem}{suffix} {n}.{extension}"));
        n = n.saturating_add(1);
    }
    candidate
}

/// Writes `path` through a temporary file next to it: created exclusively
/// (an existing file is never truncated), flushed and synced, then renamed
/// over `path` durably (the new name survives a power cut). On any error
/// the temporary file is removed.
fn atomic_write<E: From<IoError>>(
    path: &Path,
    encode: impl FnOnce(&mut BufWriter<File>) -> Result<(), E>,
) -> Result<(), E> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (tmp, file) = create_temporary(dir, &name)?;
    let result = (|| {
        let mut w = BufWriter::new(file);
        encode(&mut w)?;
        w.flush().map_err(IoError::Io)?;
        w.get_ref().sync_all().map_err(IoError::Io)?;
        drop(w);
        denoise_platform::rename_durably(&tmp, path).map_err(IoError::Io)?;
        Ok(())
    })();
    if result.is_err() {
        // best effort: the error returned is the one that matters
        std::fs::remove_file(&tmp).ok();
    }
    result
}

/// Attempts at a free temporary name before giving up.
const TEMPORARY_ATTEMPTS: u32 = 100;

/// A new, empty file `.<name>.<process>-<n>.tmp` in `dir`: the next number
/// when a file of that name exists (one left by a crash, say).
fn create_temporary(dir: &Path, name: &str) -> Result<(PathBuf, File), IoError> {
    for n in 0..TEMPORARY_ATTEMPTS {
        let tmp = dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(IoError::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "no free name for a temporary file")))
}

/// Whether 8-bit output from this source is dithered: only when the source
/// had more than 8 bits, so an 8-bit file written back unchanged stays
/// identical.
pub(crate) fn dither_to_8bit(info: &SourceInfo) -> bool {
    info.bit_depth != BitDepth::U8
}

/// Reduces an RGB image to grey (mean of the channels): used to write
/// greyscale sources back as grey.
pub(crate) fn gray_of(rgb: &Image) -> Vec<f32> {
    rgb.as_slice().as_chunks::<3>().0.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).collect()
}

/// The samples of a band as a file stores them, pixel after pixel: grey or
/// RGB, followed by alpha when the band has it.
pub(crate) fn band_samples(band: &Band<'_>, gray: bool) -> Vec<f32> {
    match (gray, band.alpha) {
        (true, Some(a)) => gray_of(band.rgb).into_iter().zip(a).flat_map(|(g, &al)| [g, al]).collect(),
        (true, None) => gray_of(band.rgb),
        (false, Some(a)) => {
            band.rgb.as_slice().as_chunks::<3>().0.iter().zip(a).flat_map(|(p, &al)| [p[0], p[1], p[2], al]).collect()
        }
        (false, None) => band.rgb.as_slice().to_vec(),
    }
}

/// Error for a source whose bands do not add up to its height.
pub(crate) fn rows_mismatch(rows: usize, height: usize) -> IoError {
    IoError::Encode(format!("the image has {rows} rows instead of {height}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_by_content() {
        assert_eq!(sniff(b"II*\0abcd"), Some(FileFormat::Tiff));
        assert_eq!(sniff(b"MM\0*abcd"), Some(FileFormat::Tiff));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(FileFormat::Jpeg));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Some(FileFormat::Png));
        assert_eq!(sniff(b"GIF89a"), None);
    }

    fn plain_info(format: FileFormat, bit_depth: BitDepth, gray: bool) -> SourceInfo {
        SourceInfo {
            format,
            bit_depth,
            gray,
            icc: None,
            exif: None,
            xmp: None,
            xmp_extended: None,
            dpi: None,
            notes: vec![],
        }
    }

    /// An image handed over in bands of `rows` rows, as an enlargement is.
    struct Split<'a> {
        rgb: &'a Image,
        alpha: Option<&'a [f32]>,
        rows: usize,
        next: usize,
        band: Image,
        band_alpha: Vec<f32>,
    }

    impl BandSource for Split<'_> {
        type Error = IoError;
        fn width(&self) -> usize {
            self.rgb.width()
        }
        fn height(&self) -> usize {
            self.rgb.height()
        }
        fn has_alpha(&self) -> bool {
            self.alpha.is_some()
        }
        fn next_band(&mut self) -> Result<Option<Band<'_>>, IoError> {
            let (w, h) = (self.rgb.width(), self.rgb.height());
            if self.next >= h {
                return Ok(None);
            }
            let r = self.rows.min(h - self.next);
            self.band = self.rgb.crop(denoise_core::PixelRect::new(0, self.next, w, r));
            if let Some(a) = self.alpha {
                self.band_alpha = a[self.next * w..(self.next + r) * w].to_vec();
            }
            self.next += r;
            Ok(Some(Band { rgb: &self.band, alpha: self.alpha.map(|_| self.band_alpha.as_slice()) }))
        }
    }

    #[test]
    fn written_in_bands_or_whole_the_file_is_the_same() {
        let (w, h) = (37, 29);
        let data = (0..w * h * 3).map(|i| ((i * 7919 + 13) % 1009) as f32 / 1008.0).collect();
        let rgb = Image::from_rgb(w, h, data).unwrap();
        let alpha: Vec<f32> = (0..w * h).map(|i| (i % 11) as f32 / 10.0).collect();
        let dir = tempfile::tempdir().unwrap();
        let cases = [
            (FileFormat::Tiff, BitDepth::U8, false, true),
            (FileFormat::Tiff, BitDepth::U16, false, false),
            (FileFormat::Tiff, BitDepth::F32, true, false),
            (FileFormat::Tiff, BitDepth::U16, true, true),
            (FileFormat::Png, BitDepth::U8, false, true),
            (FileFormat::Png, BitDepth::U16, true, false),
            (FileFormat::Jpeg, BitDepth::U8, false, false),
            (FileFormat::Jpeg, BitDepth::U8, true, false),
        ];
        for (k, (format, bit_depth, gray, with_alpha)) in cases.into_iter().enumerate() {
            let info = plain_info(format, bit_depth, gray);
            let opts = ExportOptions::default();
            let a = with_alpha.then_some(alpha.as_slice());
            let whole = dir.path().join(format!("whole{k}.{}", format.extension()));
            let banded = dir.path().join(format!("banded{k}.{}", format.extension()));
            write(&whole, &rgb, a, &info, &opts).unwrap();
            let mut split = Split { rgb: &rgb, alpha: a, rows: 4, next: 0, band: Image::default(), band_alpha: vec![] };
            write_from(&banded, &mut split, &info, &opts).unwrap();
            assert_eq!(std::fs::read(&whole).unwrap(), std::fs::read(&banded).unwrap(), "case {k}");
        }
    }

    #[test]
    fn output_limits_are_checked_first() {
        let info = |format, bit_depth| plain_info(format, bit_depth, false);
        let opts = ExportOptions::default();
        assert!(check_output(65_535, 100, false, &info(FileFormat::Jpeg, BitDepth::U8), &opts).is_ok());
        assert!(matches!(
            check_output(65_536, 100, false, &info(FileFormat::Jpeg, BitDepth::U8), &opts),
            Err(IoError::TooLarge { format: "JPEG", .. })
        ));
        // 24 MP enlarged 4 times: fine in 16 bit, over 4 GB in floating point
        let tiff16 = info(FileFormat::Tiff, BitDepth::U16);
        assert!(check_output(24_000, 16_000, false, &tiff16, &opts).is_ok());
        assert!(matches!(
            check_output(24_000, 16_000, false, &info(FileFormat::Tiff, BitDepth::F32), &opts),
            Err(IoError::FileTooLarge { .. })
        ));
        // a source whose bands fall short is an error, and leaves no file
        let dir = tempfile::tempdir().unwrap();
        let rgb = Image::new(10, 10);
        let mut short = Split { rgb: &rgb, alpha: None, rows: 3, next: 4, band: Image::default(), band_alpha: vec![] };
        let out = dir.path().join("short.tif");
        assert!(write_from(&out, &mut short, &tiff16, &opts).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn writing_never_truncates_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("photo.png");
        // a temporary file left by an earlier crash, with the first name tried
        let left = dir.path().join(format!(".photo.png.{}-0.tmp", std::process::id()));
        std::fs::write(&left, b"keep me").unwrap();
        atomic_write(&out, |w| w.write_all(b"new").map_err(IoError::Io)).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        assert_eq!(std::fs::read(&left).unwrap(), b"keep me");
        // a failed write leaves neither the output nor a temporary file
        let failed = dir.path().join("failed.png");
        assert!(atomic_write(&failed, |_| Err(IoError::Encode("test".into()))).is_err());
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn extensions_map_back_to_their_format() {
        for f in FileFormat::ALL {
            for e in f.extensions() {
                assert_eq!(FileFormat::from_extension(e), Some(f));
                assert_eq!(FileFormat::from_extension(&e.to_uppercase()), Some(f));
            }
        }
        assert_eq!(FileFormat::from_extension("gif"), None);
        assert_eq!(FileFormat::all_extensions().len(), 5);
    }

    #[test]
    fn export_choice_follows_the_extension() {
        let (p, f) = export_choice(Path::new("out/a.JPG"), FileFormat::Tiff);
        assert_eq!((p, f), (PathBuf::from("out/a.JPG"), ExportFormat::Jpeg));
        assert_eq!(export_choice(Path::new("a.tif"), FileFormat::Tiff).1, ExportFormat::SameAsSource);
        assert_eq!(export_choice(Path::new("a.png"), FileFormat::Jpeg).1, ExportFormat::Png16);
        // no or unknown extension: TIFF, extension added
        assert_eq!(export_choice(Path::new("a"), FileFormat::Jpeg), (PathBuf::from("a.tif"), ExportFormat::Tiff16));
        assert_eq!(export_choice(Path::new("a.gif"), FileFormat::Png).0, PathBuf::from("a.tif"));
    }

    #[test]
    fn limits_reject_large_images() {
        let l = ReadLimits { max_pixels: 1000 };
        assert!(l.check(40, 25).is_ok());
        assert!(matches!(l.check(40, 26), Err(IoError::TooManyPixels { width: 40, height: 26, .. })));
        assert!(ReadLimits::default().check(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn output_path_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("photo.tif");
        std::fs::write(&src, b"x").unwrap();
        let first = output_path(&src, dir.path(), "_dn", "tif");
        assert_eq!(first.file_name().unwrap(), "photo_dn.tif");
        std::fs::write(&first, b"x").unwrap();
        let second = output_path(&src, dir.path(), "_dn", "tif");
        assert_eq!(second.file_name().unwrap(), "photo_dn 2.tif");
        // empty suffix in the same folder: never the original
        let same = output_path(&src, dir.path(), "", "tif");
        assert_eq!(same.file_name().unwrap(), "photo 2.tif");
    }
}
