//! Shared widgets, ported from azala's SwiftUI components so every panel
//! looks the same: section headers, fixed-height rows with aligned columns,
//! pills and pill tabs, ghost / primary / danger buttons, icon buttons, a
//! search field, cards, key hints and empty states. Icons are drawn with
//! the painter, so they are crisp at any scale and need no font.

use artalde_core::theme::Theme;
use eframe::egui::{
    self, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2,
};

use crate::fonts;
use crate::style::{self, tint};

/// The spacing scale.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
/// One list row.
pub const ROW: f32 = 32.0;
/// A row with a second line.
pub const ROW2: f32 = 46.0;
/// Every control (button, field, pill tab) is this tall, so they line up.
pub const CONTROL: f32 = 26.0;

/// azala's radii.
pub const PILL_RADIUS: u8 = 6;
pub const BUTTON_RADIUS: u8 = 7;
pub const CARD_RADIUS: u8 = 10;
pub const SHEET_RADIUS: u8 = 14;

/// A column of a [`row`]: fixed width, or a share of what is left.
#[derive(Clone, Copy, Debug)]
pub enum Col {
    Px(f32),
    Flex,
}

/// Painter-drawn glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Search,
    Plus,
    Close,
    Stop,
    Focus,
    Reopen,
    More,
    Gear,
    Sun,
    Terminal,
    Send,
    ChevronRight,
    ChevronDown,
    Check,
    Sweep,
    Branch,
    Editor,
}

/// Draw `icon` centred in `rect`.
pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let at = |x: f32, y: f32| pos2(c.x + (x - 0.5) * s, c.y + (y - 0.5) * s);
    let w = (s / 10.0).clamp(1.2, 1.8);
    let st = Stroke::new(w, color);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    let poly = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            pts.iter().map(|(x, y)| at(*x, *y)).collect(),
            st,
        ));
    };
    match icon {
        Icon::Search => {
            p.circle_stroke(at(0.43, 0.43), s * 0.27, st);
            line(at(0.63, 0.63), at(0.88, 0.88));
        }
        Icon::Plus => {
            line(at(0.5, 0.18), at(0.5, 0.82));
            line(at(0.18, 0.5), at(0.82, 0.5));
        }
        Icon::Close => {
            line(at(0.24, 0.24), at(0.76, 0.76));
            line(at(0.76, 0.24), at(0.24, 0.76));
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.62, s * 0.62)),
                CornerRadius::same(1),
                color,
            );
        }
        Icon::Focus => {
            // A box with an arrow leaving its corner.
            poly(&[(0.55, 0.2), (0.2, 0.2), (0.2, 0.8), (0.8, 0.8), (0.8, 0.45)]);
            line(at(0.45, 0.55), at(0.85, 0.15));
            line(at(0.6, 0.15), at(0.85, 0.15));
            line(at(0.85, 0.15), at(0.85, 0.4));
        }
        Icon::Reopen => {
            let r = s * 0.3;
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.15 + 1.55 * i as f32 / 20.0);
                    pos2(c.x + r * a.cos(), c.y - r * a.sin())
                })
                .collect();
            let head = pts[0];
            p.add(egui::Shape::line(pts, st));
            line(head, head + vec2(-s * 0.2, -s * 0.02));
            line(head, head + vec2(-s * 0.02, -s * 0.2));
        }
        Icon::More => {
            for x in [0.25, 0.5, 0.75] {
                p.circle_filled(at(x, 0.5), s * 0.07, color);
            }
        }
        Icon::Gear => {
            p.circle_stroke(c, s * 0.16, st);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(c + d * s * 0.26, c + d * s * 0.4);
            }
        }
        Icon::Sun => {
            p.circle_filled(c, s * 0.17, color);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(c + d * s * 0.3, c + d * s * 0.42);
            }
        }
        Icon::Terminal => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(s * 0.8, s * 0.64)),
                CornerRadius::same(2),
                st,
                egui::StrokeKind::Middle,
            );
            poly(&[(0.3, 0.38), (0.42, 0.5), (0.3, 0.62)]);
            line(at(0.5, 0.63), at(0.7, 0.63));
        }
        Icon::Send => {
            line(at(0.2, 0.5), at(0.8, 0.5));
            poly(&[(0.55, 0.25), (0.8, 0.5), (0.55, 0.75)]);
        }
        Icon::ChevronRight => poly(&[(0.4, 0.25), (0.65, 0.5), (0.4, 0.75)]),
        Icon::ChevronDown => poly(&[(0.25, 0.4), (0.5, 0.65), (0.75, 0.4)]),
        Icon::Check => poly(&[(0.22, 0.52), (0.42, 0.72), (0.8, 0.3)]),
        Icon::Sweep => {
            // A bin: lid, body, two ribs.
            line(at(0.2, 0.28), at(0.8, 0.28));
            line(at(0.4, 0.18), at(0.6, 0.18));
            poly(&[(0.28, 0.28), (0.33, 0.85), (0.67, 0.85), (0.72, 0.28)]);
            line(at(0.45, 0.42), at(0.45, 0.72));
            line(at(0.55, 0.42), at(0.55, 0.72));
        }
        Icon::Branch => {
            p.circle_stroke(at(0.3, 0.22), s * 0.09, st);
            p.circle_stroke(at(0.3, 0.8), s * 0.09, st);
            p.circle_stroke(at(0.72, 0.36), s * 0.09, st);
            line(at(0.3, 0.31), at(0.3, 0.71));
            poly(&[(0.72, 0.45), (0.72, 0.52), (0.3, 0.66)]);
        }
        Icon::Editor => {
            poly(&[(0.35, 0.28), (0.15, 0.5), (0.35, 0.72)]);
            poly(&[(0.65, 0.28), (0.85, 0.5), (0.65, 0.72)]);
            line(at(0.56, 0.2), at(0.44, 0.8));
        }
    }
}

