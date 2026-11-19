//! Theme tokens → egui colours and visuals.

use artalde_core::model::AgentState;
use artalde_core::theme::Theme;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const RADIUS: u8 = 8;

pub fn c(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// A token or `#hex`, falling back to `text`.
pub fn token(theme: &Theme, name: &str) -> Color32 {
    c(theme.color(name).unwrap_or(theme.text))
}

pub fn state(theme: &Theme, s: AgentState) -> Color32 {
    token(theme, s.as_str())
}

/// `color` at `alpha` (0..1) over `base`, as an opaque colour.
pub fn tint(base: Color32, color: Color32, alpha: f32) -> Color32 {
    let mix = |a: u8, b: u8| -> u8 {
        let v = f32::from(a) + (f32::from(b) - f32::from(a)) * alpha.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0..=255"
        )]
        let out = v.round().clamp(0.0, 255.0) as u8;
        out
    };
    Color32::from_rgb(
        mix(base.r(), color.r()),
        mix(base.g(), color.g()),
        mix(base.b(), color.b()),
    )
}

pub fn apply(ctx: &egui::Context, theme: &Theme, font_size: f32) {
    let mut style = (*ctx.global_style()).clone();
    let size = font_size.clamp(9.0, 24.0);
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(size - 2.5, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(size, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(size, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(
                size + 5.0,
                FontFamily::Name(crate::fonts::DISPLAY_FAMILY.into()),
            ),
        ),
        (
            TextStyle::Monospace,
            FontId::new(size - 1.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.combo_height = 320.0;
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.window_margin = egui::Margin::same(14);
    style.spacing.text_edit_width = 240.0;
    // Chrome text is not selectable: a label inside a clickable row would
    // otherwise take the click (and show an I-beam). Reading surfaces —
    // markdown, diffs, logs — opt back in with `selectable_text`.
    style.interaction.selectable_labels = false;
    // Thin bars that take their own lane, so they never sit on top of a
    // row's last column.
    style.spacing.scroll = egui::style::ScrollStyle {
        floating: false,
        bar_width: 6.0,
        bar_inner_margin: 2.0,
        bar_outer_margin: 0.0,
        ..egui::style::ScrollStyle::solid()
    };
    style.spacing.slider_rail_height = 4.0;
    style.spacing.slider_width = 180.0;
    style.interaction.multi_widget_text_select = false;

    let mut v = if theme.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let bg = c(theme.background);
    let surface = c(theme.surface);
    let raised = c(theme.raised);
    let border = c(theme.border);
    let text = c(theme.text);
    let accent = c(theme.accent);
    v.panel_fill = bg;
    v.window_fill = surface;
    v.extreme_bg_color = bg;
    v.faint_bg_color = raised;
    v.window_stroke = Stroke::new(1.0, border);
    v.window_corner_radius = CornerRadius::same(RADIUS + 2);
    v.override_text_color = Some(text);
    v.hyperlink_color = accent;
    v.selection.bg_fill = tint(surface, accent, 0.35);
    v.selection.stroke = Stroke::new(1.0, accent);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, surface),
        (&mut v.widgets.inactive, raised),
        (&mut v.widgets.hovered, tint(raised, accent, 0.18)),
        (&mut v.widgets.active, tint(raised, accent, 0.32)),
        (&mut v.widgets.open, raised),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = CornerRadius::same(crate::kit::BUTTON_RADIUS);
        w.fg_stroke = Stroke::new(1.0, text);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, tint(border, text, 0.25));
    v.widgets.active.bg_stroke = Stroke::new(1.0, tint(border, text, 0.35));
    // Stock buttons read as the kit's ghost buttons: no fill until hovered.
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.weak_bg_fill = tint(surface, text, 0.07);
    v.widgets.active.weak_bg_fill = tint(surface, text, 0.12);
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.expansion = 0.0;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, text);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if theme.dark { 90 } else { 28 }),
    };
    v.handle_shape = egui::style::HandleShape::Circle;
    v.slider_trailing_fill = true;
    style.visuals = v;
    ctx.set_global_style(style);
}
