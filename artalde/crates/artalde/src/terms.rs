//! Terminals: the connection to `artalde host`, what it runs, and which
//! panes show what. Drawing is in `term_view.rs`, the panes in
//! `term_panes.rs`. See `docs/terminals.md`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use artalde_core::panes::{Dir, Pane};
use artalde_host::{
    Client, ClientError, Event, Events, Host, HostHandle, RunOptions, Spawn, State, TerminalInfo,
};
use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::term_view::TermView;

/// How long a request to the host may hold up a frame.
const HOST_TIMEOUT: Duration = Duration::from_secs(2);
/// Wait this long before trying an unreachable host again.
const RETRY_AFTER: Duration = Duration::from_secs(3);
/// A view nobody has drawn for this long lets go of its stream.
const DETACH_AFTER_SECS: f64 = 20.0;
/// The workspace the `terminal` panel of a layout shows.
pub const HOME_WORKSPACE: u32 = 0;

/// One tab's worth of panes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: u32,
    pub root: Pane,
    /// The pane keys go to.
    pub focus: u32,
    /// Only the focused pane is shown.
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    next: u32,
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

/// What happened to terminals since the last frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Exited { terminal: u32, clean: bool },
    Bell(u32),
}

pub struct Terms {
    state_dir: PathBuf,
    ctx: egui::Context,
    /// `artalde drive`: no host unless `ARTALDE_DRIVE_TERMINALS` asks for
    /// a private one inside the scratch state directory.
    pub dry_run: bool,
    client: Option<Client>,
    events: Option<Events>,
    /// The private host of a drive run; stops with the window.
    local: Option<HostHandle>,
    retry_at: Option<Instant>,
    /// The focused pane should take the keyboard on its next frame, even
    /// where panes do not hold it by default (a dashboard layout).
    pub grab: bool,
    /// Why the host is out of reach.
    pub error: Option<String>,
    pub infos: BTreeMap<u32, TerminalInfo>,
    pub views: HashMap<u32, TermView>,
    pub workspaces: Vec<Workspace>,
    next_workspace: u32,
    /// Terminals that rang since they were last looked at.
    pub bells: HashSet<u32>,
    listed: bool,
}

impl std::fmt::Debug for Terms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terms")
            .field("terminals", &self.infos.len())
            .field("workspaces", &self.workspaces.len())
            .finish_non_exhaustive()
    }
}

