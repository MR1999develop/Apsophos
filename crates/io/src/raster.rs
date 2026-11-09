//! JPEG and PNG: reading with `image`, JPEG writing with `jpeg-encoder`
//! (which can embed ICC, EXIF and XMP segments) and PNG writing with `png`
//! (ICC, EXIF, XMP and resolution chunks).

use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use denoise_core::quant::{quantize_u8_at, to_u16};
use denoise_core::{num, Image};
use image::{ColorType, DynamicImage, ImageDecoder, ImageReader};

use crate::jpeg_app::{self, AppData, MAX_APP_SEGMENT, XMP_HEADER};
use crate::{
    band_samples, dither_to_8bit, exif, icc, meta, orient, rows_mismatch, Band, BandSource, BitDepth, Dpi, FileFormat,
    IoError, Picture, ReadLimits, SourceInfo,
};

/// Maximum side of a JPEG.
pub(crate) const MAX_JPEG_PX: u32 = 65_535;
/// Keyword of the PNG iTXt chunk that holds XMP.
const PNG_XMP_KEYWORD: &str = "XML:com.adobe.xmp";
/// Metres per inch, for the PNG resolution (pixels per metre).
const METRES_PER_INCH: f64 = 0.0254;

fn decode_err(e: image::ImageError) -> IoError {
    match e {
        image::ImageError::IoError(io) => IoError::Io(io),
        image::ImageError::Unsupported(u) => IoError::Unsupported(format!("This file variant ({u})")),
        other => IoError::Decode(other.to_string()),
    }
}

pub fn read(path: &Path, format: FileFormat, limits: &ReadLimits) -> Result<Picture, IoError> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    // the size is checked here, from the header, before decoding
    reader.no_limits();
    let mut dec = reader.into_decoder().map_err(decode_err)?;
    let (w, h) = dec.dimensions();
    limits.check(w, h)?;
    let mut icc = dec.icc_profile().map_err(decode_err)?;
    let exif = dec.exif_metadata().map_err(decode_err)?.map(|e| exif::strip_header(&e).to_vec());
    let xmp = dec.xmp_metadata().map_err(decode_err)?.and_then(xmp_packet);
    let img = DynamicImage::from_decoder(dec).map_err(decode_err)?;
    // what the decoder does not give: resolution, extended XMP and the JPEG
    // quantisation, from the file's headers
    let (dpi_in_file, xmp_extended, jpeg_quality) = match format {
        FileFormat::Jpeg => {
            let app = File::open(path).map(|f| AppData::read(&mut BufReader::new(f))).unwrap_or_default();
            let extended = xmp.as_deref().and_then(jpeg_app::extended_guid).and_then(|g| app.extended(&g));
            (app.dpi, extended, app.quality)
        }
        FileFormat::Png => (png_dpi(path), None, None),
        FileFormat::Tiff => (None, None, None),
    };
    // the EXIF resolution first: it is the one cameras and editors set
    let dpi = exif.as_deref().and_then(meta::from_exif).and_then(|m| m.dpi).or(dpi_in_file);

    let color = img.color();
    let bit_depth = match color {
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8 => BitDepth::U8,
        ColorType::Rgb32F | ColorType::Rgba32F => BitDepth::F32,
        _ => BitDepth::U16,
    };
    let gray = !color.has_color();
    let mut notes = Vec::new();
    // a CMYK JPEG is converted to RGB by the decoder: its CMYK profile no longer applies
    if icc.as_deref().and_then(icc::color_space) == Some(b"CMYK") {
        icc = None;
        notes.push("CMYK image converted to RGB: the CMYK colour profile is not kept.".to_owned());
    }

    let (w, h) = (img.width() as usize, img.height() as usize);
    let alpha =
        color.has_alpha().then(|| img.to_rgba32f().into_raw().as_chunks::<4>().0.iter().map(|p| p[3]).collect());
    let rgb = Image::from_rgb(w, h, img.to_rgb32f().into_raw()).map_err(|e| IoError::Decode(e.to_string()))?;
    let orientation = exif.as_deref().map_or(1, exif::orientation);
    let (rgb, alpha) = orient::apply(rgb, alpha, orientation);
    Ok(Picture {
        rgb,
        alpha,
        info: SourceInfo { format, bit_depth, gray, icc, exif, xmp, xmp_extended, dpi, notes, jpeg_quality },
    })
}

