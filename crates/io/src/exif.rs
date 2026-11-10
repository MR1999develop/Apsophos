//! Changes made in place to an EXIF block before it goes into an output
//! file: the Orientation tag (0x0112), because the pixels are already
//! rotated, and the pixel dimensions of the EXIF sub-IFD (`PixelXDimension`,
//! `PixelYDimension`), because the output may be rotated.
//!
//! An EXIF block is structured like a TIFF: a header with the byte order, then
//! the first IFD with 12-byte entries, which may point to the EXIF sub-IFD.
//! Only values that fit in an entry are touched, so the block keeps its size
//! and every offset in it stays valid; everything else stays unchanged. Every
//! read is bounded by the block: a damaged one is left as it is.

const HEADER: &[u8] = b"Exif\0\0";
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_EXIF_IFD: u16 = 34665;
pub(crate) const TAG_PIXEL_X: u16 = 0xA002;
pub(crate) const TAG_PIXEL_Y: u16 = 0xA003;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
/// The IFD type, which some writers use for sub-IFD pointers.
const TYPE_IFD: u16 = 13;
const ENTRY_BYTES: usize = 12;

/// Removes the `Exif\0\0` header, if present.
pub fn strip_header(blob: &[u8]) -> &[u8] {
    blob.strip_prefix(HEADER).unwrap_or(blob)
}

/// Header to prepend in the JPEG APP1 segment.
pub fn with_header(tiff: &[u8]) -> Vec<u8> {
    let mut v = HEADER.to_vec();
    v.extend_from_slice(tiff);
    v
}

/// The EXIF block (without header) for an output file of `width × height`
/// pixels made from `blob`: orientation "normal" and the pixel dimensions
/// of the file.
pub fn for_output(blob: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = blob.to_vec();
    reset_orientation(&mut out);
    set_pixel_dimensions(&mut out, width, height);
    out
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
    little: bool,
}

impl<'a> Reader<'a> {
    /// The reader of a TIFF structure, if its header is one.
    fn open(data: &'a [u8]) -> Option<Self> {
        let little = match data.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let r = Reader { data, little };
        (r.u16(2)? == 42).then_some(r)
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }

    /// Byte position of the entry with `tag` in the IFD at `ifd`.
    fn entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        let count = usize::from(self.u16(ifd)?);
        (0..count).map_while(|i| ifd.checked_add(2 + i * ENTRY_BYTES)).find(|&e| self.u16(e) == Some(tag))
    }

    /// Byte position of the first IFD.
    fn first_ifd(&self) -> Option<usize> {
        usize::try_from(self.u32(4)?).ok()
    }

    /// Byte position of the EXIF sub-IFD.
    fn exif_ifd(&self) -> Option<usize> {
        let e = self.entry(self.first_ifd()?, TAG_EXIF_IFD)?;
        matches!(self.u16(e + 2)?, TYPE_LONG | TYPE_IFD).then_some(())?;
        (self.u32(e + 4)? == 1).then_some(())?;
        usize::try_from(self.u32(e + 8)?).ok()
    }
}

/// Byte position of the SHORT orientation value in IFD0.
fn orientation_offset(tiff: &[u8]) -> Option<(usize, bool)> {
    let r = Reader::open(tiff)?;
    let e = r.entry(r.first_ifd()?, TAG_ORIENTATION)?;
    (r.u16(e + 2)? == TYPE_SHORT).then_some((e + 8, r.little))
}

/// Orientation 1..=8 (1 if missing or unreadable).
pub fn orientation(blob: &[u8]) -> u16 {
    let tiff = strip_header(blob);
    orientation_offset(tiff)
        .and_then(|(at, little)| Reader { data: tiff, little }.u16(at))
        .filter(|v| (1..=8).contains(v))
        .unwrap_or(1)
}

