//! Colour tags and labels the user gives sessions, kept in
//! `<state dir>/tags.json` so they survive restarts. Only the window writes
//! this file.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme::Rgba;

/// Tags not touched for this long are dropped on save.
const TTL_SECS: u64 = 30 * 86_400;
const MAX_LABEL_CHARS: usize = 40;

/// The tag palette: name → colour. Mirrored onto iTerm2 tab colours.
pub const PALETTE: [(&str, Rgba); 8] = [
    ("red", [0xef, 0x53, 0x50, 255]),
    ("orange", [0xff, 0x8f, 0x3a, 255]),
    ("yellow", [0xf2, 0xc9, 0x4c, 255]),
    ("green", [0x4c, 0xc3, 0x6a, 255]),
    ("teal", [0x2b, 0xbb, 0xad, 255]),
    ("blue", [0x4a, 0x8d, 0xf7, 255]),
    ("purple", [0xa7, 0x7b, 0xf3, 255]),
    ("pink", [0xf0, 0x6e, 0xb4, 255]),
];

pub fn palette_color(name: &str) -> Option<Rgba> {
    PALETTE.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tag {
    pub color: Option<String>,
    pub label: Option<String>,
    pub updated: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tags {
    pub sessions: BTreeMap<String, Tag>,
    #[serde(skip)]
    path: PathBuf,
}

impl Tags {
    pub fn load(state_dir: &Path) -> Tags {
        let path = state_dir.join("tags.json");
        let mut tags: Tags = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        tags.path = path;
        tags
    }

    pub fn get(&self, id: &str) -> Option<&Tag> {
        self.sessions.get(id)
    }

    /// Set or clear (`None` / `"none"`) a session's colour.
    pub fn set_color(&mut self, id: &str, color: Option<&str>, now: u64) -> Result<(), String> {
        let color = match color {
            None | Some("" | "none") => None,
            Some(c) if palette_color(c).is_some() => Some(c.to_owned()),
            Some(c) => {
                let names: Vec<&str> = PALETTE.iter().map(|(n, _)| *n).collect();
                return Err(format!(
                    "unknown tag colour {c:?}; one of {}",
                    names.join(", ")
                ));
            }
        };
        self.edit(id, now, |t| t.color = color);
        Ok(())
    }

    pub fn set_label(&mut self, id: &str, label: &str, now: u64) {
        let label = crate::text::clip(label, MAX_LABEL_CHARS);
        self.edit(id, now, |t| t.label = (!label.is_empty()).then_some(label));
    }

    fn edit(&mut self, id: &str, now: u64, f: impl FnOnce(&mut Tag)) {
        let tag = self.sessions.entry(id.to_owned()).or_default();
        f(tag);
        tag.updated = now;
        if tag.color.is_none() && tag.label.is_none() {
            self.sessions.remove(id);
        }
    }

    pub fn save(&mut self, now: u64) -> io::Result<()> {
        self.sessions
            .retain(|_, t| now.saturating_sub(t.updated) < TTL_SECS);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip_and_clear() {
        let dir = std::env::temp_dir().join(format!("artalde-tags-{}", std::process::id()));
        let _fresh = fs::remove_dir_all(&dir);
        let mut tags = Tags::load(&dir);
        tags.set_color("s1", Some("blue"), 100).unwrap();
        tags.set_label("s1", "cv parser", 100);
        assert!(tags.set_color("s1", Some("mauve"), 100).is_err());
        tags.set_color("old", Some("red"), 1).unwrap();
        tags.save(TTL_SECS + 50).unwrap();
        let back = Tags::load(&dir);
        assert_eq!(
            back.get("s1").and_then(|t| t.color.as_deref()),
            Some("blue")
        );
        assert_eq!(
            back.get("s1").and_then(|t| t.label.as_deref()),
            Some("cv parser")
        );
        assert!(back.get("old").is_none(), "stale tags expire");
        let mut tags = back;
        tags.set_color("s1", None, 200).unwrap();
        tags.set_label("s1", "", 200);
        assert!(tags.get("s1").is_none(), "an empty tag disappears");
    }
}
