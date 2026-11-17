use crate::clock;
use crate::watch::events::{Event, Kind};
use crate::watch::graph::Index;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn action(e: &Event) -> String {
    match e.kind {
        Kind::Merged => {
            // Anything after "merged by @x" is a ticket the watcher tied to
            // this PR, with the state Linear still has it in.
            let tickets: Vec<&str> = e
                .detail
                .split(" · ")
                .filter(|part| !part.is_empty() && !part.starts_with("merged by"))
                .collect();
            if tickets.is_empty() {
                "tick Next up, add a Recent log bullet".into()
            } else {
                format!("tick Next up, add a Recent log bullet, check {}", tickets.join(", "))
            }
        }
        Kind::Approved => {
            let who = if e.detail.is_empty() { String::new() } else { format!(" by {}", e.detail) };
            format!("approved{who} — ready to merge")
        }
        Kind::ChangesRequested => {
            let who = if e.detail.is_empty() { String::new() } else { format!(" from {}", e.detail) };
            format!("address the review{who}")
        }
        Kind::ChecksRed => {
            if e.detail.is_empty() {
                "checks went red".into()
            } else {
                format!("checks red: {}", e.detail)
            }
        }
        Kind::ChecksGreen => "checks recovered".into(),
        Kind::Closed => "closed unmerged — does a project page still claim it?".into(),
        Kind::ReviewRequested => format!("you are blocking {}", e.detail),
        Kind::IssueAssigned => format!("new ticket ({}): {}", e.detail, e.title),
        Kind::IssueState => format!("{}: {}", e.detail, e.title),
        Kind::IssueDone => format!("{} — close it out on the project page: {}", e.detail, e.title),
        Kind::IssueUnblocked => format!("{} — ready to start: {}", e.detail, e.title),
        Kind::IssuePriority => format!("now {}: {}", e.detail, e.title),
        Kind::IssueCommented => format!("{} commented — reply or act: {}", e.detail, e.title),
    }
}

fn verb(kind: Kind) -> &'static str {
    match kind {
        Kind::Merged => "merged",
        Kind::Approved => "approved",
        Kind::ChangesRequested => "changes",
        Kind::ChecksRed => "red",
        Kind::ChecksGreen => "green",
        Kind::Closed => "closed",
        Kind::ReviewRequested => "review",
        Kind::IssueAssigned => "assigned",
        Kind::IssueState => "moved",
        Kind::IssueDone => "done",
        Kind::IssueUnblocked => "unblocked",
        Kind::IssuePriority => "priority",
        Kind::IssueCommented => "comment",
    }
}

pub fn render(events: &[Event], idx: &Index) -> Vec<String> {
    let mut out = Vec::new();
    for e in events {
        // An issue event carries its Linear project where a PR carries its
        // repo; a project page claims either one by id.
        let repo_id = if e.kind.is_issue() { e.repo.clone() } else { format!("gh:{}", e.repo) };
        let mut context: Vec<String> = Vec::new();
        // A busy monorepo is claimed by several wiki pages; naming them all
        // buries the event under its own links.
        let projects = idx.projects_for(&repo_id);
        for project in projects.iter().take(2) {
            context.push(format!("[[{project}]]"));
        }
        if projects.len() > 2 {
            context.push(format!("+{} more", projects.len() - 2));
        }
        let middle = if context.is_empty() {
            String::new()
        } else {
            format!(" · {}", context.join(" · "))
        };
        let box_ = if e.kind.actionable() { "- [ ] " } else { "- " };
        out.push(format!(
            "{box_}{} {} {}{middle} — {}",
            clock::clock_of(&e.ts),
            verb(e.kind),
            e.slug(),
            action(e)
        ));
    }
    out
}

pub fn path_for(root: &Path, date: &str) -> PathBuf {
    root.join("inbox/events").join(format!("{date}.md"))
}

