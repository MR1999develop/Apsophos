//! Colour management of the viewer: the pixels go from the image's ICC
//! profile to sRGB, so a photo in Adobe RGB or ProPhoto does not look dull.
//!
//! Images without a profile are taken as sRGB. Only the screen is affected:
//! the exported files keep their own values and profile.

use denoise_core::quant::{to_u16, to_u8};
use lcms2::{ColorSpaceSignature, DisallowCache, Flags, GlobalContext, Intent, PixelFormat, Profile, Transform};

use crate::text::tr;

/// Transforms built without the one-pixel cache, so that the rows of a tile
/// can be converted in parallel.
type RgbTransform = Transform<[u16; 3], [u8; 3], GlobalContext, DisallowCache>;
type GrayTransform = Transform<u16, [u8; 3], GlobalContext, DisallowCache>;

enum Kind {
    /// The image is sRGB: rounding is all that is needed.
    Identity,
    Rgb(RgbTransform),
    /// Grey image with a grey profile: one channel in, RGB out.
    Gray(GrayTransform),
}

/// Conversion of the image's pixels (0..=1, RGB) into sRGB for the screen.
pub struct DisplayTransform {
    kind: Kind,
    /// Why the image's profile could not be used, for the information panel.
    pub problem: Option<String>,
}

impl DisplayTransform {
    /// sRGB in, sRGB out.
    pub fn identity() -> Self {
        Self { kind: Kind::Identity, problem: None }
    }

    /// From the image's profile `source` (`None`: sRGB) to sRGB. A profile
    /// that cannot be used is replaced by sRGB and reported in
    /// [`problem`](Self::problem).
    pub fn new(source: Option<&[u8]>) -> Self {
        if source.is_none() {
            return Self::identity();
        }
        let mut problems = Vec::new();
        let output = Profile::new_srgb();
        let input = match source.map(Profile::new_icc) {
            None => None,
            Some(Ok(p)) if matches!(p.color_space(), ColorSpaceSignature::RgbData | ColorSpaceSignature::GrayData) => {
                Some(p)
            }
            Some(_) => {
                problems.push(tr().info.display_unusable_profile);
                None
            }
        };
        let flags = Flags::NO_CACHE;
        let kind = match &input {
            Some(p) if p.color_space() == ColorSpaceSignature::GrayData => GrayTransform::new_flags_context(
                GlobalContext::new(),
                p,
                PixelFormat::GRAY_16,
                &output,
                PixelFormat::RGB_8,
                Intent::Perceptual,
                flags,
            )
            .map(Kind::Gray),
            _ => {
                let srgb;
                let p = match &input {
                    Some(p) => p,
                    None => {
                        srgb = Profile::new_srgb();
                        &srgb
                    }
                };
                RgbTransform::new_flags_context(
                    GlobalContext::new(),
                    p,
                    PixelFormat::RGB_16,
                    &output,
                    PixelFormat::RGB_8,
                    Intent::Perceptual,
                    flags,
                )
                .map(Kind::Rgb)
            }
        };
        let kind = kind.unwrap_or_else(|_| {
            problems.push(tr().info.display_no_conversion);
            Kind::Identity
        });
        Self { kind, problem: (!problems.is_empty()).then(|| problems.join("; ")) }
    }

    /// Whether the pixels are converted (the image has its own profile).
    pub fn is_managed(&self) -> bool {
        !matches!(self.kind, Kind::Identity)
    }

