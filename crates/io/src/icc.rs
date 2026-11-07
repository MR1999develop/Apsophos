//! Reading the name of an ICC profile (`desc` tag, versions 2 and 4).

/// Readable name of the profile, e.g. "Adobe RGB (1998)".
pub fn icc_description(icc: &[u8]) -> Option<String> {
    let be32 = |at: usize| -> Option<usize> {
        let b: [u8; 4] = icc.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_be_bytes(b)).ok()
    };
    let count = be32(128)?;
    let (offset, size) = (0..count.min(256)).find_map(|i| {
        let e = 132 + i * 12;
        (icc.get(e..e + 4)? == b"desc").then(|| Some((be32(e + 4)?, be32(e + 8)?))).flatten()
    })?;
    let tag = icc.get(offset..offset.checked_add(size)?)?;
    let text = match tag.get(..4)? {
        b"desc" => {
            // textDescriptionType: ASCII length (including the final zero) at +8
            let n = usize::try_from(u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?)).ok()?;
            String::from_utf8_lossy(tag.get(12..12 + n)?).into_owned()
        }
        b"mluc" => {
            // multiLocalizedUnicodeType: first record, UTF-16BE text
            let len = usize::try_from(u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?)).ok()?;
            let off = usize::try_from(u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?)).ok()?;
            let units: Vec<u16> = tag
                .get(off..off.checked_add(len)?)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => return None,
    };
    let text = text.trim_matches(char::from(0)).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Colour space declared in the header (e.g. `RGB `, `CMYK`, `GRAY`).
pub fn color_space(icc: &[u8]) -> Option<&[u8]> {
    icc.get(16..20)
}

#[cfg(test)]
pub(crate) fn sample_v2(name: &str) -> Vec<u8> {
    let mut tag = b"desc\0\0\0\0".to_vec();
    tag.extend_from_slice(&u32::try_from(name.len() + 1).unwrap_or(0).to_be_bytes());
    tag.extend_from_slice(name.as_bytes());
    tag.push(0);
    let mut icc = vec![0u8; 128];
    icc[16..20].copy_from_slice(b"RGB ");
    icc.extend_from_slice(&1u32.to_be_bytes());
    icc.extend_from_slice(b"desc");
    icc.extend_from_slice(&144u32.to_be_bytes());
    icc.extend_from_slice(&u32::try_from(tag.len()).unwrap_or(0).to_be_bytes());
    icc.extend_from_slice(&tag);
    let total = u32::try_from(icc.len()).unwrap_or(0).to_be_bytes();
    icc[0..4].copy_from_slice(&total);
    icc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_v2_description() {
        assert_eq!(icc_description(&sample_v2("Adobe RGB (1998)")).as_deref(), Some("Adobe RGB (1998)"));
        assert_eq!(color_space(&sample_v2("x")), Some(&b"RGB "[..]));
    }

    #[test]
    fn reads_v4_mluc() {
        let text: Vec<u8> = "Display P3".encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut tag = b"mluc\0\0\0\0".to_vec();
        tag.extend_from_slice(&1u32.to_be_bytes()); // record
        tag.extend_from_slice(&12u32.to_be_bytes()); // record size
        tag.extend_from_slice(b"enUS");
        tag.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
        tag.extend_from_slice(&28u32.to_be_bytes());
        tag.extend_from_slice(&text);
        let mut icc = vec![0u8; 128];
        icc.extend_from_slice(&1u32.to_be_bytes());
        icc.extend_from_slice(b"desc");
        icc.extend_from_slice(&144u32.to_be_bytes());
        icc.extend_from_slice(&u32::try_from(tag.len()).unwrap().to_be_bytes());
        icc.extend_from_slice(&tag);
        assert_eq!(icc_description(&icc).as_deref(), Some("Display P3"));
    }

    #[test]
    fn truncated_profile_is_none() {
        assert_eq!(icc_description(&[0u8; 50]), None);
        let mut p = sample_v2("Name");
        p.truncate(150);
        assert_eq!(icc_description(&p), None);
    }
}
