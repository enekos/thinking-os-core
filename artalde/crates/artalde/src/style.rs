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
    style.spacing.button_padding = egui::vec2(8.0, 3.0);

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
        w.corner_radius = CornerRadius::same(6);
        w.fg_stroke = Stroke::new(1.0, text);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
    style.visuals = v;
    ctx.set_global_style(style);
}
