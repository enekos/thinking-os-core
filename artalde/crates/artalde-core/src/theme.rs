//! Themes: named colour tokens. Panels and mods ask for tokens (`blocked`,
//! `surface`), never raw colours, so a theme file restyles everything.

use serde::Deserialize;

/// sRGBA.
pub type Rgba = [u8; 4];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub background: Rgba,
    pub surface: Rgba,
    pub raised: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    /// Tertiary text: hints, counts, captions. Derived from `dim` and
    /// `surface` when a theme leaves it out.
    pub faint: Rgba,
    pub accent: Rgba,
    pub working: Rgba,
    pub blocked: Rgba,
    pub done: Rgba,
    pub idle: Rgba,
    pub unknown: Rgba,
    pub danger: Rgba,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTheme {
    dark: Option<bool>,
    background: Option<String>,
    surface: Option<String>,
    raised: Option<String>,
    border: Option<String>,
    text: Option<String>,
    dim: Option<String>,
    faint: Option<String>,
    accent: Option<String>,
    working: Option<String>,
    blocked: Option<String>,
    done: Option<String>,
    idle: Option<String>,
    unknown: Option<String>,
    danger: Option<String>,
}

impl Theme {
    /// azala's dark palette.
    pub fn graphite() -> Theme {
        Theme {
            name: "graphite".into(),
            dark: true,
            background: [0x1b, 0x1d, 0x23, 255],
            surface: [0x22, 0x25, 0x2c, 255],
            raised: [0x2a, 0x2e, 0x37, 255],
            border: [0x32, 0x36, 0x3f, 255],
            text: [0xe6, 0xe8, 0xee, 255],
            dim: [0x9a, 0xa1, 0xaf, 255],
            faint: [0x64, 0x6b, 0x79, 255],
            accent: [0x5b, 0x8d, 0xef, 255],
            working: [0x6f, 0xb3, 0xe0, 255],
            blocked: [0xf5, 0xc5, 0x42, 255],
            done: [0x3d, 0xba, 0x6e, 255],
            idle: [0x6e, 0x72, 0x80, 255],
            unknown: [0x4e, 0x53, 0x60, 255],
            danger: [0xe5, 0x54, 0x4b, 255],
        }
    }

    /// A token by name, or a `#rrggbb[aa]` literal.
    pub fn color(&self, token: &str) -> Option<Rgba> {
        Some(match token {
            "background" => self.background,
            "surface" => self.surface,
            "raised" => self.raised,
            "border" => self.border,
            "text" => self.text,
            "dim" => self.dim,
            "faint" => self.faint,
            "accent" => self.accent,
            "working" => self.working,
            "blocked" => self.blocked,
            "done" => self.done,
            "idle" => self.idle,
            "unknown" => self.unknown,
            "danger" => self.danger,
            other => return crate::tags::palette_color(other).or_else(|| hex(other)),
        })
    }

    pub fn parse(name: &str, text: &str) -> Result<Theme, String> {
        let raw: RawTheme = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut t = Theme::graphite();
        t.name = name.to_owned();
        if let Some(dark) = raw.dark {
            t.dark = dark;
        }
        let derive_faint = raw.faint.is_none();
        let slots: [(&str, &Option<String>, &mut Rgba); 14] = [
            ("background", &raw.background, &mut t.background),
            ("surface", &raw.surface, &mut t.surface),
            ("raised", &raw.raised, &mut t.raised),
            ("border", &raw.border, &mut t.border),
            ("text", &raw.text, &mut t.text),
            ("dim", &raw.dim, &mut t.dim),
            ("faint", &raw.faint, &mut t.faint),
            ("accent", &raw.accent, &mut t.accent),
            ("working", &raw.working, &mut t.working),
            ("blocked", &raw.blocked, &mut t.blocked),
            ("done", &raw.done, &mut t.done),
            ("idle", &raw.idle, &mut t.idle),
            ("unknown", &raw.unknown, &mut t.unknown),
            ("danger", &raw.danger, &mut t.danger),
        ];
        for (key, value, slot) in slots {
            if let Some(v) = value {
                *slot =
                    hex(v).ok_or_else(|| format!("{key}: {v:?} is not #rrggbb or #rrggbbaa"))?;
            }
        }
        if derive_faint {
            t.faint = mix(t.dim, t.surface, 0.3);
        }
        Ok(t)
    }
}

/// `a` moved `by` (0..1) toward `b`, opaque.
fn mix(a: Rgba, b: Rgba, by: f32) -> Rgba {
    let ch = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * by.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0..=255"
        )]
        let out = v.round().clamp(0.0, 255.0) as u8;
        out
    };
    [ch(a[0], b[0]), ch(a[1], b[1]), ch(a[2], b[2]), 255]
}

/// `#rrggbb` or `#rrggbbaa`.
pub fn hex(s: &str) -> Option<Rgba> {
    let h = s.strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok());
    Some([
        byte(0)?,
        byte(2)?,
        byte(4)?,
        if h.len() == 8 { byte(6)? } else { 255 },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_themes_inherit_graphite() {
        let t = Theme::parse("paper", r##"{ "dark": false, "background": "#ffffff" }"##).unwrap();
        assert!(!t.dark);
        assert_eq!(t.background, [255, 255, 255, 255]);
        assert_eq!(t.blocked, Theme::graphite().blocked);
        assert_ne!(
            t.faint,
            Theme::graphite().faint,
            "derived for the light surface"
        );
        assert!(Theme::parse("x", r#"{ "background": "red" }"#).is_err());
        assert!(Theme::parse("x", r##"{ "bakground": "#000000" }"##).is_err());
    }

    #[test]
    fn hex_literals_and_tokens() {
        let t = Theme::graphite();
        assert_eq!(t.color("#10203080"), Some([16, 32, 48, 128]));
        assert_eq!(t.color("blocked"), Some(t.blocked));
        assert_eq!(t.color("#ñññññ"), None);
        assert_eq!(t.color("nope"), None);
    }
}
