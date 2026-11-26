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
        if s.contains("://") {
            return Self::link(s).map(|(pr, _)| pr);
        }
        let (repo, num) = s.split_once('#')?;
        let repo = repo.to_owned();
        let ok = repo.split('/').count() == 2
            && repo.split('/').all(|p| {
                !p.is_empty()
                    && p.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            });
        let number = num.parse().ok().filter(|n| *n > 0)?;
        ok.then_some(PrRef { repo, number })
    }

    /// A GitHub PR URL and the path following its number (such as `files`).
    pub fn link(url: &str) -> Option<(PrRef, &str)> {
        let (scheme, rest) = url.trim().split_once("://")?;
        if !["https", "http", "artalde"]
            .iter()
            .any(|s| scheme.eq_ignore_ascii_case(s))
        {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host = authority
            .strip_suffix(if scheme.eq_ignore_ascii_case("http") {
                ":80"
            } else {
                ":443"
            })
            .unwrap_or(authority);
        if !["github.com", "www.github.com"]
            .iter()
            .any(|h| host.eq_ignore_ascii_case(h))
        {
            return None;
        }
        let mut parts = path.split(['?', '#']).next()?.splitn(5, '/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        if parts.next()? != "pull" {
            return None;
        }
        let number = parts.next()?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let pr = Self::parse(&format!("{owner}/{repo}#{number}"))?;
        Some((pr, parts.next().unwrap_or_default()))
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
    Drafts,
    Ready,
}

impl Bucket {
    pub const INBOX: [Bucket; 4] = [
        Bucket::Review,
        Bucket::Mine,
        Bucket::Involved,
        Bucket::Merged,
    ];

    pub const REVIEW_QUEUE: [Bucket; 2] = [Bucket::Drafts, Bucket::Ready];

    pub fn label(self) -> &'static str {
        match self {
            Bucket::Review => "Review requested",
            Bucket::Mine => "Mine",
            Bucket::Involved => "Involved",
            Bucket::Merged => "Recently merged",
            Bucket::Drafts => "My latest drafts",
            Bucket::Ready => "Ready for review",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Bucket::Review => "review",
            Bucket::Mine => "mine",
            Bucket::Involved => "involved",
            Bucket::Merged => "merged",
            Bucket::Drafts => "drafts",
            Bucket::Ready => "ready",
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
            Bucket::Drafts => "is:pr is:open archived:false author:@me draft:true sort:created-desc".into(),
            Bucket::Ready => "is:pr is:open label:\"ready-for-review\" sort:created-desc".into(),
        }
    }
}

const ROW_FIELDS: &str = "
  number title url isDraft state createdAt updatedAt headRefName baseRefName
  repository { nameWithOwner } author { __typename login }
  labels(first: 6) { nodes { name } }";

const HEALTH_FIELDS: &str = "
  additions deletions reviewDecision comments { totalCount }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }";

/// The GraphQL request body (for `gh api graphql --input -`) for a bucket.
pub fn search_request(bucket: Bucket, since: &str, limit: u32) -> Value {
    search_page(bucket, &bucket.query(since), limit, None)
}

