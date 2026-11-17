use crate::watch::model::{Checks, Lifecycle, Pr, Review, Snapshot};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Merged,
    Approved,
    ChangesRequested,
    ChecksRed,
    ChecksGreen,
    Closed,
    ReviewRequested,
    IssueAssigned,
    IssueState,
    IssueDone,
    IssueUnblocked,
    IssuePriority,
    IssueCommented,
}

const KINDS: [Kind; 13] = [
    Kind::Merged,
    Kind::Approved,
    Kind::ChangesRequested,
    Kind::ChecksRed,
    Kind::ChecksGreen,
    Kind::Closed,
    Kind::ReviewRequested,
    Kind::IssueAssigned,
    Kind::IssueState,
    Kind::IssueDone,
    Kind::IssueUnblocked,
    Kind::IssuePriority,
    Kind::IssueCommented,
];

impl Kind {
    pub fn tag(self) -> &'static str {
        match self {
            Kind::Merged => "pr.merged",
            Kind::Approved => "pr.approved",
            Kind::ChangesRequested => "pr.changes_requested",
            Kind::ChecksRed => "pr.checks_red",
            Kind::ChecksGreen => "pr.checks_green",
            Kind::Closed => "pr.closed",
            Kind::ReviewRequested => "pr.review_requested",
            Kind::IssueAssigned => "issue.assigned",
            Kind::IssueState => "issue.state_changed",
            Kind::IssueDone => "issue.done",
            Kind::IssueUnblocked => "issue.unblocked",
            Kind::IssuePriority => "issue.priority_raised",
            Kind::IssueCommented => "issue.commented",
        }
    }

    pub fn parse(tag: &str) -> Option<Kind> {
        KINDS
            .into_iter()
            .find(|k| k.tag() == tag || k.tag().split_once('.').is_some_and(|(_, short)| short == tag))
    }

    /// A recovery and a plain state move are timeline, not work.
    pub fn actionable(self) -> bool {
        !matches!(self, Kind::ChecksGreen | Kind::IssueState)
    }

    pub fn is_issue(self) -> bool {
        self.tag().starts_with("issue.")
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub ts: String,
    pub kind: Kind,
    pub subject: String,
    pub repo: String,
    pub title: String,
    pub url: String,
    pub detail: String,
}

pub const COLS: [&str; 7] = ["ts", "kind", "subject", "repo", "title", "url", "detail"];

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

impl Event {
    pub fn row(&self) -> String {
        [
            &self.ts,
            self.kind.tag(),
            &self.subject,
            &self.repo,
            &self.title,
            &self.url,
            &self.detail,
        ]
        .iter()
        .map(|f| clean(f))
        .collect::<Vec<_>>()
        .join("\t")
    }

    pub fn slug(&self) -> String {
        self.subject.strip_prefix("gh:").unwrap_or(&self.subject).to_string()
    }

    fn of(ts: &str, kind: Kind, pr: &Pr, detail: String) -> Event {
        Event {
            ts: ts.to_string(),
            kind,
            subject: pr.id.clone(),
            repo: pr.nwo().to_string(),
            title: pr.title.clone(),
            url: pr.url.clone(),
            detail,
        }
    }
}

pub struct Window {
    pub ts: String,
    pub merged_from: String,
    pub complete: bool,
}

