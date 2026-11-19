//! Built-in panels: agents, spaces, inbox, prs, detail, status, log. All
//! lists use the kit's fixed-height rows so columns line up.

use artalde_core::command::Command;
use artalde_core::model::{AgentState, Resource, ResourceKind, Session};
use artalde_core::tags;
use artalde_core::text::{ago, shell_quote};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};

use crate::app::App;
use crate::kit::{self, Col};
use crate::style::{self, tint};
use crate::views::Tab;

/// Resource chips on an agent row.
const ROW_RESOURCES: usize = 2;
const LABEL_W: f32 = 72.0;

/// Which agents the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentView {
    #[default]
    All,
    NeedsYou,
    Working,
    Idle,
}

impl AgentView {
    const ALL: [AgentView; 4] = [
        AgentView::All,
        AgentView::NeedsYou,
        AgentView::Working,
        AgentView::Idle,
    ];

    fn label(self) -> &'static str {
        match self {
            AgentView::All => "All",
            AgentView::NeedsYou => "Needs you",
            AgentView::Working => "Working",
            AgentView::Idle => "Idle",
        }
    }

    fn admits(self, s: AgentState) -> bool {
        match self {
            AgentView::All => true,
            AgentView::NeedsYou => matches!(s, AgentState::Blocked | AgentState::Done),
            AgentView::Working => s == AgentState::Working,
            AgentView::Idle => matches!(s, AgentState::Idle | AgentState::Unknown),
        }
    }
}

