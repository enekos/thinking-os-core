//! What artalde knows about: agent sessions, the repos they work in, PRs and
//! zaindu proposals. A [`Snapshot`] is one consistent view of all of it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What an agent is doing, from the user's point of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    /// Waiting for the user: a permission prompt or a question.
    Blocked,
    /// Finished a turn the user has not looked at yet.
    Done,
    /// Running a turn.
    Working,
    /// Alive, nothing pending.
    Idle,
    /// Seen as a process but never reported.
    #[default]
    Unknown,
}

impl AgentState {
    pub const ALL: [AgentState; 5] = [
        AgentState::Blocked,
        AgentState::Done,
        AgentState::Working,
        AgentState::Idle,
        AgentState::Unknown,
    ];

    /// Higher needs the user sooner: blocked > done > working > idle > unknown.
    pub fn urgency(self) -> u8 {
        match self {
            AgentState::Blocked => 4,
            AgentState::Done => 3,
            AgentState::Working => 2,
            AgentState::Idle => 1,
            AgentState::Unknown => 0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Blocked => "blocked",
            AgentState::Done => "done",
            AgentState::Working => "working",
            AgentState::Idle => "idle",
            AgentState::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<AgentState> {
        AgentState::ALL.into_iter().find(|st| st.as_str() == s)
    }

    /// The state a group shows: its most urgent member's.
    pub fn most_urgent(states: impl IntoIterator<Item = AgentState>) -> Option<AgentState> {
        states.into_iter().max_by_key(|s| s.urgency())
    }
}

/// What the hook persists for one agent session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionRecord {
    pub id: String,
    /// `claude`, `codex`, or whatever reported it.
    pub agent: String,
    pub pid: Option<u32>,
    pub tty: Option<String>,
    pub cwd: PathBuf,
    pub state: AgentState,
    /// When `state` last changed.
    pub since: u64,
    /// When any event last arrived.
    pub updated: u64,
    /// The session's first prompt, as a name.
    pub title: Option<String>,
    pub last_prompt: Option<String>,
    /// The tool call in flight, e.g. `Bash · cargo test`.
    pub activity: Option<String>,
    /// Why the agent is blocked.
    pub message: Option<String>,
    /// The last absolute path a tool touched: where the work really is.
    pub touched: Option<PathBuf>,
    pub transcript: Option<PathBuf>,
    /// The artalde terminal the agent runs in (`ARTALDE_TERMINAL`).
    pub terminal: Option<u32>,
}

/// A session as the UI shows it: the record plus what probes add.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Session {
    #[serde(flatten)]
    pub rec: SessionRecord,
    /// The git worktree the session works in (from `touched`, else `cwd`).
    pub worktree: Option<PathBuf>,
    /// The repository name that worktree belongs to.
    pub repo: Option<String>,
    pub branch: Option<String>,
    /// The task in progress, from `~/.claude/tasks/<id>`.
    pub task: Option<String>,
    pub pending_tasks: u32,
    /// The agent's last words, from the transcript.
    pub last_said: Option<String>,
    /// Found as a process with no hook record.
    pub discovered: bool,
    /// The conversation title Claude Code generated (`ai-title`).
    pub ai_title: Option<String>,
    /// Claude Code's short session name, e.g. `thinking-os-a1`.
    pub handle: Option<String>,
    /// The user's colour tag (`red`, `blue`, …) and label for the session.
    pub tag: Option<String>,
    pub label: Option<String>,
    /// PRs and tickets the conversation is about, most relevant first.
    pub resources: Vec<Resource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Pr,
    Ticket,
}

/// A PR or ticket a session touches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resource {
    pub kind: ResourceKind,
    /// `owner/repo#12`, `gh:owner/repo/issues/12`, or `THU-123`.
    pub id: String,
    pub title: Option<String>,
    /// `open`, `merged`, `open+red`, `In Progress`, … when the wiki knows it.
    pub state: Option<String>,
    pub url: String,
    /// Times the conversation mentions it.
    pub mentions: u32,
}

impl Resource {
    /// `data#7939` / `THU-123`.
    pub fn short(&self) -> String {
        match self.kind {
            ResourceKind::Pr => self.id.rsplit('/').next().unwrap_or(&self.id).to_owned(),
            ResourceKind::Ticket => match self
                .id
                .strip_prefix("gh:")
                .and_then(|id| id.split_once("/issues/"))
            {
                Some((repo, number)) => {
                    format!("{}#{number}", repo.rsplit('/').next().unwrap_or(repo))
                }
                None => self.id.clone(),
            },
        }
    }
}

impl Session {
    /// The best one-line name for the session.
    pub fn name(&self) -> String {
        self.label
            .clone()
            .or_else(|| self.ai_title.clone())
            .or_else(|| self.rec.title.clone())
            .or_else(|| self.task.clone())
            .unwrap_or_else(|| {
                let place = self
                    .repo
                    .clone()
                    .or_else(|| {
                        self.rec
                            .cwd
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                    })
                    .unwrap_or_default();
                format!("{} · {place}", self.rec.agent)
            })
    }
}

/// One checkout of a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    /// Changed or untracked entries.
    pub dirty: u32,
    pub ahead: u32,
    pub behind: u32,
}

/// A repository and its worktrees: where agents work.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Space {
    pub repo: String,
    /// `owner/repo` on GitHub, from the `origin` remote.
    pub slug: Option<String>,
    /// The main checkout.
    pub root: PathBuf,
    pub worktrees: Vec<Worktree>,
}

/// An open pull request authored by the user.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Pr {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub draft: bool,
    pub updated: String,
}

/// One zaindu proposal line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct InboxItem {
    pub file: PathBuf,
    /// Zero-based line index in `file`.
    pub line: usize,
    pub done: bool,
    pub time: String,
    /// `review`, `red`, `approved`, `merged`, …
    pub kind: String,
    /// `owner/repo#123`, a Linear id, …
    pub target: String,
    pub links: Vec<String>,
    pub detail: String,
    pub url: Option<String>,
}

/// Everything the UI draws, as of `now`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Snapshot {
    /// Bumped whenever any part changes.
    pub version: u64,
    pub now: u64,
    pub sessions: Vec<Session>,
    pub spaces: Vec<Space>,
    pub prs: Vec<Pr>,
    pub inbox: Vec<InboxItem>,
    /// Probe failures worth showing.
    pub errors: Vec<String>,
}

impl Snapshot {
    pub fn count(&self, state: AgentState) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.rec.state == state)
            .count()
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.rec.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_urgent_wins() {
        use AgentState::*;
        assert_eq!(
            AgentState::most_urgent([Idle, Blocked, Working]),
            Some(Blocked)
        );
        assert_eq!(AgentState::most_urgent([Idle, Done, Working]), Some(Done));
        assert_eq!(AgentState::most_urgent([]), None);
    }

    #[test]
    fn state_round_trips() {
        for s in AgentState::ALL {
            assert_eq!(AgentState::parse(s.as_str()), Some(s));
        }
    }
}
