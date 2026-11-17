use os::kg::{self, Edge, Node, Ontology, Paths};
use os::sare::{extract, fuse, retype, serve};
use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> Paths {
    let root = std::env::temp_dir().join(format!("sare-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("graph/raw")).unwrap();
    let wiki = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    fs::copy(wiki.join("template/graph/ontology.yaml"), root.join("graph/ontology.yaml")).unwrap();
    fs::write(root.join("graph/services.tsv"), "id\ttype\ttitle\tsummary\n").unwrap();
    Paths { root }
}

fn ont(paths: &Paths) -> Ontology {
    Ontology::load(&paths.ontology()).expect("the real ontology parses")
}

fn put_raw(paths: &Paths, name: &str, nodes: Vec<Node>, edges: Vec<Edge>) {
    kg::dump_raw(paths, name, &nodes, &edges).unwrap();
}

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn commit_file(repo: &std::path::Path, name: &str, contents: &str, date: &str) -> String {
    let file = repo.join(name);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, contents).unwrap();
    git(repo, &["add", "--", name]);
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["commit", "-q", "-m"])
        .arg(format!("change {name}"))
        .args(["-m", "commit bodies, diffs and transcripts are not graph memory"])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(output.status.success(), "commit: {}", String::from_utf8_lossy(&output.stderr));
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn the_real_ontology_parses_into_typed_rules() {
    let paths = scratch("ontology");
    let o = ont(&paths);
    assert!(o.has_relation("DEPENDS_ON"));
    assert!(!o.has_relation("RELATED_TO"));
    assert_eq!(o.relations["OPENED_IN"].card, "many-to-one");
    assert!(o.relations["IMPLEMENTS"].accepts_domain("commit"));
    assert!(o.relations["IMPLEMENTS"].accepts_range("task"));
    assert!(!o.relations["IMPLEMENTS"].accepts_domain("project"));
    assert!(o.relations["VERIFIES"].accepts_domain("test"));
    assert!(o.relations["MENTIONS"].deprecated);
    assert!(o.relations["MENTIONS"].accepts_domain("project"));
    assert!(!o.relations["DECIDES"].accepts_domain("project"));
    assert_eq!(o.conf_for("legacy"), 0.5);
    assert_eq!(o.canon("octocat"), "octocat", "no aliases ship in the template");
    assert_eq!(o.gate.stale_project_days, 45);
}

#[test]
fn a_slug_survives_every_way_a_link_is_written() {
    assert_eq!(kg::slug("[[bai]]"), "bai");
    assert_eq!(kg::slug("projects/bai.md"), "bai");
    assert_eq!(kg::slug("\"[[bai|the box]]\""), "bai");
    assert_eq!(kg::slug("sources/2026-09-07-oncall.md#section"), "2026-09-07-oncall");
    assert_eq!(kg::slug("  bai  "), "bai");
}

#[test]
fn tsv_storage_never_carries_a_tab_or_a_newline() {
    assert_eq!(kg::clean("a\tb\nc"), "a b c");
    assert_eq!(kg::clean("  padded \n"), "padded");
    let e = Edge::new("a", "DEPENDS_ON", "b", "2026-09-16", "page", 1.0, "x");
    assert_eq!(e.row()[5], "1.00");
}

#[test]
fn an_external_id_carries_its_own_type() {
    assert_eq!(fuse::infer_type("gh:join-com/data#7276"), "pr");
    assert_eq!(fuse::infer_type("gh:join-com/data"), "repo");
    assert_eq!(fuse::infer_type("local:/Users/x/src"), "repo");
    assert_eq!(fuse::infer_type("svc:bai.hirusta.io"), "service");
    assert_eq!(fuse::infer_type("commit:gh:acme/app@abcdef1234567"), "commit");
    assert_eq!(fuse::infer_type("path:gh:acme/app:src/lib.rs"), "path");
    assert_eq!(fuse::infer_type("ABC-3461"), "", "ticket-shaped ids are not a type here");
    assert_eq!(fuse::infer_type("sutegi"), "");
}

#[test]
fn code_memory_is_bounded_temporal_and_evidence_based() {
    let paths = scratch("code-memory");
    let repo = paths.root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Thinking OS Test"]);
    git(&repo, &["config", "user.email", "thinking-os@example.invalid"]);
    let first = commit_file(&repo, "src/first.rs", "first\n", "2026-01-01T10:00:00+00:00");
    let second = commit_file(&repo, "tests/verified.rs", "verified\n", "2026-01-02T10:00:00+02:00");
    let third = commit_file(&repo, "output/transcript.jsonl", "not memory\n", "2026-01-03T10:00:00+00:00");
    let fourth = commit_file(&repo, "src/latest.rs", "latest\n", "2026-01-04T10:00:00+00:00");

    for dir in ["projects", "tasks", "attempts", "tests"] {
        fs::create_dir_all(paths.root.join(dir)).unwrap();
    }
    fs::write(
        paths.root.join("projects/app.md"),
        format!("---\ntitle: app\nsummary: test app\nstatus: active\nupdated: 2026-01-04\npath: {}\nhistory: 1\n---\n", repo.display()),
    )
    .unwrap();
    fs::write(
        paths.root.join("tasks/context.md"),
        format!("---\ntitle: context\nsummary: scoped context\nstatus: done\nupdated: 2026-01-04\nproject: app\ncommits:\n  - {first}\n---\n"),
    )
    .unwrap();
    fs::write(
        paths.root.join("attempts/transcript-index.md"),
        "---\ntitle: transcript index\nsummary: index every tool call\nstatus: rejected\nupdated: 2026-01-03\ntask: context\nrejection: transcript volume hides durable decisions\n---\n",
    )
    .unwrap();
    fs::write(
        paths.root.join("tests/context-pass.md"),
        format!("---\ntitle: context pass\nsummary: context regression passed\nstatus: passed\nran_at: 2026-01-04T11:00:00+00:00\ntask: context\ncommit: {}\ncommand: cargo test context\nexit_code: 0\n---\n", &second[..8]),
    )
    .unwrap();
    fs::write(
        paths.root.join("tests/context-fail.md"),
        format!("---\ntitle: context failure\nsummary: stale-context regression failed\nstatus: failed\nran_at: 2026-01-04T11:01:00+00:00\ntask: context\ncommit: {}\ncommand: cargo test stale_context\nexit_code: 1\n---\n", &second[..8]),
    )
    .unwrap();

    let pages = extract::pages(&paths, &ont(&paths));
    assert!(pages.problems.is_empty(), "{:?}", pages.problems);
    let attempt = pages.nodes.iter().find(|node| node.id == "transcript-index").unwrap();
    assert_eq!(attempt.state, "transcript volume hides durable decisions");
    let passed = pages.nodes.iter().find(|node| node.id == "context-pass").unwrap();
    assert_eq!(passed.updated, "2026-01-04T11:00:00+00:00");
    assert_eq!(passed.state, "exit 0 · cargo test context");

    let git = extract::git(&paths, &ont(&paths));
    assert!(git.problems.is_empty(), "{:?}", git.problems);
    put_raw(&paths, "pages", pages.nodes, pages.edges);
    put_raw(&paths, "git", git.nodes, git.edges);
    let fused = fuse::fuse(&paths, &ont(&paths));
    assert!(fused.rejected.is_empty(), "{:?}", fused.rejected);

    let rid = format!("local:{}", repo.display());
    let commit_id = |sha: &str| format!("commit:{rid}@{sha}");
    assert!(fused.nodes.contains(&commit_id(&first)), "an old task commit is retained");
    assert!(fused.nodes.contains(&commit_id(&second)), "an old test commit is retained");
    assert!(fused.nodes.contains(&commit_id(&fourth)), "the latest commit is retained");
    assert!(!fused.nodes.contains(&commit_id(&third)), "unreferenced history outside the window stays out");
    assert!(!fused.nodes.contains(&format!("path:{rid}:output/transcript.jsonl")));
    assert_eq!(fused.nodes.values().filter(|node| node.kind == "commit").count(), 3);
    assert_eq!(fused.nodes.get(&commit_id(&first)).unwrap().updated, "2026-01-01T10:00:00Z");
    assert_eq!(fused.nodes.get(&commit_id(&second)).unwrap().updated, "2026-01-02T08:00:00Z");
    assert!(fused.nodes.values().all(|node| !node.summary.contains("transcripts are not graph memory")));

    let has = |src: &str, rel: &str, dst: &str| {
        fused.edges.values().any(|edge| edge.src == src && edge.rel == rel && edge.dst == dst)
    };
    assert!(has(&commit_id(&first), "IMPLEMENTS", "context"));
    assert!(has(&commit_id(&first), "TOUCHES", &format!("path:{rid}:src/first.rs")));
    assert!(has(&rid, "CONTAINS", &commit_id(&first)));
    assert!(has("transcript-index", "ATTEMPTS", "context"));
    assert!(has("context-pass", "TESTS", "context"));
    assert!(has("context-pass", "VERIFIES", "context"));
    assert!(has("context-pass", "VERIFIES", &commit_id(&second)));
    let verification = fused.edges.values().find(|edge| {
        edge.src == "context-pass" && edge.rel == "VERIFIES" && edge.dst == commit_id(&second)
    }).unwrap();
    assert_eq!(verification.since, "2026-01-04T11:00:00+00:00");
    assert!(has("context-fail", "TESTS", &commit_id(&second)));
    assert!(!has("context-fail", "VERIFIES", "context"));
    assert!(!has("context-fail", "VERIFIES", &commit_id(&second)));

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sare"))
        .arg("build")
        .arg("local")
        .arg("--root")
        .arg(&paths.root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("pages"));
    assert!(stdout.contains("git"));
    assert!(!stdout.contains("github"), "local builds must not contact GitHub: {stdout}");
    assert!(kg::read_tsv(&paths.nodes_tsv()).iter().any(|node| kg::get(node, "id") == commit_id(&first)));

    let mut legacy = ont(&paths);
    legacy.nodes.remove("commit");
    legacy.nodes.remove("path");
    legacy.relations.remove("CONTAINS");
    legacy.relations.remove("TOUCHES");
    let legacy_pull = extract::git(&paths, &legacy);
    assert!(legacy_pull.nodes.iter().all(|node| node.kind == "repo"));
    assert!(legacy_pull.edges.iter().all(|edge| edge.rel == "LIVES_IN"));
    let _ = fs::remove_dir_all(&paths.root);
}

#[test]
fn failed_or_incomplete_tests_cannot_claim_verification() {
    let paths = scratch("verification-gate");
    for dir in ["projects", "tasks", "tests"] {
        fs::create_dir_all(paths.root.join(dir)).unwrap();
    }
    fs::write(paths.root.join("projects/app.md"), "---\ntitle: app\n---\n").unwrap();
    fs::write(paths.root.join("tasks/context.md"), "---\ntitle: context\nproject: app\n---\n").unwrap();
    fs::write(
        paths.root.join("tests/failed.md"),
        "---\ntitle: failed\nstatus: failed\nran_at: 2026-01-04T11:00:00Z\ntask: context\ncommit: abcdef1234567\ncommand: cargo test\nexit_code: 1\nedges:\n  - VERIFIES context\n---\n",
    )
    .unwrap();
    fs::write(
        paths.root.join("tests/incomplete.md"),
        "---\ntitle: incomplete\nstatus: passed\nran_at: 2026-01-04T11:00:00Z\ntask: context\ncommit: abcdef1234567\nexit_code: 0\n---\n",
    )
    .unwrap();

    let pages = extract::pages(&paths, &ont(&paths));
    assert!(pages.problems.iter().any(|problem| problem.contains("cannot declare VERIFIES")));
    assert!(pages.problems.iter().any(|problem| problem.contains("command is required")));
    assert!(!pages.edges.iter().any(|edge| edge.rel == "VERIFIES"));
    assert!(pages.edges.iter().any(|edge| edge.src == "failed" && edge.rel == "TESTS"));
    let _ = fs::remove_dir_all(&paths.root);
}

#[test]
fn fusion_rejects_an_edge_whose_endpoints_have_the_wrong_type() {
    let paths = scratch("domain");
    let o = ont(&paths);
    let mut project = Node::new("bai", "project");
    project.status = "active".into();
    put_raw(
        &paths,
        "pages",
        vec![project, Node::new("llm-wiki", "concept")],
        vec![
            Edge::new("bai", "DEPENDS_ON", "llm-wiki", "", "page", 1.0, "ok-domain-wrong-range"),
            Edge::new("llm-wiki", "APPLIES_TO", "bai", "", "page", 1.0, "valid"),
            Edge::new("bai", "DECIDES", "llm-wiki", "", "page", 1.0, "wrong domain"),
            Edge::new("bai", "NONSENSE", "llm-wiki", "", "page", 1.0, "unknown relation"),
            Edge::new("bai", "DEPENDS_ON", "bai", "", "page", 1.0, "self"),
            Edge::new("bai", "DEPENDS_ON", "ghost", "", "page", 1.0, "dangling"),
        ],
    );
    let fused = fuse::fuse(&paths, &o);
    let kept: Vec<String> = fused.edges.values().map(|e| e.evidence.clone()).collect();
    assert_eq!(kept, vec!["valid"]);

    let why = |evidence: &str| {
        fused
            .rejected
            .iter()
            .find(|(e, _)| e.evidence == evidence)
            .map(|(_, w)| w.clone())
            .unwrap_or_default()
    };
    assert_eq!(why("ok-domain-wrong-range"), "DEPENDS_ON range is ['project', 'pr'], dst is concept");
    assert_eq!(why("wrong domain"), "DECIDES domain is ['decision'], src is project");
    assert_eq!(why("unknown relation"), "unknown relation NONSENSE");
    assert_eq!(why("self"), "self-edge");
    assert_eq!(why("dangling"), "dangling dst (ghost is not a node)");
}

#[test]
fn a_many_to_one_relation_keeps_only_its_best_edge() {
    let paths = scratch("cardinality");
    let o = ont(&paths);
    put_raw(
        &paths,
        "github",
        vec![Node::new("gh:acme/app#1", "pr"), Node::new("gh:acme/app", "repo"), Node::new("gh:acme/fork", "repo")],
        vec![
            Edge::new("gh:acme/app#1", "OPENED_IN", "gh:acme/fork", "2026-09-01", "git", 0.9, "weak"),
            Edge::new("gh:acme/app#1", "OPENED_IN", "gh:acme/app", "2026-09-02", "github", 1.0, "strong"),
        ],
    );
    let fused = fuse::fuse(&paths, &o);
    let kept: Vec<String> = fused.edges.values().map(|e| e.evidence.clone()).collect();
    assert_eq!(kept, vec!["strong"]);
    assert!(fused.rejected[0].1.contains("many-to-one"));
}

#[test]
fn an_alias_and_a_missing_endpoint_are_settled_before_the_gate() {
    let paths = scratch("alias");
    let text = fs::read_to_string(paths.ontology()).unwrap();
    fs::write(paths.ontology(), text.replace("aliases:\n    {}", "aliases:\n    octocat: octo-cat")).unwrap();
    let o = ont(&paths);
    put_raw(
        &paths,
        "github",
        vec![Node::new("gh:acme/app#7276", "pr"), Node::new("octocat", "person")],
        vec![Edge::new("gh:acme/app#7276", "AUTHORED_BY", "octocat", "", "github", 1.0, "pr")],
    );
    let fused = fuse::fuse(&paths, &o);
    assert!(fused.nodes.contains("octo-cat"), "the alias is canonicalised on the node");
    assert!(!fused.nodes.contains("octocat"), "and the alias itself never becomes a second node");
    assert_eq!(fused.edges.values().next().unwrap().dst, "octo-cat");
    assert!(fused.rejected.is_empty());
}

#[test]
fn a_pr_no_extractor_saw_is_minted_as_not_open() {
    let paths = scratch("minted");
    let o = ont(&paths);
    put_raw(
        &paths,
        "pages",
        vec![Node::new("my-app", "project")],
        vec![Edge::new("my-app", "DEPENDS_ON", "gh:acme/app#1", "", "page", 1.0, "projects/my-app.md")],
    );
    let fused = fuse::fuse(&paths, &o);
    let pr = fused.nodes.get("gh:acme/app#1").expect("minted");
    assert_eq!(pr.kind, "pr");
    assert_eq!(pr.status, "not-open");
    assert_eq!(pr.title, "", "a minted node never echoes its id as a title");
    assert_eq!(fused.edges.len(), 1);
}

#[test]
fn a_higher_confidence_source_wins_and_records_the_loser() {
    let paths = scratch("conflict");
    let o = ont(&paths);
    put_raw(
        &paths,
        "git",
        vec![Node::new("gh:r/p#1", "pr"), Node::new("gh:r/p#2", "pr")],
        vec![Edge::new("gh:r/p#2", "DEPENDS_ON", "gh:r/p#1", "2026-09-01", "git", 0.9, "branch")],
    );
    put_raw(
        &paths,
        "github",
        vec![],
        vec![Edge::new("gh:r/p#2", "DEPENDS_ON", "gh:r/p#1", "2026-09-02", "github", 1.0, "base=feat")],
    );
    let fused = fuse::fuse(&paths, &o);
    let e = fused.edges.values().next().unwrap();
    assert_eq!(e.prov, "github");
    assert_eq!(e.evidence, "base=feat +git");
}

#[test]
fn the_views_read_the_graph_the_way_a_human_would() {
    let paths = scratch("views");
    let o = ont(&paths);
    fs::write(
        paths.nodes_tsv(),
        "id\ttype\ttitle\tsummary\tstatus\tstate\tupdated\tpath\turl\n\
         bai\tproject\tbai\ta box that listens\tactive\t\t2026-09-16\tprojects/bai.md\t\n\
         ghost\tproject\tghost\t\tactive\t\t\tprojects/ghost.md\t\n\
         llm-wiki\tconcept\tLLM wiki\tthe pattern\t\t\t\tconcepts/llm-wiki.md\t\n\
         gh:acme/app#7276\tpr\tfix: anchor\tci: test\topen+red\t\t2026-09-16\t\thttps://github.com/acme/app/pull/7276\n\
         gh:acme/app#7277\tpr\tfeat: green\t\topen\t\t2026-09-16\t\t\n\
         gh:acme/app#7000\tpr\told\t\tmerged\t\t2026-09-01\t\t\n\
         gh:acme/web#9\tpr\ttheirs\t\tdraft\t\t2026-09-16\t\t\n\
         octocat\tperson\tocto\t\t\t\t\t\t\n",
    )
    .unwrap();
    fs::write(
        paths.edges_tsv(),
        "src\trel\tdst\tsince\tprov\tconf\tevidence\n\
         llm-wiki\tAPPLIES_TO\tbai\t\tpage\t1.00\tx\n\
         gh:acme/app#7277\tAUTHORED_BY\toctocat\t\tgithub\t1.00\tx\n\
         gh:acme/app#7276\tAUTHORED_BY\toctocat\t\tgithub\t1.00\tx\n\
         gh:acme/app#7000\tAUTHORED_BY\toctocat\t\tgithub\t1.00\tx\n\
         gh:acme/app#7277\tDEPENDS_ON\tgh:acme/app#7276\t\tgithub\t1.00\tx\n\
         octocat\tREVIEWS\tgh:acme/web#9\t\tgithub\t1.00\tx\n",
    )
    .unwrap();

    let graph = serve::Graph::load(&paths);
    let index = graph.view_index();
    assert!(index.contains("- [bai](projects/bai.md) — a box that listens"));
    assert!(index.contains("- [ghost](projects/ghost.md) — **orphan, no summary**"));
    assert!(index.contains("### active (2)"));
    assert!(index.contains("- **GitHub PRs** — 4"));

    let work = graph.view_work(Some("octocat"));
    assert!(work.contains("## Mine (2)"));
    let red = work.find("acme/app#7276").unwrap();
    let green = work.find("acme/app#7277").unwrap();
    assert!(red < green, "red checks sort first");
    assert!(work.contains("- [acme/app#7276](https://github.com/acme/app/pull/7276) 🔴 ci: test — fix: anchor"));
    assert!(work.contains("  - stacked on acme/app#7276"));
    assert!(!work.contains("#7000"), "a merged PR is not work");
    assert!(work.contains("## Waiting on my review (1)"));
    assert!(work.contains("- acme/web#9 (draft) — theirs"));
    assert!(graph.view_work(None).contains("No owner configured"));

    let health = graph.view_health(&paths, &o);
    assert!(health.contains("nodes **8** · edges **6**"));
    assert!(health.contains("## 5 · Active projects with no reachable repo (2)"));

    let projects = graph.view_projects(&o);
    assert!(projects.contains("| [bai](projects/bai.md) | active | — | — |  |"));
}

#[test]
fn retype_only_converts_what_a_stated_rule_covers() {
    let nodes = std::collections::HashMap::new();
    let typed = |src: &str, st: &str, dst: &str, dt: &str| match retype::rule(src, st, dst, dt, &nodes) {
        retype::Verdict::Typed(rel, _) => rel.to_string(),
        retype::Verdict::Drop(_) => "DROP".to_string(),
        retype::Verdict::Unclear(_) => "-".to_string(),
    };
    assert_eq!(typed("bai-forge-2026-09-15", "decision", "bai", "project"), "DECIDES");
    assert_eq!(typed("odei-ci-2026-09-15", "decision", "bai", "project"), "AFFECTS");
    assert_eq!(typed("bai-forge-2026-09-15", "decision", "2026-09-07-oncall", "source"), "EVIDENCED_BY");
    assert_eq!(typed("llm-wiki", "concept", "bai", "project"), "APPLIES_TO");
    assert_eq!(typed("bai", "project", "bai-forge-2026-09-15", "decision"), "DROP");
    assert_eq!(typed("2026-09-07-oncall", "source", "bai", "project"), "DROP");
    assert_eq!(typed("bai", "project", "sutegi", "project"), "-");
    assert_eq!(typed("bai", "project", "llm-wiki", "concept"), "-");
}

#[test]
fn superseding_needs_the_target_to_say_so_and_the_date_to_agree() {
    let mut nodes = std::collections::HashMap::new();
    let mut row = kg::Row::new();
    row.insert("id".into(), "forge-2026-08-01".into());
    row.insert("type".into(), "decision".into());
    row.insert("status".into(), "superseded by forge-2026-09-15".into());
    nodes.insert("forge-2026-08-01".to_string(), row);

    let verdict = retype::rule("forge-2026-09-15", "decision", "forge-2026-08-01", "decision", &nodes);
    assert!(matches!(verdict, retype::Verdict::Typed("SUPERSEDES", _)));

    let backwards = retype::rule("forge-2026-07-01", "decision", "forge-2026-08-01", "decision", &nodes);
    assert!(matches!(backwards, retype::Verdict::Unclear(_)), "an older page never supersedes a newer one");
}

#[test]
fn dates_that_are_not_dates_are_refused() {
    use os::clock;
    assert_eq!(clock::parse_date("2026-09-16").map(|_| true), Some(true));
    assert_eq!(clock::parse_date("2026-02-30"), None);
    assert_eq!(clock::parse_date("not-a-date"), None);
    assert_eq!(clock::parse_date(""), None);
    let today = clock::parse_date("2026-09-16").unwrap();
    assert_eq!(clock::days_since("2026-08-02", today), Some(45));
    assert_eq!(clock::days_since("2026-09-17", today), Some(-1));
}

#[test]
fn the_layout_is_deterministic_and_finite() {
    let links: Vec<(usize, usize)> = vec![(0, 1), (1, 2), (2, 0), (3, 4)];
    let a = os::sare::viz::layout(8, &links, 80);
    let b = os::sare::viz::layout(8, &links, 80);
    assert_eq!(a, b, "the same graph always lays out the same way");
    assert!(a.iter().all(|(x, y)| x.is_finite() && y.is_finite()));
    assert_eq!(a.len(), 8);
}

#[test]
fn disconnected_nodes_are_parked_clear_of_the_connected_core() {
    let links: Vec<(usize, usize)> = (1..14).map(|i| (0usize, i)).collect();
    let pos = os::sare::viz::layout(20, &links, 120);
    let core: Vec<(f32, f32)> = pos[..14].to_vec();
    let cx = core.iter().map(|p| p.0).sum::<f32>() / core.len() as f32;
    let core_r = core
        .iter()
        .map(|p| ((p.0 - cx).powi(2)).sqrt())
        .fold(0.0f32, f32::max);
    for orphan in &pos[14..] {
        assert!(orphan.0 > cx + core_r, "an orphan sits outside the core, not inside it");
    }
}

#[test]
fn every_type_lands_in_a_world_with_a_shape() {
    use os::sare::viz::{shape_of, world_of};
    for kind in ["project", "task", "attempt", "test", "concept", "decision", "person", "source", "briefing"] {
        assert_eq!(world_of(kind), "wiki");
    }
    for kind in ["repo", "commit", "path", "service", "pr"] {
        assert_eq!(world_of(kind), "code");
    }
    let mut seen = std::collections::HashSet::new();
    for kind in ["project", "concept", "decision", "person", "source", "briefing"] {
        assert!(seen.insert(shape_of(kind)), "{kind} reuses a shape inside its own world");
    }
    assert!(["project", "task", "attempt", "test", "concept", "decision", "person", "source", "briefing", "repo", "commit", "path", "service", "pr"]
        .iter()
        .all(|k| shape_of(k) < 6));
}

#[test]
fn the_page_carries_the_graph_and_nothing_external() {
    let paths = scratch("viz");
    fs::write(
        paths.nodes_tsv(),
        "id\ttype\ttitle\tsummary\tstatus\tstate\tupdated\tpath\turl\n\
         bai\tproject\tbai\ta box\tactive\t\t\tprojects/bai.md\t\n\
         llm-wiki\tconcept\tLLM wiki\t\t\t\t\tconcepts/llm-wiki.md\t\n\
         lonely\tsource\tlonely\t\t\t\t\tsources/lonely.md\t\n",
    )
    .unwrap();
    fs::write(
        paths.edges_tsv(),
        "src\trel\tdst\tsince\tprov\tconf\tevidence\n\
         llm-wiki\tAPPLIES_TO\tbai\t\tpage\t1.00\tx\n",
    )
    .unwrap();

    let graph = serve::Graph::load(&paths);
    let scene = os::sare::viz::build(&graph, 60);
    assert_eq!(scene.nodes.len(), 3);
    assert_eq!(scene.edges.len(), 1);
    assert!(os::sare::viz::placement_is_spread(&scene));

    let html = os::sare::viz_html::render(&scene);
    assert!(html.contains("<title>thinking-os — the whole graph</title>"));
    assert!(html.contains("\"bai\""), "the node data is embedded, not fetched");
    let fetched: Vec<&str> = html
        .match_indices("http")
        .map(|(i, _)| &html[i..(i + 40).min(html.len())])
        .filter(|s| !s.starts_with("http://www.w3.org"))
        .collect();
    assert!(fetched.is_empty(), "the page fetches nothing: {fetched:?}");
    assert!(!html.contains("cdn"), "no CDN is referenced");
    assert!(html.contains("prefers-color-scheme"), "both themes ship");
    assert!(html.contains("data-theme=\"dark\""));
    assert!(!html.contains("__NODES__") && !html.contains("__META__"), "every placeholder is filled");

    let dot = os::sare::viz::render_dot(&scene);
    assert!(dot.starts_with("digraph thinking_os {"));
    assert!(dot.contains("\"llm-wiki\" -> \"bai\" [label=\"APPLIES_TO\"];"));
    let _ = fs::remove_dir_all(&paths.root);
}

#[test]
fn a_reader_never_sees_a_half_written_file() {
    let paths = scratch("atomic");
    let target = paths.raw().join("watch.json");
    fs::write(&target, b"{\"nodes\":[],\"edges\":[]}").unwrap();

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let watched = target.clone();
    let reader = std::thread::spawn(move || {
        let mut torn = 0;
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(text) = fs::read_to_string(&watched) {
                if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                    torn += 1;
                }
            }
        }
        torn
    });

    let payload = serde_json::to_string_pretty(&serde_json::json!({
        "nodes": (0..400).map(|i| serde_json::json!({"id": format!("n{i}"), "type": "pr"})).collect::<Vec<_>>(),
        "edges": [],
    }))
    .unwrap();
    for _ in 0..80 {
        kg::write_atomic(&target, payload.as_bytes()).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(reader.join().unwrap(), 0, "a concurrent reader saw a torn file");

    let leftovers: Vec<String> = fs::read_dir(paths.raw())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    let _ = fs::remove_dir_all(&paths.root);
}

