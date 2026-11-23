//! Draw a mod's [`UiNode`] tree; collect what was clicked.

use artalde_core::theme::Theme;
use artalde_mods::{Action, Kind, UiNode};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind};

use crate::kit;
use crate::style::{self, tint, token};

const CARD_WIDTH: f32 = 230.0;
const WRAP_GAP: f32 = 8.0;

pub fn show(ui: &mut egui::Ui, theme: &Theme, node: &UiNode, clicked: &mut Vec<Action>) {
    let mut path = 0usize;
    draw(ui, theme, node, clicked, &mut path);
}

/// An unknown name draws a dashed circle rather than nothing.
fn named_icon(name: &str) -> kit::Icon {
    kit::Icon::named(name).unwrap_or(kit::Icon::CircleDashed)
}

fn rich(theme: &Theme, node: &UiNode, default_color: Option<Color32>) -> RichText {
    let mut t = RichText::new(&node.text);
    if let Some(size) = node.size {
        t = t.size(size);
    }
    if node.bold {
        t = t.strong();
    }
    if node.mono {
        t = t.monospace();
    }
    match node
        .color
        .as_deref()
        .map(|c| token(theme, c))
        .or(default_color)
    {
        Some(c) => t.color(c),
        None => t,
    }
}

fn draw(
    ui: &mut egui::Ui,
    theme: &Theme,
    node: &UiNode,
    clicked: &mut Vec<Action>,
    path: &mut usize,
) {
    *path += 1;
    let id = ui.scope_id().with(("mod-node", *path));
    let resp = match node.kind {
        Kind::Text => Some(ui.add(egui::Label::new(rich(theme, node, None)).wrap())),
        Kind::Heading => {
            let color = node
                .color
                .as_deref()
                .map_or(style::c(theme.text), |c| token(theme, c));
            Some(
                ui.label(
                    RichText::new(&node.text)
                        .font(crate::fonts::display(node.size.unwrap_or(17.0)))
                        .color(color),
                ),
            )
        }
        Kind::Markdown => {
            let out = crate::markdown::show(ui, theme, &node.text, node.size.unwrap_or(13.5));
            clicked.extend(out.clicked_links.into_iter().map(|u| {
                Action::Command(artalde_core::command::Command::new("url.open", Some(&u)))
            }));
            None
        }
        Kind::Row => Some(
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                children(ui, theme, node, clicked, path);
            })
            .response,
        ),
        Kind::Col => Some(
            ui.vertical(|ui| children(ui, theme, node, clicked, path))
                .response,
        ),
        Kind::Wrap => {
            // Rows of as many cards as fit; egui's own wrapping does not
            // wrap framed children.
            let per_row = ((ui.available_width() + WRAP_GAP) / (CARD_WIDTH + 20.0 + WRAP_GAP))
                .floor()
                .max(1.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a small positive count"
            )]
            let per_row = per_row as usize;
            let r = ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(WRAP_GAP, WRAP_GAP);
                for chunk in node.children.chunks(per_row) {
                    ui.horizontal_top(|ui| {
                        for child in chunk {
                            draw(ui, theme, child, clicked, path);
                        }
                    });
                }
            });
            Some(r.response)
        }
        Kind::Grid => {
            // Keep columns readable and align the top of every new row.
            let fitting = (ui.available_width() / 180.0).floor().max(1.0) as usize;
            let cols = node.cols.max(1).min(fitting);
            for row in node.children.chunks(cols) {
                ui.columns(cols, |columns| {
                    for (child, col) in row.iter().zip(columns) {
                        draw(col, theme, child, clicked, path);
                    }
                });
            }
            None
        }
        Kind::Card => {
            let surface = style::c(theme.raised);
            let fill = node
                .fill
                .as_deref()
                .map_or(surface, |f| tint(surface, token(theme, f), 0.14));
            let stroke_color = node
                .color
                .as_deref()
                .map_or(style::c(theme.border), |c| token(theme, c));
            let width = if node.color.is_some() { 1.5 } else { 1.0 };
            // A clickable container registers before its children, so
            // buttons inside a card keep their own clicks.
            let scoped = ui.scope_builder(
                egui::UiBuilder::new().id_salt(id).sense(Sense::click()),
                |ui| {
                    egui::Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(width, stroke_color))
                        .corner_radius(CornerRadius::same(kit::CARD_RADIUS))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            let w = CARD_WIDTH.min(ui.available_width());
                            ui.set_max_width(w);
                            ui.set_min_width(w);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                children(ui, theme, node, clicked, path);
                            });
                        });
                },
            );
            let mut resp = scoped.response;
            if node.action.is_some() {
                if ui.rect_contains_pointer(resp.rect) {
                    ui.painter().rect_stroke(
                        resp.rect,
                        CornerRadius::same(kit::CARD_RADIUS),
                        Stroke::new(1.0, tint(stroke_color, style::c(theme.text), 0.35)),
                        StrokeKind::Inside,
                    );
                }
                resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            Some(resp)
        }
        Kind::Button => Some(match node.icon.as_deref().map(named_icon) {
            Some(icon) => kit::button_icon(ui, theme, icon, &node.text, None),
            None => kit::button(ui, theme, &node.text, None),
        }),
        Kind::Icon => {
            let color = node
                .color
                .as_deref()
                .map_or(style::c(theme.dim), |c| token(theme, c));
            let size = node.size.unwrap_or(14.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
            kit::paint_icon(ui.painter(), rect, named_icon(&node.text), color);
            Some(resp)
        }
        Kind::Badge => {
            let color = node
                .color
                .as_deref()
                .map_or(style::c(theme.dim), |c| token(theme, c));
            Some(kit::pill(ui, theme, &node.text, color))
        }
        Kind::Dot => {
            let color = node
                .color
                .as_deref()
                .map_or(style::c(theme.dim), |c| token(theme, c));
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, color);
            Some(resp)
        }
        Kind::Sep => {
            ui.add_space(kit::S1);
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, style::c(theme.border));
            ui.add_space(kit::S1);
            Some(resp)
        }
        Kind::Space => {
            ui.add_space(node.value.clamp(0.0, 200.0));
            None
        }
        Kind::Bar => {
            let color = node
                .color
                .as_deref()
                .map_or(style::c(theme.accent), |c| token(theme, c));
            Some(
                ui.add(
                    egui::ProgressBar::new(node.value.clamp(0.0, 1.0))
                        .fill(color)
                        .desired_height(6.0),
                ),
            )
        }
    };
    if let Some(resp) = resp {
        let resp = match &node.tip {
            Some(tip) => resp.on_hover_text(tip),
            None => resp,
        };
        if resp.clicked()
            && let Some(action) = &node.action
        {
            clicked.push(action.clone());
        }
    }
}

fn children(
    ui: &mut egui::Ui,
    theme: &Theme,
    node: &UiNode,
    clicked: &mut Vec<Action>,
    path: &mut usize,
) {
    for child in &node.children {
        draw(ui, theme, child, clicked, path);
    }
}
