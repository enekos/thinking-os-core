//! The window: state, commands, keys, the palette and the spawn dialog.
//! Panels live in `panels.rs`; the layout tree in `layout_view.rs`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use artalde_core::command::{COMMANDS, Command};
use artalde_core::config::Config;
use artalde_core::inbox;
use artalde_core::keys::Chord;
use artalde_core::layout::Panel;
use artalde_core::model::{AgentState, ResourceKind, Session, Snapshot};
use artalde_core::store::Store;
use artalde_core::tags::{self, Tags};
use artalde_core::text::clip;
use artalde_core::theme::Theme;
use artalde_mods::{Action, Context as ModContext, ModHost, ViewState};
use eframe::egui::{self, Key, Modifiers};

use crate::actions::{Actions, Job};
use crate::feed::{Feed, Msg};
use crate::gh::Gh;
use crate::layout_view::{self, LayoutState};
use crate::lifecycle_view::{CloseConfirm, SpawnForm};
use crate::palette::{PaletteEntry, PaletteKind, PaletteQuery};
use crate::panels::Lane;
use crate::views::{Conversations, InboxState, Logs, PrView, TAB_BAR, Tab};
use crate::{fonts, kit, mod_view, style};
use artalde_core::github::{self, PrRef};
use artalde_core::lifecycle::{ClosedLog, Launch};

const LOG_LINES: usize = 500;
const TOAST_SECS: f64 = 4.0;
const MAX_TOASTS: usize = 4;
const PALETTE_ROWS: usize = 9;
const PALETTE_RECENTS: usize = 8;

pub struct App {
    pub(crate) feed: Feed,
    pub(crate) actions: Actions,
    pub(crate) config: Option<Config>,
    pub(crate) snap: Snapshot,
    pub(crate) theme: Theme,
    applied_style: Option<(String, u32)>,
    pub(crate) layout_name: Option<String>,
    pub(crate) theme_name: Option<String>,
    layout_state: LayoutState,
    pub(crate) mods: ModHost,
    mod_ctx: Option<(ModCtxKey, ModContext)>,
    ctx_version: u64,
    pub(crate) selected: Option<String>,
    pub(crate) filter: Option<String>,
    /// Done sessions the user has seen: id → the `since` they were seen at.
    acked: HashMap<String, u64>,
    pub(crate) tags: Tags,
    /// The tab colour last sent to each session's terminal.
    tab_colors: HashMap<String, (String, Option<String>)>,
    pub(crate) label_edit: String,
    pub(crate) settings_draft: Option<artalde_core::config::Settings>,
    /// The saved settings the draft started from, to tell an untouched
    /// draft (follows outside changes) from an edited one (kept).
    pub(crate) settings_base: Option<artalde_core::config::Settings>,
    pub(crate) hook_confirm: bool,
    pub(crate) reviewer: crate::reviewer::Reviewer,
    /// A review to start next frame: PR and agent index.
    pub(crate) queue_review: Option<(PrRef, usize)>,
    pub(crate) lane: Lane,
    pub(crate) closed_open: bool,
    pub(crate) agent_query: String,
    pub(crate) spaces_closed: Vec<String>,
    /// The session `label_edit` was filled from.
    pub(crate) label_for: Option<String>,
    prev_states: HashMap<String, AgentState>,
    have_snapshot: bool,
    pub(crate) log: VecDeque<String>,
    toasts: VecDeque<(String, f64)>,
    pub(crate) queue: Vec<Command>,
    palette: Option<Palette>,
    palette_recents: Vec<Command>,
    pub(crate) spawn: Option<SpawnForm>,
    /// Agents started from here that have not shown up yet.
    pub(crate) launches: Vec<Launch>,
    pub(crate) closed: ClosedLog,
    /// Sessions being closed: id → when, hidden until their process exits.
    pub(crate) closing: HashMap<String, u64>,
    pub(crate) close_confirm: Option<CloseConfirm>,
    pub(crate) egui_ctx: egui::Context,
    /// `artalde drive`: log every side effect (terminal, process, gh write,
    /// review agent) instead of doing it.
    pub(crate) dry_run: bool,
    pub(crate) reply: String,
    pub(crate) hide_done: bool,
    pub(crate) shot: Option<Shot>,
    pub(crate) gh: Gh,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    pub(crate) prs: HashMap<PrRef, PrView>,
    pub(crate) repos: HashMap<String, crate::repo_view::RepoView>,
    pub(crate) inbox: InboxState,
    pub(crate) logs: Logs,
    pub(crate) confirm: Option<(PrRef, github::Action)>,
    pub(crate) conversations: Conversations,
}

#[derive(Clone, PartialEq, Eq)]
struct ModCtxKey {
    snap: u64,
    selected: Option<String>,
    filter: Option<String>,
    theme: String,
}

#[derive(Default)]
struct Palette {
    query: String,
    cursor: usize,
}

