//! TIFF: reading and writing.

use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Seek, Write};
use std::path::Path;

use denoise_core::quant::{from_u16, from_u8, quantize_u8_at, to_u16};
use denoise_core::{num, Image};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::encoder::colortype::{self, ColorType as EncColor};
use tiff::encoder::compression::Deflate;
use tiff::encoder::{Rational, TiffEncoder, TiffValue};
use tiff::tags::{ResolutionUnit, Tag, Type};
use tiff::ColorType;

use crate::meta::{self, Metadata, TAG_EXIF_IFD, TAG_GPS_IFD, TYPE_ASCII};
use crate::{
    band_samples, dither_to_8bit, orient, rows_mismatch, BandSource, BitDepth, Dpi, FileFormat, IoError, Picture,
    ReadLimits, SourceInfo,
};

const TAG_ICC: u16 = 34675;
const TAG_ORIENTATION: u16 = 274;
/// Largest tag value the decoder reads (ICC profiles of printers reach a few MB).
const MAX_TAG_BYTES: usize = 16 << 20;
/// Bytes per pixel of the largest decoded layout (4 channels of 64 bits).
const MAX_DECODED_BYTES_PER_PX: usize = 32;

fn decode_err(e: tiff::TiffError) -> IoError {
    match e {
        tiff::TiffError::UnsupportedError(u) => IoError::Unsupported(format!("This TIFF variant ({u})")),
        tiff::TiffError::IoError(io) => IoError::Io(io),
        other => IoError::Decode(other.to_string()),
    }
}

fn encode_err(e: tiff::TiffError) -> IoError {
    match e {
        tiff::TiffError::IoError(io) => IoError::Io(io),
        other => IoError::Encode(other.to_string()),
    }
}

pub fn read(path: &Path, limits: &ReadLimits) -> Result<Picture, IoError> {
    let file = BufReader::new(File::open(path)?);
    let mut dec = Decoder::new(file).map_err(decode_err)?;
    let (w, h) = dec.dimensions().map_err(decode_err)?;
    limits.check(w, h)?;
    let (w, h) = (w as usize, h as usize);
    // the size is now known to be acceptable: let the decoder allocate it
    let mut decoder_limits = Limits::default();
    decoder_limits.decoding_buffer_size = (w * h).saturating_mul(MAX_DECODED_BYTES_PER_PX);
    decoder_limits.intermediate_buffer_size = decoder_limits.decoding_buffer_size;
    decoder_limits.ifd_value_size = MAX_TAG_BYTES;
    let mut dec = dec.with_limits(decoder_limits);
    let color = dec.colortype().map_err(decode_err)?;
    let (channels, gray, has_alpha) = match color {
        ColorType::Gray(_) => (1, true, false),
        ColorType::GrayA(_) => (2, true, true),
        ColorType::RGB(_) => (3, false, false),
        ColorType::RGBA(_) => (4, false, true),
        ColorType::CMYK(_) => return Err(IoError::Unsupported("CMYK TIFF".into())),
        ColorType::Palette(_) => return Err(IoError::Unsupported("Palette (indexed colour) TIFF".into())),
        other => return Err(IoError::Unsupported(format!("TIFF with colour model {other:?}"))),
    };
    let mut tag = |n: u16| dec.find_tag(Tag::from_u16_exhaustive(n)).ok().flatten();
    let icc = tag(TAG_ICC).and_then(|v| v.into_u8_vec().ok());
    let orientation = tag(TAG_ORIENTATION).and_then(|v| v.into_u16().ok()).unwrap_or(1);

    let samples = dec.read_image().map_err(decode_err)?;
    let mut notes = Vec::new();
    let pages = extra_full_pages(&mut dec);
    if pages > 0 {
        let what = if pages == 1 {
            "another full-resolution image".to_owned()
        } else {
            format!("{pages} more full-resolution images")
        };
        notes.push(format!("The file contains {what} (pages): only the first one is processed and exported."));
    }
    let (bit_depth, values): (BitDepth, Vec<f32>) = match samples {
        DecodingResult::U8(v) => (BitDepth::U8, v.into_iter().map(from_u8).collect()),
        DecodingResult::U16(v) => (BitDepth::U16, v.into_iter().map(from_u16).collect()),
        DecodingResult::F32(v) => (BitDepth::F32, v),
        // 32-bit integer and 64-bit float: rare; written back as 32-bit float
        DecodingResult::F64(v) => (BitDepth::F32, v.into_iter().map(num::narrow).collect()),
        DecodingResult::U32(v) => {
            (BitDepth::F32, v.into_iter().map(|x| num::narrow(f64::from(x) / f64::from(u32::MAX))).collect())
        }
        _ => return Err(IoError::Unsupported("This TIFF bit depth".into())),
    };
    if values.len() < w * h * channels {
        return Err(IoError::Decode("incomplete pixel data".into()));
    }

    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut alpha = has_alpha.then(|| Vec::with_capacity(w * h));
    for px in values.chunks_exact(channels).take(w * h) {
        match (channels, alpha.as_mut()) {
            (1, _) => rgb.extend_from_slice(&[px[0]; 3]),
            (2, Some(a)) => {
                rgb.extend_from_slice(&[px[0]; 3]);
                a.push(px[1]);
            }
            (4, Some(a)) => {
                rgb.extend_from_slice(&px[..3]);
                a.push(px[3]);
            }
            _ => rgb.extend_from_slice(&px[..3]),
        }
    }
    let image = Image::from_rgb(w, h, rgb).map_err(|e| IoError::Decode(e.to_string()))?;
    let (image, alpha) = orient::apply(image, alpha, orientation);
    // metadata straight from the directories (the decoder does not read sub-IFDs)
    let (metadata, xmp) = File::open(path).ok().map(BufReader::new).and_then(meta::from_tiff).unzip();
    let dpi = metadata.as_ref().and_then(|m| m.dpi);
    let exif = metadata.filter(|m| !m.is_empty()).map(|m| meta::to_exif(&m));
    let info = SourceInfo {
        format: FileFormat::Tiff,
        bit_depth,
        gray,
        icc,
        exif,
        xmp: xmp.flatten(),
        xmp_extended: None,
        dpi,
        notes,
    };
    Ok(Picture { rgb: image, alpha, info })
}

