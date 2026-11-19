//! Tabs, Linear-style: the dashboard (the layout) is always first; the PR
//! inbox, pull requests and agent sessions open as their own tabs. PR tabs
//! carry adar's PR client: conversation with review threads, files, checks
//! with failed-step logs, and the write actions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use artalde_core::github::{
    self, Action, Bucket, Check, DiffFile, Filter, MergeMethod, PrDetail, PrRef, PrRow,
    TimelineItem,
};
use artalde_core::probe::Turn;
use artalde_core::text::{ago, clip};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, UiBuilder};

use crate::app::App;
use crate::gh::Req;
use crate::kit::{self, Col};
use crate::style::{self, tint};
use crate::{fonts, markdown};

pub const TAB_BAR: f32 = 38.0;
const INBOX_EVERY_SECS: u64 = 120;
const DETAIL_EVERY_SECS: u64 = 60;
const SESSION_TURNS: usize = 60;
const SIDE_WIDTH: f32 = 380.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Inbox,
    Pr(PrRef),
    Session(String),
    Settings,
    Repo(String),
}

impl Tab {
    pub fn key(&self) -> String {
        match self {
            Tab::Home => "home".into(),
            Tab::Inbox => "inbox".into(),
            Tab::Pr(p) => format!("pr:{p}"),
            Tab::Session(id) => format!("session:{id}"),
            Tab::Settings => "settings".into(),
            Tab::Repo(r) => format!("repo:{r}"),
        }
    }

