//! Interface tests: the real app in an `egui_kittest` harness, without a
//! window or a GPU. They find widgets through the accessibility tree, as a
//! screen reader would, and drive them with clicks, keys, the wheel and
//! dropped files. The harness also fails a test when an input widget has no
//! accessible name.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use denoise_core::Image;
use denoise_io::{BitDepth, ExportOptions, FileFormat, SourceInfo};
use eframe::egui::{self, accesskit::Role, Key};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

use crate::app::ApsophosApp;
use crate::text::tr;
use crate::viewer::ResultView;

const SIZE: [f32; 2] = [1280.0, 820.0];

fn harness() -> Harness<'static, ApsophosApp> {
    let mut h = Harness::builder().with_size(SIZE).build_eframe(|cc| ApsophosApp::new(cc, Vec::new()));
    h.run_steps(3);
    h
}

/// A small 8-bit PNG with a gradient, in `dir`.
fn sample_png(dir: &Path, name: &str) -> PathBuf {
    let (w, h) = (48, 32);
    let data = (0..w * h).flat_map(|i| [(i % w) as f32 / w as f32, 0.5, (i / w) as f32 / h as f32]).collect();
    let img = Image::from_rgb(w, h, data).unwrap();
    let info = SourceInfo {
        format: FileFormat::Png,
        bit_depth: BitDepth::U8,
        gray: false,
        icc: None,
        exif: None,
        xmp: None,
        xmp_extended: None,
        dpi: None,
        notes: vec![],
        jpeg_quality: None,
    };
    let path = dir.join(name);
    denoise_io::write(&path, &img, None, &info, &ExportOptions::default()).unwrap();
    path
}

/// Steps the app until `done` holds: the worker thread answers on its own
/// time, so a fixed number of frames is not enough.
fn step_until(h: &mut Harness<'_, ApsophosApp>, what: &str, done: impl Fn(&ApsophosApp) -> bool) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run_steps(3);
}

/// A file dropped onto the window, as the native integration hands it over.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn open_dropped(h: &mut Harness<'_, ApsophosApp>, path: PathBuf) {
    h.input_mut().dropped_files.push(Arc::new(Dropped(path)));
    h.step();
    step_until(h, "the dropped image to open", |app| app.doc.is_some());
}

/// Whether the open menu's entry `label` is disabled. A menu entry's
/// accessible name ends with its shortcut ("Export Ctrl+E"), which tells it
/// apart from the toolbar button of the same name.
fn menu_entry_disabled(h: &Harness<'_, ApsophosApp>, label: &'static str) -> bool {
    let entry = h.get_by(move |n| {
        n.role() == Role::Button && n.label().is_some_and(|l| l == label || l.starts_with(&format!("{label} ")))
    });
    entry.accesskit_node().is_disabled()
}

#[test]
fn starts_empty_with_named_widgets() {
    let h = harness();
    assert!(h.state().doc.is_none());
    // the empty viewer says how to open an image, with the real shortcut
    h.get_by_label_contains("Ctrl+O");
    h.get_by_role_and_label(Role::Button, tr().menu.file);
}

#[test]
fn file_menu_disables_export_without_an_image() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, tr().menu.file).click();
    h.run_steps(3);
    assert!(menu_entry_disabled(&h, tr().menu.export_as));
    assert!(menu_entry_disabled(&h, tr().menu.close_image));
    assert!(!menu_entry_disabled(&h, tr().menu.add_files));
}

#[test]
fn export_settings_offer_the_enlargement() {
    let mut h = harness();
    h.state_mut().windows.export = true;
    h.run_steps(3);
    let te = &tr().export;
    // "none" is always there; 2Ã— and 4Ã— are there, enabled or not as the
    // models on this computer allow
    h.get_by_label(te.upscale_none);
    h.get_by_label(te.upscale_x2);
    h.get_by_label(te.upscale_x4);
    assert!(h.state().settings.export.upscale.is_none());
    assert_eq!(h.state().settings.export.full_suffix(), "_dn");
}

#[test]
fn menu_closes_after_a_command() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, tr().menu.view).click();
    h.run_steps(3);
    h.get_by_label(tr().menu.show_queue).click();
    h.run_steps(3);
    assert!(h.query_by_label(tr().menu.show_queue).is_none(), "the View menu is still open");
}

#[test]
fn opens_a_dropped_image_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = sample_png(dir.path(), "drop.png");
    let mut h = harness();
    open_dropped(&mut h, path);
    // the hand-drawn viewer has a name for screen readers
    h.get_by_role_and_label(Role::Image, &(tr().viewer.a11y_name)("drop.png"));
    h.get_by_role_and_label(Role::Button, tr().menu.file).click();
    h.run_steps(3);
    assert!(!menu_entry_disabled(&h, tr().menu.export_as));
}

