//! Viewer: zoom, pan, before/after comparison, navigator.
//!
//! The image is shown as 1024 px display tiles (separate textures), so that:
//! * no texture exceeds GPU limits, even for huge images;
//! * when a network tile arrives or the parameters change, only the affected
//!   visible display tiles are rebuilt, within a per-frame pixel budget: the
//!   interface stays fluid.
//!
//! The before/after divider is always shown; holding `\` or Space shows the
//! whole original. The "after" side shows the result or, to judge it, the
//! noise that was removed. Photographs go through the colour management of
//! [`DisplayTransform`]; the removed-noise view does not.

use std::sync::Arc;

use denoise_core::{num, Image, Mixer, Params, PixelRect, TilePlan};
use eframe::egui::{
    self, pos2, vec2, Color32, ColorImage, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, TextureFilter, TextureHandle,
    TextureOptions, TextureWrapMode, Vec2,
};
use rayon::prelude::*;

use crate::color::{unconverted_row, DisplayTransform};
use crate::document::Document;
use crate::format::{self, file_name};
use crate::inspect::{removed_noise, NOISE_GAIN};
use crate::text::tr;
use crate::theme::*;

const DISPLAY_TILE: usize = 1024;
/// Maximum pixels rebuilt in one frame.
const REBUILD_BUDGET_PX: usize = 6_000_000;
const THUMB_PX: usize = 180;
/// Distance (px) within which the comparison divider can be grabbed.
const SPLIT_GRAB_PX: f32 = 8.0;
/// Maximum zoom: 3200 % in physical pixels.
const MAX_ZOOM_PHYSICAL: f32 = 32.0;
const ZOOM_ANIM_S: f32 = 0.2;

/// Position in image coordinates (pixels, with fractions).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ImagePos {
    pub x: f32,
    pub y: f32,
}

fn texture_options() -> TextureOptions {
    TextureOptions {
        // sharp pixels above 100 %, smooth when zoomed out
        magnification: TextureFilter::Nearest,
        minification: TextureFilter::Linear,
        wrap_mode: TextureWrapMode::ClampToEdge,
        mipmap_mode: Some(TextureFilter::Linear),
    }
}

/// What the "after" side of the viewer shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResultView {
    #[default]
    Denoised,
    /// What the network took out, amplified around mid grey.
    RemovedNoise,
}

impl ResultView {
    fn label(self) -> String {
        match self {
            ResultView::Denoised => tr().viewer.after.into(),
            ResultView::RemovedNoise => (tr().viewer.removed_noise)(NOISE_GAIN),
        }
    }
}

/// What a display tile is rendered from.
#[derive(Clone, Copy)]
enum Render<'m> {
    Original,
    Result(&'m Mixer),
    Noise(&'m Mixer),
}

/// References to the data needed for drawing.
struct Sources<'a> {
    original: &'a Image,
    raw: &'a Image,
    plan: &'a TilePlan,
    done: &'a [bool],
}

impl Sources<'_> {
    fn pixel_done(&self, x: usize, y: usize) -> bool {
        self.plan.index_at(x, y).is_some_and(|i| self.done.get(i).copied().unwrap_or(false))
    }

    fn any_done_in(&self, r: PixelRect) -> bool {
        let core = self.plan.core_px();
        if r.is_empty() {
            return false;
        }
        (r.y / core..=(r.bottom() - 1) / core)
            .any(|ty| (r.x / core..=(r.right() - 1) / core).any(|tx| self.pixel_done(tx * core, ty * core)))
    }

    /// 8-bit RGBA pixels of the region, for the display: where the network
    /// has not finished, the original (or mid grey for the removed noise).
    fn render(&self, r: PixelRect, how: Render<'_>, transform: &DisplayTransform) -> ColorImage {
        let mut rgba = vec![0u8; r.area() * 4];
        rgba.par_chunks_exact_mut(r.width * 4).enumerate().for_each(|(dy, out)| {
            let y = r.y + dy;
            let (o, n) = (self.original.row(y), self.raw.row(y));
            let row: Vec<[f32; 3]> = (r.x..r.x + r.width)
                .map(|x| {
                    let i = x * 3;
                    let (Some(po), Some(pn)) = (o.get(i..i + 3), n.get(i..i + 3)) else { return [0.0; 3] };
                    let (po, pn) = ([po[0], po[1], po[2]], [pn[0], pn[1], pn[2]]);
                    let done = self.pixel_done(x, y);
                    match how {
                        Render::Original => po,
                        Render::Result(m) if done => m.apply(po, pn),
                        Render::Result(_) => po,
                        Render::Noise(m) if done => removed_noise(po, m.apply(po, pn)),
                        Render::Noise(_) => [0.5; 3],
                    }
                })
                .collect();
            match how {
                Render::Noise(_) => unconverted_row(&row, out),
                _ => transform.row(&row, out),
            }
        });
        ColorImage::from_rgba_unmultiplied([r.width, r.height], &rgba)
    }
}