    pub fn parse(s: &str) -> Option<Tab> {
        match s.split_once(':') {
            None if s == "home" => Some(Tab::Home),
            None if s == "inbox" => Some(Tab::Inbox),
            None if s == "settings" => Some(Tab::Settings),
            Some(("pr", r)) => PrRef::parse(r).map(Tab::Pr),
            Some(("session", id)) if !id.is_empty() => Some(Tab::Session(id.to_owned())),
            Some(("repo", r)) => crate::repo_view::parse_repo(r).map(Tab::Repo),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrSub {
    #[default]
    Conversation,
    Files,
    Checks,
    Review,
}

/// One PR tab's state.
#[derive(Default)]
pub struct PrView {
    pub detail: Option<Box<PrDetail>>,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded_at: Option<Instant>,
    pub diff: Option<Result<Vec<DiffFile>, String>>,
    pub diff_loading: bool,
    pub sub: PrSub,
    pub composer: String,
    pub replies: HashMap<String, String>,
    pub file_filter: String,
    pub file: usize,
    pub method: Option<MergeMethod>,
    pub delete_branch: bool,
    pub busy: Option<String>,
    pub show_resolved: bool,
    pub hide_bots: bool,
    pub wrap: bool,
    /// The review being prepared (loaded on first use).
    pub review: Option<artalde_core::review::PendingReview>,
    /// Index into settings.review_agents.
    pub agent: usize,
    pub agent_status: Option<String>,
    /// An agent reply waiting for the diff to be parsed.
    pub pending_run: Option<artalde_core::review::AgentReview>,
    /// The draft being edited and its text.
    pub editing: Option<(u64, String)>,
    pub verdict: artalde_core::review::Verdict,
    pub submitting: bool,
    /// Show this file and line in Files.
    pub jump: Option<(String, u32)>,
    pub jump_line: Option<u32>,
    /// The line composer: where, and what was typed.
    pub inline_at: Option<(String, u32, artalde_core::review::Side)>,
    pub inline_text: String,
    pub inline_line: String,
}

#[derive(Default)]
pub struct InboxState {
    pub login: String,
    pub buckets: Vec<(Bucket, Vec<PrRow>)>,
    pub error: Option<String>,
    pub loading: bool,
    pub at: Option<Instant>,
    pub filter: String,
    pub collapsed: Vec<Bucket>,
}

/// Failed-job logs by job id; `None` while loading.
pub type Logs = HashMap<u64, Option<Result<String, String>>>;

/// Conversation cache: transcript → (mtime, turns).
pub type Conversations = HashMap<PathBuf, (SystemTime, Vec<Turn>)>;

pub(crate) fn check_color(theme: &artalde_core::theme::Theme, verdict: &str) -> Color32 {
    style::c(match verdict {
        "SUCCESS" | "NEUTRAL" => theme.done,
        "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "STARTUP_FAILURE" | "ACTION_REQUIRED" => {
            theme.danger
        }
        "SKIPPED" | "STALE" | "" => theme.dim,
        _ => theme.working,
    })
}

fn review_label(r: &str) -> (&'static str, &'static str) {
    match r {
        "APPROVED" => ("approved", "done"),
        "CHANGES_REQUESTED" => ("changes requested", "danger"),
        "REVIEW_REQUIRED" => ("review required", "blocked"),
        "COMMENTED" => ("commented", "dim"),
        "DISMISSED" => ("dismissed", "dim"),
        _ => ("", "dim"),
    }
}

impl App {
    pub(crate) fn since(&self) -> String {
        let t = artalde_core::now().saturating_sub(14 * 86_400);
        crate::civil_date(t)
    }

    /// Periodic refreshes for what is on screen.
    pub(crate) fn tick_github(&mut self) {
        let due = |at: Option<Instant>, secs: u64| at.is_none_or(|a| a.elapsed().as_secs() >= secs);
        let wants_inbox = self.tabs.contains(&Tab::Inbox) || self.layout_shows("prs");
        if wants_inbox
            && !self.inbox.loading
            && due(self.inbox.at, INBOX_EVERY_SECS)
            && self.inbox.error.is_none()
        {
            self.refresh_inbox();
        }
        if let Some(Tab::Repo(repo)) = self.tabs.get(self.active).cloned() {
            let v = self.repos.entry(repo.clone()).or_default();
            if !v.loading && v.loaded_at.is_some() && due(v.loaded_at, DETAIL_EVERY_SECS) {
                self.load_repo(&repo);
            }
        }
        if let Some(Tab::Pr(pr)) = self.tabs.get(self.active).cloned() {
            let view = self.prs.entry(pr.clone()).or_default();
            if !view.loading
                && due(view.loaded_at, DETAIL_EVERY_SECS)
                && (view.error.is_none() || view.loaded_at.is_none())
            {
                self.load_pr(&pr);
            }
        }
    }

    pub(crate) fn refresh_inbox(&mut self) {
        let since = self.since();
        match self.gh.send(Req::Inbox { since }) {
            Ok(()) => {
                self.inbox.loading = true;
                self.inbox.error = None;
            }
            Err(e) => self.push_log(format!("inbox: {e}")),
        }
    }

    pub(crate) fn load_pr(&mut self, pr: &PrRef) {
        let ok = self.gh.send(Req::Detail(pr.clone()));
        let view = self.prs.entry(pr.clone()).or_default();
        match ok {
            Ok(()) => view.loading = true,
            Err(e) => view.error = Some(e),
        }
    }

    pub(crate) fn drain_github(&mut self, ctx: &egui::Context) {
        use crate::gh::Res;
        while let Ok(res) = self.gh.rx.try_recv() {
            match res {
                Res::InboxBucket {
                    login,
                    bucket,
                    rows,
                    last,
                } => {
                    if last {
                        self.inbox.loading = false;
                        self.inbox.at = Some(Instant::now());
                    }
                    match rows {
                        Ok(rows) => {
                            if !login.is_empty() {
                                self.inbox.login = login;
                            }
                            match self.inbox.buckets.iter_mut().find(|(b, _)| *b == bucket) {
                                Some((_, r)) => *r = rows,
                                None => {
                                    self.inbox.buckets.push((bucket, rows));
                                    self.inbox.buckets.sort_by_key(|(b, _)| *b);
                                }
                            }
                        }
                        Err(e) => {
                            let e = format!("{}: {e}", bucket.label());
                            self.push_log(format!("inbox: {e}"));
                            self.inbox.error = Some(e);
                        }
                    }
                }
                Res::Detail(pr, r) => {
                    let v = self.prs.entry(pr).or_default();
                    v.loading = false;
                    v.loaded_at = Some(Instant::now());
                    match r {
                        Ok(d) => {
                            if v.method.is_none_or(|m| !d.merge_methods.contains(&m)) {
                                v.method = d.merge_methods.first().copied();
                            }
                            v.detail = Some(d);
                            v.error = None;
                        }
                        Err(e) => v.error = Some(e),
                    }
                }
                Res::Repo(repo, r) => {
                    let v = self.repos.entry(repo).or_default();
                    v.loading = false;
                    v.loaded_at = Some(Instant::now());
                    match r {
                        Ok(d) => {
                            v.detail = Some(d);
                            v.error = None;
                        }
                        Err(e) => v.error = Some(e),
                    }
                }
                Res::Diff(pr, r) => {
                    let v = self.prs.entry(pr).or_default();
                    v.diff_loading = false;
                    v.diff = Some(r);
                }
                Res::Log { job, log } => {
                    self.logs.insert(job, Some(log));
                }
                Res::Acted { pr, what, result } => {
                    if let Some(mut v) = self.prs.remove(&pr) {
                        v.busy = None;
                        if v.submitting {
                            v.submitting = false;
                            if result.is_ok()
                                && let Some(r) = &mut v.review
                            {
                                // Posted: what was sent and what was dropped go.
                                r.drafts.retain(|d| {
                                    d.state == artalde_core::review::DraftState::Proposed
                                });
                                r.summary.clear();
                                r.general_notes.clear();
                                r.agent_verdict.clear();
                                self.save_review(&pr, &v);
                                v.sub = PrSub::Conversation;
                            }
                        }
                        self.prs.insert(pr.clone(), v);
                    }
                    let line = match &result {
                        Ok(()) => format!("{pr}: {what} — done"),
                        Err(e) => format!("{pr}: {what} failed: {e}"),
                    };
                    self.push_log(line.clone());
                    self.toast_line(ctx, line);
                    self.load_pr(&pr);
                    self.inbox.at = None;
                }
            }
        }
    }

    pub(crate) fn act(&mut self, pr: &PrRef, action: Action) {
        if action.needs_confirm() {
            self.confirm = Some((pr.clone(), action));
            return;
        }
        self.act_now(pr, action);
    }

    pub(crate) fn act_now(&mut self, pr: &PrRef, action: Action) {
        let detail = self.prs.get(pr).and_then(|v| v.detail.clone());
        let what = action.describe();
        match self.gh.send(Req::Act {
            pr: pr.clone(),
            action,
            detail,
        }) {
            Ok(()) => {
                if let Some(v) = self.prs.get_mut(pr) {
                    v.busy = Some(what);
                }
            }
            Err(e) => self.push_log(format!("{pr}: {e}")),
        }
    }

    pub(crate) fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some((pr, action)) = self.confirm.clone() else {
            return;
        };
        let mut decided = None;
        let theme = self.theme.clone();
        let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(action.describe()).font(fonts::display(18.0)));
            ui.label(
                RichText::new(pr.to_string())
                    .monospace()
                    .color(style::c(theme.dim)),
            );
            ui.add_space(6.0);
            ui.label("This writes to GitHub and is not undone automatically.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let go = egui::Button::new(
                    RichText::new(action.describe()).color(style::c(theme.background)),
                )
                .fill(style::c(theme.accent));
                if ui.add(go).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    decided = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    decided = Some(false);
                }
            });
        });
        if modal.should_close() {
            decided = Some(false);
        }
        match decided {
            Some(true) => {
                self.confirm = None;
                self.act_now(&pr, action);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    // ------------------------------------------------------------- tab bar

    fn tab_label(&self, tab: &Tab) -> (String, String) {
        match tab {
            Tab::Home => (
                "◆".into(),
                self.config
                    .as_ref()
                    .and_then(|c| c.layout(&self.current_layout_name()))
                    .map_or("Dashboard".into(), |l| l.title.clone()),
            ),
            Tab::Inbox => {
                let n = self
                    .inbox
                    .buckets
                    .iter()
                    .find(|(b, _)| *b == Bucket::Review)
                    .map_or(0, |(_, r)| r.len());
                (
                    "⇅".into(),
                    if n > 0 {
                        format!("Pull requests · {n}")
                    } else {
                        "Pull requests".into()
                    },
                )
            }
            Tab::Pr(pr) => {
                let title = self
                    .prs
                    .get(pr)
                    .and_then(|v| v.detail.as_ref())
                    .map(|d| d.title.clone());
                (
                    "⑂".into(),
                    title.map_or_else(
                        || pr.short(),
                        |t| format!("{} {}", pr.short(), clip(&t, 28)),
                    ),
                )
            }
            Tab::Settings => ("⚙".into(), "Settings".into()),
            Tab::Repo(r) => ("▣".into(), r.rsplit('/').next().unwrap_or(r).to_owned()),
            Tab::Session(id) => (
                "●".into(),
                self.snap
                    .session(id)
                    .map_or_else(|| "session".into(), |s| clip(&s.name(), 34)),
            ),
        }
    }

    pub(crate) fn tab_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let theme = self.theme.clone();
        let painter = ui.painter().clone();
        let mut x = rect.min.x + 4.0;
        let y = rect.min.y + 5.0;
        let h = TAB_BAR - 10.0;
        let mut close = None;
        let mut select = None;
        for (i, tab) in self.tabs.clone().iter().enumerate() {
            let (icon, label) = self.tab_label(tab);
            let font = egui::FontId::proportional(13.0);
            let galley = painter.layout_no_wrap(label.clone(), font.clone(), style::c(theme.text));
            let closable = *tab != Tab::Home;
            let w = (galley.size().x + 44.0 + if closable { 16.0 } else { 0.0 }).min(260.0);
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
            let resp = ui.interact(r, egui::Id::unique(("tab", tab.key())), Sense::click());
            let on = i == self.active;
            let fill = if on {
                style::c(theme.raised)
            } else if resp.hovered() {
                tint(style::c(theme.background), style::c(theme.raised), 0.6)
            } else {
                Color32::TRANSPARENT
            };
            painter.rect_filled(r, CornerRadius::same(7), fill);
            if on {
                painter.rect_stroke(
                    r,
                    CornerRadius::same(7),
                    Stroke::new(1.0, style::c(theme.border)),
                    egui::StrokeKind::Inside,
                );
            }
            let icon_color = match tab {
                Tab::Session(id) => self.snap.session(id).map_or(style::c(theme.dim), |s| {
                    s.tag.as_deref().map_or_else(
                        || style::state(&theme, s.rec.state),
                        |t| style::token(&theme, t),
                    )
                }),
                Tab::Pr(pr) => self
                    .prs
                    .get(pr)
                    .and_then(|v| v.detail.as_ref())
                    .map_or(style::c(theme.dim), |d| {
                        check_color(&theme, &d.checks_state)
                    }),
                _ => style::c(theme.accent),
            };
            painter.text(
                egui::pos2(r.min.x + 14.0, r.center().y),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(12.0),
                icon_color,
            );
            let text_color = if on {
                style::c(theme.text)
            } else {
                style::c(theme.dim)
            };
            let clip_rect = egui::Rect::from_min_max(
                egui::pos2(r.min.x + 26.0, r.min.y),
                egui::pos2(r.max.x - if closable { 22.0 } else { 8.0 }, r.max.y),
            );
            painter.with_clip_rect(clip_rect).text(
                egui::pos2(clip_rect.min.x, r.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                text_color,
            );
            if closable && (resp.hovered() || on) {
                let cr = egui::Rect::from_center_size(
                    egui::pos2(r.max.x - 12.0, r.center().y),
                    egui::vec2(16.0, 16.0),
                );
                let cresp = ui.interact(
                    cr,
                    egui::Id::unique(("tab-close", tab.key())),
                    Sense::click(),
                );
                if cresp.hovered() {
                    painter.rect_filled(cr, CornerRadius::same(4), style::c(theme.border));
                }
                painter.text(
                    cr.center(),
                    egui::Align2::CENTER_CENTER,
                    "×",
                    egui::FontId::proportional(14.0),
                    style::c(theme.dim),
                );
                if cresp.clicked() {
                    close = Some(i);
                }
            }
            if resp.clicked() {
                select = Some(i);
            }
            if resp.middle_clicked() && closable {
                close = Some(i);
            }
            x += w + 4.0;
        }
        let plus = egui::Rect::from_min_size(egui::pos2(x + 2.0, y), egui::vec2(h, h));
        let presp = ui
            .interact(plus, egui::Id::unique("tab-plus"), Sense::click())
            .on_hover_text("Open… (Cmd+T)");
        if presp.hovered() {
            painter.rect_filled(plus, CornerRadius::same(7), style::c(theme.raised));
        }
        painter.text(
            plus.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(16.0),
            style::c(theme.dim),
        );
        if presp.clicked() {
            self.queue
                .push(artalde_core::command::Command::new("palette.open", None));
        }
        painter.hline(
            rect.min.x..=rect.max.x,
            rect.max.y + 2.0,
            Stroke::new(1.0, style::c(theme.border)),
        );
        // Right end: appearance and settings.
        let btn = |painter: &egui::Painter,
                   ui: &mut egui::Ui,
                   x: f32,
                   glyph: &str,
                   tip: &str,
                   key: &str|
         -> bool {
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(h, h));
            let resp = ui
                .interact(r, egui::Id::unique(("tabbar", key)), Sense::click())
                .on_hover_text(tip);
            if resp.hovered() {
                painter.rect_filled(r, CornerRadius::same(7), style::c(theme.raised));
            }
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(15.0),
                style::c(theme.dim),
            );
            resp.clicked()
        };
        if btn(
            &painter,
            ui,
            rect.max.x - h,
            "⚙",
            "Settings (⌘,)",
            "settings",
        ) {
            self.queue
                .push(artalde_core::command::Command::new("settings.open", None));
        }
        let glyph = if theme.dark { "☀" } else { "☾" };
        if btn(
            &painter,
            ui,
            rect.max.x - 2.0 * h - 4.0,
            glyph,
            "Light / dark (⇧⌘D)",
            "mode",
        ) {
            self.queue.push(artalde_core::command::Command::new(
                "appearance.toggle",
                None,
            ));
        }
        if let Some(i) = select {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    pub(crate) fn open_tab(&mut self, tab: Tab) {
        if let Some(i) = self.tabs.iter().position(|t| *t == tab) {
            self.active = i;
        } else {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        }
        self.save_tabs();
    }

    pub(crate) fn close_tab(&mut self, i: usize) {
        if i == 0 || i >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(i);
        if let Tab::Pr(pr) = &tab
            && !self.tabs.contains(&tab)
        {
            self.prs.remove(pr);
        }
        if self.active >= i {
            self.active = self.active.saturating_sub(1);
        }
        self.save_tabs();
    }

    pub(crate) fn save_tabs(&self) {
        let keys: Vec<String> = self.tabs.iter().skip(1).map(Tab::key).collect();
        let path = artalde_core::store::state_dir().join("tabs.json");
        let body = serde_json::json!({ "tabs": keys, "active": self.active }).to_string();
        if let Err(e) = std::fs::create_dir_all(artalde_core::store::state_dir())
            .and_then(|()| std::fs::write(&path, body))
        {
            eprintln!("artalde: tabs: {e}");
        }
    }

    pub(crate) fn load_tabs() -> (Vec<Tab>, usize) {
        let path = artalde_core::store::state_dir().join("tabs.json");
        let v: serde_json::Value = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut tabs = vec![Tab::Home];
        tabs.extend(
            v.get("tabs")
                .and_then(|t| t.as_array())
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().and_then(Tab::parse))
                .filter(|t| *t != Tab::Home)
                .take(30),
        );
        let active = v
            .get("active")
            .and_then(serde_json::Value::as_u64)
            .and_then(|a| usize::try_from(a).ok())
            .unwrap_or(0);
        let active = active.min(tabs.len() - 1);
        (tabs, active)
    }

    /// Route a link from rendered markdown: PRs open as tabs.
    pub(crate) fn open_link(&mut self, url: &str) {
        match PrRef::parse(url) {
            Some(pr) if url.contains("/pull/") || !url.contains("://") => {
                self.open_tab(Tab::Pr(pr))
            }
            _ => self
                .queue
                .push(artalde_core::command::Command::new("url.open", Some(url))),
        }
    }

    pub(crate) fn md(&mut self, ui: &mut egui::Ui, text: &str, size: f32) {
        let out = markdown::show(ui, &self.theme, text, size);
        for url in out.clicked_links {
            self.open_link(&url);
        }
    }

    // --------------------------------------------------------------- inbox

    /// The PR inbox; `compact` for the dashboard panel.
    pub(crate) fn inbox_view(&mut self, ui: &mut egui::Ui, compact: bool) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.inbox.filter)
                    .hint_text("filter: repo:data ci:red review:changes @alice -is:bot \"words\"")
                    .desired_width(ui.available_width() - if compact { 30.0 } else { 120.0 }),
            );
            let _ = edit;
            if self.inbox.loading {
                ui.spinner();
            } else if ui.small_button("⟳").on_hover_text("Refresh").clicked() {
                self.refresh_inbox();
            }
            if !compact {
                ui.label(
                    RichText::new(self.inbox.at.map_or(String::new(), |a| {
                        format!("{} ago", ago(a.elapsed().as_secs()))
                    }))
                    .small()
                    .color(style::c(theme.dim)),
                );
            }
        });
        let filter = Filter::parse(&self.inbox.filter);
        if !filter.errors.is_empty() {
            ui.label(
                RichText::new(filter.errors.join(" · "))
                    .small()
                    .color(style::c(theme.blocked)),
            );
        }
        if !compact {
            ui.horizontal_wrapped(|ui| {
                for chip in [
                    "ci:red",
                    "review:changes",
                    "review:approved",
                    "is:draft",
                    "-is:bot",
                    "size:<100",
                ] {
                    let on = self.inbox.filter.split_whitespace().any(|t| t == chip);
                    if ui
                        .selectable_label(on, RichText::new(chip).small().monospace())
                        .clicked()
                    {
                        let mut parts: Vec<&str> = self
                            .inbox
                            .filter
                            .split_whitespace()
                            .filter(|t| *t != chip)
                            .collect();
                        if !on {
                            parts.push(chip);
                        }
                        self.inbox.filter = parts.join(" ");
                    }
                }
            });
        }
        if let Some(e) = &self.inbox.error {
            ui.label(RichText::new(e).color(style::c(theme.danger)));
            if ui.button("Retry").clicked() {
                self.refresh_inbox();
            }
        }
        if self.inbox.buckets.is_empty() && self.inbox.loading {
            ui.label(RichText::new("Loading pull requests…").color(style::c(theme.dim)));
        }
        let login = self.inbox.login.clone();
        for (bucket, rows) in self.inbox.buckets.clone() {
            if compact && bucket == Bucket::Merged {
                continue;
            }
            let shown: Vec<&PrRow> = rows
                .iter()
                .filter(|r| filter.matches(r, bucket, &login))
                .collect();
            let collapsed = self.inbox.collapsed.contains(&bucket);
            let head = ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if collapsed { "▸" } else { "▾" }).color(style::c(theme.dim)),
                );
                ui.label(RichText::new(bucket.label()).font(fonts::semibold(13.0)));
                ui.label(
                    RichText::new(format!("{}", shown.len()))
                        .small()
                        .color(style::c(theme.dim)),
                );
            });
            if ui
                .interact(
                    head.response.rect,
                    egui::Id::unique(("bucket", bucket.key(), compact)),
                    Sense::click(),
                )
                .clicked()
            {
                if collapsed {
                    self.inbox.collapsed.retain(|b| *b != bucket);
                } else {
                    self.inbox.collapsed.push(bucket);
                }
            }
            if collapsed {
                continue;
            }
            for row in shown {
                self.inbox_row(ui, row, now, compact);
            }
            ui.add_space(6.0);
        }
    }

    fn inbox_row(&mut self, ui: &mut egui::Ui, row: &PrRow, now: u64, compact: bool) {
        let theme = self.theme.clone();
        let Some(pr) = row.pr.clone() else { return };
        let open = self.tabs.contains(&Tab::Pr(pr.clone()));
        let cols: &[Col] = if compact {
            &[Col::Px(12.0), Col::Px(112.0), Col::Flex, Col::Px(34.0)]
        } else {
            &[
                Col::Px(12.0),
                Col::Px(220.0),
                Col::Flex,
                Col::Px(118.0),
                Col::Px(120.0),
                Col::Px(92.0),
                Col::Px(36.0),
            ]
        };
        let (rl, rc) = review_label(&row.review);
        let repo_label = if compact { pr.short() } else { pr.to_string() };
        let resp = kit::row(
            ui,
            &theme,
            ("inbox-row", pr.to_string(), compact),
            kit::ROW,
            open,
            cols,
            |c, ui| match (c, compact) {
                (0, _) => kit::dot(ui, check_color(&theme, &row.checks), 4.0),
                (1, _) => {
                    kit::mono(ui, &repo_label, 11.5, style::c(theme.dim));
                }
                (2, _) => {
                    if row.draft {
                        ui.label(
                            RichText::new("draft")
                                .size(11.0)
                                .color(style::c(theme.unknown)),
                        );
                    }
                    kit::text(ui, &row.title, 13.0, style::c(theme.text));
                }
                (3, false) => {
                    if !rl.is_empty() {
                        kit::text(ui, rl, 11.5, style::token(&theme, rc));
                    }
                }
                (4, false) => {
                    kit::text(ui, &format!("@{}", row.author), 11.5, style::c(theme.dim));
                }
                (5, false) => {
                    ui.label(
                        RichText::new(format!("+{}", row.additions))
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.done)),
                    );
                    ui.label(
                        RichText::new(format!("-{}", row.deletions))
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.danger)),
                    );
                }
                _ => kit::right(
                    ui,
                    &github::age(&row.updated, now),
                    11.0,
                    style::c(theme.dim),
                ),
            },
        );
        let tip = format!(
            "{}\n{} → {}{}",
            row.title,
            row.head,
            row.base,
            if row.labels.is_empty() {
                String::new()
            } else {
                format!("\nlabels: {}", row.labels.join(", "))
            }
        );
        let resp = resp.on_hover_text(tip);
        resp.context_menu(|ui| self.pr_copy_menu(ui, &pr, &row.head, &row.url));
        if resp.clicked() {
            self.open_tab(Tab::Pr(pr));
        }
    }

    /// The things a developer copies about a PR.
    fn pr_copy_menu(&mut self, ui: &mut egui::Ui, pr: &PrRef, head: &str, url: &str) {
        let repo_name = pr.repo.rsplit('/').next().unwrap_or(&pr.repo).to_owned();
        let items = [
            ("Copy URL", url.to_owned()),
            ("Copy owner/repo#N", pr.to_string()),
            ("Copy branch", head.to_owned()),
            (
                "Copy gh pr checkout",
                format!("gh pr checkout {} --repo {}", pr.number, pr.repo),
            ),
            (
                "Copy zz worktree command",
                format!("zz -c {repo_name}@{head} claude"),
            ),
        ];
        for (label, value) in items {
            if !value.is_empty() && ui.button(label).on_hover_text(&value).clicked() {
                ui.ctx().copy_text(value);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Open on GitHub").clicked() {
            self.queue
                .push(artalde_core::command::Command::new("url.open", Some(url)));
            ui.close();
        }
    }

    // ------------------------------------------------------------- PR tab

    pub(crate) fn pr_view(&mut self, ui: &mut egui::Ui, pr: &PrRef) {
        let mut view = self.prs.remove(pr).unwrap_or_default();
        // Screenshot runs can open a PR on another sub-tab.
        if view.loaded_at.is_none() && std::env::var_os("ARTALDE_SCREENSHOT").is_some() {
            view.sub = match std::env::var("ARTALDE_PR_SUB").as_deref() {
                Ok("files") => PrSub::Files,
                Ok("checks") => PrSub::Checks,
                Ok("review") => PrSub::Review,
                _ => view.sub,
            };
        }
        self.pr_view_inner(ui, pr, &mut view);
        self.prs.insert(pr.clone(), view);
    }

    fn pr_view_inner(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        let Some(d) = view.detail.clone() else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(pr.to_string()).font(fonts::display(20.0)));
                match &view.error {
                    Some(e) => {
                        ui.label(RichText::new(e).color(style::c(theme.danger)));
                        if ui.button("Retry").clicked() {
                            self.load_pr(pr);
                        }
                    }
                    None => {
                        ui.spinner();
                    }
                }
            });
            return;
        };
        // Header.
        ui.horizontal(|ui| {
            let (state_text, state_color) = match (d.state.as_str(), d.draft) {
                ("MERGED", _) => ("Merged", theme.accent),
                ("CLOSED", _) => ("Closed", theme.danger),
                (_, true) => ("Draft", theme.dim),
                _ => ("Open", theme.done),
            };
            pill(ui, state_text, style::c(state_color), &theme);
            if ui
                .link(
                    RichText::new(&pr.repo)
                        .monospace()
                        .color(style::c(theme.dim)),
                )
                .on_hover_text("Open the repository")
                .clicked()
            {
                self.open_tab(Tab::Repo(pr.repo.clone()));
            }
            ui.label(
                RichText::new(format!("#{}", pr.number))
                    .monospace()
                    .color(style::c(theme.dim)),
            );
            if view.loading {
                ui.spinner();
            }
            if let Some(b) = &view.busy {
                ui.label(
                    RichText::new(format!("{b}…"))
                        .small()
                        .color(style::c(theme.accent)),
                );
            }
        });
        ui.add(egui::Label::new(RichText::new(&d.title).font(fonts::display(22.0))).wrap());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("@{}", d.author)).color(style::c(theme.dim)));
            ui.label(
                RichText::new(format!("{} → {}", d.head, d.base))
                    .monospace()
                    .small(),
            );
            ui.label(
                RichText::new(format!("+{}", d.additions))
                    .monospace()
                    .small()
                    .color(style::c(theme.done)),
            );
            ui.label(
                RichText::new(format!("-{}", d.deletions))
                    .monospace()
                    .small()
                    .color(style::c(theme.danger)),
            );
            ui.label(
                RichText::new(format!("{} files", d.changed_files))
                    .small()
                    .color(style::c(theme.dim)),
            );
            ui.label(
                RichText::new(format!("updated {}", github::age(&d.updated, now)))
                    .small()
                    .color(style::c(theme.dim)),
            );
            let (rl, rc) = review_label(&d.review);
            if !rl.is_empty() {
                pill(ui, rl, style::token(&theme, rc), &theme);
            }
            pill(
                ui,
                &github::checks_summary(&d),
                check_color(&theme, &d.checks_state),
                &theme,
            );
            if d.merge_state == "BEHIND" {
                pill(ui, "behind base", style::c(theme.blocked), &theme);
            }
            if d.mergeable == "CONFLICTING" {
                pill(ui, "conflicts", style::c(theme.danger), &theme);
            }
            for l in &d.labels {
                pill(ui, l, style::c(theme.accent), &theme);
            }
        });
        ui.add_space(4.0);
        self.pr_actions(ui, pr, view, &d);
        ui.add_space(6.0);
        // Sub-tabs.
        let open_threads = d.threads.iter().filter(|t| !t.resolved).count();
        let failing = d.checks.iter().filter(|c| c.failed()).count();
        let subs = [
            PrSub::Conversation,
            PrSub::Files,
            PrSub::Checks,
            PrSub::Review,
        ];
        let drafts = self
            .pending(pr, view)
            .drafts
            .iter()
            .filter(|d| d.state != artalde_core::review::DraftState::Dismissed)
            .count();
        let running = self.reviewer.runs.contains_key(pr);
        let labels = vec![
            (
                "Conversation".to_owned(),
                Some(d.timeline.len() + open_threads),
            ),
            ("Files".to_owned(), usize::try_from(d.changed_files).ok()),
            (
                if failing > 0 {
                    format!("Checks · {failing} failing")
                } else {
                    "Checks".to_owned()
                },
                Some(d.checks.len()),
            ),
            (
                if running {
                    "Review · agent running".to_owned()
                } else {
                    "Review".to_owned()
                },
                Some(drafts),
            ),
        ];
        let sel = subs.iter().position(|s| *s == view.sub).unwrap_or(0);
        if let Some(i) = kit::tabs(ui, &theme, &labels, sel) {
            view.sub = subs[i.min(3)];
        }
        ui.add_space(kit::S2);
        if view.sub == PrSub::Files {
            self.pr_files_view(ui, pr, view);
            return;
        }
        // Main column plus a metadata sidebar, so prose keeps a readable width.
        let rect = ui.available_rect_before_wrap();
        let side_w = if rect.width() > 980.0 { 290.0 } else { 0.0 };
        let main = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(
                rect.max.x - if side_w > 0.0 { side_w + 20.0 } else { 0.0 },
                rect.max.y,
            ),
        );
        let mut left = ui.new_child(UiBuilder::new().max_rect(main));
        match view.sub {
            PrSub::Checks => self.pr_checks(&mut left, pr, &d),
            PrSub::Review => self.review_tab(&mut left, pr, view, &d),
            _ => self.pr_conversation(&mut left, pr, view, &d),
        }
        if side_w > 0.0 {
            let side =
                egui::Rect::from_min_max(egui::pos2(rect.max.x - side_w, rect.min.y), rect.max);
            let mut right = ui.new_child(UiBuilder::new().max_rect(side));
            egui::ScrollArea::vertical()
                .id_salt(("pr-side", pr.to_string()))
                .auto_shrink([false, false])
                .show(&mut right, |ui| self.pr_sidebar(ui, &d));
        }
    }

    fn pr_sidebar(&mut self, ui: &mut egui::Ui, d: &PrDetail) {
        let theme = self.theme.clone();
        let dim = style::c(theme.dim);
        kit::section(
            ui,
            &theme,
            "Reviewers",
            Some(d.reviews.len() + d.requested.len()),
            |_| {},
        );
        if d.reviews.is_empty() && d.requested.is_empty() {
            ui.label(RichText::new("No reviews yet").size(12.0).color(dim));
        }
        for (who, state) in &d.reviews {
            let (label, token) = review_label(state);
            ui.horizontal(|ui| {
                kit::dot(ui, style::token(&theme, token), 3.5);
                ui.label(RichText::new(format!("@{who}")).size(12.5));
                kit::right(ui, label, 11.5, style::token(&theme, token));
            });
        }
        for who in &d.requested {
            ui.horizontal(|ui| {
                kit::dot(ui, style::c(theme.unknown), 3.5);
                ui.label(RichText::new(format!("@{who}")).size(12.5));
                kit::right(ui, "requested", 11.5, dim);
            });
        }
        ui.add_space(kit::S3);
        kit::section(ui, &theme, "Merge", None, |_| {});
        let (merge_text, merge_color) = match (d.mergeable.as_str(), d.merge_state.as_str()) {
            ("CONFLICTING", _) => ("Conflicts with base", theme.danger),
            (_, "BEHIND") => ("Behind base", theme.blocked),
            (_, "BLOCKED") => ("Blocked by rules or reviews", theme.blocked),
            (_, "UNSTABLE") => ("Checks not passing", theme.blocked),
            (_, "CLEAN") => ("Ready to merge", theme.done),
            _ if d.state == "MERGED" => ("Merged", theme.accent),
            _ => ("Unknown", theme.dim),
        };
        ui.label(
            RichText::new(merge_text)
                .size(12.5)
                .color(style::c(merge_color)),
        );
        if let Some(m) = &d.auto_merge {
            ui.label(
                RichText::new(format!("auto-merge: {}", m.to_lowercase()))
                    .size(11.5)
                    .color(dim),
            );
        }
        ui.add_space(kit::S3);
        kit::section(ui, &theme, "Checks", Some(d.checks.len()), |_| {});
        let failing = d.checks.iter().filter(|c| c.failed()).count();
        let running = d.checks.iter().filter(|c| c.running()).count();
        let passed = d.checks.len().saturating_sub(failing + running);
        for (n, label, color) in [
            (failing, "failing", theme.danger),
            (running, "running", theme.working),
            (passed, "passed or skipped", theme.done),
        ] {
            if n > 0 {
                ui.horizontal(|ui| {
                    kit::dot(ui, style::c(color), 3.5);
                    ui.label(RichText::new(format!("{n} {label}")).size(12.5));
                });
            }
        }
        ui.add_space(kit::S3);
        kit::section(ui, &theme, "Branch", None, |_| {});
        let mut copied = kit::kv(ui, &theme, "head", &d.head, 44.0);
        copied |= kit::kv(ui, &theme, "base", &d.base, 44.0);
        if !d.head_oid.is_empty() {
            copied |= kit::kv(
                ui,
                &theme,
                "sha",
                d.head_oid.get(..12).unwrap_or(&d.head_oid),
                44.0,
            );
        }
        if copied {
            let ctx = ui.ctx().clone();
            self.toast_line(&ctx, "copied".into());
        }
        if !d.labels.is_empty() {
            ui.add_space(kit::S3);
            kit::section(ui, &theme, "Labels", Some(d.labels.len()), |_| {});
            ui.horizontal_wrapped(|ui| {
                for l in &d.labels {
                    kit::pill(ui, &theme, l, style::c(theme.accent));
                }
            });
        }
        let tickets =
            artalde_core::claude::tickets(&format!("{} {}", d.title, d.head), &self.linear_teams());
        if !tickets.is_empty() {
            ui.add_space(kit::S3);
            kit::section(ui, &theme, "Tickets", Some(tickets.len()), |_| {});
            for t in tickets {
                let known = self
                    .snap
                    .sessions
                    .iter()
                    .flat_map(|s| &s.resources)
                    .find(|r| r.id == t)
                    .map(|r| r.url.clone());
                let base = self
                    .snap
                    .sessions
                    .iter()
                    .flat_map(|s| &s.resources)
                    .find_map(|r| {
                        r.url
                            .find("/issue/")
                            .and_then(|i| r.url.get(..i))
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "https://linear.app".into());
                let url = known.unwrap_or_else(|| format!("{base}/issue/{t}"));
                if ui
                    .link(RichText::new(&t).monospace().size(12.0))
                    .on_hover_text(&url)
                    .clicked()
                {
                    self.queue
                        .push(artalde_core::command::Command::new("url.open", Some(&url)));
                }
            }
        }
    }

    fn pr_actions(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView, d: &PrDetail) {
        let theme = self.theme.clone();
        let busy = view.busy.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.add_enabled_ui(!busy, |ui| {
                if d.open() {
                    if d.can_write && !d.draft && !d.merge_methods.is_empty() {
                        let method = view.method.unwrap_or(MergeMethod::Squash);
                        let ready = d.mergeable != "CONFLICTING";
                        let color = if ready {
                            style::c(theme.done)
                        } else {
                            style::c(theme.dim)
                        };
                        if kit::primary(ui, &theme, method.label(), color).clicked() {
                            self.act(
                                pr,
                                Action::Merge {
                                    method,
                                    delete_branch: view.delete_branch,
                                },
                            );
                        }
                        let r = kit::button(ui, &theme, "▾", None).on_hover_text("Merge options");
                        egui::Popup::menu(&r).show(|ui| {
                            for m in &d.merge_methods {
                                ui.selectable_value(&mut view.method, Some(*m), m.label());
                            }
                            ui.separator();
                            ui.checkbox(&mut view.delete_branch, "Delete branch after merge");
                            ui.separator();
                            if d.auto_merge.is_some() {
                                if ui.button("Disable auto-merge").clicked() {
                                    self.act(pr, Action::DisableAutoMerge);
                                }
                            } else if ui.button("Auto-merge when ready").clicked() {
                                self.act(
                                    pr,
                                    Action::AutoMerge {
                                        method,
                                        delete_branch: view.delete_branch,
                                    },
                                );
                            }
                        });
                    }
                    if !d.viewer_is_author {
                        if kit::button(ui, &theme, "Approve", None).clicked() {
                            self.act(pr, Action::Approve(view.composer.trim().to_owned()));
                        }
                        if kit::button(ui, &theme, "Request changes", None)
                            .on_hover_text("Uses the comment box as the review body")
                            .clicked()
                        {
                            self.act(pr, Action::RequestChanges(view.composer.trim().to_owned()));
                        }
                    }
                    if d.can_update_branch
                        && d.merge_state == "BEHIND"
                        && kit::button(ui, &theme, "Update branch", None).clicked()
                    {
                        self.act(pr, Action::UpdateBranch { rebase: false });
                    }
                    let failed = d.failed_runs();
                    if !failed.is_empty()
                        && kit::button(
                            ui,
                            &theme,
                            &format!("Re-run failed ({})", failed.len()),
                            None,
                        )
                        .clicked()
                    {
                        self.act(pr, Action::RerunFailed(failed));
                    }
                    if d.viewer_is_author || d.can_write {
                        let r = kit::button(ui, &theme, "More ▾", None);
                        egui::Popup::menu(&r).show(|ui| {
                            if d.draft {
                                if ui.button("Mark ready for review").clicked() {
                                    self.act(pr, Action::Ready);
                                }
                            } else if ui.button("Convert to draft").clicked() {
                                self.act(pr, Action::ToDraft);
                            }
                            if d.can_update_branch && ui.button("Update branch (rebase)").clicked()
                            {
                                self.act(pr, Action::UpdateBranch { rebase: true });
                            }
                            ui.separator();
                            if ui.button("Close pull request").clicked() {
                                self.act(pr, Action::Close);
                            }
                        });
                    }
                } else if d.state == "CLOSED" && kit::button(ui, &theme, "Reopen", None).clicked() {
                    self.act(pr, Action::Reopen);
                }
            });
            if d.open() {
                let running = self.reviewer.runs.contains_key(pr);
                let label = if running {
                    "Agent reviewing…"
                } else {
                    "Review with agent"
                };
                if kit::button(ui, &theme, label, Some("⇧⌘R")).clicked() {
                    view.sub = PrSub::Review;
                    if !running {
                        self.queue_review = Some((pr.clone(), view.agent));
                    }
                }
                let n = self.pending(pr, view).accepted().count();
                if n > 0 && kit::button(ui, &theme, &format!("Finish review · {n}"), None).clicked()
                {
                    view.sub = PrSub::Review;
                }
            }
            let r = kit::button(ui, &theme, "Copy ▾", None);
            egui::Popup::menu(&r).show(|ui| self.pr_copy_menu(ui, pr, &d.head, &d.url));
            if kit::button(ui, &theme, "GitHub ↗", None).clicked() {
                self.queue.push(artalde_core::command::Command::new(
                    "url.open",
                    Some(&d.url),
                ));
            }
            if kit::button(ui, &theme, "Reload", Some("⌘R")).clicked() {
                self.load_pr(pr);
            }
        });
    }

    fn pr_conversation(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView, d: &PrDetail) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        let mut area = egui::ScrollArea::vertical().id_salt(("pr-convo", pr.to_string()));
        if std::env::var_os("ARTALDE_SCREENSHOT").is_some()
            && let Some(y) = std::env::var("ARTALDE_PR_SCROLL")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
        {
            area = area.vertical_scroll_offset(y);
        }
        area.auto_shrink([false, false])
            .show(ui, |ui| {
                // Description.
                card(ui, &theme, None, |ui| {
                    author_line(ui, &theme, &d.author, &github::age(&d.created, now), None);
                    if d.body.trim().is_empty() {
                        ui.label(
                            RichText::new("No description.")
                                .italics()
                                .color(style::c(theme.dim)),
                        );
                    } else {
                        self.md(ui, &d.body, 14.0);
                    }
                });
                // Open threads first, as adar does.
                let threads: Vec<_> = d
                    .threads
                    .iter()
                    .filter(|t| view.show_resolved || !t.resolved)
                    .cloned()
                    .collect();
                let resolved = d.threads.iter().filter(|t| t.resolved).count();
                if !d.threads.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Review threads · {} open",
                                d.threads.len() - resolved
                            ))
                            .font(fonts::semibold(14.0)),
                        );
                        if resolved > 0 {
                            ui.checkbox(
                                &mut view.show_resolved,
                                RichText::new(format!("show {resolved} resolved")).small(),
                            );
                        }
                    });
                }
                for t in threads {
                    let color = if t.resolved {
                        style::c(theme.dim)
                    } else {
                        style::c(theme.blocked)
                    };
                    card(ui, &theme, Some(color), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}{}",
                                    t.path,
                                    t.line.map(|l| format!(":{l}")).unwrap_or_default()
                                ))
                                .monospace()
                                .small(),
                            );
                            if t.outdated {
                                ui.label(
                                    RichText::new("outdated").small().color(style::c(theme.dim)),
                                );
                            }
                            if t.resolved {
                                ui.label(
                                    RichText::new("resolved")
                                        .small()
                                        .color(style::c(theme.done)),
                                );
                            }
                        });
                        if let Some(hunk) = t
                            .comments
                            .first()
                            .map(|c| c.diff_hunk.clone())
                            .filter(|h| !h.is_empty())
                        {
                            let tail: Vec<&str> = hunk
                                .lines()
                                .rev()
                                .take(4)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect();
                            self.md(ui, &format!("```diff\n{}\n```", tail.join("\n")), 12.5);
                        }
                        for c in &t.comments {
    let who = if c.bot { github::bot_name(&c.author) } else { c.author.clone() };
    author_line(ui, &theme, &who, &github::age(&c.created, now), c.bot.then(|| ("bot", style::c(theme.unknown))));
                            self.md(ui, &c.body, 13.5);
                        }
                        let reply = view.replies.entry(t.id.clone()).or_default();
                        ui.horizontal(|ui| {
                            if t.can_reply {
                                ui.add(
                                    egui::TextEdit::singleline(reply)
                                        .hint_text("Reply…")
                                        .desired_width(ui.available_width() - 170.0),
                                );
                                if ui.button("Reply").clicked() && !reply.trim().is_empty() {
                                    let body = std::mem::take(reply);
                                    self.act(
                                        pr,
                                        Action::Reply {
                                            thread: t.id.clone(),
                                            body,
                                        },
                                    );
                                }
                            }
                            if !t.resolved && t.can_resolve && ui.button("Resolve").clicked() {
                                self.act(pr, Action::Resolve(t.id.clone()));
                            }
                            if t.resolved && t.can_unresolve && ui.button("Unresolve").clicked() {
                                self.act(pr, Action::Unresolve(t.id.clone()));
                            }
                        });
                    });
                }
                // Timeline.