fn search_page(bucket: Bucket, query: &str, limit: u32, after: Option<&str>) -> Value {
    let extra = match bucket {
        Bucket::Drafts => "repository { label(name: \"ready-for-review\") { id } }",
        Bucket::Ready => "",
        _ => HEALTH_FIELDS,
    };
    json!({
        "query": format!("query($q: String!, $limit: Int!, $after: String) {{ viewer {{ login }} search(type: ISSUE, first: $limit, after: $after, query: $q) {{ issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ ... on PullRequest {{ {ROW_FIELDS} {extra} }} }} }} }}"),
        "variables": { "q": query, "limit": limit, "after": after },
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
    pub has_ready_label: bool,
    pub comments: u64,
    pub merged_at: String,
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
        has_ready_label: v
            .pointer("/repository/label/id")
            .is_some_and(Value::is_string),
        comments: n(v, "/comments/totalCount"),
        merged_at: s(v, "/mergedAt"),
    }
}

const DETAIL_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    viewerPermission squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed
    label(name: \"ready-for-review\") { id }
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
          ... on IssueComment { id author { __typename login } body createdAt url }
          ... on PullRequestReview { id author { __typename login } body state createdAt url }
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
          comments(first: 50) { nodes { id author { __typename login } body createdAt url diffHunk } }
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
    /// Written by an app (CodeRabbit, Bender, …), not a person.
    pub bot: bool,
}

/// A friendly name for a bot login.
pub fn bot_name(login: &str) -> String {
    let l = login.trim_end_matches("[bot]").to_lowercase();
    let known = [
        ("coderabbit", "CodeRabbit"),
        ("bender", "Bender"),
        ("sonarqube", "SonarQube"),
        ("linear", "Linear"),
        ("cursor", "Cursor Bugbot"),
        ("github-actions", "GitHub Actions"),
        ("dependabot", "Dependabot"),
        ("renovate", "Renovate"),
        ("copilot", "Copilot"),
        ("codecov", "Codecov"),
        ("vercel", "Vercel"),
    ];
    known.iter().find(|(k, _)| l.contains(k)).map_or_else(
        || login.trim_end_matches("[bot]").to_owned(),
        |(_, n)| (*n).to_owned(),
    )
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
    pub has_ready_label: bool,
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
        bot: s(v, "/author/__typename") == "Bot" || s(v, "/author/login").ends_with("[bot]"),
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
        has_ready_label: repo_v.pointer("/label/id").is_some_and(Value::is_string),
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
    ReadyAndLabel,
    AddReadyLabel,
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
    /// Post the prepared review: verdict, summary and every accepted draft.
    SubmitReview {
        verdict: crate::review::Verdict,
        body: String,
        review: Box<crate::review::PendingReview>,
    },
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
                | Action::SubmitReview { .. }
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
            Action::ReadyAndLabel => "Mark ready and add ready-for-review".into(),
            Action::AddReadyLabel => "Add ready-for-review label".into(),
            Action::ToDraft => "Convert to draft".into(),
            Action::Close => "Close pull request".into(),
            Action::Reopen => "Reopen pull request".into(),
            Action::RerunFailed(ids) => format!("Re-run failed jobs ({} runs)", ids.len()),
            Action::RerunJob(_) => "Re-run job".into(),
            Action::Reply { .. } => "Reply".into(),
            Action::Resolve(_) => "Resolve thread".into(),
            Action::Unresolve(_) => "Unresolve thread".into(),
            Action::SubmitReview {
                verdict, review, ..
            } => {
                let n = review.accepted().count();
                format!(
                    "Submit review: {} with {n} comment{}",
                    verdict.label().to_lowercase(),
                    if n == 1 { "" } else { "s" }
                )
            }
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
        let add_ready_label = || gh(&["pr", "edit", &num, "--add-label", "ready-for-review"]);
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
            Action::ReadyAndLabel => vec![gh(&["pr", "ready", &num]), add_ready_label()],
            Action::AddReadyLabel => vec![add_ready_label()],
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
            Action::SubmitReview {
                verdict,
                body,
                review,
            } => {
                let d = detail.ok_or("still loading — try again in a moment")?;
                vec![crate::review::submit_invocation(d, *verdict, body, review)?]
            }
            Action::Unresolve(thread) => vec![graphql(
                "mutation($i: UnresolveReviewThreadInput!) { unresolveReviewThread(input: $i) { thread { id } } }",
                json!({ "threadId": thread }),
            )],
        })
    }
}

/// One GitHub deployment and its latest status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deployment {
    pub environment: String,
    /// `ACTIVE`, `INACTIVE`, `IN_PROGRESS`, `FAILURE`, `ERROR`, `PENDING`, …
    pub state: String,
    pub created: String,
    pub creator: String,
    /// The deployed ref, usually a version tag.
    pub reference: String,
    pub sha: String,
    pub headline: String,
    pub status: String,
    pub description: String,
    pub env_url: String,
    pub log_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub published: String,
    pub author: String,
    pub body: String,
    pub url: String,
    pub prerelease: bool,
}

