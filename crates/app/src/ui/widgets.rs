//! The pieces every view draws the same way: the primary button, dialog
//! windows, check boxes, icon buttons, list items. Built on the tokens of
//! [`crate::theme`], with the shapes of the other desktop programs of this
//! family.

use eframe::egui::{self, accesskit::Role, Align2, Button, Color32, Response, RichText, Sense, Stroke, Ui, WidgetInfo};

use crate::theme::{self, Tokens};

/// The one main action of a view: accent fill.
pub fn primary_button(ui: &Ui, text: impl Into<String>) -> Button<'static> {
    let t = Tokens::current(ui);
    Button::new(RichText::new(text).color(t.on_accent)).fill(t.accent).stroke(Stroke::NONE)
}

/// Adds a button as wide as the panel, with its text centred (egui places a
/// button's text as the layout around it does).
pub fn wide_button(ui: &mut Ui, button: Button<'_>) -> Response {
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| ui.add(button)).inner
}

/// A secondary window (Preferences, Export settings, About) centred on the
/// main one the first time it opens.
pub fn dialog(ctx: &egui::Context, title: impl Into<egui::WidgetText>) -> egui::Window<'static> {
    egui::Window::new(title).pivot(Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false)
}

/// A check box whose ticked box is filled with the accent, as in Windows 11
/// (egui only draws the tick).
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<egui::WidgetText>) -> Response {
    let resp = ui.checkbox(checked, text);
    if *checked {
        let t = Tokens::current(ui);
        let size = ui.spacing().icon_width;
        let center = egui::pos2(resp.rect.left() + size * 0.5, resp.rect.center().y);
        let rect = egui::Rect::from_center_size(center, egui::Vec2::splat(size));
        // the corners of the box, as egui caps them
        let radius = f32::from(theme::RADIUS).min((0.3 * size).round());
        let fill = if ui.is_enabled() { t.accent } else { t.stroke };
        ui.painter().rect_filled(rect, radius, fill);
        let inner = rect.shrink(rect.width() * 0.25);
        let points = vec![
            egui::pos2(inner.left(), inner.center().y),
            egui::pos2(inner.left() + inner.width() * 0.4, inner.bottom()),
            egui::pos2(inner.right(), inner.top()),
        ];
        ui.painter().add(egui::Shape::line(points, Stroke::new(1.5, t.on_accent)));
    }
    resp
}

/// A quiet button with one icon (close, remove): no border or fill until
/// hovered. `hint` names the action for the tooltip and screen readers.
pub fn icon_button(ui: &mut Ui, icon: &str, hint: impl Into<String>) -> Response {
    let hint = hint.into();
    let button = Button::new(RichText::new(icon).size(theme::FONT_SMALL)).frame_when_inactive(false).small();
    ui.add(button).on_hover_text(&hint).accessible_name(hint)
}

/// An item of a list as wide as the list: an optional status icon, its
/// title and an optional weak second line, selected and hovered as one,
/// with the accent mark of Windows 11 lists. Screen readers hear the title
/// and the second line, not the icon.
pub fn list_item(
    ui: &mut Ui,
    selected: bool,
    icon: Option<(&str, Color32)>,
    title: &str,
    detail: Option<&str>,
) -> Response {
    let t = Tokens::current(ui);
    let pad = egui::vec2(theme::SPACE_S, theme::SPACE_XS);
    let wrap = Some(egui::TextWrapMode::Truncate);
    let icon_galley = icon.map(|(i, color)| {
        let text = RichText::new(i).size(theme::FONT_BODY).color(color);
        egui::WidgetText::from(text).into_galley(ui, wrap, f32::INFINITY, egui::TextStyle::Body)
    });
    let icon_w = icon_galley.as_ref().map_or(0.0, |g| g.size().x + theme::SPACE_S);
    let width = ui.available_width() - 2.0 * pad.x - icon_w;
    let title_galley =
        egui::WidgetText::from(RichText::new(title).color(t.text)).into_galley(ui, wrap, width, egui::TextStyle::Body);
    let detail_galley = detail.map(|d| {
        let text = RichText::new(d).size(theme::FONT_SMALL).color(t.text_weak);
        egui::WidgetText::from(text).into_galley(ui, wrap, width, egui::TextStyle::Small)
    });
    let text_height = title_galley.size().y + detail_galley.as_ref().map_or(0.0, |g| g.size().y);
    let height = (text_height + 2.0 * pad.y).max(ui.spacing().interact_size.y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    let name = match detail {
        Some(d) => format!("{title}, {d}"),
        None => title.to_owned(),
    };
    resp.widget_info(|| WidgetInfo::selected(Role::ListItem, true, selected, &name));
    if ui.is_rect_visible(rect) {
        let fill = if selected { Some(t.selected) } else { resp.hovered().then_some(t.control_hover) };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, theme::RADIUS, fill);
        }
        if selected {
            let mark = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 1.5, rect.center().y),
                egui::vec2(3.0, (height * 0.5).min(16.0)),
            );
            ui.painter().rect_filled(mark, 1.5, t.accent);
        }
        let left = rect.left() + pad.x;
        if let Some(g) = icon_galley {
            let at = egui::pos2(left, rect.center().y - g.size().y / 2.0);
            ui.painter().galley(at, g, t.text);
        }
        let top = egui::pos2(left + icon_w, rect.center().y - text_height / 2.0);
        let below = top + egui::vec2(0.0, title_galley.size().y);
        ui.painter().galley(top, title_galley, t.text);
        if let Some(g) = detail_galley {
            ui.painter().galley(below, g, t.text_weak);
        }
    }
    resp
}
