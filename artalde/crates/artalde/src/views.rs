//! Tabs, Linear-style: the dashboard (the layout) is always first; the PR
//! inbox, pull requests and agent sessions open as their own tabs. PR tabs
//! carry adar's PR client: conversation with review threads, files, checks
//! with failed-step logs, and the write actions.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use artalde_core::github::{
    self, Action, Bucket, Check, DiffFile, Filter, LineKind, MergeMethod, PrDetail, PrRef, PrRow,
    TimelineItem,
};
use artalde_core::probe::Turn;
use artalde_core::text::{ago, clip};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, UiBuilder};

use crate::app::App;
use crate::gh::Req;
use crate::style::{self, tint};
use crate::{fonts, markdown};

pub const TAB_BAR: f32 = 38.0;
const INBOX_EVERY_SECS: u64 = 120;
const DETAIL_EVERY_SECS: u64 = 60;
const SESSION_TURNS: usize = 60;
const SIDE_WIDTH: f32 = 380.0;
const FILE_LIST_WIDTH: f32 = 280.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Inbox,
    Pr(PrRef),
    Session(String),
}

impl Tab {
    pub fn key(&self) -> String {
        match self {
            Tab::Home => "home".into(),
            Tab::Inbox => "inbox".into(),
            Tab::Pr(p) => format!("pr:{p}"),
            Tab::Session(id) => format!("session:{id}"),
        }
    }