/// An icon laid out inline, `size` square.
pub fn icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_icon(ui.painter(), rect, icon, color);
}

/// azala's IconButton: 26×24, quiet until hovered.
pub fn icon_button(ui: &mut egui::Ui, theme: &Theme, icon: Icon, help: &str) -> egui::Response {
    icon_button_tinted(ui, theme, icon, help, style::c(theme.dim))
}

pub fn icon_button_tinted(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    help: &str,
    color: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    let enabled = ui.is_enabled();
    if resp.hovered() && enabled {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(style::c(theme.surface), style::c(theme.text), 0.08),
        );
    }
    let color = if enabled {
        color
    } else {
        color.gamma_multiply(0.35)
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(13.0, 13.0)),
        icon,
        color,
    );
    resp.on_hover_text(help)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `SECTION  3` with right-aligned extras.
pub fn section(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    count: Option<usize>,
    right: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let r = ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(title.to_uppercase())
                .font(fonts::semibold(10.5))
                .extra_letter_spacing(0.6)
                .color(style::c(theme.faint)),
        );
        if let Some(n) = count {
            badge(ui, theme, &n.to_string(), style::c(theme.dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(2.0);
    r.response
}

/// A small mono count in a capsule.
pub fn badge(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), style::c(theme.text), 0.08))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(10.0).color(color));
        });
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
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = id;
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(style::c(theme.surface), style::c(theme.accent), 0.15),
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(style::c(theme.surface), style::c(theme.text), 0.05),
        );
    }
    let inset = S2.min(width.max(0.0) / 2.0);
    let inner = rect.shrink2(vec2(inset, 0.0));
    let gap = S2.min(inner.width() / cols.len().max(1) as f32);
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
    let budget = (inner.width() - gaps).max(0.0);
    let fixed_scale = if fixed > budget { budget / fixed } else { 1.0 };
    let flex_w = ((budget - fixed) / flex_n).max(0.0);
    let mut x = inner.min.x;
    for (i, c) in cols.iter().enumerate() {
        let w = match c {
            Col::Px(w) => *w * fixed_scale,
            Col::Flex => flex_w,
        };
        let r = Rect::from_min_size(pos2(x, inner.min.y), vec2(w, inner.height()));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(r)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(r.intersect(rect).intersect(ui.clip_rect()));
        fill(i, &mut child);
        x += w + gap;
    }
    resp
}