/// Sets the orientation to 1 ("normal"), because the pixels are already rotated.
pub fn reset_orientation(tiff: &mut [u8]) {
    if let Some((at, little)) = orientation_offset(tiff) {
        let one = if little { 1u16.to_le_bytes() } else { 1u16.to_be_bytes() };
        if let Some(slot) = tiff.get_mut(at..at + 2) {
            slot.copy_from_slice(&one);
        }
    }
}

/// Sets `PixelXDimension` and `PixelYDimension` of the EXIF sub-IFD to
/// `width` and `height`, where they are present as one SHORT or LONG
/// value. They become LONG (EXIF allows either), so that any size fits in
/// the entry; tags that are missing are not added.
pub fn set_pixel_dimensions(tiff: &mut [u8], width: u32, height: u32) {
    let Some(r) = Reader::open(tiff) else { return };
    let little = r.little;
    let Some(ifd) = r.exif_ifd() else { return };
    let slots: Vec<(usize, u32)> = [(TAG_PIXEL_X, width), (TAG_PIXEL_Y, height)]
        .into_iter()
        .filter_map(|(tag, value)| {
            let e = r.entry(ifd, tag)?;
            let one_number = matches!(r.u16(e + 2)?, TYPE_SHORT | TYPE_LONG) && r.u32(e + 4)? == 1;
            (one_number && e.checked_add(ENTRY_BYTES)? <= tiff.len()).then_some((e, value))
        })
        .collect();
    for (e, value) in slots {
        let (typ, count, value) = if little {
            (TYPE_LONG.to_le_bytes(), 1u32.to_le_bytes(), value.to_le_bytes())
        } else {
            (TYPE_LONG.to_be_bytes(), 1u32.to_be_bytes(), value.to_be_bytes())
        };
        if let Some(entry) = tiff.get_mut(e + 2..e + ENTRY_BYTES) {
            entry[..2].copy_from_slice(&typ);
            entry[2..6].copy_from_slice(&count);
            entry[6..].copy_from_slice(&value);
        }
    }
}

/// The pixel dimensions of the EXIF sub-IFD, where both are one SHORT or
/// LONG value.
#[cfg(test)]
pub(crate) fn pixel_dimensions(blob: &[u8]) -> Option<(u32, u32)> {
    let r = Reader::open(strip_header(blob))?;
    let ifd = r.exif_ifd()?;
    let value = |tag| {
        let e = r.entry(ifd, tag)?;
        match r.u16(e + 2)? {
            TYPE_SHORT => r.u16(e + 8).map(u32::from),
            TYPE_LONG => r.u32(e + 8),
            _ => None,
        }
    };
    Some((value(TAG_PIXEL_X)?, value(TAG_PIXEL_Y)?))
}

#[cfg(test)]
pub(crate) fn sample(orientation: u16, little: bool) -> Vec<u8> {
    let p16 = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let p32 = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let mut v = Vec::new();
    v.extend_from_slice(if little { b"II" } else { b"MM" });
    v.extend_from_slice(&p16(42));
    v.extend_from_slice(&p32(8));
    v.extend_from_slice(&p16(1)); // one entry
    v.extend_from_slice(&p16(TAG_ORIENTATION));
    v.extend_from_slice(&p16(TYPE_SHORT));
    v.extend_from_slice(&p32(1));
    v.extend_from_slice(&p16(orientation));
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&p32(0)); // no next IFD
    v
}