/// Textures of the open image.
pub struct DisplayCache {
    width: usize,
    height: usize,
    cols: usize,
    original: Vec<Option<TextureHandle>>,
    result: Vec<Option<TextureHandle>>,
    stale: Vec<bool>,
    rendered: Option<(Params, ResultView)>,
    thumb: Option<TextureHandle>,
    transform: Arc<DisplayTransform>,
}

impl DisplayCache {
    pub fn new(width: usize, height: usize, transform: Arc<DisplayTransform>) -> Self {
        let cols = width.div_ceil(DISPLAY_TILE).max(1);
        let n = cols * height.div_ceil(DISPLAY_TILE).max(1);
        Self {
            width,
            height,
            cols,
            original: vec![None; n],
            result: vec![None; n],
            stale: vec![true; n],
            rendered: None,
            thumb: None,
            transform,
        }
    }

    pub fn transform(&self) -> &DisplayTransform {
        &self.transform
    }

    fn tile_rect(&self, i: usize) -> PixelRect {
        let (x, y) = ((i % self.cols) * DISPLAY_TILE, (i / self.cols) * DISPLAY_TILE);
        PixelRect::new(x, y, DISPLAY_TILE.min(self.width - x), DISPLAY_TILE.min(self.height - y))
    }

    fn tiles_in(&self, r: PixelRect) -> Vec<usize> {
        if r.is_empty() {
            return Vec::new();
        }
        let (c0, c1) = (r.x / DISPLAY_TILE, (r.right() - 1) / DISPLAY_TILE);
        let (r0, r1) = (r.y / DISPLAY_TILE, (r.bottom() - 1) / DISPLAY_TILE);
        (r0..=r1)
            .flat_map(|row| (c0..=c1).map(move |c| row * self.cols + c))
            .filter(|&i| i < self.stale.len())
            .collect()
    }

    /// The result in `rect` changed.
    pub fn invalidate(&mut self, rect: PixelRect) {
        for i in self.tiles_in(rect) {
            self.stale[i] = true;
        }
    }

    /// Prepares the textures of the `visible` display tiles; returns `true` if
    /// work is left for the next frames.
    fn ensure(
        &mut self,
        ctx: &egui::Context,
        src: &Sources<'_>,
        params: &Params,
        view: ResultView,
        visible: &[usize],
    ) -> bool {
        if self.rendered != Some((*params, view)) {
            self.stale.iter_mut().for_each(|s| *s = true);
            self.rendered = Some((*params, view));
        }
        if self.thumb.is_none() {
            let thumb = thumbnail(src.original, &self.transform);
            self.thumb = Some(ctx.load_texture("thumbnail", thumb, texture_options()));
        }
        let mixer = Mixer::new(params);
        let result = match view {
            ResultView::Denoised => Render::Result(&mixer),
            ResultView::RemovedNoise => Render::Noise(&mixer),
        };
        let mut budget = REBUILD_BUDGET_PX;
        let mut pending = false;
        for &i in visible {
            let r = self.tile_rect(i);
            let needs_original = self.original[i].is_none();
            let needs_result = self.stale[i];
            if !needs_original && !needs_result {
                continue;
            }
            // at least one display tile per frame, even beyond the budget
            if budget < r.area() && budget < REBUILD_BUDGET_PX {
                pending = true;
                continue;
            }
            if needs_original {
                let img = src.render(r, Render::Original, &self.transform);
                self.original[i] = Some(ctx.load_texture(format!("orig-{i}"), img, texture_options()));
                budget = budget.saturating_sub(r.area());
            }
            if needs_result {
                if src.any_done_in(r) {
                    let img = src.render(r, result, &self.transform);
                    match &mut self.result[i] {
                        Some(tex) => tex.set(img, texture_options()),
                        slot => *slot = Some(ctx.load_texture(format!("dn-{i}"), img, texture_options())),
                    }
                    budget = budget.saturating_sub(r.area());
                } else {
                    self.result[i] = None;
                }
                self.stale[i] = false;
            }
        }
        pending
    }
}

