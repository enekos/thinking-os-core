//! Agent-assisted, human-in-the-loop review, after adar's: an agent reads
//! the PR (diff + context file) read-only and *proposes* comments; the user
//! accepts, edits or dismisses each one and adds their own; nothing reaches
//! GitHub until the user submits the review, as one review with every
//! accepted comment on its line.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::github::{DiffFile, Invocation, LineKind, PrDetail, PrRef, TimelineItem};
use crate::text::clip;

/// Which side of the diff a comment sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    /// The old file (removed lines).
    Left,
    /// The new file (added and context lines).
    #[default]
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Agent,
    #[default]
    Me,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftState {
    /// From the agent, waiting for a decision.
    #[default]
    Proposed,
    /// Will be part of the review.
    Accepted,
    Dismissed,
}

/// One comment on its way to a review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub id: u64,
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub body: String,
    /// `blocker`, `major`, `minor`, `nit` (agent drafts).
    pub severity: String,
    pub title: String,
    /// Replacement for the commented line(s), shown as a GitHub suggestion.
    pub suggestion: Option<String>,
    pub origin: Origin,
    pub state: DraftState,
}

impl Draft {
    /// The comment as GitHub will show it.
    pub fn rendered(&self) -> String {
        let mut out = String::new();
        if !self.title.trim().is_empty() {
            let sev = if self.severity.is_empty() {
                String::new()
            } else {
                format!("{}: ", self.severity)
            };
            out.push_str(&format!("**{sev}{}**\n\n", self.title.trim()));
        }
        out.push_str(self.body.trim());
        if let Some(s) = self.suggestion.as_ref().filter(|s| !s.trim().is_empty()) {
            out.push_str(&format!(
                "\n\n```suggestion\n{}\n```",
                s.trim_end_matches('\n')
            ));
        }
        out
    }
}

/// The review being prepared for one PR.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PendingReview {
    pub drafts: Vec<Draft>,
    pub summary: String,
    /// The agent's suggested verdict: `approve`, `request_changes`, `comment`.
    pub agent_verdict: String,
    /// Findings the agent could not anchor to a diff line.
    pub general_notes: Vec<String>,
    pub next_id: u64,
}

impl PendingReview {
    pub fn add(&mut self, mut d: Draft) -> u64 {
        self.next_id += 1;
        d.id = self.next_id;
        self.drafts.push(d);
        self.next_id
    }

    pub fn accepted(&self) -> impl Iterator<Item = &Draft> {
        self.drafts
            .iter()
            .filter(|d| d.state == DraftState::Accepted)
    }

    pub fn proposed(&self) -> usize {
        self.drafts
            .iter()
            .filter(|d| d.state == DraftState::Proposed)
            .count()
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Draft> {
        self.drafts.iter_mut().find(|d| d.id == id)
    }

    /// Merge an agent run: its comments arrive as proposals, anchored ones
    /// only; the rest become general notes. Earlier proposals from the agent
    /// that were never decided are replaced.
    pub fn absorb(&mut self, run: AgentReview, files: &[DiffFile]) -> (usize, usize) {
        self.drafts
            .retain(|d| !(d.origin == Origin::Agent && d.state == DraftState::Proposed));
        let mut anchored = 0;
        let mut loose = 0;
        for f in run.findings {
            let side = if f.side.eq_ignore_ascii_case("left") {
                Side::Left
            } else {
                Side::Right
            };
            let line = f.line.unwrap_or(0);
            let ok = line > 0 && commentable(files, &f.path, line, side);
            if ok {
                anchored += 1;
                self.add(Draft {
                    path: f.path,
                    line,
                    side,
                    body: f.body,
                    severity: f.severity.to_lowercase(),
                    title: f.title,
                    suggestion: f.suggestion,
                    origin: Origin::Agent,
                    state: DraftState::Proposed,
                    id: 0,
                });
            } else {
                loose += 1;
                let at = if f.path.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (`{}{}`)",
                        f.path,
                        f.line.map(|l| format!(":{l}")).unwrap_or_default()
                    )
                };
                self.general_notes
                    .push(format!("**{}**{at} — {}", f.title.trim(), f.body.trim()));
            }
        }
        if self.summary.trim().is_empty() {
            self.summary = run.summary;
        }
        self.agent_verdict = run.verdict;
        (anchored, loose)
    }
}

