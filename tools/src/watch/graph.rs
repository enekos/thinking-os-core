use crate::clock;
use crate::watch::model::{Lifecycle, Pr, Review};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default)]
pub struct Index {
    pub project_of_repo: BTreeMap<String, Vec<String>>,
    pub project_title: BTreeMap<String, String>,
}

pub fn read_tsv(path: &Path) -> Vec<BTreeMap<String, String>> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut lines = text.lines();
    let cols: Vec<&str> = match lines.next() {
        Some(h) => h.split('\t').collect(),
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let mut row = BTreeMap::new();
        for (i, col) in cols.iter().enumerate() {
            row.insert((*col).to_string(), parts.get(i).copied().unwrap_or("").to_string());
        }
        out.push(row);
    }
    out
}

pub fn index(root: &Path) -> Index {
    let mut idx = Index::default();
    for row in read_tsv(&root.join("graph/nodes.tsv")) {
        let (id, kind) = (row.get("id").cloned().unwrap_or_default(), row.get("type").cloned().unwrap_or_default());
        let title = row.get("title").cloned().unwrap_or_default();
        match kind.as_str() {
            "project" => {
                idx.project_title.insert(id, title);
            }
            _ => {}
        }
    }
    for row in read_tsv(&root.join("graph/edges.tsv")) {
        if row.get("rel").map(String::as_str) == Some("LIVES_IN") {
            let src = row.get("src").cloned().unwrap_or_default();
            let dst = row.get("dst").cloned().unwrap_or_default();
            if src.is_empty() || dst.is_empty() {
                continue;
            }
            idx.project_of_repo.entry(dst).or_default().push(src);
        }
    }
    for projects in idx.project_of_repo.values_mut() {
        projects.sort();
        projects.dedup();
    }
    idx
}

