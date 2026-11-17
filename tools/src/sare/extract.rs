use crate::kg::{self, Edge, Node, Ontology, Paths};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

pub struct Pull {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub problems: Vec<String>,
}

fn re_of(pattern: &'static str) -> Regex {
    Regex::new(pattern).unwrap()
}

fn edge_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| re_of(r"^([A-Z_]+)\s+(.+)$"))
}

// ---------------------------------------------------------------- pages

pub fn pages(paths: &Paths, ont: &Ontology) -> Pull {
    let mut nodes = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut problems = Vec::new();

    for (nid, ntype, path) in kg::wiki_pages(&paths.root) {
        let fm = kg::frontmatter(&path);
        let rel_path = paths.rel(&path);
        let id = ont.canon(&nid);
        let mut node = Node::new(&id, &ntype);
        let title = fm.text("title");
        node.title = kg::clean(if title.is_empty() { &nid } else { &title });
        node.summary = kg::clean(&fm.text("summary"));
        node.status = kg::clean(&fm.text("status"));
        node.updated = kg::clean(&fm.text("updated"));
        node.path = rel_path.clone();
        nodes.push(node);

        let updated = kg::clean(&fm.text("updated"));
        for raw in fm.list("edges") {
            let raw = raw.trim().to_string();
            let Some(m) = edge_line().captures(&raw) else {
                problems.push(format!("{rel_path}: unparseable edge '{raw}'"));
                continue;
            };
            let rel = m.get(1).unwrap().as_str();
            let target = kg::slug(m.get(2).unwrap().as_str());
            if !ont.has_relation(rel) {
                problems.push(format!("{rel_path}: unknown relation {rel}"));
                continue;
            }
            edges.push(Edge::new(&id, rel, &ont.canon(&target), &updated, "page", 1.0, &rel_path));
        }

        let parent = fm.text("parent");
        if !parent.is_empty() {
            edges.push(Edge::new(&id, "PART_OF", &ont.canon(&kg::slug(&parent)), "", "page", 1.0, &rel_path));
        }
        for src in fm.list("sources") {
            edges.push(Edge::new(&id, "EVIDENCED_BY", &ont.canon(&kg::slug(&src)), "", "page", 1.0, &rel_path));
        }

        let typed: HashSet<String> = edges
            .iter()
            .filter(|e| e.src == id)
            .map(|e| e.dst.clone())
            .collect();
        for link in fm.list("links") {
            let target = ont.canon(&kg::slug(&link));
            if !target.is_empty() && target != id && !typed.contains(&target) {
                edges.push(Edge::new(&id, "MENTIONS", &target, "", "legacy", 0.5, &rel_path));
            }
        }
    }
    let _ = kg::dump_raw(paths, "pages", &nodes, &edges);
    Pull { nodes, edges, problems }
}

// ---------------------------------------------------------------- git

pub fn git(paths: &Paths, ont: &Ontology) -> Pull {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut problems = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let paren = re_of(r"\s*\(.*\)$");
    let remote_re = re_of(r"[:/]([\w.-]+)/([\w.-]+?)(?:\.git)?$");
    let home = std::env::var("HOME").unwrap_or_default();

    for (nid, ntype, path) in kg::wiki_pages(&paths.root) {
        if ntype != "project" {
            continue;
        }
        let fm = kg::frontmatter(&path);
        let raw_paths = fm.text("path");
        if raw_paths.is_empty() {
            continue;
        }
        for piece in raw_paths.split('\u{b7}') {
            let piece = piece.trim().trim_matches('`');
            let piece = paren.replace(piece, "").to_string();
            if piece.is_empty() {
                continue;
            }
            let abs = if let Some(rest) = piece.strip_prefix("~") {
                format!("{home}{rest}")
            } else {
                piece.clone()
            };
            if !std::path::Path::new(&abs).is_dir() {
                problems.push(format!("{nid}: path does not exist: {piece}"));
                continue;
            }
            let remote = kg::run(&["git", "-C", &abs, "remote", "get-url", "origin"], None, 120);
            let last = kg::run(&["git", "-C", &abs, "log", "-1", "--format=%cs"], None, 120);
            let rid = match remote.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
                Some(r) => match remote_re.captures(r) {
                    Some(m) => format!("gh:{}/{}", &m[1], &m[2]),
                    None => format!("local:{abs}"),
                },
                None => format!("local:{abs}"),
            };
            let since = kg::clean(last.as_deref().unwrap_or(""));
            if seen.insert(rid.clone()) {
                let mut n = Node::new(&rid, "repo");
                n.title = rid.splitn(2, ':').nth(1).unwrap_or("").to_string();
                n.updated = since.clone();
                n.path = abs.clone();
                nodes.push(n);
            }
            edges.push(Edge::new(&ont.canon(&nid), "LIVES_IN", &rid, &since, "git", 0.9, &abs));
        }
    }
    let _ = kg::dump_raw(paths, "git", &nodes, &edges);
    Pull { nodes, edges, problems }
}

// ---------------------------------------------------------------- helpers