const TAG_NEW_SUBFILE_TYPE: u16 = 254;

/// Pages after the first one that are not thumbnails.
///
/// Many programs (Canon DPP, Photoshop…) add a reduced preview as a second
/// IFD, flagged by bit 0 of NewSubfileType: that is not a page.
fn extra_full_pages<R: std::io::Read + Seek>(dec: &mut Decoder<R>) -> usize {
    let mut pages = 0;
    while dec.more_images() && dec.next_image().is_ok() {
        let reduced = dec
            .find_tag(Tag::from_u16_exhaustive(TAG_NEW_SUBFILE_TYPE))
            .ok()
            .flatten()
            .and_then(|v| v.into_u32().ok())
            .is_some_and(|v| v & 1 == 1);
        if !reduced {
            pages += 1;
        }
    }
    pages
}

/// Tag value of type UNDEFINED (as the specification requires for the ICC profile).
struct Undefined<'a>(&'a [u8]);

impl TiffValue for Undefined<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: Type = Type::UNDEFINED;

    fn count(&self) -> usize {
        self.0.len()
    }

    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.0)
    }
}

/// Tag value of type ASCII, with its terminating NUL already in the bytes.
struct Ascii<'a>(&'a [u8]);

impl TiffValue for Ascii<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: Type = Type::ASCII;

    fn count(&self) -> usize {
        self.0.len()
    }

    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.0)
    }
}

// the tiff encoder writes in the machine's byte order, the metadata is kept
// little-endian: the two must agree
const _: () = assert!(cfg!(target_endian = "little"), "TIFF metadata writing assumes a little-endian machine");

/// Everything besides the pixels written into the TIFF.
struct Tags<'a> {
    icc: Option<&'a [u8]>,
    meta: Option<Metadata>,
    /// One packet: a JPEG's extended XMP is merged in.
    xmp: Option<Cow<'a, [u8]>>,
    dpi: Option<Dpi>,
}

