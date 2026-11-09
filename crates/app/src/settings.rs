//! Settings remembered between sessions.

use std::path::{Path, PathBuf};

use denoise_core::upscale::{Detail, Upscale};
use denoise_core::{product, Params};
use denoise_engine::{Device, EngineOptions, TileProfile, UpscaleSpeed, MODEL_FILE};
use denoise_io::ExportOptions;
use serde::{Deserialize, Serialize};

use crate::text::Language;

pub const STORAGE_KEY: &str = product::ID;
const MAX_RECENT: usize = 10;
/// Environment variable with the model path (for installs without a
/// `models` folder).
pub const MODEL_ENV: &str = product::MODEL_ENV;

/// Where exported files go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Destination {
    /// Next to the original.
    #[default]
    SameAsSource,
    Folder(PathBuf),
}

/// Export settings (also used by the queue).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub options: ExportOptions,
    /// Appended to the name: `photo` → `photo_dn`.
    pub suffix: String,
    pub destination: Destination,
    /// Enlargement by a network, the last step before the file; `None`: the
    /// photo keeps its size.
    pub upscale: Option<Upscale>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            options: ExportOptions::default(),
            suffix: "_dn".into(),
            destination: Destination::SameAsSource,
            upscale: None,
        }
    }
}

impl ExportSettings {
    /// The suffix with what the enlargement adds: `_dn_2x`, `_dn_4x-gen`
    /// (generated detail is said in the name).
    pub fn full_suffix(&self) -> String {
        let Some(u) = self.upscale else { return self.suffix.clone() };
        let detail = match u.detail {
            Detail::Faithful => "",
            Detail::Generated => "-gen",
        };
        format!("{}_{}x{detail}", self.suffix, u.factor.value())
    }

    /// Destination folder for `source`.
    pub fn dir_for(&self, source: &Path) -> PathBuf {
        match &self.destination {
            Destination::Folder(d) => d.clone(),
            Destination::SameAsSource => source.parent().map(Path::to_path_buf).unwrap_or_default(),
        }
    }
}

/// Tile speeds measured by the GPU search, remembered for the same device
/// setting and the same model files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TileHint {
    pub device: Device,
    pub models: Vec<PathBuf>,
    /// `None` in settings saved by older versions (which kept only a size):
    /// the search then runs once more.
    pub profile: Option<TileProfile>,
}

/// The speed of the last enlargement made with one choice (2× or 4×,
/// faithful or generated) and one device setting.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpscaleTiming {
    pub upscale: Upscale,
    pub device: Device,
    pub speed: UpscaleSpeed,
}

/// The measured speed for `upscale` on `device`, if an enlargement measured it.
pub fn upscale_speed(timings: &[UpscaleTiming], upscale: Upscale, device: Device) -> Option<UpscaleSpeed> {
    timings.iter().find(|t| t.upscale == upscale && t.device == device).map(|t| t.speed)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub tile_hint: Option<TileHint>,
    pub params: Params,
    pub engine: EngineOptions,
    pub export: ExportSettings,
    pub recent: Vec<PathBuf>,
    pub last_open_dir: Option<PathBuf>,
    /// Model chosen by hand by the user.
    pub model_path: Option<PathBuf>,
    pub reduce_motion: bool,
    /// Single-key shortcuts (F, 1, N, +, −, \) turned off: they get in the
    /// way of speech input and are easy to press by accident.
    pub single_keys_off: bool,
    pub show_queue: bool,
    /// Interface language; `None` follows the system.
    pub language: Option<Language>,
    /// The user asked not to be offered the upscaling models at start.
    pub no_upscaler_offer: bool,
    /// Enlargement speeds measured on this computer, at most one for each
    /// choice and device setting: they give the time estimate in the export
    /// settings.
    pub upscale_timings: Vec<UpscaleTiming>,
}

impl Settings {
    /// Remembers the speed of an enlargement just made, in place of the
    /// earlier one for the same choice and device setting.
    pub fn remember_upscale_speed(&mut self, upscale: Upscale, device: Device, speed: UpscaleSpeed) {
        self.upscale_timings.retain(|t| !(t.upscale == upscale && t.device == device));
        self.upscale_timings.push(UpscaleTiming { upscale, device, speed });
    }

