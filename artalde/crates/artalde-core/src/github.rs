//! Pull requests through the `gh` CLI, after adar (the-yolo-company/adar):
//! the same inbox buckets, detail query, filter qualifiers and write
//! actions. This module builds requests and parses answers; running `gh`
//! is the caller's job, so all of it is testable offline.

use std::fmt;

use serde_json::{Value, json};

use crate::text::clip;

/// A PR address: `owner/repo#123`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrRef {
    pub repo: String,
    pub number: u64,
}

impl PrRef {
    /// `owner/repo#12`, `https://github.com/owner/repo/pull/12[/...]`.
    pub fn parse(s: &str) -> Option<PrRef> {
        let s = s.trim();
        let (repo, num) = if let Some(rest) = s.split("github.com/").nth(1) {
            let mut parts = rest.split('/');
            let (owner, name, kind, num) =
                (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            if kind != "pull" && kind != "issues" {
                return None;
            }
            (format!("{owner}/{name}"), num.split(['#', '?']).next()?)
        } else {
            let (repo, num) = s.split_once('#')?;
            (repo.to_owned(), num)
        };
        let ok = repo.split('/').count() == 2
            && repo.split('/').all(|p| {
                !p.is_empty()
                    && p.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            });
        let number = num.parse().ok().filter(|n| *n > 0)?;
        ok.then_some(PrRef { repo, number })
    }

    pub fn owner_name(&self) -> (&str, &str) {
        self.repo.split_once('/').unwrap_or((&self.repo, ""))
    }

    pub fn url(&self) -> String {
        format!("https://github.com/{}/pull/{}", self.repo, self.number)
    }

    pub fn short(&self) -> String {
        format!(
            "{}#{}",
            self.repo.rsplit('/').next().unwrap_or(&self.repo),
            self.number
        )
    }
}

impl fmt::Display for PrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// Inbox sections, as adar has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bucket {
    Review,
    Mine,
    Involved,
    Merged,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [
        Bucket::Review,
        Bucket::Mine,
        Bucket::Involved,
        Bucket::Merged,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Bucket::Review => "Review requested",
            Bucket::Mine => "Mine",
            Bucket::Involved => "Involved",
            Bucket::Merged => "Recently merged",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Bucket::Review => "review",
            Bucket::Mine => "mine",
            Bucket::Involved => "involved",
            Bucket::Merged => "merged",
        }
    }

    /// The GitHub search; `since` is `YYYY-MM-DD` for merged.
    pub fn query(self, since: &str) -> String {
        match self {
            Bucket::Review => "is:pr is:open archived:false review-requested:@me sort:updated-desc".into(),
            Bucket::Mine => "is:pr is:open archived:false author:@me sort:updated-desc".into(),
            Bucket::Involved => {
                "is:pr is:open archived:false involves:@me -author:@me -review-requested:@me sort:updated-desc".into()
            }
            Bucket::Merged => format!("is:pr is:merged archived:false involves:@me merged:>={since} sort:updated-desc"),
        }
    }
}

const SUMMARY_FIELDS: &str = "
  id number title url isDraft state createdAt updatedAt
  headRefName baseRefName additions deletions reviewDecision
  repository { nameWithOwner }
  author { __typename login }
  labels(first: 6) { nodes { name color } }
  comments { totalCount }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }";

/// The GraphQL request body (for `gh api graphql --input -`) for a bucket.
pub fn search_request(bucket: Bucket, since: &str, limit: u32) -> Value {
    json!({
        "query": format!("query($q: String!, $limit: Int!) {{ viewer {{ login }} search(type: ISSUE, first: $limit, query: $q) {{ nodes {{ ... on PullRequest {{ {SUMMARY_FIELDS} }} }} }} }}"),
        "variables": { "q": bucket.query(since), "limit": limit },
    })
}

