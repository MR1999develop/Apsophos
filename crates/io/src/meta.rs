//! EXIF, GPS and descriptive tags carried from the source file to the output.
//!
//! This metadata is stored in TIFF-structured directories (IFDs) of 12-byte
//! entries: a JPEG keeps them in its EXIF block (a small TIFF of its own), a
//! TIFF in its first IFD, with the EXIF and GPS tags in sub-IFDs pointed to
//! from there. Both are read into [`Metadata`], normalised to little-endian,
//! and can be written to either.
//!
//! Only what can be moved safely is kept:
//! * of the first IFD, the descriptive text tags (camera, date, author…); the
//!   others describe the pixel layout, which the encoder writes anew, and the
//!   orientation, which is already applied to the pixels;
//! * the EXIF and GPS sub-IFDs, without the maker note and the
//!   interoperability pointer, whose offsets are only valid in the original
//!   file;
//! * the print resolution (XResolution, YResolution, ResolutionUnit), read
//!   apart as [`Dpi`]: each encoder writes it in its own way.

use std::io::{Read, Seek, SeekFrom};

use crate::Dpi;

pub(crate) const TAG_EXIF_IFD: u16 = 34665;
pub(crate) const TAG_GPS_IFD: u16 = 34853;
pub(crate) const TAG_XMP: u16 = 700;
/// ImageDescription, Make, Model, DateTime, Artist, HostComputer, Copyright:
/// all text (ASCII) tags of the first IFD.
const DESCRIPTIVE: [u16; 7] = [270, 271, 272, 306, 315, 316, 33432];
/// MakerNote and InteroperabilityIFD: offsets that do not survive a move.
const NOT_PORTABLE: [u16; 2] = [37500, 40965];
pub(crate) const TYPE_ASCII: u16 = 2;
const TYPE_LONG: u16 = 4;
/// Largest value read for one tag (an XMP packet is a few kB to a few hundred).
pub(crate) const MAX_VALUE_BYTES: usize = 4 << 20;
/// More entries than this in one IFD means a damaged file.
const MAX_ENTRIES: usize = 1000;

/// One IFD entry; `data` holds `count` values in little-endian order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    pub data: Vec<u8>,
}

/// The portable metadata of an image.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Metadata {
    /// Descriptive tags of the first IFD.
    pub main: Vec<Entry>,
    pub exif: Vec<Entry>,
    pub gps: Vec<Entry>,
    /// From the first IFD's resolution tags.
    pub dpi: Option<Dpi>,
}

impl Metadata {
    /// Nothing to carry as EXIF (the resolution is written apart).
    pub fn is_empty(&self) -> bool {
        self.main.is_empty() && self.exif.is_empty() && self.gps.is_empty()
    }

    /// Sets `PixelXDimension` and `PixelYDimension` of the EXIF sub-IFD,
    /// where present, to the size of the file written (as LONG values, so
    /// that any size fits); tags that are missing are not added.
    pub fn set_pixel_dimensions(&mut self, width: u32, height: u32) {
        use crate::exif::{TAG_PIXEL_X, TAG_PIXEL_Y};
        for e in &mut self.exif {
            let value = match e.tag {
                TAG_PIXEL_X => width,
                TAG_PIXEL_Y => height,
                _ => continue,
            };
            *e = Entry { tag: e.tag, typ: TYPE_LONG, count: 1, data: value.to_le_bytes().to_vec() };
        }
    }
}

const TAG_X_RESOLUTION: u16 = 282;
const TAG_Y_RESOLUTION: u16 = 283;
const TAG_RESOLUTION_UNIT: u16 = 296;
const TYPE_SHORT: u16 = 3;
const TYPE_RATIONAL: u16 = 5;

/// Bytes of one value of a TIFF type, and the size of the units whose byte
/// order is swapped (a RATIONAL is two LONGs). `None` for unknown types.
fn type_size(typ: u16) -> Option<(usize, usize)> {
    match typ {
        1 | 2 | 6 | 7 => Some((1, 1)),
        3 | 8 => Some((2, 2)),
        4 | 9 | 11 | 13 => Some((4, 4)),
        5 | 10 => Some((8, 4)),
        12 => Some((8, 8)),
        _ => None,
    }
}

