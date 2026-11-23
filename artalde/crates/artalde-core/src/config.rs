//! The config folder lives in the wiki, `<wiki>/artalde/`
//! (`$ARTALDE_CONFIG_DIR` overrides), so the UI is versioned with the wiki
//! and agents reshape it by editing files:
//!
//! ```text
//! settings.json   keys.json   layouts/<name>.json   themes/<name>.json   mods/<name>.rhai
//! ```
//!
//! Missing files are filled from `defaults/` on start. Edits take effect
//! within a second. A file that fails to parse keeps its last good version
//! and writes the reason next to it as `<file>.error`, which an agent
//! editing the file can read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use crate::keys::Keymap;
use crate::layout::{self, Layout};
use crate::theme::Theme;

/// Config files larger than this are refused.
const MAX_FILE_BYTES: u64 = 512 * 1024;

pub const DEFAULT_FILES: &[(&str, &str)] = &[
    (
        "settings.json",
        include_str!("../../../../template/artalde/settings.json"),
    ),
    (
        "keys.json",
        include_str!("../../../../template/artalde/keys.json"),
    ),
    (
        "layouts/main.json",
        include_str!("../../../../template/artalde/layouts/main.json"),
    ),
    (
        "layouts/focus.json",
        include_str!("../../../../template/artalde/layouts/focus.json"),
    ),
    (
        "layouts/radar.json",
        include_str!("../../../../template/artalde/layouts/radar.json"),
    ),
    (
        "layouts/terminals.json",
        include_str!("../../../../template/artalde/layouts/terminals.json"),
    ),
    (
        "themes/graphite.json",
        include_str!("../../../../template/artalde/themes/graphite.json"),
    ),
    (
        "themes/paper.json",
        include_str!("../../../../template/artalde/themes/paper.json"),
    ),
    (
        "themes/moss.json",
        include_str!("../../../../template/artalde/themes/moss.json"),
    ),
    (
        "mods/radar.rhai",
        include_str!("../../../../template/artalde/mods/radar.rhai"),
    ),
    (
        "mods/board.rhai",
        include_str!("../../../../template/artalde/mods/board.rhai"),
    ),
    (
        "mods/pulse.rhai",
        include_str!("../../../../template/artalde/mods/pulse.rhai"),
    ),
    (
        "AGENTS.md",
        include_str!("../../../../template/artalde/AGENTS.md"),
    ),
];

/// The thinking-os wiki, found the way zaindu finds it: `$THINKING_OS_ROOT`,
/// else the nearest folder up from here holding `graph/ontology.yaml`, else
/// `~/thinking-os`.
pub fn wiki_root() -> PathBuf {
    if let Some(p) = std::env::var_os("THINKING_OS_ROOT").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(mut here) = std::env::current_dir() {
        loop {
            if here.join("graph/ontology.yaml").exists() {
                return here;
            }
            if !here.pop() {
                break;
            }
        }
    }
    crate::home().unwrap_or_default().join("thinking-os")
}

/// `$ARTALDE_CONFIG_DIR`, else `<wiki>/artalde`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("ARTALDE_CONFIG_DIR")
        .filter(|p| !p.is_empty())
        .map_or_else(|| wiki_root().join("artalde"), PathBuf::from)
}

/// Write every default file that does not exist yet; returns what it wrote.
pub fn install_defaults(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut wrote = Vec::new();
    for (rel, body) in DEFAULT_FILES {
        let path = dir.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, body)?;
        wrote.push((*rel).to_owned());
    }
    Ok(wrote)
}

#[derive(Clone, Debug, PartialEq, Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub layout: String,
    /// The dark theme.
    pub theme: String,
    /// The light theme.
    pub light_theme: String,
    /// `system`, `light` or `dark`.
    pub appearance: String,
    /// Retired: artalde no longer sends macOS notifications (its own
    /// toasts cover blocked and finished agents). Still read so older
    /// `settings.json` files load; never written back.
    #[serde(rename = "notify", skip_serializing)]
    pub legacy_notify: Option<bool>,
    /// Days of zaindu proposal files to show.
    pub inbox_days: usize,
    /// Poll `gh` for open PRs.
    pub gh: bool,
    /// Shell line that starts an agent in a new worktree. `{repo}`,
    /// `{branch}`, `{agent}`, `{dir}` and `{prompt}` are filled in quoted.
    pub spawn_worktree: String,
    /// Shell line that starts an agent in an existing folder.
    pub spawn_here: String,
    /// Agents offered when spawning.
    pub agents: Vec<String>,
    pub font_size: f32,
    /// The app `path.edit` opens folders in (`open -a <editor>`).
    pub editor: String,
    /// Agents that can review a PR. Each runs read-only with the prompt on
    /// stdin; `{dir}` (pr.md + pr.diff) and `{checkout}` are filled in.
    pub review_agents: Vec<ReviewAgent>,
    /// Minutes idle before a session counts as stale and is offered for
    /// closing; 0 turns it off.
    pub stale_after_mins: u64,
    /// Close the agent's iTerm2 tab too when closing a session.
    pub close_terminal: bool,
    /// Where agents start and focus goes: `embedded` (artalde's own
    /// terminals) or `iterm`.
    pub terminal: String,
    /// The terminal's font size in points.
    pub terminal_font_size: f32,
    /// Option sends Alt (Meta) in terminals instead of typing the layout's
    /// characters. Off by default: many layouts need Option for `@` or `[`.
    pub option_as_alt: bool,
    /// The program a new terminal runs; empty is the login shell.
    pub shell: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAgent {
    pub name: String,
    pub command: String,
}