/// One inbox row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrRow {
    pub pr: Option<PrRef>,
    pub title: String,
    pub url: String,
    pub draft: bool,
    /// `OPEN`, `MERGED`, `CLOSED`.
    pub state: String,
    pub author: String,
    pub bot: bool,
    pub created: String,
    pub updated: String,
    pub head: String,
    pub base: String,
    pub additions: u64,
    pub deletions: u64,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED`, or empty.
    pub review: String,
    /// `SUCCESS`, `FAILURE`, `PENDING`, `ERROR`, or empty.
    pub checks: String,
    pub labels: Vec<String>,
    pub comments: u64,
}

fn s(v: &Value, ptr: &str) -> String {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn n(v: &Value, ptr: &str) -> u64 {
    v.pointer(ptr).and_then(Value::as_u64).unwrap_or(0)
}

fn b(v: &Value, ptr: &str) -> bool {
    v.pointer(ptr).and_then(Value::as_bool).unwrap_or(false)
}

fn nodes<'a>(v: &'a Value, ptr: &str) -> impl Iterator<Item = &'a Value> {
    v.pointer(ptr)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// GraphQL errors, if the answer carries any.
pub fn errors(v: &Value) -> Option<String> {
    let errs: Vec<String> = nodes(v, "/errors")
        .map(|e| s(e, "/message"))
        .filter(|m| !m.is_empty())
        .collect();
    (!errs.is_empty()).then(|| errs.join("; "))
}

/// `(viewer login, rows)` from a search answer.
pub fn parse_search(v: &Value) -> (String, Vec<PrRow>) {
    let login = s(v, "/data/viewer/login");
    let rows = nodes(v, "/data/search/nodes")
        .filter(|n| n.get("number").is_some())
        .map(parse_row)
        .collect();
    (login, rows)
}

fn parse_row(v: &Value) -> PrRow {
    let repo = s(v, "/repository/nameWithOwner");
    PrRow {
        pr: Some(PrRef {
            repo,
            number: n(v, "/number"),
        })
        .filter(|p| p.number > 0 && !p.repo.is_empty()),
        title: s(v, "/title"),
        url: s(v, "/url"),
        draft: b(v, "/isDraft"),
        state: s(v, "/state"),
        author: s(v, "/author/login"),
        bot: s(v, "/author/__typename") == "Bot" || s(v, "/author/login").ends_with("[bot]"),
        created: s(v, "/createdAt"),
        updated: s(v, "/updatedAt"),
        head: s(v, "/headRefName"),
        base: s(v, "/baseRefName"),
        additions: n(v, "/additions"),
        deletions: n(v, "/deletions"),
        review: s(v, "/reviewDecision"),
        checks: nodes(v, "/commits/nodes")
            .next()
            .map(|c| s(c, "/commit/statusCheckRollup/state"))
            .unwrap_or_default(),
        labels: nodes(v, "/labels/nodes").map(|l| s(l, "/name")).collect(),
        comments: n(v, "/comments/totalCount"),
    }
}

const DETAIL_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    viewerPermission squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed
    pullRequest(number: $number) {
      id number title url body state isDraft createdAt updatedAt
      headRefName baseRefName additions deletions changedFiles
      mergeable mergeStateStatus reviewDecision viewerDidAuthor viewerCanUpdateBranch
      author { login }
      autoMergeRequest { mergeMethod }
      labels(first: 10) { nodes { name color } }
      reviewRequests(first: 10) { nodes { requestedReviewer { __typename ... on User { login } ... on Team { name } } } }
      latestReviews(first: 10) { nodes { author { login } state } }
      timelineItems(last: 100, itemTypes: [ISSUE_COMMENT, PULL_REQUEST_REVIEW, MERGED_EVENT, CLOSED_EVENT, REOPENED_EVENT, HEAD_REF_FORCE_PUSHED_EVENT, READY_FOR_REVIEW_EVENT, PULL_REQUEST_COMMIT]) {
        nodes {
          __typename
          ... on IssueComment { id author { login } body createdAt url }
          ... on PullRequestReview { id author { login } body state createdAt url }
          ... on MergedEvent { actor { login } createdAt }
          ... on ClosedEvent { actor { login } createdAt }
          ... on ReopenedEvent { actor { login } createdAt }
          ... on HeadRefForcePushedEvent { actor { login } createdAt }
          ... on ReadyForReviewEvent { actor { login } createdAt }
          ... on PullRequestCommit { commit { oid messageHeadline committedDate author { name user { login } } } }
        }
      }
      reviewThreads(first: 100) {
        nodes {
          id isResolved isOutdated path line originalLine viewerCanResolve viewerCanUnresolve viewerCanReply
          comments(first: 50) { nodes { id author { login } body createdAt url diffHunk } }
        }
      }
      commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(first: 100) { nodes {
        __typename
        ... on CheckRun { databaseId name status conclusion detailsUrl startedAt completedAt checkSuite { workflowRun { databaseId workflow { name } } } }
        ... on StatusContext { context state targetUrl description createdAt }
      } } } } } }
    }
  }
}";

pub fn detail_request(pr: &PrRef) -> Value {
    let (owner, name) = pr.owner_name();
    json!({ "query": DETAIL_QUERY, "variables": { "owner": owner, "name": name, "number": pr.number } })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    pub fn flag(self) -> &'static str {
        match self {
            MergeMethod::Squash => "--squash",
            MergeMethod::Merge => "--merge",
            MergeMethod::Rebase => "--rebase",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MergeMethod::Squash => "Squash and merge",
            MergeMethod::Merge => "Create a merge commit",
            MergeMethod::Rebase => "Rebase and merge",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created: String,
    pub url: String,
    pub diff_hunk: String,
}

/// One entry of the conversation, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineItem {
    Comment(Comment),
    /// A review: `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, …
    Review {
        comment: Comment,
        state: String,
    },
    Event {
        actor: String,
        what: String,
        at: String,
    },
    Commit {
        oid: String,
        headline: String,
        author: String,
        at: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub path: String,
    pub line: Option<u64>,
    pub can_resolve: bool,
    pub can_unresolve: bool,
    pub can_reply: bool,
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub workflow: String,
    /// `SUCCESS`, `FAILURE`, `IN_PROGRESS`, `PENDING`, `SKIPPED`, …
    pub verdict: String,
    pub url: String,
    /// Actions job id, when it is one (for logs and re-runs).
    pub job_id: Option<u64>,
    pub run_id: Option<u64>,
    pub started: String,
    pub completed: String,
}

impl Check {
    pub fn failed(&self) -> bool {
        matches!(
            self.verdict.as_str(),
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "STARTUP_FAILURE" | "ACTION_REQUIRED"
        )
    }