fn thumbnail(img: &Image, transform: &DisplayTransform) -> ColorImage {
    let (w, h) = (img.width(), img.height());
    let scale = THUMB_PX as f32 / w.max(h) as f32;
    let (tw, th) = (num::round_to_usize(w as f32 * scale, w).max(1), num::round_to_usize(h as f32 * scale, h).max(1));
    let mut rgba = vec![0u8; tw * th * 4];
    for (y, out) in rgba.chunks_exact_mut(tw * 4).enumerate() {
        let sy = (y * h / th).min(h - 1);
        let row: Vec<[f32; 3]> = (0..tw).map(|x| img.pixel((x * w / tw).min(w - 1), sy).unwrap_or_default()).collect();
        transform.row(&row, out);
    }
    ColorImage::from_rgba_unmultiplied([tw, th], &rgba)
}

/// View state: zoom, centre, comparison divider.
pub struct ViewState {
    /// Screen points per image pixel (animated value).
    zoom: f32,
    target: f32,
    center: ImagePos,
    /// Screen point that must stay over the same image point while the zoom
    /// animates.
    anchor: Option<(Pos2, ImagePos)>,
    fit_requested: bool,
    /// Divider position, fraction of the view width (0..1).
    pub split: f32,
    dragging_split: bool,
    canvas: Rect,
    image: Vec2,
    ppp: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            target: 1.0,
            center: ImagePos::default(),
            anchor: None,
            fit_requested: true,
            split: 0.5,
            dragging_split: false,
            canvas: Rect::NOTHING,
            image: Vec2::ZERO,
            ppp: 1.0,
        }
    }
}

impl ViewState {
    /// Centre of the view on screen.
    pub fn canvas_center(&self) -> Pos2 {
        self.canvas.center()
    }

    pub fn request_fit(&mut self) {
        self.fit_requested = true;
    }

    /// Centre of the view, so the network starts from what is being looked at.
    pub fn center(&self) -> ImagePos {
        self.center
    }

    /// Zoom in percent of physical pixels (100 % = one image pixel per screen pixel).
    pub fn zoom_percent(&self) -> f32 {
        self.target * self.ppp * 100.0
    }

    pub fn set_zoom_percent(&mut self, pct: f32) {
        let z = pct / 100.0 / self.ppp;
        self.zoom_to(z, self.canvas.center());
    }

    /// 100 %, centred on `at` (or on the centre of the view).
    pub fn actual_size(&mut self, at: Option<Pos2>) {
        let z = 1.0 / self.ppp;
        self.zoom_to(z, at.unwrap_or(self.canvas.center()));
    }

    pub fn zoom_by(&mut self, factor: f32, at: Pos2) {
        self.zoom_to(self.target * factor, at);
    }

    /// Moves the view by `delta` screen points.
    pub fn pan(&mut self, delta: Vec2) {
        self.anchor = None;
        self.center.x -= delta.x / self.zoom;
        self.center.y -= delta.y / self.zoom;
        self.clamp_center();
    }

    fn fit_zoom(&self) -> f32 {
        if self.image.x <= 0.0 || self.image.y <= 0.0 || self.canvas.width() <= 0.0 {
            return 1.0;
        }
        let z = (self.canvas.width() / self.image.x).min(self.canvas.height() / self.image.y) * 0.97;
        z.min(1.0 / self.ppp) // small images are not enlarged beyond 100 %
    }