/// A block whose EXIF sub-IFD holds the pixel dimensions as SHORT values,
/// after a maker note whose offset (68) must stay valid.
#[cfg(test)]
pub(crate) fn sample_with_dimensions(width: u16, height: u16, little: bool) -> Vec<u8> {
    let p16 = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let p32 = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let entry = |v: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: [u8; 4]| {
        v.extend_from_slice(&p16(tag));
        v.extend_from_slice(&p16(typ));
        v.extend_from_slice(&p32(count));
        v.extend_from_slice(&value);
    };
    let short = |s: u16| {
        let b = p16(s);
        [b[0], b[1], 0, 0]
    };
    let mut v = Vec::new();
    v.extend_from_slice(if little { b"II" } else { b"MM" });
    v.extend_from_slice(&p16(42));
    v.extend_from_slice(&p32(8));
    // IFD0 at 8: the pointer to the EXIF sub-IFD at 26
    v.extend_from_slice(&p16(1));
    entry(&mut v, TAG_EXIF_IFD, TYPE_LONG, 1, p32(26));
    v.extend_from_slice(&p32(0));
    // EXIF sub-IFD at 26, three entries at 28, 40 and 52; the maker note at 68
    v.extend_from_slice(&p16(3));
    entry(&mut v, 37500, 7, 8, p32(68));
    entry(&mut v, TAG_PIXEL_X, TYPE_SHORT, 1, short(width));
    entry(&mut v, TAG_PIXEL_Y, TYPE_SHORT, 1, short(height));
    v.extend_from_slice(&p32(0));
    v.extend_from_slice(b"maker!!!");
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_reset_both_byte_orders() {
        for little in [true, false] {
            let mut blob = sample(6, little);
            assert_eq!(orientation(&blob), 6);
            assert_eq!(orientation(&with_header(&blob)), 6);
            reset_orientation(&mut blob);
            assert_eq!(orientation(&blob), 1);
        }
    }

    #[test]
    fn garbage_is_harmless() {
        let original = vec![b'I', b'I', 42, 0, 255, 255, 255, 255];
        let mut junk = original.clone();
        assert_eq!(orientation(&junk), 1);
        reset_orientation(&mut junk);
        set_pixel_dimensions(&mut junk, 10, 10);
        assert_eq!(junk, original);
        assert_eq!(pixel_dimensions(&junk), None);
        assert_eq!(orientation(&[]), 1);
    }

    #[test]
    fn pixel_dimensions_follow_the_output_in_both_byte_orders() {
        for little in [true, false] {
            let blob = sample_with_dimensions(6000, 4000, little);
            assert_eq!(pixel_dimensions(&blob), Some((6000, 4000)));
            assert_eq!(pixel_dimensions(&with_header(&blob)), Some((6000, 4000)));
            // beyond what a SHORT holds: the entries become LONG in place
            let out = for_output(&blob, 24_000, 70_000);
            assert_eq!(pixel_dimensions(&out), Some((24_000, 70_000)));
            // nothing else moves: only the two entries change, the maker
            // note stays where its offset says
            assert_eq!(out.len(), blob.len());
            assert!((0..blob.len()).filter(|&i| blob[i] != out[i]).all(|i| (40..64).contains(&i)));
            assert_eq!(&out[68..], b"maker!!!");
        }
    }

    #[test]
    fn missing_dimensions_are_not_added() {
        let blob = sample(6, true);
        let out = for_output(&blob, 100, 100);
        assert_eq!(pixel_dimensions(&out), None);
        assert_eq!(orientation(&out), 1);
        assert_eq!(out.len(), blob.len());
    }

    #[test]
    fn structures_out_of_the_block_are_left_alone() {
        // the pointer to the sub-IFD (the value of IFD0's only entry) past the end
        let mut blob = sample_with_dimensions(60, 40, true);
        blob[18..22].copy_from_slice(&1000u32.to_le_bytes());
        let before = blob.clone();
        set_pixel_dimensions(&mut blob, 1, 1);
        assert_eq!(blob, before);
        // a sub-IFD cut in its third entry: the second one is set, nothing past the end
        let mut cut = sample_with_dimensions(60, 40, true);
        cut.truncate(58);
        let before = cut.clone();
        set_pixel_dimensions(&mut cut, 1, 1);
        assert_eq!(cut.len(), before.len());
        assert_eq!(cut[..40], before[..40]);
        assert_eq!(cut[48..52], 1u32.to_le_bytes());
        assert_eq!(cut[52..], before[52..]);
    }
}
