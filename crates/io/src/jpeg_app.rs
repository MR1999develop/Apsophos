//! What a JPEG keeps in its APP segments besides EXIF and ICC: the JFIF
//! print resolution and the "extended XMP".
//!
//! One APP1 segment holds at most 65,533 bytes, so a larger XMP packet is
//! split (XMP specification part 3, 1.1.3.1): the main packet, in the usual
//! segment, names in `xmpNote:HasExtendedXMP` the MD5 of the rest, which
//! follows in numbered chunks, each with that MD5, the full length and its
//! offset. Reading assembles the chunks of the named MD5; writing either
//! puts back a packet that came that way, or splits one that is too large
//! (from a TIFF) behind a small main packet.

use std::io::Read;

use md5::{Digest, Md5};

use crate::meta::MAX_VALUE_BYTES;
use crate::Dpi;

/// Largest payload of an APP segment.
pub(crate) const MAX_APP_SEGMENT: usize = 65_533;
/// Start of the APP1 segment holding the main XMP packet.
pub(crate) const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// Start of an APP1 segment holding a chunk of extended XMP.
const EXTENSION_HEADER: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// The MD5 of the extended part, in upper-case hexadecimal.
const GUID_LEN: usize = 32;
/// Data in one extended chunk: the segment minus header, GUID, full length
/// and offset.
const CHUNK_DATA: usize = MAX_APP_SEGMENT - EXTENSION_HEADER.len() - GUID_LEN - 8;

/// The APP segments of a JPEG that matter here.
#[derive(Debug, Default)]
pub(crate) struct AppData {
    /// From the JFIF segment, when it gives dots per inch or per centimetre.
    pub dpi: Option<Dpi>,
    /// Chunks of extended XMP: GUID, full length, offset, data.
    chunks: Vec<(String, u32, u32, Vec<u8>)>,
}

impl AppData {
    /// Reads the segments before the image data. Stops quietly at anything
    /// unexpected: what was read until then is kept.
    pub fn read(jpeg: &mut impl Read) -> Self {
        let mut out = Self::default();
        let mut soi = [0u8; 2];
        if jpeg.read_exact(&mut soi).is_err() || soi != [0xFF, 0xD8] {
            return out;
        }
        let mut total = 0usize;
        loop {
            let mut head = [0u8; 4];
            if jpeg.read_exact(&mut head).is_err() || head[0] != 0xFF {
                return out;
            }
            let marker = head[1];
            // start of scan or end of image: no more metadata
            if marker == 0xDA || marker == 0xD9 {
                return out;
            }
            let len = usize::from(u16::from_be_bytes([head[2], head[3]])).saturating_sub(2);
            total += len;
            if total > MAX_VALUE_BYTES * 2 {
                return out;
            }
            let mut payload = vec![0u8; len];
            if jpeg.read_exact(&mut payload).is_err() {
                return out;
            }
            match marker {
                0xE0 => out.dpi = out.dpi.or_else(|| jfif_dpi(&payload)),
                0xE1 => {
                    if let Some(chunk) = extension_chunk(&payload) {
                        out.chunks.push(chunk);
                    }
                }
                _ => {}
            }
        }
    }

    /// The extended part named by `guid`, when all its chunks are there.
    pub fn extended(&self, guid: &str) -> Option<Vec<u8>> {
        let mut full = None;
        let mut buf = Vec::new();
        let mut filled = 0usize;
        for (g, len, offset, data) in &self.chunks {
            if g != guid {
                continue;
            }
            let len = usize::try_from(*len).ok().filter(|&l| l <= MAX_VALUE_BYTES)?;
            if *full.get_or_insert(len) != len {
                return None;
            }
            buf.resize(len, 0);
            let at = usize::try_from(*offset).ok()?;
            let target = buf.get_mut(at..at.checked_add(data.len())?)?;
            target.copy_from_slice(data);
            filled += data.len();
        }
        (full.is_some_and(|l| l > 0 && filled == l)).then_some(buf)
    }
}

/// Dots per inch from a JFIF APP0 payload (units 1: per inch, 2: per cm).
fn jfif_dpi(p: &[u8]) -> Option<Dpi> {
    if !p.starts_with(b"JFIF\0") || p.len() < 12 {
        return None;
    }
    let per_inch = match p[7] {
        1 => 1.0,
        2 => 2.54,
        _ => return None,
    };
    let (x, y) = (u16::from_be_bytes([p[8], p[9]]), u16::from_be_bytes([p[10], p[11]]));
    Dpi::new(f64::from(x) * per_inch, f64::from(y) * per_inch)
}

fn extension_chunk(p: &[u8]) -> Option<(String, u32, u32, Vec<u8>)> {
    let rest = p.strip_prefix(EXTENSION_HEADER)?;
    let guid = std::str::from_utf8(rest.get(..GUID_LEN)?).ok()?.to_owned();
    let num = |at: usize| Some(u32::from_be_bytes(rest.get(at..at + 4)?.try_into().ok()?));
    let (full, offset) = (num(GUID_LEN)?, num(GUID_LEN + 4)?);
    Some((guid, full, offset, rest.get(GUID_LEN + 8..)?.to_vec()))
}

/// The MD5 that a main packet names in `xmpNote:HasExtendedXMP`, as an
/// attribute (`="…"`) or an element (`>…<`).
pub(crate) fn extended_guid(main: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(main).ok()?;
    let key = "HasExtendedXMP";
    let rest = &text[text.find(key)? + key.len()..];
    let rest = rest.trim_start_matches(|c: char| c == '=' || c == '"' || c == '\'' || c == '>' || c.is_whitespace());
    let guid = rest.get(..GUID_LEN)?;
    guid.bytes().all(|b| b.is_ascii_hexdigit()).then(|| guid.to_owned())
}