#[test]
fn single_keys_follow_the_setting() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    open_dropped(&mut h, sample_png(dir.path(), "keys.png"));
    h.key_press(Key::N);
    h.run_steps(2);
    assert_eq!(h.state().result_view, ResultView::RemovedNoise);
    h.key_press(Key::N);
    h.run_steps(2);
    assert_eq!(h.state().result_view, ResultView::Denoised);
    // turned off in the preferences: N does nothing
    h.state_mut().settings.single_keys_off = true;
    h.key_press(Key::N);
    h.run_steps(2);
    assert_eq!(h.state().result_view, ResultView::Denoised);
}

#[test]
fn single_keys_type_into_a_text_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    open_dropped(&mut h, sample_png(dir.path(), "typing.png"));
    h.state_mut().windows.export = true;
    h.run_steps(3);
    let field = h.get_by_role(Role::TextInput);
    field.focus();
    field.type_text("N");
    h.run_steps(3);
    // the letter went into the field, not to the "removed noise" view
    assert_eq!(h.state().result_view, ResultView::Denoised);
    assert!(h.state().settings.export.suffix.ends_with('N'), "{}", h.state().settings.export.suffix);
}

#[test]
fn ctrl_plus_zooms_the_photo_not_the_interface() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    open_dropped(&mut h, sample_png(dir.path(), "keys-zoom.png"));
    let before = h.state().view.zoom_percent();
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Plus);
    h.run_steps(4);
    assert!(h.state().view.zoom_percent() > before * 1.1, "the photo did not zoom in");
    let scale = h.ctx.zoom_factor();
    assert!((scale - 1.0).abs() < 1e-6, "the interface zoomed to {scale}");
}

#[test]
fn wheel_zooms_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    open_dropped(&mut h, sample_png(dir.path(), "zoom.png"));
    let before = h.state().view.zoom_percent();
    let centre = h.get_by_role_and_label(Role::Image, &(tr().viewer.a11y_name)("zoom.png")).rect().center();
    h.hover_at(centre);
    h.run_steps(2);
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 120.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(8);
    let after = h.state().view.zoom_percent();
    assert!(after > before * 1.05, "zoom {before} % â†’ {after} %");
}

#[test]
fn queue_context_menu_opens_an_item() {
    let dir = tempfile::tempdir().unwrap();
    let a = sample_png(dir.path(), "first.png");
    let b = sample_png(dir.path(), "second.png");
    let mut h = harness();
    h.state_mut().settings.show_queue = true;
    h.state_mut().add_to_queue(vec![a, b]);
    step_until(&mut h, "the queue to fill", |app| app.queue.items.len() == 2);
    h.get_by(|n| n.role() == Role::ListItem && n.label().is_some_and(|l| l.contains("second.png"))).click_secondary();
    h.run_steps(3);
    h.get_by_label(tr().queue.open_in_viewer).click();
    h.run_steps(2);
    assert!(h.query_by_label(tr().queue.open_in_viewer).is_none(), "the context menu is still open");
    step_until(&mut h, "the queue item to open", |app| app.doc.is_some());
    assert!(h.state().doc.as_ref().is_some_and(|d| d.path.ends_with("second.png")));
}

#[test]
fn a_click_chooses_the_point_of_the_enlargement_preview() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    open_dropped(&mut h, sample_png(dir.path(), "point.png"));
    let te = &tr().export;
    h.state_mut().windows.upscale_preview = true;
    h.run_steps(3);
    // not denoised: the button says why it cannot be used
    assert!(h.get_by_label(te.preview_pick).accesskit_node().is_disabled());
    // choosing (as the button does once the photo is denoised), then Esc
    let name = (tr().viewer.a11y_name)("point.png");
    // the hint over the photo, as a screen reader hears it
    let hint = |h: &Harness<'_, ApsophosApp>| {
        let photo = h.get_by_role_and_label(Role::Image, &name);
        photo.accesskit_node().description().is_some_and(|d| d.starts_with(te.preview_picking))
    };
    h.state_mut().pick_preview_point();
    h.run_steps(3);
    assert!(hint(&h));
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(!h.state().picking_preview_point && !hint(&h));
    // choosing again, then a click on the photo: that point, and the choice
    // ends; the small photo is zoomed in to fill the view
    h.state_mut().settings.reduce_motion = true;
    h.state_mut().view.set_zoom_percent(2000.0);
    h.state_mut().pick_preview_point();
    h.run_steps(3);
    let photo = h.get_by_role_and_label(Role::Image, &name).rect();
    // right of the centre, clear of the preview window in the middle
    let at = photo.center() + egui::vec2(photo.width() * 0.4, 0.0);
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(3);
    let state = h.state();
    assert!(!state.picking_preview_point);
    let (x, y) = state.preview_point.expect("a point was chosen").pixel(48, 32);
    // right of the centre of the 48 × 32 px photo, at its height
    assert!(x > 24 && (14..=17).contains(&y), "({x}, {y})");
}