/// Whether GitHub accepts a comment on `line` of `path` on `side`: the line
/// must be in one of the diff's hunks.
pub fn commentable(files: &[DiffFile], path: &str, line: u32, side: Side) -> bool {
    files
        .iter()
        .filter(|f| f.path == path)
        .flat_map(|f| &f.lines)
        .any(|l| match side {
            Side::Right => {
                l.new == Some(line) && matches!(l.kind, LineKind::Add | LineKind::Context)
            }
            Side::Left => {
                l.old == Some(line) && matches!(l.kind, LineKind::Remove | LineKind::Context)
            }
        })
}

/// What an agent run returned.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AgentReview {
    pub summary: String,
    pub verdict: String,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Finding {
    pub path: String,
    pub line: Option<u32>,
    pub side: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub suggestion: Option<String>,
}

/// The JSON object in an agent's reply: a ```json block, else the outermost
/// `{…}`. Agents wrap answers in prose; this tolerates it.
pub fn parse_agent_output(text: &str) -> Result<AgentReview, String> {
    let fenced = text
        .split("```json")
        .nth(1)
        .and_then(|r| r.split("```").next());
    let candidate = fenced.map(str::to_owned).or_else(|| {
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        (end > start).then(|| text.get(start..=end).unwrap_or_default().to_owned())
    });
    let Some(json) = candidate else {
        return Err(format!("no JSON in the agent's reply: {}", clip(text, 200)));
    };
    serde_json::from_str::<AgentReview>(json.trim()).map_err(|e| {
        format!(
            "agent reply is not the review JSON ({e}): {}",
            clip(&json, 200)
        )
    })
}

/// `pr.md`: what a reviewer needs besides the diff.
pub fn context_markdown(d: &PrDetail) -> String {
    let mut s = String::new();
    let pr = d.pr.as_ref().map(ToString::to_string).unwrap_or_default();
    s.push_str(&format!("# {pr} — {}\n\n", d.title));
    s.push_str(&format!(
        "Author @{} · {} → {} · +{} −{} in {} files\n\n",
        d.author, d.head, d.base, d.additions, d.deletions, d.changed_files
    ));
    s.push_str("## Description\n\n");
    s.push_str(if d.body.trim().is_empty() {
        "(none)"
    } else {
        d.body.trim()
    });
    s.push_str("\n\n");
    let open: Vec<_> = d.threads.iter().filter(|t| !t.resolved).collect();
    if !open.is_empty() {
        s.push_str("## Open review threads\n\n");
        for t in open {
            s.push_str(&format!("### {}:{}\n\n", t.path, t.line.unwrap_or(0)));
            for c in &t.comments {
                s.push_str(&format!(
                    "- @{}: {}\n",
                    c.author,
                    c.body.trim().replace('\n', "\n  ")
                ));
            }
            s.push('\n');
        }
    }
    let failing: Vec<_> = d.checks.iter().filter(|c| c.failed()).collect();
    if !failing.is_empty() {
        s.push_str("## Failing checks\n\n");
        for c in failing {
            s.push_str(&format!("- {} / {}\n", c.workflow, c.name));
        }
        s.push('\n');
    }
    let human: Vec<_> = d
        .timeline
        .iter()
        .filter_map(|t| match t {
            TimelineItem::Comment(c) | TimelineItem::Review { comment: c, .. }
                if !c.bot && !c.body.trim().is_empty() =>
            {
                Some(c)
            }
            _ => None,
        })
        .collect();
    if !human.is_empty() {
        s.push_str("## Conversation\n\n");
        for c in human {
            s.push_str(&format!(
                "- @{}: {}\n",
                c.author,
                clip(&c.body.replace('\n', " "), 600)
            ));
        }
    }
    s
}

