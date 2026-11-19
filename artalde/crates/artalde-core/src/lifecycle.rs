//! The agent lifecycle artalde enforces: launch → working ⇄ needs you →
//! idle → stale → closed → (reopen). Pure rules here; the app runs the
//! side effects.
//!
//! - A [`Launch`] is an agent the user started that has not shown up as a
//!   process yet; [`Launch::claims`] matches it to the first new session.
//! - [`Stage`] is where a live session sits; idle past a threshold is stale
//!   and offered for closing in bulk.
//! - Closing ends the process (TERM, then KILL) and records a [`Closed`]
//!   entry, so a closed Claude session can be reopened with `--resume`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{AgentState, Session};
use crate::text::shell_quote;

/// Closed sessions kept for reopening.
pub const CLOSED_KEEP: usize = 30;
/// A launch that has not shown up after this long is dropped.
pub const LAUNCH_TTL_SECS: u64 = 120;
/// A session counts as a launch's when it started at most this long before it.
const LAUNCH_SLACK_SECS: u64 = 5;
const MAX_CLOSED_BYTES: u64 = 256 * 1024;
const BRANCH_WORDS: usize = 5;
const BRANCH_MAX: usize = 48;

/// Where a live session is in its life, in the order the list shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Blocked on the user, or finished a turn the user has not seen.
    NeedsYou,
    Working,
    Idle,
    /// Idle for longer than the stale threshold: a candidate for closing.
    Stale,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::NeedsYou, Stage::Working, Stage::Idle, Stage::Stale];

    pub fn of(state: AgentState, since: u64, now: u64, stale_after_secs: u64) -> Stage {
        match state {
            AgentState::Blocked | AgentState::Done => Stage::NeedsYou,
            AgentState::Working => Stage::Working,
            AgentState::Idle | AgentState::Unknown => {
                if stale_after_secs > 0 && now.saturating_sub(since) >= stale_after_secs {
                    Stage::Stale
                } else {
                    Stage::Idle
                }
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::NeedsYou => "Needs you",
            Stage::Working => "Working",
            Stage::Idle => "Idle",
            Stage::Stale => "Stale",
        }
    }
}

/// Whether closing `state` throws away work in flight and so asks first.
pub fn close_needs_confirm(state: AgentState) -> bool {
    matches!(state, AgentState::Working | AgentState::Blocked)
}

/// An agent the user started from artalde, until its process appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    /// The repository it was started in.
    pub repo: String,
    /// The folder, when started in an existing worktree.
    pub dir: Option<PathBuf>,
    /// The branch, when started in a new worktree.
    pub branch: Option<String>,
    pub agent: String,
    pub prompt: String,
    pub at: u64,
}

impl Launch {
    /// Whether `s` is this launch showing up: same agent and place, and it
    /// started no earlier than the launch.
    pub fn claims(&self, s: &Session) -> bool {
        if s.rec.agent != self.agent || s.rec.since + LAUNCH_SLACK_SECS < self.at {
            return false;
        }
        let in_dir = |d: &Path| {
            s.worktree.as_deref() == Some(d) || s.rec.cwd == d || s.rec.cwd.starts_with(d)
        };
        match (&self.dir, &self.branch) {
            (Some(d), _) => in_dir(d),
            (None, Some(b)) => {
                s.branch.as_deref() == Some(b.as_str())
                    || s.rec.cwd.to_string_lossy().ends_with(&slug_path(b))
                    || (s.repo.as_deref() == Some(self.repo.as_str()) && s.branch.is_none())
            }
            (None, None) => s.repo.as_deref() == Some(self.repo.as_str()),
        }
    }

    pub fn expired(&self, now: u64) -> bool {
        now.saturating_sub(self.at) > LAUNCH_TTL_SECS
    }

    /// A one-line name until the session reports its own.
    pub fn name(&self) -> String {
        let what = crate::text::clip(&self.prompt, 70);
        if what.is_empty() {
            format!(
                "{} · {}",
                self.agent,
                self.branch.as_deref().unwrap_or(&self.repo)
            )
        } else {
            what
        }
    }
}

/// `zz`'s worktree folder name for a branch: slashes become dashes.
fn slug_path(branch: &str) -> String {
    branch.replace('/', "-")
}