    pub fn running(&self) -> bool {
        matches!(
            self.verdict.as_str(),
            "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" | "REQUESTED" | "EXPECTED"
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrDetail {
    pub pr: Option<PrRef>,
    pub node_id: String,
    pub viewer: String,
    pub title: String,
    pub url: String,
    pub body: String,
    pub state: String,
    pub draft: bool,
    pub author: String,
    pub created: String,
    pub updated: String,
    pub head: String,
    pub base: String,
    pub head_oid: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    /// `MERGEABLE`, `CONFLICTING`, `UNKNOWN`.
    pub mergeable: String,
    /// `CLEAN`, `BEHIND`, `BLOCKED`, `DIRTY`, `UNSTABLE`, …
    pub merge_state: String,
    pub review: String,
    pub viewer_is_author: bool,
    pub can_update_branch: bool,
    pub can_write: bool,
    pub auto_merge: Option<String>,
    pub merge_methods: Vec<MergeMethod>,
    pub labels: Vec<String>,
    pub requested: Vec<String>,
    /// `(login, state)` of each reviewer's latest review.
    pub reviews: Vec<(String, String)>,
    pub timeline: Vec<TimelineItem>,
    pub threads: Vec<Thread>,
    pub checks_state: String,
    pub checks: Vec<Check>,
}

impl PrDetail {
    pub fn open(&self) -> bool {
        self.state == "OPEN"
    }

    /// Workflow runs with a failed job, for "re-run failed".
    pub fn failed_runs(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .checks
            .iter()
            .filter(|c| c.failed())
            .filter_map(|c| c.run_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

fn comment(v: &Value) -> Comment {
    Comment {
        id: s(v, "/id"),
        author: s(v, "/author/login"),
        body: s(v, "/body"),
        created: s(v, "/createdAt"),
        url: s(v, "/url"),
        diff_hunk: s(v, "/diffHunk"),
    }
}

pub fn parse_detail(v: &Value) -> Option<PrDetail> {
    let repo_v = v.pointer("/data/repository")?;
    let p = repo_v.get("pullRequest").filter(|p| !p.is_null())?;
    let event = |what: &str, it: &Value| TimelineItem::Event {
        actor: s(it, "/actor/login"),
        what: what.to_owned(),
        at: s(it, "/createdAt"),
    };
    let timeline = nodes(p, "/timelineItems/nodes")
        .filter_map(|it| {
            Some(match s(it, "/__typename").as_str() {
                "IssueComment" => TimelineItem::Comment(comment(it)),
                "PullRequestReview" => TimelineItem::Review {
                    comment: comment(it),
                    state: s(it, "/state"),
                },
                "MergedEvent" => event("merged", it),
                "ClosedEvent" => event("closed", it),
                "ReopenedEvent" => event("reopened", it),
                "HeadRefForcePushedEvent" => event("force-pushed", it),
                "ReadyForReviewEvent" => event("marked ready for review", it),
                "PullRequestCommit" => TimelineItem::Commit {
                    oid: s(it, "/commit/oid"),
                    headline: s(it, "/commit/messageHeadline"),
                    author: Some(s(it, "/commit/author/user/login"))
                        .filter(|l| !l.is_empty())
                        .unwrap_or_else(|| s(it, "/commit/author/name")),
                    at: s(it, "/commit/committedDate"),
                },
                _ => return None,
            })
        })
        .collect();
    let threads = nodes(p, "/reviewThreads/nodes")
        .map(|t| Thread {
            id: s(t, "/id"),
            resolved: b(t, "/isResolved"),
            outdated: b(t, "/isOutdated"),
            path: s(t, "/path"),
            line: t
                .get("line")
                .and_then(Value::as_u64)
                .or_else(|| t.get("originalLine").and_then(Value::as_u64)),
            can_resolve: b(t, "/viewerCanResolve"),
            can_unresolve: b(t, "/viewerCanUnresolve"),
            can_reply: b(t, "/viewerCanReply"),
            comments: nodes(t, "/comments/nodes").map(comment).collect(),
        })
        .collect();
    let commit = nodes(p, "/commits/nodes").next();
    let checks = commit
        .map(|c| {
            nodes(c, "/commit/statusCheckRollup/contexts/nodes")
                .map(|x| {
                    if s(x, "/__typename") == "StatusContext" {
                        Check {
                            name: s(x, "/context"),
                            workflow: "Statuses".into(),
                            verdict: s(x, "/state"),
                            url: s(x, "/targetUrl"),
                            started: s(x, "/createdAt"),
                            ..Check::default()
                        }
                    } else {
                        let conclusion = s(x, "/conclusion");
                        Check {
                            name: s(x, "/name"),
                            workflow: Some(s(x, "/checkSuite/workflowRun/workflow/name"))
                                .filter(|w| !w.is_empty())
                                .unwrap_or_else(|| "Checks".into()),
                            verdict: if conclusion.is_empty() {
                                s(x, "/status")
                            } else {
                                conclusion
                            },
                            url: s(x, "/detailsUrl"),
                            job_id: x.get("databaseId").and_then(Value::as_u64),
                            run_id: x
                                .pointer("/checkSuite/workflowRun/databaseId")
                                .and_then(Value::as_u64),
                            started: s(x, "/startedAt"),
                            completed: s(x, "/completedAt"),
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut merge_methods = Vec::new();
    for (key, m) in [
        ("/squashMergeAllowed", MergeMethod::Squash),
        ("/mergeCommitAllowed", MergeMethod::Merge),
        ("/rebaseMergeAllowed", MergeMethod::Rebase),
    ] {
        if b(repo_v, key) {
            merge_methods.push(m);
        }
    }
    let repo = p
        .pointer("/url")
        .and_then(Value::as_str)
        .and_then(PrRef::parse)
        .map(|r| r.repo);
    Some(PrDetail {
        pr: repo.map(|repo| PrRef {
            repo,
            number: n(p, "/number"),
        }),
        node_id: s(p, "/id"),
        viewer: s(v, "/data/viewer/login"),
        title: s(p, "/title"),
        url: s(p, "/url"),
        body: s(p, "/body"),
        state: s(p, "/state"),
        draft: b(p, "/isDraft"),
        author: s(p, "/author/login"),
        created: s(p, "/createdAt"),
        updated: s(p, "/updatedAt"),
        head: s(p, "/headRefName"),
        base: s(p, "/baseRefName"),
        head_oid: commit.map(|c| s(c, "/commit/oid")).unwrap_or_default(),
        additions: n(p, "/additions"),
        deletions: n(p, "/deletions"),
        changed_files: n(p, "/changedFiles"),
        mergeable: s(p, "/mergeable"),
        merge_state: s(p, "/mergeStateStatus"),
        review: s(p, "/reviewDecision"),
        viewer_is_author: b(p, "/viewerDidAuthor"),
        can_update_branch: b(p, "/viewerCanUpdateBranch"),
        can_write: matches!(
            s(repo_v, "/viewerPermission").as_str(),
            "WRITE" | "MAINTAIN" | "ADMIN"
        ),
        auto_merge: Some(s(p, "/autoMergeRequest/mergeMethod")).filter(|m| !m.is_empty()),
        merge_methods,
        labels: nodes(p, "/labels/nodes").map(|l| s(l, "/name")).collect(),
        requested: nodes(p, "/reviewRequests/nodes")
            .map(|r| {
                let l = s(r, "/requestedReviewer/login");
                if l.is_empty() {
                    s(r, "/requestedReviewer/name")
                } else {
                    l
                }
            })
            .filter(|r| !r.is_empty())
            .collect(),
        reviews: nodes(p, "/latestReviews/nodes")
            .map(|r| (s(r, "/author/login"), s(r, "/state")))
            .collect(),
        timeline,
        threads,
        checks_state: commit
            .map(|c| s(c, "/commit/statusCheckRollup/state"))
            .unwrap_or_default(),
        checks,
    })
}

/// A write, as adar does it. Every one runs once and is never retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Comment(String),
    Approve(String),
    RequestChanges(String),
    /// A review with only a body (`COMMENT`).
    ReviewComment(String),
    Merge {
        method: MergeMethod,
        delete_branch: bool,
    },
    AutoMerge {
        method: MergeMethod,
        delete_branch: bool,
    },
    DisableAutoMerge,
    UpdateBranch {
        rebase: bool,
    },
    Ready,
    ToDraft,
    Close,
    Reopen,
    RerunFailed(Vec<u64>),
    RerunJob(u64),
    Reply {
        thread: String,
        body: String,
    },
    Resolve(String),
    Unresolve(String),
}

/// How an action runs: `gh` arguments, with an optional stdin body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

fn graphql(query: &str, input: Value) -> Invocation {
    Invocation {
        args: vec!["api".into(), "graphql".into(), "--input".into(), "-".into()],
        stdin: Some(json!({ "query": query, "variables": { "i": input } }).to_string()),
    }
}

impl Action {
    /// Writes that cannot be undone ask first.
    pub fn needs_confirm(&self) -> bool {
        matches!(
            self,
            Action::Merge { .. }
                | Action::Close
                | Action::Approve(_)
                | Action::RequestChanges(_)
                | Action::UpdateBranch { .. }
        )
    }

    pub fn describe(&self) -> String {
        match self {
            Action::Comment(_) => "Comment".into(),
            Action::Approve(_) => "Approve".into(),
            Action::RequestChanges(_) => "Request changes".into(),
            Action::ReviewComment(_) => "Submit review comment".into(),
            Action::Merge {
                method,
                delete_branch,
            } => {
                format!(
                    "{}{}",
                    method.label(),
                    if *delete_branch {
                        ", delete branch"
                    } else {
                        ""
                    }
                )
            }
            Action::AutoMerge { method, .. } => {
                format!("Auto-merge ({}) when ready", method.label().to_lowercase())
            }
            Action::DisableAutoMerge => "Disable auto-merge".into(),
            Action::UpdateBranch { rebase } => format!(
                "Update branch ({})",
                if *rebase { "rebase" } else { "merge" }
            ),
            Action::Ready => "Mark ready for review".into(),
            Action::ToDraft => "Convert to draft".into(),
            Action::Close => "Close pull request".into(),
            Action::Reopen => "Reopen pull request".into(),
            Action::RerunFailed(ids) => format!("Re-run failed jobs ({} runs)", ids.len()),
            Action::RerunJob(_) => "Re-run job".into(),
            Action::Reply { .. } => "Reply".into(),
            Action::Resolve(_) => "Resolve thread".into(),
            Action::Unresolve(_) => "Unresolve thread".into(),
        }
    }

    /// The `gh` calls that perform this action on `pr`.
    pub fn invocations(
        &self,
        pr: &PrRef,
        detail: Option<&PrDetail>,
    ) -> Result<Vec<Invocation>, String> {
        let num = pr.number.to_string();
        let repo = ["--repo".to_owned(), pr.repo.clone()];
        let gh = |args: &[&str]| -> Invocation {
            Invocation {
                args: args
                    .iter()
                    .map(|a| (*a).to_owned())
                    .chain(repo.iter().cloned())
                    .collect(),
                stdin: None,
            }
        };
        let node = || {
            detail
                .map(|d| d.node_id.clone())
                .filter(|id| !id.is_empty())
                .ok_or("still loading — try again in a moment")
        };
        let review = |event: &str, body: &str| -> Result<Invocation, String> {
            let mut input = json!({ "pullRequestId": node()?, "event": event, "body": body });
            if let Some(oid) = detail.map(|d| d.head_oid.clone()).filter(|o| !o.is_empty()) {
                input["commitOID"] = json!(oid);
            }
            Ok(graphql(
                "mutation($i: AddPullRequestReviewInput!) { addPullRequestReview(input: $i) { pullRequestReview { id } } }",
                input,
            ))
        };
        Ok(match self {
            Action::Comment(body) => vec![Invocation {
                stdin: Some(body.clone()),
                ..gh(&["pr", "comment", &num, "--body-file", "-"])
            }],
            Action::Approve(body) => vec![review("APPROVE", body)?],
            Action::RequestChanges(body) => {
                if body.trim().is_empty() {
                    return Err("requesting changes needs a comment".into());
                }
                vec![review("REQUEST_CHANGES", body)?]
            }
            Action::ReviewComment(body) => vec![review("COMMENT", body)?],
            Action::Merge {
                method,
                delete_branch,
            } => {
                let mut a = vec!["pr", "merge", &num, method.flag()];
                if *delete_branch {
                    a.push("--delete-branch");
                }
                vec![gh(&a)]
            }
            Action::AutoMerge {
                method,
                delete_branch,
            } => {
                let mut a = vec!["pr", "merge", &num, "--auto", method.flag()];
                if *delete_branch {
                    a.push("--delete-branch");
                }
                vec![gh(&a)]
            }
            Action::DisableAutoMerge => vec![gh(&["pr", "merge", &num, "--disable-auto"])],
            Action::UpdateBranch { rebase } => {
                let mut a = vec!["pr", "update-branch", &num];
                if *rebase {
                    a.push("--rebase");
                }
                vec![gh(&a)]
            }
            Action::Ready => vec![gh(&["pr", "ready", &num])],
            Action::ToDraft => vec![gh(&["pr", "ready", &num, "--undo"])],
            Action::Close => vec![gh(&["pr", "close", &num])],
            Action::Reopen => vec![gh(&["pr", "reopen", &num])],
            Action::RerunFailed(ids) => ids
                .iter()
                .map(|id| gh(&["run", "rerun", &id.to_string(), "--failed"]))
                .collect(),
            Action::RerunJob(id) => vec![gh(&["run", "rerun", "--job", &id.to_string()])],
            Action::Reply { thread, body } => vec![graphql(
                "mutation($i: AddPullRequestReviewThreadReplyInput!) { addPullRequestReviewThreadReply(input: $i) { comment { id } } }",
                json!({ "pullRequestReviewThreadId": thread, "body": body }),
            )],
            Action::Resolve(thread) => vec![graphql(
                "mutation($i: ResolveReviewThreadInput!) { resolveReviewThread(input: $i) { thread { id } } }",
                json!({ "threadId": thread }),
            )],
            Action::Unresolve(thread) => vec![graphql(
                "mutation($i: UnresolveReviewThreadInput!) { unresolveReviewThread(input: $i) { thread { id } } }",
                json!({ "threadId": thread }),
            )],
        })
    }
}

/// Run one `gh` invocation; stdout on success, stderr's last line on failure.
pub fn exec(inv: &Invocation) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("gh")
        .args(&inv.args)
        .stdin(if inv.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("gh: {e}"))?;
    if let (Some(body), Some(mut stdin)) = (&inv.stdin, child.stdin.take()) {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| format!("gh stdin: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("gh: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("gh failed")
            .trim()
            .to_owned())
    }
}

/// POST a GraphQL body; errors in the answer become `Err`.
pub fn graphql_exec(body: &Value) -> Result<Value, String> {
    let out = exec(&Invocation {
        args: vec!["api".into(), "graphql".into(), "--input".into(), "-".into()],
        stdin: Some(body.to_string()),
    })?;
    let v: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    match errors(&v) {
        Some(e) if v.pointer("/data").is_none_or(Value::is_null) => Err(e),
        _ => Ok(v),
    }
}

pub fn fetch_bucket(bucket: Bucket, since: &str) -> Result<(String, Vec<PrRow>), String> {
    graphql_exec(&search_request(bucket, since, 40)).map(|v| parse_search(&v))
}

pub fn fetch_detail(pr: &PrRef) -> Result<PrDetail, String> {
    let v = graphql_exec(&detail_request(pr))?;
    parse_detail(&v).ok_or_else(|| format!("{pr}: not found"))
}

pub fn fetch_diff(pr: &PrRef) -> Result<Vec<DiffFile>, String> {
    exec(&Invocation {
        args: vec![
            "pr".into(),
            "diff".into(),
            pr.number.to_string(),
            "--repo".into(),
            pr.repo.clone(),
        ],
        stdin: None,
    })
    .map(|d| parse_diff(&d))
}

pub fn fetch_job_log(repo: &str, job: u64) -> Result<String, String> {
    exec(&Invocation {
        args: [
            "run",
            "view",
            "--job",
            &job.to_string(),
            "--log-failed",
            "--repo",
            repo,
        ]
        .map(str::to_owned)
        .to_vec(),
        stdin: None,
    })
    .map(|log| trim_log(&log, 400))
}

/// One file of a unified diff.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Add,
    Remove,
    Context,
    Meta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
}

/// Lines kept per file; a generated lockfile should not freeze the window.
const MAX_FILE_LINES: usize = 4000;

/// Split `gh pr diff` output into files.
pub fn parse_diff(text: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.split(" b/").nth(1).unwrap_or(rest).to_owned();
            files.push(DiffFile {
                path,
                ..DiffFile::default()
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(p) = line.strip_prefix("+++ b/") {
            p.clone_into(&mut file.path);
            continue;
        }
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("index ") {
            continue;
        }
        let (kind, o, nn) = if line.starts_with("@@") {
            let mut it = line.split_whitespace().skip(1);
            let start = |t: Option<&str>| {
                t.and_then(|t| t.get(1..))
                    .and_then(|t| t.split(',').next())
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0)
            };
            old = start(it.next());
            new = start(it.next());
            (LineKind::Hunk, None, None)
        } else if line.starts_with('+') {
            file.added = file.added.saturating_add(1);
            new += 1;
            (LineKind::Add, None, Some(new - 1))
        } else if line.starts_with('-') {
            file.removed = file.removed.saturating_add(1);
            old += 1;
            (LineKind::Remove, Some(old - 1), None)
        } else if line.starts_with(' ') {
            old += 1;
            new += 1;
            (LineKind::Context, Some(old - 1), Some(new - 1))
        } else {
            (LineKind::Meta, None, None)
        };
        if file.lines.len() < MAX_FILE_LINES {
            file.lines.push(DiffLine {
                kind,
                text: line.to_owned(),
                old: o,
                new: nn,
            });
        }
    }
    files
}

/// A failed job's log, trimmed to what matters: from a little before the
/// first line that looks like an error, timestamps stripped.
pub fn trim_log(log: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = log
        .lines()
        .map(|l| {
            // `job<TAB>step<TAB>2026-11-19T10:00:00.0000000Z text`
            let l = l.rsplit('\t').next().unwrap_or(l);
            match l.split_once(' ') {
                Some((ts, rest)) if ts.len() > 20 && ts.ends_with('Z') && ts.contains('T') => rest,
                _ => l,
            }
        })
        .collect();
    let first_error = lines.iter().position(|l| {
        let low = l.to_ascii_lowercase();
        (low.contains("error")
            || low.contains("failed")
            || low.contains("panicked")
            || l.starts_with("FAIL"))
            && !low.contains("exit code")
            && !low.contains("0 failed")
    });
    let start = first_error.map_or(lines.len().saturating_sub(max_lines), |i| {
        i.saturating_sub(8)
    });
    lines
        .iter()
        .skip(start)
        .take(max_lines)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// A filter over inbox rows, after adar's qualifiers:
/// `repo:data owner:x @alice author:me -@me ci:red review:changes is:draft
/// is:bot is:mine label:bug size:<100 in:review "phrase" words`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    terms: Vec<(bool, Term)>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Term {
    Repo(String),
    Owner(String),
    Author(String),
    Ci(String),
    Review(String),
    Is(String),
    Label(Vec<String>),
    Size(char, u64),
    In(String),
    Word(String),
}

impl Filter {
    pub fn parse(q: &str) -> Filter {
        let mut f = Filter::default();
        for raw in split_query(q) {
            let (neg, tok) = match raw.strip_prefix('-') {
                Some(t) if !t.is_empty() => (true, t.to_owned()),
                _ => (false, raw.clone()),
            };
            let term = if let Some(a) = tok.strip_prefix('@') {
                Some(Term::Author(a.to_lowercase()))
            } else if let Some((k, v)) = tok.split_once(':').filter(|(_, v)| !v.is_empty()) {
                let v = v.to_lowercase();
                match k {
                    "repo" => Some(Term::Repo(v)),
                    "owner" | "org" => Some(Term::Owner(v)),
                    "author" => Some(Term::Author(v)),
                    "ci" if ["red", "green", "pending"].contains(&v.as_str()) => Some(Term::Ci(v)),
                    "review" if ["approved", "changes", "none"].contains(&v.as_str()) => {
                        Some(Term::Review(v))
                    }
                    "is" if ["draft", "bot", "mine", "merged", "open"].contains(&v.as_str()) => {
                        Some(Term::Is(v))
                    }
                    "label" => Some(Term::Label(v.split(',').map(str::to_owned).collect())),
                    "in" if Bucket::ALL.iter().any(|b| b.key() == v) => Some(Term::In(v)),
                    "size" => {
                        let (op, num) = match v.chars().next() {
                            Some(c @ ('<' | '>')) => (c, v.get(1..).unwrap_or_default()),
                            _ => ('=', v.as_str()),
                        };
                        num.parse().ok().map(|n| Term::Size(op, n))
                    }
                    _ => None,
                }
                .or_else(|| {
                    f.errors.push(format!("{tok}: not understood"));
                    None
                })
            } else {
                Some(Term::Word(tok.to_lowercase()))
            };
            if let Some(t) = term {
                f.terms.push((neg, t));
            }
        }
        f
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn matches(&self, row: &PrRow, bucket: Bucket, me: &str) -> bool {
        self.terms
            .iter()
            .all(|(neg, t)| *neg != self.term(t, row, bucket, me))
    }

    fn term(&self, t: &Term, row: &PrRow, bucket: Bucket, me: &str) -> bool {
        let repo = row
            .pr
            .as_ref()
            .map(|p| p.repo.to_lowercase())
            .unwrap_or_default();
        let author = row.author.to_lowercase();
        match t {
            Term::Repo(r) => {
                if r.contains('/') {
                    glob(r, &repo)
                } else {
                    glob(r, repo.rsplit('/').next().unwrap_or(&repo))
                }
            }
            Term::Owner(o) => repo.split('/').next() == Some(o.as_str()),
            Term::Author(a) => {
                author
                    == if a == "me" {
                        me.to_lowercase()
                    } else {
                        a.clone()
                    }
            }
            Term::Ci(c) => match c.as_str() {
                "red" => matches!(row.checks.as_str(), "FAILURE" | "ERROR"),
                "green" => row.checks == "SUCCESS",
                _ => matches!(row.checks.as_str(), "PENDING" | "EXPECTED"),
            },
            Term::Review(r) => match r.as_str() {
                "approved" => row.review == "APPROVED",
                "changes" => row.review == "CHANGES_REQUESTED",
                _ => row.review.is_empty() || row.review == "REVIEW_REQUIRED",
            },
            Term::Is(i) => match i.as_str() {
                "draft" => row.draft,
                "bot" => row.bot,
                "mine" => !me.is_empty() && author == me.to_lowercase(),
                "merged" => row.state == "MERGED",
                _ => row.state == "OPEN",
            },
            Term::Label(any) => row.labels.iter().any(|l| any.contains(&l.to_lowercase())),
            Term::Size(op, n) => {
                let size = row.additions + row.deletions;
                match op {
                    '<' => size < *n,
                    '>' => size > *n,
                    _ => size == *n,
                }
            }
            Term::In(b) => bucket.key() == b,
            Term::Word(w) => {
                let hay = format!(
                    "{} {} {} {} {} {}",
                    row.title.to_lowercase(),
                    repo,
                    row.pr
                        .as_ref()
                        .map(|p| p.number.to_string())
                        .unwrap_or_default(),
                    author,
                    row.head.to_lowercase(),
                    row.labels.join(" ").to_lowercase()
                );
                hay.contains(w.as_str())
            }
        }
    }
}

fn split_query(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `*` wildcards.
fn glob(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == s;
    }
    let mut rest = s;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(at) if i > 0 || at == 0 => rest = rest.get(at + part.len()..).unwrap_or_default(),
            _ => return false,
        }
    }
    parts.last().is_none_or(|l| l.is_empty() || s.ends_with(l))
}

/// A short, human age for an ISO timestamp, relative to `now` (Unix secs).
pub fn age(iso: &str, now: u64) -> String {
    parse_iso(iso).map_or_else(String::new, |t| crate::text::ago(now.saturating_sub(t)))
}

/// `2026-11-19T10:20:30Z` → Unix seconds.
pub fn parse_iso(iso: &str) -> Option<u64> {
    let date = iso.get(..10)?;
    let time = iso.get(11..19)?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi, sec) = (t.next()??, t.next()??, t.next()??);
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()
}

/// A one-line summary of a PR's checks, e.g. `3 failing`.
pub fn checks_summary(d: &PrDetail) -> String {
    let failing = d.checks.iter().filter(|c| c.failed()).count();
    let running = d.checks.iter().filter(|c| c.running()).count();
    match (failing, running) {
        (0, 0) if d.checks.is_empty() => "no checks".into(),
        (0, 0) => "checks passed".into(),
        (0, r) => format!("{r} running"),
        (f, _) => format!("{f} failing"),
    }
}

/// First line of a commit or comment, for compact rows.
pub fn headline(s: &str, max: usize) -> String {
    clip(s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse() {
        let r = PrRef::parse("https://github.com/join-com/data/pull/7939/files").unwrap();
        assert_eq!(
            (r.repo.as_str(), r.number, r.short()),
            ("join-com/data", 7939, "data#7939".into())
        );
        assert_eq!(PrRef::parse("enekos/zin#10").map(|r| r.number), Some(10));
        assert!(PrRef::parse("zin#10").is_none());
        assert!(PrRef::parse("a/b#x").is_none());
        assert!(PrRef::parse("https://github.com/a/b/tree/main").is_none());
    }

    #[test]
    fn search_parses() {
        let v: Value = serde_json::from_str(r#"{"data":{"viewer":{"login":"enekos"},"search":{"nodes":[
            {"number":7,"title":"t","url":"u","isDraft":true,"state":"OPEN","headRefName":"h","baseRefName":"main",
             "additions":10,"deletions":2,"reviewDecision":"APPROVED","repository":{"nameWithOwner":"o/r"},
             "author":{"__typename":"Bot","login":"dependabot"},"labels":{"nodes":[{"name":"bug"}]},"comments":{"totalCount":3},
             "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE"}}}]}},
            {}]}}}"#).unwrap();
        let (login, rows) = parse_search(&v);
        assert_eq!(login, "enekos");
        assert_eq!(rows.len(), 1, "non-PR nodes are skipped");
        let r = &rows[0];
        assert_eq!(
            (r.checks.as_str(), r.bot, r.labels.len()),
            ("FAILURE", true, 1)
        );
        let f = Filter::parse(
            "repo:r ci:red review:approved is:draft is:bot label:bug,x size:<20 -@me in:review",
        );
        assert!(f.errors.is_empty(), "{:?}", f.errors);
        assert!(f.matches(r, Bucket::Review, "enekos"));
        assert!(!f.matches(r, Bucket::Mine, "enekos"));
        assert!(!Filter::parse("author:me").matches(r, Bucket::Mine, "enekos"));
        assert!(Filter::parse("\"t\" o/r").matches(r, Bucket::Mine, "enekos"));
        assert!(Filter::parse("repo:o/*").matches(r, Bucket::Mine, ""));
        assert_eq!(Filter::parse("size:lots ci:blue").errors.len(), 2);
    }

