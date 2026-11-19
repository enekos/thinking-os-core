//! Draw a [`Node`] tree into a rectangle: splits with draggable dividers,
//! tabbed stacks, framed blocks. What a block shows is the caller's job.

use std::collections::HashMap;

use artalde_core::layout::{Axis, Node, Panel, Style, distribute};
use artalde_core::theme::Theme;
use eframe::egui::{self, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, UiBuilder};

use crate::style::{self, RADIUS, tint, token};

const TITLE_HEIGHT: f32 = 24.0;
const TAB_HEIGHT: f32 = 30.0;
const GRAB: f32 = 4.0;

/// Drag sizes and selected tabs, keyed by `<layout>/<node path>`.
#[derive(Debug, Default)]
pub struct LayoutState {
    pub sizes: HashMap<String, f32>,
    pub tabs: HashMap<String, usize>,
}

pub type DrawPanel<'a> = dyn FnMut(&mut egui::Ui, &Panel) + 'a;

pub fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    root: &Node,
    layout: &str,
    theme: &Theme,
    st: &mut LayoutState,
    draw: &mut DrawPanel<'_>,
) {
    node(ui, rect, root, layout.to_owned(), theme, st, draw);
}

fn node(
    ui: &mut egui::Ui,
    rect: Rect,
    n: &Node,
    path: String,
    theme: &Theme,
    st: &mut LayoutState,
    draw: &mut DrawPanel<'_>,
) {
    match n {
        Node::Block { panel, style } => {
            let content = frame(ui, rect, style, theme, style.title.as_deref());
            body(ui, content, panel, &path, draw);
        }
        Node::Stack { style, children } => {
            let inner = frame(ui, rect, style, theme, None);
            let selected = st
                .tabs
                .get(&path)
                .copied()
                .unwrap_or(0)
                .min(children.len().saturating_sub(1));
            let bar = Rect::from_min_size(inner.min, egui::vec2(inner.width(), TAB_HEIGHT));
            let mut x = bar.min.x;
            for (i, child) in children.iter().enumerate() {
                let label = tab_label(child);
                let galley = ui.painter().layout_no_wrap(
                    label.clone(),
                    egui::TextStyle::Button.resolve(ui.style()),
                    style::c(theme.text),
                );
                let w = galley.size().x + 22.0;
                let tab = Rect::from_min_size(
                    egui::pos2(x, bar.min.y + 2.0),
                    egui::vec2(w, TAB_HEIGHT - 6.0),
                );
                let resp = ui.interact(tab, egui::Id::unique((&path, "tab", i)), Sense::click());
                let on = i == selected;
                if on || resp.hovered() {
                    let fill = if on {
                        style::c(theme.raised)
                    } else {
                        tint(style::c(theme.surface), style::c(theme.raised), 0.6)
                    };
                    ui.painter().rect_filled(tab, CornerRadius::same(6), fill);
                }
                let color = if on {
                    style::c(theme.text)
                } else {
                    style::c(theme.dim)
                };
                ui.painter().text(
                    tab.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::TextStyle::Button.resolve(ui.style()),
                    color,
                );
                if resp.clicked() {
                    st.tabs.insert(path.clone(), i);
                }
                x += w + 4.0;
            }
            let rest = Rect::from_min_max(egui::pos2(inner.min.x, bar.max.y + 4.0), inner.max);
            if let Some(child) = children.get(selected) {
                let child_path = format!("{path}/{selected}");
                match child {
                    Node::Block { panel, .. } => body(ui, rest, panel, &child_path, draw),
                    other => node(ui, rest, other, child_path, theme, st, draw),
                }
            }
        }
        Node::Split {
            axis,
            gap,
            children,
        } => {
            let slots: Vec<_> = children.iter().map(|(s, _)| s).collect();
            let overrides: Vec<Option<f32>> = (0..children.len())
                .map(|i| st.sizes.get(&format!("{path}/{i}")).copied())
                .collect();
            let total = match axis {
                Axis::Row => rect.width(),
                Axis::Column => rect.height(),
            };
            let sizes = distribute(total, *gap, &slots, &overrides);
            let mut pos = match axis {
                Axis::Row => rect.min.x,
                Axis::Column => rect.min.y,
            };
            let span = |a: f32, b: f32| match axis {
                Axis::Row => {
                    Rect::from_min_max(egui::pos2(a, rect.min.y), egui::pos2(b, rect.max.y))
                }
                Axis::Column => {
                    Rect::from_min_max(egui::pos2(rect.min.x, a), egui::pos2(rect.max.x, b))
                }
            };
            for (i, ((slot, child), size)) in children.iter().zip(&sizes).enumerate() {
                node(
                    ui,
                    span(pos, pos + size),
                    child,
                    format!("{path}/{i}"),
                    theme,
                    st,
                    draw,
                );
                pos += size;
                let Some((next, _)) = children.get(i + 1) else {
                    break;
                };
                if slot.resizable || next.resizable {
                    let handle = span(pos - GRAB + gap / 2.0, pos + gap / 2.0 + GRAB);
                    let resp = ui.interact(
                        handle,
                        egui::Id::unique((&path, "divider", i)),
                        Sense::drag(),
                    );
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(match axis {
                            Axis::Row => CursorIcon::ResizeHorizontal,
                            Axis::Column => CursorIcon::ResizeVertical,
                        });
                        let line = span(pos + gap / 2.0 - 1.0, pos + gap / 2.0 + 1.0);
                        ui.painter()
                            .rect_filled(line, CornerRadius::ZERO, style::c(theme.accent));
                    }
                    if resp.dragged() {
                        let d = match axis {
                            Axis::Row => resp.drag_delta().x,
                            Axis::Column => resp.drag_delta().y,
                        };
                        if slot.resizable {
                            st.sizes.insert(format!("{path}/{i}"), size + d);
                        } else if let Some(next_size) = sizes.get(i + 1) {
                            st.sizes.insert(format!("{path}/{}", i + 1), next_size - d);
                        }
                    }
                    if resp.double_clicked() {
                        st.sizes.remove(&format!("{path}/{i}"));
                        st.sizes.remove(&format!("{path}/{}", i + 1));
                    }
                }
                pos += gap;
            }
        }
    }
}

