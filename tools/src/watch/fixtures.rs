use crate::watch::model::{Checks, Issue, Lifecycle, Pr, Review, Snapshot};
use serde_json::{Value, json};

pub fn pr(nwo: &str, number: u64, title: &str) -> Pr {
    Pr {
        id: format!("gh:{nwo}#{number}"),
        repo: format!("gh:{nwo}"),
        number,
        title: title.to_string(),
        url: format!("https://github.com/{nwo}/pull/{number}"),
        lifecycle: Lifecycle::Open,
        draft: false,
        mine: true,
        author: "enekos".into(),
        head: "feat-branch".into(),
        base: "master".into(),
        updated: "2026-09-16".into(),
        merged_at: String::new(),
        merged_by: String::new(),
        review: Review::None,
        review_by: Vec::new(),
        checks: Checks::Green,
        failing: Vec::new(),
        review_requested_of_me: false,
    }
}

pub fn snapshot(prs: &[Pr]) -> Snapshot {
    Snapshot {
        taken_at: "2026-09-16T10:00:00Z".into(),
        login: "enekos".into(),
        prs: prs.iter().map(|p| (p.id.clone(), p.clone())).collect(),
        ..Snapshot::default()
    }
}

pub fn issue(id: &str, state: &str, state_type: &str) -> Issue {
    Issue {
        id: id.to_string(),
        title: format!("{id} title"),
        url: format!("https://linear.app/acme/issue/{id}"),
        state: state.to_string(),
        state_type: state_type.to_string(),
        updated: "2026-09-16".into(),
        ..Issue::default()
    }
}

pub fn with_issues(mut snap: Snapshot, issues: &[Issue]) -> Snapshot {
    snap.issues = issues.iter().map(|i| (i.id.clone(), i.clone())).collect();
    snap.issues_at = snap.taken_at.clone();
    snap
}

pub fn issue_node(id: &str, title: &str, state: &str, state_type: &str) -> Value {
    json!({
        "identifier": id,
        "title": title,
        "priority": 0,
        "updatedAt": "2026-09-16T09:13:21.000Z",
        "url": format!("https://linear.app/acme/issue/{id}"),
        "state": {"name": state, "type": state_type},
        "project": Value::Null,
        "cycle": Value::Null,
        "assignee": {"displayName": "me", "name": "Me"},
        "parent": Value::Null,
        "attachments": {"nodes": []},
        "relations": {"nodes": []},
        "inverseRelations": {"nodes": []},
        "comments": {"nodes": []}
    })
}

pub fn node(nwo: &str, number: u64, title: &str, state: &str) -> Value {
    json!({
        "number": number,
        "title": title,
        "url": format!("https://github.com/{nwo}/pull/{number}"),
        "state": state,
        "isDraft": false,
        "updatedAt": "2026-09-16T09:13:21Z",
        "mergedAt": if state == "MERGED" { json!("2026-09-16T09:00:00Z") } else { Value::Null },
        "reviewDecision": Value::Null,
        "headRefName": "feat-branch",
        "baseRefName": "master",
        "repository": {"nameWithOwner": nwo},
        "author": {"login": "enekos"},
        "mergedBy": if state == "MERGED" { json!({"login": "mergequeue"}) } else { Value::Null },
        "latestReviews": {"nodes": []},
        "commits": {"nodes": [{"commit": {"statusCheckRollup": Value::Null}}]}
    })
}

pub fn payload(open: Vec<Value>, merged: Vec<Value>, review: Vec<Value>) -> Value {
    json!({"data": {
        "open": {"nodes": open},
        "merged": {"nodes": merged},
        "review": {"nodes": review}
    }})
}

pub fn with_checks(mut node: Value, rollup: Value) -> Value {
    node["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = rollup;
    node
}

pub fn rollup(state: &str, contexts: Vec<Value>) -> Value {
    json!({"state": state, "contexts": {"nodes": contexts}})
}

pub fn check_run(name: &str, conclusion: &str) -> Value {
    json!({"__typename": "CheckRun", "name": name, "conclusion": conclusion})
}

pub fn status_context(context: &str, state: &str) -> Value {
    json!({"__typename": "StatusContext", "context": context, "state": state})
}
