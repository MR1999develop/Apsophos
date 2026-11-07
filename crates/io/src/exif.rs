//! Reading and resetting the Orientation tag (0x0112) in an EXIF block.
//!
//! An EXIF block is structured like a TIFF: a header with the byte order, then
//! the first IFD with 12-byte entries. Only the SHORT orientation value in
//! IFD0 is touched; everything else stays unchanged.

const HEADER: &[u8] = b"Exif\0\0";
const TAG_ORIENTATION: u16 = 0x0112;
const TYPE_SHORT: u16 = 3;

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

struct Reader<'a> {
    data: &'a [u8],
    little: bool,
}

impl Reader<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }
}

/// Byte position of the SHORT orientation value in IFD0.
fn orientation_offset(tiff: &[u8]) -> Option<(usize, bool)> {
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r = Reader { data: tiff, little };
    if r.u16(2)? != 42 {
        return None;
    }
    let ifd = usize::try_from(r.u32(4)?).ok()?;
    let count = usize::from(r.u16(ifd)?);
    (0..count).find_map(|i| {
        let entry = ifd + 2 + i * 12;
        (r.u16(entry)? == TAG_ORIENTATION && r.u16(entry + 2)? == TYPE_SHORT).then_some((entry + 8, little))
    })
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
        let mut junk = vec![b'I', b'I', 42, 0, 255, 255, 255, 255];
        assert_eq!(orientation(&junk), 1);
        reset_orientation(&mut junk);
        assert_eq!(orientation(&[]), 1);
    }
}