    /// Converts one row of RGB pixels into `out` (RGBA, alpha 255);
    /// `out.len() == 4 * rgb.len()`.
    pub fn row(&self, rgb: &[[f32; 3]], out: &mut [u8]) {
        let mut converted = vec![[0u8; 3]; rgb.len()];
        match &self.kind {
            Kind::Identity => return unconverted_row(rgb, out),
            Kind::Rgb(t) => {
                let input: Vec<[u16; 3]> = rgb.iter().map(|s| [to_u16(s[0]), to_u16(s[1]), to_u16(s[2])]).collect();
                t.transform_pixels(&input, &mut converted);
            }
            Kind::Gray(t) => {
                let input: Vec<u16> = rgb.iter().map(|s| to_u16(s[0])).collect();
                t.transform_pixels(&input, &mut converted);
            }
        }
        for (px, c) in out.as_chunks_mut::<4>().0.iter_mut().zip(&converted) {
            px.copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
}

/// One row of RGB pixels into `out` (RGBA, alpha 255) without colour
/// conversion: for sRGB, and for views that are not photographs (the removed
/// noise).
pub fn unconverted_row(rgb: &[[f32; 3]], out: &mut [u8]) {
    for (px, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(rgb) {
        px.copy_from_slice(&[to_u8(s[0]), to_u8(s[1]), to_u8(s[2]), 255]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb_bytes() -> Vec<u8> {
        Profile::new_srgb().icc().unwrap()
    }

    #[test]
    fn srgb_to_srgb_is_rounding() {
        let row = [[0.0, 0.5, 1.0], [0.25, 0.75, 0.1]];
        let mut plain = [0u8; 8];
        DisplayTransform::identity().row(&row, &mut plain);
        assert_eq!(plain, [0, 128, 255, 255, 64, 191, 26, 255]);
        // an explicit sRGB profile gives the same values, give or take a level
        let managed = DisplayTransform::new(Some(&srgb_bytes()));
        assert!(managed.is_managed() && managed.problem.is_none());
        let mut out = [0u8; 8];
        managed.row(&row, &mut out);
        assert!(out.iter().zip(plain).all(|(a, b)| a.abs_diff(b) <= 1), "{out:?} vs {plain:?}");
    }

    #[test]
    fn wide_gamut_source_is_converted() {
        // a pure green in a wider space is more saturated than sRGB can show:
        // red and blue must go negative, i.e. clip to 0, and green stay high
        let gamma = lcms2::ToneCurve::new(2.2);
        let wide = Profile::new_rgb(
            &lcms2::CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 },
            &lcms2::CIExyYTRIPLE {
                Red: lcms2::CIExyY { x: 0.7347, y: 0.2653, Y: 1.0 },
                Green: lcms2::CIExyY { x: 0.1596, y: 0.8404, Y: 1.0 },
                Blue: lcms2::CIExyY { x: 0.0366, y: 0.0001, Y: 1.0 },
            },
            &[&gamma, &gamma, &gamma],
        )
        .unwrap();
        let t = DisplayTransform::new(Some(&wide.icc().unwrap()));
        let mut out = [0u8; 4];
        t.row(&[[0.5, 0.5, 0.5]], &mut out);
        // neutral stays neutral
        assert!(out[0].abs_diff(out[1]) <= 1 && out[1].abs_diff(out[2]) <= 1, "{out:?}");
        t.row(&[[0.0, 0.6, 0.0]], &mut out);
        assert!(out[0] < 40 && out[2] < 40 && out[1] > 150, "{out:?}");
    }

    #[test]
    fn unusable_profiles_fall_back_to_srgb() {
        let t = DisplayTransform::new(Some(b"not a profile"));
        assert!(t.problem.is_some());
        let mut out = [0u8; 4];
        t.row(&[[1.0, 0.0, 0.0]], &mut out);
        assert_eq!(out, [255, 0, 0, 255]);
        assert!(DisplayTransform::new(None).problem.is_none());
    }

    #[test]
    fn gray_profile_reads_one_channel() {
        let gray =
            Profile::new_gray(&lcms2::CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 }, &lcms2::ToneCurve::new(2.2)).unwrap();
        let t = DisplayTransform::new(Some(&gray.icc().unwrap()));
        assert!(t.problem.is_none());
        let mut out = [0u8; 4];
        t.row(&[[0.5, 0.5, 0.5]], &mut out);
        assert!(out[0] == out[1] && out[1] == out[2] && (100..160).contains(&out[0]), "{out:?}");
    }
}