pub fn append(root: &Path, date: &str, lines: &[String]) -> std::io::Result<PathBuf> {
    let path = path_for(root, date);
    if lines.is_empty() {
        return Ok(path);
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let fresh = !path.exists();
    let mut fh = fs::OpenOptions::new().create(true).append(true).open(&path)?;
    if fresh {
        writeln!(
            fh,
            "# events — {date}\n\nProposals from watched state changes, written by zaindu. Each unchecked box is work /today can rank; nothing here has been done.\n"
        )?;
    }
    for line in lines {
        writeln!(fh, "{line}")?;
    }
    Ok(path)
}

pub fn open_items(root: &Path, date: &str) -> Vec<String> {
    fs::read_to_string(path_for(root, date))
        .map(|t| {
            t.lines()
                .filter(|l| l.starts_with("- [ ] "))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::events::{Kind, diff};
    use crate::watch::fixtures::{issue, pr, snapshot, with_issues};
    use crate::watch::linear;
    use crate::watch::graph::Index;
    use crate::watch::model::{Checks, Lifecycle, Review};

    fn index_with_project() -> Index {
        let mut idx = Index::default();
        idx.project_of_repo.insert(
            "gh:join-com/data".into(),
            vec!["meta-data-service".into()],
        );
        idx
    }

    fn window() -> crate::watch::events::Window {
        crate::watch::events::Window {
            ts: "2026-09-16T10:04:00Z".into(),
            merged_from: "2026-09-02".into(),
            complete: true,
        }
    }

    #[test]
    fn a_merge_proposes_wiki_work_and_names_the_project() {
        let before = pr("join-com/data", 7276, "fix: THU-3461 anchor");
        let mut after = before.clone();
        after.lifecycle = Lifecycle::Merged;
        let curr = snapshot(&[after]);
        let events = diff(&snapshot(&[before]), &curr, &window());
        let lines = render(&events, &index_with_project());
        assert_eq!(
            lines[0],
            "- [ ] 10:04Z merged join-com/data#7276 · [[meta-data-service]] — tick Next up, add a Recent log bullet"
        );
    }

    #[test]
    fn a_merge_with_a_ticket_asks_for_the_ticket_to_be_checked() {
        let mut e = Event {
            ts: "2026-09-16T10:04:00Z".into(),
            kind: Kind::Merged,
            subject: "gh:acme/app#41".into(),
            repo: "acme/app".into(),
            title: "fix: APP-12 anchor".into(),
            url: String::new(),
            detail: "merged by @mergequeue · APP-12 (Code Review)".into(),
        };
        let lines = render(std::slice::from_ref(&e), &Index::default());
        assert_eq!(
            lines[0],
            "- [ ] 10:04Z merged acme/app#41 — tick Next up, add a Recent log bullet, check APP-12 (Code Review)"
        );
        e.detail = "APP-12 (Done)".into();
        assert!(render(&[e], &Index::default())[0].ends_with("check APP-12 (Done)"));
    }

    #[test]
    fn an_issue_event_names_the_page_that_tracks_its_linear_project() {
        let mut idx = Index::default();
        idx.project_of_repo.insert("lin:project:76ed37275476".into(), vec!["importer".into()]);
        let mut fresh = issue("APP-12", "Todo", "unstarted");
        fresh.project = "lin:project:76ed37275476".into();
        fresh.priority = 2;
        let events = linear::diff(&snapshot(&[]), &with_issues(snapshot(&[]), &[fresh.clone()]), "2026-09-16T10:04:00Z");
        assert_eq!(
            render(&events, &idx)[0],
            "- [ ] 10:04Z assigned APP-12 · [[importer]] — new ticket (Todo · high): APP-12 title"
        );

        let mut moved = fresh.clone();
        moved.state = "In Progress".into();
        moved.state_type = "started".into();
        let events = linear::diff(
            &with_issues(snapshot(&[]), &[fresh]),
            &with_issues(snapshot(&[]), &[moved]),
            "2026-09-16T10:09:00Z",
        );
        assert_eq!(
            render(&events, &Index::default())[0],
            "- 10:09Z moved APP-12 — Todo → In Progress: APP-12 title"
        );
    }

    #[test]
    fn a_recovery_is_a_timeline_line_not_a_task() {
        let mut red = pr("join-com/data", 7280, "feat: prompts");
        red.checks = Checks::Red;
        let green = pr("join-com/data", 7280, "feat: prompts");
        let curr = snapshot(&[green]);
        let events = diff(&snapshot(&[red]), &curr, &window());
        let lines = render(&events, &Index::default());
        assert_eq!(events[0].kind, Kind::ChecksGreen);
        assert!(lines[0].starts_with("- 10:04Z green"));
        assert!(!lines[0].contains("[ ]"));
    }

    #[test]
    fn a_review_request_says_who_is_waiting() {
        let mut theirs = pr("join-com/frontend", 13400, "feat: their work");
        theirs.mine = false;
        theirs.author = "colleague".into();
        theirs.review_requested_of_me = true;
        let curr = snapshot(&[theirs]);
        let events = diff(&snapshot(&[]), &curr, &window());
        let lines = render(&events, &Index::default());
        assert_eq!(
            lines[0],
            "- [ ] 10:04Z review join-com/frontend#13400 — you are blocking @colleague"
        );
    }

    #[test]
    fn an_approval_reads_as_ready_to_merge() {
        let open = pr("join-com/data", 7277, "fix: salary");
        let mut approved = open.clone();
        approved.review = Review::Approved;
        approved.review_by = vec!["reviewer".into()];
        let curr = snapshot(&[approved]);
        let lines = render(&diff(&snapshot(&[open]), &curr, &window()), &index_with_project());
        assert!(lines[0].contains("approved by @reviewer — ready to merge"));
        assert!(lines[0].contains("[[meta-data-service]]"));
    }

    #[test]
    fn the_file_gets_a_header_once_and_open_items_read_back() {
        let root = std::env::temp_dir().join(format!("zaindu-inbox-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let lines = vec![
            "- [ ] 10:04Z merged join-com/data#7276 — tick Next up".to_string(),
            "- 10:05Z green join-com/data#7280 — checks recovered".to_string(),
        ];
        append(&root, "2026-09-16", &lines).unwrap();
        append(&root, "2026-09-16", &lines[..1]).unwrap();
        let text = fs::read_to_string(path_for(&root, "2026-09-16")).unwrap();
        assert_eq!(text.matches("# events — 2026-09-16").count(), 1);
        assert_eq!(open_items(&root, "2026-09-16").len(), 2);
        assert!(open_items(&root, "2026-09-15").is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_to_say_writes_no_file() {
        let root = std::env::temp_dir().join(format!("zaindu-quiet-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        append(&root, "2026-09-16", &[]).unwrap();
        assert!(!path_for(&root, "2026-09-16").exists());
    }
}
