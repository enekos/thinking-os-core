//! The status board: what I opened last and what I am, or should be, working
//! on. Read from the poller's own snapshot rather than the graph — it is five
//! minutes old at worst, and it is the only place a creation time survives.

use crate::watch::model::{Checks, Issue, Lifecycle, Pr, Review, Snapshot};
use serde_json::{Value, json};
use std::path::Path;

pub fn load(root: &Path) -> Option<Snapshot> {
    let text = std::fs::read_to_string(root.join(crate::watch::model::STATE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Newest first. State written before `created` was read has only the day of
/// the last update to go on.
fn newest<T>(mut rows: Vec<&T>, created: impl Fn(&T) -> &str, updated: impl Fn(&T) -> &str, id: impl Fn(&T) -> &str) -> Vec<&T> {
    let when = |row: &T| -> String {
        let at = created(row);
        if at.is_empty() { updated(row).to_string() } else { at.to_string() }
    };
    rows.sort_by(|a, b| when(b).cmp(&when(a)).then_with(|| id(a).cmp(id(b))));
    rows
}

fn pr_row(pr: &Pr) -> Value {
    json!({
        "id": pr.id,
        "slug": pr.slug(),
        "title": pr.title,
        "url": pr.url,
        "author": pr.author,
        "draft": pr.draft,
        "review": match pr.review {
            Review::Approved => "approved",
            Review::ChangesRequested => "changes requested",
            Review::None => "",
        },
        "checks": match pr.checks {
            Checks::Red => "red",
            Checks::Green => "green",
            Checks::Pending => "pending",
            Checks::Unknown => "",
        },
        "failing": pr.failing,
        "created": pr.created,
        "updated": pr.updated,
    })
}

fn issue_row(issue: &Issue) -> Value {
    json!({
        "id": issue.id,
        "title": issue.title,
        "url": issue.url,
        "state": issue.state,
        "state_type": issue.state_type,
        "priority": issue.priority,
        "project": issue.project_name,
        "blocked_by": issue.blocked_by,
        "prs": issue.prs,
        "created": issue.created,
        "updated": issue.updated,
    })
}

/// `limit` caps each list; the counts beside them are of the whole.
pub fn board(snap: &Snapshot, limit: usize) -> Value {
    let open: Vec<&Pr> = snap.prs.values().filter(|p| p.lifecycle == Lifecycle::Open).collect();
    let mine: Vec<&Pr> = open.iter().copied().filter(|p| p.mine).collect();
    let review: Vec<&Pr> = open.iter().copied().filter(|p| !p.mine && p.review_requested_of_me).collect();
    let doing: Vec<&Issue> = snap.issues.values().chain(snap.github_issues.values()).filter(|i| i.state_type == "started").collect();
    let next: Vec<&Issue> = snap
        .issues
        .values()
        .chain(snap.github_issues.values())
        .filter(|i| i.is_open() && i.state_type != "started")
        .collect();

    let prs = |rows: Vec<&Pr>| -> Value {
        let total = rows.len();
        let rows = newest(rows, |p| &p.created, |p| &p.updated, |p| &p.id);
        json!({ "total": total, "rows": rows.into_iter().take(limit).map(pr_row).collect::<Vec<_>>() })
    };
    let issues = |rows: Vec<&Issue>| -> Value {
        let total = rows.len();
        let rows = newest(rows, |i| &i.created, |i| &i.updated, |i| &i.id);
        json!({ "total": total, "rows": rows.into_iter().take(limit).map(issue_row).collect::<Vec<_>>() })
    };

    json!({
        "taken_at": snap.taken_at,
        "login": snap.login,
        "linear": !snap.issues_at.is_empty(),
        "github_issues": !snap.github_issues_at.is_empty(),
        "issues_at": {"linear": snap.issues_at, "github": snap.github_issues_at},
        "opened": prs(mine),
        "review": prs(review),
        "doing": issues(doing),
        "next": issues(next),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::fixtures;

    fn ids(board: &Value, list: &str) -> Vec<String> {
        board[list]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn my_open_prs_come_newest_first_and_closed_ones_stay_out() {
        let mut old = fixtures::pr("acme/app", 1, "old");
        old.created = "2026-09-01T08:00:00Z".into();
        let mut new = fixtures::pr("acme/app", 2, "new");
        new.created = "2026-09-10T08:00:00Z".into();
        let mut merged = fixtures::pr("acme/app", 3, "merged");
        merged.created = "2026-09-12T08:00:00Z".into();
        merged.lifecycle = Lifecycle::Merged;

        let board = board(&fixtures::snapshot(&[old, new, merged]), 10);

        assert_eq!(ids(&board, "opened"), ["gh:acme/app#2", "gh:acme/app#1"]);
        assert_eq!(board["opened"]["total"], 2);
    }

    #[test]
    fn a_pr_awaiting_my_review_is_not_one_i_opened() {
        let mut theirs = fixtures::pr("acme/app", 4, "theirs");
        theirs.mine = false;
        theirs.review_requested_of_me = true;
        let mut bystander = fixtures::pr("acme/app", 5, "not asked");
        bystander.mine = false;

        let board = board(&fixtures::snapshot(&[theirs, bystander]), 10);

        assert!(ids(&board, "opened").is_empty());
        assert_eq!(ids(&board, "review"), ["gh:acme/app#4"]);
    }

    #[test]
    fn issues_split_into_started_and_waiting_and_done_ones_drop() {
        let mut doing = fixtures::issue("THU-1", "In Progress", "started");
        doing.created = "2026-09-02T08:00:00.000Z".into();
        let mut todo = fixtures::issue("THU-2", "Todo", "unstarted");
        todo.created = "2026-09-03T08:00:00.000Z".into();
        let mut backlog = fixtures::issue("THU-3", "Backlog", "backlog");
        backlog.created = "2026-09-09T08:00:00.000Z".into();
        let done = fixtures::issue("THU-4", "Done", "completed");
        let snap = fixtures::with_issues(fixtures::snapshot(&[]), &[doing, todo, backlog, done]);

        let board = board(&snap, 10);

        assert_eq!(ids(&board, "doing"), ["THU-1"]);
        assert_eq!(ids(&board, "next"), ["THU-3", "THU-2"]);
        assert_eq!(board["linear"], true);
    }

    #[test]
    fn the_limit_trims_rows_but_not_the_total() {
        let prs: Vec<Pr> = (1..=5).map(|n| fixtures::pr("acme/app", n, "pr")).collect();

        let board = board(&fixtures::snapshot(&prs), 2);

        assert_eq!(board["opened"]["rows"].as_array().unwrap().len(), 2);
        assert_eq!(board["opened"]["total"], 5);
    }

    #[test]
    fn state_without_a_creation_time_falls_back_to_the_last_update() {
        let mut stale = fixtures::pr("acme/app", 1, "stale");
        stale.updated = "2026-01-04".into();
        let fresh = fixtures::pr("acme/app", 2, "fresh");

        let board = board(&fixtures::snapshot(&[stale, fresh]), 10);

        assert_eq!(ids(&board, "opened"), ["gh:acme/app#2", "gh:acme/app#1"]);
    }
}
