//! Shared widgets, so every panel lines up the same way: section headers,
//! fixed-height rows with aligned columns, key hints, copy buttons, pills
//! and empty states. Sizes come from one spacing scale.

use artalde_core::theme::Theme;
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, UiBuilder};

use crate::fonts;
use crate::style::{self, tint};

/// The spacing scale.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
/// One list row.
pub const ROW: f32 = 30.0;
/// A row with a second line.
pub const ROW2: f32 = 44.0;

/// A column of a [`row`]: fixed width, or a share of what is left.
#[derive(Clone, Copy, Debug)]
pub enum Col {
    Px(f32),
    Flex,
}

/// `SECTION · 3` with right-aligned extras; returns whether it was clicked.
pub fn section(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    count: Option<usize>,
    right: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let r = ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        let mut label = RichText::new(title.to_uppercase())
            .font(fonts::semibold(11.0))
            .color(style::c(theme.dim));
        label = label.extra_letter_spacing(0.6);
        ui.label(label);
        if let Some(n) = count {
            ui.label(
                RichText::new(n.to_string())
                    .size(11.0)
                    .color(style::c(theme.unknown)),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(2.0);
    r.response
}

/// A clickable, full-width row of fixed `height`, its columns laid out
/// left to right and clipped; `fill` paints each column.
pub fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash,
    height: f32,
    selected: bool,
    cols: &[Col],
    mut fill: impl FnMut(usize, &mut egui::Ui),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = id;
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            tint(style::c(theme.surface), style::c(theme.accent), 0.16),
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            tint(style::c(theme.surface), style::c(theme.text), 0.05),
        );
    }
    let gap = S2;
    let inner = rect.shrink2(egui::vec2(S2, 0.0));
    let fixed: f32 = cols
        .iter()
        .map(|c| if let Col::Px(w) = c { *w } else { 0.0 })
        .sum();
    let flex_n = cols
        .iter()
        .filter(|c| matches!(c, Col::Flex))
        .count()
        .max(1) as f32;
    let gaps = gap * cols.len().saturating_sub(1) as f32;
    let flex_w = ((inner.width() - fixed - gaps) / flex_n).max(0.0);
    let mut x = inner.min.x;
    for (i, c) in cols.iter().enumerate() {
        let w = match c {
            Col::Px(w) => *w,
            Col::Flex => flex_w,
        };
        let r =
            egui::Rect::from_min_size(egui::pos2(x, inner.min.y), egui::vec2(w, inner.height()));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(r)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(r.intersect(ui.clip_rect()));
        fill(i, &mut child);
        x += w + gap;
    }
    resp
}

/// A key hint chip: `⌘K`.
pub fn key(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), style::c(theme.text), 0.06))
        .stroke(Stroke::new(1.0, style::c(theme.border)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(keys)
                    .monospace()
                    .size(10.5)
                    .color(style::c(theme.dim)),
            )
        });
}

/// A label then its key hint, e.g. `j/k move`.
pub fn hint(ui: &mut egui::Ui, theme: &Theme, keys: &str, what: &str) {
    key(ui, theme, keys);
    ui.label(RichText::new(what).size(11.0).color(style::c(theme.dim)));
    ui.add_space(S1);
}

/// A rounded pill in `color`.
pub fn pill(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), color, 0.18))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(7, 0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(color))
        })
        .response
}

/// A small coloured dot.
pub fn dot(ui: &mut egui::Ui, color: Color32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(radius * 2.0 + 4.0, radius * 2.0 + 4.0),
        Sense::hover(),
    );
    ui.painter().circle_filled(rect.center(), radius, color);
}

