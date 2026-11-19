//! What Claude Code itself records about running sessions:
//!
//! - `~/.claude/sessions/<pid>.json`: session id, cwd, short name and a live
//!   `status` (`busy`, `idle`, `waiting` + `waitingFor`) for every process.
//! - `~/.claude/projects/<cwd slug>/<session id>.jsonl`: the transcript, with
//!   `ai-title` entries (the conversation title) and everything said, which
//!   [`Scan`] reads incrementally for titles and PR/ticket mentions.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::model::{AgentState, ResourceKind};
use crate::text::clip;

const MAX_SESSION_FILE: u64 = 64 * 1024;
/// Transcript bytes read per scan step, so a huge transcript is caught up
/// over several ticks instead of one long stall.
const SCAN_STEP: u64 = 4 * 1024 * 1024;
const TITLE_CHARS: usize = 90;
const MAX_TRACKED: usize = 200;
/// Ticket numbers docs use as examples (`THU-123-desc`), never real work.
const PLACEHOLDER_IDS: [&str; 6] = ["1", "12", "123", "1234", "12345", "0"];

/// One `~/.claude/sessions/<pid>.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeSession {
    pub pid: u32,
    pub session_id: String,
    pub cwd: PathBuf,
    pub name: Option<String>,
    pub status: Option<String>,
    pub waiting_for: Option<String>,
    /// Milliseconds since the epoch.
    pub started_at: u64,
    pub status_updated_at: u64,
    pub kind: Option<String>,
}

impl ClaudeSession {
    /// The live state this file reports, before done-detection.
    pub fn state(&self) -> AgentState {
        match self.status.as_deref() {
            Some("busy") => AgentState::Working,
            Some("waiting") => AgentState::Blocked,
            Some("idle") => AgentState::Idle,
            _ => AgentState::Unknown,
        }
    }

    /// Seconds since the epoch the status last changed.
    pub fn status_since(&self) -> u64 {
        if self.status_updated_at > 0 {
            self.status_updated_at / 1000
        } else {
            self.started_at / 1000
        }
    }
}

pub fn claude_home() -> Option<PathBuf> {
    crate::home().map(|h| h.join(".claude"))
}

/// Every session file, keyed by pid.
pub fn sessions() -> HashMap<u32, ClaudeSession> {
    let Some(dir) = claude_home().map(|h| h.join("sessions")) else {
        return HashMap::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter(|p| fs::metadata(p).is_ok_and(|m| m.len() <= MAX_SESSION_FILE))
        .filter_map(|p| serde_json::from_slice::<ClaudeSession>(&fs::read(p).ok()?).ok())
        .filter(|s| s.pid != 0 && !s.session_id.is_empty())
        .map(|s| (s.pid, s))
        .collect()
}

/// Claude Code's project folder name for a cwd: every non-alphanumeric
/// character becomes `-`.
pub fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The transcript of `session_id` started in `cwd`, if it exists.
pub fn transcript_path(cwd: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.contains(['/', '.']) {
        return None;
    }
    let projects = claude_home()?.join("projects");
    let direct = projects
        .join(project_slug(cwd))
        .join(format!("{session_id}.jsonl"));
    if direct.exists() {
        return Some(direct);
    }
    fs::read_dir(projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join(format!("{session_id}.jsonl")))
        .find(|p| p.exists())
}

/// What a transcript has said so far, read incrementally.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    offset: u64,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    /// id → (mentions, position of the last mention).
    mentions: HashMap<(ResourceKind, String), (u32, u64)>,
    seen: u64,
}

impl Scan {
    /// Read whatever was appended since the last call (at most [`SCAN_STEP`]).
    pub fn advance(&mut self, path: &Path, teams: &[String]) {
        let Ok(mut file) = fs::File::open(path) else {
            return;
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            return;
        };
        if len < self.offset {
            *self = Scan::default();
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.take(SCAN_STEP).read_to_end(&mut buf).is_err() {
            return;
        }
        // Only whole lines; a half-written one is read next time.
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
            if buf.len() as u64 == SCAN_STEP {
                // One line longer than a step: skip it.
                self.offset += SCAN_STEP;
            }
            return;
        };
        let chunk = String::from_utf8_lossy(buf.get(..=end).unwrap_or_default()).into_owned();
        self.offset += end as u64 + 1;
        for line in chunk.lines() {
            self.line(line, teams);
        }
    }