impl Index {
    pub fn projects_for(&self, repo_id: &str) -> &[String] {
        self.project_of_repo
            .get(repo_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

fn node(id: &str, kind: &str) -> Map<String, Value> {
    let mut m = Map::new();
    for key in ["id", "type", "title", "summary", "status", "state", "updated", "path", "url"] {
        m.insert(key.to_string(), json!(""));
    }
    m.insert("id".into(), json!(id));
    m.insert("type".into(), json!(kind));
    m
}

fn edge(src: &str, rel: &str, dst: &str, since: &str, prov: &str, conf: f64, evidence: &str) -> Value {
    json!({
        "src": src, "rel": rel, "dst": dst, "since": since,
        "prov": prov, "conf": conf, "evidence": evidence
    })
}

pub fn raw_payload(prs: &[Pr], login: &str) -> Value {
    let mut nodes: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut edges: Vec<Value> = Vec::new();

    for pr in prs {
        let mut n = node(&pr.id, "pr");
        n.insert("title".into(), json!(pr.title));
        n.insert("status".into(), json!(pr.lifecycle.label()));
        n.insert("state".into(), json!(pr.state_label()));
        n.insert("updated".into(), json!(pr.updated));
        n.insert("url".into(), json!(pr.url));
        n.insert("summary".into(), json!(pr.failing.join(", ")));
        nodes.insert(pr.id.clone(), n);

        let mut repo = node(&pr.repo, "repo");
        repo.insert("title".into(), json!(pr.nwo()));
        nodes.entry(pr.repo.clone()).or_insert(repo);

        edges.push(edge(&pr.id, "OPENED_IN", &pr.repo, &pr.updated, "github", 1.0, "zaindu"));

        if !pr.author.is_empty() {
            let who = pr.author.to_ascii_lowercase();
            let mut person = node(&who, "person");
            person.insert("title".into(), json!(pr.author));
            nodes.entry(who.clone()).or_insert(person);
            edges.push(edge(&pr.id, "AUTHORED_BY", &who, &pr.updated, "github", 1.0, "zaindu"));
        }

        let mut reviewers: Vec<String> = pr.review_by.iter().map(|r| r.to_ascii_lowercase()).collect();
        if pr.review_requested_of_me {
            reviewers.push(login.to_ascii_lowercase());
        }
        reviewers.sort();
        reviewers.dedup();
        for who in reviewers {
            if who.is_empty() {
                continue;
            }
            let mut person = node(&who, "person");
            person.insert("title".into(), json!(who));
            nodes.entry(who.clone()).or_insert(person);
            edges.push(edge(&who, "REVIEWS", &pr.id, &pr.updated, "github", 1.0, "zaindu"));
        }
    }

    json!({
        "extracted_at": clock::stamp(),
        "nodes": nodes.into_values().map(Value::Object).collect::<Vec<_>>(),
        "edges": edges,
    })
}

pub fn write_raw(root: &Path, prs: &[Pr], login: &str) -> std::io::Result<usize> {
    let dir = root.join("graph/raw");
    fs::create_dir_all(&dir)?;
    let payload = raw_payload(prs, login);
    let count = payload["nodes"].as_array().map(Vec::len).unwrap_or(0);
    crate::kg::write_atomic(&dir.join("watch.json"), serde_json::to_string_pretty(&payload)?.as_bytes())?;
    Ok(count)
}

pub fn headline(pr: &Pr) -> String {
    match (pr.lifecycle, pr.review) {
        (Lifecycle::Merged, _) => "merged".into(),
        (Lifecycle::Closed, _) => "closed".into(),
        (_, Review::ChangesRequested) => "changes requested".into(),
        (_, Review::Approved) => "approved".into(),
        _ => pr.state_label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::fixtures::pr;
    use crate::watch::model::{Checks, Review};
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zaindu-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("graph")).unwrap();
        dir
    }

    #[test]
    fn the_index_reads_a_repo_back_to_its_project_page() {
        let root = scratch("index");
        fs::write(
            root.join("graph/nodes.tsv"),
            "id\ttype\ttitle\nmeta-data-service\tproject\tJOIN data service\n",
        )
        .unwrap();
        fs::write(
            root.join("graph/edges.tsv"),
            "src\trel\tdst\nmeta-data-service\tLIVES_IN\tgh:join-com/data\nmeta-join\tLIVES_IN\tgh:join-com/data\nbai\tMENTIONS\tgh:join-com/data\n",
        )
        .unwrap();

        let idx = index(&root);
        assert_eq!(idx.projects_for("gh:join-com/data"), ["meta-data-service", "meta-join"]);
        assert!(idx.projects_for("gh:join-com/frontend").is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_absent_graph_yields_an_empty_index_not_a_crash() {
        let idx = index(Path::new("/nonexistent"));
        assert!(idx.projects_for("gh:join-com/data").is_empty());
    }

    #[test]
    fn a_merged_pr_reaches_the_graph_with_its_title_and_edges() {
        let mut merged = pr("join-com/data", 7276, "fix: THU-3461 anchor");
        merged.lifecycle = Lifecycle::Merged;
        merged.review = Review::Approved;
        merged.review_by = vec!["Mohammed-Ali-1".into()];

        let payload = raw_payload(&[merged], "enekos");
        let nodes = payload["nodes"].as_array().unwrap();
        let edges = payload["edges"].as_array().unwrap();

        let pr_node = nodes.iter().find(|n| n["id"] == "gh:join-com/data#7276").unwrap();
        assert_eq!(pr_node["type"], "pr");
        assert_eq!(pr_node["status"], "merged");
        assert_eq!(pr_node["state"], "approved");
        assert_eq!(pr_node["title"], "fix: THU-3461 anchor");
        assert!(nodes.iter().any(|n| n["id"] == "gh:join-com/data" && n["type"] == "repo"));
        assert!(nodes.iter().any(|n| n["id"] == "enekos" && n["type"] == "person"));
        assert!(nodes.iter().any(|n| n["id"] == "mohammed-ali-1"));

        let has = |src: &str, rel: &str, dst: &str| {
            edges.iter().any(|e| e["src"] == src && e["rel"] == rel && e["dst"] == dst)
        };
        assert!(has("gh:join-com/data#7276", "OPENED_IN", "gh:join-com/data"));
        assert!(has("gh:join-com/data#7276", "AUTHORED_BY", "enekos"));
        assert!(has("mohammed-ali-1", "REVIEWS", "gh:join-com/data#7276"));
    }

    #[test]
    fn every_node_carries_the_full_column_set_fusion_expects() {
        let payload = raw_payload(&[pr("join-com/data", 1, "x")], "enekos");
        for node in payload["nodes"].as_array().unwrap() {
            for col in ["id", "type", "title", "summary", "status", "state", "updated", "path", "url"] {
                assert!(node.get(col).is_some(), "node {node} has no {col}");
            }
        }
    }

    #[test]
    fn a_red_pr_carries_its_failing_checks_as_the_summary() {
        let mut red = pr("join-com/data", 7280, "feat: prompts");
        red.checks = Checks::Red;
        red.failing = vec!["ci/circleci: test".into()];
        let payload = raw_payload(&[red], "enekos");
        let node = payload["nodes"].as_array().unwrap().iter()
            .find(|n| n["id"] == "gh:join-com/data#7280").unwrap();
        assert_eq!(node["summary"], "ci/circleci: test");
        assert_eq!(node["state"], "red");
    }

    #[test]
    fn writing_raw_lands_where_fusion_looks_for_it() {
        let root = scratch("raw");
        let count = write_raw(&root, &[pr("join-com/data", 7276, "fix: anchor")], "enekos").unwrap();
        assert!(count >= 3);
        let text = fs::read_to_string(root.join("graph/raw/watch.json")).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert!(back["extracted_at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(back["nodes"].as_array().unwrap().len(), count);
        let _ = fs::remove_dir_all(&root);
    }
}