/// azala's KeyHint chip: `⌘K`.
pub fn key(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), style::c(theme.text), 0.07))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(keys)
                    .monospace()
                    .size(10.0)
                    .color(style::c(theme.dim)),
            )
        });
}

/// `move  j k`: what, then its keys.
pub fn hint(ui: &mut egui::Ui, theme: &Theme, keys: &str, what: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(RichText::new(what).size(11.0).color(style::c(theme.faint)));
        key(ui, theme, keys);
    });
    ui.add_space(S2);
}

/// azala's Pill: tinted text on a 14% wash.
pub fn pill(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), color, 0.14))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color))
        })
        .response
}

/// A small coloured dot.
pub fn dot(ui: &mut egui::Ui, color: Color32, radius: f32) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(radius * 2.0 + 4.0, radius * 2.0 + 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
}

/// A state dot with a soft halo, for live things.
pub fn status_dot(ui: &mut egui::Ui, color: Color32, halo: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    if halo > 0.0 {
        ui.painter()
            .circle_filled(rect.center(), 6.5, color.gamma_multiply(0.25 * halo));
    }
    ui.painter().circle_filled(rect.center(), 3.8, color);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ghost,
    Primary,
    Danger,
}

fn control(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: Kind,
    lead: Option<Icon>,
    text: &str,
    keys: Option<&str>,
    fill_color: Color32,
) -> egui::Response {
    let enabled = ui.is_enabled();
    let font = if kind == Kind::Primary {
        fonts::semibold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    let fg = match kind {
        Kind::Primary => Color32::WHITE,
        Kind::Danger => style::c(theme.danger),
        Kind::Ghost => style::c(theme.text),
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let key_color = if kind == Kind::Primary {
        Color32::from_white_alpha(230)
    } else {
        style::c(theme.dim)
    };
    let key_galley = keys.map(|k| {
        ui.painter()
            .layout_no_wrap(k.to_owned(), egui::FontId::monospace(10.0), key_color)
    });
    let lead_w = if lead.is_some() { 18.0 } else { 0.0 };
    let key_w = key_galley.as_ref().map_or(0.0, |g| g.size().x + 14.0);
    let size = vec2(galley.size().x + 24.0 + lead_w + key_w, CONTROL);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let surface = style::c(theme.surface);
    let hovered = resp.hovered() && enabled;
    let pressed = resp.is_pointer_button_down_on() && enabled;
    let p = ui.painter();
    let radius = CornerRadius::same(BUTTON_RADIUS);
    match kind {
        Kind::Primary => {
            let fill = if pressed {
                fill_color.gamma_multiply(0.85)
            } else if hovered {
                tint(fill_color, Color32::WHITE, 0.1)
            } else {
                fill_color
            };
            p.rect_filled(
                rect,
                radius,
                if enabled {
                    fill
                } else {
                    fill.gamma_multiply(0.4)
                },
            );
        }
        Kind::Ghost | Kind::Danger => {
            if hovered || pressed {
                let wash = if kind == Kind::Danger {
                    style::c(theme.danger)
                } else {
                    style::c(theme.text)
                };
                p.rect_filled(
                    rect,
                    radius,
                    tint(surface, wash, if pressed { 0.12 } else { 0.07 }),
                );
            }
            p.rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, style::c(theme.border)),
                egui::StrokeKind::Inside,
            );
        }
    }
    let fg = if enabled { fg } else { fg.gamma_multiply(0.4) };
    let mut x = rect.min.x + 12.0;
    if let Some(i) = lead {
        paint_icon(
            p,
            Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0)),
            i,
            fg,
        );
        x += lead_w;
    }
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if let Some(kg) = key_galley {
        let chip = Rect::from_min_size(
            pos2(rect.max.x - kg.size().x - 16.0, rect.center().y - 8.0),
            vec2(kg.size().x + 8.0, 16.0),
        );
        let chip_fill = if kind == Kind::Primary {
            Color32::from_white_alpha(45)
        } else {
            tint(surface, style::c(theme.text), 0.07)
        };
        p.rect_filled(chip, CornerRadius::same(4), chip_fill);
        p.galley(
            pos2(chip.min.x + 4.0, chip.center().y - kg.size().y / 2.0),
            kg,
            key_color,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// azala's GhostButton: hairline border, quiet until hovered.
pub fn button(ui: &mut egui::Ui, theme: &Theme, text: &str, keys: Option<&str>) -> egui::Response {
    control(
        ui,
        theme,
        Kind::Ghost,
        None,
        text,
        keys,
        Color32::TRANSPARENT,
    )
}

/// A ghost button with a leading icon.
pub fn button_icon(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    text: &str,
    keys: Option<&str>,
) -> egui::Response {
    control(
        ui,
        theme,
        Kind::Ghost,
        Some(icon),
        text,
        keys,
        Color32::TRANSPARENT,
    )
}

/// azala's PrimaryButton in `color`, for the one main action.
pub fn primary(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) -> egui::Response {
    control(ui, theme, Kind::Primary, None, text, None, color)
}

/// The primary button with a leading icon and key hint.
pub fn primary_with(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Icon>,
    text: &str,
    keys: Option<&str>,
    color: Color32,
) -> egui::Response {
    control(ui, theme, Kind::Primary, icon, text, keys, color)
}

/// A ghost button in the danger colour, for ending things.
pub fn danger(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Icon>,
    text: &str,
    keys: Option<&str>,
) -> egui::Response {
    control(
        ui,
        theme,
        Kind::Danger,
        icon,
        text,
        keys,
        Color32::TRANSPARENT,
    )
}

/// azala's FieldRow: `label  value`, the value copies on click.
pub fn kv(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str, label_w: f32) -> bool {
    let mut copied = false;
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        ui.add_sized(
            [label_w, 18.0],
            egui::Label::new(RichText::new(label).size(12.0).color(style::c(theme.faint)))
                .halign(egui::Align::Min),
        );
        let r = ui
            .add(
                egui::Label::new(RichText::new(value).monospace().size(11.5))
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

/// azala's SectionCard: an uppercase caption, then the content, on a
/// hairline-bordered card.
pub fn card<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    trailing: Option<&str>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(tint(
            style::c(theme.surface),
            style::c(theme.background),
            0.35,
        ))
        .stroke(Stroke::new(1.0, style::c(theme.border)))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .font(fonts::semibold(10.0))
                        .extra_letter_spacing(0.6)
                        .color(style::c(theme.faint)),
                );
                if let Some(t) = trailing {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(t).size(11.0).color(style::c(theme.faint)));
                    });
                }
            });
            ui.add_space(6.0);
            add(ui)
        })
        .inner
}