fn joined(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("@{n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn diff(prev: &Snapshot, curr: &Snapshot, win: &Window) -> Vec<Event> {
    let mut out = Vec::new();
    for pr in curr.sorted() {
        let was = prev.prs.get(&pr.id);
        if pr.mine {
            let was_merged = was.map(|p| p.lifecycle == Lifecycle::Merged).unwrap_or(false);
            if pr.lifecycle == Lifecycle::Merged && !was_merged {
                let detail = if pr.merged_by.is_empty() {
                    String::new()
                } else {
                    format!("merged by @{}", pr.merged_by)
                };
                out.push(Event::of(&win.ts, Kind::Merged, pr, detail));
            }
            let was_closed = was.map(|p| p.lifecycle == Lifecycle::Closed).unwrap_or(false);
            if pr.lifecycle == Lifecycle::Closed && !was_closed {
                out.push(Event::of(&win.ts, Kind::Closed, pr, "closed unmerged".into()));
            }

            if pr.lifecycle == Lifecycle::Open {
                let was_review = was.map(|p| p.review).unwrap_or(Review::None);
                if pr.review != was_review {
                    match pr.review {
                        Review::Approved => out.push(Event::of(
                            &win.ts,
                            Kind::Approved,
                            pr,
                            joined(&pr.review_by),
                        )),
                        Review::ChangesRequested => out.push(Event::of(
                            &win.ts,
                            Kind::ChangesRequested,
                            pr,
                            joined(&pr.review_by),
                        )),
                        Review::None => {}
                    }
                }
                let was_checks = was.map(|p| p.checks).unwrap_or(Checks::Unknown);
                if pr.checks == Checks::Red && was_checks != Checks::Red {
                    out.push(Event::of(&win.ts, Kind::ChecksRed, pr, pr.failing.join(", ")));
                }
                if pr.checks == Checks::Green && was_checks == Checks::Red {
                    out.push(Event::of(&win.ts, Kind::ChecksGreen, pr, String::new()));
                }
            }
        } else if pr.review_requested_of_me
            && !was.map(|p| p.review_requested_of_me).unwrap_or(false)
        {
            out.push(Event::of(
                &win.ts,
                Kind::ReviewRequested,
                pr,
                format!("@{}", pr.author),
            ));
        }
    }

    if win.complete {
        for pr in prev.prs.values() {
            let gone = !curr.prs.contains_key(&pr.id);
            let watchable = pr.mine
                && pr.lifecycle == Lifecycle::Open
                && pr.updated.as_str() >= win.merged_from.as_str();
            if gone && watchable {
                out.push(Event::of(&win.ts, Kind::Closed, pr, "closed unmerged".into()));
            }
        }
    }
    out
}

/// The ledger is one file per month, `graph/events/YYYY-MM.tsv`, named by each
/// event's own timestamp. A single `events.tsv` grew without bound — 615 KB in
/// two months — and every reader and every diff paid for all of it.
pub fn ledger_dir(root: &Path) -> PathBuf {
    root.join("graph/events")
}

pub fn ledger_for(root: &Path, ts: &str) -> PathBuf {
    let month = ts.get(..7).unwrap_or("unknown");
    ledger_dir(root).join(format!("{month}.tsv"))
}

/// Appends each event to its month's file and returns each file touched with
/// how many rows it gained, in first-touched order.
pub fn append_monthly(root: &Path, events: &[Event]) -> std::io::Result<Vec<(PathBuf, usize)>> {
    let mut groups: Vec<(PathBuf, Vec<Event>)> = Vec::new();
    for e in events {
        let path = ledger_for(root, &e.ts);
        match groups.iter_mut().find(|(p, _)| *p == path) {
            Some((_, group)) => group.push(e.clone()),
            None => groups.push((path, vec![e.clone()])),
        }
    }
    for (path, group) in &groups {
        append(path, group)?;
    }
    Ok(groups.into_iter().map(|(p, g)| (p, g.len())).collect())
}

/// Every month's ledger, oldest first.
pub fn read_all(root: &Path) -> Vec<Event> {
    let mut files: Vec<PathBuf> = match fs::read_dir(ledger_dir(root)) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "tsv"))
            .collect(),
        Err(_) => return Vec::new(),
    };
    files.sort();
    files.iter().flat_map(|p| read(p)).collect()
}

pub fn append(path: &Path, events: &[Event]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let fresh = !path.exists() || fs::metadata(path)?.len() == 0;
    let mut fh = fs::OpenOptions::new().create(true).append(true).open(path)?;
    if fresh {
        writeln!(fh, "{}", COLS.join("\t"))?;
    }
    for e in events {
        writeln!(fh, "{}", e.row())?;
    }
    Ok(())
}