/// `ARTALDE_SCREENSHOT=<png>`: save one frame once data is in, then quit.
pub(crate) struct Shot {
    path: PathBuf,
    requested: bool,
    done: Arc<AtomicBool>,
    ready_at: Option<f64>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        store: Store,
        config_dir: PathBuf,
    ) -> anyhow::Result<App> {
        let feed = Feed::start(&cc.egui_ctx, store, config_dir)?;
        let actions = Actions::start(&cc.egui_ctx)?;
        let gh = Gh::start(&cc.egui_ctx)?;
        fonts::install(&cc.egui_ctx);
        let (tabs, active) = App::load_tabs();
        let theme = Theme::graphite();
        style::apply(&cc.egui_ctx, &theme, 13.5);
        Ok(App {
            feed,
            actions,
            config: None,
            snap: Snapshot::default(),
            theme,
            applied_style: None,
            layout_name: std::env::var("ARTALDE_LAYOUT").ok(),
            theme_name: std::env::var("ARTALDE_THEME").ok(),
            layout_state: LayoutState::default(),
            mods: ModHost::new(),
            mod_ctx: None,
            ctx_version: 0,
            selected: None,
            filter: None,
            acked: HashMap::new(),
            tags: Tags::load(&artalde_core::store::state_dir()),
            tab_colors: HashMap::new(),
            label_edit: String::new(),
            settings_draft: None,
            settings_base: None,
            hook_confirm: false,
            reviewer: crate::reviewer::Reviewer::new(),
            queue_review: None,
            lane: Lane::default(),
            closed_open: false,
            agent_query: String::new(),
            spaces_closed: Vec::new(),
            label_for: None,
            prev_states: HashMap::new(),
            have_snapshot: false,
            log: VecDeque::new(),
            toasts: VecDeque::new(),
            queue: Vec::new(),
            palette: None,
            palette_recents: Vec::new(),
            spawn: None,
            launches: Vec::new(),
            closed: ClosedLog::load(&artalde_core::store::state_dir()),
            closing: HashMap::new(),
            close_confirm: None,
            egui_ctx: cc.egui_ctx.clone(),
            dry_run: std::env::var_os("ARTALDE_DRY_RUN").is_some(),
            reply: String::new(),
            hide_done: true,
            gh,
            tabs,
            active,
            prs: HashMap::new(),
            repos: HashMap::new(),
            inbox: InboxState::default(),
            logs: Logs::new(),
            confirm: None,
            conversations: Conversations::new(),
            shot: std::env::var_os("ARTALDE_SCREENSHOT").map(|p| Shot {
                path: PathBuf::from(p),
                requested: false,
                done: Arc::new(AtomicBool::new(false)),
                ready_at: None,
            }),
        })
    }

    pub(crate) fn push_log(&mut self, line: impl Into<String>) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        let secs = artalde_core::now() % 86_400;
        self.log.push_back(format!(
            "{:02}:{:02}:{:02}  {}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60,
            line.into()
        ));
    }

    /// Linear team keys from the wiki's os.yaml.
    pub(crate) fn linear_teams(&self) -> Vec<String> {
        artalde_core::catalog::linear_teams(&artalde_core::config::wiki_root())
    }

    pub(crate) fn palette_open(&self) -> bool {
        self.palette.is_some()
            || self.spawn.is_some()
            || self.confirm.is_some()
            || self.close_confirm.is_some()
    }

    pub(crate) fn toast_line(&mut self, ctx: &egui::Context, text: String) {
        self.toast(ctx, text);
    }

    /// Whether the current layout shows the built-in panel `name`.
    pub(crate) fn layout_shows(&self, name: &str) -> bool {
        let Some(cfg) = &self.config else {
            return false;
        };
        let Some(l) = cfg
            .layout(&self.current_layout_name())
            .or_else(|| cfg.layouts.first())
        else {
            return false;
        };
        fn walk(n: &artalde_core::layout::Node, name: &str) -> bool {
            use artalde_core::layout::Node;
            match n {
                Node::Split { children, .. } => children.iter().any(|(_, c)| walk(c, name)),
                Node::Stack { children, .. } => children.iter().any(|c| walk(c, name)),
                Node::Block {
                    panel: Panel::Builtin(b),
                    ..
                } => b == name,
                Node::Block { .. } => false,
            }
        }
        walk(&l.root, name)
    }

    pub(crate) fn mod_ctx_dirty(&mut self) {
        self.mod_ctx = None;
    }

    fn toast(&mut self, ctx: &egui::Context, text: String) {
        let until = ctx.input(|i| i.time) + TOAST_SECS;
        if self.toasts.len() == MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back((text, until));
    }

    pub(crate) fn settings(&self) -> artalde_core::config::Settings {
        self.config
            .as_ref()
            .map(|c| c.settings.clone())
            .unwrap_or_default()
    }

    /// Sessions in the order the agents panel shows them.
    pub(crate) fn visible_sessions(&self) -> Vec<&Session> {
        self.snap
            .sessions
            .iter()
            .filter(|s| {
                self.filter
                    .as_ref()
                    .is_none_or(|f| s.repo.as_ref() == Some(f))
            })
            .collect()
    }

    pub(crate) fn selected_session(&self) -> Option<&Session> {
        self.selected
            .as_deref()
            .and_then(|id| self.snap.session(id))
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.feed.rx.try_recv() {
            match msg {
                Msg::Log(line) => self.push_log(line),
                Msg::Config(cfg) => {
                    for e in &cfg.errors {
                        self.toast(ctx, format!("config: {e}"));
                    }
                    self.mods.sync(&cfg.mods);
                    self.config = Some(*cfg);
                    self.mod_ctx = None;
                }
                Msg::Snapshot(snap) => self.receive(ctx, *snap),
            }
        }
        while let Ok(line) = self.actions.results.try_recv() {
            self.push_log(line);
        }
    }

    fn receive(&mut self, ctx: &egui::Context, mut snap: Snapshot) {
        let mut events = Vec::new();
        for s in &snap.sessions {
            let before = self.prev_states.get(&s.rec.id).copied();
            let now = s.rec.state;
            if self.have_snapshot
                && before != Some(now)
                && matches!(now, AgentState::Blocked | AgentState::Done)
            {
                let what = match now {
                    AgentState::Blocked => {
                        s.rec.message.clone().unwrap_or_else(|| "needs you".into())
                    }
                    _ => "finished".into(),
                };
                events.push((s.name(), what, s.repo.clone().unwrap_or_default()));
            }
        }
        let fresh: HashSet<String> = if self.have_snapshot {
            snap.sessions
                .iter()
                .filter(|s| !self.prev_states.contains_key(&s.rec.id))
                .map(|s| s.rec.id.clone())
                .collect()
        } else {
            HashSet::new()
        };
        self.prev_states = snap
            .sessions
            .iter()
            .map(|s| (s.rec.id.clone(), s.rec.state))
            .collect();
        self.acked.retain(|id, _| snap.session(id).is_some());
        self.tab_colors.retain(|id, _| snap.session(id).is_some());
        for s in &mut snap.sessions {
            if s.rec.state == AgentState::Done && self.acked.get(&s.rec.id) == Some(&s.rec.since) {
                s.rec.state = AgentState::Idle;
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| snap.session(id).is_none())
        {
            self.selected = None;
        }
        if self.selected.is_none() && std::env::var_os("ARTALDE_SCREENSHOT").is_some() {
            self.selected = snap.sessions.first().map(|s| s.rec.id.clone());
        }
        self.snap = snap;
        self.lifecycle_receive(&fresh);
        self.apply_tags();
        if !self.have_snapshot && self.shot.is_some() {
            let command = match std::env::var("ARTALDE_SCREENSHOT_OVERLAY").as_deref() {
                Ok("palette") => Some("palette.open"),
                Ok("spawn") => Some("spawn.open"),
                Ok("sweep") => Some("session.sweep"),
                _ => None,
            };
            if let Some(command) = command {
                self.queue.push(Command::new(command, None));
            }
        }
        self.have_snapshot = true;
        for (name, what, repo) in events {
            let line = format!(
                "{} · {name}: {what}",
                if repo.is_empty() { "agent" } else { &repo }
            );
            self.push_log(line.clone());
            self.toast(ctx, line);
        }
    }

    pub(crate) fn submit_job(&mut self, job: Job) {
        self.submit(job);
    }

    fn submit(&mut self, job: Job) {
        if self.dry_run {
            self.push_log(format!("dry run: {job:?}"));
            return;
        }
        if let Err(e) = self.actions.submit(job) {
            self.push_log(e);
        }
    }

    fn ack(&mut self, id: &str) {
        if let Some(s) = self.snap.sessions.iter_mut().find(|s| s.rec.id == id) {
            self.acked.insert(id.to_owned(), s.rec.since);
            if s.rec.state == AgentState::Done {
                s.rec.state = AgentState::Idle;
                self.mod_ctx = None;
            }
        }
    }

    fn select(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
        self.ack(id);
    }

    fn target_id(&self, arg: Option<&str>) -> Option<String> {
        arg.filter(|a| !a.is_empty())
            .map(str::to_owned)
            .or_else(|| self.selected.clone())
    }

    fn tty_of(&self, id: &str) -> Option<String> {
        self.snap.session(id).and_then(|s| s.rec.tty.clone())
    }

    fn type_into(&mut self, text: String, enter: bool) {
        let Some(id) = self.selected.clone() else {
            return self.push_log("send: select a session first");
        };
        match self.tty_of(&id) {
            Some(tty) => self.submit(Job::Type { tty, text, enter }),
            None => self.push_log("send: that session has no terminal"),
        }
    }

    /// Copy tags onto the snapshot's sessions and mirror colours onto
    /// their terminal tabs.
    fn apply_tags(&mut self) {
        let mut jobs = Vec::new();
        for s in &mut self.snap.sessions {
            let tag = self.tags.get(&s.rec.id);
            s.tag = tag.and_then(|t| t.color.clone());
            s.label = tag.and_then(|t| t.label.clone());
            let Some(tty) = s.rec.tty.clone() else {
                continue;
            };
            let want = (tty.clone(), s.tag.clone());
            let sent = self.tab_colors.get(&s.rec.id);
            // Untagged sessions are left alone unless artalde coloured them.
            if sent != Some(&want) && (s.tag.is_some() || sent.is_some()) {
                let rgb = s
                    .tag
                    .as_deref()
                    .and_then(tags::palette_color)
                    .map(|c| [c[0], c[1], c[2]]);
                jobs.push(Job::TabColor { tty, rgb });
                self.tab_colors.insert(s.rec.id.clone(), want);
            }
        }
        // A screenshot run never touches real terminals.
        if self.shot.is_none() {
            for job in jobs {
                self.submit(job);
            }
        }
        self.mod_ctx = None;
    }

    fn retag(&mut self, f: impl FnOnce(&mut Tags) -> Result<(), String>) {
        match f(&mut self.tags).and_then(|()| {
            self.tags
                .save(artalde_core::now())
                .map_err(|e| e.to_string())
        }) {
            Ok(()) => self.apply_tags(),
            Err(e) => self.push_log(format!("tag: {e}")),
        }
    }

    pub(crate) fn run(&mut self, ctx: &egui::Context, cmd: &Command) {
        let arg = cmd.arg.as_deref();
        match cmd.name.as_str() {
            "palette.open" => self.palette = Some(Palette::default()),
            "agent.next" => {
                let wanting: Vec<String> = self
                    .snap
                    .sessions
                    .iter()
                    .filter(|s| matches!(s.rec.state, AgentState::Blocked | AgentState::Done))
                    .map(|s| s.rec.id.clone())
                    .collect();
                let pos = self
                    .selected
                    .as_ref()
                    .and_then(|id| wanting.iter().position(|w| w == id));
                let next = pos.map_or(0, |p| p + 1) % wanting.len().max(1);
                match wanting.get(next).cloned() {
                    Some(id) => self.select(&id),
                    None => self.toast(ctx, "nothing needs you".into()),
                }
            }
            "agent.select" => {
                let id = match arg.and_then(|a| a.parse::<usize>().ok()) {
                    Some(n) => self
                        .visible_sessions()
                        .get(n.wrapping_sub(1))
                        .map(|s| s.rec.id.clone()),
                    None => arg.map(str::to_owned),
                };
                if let Some(id) = id {
                    self.select(&id);
                }
            }
            "session.focus" => {
                if let Some(id) = self.target_id(arg) {
                    self.ack(&id);
                    match self.tty_of(&id) {
                        Some(tty) => self.submit(Job::Focus { tty }),
                        None => self.push_log("focus: that session has no terminal"),
                    }
                }
            }
            "session.tag" => {
                // `<colour>` for the selected session, or `<id>:<colour>`.
                let (id, color) = match arg.and_then(|a| a.rsplit_once(':')) {
                    Some((id, c)) => (Some(id.to_owned()), Some(c)),
                    None => (self.selected.clone(), arg),
                };
                match id {
                    Some(id) => self.retag(|t| t.set_color(&id, color, artalde_core::now())),
                    None => self.push_log("tag: select a session first"),
                }
            }
            "session.label" => match self.selected.clone() {
                Some(id) => {
                    let label = arg.unwrap_or_default().to_owned();
                    self.retag(|t| {
                        t.set_label(&id, &label, artalde_core::now());
                        Ok(())
                    });
                }
                None => self.push_log("label: select a session first"),
            },
            "session.ack" => {
                if let Some(id) = self.target_id(arg) {
                    self.ack(&id);
                }
            }
            "session.send" => {
                let text = arg.unwrap_or_default().to_owned();
                self.type_into(text, true);
            }
            "session.key" => match arg {
                Some(k @ ("1" | "2" | "3")) => self.type_into(k.to_owned(), false),
                Some("enter") => self.type_into(String::new(), true),
                Some("esc") => self.type_into("<esc>".into(), false),
                _ => self.push_log("session.key: 1, 2, 3, enter or esc"),
            },
            "space.filter" => {
                self.filter = arg.filter(|a| !a.is_empty()).map(str::to_owned);
            }
            "spawn.open" => self.open_spawn(arg),
            "session.stop" => {
                if let Some(id) = self.target_id(arg) {
                    self.stop_session(&id);
                }
            }
            "session.wrap" => {
                if let Some(id) = self.target_id(arg) {
                    self.wrap_session(&id);
                }
            }
            "session.close" => {
                if let Some(id) = self.target_id(arg) {
                    let title = self
                        .snap
                        .session(&id)
                        .map_or_else(String::new, |s| format!("Close {}?", clip(&s.name(), 48)));
                    self.request_close(vec![id], title);
                }
            }
            "session.sweep" => self.sweep(),
            "session.reopen" => self.reopen(arg.filter(|a| !a.is_empty())),
            "layout.select" => self.layout_name = arg.map(str::to_owned),
            "layout.cycle" => {
                if let Some(cfg) = &self.config {
                    let names: Vec<&str> = cfg.layouts.iter().map(|l| l.name.as_str()).collect();
                    let cur = self.current_layout_name();
                    let i = names.iter().position(|n| *n == cur).map_or(0, |i| i + 1);
                    self.layout_name = names.get(i % names.len().max(1)).map(|s| (*s).to_owned());
                }
            }
            "theme.select" => self.theme_name = arg.map(str::to_owned),
            "theme.cycle" => {
                if let Some(cfg) = &self.config {
                    let names: Vec<&str> = cfg.themes.iter().map(|t| t.name.as_str()).collect();
                    let i = names
                        .iter()
                        .position(|n| *n == self.theme.name)
                        .map_or(0, |i| i + 1);
                    self.theme_name = names.get(i % names.len().max(1)).map(|s| (*s).to_owned());
                }
            }
            "inbox.toggle" if self.dry_run => {
                self.push_log(format!("dry run: inbox.toggle {}", arg.unwrap_or_default()));
            }
            "inbox.toggle" => {
                if let Some(item) = arg
                    .and_then(|a| a.parse::<usize>().ok())
                    .and_then(|i| self.snap.inbox.get(i))
                    .cloned()
                {
                    match inbox::set_done(&item, !item.done) {
                        Ok(()) => {
                            self.push_log(format!(
                                "{} {}",
                                if item.done { "unticked" } else { "ticked" },
                                item.target
                            ));
                            self.feed.refresh();
                        }
                        Err(e) => self.push_log(format!("inbox: {e}")),
                    }
                }
            }
            "url.open" => match arg {
                Some(u) if u.starts_with("https://") || u.starts_with("http://") => {
                    self.submit(Job::Open {
                        target: u.to_owned(),
                    });
                }
                _ => self.push_log("url.open: only http(s) URLs"),
            },
            "path.edit" => match arg.map(PathBuf::from) {
                Some(p) if p.is_absolute() && p.exists() => {
                    let app = self.settings().editor;
                    self.submit(Job::OpenWith {
                        app,
                        target: p.display().to_string(),
                    });
                }
                _ => self.push_log("path.edit: no such folder"),
            },
            "path.open" => match arg.map(PathBuf::from) {
                Some(p) if p.is_absolute() && p.exists() => self.submit(Job::Open {
                    target: p.display().to_string(),
                }),
                _ => self.push_log("path.open: no such folder"),
            },
            "config.open" => {
                let dir = artalde_core::config::config_dir().display().to_string();
                self.submit(Job::Open { target: dir });
            }
            "refresh" => {
                self.feed.refresh();
                self.refresh_inbox();
                if let Some(Tab::Pr(pr)) = self.tabs.get(self.active).cloned() {
                    self.load_pr(&pr);
                }
            }
            "tab.close" => {
                let i = arg.and_then(|a| a.parse().ok()).unwrap_or(self.active);
                self.close_tab(i);
            }
            "tab.next" => self.active = (self.active + 1) % self.tabs.len().max(1),
            "tab.prev" => {
                self.active = (self.active + self.tabs.len().max(1) - 1) % self.tabs.len().max(1)
            }
            "tab.select" => {
                if let Some(n) = arg.and_then(|a| a.parse::<usize>().ok()) {
                    self.active = n.saturating_sub(1).min(self.tabs.len().saturating_sub(1));
                }
            }
            "inbox.open" => self.open_tab(Tab::Inbox),
            "repo.open" => match arg.and_then(crate::repo_view::parse_repo) {
                Some(r) => self.open_tab(Tab::Repo(r)),
                None => self.push_log("repo.open: owner/repo or a GitHub URL"),
            },
            "repo.refresh" => {
                let repo = arg
                    .and_then(crate::repo_view::parse_repo)
                    .or_else(|| self.palette_current_repo());
                match repo {
                    Some(repo) => {
                        self.load_repo(&repo);
                        self.load_repo_activity(&repo);
                    }
                    None => self.push_log("repo.refresh: open a repository first"),
                }
            }
            "pr.review" => {
                if let Some(Tab::Pr(pr)) = self.tabs.get(self.active).cloned() {
                    let agent = self.prs.get(&pr).map_or(0, |v| v.agent);
                    if let Some(v) = self.prs.get_mut(&pr) {
                        v.sub = crate::views::PrSub::Review;
                    }
                    if !self.reviewer.runs.contains_key(&pr) {
                        self.queue_review = Some((pr, agent));
                    }
                }
            }
            "settings.open" => {
                self.settings_draft = None;
                self.open_tab(Tab::Settings);
            }
            "appearance.toggle" | "appearance.set" => {
                if let Some(cfg) = &self.config {
                    let mut s = cfg.settings.clone();
                    s.appearance = match (cmd.name.as_str(), arg) {
                        ("appearance.set", Some(m @ ("system" | "light" | "dark"))) => m.to_owned(),
                        _ => {
                            if self.theme.dark {
                                "light".into()
                            } else {
                                "dark".into()
                            }
                        }
                    };
                    self.theme_name = None;
                    let dir = cfg.dir.clone();
                    if let Err(e) = artalde_core::config::save_settings(&dir, &s) {
                        self.push_log(format!("appearance: {e}"));
                    }
                    if let Some(c) = &mut self.config {
                        c.settings = s;
                    }
                }
            }
            "pr.open" => match arg.and_then(PrRef::parse) {
                Some(pr) => self.open_tab(Tab::Pr(pr)),
                None => self.push_log("pr.open: owner/repo#N or a PR URL"),
            },
            "session.open" => {
                if let Some(id) = self.target_id(arg) {
                    self.select(&id);
                    self.open_tab(Tab::Session(id));
                }
            }
            other => self.push_log(format!("unknown command {other:?}")),
        }
    }

    pub(crate) fn current_layout_name(&self) -> String {
        self.layout_name
            .clone()
            .unwrap_or_else(|| self.settings().layout)
    }

    /// `(label, path, repo)` of every known worktree.
    pub(crate) fn worktree_choices(&self) -> Vec<(String, PathBuf, String)> {
        self.snap
            .spaces
            .iter()
            .flat_map(|sp| {
                sp.worktrees.iter().map(|w| {
                    let branch = w.branch.clone().unwrap_or_else(|| "detached".into());
                    (
                        format!("{} · {branch}", sp.repo),
                        w.path.clone(),
                        sp.repo.clone(),
                    )
                })
            })
            .collect()
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if self.palette.is_some() || self.spawn.is_some() || self.close_confirm.is_some() {
            return;
        }
        let Some(cfg) = &self.config else { return };
        let mut fired = Vec::new();
        let ctrl_k_bound = cfg.keys.bindings.iter().any(|(chord, _)| {
            chord.ctrl && !chord.cmd && !chord.alt && !chord.shift && chord.key == "k"
        });
        if !ctrl_k_bound && ctx.input_mut(|input| input.consume_key(Modifiers::CTRL, Key::K)) {
            fired.push(Command::new("palette.open", None));
        }
        for (chord, cmd) in &cfg.keys.bindings {
            let Some(key) = Key::from_name(&chord.key) else {
                continue;
            };
            if ctx.input_mut(|i| i.consume_key(modifiers(chord), key)) {
                fired.push(cmd.clone());
            }
        }
        self.queue.extend(fired);
    }

    fn palette_current_repo(&self) -> Option<String> {
        match self.tabs.get(self.active) {
            Some(Tab::Repo(repo)) => Some(repo.clone()),
            Some(Tab::Pr(pr)) => Some(pr.repo.clone()),
            _ => None,
        }
    }

    fn palette_entries(&self) -> Vec<PaletteEntry> {
        let keys = self.config.as_ref().map(|c| &c.keys);
        let hint = |cmd: &str| {
            keys.and_then(|k| k.chord_for(cmd))
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        for (title, status, command) in self.lifecycle_entries() {
            let detail = command.arg.as_ref().map_or_else(
                || "Agent lifecycle".to_owned(),
                |id| format!("Agent lifecycle · {id}"),
            );
            out.push(
                PaletteEntry::new(PaletteKind::Action, title, detail, command)
                    .hint(status)
                    .keywords("agent close stop reopen stale"),
            );
        }
        if let Some(sel) = self.selected_session() {
            for (name, _) in tags::PALETTE {
                out.push(
                    PaletteEntry::new(
                        PaletteKind::Action,
                        format!("Tag {}: {name}", sel.name()),
                        "Selected agent",
                        Command::new("session.tag", Some(name)),
                    )
                    .keywords("color colour tag agent"),
                );
            }
            out.push(
                PaletteEntry::new(
                    PaletteKind::Action,
                    format!("Untag {}", sel.name()),
                    "Selected agent",
                    Command::new("session.tag", Some("none")),
                )
                .keywords("color colour tag agent clear"),
            );
        }
        let mut seen_prs = HashSet::new();
        for s in &self.snap.sessions {
            for r in &s.resources {
                let title = r.title.clone().unwrap_or_default();
                let (kind, command) = match r.kind {
                    ResourceKind::Pr => {
                        let command = PrRef::parse(&r.id)
                            .map(|pr| {
                                seen_prs.insert(pr.clone());
                                Command::new("pr.open", Some(&pr.to_string()))
                            })
                            .unwrap_or_else(|| Command::new("url.open", Some(&r.url)));
                        (PaletteKind::PullRequest, command)
                    }
                    ResourceKind::Ticket => {
                        (PaletteKind::Action, Command::new("url.open", Some(&r.url)))
                    }
                };
                out.push(
                    PaletteEntry::new(
                        kind,
                        format!("{} {title}", r.short()),
                        format!("{} · {}", r.id, s.name()),
                        command,
                    )
                    .hint(r.state.clone().unwrap_or_default())
                    .keywords(format!("{} {}", r.id, s.name())),
                );
            }
        }
        for s in &self.snap.sessions {
            let place = s.repo.clone().unwrap_or_else(|| s.rec.agent.clone());
            let boost = match s.rec.state {
                AgentState::Blocked => 2_500,
                AgentState::Done => 1_800,
                AgentState::Working => 700,
                _ => 200,
            };
            let detail = s
                .branch
                .as_ref()
                .map(|branch| format!("{place} · {branch}"))
                .unwrap_or(place);
            out.push(
                PaletteEntry::new(
                    PaletteKind::Agent,
                    s.name(),
                    detail,
                    Command::new("agent.select", Some(&s.rec.id)),
                )
                .hint(s.rec.state.as_str())
                .keywords(format!("agent {} {}", s.rec.agent, s.rec.id))
                .boost(boost),
            );
            if s.rec.tty.is_some() {
                out.push(
                    PaletteEntry::new(
                        PaletteKind::Agent,
                        format!("Focus terminal — {}", s.name()),
                        "Bring the session's terminal to the front",
                        Command::new("session.focus", Some(&s.rec.id)),
                    )
                    .hint("terminal")
                    .keywords(format!("agent focus {}", s.rec.id)),
                );
            }
        }
        for (name, desc) in COMMANDS {
            if desc.contains('<') {
                continue;
            }
            out.push(
                PaletteEntry::new(PaletteKind::Command, *desc, *name, Command::new(name, None))
                    .hint(hint(name))
                    .keywords(*name),
            );
        }
        if let Some(cfg) = &self.config {
            for l in &cfg.layouts {
                out.push(
                    PaletteEntry::new(
                        PaletteKind::Layout,
                        &l.title,
                        "Switch layout",
                        Command::new("layout.select", Some(&l.name)),
                    )
                    .hint(&l.name)
                    .keywords(format!("layout {}", l.name)),
                );
            }
            for t in &cfg.themes {
                out.push(
                    PaletteEntry::new(
                        PaletteKind::Theme,
                        &t.name,
                        "Switch theme",
                        Command::new("theme.select", Some(&t.name)),
                    )
                    .keywords(format!("theme {}", t.name)),
                );
            }
        }
        let mut repos: Vec<String> = self
            .inbox
            .buckets
            .iter()
            .flat_map(|(_, rows)| {
                rows.iter()
                    .filter_map(|r| r.pr.as_ref().map(|p| p.repo.clone()))
            })
            .chain(self.snap.prs.iter().map(|p| p.repo.clone()))
            .chain(self.snap.spaces.iter().filter_map(|s| s.slug.clone()))
            .collect();
        repos.sort();
        repos.dedup();
        for r in repos {
            let detail = self.repos.get(&r).and_then(|view| {
                view.detail.as_ref().map(|detail| {
                    format!(
                        "{} open PRs · {} environments",
                        detail.open.len(),
                        detail.environments().len()
                    )
                })
            });
            let local = self
                .snap
                .spaces
                .iter()
                .any(|space| space.slug.as_deref() == Some(&r));
            out.push(
                PaletteEntry::new(
                    PaletteKind::Repository,
                    &r,
                    detail.unwrap_or_else(|| "PRs · deployments · releases".into()),
                    Command::new("repo.open", Some(&r)),
                )
                .hint(if local { "local" } else { "GitHub" })
                .keywords("repo repository github")
                .boost(if self.palette_current_repo().as_deref() == Some(&r) {
                    1_500
                } else {
                    300
                }),
            );
        }
        for sp in &self.snap.spaces {
            out.push(
                PaletteEntry::new(
                    PaletteKind::Action,
                    format!("Filter agents: {}", sp.repo),
                    "Workspace",
                    Command::new("space.filter", Some(&sp.repo)),
                )
                .keywords("filter repo agents"),
            );
            out.push(
                PaletteEntry::new(
                    PaletteKind::Action,
                    format!("New agent in {}", sp.repo),
                    sp.root.display().to_string(),
                    Command::new("spawn.open", Some(&sp.repo)),
                )
                .keywords("spawn start agent worktree"),
            );
        }
        for (_, rows) in &self.inbox.buckets {
            for row in rows {
                let Some(pr) = row.pr.clone().filter(|pr| seen_prs.insert(pr.clone())) else {
                    continue;
                };
                out.push(
                    PaletteEntry::new(
                        PaletteKind::PullRequest,
                        format!("#{} {}", pr.number, row.title),
                        format!("{} · @{}", pr.repo, row.author),
                        Command::new("pr.open", Some(&pr.to_string())),
                    )
                    .hint(if row.checks.is_empty() {
                        row.state.clone()
                    } else {
                        row.checks.clone()
                    })
                    .keywords(format!("{} {} PR pull request", pr, row.head)),
                );
            }
        }
        if let Some(repo) = self.palette_current_repo() {
            out.push(
                PaletteEntry::new(
                    PaletteKind::Action,
                    format!("Open {repo} on GitHub"),
                    "Current repository",
                    Command::new("url.open", Some(&format!("https://github.com/{repo}"))),
                )
                .keywords("browser remote github")
                .boost(4_000),
            );
            out.push(
                PaletteEntry::new(
                    PaletteKind::Action,
                    "Reload current repository",
                    &repo,
                    Command::new("repo.refresh", None),
                )
                .hint(hint("repo.refresh"))
                .keywords("refresh reload pull requests deployments releases")
                .boost(4_000),
            );
            if let Some(path) = self.local_space(&repo) {
                let path = path.display().to_string();
                out.push(
                    PaletteEntry::new(
                        PaletteKind::Action,
                        format!("Open {repo} in editor"),
                        &path,
                        Command::new("path.edit", Some(&path)),
                    )
                    .keywords("local workspace code")
                    .boost(4_000),
                );
            }
        }
        let mut unique = HashMap::new();
        for entry in out {
            unique
                .entry(entry.command.clone())
                .and_modify(|current: &mut PaletteEntry| {
                    if entry.boost > current.boost {
                        *current = entry.clone();
                    }
                })
                .or_insert(entry);
        }
        let mut out: Vec<PaletteEntry> = unique.into_values().collect();
        for entry in &mut out {
            entry.recent = self
                .palette_recents
                .iter()
                .position(|command| *command == entry.command);
        }
        out
    }

    fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.palette.take() else {
            return;
        };
        let mut entries = self.palette_entries();
        let query = PaletteQuery::parse(&p.query);
        if query.scope == crate::palette::PaletteScope::All
            && let Some(repo) =
                crate::repo_view::parse_repo(&query.text).filter(|_| !query.text.contains('#'))
        {
            entries.insert(
                0,
                PaletteEntry::new(
                    PaletteKind::Repository,
                    format!("Open {repo}"),
                    "Repository · PRs, deployments and releases",
                    Command::new("repo.open", Some(&repo)),
                )
                .keywords("direct repository github")
                .boost(60_000),
            );
        }
        let direct_pr = PrRef::parse(&query.text).or_else(|| {
            (query.scope == crate::palette::PaletteScope::PullRequests
                && !query.text.is_empty()
                && query.text.chars().all(|c| c.is_ascii_digit()))
            .then(|| {
                self.palette_current_repo().and_then(|repo| {
                    PrRef::parse(&format!("{repo}#{}", query.text.trim_start_matches('#')))
                })
            })
            .flatten()
        });
        if let Some(pr) = direct_pr {
            entries.insert(
                0,
                PaletteEntry::new(
                    PaletteKind::PullRequest,
                    format!("Open pull request #{}", pr.number),
                    &pr.repo,
                    Command::new("pr.open", Some(&pr.to_string())),
                )
                .keywords(format!("{pr} direct github"))
                .boost(70_000),
            );
        }
        let scored = crate::palette::rank(entries, &p.query);
        let len = scored.len();
        let height = ctx.content_rect().height();
        let visible_rows = if height < 600.0 {
            4
        } else if height < 800.0 {
            7
        } else {
            PALETTE_ROWS
        };
        let (down, up, next_page, previous_page, home, end, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown)
                    || i.consume_key(Modifiers::CTRL, Key::N)
                    || i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::ArrowUp)
                    || i.consume_key(Modifiers::CTRL, Key::P)
                    || i.consume_key(Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::PageDown),
                i.consume_key(Modifiers::NONE, Key::PageUp),
                i.consume_key(Modifiers::NONE, Key::Home),
                i.consume_key(Modifiers::NONE, Key::End),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down && len > 0 {
            p.cursor = (p.cursor + 1) % len;
        }
        if up && len > 0 {
            p.cursor = (p.cursor + len - 1) % len;
        }
        if next_page {
            p.cursor = (p.cursor + visible_rows).min(len.saturating_sub(1));
        }
        if previous_page {
            p.cursor = p.cursor.saturating_sub(visible_rows);
        }
        if home {
            p.cursor = 0;
        }
        if end {
            p.cursor = len.saturating_sub(1);
        }
        let mut run = None;
        if enter {
            run = scored.get(p.cursor).map(|entry| entry.command.clone());
        }
        let theme = self.theme.clone();
        let modal = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 48.0).clamp(280.0, 740.0);
            ui.set_width(width);
            let w = ui.available_width();
            let edit = crate::kit::search(
                ui,
                &theme,
                &mut p.query,
                "Jump anywhere…  > commands  @ agents  # PRs",
                w,
            );
            edit.request_focus();
            if edit.changed() {
                p.cursor = 0;
            }
            ui.add_space(kit::S2);
            p.cursor = p.cursor.min(len.saturating_sub(1));
            let start = p.cursor.saturating_add(1).saturating_sub(visible_rows);
            let mut section = None;
            for (i, entry) in scored.iter().enumerate().skip(start).take(visible_rows) {
                let on = i == p.cursor;
                let next_section = entry.section(&query);
                if section != Some(next_section) {
                    section = Some(next_section);
                    crate::kit::text(
                        ui,
                        &next_section.to_uppercase(),
                        10.0,
                        style::c(theme.faint),
                    );
                    ui.add_space(2.0);
                }
                let icon = match entry.kind {
                    PaletteKind::Action | PaletteKind::Command => kit::Icon::ChevronRight,
                    PaletteKind::Agent => kit::Icon::Terminal,
                    PaletteKind::Repository => kit::Icon::Branch,
                    PaletteKind::PullRequest => kit::Icon::Editor,
                    PaletteKind::Layout => kit::Icon::More,
                    PaletteKind::Theme => kit::Icon::Sun,
                };
                let r = crate::kit::row(
                    ui,
                    &theme,
                    ("palette-entry", &entry.command),
                    kit::ROW2,
                    on,
                    &[
                        crate::kit::Col::Px(28.0),
                        crate::kit::Col::Flex,
                        crate::kit::Col::Px(116.0),
                    ],
                    |column, ui| match column {
                        0 => kit::icon(ui, icon, 16.0, style::c(theme.dim)),
                        1 => {
                            ui.vertical(|ui| {
                                let text = egui::RichText::new(&entry.title).size(13.0);
                                ui.add(
                                    egui::Label::new(if on { text.strong() } else { text })
                                        .truncate(),
                                );
                                if !entry.detail.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&entry.detail)
                                                .size(10.5)
                                                .color(style::c(theme.dim)),
                                        )
                                        .truncate(),
                                    );
                                }
                            });
                        }
                        _ => {
                            let color = AgentState::parse(&entry.hint)
                                .map_or(style::c(theme.dim), |s| style::state(&theme, s));
                            crate::kit::right(ui, &entry.hint, 10.5, color);
                        }
                    },
                )
                .on_hover_text(entry.command.to_string());
                if r.hovered() && ctx.input(|input| input.pointer.delta().length_sq() > 0.0) {
                    p.cursor = i;
                }
                if r.clicked() {
                    run = Some(entry.command.clone());
                }
            }
            if scored.is_empty() {
                ui.add_space(kit::S4);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No matching destination").strong());
                    ui.add_space(kit::S1);
                    crate::kit::text(
                        ui,
                        "Try owner/repo, owner/repo#123, > a command, or @ an agent",
                        11.5,
                        style::c(theme.dim),
                    );
                });
                ui.add_space(kit::S4);
            }
            ui.add_space(kit::S1);
            ui.separator();
            ui.horizontal(|ui| {
                crate::kit::text(
                    ui,
                    "Arrows navigate   Enter open   Esc close",
                    10.5,
                    style::c(theme.faint),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::kit::text(ui, &format!("{len} results"), 10.5, style::c(theme.faint));
                });
            });
        });
        if esc || modal.should_close() {
            return;
        }
        if let Some(cmd) = run {
            self.palette_recents.retain(|recent| *recent != cmd);
            self.palette_recents.insert(0, cmd.clone());
            self.palette_recents.truncate(PALETTE_RECENTS);
            self.queue.push(cmd);
            return;
        }
        self.palette = Some(p);
    }

    fn apply_config_style(&mut self, ctx: &egui::Context) {
        let Some(cfg) = &self.config else { return };
        let dark = match cfg.settings.appearance.as_str() {
            "light" => false,
            "dark" => true,
            _ => ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark),
        };
        let name = self.theme_name.clone().unwrap_or_else(|| {
            if dark {
                cfg.settings.theme.clone()
            } else {
                cfg.settings.light_theme.clone()
            }
        });
        let theme = cfg.theme(&name);
        let font = cfg.settings.font_size;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a font size key"
        )]
        let key = (format!("{theme:?}"), (font * 10.0) as u32);
        if self.applied_style.as_ref() != Some(&key) {
            style::apply(ctx, &theme, font);
            self.theme = theme;
            self.applied_style = Some(key);
            self.mod_ctx = None;
        }
    }

    pub(crate) fn mod_context(&mut self) -> ModContext {
        let key = ModCtxKey {
            snap: self.snap.version,
            selected: self.selected.clone(),
            filter: self.filter.clone(),
            theme: self.theme.name.clone(),
        };
        if let Some((k, c)) = &self.mod_ctx
            && *k == key
        {
            return c.clone();
        }
        self.ctx_version = self.ctx_version.wrapping_add(1);
        let c = artalde_mods::context(
            &self.snap,
            ViewState {
                selected: self.selected.as_deref(),
                filter: self.filter.as_deref(),
                theme: &self.theme.name,
            },
            self.ctx_version,
        );
        self.mod_ctx = Some((key, c.clone()));
        c
    }

    pub(crate) fn mod_panel(&mut self, ui: &mut egui::Ui, name: &str) {
        let ctx = self.mod_context();
        let rendered = self.mods.render(name, &ctx);
        match &rendered.node {
            Ok(node) => {
                let mut clicked = Vec::new();
                mod_view::show(ui, &self.theme, node, &mut clicked);
                for a in clicked {
                    match a {
                        Action::Command(c) => self.queue.push(c),
                        Action::Set(patch) => self.mods.apply(name, &patch),
                    }
                }
            }
            Err(e) => {
                ui.label(
                    egui::RichText::new(format!("mod {name}"))
                        .strong()
                        .color(style::c(self.theme.danger)),
                );
                ui.label(egui::RichText::new(e).monospace().small());
                ui.weak(format!(
                    "Fix mods/{name}.rhai — the window updates on save."
                ));
            }
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|(_, until)| *until > now);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let theme = &self.theme;
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (text, _) in &self.toasts {
                    egui::Frame::new()
                        .fill(style::c(theme.raised))
                        .stroke(egui::Stroke::new(1.0, style::c(theme.border)))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.label(text);
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        let now = ctx.input(|i| i.time);
        if shot.done.load(Ordering::Acquire) {
            println!("artalde: screenshot {}", shot.path.display());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        if !self.have_snapshot {
            return;
        }
        let wait = std::env::var("ARTALDE_SCREENSHOT_WAIT")
            .ok()
            .and_then(|w| w.parse::<f64>().ok())
            .unwrap_or(1.5);
        let ready = *shot.ready_at.get_or_insert(now + wait);
        if shot.requested || now < ready {
            return;
        }
        shot.requested = true;
        let path = shot.path.clone();
        let done = Arc::clone(&shot.done);
        let repaint = ctx.clone();
        ctx.request_screenshot(move |image| {
            let [w, h] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Err(e) = save_png(&path, &rgba, w, h) {
                eprintln!("artalde: screenshot: {e:#}");
            }
            done.store(true, Ordering::Release);
            repaint.request_repaint();
        });
    }
}

pub(crate) fn save_png(
    path: &std::path::Path,
    rgba: &[u8],
    w: usize,
    h: usize,
) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, u32::try_from(w)?, u32::try_from(h)?);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn modifiers(c: &Chord) -> Modifiers {
    let mut m = Modifiers::NONE;
    if c.cmd {
        m |= Modifiers::COMMAND;
    }
    if c.ctrl {
        m |= Modifiers::CTRL;
    }
    if c.alt {
        m |= Modifiers::ALT;
    }
    if c.shift {
        m |= Modifiers::SHIFT;
    }
    m
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        self.drain_github(&ctx);
        self.drain_reviews(&ctx);
        if let Some((pr, agent)) = self.queue_review.take() {
            self.start_agent_review(&ctx, &pr, agent);
        }
        self.apply_config_style(&ctx);
        self.keys(&ctx);
        self.tick_github();

        let full = ui.max_rect();
        ui.painter().rect_filled(
            full,
            egui::CornerRadius::ZERO,
            style::c(self.theme.background),
        );
        let bar = egui::Rect::from_min_size(
            full.min + egui::vec2(8.0, 4.0),
            egui::vec2(full.width() - 16.0, TAB_BAR),
        );
        self.tab_bar(ui, bar);
        let rect = egui::Rect::from_min_max(egui::pos2(full.min.x, bar.max.y), full.max);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        let page = egui::Rect::from_min_max(
            rect.min + egui::vec2(16.0, 6.0),
            rect.max - egui::vec2(16.0, 12.0),
        );
        match self.tabs.get(self.active).cloned() {
            Some(Tab::Inbox) => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page));
                child.label(egui::RichText::new("Pull requests").font(fonts::display(22.0)));
                egui::ScrollArea::vertical()
                    .id_salt("inbox-page")
                    .auto_shrink([false, false])
                    .show(&mut child, |ui| self.inbox_view(ui, false));
                self.finish_frame(&ctx);
                return;
            }
            Some(Tab::Pr(pr)) => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page));
                self.pr_view(&mut child, &pr);
                self.finish_frame(&ctx);
                return;
            }
            Some(Tab::Repo(repo)) => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page));
                self.repo_view(&mut child, &repo);
                self.finish_frame(&ctx);
                return;
            }
            Some(Tab::Settings) => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page));
                self.settings_view(&mut child);
                self.finish_frame(&ctx);
                return;
            }
            Some(Tab::Session(id)) => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page));
                self.session_view(&mut child, &id);
                self.finish_frame(&ctx);
                return;
            }
            _ => {}
        }
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            style::c(self.theme.background),
        );
        let layout = self.config.as_ref().and_then(|cfg| {
            let name = self.current_layout_name();
            cfg.layout(&name)
                .or_else(|| cfg.layouts.first())
                .map(|l| (l.name.clone(), l.root.clone()))
        });
        match layout {
            Some((name, root)) => {
                let mut st = std::mem::take(&mut self.layout_state);
                let theme = self.theme.clone();
                layout_view::show(
                    ui,
                    rect.shrink(8.0),
                    &root,
                    &name,
                    &theme,
                    &mut st,
                    &mut |ui, panel| match panel {
                        Panel::Builtin(b) => self.builtin(ui, b),
                        Panel::Mod(m) => self.mod_panel(ui, m),
                    },
                );
                self.layout_state = st;
            }
            None => {
                ui.centered_and_justified(|ui| ui.weak("loading config…"));
            }
        }

        self.finish_frame(&ctx);
    }
}

impl App {
    /// Overlays, queued commands and repaint pacing, after any page.
    fn finish_frame(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.confirm_ui(&ctx);
        self.hook_confirm_ui(&ctx);
        self.palette_ui(&ctx);
        self.spawn_ui(&ctx);
        self.close_confirm_ui(&ctx);
        self.toasts_ui(&ctx);
        for cmd in std::mem::take(&mut self.queue) {
            self.run(&ctx, &cmd);
        }
        if self
            .snap
            .sessions
            .iter()
            .any(|s| s.rec.state == AgentState::Blocked)
        {
            // Blocked cards pulse.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.screenshot(&ctx);
    }
}
