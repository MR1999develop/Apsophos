//! Every text the interface shows, in one catalog.
//!
//! A language is one value of [`Texts`]: fixed texts are `&'static str`,
//! texts with numbers or names are functions, so that each language places
//! them and chooses its plural forms itself. Adding a language means writing
//! another value: the compiler then asks for every text. [`tr`] returns the
//! one in use, chosen with [`set_language`]: English ([`EN`]) or Italian
//! ([`IT`]).
//!
//! The libraries' errors are worded here too ([`Errors`], used by
//! [`crate::errors`]). Not here: format and device names (TIFF, DirectML…);
//! notes and tile-search details that the libraries write in English; the
//! self-test report, the log and the details of internal errors (a panic, a
//! thread that cannot start), which are for diagnosis.

use std::sync::atomic::{AtomicU8, Ordering};

use denoise_io::BitDepth;
use serde::{Deserialize, Serialize};

/// A language with a catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Language {
    English = 0,
    Italian = 1,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Italian];

    /// The language's own name, as a language list shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Italian => "Italiano",
        }
    }

    /// The system's language when it has a catalog, otherwise English. Tests
    /// always get English, so they do not depend on the computer.
    pub fn system() -> Self {
        if cfg!(test) {
            return Self::English;
        }
        match sys_locale::get_locale() {
            Some(tag) if tag.to_ascii_lowercase().starts_with("it") => Self::Italian,
            _ => Self::English,
        }
    }
}

/// The language in use, as `Language as u8`.
static CURRENT: AtomicU8 = AtomicU8::new(Language::English as u8);

/// Chooses the language: `None` follows the system.
pub fn set_language(choice: Option<Language>) {
    CURRENT.store(choice.unwrap_or_else(Language::system) as u8, Ordering::Relaxed);
}

/// The catalog in use.
pub fn tr() -> &'static Texts {
    if CURRENT.load(Ordering::Relaxed) == Language::Italian as u8 {
        &IT
    } else {
        &EN
    }
}

/// All the texts of one language.
pub struct Texts {
    pub numbers: Numbers,
    pub menu: Menu,
    pub toolbar: Toolbar,
    pub params: ParamsTexts,
    pub info: Info,
    pub queue: Queue,
    pub status: Status,
    pub central: Central,
    pub viewer: Viewer,
    pub preferences: Preferences,
    pub export: Export,
    pub shortcuts: Shortcuts,
    pub about: About,
    pub dialogs: Dialogs,
    pub notices: Notices,
    pub history: HistoryTexts,
    pub errors: Errors,
}

/// Numbers, durations and counts.
pub struct Numbers {
    /// Between groups of three digits: `1,024`.
    pub thousands: char,
    /// Before the decimals: `0.5`.
    pub decimal: char,
    /// `45 s`, `1 min 20 s`, `1 h 5 min`, from hours, minutes and seconds.
    pub duration: fn(u64, u64, u64) -> String,
    /// `24.2 MP`, from the number already formatted.
    pub megapixels: fn(&str) -> String,
    /// `about 1 min 20 s left`, from the duration already formatted.
    pub time_left: fn(&str) -> String,
    /// `1 file`, `1,200 files`.
    pub files: fn(usize) -> String,
}

pub struct Menu {
    pub file: &'static str,
    pub open: &'static str,
    pub open_recent: &'static str,
    pub no_recent: &'static str,
    pub clear_list: &'static str,
    pub add_files: &'static str,
    pub add_folder: &'static str,
    pub export: &'static str,
    pub export_as: &'static str,
    pub export_settings: &'static str,
    pub close_image: &'static str,
    pub quit: &'static str,
    pub edit: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    /// `Undo grain 30 %`, from the label of the action.
    pub undo_action: fn(&str) -> String,
    pub redo_action: fn(&str) -> String,
    pub reset: &'static str,
    pub preferences: &'static str,
    pub view: &'static str,
    pub fit: &'static str,
    pub actual_size: &'static str,
    pub show_removed_noise: &'static str,
    pub show_result: &'static str,
    pub show_queue: &'static str,
    pub process: &'static str,
    pub denoise: &'static str,
    pub stop: &'static str,
    pub process_queue: &'static str,
    pub pause_queue: &'static str,
    pub resume_queue: &'static str,
    pub help: &'static str,
    pub keyboard_shortcuts: &'static str,
    pub open_log_folder: &'static str,
    /// `About Apsophos`, from the product name.
    pub about: fn(&str) -> String,
    /// The keyboard focus to the next or previous pane (F6, Shift+F6).
    pub next_pane: &'static str,
    pub previous_pane: &'static str,
}

/// Why a command cannot be used now (shown when hovering over it).
pub struct Why {
    pub no_image: &'static str,
    pub open_image_to_export: &'static str,
    pub open_image: &'static str,
    pub already_processing: &'static str,
    pub queue_running: &'static str,
    pub nothing_running: &'static str,
    pub nothing_to_undo: &'static str,
    pub nothing_to_redo: &'static str,
    pub already_recommended: &'static str,
    pub already_exporting: &'static str,
    pub no_gpu_build: &'static str,
    pub queue_not_running: &'static str,
    pub no_files_to_process: &'static str,
    /// Denoise with the noise reduction switched off.
    pub noise_off: &'static str,
    /// Export or the queue with neither noise reduction nor enlargement.
    pub nothing_to_do: &'static str,
    pub wait_for_denoising: &'static str,
    pub wait_for_queue: &'static str,
    pub wait_for_processing: &'static str,
    pub no_rtx: &'static str,
    pub no_directml: &'static str,
    pub threads_cpu_only: &'static str,
}

pub struct Toolbar {
    pub open: &'static str,
    /// Tooltips, from the keys of the shortcut.
    pub open_tip: fn(&str) -> String,
    pub export: &'static str,
    pub export_tip: fn(&str) -> String,
    pub fit: &'static str,
    pub fit_tip: fn(&str) -> String,
    pub actual: &'static str,
    pub actual_tip: fn(&str) -> String,
    pub queue: &'static str,
    pub queue_tip: &'static str,
    pub why: Why,
}

pub struct ParamsTexts {
    pub title: &'static str,
    pub intro: &'static str,
    /// The switch of the noise reduction, independent of the enlargement.
    pub reduce_noise: &'static str,
    pub reduce_noise_tip: &'static str,
    pub preset: &'static str,
    pub light: &'static str,
    pub light_tip: &'static str,
    pub medium: &'static str,
    /// Marks the recommended preset, after its name.
    pub recommended_mark: &'static str,
    pub medium_tip: &'static str,
    pub strong: &'static str,
    pub strong_tip: &'static str,
    pub grain: &'static str,
    pub grain_tip: &'static str,
    pub colour: &'static str,
    pub colour_tip: &'static str,
    pub stop: &'static str,
    /// From the keys of the shortcut.
    pub stop_image_tip: fn(&str) -> String,
    pub open_image: &'static str,
    pub export: &'static str,
    pub export_tip: fn(&str) -> String,
    pub export_as: &'static str,
    pub export_settings: &'static str,
    pub sliders_live: &'static str,
    /// The enlargement in the right panel, a shortcut to the export setting.
    pub enlarge: &'static str,
    pub enlarge_none: &'static str,
    pub enlarge_x2: &'static str,
    pub enlarge_x4: &'static str,
    pub enlarge_tip: &'static str,
    /// Under the choice: when it acts.
    pub enlarge_when: &'static str,
    /// A disabled factor: no model on disk.
    pub enlarge_missing: &'static str,
    /// Opens the export settings at the enlargement.
    pub enlarge_options: &'static str,
    pub denoise: &'static str,
    pub finish_denoising: &'static str,
    pub denoise_tip: fn(&str) -> String,
    pub exporting: &'static str,
    /// `Export 2×`, from the factor.
    pub export_enlarged: fn(&str) -> String,
    pub enlarging: &'static str,
    pub cancel_export: &'static str,
    /// `Tile 3 of 12`, from the numbers already formatted.
    pub tile_of: fn(&str, &str) -> String,
    pub advanced: &'static str,
    pub device: &'static str,
    pub auto: &'static str,
    pub auto_tip: &'static str,
    pub rtx: &'static str,
    pub rtx_tip: &'static str,
    pub directml: &'static str,
    pub directml_tip: &'static str,
    pub cpu: &'static str,
    pub threads: &'static str,
    pub threads_auto: &'static str,
    /// `Use all 12 cores`.
    pub threads_auto_tip: fn(usize) -> String,
    pub next_run: &'static str,
    pub redetect: &'static str,
    pub redetect_tip: &'static str,
    /// `In use: GPU (DirectML), 960 px tiles`.
    pub in_use: fn(&str) -> String,
    /// `Model not loaded: …`, from the error.
    pub model_not_loaded: fn(&str) -> String,
    /// What the status shows for the loaded model: device and tile size.
    pub model_ready: fn(&str, usize) -> String,
    pub histogram: &'static str,
    pub histogram_tip: &'static str,
    pub histogram_legend: &'static str,
    /// What the histogram shows, for screen readers: median brightness of
    /// the original and of the result, already formatted.
    pub histogram_a11y: fn(&str, &str) -> String,
}

