//! The program's name and the names derived from it, in one place: window
//! title, data folders, settings key, log files, environment variable.
//!
//! Apsophos, from the ancient Greek ἄψοφος, "noiseless".

/// Name shown to the user.
pub const NAME: &str = "Apsophos";
/// Lower-case identifier: executable, settings key, log file prefix, the
/// application id the desktop uses to group its windows.
pub const ID: &str = "apsophos";
/// Folder under the user's data folders (`%APPDATA%`, `%LOCALAPPDATA%`,
/// `~/.local/share`) for models, logs and GPU caches.
pub const DATA_DIR: &str = "Apsophos";
/// Environment variable with the path of the model file.
pub const MODEL_ENV: &str = "APSOPHOS_MODEL";