/// The APP1 payloads that carry an XMP packet: the main one, then the
/// extended chunks if any.
pub(crate) fn xmp_segments(main: &[u8], extended: Option<&[u8]>) -> Vec<Vec<u8>> {
    if XMP_HEADER.len() + main.len() <= MAX_APP_SEGMENT {
        let mut out = vec![[XMP_HEADER, main].concat()];
        // a packet that came split goes back the same way, under its own MD5
        if let (Some(ext), Some(guid)) = (extended, extended_guid(main)) {
            out.extend(chunks(ext, &guid));
        }
        return out;
    }
    // too large for one segment: a small main packet names the whole one
    let guid: String = Md5::digest(main).iter().map(|b| format!("{b:02X}")).collect();
    let stub = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" \
         xmlns:xmpNote=\"http://ns.adobe.com/xmp/note/\" xmpNote:HasExtendedXMP=\"{guid}\"/></rdf:RDF>\
         </x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let mut out = vec![[XMP_HEADER, stub.as_bytes()].concat()];
    out.extend(chunks(main, &guid));
    out
}

fn chunks(ext: &[u8], guid: &str) -> Vec<Vec<u8>> {
    let Ok(full) = u32::try_from(ext.len()) else { return Vec::new() };
    ext.chunks(CHUNK_DATA)
        .enumerate()
        .filter_map(|(i, part)| {
            let offset = u32::try_from(i * CHUNK_DATA).ok()?;
            Some([EXTENSION_HEADER, guid.as_bytes(), &full.to_be_bytes(), &offset.to_be_bytes(), part].concat())
        })
        .collect()
}

/// One packet with the properties of both parts, for formats that hold the
/// XMP in one piece (TIFF, PNG): the extended part's descriptions go into
/// the main packet's `rdf:RDF`. `None` if either has no `rdf:RDF`.
pub(crate) fn merged(main: &[u8], extended: &[u8]) -> Option<Vec<u8>> {
    let (main, ext) = (std::str::from_utf8(main).ok()?, std::str::from_utf8(extended).ok()?);
    let open = ext.find("<rdf:RDF")?;
    let body_start = open + ext[open..].find('>')? + 1;
    let body_end = ext.rfind("</rdf:RDF>")?;
    let insert_at = main.rfind("</rdf:RDF>")?;
    let body = ext.get(body_start..body_end)?;
    Some([&main[..insert_at], body, &main[insert_at..]].concat().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(body: &str) -> Vec<u8> {
        format!(
            "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\
             {body}</rdf:RDF></x:xmpmeta>"
        )
        .into_bytes()
    }

    /// A JPEG made only of APP segments, as `AppData::read` sees one.
    fn jpeg_of(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for (marker, payload) in segments {
            out.extend([0xFF, *marker]);
            out.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
            out.extend(payload);
        }
        out.extend([0xFF, 0xDA]);
        out
    }

    #[test]
    fn large_packet_is_split_and_assembled() {
        let big = packet(&format!("<rdf:Description rdf:about=''>{}</rdf:Description>", "x".repeat(150_000)));
        let segments = xmp_segments(&big, None);
        assert!(segments.len() >= 4, "{} segments", segments.len());
        assert!(segments.iter().all(|s| s.len() <= MAX_APP_SEGMENT));
        let main = segments[0].strip_prefix(XMP_HEADER).unwrap();
        let guid = extended_guid(main).unwrap();
        let file = jpeg_of(&segments.iter().map(|s| (0xE1, s.clone())).collect::<Vec<_>>());
        let app = AppData::read(&mut file.as_slice());
        assert_eq!(app.extended(&guid).as_deref(), Some(big.as_slice()));
        // a chunk missing: nothing half-assembled
        let partial = jpeg_of(&[(0xE1, segments[0].clone()), (0xE1, segments[1].clone())]);
        assert_eq!(AppData::read(&mut partial.as_slice()).extended(&guid), None);
    }

    #[test]
    fn split_packet_goes_back_under_its_md5() {
        let guid = "0123456789ABCDEF0123456789ABCDEF";
        let main = packet(&format!("<rdf:Description xmpNote:HasExtendedXMP='{guid}'/>"));
        let ext = packet("<rdf:Description><crs:History>long</crs:History></rdf:Description>");
        let segments = xmp_segments(&main, Some(&ext));
        assert_eq!(segments.len(), 2);
        assert!(segments[1].windows(GUID_LEN).any(|w| w == guid.as_bytes()));
        let merged = String::from_utf8(merged(&main, &ext).unwrap()).unwrap();
        assert!(merged.contains("HasExtendedXMP") && merged.contains("crs:History"), "{merged}");
        assert_eq!(merged.matches("</rdf:RDF>").count(), 1);
    }

    #[test]
    fn jfif_density() {
        let mut jfif = b"JFIF\0\x01\x02".to_vec();
        jfif.extend([1, 1, 44, 1, 44, 0, 0]); // dpi, 300 × 300
        let file = jpeg_of(&[(0xE0, jfif)]);
        let dpi = AppData::read(&mut file.as_slice()).dpi.unwrap();
        assert!((dpi.x - 300.0).abs() < 1e-9 && (dpi.y - 300.0).abs() < 1e-9);
    }
}