/// The image information in the right panel.
pub struct Info {
    pub title: &'static str,
    pub no_image: &'static str,
    pub file: &'static str,
    pub format: &'static str,
    pub grey: &'static str,
    pub rgb: &'static str,
    pub with_alpha: &'static str,
    pub size: &'static str,
    pub profile: &'static str,
    pub profile_unnamed: &'static str,
    pub profile_none: &'static str,
    pub display: &'static str,
    pub display_converted: &'static str,
    pub display_as_is: &'static str,
    pub display_unusable_profile: &'static str,
    pub display_no_conversion: &'static str,
    pub exif: &'static str,
    pub xmp: &'static str,
    pub resolution: &'static str,
    /// `300 dpi` or `300 × 240 dpi`, from the values already formatted.
    pub dpi: fn(&str, Option<&str>) -> String,
    pub kept: &'static str,
    pub none: &'static str,
    /// `8 bit`, `16 bit`, `32-bit float`.
    pub bit_depth: fn(BitDepth) -> &'static str,
    /// `Partially denoised: 3 of 12 tiles.`, numbers already formatted.
    pub partial: fn(&str, &str) -> String,
    /// A heavily compressed JPEG: what, why it matters, what to do; from
    /// the estimated quality (1 to 100).
    pub heavy_jpeg: fn(u8) -> String,
}

pub struct Queue {
    pub title: &'static str,
    pub adding: &'static str,
    pub add_files: &'static str,
    pub add_folder: &'static str,
    pub add_folder_tip: &'static str,
    pub empty: &'static str,
    pub queued: &'static str,
    /// `Processing · 3/12`.
    pub processing: fn(usize, usize) -> String,
    pub processing_start: &'static str,
    /// `Done → photo_dn.tif`.
    pub done: fn(&str) -> String,
    pub failed: &'static str,
    pub open_in_viewer: &'static str,
    pub show_result: &'static str,
    pub remove: &'static str,
    pub destination: &'static str,
    pub next_to_originals: &'static str,
    pub change: &'static str,
    /// `3 of 12`.
    pub progress: fn(usize, usize) -> String,
    pub pause: &'static str,
    pub resume: &'static str,
    pub pause_tip: &'static str,
    pub stop: &'static str,
    /// From the keys of the shortcut.
    pub stop_tip: fn(&str) -> String,
    /// `Process 12 files`.
    pub process_n: fn(usize) -> String,
    pub process: &'static str,
    pub process_tip: fn(&str) -> String,
    pub clear: &'static str,
}

pub struct Status {
    /// `6,000 × 4,000 px`, numbers already formatted.
    pub size: fn(&str, &str) -> String,
    pub zoom: &'static str,
    pub zoom_tip: &'static str,
    pub loading_model: &'static str,
    /// `Tile 3/12`.
    pub tile: fn(usize, usize) -> String,
    /// `Download 12 MB of 192 MB`, from the sizes already formatted.
    pub download: fn(&str, &str) -> String,
    /// `Queue 3/12`.
    pub queue: fn(usize, usize) -> String,
}

pub struct Central {
    /// `Opening “photo.tif”…`.
    pub opening: fn(&str) -> String,
    pub drop_here: &'static str,
    pub dismiss: &'static str,
    /// `Drop images here or press Ctrl+O`.
    pub empty_title: fn(&str) -> String,
    pub empty_formats: &'static str,
    pub open_image: &'static str,
    pub model_missing: &'static str,
    pub model_missing_detail: &'static str,
    pub choose_model: &'static str,
    pub recent: &'static str,
}

pub struct Viewer {
    pub before: &'static str,
    pub after: &'static str,
    /// The "after" side with the enlargement shown: `After · 2×`.
    pub after_enlarged: fn(&str) -> String,
    /// Over the photo while the area in view is being enlarged.
    pub enlarging_view: &'static str,
    /// Over the photo when the view is too wide to enlarge.
    pub enlarge_zoom_in: &'static str,
    pub original: &'static str,
    /// `Removed noise ×4`.
    pub removed_noise: fn(f32) -> String,
    pub navigator_tip: &'static str,
    /// Name of the image area for screen readers, from the file name.
    pub a11y_name: fn(&str) -> String,
    /// What the image area shows: zoom (formatted), mode, tiles done and
    /// total (formatted).
    pub a11y_state: fn(&str, &str, &str, &str) -> String,
    pub mode_split: &'static str,
    pub mode_noise: &'static str,
    pub mode_original: &'static str,
    pub navigator: &'static str,
}

pub struct Preferences {
    pub title: &'static str,
    pub appearance: &'static str,
    pub reduce_motion: &'static str,
    pub keyboard: &'static str,
    pub single_keys: &'static str,
    pub single_keys_tip: &'static str,
    pub model: &'static str,
    pub not_found: &'static str,
    pub change: &'static str,
    pub automatic_search: &'static str,
    pub model_where: &'static str,
    pub language: &'static str,
    /// `Automatic (English)`, from the system language's name.
    pub language_auto: fn(&str) -> String,
}

pub struct Export {
    pub title: &'static str,
    pub format: &'static str,
    pub same_as_original: &'static str,
    pub same_as_original_tip: &'static str,
    pub tiff16: &'static str,
    pub jpeg: &'static str,
    pub png16: &'static str,
    pub jpeg_quality: &'static str,
    pub jpeg_quality_tip: &'static str,
    pub suffix: &'static str,
    /// `Example: photo_dn.tif — …`, from the suffix.
    pub suffix_example: fn(&str) -> String,
    pub destination: &'static str,
    pub next_to_original: &'static str,
    /// `Folder: D:\out`.
    pub folder: fn(&str) -> String,
    pub a_folder: &'static str,
    pub upscale: &'static str,
    pub upscale_none: &'static str,
    pub upscale_x2: &'static str,
    pub upscale_x4: &'static str,
    pub upscale_x4_tip: &'static str,
    pub detail_faithful: &'static str,
    pub detail_faithful_tip: &'static str,
    pub detail_generated: &'static str,
    pub detail_generated_tip: &'static str,
    pub upscale_missing: &'static str,
    /// From the size of the result, already formatted.
    pub upscale_result: fn(&str) -> String,
    /// From the network, its licence and its training data.
    pub upscale_model: fn(&str, &str, &str) -> String,
    pub preview: &'static str,
    pub preview_tip: &'static str,
    pub preview_needs_denoise: &'static str,
    pub preview_title: &'static str,
    pub preview_computing: &'static str,
    pub preview_before: &'static str,
    pub preview_after: &'static str,
    /// From what went wrong.
    pub preview_failed: fn(&str) -> String,
    /// The download buttons, from the size still missing.
    pub download_faithful: fn(&str) -> String,
    pub download_generated: fn(&str) -> String,
    pub download_note: &'static str,
    pub downloading: &'static str,
    pub cancel_download: &'static str,
    /// The estimated time of the enlargement of the open photo, from the
    /// duration already formatted.
    pub upscale_time: fn(&str) -> String,
    pub upscale_time_unknown: &'static str,
    /// Choosing the point of the enlargement preview with a click.
    pub preview_pick: &'static str,
    pub preview_pick_tip: &'static str,
    /// Over the photo while a click chooses the point.
    pub preview_picking: &'static str,
    pub preview_computing_point: &'static str,
}

/// The keyboard shortcuts window: what each key does.
pub struct Shortcuts {
    pub title: &'static str,
    pub open: &'static str,
    pub export: &'static str,
    pub export_as: &'static str,
    pub denoise: &'static str,
    pub process_queue: &'static str,
    pub stop: &'static str,
    pub undo_redo: &'static str,
    pub close_image: &'static str,
    pub fit: &'static str,
    pub actual_size: &'static str,
    pub toggle_noise: &'static str,
    pub zoom_keys: &'static str,
    pub zoom: &'static str,
    pub double_click: &'static str,
    pub double_click_does: &'static str,
    pub original_keys: &'static str,
    pub original: &'static str,
    pub arrows: &'static str,
    pub move_view: &'static str,
    pub drag: &'static str,
    pub drag_does: &'static str,
    /// After a single-key shortcut turned off in the preferences.
    pub off: &'static str,
    /// What F6 and Shift+F6 do.
    pub panes: &'static str,
}

pub struct About {
    /// What the product name means.
    pub meaning: &'static str,
    /// From the version.
    pub version: fn(&str) -> String,
    pub network: &'static str,
    pub credits: &'static str,
    /// The Models section: each network's card and its files.
    pub models: &'static str,
    pub model_source: &'static str,
    pub model_licence: &'static str,
    pub model_training: &'static str,
    pub model_files: &'static str,
    pub model_not_installed: &'static str,
    /// `D:\models: a.onnx, b.onnx`, from the folder and the names.
    pub model_folder: fn(&str, &str) -> String,
}

/// File dialogs.
pub struct Dialogs {
    pub open_image: &'static str,
    /// `Images (TIFF, JPEG, PNG)`, from the list of formats.
    pub images_filter: fn(&str) -> String,
    pub all_files: &'static str,
    pub export_image: &'static str,
    pub add_to_queue: &'static str,
    pub add_folder: &'static str,
    pub choose_model: &'static str,
    pub model_filter: &'static str,
    pub destination_folder: &'static str,
}