impl Terms {
    pub fn new(ctx: &egui::Context) -> Terms {
        let state_dir = artalde_core::store::state_dir();
        let saved: Saved = std::fs::read(state_dir.join("panes.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let next = saved
            .workspaces
            .iter()
            .map(|w| w.id + 1)
            .max()
            .unwrap_or(1)
            .max(saved.next)
            .max(1);
        Terms {
            state_dir,
            ctx: ctx.clone(),
            dry_run: false,
            client: None,
            events: None,
            local: None,
            retry_at: None,
            grab: false,
            error: None,
            infos: BTreeMap::new(),
            views: HashMap::new(),
            workspaces: saved.workspaces,
            next_workspace: next,
            bells: HashSet::new(),
            listed: false,
        }
    }

    /// Whether terminal commands do anything here.
    pub fn enabled(&self) -> bool {
        !self.dry_run || std::env::var_os("ARTALDE_DRIVE_TERMINALS").is_some()
    }

    /// Forget the back-off and try the host again now.
    pub fn reconnect(&mut self) {
        self.retry_at = None;
        drop(self.client());
    }

    pub fn connected(&self) -> bool {
        self.client.as_ref().is_some_and(Client::is_connected)
    }

    /// The host, starting it if need be.
    pub fn client(&mut self) -> Result<&Client, String> {
        if !self.enabled() {
            return Err("terminals are off in a dry run".into());
        }
        if self.client.as_ref().is_some_and(|c| !c.is_connected()) {
            self.drop_connection();
        }
        if self.client.is_none() {
            if self.retry_at.is_some_and(|t| Instant::now() < t) {
                return Err(self
                    .error
                    .clone()
                    .unwrap_or_else(|| "host unreachable".into()));
            }
            match self.connect() {
                Ok(()) => {
                    self.error = None;
                    self.retry_at = None;
                }
                Err(e) => {
                    self.retry_at = Some(Instant::now() + RETRY_AFTER);
                    self.error = Some(e.clone());
                    return Err(e);
                }
            }
        }
        self.client
            .as_ref()
            .ok_or_else(|| "host unreachable".to_owned())
    }

    fn connect(&mut self) -> Result<(), String> {
        let spawn = if self.dry_run {
            if self.local.is_none() {
                self.local = Some(Host::serve(&self.state_dir).map_err(|e| e.to_string())?);
            }
            Spawn::Never
        } else {
            Spawn::IfNeeded {
                exe: std::env::current_exe().map_err(|e| e.to_string())?,
            }
        };
        let mut client = Client::connect(&self.state_dir, spawn).map_err(|e| e.to_string())?;
        client.set_timeout(HOST_TIMEOUT);
        let ctx = self.ctx.clone();
        client.set_waker(Arc::new(move || ctx.request_repaint()));
        // Subscribe, then list: an event that races the list is absolute
        // and so harmless to apply twice.
        let events = client.subscribe().map_err(|e| e.to_string())?;
        let list = client.list().map_err(|e| e.to_string())?;
        self.infos = list.into_iter().map(|i| (i.id, i)).collect();
        self.events = Some(events);
        self.client = Some(client);
        self.listed = true;
        self.prune();
        Ok(())
    }

    fn drop_connection(&mut self) {
        self.client = None;
        self.events = None;
        for v in self.views.values_mut() {
            v.detach();
        }
    }

    /// Apply what the host reported; returns what the window should react to.
    pub fn tick(&mut self, now: f64) -> Vec<Change> {
        let mut out = Vec::new();
        // Reconnect only when something is on screen to need it.
        if self.client.is_none() && !self.workspaces.is_empty() && self.enabled() {
            // A failure is kept in `self.error` and drawn by the panes.
            drop(self.client());
        }
        let mut relist = false;
        if let Some(events) = &mut self.events {
            if events.lagged() {
                relist = true;
            }
            while let Some(ev) = events.try_next() {
                let id = ev.terminal();
                match ev {
                    Event::Created { info, .. } => {
                        self.infos.insert(id, *info);
                    }
                    Event::Removed { .. } => {
                        self.infos.remove(&id);
                        self.views.remove(&id);
                    }
                    Event::Bell { .. } => out.push(Change::Bell(id)),
                    other => {
                        let Some(info) = self.infos.get_mut(&id) else {
                            relist = true;
                            continue;
                        };
                        match other {
                            Event::Exited {
                                code, signal, at, ..
                            } => {
                                info.state = State::Exited {
                                    code,
                                    signal,
                                    at,
                                    lost: false,
                                };
                                info.status = None;
                                info.foreground = None;
                                out.push(Change::Exited {
                                    terminal: id,
                                    clean: code == Some(0),
                                });
                            }
                            Event::Title { title, .. } => info.title = title,
                            Event::Cwd { cwd, .. } => info.cwd = cwd,
                            Event::Status { status, .. } => info.status = status,
                            Event::Renamed { name, .. } => info.name = name,
                            Event::Labels { labels, .. } => info.labels = labels,
                            Event::Size { cols, rows, .. } => {
                                info.cols = cols;
                                info.rows = rows;
                            }
                            Event::Foreground { name, .. } => info.foreground = name,
                            _ => {}
                        }
                    }
                }
            }
        }
        if relist
            && let Some(c) = &self.client
            && let Ok(list) = c.list()
        {
            self.infos = list.into_iter().map(|i| (i.id, i)).collect();
        }
        self.views
            .retain(|id, v| self.infos.contains_key(id) || v.last_drawn + 1.0 > now);
        for v in self.views.values_mut() {
            if v.last_drawn + DETACH_AFTER_SECS < now {
                v.detach();
            }
        }
        out
    }

    /// Drop panes whose terminal the host no longer runs.
    fn prune(&mut self) {
        if !self.listed {
            return;
        }
        let alive: HashSet<u32> = self
            .infos
            .values()
            .filter(|i| i.state.is_running())
            .map(|i| i.id)
            .collect();
        let before = self.workspaces.clone();
        self.workspaces = std::mem::take(&mut self.workspaces)
            .into_iter()
            .filter_map(|mut w| {
                w.root = w.root.retain(&|id| alive.contains(&id))?;
                if !w.root.contains(w.focus) {
                    w.focus = *w.root.leaves().first()?;
                }
                Some(w)
            })
            .collect();
        if before != self.workspaces {
            self.save();
        }
    }

    pub fn save(&self) {
        let saved = Saved {
            next: self.next_workspace,
            workspaces: self.workspaces.clone(),
        };
        let path = self.state_dir.join("panes.json");
        let tmp = self.state_dir.join("panes.json.tmp");
        let res = serde_json::to_vec(&saved)
            .map_err(std::io::Error::other)
            .and_then(|body| {
                std::fs::create_dir_all(&self.state_dir)?;
                std::fs::write(&tmp, body)?;
                std::fs::rename(&tmp, &path)
            });
        if let Err(e) = res {
            eprintln!("artalde: panes: {e}");
        }
    }

    pub fn workspace(&self, id: u32) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn workspace_mut(&mut self, id: u32) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }

    /// The workspace showing `terminal`.
    pub fn workspace_of(&self, terminal: u32) -> Option<u32> {
        self.workspaces
            .iter()
            .find(|w| w.root.contains(terminal))
            .map(|w| w.id)
    }

    /// A new workspace holding just `terminal`; `home` makes it the one
    /// layouts show.
    pub fn add_workspace(&mut self, terminal: u32, home: bool) -> u32 {
        let id = if home {
            HOME_WORKSPACE
        } else {
            self.next_workspace += 1;
            self.next_workspace - 1
        };
        self.workspaces.retain(|w| w.id != id);
        self.workspaces.push(Workspace {
            id,
            root: Pane::Leaf(terminal),
            focus: terminal,
            zoomed: false,
        });
        self.save();
        id
    }

    /// Put `new` beside the focused pane of `workspace`.
    pub fn split(&mut self, workspace: u32, new: u32, dir: Dir) {
        if let Some(w) = self.workspace_mut(workspace) {
            let at = w.focus;
            if w.root.split(at, new, dir) {
                w.focus = new;
                w.zoomed = false;
            }
        }
        self.save();
    }

    /// Take `terminal` out of its workspace. Returns the workspace when
    /// that emptied it.
    pub fn remove_pane(&mut self, terminal: u32) -> Option<u32> {
        let ws = self.workspace_of(terminal)?;
        let i = self.workspaces.iter().position(|w| w.id == ws)?;
        let mut w = self.workspaces.remove(i);
        let emptied = match w.root.clone().without(terminal) {
            Some(root) => {
                w.root = root;
                w.zoomed = false;
                if w.focus == terminal {
                    w.focus = w.root.leaves().first().copied().unwrap_or(terminal);
                }
                self.workspaces.insert(i, w);
                None
            }
            None => Some(ws),
        };
        self.views.remove(&terminal);
        self.save();
        emptied
    }

    /// The view of `terminal`, made on first use, with the host to attach
    /// it to.
    pub fn view(&mut self, terminal: u32) -> Result<(Option<&Client>, &mut TermView), String> {
        if let std::collections::hash_map::Entry::Vacant(slot) = self.views.entry(terminal) {
            slot.insert(TermView::new(terminal)?);
        }
        let client = self.client.as_ref().filter(|c| c.is_connected());
        let view = self
            .views
            .get_mut(&terminal)
            .ok_or_else(|| "no view".to_owned())?;
        Ok((client, view))
    }

    pub fn run(&mut self, options: &RunOptions) -> Result<u32, String> {
        let info = self.client()?.run(options).map_err(|e| e.to_string())?;
        let id = info.id;
        self.infos.insert(id, info);
        Ok(id)
    }

    /// End a terminal's program and forget the terminal.
    pub fn end(&mut self, terminal: u32) {
        self.views.remove(&terminal);
        self.bells.remove(&terminal);
        let Ok(client) = self.client() else { return };
        let res = client.remove(terminal, true);
        match res {
            Ok(()) | Err(ClientError::Host(_)) => {
                self.infos.remove(&terminal);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn send(&mut self, terminal: u32, text: &str, submit: bool) -> Result<(), String> {
        self.client()?
            .send(terminal, text, submit)
            .map_err(|e| e.to_string())
    }

    /// Raw bytes, as if typed.
    pub fn write(&mut self, terminal: u32, bytes: &[u8]) -> Result<(), String> {
        if let Some(v) = self.views.get(&terminal)
            && v.write(bytes)
        {
            return Ok(());
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        self.send(terminal, &text, false)
    }

    /// Whether closing `terminal` would interrupt something: a program
    /// other than the shell it started with is in the foreground.
    pub fn busy(&self, terminal: u32) -> bool {
        let Some(info) = self.infos.get(&terminal) else {
            return false;
        };
        if !info.state.is_running() {
            return false;
        }
        let shell = info
            .argv
            .first()
            .map(|a| a.rsplit('/').next().unwrap_or(a).trim_start_matches('-'))
            .unwrap_or_default();
        match info.foreground.as_deref() {
            None => false,
            Some(f) => {
                let f = f.trim_start_matches('-');
                !(f == shell || is_shell(f) && (shell.is_empty() || is_shell(shell)))
            }
        }
    }
}

fn is_shell(name: &str) -> bool {
    matches!(name, "zsh" | "bash" | "fish" | "sh" | "nu" | "login")
}

/// What to call a terminal in a tab or header.
pub fn display_name(info: &TerminalInfo) -> String {
    if !info.name.is_empty() && !info.name.starts_with("term-") {
        return info.name.clone();
    }
    if let Some(t) = info.title.as_deref().filter(|t| !t.trim().is_empty()) {
        return t.trim().to_owned();
    }
    if let Some(f) = info.foreground.as_deref().filter(|f| !f.is_empty()) {
        return f.trim_start_matches('-').to_owned();
    }
    info.argv
        .first()
        .map(|a| a.rsplit('/').next().unwrap_or(a).to_owned())
        .unwrap_or_else(|| "shell".into())
}

/// `~/src/x` for a path under `$HOME`.
pub fn short_path(path: &str) -> String {
    match artalde_core::home() {
        Some(h) => match std::path::Path::new(path).strip_prefix(&h) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.to_owned(),
        },
        None => path.to_owned(),
    }
}