#[test]
fn only_the_wikis_own_claims_reach_the_tracked_projection() {
    let paths = scratch("projection");
    let node = |id: &str, kind: &str, path: &str| Node {
        id: id.into(),
        kind: kind.into(),
        path: path.into(),
        ..Node::default()
    };
    let edge = |src: &str, rel: &str, dst: &str, prov: &str| Edge {
        src: src.into(),
        rel: rel.into(),
        dst: dst.into(),
        since: String::new(),
        prov: prov.into(),
        conf: 1.0,
        evidence: String::new(),
    };
    let mut fused = fuse::Fused { nodes: os::sare::Ordered::new(), edges: os::sare::Ordered::new(), rejected: Vec::new() };
    fused.nodes.insert("bai", node("bai", "project", "projects/bai.md"));
    fused.nodes.insert("gh:enekos/bai#1", node("gh:enekos/bai#1", "pr", ""));
    fused.nodes.insert("gh:enekos/bai", node("gh:enekos/bai", "repo", "/Users/x/bai"));
    fused.edges.insert("a", edge("llm-wiki", "APPLIES_TO", "bai", "page"));
    fused.edges.insert("b", edge("gh:enekos/bai#1", "OPENED_IN", "gh:enekos/bai", "github"));
    fused.edges.insert("c", edge("bai", "LIVES_IN", "gh:enekos/bai", "git"));

    fuse::write(&paths, &fused).unwrap();
    let ids = |p: PathBuf| kg::read_tsv(&p).iter().map(|r| kg::get(r, "id").to_string()).collect::<Vec<_>>();
    let srcs = |p: PathBuf| kg::read_tsv(&p).iter().map(|r| kg::get(r, "src").to_string()).collect::<Vec<_>>();
    assert_eq!(ids(paths.nodes_tsv()).len(), 3);
    assert_eq!(ids(paths.wiki_nodes_tsv()), ["bai"]);
    assert_eq!(srcs(paths.edges_tsv()).len(), 3);
    assert_eq!(srcs(paths.wiki_edges_tsv()), ["llm-wiki"]);
}