/// Notices at the top of the image area, and the hints under the primary
/// button.
pub struct Notices {
    pub open_another: &'static str,
    pub show_in_folder: &'static str,
    pub choose_model: &'static str,
    pub retry_failed: &'static str,
    pub restore: &'static str,
    pub not_an_image: &'static str,
    pub file_gone: &'static str,
    /// `Could not open “photo.tif”: …`, from file name and reason.
    pub cannot_open: fn(&str, &str) -> String,
    /// From the error.
    pub denoise_failed: fn(&str) -> String,
    /// `Open an image first (Ctrl+O).`, from the shortcut.
    pub open_first: fn(&str) -> String,
    pub queue_busy: &'static str,
    pub export_after_denoise: &'static str,
    pub queue_wait: &'static str,
    /// `Queue cleared (12 files).`
    pub queue_cleared: fn(usize) -> String,
    /// From the reason.
    pub gpu_unused: fn(&str) -> String,
    /// From the reason and the device used instead.
    pub device_unused: fn(&str, &str) -> String,
    pub model_failed: fn(&str) -> String,
    /// From the file name.
    pub exported: fn(&str) -> String,
    pub export_failed: fn(&str) -> String,
    pub export_cancelled: &'static str,
    /// From the reason the GPU was not used.
    pub upscaler_on_cpu: fn(&str) -> String,
    /// The enlargement of the area in view failed, with why.
    pub view_enlarge_failed: fn(&str) -> String,
    /// The offer at start, from the size of the download.
    pub upscalers_offer: fn(&str) -> String,
    pub download_models: &'static str,
    pub no_offer: &'static str,
    pub models_downloaded: &'static str,
    pub models_present: &'static str,
    pub download_cancelled: &'static str,
    /// From what went wrong.
    pub download_failed: fn(&str) -> String,
    pub internal_error: fn(&str) -> String,
    /// Summary of a queue run: stopped or finished, succeeded, failed.
    pub queue_summary: fn(bool, usize, usize) -> String,
    /// After files were added: added, already there, skipped; `None` when
    /// everything went in.
    pub added_to_queue: fn(usize, usize, usize) -> Option<String>,
    pub model_missing: &'static str,
    pub model_not_available: &'static str,
    pub tile_size_changed: &'static str,
    pub compute_thread_stopped: &'static str,
}

/// The libraries' errors ([`crate::errors`]), shown inside the notices'
/// sentences: lower case, no final full stop.
pub struct Errors {
    pub unknown_format: &'static str,
    /// From what is not supported (in English, from the library).
    pub unsupported: fn(&str) -> String,
    /// From the decoder's message.
    pub damaged: fn(&str) -> String,
    /// From the encoder's message.
    pub encode: fn(&str) -> String,
    /// From width, height and the megapixel limit, already formatted.
    pub too_many_pixels: fn(&str, &str, &str) -> String,
    /// From the format and its largest side, already formatted.
    pub too_large: fn(&str, &str) -> String,
    /// From the format and its largest file size in GB, already formatted.
    pub file_too_large: fn(&str, &str) -> String,
    pub not_found: &'static str,
    pub permission_denied: &'static str,
    pub cancelled: &'static str,
    pub invalid_size: fn(&str) -> String,
    /// From the memory needed and the memory free, already formatted.
    pub not_enough_memory: fn(&str, &str) -> String,
    /// From the least space needed and the space free, already formatted.
    pub not_enough_disk: fn(&str, &str) -> String,
    pub out_of_range: fn(&str) -> String,
    /// The network failed, from the reason.
    pub network: fn(&str) -> String,
    /// From the model's path.
    pub model_missing: fn(&str) -> String,
    /// From the reason.
    pub model_load: fn(&str) -> String,
    pub gpu_memory: &'static str,
    pub gpu_lost: &'static str,
    pub gpu_unsupported: &'static str,
    /// Before a Windows error code: `error`.
    pub error_code: &'static str,
}

/// Labels of the actions in Undo and Redo.
pub struct HistoryTexts {
    pub recommended: &'static str,
    /// `preset Light`.
    pub preset: fn(&str) -> String,
    /// `grain 30 %`, from the rounded percentage.
    pub grain: fn(f32) -> String,
    pub colour: fn(f32) -> String,
}

/// `1 thing` or `1,200 things`, with the separators of the language in use.
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", crate::format::int(n), if n == 1 { one } else { many })
}

