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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken_at: String,
    pub login: String,
    pub prs: BTreeMap<String, Pr>,
}

impl Snapshot {
    pub fn sorted(&self) -> Vec<&Pr> {
        let mut out: Vec<&Pr> = self.prs.values().collect();
        out.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.id.cmp(&b.id)));
        out
    }
}