    #[test]
    fn detail_parses() {
        let v: Value = serde_json::from_str(r#"{"data":{"viewer":{"login":"me"},"repository":{
          "viewerPermission":"WRITE","squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":true,
          "pullRequest":{"id":"PR_1","number":5,"title":"Fix","url":"https://github.com/o/r/pull/5","body":"**hi**","state":"OPEN",
            "isDraft":false,"headRefName":"fix","baseRefName":"main","mergeable":"MERGEABLE","mergeStateStatus":"BEHIND",
            "author":{"login":"a"},"viewerCanUpdateBranch":true,"autoMergeRequest":null,
            "timelineItems":{"nodes":[
              {"__typename":"IssueComment","id":"c1","author":{"login":"b"},"body":"lgtm","createdAt":"2026-11-19T10:00:00Z"},
              {"__typename":"PullRequestReview","id":"r1","author":{"login":"c"},"body":"","state":"APPROVED"},
              {"__typename":"MergedEvent","actor":{"login":"a"},"createdAt":"x"},
              {"__typename":"PullRequestCommit","commit":{"oid":"abc","messageHeadline":"wip","author":{"name":"A","user":null}}}]},
            "reviewThreads":{"nodes":[{"id":"t1","isResolved":false,"path":"src/a.rs","line":null,"originalLine":12,"viewerCanResolve":true,
              "comments":{"nodes":[{"id":"x","author":{"login":"b"},"body":"why?"}]}}]},
            "commits":{"nodes":[{"commit":{"oid":"abc","statusCheckRollup":{"state":"FAILURE","contexts":{"nodes":[
              {"__typename":"CheckRun","databaseId":9,"name":"test","status":"COMPLETED","conclusion":"FAILURE","checkSuite":{"workflowRun":{"databaseId":77,"workflow":{"name":"CI"}}}},
              {"__typename":"StatusContext","context":"sonar","state":"SUCCESS"}]}}}}]}}}}}"#).unwrap();
        let d = parse_detail(&v).unwrap();
        assert_eq!(
            d.pr,
            Some(PrRef {
                repo: "o/r".into(),
                number: 5
            })
        );
        assert_eq!(d.merge_methods, [MergeMethod::Squash, MergeMethod::Rebase]);
        assert!(d.can_write && d.can_update_branch && d.open());
        assert_eq!(d.timeline.len(), 4);
        assert_eq!(
            d.threads[0].line,
            Some(12),
            "outdated threads fall back to the original line"
        );
        assert_eq!(d.failed_runs(), [77]);
        assert_eq!(d.checks[1].workflow, "Statuses");
        assert_eq!(checks_summary(&d), "1 failing");
    }

    #[test]
    fn actions_build_gh_calls() {
        let pr = PrRef {
            repo: "o/r".into(),
            number: 5,
        };
        let inv = Action::Merge {
            method: MergeMethod::Squash,
            delete_branch: true,
        }
        .invocations(&pr, None)
        .unwrap();
        assert_eq!(
            inv[0].args,
            [
                "pr",
                "merge",
                "5",
                "--squash",
                "--delete-branch",
                "--repo",
                "o/r"
            ]
        );
        let c = Action::Comment("hi `x`".into())
            .invocations(&pr, None)
            .unwrap();
        assert_eq!(
            c[0].stdin.as_deref(),
            Some("hi `x`"),
            "bodies go through stdin, never argv"
        );
        assert!(
            Action::Approve(String::new())
                .invocations(&pr, None)
                .is_err(),
            "reviews need the loaded detail"
        );
        let d = PrDetail {
            node_id: "PR_1".into(),
            head_oid: "abc".into(),
            ..PrDetail::default()
        };
        let a = Action::Approve("ok".into())
            .invocations(&pr, Some(&d))
            .unwrap();
        let body: Value = serde_json::from_str(a[0].stdin.as_deref().unwrap()).unwrap();
        assert_eq!(body["variables"]["i"]["event"], "APPROVE");
        assert_eq!(body["variables"]["i"]["commitOID"], "abc");
        assert!(
            Action::RequestChanges(" ".into())
                .invocations(&pr, Some(&d))
                .is_err()
        );
        assert_eq!(
            Action::RerunFailed(vec![1, 2])
                .invocations(&pr, None)
                .unwrap()
                .len(),
            2
        );
        assert!(Action::Close.needs_confirm() && !Action::Comment(String::new()).needs_confirm());
    }

    #[test]
    fn diff_splits_files_with_line_numbers() {
        let d = parse_diff(
            "diff --git a/x.rs b/x.rs\nindex 1..2\n--- a/x.rs\n+++ b/x.rs\n@@ -10,3 +10,4 @@ fn x\n ctx\n-old\n+new\n+more\ndiff --git a/y b/y\n@@ -1 +1 @@\n-a\n+b\n",
        );
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].path.as_str(), d[0].added, d[0].removed),
            ("x.rs", 2, 1)
        );
        assert_eq!(
            d[0].lines[1],
            DiffLine {
                kind: LineKind::Context,
                text: " ctx".into(),
                old: Some(10),
                new: Some(10)
            }
        );
        assert_eq!(d[0].lines[3].new, Some(11));
    }

    #[test]
    fn logs_start_near_the_first_error() {
        let mut log = String::new();
        for i in 0..50 {
            log.push_str(&format!(
                "build\tstep\t2026-11-19T10:00:00.0000000Z line {i}\n"
            ));
        }
        log.push_str("build\tstep\t2026-11-19T10:00:01.0000000Z error[E0308]: mismatched types\n");
        log.push_str(
            "build\tstep\t2026-11-19T10:00:02.0000000Z Process completed with exit code 1\n",
        );
        let t = trim_log(&log, 20);
        assert!(t.starts_with("line 42"), "{t}");
        assert!(t.contains("error[E0308]"));
        assert!(!t.contains("2026-11-19T"));
    }

    #[test]
    fn iso_and_globs() {
        assert_eq!(parse_iso("1970-01-02T00:00:00Z"), Some(86_400));
        assert_eq!(
            parse_iso("2026-11-19T10:00:00Z").map(|t| t > 1_790_000_000),
            Some(true)
        );
        assert!(
            glob("feat-*", "feat-x") && !glob("feat-*", "fix-x") && glob("*e2e*", "ci-e2e-run")
        );
    }
}
