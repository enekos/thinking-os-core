//! Built-in panels: agents, spaces, inbox, prs, detail, status, log. All
//! lists use the kit's fixed-height rows so columns line up.

use artalde_core::command::Command;
use artalde_core::lifecycle::{Launch, Stage};
use artalde_core::model::{AgentState, Resource, ResourceKind, Session};
use artalde_core::tags;
use artalde_core::text::{ago, shell_quote};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};

use crate::app::App;
use crate::kit::{self, Col, Icon};
use crate::style::{self, tint};
use crate::views::Tab;

/// Resource chips on an agent row.
const ROW_RESOURCES: usize = 2;
const LABEL_W: f32 = 76.0;
/// Closed sessions the list shows under "Recently closed".
const CLOSED_ROWS: usize = 6;
/// The reply box's send button.
const SEND_W: f32 = 26.0;
/// Width of the row's last column: the age, or the hover actions.
const ACTIONS_W: f32 = 84.0;

/// Which lane of the lifecycle the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lane {
    #[default]
    All,
    Only(Stage),
}

impl Lane {
    const ALL: [Lane; 5] = [
        Lane::All,
        Lane::Only(Stage::NeedsYou),
        Lane::Only(Stage::Working),
        Lane::Only(Stage::Idle),
        Lane::Only(Stage::Stale),
    ];

    fn label(self) -> &'static str {
        match self {
            Lane::All => "All",
            Lane::Only(s) => s.label(),
        }
    }

    fn admits(self, s: Stage) -> bool {
        self == Lane::All || self == Lane::Only(s)
    }
}

/// The shape a session's state takes in lists and tabs.
pub(crate) fn state_mark(state: AgentState, stale: bool) -> kit::Mark {
    match state {
        _ if stale => kit::Mark::Ghost,
        AgentState::Working => kit::Mark::Spin,
        AgentState::Blocked => kit::Mark::Alert,
        AgentState::Done => kit::Mark::Dot,
        AgentState::Idle => kit::Mark::Ring,
        AgentState::Unknown => kit::Mark::Ghost,
    }
}

impl App {
    pub(crate) fn builtin(&mut self, ui: &mut egui::Ui, name: &str) {
        match name {
            "agents" => self.agents(ui),
            "spaces" => self.spaces(ui),
            "inbox" => self.inbox(ui),
            "prs" => self.inbox_view(ui, true),
            "detail" => self.detail(ui),
            "status" => self.status(ui),
            "log" => self.log_panel(ui),
            "terminal" => {
                let rect = ui.max_rect();
                self.terminal_page(ui, rect, crate::terms::HOME_WORKSPACE, false);
            }
            other => {
                ui.weak(format!("no panel {other:?}"));
            }
        }
    }

    fn color(&self, s: AgentState) -> egui::Color32 {
        style::state(&self.theme, s)
    }

    fn dimc(&self) -> egui::Color32 {
        style::c(self.theme.dim)
    }

    fn stage_color(&self, stage: Stage, state: AgentState) -> egui::Color32 {
        match stage {
            Stage::Stale => style::c(self.theme.faint),
            _ => self.color(state),
        }
    }

