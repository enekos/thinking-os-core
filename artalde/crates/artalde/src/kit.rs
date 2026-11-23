//! Shared widgets, ported from azala's SwiftUI components so every panel
//! looks the same: section headers, fixed-height rows with aligned columns,
//! pills and pill tabs, ghost / primary / danger buttons, icon buttons, a
//! search field, cards, key hints and empty states. Icons are Lucide glyphs
//! from a subset font (`fonts/icons.txt`, `tools/icons.py`).

use artalde_core::theme::Theme;
use eframe::egui::emath::GuiRounding as _;
use eframe::egui::{
    self, Color32, CornerRadius, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2,
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
pub const ROW_RADIUS: u8 = 8;
pub const BUTTON_RADIUS: u8 = 7;
pub const CARD_RADIUS: u8 = 10;
pub const SHEET_RADIUS: u8 = 14;

/// A column of a [`row`]: fixed width, or a share of what is left.
#[derive(Clone, Copy, Debug)]
pub enum Col {
    Px(f32),
    Flex,
}

pub use crate::icons::Icon;

/// Draw `icon` centred in `rect`, as large as its shorter side. The glyph
/// comes from the bundled Lucide subset, so it is hinted and cached like text.
pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let galley = p.layout_no_wrap(icon.glyph().to_string(), fonts::icon(size), color);
    let at = rect.center() - galley.size() / 2.0;
    p.galley(at.round_to_pixels(p.pixels_per_point()), galley, color);
}

/// An icon laid out inline, `size` square.
pub fn icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_icon(ui.painter(), rect, icon, color);
}

/// Width and glyph size of an [`icon_button`]; it is `CONTROL` tall.
pub const ICON_BUTTON: f32 = 28.0;
const ICON_BUTTON_GLYPH: f32 = 15.0;
/// The icon that leads a button's label.
const LEAD_ICON: f32 = 14.0;
const MENU_CHEVRON: f32 = 12.0;

/// A square-ish icon button, quiet until hovered.
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
    let (rect, resp) = ui.allocate_exact_size(vec2(ICON_BUTTON, CONTROL), Sense::click());
    let enabled = ui.is_enabled();
    let hot = resp.hovered() && enabled;
    if hot {
        let pressed = resp.is_pointer_button_down_on();
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(BUTTON_RADIUS),
            tint(
                style::c(theme.surface),
                style::c(theme.text),
                if pressed { 0.14 } else { 0.08 },
            ),
        );
    }
    // Quiet at rest, full strength under the pointer.
    let color = if !enabled {
        color.gamma_multiply(0.35)
    } else if hot && color == style::c(theme.dim) {
        style::c(theme.text)
    } else {
        color
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(ICON_BUTTON_GLYPH, ICON_BUTTON_GLYPH)),
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