/// English.
pub static EN: Texts = Texts {
    numbers: Numbers {
        thousands: ',',
        decimal: '.',
        duration: |h, m, s| match (h, m) {
            (0, 0) => format!("{s} s"),
            (0, _) => format!("{m} min {s} s"),
            _ => format!("{h} h {m} min"),
        },
        megapixels: |mp| format!("{mp} MP"),
        time_left: |d| format!("about {d} left"),
        files: |n| plural(n, "file", "files"),
    },
    menu: Menu {
        file: "File",
        open: "Open…",
        open_recent: "Open recent",
        no_recent: "No recent files",
        clear_list: "Clear list",
        add_files: "Add files to queue…",
        add_folder: "Add folder to queue…",
        export: "Export",
        export_as: "Export as…",
        export_settings: "Export settings…",
        close_image: "Close image",
        quit: "Quit",
        edit: "Edit",
        undo: "Undo",
        redo: "Redo",
        undo_action: |a| format!("Undo {a}"),
        redo_action: |a| format!("Redo {a}"),
        reset: "Reset to recommended settings",
        preferences: "Preferences…",
        view: "View",
        fit: "Fit to window",
        actual_size: "Actual size (100 %)",
        show_removed_noise: "Show the removed noise",
        show_result: "Show the denoised result",
        show_queue: "Processing queue",
        process: "Process",
        denoise: "Denoise",
        stop: "Stop",
        process_queue: "Process queue",
        pause_queue: "Pause queue",
        resume_queue: "Resume queue",
        help: "Help",
        keyboard_shortcuts: "Keyboard shortcuts",
        open_log_folder: "Open log folder",
        about: |name| format!("About {name}"),
        next_pane: "Next pane",
        previous_pane: "Previous pane",
    },
    toolbar: Toolbar {
        open: "Open…",
        open_tip: |k| format!("Open an image ({k})"),
        export: "Export",
        export_tip: |k| format!("Export with the last settings ({k})"),
        fit: "Fit",
        fit_tip: |k| format!("Fit to window ({k})"),
        actual: "100 %",
        actual_tip: |k| format!("Actual size ({k})"),
        queue: "Queue",
        queue_tip: "Show or hide the processing queue",
        why: Why {
            no_image: "No image open",
            open_image_to_export: "Open an image to export",
            open_image: "Open an image",
            already_processing: "Already processing",
            queue_running: "The queue is being processed",
            nothing_running: "Nothing is being processed",
            nothing_to_undo: "Nothing to undo",
            nothing_to_redo: "Nothing to redo",
            already_recommended: "The settings are already the recommended ones",
            already_exporting: "An export is already in progress",
            no_gpu_build: "This build has no GPU support",
            queue_not_running: "The queue is not being processed",
            no_files_to_process: "No files to process",
            noise_off: "Noise reduction is switched off in the right panel",
            nothing_to_do: "Switch on the noise reduction or choose an enlargement",
            wait_for_denoising: "Wait for the current denoising to finish",
            wait_for_queue: "Wait for the queue to finish",
            wait_for_processing: "Wait for the current processing to finish",
            no_rtx: "This build has no TensorRT for RTX support",
            no_directml: "This build has no DirectML support",
            threads_cpu_only: "Threads only matter when computing on the CPU",
        },
    },
    params: ParamsTexts {
        title: "Noise reduction",
        intro: "SCUNet neural network trained on real photos: it estimates the noise by itself.",
        reduce_noise: "Reduce the noise",
        reduce_noise_tip: "Off, the photo is enlarged or exported as it is, without waiting for the noise reduction",
        preset: "Preset",
        light: "Light",
        light_tip: "Removes half of the grain: natural look, full detail",
        medium: "Medium",
        recommended_mark: " ★",
        medium_tip: "Recommended: much less grain, clean colour",
        strong: "Strong",
        strong_tip: "The network's full correction: maximum cleaning",
        grain: "Grain (luminance)",
        grain_tip: "How much grain to remove. Higher = smoother image, at the risk of losing fine detail.",
        colour: "Colour blotches",
        colour_tip: "How much colour noise to remove. Usually best kept high: colour has little fine detail.",
        stop: "Stop",
        stop_image_tip: |k| format!("{k}. Finished tiles are kept."),
        open_image: "Open image…",
        export: "Export",
        export_tip: |k| format!("{k} · uses the export settings"),
        export_as: "Export as…",
        export_settings: "Settings…",
        sliders_live: "The sliders act immediately, without recomputing.",
        enlarge: "AI enlargement",
        enlarge_none: "None",
        enlarge_x2: "2×",
        enlarge_x4: "4×",
        enlarge_tip:
            "Enlarges the photo with a neural network when exporting; 2× is more credible than 4× on fine textures",
        enlarge_when: "Shown on the “after” side of the photo, for the area in view; applied when exporting.",
        enlarge_missing: "The upscaling models are not installed: download them below",
        enlarge_options: "Options and preview…",
        denoise: "Denoise",
        finish_denoising: "Finish denoising",
        denoise_tip: |k| format!("{k} · starts from the area you are looking at"),
        exporting: "Exporting…",
        export_enlarged: |factor| format!("Export {factor}"),
        enlarging: "Enlarging and exporting…",
        cancel_export: "Cancel export",
        tile_of: |done, total| format!("Tile {done} of {total}"),
        advanced: "Advanced",
        device: "Compute device",
        auto: "Auto",
        auto_tip: "The fastest available: TensorRT for RTX, DirectML, then CPU",
        rtx: "TensorRT RTX",
        rtx_tip: "NVIDIA RTX 30xx or later through TensorRT for RTX, half precision with the _fp16 models",
        directml: "DirectML",
        directml_tip: "Any DirectX 12 GPU, NVIDIA RTX included",
        cpu: "CPU",
        threads: "CPU threads",
        threads_auto: "Automatic",
        threads_auto_tip: |n| format!("Use all {n} cores"),
        next_run: "Changes apply from the next processing run.",
        redetect: "Re-detect best tile size",
        redetect_tip: "Tries the tile sizes again from the smallest up (about 20 s at the next run)",
        in_use: |d| format!("In use: {d}"),
        model_not_loaded: |e| format!("Model not loaded: {e}"),
        model_ready: |device, px| format!("{device}, {px} px tiles"),
        histogram: "Histogram",
        histogram_tip: "Brightness (luma) of the original, filled, and of the result, as a line",
        histogram_legend: "Filled: original · line: result",
        histogram_a11y: |orig, result| format!("Median brightness: original {orig} %, result {result} %."),
    },
    info: Info {
        title: "Image",
        no_image: "No image open.",
        file: "File",
        format: "Format",
        grey: "grey",
        rgb: "RGB",
        with_alpha: " + alpha",
        size: "Size",
        profile: "Colour profile",
        profile_unnamed: "embedded (unnamed)",
        profile_none: "none (sRGB assumed)",
        display: "Display",
        display_converted: "converted from the image's profile to sRGB",
        display_as_is: "sRGB, shown as is",
        display_unusable_profile: "the image's colour profile cannot be used for display: shown as sRGB",
        display_no_conversion: "the colour conversion could not be set up: colours are shown unconverted",
        exif: "EXIF metadata",
        xmp: "XMP metadata",
        resolution: "Print resolution",
        dpi: |x, y| match y {
            Some(y) => format!("{x} × {y} dpi"),
            None => format!("{x} dpi"),
        },
        kept: "present, kept",
        none: "none",
        bit_depth: |d| match d {
            BitDepth::U8 => "8 bit",
            BitDepth::U16 => "16 bit",
            BitDepth::F32 => "32-bit float",
        },
        partial: |done, total| format!("Partially denoised: {done} of {total} tiles."),
        heavy_jpeg: |q| {
            format!(
                "Heavily compressed JPEG (quality about {q} of 100). The network learned milder compression: the \
                 8 × 8 pixel blocks may stay or turn into blotches. If you have a less compressed copy (the camera's \
                 JPEG, a RAW or a TIFF), start from that."
            )
        },
    },
    queue: Queue {
        title: "Queue",
        adding: "adding…",
        add_files: "Add files…",
        add_folder: "Folder…",
        add_folder_tip: "Add every image in a folder",
        empty: "Drop files or folders here, or use the buttons above.",
        queued: "Queued",
        processing: |done, total| format!("Processing · {done}/{total}"),
        processing_start: "Processing…",
        done: |name| format!("Done → {name}"),
        failed: "Failed",
        open_in_viewer: "Open in viewer",
        show_result: "Show result in folder",
        remove: "Remove from queue",
        destination: "Destination",
        next_to_originals: "Next to the originals",
        change: "Change…",
        progress: |done, total| format!("{done} of {total}"),
        pause: "Pause",
        resume: "Resume",
        pause_tip: "Pauses after the current tile",
        stop: "Stop",
        stop_tip: |k| format!("{k}. Files already exported are kept."),
        process_n: |n| format!("Process {}", plural(n, "file", "files")),
        process: "Process queue",
        process_tip: |k| format!("{k} · uses the current settings and export settings"),
        clear: "Clear queue",
    },
    status: Status {
        size: |w, h| format!("{w} × {h} px"),
        zoom: "Zoom",
        zoom_tip: "Zoom: drag or type a value",
        loading_model: "Loading model…",
        tile: |done, total| format!("Tile {done}/{total}"),
        download: |done, total| format!("Download {done} of {total}"),
        queue: |done, total| format!("Queue {done}/{total}"),
    },
    central: Central {
        opening: |name| format!("Opening “{name}”…"),
        drop_here: "Drop: one file opens, several files or a folder go to the queue",
        dismiss: "Dismiss",
        empty_title: |k| format!("Drop images here or press {k}"),
        empty_formats: "TIFF, JPEG and PNG up to 32 bit. Colour profile, EXIF and bit depth are kept.",
        open_image: "Open image…",
        model_missing: "SCUNet model not found",
        model_missing_detail: "The file scunet_color_real_448.onnx is needed in the program's “models” folder.",
        choose_model: "Choose model file…",
        recent: "Recent",
    },
    viewer: Viewer {
        before: "Before",
        after: "After",
        after_enlarged: |factor| format!("After · {factor}"),
        enlarging_view: "Enlarging the area in view…",
        enlarge_zoom_in: "Zoom in to see the enlargement",
        original: "Original",
        removed_noise: |gain| format!("Removed noise ×{gain}"),
        navigator_tip: "Navigator: click or drag to move the view",
        a11y_name: |name| format!("Image {name}"),
        a11y_state: |zoom, mode, done, total| {
            format!(
                "Zoom {zoom} %, {mode}; {done} of {total} tiles denoised. Drag or use the arrow keys to move, the \
                 wheel to zoom."
            )
        },
        mode_split: "original on the left of the divider, result on the right",
        mode_noise: "original on the left of the divider, removed noise on the right",
        mode_original: "original only",
        navigator: "Navigator",
    },
    preferences: Preferences {
        title: "Preferences",
        appearance: "Appearance",
        reduce_motion: "Reduce motion",
        keyboard: "Keyboard",
        single_keys: "Single-key shortcuts (F, 1, N, +, −, \\)",
        single_keys_tip: "Turn off if you use speech input, or if keys get pressed by accident. Shortcuts with Ctrl \
                          stay on.",
        model: "Model",
        not_found: "not found",
        change: "Change…",
        automatic_search: "Use automatic search",
        model_where: "The program looks for the model in the “models” folder next to the executable.",
        language: "Language",
        language_auto: |name| format!("Automatic ({name})"),
    },
    export: Export {
        title: "Export settings",
        format: "Format",
        same_as_original: "Same as original",
        same_as_original_tip: "Same format and bit depth",
        tiff16: "TIFF 16-bit",
        jpeg: "JPEG",
        png16: "PNG 16-bit",
        jpeg_quality: "JPEG quality",
        jpeg_quality_tip: "From 90 up, colour is not subsampled",
        suffix: "File name suffix",
        suffix_example: |s| format!("Example: photo{s}.tif — the original is never overwritten."),
        destination: "Destination",
        next_to_original: "Next to the original",
        folder: |d| format!("Folder: {d}"),
        a_folder: "A folder…",
        upscale: "Enlargement",
        upscale_none: "None: the photo keeps its size",
        upscale_x2: "2× (more credible)",
        upscale_x4: "4×",
        upscale_x4_tip: "16 times the pixels: less credible than 2× on fine textures, and several times slower",
        detail_faithful: "Faithful",
        detail_faithful_tip: "A network trained for fidelity: it adds no detail that was not in the photo",
        detail_generated: "Generated detail",
        detail_generated_tip: "A GAN network invents plausible detail where the photo has none: what it draws is not \
            the real scene. The file name ends in -gen.",
        upscale_missing: "Upscaling models not installed: download them in the export settings",
        upscale_result: |size| format!("Result: {size}"),
        upscale_model: |network, licence, data| format!("{network} · licence {licence} · trained on {data}"),
        preview: "Preview the enlargement",
        preview_tip: "Enlarges the area at the centre of the view with the same network and settings as the export",
        preview_needs_denoise: "Reduce the noise first: the preview shows the photo as it will be exported",
        preview_title: "Enlargement preview",
        preview_computing: "Enlarging the area at the centre of the view…",
        preview_before: "Catmull-Rom",
        preview_after: "Network",
        preview_failed: |why| format!("The preview could not be made: {why}"),
        download_faithful: |size| format!("Download the models ({size})"),
        download_generated: |size| format!("Download the generated-detail models ({size})"),
        download_note: "From the project's page on GitHub, each file checked before use; nothing else is sent.",
        downloading: "Downloading the upscaling models…",
        cancel_download: "Cancel download",
        upscale_time: |d| format!("Enlargement time: about {d} on this computer"),
        upscale_time_unknown: "Enlargement time: not measured yet (the first enlargement measures it)",
        preview_pick: "Choose the point…",
        preview_pick_tip: "Then click the photo where the preview should be taken; without it, the centre of the view \
                           is used. Esc cancels.",
        preview_picking: "Click the point to preview · Esc cancels",
        preview_computing_point: "Enlarging the area around the chosen point…",
    },
    shortcuts: Shortcuts {
        title: "Keyboard shortcuts",
        open: "Open",
        export: "Export",
        export_as: "Export as…",
        denoise: "Denoise",
        process_queue: "Process the queue",
        stop: "Stop",
        undo_redo: "Undo / Redo",
        close_image: "Close image",
        fit: "Fit to window",
        actual_size: "Actual size (100 %)",
        toggle_noise: "Removed noise ↔ result",
        zoom_keys: "+ / − (also with Ctrl), wheel",
        zoom: "Zoom in / out",
        double_click: "Double click",
        double_click_does: "100 % ↔ fit",
        original_keys: "\\ or Space (hold)",
        original: "Show the original",
        arrows: "Arrows (Shift ×10)",
        move_view: "Move the view",
        drag: "Drag",
        drag_does: "Move the view or the divider",
        off: " (off)",
        panes: "Focus to the next / previous pane: queue, photo, settings",
    },
    about: About {
        meaning: "From the ancient Greek ápsophos, “noiseless”.",
        version: |v| {
            format!("Version {v} · Copyright (C) 2026 MR · GPL-3.0-only licence, with a linking exception for DirectML and TensorRT for RTX")
        },
        network:
            "Neural network: SCUNet, K. Zhang et al., “Practical Blind Image Denoising via Swin-Conv-UNet and Data \
                  Synthesis” (Apache-2.0 licence).",
        credits: "Inference: ONNX Runtime (MIT) with DirectML (Microsoft licence) and NVIDIA TensorRT for RTX (NVIDIA \
                  licence). Colour management: Little CMS (MIT). Interface: egui (MIT/Apache-2.0). Fonts: Segoe UI \
                  and Segoe MDL2 Assets from Windows, Inter (SIL OFL \
                  1.1). Details in THIRD-PARTY-NOTICES.txt.",
        models: "Models",
        model_source: "Source",
        model_licence: "Licence",
        model_training: "Trained on",
        model_files: "Installed",
        model_not_installed: "not installed",
        model_folder: |folder, names| format!("{folder}: {names}"),
    },
    dialogs: Dialogs {
        open_image: "Open image",
        images_filter: |formats| format!("Images ({formats})"),
        all_files: "All files",
        export_image: "Export image",
        add_to_queue: "Add to queue",
        add_folder: "Add a folder to the queue",
        choose_model: "Choose the SCUNet model (.onnx)",
        model_filter: "ONNX model",
        destination_folder: "Destination folder",
    },
    notices: Notices {
        open_another: "Open another file…",
        show_in_folder: "Show in folder",
        choose_model: "Choose model file…",
        retry_failed: "Retry failed",
        restore: "Restore",
        not_an_image: "it is not a TIFF, JPEG or PNG image",
        file_gone: "the file no longer exists (it may have been moved or renamed)",
        cannot_open: |name, why| format!("Could not open “{name}”: {why}."),
        denoise_failed: |e| format!("Noise reduction failed: {e}."),
        open_first: |keys| format!("Open an image first ({keys})."),
        queue_busy: "The queue is being processed: wait for it to finish or stop it.",
        export_after_denoise: "The image will be exported as soon as noise reduction is complete.",
        queue_wait: "Wait for noise reduction of the open image to finish, then start the queue.",
        queue_cleared: |n| format!("Queue cleared ({}).", plural(n, "file", "files")),
        gpu_unused: |why| format!("The GPU could not be used ({why}). Processing continues on the CPU."),
        device_unused: |why, device| {
            format!("The chosen device could not be used ({why}). Processing continues on {device}.")
        },
        model_failed: |e| format!("The model could not be loaded: {e}."),
        exported: |name| format!("Exported “{name}”."),
        export_cancelled: "Export cancelled: no file written.",
        upscaler_on_cpu: |why| format!("The enlargement runs on the CPU, much more slowly: {why}"),
        view_enlarge_failed: |why| format!("The area in view could not be enlarged: {why}"),
        upscalers_offer: |size| {
            format!(
                "To enlarge photos 2× and 4×, Apsophos needs its upscaling models ({size}, downloaded once). \
                 Press “Download the models” to download them; then choose the enlargement in the export settings."
            )
        },
        download_models: "Download the models",
        no_offer: "Don't ask again",
        models_downloaded: "Upscaling models installed: choose 2× or 4× in the export settings.",
        models_present: "The upscaling models are already installed.",
        download_cancelled: "Download cancelled: the files already complete are kept.",
        download_failed: |why| format!("The upscaling models could not be downloaded: {why}"),
        export_failed: |e| format!("Export failed: {e}. Try another folder with File › Export as…"),
        internal_error: |e| format!("An internal error occurred ({e}). Please try again."),
        queue_summary: |cancelled, ok, failed| {
            let head = if cancelled { "Queue stopped" } else { "Queue finished" };
            if failed > 0 {
                format!("{head}: {ok} succeeded, {failed} failed (hover over a file to see why).")
            } else {
                format!("{head}: {ok} succeeded.")
            }
        },
        added_to_queue: |added, already, skipped| {
            let mut parts = Vec::new();
            if already > 0 {
                parts.push(format!("{} already there", plural(already, "file was", "files were")));
            }
            if skipped > 0 {
                parts.push(format!("{} skipped because not TIFF, JPEG or PNG", plural(skipped, "file", "files")));
            }
            (!parts.is_empty())
                .then(|| format!("{} added to the queue; {}.", plural(added, "file", "files"), parts.join(", ")))
        },
        model_missing: "SCUNet model not found. Point to the file scunet_color_real_448.onnx (in the program's \
                        models folder).",
        model_not_available: "model not available",
        tile_size_changed: "the tile size changed, please try again",
        compute_thread_stopped: "the compute thread stopped; please restart the program",
    },
    history: HistoryTexts {
        recommended: "recommended settings",
        preset: |p| format!("preset {p}"),
        grain: |v| format!("grain {v} %"),
        colour: |v| format!("colour {v} %"),
    },
    errors: Errors {
        unknown_format: "unrecognised format: TIFF, JPEG and PNG files can be opened",
        unsupported: |what| format!("{what} is not supported"),
        damaged: |why| format!("the file is damaged or unreadable ({why})"),
        encode: |why| format!("encoding the file failed ({why})"),
        too_many_pixels: |w, h, max| {
            format!("image too large to open: {w} × {h} px; the limit on this computer is {max} megapixels")
        },
        too_large: |format, max| format!("image too large for the {format} format: at most {max} px per side"),
        file_too_large: |format, max| {
            format!("image too large for a {format} file (at most {max} GB): choose 8 or 16 bits, or another format")
        },
        not_found: "the file or folder does not exist",
        permission_denied: "access denied: the file may be open in another program, or the folder is protected",
        cancelled: "processing cancelled",
        invalid_size: |what| format!("invalid size: {what}"),
        not_enough_memory: |need, free| {
            format!("not enough memory: the file needs about {need}, {free} are free; choose 2×, 8 bits or PNG")
        },
        not_enough_disk: |need, free| format!("not enough disk space: the file needs at least {need}, {free} are free"),
        out_of_range: |v| format!("value out of range: {v}"),
        network: |why| format!("the neural network could not process the image: {why}"),
        model_missing: |path| format!("the model file does not exist: {path}"),
        model_load: |why| {
            format!(
                "the model could not be loaded ({why}). The file may be damaged or may not be the SCUNet colour \
                 denoising model"
            )
        },
        gpu_memory: "not enough GPU memory for this tile size",
        gpu_lost: "the GPU stopped responding or was reset (driver update, crash or sleep); restarting the program \
                   usually helps",
        gpu_unsupported:
            "DirectML cannot run the model: the DirectML.dll next to the program is missing or out of date",
        error_code: "error",
    },
};