fn head(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn first10(s: &str) -> String {
    s.chars().take(10).collect()
}

// ---------------------------------------------------------------- github

const PR_FIELDS: &str = "number,title,state,isDraft,url,updatedAt,headRefName,baseRefName,author,reviewRequests,statusCheckRollup";

fn repos_of_interest(known: &[String]) -> Vec<String> {
    let mut repos: HashSet<String> = known.iter().filter(|r| r.starts_with("gh:")).cloned().collect();
    if let Some(out) = kg::run(
        &["gh", "search", "prs", "--author=@me", "--state=open", "--limit=100", "--json", "repository"],
        None,
        90,
    ) {
        if let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(&out) {
            for row in rows {
                let name = row.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("");
                if !name.is_empty() {
                    repos.insert(format!("gh:{name}"));
                }
            }
        }
    }
    let mut out: Vec<String> = repos.into_iter().collect();
    out.sort();
    out
}

pub fn github(paths: &Paths, ont: &Ontology, known_repos: &[String]) -> Pull {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let non_alnum = re_of(r"[^a-z0-9]+");

    for rid in repos_of_interest(known_repos) {
        let nwo = rid[3..].to_string();
        let Some(out) = kg::run(
            &["gh", "pr", "list", "--repo", &nwo, "--state", "open", "--limit", "60", "--json", PR_FIELDS],
            None,
            120,
        ) else {
            problems.push(format!("github: cannot list PRs for {nwo}"));
            continue;
        };
        let prs: Vec<Value> = match serde_json::from_str::<Value>(&out) {
            Ok(Value::Array(v)) => v,
            _ => {
                problems.push(format!("github: unparseable PR list for {nwo}"));
                continue;
            }
        };

        let mut heads: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        for p in &prs {
            heads.insert(head(p, "headRefName"), p.get("number").and_then(Value::as_i64).unwrap_or(0));
        }
        if seen.insert(rid.clone()) {
            let mut n = Node::new(&rid, "repo");
            n.title = nwo.clone();
            nodes.push(n);
        }

        for p in &prs {
            let number = p.get("number").and_then(Value::as_i64).unwrap_or(0);
            let pid = format!("{rid}#{number}");
            let failing: Vec<String> = p
                .get("statusCheckRollup")
                .and_then(Value::as_array)
                .map(|checks| {
                    checks
                        .iter()
                        .filter(|c| {
                            let verdict = c
                                .get("conclusion")
                                .and_then(Value::as_str)
                                .or_else(|| c.get("state").and_then(Value::as_str))
                                .unwrap_or("");
                            matches!(verdict, "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "FAILING")
                        })
                        .map(|c| {
                            c.get("name")
                                .and_then(Value::as_str)
                                .or_else(|| c.get("context").and_then(Value::as_str))
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut status = if p.get("isDraft").and_then(Value::as_bool).unwrap_or(false) {
                "draft".to_string()
            } else {
                "open".to_string()
            };
            if !failing.is_empty() {
                status.push_str("+red");
            }
            let since = first10(&head(p, "updatedAt"));
            if seen.insert(pid.clone()) {
                let mut n = Node::new(&pid, "pr");
                n.title = kg::clean(&head(p, "title"));
                n.status = status;
                n.updated = since.clone();
                n.url = head(p, "url");
                n.summary = kg::clean(&failing.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
                nodes.push(n);
            }
            edges.push(Edge::new(&pid, "OPENED_IN", &rid, &since, "github", 1.0, &pid));

            // `gh pr list` names a GitHub App `app/<name>`; search and the
            // watcher say `<name>` — one person node, not two.
            let author = p.pointer("/author/login").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let author = author.strip_prefix("app/").unwrap_or(&author).to_string();
            if !author.is_empty() {
                let aid = ont.canon(&non_alnum.replace_all(&author, "-"));
                if seen.insert(aid.clone()) {
                    let mut n = Node::new(&aid, "person");
                    n.title = author.clone();
                    nodes.push(n);
                }
                edges.push(Edge::new(&pid, "AUTHORED_BY", &aid, &since, "github", 1.0, &pid));
            }

            if let Some(reqs) = p.get("reviewRequests").and_then(Value::as_array) {
                for rr in reqs {
                    let login = rr
                        .get("login")
                        .and_then(Value::as_str)
                        .or_else(|| rr.get("name").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_lowercase();
                    if login.is_empty() {
                        continue;
                    }
                    let rpid = ont.canon(&non_alnum.replace_all(&login, "-"));
                    if seen.insert(rpid.clone()) {
                        let mut n = Node::new(&rpid, "person");
                        n.title = login.clone();
                        nodes.push(n);
                    }
                    edges.push(Edge::new(&rpid, "REVIEWS", &pid, &since, "github", 1.0, &pid));
                }
            }

            let base = head(p, "baseRefName");
            if let Some(other) = heads.get(&base).filter(|n| **n != number) {
                edges.push(Edge::new(&pid, "DEPENDS_ON", &format!("{rid}#{other}"), &since, "github", 1.0, &format!("base={base}")));
            }
        }
    }
    let _ = kg::dump_raw(paths, "github", &nodes, &edges);
    Pull { nodes, edges, problems }
}
