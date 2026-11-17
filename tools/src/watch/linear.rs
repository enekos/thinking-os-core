//! Linear as a second watched source. Off unless `os.yaml` carries a `linear:`
//! key. Read-only: one paginated GraphQL query through the `linear` CLI, which
//! owns the credentials (`$LINEAR_API_KEY` or `linear auth login`).

use crate::kg::run_capture;
use crate::watch::events::{Event, Kind};
use crate::watch::model::{Issue, Pr, Snapshot};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

/// Every issue assigned to me that is still open, plus the ones that finished
/// inside the window — an issue has to be seen once more after it completes or
/// the transition would be a silent disappearance. The explicit `and` matters:
/// two fields side by side inside an `or` branch are not conjoined by Linear.
pub const QUERY: &str = r#"
query($after: String, $since: DateTimeOrDuration) {
  viewer {
    assignedIssues(first: 50, after: $after, orderBy: updatedAt,
      filter: { or: [
        { state: { type: { in: ["triage", "backlog", "unstarted", "started"] } } },
        { and: [
          { updatedAt: { gte: $since } },
          { state: { type: { eq: "completed" } } }
        ] }
      ] }) {
      pageInfo { hasNextPage endCursor }
      nodes {
        identifier title priority createdAt updatedAt url
        state { name type }
        project { id name state health url }
        cycle { id number startsAt endsAt team { key } }
        assignee { displayName name }
        parent { identifier title }
        attachments(first: 8) { nodes { url sourceType } }
        relations(first: 8) { nodes { type relatedIssue { identifier title } } }
        inverseRelations(first: 8) { nodes { type issue { identifier state { type } } } }
        comments(last: 1) { nodes { createdAt user { displayName name isMe } } }
      }
    }
  }
}
"#;

const PAGE_LIMIT: usize = 8;
pub const OPEN_TYPES: [&str; 4] = ["triage", "backlog", "unstarted", "started"];
const TITLE_WIDTH: usize = 70;

/// Words a branch name puts in front of a number that are not team keys.
const BRANCH_NOISE: [&str; 16] = [
    "FIX", "FEAT", "CHORE", "TEST", "DOCS", "WIP", "PART", "STEP", "BUG", "PR", "ISSUE", "REFACTOR",
    "PERF", "CI", "REV", "TMP",
];

pub struct Pull {
    pub nodes: Vec<Value>,
    /// False when pagination stopped early; a partial list never drives events.
    pub complete: bool,
}

fn page(since: &str, cursor: Option<&str>) -> Result<Value, String> {
    let variables = json!({ "since": since, "after": cursor }).to_string();
    let body = run_capture(
        Command::new("linear").args(["api", QUERY, "--variables-json", &variables]),
        Duration::from_secs(90),
    )?;
    let parsed: Value = serde_json::from_str(&body).map_err(|e| format!("unparseable response: {e}"))?;
    if let Some(message) = parsed.pointer("/errors/0/message").and_then(Value::as_str) {
        return Err(message.to_string());
    }
    parsed
        .pointer("/data/viewer/assignedIssues")
        .cloned()
        .ok_or_else(|| "response carried no assignedIssues".to_string())
}

/// `since` is a `YYYY-MM-DD` date: how far back a completed issue stays visible.
pub fn fetch(since: &str) -> Result<Pull, String> {
    let mut nodes: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..PAGE_LIMIT {
        let page = match page(since, cursor.as_deref()) {
            Ok(p) => p,
            Err(e) if nodes.is_empty() => return Err(e),
            Err(_) => return Ok(Pull { nodes, complete: false }),
        };
        if let Some(items) = page.get("nodes").and_then(Value::as_array) {
            nodes.extend(items.iter().cloned());
        }
        let more = page.pointer("/pageInfo/hasNextPage").and_then(Value::as_bool).unwrap_or(false);
        cursor = page.pointer("/pageInfo/endCursor").and_then(Value::as_str).map(str::to_string);
        if !more || cursor.is_none() {
            return Ok(Pull { nodes, complete: true });
        }
    }
    Ok(Pull { nodes, complete: false })
}

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or("").to_string()
}

pub fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "urgent",
        2 => "high",
        3 => "medium",
        4 => "low",
        _ => "none",
    }
}