    fn zoom_to(&mut self, z: f32, at: Pos2) {
        let min = self.fit_zoom().min(0.01 / self.ppp);
        self.target = z.clamp(min, MAX_ZOOM_PHYSICAL / self.ppp);
        self.anchor = Some((at, self.to_image(at)));
    }

    fn to_screen(&self, p: ImagePos) -> Pos2 {
        self.canvas.center() + vec2((p.x - self.center.x) * self.zoom, (p.y - self.center.y) * self.zoom)
    }

    fn to_image(&self, s: Pos2) -> ImagePos {
        let d = s - self.canvas.center();
        ImagePos { x: self.center.x + d.x / self.zoom, y: self.center.y + d.y / self.zoom }
    }

    fn clamp_center(&mut self) {
        self.center.x = self.center.x.clamp(0.0, self.image.x);
        self.center.y = self.center.y.clamp(0.0, self.image.y);
    }

    fn screen_rect(&self, r: PixelRect) -> Rect {
        Rect::from_min_max(
            self.to_screen(ImagePos { x: r.x as f32, y: r.y as f32 }),
            self.to_screen(ImagePos { x: r.right() as f32, y: r.bottom() as f32 }),
        )
    }

    /// The image's width and height in pixels.
    fn image_size(&self) -> (usize, usize) {
        (num::round_to_usize(self.image.x, usize::MAX), num::round_to_usize(self.image.y, usize::MAX))
    }

    /// Image pixels visible in the view.
    pub fn visible_pixels(&self) -> PixelRect {
        let a = self.to_image(self.canvas.min);
        let b = self.to_image(self.canvas.max);
        let (w, h) = self.image_size();
        let (x0, y0) = (num::floor_to_usize(a.x, w), num::floor_to_usize(a.y, h));
        let (x1, y1) = (num::ceil_to_usize(b.x, w), num::ceil_to_usize(b.y, h));
        PixelRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }

    /// Updates geometry and animation at the start of the frame.
    fn update(&mut self, ctx: &egui::Context, canvas: Rect, image: Vec2, reduce_motion: bool) {
        self.canvas = canvas;
        self.image = image;
        self.ppp = ctx.pixels_per_point();
        if self.fit_requested && canvas.width() > 1.0 {
            self.fit_requested = false;
            self.target = self.fit_zoom();
            self.anchor = Some((canvas.center(), ImagePos { x: image.x / 2.0, y: image.y / 2.0 }));
            // no animation on open: start already fitted
            self.zoom = ctx.animate_value_with_time(egui::Id::unique("zoom"), self.target, 0.0);
        }
        let duration = if reduce_motion { 0.0 } else { ZOOM_ANIM_S };
        self.zoom = ctx.animate_value_with_time(egui::Id::unique("zoom"), self.target, duration).max(1e-4);
        if let Some((screen, img)) = self.anchor {
            let d = screen - canvas.center();
            self.center = ImagePos { x: img.x - d.x / self.zoom, y: img.y - d.y / self.zoom };
            if (self.zoom - self.target).abs() < f32::EPSILON {
                self.anchor = None;
            }
        }
        self.clamp_center();
    }
}

/// Drawing options decided by the application.
pub struct ViewerInput<'a> {
    pub tokens: &'a Tokens,
    pub params: &'a Params,
    pub result_view: ResultView,
    pub show_original: bool,
    pub reduce_motion: bool,
    /// Tile the network is processing, to highlight.
    pub working_on: Option<PixelRect>,
}

/// Name and state of the image area for screen readers: it is drawn by
/// hand, so egui knows nothing about it otherwise.
fn describe(response: &egui::Response, view: &ViewState, doc: &Document, input: &ViewerInput<'_>) {
    let tv = &tr().viewer;
    let name = (tv.a11y_name)(&file_name(&doc.path));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &name));
    let mode = match (input.show_original, input.result_view) {
        (true, _) => tv.mode_original,
        (false, ResultView::Denoised) => tv.mode_split,
        (false, ResultView::RemovedNoise) => tv.mode_noise,
    };
    let state = (tv.a11y_state)(
        &format::int(num::round_to_usize(view.zoom_percent(), usize::MAX)),
        mode,
        &format::int(doc.done_count()),
        &format::int(doc.plan.len()),
    );
    response.ctx.accesskit_node_builder(response.id, |node| node.set_description(state));
}

