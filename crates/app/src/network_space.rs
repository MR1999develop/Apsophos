//! The network was trained on sRGB photographs: on a photo in Adobe RGB,
//! ProPhoto or a linear space the same noise has other values, and the
//! network removes too little or too much of it. Such tiles are converted to
//! sRGB before the network and only the correction it makes is brought back,
//! so the result keeps the photo's own colour space and profile, and colours
//! outside sRGB are not clipped. For an upscaler the correction is measured
//! against the classical enlargement (Catmull-Rom) of the same tile.
//!
//! Photos without a profile, with an sRGB profile, with a grey profile or
//! with a profile that cannot be used go to the network as they are.

use denoise_core::resample::enlarge;
use denoise_core::{ModelError, TileModel};
use lcms2::{ColorSpaceSignature, Intent, PixelFormat, Profile, Transform};

type FloatTransform = Transform<[f32; 3], [f32; 3]>;

/// Largest difference from sRGB, on a grid of test colours, for a profile to
/// count as sRGB (about a quarter of an 8-bit level).
const SRGB_TOLERANCE: f32 = 0.001;

/// Conversions between the photo's colour space and sRGB.
pub struct NetworkSpace {
    to_srgb: FloatTransform,
    from_srgb: FloatTransform,
}

impl NetworkSpace {
    /// The conversions for the profile `icc`; `None` when the network can
    /// take the pixels as they are (see the module documentation).
    pub fn for_profile(icc: Option<&[u8]>) -> Option<Self> {
        let source = Profile::new_icc(icc?).ok()?;
        if source.color_space() != ColorSpaceSignature::RgbData {
            return None;
        }
        let srgb = Profile::new_srgb();
        let intent = Intent::RelativeColorimetric;
        let to_srgb = Transform::new(&source, PixelFormat::RGB_FLT, &srgb, PixelFormat::RGB_FLT, intent).ok()?;
        let from_srgb = Transform::new(&srgb, PixelFormat::RGB_FLT, &source, PixelFormat::RGB_FLT, intent).ok()?;
        let space = Self { to_srgb, from_srgb };
        (!space.is_srgb()).then_some(space)
    }

    /// `true` if the conversion to sRGB changes no test colour noticeably:
    /// an sRGB profile written by another program, with other tags.
    fn is_srgb(&self) -> bool {
        const STEPS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut grid = Vec::with_capacity(STEPS.len().pow(3));
        for r in STEPS {
            for g in STEPS {
                grid.extend(STEPS.map(|b| [r, g, b]));
            }
        }
        let mut out = vec![[0.0; 3]; grid.len()];
        self.to_srgb.transform_pixels(&grid, &mut out);
        grid.iter().zip(&out).all(|(a, b)| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= SRGB_TOLERANCE))
    }
}

/// A tile model that sees sRGB: wraps the network for a photo in another
/// colour space.
pub struct InSrgb<'a> {
    model: &'a mut dyn TileModel,
    space: &'a NetworkSpace,
    /// Buffers reused from tile to tile.
    source: Vec<[f32; 3]>,
    srgb: Vec<[f32; 3]>,
    planar: Vec<f32>,
    back: Vec<[f32; 3]>,
    base: Vec<[f32; 3]>,
}

impl<'a> InSrgb<'a> {
    pub fn new(model: &'a mut dyn TileModel, space: &'a NetworkSpace) -> Self {
        Self {
            model,
            space,
            source: Vec::new(),
            srgb: Vec::new(),
            planar: Vec::new(),
            back: Vec::new(),
            base: Vec::new(),
        }
    }
}

