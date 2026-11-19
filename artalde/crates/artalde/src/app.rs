//! The window: state, commands, keys, the palette and the spawn dialog.
//! Panels live in `panels.rs`; the layout tree in `layout_view.rs`.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use artalde_core::command::{COMMANDS, Command};
use artalde_core::config::Config;
use artalde_core::inbox;
use artalde_core::keys::Chord;
use artalde_core::layout::Panel;
use artalde_core::model::{AgentState, Session, Snapshot};
use artalde_core::store::Store;
use artalde_core::tags::{self, Tags};
use artalde_core::text::{clip, shell_quote};
use artalde_core::theme::Theme;
use artalde_mods::{Action, Context as ModContext, ModHost, ViewState};
use eframe::egui::{self, Key, Modifiers};

use crate::actions::{Actions, Job};
use crate::feed::{Feed, Msg};
use crate::gh::Gh;
use crate::layout_view::{self, LayoutState};
use crate::panels::AgentView;
use crate::views::{Conversations, InboxState, Logs, PrView, TAB_BAR, Tab};
use crate::{fonts, mod_view, style};
use artalde_core::github::{self, PrRef};

const LOG_LINES: usize = 500;
const TOAST_SECS: f64 = 4.0;
const MAX_TOASTS: usize = 4;
const PALETTE_ROWS: usize = 12;

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
    pub(crate) hook_confirm: bool,
    pub(crate) reviewer: crate::reviewer::Reviewer,
    /// A review to start next frame: PR and agent index.
    pub(crate) queue_review: Option<(PrRef, usize)>,
    pub(crate) agent_view: AgentView,
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
    pub(crate) spawn: Option<SpawnForm>,
    pub(crate) reply: String,
    pub(crate) hide_done: bool,
    shot: Option<Shot>,
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

pub struct SpawnForm {
    /// Index into the worktree list, or `None` for a new worktree.
    pub target: Option<usize>,
    pub repo: String,
    pub branch: String,
    pub agent: String,
    pub prompt: String,
}

/// `ARTALDE_SCREENSHOT=<png>`: save one frame once data is in, then quit.
struct Shot {
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
            hook_confirm: false,
            reviewer: crate::reviewer::Reviewer::new(),
            queue_review: None,
            agent_view: AgentView::default(),
            agent_query: String::new(),
            spaces_closed: Vec::new(),
            label_for: None,
            prev_states: HashMap::new(),
            have_snapshot: false,
            log: VecDeque::new(),
            toasts: VecDeque::new(),
            queue: Vec::new(),
            palette: None,
            spawn: None,
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
        self.palette.is_some() || self.spawn.is_some() || self.confirm.is_some()
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
        let notify = self.settings().notify;
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
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
        self.apply_tags();
        if !self.have_snapshot && self.shot.is_some() {
            let command = match std::env::var("ARTALDE_SCREENSHOT_OVERLAY").as_deref() {
                Ok("palette") => Some("palette.open"),
                Ok("spawn") => Some("spawn.open"),
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
            if notify && !focused {
                let _queued_or_logged = self
                    .actions
                    .submit(Job::Notify {
                        title: clip(&name, 60),
                        body: clip(&what, 140),
                    })
                    .map_err(|e| self.push_log(format!("notify: {e}")));
            }
        }
    }

    pub(crate) fn submit_job(&mut self, job: Job) {
        self.submit(job);
    }