pub fn default_review_agents() -> Vec<ReviewAgent> {
    vec![
        ReviewAgent {
            name: "Claude".into(),
            command: "claude -p --output-format text --allowedTools Read Grep Glob --add-dir {dir} {checkout}".into(),
        },
        ReviewAgent { name: "Codex".into(), command: "codex exec --sandbox read-only --skip-git-repo-check -C {dir} -".into() },
    ]
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            layout: "main".into(),
            theme: "graphite".into(),
            light_theme: "paper".into(),
            appearance: "system".into(),
            legacy_notify: None,
            inbox_days: 2,
            gh: true,
            spawn_worktree: "zz -c {repo}@{branch} {agent} {prompt}".into(),
            spawn_here: "cd {dir} && {agent} {prompt}".into(),
            agents: vec!["claude".into(), "codex".into()],
            font_size: 13.5,
            editor: "Zed".into(),
            review_agents: default_review_agents(),
            stale_after_mins: 60,
            close_terminal: true,
            terminal: "embedded".into(),
            terminal_font_size: 12.5,
            option_as_alt: false,
            shell: Vec::new(),
        }
    }
}

/// Write `settings` to `<dir>/settings.json`, atomically; the window's
/// watcher picks it up like any other edit.
pub fn save_settings(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)? + "\n";
    let path = dir.join("settings.json");
    let tmp = dir.join("settings.json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, &path)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModSource {
    pub name: String,
    pub path: PathBuf,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub dir: PathBuf,
    pub settings: Settings,
    pub keys: Keymap,
    pub layouts: Vec<Layout>,
    pub themes: Vec<Theme>,
    pub mods: Vec<ModSource>,
    /// `file: reason` for every file that failed this load.
    pub errors: Vec<String>,
}

impl Config {
    pub fn layout(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.name == name)
    }

    pub fn theme(&self, name: &str) -> Theme {
        self.themes
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .unwrap_or_else(Theme::graphite)
    }

    pub fn mod_source(&self, name: &str) -> Option<&ModSource> {
        self.mods.iter().find(|m| m.name == name)
    }

    /// Load `dir`. Files that fail fall back to their version in `previous`.
    pub fn load(dir: &Path, previous: Option<&Config>) -> Config {
        let mut errors = Vec::new();
        let mut fail = |path: &Path, msg: String| {
            let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
            write_error(path, Some(&msg));
            errors.push(format!("{rel}: {msg}"));
        };

        let settings_path = dir.join("settings.json");
        let settings = match read(&settings_path).map(|t| serde_json::from_str::<Settings>(&t)) {
            None => Settings::default(),
            Some(Ok(s)) => {
                write_error(&settings_path, None);
                s
            }
            Some(Err(e)) => {
                fail(&settings_path, e.to_string());
                previous.map(|p| p.settings.clone()).unwrap_or_default()
            }
        };

        let keys_path = dir.join("keys.json");
        let keys = match read(&keys_path).map(|t| Keymap::parse(&t)) {
            None => Keymap::default(),
            Some(Ok(k)) => {
                write_error(&keys_path, None);
                k
            }
            Some(Err(e)) => {
                fail(&keys_path, e);
                previous.map(|p| p.keys.clone()).unwrap_or_default()
            }
        };

        let mut layouts = Vec::new();
        for (name, path) in files(&dir.join("layouts"), "json") {
            match read(&path).map(|t| layout::parse(&name, &t)) {
                Some(Ok(l)) => {
                    write_error(&path, None);
                    layouts.push(l);
                }
                Some(Err(e)) => {
                    fail(&path, e.to_string());
                    if let Some(old) = previous.and_then(|p| p.layout(&name)) {
                        layouts.push(old.clone());
                    }
                }
                None => {}
            }
        }

        let mut themes = Vec::new();
        for (name, path) in files(&dir.join("themes"), "json") {
            match read(&path).map(|t| Theme::parse(&name, &t)) {
                Some(Ok(t)) => {
                    write_error(&path, None);
                    themes.push(t);
                }
                Some(Err(e)) => {
                    fail(&path, e);
                    if let Some(old) =
                        previous.and_then(|p| p.themes.iter().find(|t| t.name == name))
                    {
                        themes.push(old.clone());
                    }
                }
                None => {}
            }
        }

        let mods = files(&dir.join("mods"), "rhai")
            .into_iter()
            .filter_map(|(name, path)| read(&path).map(|source| ModSource { name, path, source }))
            .collect();

        Config {
            dir: dir.to_path_buf(),
            settings,
            keys,
            layouts,
            themes,
            mods,
            errors,
        }
    }
}