/// Repository activity loaded after the pull requests are already visible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoActivity {
    pub deployments: Vec<Deployment>,
    pub releases: Vec<Release>,
}

/// A repository's page: open and merged PRs, deployments, releases.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoDetail {
    pub name: String,
    pub description: String,
    pub url: String,
    pub private: bool,
    pub default_branch: String,
    pub open_total: u64,
    pub open: Vec<PrRow>,
    pub merged: Vec<PrRow>,
    /// Newest first.
    pub deployments: Vec<Deployment>,
    pub releases: Vec<Release>,
}

impl RepoDetail {
    /// The newest deployment of each environment, production last.
    pub fn environments(&self) -> Vec<&Deployment> {
        let mut seen: Vec<&Deployment> = Vec::new();
        for d in &self.deployments {
            if !seen.iter().any(|s| s.environment == d.environment) {
                seen.push(d);
            }
        }
        let rank = |e: &str| match e.to_lowercase().as_str() {
            e if e.contains("dev") => 0,
            e if e.contains("stag") => 1,
            e if e.contains("prod") => 3,
            _ => 2,
        };
        seen.sort_by_key(|d| (rank(&d.environment), d.environment.clone()));
        seen
    }
}

const REPO_QUERY: &str = "query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    nameWithOwner description url isPrivate defaultBranchRef { name }
    open: pullRequests(states: OPEN, first: 50, orderBy: { field: UPDATED_AT, direction: DESC }) { totalCount nodes { SUMMARY } }
    merged: pullRequests(states: MERGED, first: 15, orderBy: { field: UPDATED_AT, direction: DESC }) { totalCount nodes { SUMMARY mergedAt } }
  }
}";

const REPO_ACTIVITY_QUERY: &str = "query($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    deployments(last: 40) { nodes {
      environment state createdAt creator { login } ref { name } commit { abbreviatedOid messageHeadline }
      latestStatus { state description environmentUrl logUrl }
    } }
    releases(first: 15, orderBy: { field: CREATED_AT, direction: DESC }) { nodes {
      tagName name publishedAt isPrerelease url author { login } description
    } }
  }
}";

pub fn repo_request(repo: &str) -> Value {
    let (owner, name) = repo.split_once('/').unwrap_or((repo, ""));
    json!({ "query": REPO_QUERY.replace("SUMMARY", &format!("{ROW_FIELDS} {HEALTH_FIELDS}")), "variables": { "owner": owner, "name": name } })
}

pub fn repo_activity_request(repo: &str) -> Value {
    let (owner, name) = repo.split_once('/').unwrap_or((repo, ""));
    json!({ "query": REPO_ACTIVITY_QUERY, "variables": { "owner": owner, "name": name } })
}

fn repo_deployments(r: &Value) -> Vec<Deployment> {
    let mut deployments: Vec<Deployment> = nodes(r, "/deployments/nodes")
        .map(|d| Deployment {
            environment: s(d, "/environment"),
            state: s(d, "/state"),
            created: s(d, "/createdAt"),
            creator: s(d, "/creator/login"),
            reference: s(d, "/ref/name"),
            sha: s(d, "/commit/abbreviatedOid"),
            headline: s(d, "/commit/messageHeadline"),
            status: s(d, "/latestStatus/state"),
            description: s(d, "/latestStatus/description"),
            env_url: s(d, "/latestStatus/environmentUrl"),
            log_url: s(d, "/latestStatus/logUrl"),
        })
        .collect();
    deployments.sort_by(|a, b| b.created.cmp(&a.created));
    deployments
}

