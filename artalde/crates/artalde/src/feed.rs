//! The feed: two background threads that turn the machine into
//! [`Snapshot`]s and the config folder into [`Config`]s.
//!
//! - `probe` (every second): config edits, hook records, processes, git
//!   places, tasks, transcripts, zaindu's inbox; worktree status every
//!   [`SPACES_EVERY`].
//! - `gh` (every [`GH_EVERY`]): open PRs.
//!
//! Both are owned by [`Feed`], which stops and joins them on drop.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use artalde_core::catalog::{self, Catalog};
use artalde_core::claude;
use artalde_core::config::{self, Config};
use artalde_core::model::{
    AgentState, Pr, Resource, ResourceKind, Session, SessionRecord, Snapshot, Space,
};
use artalde_core::probe::{self, GitPlace};
use artalde_core::store::Store;
use artalde_core::{inbox, now};
use eframe::egui;

const TICK: Duration = Duration::from_secs(1);
const SPACES_EVERY: Duration = Duration::from_secs(10);
const HEARTBEAT: Duration = Duration::from_secs(5);
const GH_EVERY: Duration = Duration::from_secs(120);
/// A record whose process is gone is dropped after this.
const DEAD_GRACE_SECS: u64 = 15;
/// A record with no pid (reported by `artalde report`) expires after this.
const PIDLESS_TTL_SECS: u64 = 6 * 3600;
const MAX_PLACE_CACHE: usize = 1024;
const MAX_WORKTREES_PER_SPACE: usize = 24;
const UI_QUEUE: usize = 32;
/// PRs and tickets kept per session, each.
const MAX_RESOURCES: usize = 6;
/// A hook record older than Claude's own status by this much yields to it.
const HOOK_STALE_SECS: u64 = 5;

pub enum Msg {
    Snapshot(Box<Snapshot>),
    Config(Box<Config>),
    Log(String),
}

enum Ctl {
    Refresh,
    Prs(Result<Vec<Pr>, String>),
    Stop,
}

pub struct Feed {
    pub rx: Receiver<Msg>,
    probe_tx: Sender<Ctl>,
    gh_tx: Sender<Ctl>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Feed {
    pub fn start(ctx: &egui::Context, store: Store, config_dir: PathBuf) -> std::io::Result<Feed> {
        let (ui_tx, rx) = sync_channel(UI_QUEUE);
        let (probe_tx, probe_rx) = channel();
        let (gh_tx, gh_rx) = channel();
        // Published by `probe` once settings load; read by `gh`.
        let gh_enabled = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));

        let probe = {
            let ctx = ctx.clone();
            let ui_tx = ui_tx.clone();
            let gh_enabled = Arc::clone(&gh_enabled);
            std::thread::Builder::new()
                .name("artalde-probe".into())
                .spawn(move || {
                    Prober::new(store, config_dir, Some(ui_tx), Some(ctx), gh_enabled)
                        .run(&probe_rx);
                })?
        };
        let gh = {
            let probe_tx = probe_tx.clone();
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("artalde-gh".into())
                .spawn(move || {
                    gh_loop(&gh_rx, &probe_tx, &gh_enabled, &stop);
                })?
        };
        Ok(Feed {
            rx,
            probe_tx,
            gh_tx,
            stop,
            threads: vec![probe, gh],
        })
    }

    pub fn refresh(&self) {
        // A closed channel means the thread is gone; nothing to refresh.
        let _closed = self.probe_tx.send(Ctl::Refresh);
        let _closed = self.gh_tx.send(Ctl::Refresh);
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _closed = self.probe_tx.send(Ctl::Stop);
        let _closed = self.gh_tx.send(Ctl::Stop);
        // Unblock a probe thread waiting on a full UI queue.
        while self.rx.try_recv().is_ok() {}
        for t in self.threads.drain(..) {
            if t.join().is_err() {
                eprintln!("artalde: a feed thread panicked");
            }
        }
    }
}