#[test]
fn the_about_window_lists_the_models() {
    let mut h = harness();
    h.state_mut().windows.about = true;
    h.run_steps(3);
    let denoiser = denoise_engine::DENOISER_CARD;
    assert!(h.query_by_label(denoiser.network).is_none(), "the section starts closed");
    h.get_by_label(tr().about.models).click();
    h.run_steps(3);
    h.get_by_label(denoiser.network);
    h.get_by_label(denoiser.source);
    assert_eq!(h.get_all_by_label(tr().about.model_files).count(), 3, "denoising, 2× and 4×");
    // closing the window forgets the files read, so the next opening reads them again
    assert!(h.state().model_list.is_some());
    h.state_mut().windows.about = false;
    h.run_steps(2);
    assert!(h.state().model_list.is_none());
}

#[test]
fn a_heavily_compressed_jpeg_is_pointed_out() {
    let dir = tempfile::tempdir().unwrap();
    let png = sample_png(dir.path(), "source.png");
    let picture = denoise_io::read(&png, &denoise_io::ReadLimits::default()).unwrap();
    let jpeg = |quality: u8| {
        let path = dir.path().join(format!("q{quality}.jpg"));
        let options = ExportOptions { format: denoise_io::ExportFormat::Jpeg, jpeg_quality: quality };
        denoise_io::write(&path, &picture.rgb, None, &picture.info, &options).unwrap();
        path
    };
    let notice = "Heavily compressed JPEG";
    assert!((tr().info.heavy_jpeg)(30).starts_with(notice));
    let mut h = harness();
    open_dropped(&mut h, jpeg(30));
    h.get_by_label_contains(notice);
    // a JPEG of ordinary quality says nothing
    h.state_mut().close_image();
    open_dropped(&mut h, jpeg(90));
    assert!(h.query_by_label_contains(notice).is_none());
}

/// Whether a widget with this role and name has the keyboard focus (the
/// same name may be on more than one widget).
fn focused(h: &Harness<'_, ApsophosApp>, role: Role, label: &str) -> bool {
    h.query_all_by_role_and_label(role, label).any(|n| n.is_focused())
}

#[test]
fn f6_moves_the_focus_between_the_panes() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    h.state_mut().settings.show_queue = true;
    open_dropped(&mut h, sample_png(dir.path(), "panes.png"));
    let photo = (tr().viewer.a11y_name)("panes.png");
    let f6 = |h: &mut Harness<'_, ApsophosApp>, modifiers| {
        h.key_press_modifiers(modifiers, Key::F6);
        h.run_steps(3);
    };
    f6(&mut h, egui::Modifiers::NONE);
    assert!(focused(&h, Role::Button, tr().queue.add_files), "F6: the queue first");
    f6(&mut h, egui::Modifiers::NONE);
    assert!(focused(&h, Role::Image, &photo), "then the photo");
    // the arrows move the view, and the focus stays on the photo
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    assert!(focused(&h, Role::Image, &photo), "an arrow took the focus away");
    f6(&mut h, egui::Modifiers::NONE);
    assert!(h.get_by_label(tr().params.light).is_focused(), "then the settings");
    f6(&mut h, egui::Modifiers::SHIFT);
    assert!(focused(&h, Role::Image, &photo), "Shift+F6 goes back");
    // a hidden queue is skipped
    h.state_mut().settings.show_queue = false;
    h.run_steps(3);
    f6(&mut h, egui::Modifiers::SHIFT);
    assert!(h.get_by_label(tr().params.light).is_focused(), "round to the settings");
    f6(&mut h, egui::Modifiers::NONE);
    assert!(focused(&h, Role::Image, &photo), "round to the photo, past the hidden queue");
}

#[test]
fn the_view_menu_moves_the_focus_too() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, tr().menu.view).click();
    h.run_steps(3);
    // the menu entry shows its shortcut
    h.get_by_label(&format!("{} F6", tr().menu.next_pane)).click();
    h.run_steps(3);
    // no image: the photo pane's control is the open button
    assert!(focused(&h, Role::Button, tr().central.open_image));
}

/// A picture of the window, drawn with wgpu, for looking at the interface:
/// `cargo test -p apsophos --release screenshot -- --ignored` writes
/// `target/screenshots/window.png`. Not run by default: it needs a GPU.
#[test]
#[ignore = "needs a GPU; run by hand to look at the interface"]
fn screenshot() {
    let dir = tempfile::tempdir().unwrap();
    let photo = sample_png(dir.path(), "photo.png");
    let other = sample_png(dir.path(), "other.png");
    let mut h = Harness::builder().with_size(SIZE).wgpu().build_eframe(|cc| ApsophosApp::new(cc, Vec::new()));
    h.state_mut().settings.show_queue = true;
    h.state_mut().add_to_queue(vec![photo.clone(), other]);
    step_until(&mut h, "the queue to fill", |app| app.queue.items.len() == 2);
    open_dropped(&mut h, photo);
    h.state_mut().windows.preferences = true;
    h.run_steps(4);
    let image = h.render().unwrap();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    image.save(out.join("window.png")).unwrap();
}