/// The resolution of a PNG's pHYs chunk, when it is in pixels per metre.
fn png_dpi(path: &Path) -> Option<Dpi> {
    let reader = png::Decoder::new(BufReader::new(File::open(path).ok()?)).read_info().ok()?;
    let dims = reader.info().pixel_dims?;
    (dims.unit == png::Unit::Meter)
        .then(|| Dpi::new(f64::from(dims.xppu) * METRES_PER_INCH, f64::from(dims.yppu) * METRES_PER_INCH))
        .flatten()
}

/// Whether an ICC profile describes the colours written: grey for grey, RGB
/// otherwise. A mismatched profile is left out; the image stays valid.
fn icc_fits(profile: &[u8], gray: bool) -> bool {
    icc::color_space(profile) == Some(if gray { b"GRAY" } else { b"RGB " })
}

/// The XMP as one packet, for a format without "extended XMP".
pub(crate) fn whole_xmp(info: &SourceInfo) -> Option<Cow<'_, [u8]>> {
    let main = info.xmp.as_deref()?;
    Some(match info.xmp_extended.as_deref().and_then(|ext| jpeg_app::merged(main, ext)) {
        Some(m) => Cow::Owned(m),
        None => Cow::Borrowed(main),
    })
}

/// The XMP packet as read by the decoder (JPEG APP1 segment or PNG iTXt
/// chunk), without the JPEG namespace header if the decoder left it; `None`
/// if empty or larger than any real packet.
fn xmp_packet(raw: Vec<u8>) -> Option<Vec<u8>> {
    let packet = if raw.starts_with(XMP_HEADER) { raw[XMP_HEADER.len()..].to_vec() } else { raw };
    (!packet.is_empty() && packet.len() <= meta::MAX_VALUE_BYTES).then_some(packet)
}

/// JPEG: the encoder takes the whole image, so the bands are gathered as
/// 8-bit samples (3 bytes per pixel, 1 for grey) and encoded at the end.
pub fn write_jpeg<W: Write, S: BandSource>(
    out: &mut W,
    source: &mut S,
    info: &SourceInfo,
    quality: u8,
) -> Result<(), S::Error> {
    let too_large = || IoError::TooLarge { format: "JPEG", max: MAX_JPEG_PX };
    let w = u16::try_from(source.width()).map_err(|_| too_large())?;
    let h = u16::try_from(source.height()).map_err(|_| too_large())?;
    let enc_err = |e: jpeg_encoder::EncodingError| IoError::Encode(e.to_string());

    let mut enc = jpeg_encoder::Encoder::new(out, quality.clamp(1, 100));
    // high quality: no chroma subsampling
    enc.set_sampling_factor(if quality >= 90 {
        jpeg_encoder::SamplingFactor::R_4_4_4
    } else {
        jpeg_encoder::SamplingFactor::R_4_2_0
    });
    if let Some(exif_blob) = &info.exif {
        let segment = exif::with_header(&exif::for_output(exif_blob, u32::from(w), u32::from(h)));
        // EXIF larger than one segment (rare: huge thumbnails) is left out
        if segment.len() <= MAX_APP_SEGMENT {
            enc.add_app_segment(1, &segment).map_err(enc_err)?;
        }
    }
    if let Some(xmp) = &info.xmp {
        // a packet larger than one segment goes as "extended XMP"
        for segment in jpeg_app::xmp_segments(xmp, info.xmp_extended.as_deref()) {
            enc.add_app_segment(1, &segment).map_err(enc_err)?;
        }
    }
    if let Some(d) = info.dpi {
        let dots = |v: f64| num::round_to_u16(v, 1);
        enc.set_density(jpeg_encoder::Density::Inch { x: dots(d.x), y: dots(d.y) });
    }
    let gray = info.gray;
    if let Some(profile) = info.icc.as_deref().filter(|p| icc_fits(p, gray)) {
        enc.add_icc_profile(profile).map_err(enc_err)?;
    }
    let dither = dither_to_8bit(info);
    let channels = if gray { 1 } else { 3 };
    let mut data: Vec<u8> = Vec::new();
    let mut rows = 0;
    // alpha is not written: JPEG has none
    while let Some(band) = source.next_band()? {
        let band = Band { rgb: band.rgb, alpha: None };
        data.extend(quantize_u8_at(&band_samples(&band, gray), data.len(), dither));
        rows += band.rgb.height();
    }
    if rows * usize::from(w) * channels != data.len() || rows != usize::from(h) {
        return Err(rows_mismatch(rows, usize::from(h)).into());
    }
    let color = if gray { jpeg_encoder::ColorType::Luma } else { jpeg_encoder::ColorType::Rgb };
    Ok(enc.encode(&data, w, h, color).map_err(enc_err)?)
}