/// The groups the list shows, in order.
const GROUPS: [(&str, &[AgentState]); 3] = [
    ("Needs you", &[AgentState::Blocked, AgentState::Done]),
    ("Working", &[AgentState::Working]),
    ("Idle", &[AgentState::Idle, AgentState::Unknown]),
];

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

    /// Sessions the agent list shows, in display order.
    pub(crate) fn listed_sessions(&self) -> Vec<Session> {
        let q = self.agent_query.trim().to_lowercase();
        let mut out = Vec::new();
        for (_, states) in GROUPS {
            for s in self.visible_sessions() {
                if !states.contains(&s.rec.state) || !self.agent_view.admits(s.rec.state) {
                    continue;
                }
                if !q.is_empty() {
                    let hay = format!(
                        "{} {} {} {} {}",
                        s.name(),
                        s.handle.clone().unwrap_or_default(),
                        s.repo.clone().unwrap_or_default(),
                        s.branch.clone().unwrap_or_default(),
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
        let (down, up, enter, focus) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::J)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::K)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F),
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
            if enter {
                self.queue.push(Command::new("session.open", Some(&id)));
            }
            if focus {
                self.queue.push(Command::new("session.focus", Some(&id)));
            }
        }
    }

    fn agents(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        let t = ui.input(|i| i.time);
        // Toolbar: view switch, search, new.
        ui.horizontal(|ui| {
            let labels: Vec<(String, Option<usize>)> = AgentView::ALL
                .iter()
                .map(|v| {
                    (
                        v.label().to_owned(),
                        Some(
                            self.snap
                                .sessions
                                .iter()
                                .filter(|s| v.admits(s.rec.state))
                                .count(),
                        ),
                    )
                })
                .collect();
            let sel = AgentView::ALL
                .iter()
                .position(|v| *v == self.agent_view)
                .unwrap_or(0);
            if let Some(i) = kit::tabs(ui, &theme, &labels, sel) {
                self.agent_view = AgentView::ALL[i.min(3)];
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, &theme, "New agent", Some("⌘N")).clicked() {
                    self.queue.push(Command::new("spawn.open", None));
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.agent_query)
                        .hint_text("Search agents, repos, PRs…")
                        .desired_width(220.0),
                );
            });
        });
        if let Some(f) = self.filter.clone() {
            ui.horizontal(|ui| {
                kit::pill(ui, &theme, &format!("repo: {f}"), style::c(theme.accent));
                if ui.small_button("clear").clicked() {
                    self.queue.push(Command::new("space.filter", None));
                }
            });
        }
        ui.add_space(kit::S1);
        let rows = self.listed_sessions();
        self.agent_keys(ui, &rows);
        if rows.is_empty() {
            let (title, hint) = if self.snap.sessions.is_empty() {
                (
                    "No agents running",
                    "Cmd+N starts one, or run claude in any terminal.",
                )
            } else {
                ("Nothing here", "Try another view or clear the search.")
            };
            kit::empty(ui, &theme, title, hint);
            return;
        }
        let width = ui.available_width();
        let wide = width > 560.0;
        let mut index = 0usize;
        for (title, states) in GROUPS {
            let group: Vec<&Session> = rows
                .iter()
                .filter(|s| states.contains(&s.rec.state))
                .collect();
            if group.is_empty() {
                continue;
            }
            ui.add_space(kit::S1);
            kit::section(ui, &theme, title, Some(group.len()), |_| {});
            for s in group {
                index += 1;
                self.agent_row(ui, s, index, now, t, wide);
            }
        }
        ui.add_space(kit::S2);
        ui.horizontal(|ui| {
            kit::hint(ui, &theme, "j k", "move");
            kit::hint(ui, &theme, "↵", "open");
            kit::hint(ui, &theme, "f", "focus terminal");
            kit::hint(ui, &theme, "⌘J", "next that needs you");
        });
    }

    fn agent_row(
        &mut self,
        ui: &mut egui::Ui,
        s: &Session,
        index: usize,
        now: u64,
        t: f64,
        wide: bool,
    ) {
        let theme = self.theme.clone();
        let id = s.rec.id.clone();
        let on = self.selected.as_deref() == Some(id.as_str());
        let state = s.rec.state;
        let sc = self.color(state);
        let sub = s
            .rec
            .message
            .clone()
            .or_else(|| s.rec.activity.clone().map(|a| format!("› {a}")));
        let height = if sub.is_some() { kit::ROW2 } else { kit::ROW };
        let mut cols = vec![Col::Px(14.0), Col::Px(18.0), Col::Flex];
        if wide {
            cols.extend([Col::Px(150.0), Col::Px(130.0)]);
        }
        cols.push(Col::Px(40.0));
        let resources: Vec<Resource> = s.resources.iter().take(ROW_RESOURCES).cloned().collect();
        let mut clicked_resource = None;
        let resp = kit::row(
            ui,
            &theme,
            ("agent", &id),
            height,
            on,
            &cols,
            |c, ui| match (c, wide) {
                (0, _) => {
                    #[expect(clippy::cast_possible_truncation, reason = "an animation phase")]
                    let pulse = if state == AgentState::Blocked {
                        (0.55 + 0.45 * (t * 3.0).sin()) as f32
                    } else {
                        1.0
                    };
                    kit::dot(ui, sc.gamma_multiply(pulse), 4.0);
                }
                (1, _) => {
                    if index <= 9 {
                        ui.label(
                            RichText::new(index.to_string())
                                .monospace()
                                .size(11.0)
                                .color(style::c(theme.unknown)),
                        );
                    }
                }
                (2, _) => {
                    ui.vertical(|ui| {
                        ui.add_space(if sub.is_some() { 4.0 } else { 0.0 });
                        ui.horizontal(|ui| {
                            kit::text(ui, &s.name(), 13.5, style::c(theme.text));
                            if let Some(h) = &s.handle {
                                ui.label(
                                    RichText::new(h)
                                        .monospace()
                                        .size(10.5)
                                        .color(style::c(theme.unknown)),
                                );
                            }
                        });
                        if let Some(m) = &sub {
                            let color = if s.rec.message.is_some() {
                                style::c(theme.blocked)
                            } else {
                                style::c(theme.dim)
                            };
                            kit::text(ui, m, 11.5, color);
                        }
                    });
                }
                (3, true) => {
                    for r in &resources {
                        let color = self.resource_color(r);
                        let resp = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(r.short()).monospace().size(11.0).color(color),
                                )
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
                                .color(style::c(theme.unknown)),
                        );
                    }
                }
                (4, true) => {
                    let place = match (&s.repo, &s.branch) {
                        (Some(r), Some(b)) => format!("{r} · {b}"),
                        (Some(r), None) => r.clone(),
                        _ => s.rec.agent.clone(),
                    };
                    kit::text(ui, &place, 11.5, style::c(theme.dim));
                }
                _ => kit::right(
                    ui,
                    &ago(now.saturating_sub(s.rec.since)),
                    11.5,
                    style::c(theme.dim),
                ),
            },
        );
        if let Some(tc) = s.tag.as_deref().map(|tg| style::token(&theme, tg)) {
            let edge = egui::Rect::from_min_size(
                resp.rect.min + egui::vec2(0.0, 5.0),
                egui::vec2(3.0, resp.rect.height() - 10.0),
            );
            ui.painter().rect_filled(edge, CornerRadius::same(2), tc);
        }
        if let Some(r) = clicked_resource {
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
        let id = s.rec.id.clone();
        if ui.button("Open as tab").clicked() {
            self.queue.push(Command::new("session.open", Some(&id)));
        }
        if ui
            .add_enabled(s.rec.tty.is_some(), egui::Button::new("Focus terminal"))
            .clicked()
        {
            self.queue.push(Command::new("session.focus", Some(&id)));
        }
        if let Some(w) = &s.worktree {
            if ui.button("Open in editor").clicked() {
                self.queue
                    .push(Command::new("path.edit", Some(&w.display().to_string())));
            }
            if ui.button("Reveal in Finder").clicked() {
                self.queue
                    .push(Command::new("path.open", Some(&w.display().to_string())));
            }
        }
        ui.separator();
        let copy = |ui: &mut egui::Ui, label: &str, value: String| {
            if ui.button(label).on_hover_text(&value).clicked() {
                ui.ctx().copy_text(value);
            }
        };
        copy(ui, "Copy session id", id.clone());
        if s.rec.agent == "claude" && !id.starts_with("pid-") {
            copy(
                ui,
                "Copy resume command",
                format!(
                    "cd {} && claude --resume {id}",
                    shell_quote(&s.rec.cwd.display().to_string())
                ),
            );
        }
        if let Some(w) = &s.worktree {
            copy(
                ui,
                "Copy cd to worktree",
                format!("cd {}", shell_quote(&w.display().to_string())),
            );
            copy(ui, "Copy worktree path", w.display().to_string());
        }
        if let Some(b) = &s.branch {
            copy(ui, "Copy branch", b.clone());
        }
        ui.separator();
        ui.horizontal(|ui| {
            for (name, rgba) in tags::PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
                ui.painter()
                    .circle_filled(rect.center(), 6.0, style::c(rgba));
                if resp.on_hover_text(name).clicked() {
                    self.queue
                        .push(Command::new("session.tag", Some(&format!("{id}:{name}"))));
                    ui.close();
                }
            }
        });
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
                    Col::Px(22.0),
                ],
                |c, ui| match c {
                    0 => {
                        ui.label(
                            RichText::new(if open { "▾" } else { "▸" })
                                .size(11.0)
                                .color(style::c(theme.dim)),
                        );
                    }
                    1 => kit::dot(ui, style::state(&theme, agg), 4.0),
                    2 => {
                        ui.label(RichText::new(&sp.repo).font(crate::fonts::semibold(13.0)));
                    }
                    3 => kit::right(
                        ui,
                        &format!("{} agents", in_space.len()),
                        11.0,
                        style::c(theme.dim),
                    ),
                    _ => {
                        if ui
                            .small_button("+")
                            .on_hover_text("New agent in a new worktree")
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
                    && ui.button(format!("Open {s}")).clicked()
                {
                    self.queue.push(Command::new("repo.open", Some(s)));
                    ui.close();
                }
                if ui.button("Filter agents to this repo").clicked() {
                    self.queue
                        .push(Command::new("space.filter", Some(&sp.repo)));
                    ui.close();
                }
                if ui.button("New agent in a new worktree").clicked() {
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
                    if ui.button("New agent here").clicked() {
                        self.queue.push(Command::new("spawn.open", Some(&path)));
                    }
                    if ui.button("Open in editor").clicked() {
                        self.queue.push(Command::new("path.edit", Some(&path)));
                    }
                    if ui.button("Reveal in Finder").clicked() {
                        self.queue.push(Command::new("path.open", Some(&path)));
                    }
                    if ui.button("Copy path").clicked() {
                        ui.ctx().copy_text(path.clone());
                    }
                    if let Some(b) = &wt.branch
                        && ui.button("Copy branch").clicked()
                    {
                        ui.ctx().copy_text(b.clone());
                    }
                });
            }
        }
    }

    fn inbox(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let items = self.snap.inbox.clone();
        let open = items.iter().filter(|i| !i.done).count();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{open} open"))
                    .size(11.5)
                    .color(self.dimc()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = RichText::new("hide ticked").size(11.5).color(self.dimc());
                ui.checkbox(&mut self.hide_done, label);
            });
        });
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
                            if ui
                                .checkbox(&mut done, "")
                                .on_hover_text("tick in the wiki")
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
            kit::empty(
                ui,
                &theme,
                "No agent selected",
                "Pick one from the list, or press Cmd+J for the next that needs you.",
            );
            return;
        };
        let now = artalde_core::now();
        let sc = self.color(s.rec.state);
        let in_tab = matches!(self.tabs.get(self.active), Some(Tab::Session(_)));
        // Header.
        ui.horizontal(|ui| {
            kit::pill(ui, &theme, s.rec.state.as_str(), sc);
            ui.label(
                RichText::new(format!("for {}", ago(now.saturating_sub(s.rec.since))))
                    .size(11.5)
                    .color(self.dimc()),
            );
            if let Some(h) = &s.handle {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(h)
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.unknown)),
                    );
                });
            }
        });
        if !in_tab {
            ui.add(
                egui::Label::new(RichText::new(s.name()).font(crate::fonts::display(19.0))).wrap(),
            );
        }
        if s.label.is_some()
            && let Some(t) = &s.ai_title
        {
            ui.label(RichText::new(t).size(12.5).color(self.dimc()));
        }
        ui.add_space(kit::S1);
        // Tag + label.
        if self.label_for.as_deref() != Some(s.rec.id.as_str()) {
            self.label_edit = s.label.clone().unwrap_or_default();
            self.label_for = Some(s.rec.id.clone());
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (name, rgba) in tags::PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                let on = s.tag.as_deref() == Some(name);
                ui.painter().circle_filled(
                    rect.center(),
                    if on { 7.5 } else { 5.5 },
                    style::c(rgba),
                );
                if on {
                    ui.painter().circle_stroke(
                        rect.center(),
                        8.5,
                        Stroke::new(1.5, style::c(theme.text)),
                    );
                }
                if resp.on_hover_text(name).clicked() {
                    self.queue.push(Command::new(
                        "session.tag",
                        Some(if on { "none" } else { name }),
                    ));
                }
            }
            ui.add_space(kit::S2);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.label_edit)
                    .hint_text("label")
                    .desired_width(ui.available_width()),
            );
            if edit.lost_focus() && self.label_edit.trim() != s.label.clone().unwrap_or_default() {
                self.queue
                    .push(Command::new("session.label", Some(self.label_edit.trim())));
            }
        });
        ui.add_space(kit::S2);
        // Toolbar.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            if s.rec.tty.is_some() && kit::button(ui, &theme, "Focus", Some("f")).clicked() {
                self.queue
                    .push(Command::new("session.focus", Some(&s.rec.id)));
            }
            if !in_tab && kit::button(ui, &theme, "Open", Some("↵")).clicked() {
                self.queue
                    .push(Command::new("session.open", Some(&s.rec.id)));
            }
            if let Some(w) = &s.worktree
                && kit::button(ui, &theme, "Editor", None)
                    .on_hover_text(w.display().to_string())
                    .clicked()
            {
                self.queue
                    .push(Command::new("path.edit", Some(&w.display().to_string())));
            }
            let r = kit::button(ui, &theme, "Copy ▾", None);
            egui::Popup::menu(&r).show(|ui| self.session_menu(ui, &s));
        });
        // Blocked: the question and quick keys.
        if let Some(m) = &s.rec.message {
            ui.add_space(kit::S2);
            egui::Frame::new()
                .fill(tint(style::c(theme.surface), sc, 0.14))
                .stroke(Stroke::new(1.0, tint(style::c(theme.surface), sc, 0.6)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(m).color(style::c(theme.text)));
                    if s.rec.tty.is_some() && s.rec.state == AgentState::Blocked {
                        ui.add_space(kit::S1);
                        ui.horizontal(|ui| {
                            for (label, key) in [
                                ("1", "1"),
                                ("2", "2"),
                                ("3", "3"),
                                ("enter", "enter"),
                                ("esc", "esc"),
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
            ui.add_space(kit::S2);
            let send = ui
                .horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.reply)
                            .hint_text("Reply in its terminal…")
                            .desired_width(ui.available_width() - 62.0),
                    );
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    kit::button(ui, &theme, "Send", None).clicked() || enter
                })
                .inner;
            if send && !self.reply.trim().is_empty() {
                let text = std::mem::take(&mut self.reply);
                self.queue
                    .push(Command::new("session.send", Some(text.trim())));
            }
        }
        // Related.
        if !s.resources.is_empty() {
            ui.add_space(kit::S3);
            kit::section(ui, &theme, "Related", Some(s.resources.len()), |_| {});
            for r in &s.resources {
                let color = self.resource_color(r);
                let resp = kit::row(
                    ui,
                    &theme,
                    ("res", &r.id),
                    24.0,
                    false,
                    &[Col::Px(104.0), Col::Px(86.0), Col::Flex],
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
        }
        // Details.
        ui.add_space(kit::S3);
        kit::section(ui, &theme, "Details", None, |_| {});
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
        copied |= kit::kv(ui, &theme, "cwd", &s.rec.cwd.display().to_string(), LABEL_W);
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
        if copied {
            let ctx = ui.ctx().clone();
            self.toast_line(&ctx, "copied".into());
        }
        if !in_tab {
            if let Some(p) = &s.rec.last_prompt {
                ui.add_space(kit::S3);
                kit::section(ui, &theme, "Last prompt", None, |_| {});
                let p = p.clone();
                self.md(ui, &p, 13.0);
            }
            if let Some(said) = &s.last_said {
                ui.add_space(kit::S3);
                kit::section(ui, &theme, "Last said", None, |_| {});
                let said = said.clone();
                self.md(ui, &said, 13.0);
            }
        }
        if s.discovered && s.rec.state == AgentState::Unknown {
            ui.add_space(kit::S2);
            ui.label(
                RichText::new("Found as a process; it reports no state.")
                    .size(11.5)
                    .color(self.dimc()),
            );
        }
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
        ui.horizontal(|ui| {
            for s in [
                AgentState::Blocked,
                AgentState::Done,
                AgentState::Working,
                AgentState::Idle,
            ] {
                let n = self.snap.count(s);
                kit::dot(
                    ui,
                    if n == 0 {
                        style::c(theme.unknown)
                    } else {
                        self.color(s)
                    },
                    3.5,
                );
                ui.label(
                    RichText::new(format!("{n} {}", s.as_str()))
                        .size(11.5)
                        .color(if n == 0 {
                            self.dimc()
                        } else {
                            style::c(theme.text)
                        }),
                );
                ui.add_space(kit::S1);
            }
            if !errors.is_empty() {
                let r = ui.label(
                    RichText::new(format!("! {} errors", errors.len()))
                        .size(11.5)
                        .color(style::c(theme.danger)),
                );
                r.on_hover_text(errors.join("\n"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(&theme.name)
                    .on_hover_text("Switch theme")
                    .clicked()
                {
                    self.queue.push(Command::new("theme.cycle", None));
                }
                if ui
                    .small_button(&layout)
                    .on_hover_text("Switch layout")
                    .clicked()
                {
                    self.queue.push(Command::new("layout.cycle", None));
                }
                ui.add_space(kit::S2);
                kit::hint(ui, &theme, "⌘I", "PRs");
                kit::hint(ui, &theme, "⌘N", "new agent");
                kit::hint(ui, &theme, "⌘K", "commands");
            });
        });
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
        for line in self.log.iter().rev() {
            ui.label(RichText::new(line).monospace().size(11.5));
        }
    }
}
