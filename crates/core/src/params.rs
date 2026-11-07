use serde::{Deserialize, Serialize};

use crate::DenoiseError;

/// Percentage validated in the range 0..=100.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct Percent(f32);

impl Percent {
    pub const ZERO: Percent = Percent(0.0);
    pub const FULL: Percent = Percent(100.0);

    /// Brings `value` into range; `NaN` becomes 0.
    pub fn clamped(value: f32) -> Self {
        if value.is_nan() {
            Self::ZERO
        } else {
            Self(value.clamp(0.0, 100.0))
        }
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// Fraction 0..=1.
    pub fn fraction(self) -> f32 {
        self.0 / 100.0
    }
}

impl TryFrom<f32> for Percent {
    type Error = DenoiseError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if (0.0..=100.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DenoiseError::OutOfRange(value))
        }
    }
}

impl From<Percent> for f32 {
    fn from(p: Percent) -> f32 {
        p.0
    }
}

/// Parameters chosen by the user.
///
/// The network works out by itself how much noise to remove; these values
/// decide how much of its correction to apply: `luma` for grain (luminance),
/// `chroma` for colour blotches (chrominance). 100 % = the network's full
/// correction, 0 % = original.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    pub luma: Percent,
    pub chroma: Percent,
}

impl Default for Params {
    fn default() -> Self {
        Preset::RECOMMENDED.params()
    }
}

/// Presets offered in the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Light,
    Medium,
    Strong,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Light, Preset::Medium, Preset::Strong];
    pub const RECOMMENDED: Preset = Preset::Medium;

    pub fn params(self) -> Params {
        let (luma, chroma) = match self {
            Preset::Light => (50.0, 70.0),
            Preset::Medium => (80.0, 100.0),
            Preset::Strong => (100.0, 100.0),
        };
        Params { luma: Percent::clamped(luma), chroma: Percent::clamped(chroma) }
    }

    /// The preset that exactly matches `params`, if any.
    pub fn matching(params: &Params) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.params() == *params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rejects_out_of_range() {
        assert!(Percent::try_from(101.0).is_err());
        assert!(Percent::try_from(-0.5).is_err());
        assert_eq!(Percent::clamped(250.0), Percent::FULL);
        assert_eq!(Percent::clamped(f32::NAN), Percent::ZERO);
    }

    #[test]
    fn default_is_recommended_preset() {
        assert_eq!(Preset::matching(&Params::default()), Some(Preset::RECOMMENDED));
    }
}