/// Random access to the bytes of a TIFF structure.
trait Bytes {
    fn bytes(&mut self, at: u64, len: usize) -> Option<Vec<u8>>;
}

struct Slice<'a>(&'a [u8]);

impl Bytes for Slice<'_> {
    fn bytes(&mut self, at: u64, len: usize) -> Option<Vec<u8>> {
        let at = usize::try_from(at).ok()?;
        self.0.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
    }
}

struct Stream<R>(R);

impl<R: Read + Seek> Bytes for Stream<R> {
    fn bytes(&mut self, at: u64, len: usize) -> Option<Vec<u8>> {
        self.0.seek(SeekFrom::Start(at)).ok()?;
        let mut buf = vec![0; len];
        self.0.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

struct Reader<B> {
    src: B,
    little: bool,
}

impl<B: Bytes> Reader<B> {
    /// Reads the header; returns the reader and the offset of the first IFD.
    fn open(mut src: B) -> Option<(Self, u64)> {
        let head = src.bytes(0, 8)?;
        let little = match &head[..2] {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let r = Reader { src, little };
        (r.u16(&head[2..4]) == 42).then_some(())?;
        let first = u64::from(r.u32(&head[4..8]));
        Some((r, first))
    }

    fn u16(&self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    }

    fn u32(&self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }

    /// Entries of the IFD at `at`; entries of unknown type or with values
    /// out of the file are skipped.
    fn ifd(&mut self, at: u64) -> Option<Vec<Entry>> {
        let head = self.src.bytes(at, 2)?;
        let n = usize::from(self.u16(&head));
        if n > MAX_ENTRIES {
            return None;
        }
        let table = self.src.bytes(at + 2, n * 12)?;
        let mut out = Vec::with_capacity(n);
        for e in table.as_chunks::<12>().0.iter() {
            let (tag, typ, count) = (self.u16(&e[0..2]), self.u16(&e[2..4]), self.u32(&e[4..8]));
            let Some((size, unit)) = type_size(typ) else { continue };
            let Some(len) = usize::try_from(count).ok().and_then(|c| c.checked_mul(size)) else { continue };
            if len > MAX_VALUE_BYTES {
                continue;
            }
            let data = if len <= 4 {
                Some(e[8..8 + len].to_vec())
            } else {
                self.src.bytes(u64::from(self.u32(&e[8..12])), len)
            };
            let Some(mut data) = data else { continue };
            if !self.little {
                data.chunks_exact_mut(unit).for_each(<[u8]>::reverse);
            }
            out.push(Entry { tag, typ, count, data });
        }
        Some(out)
    }

    /// The sub-IFD pointed to by `tag` in `entries`, without its non-portable tags.
    fn sub_ifd(&mut self, entries: &[Entry], tag: u16) -> Vec<Entry> {
        let Some(at) = entries.iter().find(|e| e.tag == tag && e.count == 1).and_then(pointer) else {
            return Vec::new();
        };
        let mut sub = self.ifd(at).unwrap_or_default();
        sub.retain(|e| !NOT_PORTABLE.contains(&e.tag));
        sub
    }

    /// Metadata and XMP packet of the structure whose first IFD is at `first`.
    fn metadata(&mut self, first: u64) -> Option<(Metadata, Option<Vec<u8>>)> {
        let ifd0 = self.ifd(first)?;
        let exif = self.sub_ifd(&ifd0, TAG_EXIF_IFD);
        let gps = self.sub_ifd(&ifd0, TAG_GPS_IFD);
        let xmp = ifd0.iter().find(|e| e.tag == TAG_XMP && matches!(e.typ, 1 | 7)).map(|e| e.data.clone());
        let dpi = dpi_of(&ifd0);
        let main = ifd0.into_iter().filter(|e| DESCRIPTIVE.contains(&e.tag) && e.typ == TYPE_ASCII).collect();
        Some((Metadata { main, exif, gps, dpi }, xmp))
    }
}

/// The print resolution of a first IFD: inches when the unit is missing (the
/// TIFF default), centimetres converted; `None` without both values or with
/// "no absolute unit".
fn dpi_of(ifd0: &[Entry]) -> Option<Dpi> {
    let rational = |tag: u16| {
        let e = ifd0.iter().find(|e| e.tag == tag && e.typ == TYPE_RATIONAL)?;
        let n = u32::from_le_bytes(e.data.get(0..4)?.try_into().ok()?);
        let d = u32::from_le_bytes(e.data.get(4..8)?.try_into().ok()?);
        (d > 0).then(|| f64::from(n) / f64::from(d))
    };
    let unit = ifd0
        .iter()
        .find(|e| e.tag == TAG_RESOLUTION_UNIT && e.typ == TYPE_SHORT)
        .and_then(|e| Some(u16::from_le_bytes(e.data.get(0..2)?.try_into().ok()?)))
        .unwrap_or(2);
    let per_inch = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return None,
    };
    Dpi::new(rational(TAG_X_RESOLUTION)? * per_inch, rational(TAG_Y_RESOLUTION)? * per_inch)
}

/// The offset in a LONG or IFD entry.
fn pointer(e: &Entry) -> Option<u64> {
    matches!(e.typ, 4 | 13).then_some(())?;
    let b: [u8; 4] = e.data.get(..4)?.try_into().ok()?;
    Some(u64::from(u32::from_le_bytes(b)))
}

/// Metadata and XMP packet of a TIFF file, read from its directories only
/// (not the pixels). `None` if the file has no readable TIFF structure.
pub(crate) fn from_tiff<R: Read + Seek>(file: R) -> Option<(Metadata, Option<Vec<u8>>)> {
    let (mut r, first) = Reader::open(Stream(file))?;
    r.metadata(first)
}

/// Metadata of an EXIF block (with or without the `Exif\0\0` header).
pub(crate) fn from_exif(blob: &[u8]) -> Option<Metadata> {
    let (mut r, first) = Reader::open(Slice(crate::exif::strip_header(blob)))?;
    r.metadata(first).map(|(m, _)| m)
}

/// Appends the IFD of `entries` (sorted by tag) to `out`, where `out[0]` is at
/// file offset `base`, followed by its values. Returns the position in `out`
/// of each entry's value field, to patch pointers.
fn put_ifd(out: &mut Vec<u8>, base: u32, entries: &[Entry]) -> Vec<usize> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.tag);
    let start = out.len();
    let table_len = 2 + 12 * sorted.len() + 4;
    let mut values = Vec::new();
    let mut fields = Vec::with_capacity(sorted.len());
    out.extend_from_slice(&u16::try_from(sorted.len()).unwrap_or(u16::MAX).to_le_bytes());
    for e in &sorted {
        out.extend_from_slice(&e.tag.to_le_bytes());
        out.extend_from_slice(&e.typ.to_le_bytes());
        out.extend_from_slice(&e.count.to_le_bytes());
        fields.push(out.len());
        if e.data.len() <= 4 {
            let mut v = e.data.clone();
            v.resize(4, 0);
            out.extend_from_slice(&v);
        } else {
            let at = base as usize + start + table_len + values.len();
            out.extend_from_slice(&u32::try_from(at).unwrap_or(0).to_le_bytes());
            values.extend_from_slice(&e.data);
            if values.len() % 2 == 1 {
                values.push(0); // values start on a word boundary
            }
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    out.extend_from_slice(&values);
    fields
}

/// A standalone IFD of `entries` to be stored at file offset `at` (even).
pub(crate) fn ifd_at(entries: &[Entry], at: u32) -> Vec<u8> {
    let mut out = Vec::new();
    put_ifd(&mut out, at, entries);
    out
}

/// A pointer entry to a sub-IFD, with its offset still to be filled in.
fn pointer_entry(tag: u16) -> Entry {
    Entry { tag, typ: TYPE_LONG, count: 1, data: vec![0; 4] }
}

/// The metadata as an EXIF block (little-endian TIFF structure, without the
/// `Exif\0\0` header), for a JPEG APP1 segment.
pub(crate) fn to_exif(meta: &Metadata) -> Vec<u8> {
    let mut out = b"II\x2A\x00\x08\x00\x00\x00".to_vec();
    let subs: Vec<(u16, &[Entry])> = [(TAG_EXIF_IFD, meta.exif.as_slice()), (TAG_GPS_IFD, meta.gps.as_slice())]
        .into_iter()
        .filter(|(_, e)| !e.is_empty())
        .collect();
    let mut main = meta.main.clone();
    main.extend(subs.iter().map(|(tag, _)| pointer_entry(*tag)));
    let fields = put_ifd(&mut out, 0, &main);
    let mut sorted_tags: Vec<u16> = main.iter().map(|e| e.tag).collect();
    sorted_tags.sort_unstable();
    for (tag, entries) in subs {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = u32::try_from(out.len()).unwrap_or(0);
        put_ifd(&mut out, 0, entries);
        if let Some(i) = sorted_tags.iter().position(|t| *t == tag) {
            out[fields[i]..fields[i] + 4].copy_from_slice(&at.to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn ascii(tag: u16, s: &str) -> Entry {
        let mut data = s.as_bytes().to_vec();
        data.push(0);
        Entry { tag, typ: TYPE_ASCII, count: u32::try_from(data.len()).unwrap(), data }
    }

    /// Metadata with a camera model, an EXIF date and exposure, and a GPS latitude.
    pub(crate) fn sample() -> Metadata {
        let rational = |n: u32, d: u32| [n.to_le_bytes(), d.to_le_bytes()].concat();
        Metadata {
            main: vec![ascii(271, "Canon"), ascii(272, "EOS R5")],
            // in tag order, as every IFD is written
            exif: vec![
                Entry { tag: 33434, typ: 5, count: 1, data: rational(1, 250) },
                Entry { tag: 34855, typ: 3, count: 1, data: 400u16.to_le_bytes().to_vec() },
                ascii(36867, "2026:10:25 12:00:00"),
            ],
            gps: vec![Entry {
                tag: 2,
                typ: 5,
                count: 3,
                data: [rational(45, 1), rational(28, 1), rational(0, 1)].concat(),
            }],
            dpi: None,
        }
    }

    #[test]
    fn resolution_is_read_apart() {
        let rational = |n: u32, d: u32| [n.to_le_bytes(), d.to_le_bytes()].concat();
        let mut m = sample();
        m.main.push(Entry { tag: TAG_X_RESOLUTION, typ: TYPE_RATIONAL, count: 1, data: rational(11811, 100) });
        m.main.push(Entry { tag: TAG_Y_RESOLUTION, typ: TYPE_RATIONAL, count: 1, data: rational(11811, 100) });
        m.main.push(Entry { tag: TAG_RESOLUTION_UNIT, typ: TYPE_SHORT, count: 1, data: 3u16.to_le_bytes().to_vec() });
        let back = from_exif(&to_exif(&m)).unwrap();
        // 118.11 per centimetre is 300 per inch
        let dpi = back.dpi.unwrap();
        assert!((dpi.x - 300.0).abs() < 0.01 && (dpi.y - 300.0).abs() < 0.01, "{dpi:?}");
        // the tags themselves are not carried: each encoder writes its own
        assert_eq!(back.main, sample().main);
    }

    /// The same structure in big-endian order, plus tags that must be dropped.
    fn big_endian_blob(meta: &Metadata) -> Vec<u8> {
        let mut m = meta.clone();
        m.main.push(Entry { tag: 274, typ: 3, count: 1, data: 6u16.to_le_bytes().to_vec() }); // orientation
        m.exif.push(Entry { tag: 37500, typ: 7, count: 6, data: b"maker!".to_vec() }); // maker note
        let le = to_exif(&m);
        // re-encode as big-endian through the reader's own model
        let (mut r, first) = Reader::open(Slice(&le)).unwrap();
        let ifd0 = r.ifd(first).unwrap();
        let exif = r.ifd(ifd0.iter().find(|e| e.tag == TAG_EXIF_IFD).and_then(pointer).unwrap()).unwrap();
        let gps = r.ifd(ifd0.iter().find(|e| e.tag == TAG_GPS_IFD).and_then(pointer).unwrap()).unwrap();
        // write the three IFDs big-endian by hand
        let swap = |e: &Entry| {
            let (_, unit) = type_size(e.typ).unwrap();
            let mut d = e.data.clone();
            d.chunks_exact_mut(unit).for_each(<[u8]>::reverse);
            d
        };
        let mut out = b"MM\x00\x2A\x00\x00\x00\x08".to_vec();
        let mut pending: Vec<(usize, Vec<Entry>)> = Vec::new();
        let write = |out: &mut Vec<u8>, entries: &[Entry], pointers: &mut Vec<(usize, Vec<Entry>)>| {
            let start = out.len();
            let table = 2 + 12 * entries.len() + 4;
            let mut values = Vec::new();
            out.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_be_bytes());
            for e in entries {
                out.extend_from_slice(&e.tag.to_be_bytes());
                out.extend_from_slice(&e.typ.to_be_bytes());
                out.extend_from_slice(&e.count.to_be_bytes());
                let d = swap(e);
                if e.tag == TAG_EXIF_IFD {
                    pointers.push((out.len(), exif.clone()));
                    out.extend_from_slice(&[0; 4]);
                } else if e.tag == TAG_GPS_IFD {
                    pointers.push((out.len(), gps.clone()));
                    out.extend_from_slice(&[0; 4]);
                } else if d.len() <= 4 {
                    let mut v = d;
                    v.resize(4, 0);
                    out.extend_from_slice(&v);
                } else {
                    out.extend_from_slice(&u32::try_from(start + table + values.len()).unwrap().to_be_bytes());
                    values.extend_from_slice(&d);
                }
            }
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&values);
        };
        write(&mut out, &ifd0, &mut pending);
        let mut none = Vec::new();
        for (field, entries) in pending {
            let at = u32::try_from(out.len()).unwrap();
            out[field..field + 4].copy_from_slice(&at.to_be_bytes());
            write(&mut out, &entries, &mut none);
        }
        out
    }

    #[test]
    fn exif_block_roundtrip_drops_what_cannot_move() {
        let meta = sample();
        assert_eq!(from_exif(&to_exif(&meta)).unwrap(), meta);
        let be = big_endian_blob(&meta);
        assert_eq!(&be[..2], b"MM");
        // orientation and maker note are gone, the rest is equal and little-endian
        assert_eq!(from_exif(&crate::exif::with_header(&be)).unwrap(), meta);
    }

    #[test]
    fn standalone_ifd_points_inside_itself() {
        let meta = sample();
        let at = 1000;
        let ifd = ifd_at(&meta.exif, at);
        let mut file = vec![0u8; at as usize];
        file[..8].copy_from_slice(b"II\x2A\x00\x00\x00\x00\x00");
        file[4..8].copy_from_slice(&at.to_le_bytes());
        file.extend_from_slice(&ifd);
        let (mut r, first) = Reader::open(Slice(&file)).unwrap();
        let mut back = r.ifd(first).unwrap();
        back.sort_by_key(|e| e.tag);
        let mut want = meta.exif.clone();
        want.sort_by_key(|e| e.tag);
        assert_eq!(back, want);
    }

    #[test]
    fn damaged_structures_are_harmless() {
        assert!(from_exif(b"").is_none());
        assert!(from_exif(b"II\x2A\x00\xFF\xFF\xFF\xFF").is_none());
        // an entry pointing past the end is skipped, the others are kept
        let mut blob = to_exif(&sample());
        let n = blob.len();
        blob.truncate(n - 4);
        assert!(from_exif(&blob).is_some());
    }
}