/// azala's SearchField: magnifier, frameless text, clear button.
pub fn search(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let fill = tint(style::c(theme.surface), style::c(theme.text), 0.06);
    egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(PILL_RADIUS))
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.set_width((width - 16.0).max(60.0));
            ui.set_height(CONTROL);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                icon(ui, Icon::Search, 12.0, style::c(theme.faint));
                let clear_w = if text.is_empty() { 0.0 } else { 28.0 };
                let r = ui.add(
                    egui::TextEdit::singleline(text)
                        .frame(egui::Frame::NONE)
                        .hint_text(RichText::new(hint).color(style::c(theme.faint)))
                        .desired_width((ui.available_width() - clear_w).max(20.0)),
                );
                if !text.is_empty() && icon_button(ui, theme, Icon::Close, "Clear").clicked() {
                    text.clear();
                }
                r
            })
            .inner
        })
        .inner
}

/// azala's EmptyState: a tinted disc with an icon, a title and a hint.
pub fn empty_with(ui: &mut egui::Ui, theme: &Theme, glyph: Icon, title: &str, hint: &str) {
    ui.add_space(S4 * 2.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
        let accent = style::c(theme.accent);
        ui.painter().circle_filled(
            rect.center(),
            26.0,
            tint(style::c(theme.surface), accent, 0.12),
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
            glyph,
            accent,
        );
        ui.add_space(S3);
        ui.label(
            RichText::new(title)
                .font(fonts::display(16.0))
                .color(style::c(theme.text)),
        );
        ui.add_space(S1);
        ui.add(
            egui::Label::new(RichText::new(hint).size(12.0).color(style::c(theme.faint)))
                .wrap()
                .halign(egui::Align::Center),
        );
    });
    ui.add_space(S4 * 2.0);
}

