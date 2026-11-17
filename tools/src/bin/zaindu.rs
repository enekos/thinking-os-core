use os::clock;
use os::watch::events;
use os::watch::events::{Event, Window};
use os::watch::gh;
use os::watch::graph;
use os::watch::linear;
use os::watch::model;
use os::watch::model::{Lifecycle, Snapshot};
use os::watch::proposals;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
zaindu — GitHub and Linear state changes as events for the thinking-os graph

  zaindu poll [--dry-run] [--seed] [--since-days N]
              [--fixture FILE] [--json] [--root DIR] [--state FILE]
  zaindu events [--kind K] [--since YYYY-MM-DD] [--limit N] [--root DIR]
  zaindu proposals [--date YYYY-MM-DD] [--root DIR]
  zaindu status [--root DIR] [--state FILE]
";

struct Args {
    command: String,
    flags: BTreeMap<String, String>,
    switches: Vec<String>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut args = Args {
        command: raw.first().cloned().unwrap_or_default(),
        flags: BTreeMap::new(),
        switches: Vec::new(),
    };
    let mut i = 1;
    while i < raw.len() {
        let token = raw[i].clone();
        if let Some(name) = token.strip_prefix("--") {
            let takes_value = matches!(
                name,
                "root" | "state" | "fixture" | "since-days" | "kind" | "since" | "limit" | "date"
            );
            if takes_value {
                args.flags
                    .insert(name.to_string(), raw.get(i + 1).cloned().unwrap_or_default());
                i += 2;
                continue;
            }
            args.switches.push(name.to_string());
        }
        i += 1;
    }
    args
}

impl Args {
    fn on(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }
    fn num(&self, name: &str, fallback: i64) -> i64 {
        self.get(name).and_then(|v| v.parse().ok()).unwrap_or(fallback)
    }
}

fn resolve_root(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    for var in ["THINKING_OS_ROOT", "ZAINDU_ROOT"] {
        if let Ok(p) = std::env::var(var) {
            if !p.is_empty() {
                return PathBuf::from(p);
            }
        }
    }
    let mut here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        if here.join("graph/ontology.yaml").exists() {
            return here;
        }
        if !here.pop() {
            break;
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("thinking-os")
}

fn state_path(root: &Path, explicit: Option<&str>) -> PathBuf {
    explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("scripts/.gh-watch-state.json"))
}

fn load_state(path: &Path) -> Snapshot {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, snap: &Snapshot) -> std::io::Result<()> {
    os::kg::write_atomic(path, serde_json::to_string_pretty(snap)?.as_bytes())
}