    pub fn push_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
        self.last_open_dir = path.parent().map(Path::to_path_buf);
    }

    /// Paths where the model is looked up, in order of preference.
    pub fn model_candidates(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        out.extend(self.model_path.clone());
        if let Some(p) = std::env::var_os(MODEL_ENV) {
            out.push(PathBuf::from(p));
        }
        // `models` folder next to the executable or in a parent folder
        // (during development the executable lives in target/release)
        if let Ok(exe) = std::env::current_exe() {
            for dir in exe.ancestors().skip(1).take(4) {
                out.push(dir.join("models").join(MODEL_FILE));
            }
        }
        if let Some(data) = dirs::data_dir() {
            out.push(data.join(product::DATA_DIR).join("models").join(MODEL_FILE));
        }
        out
    }

    /// First existing model.
    pub fn find_model(&self) -> Option<PathBuf> {
        self.model_candidates().into_iter().find(|p| p.is_file())
    }

    /// The upscaling model files: in the denoiser's model folder, then in
    /// the folder the downloads go to.
    pub fn upscaler_files(&self) -> Vec<PathBuf> {
        let dirs = [self.find_model().and_then(|p| p.parent().map(Path::to_path_buf)), crate::download::download_dir()];
        let mut found: Vec<PathBuf> = Vec::new();
        for dir in dirs.into_iter().flatten() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for path in rd.filter_map(Result::ok).map(|e| e.path()) {
                let known = found.iter().any(|f| f.file_name() == path.file_name());
                if !known && denoise_engine::upscaler_file_kind(&path).is_some() {
                    found.push(path);
                }
            }
        }
        found
    }

    /// Models (same network, different tile sizes) to offer the engine: a
    /// model chosen by hand is used alone, otherwise every
    /// `scunet_color_real_<px>.onnx` next to the default model.
    pub fn model_set(&self) -> Vec<PathBuf> {
        let Some(base) = self.find_model() else { return Vec::new() };
        if self.model_path.is_some() {
            return vec![base];
        }
        let dir = base.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| denoise_engine::is_model_file(p)).collect()
            })
            .unwrap_or_default();
        if !found.contains(&base) {
            found.push(base);
        }
        found.sort();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_unique_and_bounded() {
        let mut s = Settings::default();
        for i in 0..15 {
            s.push_recent(Path::new(&format!("/photos/{i}.tif")));
        }
        s.push_recent(Path::new("/photos/3.tif"));
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent[0], Path::new("/photos/3.tif"));
        assert_eq!(s.recent.iter().filter(|p| p.ends_with("3.tif")).count(), 1);
    }

    #[test]
    fn each_choice_keeps_its_last_speed() {
        use denoise_core::upscale::Factor;
        let x2 = Upscale { factor: Factor::X2, detail: Detail::Faithful };
        let x4 = Upscale { factor: Factor::X4, detail: Detail::Faithful };
        let speed = |secs_per_tile| UpscaleSpeed { tile_px: 640, secs_per_tile };
        let mut s = Settings::default();
        assert_eq!(upscale_speed(&s.upscale_timings, x2, Device::Auto), None);
        s.remember_upscale_speed(x2, Device::Auto, speed(5.0));
        s.remember_upscale_speed(x4, Device::Auto, speed(9.0));
        s.remember_upscale_speed(x2, Device::Cpu, speed(80.0));
        // a new measure replaces the old one
        s.remember_upscale_speed(x2, Device::Auto, speed(4.0));
        assert_eq!(s.upscale_timings.len(), 3);
        assert_eq!(upscale_speed(&s.upscale_timings, x2, Device::Auto), Some(speed(4.0)));
        assert_eq!(upscale_speed(&s.upscale_timings, x2, Device::Cpu), Some(speed(80.0)));
        assert_eq!(upscale_speed(&s.upscale_timings, x4, Device::Cpu), None);
    }

    #[test]
    fn destination_folder() {
        let mut e = ExportSettings::default();
        assert_eq!(e.dir_for(Path::new("/a/b/photo.tif")), Path::new("/a/b"));
        e.destination = Destination::Folder("/out".into());
        assert_eq!(e.dir_for(Path::new("/a/b/photo.tif")), Path::new("/out"));
    }
}