/// Centered title and hint for an empty list.
pub fn empty(ui: &mut egui::Ui, theme: &Theme, title: &str, hint: &str) {
    empty_with(ui, theme, Icon::Check, title, hint);
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

/// One azala PillTab; `badge` is an optional count.
pub fn pill_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    badge: Option<&str>,
    active: bool,
    accent: Color32,
) -> egui::Response {
    let font = if active {
        fonts::semibold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    let color = if active { accent } else { style::c(theme.dim) };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let bg = badge.map(|b| {
        ui.painter()
            .layout_no_wrap(b.to_owned(), egui::FontId::monospace(10.0), color)
    });
    let badge_w = bg.as_ref().map_or(0.0, |g| g.size().x + 13.0);
    let (rect, resp) = ui.allocate_exact_size(
        vec2(galley.size().x + 20.0 + badge_w, CONTROL),
        Sense::click(),
    );
    let surface = style::c(theme.surface);
    let p = ui.painter();
    if active {
        p.rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(surface, accent, 0.15),
        );
    } else if resp.hovered() {
        p.rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(surface, style::c(theme.text), 0.06),
        );
    }
    let text_w = galley.size().x;
    p.galley(
        pos2(rect.min.x + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if let Some(g) = bg {
        let chip = Rect::from_min_size(
            pos2(rect.min.x + 10.0 + text_w + 5.0, rect.center().y - 7.5),
            vec2(g.size().x + 8.0, 15.0),
        );
        p.rect_filled(
            chip,
            CornerRadius::same(8),
            tint(surface, style::c(theme.text), 0.08),
        );
        p.galley(
            pos2(chip.min.x + 4.0, chip.center().y - g.size().y / 2.0),
            g,
            color,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row of pill tabs; returns the index clicked, if any.
pub fn tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: &[(String, Option<usize>)],
    selected: usize,
) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, (label, count)) in labels.iter().enumerate() {
            let badge = count.map(|n| n.to_string());
            if pill_tab(
                ui,
                theme,
                label,
                badge.as_deref(),
                i == selected,
                style::c(theme.accent),
            )
            .clicked()
            {
                clicked = Some(i);
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_columns_do_not_escape_a_narrow_row() {
        egui::__run_test_ui(|ui| {
            ui.set_width(90.0);
            let mut columns = Vec::new();
            let response = row(
                ui,
                &Theme::graphite(),
                "narrow",
                ROW,
                false,
                &[Col::Px(70.0), Col::Flex, Col::Px(80.0)],
                |_, child| columns.push(child.max_rect()),
            );
            for rect in columns {
                assert!(rect.width() >= 0.0);
                assert!(rect.min.x >= response.rect.min.x);
                assert!(rect.max.x <= response.rect.max.x + 0.01);
                assert!((rect.center().y - response.rect.center().y).abs() < 0.01);
            }
        });
    }

    #[test]
    fn controls_share_one_height() {
        egui::__run_test_ui(|ui| {
            let t = Theme::graphite();
            let a = button(ui, &t, "Ghost", Some("⌘N")).rect.height();
            let b = primary(ui, &t, "Go", style::c(t.accent)).rect.height();
            let c = pill_tab(ui, &t, "Tab", Some("3"), true, style::c(t.accent))
                .rect
                .height();
            let d = danger(ui, &t, Some(Icon::Close), "Close", None)
                .rect
                .height();
            assert!([a, b, c, d].iter().all(|h| (*h - CONTROL).abs() < 0.01));
        });
    }
}