pub struct ViewerOutput {
    /// Pixel under the cursor.
    pub hover: Option<(usize, usize)>,
    /// The image area, which can hold the keyboard focus (F6).
    pub response: egui::Response,
}

/// Draws the image and handles mouse and wheel.
pub fn show(ui: &mut egui::Ui, view: &mut ViewState, doc: &mut Document, input: &ViewerInput<'_>) -> ViewerOutput {
    let t = input.tokens;
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    describe(&response, view, doc, input);
    let canvas = response.rect;
    let ctx = ui.ctx().clone();
    let size = vec2(doc.width() as f32, doc.height() as f32);
    view.update(&ctx, canvas, size, input.reduce_motion);
    let split_on = !input.show_original;
    let split_x = canvas.left() + canvas.width() * view.split;

    // --- input ---
    let pointer = response.hover_pos();
    if let Some(pos) = pointer {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (scroll * 0.0045).exp() * pinch;
        if (factor - 1.0).abs() > 1e-4 {
            view.zoom_by(factor, pos);
        }
        let near_split = split_on && (pos.x - split_x).abs() <= SPLIT_GRAB_PX;
        if near_split || view.dragging_split {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        } else if response.dragged() {
            ctx.set_cursor_icon(CursorIcon::Grabbing);
        } else {
            ctx.set_cursor_icon(CursorIcon::Grab);
        }
        if response.drag_started() && near_split {
            view.dragging_split = true;
        }
    }
    if response.dragged() {
        if view.dragging_split {
            if let Some(p) = response.interact_pointer_pos() {
                view.split = ((p.x - canvas.left()) / canvas.width().max(1.0)).clamp(0.02, 0.98);
            }
        } else {
            view.pan(response.drag_delta());
        }
    }
    if response.drag_stopped() {
        view.dragging_split = false;
    }
    if response.double_clicked() {
        if (view.zoom_percent() - 100.0).abs() < 1.0 {
            view.request_fit();
        } else {
            view.actual_size(pointer);
        }
    }

    // --- texture ---
    let visible = view.visible_pixels();
    let (original, raw, plan, done, display) = doc.parts();
    let src = Sources { original, raw, plan, done };
    let tiles = display.tiles_in(visible);
    if display.ensure(&ctx, &src, input.params, input.result_view, &tiles) {
        ctx.request_repaint();
    }

    // --- drawing ---
    painter.rect_filled(canvas, 0.0, t.canvas);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    for &i in &tiles {
        if let Some(tex) = &display.original[i] {
            painter.image(tex.id(), view.screen_rect(display.tile_rect(i)), uv, Color32::WHITE);
        }
    }
    if !input.show_original {
        let mut clip = canvas;
        if split_on {
            clip.min.x = split_x;
        }
        let p = painter.with_clip_rect(clip);
        for &i in &tiles {
            if let Some(tex) = &display.result[i] {
                p.image(tex.id(), view.screen_rect(display.tile_rect(i)), uv, Color32::WHITE);
            }
        }
    }

    if let Some(r) = input.working_on {
        let rect = view.screen_rect(r).intersect(canvas);
        if rect.is_positive() {
            let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.5, t.accent), 6.0, 4.0));
        }
    }

    if split_on {
        draw_split(&painter, canvas, split_x, &input.result_view.label(), t);
    } else if input.show_original {
        badge(
            &painter,
            pos2(canvas.center().x, canvas.top() + SPACE_M),
            egui::Align2::CENTER_TOP,
            tr().viewer.original,
            t,
        );
    }

    if let Some(thumb) = &display.thumb {
        minimap(ui, view, thumb, canvas, t);
    }

    if response.has_focus() {
        // the arrows move the view, not the focus
        let arrows = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
        ctx.memory_mut(|m| m.set_focus_lock_filter(response.id, arrows));
        painter.rect_stroke(canvas.shrink(1.0), 0.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }

    let hover = pointer.and_then(|p| {
        let ip = view.to_image(p);
        (ip.x >= 0.0 && ip.y >= 0.0 && ip.x < size.x && ip.y < size.y)
            .then_some((num::floor_to_usize(ip.x, usize::MAX), num::floor_to_usize(ip.y, usize::MAX)))
    });
    ViewerOutput { hover, response }
}