    /// Sessions the agent list shows, in display order.
    pub(crate) fn listed_sessions(&self) -> Vec<Session> {
        let q = self.agent_query.trim().to_lowercase();
        let mut out = Vec::new();
        for stage in Stage::ALL {
            for s in self.visible_sessions() {
                let st = self.stage(s);
                if st != stage || !self.lane.admits(st) {
                    continue;
                }
                if !q.is_empty() {
                    let hay = format!(
                        "{} {} {} {} {} {}",
                        s.name(),
                        s.handle.clone().unwrap_or_default(),
                        s.repo.clone().unwrap_or_default(),
                        s.branch.clone().unwrap_or_default(),
                        s.rec.agent,
                        s.resources
                            .iter()
                            .map(|r| r.id.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                    .to_lowercase();
                    if !hay.contains(&q) {
                        continue;
                    }
                }
                out.push(s.clone());
            }
        }
        out
    }

    fn agent_keys(&mut self, ui: &egui::Ui, rows: &[Session]) {
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if typing || rows.is_empty() || self.palette_open() {
            return;
        }
        let none = egui::Modifiers::NONE;
        let (down, up, enter, focus, close, stop) = ui.input_mut(|i| {
            (
                i.consume_key(none, egui::Key::J) || i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::K) || i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::Enter),
                i.consume_key(none, egui::Key::F),
                i.consume_key(none, egui::Key::X),
                i.consume_key(none, egui::Key::Period),
            )
        });
        let pos = self
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|s| &s.rec.id == id));
        let next = match (down, up, pos) {
            (true, _, Some(p)) => Some((p + 1).min(rows.len() - 1)),
            (true, _, None) | (_, true, None) => Some(0),
            (_, true, Some(p)) => Some(p.saturating_sub(1)),
            _ => None,
        };
        if let Some(n) = next.and_then(|n| rows.get(n)) {
            self.queue
                .push(Command::new("agent.select", Some(&n.rec.id)));
        }
        if let Some(id) = self.selected.clone() {
            for (pressed, cmd) in [
                (enter, "session.open"),
                (focus, "session.focus"),
                (close, "session.close"),
                (stop, "session.stop"),
            ] {
                if pressed {
                    self.queue.push(Command::new(cmd, Some(&id)));
                }
            }
        }
    }

    fn agents(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        // Toolbar: lanes on the left, search and New agent on the right.
        let counts: Vec<usize> = Lane::ALL
            .iter()
            .map(|lane| {
                self.visible_sessions()
                    .iter()
                    .filter(|s| lane.admits(self.stage(s)))
                    .count()
            })
            .collect();
        let width = ui.available_width();
        let stacked = width < 760.0;
        let lanes = |app: &mut App, ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (lane, n) in Lane::ALL.iter().zip(&counts) {
                    let accent = match lane {
                        Lane::Only(Stage::NeedsYou) if *n > 0 => style::c(theme.blocked),
                        _ => style::c(theme.accent),
                    };
                    let badge = n.to_string();
                    if kit::pill_tab(
                        ui,
                        &theme,
                        lane.label(),
                        Some(&badge),
                        app.lane == *lane,
                        accent,
                    )
                    .clicked()
                    {
                        app.lane = *lane;
                    }
                }
            });
        };
        let tools = |app: &mut App, ui: &mut egui::Ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::primary_with(
                    ui,
                    &theme,
                    Some(Icon::Plus),
                    "New agent",
                    Some("⌘N"),
                    style::c(theme.accent),
                )
                .clicked()
                {
                    app.queue.push(Command::new("spawn.open", None));
                }
                // Never wider than what is left: an overflowing right-to-left
                // row would push the whole panel's content left.
                let w = ui.available_width().min(260.0);
                if w >= 100.0 {
                    kit::search(
                        ui,
                        &theme,
                        &mut app.agent_query,
                        "Search agents, repos, PRs",
                        w,
                    );
                }
            });
        };
        if stacked {
            egui::ScrollArea::horizontal()
                .id_salt("agent-lanes")
                .max_width(width)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| lanes(self, ui));
            ui.add_space(kit::S1);
            ui.allocate_ui(egui::vec2(width, kit::CONTROL), |ui| tools(self, ui));
        } else {
            ui.allocate_ui_with_layout(
                egui::vec2(width, kit::CONTROL),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    lanes(self, ui);
                    tools(self, ui);
                },
            );
        }
        if let Some(f) = self.filter.clone() {
            ui.add_space(kit::S1);
            ui.horizontal(|ui| {
                kit::pill(ui, &theme, &format!("repo: {f}"), style::c(theme.accent));
                if kit::icon_button(ui, &theme, Icon::Close, "Show every repo").clicked() {
                    self.queue.push(Command::new("space.filter", None));
                }
            });
        }
        ui.add_space(kit::S2);
        let rows = self.listed_sessions();
        self.agent_keys(ui, &rows);
        let wide = ui.available_width() > 640.0;
        // Starting: launches that have not shown up yet.
        let launches = self.launches.clone();
        if !launches.is_empty() && self.lane == Lane::All {
            kit::section(ui, &theme, "Starting", Some(launches.len()), |_| {});
            for l in &launches {
                self.launch_row(ui, l, now, wide);
            }
            ui.add_space(kit::S2);
        }
        if rows.is_empty() && launches.is_empty() {
            let (title, hint) = if self.snap.sessions.is_empty() {
                (
                    "No agents running",
                    "⌘N starts one from a prompt, or run claude in any terminal.",
                )
            } else {
                (
                    "Nothing in this lane",
                    "Try another lane or clear the search.",
                )
            };
            kit::empty_with(ui, &theme, Icon::Terminal, title, hint);
        }
        for stage in Stage::ALL {
            let group: Vec<&Session> = rows.iter().filter(|s| self.stage(s) == stage).collect();
            if group.is_empty() {
                continue;
            }
            let n = group.len();
            kit::section(ui, &theme, stage.label(), Some(n), |ui| {
                if stage == Stage::Stale
                    && kit::danger(
                        ui,
                        &theme,
                        Some(Icon::Sweep),
                        &format!("Close all {n}"),
                        None,
                    )
                    .on_hover_text(format!(
                        "Idle for over {} min. Claude sessions can be reopened.",
                        self.settings().stale_after_mins
                    ))
                    .clicked()
                {
                    self.queue.push(Command::new("session.sweep", None));
                }
            });
            for s in group {
                self.agent_row(ui, s, now, wide);
            }
            ui.add_space(kit::S2);
        }
        self.closed_rows(ui, now);
        ui.add_space(kit::S2);
        ui.horizontal_wrapped(|ui| {
            kit::hint(ui, &theme, "j k", "move");
            kit::hint(ui, &theme, "↵", "open");
            kit::hint(ui, &theme, "f", "focus");
            kit::hint(ui, &theme, ".", "stop turn");
            kit::hint(ui, &theme, "x", "close");
            kit::hint(ui, &theme, "⌘J", "next that needs you");
        });
    }

    fn launch_row(&mut self, ui: &mut egui::Ui, l: &Launch, now: u64, wide: bool) {
        let theme = self.theme.clone();
        let place = match (&l.branch, &l.dir) {
            (Some(b), _) => format!("{} · {b}", l.repo),
            (None, Some(d)) => d
                .file_name()
                .map_or_else(|| l.repo.clone(), |n| n.to_string_lossy().into_owned()),
            _ => l.repo.clone(),
        };
        let mut cols = vec![Col::Px(16.0), Col::Flex];
        if wide {
            cols.push(Col::Px(170.0));
        }
        cols.push(Col::Px(ACTIONS_W));
        let working = style::c(theme.working);
        kit::row(
            ui,
            &theme,
            ("launch", l.at),
            kit::ROW2,
            false,
            &cols,
            |c, ui| match (c, wide) {
                (0, _) => kit::mark(ui, kit::Mark::Spin, working),
                (1, _) => {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        kit::text(ui, &l.name(), 13.0, style::c(theme.text));
                        kit::text(
                            ui,
                            &format!("starting {} in a new terminal tab…", l.agent),
                            11.5,
                            style::c(theme.faint),
                        );
                    });
                }
                (2, true) => {
                    kit::mono(ui, &place, 11.0, style::c(theme.dim));
                }
                _ => kit::right(
                    ui,
                    &ago(now.saturating_sub(l.at)),
                    11.5,
                    style::c(theme.faint),
                ),
            },
        );
    }

    fn closed_rows(&mut self, ui: &mut egui::Ui, now: u64) {
        let theme = self.theme.clone();
        if self.closed.entries.is_empty() || self.lane != Lane::All {
            return;
        }
        let open = self.closed_open;
        let n = self.closed.entries.len().min(CLOSED_ROWS);
        if kit::fold(ui, &theme, "closed", "Recently closed", Some(n), open) {
            self.closed_open = !open;
        }
        if !open {
            return;
        }
        let entries: Vec<_> = self
            .closed
            .entries
            .iter()
            .take(CLOSED_ROWS)
            .cloned()
            .collect();
        for c in entries {
            let can = c.reopen_line().is_some();
            let mut reopen = false;
            kit::row(
                ui,
                &theme,
                ("closed", &c.id),
                kit::ROW,
                false,
                &[Col::Px(16.0), Col::Flex, Col::Px(120.0), Col::Px(ACTIONS_W)],
                |col, ui| match col {
                    0 => kit::mark(ui, kit::Mark::Ghost, style::c(theme.unknown)),
                    1 => {
                        kit::text(ui, &c.name, 12.5, style::c(theme.dim));
                    }
                    2 => {
                        let what = format!(
                            "{} · {} ago",
                            c.repo.as_deref().unwrap_or(&c.agent),
                            ago(now.saturating_sub(c.at))
                        );
                        kit::text(ui, &what, 11.0, style::c(theme.faint));
                    }
                    _ => {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(can, |ui| {
                                if kit::icon_button(
                                    ui,
                                    &theme,
                                    Icon::Reopen,
                                    "Reopen (claude --resume)",
                                )
                                .clicked()
                                {
                                    reopen = true;
                                }
                            });
                        });
                    }
                },
            );
            if reopen {
                self.queue.push(Command::new("session.reopen", Some(&c.id)));
            }
        }
    }

    fn agent_row(&mut self, ui: &mut egui::Ui, s: &Session, now: u64, wide: bool) {
        let theme = self.theme.clone();
        let id = s.rec.id.clone();
        let on = self.selected.as_deref() == Some(id.as_str());
        let state = s.rec.state;
        let stage = self.stage(s);
        let sc = self.stage_color(stage, state);
        let sub = s
            .rec
            .message
            .clone()
            .filter(|_| state == AgentState::Blocked)
            .or_else(|| {
                s.rec
                    .activity
                    .clone()
                    .filter(|_| state == AgentState::Working)
                    .map(|a| format!("› {a}"))
            })
            .or_else(|| (state == AgentState::Done).then(|| "finished a turn".to_owned()));
        let height = if sub.is_some() { kit::ROW2 } else { kit::ROW };
        let next = ui.cursor().min;
        let hovered = ui.rect_contains_pointer(egui::Rect::from_min_size(
            next,
            egui::vec2(ui.available_width(), height),
        ));
        let mut cols = vec![Col::Px(16.0), Col::Flex];
        if wide {
            cols.extend([Col::Px(150.0), Col::Px(150.0)]);
        }
        cols.push(Col::Px(ACTIONS_W));
        let resources: Vec<Resource> = s.resources.iter().take(ROW_RESOURCES).cloned().collect();
        let mut clicked_resource = None;
        let mut action = None;
        let name_color = if stage == Stage::Stale {
            style::c(theme.dim)
        } else {
            style::c(theme.text)
        };
        let resp = kit::row(
            ui,
            &theme,
            // One conversation can be resumed in two terminals: the pid
            // tells their rows apart.
            ("agent", &id, s.rec.pid),
            height,
            on,
            &cols,
            |c, ui| match (c, wide) {
                (0, _) => kit::mark(ui, state_mark(state, stage == Stage::Stale), sc),
                (1, _) => {
                    let content_h = if sub.is_some() { 34.0 } else { 18.0 };
                    let rect = egui::Rect::from_center_size(
                        ui.max_rect().center(),
                        egui::vec2(ui.available_width(), content_h),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(s.name())
                                            .font(crate::fonts::semibold(13.0))
                                            .color(name_color),
                                    )
                                    .truncate(),
                                );
                                if s.rec.agent != "claude" {
                                    kit::pill(ui, &theme, &s.rec.agent, style::c(theme.faint));
                                }
                            });
                            if let Some(m) = &sub {
                                let color = match state {
                                    AgentState::Blocked => style::c(theme.blocked),
                                    AgentState::Done => style::c(theme.done),
                                    _ => style::c(theme.faint),
                                };
                                kit::text(ui, m, 11.5, color);
                            }
                        },
                    );
                }
                (2, true) => {
                    let extra = if s.resources.len() > ROW_RESOURCES {
                        26.0
                    } else {
                        0.0
                    };
                    let width = ((ui.available_width()
                        - extra
                        - ui.spacing().item_spacing.x * resources.len().saturating_sub(1) as f32)
                        / resources.len().max(1) as f32)
                        .max(0.0);
                    for r in &resources {
                        let color = self.resource_color(r);
                        let resp = ui
                            .add_sized(
                                [width, 18.0],
                                egui::Label::new(
                                    RichText::new(r.short()).monospace().size(11.0).color(color),
                                )
                                .truncate()
                                .sense(Sense::click()),
                            )
                            .on_hover_text(r.title.clone().unwrap_or_else(|| r.id.clone()));
                        if resp.clicked() {
                            clicked_resource = Some(r.clone());
                        }
                    }
                    if s.resources.len() > ROW_RESOURCES {
                        ui.label(
                            RichText::new(format!("+{}", s.resources.len() - ROW_RESOURCES))
                                .size(11.0)
                                .color(style::c(theme.faint)),
                        );
                    }
                }
                (3, true) => {
                    let place = match (&s.repo, &s.branch) {
                        (Some(r), Some(b)) => format!("{r} · {b}"),
                        (Some(r), None) => r.clone(),
                        _ => s.rec.agent.clone(),
                    };
                    kit::text(ui, &place, 11.5, style::c(theme.faint));
                }
                _ => {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if hovered || on {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if kit::icon_button_tinted(
                                ui,
                                &theme,
                                Icon::Close,
                                "Close agent (x)",
                                style::c(theme.danger),
                            )
                            .clicked()
                            {
                                action = Some("session.close");
                            }
                            if state == AgentState::Working
                                && s.rec.tty.is_some()
                                && kit::icon_button(ui, &theme, Icon::Stop, "Stop turn (.)")
                                    .clicked()
                            {
                                action = Some("session.stop");
                            }
                            if s.rec.tty.is_some()
                                && kit::icon_button(ui, &theme, Icon::Focus, "Focus terminal (f)")
                                    .clicked()
                            {
                                action = Some("session.focus");
                            }
                        } else {
                            ui.label(
                                RichText::new(ago(now.saturating_sub(s.rec.since)))
                                    .size(11.5)
                                    .color(style::c(theme.faint)),
                            );
                        }
                    });
                }
            },
        );
        if let Some(tc) = s.tag.as_deref().map(|tg| style::token(&theme, tg)) {
            let edge = egui::Rect::from_min_size(
                resp.rect.min + egui::vec2(0.0, 7.0),
                egui::vec2(3.0, resp.rect.height() - 14.0),
            );
            ui.painter().rect_filled(edge, CornerRadius::same(2), tc);
        }
        if let Some(cmd) = action {
            self.queue.push(Command::new(cmd, Some(&id)));
        } else if let Some(r) = clicked_resource {
            self.open_resource(&r);
        } else if resp.double_clicked() {
            self.queue.push(Command::new("session.open", Some(&id)));
        } else if resp.clicked() {
            self.queue.push(Command::new("agent.select", Some(&id)));
        }
        resp.context_menu(|ui| self.session_menu(ui, s));
    }

    fn open_resource(&mut self, r: &Resource) {
        match r.kind {
            ResourceKind::Pr => self.queue.push(Command::new("pr.open", Some(&r.id))),
            ResourceKind::Ticket => self.queue.push(Command::new("url.open", Some(&r.url))),
        }
    }

    /// Right-click menu for a session: the things a developer copies.
    fn session_menu(&mut self, ui: &mut egui::Ui, s: &Session) {
        let theme = self.theme.clone();
        let id = s.rec.id.clone();
        let has_tty = s.rec.tty.is_some();
        let mut fire = None;
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str, keys: Option<&str>| {
            let hit = kit::menu_action(ui, &theme, icon, label, keys).clicked();
            if hit {
                ui.close();
            }
            hit
        };
        if item(ui, Icon::ArrowUpRight, "Open as tab", Some("↵")) {
            fire = Some("session.open");
        }
        if has_tty && item(ui, Icon::Focus, "Focus terminal", Some("f")) {
            fire = Some("session.focus");
        }
        if s.rec.state == AgentState::Working
            && has_tty
            && item(ui, Icon::Stop, "Stop turn", Some("."))
        {
            fire = Some("session.stop");
        }
        if s.rec.agent == "claude" && has_tty && item(ui, Icon::CheckCheck, "Wrap up (/wrap)", None)
        {
            fire = Some("session.wrap");
        }
        if let Some(w) = &s.worktree {
            kit::menu_rule(ui, &theme);
            let path = w.display().to_string();
            if item(ui, Icon::Editor, "Open in editor", None) {
                self.queue.push(Command::new("path.edit", Some(&path)));
            }
            if item(ui, Icon::FolderOpen, "Reveal in Finder", None) {
                self.queue.push(Command::new("path.open", Some(&path)));
            }
        }
        kit::menu_rule(ui, &theme);
        let mut copies = vec![("Copy session id", id.clone())];
        if s.rec.agent == "claude" && !id.starts_with("pid-") {
            copies.push((
                "Copy resume command",
                format!(
                    "cd {} && claude --resume {id}",
                    shell_quote(&s.rec.cwd.display().to_string())
                ),
            ));
        }
        if let Some(w) = &s.worktree {
            copies.push((
                "Copy cd to worktree",
                format!("cd {}", shell_quote(&w.display().to_string())),
            ));
            copies.push(("Copy worktree path", w.display().to_string()));
        }
        if let Some(b) = &s.branch {
            copies.push(("Copy branch", b.clone()));
        }
        for (label, value) in copies {
            let r = kit::menu_action(ui, &theme, Icon::Copy, label, None).on_hover_text(&value);
            if r.clicked() {
                ui.ctx().copy_text(value);
                ui.close();
            }
        }
        kit::menu_rule(ui, &theme);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            for (name, rgba) in tags::PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 22.0), Sense::click());
                let r = if resp.hovered() { 6.5 } else { 5.5 };
                ui.painter().circle_filled(rect.center(), r, style::c(rgba));
                if resp.on_hover_text(name).clicked() {
                    self.queue
                        .push(Command::new("session.tag", Some(&format!("{id}:{name}"))));
                    ui.close();
                }
            }
        });
        kit::menu_rule(ui, &theme);
        if item(ui, Icon::Close, "Close agent", Some("x")) {
            fire = Some("session.close");
        }
        if let Some(cmd) = fire {
            self.queue.push(Command::new(cmd, Some(&id)));
        }
    }

    fn spaces(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let spaces = self.snap.spaces.clone();
        if spaces.is_empty() {
            kit::empty(
                ui,
                &theme,
                "No repos yet",
                "A repo appears once an agent touches it.",
            );
            return;
        }
        for sp in &spaces {
            let in_space: Vec<Session> = self
                .snap
                .sessions
                .iter()
                .filter(|s| s.repo.as_ref() == Some(&sp.repo))
                .cloned()
                .collect();
            let agg = AgentState::most_urgent(in_space.iter().map(|s| s.rec.state))
                .unwrap_or(AgentState::Unknown);
            let filtered = self.filter.as_ref() == Some(&sp.repo);
            let open = !self.spaces_closed.contains(&sp.repo);
            let resp = kit::row(
                ui,
                &theme,
                ("space", &sp.repo),
                kit::ROW,
                filtered,
                &[
                    Col::Px(12.0),
                    Col::Px(12.0),
                    Col::Flex,
                    Col::Px(70.0),
                    Col::Px(26.0),
                ],
                |c, ui| match c {
                    0 => kit::icon(
                        ui,
                        if open {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        11.0,
                        style::c(theme.faint),
                    ),
                    1 => kit::dot(ui, style::state(&theme, agg), 4.0),
                    2 => {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&sp.repo).font(crate::fonts::semibold(13.0)),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&sp.repo);
                    }
                    3 => kit::right(
                        ui,
                        &format!("{} agents", in_space.len()),
                        11.0,
                        style::c(theme.dim),
                    ),
                    _ => {
                        if kit::icon_button(ui, &theme, Icon::Plus, "New agent in a new worktree")
                            .clicked()
                        {
                            self.queue.push(Command::new("spawn.open", Some(&sp.repo)));
                        }
                    }
                },
            );
            if resp.clicked() {
                if open {
                    self.spaces_closed.push(sp.repo.clone());
                } else {
                    self.spaces_closed.retain(|r| r != &sp.repo);
                }
            }
            if resp.double_clicked() {
                let arg = if filtered { "" } else { sp.repo.as_str() };
                self.queue.push(Command::new("space.filter", Some(arg)));
            }
            let slug = sp.slug.clone();
            resp.on_hover_text(
                "Click to expand · double-click to filter agents · right-click for more",
            )
            .context_menu(|ui| {
                if let Some(s) = &slug
                    && kit::menu_action(ui, &theme, Icon::Repo, &format!("Open {s}"), None)
                        .clicked()
                {
                    self.queue.push(Command::new("repo.open", Some(s)));
                    ui.close();
                }
                if kit::menu_action(ui, &theme, Icon::Filter, "Filter agents to this repo", None)
                    .clicked()
                {
                    self.queue
                        .push(Command::new("space.filter", Some(&sp.repo)));
                    ui.close();
                }
                if kit::menu_action(ui, &theme, Icon::Plus, "New agent in a new worktree", None)
                    .clicked()
                {
                    self.queue.push(Command::new("spawn.open", Some(&sp.repo)));
                    ui.close();
                }
            });
            if !open {
                continue;
            }
            for wt in &sp.worktrees {
                let agents: Vec<&Session> = in_space
                    .iter()
                    .filter(|s| s.worktree.as_ref() == Some(&wt.path))
                    .collect();
                let label = wt.branch.clone().unwrap_or_else(|| {
                    wt.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
                let mut status = Vec::new();
                if wt.dirty > 0 {
                    status.push(format!("~{}", wt.dirty));
                }
                if wt.ahead > 0 {
                    status.push(format!("↑{}", wt.ahead));
                }
                if wt.behind > 0 {
                    status.push(format!("↓{}", wt.behind));
                }
                let r = kit::row(
                    ui,
                    &theme,
                    ("wt", &wt.path),
                    24.0,
                    false,
                    &[Col::Px(18.0), Col::Flex, Col::Px(60.0), Col::Px(70.0)],
                    |c, ui| match c {
                        1 => {
                            let color = if agents.is_empty() {
                                style::c(theme.dim)
                            } else {
                                style::c(theme.text)
                            };
                            kit::mono(ui, &label, 11.5, color);
                        }
                        2 => {
                            for a in agents.iter().take(5) {
                                kit::dot(ui, style::state(&theme, a.rec.state), 3.0);
                            }
                        }
                        3 => kit::right(ui, &status.join(" "), 11.0, style::c(theme.blocked)),
                        _ => {}
                    },
                );
                let path = wt.path.display().to_string();
                let r = r.on_hover_text(&path);
                r.context_menu(|ui| {
                    for (icon, label, cmd) in [
                        (Icon::Plus, "New agent here", "spawn.open"),
                        (Icon::Editor, "Open in editor", "path.edit"),
                        (Icon::FolderOpen, "Reveal in Finder", "path.open"),
                    ] {
                        if kit::menu_action(ui, &theme, icon, label, None).clicked() {
                            self.queue.push(Command::new(cmd, Some(&path)));
                            ui.close();
                        }
                    }
                    kit::menu_rule(ui, &theme);
                    if kit::menu_action(ui, &theme, Icon::Copy, "Copy path", None).clicked() {
                        ui.ctx().copy_text(path.clone());
                        ui.close();
                    }
                    if let Some(b) = &wt.branch
                        && kit::menu_action(ui, &theme, Icon::Branch, "Copy branch", None).clicked()
                    {
                        ui.ctx().copy_text(b.clone());
                        ui.close();
                    }
                });
            }
        }
    }

    fn inbox(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let items = self.snap.inbox.clone();
        let open = items.iter().filter(|i| !i.done).count();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), kit::CONTROL),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    RichText::new(format!("{open} open"))
                        .size(11.5)
                        .color(self.dimc()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    kit::check(ui, &theme, &mut self.hide_done, "hide ticked", 11.5);
                });
            },
        );
        if items.is_empty() {
            kit::empty(
                ui,
                &theme,
                "Inbox zero",
                "No zaindu proposals in the last days.",
            );
            return;
        }
        for kind in ["review", "red", "approved", "merged", ""] {
            let known = ["review", "red", "approved", "merged"];
            let group: Vec<(usize, &artalde_core::model::InboxItem)> = items
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    (if kind.is_empty() {
                        !known.contains(&i.kind.as_str())
                    } else {
                        i.kind == kind
                    }) && !(self.hide_done && i.done)
                })
                .collect();
            if group.is_empty() {
                continue;
            }
            let color = style::c(match kind {
                "review" => theme.blocked,
                "red" => theme.danger,
                "approved" => theme.done,
                "merged" => theme.dim,
                _ => theme.accent,
            });
            ui.add_space(kit::S1);
            kit::section(
                ui,
                &theme,
                if kind.is_empty() { "other" } else { kind },
                Some(group.len()),
                |_| {},
            );
            for (idx, item) in group {
                let mut toggled = false;
                let target_short = item
                    .target
                    .rsplit('/')
                    .next()
                    .unwrap_or(&item.target)
                    .to_owned();
                let resp = kit::row(
                    ui,
                    &theme,
                    ("zaindu", idx),
                    40.0,
                    false,
                    &[Col::Px(18.0), Col::Px(10.0), Col::Flex, Col::Px(40.0)],
                    |c, ui| match c {
                        0 => {
                            let mut done = item.done;
                            if kit::check(ui, &theme, &mut done, "", 11.5)
                                .on_hover_text("Tick in the wiki")
                                .changed()
                            {
                                toggled = true;
                            }
                        }
                        1 => kit::dot(ui, color, 3.5),
                        2 => {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                kit::mono(
                                    ui,
                                    &target_short,
                                    12.0,
                                    if item.done {
                                        style::c(theme.dim)
                                    } else {
                                        style::c(theme.text)
                                    },
                                );
                                kit::text(ui, &item.detail, 11.0, style::c(theme.dim));
                            });
                        }
                        _ => kit::right(
                            ui,
                            item.time.trim_end_matches('Z'),
                            11.0,
                            style::c(theme.unknown),
                        ),
                    },
                );
                if toggled {
                    self.queue
                        .push(Command::new("inbox.toggle", Some(&idx.to_string())));
                } else if resp.clicked()
                    && let Some(url) = &item.url
                {
                    self.open_link(url);
                }
                resp.on_hover_text(format!("{} {}\n{}", item.kind, item.target, item.detail));
            }
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let Some(s) = self.selected_session().cloned() else {
            kit::empty_with(
                ui,
                &theme,
                Icon::Terminal,
                "No agent selected",
                "Pick one from the list, or ⌘J for the next that needs you.",
            );
            return;
        };
        let now = artalde_core::now();
        let stage = self.stage(&s);
        let sc = self.stage_color(stage, s.rec.state);
        let in_tab = matches!(self.tabs.get(self.active), Some(Tab::Session(_)));
        // Header: state, age, handle.
        ui.horizontal(|ui| {
            let label = if stage == Stage::Stale {
                "stale"
            } else {
                s.rec.state.as_str()
            };
            kit::pill(ui, &theme, label, sc);
            ui.label(
                RichText::new(format!("for {}", ago(now.saturating_sub(s.rec.since))))
                    .size(11.5)
                    .color(style::c(theme.faint)),
            );
            if s.rec.agent != "claude" {
                kit::pill(ui, &theme, &s.rec.agent, style::c(theme.dim));
            }
            if let Some(h) = &s.handle {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(h)
                            .monospace()
                            .size(10.5)
                            .color(style::c(theme.faint)),
                    );
                });
            }
        });
        ui.add_space(kit::S1);
        if !in_tab {
            ui.add(
                egui::Label::new(RichText::new(s.name()).font(crate::fonts::display(20.0))).wrap(),
            );
        }
        if s.label.is_some()
            && let Some(t) = &s.ai_title
        {
            ui.label(RichText::new(t).size(12.5).color(self.dimc()));
        }
        ui.add_space(kit::S2);
        self.lifecycle_bar(ui, &s, in_tab);
        // Blocked: the question and quick keys.
        if let Some(m) = s
            .rec
            .message
            .as_ref()
            .filter(|_| s.rec.state == AgentState::Blocked)
        {
            ui.add_space(kit::S3);
            egui::Frame::new()
                .fill(tint(style::c(theme.surface), sc, 0.10))
                .stroke(Stroke::new(1.0, tint(style::c(theme.surface), sc, 0.45)))
                .corner_radius(CornerRadius::same(kit::CARD_RADIUS))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new("WAITING ON YOU")
                            .font(crate::fonts::semibold(10.0))
                            .extra_letter_spacing(0.6)
                            .color(sc),
                    );
                    ui.add_space(kit::S1);
                    ui.label(RichText::new(m).color(style::c(theme.text)));
                    if s.rec.tty.is_some() {
                        ui.add_space(kit::S2);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (label, key) in [
                                ("1  Yes", "1"),
                                ("2", "2"),
                                ("3", "3"),
                                ("Enter", "enter"),
                                ("Esc", "esc"),
                            ] {
                                if kit::button(ui, &theme, label, None).clicked() {
                                    self.queue.push(Command::new("session.key", Some(key)));
                                }
                            }
                        });
                    }
                });
        }
        if s.rec.tty.is_some() {
            ui.add_space(kit::S3);
            self.reply_box(ui);
        }
        // Tag and label.
        ui.add_space(kit::S3);
        self.tag_row(ui, &s);
        // Related.
        if !s.resources.is_empty() {
            ui.add_space(kit::S3);
            let n = s.resources.len().to_string();
            kit::card(ui, &theme, "Related", Some(&n), |ui| {
                for r in &s.resources {
                    let color = self.resource_color(r);
                    let resp = kit::row(
                        ui,
                        &theme,
                        ("res", &r.id),
                        26.0,
                        false,
                        &[Col::Px(96.0), Col::Px(84.0), Col::Flex],
                        |c, ui| match c {
                            0 => {
                                kit::mono(ui, &r.short(), 11.5, color);
                            }
                            1 => {
                                kit::text(ui, r.state.as_deref().unwrap_or("—"), 11.0, color);
                            }
                            _ => {
                                kit::text(
                                    ui,
                                    r.title.as_deref().unwrap_or(&r.id),
                                    11.5,
                                    style::c(theme.dim),
                                );
                            }
                        },
                    );
                    let resp = resp.on_hover_text(format!(
                        "{}\n{}",
                        r.title.clone().unwrap_or_default(),
                        r.url
                    ));
                    if resp.clicked() {
                        self.open_resource(r);
                    }
                }
            });
        }
        if !in_tab {
            if let Some(said) = &s.last_said {
                ui.add_space(kit::S3);
                let said = said.clone();
                kit::card(ui, &theme, "Last said", None, |ui| self.md(ui, &said, 13.0));
            }
            if let Some(p) = &s.rec.last_prompt {
                ui.add_space(kit::S3);
                let p = p.clone();
                kit::card(ui, &theme, "Last prompt", None, |ui| self.md(ui, &p, 13.0));
            }
        }
        // Details.
        ui.add_space(kit::S3);
        let copied = kit::card(ui, &theme, "Details", Some("click to copy"), |ui| {
            let mut copied = false;
            copied |= kit::kv(ui, &theme, "agent", &s.rec.agent, LABEL_W);
            if let Some(r) = &s.repo {
                copied |= kit::kv(ui, &theme, "repo", r, LABEL_W);
            }
            if let Some(b) = &s.branch {
                copied |= kit::kv(ui, &theme, "branch", b, LABEL_W);
            }
            if let Some(w) = &s.worktree {
                copied |= kit::kv(ui, &theme, "worktree", &w.display().to_string(), LABEL_W);
            }
            if s.worktree.as_deref() != Some(s.rec.cwd.as_path()) {
                copied |= kit::kv(ui, &theme, "cwd", &s.rec.cwd.display().to_string(), LABEL_W);
            }
            if let Some(a) = &s.rec.activity {
                copied |= kit::kv(ui, &theme, "doing", a, LABEL_W);
            }
            if let Some(task) = &s.task {
                copied |= kit::kv(
                    ui,
                    &theme,
                    "task",
                    &format!("{task} (+{} pending)", s.pending_tasks),
                    LABEL_W,
                );
            }
            if let Some(t) = &s.rec.tty {
                copied |= kit::kv(ui, &theme, "tty", t, LABEL_W);
            }
            if let Some(p) = s.rec.pid {
                copied |= kit::kv(ui, &theme, "pid", &p.to_string(), LABEL_W);
            }
            copied |= kit::kv(ui, &theme, "session", &s.rec.id, LABEL_W);
            copied
        });
        if copied {
            let ctx = ui.ctx().clone();
            self.toast_line(&ctx, "copied".into());
        }
        if s.discovered && s.rec.state == AgentState::Unknown {
            ui.add_space(kit::S2);
            ui.label(
                RichText::new("Found as a process; it reports no state.")
                    .size(11.5)
                    .color(style::c(theme.faint)),
            );
        }
    }

    /// The opinionated next step for where the session is, then the rest.
    fn lifecycle_bar(&mut self, ui: &mut egui::Ui, s: &Session, in_tab: bool) {
        let theme = self.theme.clone();
        let id = s.rec.id.clone();
        let has_tty = s.rec.tty.is_some();
        let claude = s.rec.agent == "claude";
        let mut fire: Vec<&'static str> = Vec::new();
        // Key chips only when the whole bar fits on one line with them.
        let roomy = ui.available_width() >= 560.0;
        // Narrower still: the ghost buttons drop their labels too.
        let compact = ui.available_width() < 400.0;
        let k = |key: &'static str| roomy.then_some(key);
        let ghost = |ui: &mut egui::Ui, icon: Icon, label: &str, key: &'static str| {
            if compact {
                kit::icon_button(ui, &theme, icon, &format!("{label} ({key})"))
            } else {
                kit::button_icon(ui, &theme, icon, label, k(key))
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            match s.rec.state {
                AgentState::Blocked if has_tty => {
                    let answer = kit::primary_with(
                        ui,
                        &theme,
                        Some(Icon::Focus),
                        "Answer",
                        k("f"),
                        style::c(theme.blocked),
                    );
                    if answer.clicked() {
                        fire.push("session.focus");
                    }
                }
                AgentState::Done if claude && has_tty => {
                    if kit::primary_with(
                        ui,
                        &theme,
                        Some(Icon::Check),
                        "Wrap up",
                        None,
                        style::c(theme.done),
                    )
                    .on_hover_text("Type /wrap: verify, commit, push, draft PR")
                    .clicked()
                    {
                        fire.push("session.wrap");
                    }
                    if ghost(ui, Icon::Focus, "Focus", "f").clicked() {
                        fire.push("session.focus");
                    }
                }
                AgentState::Working if has_tty => {
                    if ghost(ui, Icon::Stop, "Stop turn", ".").clicked() {
                        fire.push("session.stop");
                    }
                    if ghost(ui, Icon::Focus, "Focus", "f").clicked() {
                        fire.push("session.focus");
                    }
                }
                _ if has_tty && ghost(ui, Icon::Focus, "Focus", "f").clicked() => {
                    fire.push("session.focus");
                }
                _ => {}
            }
            if !in_tab
                && kit::icon_button(ui, &theme, Icon::ChevronRight, "Open as a tab (↵)").clicked()
            {
                fire.push("session.open");
            }
            if let Some(w) = &s.worktree
                && kit::icon_button(
                    ui,
                    &theme,
                    Icon::Editor,
                    &format!("Open {} in the editor", w.display()),
                )
                .clicked()
            {
                self.queue
                    .push(Command::new("path.edit", Some(&w.display().to_string())));
            }
            let more = kit::icon_button(ui, &theme, Icon::More, "More");
            egui::Popup::menu(&more).show(|ui| self.session_menu(ui, s));
            // Narrow: Close shrinks to its icon rather than wrap onto a
            // line of its own.
            let close = if roomy {
                kit::danger(ui, &theme, Some(Icon::Close), "Close", k("x"))
                    .on_hover_text("End the agent and its terminal tab")
            } else {
                kit::icon_button_tinted(
                    ui,
                    &theme,
                    Icon::Close,
                    "Close: end the agent and its terminal tab (x)",
                    style::c(theme.danger),
                )
            };
            if close.clicked() {
                fire.push("session.close");
            }
        });
        for cmd in fire {
            self.queue.push(Command::new(cmd, Some(&id)));
        }
    }

    fn reply_box(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let send = egui::Frame::new()
            .fill(tint(style::c(theme.surface), style::c(theme.text), 0.05))
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius::same(kit::BUTTON_RADIUS))
            .inner_margin(egui::Margin::symmetric(10, 2))
            .outer_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.reply)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .min_size(egui::vec2(0.0, kit::CONTROL))
                            .align(egui::Align2::LEFT_CENTER)
                            .hint_text(
                                RichText::new("Reply in its terminal…")
                                    .color(style::c(theme.faint)),
                            )
                            .desired_width(
                                ui.available_width() - SEND_W - ui.spacing().item_spacing.x,
                            ),
                    );
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let tint_c = if self.reply.trim().is_empty() {
                        style::c(theme.faint)
                    } else {
                        style::c(theme.accent)
                    };
                    kit::icon_button_tinted(ui, &theme, Icon::Send, "Send (↵)", tint_c).clicked()
                        || enter
                })
                .inner
            })
            .inner;
        if send && !self.reply.trim().is_empty() {
            let text = std::mem::take(&mut self.reply);
            self.queue
                .push(Command::new("session.send", Some(text.trim())));
        }
    }

    fn tag_row(&mut self, ui: &mut egui::Ui, s: &Session) {
        let theme = self.theme.clone();
        if self.label_for.as_deref() != Some(s.rec.id.as_str()) {
            self.label_edit = s.label.clone().unwrap_or_default();
            self.label_for = Some(s.rec.id.clone());
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (name, rgba) in tags::PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                let on = s.tag.as_deref() == Some(name);
                let r = if on || resp.hovered() { 6.5 } else { 5.0 };
                ui.painter().circle_filled(rect.center(), r, style::c(rgba));
                if on {
                    ui.painter().circle_stroke(
                        rect.center(),
                        8.5,
                        Stroke::new(1.5, style::c(theme.text)),
                    );
                }
                if resp
                    .on_hover_text(format!("Tag {name} (also its iTerm tab)"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.queue.push(Command::new(
                        "session.tag",
                        Some(if on { "none" } else { name }),
                    ));
                }
            }
            ui.add_space(kit::S3);
            let w = ui.available_width();
            let edit = kit::input(
                ui,
                &theme,
                &mut self.label_edit,
                "Label this agent",
                w,
                false,
            );
            if edit.lost_focus() && self.label_edit.trim() != s.label.clone().unwrap_or_default() {
                self.queue
                    .push(Command::new("session.label", Some(self.label_edit.trim())));
            }
        });
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let errors: Vec<String> = self
            .config
            .as_ref()
            .map(|c| c.errors.clone())
            .unwrap_or_default()
            .into_iter()
            .chain(self.snap.errors.clone())
            .collect();
        let layout = self.current_layout_name();
        let show_hints = ui.available_width() >= 1100.0;
        let mut counts = [0usize; 4];
        for s in &self.snap.sessions {
            let i = Stage::ALL
                .iter()
                .position(|st| *st == self.stage(s))
                .unwrap_or(0);
            counts[i] += 1;
        }
        let strip = egui::vec2(ui.available_width(), kit::CONTROL);
        ui.allocate_ui_with_layout(
            strip,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (stage, n) in Stage::ALL.iter().zip(counts) {
                    let color = match (stage, n) {
                        (_, 0) => style::c(theme.faint),
                        (Stage::NeedsYou, _) => style::c(theme.blocked),
                        (Stage::Working, _) => style::c(theme.working),
                        (Stage::Idle, _) => style::c(theme.idle),
                        (Stage::Stale, _) => style::c(theme.dim),
                    };
                    let r = kit::pill(
                        ui,
                        &theme,
                        &format!("{n} {}", stage.label().to_lowercase()),
                        color,
                    );
                    let lane = Lane::Only(*stage);
                    if ui
                        .interact(
                            r.rect,
                            ui.scope_id().with(("status-lane", stage.label())),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Show this lane")
                        .clicked()
                    {
                        self.lane = if self.lane == lane { Lane::All } else { lane };
                    }
                }
                if !self.launches.is_empty() {
                    kit::pill(
                        ui,
                        &theme,
                        &format!("{} starting", self.launches.len()),
                        style::c(theme.working),
                    );
                }
                if !errors.is_empty() {
                    kit::pill(
                        ui,
                        &theme,
                        &format!("{} errors", errors.len()),
                        style::c(theme.danger),
                    )
                    .on_hover_text(errors.join("\n"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if kit::pill_tab(ui, &theme, &theme.name, None, false, style::c(theme.accent))
                        .on_hover_text("Switch theme")
                        .clicked()
                    {
                        self.queue.push(Command::new("theme.cycle", None));
                    }
                    if kit::pill_tab(ui, &theme, &layout, None, false, style::c(theme.accent))
                        .on_hover_text("Switch layout (⇧⌘L)")
                        .clicked()
                    {
                        self.queue.push(Command::new("layout.cycle", None));
                    }
                    ui.add_space(kit::S3);
                    if show_hints {
                        ui.allocate_ui_with_layout(
                            egui::vec2(400.0, kit::CONTROL),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                kit::hint(ui, &theme, "⌘K", "commands");
                                kit::hint(ui, &theme, "⌘N", "new agent");
                                kit::hint(ui, &theme, "⌘⇧T", "reopen");
                                kit::hint(ui, &theme, "⌘I", "PRs");
                            },
                        );
                    }
                });
            },
        );
    }

    pub(crate) fn resource_color(&self, r: &Resource) -> egui::Color32 {
        let st = r.state.as_deref().unwrap_or_default().to_lowercase();
        let token = if st.contains("red") || st.contains("closed") || st.contains("cancel") {
            self.theme.danger
        } else if st.contains("merged")
            || st.contains("approved")
            || st.contains("done")
            || st.contains("production")
        {
            self.theme.done
        } else if st.contains("draft") || st.is_empty() {
            self.theme.dim
        } else {
            self.theme.working
        };
        style::c(token)
    }

    fn log_panel(&mut self, ui: &mut egui::Ui) {
        if self.log.is_empty() {
            kit::empty(
                ui,
                &self.theme.clone(),
                "Quiet",
                "Actions, config reloads and state changes land here.",
            );
        }
        kit::selectable_text(ui);
        for line in self.log.iter().rev() {
            ui.label(RichText::new(line).monospace().size(11.5));
        }
    }
}