fn put<C, W>(enc: &mut TiffEncoder<W>, w: u32, h: u32, data: &[C::Inner], tags: &Tags) -> Result<(), IoError>
where
    C: EncColor,
    [C::Inner]: TiffValue,
    W: Write + Seek,
{
    let mut image = enc.new_image_with_compression::<C, _>(w, h, Deflate::default()).map_err(encode_err)?;
    if let Some(d) = tags.dpi {
        // hundredths of a pixel per inch: enough for any real value
        let rational = |v: f64| Rational { n: num::round_to_u32(v * 100.0, 1), d: 100 };
        image.x_resolution(rational(d.x));
        image.y_resolution(rational(d.y));
        image.resolution_unit(ResolutionUnit::Inch);
    }
    let dir = image.encoder();
    if let Some(icc) = tags.icc {
        dir.write_tag(Tag::Unknown(TAG_ICC), Undefined(icc)).map_err(encode_err)?;
    }
    if let Some(xmp) = tags.xmp.as_deref() {
        dir.write_tag(Tag::Unknown(meta::TAG_XMP), xmp).map_err(encode_err)?;
    }
    if let Some(m) = &tags.meta {
        for e in m.main.iter().filter(|e| e.typ == TYPE_ASCII) {
            dir.write_tag(Tag::Unknown(e.tag), Ascii(&e.data)).map_err(encode_err)?;
        }
        // sub-IFDs go in the file as plain data, before the pixels; the
        // first IFD points to them
        for (tag, entries) in [(TAG_EXIF_IFD, &m.exif), (TAG_GPS_IFD, &m.gps)] {
            if entries.is_empty() {
                continue;
            }
            let mut at = dir.write_data(&[] as &[u8]).map_err(encode_err)?;
            if at % 2 == 1 {
                at = dir.write_data(&[0u8] as &[u8]).map_err(encode_err)? + 1; // word boundary
            }
            let at = u32::try_from(at).map_err(|_| IoError::Encode("file larger than 4 GB".into()))?;
            dir.write_data(meta::ifd_at(entries, at).as_slice()).map_err(encode_err)?;
            dir.write_tag(Tag::Unknown(tag), at).map_err(encode_err)?;
        }
    }
    image.write_data(data).map_err(encode_err)
}

/// All the samples of `source` as the file stores them, converted as the
/// bands arrive (`convert` gets the index of the band's first sample, for
/// the dither): the TIFF encoder compresses only a whole image, so the
/// converted samples are gathered, never the floating-point ones.
fn gather<S: BandSource, T>(
    source: &mut S,
    gray: bool,
    convert: impl Fn(&[f32], usize) -> Vec<T>,
) -> Result<Vec<T>, S::Error> {
    let (height, mut rows) = (source.height(), 0);
    let mut out = Vec::new();
    while let Some(band) = source.next_band()? {
        out.extend(convert(&band_samples(&band, gray), out.len()));
        rows += band.rgb.height();
    }
    if rows != height {
        return Err(rows_mismatch(rows, height).into());
    }
    Ok(out)
}