    fn submit(&mut self, job: Job) {
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

    fn open_spawn(&mut self, arg: Option<&str>) {
        let worktrees = self.worktree_choices();
        let target = arg
            .and_then(|a| {
                worktrees
                    .iter()
                    .position(|(_, path, _)| path.display().to_string() == a)
            })
            .or(if worktrees.is_empty() { None } else { Some(0) });
        let repo = arg
            .and_then(|a| self.snap.spaces.iter().find(|s| s.repo == a))
            .map(|s| s.repo.clone())
            .or_else(|| self.snap.spaces.first().map(|s| s.repo.clone()))
            .unwrap_or_default();
        let agent = self
            .settings()
            .agents
            .first()
            .cloned()
            .unwrap_or_else(|| "claude".into());
        self.spawn = Some(SpawnForm {
            target,
            repo,
            branch: String::new(),
            agent,
            prompt: String::new(),
        });
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

    pub(crate) fn spawn_line(&self, form: &SpawnForm) -> Result<String, String> {
        let s = self.settings();
        let agent = form.agent.trim();
        if agent.is_empty()
            || !agent
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        {
            return Err("agent must be a plain command name".into());
        }
        let prompt = if form.prompt.trim().is_empty() {
            String::new()
        } else {
            shell_quote(form.prompt.trim())
        };
        let line = match form.target {
            Some(i) => {
                let choices = self.worktree_choices();
                let (_, dir, repo) = choices.get(i).ok_or("pick a worktree")?;
                s.spawn_here
                    .replace("{dir}", &shell_quote(&dir.display().to_string()))
                    .replace("{repo}", &shell_quote(repo))
            }
            None => {
                let branch = form.branch.trim();
                if branch.is_empty()
                    || !branch
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
                {
                    return Err("branch: letters, digits, - _ . / only".into());
                }
                if form.repo.trim().is_empty() {
                    return Err("pick a repo".into());
                }
                s.spawn_worktree
                    .replace("{repo}", &shell_quote(form.repo.trim()))
                    .replace("{branch}", &shell_quote(branch))
            }
        };
        Ok(line
            .replace("{agent}", &shell_quote(agent))
            .replace("{prompt}", &prompt)
            .trim()
            .to_owned())
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if self.palette.is_some() || self.spawn.is_some() {
            return;
        }
        let Some(cfg) = &self.config else { return };
        let mut fired = Vec::new();
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

    fn palette_entries(&self) -> Vec<(String, String, Command)> {
        let keys = self.config.as_ref().map(|c| &c.keys);
        let hint = |cmd: &str| {
            keys.and_then(|k| k.chord_for(cmd))
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        if let Some(sel) = self.selected_session() {
            for (name, _) in tags::PALETTE {
                out.push((
                    format!("Tag {}: {name}", sel.name()),
                    String::new(),
                    Command::new("session.tag", Some(name)),
                ));
            }
            out.push((
                format!("Untag {}", sel.name()),
                String::new(),
                Command::new("session.tag", Some("none")),
            ));
        }
        for s in &self.snap.sessions {
            for r in &s.resources {
                let title = r.title.clone().unwrap_or_default();
                out.push((
                    format!("Open {} {title} — {}", r.short(), s.name()),
                    r.state.clone().unwrap_or_default(),
                    Command::new("url.open", Some(&r.url)),
                ));
            }
        }
        for s in &self.snap.sessions {
            let place = s.repo.clone().unwrap_or_else(|| s.rec.agent.clone());
            out.push((
                format!("Agent: {} — {place}", s.name()),
                s.rec.state.as_str().to_owned(),
                Command::new("agent.select", Some(&s.rec.id)),
            ));
            if s.rec.tty.is_some() {
                out.push((
                    format!("Focus terminal: {}", s.name()),
                    String::new(),
                    Command::new("session.focus", Some(&s.rec.id)),
                ));
            }
        }
        for (name, desc) in COMMANDS {
            if desc.contains('<') {
                continue;
            }
            out.push(((*desc).to_owned(), hint(name), Command::new(name, None)));
        }
        if let Some(cfg) = &self.config {
            for l in &cfg.layouts {
                out.push((
                    format!("Layout: {}", l.title),
                    l.name.clone(),
                    Command::new("layout.select", Some(&l.name)),
                ));
            }
            for t in &cfg.themes {
                out.push((
                    format!("Theme: {}", t.name),
                    String::new(),
                    Command::new("theme.select", Some(&t.name)),
                ));
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
            out.push((
                format!("Repo: {r}"),
                "deployments · PRs".into(),
                Command::new("repo.open", Some(&r)),
            ));
        }
        for sp in &self.snap.spaces {
            out.push((
                format!("Filter: {}", sp.repo),
                String::new(),
                Command::new("space.filter", Some(&sp.repo)),
            ));
            out.push((
                format!("New agent in {}", sp.repo),
                String::new(),
                Command::new("spawn.open", Some(&sp.repo)),
            ));
        }
        out
    }

    fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.palette.take() else {
            return;
        };
        let mut entries = self.palette_entries();
        if let Some(repo) =
            crate::repo_view::parse_repo(&p.query).filter(|_| !p.query.contains('#'))
        {
            entries.insert(
                0,
                (
                    format!("Open repo {repo}"),
                    String::new(),
                    Command::new("repo.open", Some(&repo)),
                ),
            );
        }
        if let Some(pr) = PrRef::parse(&p.query) {
            entries.insert(
                0,
                (
                    format!("Open PR {pr}"),
                    String::new(),
                    Command::new("pr.open", Some(&pr.to_string())),
                ),
            );
        }
        let mut scored: Vec<(i32, &(String, String, Command))> = entries
            .iter()
            .filter_map(|e| fuzzy(&p.query, &e.0).map(|s| (s, e)))
            .collect();
        scored.sort_by_key(|e| std::cmp::Reverse(e.0));
        scored.truncate(PALETTE_ROWS * 4);
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down {
            p.cursor = (p.cursor + 1).min(scored.len().saturating_sub(1));
        }
        if up {
            p.cursor = p.cursor.saturating_sub(1);
        }
        let mut run = None;
        if enter {
            run = scored.get(p.cursor).map(|(_, e)| e.2.clone());
        }
        let theme = self.theme.clone();
        let modal = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(560.0);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut p.query)
                    .hint_text("Type a command, agent, layout…")
                    .desired_width(f32::INFINITY),
            );
            edit.request_focus();
            if edit.changed() {
                p.cursor = 0;
            }
            ui.add_space(6.0);
            let start = p.cursor.saturating_sub(PALETTE_ROWS - 1);
            for (i, (_, (label, hint, cmd))) in
                scored.iter().enumerate().skip(start).take(PALETTE_ROWS)
            {
                let on = i == p.cursor;
                let r = crate::kit::row(
                    ui,
                    &theme,
                    ("palette-entry", i),
                    26.0,
                    on,
                    &[crate::kit::Col::Flex, crate::kit::Col::Px(108.0)],
                    |column, ui| {
                        if column == 0 {
                            let text = egui::RichText::new(label);
                            ui.add(
                                egui::Label::new(if on { text.strong() } else { text }).truncate(),
                            );
                        } else {
                            let color = AgentState::parse(hint)
                                .map_or(style::c(theme.dim), |s| style::state(&theme, s));
                            crate::kit::right(ui, hint, 11.0, color);
                        }
                    },
                )
                .on_hover_text(label);
                if r.clicked() {
                    run = Some(cmd.clone());
                }
            }
            if scored.is_empty() {
                ui.weak("nothing matches");
            }
        });
        if esc || modal.should_close() {
            return;
        }
        if let Some(cmd) = run {
            self.queue.push(cmd);
            return;
        }
        self.palette = Some(p);
    }

