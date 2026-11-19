//! artalde — the thinking-os control plane for the coding agents on this
//! machine. See `template/artalde/AGENTS.md` for the config folder.

mod actions;
mod app;
mod diff_view;
mod feed;
mod fonts;
mod gfm;
mod gh;
mod kit;
mod layout_view;
mod markdown;
mod mod_view;
mod panels;
mod repo_view;
mod review_view;
mod reviewer;
mod settings_view;
mod style;
mod views;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use artalde_core::command::COMMANDS;
use artalde_core::config;
use artalde_core::hook::{self, HookEvent, Outcome};
use artalde_core::model::{AgentState, SessionRecord};
use artalde_core::store::{self, Store};
use artalde_core::{now, probe};
use serde_json::{Value, json};

const USAGE: &str = "\
artalde — a live control plane for your coding agents

usage:
  artalde                    open the window
  artalde hook               Claude Code hook: read one event on stdin (wire with `artalde hooks`)
  artalde report --id ID --state STATE [--agent A] [--message M] [--cwd DIR] [--end]
                             report state from any other agent (working|blocked|done|idle)
  artalde check              load the config folder, render every mod against live data
  artalde snapshot           print one probe pass as JSON
  artalde inbox              list your PR inbox (review requested, mine, involved, merged)
    artalde pr <owner/repo#N|url>  summarise one PR: state, checks, reviews, threads
    artalde repo <owner/repo>  open PRs, what is deployed where, recent releases
  artalde review <owner/repo#N> [--agent NAME]  run a read-only agent review; its comments
                             are saved as proposals for the PR's Review tab (nothing is posted)
  artalde commands           list commands for keys.json and mods
  artalde hooks [--install]  print (or merge into ~/.claude/settings.json) the hook entries
  artalde init               write missing default files into the config folder

config: <wiki>/artalde (ARTALDE_CONFIG_DIR overrides) · state: ~/.local/state/artalde
";

/// Hook payloads larger than this are ignored.
const MAX_HOOK_BYTES: u64 = 4 * 1024 * 1024;

