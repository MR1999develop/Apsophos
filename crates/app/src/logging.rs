//! Log file: what the program did (model loading, devices, files, errors),
//! to understand a problem after the fact.
//!
//! One file per day in the local data folder
//! (`%LOCALAPPDATA%\Apsophos\logs` on Windows,
//! `~/.local/share/Apsophos/logs` on Linux); only the most recent ones are
//! kept. ONNX Runtime's own messages arrive here too, through `tracing`.

use std::path::PathBuf;

use denoise_core::product;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Days of logs kept.
const KEPT_FILES: usize = 7;

/// Folder of the log files.
pub fn folder() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(product::DATA_DIR).join("logs"))
}

/// What is written: the program's messages from `info` up, ONNX Runtime's
/// from `warn` up (its `info` level describes every graph optimisation).
fn filter() -> Targets {
    Targets::new().with_default(LevelFilter::INFO).with_target("ort", LevelFilter::WARN)
}

/// Starts logging to the file; keep the returned guard alive until the
/// program ends, so the last lines are written. `None` (and no logging) if
/// the folder cannot be created.
pub fn init() -> Option<WorkerGuard> {
    let dir = folder()?;
    std::fs::create_dir_all(&dir).ok()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(product::ID)
        .filename_suffix("log")
        .max_log_files(KEPT_FILES)
        .build(&dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let layer = tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false).with_target(true);
    tracing_subscriber::registry().with(layer).with(filter()).try_init().ok()?;
    // a crash leaves its message in the log too
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        previous(info);
    }));
    tracing::info!("{} {} started", product::NAME, env!("CARGO_PKG_VERSION"));
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn onnx_runtime_chatter_is_filtered() {
        let f = filter();
        assert!(f.would_enable("apsophos::worker", &Level::INFO));
        assert!(!f.would_enable("apsophos::worker", &Level::DEBUG));
        assert!(!f.would_enable("ort::session", &Level::INFO));
        assert!(f.would_enable("ort::session", &Level::WARN));
    }

    #[test]
    fn folder_is_under_local_data() {
        if let Some(dir) = folder() {
            assert!(dir.ends_with(std::path::Path::new(product::DATA_DIR).join("logs")));
        }
    }
}
