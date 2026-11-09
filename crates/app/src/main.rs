//! Apsophos: photo noise reduction with the SCUNet network.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod color;
mod document;
mod download;
mod errors;
mod files;
mod format;
mod history;
mod inspect;
mod logging;
mod model_list;
mod network_space;
mod platform;
mod resources;
mod selftest;
mod settings;
mod symbol;
mod text;
mod theme;
mod ui;
#[cfg(test)]
mod ui_tests;
mod upscaling;
mod viewer;
mod worker;

use std::sync::Arc;

use denoise_core::product;
use eframe::egui;

fn main() -> eframe::Result {
    // first of all, before any library or window loads a DLL by name
    let dll_search = denoise_platform::restrict_dll_search();
    // kept until the end of main, so the last lines reach the file
    let _log = logging::init();
    if let Err(e) = dll_search {
        tracing::warn!(error = %e, "DLL search not restricted to the program folder and System32");
    }
    // `--self-test [report.txt]`: tests the model on GPU and CPU without a window
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--self-test") {
        let report = args.get(2).map_or_else(|| std::path::PathBuf::from("self-test.txt"), std::path::PathBuf::from);
        if let Err(e) = selftest::run(&report) {
            eprintln!("could not write {}: {e}", report.display());
        }
        return Ok(());
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(product::NAME)
        .with_app_id(product::ID)
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([900.0, 600.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, persist_window: true, ..Default::default() };
    // files passed on the command line (`--self-test` was handled above)
    let files = args.into_iter().skip(1).map(std::path::PathBuf::from).collect();
    eframe::run_native(product::NAME, options, Box::new(|cc| Ok(Box::new(app::ApsophosApp::new(cc, files)))))
}