/// A flat button: text, optional key hint, quiet until hovered.
pub fn button(ui: &mut egui::Ui, theme: &Theme, text: &str, keys: Option<&str>) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(13.0),
        style::c(theme.text),
    );
    let key_w = keys.map_or(0.0, |k| k.chars().count() as f32 * 7.0 + 14.0);
    let size = egui::vec2(galley.size().x + 20.0 + key_w, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.is_pointer_button_down_on() {
        tint(style::c(theme.raised), style::c(theme.accent), 0.25)
    } else if resp.hovered() {
        tint(style::c(theme.raised), style::c(theme.text), 0.08)
    } else {
        style::c(theme.raised)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, style::c(theme.border)),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(rect.min.x + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        style::c(theme.text),
    );
    if let Some(k) = keys {
        ui.painter().text(
            egui::pos2(rect.max.x - 9.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            k,
            egui::FontId::monospace(10.5),
            style::c(theme.dim),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The accented variant of [`button`] for the one primary action.
pub fn primary(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        fonts::semibold(13.0),
        style::c(theme.background),
    );
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(galley.size().x + 22.0, 26.0), Sense::click());
    let fill = if resp.hovered() {
        tint(color, Color32::WHITE, 0.12)
    } else {
        color
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter().galley(
        egui::pos2(rect.min.x + 11.0, rect.center().y - galley.size().y / 2.0),
        galley,
        style::c(theme.background),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `label  value  ⧉`: a key/value line whose value copies on click.
pub fn kv(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str, label_w: f32) -> bool {
    let mut copied = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [label_w, 18.0],
            egui::Label::new(RichText::new(label).size(11.5).color(style::c(theme.dim)))
                .halign(egui::Align::Min),
        );
        let r = ui
            .add(
                egui::Label::new(RichText::new(value).monospace().size(12.0))
                    .truncate()
                    .sense(Sense::click()),
            )
            .on_hover_text(format!("{value}\n(click to copy)"));
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        }
        if r.clicked() {
            ui.ctx().copy_text(value.to_owned());
            copied = true;
        }
    });
    copied
}

/// Centered title and hint for an empty list.
pub fn empty(ui: &mut egui::Ui, theme: &Theme, title: &str, hint: &str) {
    ui.add_space(S3 * 2.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(title)
                .font(fonts::semibold(14.0))
                .color(style::c(theme.text)),
        );
        ui.add_space(S1);
        ui.label(RichText::new(hint).size(12.0).color(style::c(theme.dim)));
    });
    ui.add_space(S3 * 2.0);
}

/// Truncating label in `color`.
pub fn text(ui: &mut egui::Ui, s: &str, size: f32, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(s).size(size).color(color)).truncate())
}

/// Truncating monospace label.
pub fn mono(ui: &mut egui::Ui, s: &str, size: f32, color: Color32) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(s).monospace().size(size).color(color)).truncate())
}

/// Right-aligned label filling its column.
pub fn right(ui: &mut egui::Ui, s: &str, size: f32, color: Color32) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(s).size(size).color(color));
    });
}

/// Underlined tabs, Linear-style; returns the index clicked, if any.
pub fn tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: &[(String, Option<usize>)],
    selected: usize,
) -> Option<usize> {
    let mut clicked = None;
    let start = ui.cursor().min;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = S3 + S1;
        for (i, (label, count)) in labels.iter().enumerate() {
            let on = i == selected;
            let color = if on {
                style::c(theme.text)
            } else {
                style::c(theme.dim)
            };
            let r = ui
                .vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        ui.label(
                            RichText::new(label)
                                .font(fonts::semibold(13.0))
                                .color(color),
                        );
                        if let Some(n) = count {
                            ui.label(
                                RichText::new(n.to_string())
                                    .size(11.0)
                                    .color(style::c(theme.unknown)),
                            );
                        }
                    });
                    ui.add_space(5.0);
                })
                .response;
            let r = ui
                .interact(
                    r.rect,
                    ui.scope_id().with(("tab", i, label)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if on {
                let line = egui::Rect::from_min_max(
                    egui::pos2(r.rect.min.x, r.rect.max.y - 2.0),
                    r.rect.max,
                );
                ui.painter()
                    .rect_filled(line, CornerRadius::same(1), style::c(theme.accent));
            }
            if r.clicked() {
                clicked = Some(i);
            }
        }
    });
    let end = ui.cursor().min.y;
    let w = ui.available_width();
    ui.painter().hline(
        start.x..=start.x + w,
        end - ui.spacing().item_spacing.y,
        Stroke::new(1.0, style::c(theme.border)),
    );
    clicked
}