impl TileModel for InSrgb<'_> {
    fn input_px(&self) -> usize {
        self.model.input_px()
    }

    fn margin_px(&self) -> usize {
        self.model.margin_px()
    }

    fn scale(&self) -> usize {
        self.model.scale()
    }

    fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
        let (side, scale) = (self.model.input_px(), self.model.scale());
        let n = side * side;
        if planar_in.len() != 3 * n || planar_out.len() != 3 * n * scale * scale {
            return Err(ModelError("tile buffers of unexpected sizes".into()));
        }
        interleave(planar_in, &mut self.source);
        self.srgb.resize(n, [0.0; 3]);
        self.space.to_srgb.transform_pixels(&self.source, &mut self.srgb);
        // the network takes 0..=1: colours outside sRGB are clipped for it,
        // and the clipping is undone below with the rest of the conversion
        for px in &mut self.srgb {
            for v in px {
                *v = v.clamp(0.0, 1.0);
            }
        }
        deinterleave(&self.srgb, &mut self.planar);
        self.model.run(&self.planar, planar_out)?;
        // correction = network output − its input (enlarged classically for
        // an upscaler), both taken back to the photo's space with the same
        // conversion, so that its rounding cancels
        let n_out = n * scale * scale;
        if scale > 1 {
            let up = |px: &[[f32; 3]]| enlarge(px.as_flattened(), side, side, 3, scale).as_chunks::<3>().0.to_vec();
            self.srgb = up(&self.srgb);
            self.source = up(&self.source);
        }
        interleave(planar_out, &mut self.back);
        let network = std::mem::take(&mut self.back);
        self.back.resize(n_out, [0.0; 3]);
        self.space.from_srgb.transform_pixels(&network, &mut self.back);
        self.base.resize(n_out, [0.0; 3]);
        self.space.from_srgb.transform_pixels(&self.srgb, &mut self.base);
        for (i, ((src, out), base)) in self.source.iter().zip(&self.back).zip(&self.base).enumerate() {
            for c in 0..3 {
                planar_out[c * n_out + i] = src[c] + out[c] - base[c];
            }
        }
        Ok(())
    }
}

/// Runs `f` with the network as it must see a photo with the profile `icc`:
/// through [`InSrgb`] when the photo is not sRGB.
pub fn with_network_space<R>(
    model: &mut dyn TileModel,
    icc: Option<&[u8]>,
    f: impl FnOnce(&mut dyn TileModel) -> R,
) -> R {
    match NetworkSpace::for_profile(icc) {
        Some(space) => f(&mut InSrgb::new(model, &space)),
        None => f(model),
    }
}

/// Planar (all red, then green, then blue) to one RGB value per pixel.
fn interleave(planar: &[f32], out: &mut Vec<[f32; 3]>) {
    let n = planar.len() / 3;
    out.clear();
    out.extend((0..n).map(|i| [planar[i], planar[n + i], planar[2 * n + i]]));
}