/// The review prompt. Read-only, structured output, no posting.
pub fn prompt(pr: &PrRef, dir: &Path, checkout: Option<&Path>) -> String {
    let code = match checkout {
        Some(c) => format!(
            "A checkout of the PR's head branch is at {} — read surrounding code there when the diff alone is not enough.",
            c.display()
        ),
        None => "There is no checkout; review from the diff.".to_owned(),
    };
    format!(
        r#"You are reviewing pull request {pr}. Read {dir}/pr.md (description, open threads, failing checks, conversation) and {dir}/pr.diff (the full unified diff). {code}

Rules: do not edit, create, commit, push or post anything. Only read. Your output is a proposal a human reviews line by line before anything reaches GitHub.

Find real problems: bugs, security issues, data loss, broken contracts, missing tests for changed behaviour, and confusing code that will cost the next reader. Skip style nits a formatter or linter would catch. Prefer few, specific findings over many vague ones. Do not repeat points already raised in open threads.

Anchor every finding to a line that appears in the diff: for added or unchanged lines use "side": "RIGHT" and the new-file line number; for removed lines use "LEFT" and the old-file line number. Only suggest a replacement ("suggestion") when it is a drop-in replacement for exactly that one line.

Answer with only this JSON, in a ```json block:
{{
  "summary": "2-4 sentences: what the PR does and your overall assessment",
  "verdict": "approve" | "request_changes" | "comment",
  "findings": [
    {{ "path": "src/file.ts", "line": 42, "side": "RIGHT", "severity": "blocker" | "major" | "minor" | "nit",
       "title": "short headline", "body": "why it matters and what to do", "suggestion": "optional replacement line" }}
  ]
}}"#,
        dir = dir.display()
    )
}

/// `reviews/<owner>__<repo>__<n>/` under the state dir.
pub fn review_dir(state_dir: &Path, pr: &PrRef) -> PathBuf {
    let key: String = format!("{}__{}", pr.repo.replace('/', "__"), pr.number)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    state_dir.join("reviews").join(key)
}

pub fn load(dir: &Path) -> PendingReview {
    fs::read(dir.join("pending.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, review: &PendingReview) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("pending.json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(review).map_err(io::Error::other)?,
    )?;
    fs::rename(&tmp, dir.join("pending.json"))
}

/// Write `pr.md` and `pr.diff` for the agent.
pub fn write_context(dir: &Path, detail: &PrDetail, diff: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(dir.join("pr.md"), context_markdown(detail))?;
    fs::write(dir.join("pr.diff"), diff)
}

/// `APPROVE`, `REQUEST_CHANGES`, `COMMENT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verdict {
    #[default]
    Comment,
    Approve,
    RequestChanges,
}

impl Verdict {
    pub fn event(self) -> &'static str {
        match self {
            Verdict::Comment => "COMMENT",
            Verdict::Approve => "APPROVE",
            Verdict::RequestChanges => "REQUEST_CHANGES",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Comment => "Comment",
            Verdict::Approve => "Approve",
            Verdict::RequestChanges => "Request changes",
        }
    }

    pub fn parse(s: &str) -> Option<Verdict> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "approve" | "approved" => Some(Verdict::Approve),
            "request_changes" | "changes_requested" => Some(Verdict::RequestChanges),
            "comment" => Some(Verdict::Comment),
            _ => None,
        }
    }
}