/// Italian.
pub static IT: Texts = Texts {
    numbers: Numbers {
        thousands: '.',
        decimal: ',',
        duration: |h, m, s| match (h, m) {
            (0, 0) => format!("{s} s"),
            (0, _) => format!("{m} min {s} s"),
            _ => format!("{h} h {m} min"),
        },
        megapixels: |mp| format!("{mp} MP"),
        time_left: |d| format!("ancora circa {d}"),
        files: |n| plural(n, "file", "file"),
    },
    menu: Menu {
        file: "File",
        open: "Apri…",
        open_recent: "Apri recenti",
        no_recent: "Nessun file recente",
        clear_list: "Svuota l'elenco",
        add_files: "Aggiungi file alla coda…",
        add_folder: "Aggiungi cartella alla coda…",
        export: "Esporta",
        export_as: "Esporta come…",
        export_settings: "Impostazioni di esportazione…",
        close_image: "Chiudi immagine",
        quit: "Esci",
        edit: "Modifica",
        undo: "Annulla",
        redo: "Ripeti",
        undo_action: |a| format!("Annulla {a}"),
        redo_action: |a| format!("Ripeti {a}"),
        reset: "Torna alle impostazioni consigliate",
        preferences: "Preferenze…",
        view: "Visualizza",
        fit: "Adatta alla finestra",
        actual_size: "Dimensioni reali (100 %)",
        show_removed_noise: "Mostra il rumore tolto",
        show_result: "Mostra il risultato",
        show_queue: "Coda di elaborazione",
        process: "Elabora",
        denoise: "Riduci il rumore",
        stop: "Ferma",
        process_queue: "Elabora la coda",
        pause_queue: "Metti in pausa la coda",
        resume_queue: "Riprendi la coda",
        help: "Aiuto",
        keyboard_shortcuts: "Scorciatoie da tastiera",
        open_log_folder: "Apri la cartella dei log",
        about: |name| format!("Informazioni su {name}"),
        next_pane: "Riquadro successivo",
        previous_pane: "Riquadro precedente",
    },
    toolbar: Toolbar {
        open: "Apri…",
        open_tip: |k| format!("Apri un'immagine ({k})"),
        export: "Esporta",
        export_tip: |k| format!("Esporta con le ultime impostazioni ({k})"),
        fit: "Adatta",
        fit_tip: |k| format!("Adatta alla finestra ({k})"),
        actual: "100 %",
        actual_tip: |k| format!("Dimensioni reali ({k})"),
        queue: "Coda",
        queue_tip: "Mostra o nascondi la coda di elaborazione",
        why: Why {
            no_image: "Nessuna immagine aperta",
            open_image_to_export: "Apri un'immagine da esportare",
            open_image: "Apri un'immagine",
            already_processing: "Elaborazione già in corso",
            queue_running: "La coda è in elaborazione",
            nothing_running: "Nessuna elaborazione in corso",
            nothing_to_undo: "Niente da annullare",
            nothing_to_redo: "Niente da ripetere",
            already_recommended: "Le impostazioni sono già quelle consigliate",
            already_exporting: "Un'esportazione è già in corso",
            no_gpu_build: "Questa versione non supporta la GPU",
            queue_not_running: "La coda non è in elaborazione",
            no_files_to_process: "Nessun file da elaborare",
            noise_off: "La riduzione del rumore è spenta nel pannello di destra",
            nothing_to_do: "Accendi la riduzione del rumore o scegli un ingrandimento",
            wait_for_denoising: "Aspetta che finisca la riduzione del rumore in corso",
            wait_for_queue: "Aspetta che finisca la coda",
            wait_for_processing: "Aspetta che finisca l'elaborazione in corso",
            no_rtx: "Questa versione non supporta TensorRT for RTX",
            no_directml: "Questa versione non supporta DirectML",
            threads_cpu_only: "I thread contano solo quando si calcola sulla CPU",
        },
    },
    params: ParamsTexts {
        title: "Riduzione del rumore",
        intro: "Rete neurale SCUNet addestrata su foto vere: stima il rumore da sola.",
        reduce_noise: "Riduci il rumore",
        reduce_noise_tip: "Spento, la foto si ingrandisce o si esporta così com'è, senza aspettare la riduzione del \
                           rumore",
        preset: "Preimpostazione",
        light: "Leggera",
        light_tip: "Toglie metà della grana: aspetto naturale, tutto il dettaglio",
        medium: "Media",
        recommended_mark: " ★",
        medium_tip: "Consigliata: molta meno grana, colori puliti",
        strong: "Forte",
        strong_tip: "La correzione completa della rete: pulizia massima",
        grain: "Grana (luminanza)",
        grain_tip: "Quanta grana togliere. Più alto = immagine più liscia, a rischio di perdere i dettagli fini.",
        colour: "Macchie di colore",
        colour_tip: "Quanto rumore di colore togliere. Di solito conviene tenerlo alto: il colore ha pochi dettagli \
                     fini.",
        stop: "Ferma",
        stop_image_tip: |k| format!("{k}. I riquadri finiti restano."),
        open_image: "Apri immagine…",
        export: "Esporta",
        export_tip: |k| format!("{k} · usa le impostazioni di esportazione"),
        export_as: "Esporta come…",
        export_settings: "Impostazioni…",
        sliders_live: "I cursori agiscono subito, senza ricalcolare.",
        enlarge: "Ingrandimento AI",
        enlarge_none: "Nessuno",
        enlarge_x2: "2×",
        enlarge_x4: "4×",
        enlarge_tip: "Ingrandisce la foto con una rete neurale durante l'esportazione; il 2× è più credibile del 4× sulle tessiture fini",
        enlarge_when: "Si vede nel lato «Dopo» della foto, per la zona inquadrata; si applica all'esportazione.",
        enlarge_missing: "I modelli di ingrandimento non sono installati: scaricali qui sotto",
        enlarge_options: "Opzioni e anteprima…",
        denoise: "Riduci il rumore",
        finish_denoising: "Completa la riduzione",
        denoise_tip: |k| format!("{k} · comincia dalla zona che stai guardando"),
        exporting: "Esportazione…",
        export_enlarged: |factor| format!("Esporta {factor}"),
        enlarging: "Ingrandimento ed esportazione…",
        cancel_export: "Annulla esportazione",
        tile_of: |done, total| format!("Riquadro {done} di {total}"),
        advanced: "Avanzate",
        device: "Dispositivo di calcolo",
        auto: "Automatico",
        auto_tip: "Il più veloce disponibile: TensorRT for RTX, DirectML, poi CPU",
        rtx: "TensorRT RTX",
        rtx_tip: "NVIDIA RTX 30xx o successive con TensorRT for RTX, mezza precisione con i modelli _fp16",
        directml: "DirectML",
        directml_tip: "Qualsiasi GPU DirectX 12, NVIDIA RTX comprese",
        cpu: "CPU",
        threads: "Thread della CPU",
        threads_auto: "Automatico",
        threads_auto_tip: |n| format!("Usa tutti i {n} core"),
        next_run: "Le modifiche valgono dalla prossima elaborazione.",
        redetect: "Rileva di nuovo la dimensione dei riquadri",
        redetect_tip: "Riprova le dimensioni dei riquadri dalla più piccola (circa 20 s alla prossima elaborazione)",
        in_use: |d| format!("In uso: {d}"),
        model_not_loaded: |e| format!("Modello non caricato: {e}"),
        model_ready: |device, px| format!("{device}, riquadri da {px} px"),
        histogram: "Istogramma",
        histogram_tip: "Luminosità (luma) dell'originale, piena, e del risultato, come linea",
        histogram_legend: "Pieno: originale · linea: risultato",
        histogram_a11y: |orig, result| format!("Luminosità mediana: originale {orig} %, risultato {result} %."),
    },
    info: Info {
        title: "Immagine",
        no_image: "Nessuna immagine aperta.",
        file: "File",
        format: "Formato",
        grey: "grigio",
        rgb: "RGB",
        with_alpha: " + alfa",
        size: "Dimensioni",
        profile: "Profilo colore",
        profile_unnamed: "incorporato (senza nome)",
        profile_none: "nessuno (si assume sRGB)",
        display: "Visualizzazione",
        display_converted: "convertita dal profilo dell'immagine in sRGB",
        display_as_is: "sRGB, mostrata così com'è",
        display_unusable_profile: "il profilo colore dell'immagine non si può usare per la visualizzazione: \
                                   mostrata come sRGB",
        display_no_conversion: "non è stato possibile preparare la conversione dei colori: colori mostrati senza \
                                conversione",
        exif: "Metadati EXIF",
        xmp: "Metadati XMP",
        resolution: "Risoluzione di stampa",
        dpi: |x, y| match y {
            Some(y) => format!("{x} × {y} dpi"),
            None => format!("{x} dpi"),
        },
        kept: "presenti, conservati",
        none: "nessuno",
        bit_depth: |d| match d {
            BitDepth::U8 => "8 bit",
            BitDepth::U16 => "16 bit",
            BitDepth::F32 => "32 bit in virgola mobile",
        },
        partial: |done, total| format!("Rumore ridotto in parte: {done} riquadri su {total}."),
        heavy_jpeg: |q| {
            format!(
                "JPEG molto compresso (qualità circa {q} su 100). La rete ha imparato compressioni più leggere: i \
                 blocchi di 8 × 8 pixel possono restare o diventare macchie. Se hai una copia meno compressa (il JPEG \
                 della fotocamera, un RAW o un TIFF), parti da quella."
            )
        },
    },
    queue: Queue {
        title: "Coda",
        adding: "aggiunta…",
        add_files: "Aggiungi file…",
        add_folder: "Cartella…",
        add_folder_tip: "Aggiungi tutte le immagini di una cartella",
        empty: "Trascina qui file o cartelle, oppure usa i pulsanti sopra.",
        queued: "In attesa",
        processing: |done, total| format!("In elaborazione · {done}/{total}"),
        processing_start: "In elaborazione…",
        done: |name| format!("Fatto → {name}"),
        failed: "Non riuscito",
        open_in_viewer: "Apri nel visualizzatore",
        show_result: "Mostra il risultato nella cartella",
        remove: "Togli dalla coda",
        destination: "Destinazione",
        next_to_originals: "Accanto agli originali",
        change: "Cambia…",
        progress: |done, total| format!("{done} di {total}"),
        pause: "Pausa",
        resume: "Riprendi",
        pause_tip: "Mette in pausa dopo il riquadro in corso",
        stop: "Ferma",
        stop_tip: |k| format!("{k}. I file già esportati restano."),
        process_n: |n| format!("Elabora {}", plural(n, "file", "file")),
        process: "Elabora la coda",
        process_tip: |k| format!("{k} · usa le impostazioni attuali e quelle di esportazione"),
        clear: "Svuota la coda",
    },
    status: Status {
        size: |w, h| format!("{w} × {h} px"),
        zoom: "Zoom",
        zoom_tip: "Zoom: trascina o scrivi un valore",
        loading_model: "Caricamento del modello…",
        tile: |done, total| format!("Riquadro {done}/{total}"),
        download: |done, total| format!("Download: {done} di {total}"),
        queue: |done, total| format!("Coda {done}/{total}"),
    },
    central: Central {
        opening: |name| format!("Apertura di “{name}”…"),
        drop_here: "Rilascia: un file si apre, più file o una cartella vanno nella coda",
        dismiss: "Chiudi",
        empty_title: |k| format!("Trascina qui le immagini o premi {k}"),
        empty_formats: "TIFF, JPEG e PNG fino a 32 bit. Profilo colore, EXIF e profondità in bit restano.",
        open_image: "Apri immagine…",
        model_missing: "Modello SCUNet non trovato",
        model_missing_detail: "Serve il file scunet_color_real_448.onnx nella cartella “models” del programma.",
        choose_model: "Scegli il file del modello…",
        recent: "Recenti",
    },
    viewer: Viewer {
        before: "Prima",
        after: "Dopo",
        after_enlarged: |factor| format!("Dopo · {factor}"),
        enlarging_view: "Ingrandimento della zona inquadrata…",
        enlarge_zoom_in: "Ingrandisci la vista per vedere l'ingrandimento",
        original: "Originale",
        removed_noise: |gain| format!("Rumore tolto ×{gain}"),
        navigator_tip: "Navigatore: fai clic o trascina per spostare la vista",
        a11y_name: |name| format!("Immagine {name}"),
        a11y_state: |zoom, mode, done, total| {
            format!(
                "Zoom {zoom} %, {mode}; rumore ridotto in {done} riquadri su {total}. Trascina o usa le frecce per \
                 spostarti, la rotella per lo zoom."
            )
        },
        mode_split: "originale a sinistra del divisore, risultato a destra",
        mode_noise: "originale a sinistra del divisore, rumore tolto a destra",
        mode_original: "solo l'originale",
        navigator: "Navigatore",
    },
    preferences: Preferences {
        title: "Preferenze",
        appearance: "Aspetto",
        reduce_motion: "Riduci le animazioni",
        keyboard: "Tastiera",
        single_keys: "Scorciatoie a un tasto (F, 1, N, +, −, \\)",
        single_keys_tip: "Spegnile se usi la dettatura o se ti capita di premerle per sbaglio. Le scorciatoie con \
                          Ctrl restano attive.",
        model: "Modello",
        not_found: "non trovato",
        change: "Cambia…",
        automatic_search: "Usa la ricerca automatica",
        model_where: "Il programma cerca il modello nella cartella “models” accanto all'eseguibile.",
        language: "Lingua",
        language_auto: |name| format!("Automatica ({name})"),
    },
    export: Export {
        title: "Impostazioni di esportazione",
        format: "Formato",
        same_as_original: "Come l'originale",
        same_as_original_tip: "Stesso formato e stessa profondità in bit",
        tiff16: "TIFF 16 bit",
        jpeg: "JPEG",
        png16: "PNG 16 bit",
        jpeg_quality: "Qualità JPEG",
        jpeg_quality_tip: "Da 90 in su il colore non è sottocampionato",
        suffix: "Suffisso del nome del file",
        suffix_example: |s| format!("Esempio: foto{s}.tif — l'originale non viene mai sovrascritto."),
        destination: "Destinazione",
        next_to_original: "Accanto all'originale",
        folder: |d| format!("Cartella: {d}"),
        a_folder: "Una cartella…",
        upscale: "Ingrandimento",
        upscale_none: "Nessuno: la foto mantiene le dimensioni",
        upscale_x2: "2× (più credibile)",
        upscale_x4: "4×",
        upscale_x4_tip: "16 volte i pixel: meno credibile del 2× sulle tessiture fini, e alcune volte più lento",
        detail_faithful: "Fedele",
        detail_faithful_tip: "Una rete addestrata per la fedeltà: non aggiunge dettagli che non erano nella foto",
        detail_generated: "Dettagli generati",
        detail_generated_tip: "Una rete GAN inventa dettagli plausibili dove la foto non ne ha: ciò che disegna non \
            è la scena reale. Il nome del file finisce con -gen.",
        upscale_missing: "Modelli di ingrandimento non installati: scaricali dalle impostazioni di esportazione",
        upscale_result: |size| format!("Risultato: {size}"),
        upscale_model: |network, licence, data| format!("{network} · licenza {licence} · addestrata su {data}"),
        preview: "Anteprima dell'ingrandimento",
        preview_tip: "Ingrandisce la zona al centro della vista con la stessa rete e le stesse impostazioni \
            dell'esportazione",
        preview_needs_denoise: "Riduci prima il rumore: l'anteprima mostra la foto come verrà esportata",
        preview_title: "Anteprima dell'ingrandimento",
        preview_computing: "Ingrandimento della zona al centro della vista…",
        preview_before: "Catmull-Rom",
        preview_after: "Rete",
        preview_failed: |why| format!("Impossibile creare l'anteprima: {why}"),
        download_faithful: |size| format!("Scarica i modelli ({size})"),
        download_generated: |size| format!("Scarica i modelli a dettagli generati ({size})"),
        download_note:
            "Dalla pagina del progetto su GitHub, ogni file controllato prima dell'uso; non si invia nient'altro.",
        downloading: "Download dei modelli di ingrandimento…",
        cancel_download: "Annulla download",
        upscale_time: |d| format!("Tempo dell'ingrandimento: circa {d} su questo computer"),
        upscale_time_unknown: "Tempo dell'ingrandimento: non ancora misurato (lo misura il primo ingrandimento)",
        preview_pick: "Scegli il punto…",
        preview_pick_tip:
            "Poi fai clic sulla foto dove prendere l'anteprima; senza, si usa il centro della vista. Esc \
                           annulla.",
        preview_picking: "Fai clic sul punto da vedere in anteprima · Esc annulla",
        preview_computing_point: "Ingrandimento della zona attorno al punto scelto…",
    },
    shortcuts: Shortcuts {
        title: "Scorciatoie da tastiera",
        open: "Apri",
        export: "Esporta",
        export_as: "Esporta come…",
        denoise: "Riduci il rumore",
        process_queue: "Elabora la coda",
        stop: "Ferma",
        undo_redo: "Annulla / Ripeti",
        close_image: "Chiudi immagine",
        fit: "Adatta alla finestra",
        actual_size: "Dimensioni reali (100 %)",
        toggle_noise: "Rumore tolto ↔ risultato",
        zoom_keys: "+ / − (anche con Ctrl), rotella",
        zoom: "Ingrandisci / riduci",
        double_click: "Doppio clic",
        double_click_does: "100 % ↔ adatta",
        original_keys: "\\ o Spazio (tenuto premuto)",
        original: "Mostra l'originale",
        arrows: "Frecce (Maiusc ×10)",
        move_view: "Sposta la vista",
        drag: "Trascina",
        drag_does: "Sposta la vista o il divisore",
        off: " (spenta)",
        panes: "Passa al riquadro successivo / precedente: coda, foto, impostazioni",
    },
    about: About {
        meaning: "Dal greco antico ápsophos, “senza rumore”.",
        version: |v| {
            format!(
                "Versione {v} · Copyright (C) 2026 MR · licenza GPL-3.0-only, con un'eccezione per DirectML e \
                 TensorRT for RTX"
            )
        },
        network: "Rete neurale: SCUNet, K. Zhang et al., “Practical Blind Image Denoising via Swin-Conv-UNet and Data \
                  Synthesis” (licenza Apache-2.0).",
        credits: "Inferenza: ONNX Runtime (MIT) con DirectML (licenza Microsoft) e NVIDIA TensorRT for RTX (licenza \
                  NVIDIA). Gestione del colore: Little CMS (MIT). Interfaccia: egui (MIT/Apache-2.0). Caratteri: \
                  Segoe UI e Segoe MDL2 Assets di Windows, Inter (SIL OFL 1.1). Dettagli in THIRD-PARTY-NOTICES.txt.",
        models: "Modelli",
        model_source: "Fonte",
        model_licence: "Licenza",
        model_training: "Addestrato su",
        model_files: "Installato",
        model_not_installed: "non installato",
        model_folder: |folder, names| format!("{folder}: {names}"),
    },
    dialogs: Dialogs {
        open_image: "Apri immagine",
        images_filter: |formats| format!("Immagini ({formats})"),
        all_files: "Tutti i file",
        export_image: "Esporta immagine",
        add_to_queue: "Aggiungi alla coda",
        add_folder: "Aggiungi una cartella alla coda",
        choose_model: "Scegli il modello SCUNet (.onnx)",
        model_filter: "Modello ONNX",
        destination_folder: "Cartella di destinazione",
    },
    notices: Notices {
        open_another: "Apri un altro file…",
        show_in_folder: "Mostra nella cartella",
        choose_model: "Scegli il file del modello…",
        retry_failed: "Riprova quelli non riusciti",
        restore: "Ripristina",
        not_an_image: "non è un'immagine TIFF, JPEG o PNG",
        file_gone: "il file non esiste più (forse è stato spostato o rinominato)",
        cannot_open: |name, why| format!("Impossibile aprire “{name}”: {why}."),
        denoise_failed: |e| format!("Riduzione del rumore non riuscita: {e}."),
        open_first: |keys| format!("Apri prima un'immagine ({keys})."),
        queue_busy: "La coda è in elaborazione: aspetta che finisca o fermala.",
        export_after_denoise: "L'immagine sarà esportata appena finita la riduzione del rumore.",
        queue_wait: "Aspetta che finisca la riduzione del rumore dell'immagine aperta, poi avvia la coda.",
        queue_cleared: |n| format!("Coda svuotata ({}).", plural(n, "file", "file")),
        gpu_unused: |why| format!("Non è stato possibile usare la GPU ({why}). L'elaborazione continua sulla CPU."),
        device_unused: |why, device| {
            format!("Non è stato possibile usare il dispositivo scelto ({why}). L'elaborazione continua con {device}.")
        },
        model_failed: |e| format!("Impossibile caricare il modello: {e}."),
        exported: |name| format!("Esportato “{name}”."),
        export_cancelled: "Esportazione annullata: nessun file scritto.",
        upscaler_on_cpu: |why| format!("L'ingrandimento usa la CPU, molto più lenta: {why}"),
        view_enlarge_failed: |why| format!("Impossibile ingrandire la zona inquadrata: {why}"),
        upscalers_offer: |size| {
            format!(
                "Per ingrandire le foto 2× e 4× servono i modelli di ingrandimento ({size}, da scaricare una volta \
                 sola). Premi «Scarica i modelli» per scaricarli; poi scegli l'ingrandimento nelle impostazioni di \
                 esportazione."
            )
        },
        download_models: "Scarica i modelli",
        no_offer: "Non chiedere più",
        models_downloaded: "Modelli di ingrandimento installati: scegli 2× o 4× nelle impostazioni di esportazione.",
        models_present: "I modelli di ingrandimento sono già installati.",
        download_cancelled: "Download annullato: i file già completi restano.",
        download_failed: |why| format!("Impossibile scaricare i modelli di ingrandimento: {why}"),
        export_failed: |e| format!("Esportazione non riuscita: {e}. Prova un'altra cartella con File › Esporta come…"),
        internal_error: |e| format!("Si è verificato un errore interno ({e}). Riprova."),
        queue_summary: |cancelled, ok, failed| {
            let head = if cancelled { "Coda fermata" } else { "Coda finita" };
            if failed > 0 {
                format!("{head}: {ok} riusciti, {failed} non riusciti (passa sopra un file per vedere perché).")
            } else {
                format!("{head}: {ok} riusciti.")
            }
        },
        added_to_queue: |added, already, skipped| {
            let mut parts = Vec::new();
            if already > 0 {
                let were = if already == 1 { "era già presente" } else { "erano già presenti" };
                parts.push(format!("{} {were}", plural(already, "file", "file")));
            }
            if skipped > 0 {
                parts.push(format!("{} saltati perché non TIFF, JPEG o PNG", plural(skipped, "file", "file")));
            }
            (!parts.is_empty())
                .then(|| format!("{} aggiunti alla coda; {}.", plural(added, "file", "file"), parts.join(", ")))
        },
        model_missing: "Modello SCUNet non trovato. Indica il file scunet_color_real_448.onnx (nella cartella models \
                        del programma).",
        model_not_available: "modello non disponibile",
        tile_size_changed: "la dimensione dei riquadri è cambiata, riprova",
        compute_thread_stopped: "il thread di calcolo si è fermato; riavvia il programma",
    },
    history: HistoryTexts {
        recommended: "impostazioni consigliate",
        preset: |p| format!("preimpostazione {p}"),
        grain: |v| format!("grana {v} %"),
        colour: |v| format!("colore {v} %"),
    },
    errors: Errors {
        unknown_format: "formato non riconosciuto: si possono aprire file TIFF, JPEG e PNG",
        unsupported: |what| format!("non supportato: {what}"),
        damaged: |why| format!("il file è danneggiato o illeggibile ({why})"),
        encode: |why| format!("la scrittura del file non è riuscita ({why})"),
        too_many_pixels: |w, h, max| {
            format!("immagine troppo grande da aprire: {w} × {h} px; il limite su questo computer è {max} megapixel")
        },
        too_large: |format, max| {
            format!("immagine troppo grande per il formato {format}: al massimo {max} px per lato")
        },
        file_too_large: |format, max| {
            format!("immagine troppo grande per un file {format} (al massimo {max} GB): scegli 8 o 16 bit, o un altro formato")
        },
        not_found: "il file o la cartella non esiste",
        permission_denied: "accesso negato: il file potrebbe essere aperto in un altro programma, o la cartella è \
                            protetta",
        cancelled: "elaborazione annullata",
        invalid_size: |what| format!("dimensioni non valide: {what}"),
        not_enough_memory: |need, free| {
            format!(
                "memoria insufficiente: il file richiede circa {need}, ne sono liberi {free}; scegli 2×, 8 bit o PNG"
            )
        },
        not_enough_disk: |need, free| {
            format!("spazio su disco insufficiente: il file richiede almeno {need}, ne sono liberi {free}")
        },
        out_of_range: |v| format!("valore fuori dai limiti: {v}"),
        network: |why| format!("la rete neurale non è riuscita a elaborare l'immagine: {why}"),
        model_missing: |path| format!("il file del modello non esiste: {path}"),
        model_load: |why| {
            format!(
                "impossibile caricare il modello ({why}). Il file potrebbe essere danneggiato o non essere il modello \
                 SCUNet per il rumore a colori"
            )
        },
        gpu_memory: "memoria della GPU insufficiente per questa dimensione dei riquadri",
        gpu_lost: "la GPU ha smesso di rispondere o è stata reimpostata (aggiornamento del driver, crash o \
                   sospensione); di solito basta riavviare il programma",
        gpu_unsupported: "DirectML non riesce a eseguire il modello: il file DirectML.dll accanto al programma manca \
                          o è vecchio",
        error_code: "errore",
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals() {
        let n = &tr().notices;
        assert_eq!((n.queue_cleared)(1), "Queue cleared (1 file).");
        assert_eq!((n.queue_cleared)(3), "Queue cleared (3 files).");
        assert_eq!((tr().queue.process_n)(1), "Process 1 file");
        assert_eq!((tr().numbers.files)(1200), "1,200 files");
        assert_eq!((n.added_to_queue)(4, 0, 0), None);
        assert_eq!(
            (n.added_to_queue)(0, 1, 0).as_deref(),
            Some("0 files added to the queue; 1 file was already there.")
        );
        assert_eq!(
            (n.added_to_queue)(5, 1, 2).as_deref(),
            Some(
                "5 files added to the queue; 1 file was already there, 2 files skipped because not TIFF, JPEG or PNG."
            )
        );
    }

    #[test]
    fn queue_summary() {
        let s = tr().notices.queue_summary;
        assert_eq!(s(false, 3, 0), "Queue finished: 3 succeeded.");
        assert_eq!(s(true, 2, 1), "Queue stopped: 2 succeeded, 1 failed (hover over a file to see why).");
    }

    #[test]
    fn italian_sentences() {
        let n = &IT.notices;
        assert_eq!((n.queue_cleared)(1), "Coda svuotata (1 file).");
        assert_eq!((n.queue_cleared)(3), "Coda svuotata (3 file).");
        assert_eq!(
            (n.added_to_queue)(5, 1, 2).as_deref(),
            Some("5 file aggiunti alla coda; 1 file era già presente, 2 file saltati perché non TIFF, JPEG o PNG.")
        );
        assert_eq!(
            (n.added_to_queue)(2, 3, 0).as_deref(),
            Some("2 file aggiunti alla coda; 3 file erano già presenti.")
        );
        assert_eq!((IT.notices.queue_summary)(true, 2, 1).split(':').next(), Some("Coda fermata"));
        assert_eq!((IT.info.bit_depth)(BitDepth::F32), "32 bit in virgola mobile");
        assert_eq!((IT.numbers.thousands, IT.numbers.decimal), ('.', ','));
    }

    #[test]
    fn languages_have_their_own_names() {
        assert_eq!(Language::ALL.map(Language::name), ["English", "Italiano"]);
        // tests never follow the computer's language
        assert_eq!(Language::system(), Language::English);
    }

    #[test]
    fn durations() {
        let d = tr().numbers.duration;
        assert_eq!(d(0, 0, 45), "45 s");
        assert_eq!(d(0, 1, 20), "1 min 20 s");
        assert_eq!(d(1, 5, 0), "1 h 5 min");
    }
}