fn repo_releases(r: &Value) -> Vec<Release> {
    nodes(r, "/releases/nodes")
        .map(|x| Release {
            tag: s(x, "/tagName"),
            name: s(x, "/name"),
            published: s(x, "/publishedAt"),
            author: s(x, "/author/login"),
            body: s(x, "/description"),
            url: s(x, "/url"),
            prerelease: b(x, "/isPrerelease"),
        })
        .collect()
}

pub fn parse_repo(v: &Value) -> Option<RepoDetail> {
    let r = v.pointer("/data/repository").filter(|r| !r.is_null())?;
    let rows = |ptr: &str| {
        nodes(r, ptr)
            .filter(|n| n.get("number").is_some())
            .map(parse_row)
            .collect::<Vec<_>>()
    };
    Some(RepoDetail {
        name: s(r, "/nameWithOwner"),
        description: s(r, "/description"),
        url: s(r, "/url"),
        private: b(r, "/isPrivate"),
        default_branch: s(r, "/defaultBranchRef/name"),
        open_total: n(r, "/open/totalCount"),
        open: rows("/open/nodes"),
        merged: rows("/merged/nodes"),
        deployments: repo_deployments(r),
        releases: repo_releases(r),
    })
}

pub fn parse_repo_activity(v: &Value) -> Option<RepoActivity> {
    let r = v.pointer("/data/repository").filter(|r| !r.is_null())?;
    Some(RepoActivity {
        deployments: repo_deployments(r),
        releases: repo_releases(r),
    })
}

pub fn fetch_repo(repo: &str) -> Result<RepoDetail, String> {
    let v = graphql_exec(&repo_request(repo))?;
    parse_repo(&v).ok_or_else(|| format!("{repo}: not found"))
}

pub fn fetch_repo_activity(repo: &str) -> Result<RepoActivity, String> {
    let v = graphql_exec(&repo_activity_request(repo))?;
    parse_repo_activity(&v).ok_or_else(|| format!("{repo}: not found"))
}

/// `owner/repo` from a git remote URL (`git@github.com:o/r.git`,
/// `https://github.com/o/r`).
pub fn slug_from_remote(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .split("github.com")
        .nth(1)?
        .trim_start_matches([':', '/']);
    let slug = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = slug.split('/');
    let (o, r) = (parts.next()?, parts.next()?);
    (!o.is_empty() && !r.is_empty() && parts.next().is_none()).then(|| format!("{o}/{r}"))
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
    let limit = if bucket == Bucket::Drafts { 8 } else { 40 };
    let page = graphql_exec(&search_request(bucket, since, limit))?;
    if let Some(e) = errors(&page) {
        return Err(e);
    }
    let (login, mut rows) = parse_search(&page);
    if bucket == Bucket::Drafts {
        rows.retain(|r| r.draft && r.state == "OPEN" && r.author.eq_ignore_ascii_case(&login));
    }
    Ok((login, rows))
}

/// Search by label across the workspace, including PRs unrelated to the viewer.
pub fn fetch_ready(owners: &[String]) -> Result<(String, Vec<PrRow>), String> {
    collect_ready(owners, graphql_exec)
}