    pub fn parse(s: &str) -> Option<Tab> {
        match s.split_once(':') {
            None if s == "home" => Some(Tab::Home),
            None if s == "inbox" => Some(Tab::Inbox),
            Some(("pr", r)) => PrRef::parse(r).map(Tab::Pr),
            Some(("session", id)) if !id.is_empty() => Some(Tab::Session(id.to_owned())),
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

fn check_color(theme: &artalde_core::theme::Theme, verdict: &str) -> Color32 {
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
                Res::Diff(pr, r) => {
                    let v = self.prs.entry(pr).or_default();
                    v.diff_loading = false;
                    v.diff = Some(r);
                }
                Res::Log { job, log } => {
                    self.logs.insert(job, Some(log));
                }
                Res::Acted { pr, what, result } => {
                    if let Some(v) = self.prs.get_mut(&pr) {
                        v.busy = None;
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
        let resp = ui
            .scope_builder(
                UiBuilder::new()
                    .id_salt(("inbox-row", pr.to_string(), compact))
                    .sense(Sense::click()),
                |ui| {
                    egui::Frame::new()
                        .fill(if open {
                            tint(style::c(theme.surface), style::c(theme.accent), 0.08)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (dot, _) =
                                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                                ui.painter().circle_filled(
                                    dot.center(),
                                    4.0,
                                    check_color(&theme, &row.checks),
                                );
                                ui.label(
                                    RichText::new(pr.short())
                                        .monospace()
                                        .small()
                                        .color(style::c(theme.dim)),
                                );
                                if row.draft {
                                    ui.label(
                                        RichText::new("draft").small().color(style::c(theme.dim)),
                                    );
                                }
                                ui.add(
                                    egui::Label::new(RichText::new(clip(
                                        &row.title,
                                        if compact { 60 } else { 110 },
                                    )))
                                    .truncate(),
                                );
                            });
                            if !compact || !row.review.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    let (rl, rc) = review_label(&row.review);
                                    if !rl.is_empty() {
                                        ui.label(
                                            RichText::new(rl)
                                                .small()
                                                .color(style::token(&theme, rc)),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(format!("@{}", row.author))
                                            .small()
                                            .color(style::c(theme.dim)),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "+{} -{}",
                                            row.additions, row.deletions
                                        ))
                                        .small()
                                        .monospace()
                                        .color(style::c(theme.dim)),
                                    );
                                    ui.label(
                                        RichText::new(github::age(&row.updated, now))
                                            .small()
                                            .color(style::c(theme.dim)),
                                    );
                                    if !compact {
                                        for l in row.labels.iter().take(3) {
                                            ui.label(
                                                RichText::new(l)
                                                    .small()
                                                    .color(style::c(theme.accent)),
                                            );
                                        }
                                    }
                                });
                            }
                        });
                },
            )
            .response;
        if resp.hovered() {
            ui.painter().rect_stroke(
                resp.rect,
                CornerRadius::same(6),
                Stroke::new(1.0, style::c(theme.border)),
                egui::StrokeKind::Inside,
            );
        }
        if resp.clicked() {
            self.open_tab(Tab::Pr(pr));
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
            ui.label(
                RichText::new(pr.to_string())
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
        ui.horizontal(|ui| {
            for (sub, label) in [
                (
                    PrSub::Conversation,
                    format!("Conversation  {}", d.timeline.len() + open_threads),
                ),
                (PrSub::Files, format!("Files  {}", d.changed_files)),
                (
                    PrSub::Checks,
                    if failing > 0 {
                        format!("Checks  {failing} failing")
                    } else {
                        format!("Checks  {}", d.checks.len())
                    },
                ),
            ] {
                if ui
                    .selectable_label(
                        view.sub == sub,
                        RichText::new(label).font(fonts::semibold(13.5)),
                    )
                    .clicked()
                {
                    view.sub = sub;
                }
            }
        });
        ui.separator();
        match view.sub {
            PrSub::Conversation => self.pr_conversation(ui, pr, view, &d),
            PrSub::Files => self.pr_files(ui, pr, view),
            PrSub::Checks => self.pr_checks(ui, pr, &d),
        }
    }

    fn pr_actions(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView, d: &PrDetail) {
        let theme = self.theme.clone();
        let busy = view.busy.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                if d.open() {
                    if !d.viewer_is_author && ui.button("✓ Approve").clicked() {
                        self.act(pr, Action::Approve(view.composer.trim().to_owned()));
                    }
                    if !d.viewer_is_author
                        && ui
                            .button("Request changes")
                            .on_hover_text("Uses the comment box below as the review body")
                            .clicked()
                    {
                        self.act(pr, Action::RequestChanges(view.composer.trim().to_owned()));
                    }
                    if d.can_write && !d.draft && !d.merge_methods.is_empty() {
                        let method = view.method.unwrap_or(MergeMethod::Squash);
                        let merge = egui::Button::new(
                            RichText::new(format!("Merge · {}", method.label().to_lowercase()))
                                .color(style::c(theme.background)),
                        )
                        .fill(style::c(theme.done));
                        if ui.add(merge).clicked() {
                            self.act(
                                pr,
                                Action::Merge {
                                    method,
                                    delete_branch: view.delete_branch,
                                },
                            );
                        }
                        egui::ComboBox::from_id_salt(("merge-method", pr.to_string()))
                            .selected_text("")
                            .width(24.0)
                            .show_ui(ui, |ui| {
                                for m in &d.merge_methods {
                                    ui.selectable_value(&mut view.method, Some(*m), m.label());
                                }
                                ui.separator();
                                ui.checkbox(&mut view.delete_branch, "Delete branch");
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
                    if d.can_update_branch && d.merge_state == "BEHIND" {
                        if ui.button("Update branch").clicked() {
                            self.act(pr, Action::UpdateBranch { rebase: false });
                        }
                        if ui.small_button("rebase").clicked() {
                            self.act(pr, Action::UpdateBranch { rebase: true });
                        }
                    }
                    if d.viewer_is_author || d.can_write {
                        if d.draft {
                            if ui.button("Ready for review").clicked() {
                                self.act(pr, Action::Ready);
                            }
                        } else if ui.button("To draft").clicked() {
                            self.act(pr, Action::ToDraft);
                        }
                        if ui.button("Close").clicked() {
                            self.act(pr, Action::Close);
                        }
                    }
                    let failed = d.failed_runs();
                    if !failed.is_empty()
                        && ui
                            .button(format!("Re-run failed ({})", failed.len()))
                            .clicked()
                    {
                        self.act(pr, Action::RerunFailed(failed));
                    }
                } else if d.state == "CLOSED" && ui.button("Reopen").clicked() {
                    self.act(pr, Action::Reopen);
                }
            });
            if ui.button("Open on GitHub").clicked() {
                self.queue.push(artalde_core::command::Command::new(
                    "url.open",
                    Some(&d.url),
                ));
            }
            if ui.button("⟳").on_hover_text("Reload").clicked() {
                self.load_pr(pr);
            }
        });
    }

    fn pr_conversation(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView, d: &PrDetail) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        egui::ScrollArea::vertical()
            .id_salt(("pr-convo", pr.to_string()))
            .auto_shrink([false, false])
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
                            author_line(ui, &theme, &c.author, &github::age(&c.created, now), None);
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
                for item in &d.timeline {
                    match item {
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

    fn pr_files(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView) {
        let theme = self.theme.clone();
        if view.diff.is_none() && !view.diff_loading {
            match self.gh.send(Req::Diff(pr.clone())) {
                Ok(()) => view.diff_loading = true,
                Err(e) => view.diff = Some(Err(e)),
            }
        }
        let files = match &view.diff {
            None => {
                ui.spinner();
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(style::c(theme.danger)));
                if ui.button("Retry").clicked() {
                    view.diff = None;
                }
                return;
            }
            Some(Ok(f)) => f.clone(),
        };
        let rect = ui.available_rect_before_wrap();
        let list = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(
                rect.min.x + FILE_LIST_WIDTH.min(rect.width() * 0.35),
                rect.max.y,
            ),
        );
        let body = egui::Rect::from_min_max(egui::pos2(list.max.x + 8.0, rect.min.y), rect.max);
        let mut left = ui.new_child(UiBuilder::new().max_rect(list));
        left.add(
            egui::TextEdit::singleline(&mut view.file_filter)
                .hint_text("filter files")
                .desired_width(f32::INFINITY),
        );
        let needle = view.file_filter.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("files", pr.to_string()))
            .auto_shrink([false, false])
            .show(&mut left, |ui| {
                for (i, f) in files.iter().enumerate() {
                    if !needle.is_empty() && !f.path.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                    let dir = f.path.strip_suffix(name).unwrap_or_default();
                    let r = ui
                        .horizontal(|ui| {
                            let sel = ui.selectable_label(
                                view.file == i,
                                RichText::new(name).monospace().small(),
                            );
                            ui.label(
                                RichText::new(format!("+{}", f.added))
                                    .small()
                                    .color(style::c(theme.done)),
                            );
                            ui.label(
                                RichText::new(format!("-{}", f.removed))
                                    .small()
                                    .color(style::c(theme.danger)),
                            );
                            sel
                        })
                        .inner
                        .on_hover_text(&f.path);
                    if !dir.is_empty() {
                        ui.label(RichText::new(dir).small().color(style::c(theme.dim)));
                    }
                    if r.clicked() {
                        view.file = i;
                    }
                }
            });
        let mut right = ui.new_child(UiBuilder::new().max_rect(body));
        let Some(f) = files.get(view.file).or_else(|| files.first()) else {
            right.label("No changes.");
            return;
        };
        right.label(RichText::new(&f.path).monospace().strong());
        let row_h = 18.0;
        let font = egui::FontId::monospace(12.5);
        egui::ScrollArea::both()
            .id_salt(("diff", pr.to_string(), view.file))
            .auto_shrink([false, false])
            .show_rows(&mut right, row_h, f.lines.len(), |ui, range| {
                for line in f.lines.get(range).unwrap_or_default() {
                    let (bg, fg) = match line.kind {
                        LineKind::Add => (
                            tint(style::c(theme.surface), style::c(theme.done), 0.14),
                            style::c(theme.text),
                        ),
                        LineKind::Remove => (
                            tint(style::c(theme.surface), style::c(theme.danger), 0.14),
                            style::c(theme.text),
                        ),
                        LineKind::Hunk => (
                            tint(style::c(theme.surface), style::c(theme.accent), 0.10),
                            style::c(theme.accent),
                        ),
                        _ => (Color32::TRANSPARENT, style::c(theme.dim)),
                    };
                    let nums = format!(
                        "{:>5} {:>5} ",
                        line.old.map(|n| n.to_string()).unwrap_or_default(),
                        line.new.map(|n| n.to_string()).unwrap_or_default()
                    );
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(1600.0), row_h),
                        Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
                    let galley_nums =
                        ui.painter()
                            .layout_no_wrap(nums, font.clone(), style::c(theme.unknown));
                    let nw = galley_nums.size().x;
                    ui.painter().galley(
                        egui::pos2(rect.min.x + 4.0, rect.min.y + 2.0),
                        galley_nums,
                        style::c(theme.unknown),
                    );
                    let text_color = if matches!(line.kind, LineKind::Context) {
                        style::c(theme.text)
                    } else {
                        fg
                    };
                    ui.painter().text(
                        egui::pos2(rect.min.x + 10.0 + nw, rect.min.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        &line.text,
                        font.clone(),
                        text_color,
                    );
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
