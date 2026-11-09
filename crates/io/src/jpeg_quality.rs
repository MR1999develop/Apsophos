//! How strongly a JPEG was compressed, estimated from its luminance
//! quantisation table (read by [`crate::jpeg_app`] from the DQT segments,
//! within that reader's limits).
//!
//! Most encoders (libjpeg and its descendants, and the IJG scale that
//! programs show as "quality 1–100") scale the example table of the JPEG
//! standard (ITU-T T.81, Annex K): a step is `(base × s + 50) / 100` with
//! `s = 5000 / q` below 50 and `s = 200 − 2q` from 50 up. The estimate
//! inverts that on the sum of the steps, so a table scaled another way (a
//! camera's own) still gets the IJG quality of the same strength.

/// The steps of the example luminance table (T.81 Annex K, table K.1); the
/// order does not matter for their sum.
const BASE_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51,
    87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];

/// A JPEG's quality on the IJG scale, 1 to 100, estimated from its
/// luminance quantisation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegQuality(u8);

impl JpegQuality {
    /// Below this the steps are coarser than the standard's example table
    /// (which the IJG scale puts at 50): the 8 × 8 blocks and the ringing
    /// along edges show in smooth areas. That is stronger compression than
    /// the moderate one the denoising network learned, so it may keep the
    /// blocks or smooth them into blotches. Cameras and phones save at 85
    /// and up, web pages mostly at 70 to 85.
    pub const HEAVY_BELOW: u8 = 50;

    /// The quality, 1 to 100.
    pub fn get(self) -> u8 {
        self.0
    }

    /// Compressed more strongly than [`Self::HEAVY_BELOW`].
    pub fn is_heavy(self) -> bool {
        self.0 < Self::HEAVY_BELOW
    }

    /// The quality whose IJG table has the steps of `table` (64 steps, in
    /// any order); `None` for a table with a zero step, which no valid JPEG
    /// has.
    pub(crate) fn estimate(table: &[u16; 64]) -> Option<Self> {
        if table.contains(&0) {
            return None;
        }
        let sum = |t: &[u16; 64]| t.iter().map(|&v| f64::from(v)).sum::<f64>();
        let scale = 100.0 * sum(table) / sum(&BASE_LUMA);
        let q = if scale <= 100.0 { (200.0 - scale) / 2.0 } else { 5000.0 / scale };
        let q = denoise_core::num::round_to_u16(q.min(100.0), 1);
        u8::try_from(q).ok().map(Self)
    }
}

/// The quantisation tables of a DQT segment's payload, as table id and the
/// 64 steps (in the file's zig-zag order); stops at the first table that is
/// malformed or cut short. The payload is at most one segment (64 KB), so
/// at most about a thousand tables.
pub(crate) fn dqt_tables(mut payload: &[u8]) -> Vec<(u8, [u16; 64])> {
    let mut out = Vec::new();
    while let Some((&head, rest)) = payload.split_first() {
        // high nibble: 0 for 8-bit steps, 1 for 16-bit; low nibble: id 0..=3
        let (width, id) = match (head >> 4, head & 0x0F) {
            (0, id @ 0..=3) => (1, id),
            (1, id @ 0..=3) => (2, id),
            _ => break,
        };
        let Some(values) = rest.get(..64 * width) else { break };
        let mut table = [0u16; 64];
        for (step, v) in table.iter_mut().zip(values.chunks_exact(width)) {
            *step = match *v {
                [b] => u16::from(b),
                [hi, lo] => u16::from_be_bytes([hi, lo]),
                _ => 0,
            };
        }
        out.push((id, table));
        payload = &rest[64 * width..];
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The table an IJG encoder writes at `quality` (baseline: steps up to 255).
    pub fn ijg_table(quality: u8) -> [u16; 64] {
        let q = u32::from(quality.clamp(1, 100));
        let s = if q < 50 { 5000 / q } else { 200 - 2 * q };
        BASE_LUMA.map(|b| u16::try_from(((u32::from(b) * s + 50) / 100).clamp(1, 255)).unwrap())
    }

    #[test]
    fn ijg_tables_give_their_quality_back() {
        // from 25 up no step reaches the 8-bit ceiling of 255
        for q in 25..=100 {
            let est = JpegQuality::estimate(&ijg_table(q)).unwrap().get();
            assert!(est.abs_diff(q) <= 1, "quality {q} estimated {est}");
        }
        // below, the ceiling flattens the table: still heavy
        for q in 1..25 {
            assert!(JpegQuality::estimate(&ijg_table(q)).unwrap().is_heavy(), "quality {q}");
        }
    }

    #[test]
    fn the_threshold() {
        assert!(JpegQuality::estimate(&ijg_table(40)).unwrap().is_heavy());
        assert!(!JpegQuality::estimate(&ijg_table(50)).unwrap().is_heavy());
        assert!(!JpegQuality::estimate(&ijg_table(92)).unwrap().is_heavy());
        // all ones: the finest table there is (steps cannot go below 1, so
        // 99 and 100 write the same table)
        assert!(JpegQuality::estimate(&[1; 64]).is_some_and(|q| q.get() >= 99));
        assert_eq!(JpegQuality::estimate(&[0; 64]), None);
    }

    #[test]
    fn dqt_payloads() {
        let luma = ijg_table(75);
        let mut payload = vec![0x00];
        payload.extend(luma.iter().map(|&v| u8::try_from(v).unwrap()));
        // a 16-bit table with id 1 after it
        payload.push(0x11);
        payload.extend(luma.iter().flat_map(|v| v.to_be_bytes()));
        let tables = dqt_tables(&payload);
        assert_eq!(tables, vec![(0, luma), (1, luma)]);
        // cut short, bad precision, bad id: what came before is kept
        assert_eq!(dqt_tables(&payload[..60]), vec![]);
        assert_eq!(dqt_tables(&payload[..65 + 30]).len(), 1);
        for bad in [0x20, 0x04] {
            let mut p = payload[..65].to_vec();
            p.push(bad);
            p.extend([1; 128]);
            assert_eq!(dqt_tables(&p).len(), 1, "head {bad:#x}");
        }
        assert!(dqt_tables(&[]).is_empty());
    }
}