/// A Linear project's graph id. Named milestone because `project` is the
/// wiki's own node type.
pub fn milestone_id(uuid: &str) -> String {
    format!("lin:project:{}", uuid.rsplit('-').next().unwrap_or(""))
}

fn pr_url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"github\.com/([\w.-]+)/([\w.-]+)/pull/(\d+)").expect("a literal pattern"))
}

/// `gh:<owner>/<repo>#<n>` for a GitHub pull-request URL, the id zaindu and
/// the github extractor already use.
pub fn pr_id_of(url: &str) -> Option<String> {
    pr_url().captures(url).map(|m| format!("gh:{}/{}#{}", &m[1], &m[2], &m[3]))
}

pub fn parse(nodes: &[Value]) -> Vec<Issue> {
    let mut by_id: BTreeMap<String, Issue> = BTreeMap::new();
    for node in nodes {
        let id = text(node, "/identifier");
        if id.is_empty() {
            continue;
        }
        let mut blocked_by: Vec<String> = node
            .pointer("/inverseRelations/nodes")
            .and_then(Value::as_array)
            .map(|rels| {
                rels.iter()
                    .filter(|r| text(r, "/type") == "blocks")
                    .filter(|r| OPEN_TYPES.contains(&text(r, "/issue/state/type").as_str()))
                    .map(|r| text(r, "/issue/identifier"))
                    .filter(|other| !other.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        blocked_by.sort();
        blocked_by.dedup();

        let mut prs: Vec<String> = node
            .pointer("/attachments/nodes")
            .and_then(Value::as_array)
            .map(|atts| atts.iter().filter_map(|a| pr_id_of(&text(a, "/url"))).collect())
            .unwrap_or_default();
        prs.sort();
        prs.dedup();

        // A comment of my own, or one with no user behind it (an integration),
        // is recorded with no author so it never reads as someone waiting.
        let comment = node.pointer("/comments/nodes/0").cloned().unwrap_or(Value::Null);
        let by_me = comment.pointer("/user/isMe").and_then(Value::as_bool).unwrap_or(false);
        let author = match (text(&comment, "/user/displayName"), text(&comment, "/user/name")) {
            _ if by_me => String::new(),
            (display, name) if display.is_empty() => name,
            (display, _) => display,
        };

        let project = node.get("project").filter(|p| !p.is_null());
        let issue = Issue {
            id: id.clone(),
            title: text(node, "/title"),
            url: text(node, "/url"),
            state: text(node, "/state/name"),
            state_type: text(node, "/state/type"),
            priority: node.get("priority").and_then(Value::as_f64).unwrap_or(0.0) as i64,
            project: project.map(|p| milestone_id(&text(p, "/id"))).unwrap_or_default(),
            project_name: project.map(|p| text(p, "/name")).unwrap_or_default(),
            updated: text(node, "/updatedAt").chars().take(10).collect(),
            created: text(node, "/createdAt"),
            blocked_by,
            comment_at: text(&comment, "/createdAt"),
            comment_by: author,
            prs,
        };
        by_id.insert(id, issue);
    }
    by_id.into_values().collect()
}

fn event(ts: &str, kind: Kind, issue: &Issue, detail: String) -> Event {
    Event {
        ts: ts.to_string(),
        kind,
        subject: issue.id.clone(),
        repo: issue.project.clone(),
        title: issue.title.chars().take(TITLE_WIDTH).collect(),
        url: issue.url.clone(),
        detail,
    }
}

/// Transitions between two complete issue lists. An issue that vanished was
/// reassigned, cancelled or aged out of the window; none of those is work, so
/// nothing fires.
pub fn diff(prev: &Snapshot, curr: &Snapshot, ts: &str) -> Vec<Event> {
    let mut out = Vec::new();
    for issue in curr.issues.values() {
        let Some(was) = prev.issues.get(&issue.id) else {
            if issue.is_open() {
                let detail = format!("{} · {}", issue.state, priority_label(issue.priority));
                out.push(event(ts, Kind::IssueAssigned, issue, detail));
            }
            continue;
        };
        if issue.state != was.state {
            let detail = format!("{} → {}", was.state, issue.state);
            let kind = if issue.state_type == "completed" && was.state_type != "completed" {
                Kind::IssueDone
            } else {
                Kind::IssueState
            };
            out.push(event(ts, kind, issue, detail));
        }
        if !issue.is_open() {
            continue;
        }
        if !was.blocked_by.is_empty() && issue.blocked_by.is_empty() {
            out.push(event(ts, Kind::IssueUnblocked, issue, format!("was blocked by {}", was.blocked_by.join(", "))));
        }
        let raised = matches!(issue.priority, 1 | 2) && (was.priority == 0 || was.priority > issue.priority);
        if raised {
            out.push(event(ts, Kind::IssuePriority, issue, priority_label(issue.priority).to_string()));
        }
        if !issue.comment_by.is_empty() && issue.comment_at.as_str() > was.comment_at.as_str() {
            out.push(event(ts, Kind::IssueCommented, issue, format!("@{}", issue.comment_by)));
        }
    }
    out
}

/// Ticket ids written in a PR title or branch. A branch is lowercase and full
/// of `fix-123` shapes, so it gets the noise list; a title must already be
/// uppercase.
pub fn tickets_in(text: &str, from_branch: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let boundary_ok = start == 0 || !chars[start - 1].is_ascii_alphanumeric();
        if !boundary_ok || word.len() < 2 || word.len() > 5 || i >= chars.len() || chars[i] != '-' {
            continue;
        }
        if !from_branch && word.chars().any(|c| c.is_ascii_lowercase()) {
            continue;
        }
        let num_start = i + 1;
        let mut j = num_start;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if !(1..=6).contains(&(j - num_start)) {
            continue;
        }
        if j < chars.len() && chars[j].is_ascii_alphanumeric() {
            continue;
        }
        let team = word.to_ascii_uppercase();
        let number: String = chars[num_start..j].iter().collect();
        if !(from_branch && BRANCH_NOISE.contains(&team.as_str())) {
            out.insert(format!("{team}-{number}"));
        }
        i = j;
    }
    out.into_iter().collect()
}

/// The tickets a PR implements: the ones Linear itself attached the PR to,
/// then ids read from the title and branch — but only for a team this
/// workspace is known to have, so `UTF-8` never becomes a ticket.
pub fn tickets_for(pr: &Pr, snap: &Snapshot, teams: &[String]) -> Vec<String> {
    let mut found: BTreeSet<String> = snap
        .issues
        .values()
        .filter(|i| i.prs.contains(&pr.id))
        .map(|i| i.id.clone())
        .collect();
    let known_team = |ticket: &str| {
        let team = ticket.split('-').next().unwrap_or("");
        teams.iter().any(|t| t.eq_ignore_ascii_case(team))
            || snap.issues.keys().any(|id| id.split('-').next() == Some(team))
    };
    for ticket in tickets_in(&pr.title, false).into_iter().chain(tickets_in(&pr.head, true)) {
        if snap.issues.contains_key(&ticket) || known_team(&ticket) {
            found.insert(ticket);
        }
    }
    found.into_iter().collect()
}

/// A merge is the moment a ticket's state is most likely stale, so the merge
/// event names the ticket and the state Linear has it in.
pub fn annotate_merges(events: &mut [Event], snap: &Snapshot, teams: &[String]) {
    for e in events.iter_mut().filter(|e| e.kind == Kind::Merged) {
        let Some(pr) = snap.prs.get(&e.subject) else {
            continue;
        };
        for ticket in tickets_for(pr, snap, teams) {
            let note = match snap.issues.get(&ticket) {
                Some(issue) => format!("{ticket} ({})", issue.state),
                None => ticket,
            };
            if !e.detail.is_empty() {
                e.detail.push_str(" · ");
            }
            e.detail.push_str(&note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::fixtures::{issue, issue_node, pr, snapshot, with_issues};
    use crate::watch::model::Lifecycle;

    const TS: &str = "2026-09-16T10:04:00Z";

    fn kinds(events: &[Event]) -> Vec<Kind> {
        events.iter().map(|e| e.kind).collect()
    }

    fn step(before: &[Issue], after: &[Issue]) -> Vec<Event> {
        diff(&with_issues(snapshot(&[]), before), &with_issues(snapshot(&[]), after), TS)
    }

    #[test]
    fn a_new_open_issue_is_an_assignment_and_a_new_finished_one_is_nothing() {
        let fresh = issue("APP-12", "Todo", "unstarted");
        let events = step(&[], std::slice::from_ref(&fresh));
        assert_eq!(kinds(&events), vec![Kind::IssueAssigned]);
        assert_eq!(events[0].detail, "Todo · none");
        assert_eq!(events[0].subject, "APP-12");
        assert!(step(std::slice::from_ref(&fresh), std::slice::from_ref(&fresh)).is_empty());

        let done = issue("APP-13", "Done", "completed");
        assert!(step(&[], &[done]).is_empty());
    }

    #[test]
    fn a_state_change_names_both_ends_and_completion_is_its_own_kind() {
        let todo = issue("APP-12", "Todo", "unstarted");
        let doing = issue("APP-12", "In Progress", "started");
        let done = issue("APP-12", "Done", "completed");

        let moved = step(&[todo], std::slice::from_ref(&doing));
        assert_eq!(kinds(&moved), vec![Kind::IssueState]);
        assert_eq!(moved[0].detail, "Todo → In Progress");
        assert!(!moved[0].kind.actionable());

        let finished = step(&[doing], std::slice::from_ref(&done));
        assert_eq!(kinds(&finished), vec![Kind::IssueDone]);
        assert!(finished[0].kind.actionable());

        let shipped = issue("APP-12", "Released", "completed");
        assert_eq!(kinds(&step(&[done], &[shipped])), vec![Kind::IssueState]);
    }

    #[test]
    fn an_issue_that_vanishes_fires_nothing() {
        assert!(step(&[issue("APP-12", "Todo", "unstarted")], &[]).is_empty());
    }

    #[test]
    fn losing_the_last_open_blocker_unblocks() {
        let mut blocked = issue("APP-12", "Todo", "unstarted");
        blocked.blocked_by = vec!["APP-3".into(), "APP-4".into()];
        let mut half = blocked.clone();
        half.blocked_by = vec!["APP-4".into()];
        let free = issue("APP-12", "Todo", "unstarted");

        assert!(step(&[blocked.clone()], &[half.clone()]).is_empty());
        let events = step(&[half], &[free]);
        assert_eq!(kinds(&events), vec![Kind::IssueUnblocked]);
        assert_eq!(events[0].detail, "was blocked by APP-4");
    }

    #[test]
    fn priority_only_fires_when_raised_to_high_or_urgent() {
        let at = |p: i64| {
            let mut i = issue("APP-12", "Todo", "unstarted");
            i.priority = p;
            i
        };
        assert_eq!(kinds(&step(&[at(3)], &[at(2)])), vec![Kind::IssuePriority]);
        assert_eq!(step(&[at(0)], &[at(1)])[0].detail, "urgent");
        assert!(step(&[at(1)], &[at(2)]).is_empty());
        assert!(step(&[at(4)], &[at(3)]).is_empty());
        assert!(step(&[at(2)], &[at(2)]).is_empty());
    }

    #[test]
    fn a_new_comment_fires_unless_it_is_mine() {
        let quiet = issue("APP-12", "Todo", "unstarted");
        let mut theirs = quiet.clone();
        theirs.comment_at = "2026-09-16T09:00:00.000Z".into();
        theirs.comment_by = "colleague".into();
        let events = step(std::slice::from_ref(&quiet), &[theirs.clone()]);
        assert_eq!(kinds(&events), vec![Kind::IssueCommented]);
        assert_eq!(events[0].detail, "@colleague");
        assert!(step(&[theirs.clone()], &[theirs.clone()]).is_empty());

        let mut mine = theirs.clone();
        mine.comment_at = "2026-09-16T09:30:00.000Z".into();
        mine.comment_by = String::new();
        assert!(step(&[theirs], &[mine]).is_empty());
    }

    #[test]
    fn parsing_keeps_open_blockers_attached_prs_and_who_commented() {
        let mut node = issue_node("APP-12", "Fix the anchor", "In Progress", "started");
        node["priority"] = json!(2);
        node["project"] = json!({"id": "aaaa-bbbb-76ed37275476", "name": "Importer", "state": "started"});
        node["attachments"]["nodes"] = json!([
            {"url": "https://github.com/acme/app/pull/41", "sourceType": "github"},
            {"url": "https://example.com/doc", "sourceType": "api"}
        ]);
        node["inverseRelations"]["nodes"] = json!([
            {"type": "blocks", "issue": {"identifier": "APP-3", "state": {"type": "started"}}},
            {"type": "blocks", "issue": {"identifier": "APP-4", "state": {"type": "completed"}}},
            {"type": "related", "issue": {"identifier": "APP-5", "state": {"type": "started"}}}
        ]);
        node["comments"]["nodes"] = json!([
            {"createdAt": "2026-09-16T09:00:00.000Z", "user": {"displayName": "colleague", "name": "A Colleague", "isMe": false}}
        ]);
        let mine = json!({"createdAt": "2026-09-16T09:00:00.000Z", "user": {"displayName": "me", "name": "Me", "isMe": true}});
        let mut other = issue_node("APP-13", "Second", "Todo", "unstarted");
        other["comments"]["nodes"] = json!([mine]);

        let issues = parse(&[node, other, json!({"title": "no identifier"})]);
        assert_eq!(issues.len(), 2);
        let first = &issues[0];
        assert_eq!(first.priority, 2);
        assert_eq!(first.project, "lin:project:76ed37275476");
        assert_eq!(first.project_name, "Importer");
        assert_eq!(first.blocked_by, vec!["APP-3"]);
        assert_eq!(first.prs, vec!["gh:acme/app#41"]);
        assert_eq!(first.comment_by, "colleague");
        assert_eq!(first.updated, "2026-09-16");
        assert!(first.is_open());
        assert_eq!(issues[1].comment_by, "", "my own comment has no author to wait on");
        assert_eq!(issues[1].comment_at, "2026-09-16T09:00:00.000Z");
    }

    #[test]
    fn a_ticket_is_read_from_a_title_and_from_a_branch() {
        assert_eq!(tickets_in("fix: APP-3461 stop an unmatched anchor", false), vec!["APP-3461"]);
        assert_eq!(tickets_in("fix-app-3461-anchor", true), vec!["APP-3461"]);
        assert!(tickets_in("fix-app-3461-anchor", false).is_empty());
        assert!(tickets_in("fix-7280", true).is_empty());
        assert!(tickets_in("chore-1234-bump", true).is_empty());
        assert!(tickets_in("feat-m53-shared-prompt-fragments", true).is_empty());
    }

    #[test]
    fn a_merge_names_its_ticket_and_the_state_linear_has_it_in() {
        let mut merged = pr("acme/app", 41, "fix: APP-12 anchor, UTF-8 safe");
        merged.lifecycle = Lifecycle::Merged;
        merged.head = "app-12-anchor".into();
        let snap = with_issues(snapshot(&[merged.clone()]), &[issue("APP-12", "Code Review", "started")]);

        assert_eq!(tickets_for(&merged, &snap, &[]), vec!["APP-12"], "UTF is not a team here");
        assert_eq!(tickets_for(&merged, &snap, &["utf".into()]), vec!["APP-12", "UTF-8"]);

        let mut events = vec![Event {
            ts: TS.into(),
            kind: Kind::Merged,
            subject: merged.id.clone(),
            repo: "acme/app".into(),
            title: merged.title.clone(),
            url: merged.url.clone(),
            detail: "merged by @mergequeue".into(),
        }];
        annotate_merges(&mut events, &snap, &[]);
        assert_eq!(events[0].detail, "merged by @mergequeue · APP-12 (Code Review)");
    }

    #[test]
    fn an_attachment_links_a_pr_whose_title_names_no_ticket() {
        let quiet = pr("acme/app", 42, "fix: anchor");
        let mut linked = issue("APP-14", "In Progress", "started");
        linked.prs = vec![quiet.id.clone()];
        let snap = with_issues(snapshot(std::slice::from_ref(&quiet)), &[linked]);
        assert_eq!(tickets_for(&quiet, &snap, &[]), vec!["APP-14"]);
    }
}