pub fn read(path: &Path) -> Vec<Event> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 3 {
            continue;
        }
        let kind = match Kind::parse(f[1]) {
            Some(k) => k,
            None => continue,
        };
        let at = |i: usize| f.get(i).copied().unwrap_or("").to_string();
        out.push(Event {
            ts: at(0),
            kind,
            subject: at(2),
            repo: at(3),
            title: at(4),
            url: at(5),
            detail: at(6),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::fixtures::{pr, snapshot};

    fn window() -> Window {
        Window {
            ts: "2026-09-16T10:04:00Z".into(),
            merged_from: "2026-09-02".into(),
            complete: true,
        }
    }

    fn kinds(events: &[Event]) -> Vec<Kind> {
        events.iter().map(|e| e.kind).collect()
    }

    #[test]
    fn a_merge_fires_once_and_names_who_merged() {
        let before = pr("join-com/data", 7276, "fix: THU-3461 anchor");
        let mut after = before.clone();
        after.lifecycle = Lifecycle::Merged;
        after.merged_by = "mergequeue".into();

        let first = diff(&snapshot(&[before]), &snapshot(&[after.clone()]), &window());
        assert_eq!(kinds(&first), vec![Kind::Merged]);
        assert_eq!(first[0].detail, "merged by @mergequeue");
        assert_eq!(first[0].subject, "gh:join-com/data#7276");

        let again = diff(&snapshot(&[after.clone()]), &snapshot(&[after]), &window());
        assert!(again.is_empty());
    }

    #[test]
    fn an_approval_and_a_later_change_request_both_fire() {
        let open = pr("join-com/data", 7277, "fix: salary conflict");
        let mut approved = open.clone();
        approved.review = Review::Approved;
        approved.review_by = vec!["mohammed-ali-1".into()];
        let events = diff(&snapshot(&[open]), &snapshot(&[approved.clone()]), &window());
        assert_eq!(kinds(&events), vec![Kind::Approved]);
        assert_eq!(events[0].detail, "@mohammed-ali-1");

        let mut blocked = approved.clone();
        blocked.review = Review::ChangesRequested;
        blocked.review_by = vec!["reviewer".into()];
        let events = diff(&snapshot(&[approved]), &snapshot(&[blocked]), &window());
        assert_eq!(kinds(&events), vec![Kind::ChangesRequested]);
    }

    #[test]
    fn checks_fire_on_the_transition_in_both_directions() {
        let green = pr("join-com/data", 7280, "feat: prompts");
        let mut red = green.clone();
        red.checks = Checks::Red;
        red.failing = vec!["ci/circleci: test".into()];

        let down = diff(&snapshot(&[green.clone()]), &snapshot(&[red.clone()]), &window());
        assert_eq!(kinds(&down), vec![Kind::ChecksRed]);
        assert_eq!(down[0].detail, "ci/circleci: test");

        let still = diff(&snapshot(&[red.clone()]), &snapshot(&[red.clone()]), &window());
        assert!(still.is_empty());

        let up = diff(&snapshot(&[red]), &snapshot(&[green]), &window());
        assert_eq!(kinds(&up), vec![Kind::ChecksGreen]);
        assert!(!up[0].kind.actionable());
    }

    #[test]
    fn a_pending_run_is_not_a_recovery() {
        let mut red = pr("join-com/data", 7280, "feat: prompts");
        red.checks = Checks::Red;
        let mut pending = red.clone();
        pending.checks = Checks::Pending;
        assert!(diff(&snapshot(&[red]), &snapshot(&[pending]), &window()).is_empty());
    }

    #[test]
    fn a_pr_that_vanishes_from_an_otherwise_complete_poll_was_closed() {
        let open = pr("join-com/data", 7100, "chore: obsolete");
        let events = diff(&snapshot(&[open.clone()]), &snapshot(&[]), &window());
        assert_eq!(kinds(&events), vec![Kind::Closed]);

        let mut stale = open;
        stale.updated = "2026-01-04".into();
        assert!(diff(&snapshot(&[stale]), &snapshot(&[]), &window()).is_empty());
    }

    #[test]
    fn an_incomplete_poll_never_invents_a_close() {
        let open = pr("join-com/data", 7100, "chore: obsolete");
        let mut win = window();
        win.complete = false;
        assert!(diff(&snapshot(&[open]), &snapshot(&[]), &win).is_empty());
    }

    #[test]
    fn someone_elses_pr_only_produces_a_review_request() {
        let mut theirs = pr("join-com/frontend", 13400, "feat: their work");
        theirs.mine = false;
        theirs.author = "colleague".into();
        theirs.review_requested_of_me = true;
        theirs.checks = Checks::Red;
        theirs.review = Review::Approved;

        let events = diff(&snapshot(&[]), &snapshot(&[theirs.clone()]), &window());
        assert_eq!(kinds(&events), vec![Kind::ReviewRequested]);
        assert_eq!(events[0].detail, "@colleague");

        let again = diff(&snapshot(&[theirs.clone()]), &snapshot(&[theirs]), &window());
        assert!(again.is_empty());
    }

    #[test]
    fn a_merged_pr_stops_being_graded_on_review_and_checks() {
        let mut before = pr("join-com/data", 7276, "fix: anchor");
        before.checks = Checks::Red;
        let mut after = before.clone();
        after.lifecycle = Lifecycle::Merged;
        after.checks = Checks::Green;
        after.review = Review::Approved;
        assert_eq!(kinds(&diff(&snapshot(&[before]), &snapshot(&[after]), &window())), vec![Kind::Merged]);
    }

    #[test]
    fn the_ledger_round_trips_and_tabs_never_reach_it() {
        let dir = std::env::temp_dir().join(format!("zaindu-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("graph/events.tsv");
        let mut messy = pr("join-com/data", 7276, "fix:\tan\nawkward\ttitle");
        messy.lifecycle = Lifecycle::Merged;
        let events = diff(&snapshot(&[pr("join-com/data", 7276, "x")]), &snapshot(&[messy]), &window());

        append(&path, &events).expect("writes");
        append(&path, &events).expect("appends");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().next().unwrap(), COLS.join("\t"));
        assert_eq!(text.lines().count(), 3);
        for line in text.lines().skip(1) {
            assert_eq!(line.split('\t').count(), COLS.len());
        }
        let back = read(&path);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].kind, Kind::Merged);
        assert_eq!(back[0].title, "fix: an awkward title");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ledger_splits_by_month_and_reads_back_in_order() {
        let root = std::env::temp_dir().join(format!("zaindu-monthly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut merged = pr("join-com/data", 7276, "x");
        merged.lifecycle = Lifecycle::Merged;
        let mut events = diff(&snapshot(&[pr("join-com/data", 7276, "x")]), &snapshot(&[merged]), &window());
        let mut later = events[0].clone();
        events[0].ts = "2026-10-31T23:59:00Z".into();
        later.ts = "2026-11-01T00:01:00Z".into();
        events.push(later);

        let touched = append_monthly(&root, &events).expect("writes");
        assert_eq!(touched, vec![(ledger_for(&root, "2026-10"), 1), (ledger_for(&root, "2026-11"), 1)]);
        let back = read_all(&root);
        let stamps: Vec<&str> = back.iter().map(|e| e.ts.as_str()).collect();
        assert_eq!(stamps, ["2026-10-31T23:59:00Z", "2026-11-01T00:01:00Z"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reading_a_ledger_that_does_not_exist_is_empty_not_a_crash() {
        assert!(read(std::path::Path::new("/nonexistent/events.tsv")).is_empty());
    }
}