/// A branch name for a task: the ticket id if the prompt names one, then
/// up to five words of it, e.g. `THU-123-fix-csrf-timing`.
pub fn branch_for(prompt: &str, teams: &[String]) -> String {
    let ticket = crate::claude::tickets(prompt, teams).into_iter().next();
    let mut words: Vec<String> = Vec::new();
    for w in prompt.split(|c: char| !c.is_ascii_alphanumeric()) {
        let w = w.to_ascii_lowercase();
        if w.len() < 2 || STOP_WORDS.contains(&w.as_str()) {
            continue;
        }
        if ticket
            .as_deref()
            .is_some_and(|t| t.to_ascii_lowercase().split('-').any(|p| p == w))
        {
            continue;
        }
        words.push(w);
        if words.len() == BRANCH_WORDS {
            break;
        }
    }
    let mut out = ticket.unwrap_or_default();
    for w in words {
        if out.len() + w.len() + 1 > BRANCH_MAX {
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&w);
    }
    out
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "can", "do", "for", "from", "how", "in", "into",
    "is", "it", "me", "my", "of", "on", "or", "please", "so", "that", "the", "this", "to", "we",
    "what", "with", "you", "your", "i", "lets", "let", "make", "need", "should", "could", "would",
];

/// A session the user closed, kept so it can be reopened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    pub id: String,
    pub name: String,
    pub agent: String,
    pub cwd: PathBuf,
    pub repo: Option<String>,
    pub at: u64,
}

impl Closed {
    pub fn of(s: &Session, at: u64) -> Closed {
        Closed {
            id: s.rec.id.clone(),
            name: s.name(),
            agent: s.rec.agent.clone(),
            cwd: s.rec.cwd.clone(),
            repo: s.repo.clone(),
            at,
        }
    }

    /// The shell line that brings it back, when the agent can resume.
    pub fn reopen_line(&self) -> Option<String> {
        resume_line(&self.agent, &self.id, &self.cwd)
    }
}

/// `cd <cwd> && claude --resume <id>`: only Claude sessions with a real
/// session id can be resumed; a process found by `ps` has none.
pub fn resume_line(agent: &str, id: &str, cwd: &Path) -> Option<String> {
    let resumable = agent == "claude"
        && !id.starts_with("pid-")
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !resumable || cwd.as_os_str().is_empty() {
        return None;
    }
    Some(format!(
        "cd {} && claude --resume {id}",
        shell_quote(&cwd.display().to_string())
    ))
}

/// The closed-session log: `<state dir>/closed.json`, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClosedLog {
    pub entries: Vec<Closed>,
    path: PathBuf,
}

impl ClosedLog {
    pub fn load(state_dir: &Path) -> ClosedLog {
        let path = state_dir.join("closed.json");
        let entries = fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= MAX_CLOSED_BYTES)
            .and_then(|_| fs::read(&path).ok())
            .and_then(|b| serde_json::from_slice::<Vec<Closed>>(&b).ok())
            .unwrap_or_default();
        ClosedLog { entries, path }
    }

    pub fn push(&mut self, c: Closed) {
        self.entries.retain(|e| e.id != c.id);
        self.entries.insert(0, c);
        self.entries.truncate(CLOSED_KEEP);
    }

    /// Forget `id`, e.g. once it has been reopened.
    pub fn take(&mut self, id: &str) -> Option<Closed> {
        let i = self.entries.iter().position(|e| e.id == id)?;
        Some(self.entries.remove(i))
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(&self.entries).map_err(io::Error::other)?;
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &self.path)
    }
}