/// PNG: the rows go to the encoder's stream writer as they arrive.
pub fn write_png<W: Write, S: BandSource>(
    out: &mut W,
    source: &mut S,
    info: &SourceInfo,
    depth: BitDepth,
) -> Result<(), S::Error> {
    let too_large = || IoError::TooLarge { format: "PNG", max: u32::MAX };
    let w = u32::try_from(source.width()).map_err(|_| too_large())?;
    let h = u32::try_from(source.height()).map_err(|_| too_large())?;
    let enc_err = |e: png::EncodingError| IoError::Encode(e.to_string());
    let has_alpha = source.has_alpha();
    let sixteen = depth != BitDepth::U8;

    let mut header = png::Info::with_size(w, h);
    header.color_type = match (info.gray, has_alpha) {
        (true, false) => png::ColorType::Grayscale,
        (true, true) => png::ColorType::GrayscaleAlpha,
        (false, false) => png::ColorType::Rgb,
        (false, true) => png::ColorType::Rgba,
    };
    header.bit_depth = if sixteen { png::BitDepth::Sixteen } else { png::BitDepth::Eight };
    header.icc_profile = info.icc.as_deref().filter(|p| icc_fits(p, info.gray)).map(Cow::Borrowed);
    header.exif_metadata = info.exif.as_ref().map(|blob| Cow::Owned(exif::for_output(blob, w, h)));
    header.pixel_dims = info.dpi.map(|d| {
        let per_metre = |v: f64| num::round_to_u32(v / METRES_PER_INCH, 1);
        png::PixelDimensions { xppu: per_metre(d.x), yppu: per_metre(d.y), unit: png::Unit::Meter }
    });
    let mut enc = png::Encoder::with_info(out, header).map_err(enc_err)?;
    enc.set_compression(png::Compression::Balanced);
    enc.set_filter(png::Filter::Adaptive);
    // XMP is text: a packet that is not UTF-8 is left out
    if let Some(text) = whole_xmp(info).and_then(|x| String::from_utf8(x.into_owned()).ok()) {
        enc.add_itxt_chunk(PNG_XMP_KEYWORD.to_owned(), text).map_err(enc_err)?;
    }
    let mut writer = enc.write_header().map_err(enc_err)?;
    let mut stream = writer.stream_writer().map_err(enc_err)?;
    let dither = dither_to_8bit(info);
    let (mut rows, mut first) = (0, 0);
    while let Some(band) = source.next_band()? {
        let samples = band_samples(&band, info.gray);
        // PNG stores 16-bit samples big-endian
        let bytes: Vec<u8> = if sixteen {
            samples.iter().flat_map(|&v| to_u16(v).to_be_bytes()).collect()
        } else {
            quantize_u8_at(&samples, first, dither)
        };
        stream.write_all(&bytes).map_err(IoError::Io)?;
        first += samples.len();
        rows += band.rgb.height();
    }
    if rows != h as usize {
        return Err(rows_mismatch(rows, h as usize).into());
    }
    stream.finish().map_err(enc_err)?;
    Ok(writer.finish().map_err(enc_err)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read as read_file, write as write_file, ExportFormat, ExportOptions};

    fn picture(w: usize, h: usize) -> Image {
        let data = (0..w * h).flat_map(|i| [(i % 7) as f32 / 6.0, 0.4, 0.8]).collect();
        Image::from_rgb(w, h, data).unwrap()
    }

    fn info(format: FileFormat) -> SourceInfo {
        SourceInfo {
            format,
            bit_depth: BitDepth::U8,
            gray: false,
            icc: Some(icc::sample_v2("Test RGB")),
            exif: Some(exif::sample(6, true)),
            xmp: None,
            xmp_extended: None,
            dpi: None,
            notes: vec![],
            jpeg_quality: None,
        }
    }

    #[test]
    fn xmp_packet_without_header() {
        let body = b"<x:xmpmeta/>".to_vec();
        assert_eq!(xmp_packet([XMP_HEADER, &body].concat()), Some(body.clone()));
        assert_eq!(xmp_packet(body.clone()), Some(body));
        assert_eq!(xmp_packet(XMP_HEADER.to_vec()), None);
        assert_eq!(xmp_packet(Vec::new()), None);
    }

    #[test]
    fn jpeg_keeps_icc_and_resets_orientation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        let img = picture(16, 8);
        let opts = ExportOptions { format: ExportFormat::Jpeg, jpeg_quality: 95 };
        write_file(&p, &img, None, &info(FileFormat::Jpeg), &opts).unwrap();
        let back = read_file(&p, &ReadLimits::default()).unwrap();
        assert_eq!((back.rgb.width(), back.rgb.height()), (16, 8), "orientation already applied: no rotation");
        assert_eq!(icc::icc_description(back.info.icc.as_deref().unwrap()).as_deref(), Some("Test RGB"));
        assert_eq!(back.info.exif.as_deref().map(exif::orientation), Some(1));
    }

    #[test]
    fn jpeg_quality_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let img = picture(16, 8);
        for q in [30, 75, 95] {
            let p = dir.path().join(format!("q{q}.jpg"));
            let opts = ExportOptions { format: ExportFormat::Jpeg, jpeg_quality: q };
            write_file(&p, &img, None, &info(FileFormat::Jpeg), &opts).unwrap();
            let got = read_file(&p, &ReadLimits::default()).unwrap().info.jpeg_quality.unwrap();
            assert!(got.get().abs_diff(q) <= 2, "written at {q}, estimated {}", got.get());
            assert_eq!(got.is_heavy(), q < 50);
        }
        // other formats have none
        let p = dir.path().join("a.png");
        write_file(&p, &img, None, &info(FileFormat::Png), &ExportOptions::default()).unwrap();
        assert_eq!(read_file(&p, &ReadLimits::default()).unwrap().info.jpeg_quality, None);
    }

    #[test]
    fn jpeg_with_orientation_is_rotated_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.jpg");
        let img = picture(16, 8);
        // writes by hand a JPEG with orientation 6 (not reset)
        let mut f = std::fs::File::create(&p).unwrap();
        let mut enc = jpeg_encoder::Encoder::new(&mut f, 90);
        enc.add_app_segment(1, &exif::with_header(&exif::sample(6, false))).unwrap();
        let data = denoise_core::quant::quantize_u8(img.as_slice(), false);
        enc.encode(&data, 16, 8, jpeg_encoder::ColorType::Rgb).unwrap();
        drop(f);
        let back = read_file(&p, &ReadLimits::default()).unwrap();
        assert_eq!((back.rgb.width(), back.rgb.height()), (8, 16));
    }

    #[test]
    fn png_16bit_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        let img = picture(5, 3);
        let mut inf = info(FileFormat::Png);
        inf.bit_depth = BitDepth::U16;
        write_file(&p, &img, Some(&[0.5; 15]), &inf, &ExportOptions::default()).unwrap();
        let back = read_file(&p, &ReadLimits::default()).unwrap();
        assert_eq!(back.info.bit_depth, BitDepth::U16);
        assert!(back.alpha.is_some());
        let err = img.as_slice().iter().zip(back.rgb.as_slice()).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-4);
    }

    fn close(dpi: Option<Dpi>, want: f64) -> bool {
        dpi.is_some_and(|d| (d.x - want).abs() < 0.1 && (d.y - want).abs() < 0.1)
    }

    #[test]
    fn png_keeps_exif_xmp_icc_and_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.png");
        let img = picture(16, 8);
        let mut src = info(FileFormat::Png);
        src.xmp = Some(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>rating 4</x:xmpmeta>".to_vec());
        src.dpi = Dpi::new(300.0, 300.0);
        write_file(&p, &img, None, &src, &ExportOptions::default()).unwrap();
        let back = read_file(&p, &ReadLimits::default()).unwrap();
        assert_eq!(back.info.xmp, src.xmp);
        assert_eq!(back.info.icc, src.icc);
        // the EXIF goes along, with the orientation reset (it is in the pixels)
        assert_eq!(back.info.exif.as_deref().map(exif::orientation), Some(1));
        assert!(close(back.info.dpi, 300.0), "{:?}", back.info.dpi);
    }

    #[test]
    fn jpeg_carries_resolution_and_extended_xmp() {
        let dir = tempfile::tempdir().unwrap();
        let img = picture(16, 8);
        let jpeg = ExportOptions { format: ExportFormat::Jpeg, ..Default::default() };
        // a packet too large for one segment, as a TIFF can carry
        let big = format!(
            "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>             <rdf:Description rdf:about=''>{}</rdf:Description></rdf:RDF></x:xmpmeta>",
            "h".repeat(120_000)
        )
        .into_bytes();
        let mut src = info(FileFormat::Jpeg);
        src.xmp = Some(big.clone());
        src.dpi = Dpi::new(240.0, 240.0);
        let a = dir.path().join("a.jpg");
        write_file(&a, &img, None, &src, &jpeg).unwrap();
        let back = read_file(&a, &ReadLimits::default()).unwrap();
        // a small main packet names the whole one, which comes back as it was
        assert!(back.info.xmp.as_deref().and_then(jpeg_app::extended_guid).is_some());
        assert_eq!(back.info.xmp_extended.as_deref(), Some(big.as_slice()));
        assert!(close(back.info.dpi, 240.0), "{:?}", back.info.dpi);

        // JPEG → JPEG: both parts again, under the same MD5
        let b = dir.path().join("b.jpg");
        write_file(&b, &img, None, &back.info, &jpeg).unwrap();
        let again = read_file(&b, &ReadLimits::default()).unwrap();
        assert_eq!(again.info.xmp, back.info.xmp);
        assert_eq!(again.info.xmp_extended, back.info.xmp_extended);

        // JPEG → TIFF: one packet with everything, and the resolution
        let t = dir.path().join("t.tif");
        write_file(&t, &img, None, &back.info, &ExportOptions { format: ExportFormat::Tiff16, ..Default::default() })
            .unwrap();
        let tiff = read_file(&t, &ReadLimits::default()).unwrap();
        let text = String::from_utf8(tiff.info.xmp.unwrap()).unwrap();
        assert!(text.contains("HasExtendedXMP") && text.contains(&"h".repeat(120_000)));
        assert!(close(tiff.info.dpi, 240.0), "{:?}", tiff.info.dpi);
    }
}
