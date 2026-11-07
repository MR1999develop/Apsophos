//! Hostile input for the parsers written in this crate (TIFF and EXIF
//! directories, JPEG APP segments, ICC headers): random bytes, and valid
//! data with random bytes changed, must give a result or nothing, never a
//! panic or an allocation beyond the caps. A stand-in for fuzzing, which
//! needs a nightly compiler; each failing input found here becomes a plain
//! test.

use proptest::prelude::*;

use crate::{exif, icc, jpeg_app, meta};

/// Up to a few kilobytes: enough for every structure the parsers walk.
fn bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..4096)
}

/// `valid` with up to 16 bytes replaced at random positions.
fn damaged(valid: Vec<u8>) -> impl Strategy<Value = Vec<u8>> {
    let len = valid.len().max(1);
    proptest::collection::vec((0..len, any::<u8>()), 1..16).prop_map(move |changes| {
        let mut out = valid.clone();
        for (at, value) in changes {
            if let Some(b) = out.get_mut(at) {
                *b = value;
            }
        }
        out
    })
}

/// A JPEG made of the APP1 segments of a large XMP packet.
fn jpeg_with_extended_xmp() -> Vec<u8> {
    let packet =
        format!("<x:xmpmeta><rdf:RDF><rdf:Description>{}</rdf:Description></rdf:RDF></x:xmpmeta>", "x".repeat(70_000));
    let mut out = vec![0xFF, 0xD8];
    for segment in jpeg_app::xmp_segments(packet.as_bytes(), None) {
        out.extend([0xFF, 0xE1]);
        out.extend(u16::try_from(segment.len() + 2).unwrap_or(u16::MAX).to_be_bytes());
        out.extend(segment);
    }
    out.extend([0xFF, 0xDA]);
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn exif_blocks_of_random_bytes(data in bytes()) {
        let _ = meta::from_exif(&data);
        let _ = exif::orientation(&data);
        let mut copy = data.clone();
        exif::reset_orientation(&mut copy);
    }

    #[test]
    fn tiff_files_of_random_bytes(data in bytes()) {
        let _ = meta::from_tiff(std::io::Cursor::new(data));
    }

    #[test]
    fn tiff_headers_with_random_directories(little in any::<bool>(), rest in bytes()) {
        let mut data = if little { b"II*\0\x08\0\0\0".to_vec() } else { b"MM\0*\0\0\0\x08".to_vec() };
        data.extend(rest);
        let _ = meta::from_tiff(std::io::Cursor::new(data));
    }

    #[test]
    fn damaged_exif_blocks(data in damaged(meta::to_exif(&meta::tests::sample()))) {
        let _ = meta::from_exif(&data);
    }

    #[test]
    fn jpeg_segments_of_random_bytes(data in bytes()) {
        let app = jpeg_app::AppData::read(&mut data.as_slice());
        let _ = app.extended("0123456789ABCDEF0123456789ABCDEF");
    }

    #[test]
    fn damaged_extended_xmp(data in damaged(jpeg_with_extended_xmp())) {
        let app = jpeg_app::AppData::read(&mut data.as_slice());
        let main = data.get(4 + 2 + jpeg_app::XMP_HEADER.len()..).unwrap_or_default();
        if let Some(guid) = jpeg_app::extended_guid(main) {
            let _ = app.extended(&guid);
        }
    }

    #[test]
    fn xmp_text(text in ".{0,512}") {
        let _ = jpeg_app::extended_guid(text.as_bytes());
        let _ = jpeg_app::merged(text.as_bytes(), text.as_bytes());
    }

    #[test]
    fn icc_headers_of_random_bytes(data in bytes()) {
        let _ = icc::color_space(&data);
        let _ = icc::icc_description(&data);
    }

    #[test]
    fn damaged_icc_profiles(data in damaged(icc::sample_v2("Test RGB"))) {
        let _ = icc::color_space(&data);
        let _ = icc::icc_description(&data);
    }
}
