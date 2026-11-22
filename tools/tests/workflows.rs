//! Offline user journeys through the real CLIs, graph gate and views.
use os::kg::{self, Ontology, Paths};
use os::sare::{fuse, serve};
use os::watch::github_issues;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Wiki(Paths);
impl Wiki {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("os-workflow-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("graph/raw")).unwrap();
        let core = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        fs::copy(
            core.join("template/graph/ontology.yaml"),
            root.join("graph/ontology.yaml"),
        )
        .unwrap();
        fs::write(root.join("os.yaml"), "me:\n  github: octocat\n").unwrap();
        Wiki(Paths { root })
    }
    fn poll(&self, nodes: Option<Vec<Value>>) -> Value {
        let mut data = json!({"open":{"nodes":[]},"merged":{"nodes":[]},"review":{"nodes":[]}});
        if let Some(nodes) = nodes {
            data["githubIssues"] = json!({"nodes":nodes});
        }
        let fixture = self.0.root.join("fixture.json");
        fs::write(&fixture, json!({"data": data}).to_string()).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_zaindu"))
            .args(["poll", "--json", "--fixture"])
            .arg(fixture)
            .arg("--root")
            .arg(&self.0.root)
            .env("ZAINDU_LOGIN", "octocat")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Wiki {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0.root);
    }
}