fn collect_ready(
    owners: &[String],
    mut fetch: impl FnMut(&Value) -> Result<Value, String>,
) -> Result<(String, Vec<PrRow>), String> {
    let mut base = Bucket::Ready.query("");
    if owners.is_empty() {
        return Err("Set review_owners in settings.json to a GitHub user or organization".into());
    }
    for owner in owners {
        if owner != "@me"
            && (owner.is_empty()
                || !owner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
        {
            return Err(format!("Invalid GitHub review owner: {owner}"));
        }
        base.push_str(&format!(" user:{owner}"));
    }
    let mut query = base.clone();
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    let mut cursor = None;
    let mut page_count = 0;
    loop {
        let page = fetch(&search_page(Bucket::Ready, &query, 100, cursor.as_deref()))?;
        if let Some(e) = errors(&page) {
            return Err(e);
        }
        let search = page
            .pointer("/data/search")
            .ok_or("Missing search results")?;
        let more = search
            .pointer("/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .ok_or("Missing pagination information")?;
        let (login, batch) = parse_search(&page);
        let oldest = batch.last().map(|r| r.created.clone());
        rows.extend(
            batch
                .into_iter()
                .filter(|r| r.pr.as_ref().is_some_and(|p| seen.insert(p.clone()))),
        );
        page_count += 1;
        // GitHub search caps a single query at 1,000. Start a new window
        // at the oldest creation time, retaining ties and deduplicating PRs.
        if page_count == 10 && n(search, "/issueCount") > 1000 {
            let stamp = oldest
                .filter(|s| parse_iso(s).is_some())
                .ok_or("Missing PR creation date")?;
            let next = format!("{base} created:<={stamp}");
            if next == query {
                return Err(
                    "Too many labeled PRs opened in the same second to paginate GitHub search"
                        .into(),
                );
            }
            query = next;
            cursor = None;
            page_count = 0;
        } else if more {
            let next = search
                .pointer("/pageInfo/endCursor")
                .and_then(Value::as_str)
                .ok_or("Missing next search cursor")?;
            if cursor.as_deref() == Some(next) {
                return Err("GitHub repeated a search cursor".into());
            }
            cursor = Some(next.to_owned());
        } else {
            return Ok((login, rows));
        }
    }
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
    pub status: FileStatus,
    /// The path before a rename.
    pub old_path: Option<String>,
    pub lines: Vec<DiffLine>,
    /// Post-image blob from the patch index, often abbreviated.
    pub postimage: String,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileStatus {
    #[default]
    Modified,
    Added,
    Deleted,
    Renamed,
}

impl FileStatus {
    pub fn letter(self) -> &'static str {
        match self {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Add,
    Remove,
    Context,
    /// Unchanged text outside the original patch; GitHub cannot anchor a review here.
    Expanded,
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
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            in_hunk = false;
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
        if !in_hunk && let Some(index) = line.strip_prefix("index ") {
            if let Some((_, new_blob)) = index
                .split_whitespace()
                .next()
                .and_then(|ids| ids.split_once(".."))
            {
                new_blob.clone_into(&mut file.postimage);
            }
            continue;
        }
        if !in_hunk && let Some(p) = line.strip_prefix("+++ b/") {
            p.clone_into(&mut file.path);
            continue;
        }
        if line.starts_with("new file mode") {
            file.status = FileStatus::Added;
            continue;
        }
        if line.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
            continue;
        }
        if let Some(from) = line.strip_prefix("rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(from.to_owned());
            continue;
        }
        if !in_hunk
            && (line.starts_with("+++ ")
                || line.starts_with("--- ")
                || line.starts_with("index ")
                || line.starts_with("rename to ")
                || line.starts_with("similarity index"))
        {
            continue;
        }
        let (kind, o, nn) = if line.starts_with("@@") {
            in_hunk = true;
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
        } else {
            file.truncated = true;
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
                    "in" if Bucket::INBOX
                        .iter()
                        .chain(&Bucket::REVIEW_QUEUE)
                        .any(|b| b.key() == v) =>
                    {
                        Some(Term::In(v))
                    }
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

    pub fn review_queue(q: &str) -> Filter {
        let mut filter = Self::parse(q);
        if filter
            .terms
            .iter()
            .any(|(_, t)| matches!(t, Term::Ci(_) | Term::Size(_, _) | Term::Review(_)))
        {
            filter
                .errors
                .push("CI, review decisions and size filters are available in the PR inbox".into());
            filter
                .terms
                .retain(|(_, t)| !matches!(t, Term::Ci(_) | Term::Size(_, _) | Term::Review(_)));
        }
        filter
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
    fn pr_links_route_by_authority_and_path() {
        for (url, section) in [
            ("https://GitHub.com/org/repo/pull/12", ""),
            (
                "https://github.com/org/repo/pull/12/files?show=1#diff-abc",
                "files",
            ),
            ("http://github.com:80/org/repo/pull/12/checks", "checks"),
            ("artalde://github.com/org/repo/pull/12/changes", "changes"),
        ] {
            let (pr, sub) = PrRef::link(url).unwrap();
            assert_eq!(pr.to_string(), "org/repo#12");
            assert_eq!(sub, section);
        }
        for external in [
            "https://github.com/org/repo/issues/12",
            "https://other.example/github.com/org/repo/pull/12",
            "https://github.com.other.example/org/repo/pull/12",
            "https://other.example/?next=https://github.com/org/repo/pull/12",
            "https://github.com@other.example/org/repo/pull/12",
            "https://github.com/org/repo/pull/0",
            "https://github.com/org/repo/pull/12x",
            "https://github.com/org/repo/actions/runs/12",
        ] {
            assert!(PrRef::parse(external).is_none(), "{external}");
        }
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
    fn review_queue_pages_past_github_search_cap_without_losing_timestamp_ties() {
        let owners = ["sample-org".into(), "@me".into()];
        let mut page = 0;
        let (login, rows) = collect_ready(&owners, |request| {
            let q = request["variables"]["q"].as_str().unwrap();
            assert!(q.contains("label:\"ready-for-review\""));
            assert!(q.contains("user:sample-org user:@me"));
            assert!(!q.contains("author:"));
            let (first, last) = if page < 10 {
                assert!(!q.contains("created:<="));
                if page == 0 {
                    assert!(request["variables"]["after"].is_null());
                } else {
                    assert_eq!(request["variables"]["after"], format!("page-{page}"));
                }
                (page * 100 + 1, (page + 1) * 100)
            } else {
                assert_eq!(page, 10);
                assert!(q.ends_with("created:<=2026-10-01T00:00:00Z"));
                assert!(request["variables"]["after"].is_null());
                (1000, 1001)
            };
            let nodes: Vec<_> = (first..=last).map(|number| json!({
                "number":number, "repository":{"nameWithOwner":"sample-org/r"},
                "createdAt": if number < 1000 { "2026-10-02T00:00:00Z" } else { "2026-10-01T00:00:00Z" }
            })).collect();
            page += 1;
            Ok(json!({"data":{"viewer":{"login":"viewer"},"search":{
                "issueCount": if page <= 10 { 1001 } else { 2 },
                "pageInfo":{"hasNextPage":page < 10,"endCursor":format!("page-{page}")},"nodes":nodes
            }}}))
        }).unwrap();
        assert_eq!(login, "viewer");
        assert_eq!(rows.len(), 1001);
        assert_eq!(rows.last().unwrap().pr.as_ref().unwrap().number, 1001);
    }

    #[test]
    fn latest_drafts_search_uses_creation_order_and_only_my_open_drafts() {
        let query = Bucket::Drafts.query("");
        for term in [
            "is:pr",
            "is:open",
            "draft:true",
            "author:@me",
            "sort:created-desc",
        ] {
            assert!(query.split_whitespace().any(|part| part == term));
        }
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
        let ready = Action::Ready.invocations(&pr, None).unwrap();
        assert_eq!(ready[0].args, ["pr", "ready", "5", "--repo", "o/r"]);
        let label = Action::AddReadyLabel.invocations(&pr, None).unwrap();
        assert_eq!(
            label[0].args,
            [
                "pr",
                "edit",
                "5",
                "--add-label",
                "ready-for-review",
                "--repo",
                "o/r"
            ]
        );
        assert_eq!(
            Action::ReadyAndLabel.invocations(&pr, None).unwrap(),
            [ready, label].concat()
        );
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
        let s = parse_diff(
            "diff --git a/n b/n\nnew file mode 100644\n@@ -0,0 +1 @@\n+x\ndiff --git a/o b/p\nsimilarity index 90%\nrename from o\nrename to p\n",
        );
        assert_eq!(
            (s[0].status, s[1].status, s[1].old_path.as_deref()),
            (FileStatus::Added, FileStatus::Renamed, Some("o"))
        );
        assert!(
            s[1].lines.is_empty(),
            "rename metadata is not shown as lines"
        );
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
    fn repo_parses_deployments_by_environment() {
        let v: Value = serde_json::from_str(r#"{"data":{"repository":{"nameWithOwner":"o/r","defaultBranchRef":{"name":"master"},
          "open":{"nodes":[{"number":1,"title":"a","repository":{"nameWithOwner":"o/r"}}]},
          "merged":{"nodes":[{"number":2,"title":"b","mergedAt":"2026-11-19T10:00:00Z","repository":{"nameWithOwner":"o/r"}}]},
          "deployments":{"nodes":[
            {"environment":"production","state":"INACTIVE","createdAt":"2026-11-18T09:00:00Z","ref":{"name":"v1.0.0"}},
            {"environment":"staging","state":"ACTIVE","createdAt":"2026-11-19T09:00:00Z","ref":{"name":"v1.1.0"}},
            {"environment":"production","state":"ACTIVE","createdAt":"2026-11-19T10:00:00Z","ref":{"name":"v1.0.3"},"latestStatus":{"state":"SUCCESS"}},
            {"environment":"dev","state":"ACTIVE","createdAt":"2026-11-19T11:00:00Z","ref":{"name":"v1.2.0"}}]},
          "releases":{"nodes":[{"tagName":"v1.2.0","author":{"login":"x"}}]}}}}"#).unwrap();
        let r = parse_repo(&v).unwrap();
        assert_eq!(
            (r.open.len(), r.merged[0].merged_at.as_str()),
            (1, "2026-11-19T10:00:00Z")
        );
        let envs: Vec<(&str, &str)> = r
            .environments()
            .iter()
            .map(|d| (d.environment.as_str(), d.reference.as_str()))
            .collect();
        assert_eq!(
            envs,
            [
                ("dev", "v1.2.0"),
                ("staging", "v1.1.0"),
                ("production", "v1.0.3")
            ]
        );
        assert_eq!(r.releases[0].tag, "v1.2.0");
    }

    #[test]
    fn repo_activity_is_deferred_to_a_second_query() {
        let fast = repo_request("o/r");
        let fast_query = fast["query"].as_str().unwrap();
        assert!(fast_query.contains("pullRequests"));
        assert!(fast_query.contains("totalCount"));
        assert!(!fast_query.contains("deployments"));
        assert!(!fast_query.contains("releases"));

        let activity = repo_activity_request("o/r");
        let activity_query = activity["query"].as_str().unwrap();
        assert!(activity_query.contains("deployments"));
        assert!(activity_query.contains("releases"));
        assert!(!activity_query.contains("pullRequests"));
        assert_eq!(activity["variables"]["owner"], "o");
        assert_eq!(activity["variables"]["name"], "r");

        let base: Value = serde_json::from_str(
            r#"{"data":{"repository":{"nameWithOwner":"o/r","open":{"totalCount":17,"nodes":[]},"merged":{"nodes":[]}}}}"#,
        )
        .unwrap();
        let detail = parse_repo(&base).unwrap();
        assert_eq!(detail.open_total, 17);
        assert!(detail.deployments.is_empty());
        assert!(detail.releases.is_empty());

        let deferred: Value = serde_json::from_str(
            r#"{"data":{"repository":{"deployments":{"nodes":[{"environment":"production","ref":{"name":"v2"}}]},"releases":{"nodes":[{"tagName":"v2","name":"v2"}]}}}}"#,
        )
        .unwrap();
        let activity = parse_repo_activity(&deferred).unwrap();
        assert_eq!(activity.deployments[0].environment, "production");
        assert_eq!(activity.releases[0].tag, "v2");
    }

    #[test]
    fn remotes_give_slugs() {
        assert_eq!(
            slug_from_remote("git@github.com:join-com/data.git").as_deref(),
            Some("join-com/data")
        );
        assert_eq!(
            slug_from_remote("https://github.com/enekos/zz").as_deref(),
            Some("enekos/zz")
        );
        assert_eq!(slug_from_remote("https://gitlab.com/a/b"), None);
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