/// Small label over the image.
fn badge(painter: &egui::Painter, at: Pos2, align: egui::Align2, text: &str, t: &Tokens) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(FONT_SMALL), t.text);
    let rect = align.anchor_size(at, galley.size() + vec2(SPACE_S * 2.0, SPACE_XS * 2.0));
    painter.rect_filled(rect, f32::from(RADIUS), t.surface.gamma_multiply(0.92));
    painter.galley(rect.min + vec2(SPACE_S, SPACE_XS), galley, t.text);
}

fn draw_split(painter: &egui::Painter, canvas: Rect, x: f32, after: &str, t: &Tokens) {
    let top = pos2(x, canvas.top());
    let bottom = pos2(x, canvas.bottom());
    painter.line_segment([top, bottom], Stroke::new(3.0, Color32::from_black_alpha(90)));
    painter.line_segment([top, bottom], Stroke::new(1.5, Color32::WHITE));
    let c = pos2(x, canvas.center().y);
    painter.circle(c, 9.0, t.surface, Stroke::new(1.5, Color32::WHITE));
    let arrow = Stroke::new(1.5, t.text);
    painter.line_segment([c + vec2(-4.0, -3.0), c + vec2(-6.5, 0.0)], arrow);
    painter.line_segment([c + vec2(-6.5, 0.0), c + vec2(-4.0, 3.0)], arrow);
    painter.line_segment([c + vec2(4.0, -3.0), c + vec2(6.5, 0.0)], arrow);
    painter.line_segment([c + vec2(6.5, 0.0), c + vec2(4.0, 3.0)], arrow);
    let y = canvas.top() + SPACE_M;
    badge(painter, pos2(x - SPACE_S, y), egui::Align2::RIGHT_TOP, tr().viewer.before, t);
    badge(painter, pos2(x + SPACE_S, y), egui::Align2::LEFT_TOP, after, t);
}

/// Navigator: shown only when the image does not fit entirely in the view.
fn minimap(ui: &mut egui::Ui, view: &mut ViewState, thumb: &TextureHandle, canvas: Rect, t: &Tokens) {
    let (w, h) = view.image_size();
    let whole = view.screen_rect(PixelRect::new(0, 0, w, h));
    if canvas.expand(1.0).contains_rect(whole) {
        return;
    }
    let ts = thumb.size_vec2();
    let scale = (THUMB_PX as f32 / ts.x.max(ts.y)).min(1.0);
    let size = ts * scale;
    let rect = Rect::from_min_size(canvas.right_bottom() - size - Vec2::splat(SPACE_M), size);
    let painter = ui.painter_at(canvas);
    painter.rect_filled(rect.expand(3.0), f32::from(RADIUS_WINDOW), t.surface);
    painter.image(thumb.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    let to_mini = |p: ImagePos| rect.min + vec2(p.x / view.image.x * size.x, p.y / view.image.y * size.y);
    let a = view.to_image(canvas.min);
    let b = view.to_image(canvas.max);
    let vp = Rect::from_min_max(to_mini(a), to_mini(b)).intersect(rect);
    painter.rect_stroke(vp, 0.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Middle);

    let resp = ui
        .interact(rect, ui.scope_id().with("navigator"), Sense::click_and_drag())
        .on_hover_text(tr().viewer.navigator_tip);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, tr().viewer.navigator));
    resp.ctx.accesskit_node_builder(resp.id, |node| node.set_description(tr().viewer.navigator_tip));
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let rel = (p - rect.min) / size;
        view.anchor = None;
        view.center = ImagePos { x: rel.x.clamp(0.0, 1.0) * view.image.x, y: rel.y.clamp(0.0, 1.0) * view.image.y };
    }
}
