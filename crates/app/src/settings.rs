//! Settings remembered between sessions.

use std::path::{Path, PathBuf};

use denoise_core::{product, Params};
use denoise_engine::{Device, EngineOptions, TileProfile, MODEL_FILE};
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
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { options: ExportOptions::default(), suffix: "_dn".into(), destination: Destination::SameAsSource }
    }
}

impl ExportSettings {
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
}

impl Settings {
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

    /// The denoising model files in use: [`Self::model_set`] and the
    /// half-precision variants next to them.
    pub fn denoiser_files(&self) -> Vec<PathBuf> {
        let set = self.model_set();
        let half: Vec<PathBuf> = set.iter().map(|p| denoise_engine::fp16_variant(p)).filter(|p| p.is_file()).collect();
        set.into_iter().chain(half).collect()
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
    fn destination_folder() {
        let mut e = ExportSettings::default();
        assert_eq!(e.dir_for(Path::new("/a/b/photo.tif")), Path::new("/a/b"));
        e.destination = Destination::Folder("/out".into());
        assert_eq!(e.dir_for(Path::new("/a/b/photo.tif")), Path::new("/out"));
    }
}