fn gh_loop(rx: &Receiver<Ctl>, probe_tx: &Sender<Ctl>, enabled: &AtomicBool, stop: &AtomicBool) {
    // Let settings load before the first call.
    let mut wait = Duration::from_secs(2);
    loop {
        match rx.recv_timeout(wait) {
            Ok(Ctl::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
        wait = GH_EVERY;
        if stop.load(Ordering::Acquire) {
            return;
        }
        if !enabled.load(Ordering::Acquire) {
            continue;
        }
        if probe_tx.send(Ctl::Prs(probe::my_prs(60))).is_err() {
            return;
        }
    }
}

/// `(path, mtime)` → value, for files re-read only when they change.
type MtimeCache<T> = HashMap<PathBuf, (SystemTime, T)>;

struct Prober {
    store: Store,
    config_dir: PathBuf,
    ui: Option<SyncSender<Msg>>,
    ctx: Option<egui::Context>,
    gh_enabled: Arc<AtomicBool>,

    config: Option<Config>,
    fingerprint: std::collections::BTreeMap<PathBuf, (SystemTime, u64)>,
    places: HashMap<PathBuf, Option<GitPlace>>,
    cwds: HashMap<u32, PathBuf>,
    said: MtimeCache<Option<String>>,
    inbox: MtimeCache<Vec<artalde_core::model::InboxItem>>,
    spaces: Vec<Space>,
    spaces_at: Option<Instant>,
    scans: HashMap<PathBuf, claude::Scan>,
    claude_prev: HashMap<String, (AgentState, u64)>,
    catalog: Catalog,
    teams: Vec<String>,
    prs: Vec<Pr>,
    errors: Vec<String>,
    last: Option<Snapshot>,
    sent_at: Instant,
    version: u64,
}

impl Prober {
    fn new(
        store: Store,
        config_dir: PathBuf,
        ui: Option<SyncSender<Msg>>,
        ctx: Option<egui::Context>,
        gh_enabled: Arc<AtomicBool>,
    ) -> Prober {
        Prober {
            store,
            config_dir,
            ui,
            ctx,
            gh_enabled,
            config: None,
            fingerprint: Default::default(),
            places: HashMap::new(),
            cwds: HashMap::new(),
            said: HashMap::new(),
            inbox: HashMap::new(),
            spaces: Vec::new(),
            spaces_at: None,
            scans: HashMap::new(),
            claude_prev: HashMap::new(),
            catalog: Catalog::default(),
            teams: Vec::new(),
            prs: Vec::new(),
            errors: Vec::new(),
            last: None,
            sent_at: Instant::now(),
            version: 0,
        }
    }

    fn send(&self, msg: Msg) -> bool {
        let Some(ui) = &self.ui else { return true };
        let ok = ui.send(msg).is_ok();
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
        ok
    }

    fn run(mut self, rx: &Receiver<Ctl>) {
        loop {
            if !self.tick() {
                return;
            }
            match rx.recv_timeout(TICK) {
                Ok(Ctl::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Ctl::Refresh) => self.spaces_at = None,
                Ok(Ctl::Prs(Ok(prs))) => {
                    self.prs = prs;
                    self.errors.retain(|e| !e.starts_with("gh"));
                }
                Ok(Ctl::Prs(Err(e))) => {
                    self.errors.retain(|e| !e.starts_with("gh"));
                    self.errors.push(format!("gh: {e}"));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// One probe pass; false once the UI is gone.
    fn tick(&mut self) -> bool {
        if !self.reload_config() {
            return false;
        }
        let snap = self.snapshot();
        let changed = self.last.as_ref().is_none_or(|l| !same(l, &snap));
        if changed || self.sent_at.elapsed() >= HEARTBEAT {
            self.sent_at = Instant::now();
            self.last = Some(snap.clone());
            return self.send(Msg::Snapshot(Box::new(snap)));
        }
        true
    }

    fn reload_config(&mut self) -> bool {
        let fp = config::fingerprint(&self.config_dir);
        if self.config.is_some() && fp == self.fingerprint {
            return true;
        }
        self.fingerprint = fp;
        let cfg = Config::load(&self.config_dir, self.config.as_ref());
        self.gh_enabled.store(cfg.settings.gh, Ordering::Release);
        let summary = if cfg.errors.is_empty() {
            format!(
                "config loaded: {} layouts, {} themes, {} mods",
                cfg.layouts.len(),
                cfg.themes.len(),
                cfg.mods.len()
            )
        } else {
            format!("config: {}", cfg.errors.join("; "))
        };
        self.config = Some(cfg.clone());
        self.send(Msg::Log(summary)) && self.send(Msg::Config(Box::new(cfg)))
    }

    fn place(&mut self, path: &Path) -> Option<GitPlace> {
        let dir = if path.extension().is_some() || path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        if let Some(p) = self.places.get(dir) {
            return p.clone();
        }
        if self.places.len() >= MAX_PLACE_CACHE {
            self.places.clear();
        }
        let p = probe::git_place(dir);
        self.places.insert(dir.to_path_buf(), p.clone());
        p
    }

    fn snapshot(&mut self) -> Snapshot {
        let t = now();
        let wiki = config::wiki_root();
        self.catalog.refresh(&wiki);
        if self.teams.is_empty() {
            self.teams = catalog::linear_teams(&wiki);
        }
        let procs = probe::processes();
        let alive: HashSet<u32> = procs.iter().map(|p| p.pid).collect();

        let mut records = Vec::new();
        for rec in self.store.all() {
            let gone = match rec.pid {
                Some(pid) => {
                    !alive.contains(&pid) && t.saturating_sub(rec.updated) > DEAD_GRACE_SECS
                }
                None => t.saturating_sub(rec.updated) > PIDLESS_TTL_SECS,
            };
            if gone {
                if let Err(e) = self.store.remove(&rec.id) {
                    self.errors.push(format!("store: {e}"));
                }
            } else {
                records.push(rec);
            }
        }

        let claude = claude::sessions();
        // Hook records first; Claude Code's own session file fills in
        // what the hook does not know, and the state when the hook is
        // older or absent.
        for rec in &mut records {
            let Some(cs) = claude.values().find(|c| c.session_id == rec.id) else {
                continue;
            };
            rec.pid = rec.pid.or(Some(cs.pid));
            if rec.tty.is_none() {
                rec.tty = procs
                    .iter()
                    .find(|p| p.pid == cs.pid)
                    .and_then(|p| p.tty.clone());
            }
            if rec.transcript.is_none() {
                rec.transcript = claude::transcript_path(&cs.cwd, &cs.session_id);
            }
            if cs.status_since() > rec.updated + HOOK_STALE_SECS {
                let (state, since) = self.claude_state(cs);
                rec.state = state;
                rec.since = since;
                rec.message = cs
                    .waiting_for
                    .clone()
                    .filter(|_| state == AgentState::Blocked);
            }
        }
        let known: HashSet<u32> = records.iter().filter_map(|r| r.pid).collect();
        let fresh: Vec<&probe::Proc> = probe::agent_processes(&procs)
            .into_iter()
            .filter(|p| !known.contains(&p.pid))
            .collect();
        let need: Vec<u32> = fresh
            .iter()
            .filter(|p| !claude.contains_key(&p.pid))
            .map(|p| p.pid)
            .filter(|p| !self.cwds.contains_key(p))
            .collect();
        self.cwds.extend(probe::cwds(&need));
        self.cwds.retain(|pid, _| alive.contains(pid));
        for p in fresh {
            if let Some(cs) = claude.get(&p.pid) {
                let (state, since) = self.claude_state(cs);
                records.push(SessionRecord {
                    id: cs.session_id.clone(),
                    agent: p.name.clone(),
                    pid: Some(p.pid),
                    tty: p.tty.clone(),
                    cwd: cs.cwd.clone(),
                    state,
                    since,
                    updated: cs.status_since(),
                    message: cs
                        .waiting_for
                        .clone()
                        .filter(|_| state == AgentState::Blocked),
                    transcript: claude::transcript_path(&cs.cwd, &cs.session_id),
                    ..Default::default()
                });
                continue;
            }
            records.push(SessionRecord {
                id: format!("pid-{}", p.pid),
                agent: p.name.clone(),
                pid: Some(p.pid),
                tty: p.tty.clone(),
                cwd: self.cwds.get(&p.pid).cloned().unwrap_or_default(),
                state: AgentState::Unknown,
                since: t.saturating_sub(p.elapsed),
                updated: t,
                ..Default::default()
            });
        }
        let live: HashSet<&str> = claude.values().map(|c| c.session_id.as_str()).collect();
        self.claude_prev.retain(|id, _| live.contains(id.as_str()));

        let teams = self.teams.clone();
        let mut sessions: Vec<Session> = records
            .into_iter()
            .map(|mut rec| {
                let hooked = self.store.load(&rec.id).is_some();
                let place = rec
                    .touched
                    .clone()
                    .and_then(|p| self.place(&p))
                    .or_else(|| self.place(&rec.cwd.clone()));
                let (task, pending_tasks) = if rec.id.starts_with("pid-") {
                    (None, 0)
                } else {
                    probe::tasks(&rec.id)
                };
                let last_said = rec.transcript.as_deref().and_then(|p| self.last_said(p));
                let scan = rec.transcript.as_ref().map(|path| {
                    let scan = self.scans.entry(path.clone()).or_default();
                    scan.advance(path, &teams);
                    scan.clone()
                });
                if rec.last_prompt.is_none() {
                    rec.last_prompt = scan.as_ref().and_then(|s| s.last_prompt.clone());
                }
                let handle = rec
                    .pid
                    .and_then(|pid| claude.get(&pid))
                    .and_then(|c| c.name.clone());
                Session {
                    worktree: place.as_ref().map(|p| p.worktree.clone()),
                    repo: place.as_ref().map(|p| p.repo.clone()),
                    branch: None,
                    task,
                    pending_tasks,
                    last_said,
                    discovered: !hooked,
                    ai_title: scan.as_ref().and_then(|s| s.ai_title.clone()),
                    handle,
                    tag: None,
                    label: None,
                    resources: scan.map(|s| self.resources(&s)).unwrap_or_default(),
                    rec,
                }
            })
            .collect();
        let transcripts: HashSet<PathBuf> = sessions
            .iter()
            .filter_map(|s| s.rec.transcript.clone())
            .collect();
        self.scans.retain(|p, _| transcripts.contains(p));

        if self.spaces_at.is_none_or(|at| at.elapsed() >= SPACES_EVERY) {
            self.spaces = self.spaces(&sessions);
            self.spaces_at = Some(Instant::now());
        }
        for s in &mut sessions {
            s.branch = s.worktree.as_ref().and_then(|w| {
                self.spaces
                    .iter()
                    .flat_map(|sp| &sp.worktrees)
                    .find(|wt| &wt.path == w)
                    .and_then(|wt| wt.branch.clone())
            });
            if let Some(id) = s
                .branch
                .as_deref()
                .and_then(|b| claude::tickets(b, &self.teams).into_iter().next())
                && !s.resources.iter().any(|r| r.id == id)
            {
                s.resources
                    .insert(0, self.catalog.resolve(ResourceKind::Ticket, &id, 0));
            }
        }
        sessions.sort_by(|a, b| {
            b.rec
                .state
                .urgency()
                .cmp(&a.rec.state.urgency())
                .then(b.rec.since.cmp(&a.rec.since))
        });

        let inbox = self.inbox();
        self.version = self.version.wrapping_add(1);
        Snapshot {
            version: self.version,
            now: t,
            sessions,
            spaces: self.spaces.clone(),
            prs: self.prs.clone(),
            inbox,
            errors: self.errors.clone(),
        }
    }

    /// The state a Claude session file reports, with `done` for a turn
    /// that finished (busy → idle) since it was last busy.
    fn claude_state(&mut self, cs: &claude::ClaudeSession) -> (AgentState, u64) {
        let raw = cs.state();
        let prev = self.claude_prev.get(&cs.session_id).copied();
        let next = match (raw, prev) {
            (AgentState::Idle, Some((AgentState::Working, _))) => {
                (AgentState::Done, cs.status_since())
            }
            (AgentState::Idle, Some((AgentState::Done, since))) => (AgentState::Done, since),
            _ => (raw, cs.status_since()),
        };
        self.claude_prev.insert(cs.session_id.clone(), next);
        next
    }

    fn resources(&self, scan: &claude::Scan) -> Vec<Resource> {
        let mut out: Vec<Resource> = scan
            .top(ResourceKind::Pr, MAX_RESOURCES)
            .into_iter()
            .map(|(id, n)| self.catalog.resolve(ResourceKind::Pr, &id, n))
            .collect();
        out.extend(
            scan.top(ResourceKind::Ticket, MAX_RESOURCES)
                .into_iter()
                .map(|(id, n)| self.catalog.resolve(ResourceKind::Ticket, &id, n)),
        );
        out
    }

    fn last_said(&mut self, transcript: &Path) -> Option<String> {
        let mtime = std::fs::metadata(transcript)
            .and_then(|m| m.modified())
            .ok()?;
        if let Some((t, v)) = self.said.get(transcript)
            && *t == mtime
        {
            return v.clone();
        }
        let v = probe::last_said(transcript);
        self.said
            .insert(transcript.to_path_buf(), (mtime, v.clone()));
        v
    }

    fn spaces(&mut self, sessions: &[Session]) -> Vec<Space> {
        let mut roots: Vec<(String, PathBuf)> = Vec::new();
        for s in sessions {
            let Some(w) = &s.worktree else { continue };
            let Some(place) = self.place(w) else { continue };
            if !roots.iter().any(|(_, r)| *r == place.root) {
                roots.push((place.repo, place.root));
            }
        }
        roots.sort();
        roots
            .into_iter()
            .map(|(repo, root)| {
                let mut paths = probe::worktree_paths(&root);
                if paths.is_empty() {
                    paths.push(root.clone());
                }
                // Worktrees with an agent first, then the rest by path.
                paths.sort_by_key(|p| {
                    (
                        !sessions.iter().any(|s| s.worktree.as_ref() == Some(p)),
                        p.clone(),
                    )
                });
                paths.truncate(MAX_WORKTREES_PER_SPACE);
                let worktrees = paths
                    .iter()
                    .filter_map(|p| probe::worktree_status(p))
                    .collect();
                Space {
                    repo,
                    root,
                    worktrees,
                }
            })
            .collect()
    }

    fn inbox(&mut self) -> Vec<artalde_core::model::InboxItem> {
        let Some(cfg) = &self.config else {
            return Vec::new();
        };
        let wiki = config::wiki_root();
        let files = inbox::files(&wiki, cfg.settings.inbox_days.clamp(1, 14));
        self.inbox.retain(|p, _| files.contains(p));
        let mut out = Vec::new();
        for f in files {
            let Ok(mtime) = std::fs::metadata(&f).and_then(|m| m.modified()) else {
                continue;
            };
            let fresh = self.inbox.get(&f).is_none_or(|(t, _)| *t != mtime);
            if fresh {
                self.inbox.insert(f.clone(), (mtime, inbox::read(&f)));
            }
            if let Some((_, items)) = self.inbox.get(&f) {
                // Newest first within a file.
                out.extend(items.iter().rev().cloned());
            }
        }
        out
    }
}

/// One probe pass without a window, for `artalde check` and `artalde snapshot`.
pub fn snapshot_once(store: Store, config_dir: PathBuf) -> (Config, Snapshot) {
    let mut p = Prober::new(
        store,
        config_dir,
        None,
        None,
        Arc::new(AtomicBool::new(false)),
    );
    p.reload_config();
    if p.config.as_ref().is_some_and(|c| c.settings.gh) {
        match probe::my_prs(60) {
            Ok(prs) => p.prs = prs,
            Err(e) => p.errors.push(format!("gh: {e}")),
        }
    }
    let snap = p.snapshot();
    let cfg = p
        .config
        .take()
        .unwrap_or_else(|| Config::load(&p.config_dir, None));
    (cfg, snap)
}

/// Snapshots equal apart from version and clock.
fn same(a: &Snapshot, b: &Snapshot) -> bool {
    a.sessions == b.sessions
        && a.spaces == b.spaces
        && a.prs == b.prs
        && a.inbox == b.inbox
        && a.errors == b.errors
}