    fn line(&mut self, line: &str, teams: &[String]) {
        self.seen += 1;
        if line.contains("\"ai-title\"") || line.contains("\"last-prompt\"") {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                match v.get("type").and_then(Value::as_str) {
                    Some("ai-title") => {
                        if let Some(t) = v
                            .get("aiTitle")
                            .and_then(Value::as_str)
                            .filter(|t| !t.trim().is_empty())
                        {
                            self.ai_title = Some(clip(t, TITLE_CHARS));
                        }
                    }
                    Some("last-prompt") => {
                        self.last_prompt = v
                            .get("lastPrompt")
                            .and_then(Value::as_str)
                            .map(|p| clip(p, 240));
                    }
                    _ => {}
                }
            }
            return;
        }
        for id in github_prs(line) {
            self.mention(ResourceKind::Pr, id);
        }
        for id in tickets(line, teams) {
            self.mention(ResourceKind::Ticket, id);
        }
    }

    fn mention(&mut self, kind: ResourceKind, id: String) {
        if self.mentions.len() >= MAX_TRACKED && !self.mentions.contains_key(&(kind, id.clone())) {
            return;
        }
        let seen = self.seen;
        let e = self.mentions.entry((kind, id)).or_insert((0, 0));
        e.0 = e.0.saturating_add(1);
        e.1 = seen;
    }

    /// The `limit` most relevant ids of `kind`: recent and repeated first.
    pub fn top(&self, kind: ResourceKind, limit: usize) -> Vec<(String, u32)> {
        let mut v: Vec<(&String, u32, u64)> = self
            .mentions
            .iter()
            .filter(|((k, _), _)| *k == kind)
            .map(|((_, id), (n, at))| (id, *n, *at))
            .collect();
        // A mention in the last tenth of the conversation counts as current.
        let recent = self.seen.saturating_sub(self.seen / 10 + 20);
        v.sort_by_key(|(id, n, at)| {
            (
                std::cmp::Reverse(*at >= recent),
                std::cmp::Reverse(*n),
                std::cmp::Reverse(*at),
                (*id).clone(),
            )
        });
        v.into_iter()
            .take(limit)
            .map(|(id, n, _)| (id.clone(), n))
            .collect()
    }
}

/// `owner/repo#N` for every `github.com/owner/repo/pull/N` in `text`.
pub fn github_prs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("github.com/") {
        rest = rest.get(i + "github.com/".len()..).unwrap_or_default();
        let mut parts = rest.splitn(4, '/');
        let (Some(owner), Some(repo), Some("pull"), Some(tail)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let num: String = tail.chars().take_while(char::is_ascii_digit).collect();
        let ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        };
        if ok(owner) && ok(repo) && !num.is_empty() {
            let id = format!("{owner}/{repo}#{num}");
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// `TEAM-123` ids for the given team keys, on word boundaries.
pub fn tickets(text: &str, teams: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for team in teams {
        let pat = format!("{team}-");
        let mut from = 0;
        while let Some(i) = text.get(from..).and_then(|t| t.find(&pat)) {
            let at = from + i;
            from = at + pat.len();
            let before_ok = text
                .get(..at)
                .and_then(|b| b.chars().next_back())
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            let digits: String = text
                .get(from..)
                .unwrap_or_default()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let after = text
                .get(from + digits.len()..)
                .and_then(|a| a.chars().next());
            let placeholder = PLACEHOLDER_IDS.contains(&digits.as_str());
            if before_ok
                && !digits.is_empty()
                && !placeholder
                && after.is_none_or(|c| !c.is_ascii_alphanumeric())
            {
                let id = format!("{team}-{digits}");
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_files_parse() {
        let s: ClaudeSession = serde_json::from_str(
            r#"{"pid":8286,"sessionId":"9d19","cwd":"/w","name":"thinking-os-a7","status":"waiting","waitingFor":"input needed","startedAt":1000,"statusUpdatedAt":5000,"other":1}"#,
        )
        .unwrap();
        assert_eq!((s.state(), s.status_since()), (AgentState::Blocked, 5));
        assert_eq!(s.waiting_for.as_deref(), Some("input needed"));
    }

    #[test]
    fn slug_matches_claude_code() {
        assert_eq!(
            project_slug(Path::new("/Users/e/thinking-os")),
            "-Users-e-thinking-os"
        );
        assert_eq!(project_slug(Path::new("/a/b.c_d")), "-a-b-c-d");
    }

    #[test]
    fn pr_urls_and_tickets_extract() {
        let text = "see https://github.com/join-com/data/pull/7939#x and github.com/a/b/pull/1, github.com/a/b/issues/2; THU-4771 and xTHU-1 THU-12a ENG-9 THU-123-desc";
        assert_eq!(github_prs(text), ["join-com/data#7939", "a/b#1"]);
        assert_eq!(
            tickets(text, &["THU".into(), "ENG".into()]),
            ["THU-4771", "ENG-9"]
        );
    }

    #[test]
    fn scan_is_incremental_and_ranks_recent_mentions() {
        let dir = std::env::temp_dir().join(format!("artalde-scan-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let mut body = String::new();
        for _ in 0..30 {
            body.push_str("{\"type\":\"user\",\"text\":\"old https://github.com/o/r/pull/1\"}\n");
        }
        body.push_str("{\"type\":\"ai-title\",\"aiTitle\":\"Fix login\"}\n");
        for _ in 0..300 {
            body.push_str("{\"type\":\"user\",\"text\":\"filler\"}\n");
        }
        fs::write(&path, &body).unwrap();
        let teams = vec!["THU".to_owned()];
        let mut scan = Scan::default();
        scan.advance(&path, &teams);
        assert_eq!(scan.ai_title.as_deref(), Some("Fix login"));
        body.push_str(
            "{\"type\":\"assistant\",\"text\":\"now github.com/o/r/pull/2 for THU-5\"}\n{\"partial",
        );
        fs::write(&path, &body).unwrap();
        scan.advance(&path, &teams);
        let prs = scan.top(ResourceKind::Pr, 5);
        assert_eq!(
            prs,
            [("o/r#2".to_owned(), 1), ("o/r#1".to_owned(), 30)],
            "recent beats frequent"
        );
        assert_eq!(scan.top(ResourceKind::Ticket, 5), [("THU-5".to_owned(), 1)]);
    }
}