/// One GitHub review carrying every accepted comment on its line.
pub fn submit_invocation(
    detail: &PrDetail,
    verdict: Verdict,
    body: &str,
    review: &PendingReview,
) -> Result<Invocation, String> {
    if detail.node_id.is_empty() {
        return Err("the PR is still loading".into());
    }
    if verdict == Verdict::RequestChanges
        && body.trim().is_empty()
        && review.accepted().next().is_none()
    {
        return Err("requesting changes needs a summary or at least one comment".into());
    }
    let threads: Vec<Value> = review
        .accepted()
        .map(|d| json!({ "path": d.path, "line": d.line, "side": d.side, "body": d.rendered() }))
        .collect();
    let mut input = json!({ "pullRequestId": detail.node_id, "event": verdict.event(), "body": body, "threads": threads });
    if !detail.head_oid.is_empty() {
        input["commitOID"] = json!(detail.head_oid);
    }
    Ok(Invocation {
        args: vec!["api".into(), "graphql".into(), "--input".into(), "-".into()],
        stdin: Some(
            json!({
                "query": "mutation($i: AddPullRequestReviewInput!) { addPullRequestReview(input: $i) { pullRequestReview { id url } } }",
                "variables": { "i": input },
            })
            .to_string(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_diff;

    fn files() -> Vec<DiffFile> {
        parse_diff("diff --git a/src/a.ts b/src/a.ts\n@@ -10,3 +10,4 @@\n ctx\n-old\n+new\n+more\n")
    }

    #[test]
    fn commentable_lines_are_in_hunks() {
        let f = files();
        assert!(commentable(&f, "src/a.ts", 10, Side::Right), "context");
        assert!(commentable(&f, "src/a.ts", 12, Side::Right), "added");
        assert!(commentable(&f, "src/a.ts", 11, Side::Left), "removed");
        assert!(!commentable(&f, "src/a.ts", 40, Side::Right));
        assert!(!commentable(&f, "other.ts", 10, Side::Right));
    }

    #[test]
    fn agent_output_parses_through_prose() {
        let reply = "Here is my review.\n```json\n{\"summary\":\"ok\",\"verdict\":\"request_changes\",\"findings\":[{\"path\":\"src/a.ts\",\"line\":12,\"side\":\"RIGHT\",\"severity\":\"Major\",\"title\":\"Null\",\"body\":\"may be null\"},{\"path\":\"src/a.ts\",\"line\":99,\"title\":\"Far\",\"body\":\"not in diff\"}]}\n```\nthanks";
        let run = parse_agent_output(reply).unwrap();
        assert_eq!(run.findings.len(), 2);
        let mut p = PendingReview::default();
        let (anchored, loose) = p.absorb(run, &files());
        assert_eq!((anchored, loose), (1, 1));
        assert_eq!(p.drafts[0].severity, "major");
        assert_eq!(p.drafts[0].state, DraftState::Proposed);
        assert_eq!(p.agent_verdict, "request_changes");
        assert!(p.general_notes[0].contains("Far"));
        assert!(parse_agent_output("no json here").is_err());
        assert!(parse_agent_output("{\"summary\": 1}").is_err());
    }

    #[test]
    fn rerun_replaces_only_undecided_agent_drafts() {
        let mut p = PendingReview::default();
        let mine = p.add(Draft {
            path: "src/a.ts".into(),
            line: 12,
            body: "mine".into(),
            state: DraftState::Accepted,
            ..Draft::default()
        });
        let run = |t: &str| AgentReview {
            findings: vec![Finding {
                path: "src/a.ts".into(),
                line: Some(12),
                title: t.into(),
                ..Finding::default()
            }],
            ..AgentReview::default()
        };
        p.absorb(run("first"), &files());
        let first = p.drafts.iter().find(|d| d.title == "first").unwrap().id;
        p.get_mut(first).unwrap().state = DraftState::Accepted;
        p.absorb(run("second"), &files());
        let titles: Vec<&str> = p.drafts.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(
            titles,
            ["", "first", "second"],
            "accepted drafts and mine stay"
        );
        assert!(p.drafts.iter().any(|d| d.id == mine));
    }

    #[test]
    fn submit_carries_accepted_comments_only() {
        let d = PrDetail {
            node_id: "PR_1".into(),
            head_oid: "abc".into(),
            ..PrDetail::default()
        };
        let mut p = PendingReview::default();
        p.add(Draft {
            path: "a".into(),
            line: 3,
            title: "Bug".into(),
            severity: "major".into(),
            body: "fix".into(),
            suggestion: Some("x();".into()),
            state: DraftState::Accepted,
            ..Draft::default()
        });
        p.add(Draft {
            path: "b".into(),
            line: 4,
            body: "no".into(),
            state: DraftState::Dismissed,
            ..Draft::default()
        });
        p.add(Draft {
            path: "c".into(),
            line: 5,
            body: "maybe".into(),
            state: DraftState::Proposed,
            origin: Origin::Agent,
            ..Draft::default()
        });
        let inv = submit_invocation(&d, Verdict::Comment, "summary", &p).unwrap();
        let v: Value = serde_json::from_str(inv.stdin.as_deref().unwrap()).unwrap();
        let threads = v["variables"]["i"]["threads"].as_array().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["side"], "RIGHT");
        assert_eq!(
            threads[0]["body"],
            "**major: Bug**\n\nfix\n\n```suggestion\nx();\n```"
        );
        assert_eq!(v["variables"]["i"]["commitOID"], "abc");
        assert!(
            submit_invocation(&d, Verdict::RequestChanges, "", &PendingReview::default()).is_err()
        );
    }

    #[test]
    fn verdicts_parse() {
        assert_eq!(
            Verdict::parse("request-changes"),
            Some(Verdict::RequestChanges)
        );
        assert_eq!(Verdict::parse("APPROVE"), Some(Verdict::Approve));
        assert_eq!(Verdict::parse("lgtm"), None);
    }
}