/// Whether `ps -o comm=` output for a pid still names `agent`, so a
/// recycled pid is never signalled.
pub fn pid_is_agent(comm: &str, agent: &str) -> bool {
    let comm = comm.trim();
    let base = comm.rsplit('/').next().unwrap_or(comm);
    !agent.is_empty() && (base == agent || base.starts_with(&format!("{agent}-")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SessionRecord;

    fn session(agent: &str, cwd: &str, since: u64) -> Session {
        Session {
            rec: SessionRecord {
                id: "s".into(),
                agent: agent.into(),
                cwd: cwd.into(),
                since,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn stages() {
        use AgentState::*;
        assert_eq!(Stage::of(Blocked, 0, 10_000, 3600), Stage::NeedsYou);
        assert_eq!(Stage::of(Done, 0, 10_000, 3600), Stage::NeedsYou);
        assert_eq!(Stage::of(Working, 0, 10_000, 3600), Stage::Working);
        assert_eq!(Stage::of(Idle, 9_000, 10_000, 3600), Stage::Idle);
        assert_eq!(Stage::of(Idle, 0, 10_000, 3600), Stage::Stale);
        assert_eq!(Stage::of(Unknown, 0, 10_000, 3600), Stage::Stale);
        assert_eq!(Stage::of(Idle, 0, 10_000, 0), Stage::Idle, "0 turns it off");
    }

    #[test]
    fn only_work_in_flight_asks_before_closing() {
        assert!(close_needs_confirm(AgentState::Working));
        assert!(close_needs_confirm(AgentState::Blocked));
        assert!(!close_needs_confirm(AgentState::Idle));
        assert!(!close_needs_confirm(AgentState::Done));
    }

    #[test]
    fn launches_claim_the_matching_new_session() {
        let here = Launch {
            repo: "data".into(),
            dir: Some("/w/data".into()),
            branch: None,
            agent: "claude".into(),
            prompt: String::new(),
            at: 100,
        };
        assert!(here.claims(&session("claude", "/w/data", 101)));
        assert!(here.claims(&session("claude", "/w/data/sub", 98)), "slack");
        assert!(
            !here.claims(&session("codex", "/w/data", 101)),
            "other agent"
        );
        assert!(!here.claims(&session("claude", "/w/data", 50)), "older");
        assert!(!here.claims(&session("claude", "/w/other", 101)));

        let fresh = Launch {
            dir: None,
            branch: Some("THU-1/fix".into()),
            ..here.clone()
        };
        assert!(fresh.claims(&session("claude", "/w/data-worktrees/THU-1-fix", 101)));
        assert!(!fresh.claims(&session("claude", "/w/elsewhere", 101)));
        assert!(fresh.expired(100 + LAUNCH_TTL_SECS + 1));
        assert!(!fresh.expired(100 + LAUNCH_TTL_SECS));
    }

    #[test]
    fn branch_names_come_from_the_prompt() {
        let teams = vec!["THU".to_owned()];
        assert_eq!(
            branch_for("Fix the CSRF timing bug in THU-4821 please", &teams),
            "THU-4821-fix-csrf-timing-bug"
        );
        assert_eq!(
            branch_for("can you add pagination to threads", &teams),
            "add-pagination-threads"
        );
        assert_eq!(branch_for("", &teams), "");
        assert!(branch_for(&"word ".repeat(40), &teams).len() <= BRANCH_MAX);
    }

    #[test]
    fn only_claude_sessions_with_an_id_resume() {
        let cwd = Path::new("/w/my repo");
        assert_eq!(
            resume_line("claude", "248530b3-887a", cwd).as_deref(),
            Some("cd '/w/my repo' && claude --resume 248530b3-887a")
        );
        assert_eq!(resume_line("claude", "pid-42", cwd), None);
        assert_eq!(resume_line("codex", "248530b3", cwd), None);
        assert_eq!(resume_line("claude", "x; rm -rf /", cwd), None);
        assert_eq!(resume_line("claude", "abc", Path::new("")), None);
    }

    #[test]
    fn closed_log_round_trips_newest_first_and_bounded() {
        let dir = std::env::temp_dir().join(format!("artalde-closed-{}", std::process::id()));
        let _fresh = fs::remove_dir_all(&dir);
        let mut log = ClosedLog::load(&dir);
        for i in 0..(CLOSED_KEEP + 5) {
            log.push(Closed {
                id: format!("id{i}"),
                name: format!("n{i}"),
                agent: "claude".into(),
                cwd: "/w".into(),
                repo: None,
                at: i as u64,
            });
        }
        log.push(log.entries[3].clone());
        log.save().unwrap();
        let back = ClosedLog::load(&dir);
        assert_eq!(back.entries.len(), CLOSED_KEEP);
        assert_eq!(back.entries[0].id, format!("id{}", CLOSED_KEEP + 1));
        let mut back = back;
        assert!(back.take("id34").is_some());
        assert!(back.take("id34").is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recycled_pids_are_not_signalled() {
        assert!(pid_is_agent("claude\n", "claude"));
        assert!(pid_is_agent("/opt/bin/codex", "codex"));
        assert!(pid_is_agent("codex-aarch64-apple-darwin", "codex"));
        assert!(!pid_is_agent("zsh", "claude"));
        assert!(!pid_is_agent("", "claude"));
        assert!(!pid_is_agent("claude", ""));
    }
}
