use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Review {
    None,
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Checks {
    Unknown,
    Pending,
    Green,
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycle {
    Open,
    Merged,
    Closed,
}

impl Lifecycle {
    pub fn label(self) -> &'static str {
        match self {
            Lifecycle::Open => "open",
            Lifecycle::Merged => "merged",
            Lifecycle::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pr {
    pub id: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub lifecycle: Lifecycle,
    pub draft: bool,
    pub mine: bool,
    pub author: String,
    pub head: String,
    pub base: String,
    pub updated: String,
    pub merged_at: String,
    pub merged_by: String,
    pub review: Review,
    pub review_by: Vec<String>,
    pub checks: Checks,
    pub failing: Vec<String>,
    pub review_requested_of_me: bool,
}

impl Pr {
    pub fn nwo(&self) -> &str {
        self.repo.strip_prefix("gh:").unwrap_or(&self.repo)
    }

    pub fn slug(&self) -> String {
        format!("{}#{}", self.nwo(), self.number)
    }

    pub fn state_label(&self) -> String {
        let mut bits = Vec::new();
        if self.draft && self.lifecycle == Lifecycle::Open {
            bits.push("draft".to_string());
        }
        match self.review {
            Review::Approved => bits.push("approved".to_string()),
            Review::ChangesRequested => bits.push("changes-requested".to_string()),
            Review::None => {}
        }
        if self.checks == Checks::Red {
            bits.push("red".to_string());
        }
        bits.join("+")
    }
}

/// A Linear issue assigned to me, as the watcher last saw it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub url: String,
    /// The workflow state's own name ("Code Review"); `state_type` is the
    /// fixed category Linear files it under ("started").
    pub state: String,
    pub state_type: String,
    pub priority: i64,
    /// Graph id of the Linear project (`lin:project:…`), or empty.
    pub project: String,
    pub project_name: String,
    pub updated: String,
    pub blocked_by: Vec<String>,
    pub comment_at: String,
    /// Empty when the latest comment is my own or an integration's.
    pub comment_by: String,
    pub prs: Vec<String>,
}

impl Issue {
    pub fn is_open(&self) -> bool {
        matches!(self.state_type.as_str(), "triage" | "backlog" | "unstarted" | "started")
    }
}

/// Bumped whenever a poll starts seeing PRs the previous version could not.
/// A snapshot from a narrower window is re-seeded rather than diffed: every
/// PR that was merely out of sight would otherwise arrive as a fresh event.
pub const WINDOW: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken_at: String,
    pub login: String,
    /// Which [`WINDOW`] this snapshot was read through. Absent in older state.
    #[serde(default)]
    pub window: u32,
    pub prs: BTreeMap<String, Pr>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub issues: BTreeMap<String, Issue>,
    /// When Linear was last read in full. Empty until the first such poll,
    /// which seeds silently.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issues_at: String,
}

impl Snapshot {
    pub fn sorted(&self) -> Vec<&Pr> {
        let mut out: Vec<&Pr> = self.prs.values().collect();
        out.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
        out
    }

    /// Open issues first by priority (urgent on top, unprioritised last), then
    /// finished ones.
    pub fn sorted_issues(&self) -> Vec<&Issue> {
        let rank = |i: &Issue| (!i.is_open(), if i.priority == 0 { 5 } else { i.priority });
        let mut out: Vec<&Issue> = self.issues.values().collect();
        out.sort_by(|a, b| rank(a).cmp(&rank(b)).then(b.updated.cmp(&a.updated)).then(a.id.cmp(&b.id)));
        out
    }
}