fn read(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return Some(String::new());
    }
    fs::read_to_string(path).ok()
}

/// `(stem, path)` of every `*.ext` in `dir`, sorted.
fn files(dir: &Path, ext: &str) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
        .collect();
    out.sort();
    out
}

/// `<file>.error` holds why `file` was refused; `None` clears it.
pub fn write_error(file: &Path, msg: Option<&str>) {
    let mut name = file.as_os_str().to_owned();
    name.push(".error");
    let err_path = PathBuf::from(name);
    match msg {
        Some(m) => {
            if fs::read_to_string(&err_path).ok().as_deref() != Some(m) {
                // Best effort: the error is also shown in the window.
                let _best_effort = fs::write(&err_path, m);
            }
        }
        None => {
            if err_path.exists() {
                let _best_effort = fs::remove_file(&err_path);
            }
        }
    }
}

/// Changes whenever any config file is added, removed or written.
pub fn fingerprint(dir: &Path) -> BTreeMap<PathBuf, (SystemTime, u64)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if d == dir {
                    stack.push(path);
                }
            } else if !path.extension().is_some_and(|x| x == "error" || x == "tmp") {
                out.insert(
                    path,
                    (
                        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        meta.len(),
                    ),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("artalde-config-{name}-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn retired_notify_still_loads_and_is_not_written() {
        let s: Settings = serde_json::from_str(r#"{"notify": true}"#).unwrap();
        assert_eq!(s.legacy_notify, Some(true));
        assert!(!serde_json::to_string(&s).unwrap().contains("notify"));
    }

    #[test]
    fn defaults_load_clean() {
        let dir = scratch("defaults");
        let wrote = install_defaults(&dir).unwrap();
        assert_eq!(wrote.len(), DEFAULT_FILES.len());
        assert!(
            install_defaults(&dir).unwrap().is_empty(),
            "second install writes nothing"
        );
        let cfg = Config::load(&dir, None);
        assert_eq!(cfg.errors, Vec::<String>::new());
        assert!(cfg.layout("main").is_some());
        assert_eq!(cfg.theme("paper").name, "paper");
        assert!(cfg.mod_source("radar").is_some());
        for layout in &cfg.layouts {
            for m in layout.root.mods() {
                assert!(
                    cfg.mod_source(&m).is_some(),
                    "{} uses missing mod {m}",
                    layout.name
                );
            }
        }
    }

    #[test]
    fn a_broken_file_keeps_its_last_good_version() {
        let dir = scratch("broken");
        install_defaults(&dir).unwrap();
        let good = Config::load(&dir, None);
        fs::write(dir.join("layouts/main.json"), "{ nope").unwrap();
        let cfg = Config::load(&dir, Some(&good));
        assert_eq!(cfg.layout("main"), good.layout("main"));
        assert_eq!(cfg.errors.len(), 1);
        assert!(dir.join("layouts/main.json.error").exists());
        fs::write(
            dir.join("layouts/main.json"),
            r#"{ "root": { "block": "agents" } }"#,
        )
        .unwrap();
        let fixed = Config::load(&dir, Some(&cfg));
        assert!(fixed.errors.is_empty());
        assert!(
            !dir.join("layouts/main.json.error").exists(),
            "fixing clears the error file"
        );
    }

    #[test]
    fn settings_round_trip() {
        let dir = scratch("settings");
        fs::create_dir_all(&dir).unwrap();
        let s = Settings {
            appearance: "light".into(),
            font_size: 15.0,
            ..Settings::default()
        };
        save_settings(&dir, &s).unwrap();
        assert_eq!(Config::load(&dir, None).settings, s);
    }

    #[test]
    fn fingerprint_sees_edits() {
        let dir = scratch("fp");
        install_defaults(&dir).unwrap();
        let a = fingerprint(&dir);
        fs::write(dir.join("keys.json"), "{}\n\n").unwrap();
        assert_ne!(a, fingerprint(&dir));
    }
}
