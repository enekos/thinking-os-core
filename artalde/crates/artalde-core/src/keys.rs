//! `keys.json`: chord → command.
//!
//! ```json
//! { "cmd+k": "palette.open", "cmd+shift+l": "layout.cycle" }
//! ```

use std::collections::BTreeMap;

use crate::command::Command;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The key's name as egui spells it: `K`, `Enter`, `1`, `ArrowDown`.
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let mut chord = Chord::default();
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let Some((key, mods)) = parts.split_last() else {
            return Err(format!("{s:?}: empty"));
        };
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "cmd" | "super" | "meta" => chord.cmd = true,
                "ctrl" | "control" => chord.ctrl = true,
                "alt" | "option" | "opt" => chord.alt = true,
                "shift" => chord.shift = true,
                other => return Err(format!("{s:?}: unknown modifier {other:?}")),
            }
        }
        if key.is_empty() {
            return Err(format!("{s:?}: no key"));
        }
        chord.key = if key.chars().count() == 1 {
            key.to_ascii_uppercase()
        } else {
            (*key).to_owned()
        };
        Ok(chord)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    pub bindings: Vec<(Chord, Command)>,
}

impl Keymap {
    pub fn parse(text: &str) -> Result<Keymap, String> {
        let raw: BTreeMap<String, String> =
            serde_json::from_str(text).map_err(|e| e.to_string())?;
        let bindings = raw
            .iter()
            .map(|(k, v)| Ok((Chord::parse(k)?, Command::parse(v))))
            .collect::<Result<_, String>>()?;
        Ok(Keymap { bindings })
    }

    /// The chord bound to a command, for showing next to it.
    pub fn chord_for(&self, cmd: &str) -> Option<&Chord> {
        self.bindings
            .iter()
            .find(|(_, c)| c.to_string() == cmd)
            .map(|(k, _)| k)
    }
}

impl std::fmt::Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        for (on, sym) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
            (self.cmd, "Cmd+"),
        ] {
            if on {
                out.push_str(sym);
            }
        }
        out.push_str(&self.key);
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse() {
        let c = Chord::parse("cmd+shift+l").unwrap();
        assert!(c.cmd && c.shift && !c.alt);
        assert_eq!(c.key, "L");
        assert_eq!(c.to_string(), "Shift+Cmd+L");
        assert_eq!(Chord::parse("ctrl+Enter").unwrap().key, "Enter");
        assert!(Chord::parse("hyper+k").is_err());
        assert!(Chord::parse("cmd+").is_err());
    }

    #[test]
    fn keymap_parses() {
        let k = Keymap::parse(r#"{ "cmd+k": "palette.open", "cmd+1": "agent.select:1" }"#).unwrap();
        assert_eq!(k.bindings.len(), 2);
        assert_eq!(
            k.chord_for("agent.select:1").map(|c| c.key.as_str()),
            Some("1")
        );
    }
}