    fn spawn_ui(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.spawn.take() else {
            return;
        };
        let choices = self.worktree_choices();
        let repos: Vec<String> = self.snap.spaces.iter().map(|s| s.repo.clone()).collect();
        let agents = self.settings().agents;
        let preview = self.spawn_line(&form);
        let mut close = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        let mut start = false;
        let theme = self.theme.clone();
        let modal = egui::Modal::new(egui::Id::unique("spawn")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("New agent");
            ui.add_space(6.0);
            egui::Grid::new("spawn-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Where");
                    let label = form
                        .target
                        .and_then(|i| choices.get(i))
                        .map_or("new worktree…".to_owned(), |c| c.0.clone());
                    egui::ComboBox::from_id_salt("spawn-target")
                        .selected_text(label)
                        .width(380.0)
                        .show_ui(ui, |ui| {
                            for (i, (l, _, _)) in choices.iter().enumerate() {
                                ui.selectable_value(&mut form.target, Some(i), l);
                            }
                            ui.selectable_value(&mut form.target, None, "new worktree…");
                        });
                    ui.end_row();
                    if form.target.is_none() {
                        ui.label("Repo");
                        egui::ComboBox::from_id_salt("spawn-repo")
                            .selected_text(&form.repo)
                            .width(380.0)
                            .show_ui(ui, |ui| {
                                for r in &repos {
                                    ui.selectable_value(&mut form.repo, r.clone(), r);
                                }
                            });
                        ui.end_row();
                        ui.label("Branch");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.branch)
                                .hint_text("THU-123-short-desc")
                                .desired_width(380.0),
                        );
                        ui.end_row();
                    }
                    ui.label("Agent");
                    egui::ComboBox::from_id_salt("spawn-agent")
                        .selected_text(&form.agent)
                        .show_ui(ui, |ui| {
                            for a in &agents {
                                ui.selectable_value(&mut form.agent, a.clone(), a);
                            }
                        });
                    ui.end_row();
                    ui.label("Prompt");
                    ui.add(
                        egui::TextEdit::multiline(&mut form.prompt)
                            .desired_rows(3)
                            .desired_width(380.0)
                            .hint_text("optional first message"),
                    );
                    ui.end_row();
                });
            ui.add_space(6.0);
            match &preview {
                Ok(line) => ui.label(
                    egui::RichText::new(format!("$ {line}"))
                        .monospace()
                        .color(style::c(theme.dim)),
                ),
                Err(e) => ui.label(egui::RichText::new(e).color(style::c(theme.danger))),
            };
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        preview.is_ok(),
                        egui::Button::new("Start in a new terminal tab"),
                    )
                    .clicked()
                {
                    start = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            return;
        }
        if start {
            if let Ok(line) = preview {
                self.submit(Job::Spawn { line });
            }
            return;
        }
        self.spawn = Some(form);
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

fn save_png(path: &std::path::Path, rgba: &[u8], w: usize, h: usize) -> anyhow::Result<()> {
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

/// Subsequence match: `Some(score)` if every query char appears in order.
fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let t = text.to_lowercase();
    if let Some(at) = t.find(&q) {
        return Some(1000 - i32::try_from(at).unwrap_or(999));
    }
    let mut chars = t.chars();
    let mut score = 0;
    for qc in q.chars() {
        let mut gap = 0;
        loop {
            match chars.next() {
                Some(tc) if tc == qc => break,
                Some(_) => gap += 1,
                None => return None,
            }
        }
        score -= gap;
    }
    Some(score)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_substrings() {
        assert!(fuzzy("lay", "Layout: Radar").unwrap() > fuzzy("lyt", "Layout: Radar").unwrap());
        assert_eq!(fuzzy("zzz", "Layout"), None);
        assert_eq!(fuzzy("", "x"), Some(0));
    }
}