fn tab_label(n: &Node) -> String {
    match n {
        Node::Block { panel, style } => style
            .title
            .clone()
            .unwrap_or_else(|| panel.label().to_owned()),
        Node::Stack { style, .. } => style.title.clone().unwrap_or_else(|| "stack".into()),
        Node::Split { .. } => "split".into(),
    }
}

/// Paint a block's frame and title; returns the content rectangle.
fn frame(ui: &egui::Ui, rect: Rect, style: &Style, theme: &Theme, title: Option<&str>) -> Rect {
    let fill = style
        .background
        .as_deref()
        .map_or(style::c(theme.surface), |b| token(theme, b));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(RADIUS), fill);
    if style.border {
        p.rect_stroke(
            rect,
            CornerRadius::same(RADIUS),
            Stroke::new(1.0, style::c(theme.border)),
            StrokeKind::Inside,
        );
    }
    let mut inner = rect.shrink(style.padding);
    if let Some(t) = title {
        p.text(
            egui::pos2(inner.min.x + 2.0, inner.min.y + TITLE_HEIGHT / 2.0 - 2.0),
            egui::Align2::LEFT_CENTER,
            t.to_uppercase(),
            egui::FontId::proportional(11.0),
            style::c(theme.dim),
        );
        inner.min.y += TITLE_HEIGHT;
    }
    inner
}

fn body(ui: &mut egui::Ui, rect: Rect, panel: &Panel, path: &str, draw: &mut DrawPanel<'_>) {
    if rect.width() < 8.0 || rect.height() < 8.0 {
        return;
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .id_salt(path)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .id_salt(path)
        .auto_shrink([false, false])
        .show(&mut child, |ui| draw(ui, panel));
}