/// A left-aligned, vertically centred label in a fixed `width`×`height`
/// cell (`add_sized` would centre it).
pub fn caption(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    height: f32,
    font: egui::FontId,
    color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let galley = ellipsized(ui.painter(), text, font, color, width);
    ui.painter().galley(
        pos2(rect.min.x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// A [`section`] header that folds: chevron, title and count, the whole
/// width clickable. Returns true when clicked.
pub fn fold(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    count: Option<usize>,
    open: bool,
) -> bool {
    let r = ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        icon(
            ui,
            if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            10.0,
            style::c(theme.faint),
        );
        ui.label(
            RichText::new(title.to_uppercase())
                .font(fonts::semibold(10.5))
                .extra_letter_spacing(0.6)
                .color(style::c(theme.faint)),
        );
        if let Some(n) = count {
            badge(ui, theme, &n.to_string(), style::c(theme.dim));
        }
    });
    ui.add_space(2.0);
    let rect = r.response.rect.with_max_x(ui.max_rect().max.x);
    ui.interact(
        rect,
        ui.scope_id().with(egui::Id::unique(("fold", id))),
        Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

/// A small mono count in a capsule.
pub fn badge(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) {
    capsule(
        ui,
        text,
        egui::FontId::monospace(10.0),
        color,
        tint(style::c(theme.surface), style::c(theme.text), 0.08),
        vec2(5.0, 16.0),
        8,
    );
}

/// Text on a painted capsule of a fixed `size.y`, padded `size.x` each side,
/// so it never stretches with the row it sits in.
fn capsule(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    fill: Color32,
    size: egui::Vec2,
    radius: u8,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let w = (galley.size().x + size.x * 2.0).max(size.y);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, size.y), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// A clickable, full-width row of fixed `height`, its columns laid out
/// left to right and clipped; `fill` paints each column.
pub fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    height: f32,
    selected: bool,
    cols: &[Col],
    mut fill: impl FnMut(usize, &mut egui::Ui),
) -> egui::Response {
    let width = ui.available_width();
    // A stable id keeps hover and press on the same row while the list
    // above it changes.
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let resp = ui
        .interact(
            rect,
            ui.scope_id().with(egui::Id::unique(("kit-row", id))),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // Lit while the pointer is anywhere on the row, its own buttons too.
    let hovered = resp.hovered()
        || (ui.rect_contains_pointer(rect)
            && !ui.ctx().is_being_dragged(resp.id)
            && ui.ctx().dragged_id().is_none());
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(ROW_RADIUS),
            tint(style::c(theme.surface), style::c(theme.accent), 0.15),
        );
    } else if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(ROW_RADIUS),
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
    capsule(
        ui,
        keys,
        egui::FontId::monospace(10.0),
        style::c(theme.dim),
        tint(style::c(theme.surface), style::c(theme.text), 0.07),
        vec2(4.0, 17.0),
        4,
    );
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
    capsule(
        ui,
        text,
        egui::FontId::proportional(11.0),
        color,
        tint(style::c(theme.surface), color, 0.14),
        vec2(6.0, 18.0),
        5,
    )
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

/// How a live thing shows its state: the shape carries the meaning, so it
/// reads without colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A turning arc: something is running.
    Spin,
    /// A filled dot inside a breathing halo: it waits on you.
    Alert,
    /// A filled dot: finished, unseen.
    Dot,
    /// A hollow ring: at rest.
    Ring,
    /// A faint dashed ring: nothing known.
    Ghost,
}

/// Paint `mark` centred at `at`; `t` is the frame time driving animation.
pub fn paint_mark(p: &egui::Painter, at: egui::Pos2, mark: Mark, color: Color32, t: f64) {
    match mark {
        Mark::Spin => {
            p.circle_stroke(at, 5.0, Stroke::new(1.6, color.gamma_multiply(0.22)));
            #[expect(clippy::cast_possible_truncation, reason = "an animation phase")]
            let start = (t * 5.0).rem_euclid(std::f64::consts::TAU) as f32;
            let points: Vec<egui::Pos2> = (0..=10)
                .map(|i| {
                    let a = start + i as f32 * (std::f32::consts::PI * 0.85 / 10.0);
                    at + vec2(a.cos(), a.sin()) * 5.0
                })
                .collect();
            p.add(egui::Shape::line(points, Stroke::new(1.6, color)));
        }
        Mark::Alert => {
            #[expect(clippy::cast_possible_truncation, reason = "an animation phase")]
            let pulse = (0.5 + 0.5 * (t * 3.0).sin()) as f32;
            p.circle_filled(
                at,
                4.5 + 2.5 * pulse,
                color.gamma_multiply(0.28 * (1.0 - 0.5 * pulse)),
            );
            p.circle_filled(at, 4.0, color);
        }
        Mark::Dot => {
            p.circle_filled(at, 3.5, color);
        }
        Mark::Ring => {
            p.circle_stroke(at, 3.6, Stroke::new(1.4, color));
        }
        Mark::Ghost => {
            p.circle_stroke(at, 3.6, Stroke::new(1.2, color.gamma_multiply(0.5)));
        }
    }
}

/// A [`Mark`] laid out inline in a 14-point cell. Animated marks keep the
/// frame loop awake.
pub fn mark(ui: &mut egui::Ui, mark: Mark, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    let t = ui.input(|i| i.time);
    paint_mark(ui.painter(), rect.center(), mark, color, t);
    match mark {
        Mark::Spin => ui
            .ctx()
            .request_repaint_after(std::time::Duration::from_millis(33)),
        Mark::Alert => ui
            .ctx()
            .request_repaint_after(std::time::Duration::from_millis(50)),
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ghost,
    /// Filled with this colour.
    Primary(Color32),
    Danger,
}

fn control(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: Kind,
    lead: Option<Icon>,
    text: &str,
    keys: Option<&str>,
    menu: bool,
) -> egui::Response {
    let fill_color = match kind {
        Kind::Primary(c) => c,
        Kind::Ghost | Kind::Danger => Color32::TRANSPARENT,
    };
    let enabled = ui.is_enabled();
    let font = if matches!(kind, Kind::Primary(_)) {
        fonts::semibold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    let fg = match kind {
        Kind::Primary(_) => Color32::WHITE,
        Kind::Danger => style::c(theme.danger),
        Kind::Ghost => style::c(theme.text),
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, fg);
    let key_color = if matches!(kind, Kind::Primary(_)) {
        Color32::from_white_alpha(230)
    } else {
        style::c(theme.dim)
    };
    let key_galley = keys.map(|k| {
        ui.painter()
            .layout_no_wrap(k.to_owned(), egui::FontId::monospace(10.0), key_color)
    });
    let lead_w = if lead.is_some() { LEAD_ICON + 6.0 } else { 0.0 };
    // An icon sits a little closer to the edge than text does.
    let pad = if lead.is_some() { 10.0 } else { 12.0 };
    let key_w = key_galley.as_ref().map_or(0.0, |g| g.size().x + 14.0);
    // A menu button ends in a chevron, a little closer to the edge.
    let (trail_w, end) = if menu {
        (MENU_CHEVRON + 4.0, 8.0)
    } else {
        (0.0, 12.0)
    };
    let size = vec2(
        galley.size().x + pad + end + lead_w + key_w + trail_w,
        CONTROL,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let surface = style::c(theme.surface);
    let hovered = resp.hovered() && enabled;
    let pressed = resp.is_pointer_button_down_on() && enabled;
    let p = ui.painter();
    let radius = CornerRadius::same(BUTTON_RADIUS);
    match kind {
        Kind::Primary(_) => {
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
    let mut x = rect.min.x + pad;
    if let Some(i) = lead {
        paint_icon(
            p,
            Rect::from_center_size(
                pos2(x + LEAD_ICON / 2.0, rect.center().y),
                vec2(LEAD_ICON, LEAD_ICON),
            ),
            i,
            fg,
        );
        x += lead_w;
    }
    p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if menu {
        paint_icon(
            p,
            Rect::from_center_size(
                pos2(rect.max.x - end - MENU_CHEVRON / 2.0, rect.center().y + 0.5),
                vec2(MENU_CHEVRON, MENU_CHEVRON),
            ),
            Icon::ChevronDown,
            if matches!(kind, Kind::Primary(_)) {
                fg
            } else {
                style::c(theme.dim)
            },
        );
    }
    if let Some(kg) = key_galley {
        let chip = Rect::from_min_size(
            pos2(rect.max.x - kg.size().x - 16.0, rect.center().y - 8.0),
            vec2(kg.size().x + 8.0, 16.0),
        );
        let chip_fill = if matches!(kind, Kind::Primary(_)) {
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
    control(ui, theme, Kind::Ghost, None, text, keys, false)
}

/// A ghost button with a leading icon.
pub fn button_icon(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    text: &str,
    keys: Option<&str>,
) -> egui::Response {
    control(ui, theme, Kind::Ghost, Some(icon), text, keys, false)
}

/// A ghost button that opens a menu: optional icon, label, chevron.
pub fn menu_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Icon>,
    text: &str,
) -> egui::Response {
    control(ui, theme, Kind::Ghost, icon, text, None, true)
}

/// azala's PrimaryButton in `color`, for the one main action.
pub fn primary(ui: &mut egui::Ui, theme: &Theme, text: &str, color: Color32) -> egui::Response {
    control(ui, theme, Kind::Primary(color), None, text, None, false)
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
    control(ui, theme, Kind::Primary(color), icon, text, keys, false)
}

/// A ghost button in the danger colour, for ending things.
pub fn danger(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<Icon>,
    text: &str,
    keys: Option<&str>,
) -> egui::Response {
    control(ui, theme, Kind::Danger, icon, text, keys, false)
}

/// azala's FieldRow: `label  value`, the value copies on click.
pub fn kv(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str, label_w: f32) -> bool {
    let mut copied = false;
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        caption(
            ui,
            label,
            label_w,
            18.0,
            egui::FontId::proportional(12.0),
            style::c(theme.faint),
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
                        .margin(field_margin(ui, &egui::TextStyle::Body, 0))
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

/// Vertical text-edit margin that centres one line of `style` in a
/// `CONTROL`-tall field.
pub fn field_margin(ui: &egui::Ui, style: &egui::TextStyle, x: i8) -> egui::Margin {
    let h = ui.text_style_height(style);
    let total = ((CONTROL - h) / 2.0).max(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a few points, clamped below CONTROL"
    )]
    let top = total.floor() as i8;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a few points, clamped below CONTROL"
    )]
    let bottom = total.ceil() as i8;
    egui::Margin {
        left: x,
        right: x,
        top,
        bottom,
    }
}

/// A single-line text field in the kit's look: a soft wash, a hairline that
/// turns accent while focused, `CONTROL` tall and exactly `width` wide.
pub fn input(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &mut String,
    hint: &str,
    width: f32,
    mono: bool,
) -> egui::Response {
    let mut frame = egui::Frame::new()
        .fill(tint(style::c(theme.surface), style::c(theme.text), 0.05))
        .stroke(Stroke::new(1.0, style::c(theme.border)))
        .corner_radius(CornerRadius::same(BUTTON_RADIUS))
        .inner_margin(egui::Margin::symmetric(8, 0))
        .begin(ui);
    // The frame's 1-point stroke sits outside its margin.
    let inner_w = (width - 16.0 - 2.0).max(24.0);
    let mut edit = egui::TextEdit::singleline(text)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .min_size(vec2(inner_w, CONTROL))
        .align(egui::Align2::LEFT_CENTER)
        .desired_width(inner_w)
        .hint_text(RichText::new(hint).color(style::c(theme.faint)));
    if mono {
        edit = edit.font(egui::TextStyle::Monospace);
    }
    let resp = frame.content_ui.add(edit);
    if resp.has_focus() {
        frame.frame.stroke = Stroke::new(1.0, style::c(theme.accent));
    } else if resp.hovered() {
        frame.frame.stroke =
            Stroke::new(1.0, tint(style::c(theme.border), style::c(theme.text), 0.2));
    }
    frame.end(ui);
    resp
}

/// A small toggleable filter chip in monospace.
pub fn chip(ui: &mut egui::Ui, theme: &Theme, text: &str, on: bool) -> egui::Response {
    let accent = style::c(theme.accent);
    let color = if on { accent } else { style::c(theme.dim) };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.0), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0, 22.0), Sense::click());
    let surface = style::c(theme.surface);
    let p = ui.painter();
    let radius = CornerRadius::same(PILL_RADIUS);
    if on {
        p.rect_filled(rect, radius, tint(surface, accent, 0.15));
    } else {
        if resp.hovered() {
            p.rect_filled(rect, radius, tint(surface, style::c(theme.text), 0.06));
        }
        p.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, style::c(theme.border)),
            egui::StrokeKind::Inside,
        );
    }
    p.galley(
        pos2(rect.min.x + 7.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A 16-point tick box; `label` follows it when not empty. The whole thing
/// toggles.
pub fn check(
    ui: &mut egui::Ui,
    theme: &Theme,
    on: &mut bool,
    label: &str,
    size: f32,
) -> egui::Response {
    let galley = (!label.is_empty()).then(|| {
        ui.painter().layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(size),
            style::c(theme.dim),
        )
    });
    let w = 16.0 + galley.as_ref().map_or(0.0, |g| 6.0 + g.size().x);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let surface = style::c(theme.surface);
    let b = Rect::from_min_size(pos2(rect.min.x, rect.center().y - 8.0), vec2(16.0, 16.0));
    let p = ui.painter();
    let radius = CornerRadius::same(4);
    if *on {
        p.rect_filled(b, radius, style::c(theme.accent));
        paint_icon(p, b.shrink(2.5), Icon::Check, Color32::WHITE);
    } else {
        let hover = resp.hovered();
        p.rect_filled(
            b,
            radius,
            tint(
                surface,
                style::c(theme.text),
                if hover { 0.1 } else { 0.04 },
            ),
        );
        p.rect_stroke(
            b,
            radius,
            Stroke::new(
                1.0,
                if hover {
                    style::c(theme.dim)
                } else {
                    style::c(theme.border)
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(g) = galley {
        p.galley(
            pos2(b.max.x + 6.0, rect.center().y - g.size().y / 2.0),
            g,
            style::c(theme.dim),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An on/off switch with its label; the whole thing toggles.
pub fn toggle(ui: &mut egui::Ui, theme: &Theme, on: &mut bool, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        style::c(theme.text),
    );
    let track = vec2(30.0, 18.0);
    let size = vec2(track.x + 8.0 + galley.size().x, CONTROL);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let surface = style::c(theme.surface);
    let off_fill = tint(surface, style::c(theme.text), 0.16);
    let fill = tint(off_fill, style::c(theme.accent), t);
    let tr = Rect::from_min_size(pos2(rect.min.x, rect.center().y - track.y / 2.0), track);
    let p = ui.painter();
    p.rect_filled(tr, CornerRadius::same(9), fill);
    let knob_x = egui::lerp(tr.min.x + 9.0..=tr.max.x - 9.0, t);
    p.circle_filled(pos2(knob_x, tr.center().y), 7.0, Color32::WHITE);
    p.galley(
        pos2(tr.max.x + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        style::c(theme.text),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A dropdown whose closed state looks like a ghost button; `add` fills the
/// menu with [`menu_item`]s.
pub fn dropdown(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    width: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, CONTROL), Sense::hover());
    let resp = ui
        .interact(
            rect,
            ui.scope_id().with(egui::Id::unique(("dropdown", id))),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let surface = style::c(theme.surface);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let p = ui.painter();
    let radius = CornerRadius::same(BUTTON_RADIUS);
    if resp.hovered() || open {
        p.rect_filled(rect, radius, tint(surface, style::c(theme.text), 0.07));
    }
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0,
            if open {
                style::c(theme.accent)
            } else {
                style::c(theme.border)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let galley = p.layout_no_wrap(
        current.to_owned(),
        egui::FontId::proportional(12.5),
        style::c(theme.text),
    );
    let text_clip = Rect::from_min_max(rect.min, pos2(rect.max.x - 24.0, rect.max.y));
    p.with_clip_rect(text_clip.intersect(ui.clip_rect()))
        .galley(
            pos2(rect.min.x + 10.0, rect.center().y - galley.size().y / 2.0),
            galley,
            style::c(theme.text),
        );
    paint_icon(
        p,
        Rect::from_center_size(pos2(rect.max.x - 13.0, rect.center().y), vec2(10.0, 10.0)),
        Icon::ChevronDown,
        style::c(theme.dim),
    );
    egui::Popup::menu(&resp).width(width).show(|ui| {
        ui.set_min_width(width - 12.0);
        add(ui);
    });
}

/// One choice in a [`dropdown`] menu; returns true when picked.
pub fn menu_item(ui: &mut egui::Ui, theme: &Theme, label: &str, on: bool) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    let surface = style::c(theme.surface);
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(surface, style::c(theme.text), 0.07),
        );
    }
    let color = if on {
        style::c(theme.accent)
    } else {
        style::c(theme.text)
    };
    let font = if on {
        fonts::semibold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    let galley = p.layout_no_wrap(label.to_owned(), font, color);
    p.galley(
        pos2(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if on {
        paint_icon(
            p,
            Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), vec2(11.0, 11.0)),
            Icon::Check,
            color,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// One action in a context menu: icon, label, optional key chip. The
/// caller closes the menu on a click.
pub fn menu_action(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    label: &str,
    keys: Option<&str>,
) -> egui::Response {
    let enabled = ui.is_enabled();
    let fg = if enabled {
        style::c(theme.text)
    } else {
        style::c(theme.faint)
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(12.5), fg);
    let key_galley = keys.map(|k| {
        ui.painter().layout_no_wrap(
            k.to_owned(),
            egui::FontId::monospace(10.5),
            style::c(theme.faint),
        )
    });
    let want = 8.0
        + 14.0
        + 9.0
        + galley.size().x
        + key_galley.as_ref().map_or(0.0, |g| g.size().x + 20.0)
        + 10.0;
    let width = ui.available_width().max(want).max(180.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() && enabled {
        p.rect_filled(
            rect,
            CornerRadius::same(PILL_RADIUS),
            tint(style::c(theme.surface), style::c(theme.text), 0.08),
        );
    }
    paint_icon(
        p,
        Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(14.0, 14.0)),
        icon,
        if enabled {
            style::c(theme.dim)
        } else {
            style::c(theme.faint)
        },
    );
    p.galley(
        pos2(rect.min.x + 31.0, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    if let Some(kg) = key_galley {
        p.galley(
            pos2(
                rect.max.x - kg.size().x - 10.0,
                rect.center().y - kg.size().y / 2.0,
            ),
            kg,
            style::c(theme.faint),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A hairline between groups of a menu.
pub fn menu_rule(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, style::c(theme.border)),
    );
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

/// One line of `text` cut with an ellipsis to fit `width`.
pub fn ellipsized(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    painter.layout_job(job)
}

/// Let labels in `ui` be selected and copied: for reading surfaces
/// (markdown, logs, diffs), never inside a clickable row.
pub fn selectable_text(ui: &mut egui::Ui) {
    ui.style_mut().interaction.selectable_labels = true;
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
    use eframe::egui::Pos2;

    /// Run one frame of `add` in a central panel with `events` as input.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, add: &mut impl FnMut(&mut egui::Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 200.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| add(ui));
        out.textures_delta.clear();
    }

    fn click_at(ctx: &egui::Context, at: Pos2, add: &mut impl FnMut(&mut egui::Ui)) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(ctx, vec![egui::Event::PointerMoved(at)], add);
        frame(ctx, vec![button(true)], add);
        frame(ctx, vec![button(false)], add);
        frame(ctx, vec![], add);
    }

    #[test]
    fn clicking_a_rows_text_clicks_the_row() {
        let ctx = egui::Context::default();
        let theme = Theme::graphite();
        style::apply(&ctx, &theme, 13.5);
        let text_rect = std::cell::Cell::new(Rect::NOTHING);
        let row_rect = std::cell::Cell::new(Rect::NOTHING);
        let clicks = std::cell::Cell::new(0);
        let mut add = |ui: &mut egui::Ui| {
            let resp = row(
                ui,
                &theme,
                "r",
                ROW,
                false,
                &[Col::Flex, Col::Px(60.0)],
                |c, ui| {
                    if c == 0 {
                        text_rect
                            .set(text(ui, "an agent with a long name", 13.0, Color32::WHITE).rect);
                    }
                },
            );
            row_rect.set(resp.rect);
            if resp.clicked() {
                clicks.set(clicks.get() + 1);
            }
        };
        frame(&ctx, vec![], &mut add);
        click_at(&ctx, text_rect.get().center(), &mut add);
        assert_eq!(clicks.get(), 1, "a click on the label must reach the row");
        let r = row_rect.get();
        click_at(&ctx, pos2(r.max.x - 20.0, r.center().y), &mut add);
        assert_eq!(
            clicks.get(),
            2,
            "a click on the empty part reaches the row too"
        );
    }

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
