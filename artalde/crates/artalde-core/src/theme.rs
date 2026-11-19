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
    accent: Option<String>,
    working: Option<String>,
    blocked: Option<String>,
    done: Option<String>,
    idle: Option<String>,
    unknown: Option<String>,
    danger: Option<String>,
}

impl Theme {
    pub fn graphite() -> Theme {
        Theme {
            name: "graphite".into(),
            dark: true,
            background: [0x14, 0x15, 0x18, 255],
            surface: [0x1b, 0x1d, 0x21, 255],
            raised: [0x24, 0x27, 0x2d, 255],
            border: [0x2e, 0x31, 0x38, 255],
            text: [0xe4, 0xe6, 0xeb, 255],
            dim: [0x8a, 0x8f, 0x99, 255],
            accent: [0x7c, 0xa8, 0xff, 255],
            working: [0x5e, 0xa8, 0xff, 255],
            blocked: [0xff, 0xb0, 0x3b, 255],
            done: [0x4c, 0xd1, 0x8a, 255],
            idle: [0x6b, 0x70, 0x7a, 255],
            unknown: [0x4a, 0x4e, 0x56, 255],
            danger: [0xff, 0x5f, 0x5f, 255],
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
            "accent" => self.accent,
            "working" => self.working,
            "blocked" => self.blocked,
            "done" => self.done,
            "idle" => self.idle,
            "unknown" => self.unknown,
            "danger" => self.danger,
            other => return hex(other),
        })
    }

    pub fn parse(name: &str, text: &str) -> Result<Theme, String> {
        let raw: RawTheme = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut t = Theme::graphite();
        t.name = name.to_owned();
        if let Some(dark) = raw.dark {
            t.dark = dark;
        }
        let slots: [(&str, &Option<String>, &mut Rgba); 13] = [
            ("background", &raw.background, &mut t.background),
            ("surface", &raw.surface, &mut t.surface),
            ("raised", &raw.raised, &mut t.raised),
            ("border", &raw.border, &mut t.border),
            ("text", &raw.text, &mut t.text),
            ("dim", &raw.dim, &mut t.dim),
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
        Ok(t)
    }
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