fn deinterleave(pixels: &[[f32; 3]], out: &mut Vec<f32>) {
    let n = pixels.len();
    out.clear();
    out.resize(3 * n, 0.0);
    for (i, px) in pixels.iter().enumerate() {
        for c in 0..3 {
            out[c * n + i] = px[c];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcms2::{CIExyY, CIExyYTRIPLE, ToneCurve};

    fn adobe_rgb() -> Vec<u8> {
        let white = CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 };
        let primaries = CIExyYTRIPLE {
            Red: CIExyY { x: 0.64, y: 0.33, Y: 1.0 },
            Green: CIExyY { x: 0.21, y: 0.71, Y: 1.0 },
            Blue: CIExyY { x: 0.15, y: 0.06, Y: 1.0 },
        };
        let gamma = ToneCurve::new(2.2);
        Profile::new_rgb(&white, &primaries, &[&gamma, &gamma, &gamma]).unwrap().icc().unwrap()
    }

    /// Returns its input, or its input plus `add` (in sRGB values).
    struct Fake {
        add: f32,
        seen: Vec<f32>,
    }

    impl TileModel for Fake {
        fn input_px(&self) -> usize {
            2
        }
        fn margin_px(&self) -> usize {
            0
        }
        fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
            self.seen = planar_in.to_vec();
            for (o, i) in planar_out.iter_mut().zip(planar_in) {
                *o = i + self.add;
            }
            Ok(())
        }
    }

    /// Four pixels, planar: grey, a saturated green outside sRGB, black, white.
    const TILE: [f32; 12] = [0.5, 0.0, 0.0, 1.0, 0.5, 0.9, 0.0, 1.0, 0.5, 0.0, 0.0, 1.0];

    #[test]
    fn srgb_and_missing_profiles_go_straight_to_the_network() {
        assert!(NetworkSpace::for_profile(None).is_none());
        assert!(NetworkSpace::for_profile(Some(&Profile::new_srgb().icc().unwrap())).is_none());
        assert!(NetworkSpace::for_profile(Some(b"not a profile")).is_none());
        let gray = Profile::new_gray(&CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 }, &ToneCurve::new(2.2)).unwrap();
        assert!(NetworkSpace::for_profile(Some(&gray.icc().unwrap())).is_none());
        assert!(NetworkSpace::for_profile(Some(&adobe_rgb())).is_some());
    }

    #[test]
    fn no_correction_leaves_the_photo_unchanged() {
        let space = NetworkSpace::for_profile(Some(&adobe_rgb())).unwrap();
        let mut fake = Fake { add: 0.0, seen: Vec::new() };
        let mut out = [0.0; 12];
        InSrgb::new(&mut fake, &space).run(&TILE, &mut out).unwrap();
        for (o, i) in out.iter().zip(&TILE) {
            assert!((o - i).abs() < 1e-4, "{out:?}");
        }
        // the green outside sRGB reached the network clipped, but comes back
        assert!(fake.seen.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!((out[5] - 0.9).abs() < 1e-4);
    }

    #[test]
    fn the_network_sees_srgb_values() {
        let space = NetworkSpace::for_profile(Some(&adobe_rgb())).unwrap();
        let mut fake = Fake { add: 0.0, seen: Vec::new() };
        let mut out = [0.0; 12];
        InSrgb::new(&mut fake, &space).run(&TILE, &mut out).unwrap();
        // a neutral grey stays grey, with the sRGB tone curve instead of 2.2
        let grey = [fake.seen[0], fake.seen[4], fake.seen[8]];
        assert!(grey.iter().all(|v| (v - grey[0]).abs() < 1e-3), "{grey:?}");
        assert!((grey[0] - 0.5).abs() > 0.001 && (grey[0] - 0.5).abs() < 0.05, "{grey:?}");
    }

    /// An "upscaler" that enlarges with Catmull-Rom: it adds no detail.
    struct Classical;

    impl TileModel for Classical {
        fn input_px(&self) -> usize {
            2
        }
        fn margin_px(&self) -> usize {
            0
        }
        fn scale(&self) -> usize {
            2
        }
        fn run(&mut self, planar_in: &[f32], planar_out: &mut [f32]) -> Result<(), ModelError> {
            let mut px = Vec::new();
            interleave(planar_in, &mut px);
            let big = enlarge(px.as_flattened(), 2, 2, 3, 2);
            let mut planar = Vec::new();
            deinterleave(big.as_chunks::<3>().0, &mut planar);
            planar_out.copy_from_slice(&planar);
            Ok(())
        }
    }

    #[test]
    fn an_enlargement_without_detail_is_the_classical_one_of_the_photo() {
        let space = NetworkSpace::for_profile(Some(&adobe_rgb())).unwrap();
        let mut out = [0.0; 48];
        InSrgb::new(&mut Classical, &space).run(&TILE, &mut out).unwrap();
        let mut px = Vec::new();
        interleave(&TILE, &mut px);
        let mut expected = Vec::new();
        deinterleave(enlarge(px.as_flattened(), 2, 2, 3, 2).as_chunks::<3>().0, &mut expected);
        for (o, e) in out.iter().zip(&expected) {
            assert!((o - e).abs() < 1e-4, "{out:?} vs {expected:?}");
        }
    }

    #[test]
    fn a_correction_is_applied_in_the_photo_space() {
        let space = NetworkSpace::for_profile(Some(&adobe_rgb())).unwrap();
        let mut fake = Fake { add: 0.02, seen: Vec::new() };
        let mut out = [0.0; 12];
        InSrgb::new(&mut fake, &space).run(&TILE, &mut out).unwrap();
        // the grey gets lighter, and stays grey
        assert!(out[0] > 0.5 && out[0] < 0.55, "{out:?}");
        assert!((out[0] - out[4]).abs() < 1e-3 && (out[0] - out[8]).abs() < 1e-3);
    }
}