pub fn write<W: Write + Seek, S: BandSource>(
    out: &mut W,
    source: &mut S,
    info: &SourceInfo,
    depth: BitDepth,
) -> Result<(), S::Error> {
    let (w, h) = (
        u32::try_from(source.width()).map_err(|_| IoError::TooLarge { format: "TIFF", max: u32::MAX })?,
        u32::try_from(source.height()).map_err(|_| IoError::TooLarge { format: "TIFF", max: u32::MAX })?,
    );
    let has_alpha = source.has_alpha();
    // grey with alpha is written as RGBA (grey+alpha TIFF is poorly supported)
    let gray = info.gray && !has_alpha;
    // an RGB ICC profile on a grey TIFF would be inconsistent: write it only if it fits
    let icc = info.icc.as_deref().filter(|p| {
        let cs = crate::icc::color_space(p);
        if gray {
            cs == Some(b"GRAY")
        } else {
            cs != Some(b"GRAY")
        }
    });
    let tags = Tags {
        icc,
        meta: info.exif.as_deref().and_then(meta::from_exif).map(|mut m| {
            m.set_pixel_dimensions(w, h);
            m
        }),
        xmp: crate::raster::whole_xmp(info),
        dpi: info.dpi,
    };
    let dither = dither_to_8bit(info);
    let mut enc = TiffEncoder::new(out).map_err(encode_err)?;
    let e = &mut enc;
    match depth {
        BitDepth::U8 => {
            let d = gather(source, gray, |s, first| quantize_u8_at(s, first, dither))?;
            Ok(match (gray, has_alpha) {
                (true, _) => put::<colortype::Gray8, _>(e, w, h, &d, &tags),
                (false, true) => put::<colortype::RGBA8, _>(e, w, h, &d, &tags),
                (false, false) => put::<colortype::RGB8, _>(e, w, h, &d, &tags),
            }?)
        }
        BitDepth::U16 => {
            let d: Vec<u16> = gather(source, gray, |s, _| s.iter().map(|&v| to_u16(v)).collect())?;
            Ok(match (gray, has_alpha) {
                (true, _) => put::<colortype::Gray16, _>(e, w, h, &d, &tags),
                (false, true) => put::<colortype::RGBA16, _>(e, w, h, &d, &tags),
                (false, false) => put::<colortype::RGB16, _>(e, w, h, &d, &tags),
            }?)
        }
        BitDepth::F32 => {
            let d = gather(source, gray, |s, _| s.to_vec())?;
            Ok(match (gray, has_alpha) {
                (true, _) => put::<colortype::Gray32Float, _>(e, w, h, &d, &tags),
                (false, true) => put::<colortype::RGBA32Float, _>(e, w, h, &d, &tags),
                (false, false) => put::<colortype::RGB32Float, _>(e, w, h, &d, &tags),
            }?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write as write_file, ExportFormat, ExportOptions};

    fn gradient(w: usize, h: usize) -> Image {
        let data = (0..w * h)
            .flat_map(|i| {
                let t = i as f32 / (w * h) as f32;
                [t, 1.0 - t, 0.5]
            })
            .collect();
        Image::from_rgb(w, h, data).unwrap()
    }

    fn info(depth: BitDepth, gray: bool) -> SourceInfo {
        SourceInfo {
            format: FileFormat::Tiff,
            bit_depth: depth,
            gray,
            icc: Some(crate::icc::sample_v2("Test RGB")),
            exif: None,
            xmp: None,
            xmp_extended: None,
            dpi: None,
            notes: vec![],
        }
    }

    #[test]
    fn exif_gps_and_xmp_survive_tiff_and_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let img = gradient(8, 8);
        let mut src = info(BitDepth::U16, false);
        let sample = crate::meta::tests::sample();
        src.exif = Some(meta::to_exif(&sample));
        let xmp = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>rating 5</x:xmpmeta>".to_vec();
        src.xmp = Some(xmp.clone());
        let p = dir.path().join("m.tif");
        write_file(&p, &img, None, &src, &ExportOptions::default()).unwrap();
        let back = read(&p, &ReadLimits::default()).unwrap();
        assert_eq!(back.info.xmp.as_deref(), Some(xmp.as_slice()));
        assert_eq!(back.info.exif.as_deref().and_then(meta::from_exif), Some(sample.clone()));
        // the pixels are still readable by the decoder, and the sub-IFDs are not pages
        assert!(back.info.notes.is_empty(), "{:?}", back.info.notes);
        let err = img.as_slice().iter().zip(back.rgb.as_slice()).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err <= 0.5 / 65535.0 + 1e-7);
        // TIFF → JPEG: the EXIF block goes along
        let j = dir.path().join("m.jpg");
        let opts = ExportOptions { format: ExportFormat::Jpeg, ..Default::default() };
        write_file(&j, &img, None, &back.info, &opts).unwrap();
        let jpeg = crate::read(&j, &ReadLimits::default()).unwrap();
        assert_eq!(jpeg.info.exif.as_deref().and_then(meta::from_exif), Some(sample));
        // ... and so does the XMP packet, read back from the JPEG
        assert_eq!(jpeg.info.xmp.as_deref(), Some(xmp.as_slice()));
        // JPEG → JPEG keeps it too
        let j2 = dir.path().join("m2.jpg");
        write_file(&j2, &img, None, &jpeg.info, &opts).unwrap();
        let again = crate::read(&j2, &ReadLimits::default()).unwrap();
        assert_eq!(again.info.xmp.as_deref(), Some(xmp.as_slice()));
    }

    #[test]
    fn exif_pixel_dimensions_are_those_of_the_file_written() {
        let dir = tempfile::tempdir().unwrap();
        // a photo of 8 × 6 according to its EXIF, written as 16 × 12 (as an
        // enlargement would), from an EXIF block in either byte order
        let img = gradient(16, 12);
        for little in [true, false] {
            let mut src = info(BitDepth::U16, false);
            src.exif = Some(crate::exif::sample_with_dimensions(8, 6, little));
            for (format, ext) in
                [(ExportFormat::Tiff16, "tif"), (ExportFormat::Jpeg, "jpg"), (ExportFormat::Png16, "png")]
            {
                let p = dir.path().join(format!("d{little}.{ext}"));
                let opts = ExportOptions { format, ..Default::default() };
                write_file(&p, &img, None, &src, &opts).unwrap();
                let back = crate::read(&p, &ReadLimits::default()).unwrap();
                let dims = back.info.exif.as_deref().and_then(crate::exif::pixel_dimensions);
                assert_eq!(dims, Some((16, 12)), "{format:?}");
                // and once more from the written file, as a second export would
                let p2 = dir.path().join(format!("e{little}.{ext}"));
                write_file(&p2, &gradient(5, 4), None, &back.info, &opts).unwrap();
                let again = crate::read(&p2, &ReadLimits::default()).unwrap();
                assert_eq!(again.info.exif.as_deref().and_then(crate::exif::pixel_dimensions), Some((5, 4)));
            }
        }
    }

    #[test]
    fn limits_are_checked_before_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.tif");
        write_file(&p, &gradient(40, 30), None, &info(BitDepth::U8, false), &ExportOptions::default()).unwrap();
        let err = read(&p, &ReadLimits { max_pixels: 1000 }).unwrap_err();
        assert!(matches!(err, IoError::TooManyPixels { width: 40, height: 30, .. }), "{err}");
    }

    #[test]
    fn roundtrip_16bit_keeps_precision_and_icc() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.tif");
        let img = gradient(37, 11);
        write_file(&p, &img, None, &info(BitDepth::U16, false), &ExportOptions::default()).unwrap();
        let back = read(&p, &ReadLimits::default()).unwrap();
        assert_eq!(back.info.bit_depth, BitDepth::U16);
        assert_eq!(crate::icc_description(back.info.icc.as_deref().unwrap()).as_deref(), Some("Test RGB"));
        let err = img.as_slice().iter().zip(back.rgb.as_slice()).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err <= 0.5 / 65535.0 + 1e-7, "error {err}");
    }

    #[test]
    fn roundtrip_8bit_rgba_and_float() {
        let dir = tempfile::tempdir().unwrap();
        let img = gradient(9, 4);
        let alpha: Vec<f32> = (0..36).map(|i| i as f32 / 35.0).collect();
        let p = dir.path().join("b.tif");
        write_file(&p, &img, Some(&alpha), &info(BitDepth::U8, false), &ExportOptions::default()).unwrap();
        let back = read(&p, &ReadLimits::default()).unwrap();
        assert_eq!(back.info.bit_depth, BitDepth::U8);
        assert_eq!(back.alpha.as_ref().map(Vec::len), Some(36));

        let p = dir.path().join("c.tif");
        write_file(&p, &img, None, &info(BitDepth::F32, false), &ExportOptions::default()).unwrap();
        assert_eq!(read(&p, &ReadLimits::default()).unwrap().rgb, img);
    }

    #[test]
    fn gray_stays_gray_and_export_override() {
        let dir = tempfile::tempdir().unwrap();
        let img = Image::from_rgb(3, 2, vec![0.25; 18]).unwrap();
        let p = dir.path().join("g.tif");
        write_file(&p, &img, None, &info(BitDepth::U16, true), &ExportOptions::default()).unwrap();
        let back = read(&p, &ReadLimits::default()).unwrap();
        assert!(back.info.gray);
        // RGB profile dropped on a grey file
        assert!(back.info.icc.is_none());
        let p = dir.path().join("h.png");
        let opts = ExportOptions { format: ExportFormat::Png16, ..Default::default() };
        write_file(&p, &img, None, &info(BitDepth::U16, true), &opts).unwrap();
        assert_eq!(crate::detect(&p), Some(FileFormat::Png));
    }

    /// TIFF with an 8×6 image followed by `extra` 4×3 images; with `thumbnails`
    /// the following ones are flagged as thumbnails.
    fn multi_ifd(path: &Path, extra: usize, thumbnails: bool) {
        let mut f = std::fs::File::create(path).unwrap();
        let mut enc = TiffEncoder::new(&mut f).unwrap();
        enc.write_image::<colortype::RGB8>(8, 6, &[100u8; 8 * 6 * 3]).unwrap();
        for _ in 0..extra {
            let mut im = enc.new_image::<colortype::RGB8>(4, 3).unwrap();
            if thumbnails {
                im.encoder().write_tag(Tag::from_u16_exhaustive(TAG_NEW_SUBFILE_TYPE), 1u32).unwrap();
            }
            im.write_data(&[50u8; 4 * 3 * 3]).unwrap();
        }
    }

    #[test]
    fn thumbnail_is_not_a_page() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("thumbnail.tif");
        multi_ifd(&p, 1, true);
        let pic = read(&p, &ReadLimits::default()).unwrap();
        assert_eq!((pic.rgb.width(), pic.rgb.height()), (8, 6));
        assert!(pic.info.notes.is_empty(), "{:?}", pic.info.notes);

        let p = dir.path().join("pages.tif");
        multi_ifd(&p, 2, false);
        let pic = read(&p, &ReadLimits::default()).unwrap();
        assert_eq!(pic.info.notes.len(), 1);
        assert!(pic.info.notes[0].contains("2 more full-resolution images"));
    }
}