ui.add_space(6.0);
let bots = d.timeline.iter().filter(|t| matches!(t, TimelineItem::Comment(c) | TimelineItem::Review { comment: c, .. } if c.bot)).count();
ui.horizontal(|ui| {
    ui.label(RichText::new("Activity").font(fonts::semibold(14.0)));
    if bots > 0 {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut view.hide_bots, RichText::new(format!("hide {bots} bot comments")).size(12.0));
        });
    }
});
                for item in &d.timeline {
                    match item {
                        TimelineItem::Comment(c) if c.bot => {
    if !view.hide_bots {
        self.bot_card(ui, c, None, now);
    }
}
TimelineItem::Review { comment: c, state } if c.bot && !c.body.trim().is_empty() => {
    if !view.hide_bots {
        let (label, token) = review_label(state);
        self.bot_card(ui, c, Some((label, style::token(&theme, token))), now);
    }
}
TimelineItem::Comment(c) => card(ui, &theme, None, |ui| {
                            author_line(ui, &theme, &c.author, &github::age(&c.created, now), None);
                            self.md(ui, &c.body, 14.0);
                        }),
                        TimelineItem::Review { comment: c, state } => {
                            let (label, token) = review_label(state);
                            let color = style::token(&theme, token);
                            if c.body.trim().is_empty() {
                                event_line(
                                    ui,
                                    &theme,
                                    &c.author,
                                    label,
                                    &github::age(&c.created, now),
                                    color,
                                );
                            } else {
                                card(ui, &theme, Some(color), |ui| {
                                    author_line(
                                        ui,
                                        &theme,
                                        &c.author,
                                        &github::age(&c.created, now),
                                        Some((label, color)),
                                    );
                                    self.md(ui, &c.body, 14.0);
                                });
                            }
                        }
                        TimelineItem::Event { actor, what, at } => {
                            event_line(
                                ui,
                                &theme,
                                actor,
                                what,
                                &github::age(at, now),
                                style::c(theme.dim),
                            );
                        }
                        TimelineItem::Commit {
                            oid,
                            headline,
                            author,
                            at,
                        } => {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                ui.label(
                                    RichText::new(oid.get(..7).unwrap_or(oid))
                                        .monospace()
                                        .small()
                                        .color(style::c(theme.accent)),
                                );
                                ui.label(RichText::new(clip(headline, 90)).small());
                                ui.label(
                                    RichText::new(format!("@{author} · {}", github::age(at, now)))
                                        .small()
                                        .color(style::c(theme.dim)),
                                );
                            });
                        }
                    }
                }
                // Composer.
                ui.add_space(8.0);
                card(ui, &theme, Some(style::c(theme.accent)), |ui| {
                    ui.label(RichText::new("Comment").font(fonts::semibold(13.0)));
                    ui.add(
                        egui::TextEdit::multiline(&mut view.composer)
                            .hint_text("Markdown. Also the body for Approve / Request changes.")
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        let has = !view.composer.trim().is_empty();
                        if ui
                            .add_enabled(has && view.busy.is_none(), egui::Button::new("Comment"))
                            .clicked()
                        {
                            let body = std::mem::take(&mut view.composer);
                            self.act(pr, Action::Comment(body));
                        }
                        if has && !d.viewer_is_author && ui.button("Comment as review").clicked() {
                            let body = std::mem::take(&mut view.composer);
                            self.act(pr, Action::ReviewComment(body));
                        }
                    });
                    if !view.composer.trim().is_empty() {
                        ui.separator();
                        ui.label(RichText::new("Preview").small().color(style::c(theme.dim)));
                        let preview = view.composer.clone();
                        self.md(ui, &preview, 13.5);
                    }
                });
            });
    }

    /// A bot comment: named, badged, and folded to one line when long.
    fn bot_card(
        &mut self,
        ui: &mut egui::Ui,
        c: &github::Comment,
        badge: Option<(&str, Color32)>,
        now: u64,
    ) {
        let theme = self.theme.clone();
        let name = github::bot_name(&c.author);
        let summary = crate::gfm::parse(&c.body)
            .iter()
            .find_map(|seg| match seg {
                crate::gfm::Segment::Markdown(m) => m
                    .lines()
                    .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', ' ']))
                    .find(|l| !l.is_empty())
                    .map(crate::gfm::plain)
                    .filter(|l| !l.is_empty()),
                crate::gfm::Segment::Details { summary, .. } => Some(summary.clone()),
                crate::gfm::Segment::Callout { kind, .. } => Some(kind.to_lowercase()),
            })
            .unwrap_or_default();
        let long = c.body.len() > 600;
        card(ui, &theme, Some(style::c(theme.unknown)), |ui| {
            let header = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&name).font(fonts::semibold(13.0)));
                    kit::pill(ui, &theme, "bot", style::c(theme.dim));
                    if let Some((b, col)) = badge {
                        ui.label(RichText::new(b).size(11.5).color(col));
                    }
                    ui.label(
                        RichText::new(github::age(&c.created, now))
                            .size(11.5)
                            .color(style::c(theme.dim)),
                    );
                });
            };
            if long {
                let id = ui.make_persistent_id(("bot-card", &c.id));
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                );
                let open = state.is_open();
                state
                    .show_header(ui, |ui| header(ui))
                    .body(|ui| self.md(ui, &c.body, 13.5));
                if !open && !summary.is_empty() {
                    ui.label(
                        RichText::new(clip(&summary, 160))
                            .size(12.0)
                            .color(style::c(theme.dim)),
                    );
                }
            } else {
                header(ui);
                self.md(ui, &c.body, 13.5);
            }
        });
    }

    fn pr_checks(&mut self, ui: &mut egui::Ui, pr: &PrRef, d: &PrDetail) {
        let theme = self.theme.clone();
        let mut groups: Vec<(String, Vec<&Check>)> = Vec::new();
        for c in &d.checks {
            match groups.iter_mut().find(|(w, _)| *w == c.workflow) {
                Some((_, v)) => v.push(c),
                None => groups.push((c.workflow.clone(), vec![c])),
            }
        }
        // Failing workflows first.
        groups.sort_by_key(|(_, cs)| std::cmp::Reverse(cs.iter().any(|c| c.failed())));
        egui::ScrollArea::vertical()
            .id_salt(("checks", pr.to_string()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if groups.is_empty() {
                    ui.label(
                        RichText::new("No checks on the head commit.").color(style::c(theme.dim)),
                    );
                }
                for (wf, checks) in groups {
                    ui.label(RichText::new(wf).font(fonts::semibold(14.0)));
                    for c in checks {
                        ui.horizontal(|ui| {
                            let (dot, _) =
                                ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                            ui.painter().circle_filled(
                                dot.center(),
                                4.0,
                                check_color(&theme, &c.verdict),
                            );
                            ui.label(&c.name);
                            ui.label(
                                RichText::new(c.verdict.to_lowercase())
                                    .small()
                                    .color(check_color(&theme, &c.verdict)),
                            );
                            if let (Some(a), Some(b)) = (
                                github::parse_iso(&c.started),
                                github::parse_iso(&c.completed),
                            ) {
                                ui.label(
                                    RichText::new(ago(b.saturating_sub(a)))
                                        .small()
                                        .color(style::c(theme.dim)),
                                );
                            }
                            if let Some(job) = c.job_id {
                                if c.failed() {
                                    let open = self.logs.contains_key(&job);
                                    if ui
                                        .small_button(if open { "hide log" } else { "log" })
                                        .clicked()
                                    {
                                        if open {
                                            self.logs.remove(&job);
                                        } else if self
                                            .gh
                                            .send(Req::Log {
                                                pr: pr.clone(),
                                                job,
                                            })
                                            .is_ok()
                                        {
                                            self.logs.insert(job, None);
                                        }
                                    }
                                }
                                if !c.running() && ui.small_button("re-run").clicked() {
                                    self.act(pr, Action::RerunJob(job));
                                }
                            }
                            if !c.url.is_empty()
                                && ui.small_button("↗").on_hover_text(&c.url).clicked()
                            {
                                self.queue.push(artalde_core::command::Command::new(
                                    "url.open",
                                    Some(&c.url),
                                ));
                            }
                        });
                        if let Some(entry) = c.job_id.and_then(|j| self.logs.get(&j)) {
                            match entry {
                                None => {
                                    ui.spinner();
                                }
                                Some(Err(e)) => {
                                    ui.label(
                                        RichText::new(e).small().color(style::c(theme.danger)),
                                    );
                                }
                                Some(Ok(log)) => {
                                    let log = log.clone();
                                    card(ui, &theme, Some(style::c(theme.danger)), |ui| {
                                        egui::ScrollArea::both()
                                            .id_salt(("log", c.job_id))
                                            .max_height(360.0)
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(log).monospace().size(12.0),
                                                    )
                                                    .extend(),
                                                );
                                            });
                                    });
                                }
                            }
                        }
                    }
                    ui.add_space(8.0);
                }
            });
    }

    // -------------------------------------------------------- session tab

    pub(crate) fn session_view(&mut self, ui: &mut egui::Ui, id: &str) {
        let theme = self.theme.clone();
        let Some(s) = self.snap.session(id).cloned() else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("This session has ended.").color(style::c(theme.dim)))
            });
            return;
        };
        if self.selected.as_deref() != Some(id) {
            self.selected = Some(id.to_owned());
        }
        let rect = ui.available_rect_before_wrap();
        let side_w = SIDE_WIDTH.min(rect.width() * 0.4);
        let main =
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x - side_w - 10.0, rect.max.y));
        let side = egui::Rect::from_min_max(egui::pos2(rect.max.x - side_w, rect.min.y), rect.max);
        let mut left = ui.new_child(UiBuilder::new().max_rect(main));
        left.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
            let c = s.tag.as_deref().map_or_else(
                || style::state(&theme, s.rec.state),
                |t| style::token(&theme, t),
            );
            ui.painter().circle_filled(dot.center(), 5.0, c);
            ui.label(RichText::new(s.name()).font(fonts::display(22.0)));
        });
        let turns = s
            .rec
            .transcript
            .as_ref()
            .map(|p| self.conversation(p))
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .id_salt(("convo", id))
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(&mut left, |ui| {
                if turns.is_empty() {
                    ui.label(
                        RichText::new("No transcript for this session.").color(style::c(theme.dim)),
                    );
                }
                for t in &turns {
                    let accent = if t.user {
                        Some(style::c(theme.accent))
                    } else {
                        None
                    };
                    card(ui, &theme, accent, |ui| {
                        ui.label(
                            RichText::new(if t.user { "You" } else { s.rec.agent.as_str() })
                                .font(fonts::semibold(12.5))
                                .color(if t.user {
                                    style::c(theme.accent)
                                } else {
                                    style::c(theme.dim)
                                }),
                        );
                        self.md(ui, &t.text, 14.0);
                    });
                }
            });
        let mut right = ui.new_child(UiBuilder::new().max_rect(side));
        egui::Frame::new()
            .fill(style::c(theme.surface))
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12))
            .show(&mut right, |ui| {
                let inner = egui::vec2(side.width() - 26.0, side.height() - 26.0);
                ui.set_min_size(inner);
                ui.set_max_width(inner.x);
                egui::ScrollArea::vertical()
                    .id_salt(("session-side", id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.builtin(ui, "detail"));
            });
    }

    fn conversation(&mut self, path: &PathBuf) -> Vec<Turn> {
        let Ok(mtime) = std::fs::metadata(path).and_then(|m| m.modified()) else {
            return Vec::new();
        };
        if let Some((t, turns)) = self.conversations.get(path)
            && *t == mtime
        {
            return turns.clone();
        }
        let turns = artalde_core::probe::conversation(path, SESSION_TURNS);
        if self.conversations.len() > 16 {
            self.conversations.clear();
        }
        self.conversations
            .insert(path.clone(), (mtime, turns.clone()));
        turns
    }
}