const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store = Store::new(&store::state_dir());
    let result = match args.first().map(String::as_str) {
        None | Some("ui") => gui(store),
        Some("hook") => {
            // A hook must never fail the agent: report and exit 0.
            if let Err(e) = run_hook(&store) {
                eprintln!("artalde hook: {e:#}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("report") => report(&store, args.get(1..).unwrap_or_default()),
        Some("check") => check(store),
        Some("snapshot") => {
            let (_, snap) = feed::snapshot_once(store, config::config_dir());
            serde_json::to_string_pretty(&snap)
                .map_err(anyhow::Error::from)
                .map(|s| {
                    println!("{s}");
                    ExitCode::SUCCESS
                })
        }
        Some("commands") => {
            for (name, desc) in COMMANDS {
                println!("{name:16} {desc}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("inbox") => inbox_cli(),
        Some("pr") => pr_cli(args.get(1).map(String::as_str).unwrap_or_default()),
        Some("review") => review_cli(args.get(1..).unwrap_or_default()),
        Some("repo") => repo_cli(args.get(1).map(String::as_str).unwrap_or_default()),
        Some("hooks") => hooks(args.get(1).is_some_and(|a| a == "--install")),
        Some("init") => {
            let dir = config::config_dir();
            config::install_defaults(&dir)
                .map_err(anyhow::Error::from)
                .map(|wrote| {
                    let what = if wrote.is_empty() {
                        "nothing".to_owned()
                    } else {
                        wrote.join(", ")
                    };
                    println!("{}: wrote {what}", dir.display());
                    ExitCode::SUCCESS
                })
        }
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => {
            eprint!("unknown command {other:?}\n\n{USAGE}");
            Ok(ExitCode::from(2))
        }
    };
    result.unwrap_or_else(|e| {
        eprintln!("artalde: {e:#}");
        ExitCode::FAILURE
    })
}

fn gui(store: Store) -> anyhow::Result<ExitCode> {
    let dir = config::config_dir();
    if !dir.exists() {
        let wrote = config::install_defaults(&dir)?;
        eprintln!("artalde: created {} ({} files)", dir.display(), wrote.len());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("artalde")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "artalde",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, store, dir)?))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(ExitCode::SUCCESS)
}

fn run_hook(store: &Store) -> anyhow::Result<()> {
    let mut body = String::new();
    std::io::stdin()
        .take(MAX_HOOK_BYTES)
        .read_to_string(&mut body)?;
    let ev: HookEvent = serde_json::from_str(&body)?;
    if ev.session_id.is_empty() {
        anyhow::bail!("event has no session_id");
    }
    let mut rec = store.load(&ev.session_id).unwrap_or_default();
    if hook::apply(&mut rec, &ev, now()) == Outcome::Remove {
        store.remove(&ev.session_id)?;
        return Ok(());
    }
    if rec.pid.is_none() {
        let procs = probe::processes();
        if let Some(p) = probe::agent_ancestor(&procs, std::os::unix::process::parent_id()) {
            rec.pid = Some(p.pid);
            rec.tty.clone_from(&p.tty);
            rec.agent.clone_from(&p.name);
        }
    }
    store.save(&rec)?;
    Ok(())
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn report(store: &Store, args: &[String]) -> anyhow::Result<ExitCode> {
    let id = flag(args, "--id").ok_or_else(|| anyhow::anyhow!("--id is required"))?;
    if args.iter().any(|a| a == "--end") {
        store.remove(id)?;
        return Ok(ExitCode::SUCCESS);
    }
    let state = flag(args, "--state")
        .and_then(AgentState::parse)
        .ok_or_else(|| anyhow::anyhow!("--state working|blocked|done|idle"))?;
    let t = now();
    let mut rec = store.load(id).unwrap_or_else(|| SessionRecord {
        id: id.to_owned(),
        ..Default::default()
    });
    if rec.state != state || rec.since == 0 {
        rec.since = t;
    }
    rec.state = state;
    rec.updated = t;
    rec.message = flag(args, "--message").map(str::to_owned);
    if let Some(a) = flag(args, "--agent") {
        a.clone_into(&mut rec.agent);
    } else if rec.agent.is_empty() {
        rec.agent = "agent".into();
    }
    if let Some(c) = flag(args, "--cwd") {
        rec.cwd = PathBuf::from(c);
    } else if rec.cwd.as_os_str().is_empty() {
        rec.cwd = std::env::current_dir().unwrap_or_default();
    }
    store.save(&rec)?;
    Ok(ExitCode::SUCCESS)
}

fn check(store: Store) -> anyhow::Result<ExitCode> {
    let dir = config::config_dir();
    let (cfg, snap) = feed::snapshot_once(store, dir.clone());
    let mut failed = cfg.errors.clone();
    let mut host = artalde_mods::ModHost::new();
    host.sync(&cfg.mods);
    let view = artalde_mods::ViewState {
        selected: None,
        filter: None,
        theme: &cfg.settings.theme,
    };
    let ctx = artalde_mods::context(&snap, view, 1);
    for m in &cfg.mods {
        let r = host.render(&m.name, &ctx);
        match &r.node {
            Ok(_) => println!("mod {:12} ok  {} µs", m.name, r.micros),
            Err(e) => failed.push(format!("mods/{}.rhai: {e}", m.name)),
        }
    }
    for l in &cfg.layouts {
        for m in l.root.mods() {
            if cfg.mod_source(&m).is_none() {
                failed.push(format!(
                    "layouts/{}.json: uses mod:{m}, but mods/{m}.rhai is missing",
                    l.name
                ));
            }
        }
    }
    if cfg.layout(&cfg.settings.layout).is_none() {
        failed.push(format!(
            "settings.json: layout {:?} does not exist",
            cfg.settings.layout
        ));
    }
    println!(
        "{}: {} layouts, {} themes, {} mods · {} sessions, {} spaces, {} proposals, {} PRs",
        dir.display(),
        cfg.layouts.len(),
        cfg.themes.len(),
        cfg.mods.len(),
        snap.sessions.len(),
        snap.spaces.len(),
        snap.inbox.len(),
        snap.prs.len()
    );
    for e in &snap.errors {
        println!("probe: {e}");
    }
    if failed.is_empty() {
        println!("ok");
        return Ok(ExitCode::SUCCESS);
    }
    for f in &failed {
        eprintln!("error: {f}");
    }
    Ok(ExitCode::FAILURE)
}

/// The hook command line: the installed binary's absolute path when this
/// is it, else plain `artalde` from PATH.
fn hook_command() -> String {
    let bin = artalde_core::home().unwrap_or_default().join(".local/bin");
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .filter(|p| p.starts_with(&bin));
    let base = exe.map_or_else(|| "artalde".to_owned(), |p| p.display().to_string());
    format!("{base} hook")
}

fn hooks(install: bool) -> anyhow::Result<ExitCode> {
    let cmd = hook_command();
    let entry = json!({ "hooks": [{ "type": "command", "command": cmd }] });
    if !install {
        let hooks: serde_json::Map<String, Value> = HOOK_EVENTS
            .iter()
            .map(|e| ((*e).to_owned(), json!([entry])))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "hooks": hooks }))?
        );
        eprintln!(
            "\nMerge into ~/.claude/settings.json, or run `artalde hooks --install` (backs the file up first)."
        );
        return Ok(ExitCode::SUCCESS);
    }
    let path = artalde_core::home()
        .ok_or_else(|| anyhow::anyhow!("no HOME"))?
        .join(".claude/settings.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut settings: Value = serde_json::from_str(&text)?;
    let root = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json is not an object"))?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks is not an object"))?;
    let mut added = Vec::new();
    for event in HOOK_EVENTS {
        let list = hooks.entry(event).or_insert_with(|| json!([]));
        let list = list
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("hooks.{event} is not a list"))?;
        if !list.iter().any(|g| g.to_string().contains("artalde hook")) {
            list.push(entry.clone());
            added.push(event);
        }
    }
    if added.is_empty() {
        println!("already installed");
        return Ok(ExitCode::SUCCESS);
    }
    let backup = path.with_extension("json.artalde-bak");
    std::fs::write(&backup, &text)?;
    let tmp = path.with_extension("json.artalde-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&settings)? + "\n")?;
    std::fs::rename(&tmp, &path)?;
    println!(
        "added `{cmd}` to {} (backup: {})",
        added.join(", "),
        backup.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn inbox_cli() -> anyhow::Result<ExitCode> {
    use artalde_core::github::{self, Bucket};
    let since = since_days(14);
    for b in Bucket::ALL {
        let (_, rows) = github::fetch_bucket(b, &since).map_err(|e| anyhow::anyhow!(e))?;
        println!("## {} ({})", b.label(), rows.len());
        for r in rows {
            let pr = r.pr.map(|p| p.to_string()).unwrap_or_default();
            println!("{pr:32} {:8} {:17} {}", r.checks, r.review, r.title);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn pr_cli(arg: &str) -> anyhow::Result<ExitCode> {
    use artalde_core::github::{self, PrRef};
    let pr = PrRef::parse(arg).ok_or_else(|| anyhow::anyhow!("usage: artalde pr owner/repo#N"))?;
    let d = github::fetch_detail(&pr).map_err(|e| anyhow::anyhow!(e))?;
    println!("{pr} — {}", d.title);
    println!(
        "{} {}  {} → {}  +{} -{}  {} files",
        d.state,
        if d.draft { "draft" } else { "" },
        d.head,
        d.base,
        d.additions,
        d.deletions,
        d.changed_files
    );
    println!(
        "merge: {} {}  review: {}  checks: {}",
        d.mergeable,
        d.merge_state,
        d.review,
        github::checks_summary(&d)
    );
    println!(
        "timeline: {} items · threads: {} ({} open)",
        d.timeline.len(),
        d.threads.len(),
        d.threads.iter().filter(|t| !t.resolved).count()
    );
    for c in d.checks.iter().filter(|c| c.failed()) {
        println!(
            "  failing: {} / {} (job {:?})",
            c.workflow, c.name, c.job_id
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// `YYYY-MM-DD`, `days` ago.
fn since_days(days: u64) -> String {
    civil_date(now().saturating_sub(days * 86_400))
}

/// `YYYY-MM-DD` for Unix seconds `t`.
pub(crate) fn civil_date(t: u64) -> String {
    let (y, m, d) = civil(t / 86_400);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since the epoch → (year, month, day).
fn civil(days: u64) -> (i64, u32, u32) {
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn review_cli(args: &[String]) -> anyhow::Result<ExitCode> {
    use artalde_core::github::{self, PrRef};
    use artalde_core::review;
    let pr = args
        .first()
        .and_then(|a| PrRef::parse(a))
        .ok_or_else(|| anyhow::anyhow!("usage: artalde review owner/repo#N [--agent NAME]"))?;
    let cfg = config::Config::load(&config::config_dir(), None);
    let agents = cfg.settings.review_agents;
    let agent = match flag(args, "--agent") {
        Some(name) => agents.iter().find(|a| a.name.eq_ignore_ascii_case(name)),
        None => agents.first(),
    }
    .ok_or_else(|| anyhow::anyhow!("no such review agent in settings.json"))?;
    let detail = github::fetch_detail(&pr).map_err(|e| anyhow::anyhow!(e))?;
    let files = github::fetch_diff(&pr).map_err(|e| anyhow::anyhow!(e))?;
    let dir = review::review_dir(&store::state_dir(), &pr);
    eprintln!(
        "{}: {} is reviewing (read-only) in {}",
        pr,
        agent.name,
        dir.display()
    );
    let (pid_tx, _pid_rx) = std::sync::mpsc::channel();
    let reply = reviewer::run(&pr, &agent.command, &detail, &dir, None, &pid_tx)
        .map_err(|e| anyhow::anyhow!(e))?;
    let run = review::parse_agent_output(&reply).map_err(|e| anyhow::anyhow!(e))?;
    let mut pending = review::load(&dir);
    let (anchored, loose) = pending.absorb(run, &files);
    review::save(&dir, &pending)?;
    println!("summary: {}", pending.summary);
    println!("verdict: {}", pending.agent_verdict);
    for d in pending
        .drafts
        .iter()
        .filter(|d| d.state == review::DraftState::Proposed)
    {
        println!(
            "- [{}] {}:{} {} — {}",
            d.severity,
            d.path,
            d.line,
            d.title,
            artalde_core::text::clip(&d.body, 120)
        );
    }
    println!(
        "{anchored} proposed on lines, {loose} general notes · open the PR's Review tab to decide"
    );
    Ok(ExitCode::SUCCESS)
}

fn repo_cli(repo: &str) -> anyhow::Result<ExitCode> {
    use artalde_core::github;
    if !repo.contains('/') {
        anyhow::bail!("usage: artalde repo owner/repo");
    }
    let r = github::fetch_repo(repo).map_err(|e| anyhow::anyhow!(e))?;
    let t = now();
    println!("{} — {} ({})", r.name, r.description, r.default_branch);
    println!("deployed:");
    for d in r.environments() {
        println!(
            "  {:12} {:10} {:8} {} by {}",
            d.environment,
            d.reference,
            d.status,
            github::age(&d.created, t),
            d.creator
        );
    }
    println!(
        "open PRs: {} · merged recently: {} · releases: {}",
        r.open.len(),
        r.merged.len(),
        r.releases.len()
    );
    for p in r.open.iter().take(10) {
        println!(
            "  #{:<6} {:8} {}",
            p.pr.as_ref().map_or(0, |x| x.number),
            p.checks,
            p.title
        );
    }
    Ok(ExitCode::SUCCESS)
}