fn poll(args: &Args) -> Result<i32, String> {
    let root = resolve_root(args.get("root"));
    let state_file = state_path(&root, args.get("state"));
    let since_days = args.num("since-days", 14);
    let prev = load_state(&state_file);

    let login = match args.get("fixture") {
        Some(_) if !prev.login.is_empty() => prev.login.clone(),
        Some(_) => std::env::var("ZAINDU_LOGIN").map_err(|_| "a fixture run needs $ZAINDU_LOGIN".to_string())?,
        None => gh::login()?,
    };

    let (payload, mut warnings): (Value, Vec<String>) = match args.get("fixture") {
        Some(f) => (
            serde_json::from_str(&fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?)
                .map_err(|e| format!("{f}: {e}"))?,
            Vec::new(),
        ),
        None => gh::fetch(&login, since_days)?,
    };
    let (prs, missing) = gh::parse_partial(&payload, &login)?;
    warnings.extend(missing.iter().map(|b| format!("{b}: absent from the response")));

    // Linear is a second, independent source: its failure never marks the
    // GitHub half incomplete, and a list it could not read in full is not
    // diffed at all — the last full one stays in the state file instead.
    let config = os::kg::linear_config(&root);
    let mut linear_warnings: Vec<String> = Vec::new();
    let fixture_issues = payload.pointer("/data/linear/nodes").and_then(Value::as_array).cloned();
    let pulled = match (args.get("fixture"), fixture_issues) {
        (Some(_), Some(nodes)) => Some(linear::Pull { nodes, complete: true }),
        (Some(_), None) => None,
        (None, _) if config.is_some() => match linear::fetch(&clock::days_ago(since_days)) {
            Ok(pull) => Some(pull),
            Err(e) => {
                linear_warnings.push(format!("linear: {e}"));
                None
            }
        },
        (None, _) => None,
    };
    let linear_on = config.is_some() || args.get("fixture").is_some();
    let teams = config.map(|c| c.teams).unwrap_or_default();

    let mut curr = Snapshot {
        taken_at: clock::stamp(),
        login: login.clone(),
        prs: prs.iter().map(|p| (p.id.clone(), p.clone())).collect(),
        ..Snapshot::default()
    };
    let fresh_issues = match pulled {
        Some(pull) if pull.complete => {
            curr.issues = linear::parse(&pull.nodes).into_iter().map(|i| (i.id.clone(), i)).collect();
            curr.issues_at = curr.taken_at.clone();
            true
        }
        other => {
            if other.is_some() {
                linear_warnings.push("linear: more issues than one poll reads — keeping the last full list".into());
            }
            if linear_on {
                curr.issues = prev.issues.clone();
                curr.issues_at = prev.issues_at.clone();
            }
            false
        }
    };

    let seeding = prev.prs.is_empty() || args.on("seed");
    let seeding_issues = fresh_issues && (prev.issues_at.is_empty() || args.on("seed"));
    let window = Window {
        ts: curr.taken_at.clone(),
        merged_from: clock::days_ago(since_days),
        complete: warnings.is_empty(),
    };
    let mut found = if seeding {
        Vec::new()
    } else {
        events::diff(&prev, &curr, &window)
    };
    linear::annotate_merges(&mut found, &curr, &teams);
    if fresh_issues && !seeding_issues {
        found.extend(linear::diff(&prev, &curr, &curr.taken_at));
    }
    warnings.extend(linear_warnings);

    let mut wrote: Vec<String> = Vec::new();
    if !args.on("dry-run") {
        let ledgers = events::append_monthly(&root, &found).map_err(|e| format!("events: {e}"))?;
        let idx = graph::index(&root);
        let lines = proposals::render(&found, &idx);
        let inbox = proposals::append(&root, &clock::today(), &lines)
            .map_err(|e| format!("inbox: {e}"))?;
        let nodes = graph::write_raw(&root, &prs, &login).map_err(|e| format!("raw: {e}"))?;
        save_state(&state_file, &curr).map_err(|e| format!("state: {e}"))?;
        if !found.is_empty() {
            for (ledger, added) in &ledgers {
                wrote.push(format!(
                    "{} (+{added})",
                    ledger.strip_prefix(&root).unwrap_or(ledger).display()
                ));
            }
            wrote.push(format!(
                "{} (+{})",
                inbox.strip_prefix(&root).unwrap_or(&inbox).display(),
                lines.len()
            ));
        }
        wrote.push(format!("graph/raw/watch.json ({nodes} nodes)"));
    }

    if args.on("json") {
        let report = serde_json::json!({
            "seeded": seeding,
            "watched": curr.prs.len(),
            "issues": curr.issues.len(),
            "events": found.iter().map(|e| serde_json::json!({
                "kind": e.kind.tag(), "subject": e.subject, "detail": e.detail
            })).collect::<Vec<_>>(),
            "wrote": wrote,
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return Ok(0);
    }

    let mine = curr.prs.values().filter(|p| p.mine).count();
    let reviewing = curr.prs.values().filter(|p| p.review_requested_of_me).count();
    let open_issues = curr.issues.values().filter(|i| i.is_open()).count();
    let tickets = if curr.issues.is_empty() {
        String::new()
    } else {
        format!(" · {open_issues} open issues")
    };
    if seeding {
        println!("zaindu · seeded {} PRs ({mine} mine, {reviewing} to review) — no events on a first run", curr.prs.len());
    } else {
        println!(
            "zaindu · {} PRs watched ({mine} mine, {reviewing} to review){tickets} · {} events",
            curr.prs.len(),
            found.len()
        );
    }
    if seeding_issues {
        println!("zaindu · seeded {} Linear issues ({open_issues} open) — no issue events on a first read", curr.issues.len());
    }
    for e in &found {
        println!("  {:<22} {:<28} {}", e.kind.tag(), e.slug(), e.detail);
    }
    if !wrote.is_empty() {
        println!("wrote {}", wrote.join(", "));
    }
    for w in &warnings {
        eprintln!("zaindu: partial poll — {w}");
    }
    Ok(0)
}

fn show_events(args: &Args) -> Result<i32, String> {
    let root = resolve_root(args.get("root"));
    let all = events::read_all(&root);
    let kind = args.get("kind").and_then(events::Kind::parse);
    let since = args.get("since").unwrap_or("").to_string();
    let limit = args.num("limit", 40) as usize;
    let mut rows: Vec<&Event> = all
        .iter()
        .filter(|e| kind.map(|k| e.kind == k).unwrap_or(true))
        .filter(|e| e.ts.as_str() >= since.as_str())
        .collect();
    rows.reverse();
    rows.truncate(limit);
    for e in rows.iter().rev() {
        println!("{}  {:<22} {:<28} {}", e.ts, e.kind.tag(), e.slug(), e.detail);
    }
    Ok(0)
}

fn show_proposals(args: &Args) -> Result<i32, String> {
    let root = resolve_root(args.get("root"));
    let date = args.get("date").map(str::to_string).unwrap_or_else(clock::today);
    let items = proposals::open_items(&root, &date);
    if items.is_empty() {
        println!("no open proposals for {date}");
    }
    for line in items {
        println!("{line}");
    }
    Ok(0)
}

fn show_status(args: &Args) -> Result<i32, String> {
    let root = resolve_root(args.get("root"));
    let snap = load_state(&state_path(&root, args.get("state")));
    if snap.prs.is_empty() {
        println!("no state yet — run `zaindu poll` once to seed");
        return Ok(0);
    }
    println!("taken at {} as @{}", snap.taken_at, snap.login);
    for pr in snap.sorted() {
        let flag = graph::headline(pr);
        let role = if pr.mine { "mine" } else { "review" };
        println!(
            "  {:<28} {:<7} {:<18} {}",
            pr.slug(),
            role,
            if flag.is_empty() { "open".into() } else { flag },
            pr.title.chars().take(60).collect::<String>()
        );
    }
    let open_red = snap
        .prs
        .values()
        .filter(|p| p.lifecycle == Lifecycle::Open && p.checks == model::Checks::Red)
        .count();
    println!("{} watched · {open_red} red", snap.prs.len());
    if !snap.issues.is_empty() {
        println!("issues as of {}", snap.issues_at);
        for issue in snap.sorted_issues().into_iter().filter(|i| i.is_open()) {
            println!(
                "  {:<28} {:<7} {:<18} {}",
                issue.id,
                linear::priority_label(issue.priority),
                issue.state,
                issue.title.chars().take(60).collect::<String>()
            );
        }
    }
    Ok(0)
}

fn main() {
    let args = parse_args();
    let result = match args.command.as_str() {
        "poll" => poll(&args),
        "events" => show_events(&args),
        "proposals" => show_proposals(&args),
        "status" => show_status(&args),
        "help" | "--help" | "-h" | "" => {
            print!("{USAGE}");
            Ok(0)
        }
        other => Err(format!("unknown command {other}\n\n{USAGE}")),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("zaindu: {message}");
            std::process::exit(1);
        }
    }
}