fn issue(state: &str) -> Value {
    json!({"number":12,"title":"Make agents faster","url":"https://github.com/acme/core/issues/12",
        "repository":{"nameWithOwner":"acme/core"},"state":state,"stateReason":"COMPLETED",
        "createdAt":"2026-11-20T10:00:00Z","updatedAt":"2026-11-22T10:00:00Z",
        "labels":{"nodes":[]},"comments":{"nodes":[]},"blockedBy":{"nodes":[]},
        "parent":{"number":1,"repository":{"nameWithOwner":"acme/core"}},
        "closedByPullRequestsReferences":{"nodes":[{"url":"https://github.com/acme/core/pull/20"}]}})
}
fn kinds(report: &Value) -> Vec<String> {
    report["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn github_issues_seed_close_reopen_and_retry_without_duplicate_proposals() {
    let wiki = Wiki::new("issue-events");
    let open = issue("OPEN");
    let seeded = wiki.poll(Some(vec![open.clone()]));
    assert_eq!(seeded["github_issues_seeded"], true);
    assert!(kinds(&seeded).is_empty());
    assert!(kinds(&wiki.poll(Some(vec![open.clone()]))).is_empty());
    let closed = issue("CLOSED");
    assert_eq!(
        kinds(&wiki.poll(Some(vec![closed.clone()]))),
        ["issue.done"]
    );
    assert!(kinds(&wiki.poll(Some(vec![closed]))).is_empty());
    assert_eq!(kinds(&wiki.poll(Some(vec![open]))), ["issue.state_changed"]);
    let inbox = fs::read_to_string(
        wiki.0
            .root
            .join(format!("inbox/events/{}.md", os::clock::today())),
    )
    .unwrap();
    assert_eq!(inbox.lines().filter(|l| l.starts_with("- [ ]")).count(), 1);
    assert!(inbox.contains("acme/core/issues/12"));
}

#[test]
fn incomplete_issue_reads_retain_state_and_graph_without_events() {
    let wiki = Wiki::new("partial");
    wiki.poll(Some(vec![issue("OPEN")]));
    let before = fs::read(wiki.0.raw().join("github-issues.json")).unwrap();
    let mut partial = issue("CLOSED");
    partial["blockedBy"]["pageInfo"] = json!({"hasNextPage":true});
    let failed = wiki.poll(Some(vec![partial]));
    assert!(kinds(&failed).is_empty());
    assert_eq!(failed["github_issues"], 1);
    assert!(!failed["warnings"].as_array().unwrap().is_empty());
    let missing = wiki.poll(None);
    assert!(kinds(&missing).is_empty());
    assert_eq!(
        before,
        fs::read(wiki.0.raw().join("github-issues.json")).unwrap()
    );
    let after: Value =
        serde_json::from_slice(&fs::read(wiki.0.raw().join("github-issues.json")).unwrap())
            .unwrap();
    let before: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(before["nodes"], after["nodes"]);
    assert_eq!(before["edges"], after["edges"]);
    assert_eq!(
        after["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "issue")
            .unwrap()["state"],
        "Open"
    );
}

#[test]
fn github_and_linear_issues_coexist_in_the_validated_work_view() {
    let wiki = Wiki::new("issue-graph");
    let ont = Ontology::load(&wiki.0.ontology()).unwrap();
    let issues = github_issues::parse(&[issue("OPEN")], "octocat");
    let (mut nodes, mut edges) = github_issues::graph(&issues, "octocat", &ont);
    let mut linear = kg::Node::new("APP-12", "issue");
    linear.title = "Independent Linear task".into();
    linear.status = "started".into();
    linear.state = "In Progress".into();
    nodes.push(linear);
    edges.push(kg::Edge::new(
        "APP-12",
        "ASSIGNED_TO",
        "octocat",
        "",
        "linear",
        1.0,
        "fixture",
    ));
    kg::dump_raw(&wiki.0, "github-issues", &nodes, &edges).unwrap();
    fs::create_dir_all(wiki.0.root.join("tasks")).unwrap();
    fs::create_dir_all(wiki.0.root.join("projects")).unwrap();
    fs::write(
        wiki.0.root.join("projects/core.md"),
        "---\ntitle: Core\nstatus: active\n---\n",
    )
    .unwrap();
    fs::write(
        wiki.0.root.join("tasks/speed.md"),
        "---\ntitle: Speed\nproject: core\nissue: gh:acme/core/issues/12\n---\n",
    )
    .unwrap();
    os::sare::extract::pages(&wiki.0, &ont);
    let fused = fuse::fuse(&wiki.0, &ont);
    assert!(fused.rejected.is_empty(), "{:?}", fused.rejected);
    assert!(
        fused
            .edges
            .values()
            .any(|e| e.src == "speed" && e.rel == "TRACKS" && e.dst == "gh:acme/core/issues/12")
    );
    fuse::write(&wiki.0, &fused).unwrap();
    let view = serve::Graph::load(&wiki.0).view_work(Some("octocat"));
    assert!(view.contains("## Tickets (2)"));
    assert!(view.contains("acme/core/issues/12"));
    assert!(view.contains("acme/core#20"));
    assert!(view.contains("APP-12"));
    let snapshot = os::watch::model::Snapshot {
        github_issues: issues,
        github_issues_at: os::clock::stamp(),
        ..Default::default()
    };
    let board = os::ikusi::status::board(&snapshot, 10);
    assert_eq!(board["github_issues"], true);
    assert_eq!(board["next"]["total"], 1);
    assert_eq!(fuse::infer_type("gh:acme/core/issues/12"), "issue");
    assert_eq!(fuse::infer_type("gh:acme/core#12"), "pr");
}

#[test]
fn issue_comments_priority_and_unblocking_use_real_transitions() {
    let wiki = Wiki::new("actionable");
    let mut open = issue("OPEN");
    open["blockedBy"]["nodes"] =
        json!([{"number":3,"state":"OPEN","repository":{"nameWithOwner":"acme/core"}}]);
    wiki.poll(Some(vec![open]));
    let mut changed = issue("OPEN");
    changed["labels"]["nodes"] = json!([{"name":"priority: high"}]);
    changed["comments"]["nodes"] =
        json!([{"createdAt":"2026-11-22T11:00:00Z","author":{"login":"reviewer"}}]);
    let report = wiki.poll(Some(vec![changed.clone()]));
    let events = kinds(&report);
    assert!(events.contains(&"issue.unblocked".to_string()));
    assert!(events.contains(&"issue.priority_raised".to_string()));
    assert!(events.contains(&"issue.commented".to_string()));
    changed["comments"]["nodes"] =
        json!([{"createdAt":"2026-11-22T12:00:00Z","author":{"login":"octocat"}}]);
    assert!(kinds(&wiki.poll(Some(vec![changed]))).is_empty());
}

#[test]
fn today_cli_collects_old_pending_work_and_bounds_views_without_mutating_them() {
    let wiki = Wiki::new("today");
    fs::create_dir_all(wiki.0.root.join("inbox/events")).unwrap();
    fs::create_dir_all(wiki.0.views()).unwrap();
    fs::create_dir_all(wiki.0.root.join("projects")).unwrap();
    fs::write(
        wiki.0.root.join("inbox/events/2026-01-01.md"),
        "- [ ] old pending issue\n- [x] handled\n",
    )
    .unwrap();
    fs::write(
        wiki.0.root.join("projects/core.md"),
        "---\ntitle: Core\nstatus: active\nsummary: Engine\n---\nSecret long project body\n",
    )
    .unwrap();
    let large = "work".repeat(10000);
    fs::write(wiki.0.views().join("work.md"), &large).unwrap();
    fs::write(
        wiki.0.root.join("log.md"),
        format!("{}\n## [2026-11-22] test | latest\n", "old".repeat(20000)),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_sare"))
        .args(["today", "--root"])
        .arg(&wiki.0.root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let packet: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(packet["proposals"][0]["text"], "- [ ] old pending issue");
    assert_eq!(packet["views"]["work"]["truncated"], true);
    assert!(packet["views"]["health"]["error"].is_string());
    assert_eq!(packet["active_projects"].as_array().unwrap().len(), 1);
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Secret long project body"));
    assert_eq!(
        packet["last_log_entries_newest_first"][0],
        "## [2026-11-22] test | latest"
    );
    assert_eq!(
        fs::read_to_string(wiki.0.views().join("work.md")).unwrap(),
        large
    );
    assert!(!wiki.0.root.join("briefings").exists());
}