fn pill(ui: &mut egui::Ui, text: &str, color: Color32, theme: &artalde_core::theme::Theme) {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), color, 0.18))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(8, 1))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(color)));
}

fn card(
    ui: &mut egui::Ui,
    theme: &artalde_core::theme::Theme,
    edge: Option<Color32>,
    add: impl FnOnce(&mut egui::Ui),
) {
    let resp = egui::Frame::new()
        .fill(style::c(theme.raised))
        .stroke(Stroke::new(1.0, style::c(theme.border)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin {
            left: 14,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        })
        .response;
    if let Some(c) = edge {
        let bar = egui::Rect::from_min_size(resp.rect.min, egui::vec2(3.0, resp.rect.height()));
        ui.painter().rect_filled(
            bar,
            CornerRadius {
                nw: 8,
                sw: 8,
                ne: 0,
                se: 0,
            },
            c,
        );
    }
    ui.add_space(4.0);
}

fn author_line(
    ui: &mut egui::Ui,
    theme: &artalde_core::theme::Theme,
    who: &str,
    when: &str,
    badge: Option<(&str, Color32)>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("@{who}")).font(fonts::semibold(13.0)));
        if let Some((b, c)) = badge {
            ui.label(RichText::new(b).small().color(c));
        }
        ui.label(RichText::new(when).small().color(style::c(theme.dim)));
    });
}

fn event_line(
    ui: &mut egui::Ui,
    theme: &artalde_core::theme::Theme,
    who: &str,
    what: &str,
    when: &str,
    color: Color32,
) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, color);
        ui.label(RichText::new(format!("@{who}")).small());
        ui.label(RichText::new(what).small().color(color));
        ui.label(RichText::new(when).small().color(style::c(theme.dim)));
    });
}
