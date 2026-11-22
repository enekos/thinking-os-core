//! GitHub Issues, independently fetched/seeded from PRs and Linear.
use crate::kg::{self, Edge, Node, Ontology, Paths};
use crate::watch::events::Event;
use crate::watch::model::{Issue, Snapshot};
use crate::watch::{gh, linear};
use serde_json::Value;
use std::collections::BTreeMap;

pub const QUERY: &str = r#"
query($q:String!, $after:String) {
  hits: search(query:$q, type:ISSUE, first:40, after:$after) {
    pageInfo { hasNextPage endCursor }
    nodes { ... on Issue {
      number title url state stateReason createdAt updatedAt
      repository { nameWithOwner }
      labels(first:100) { pageInfo { hasNextPage } nodes { name } }
      comments(last:1) { nodes { createdAt author { login __typename } } }
      closedByPullRequestsReferences(first:100, includeClosedPrs:true) {
        pageInfo { hasNextPage } nodes { number url repository { nameWithOwner } }
      }
      blockedBy(first:100) { pageInfo { hasNextPage } nodes { number state repository { nameWithOwner } } }
      parent { number title repository { nameWithOwner } }
    } }
  }
}
"#;

fn text(v: &Value, pointer: &str) -> String {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn id(node: &Value) -> Option<String> {
    let repo = text(node, "/repository/nameWithOwner");
    let number = node.get("number")?.as_u64()?;
    (!repo.is_empty() && number > 0).then(|| format!("gh:{repo}/issues/{number}"))
}

/// Assigned open issues and recently updated closed issues. Separate searches
/// avoid GitHub's OR precedence and ensure closures are still observed.
pub fn fetch(login: &str, since: &str) -> Result<Vec<Value>, String> {
    let mut nodes = Vec::new();
    for state in [
        "is:open".to_string(),
        format!("is:closed updated:>={since}"),
    ] {
        let query = format!("is:issue assignee:{login} {state}");
        nodes.extend(gh::collect_pages(|after| {
            gh::fetch_query_page(QUERY, &query, after)
        })?);
    }
    validate(&nodes)?;
    Ok(nodes)
}

pub fn validate(nodes: &[Value]) -> Result<(), String> {
    for node in nodes {
        if id(node).is_none() || !matches!(text(node, "/state").as_str(), "OPEN" | "CLOSED") {
            return Err("GitHub Issues response contained an invalid issue".into());
        }
        for connection in ["labels", "closedByPullRequestsReferences", "blockedBy"] {
            if node.pointer(&format!("/{connection}/pageInfo/hasNextPage"))
                == Some(&Value::Bool(true))
            {
                return Err(format!(
                    "GitHub Issue {} has truncated {connection}",
                    text(node, "/url")
                ));
            }
        }
    }
    Ok(())
}

pub fn parse(nodes: &[Value], login: &str) -> BTreeMap<String, Issue> {
    nodes
        .iter()
        .filter_map(|node| {
            let id = id(node)?;
            let closed = text(node, "/state") == "CLOSED";
            let reason = text(node, "/stateReason");
            let state_type = if !closed {
                "unstarted"
            } else if reason == "NOT_PLANNED" {
                "canceled"
            } else {
                "completed"
            };
            let priority = node
                .pointer("/labels/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(
                    |label| match text(label, "/name").to_ascii_lowercase().as_str() {
                        "urgent" | "priority: urgent" | "p0" => 1,
                        "high" | "priority: high" | "p1" => 2,
                        "medium" | "priority: medium" | "p2" => 3,
                        "low" | "priority: low" | "p3" => 4,
                        _ => 0,
                    },
                )
                .filter(|p| *p != 0)
                .min()
                .unwrap_or(0);
            let prs = node
                .pointer("/closedByPullRequestsReferences/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|pr| linear::pr_id_of(&text(pr, "/url")))
                .collect();
            let blocked_by = node
                .pointer("/blockedBy/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|n| text(n, "/state") == "OPEN")
                .filter_map(id_of)
                .collect();
            let comment = node.pointer("/comments/nodes/0").unwrap_or(&Value::Null);
            let author = text(comment, "/author/login");
            let issue = Issue {
                id: id.clone(),
                title: text(node, "/title"),
                url: text(node, "/url"),
                state: if closed {
                    if reason == "NOT_PLANNED" {
                        "Closed (not planned)"
                    } else {
                        "Closed"
                    }
                } else {
                    "Open"
                }
                .into(),
                state_type: state_type.into(),
                priority,
                project: format!("gh:{}", text(node, "/repository/nameWithOwner")),
                project_name: text(node, "/repository/nameWithOwner"),
                updated: text(node, "/updatedAt").chars().take(10).collect(),
                created: text(node, "/createdAt"),
                parent: node.get("parent").and_then(id_of).unwrap_or_default(),
                blocked_by,
                prs,
                comment_at: text(comment, "/createdAt"),
                comment_by: if author.eq_ignore_ascii_case(login)
                    || author.ends_with("[bot]")
                    || text(comment, "/author/__typename") == "Bot"
                {
                    String::new()
                } else {
                    author
                },
            };
            Some((id, issue))
        })
        .collect()
}

fn id_of(node: &Value) -> Option<String> {
    id(node)
}

/// Uses the source-neutral transition logic with only this source's snapshot.
pub fn diff(prev: &Snapshot, curr: &Snapshot) -> Vec<Event> {
    let before = Snapshot {
        issues: prev.github_issues.clone(),
        ..Snapshot::default()
    };
    let after = Snapshot {
        issues: curr.github_issues.clone(),
        ..Snapshot::default()
    };
    linear::diff(&before, &after, &curr.taken_at)
}

pub fn graph(
    issues: &BTreeMap<String, Issue>,
    login: &str,
    ont: &Ontology,
) -> (Vec<Node>, Vec<Edge>) {
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    let who = ont.canon(&login.to_ascii_lowercase());
    for issue in issues.values() {
        let mut node = Node::new(&issue.id, "issue");
        node.title = kg::clean(&issue.title);
        node.status = issue.state_type.clone();
        node.state = issue.state.clone();
        node.summary = linear::priority_label(issue.priority).into();
        node.updated = issue.updated.clone();
        node.url = issue.url.clone();
        nodes.insert(node.id.clone(), node);
        let mut repo = Node::new(&issue.project, "repo");
        repo.title = issue.project_name.clone();
        nodes.insert(repo.id.clone(), repo);
        let mut person = Node::new(&who, "person");
        person.title = login.into();
        nodes.insert(who.clone(), person);
        for (rel, dst) in [("OPENED_IN", &issue.project), ("ASSIGNED_TO", &who)] {
            edges.push(Edge::new(
                &issue.id,
                rel,
                dst,
                &issue.updated,
                "github",
                1.0,
                &issue.url,
            ));
        }
        if !issue.parent.is_empty() {
            edges.push(Edge::new(
                &issue.id,
                "SUBTASK_OF",
                &issue.parent,
                &issue.updated,
                "github",
                1.0,
                &issue.url,
            ));
        }
        for blocker in &issue.blocked_by {
            edges.push(Edge::new(
                blocker,
                "BLOCKS",
                &issue.id,
                &issue.updated,
                "github",
                1.0,
                &issue.url,
            ));
        }
        for pr in &issue.prs {
            edges.push(Edge::new(
                pr,
                "IMPLEMENTS",
                &issue.id,
                &issue.updated,
                "github",
                1.0,
                &issue.url,
            ));
        }
    }
    (nodes.into_values().collect(), edges)
}

/// Keep the previous complete graph if a remote read fails.
pub fn extract(paths: &Paths, ont: &Ontology) -> crate::sare::extract::Pull {
    let result = paths
        .me()
        .ok_or_else(|| "set me.github in os.yaml".to_string())
        .and_then(|login| fetch(&login, &crate::clock::days_ago(14)).map(|nodes| (login, nodes)));
    match result {
        Ok((login, raw)) => {
            let issues = parse(&raw, &login);
            let (nodes, edges) = graph(&issues, &login, ont);
            let problems = kg::dump_raw(paths, "github-issues", &nodes, &edges)
                .err()
                .map(|e| format!("github-issues: {e}"))
                .into_iter()
                .collect();
            crate::sare::extract::Pull {
                nodes,
                edges,
                problems,
            }
        }
        Err(error) => {
            let previous = kg::load_raw(paths, "github-issues");
            crate::sare::extract::Pull {
                nodes: previous.nodes,
                edges: previous.edges,
                problems: vec![format!(
                    "github-issues: {error}; retained previous snapshot"
                )],
            }
        }
    }
}
