use crate::kg::{self, Edge, Node, Ontology, Paths};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
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
        if ntype == "test" {
            let ran_at = kg::clean(&fm.text("ran_at"));
            if !ran_at.is_empty() {
                node.updated = ran_at;
            }
            let command = kg::clean(&fm.text("command"));
            let exit_code = kg::clean(&fm.text("exit_code"));
            node.state = if exit_code.is_empty() {
                command
            } else {
                format!("exit {exit_code} · {command}")
            };
        } else if ntype == "attempt" {
            node.state = kg::clean(&fm.text("rejection"));
        } else if ntype == "task" {
            if let Some(nominated) = nomination(&fm) {
                node.state = nominated.why.clone();
                if nominated.concepts.is_empty() || nominated.why.is_empty() {
                    problems.push(format!("{rel_path}: exemplar needs at least one concept and a why"));
                }
            }
        }
        node.path = rel_path.clone();
        let updated = node.updated.clone();
        nodes.push(node);

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
            if ntype == "test" && rel == "VERIFIES" && !passing_test(&fm) {
                problems.push(format!("{rel_path}: a failed or incomplete test cannot declare VERIFIES"));
                continue;
            }
            if rel == "EXEMPLIFIES" {
                problems.push(format!("{rel_path}: EXEMPLIFIES is earned through an exemplar: block, never declared"));
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

        let (field, relation) = match ntype.as_str() {
            "task" => ("project", "PART_OF"),
            "attempt" => ("task", "ATTEMPTS"),
            "test" => ("task", "TESTS"),
            _ => ("", ""),
        };
        if !field.is_empty() {
            let target = kg::slug(&fm.text(field));
            if target.is_empty() {
                problems.push(format!("{rel_path}: {field} is required"));
            } else {
                let target = ont.canon(&target);
                edges.push(Edge::new(&id, relation, &target, &updated, "page", 1.0, &rel_path));
                if ntype == "test" && passing_test(&fm) {
                    edges.push(Edge::new(&id, "VERIFIES", &target, &updated, "page", 1.0, &rel_path));
                }
            }
        }
        if ntype == "task" && !fm.text("issue").is_empty() {
            edges.push(Edge::new(&id, "TRACKS", &ont.canon(&fm.text("issue")), &updated, "page", 1.0, &rel_path));
        }
        if ntype == "task" && !fm.list("commits").is_empty() && fm.text("project").is_empty() {
            problems.push(format!("{rel_path}: commits require a project so their SHAs can be resolved"));
        }
        if ntype == "test" {
            for required in ["commit", "command", "ran_at", "exit_code"] {
                if fm.text(required).is_empty() {
                    problems.push(format!("{rel_path}: {required} is required for a test result"));
                }
            }
            if fm.text("status") == "passed" && !passing_test(&fm) {
                problems.push(format!("{rel_path}: a passed test must have exit_code: 0"));
            }
        }
        if ntype == "attempt" && fm.text("rejection").is_empty() {
            problems.push(format!("{rel_path}: rejection is required so the failed approach remains useful"));
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

const DEFAULT_HISTORY: usize = 20;
const MAX_HISTORY: usize = 200;

struct CommitRef {
    revision: String,
    wiki_id: String,
    task: String,
    evidence: String,
    since: String,
    test: bool,
    passing: bool,
}

/// A task's `exemplar:` block: the concepts its code demonstrates and why a
/// reader should study it. A nomination, not a claim — the gate decides.
pub struct Nomination {
    pub concepts: Vec<String>,
    pub why: String,
}

pub fn nomination(fm: &kg::Front) -> Option<Nomination> {
    let block = fm.get("exemplar")?;
    let concepts = match block.get("concepts") {
        Some(serde_yaml::Value::Sequence(items)) => items
            .iter()
            .map(|item| kg::slug(&kg::page::scalar(item)))
            .filter(|concept| !concept.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let why = block.get("why").map(kg::page::scalar).map(|why| kg::clean(&why)).unwrap_or_default();
    Some(Nomination { concepts, why })
}

/// The branch a commit must be on to count as landed: what `origin/HEAD`
/// points at, else the first conventional default that exists.
fn default_branch(abs: &str) -> Option<String> {
    let symbolic = kg::run(&["git", "-C", abs, "symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"], None, 30);
    if let Some(name) = symbolic.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        return Some(name.to_string());
    }
    ["origin/main", "origin/master", "main", "master"]
        .into_iter()
        .find(|candidate| {
            let target = format!("{candidate}^{{commit}}");
            kg::run(&["git", "-C", abs, "rev-parse", "--verify", "-q", &target], None, 30).is_some()
        })
        .map(str::to_string)
}

fn contains_commit(abs: &str, ancestor: &str, descendant: &str) -> bool {
    kg::run(&["git", "-C", abs, "merge-base", "--is-ancestor", ancestor, descendant], None, 60).is_some()
}

fn sha_of(cid: &str) -> &str {
    cid.rsplit('@').next().unwrap_or("")
}

/// Where a nominated task stands against the exemplar gate, given what the
/// git extractor resolved for it in one repository.
struct Candidate<'a> {
    abs: &'a str,
    status: String,
    nominated: Nomination,
    declared: usize,
    commits: Vec<String>,
    tested_at: Vec<String>,
    touched: usize,
}

fn judge(candidate: &Candidate, default_ref: Option<&str>, max_paths: usize) -> Result<(), String> {
    if candidate.nominated.concepts.is_empty() || candidate.nominated.why.is_empty() {
        return Err("exemplar needs at least one concept and a why".into());
    }
    if candidate.status != "done" {
        return Err(format!("status is '{}', not done", candidate.status));
    }
    if candidate.declared == 0 {
        return Err("names no commits".into());
    }
    if candidate.commits.len() < candidate.declared {
        return Err("a named commit does not resolve".into());
    }
    let Some(default_ref) = default_ref else {
        return Err("no default branch to check the commits against".into());
    };
    if let Some(stray) = candidate.commits.iter().find(|cid| !contains_commit(candidate.abs, sha_of(cid), default_ref)) {
        let short: String = sha_of(stray).chars().take(12).collect();
        return Err(format!("commit {short} is not on {default_ref}"));
    }
    let verified = candidate.tested_at.iter().any(|tested| {
        candidate.commits.iter().all(|cid| contains_commit(candidate.abs, sha_of(cid), sha_of(tested)))
    });
    if !verified {
        return Err("no passing test ran at a commit containing all of its commits".into());
    }
    if candidate.touched > max_paths {
        return Err(format!("touches {} paths (gate: {max_paths}) — too large to read as an example", candidate.touched));
    }
    Ok(())
}

pub const EXEMPLAR_COLS: [&str; 3] = ["id", "project", "reason"];

fn passing_test(fm: &kg::Front) -> bool {
    fm.text("status") == "passed"
        && fm.text("exit_code") == "0"
        && ["task", "commit", "command", "ran_at"]
            .iter()
            .all(|field| !fm.text(field).is_empty())
}

fn valid_revision(revision: &str) -> bool {
    (7..=64).contains(&revision.len()) && revision.chars().all(|c| c.is_ascii_hexdigit())
}

fn add_commit(
    abs: &str,
    rid: &str,
    revision: &str,
    evidence: &str,
    memory_nodes: &mut BTreeMap<String, Node>,
    edges: &mut Vec<Edge>,
    problems: &mut Vec<String>,
) -> Option<String> {
    let revision = revision.trim();
    if !valid_revision(revision) {
        problems.push(format!("{evidence}: commit must be an unambiguous SHA of 7-64 hex characters, got '{revision}'"));
        return None;
    }
    let suffix = format!("{revision}^{{commit}}");
    let Some(resolved) = kg::run(&["git", "-C", abs, "rev-parse", "--verify", &suffix], None, 120) else {
        problems.push(format!("{evidence}: cannot resolve commit {revision} in {abs}"));
        return None;
    };
    let full = resolved.lines().next().unwrap_or("").trim();
    if !valid_revision(full) {
        problems.push(format!("{evidence}: git returned an invalid commit id for {revision}"));
        return None;
    }
    let cid = format!("commit:{rid}@{full}");
    if memory_nodes.contains_key(&cid) {
        return Some(cid);
    }
    let Some(details) = kg::run(&["git", "-C", abs, "show", "-s", "--format=%H%n%ct%n%s", full], None, 120) else {
        problems.push(format!("{evidence}: cannot read commit {full} in {abs}"));
        return None;
    };
    let mut lines = details.lines();
    let sha = lines.next().unwrap_or(full).trim();
    let Ok(epoch) = lines.next().unwrap_or("").trim().parse::<i64>() else {
        problems.push(format!("{evidence}: commit {full} has no valid committer timestamp"));
        return None;
    };
    let committed_at = crate::clock::stamp_at(epoch);
    let subject = lines.next().unwrap_or("").trim();
    let cid = format!("commit:{rid}@{sha}");
    let mut commit = Node::new(&cid, "commit");
    commit.title = sha.chars().take(12).collect();
    commit.summary = kg::clean(subject);
    commit.updated = committed_at.clone();
    if let Some(nwo) = rid.strip_prefix("gh:") {
        commit.url = format!("https://github.com/{nwo}/commit/{sha}");
    }
    memory_nodes.insert(cid.clone(), commit);
    edges.push(Edge::new(rid, "CONTAINS", &cid, &committed_at, "git", 0.9, abs));

    let Some(changed) = kg::run(
        &["git", "-C", abs, "diff-tree", "--root", "--no-commit-id", "--name-only", "-r", "-z", "--no-renames", sha],
        None,
        120,
    ) else {
        problems.push(format!("{evidence}: cannot list changed paths for {sha}"));
        return Some(cid);
    };
    for name in changed.split('\0').filter(|name| !name.is_empty()) {
        let path_id = format!("path:{rid}:{name}");
        let path_node = memory_nodes.entry(path_id.clone()).or_insert_with(|| {
            let mut node = Node::new(&path_id, "path");
            node.title = name.to_string();
            node.summary = rid.to_string();
            node.path = std::path::Path::new(abs).join(name).to_string_lossy().to_string();
            node
        });
        if committed_at > path_node.updated {
            path_node.updated = committed_at.clone();
        }
        edges.push(Edge::new(&cid, "TOUCHES", &path_id, &committed_at, "git", 0.9, sha));
        edges.push(Edge::new(rid, "CONTAINS", &path_id, &committed_at, "git", 0.9, abs));
    }
    Some(cid)
}

pub fn git(paths: &Paths, ont: &Ontology) -> Pull {
    let mut nodes = Vec::new();
    let mut memory_nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut problems = Vec::new();
    let mut seen_repos: HashSet<String> = HashSet::new();
    let mut verdicts: BTreeMap<String, (String, Result<(), String>)> = BTreeMap::new();
    let paren = re_of(r"\s*\(.*\)$");
    let remote_re = re_of(r"[:/]([\w.-]+)/([\w.-]+?)(?:\.git)?$");
    let home = std::env::var("HOME").unwrap_or_default();
    let wiki = kg::wiki_pages(&paths.root);
    let task_projects: HashMap<String, String> = wiki
        .iter()
        .filter(|(_, kind, _)| kind == "task")
        .map(|(id, _, path)| (id.clone(), kg::slug(&kg::frontmatter(path).text("project"))))
        .collect();
    let code_memory = ont.nodes.contains_key("commit")
        && ont.nodes.contains_key("path")
        && ont.has_relation("CONTAINS")
        && ont.has_relation("TOUCHES");

    for (nid, ntype, path) in &wiki {
        if ntype != "project" {
            continue;
        }
        let fm = kg::frontmatter(path);
        let history = match fm.text("history") {
            raw if raw.is_empty() => DEFAULT_HISTORY,
            raw => match raw.parse::<usize>() {
                Ok(value) if value <= MAX_HISTORY => value,
                Ok(value) => {
                    problems.push(format!("{nid}: history {value} exceeds {MAX_HISTORY}; using {MAX_HISTORY}"));
                    MAX_HISTORY
                }
                Err(_) => {
                    problems.push(format!("{nid}: invalid history '{raw}'; using {DEFAULT_HISTORY}"));
                    DEFAULT_HISTORY
                }
            },
        };
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
            if seen_repos.insert(rid.clone()) {
                let mut n = Node::new(&rid, "repo");
                n.title = rid.split_once(':').map(|x| x.1).unwrap_or("").to_string();
                n.updated = since.clone();
                n.path = abs.clone();
                nodes.push(n);
            }
            edges.push(Edge::new(&ont.canon(nid), "LIVES_IN", &rid, &since, "git", 0.9, &abs));

            if code_memory && history > 0 {
                let limit = history.to_string();
                if let Some(log) = kg::run(
                    &["git", "-C", &abs, "log", "--no-merges", "-n", &limit, "--format=%H"],
                    None,
                    120,
                ) {
                    for revision in log.lines().map(str::trim).filter(|line| !line.is_empty()) {
                        add_commit(&abs, &rid, revision, nid, &mut memory_nodes, &mut edges, &mut problems);
                    }
                } else {
                    problems.push(format!("{nid}: cannot read bounded commit history in {abs}"));
                }
            }

            let mut references = Vec::new();
            if !code_memory {
                continue;
            }
            for (tid, kind, task_path) in &wiki {
                if kind != "task" || task_projects.get(tid).is_none_or(|project| project != nid) {
                    continue;
                }
                let task_fm = kg::frontmatter(task_path);
                for revision in task_fm.list("commits") {
                    references.push(CommitRef {
                        revision,
                        wiki_id: ont.canon(tid),
                        task: ont.canon(tid),
                        evidence: paths.rel(task_path),
                        since: String::new(),
                        test: false,
                        passing: false,
                    });
                }
            }
            for (test_id, kind, test_path) in &wiki {
                if kind != "test" {
                    continue;
                }
                let test_fm = kg::frontmatter(test_path);
                let task = kg::slug(&test_fm.text("task"));
                if task_projects.get(&task).is_none_or(|project| project != nid) {
                    continue;
                }
                references.push(CommitRef {
                    revision: test_fm.text("commit"),
                    wiki_id: ont.canon(test_id),
                    task: ont.canon(&task),
                    evidence: paths.rel(test_path),
                    since: test_fm.text("ran_at"),
                    test: true,
                    passing: passing_test(&test_fm),
                });
            }
            let mut implemented: HashMap<String, Vec<String>> = HashMap::new();
            let mut tested_at: HashMap<String, Vec<String>> = HashMap::new();
            for reference in references {
                if reference.revision.is_empty() {
                    continue;
                }
                let Some(cid) = add_commit(
                    &abs,
                    &rid,
                    &reference.revision,
                    &reference.evidence,
                    &mut memory_nodes,
                    &mut edges,
                    &mut problems,
                ) else {
                    continue;
                };
                let committed_at = memory_nodes.get(&cid).map(|node| node.updated.clone()).unwrap_or_default();
                let since = if reference.since.is_empty() { &committed_at } else { &reference.since };
                if reference.test {
                    edges.push(Edge::new(
                        &reference.wiki_id, "TESTS", &cid, since, "page", 1.0, &reference.evidence,
                    ));
                    if reference.passing {
                        tested_at.entry(reference.task.clone()).or_default().push(cid.clone());
                        edges.push(Edge::new(
                            &reference.wiki_id, "VERIFIES", &cid, since, "page", 1.0, &reference.evidence,
                        ));
                    }
                } else {
                    implemented.entry(reference.task.clone()).or_default().push(cid.clone());
                    edges.push(Edge::new(
                        &cid, "IMPLEMENTS", &reference.wiki_id, &committed_at, "page", 1.0, &reference.evidence,
                    ));
                }
            }

            if !ont.has_relation("EXEMPLIFIES") {
                continue;
            }
            let default_ref = default_branch(&abs);
            for (tid, kind, task_path) in &wiki {
                if kind != "task" || task_projects.get(tid).is_none_or(|project| project != nid) {
                    continue;
                }
                let task_fm = kg::frontmatter(task_path);
                let Some(nominated) = nomination(&task_fm) else {
                    continue;
                };
                let task_id = ont.canon(tid);
                let commits = implemented.remove(&task_id).unwrap_or_default();
                let touched: HashSet<&str> = edges
                    .iter()
                    .filter(|edge| edge.rel == "TOUCHES" && commits.contains(&edge.src))
                    .map(|edge| edge.dst.as_str())
                    .collect();
                let candidate = Candidate {
                    abs: &abs,
                    status: task_fm.text("status"),
                    declared: task_fm.list("commits").len(),
                    tested_at: tested_at.remove(&task_id).unwrap_or_default(),
                    touched: touched.len(),
                    commits,
                    nominated,
                };
                let verdict = judge(&candidate, default_ref.as_deref(), ont.gate.max_exemplar_paths);
                if verdict.is_ok() {
                    let since = candidate
                        .commits
                        .iter()
                        .filter_map(|cid| memory_nodes.get(cid).map(|node| node.updated.clone()))
                        .max()
                        .unwrap_or_default();
                    let evidence = paths.rel(task_path);
                    for concept in &candidate.nominated.concepts {
                        edges.push(Edge::new(&task_id, "EXEMPLIFIES", &ont.canon(concept), &since, "page", 1.0, &evidence));
                    }
                }
                let held = verdicts.entry(task_id).or_insert_with(|| (nid.clone(), verdict.clone()));
                if verdict.is_ok() {
                    *held = (nid.clone(), verdict);
                }
            }
        }
    }

    if ont.has_relation("EXEMPLIFIES") {
        for (tid, kind, task_path) in &wiki {
            let task_id = ont.canon(tid);
            if kind == "task" && !verdicts.contains_key(&task_id) && nomination(&kg::frontmatter(task_path)).is_some() {
                let project = task_projects.get(tid).cloned().unwrap_or_default();
                verdicts.insert(task_id, (project, Err("its project has no reachable repository".into())));
            }
        }
    }
    let mut refusals: Vec<Vec<String>> = Vec::new();
    for (task_id, (project, verdict)) in &verdicts {
        if let Err(reason) = verdict {
            problems.push(format!("{task_id}: exemplar held back — {reason}"));
            refusals.push(vec![task_id.clone(), project.clone(), reason.clone()]);
        }
    }
    let _ = kg::write_tsv(&paths.raw().join("exemplars.tsv"), &EXEMPLAR_COLS, &refusals);

    nodes.extend(memory_nodes.into_values());
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

fn person_id(display: &str, name: &str, ont: &Ontology) -> String {
    let base = if !display.is_empty() { display } else { name };
    let lowered = base.trim().to_lowercase();
    let cleaned = re_of(r"[^a-z0-9]+").replace_all(&lowered, "-").to_string();
    ont.canon(cleaned.trim_matches('-'))
}

/// How far back a finished issue stays in the graph.
const LINEAR_WINDOW_DAYS: i64 = 30;

/// Issues assigned to the owner, with the Linear project (a `milestone` here —
/// `project` is the wiki's own type) and cycle each sits in. Every issue is
/// the viewer's, so it is `ASSIGNED_TO` the owner's own node when `me` is known
/// rather than to a second person minted from a Linear display name.
pub fn linear_graph(data: &[Value], me: Option<&str>, ont: &Ontology) -> (Vec<Node>, Vec<Edge>) {
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // An issue only referenced — a parent, a blocked ticket — may belong to
    // someone else and never arrive as a row of its own.
    let mut referenced: Vec<(String, String)> = Vec::new();

    for it in data {
        let iid = head(it, "identifier");
        if iid.is_empty() {
            continue;
        }
        let since = first10(&head(it, "updatedAt"));
        if seen.insert(iid.clone()) {
            let mut node = Node::new(&iid, "issue");
            node.title = kg::clean(&head(it, "title"));
            node.status = kg::clean(it.pointer("/state/type").and_then(Value::as_str).unwrap_or(""));
            node.state = kg::clean(it.pointer("/state/name").and_then(Value::as_str).unwrap_or(""));
            let priority = it.get("priority").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            node.summary = crate::watch::linear::priority_label(priority).to_string();
            node.updated = since.clone();
            node.url = head(it, "url");
            nodes.push(node);
        }

        if let Some(proj) = it.get("project").filter(|p| !p.is_null()) {
            let mid = crate::watch::linear::milestone_id(&head(proj, "id"));
            if seen.insert(mid.clone()) {
                let mut n = Node::new(&mid, "milestone");
                n.title = kg::clean(&head(proj, "name"));
                n.status = kg::clean(&head(proj, "state"));
                n.summary = kg::clean(&head(proj, "health"));
                n.updated = since.clone();
                n.url = head(proj, "url");
                nodes.push(n);
            }
            edges.push(Edge::new(&iid, "SCOPED_TO", &mid, &since, "linear", 1.0, &iid));
        }

        if let Some(cyc) = it.get("cycle").filter(|c| !c.is_null()) {
            let team = cyc.pointer("/team/key").and_then(Value::as_str).unwrap_or("?").to_string();
            let number = cyc.get("number").and_then(Value::as_f64).unwrap_or(0.0) as i64;
            let cid = format!("lin:cycle:{team}-{number}");
            if seen.insert(cid.clone()) {
                let mut n = Node::new(&cid, "cycle");
                n.title = format!("{team} cycle {number}");
                n.summary = format!("{}..{}", first10(&head(cyc, "startsAt")), first10(&head(cyc, "endsAt")));
                n.updated = since.clone();
                nodes.push(n);
            }
            edges.push(Edge::new(&iid, "PLANNED_IN", &cid, &since, "linear", 1.0, &iid));
        }

        if let Some(asg) = it.get("assignee").filter(|a| !a.is_null()) {
            let pid = match me {
                Some(me) => ont.canon(me),
                None => person_id(&head(asg, "displayName"), &head(asg, "name"), ont),
            };
            if !pid.is_empty() {
                if seen.insert(pid.clone()) {
                    let mut n = Node::new(&pid, "person");
                    n.title = if me.is_some() { pid.clone() } else { kg::clean(&head(asg, "name")) };
                    nodes.push(n);
                }
                edges.push(Edge::new(&iid, "ASSIGNED_TO", &pid, &since, "linear", 1.0, &iid));
            }
        }

        if let Some(parent) = it.get("parent").filter(|p| !p.is_null()) {
            let pid = head(parent, "identifier");
            if !pid.is_empty() {
                edges.push(Edge::new(&iid, "SUBTASK_OF", &pid, &since, "linear", 1.0, &iid));
                referenced.push((pid, head(parent, "title")));
            }
        }

        if let Some(rels) = it.pointer("/relations/nodes").and_then(Value::as_array) {
            for rel in rels.iter().filter(|r| head(r, "type") == "blocks") {
                let Some(other) = rel.get("relatedIssue").filter(|r| !r.is_null()) else {
                    continue;
                };
                let oid = head(other, "identifier");
                if !oid.is_empty() {
                    edges.push(Edge::new(&iid, "BLOCKS", &oid, &since, "linear", 1.0, &iid));
                    referenced.push((oid, head(other, "title")));
                }
            }
        }

        if let Some(atts) = it.pointer("/attachments/nodes").and_then(Value::as_array) {
            for att in atts {
                let url = head(att, "url");
                if let Some(pr) = crate::watch::linear::pr_id_of(&url) {
                    edges.push(Edge::new(&pr, "IMPLEMENTS", &iid, &since, "linear", 1.0, &url));
                }
            }
        }
    }

    for (id, title) in referenced {
        if seen.insert(id.clone()) {
            let mut n = Node::new(&id, "issue");
            n.title = kg::clean(&title);
            nodes.push(n);
        }
    }
    (nodes, edges)
}

pub fn linear(paths: &Paths, ont: &Ontology) -> Pull {
    let since = crate::clock::days_ago(LINEAR_WINDOW_DAYS);
    let pull = match crate::watch::linear::fetch(&since) {
        Ok(pull) => pull,
        Err(e) => {
            return Pull {
                nodes: Vec::new(),
                edges: Vec::new(),
                problems: vec![format!("linear: {e} — is $LINEAR_API_KEY set, or `linear auth login` current?")],
            };
        }
    };
    let mut problems: Vec<String> = Vec::new();
    if !pull.complete {
        problems.push("linear: pagination stopped early".into());
    }
    let (nodes, edges) = linear_graph(&pull.nodes, paths.me().as_deref(), ont);
    let _ = kg::dump_raw(paths, "linear", &nodes, &edges);
    Pull { nodes, edges, problems }
}

const PR_FIELDS: &str = "number,title,state,isDraft,url,updatedAt,headRefName,baseRefName,author,reviewRequests,statusCheckRollup";

fn repos_of_interest(known: &[String]) -> Vec<String> {
    let mut repos: HashSet<String> = known.iter().filter(|r| r.starts_with("gh:")).cloned().collect();
    let rows = kg::run(
        &["gh", "search", "prs", "--author=@me", "--state=open", "--limit=100", "--json", "repository"],
        None,
        90,
    )
    .and_then(|out| serde_json::from_str::<Value>(&out).ok());
    if let Some(Value::Array(rows)) = rows {
        for row in rows {
            let name = row.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("");
            if !name.is_empty() {
                repos.insert(format!("gh:{name}"));
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
