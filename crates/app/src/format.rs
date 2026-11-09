//! Numbers, durations, file names and the status lines built from them, in
//! the words of the text catalog ([`crate::text`]). Pure functions, so they
//! can be tested without a window.

use std::path::Path;
use std::time::Duration;

use crate::text::tr;

/// Integer with thousands separators: `1,024`.
pub fn int(n: usize) -> String {
    let digits = n.to_string();
    let sep = tr().numbers.thousands;
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// Decimal number: `0.5`.
pub fn dec(v: f32, decimals: usize) -> String {
    let text = format!("{v:.decimals$}");
    match tr().numbers.decimal {
        '.' => text,
        sep => text.replace('.', &sep.to_string()),
    }
}

/// Readable duration: `45 s`, `1 min 20 s`, `1 h 5 min`.
pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    (tr().numbers.duration)(s / 3600, (s % 3600) / 60, s % 60)
}

/// A size in bytes: `850 MB`, `2.3 GB`.
pub fn bytes(n: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "shown with one decimal")]
    let gb = n as f64 / 1e9;
    if gb >= 1.0 {
        format!("{} GB", dec(denoise_core::num::narrow(gb), 1))
    } else {
        format!("{} MB", int(usize::try_from(n / 1_000_000).unwrap_or(usize::MAX)))
    }
}

/// Megapixels: `24.2 MP`.
pub fn megapixels(w: usize, h: usize) -> String {
    let mp = (w * h) as f32 / 1e6;
    (tr().numbers.megapixels)(&dec(mp, 1))
}

/// Estimated time left after `done` of `total` steps in `elapsed`: `about
/// 1 min 20 s left`; `None` before the first step or after the last.
pub fn eta(elapsed: Duration, done: usize, total: usize) -> Option<String> {
    (done > 0 && total > done).then(|| {
        let per = elapsed / u32::try_from(done).unwrap_or(u32::MAX);
        let left = per.saturating_mul(u32::try_from(total - done).unwrap_or(u32::MAX));
        (tr().numbers.time_left)(&duration(left))
    })
}

/// The time an enlargement should take, from `secs` estimated with the
/// speed measured on this computer: `about 2 min 30 s on this computer`, or
/// that no enlargement has measured it yet.
pub fn upscale_estimate(secs: Option<f64>) -> String {
    let te = &tr().export;
    // at least a second: "about 0 s" would read as "instant"
    match secs.filter(|s| s.is_finite()).and_then(|s| Duration::try_from_secs_f64(s.max(1.0)).ok()) {
        Some(d) => (te.upscale_time)(&duration(d)),
        None => te.upscale_time_unknown.to_owned(),
    }
}

/// Progress 0..=1 (0 for an empty job).
pub fn fraction(done: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32).min(1.0)
    }
}

/// Progress line of the open image: `Tile 3 of 12 · about 40 s left`.
pub fn tile_progress(elapsed: Duration, done: usize, total: usize) -> String {
    let mut line = (tr().params.tile_of)(&int(done), &int(total));
    if let Some(e) = eta(elapsed, done, total) {
        line.push_str(" · ");
        line.push_str(&e);
    }
    line
}

/// Why denoising the open image cannot start, worded for the user; `None`
/// if it can. `open_keys` is the shortcut that opens an image.
pub fn denoise_blocker(queue_running: bool, has_image: bool, open_keys: &str) -> Option<String> {
    if queue_running {
        Some(tr().notices.queue_busy.to_owned())
    } else if !has_image {
        Some((tr().notices.open_first)(open_keys))
    } else {
        None
    }
}

/// Status of a queue item being processed: `Processing · 3/12`, or
/// `Processing…` before the number of tiles is known.
pub fn processing(done: usize, total: usize) -> String {
    if total > 0 {
        (tr().queue.processing)(done, total)
    } else {
        tr().queue.processing_start.to_owned()
    }
}

/// File name (without folder).
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Shortens `text` by removing its middle: `Paris_2025…MG_9855.tif`.
pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    let n = text.chars().count();
    if n <= max_chars || max_chars < 5 {
        return text.to_owned();
    }
    let keep = max_chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(int(0), "0");
        assert_eq!(int(1024), "1,024");
        assert_eq!(int(1_234_567), "1,234,567");
        assert_eq!(dec(0.5, 1), "0.5");
        assert_eq!(megapixels(6000, 4000), "24.0 MP");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(Duration::from_secs(45)), "45 s");
        assert_eq!(duration(Duration::from_secs(80)), "1 min 20 s");
        assert_eq!(duration(Duration::from_secs(3900)), "1 h 5 min");
    }

    #[test]
    fn progress_texts() {
        assert_eq!(eta(Duration::from_secs(10), 0, 4), None);
        assert_eq!(eta(Duration::from_secs(10), 4, 4), None);
        assert_eq!(eta(Duration::from_secs(20), 2, 6).as_deref(), Some("about 40 s left"));
        assert_eq!(tile_progress(Duration::from_secs(20), 2, 6), "Tile 2 of 6 · about 40 s left");
        assert_eq!(tile_progress(Duration::ZERO, 0, 1200), "Tile 0 of 1,200");
        assert!((fraction(1, 4) - 0.25).abs() < 1e-6);
        assert!(fraction(3, 0).abs() < 1e-6 && (fraction(5, 4) - 1.0).abs() < 1e-6);
        assert_eq!(processing(3, 12), "Processing · 3/12");
        assert_eq!(processing(0, 0), "Processing…");
    }

    #[test]
    fn enlargement_estimates() {
        assert_eq!(upscale_estimate(Some(150.4)), "Enlargement time: about 2 min 30 s on this computer");
        assert_eq!(upscale_estimate(Some(0.2)), "Enlargement time: about 1 s on this computer");
        let unknown = "Enlargement time: not measured yet (the first enlargement measures it)";
        assert_eq!(upscale_estimate(None), unknown);
        // values that are no time at all
        assert_eq!(upscale_estimate(Some(f64::NAN)), unknown);
        assert_eq!(upscale_estimate(Some(f64::INFINITY)), unknown);
    }

    #[test]
    fn denoise_blockers() {
        assert_eq!(denoise_blocker(false, true, "Ctrl+O"), None);
        assert_eq!(denoise_blocker(false, false, "Ctrl+O").as_deref(), Some("Open an image first (Ctrl+O)."));
        // the queue is the stronger reason
        assert!(denoise_blocker(true, false, "Ctrl+O").is_some_and(|m| m.contains("queue")));
    }

    #[test]
    fn middle_truncation() {
        assert_eq!(truncate_middle("short.tif", 20), "short.tif");
        let t = truncate_middle("Parigi_2025-12-22 11.09.01_MG_9855.tif", 21);
        assert_eq!(t.chars().count(), 21);
        assert!(t.starts_with("Parigi_2") && t.ends_with("9855.tif"));
    }
}
