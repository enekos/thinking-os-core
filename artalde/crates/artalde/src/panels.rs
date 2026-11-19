//! Built-in panels: agents, spaces, inbox, prs, detail, status, log.

use artalde_core::command::Command;
use artalde_core::model::{AgentState, Resource, ResourceKind, Session};
use artalde_core::tags;
use artalde_core::text::{ago, clip};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke, UiBuilder};

use crate::app::App;
use crate::style::{self, tint};

/// Resource chips shown on an agent card.
const CARD_RESOURCES: usize = 4;

impl App {
    pub(crate) fn builtin(&mut self, ui: &mut egui::Ui, name: &str) {
        match name {
            "agents" => self.agents(ui),
            "spaces" => self.spaces(ui),
            "inbox" => self.inbox(ui),
            "prs" => self.prs(ui),
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

    fn dim(&self, text: impl Into<String>) -> RichText {
        RichText::new(text).color(style::c(self.theme.dim))
    }

    fn agents(&mut self, ui: &mut egui::Ui) {
        let now = artalde_core::now();
        let t = ui.input(|i| i.time);
        let rows: Vec<Session> = self.visible_sessions().into_iter().cloned().collect();
        if let Some(f) = self.filter.clone() {
            ui.horizontal(|ui| {
                ui.label(self.dim(format!("only {f}")));
                if ui.small_button("show all").clicked() {
                    self.queue.push(Command::new("space.filter", None));
                }
            });
        }
        if rows.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No agents running").strong());
                ui.label(self.dim("Cmd+N starts one, or run claude in any terminal."));
            });
            return;
        }
        if rows.iter().all(|s| s.rec.state == AgentState::Unknown) {
            egui::Frame::new()
                .fill(tint(style::c(self.theme.surface), style::c(self.theme.blocked), 0.12))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label("No agent reports a state. Claude Code sessions do on their own; for details (tool in flight, permission prompts) run `artalde hooks`.");
                });
            ui.add_space(4.0);
        }
        for (i, s) in rows.iter().enumerate() {
            let id = s.rec.id.clone();
            let on = self.selected.as_deref() == Some(id.as_str());
            let state = s.rec.state;
            let sc = self.color(state);
            let raised = style::c(self.theme.raised);
            let tag_color = s.tag.as_deref().map(|t| style::token(&self.theme, t));
            let fill = if let (false, Some(tc)) = (on, tag_color) {
                tint(raised, tc, 0.10)
            } else if on {
                tint(raised, style::c(self.theme.accent), 0.14)
            } else {
                raised
            };
            let stroke = if on {
                Stroke::new(1.5, style::c(self.theme.accent))
            } else {
                Stroke::new(1.0, style::c(self.theme.border))
            };
            let resp = ui
                .scope_builder(
                    UiBuilder::new()
                        .id_salt(("agent", &id))
                        .sense(Sense::click()),
                    |ui| {
                        egui::Frame::new()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin {
                                left: 16,
                                right: 10,
                                top: 8,
                                bottom: 8,
                            })
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    if i < 9 {
                                        ui.label(
                                            self.dim(format!("{}", i + 1)).small().monospace(),
                                        );
                                    }
                                    ui.label(RichText::new(clip(&s.name(), 90)).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                self.dim(ago(now.saturating_sub(s.rec.since)))
                                                    .small(),
                                            );
                                            chip(ui, state.as_str(), sc, &self.theme);
                                        },
                                    );
                                });
                                let mut sub = Vec::new();
                                if let Some(h) = &s.handle {
                                    sub.push(h.clone());
                                }
                                if s.label.is_some()
                                    && let Some(t) = &s.ai_title
                                {
                                    sub.push(clip(t, 50));
                                }
                                sub.push(s.repo.clone().unwrap_or_else(|| "—".into()));
                                if let Some(b) = &s.branch {
                                    sub.push(b.clone());
                                }
                                sub.push(s.rec.agent.clone());
                                if s.pending_tasks > 0 {
                                    sub.push(format!("+{} tasks", s.pending_tasks));
                                }
                                ui.label(self.dim(sub.join(" · ")).small());
                                if let Some(m) = &s.rec.message {
                                    ui.label(
                                        RichText::new(clip(m, 140))
                                            .color(self.color(AgentState::Blocked)),
                                    );
                                } else if let Some(a) = &s.rec.activity {
                                    ui.label(
                                        RichText::new(format!("› {}", clip(a, 120)))
                                            .small()
                                            .monospace(),
                                    );
                                } else if let (AgentState::Done, Some(said)) = (state, &s.last_said)
                                {
                                    ui.label(RichText::new(clip(said, 160)).small().italics());
                                }
                                if !s.resources.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        for r in s.resources.iter().take(CARD_RESOURCES) {
                                            self.resource_chip(ui, r);
                                        }
                                        if s.resources.len() > CARD_RESOURCES {
                                            ui.label(
                                                self.dim(format!(
                                                    "+{}",
                                                    s.resources.len() - CARD_RESOURCES
                                                ))
                                                .small(),
                                            );
                                        }
                                    });
                                }
                            });
                    },
                )
                .response;
            let bar = egui::Rect::from_min_size(
                resp.rect.min + egui::vec2(5.0, 8.0),
                egui::vec2(3.0, resp.rect.height() - 16.0),
            );
            #[expect(clippy::cast_possible_truncation, reason = "an animation phase")]
            let pulse = if state == AgentState::Blocked {
                (0.55 + 0.45 * (t * 3.0).sin()) as f32
            } else {
                1.0
            };
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), sc.gamma_multiply(pulse));
            if let Some(tc) = tag_color {
                let edge =
                    egui::Rect::from_min_size(resp.rect.min, egui::vec2(4.0, resp.rect.height()));
                ui.painter().rect_filled(
                    edge,
                    CornerRadius {
                        nw: 8,
                        sw: 8,
                        ne: 0,
                        se: 0,
                    },
                    tc,
                );
            }
            if resp.clicked() {
                self.queue.push(Command::new("agent.select", Some(&id)));
            }
            if resp.double_clicked() {
                self.queue.push(Command::new("session.focus", Some(&id)));
            }
            ui.add_space(2.0);
        }
    }

    fn spaces(&mut self, ui: &mut egui::Ui) {
        let spaces = self.snap.spaces.clone();
        if spaces.is_empty() {
            ui.label(self.dim("Repos appear here once an agent touches them."));
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
            ui.horizontal(|ui| {
                dot(ui, self.color(agg));
                let r = ui.selectable_label(filtered, RichText::new(&sp.repo).strong());
                if r.clicked() {
                    let arg = if filtered { "" } else { sp.repo.as_str() };
                    self.queue.push(Command::new("space.filter", Some(arg)));
                }
                ui.label(self.dim(format!("{} agents", in_space.len())).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("+")
                        .on_hover_text("New agent in a new worktree")
                        .clicked()
                    {
                        self.queue.push(Command::new("spawn.open", Some(&sp.repo)));
                    }
                });
            });
            for wt in &sp.worktrees {
                let agents: Vec<&Session> = in_space
                    .iter()
                    .filter(|s| s.worktree.as_ref() == Some(&wt.path))
                    .collect();
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    let label = wt.branch.clone().unwrap_or_else(|| {
                        wt.path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                    let text = RichText::new(clip(&label, 34)).monospace().small();
                    let r = ui.add(
                        egui::Label::new(if agents.is_empty() {
                            text.color(style::c(self.theme.dim))
                        } else {
                            text
                        })
                        .sense(Sense::click()),
                    );
                    let r = r.on_hover_text(wt.path.display().to_string());
                    if r.clicked() {
                        self.queue.push(Command::new(
                            "spawn.open",
                            Some(&wt.path.display().to_string()),
                        ));
                    }
                    for a in &agents {
                        dot(ui, self.color(a.rec.state));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if wt.behind > 0 {
                            ui.label(self.dim(format!("-{}", wt.behind)).small());
                        }
                        if wt.ahead > 0 {
                            ui.label(self.dim(format!("+{}", wt.ahead)).small());
                        }
                        if wt.dirty > 0 {
                            ui.label(
                                RichText::new(format!("~{}", wt.dirty))
                                    .small()
                                    .color(self.color(AgentState::Blocked)),
                            );
                        }
                    });
                });
            }
            ui.add_space(8.0);
        }
    }

    fn inbox(&mut self, ui: &mut egui::Ui) {
        let items = self.snap.inbox.clone();
        let open = items.iter().filter(|i| !i.done).count();
        ui.horizontal(|ui| {
            ui.label(self.dim(format!("{open} open")).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = self.dim("hide ticked").small();
                ui.checkbox(&mut self.hide_done, label);
            });
        });
        if items.is_empty() {
            ui.label(self.dim("No zaindu proposals in the last days."));
        }
        for kind in ["review", "red", "approved", "merged", ""] {
            let group: Vec<(usize, &artalde_core::model::InboxItem)> = items
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    let known = ["review", "red", "approved", "merged"].contains(&i.kind.as_str());
                    (if kind.is_empty() {
                        !known
                    } else {
                        i.kind == kind
                    }) && !(self.hide_done && i.done)
                })
                .collect();
            if group.is_empty() {
                continue;
            }
            let color = match kind {
                "review" => self.theme.blocked,
                "red" => self.theme.danger,
                "approved" => self.theme.done,
                "merged" => self.theme.dim,
                _ => self.theme.accent,
            };
            ui.add_space(4.0);
            ui.label(
                RichText::new(if kind.is_empty() {
                    "other".to_uppercase()
                } else {
                    kind.to_uppercase()
                })
                .small()
                .color(style::c(color)),
            );
            for (idx, item) in group {
                ui.horizontal(|ui| {
                    let mut done = item.done;
                    if ui
                        .checkbox(&mut done, "")
                        .on_hover_text("tick in the wiki")
                        .changed()
                    {
                        self.queue
                            .push(Command::new("inbox.toggle", Some(&idx.to_string())));
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let target = RichText::new(&item.target).monospace().small();
                            match &item.url {
                                Some(url) => {
                                    if ui.link(target).on_hover_text(url).clicked() {
                                        self.queue.push(Command::new("url.open", Some(url)));
                                    }
                                }
                                None => {
                                    ui.label(target);
                                }
                            }
                            if !kind.is_empty() {
                                ui.label(self.dim(&item.time).small());
                            } else {
                                ui.label(self.dim(&item.kind).small());
                            }
                        });
                        if !item.detail.is_empty() {
                            ui.label(self.dim(clip(&item.detail, 80)).small());
                        }
                    });
                });
            }
        }
    }

    fn prs(&mut self, ui: &mut egui::Ui) {
        let prs = self.snap.prs.clone();
        if prs.is_empty() {
            ui.label(self.dim(if self.settings().gh {
                "No open PRs (gh refreshes every 2 minutes)."
            } else {
                "gh is off in settings.json."
            }));
        }
        for pr in &prs {
            ui.horizontal(|ui| {
                if pr.draft {
                    chip(ui, "draft", style::c(self.theme.dim), &self.theme);
                }
                let r = ui.link(
                    RichText::new(format!(
                        "{}#{}",
                        pr.repo.rsplit('/').next().unwrap_or(&pr.repo),
                        pr.number
                    ))
                    .monospace()
                    .small(),
                );
                if r.on_hover_text(&pr.url).clicked() {
                    self.queue.push(Command::new("url.open", Some(&pr.url)));
                }
            });
            ui.label(clip(&pr.title, 90));
            ui.add_space(4.0);
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.selected_session().cloned() else {
            ui.add_space(12.0);
            ui.label(self.dim("Select an agent. Cmd+J jumps to the next one that needs you."));
            return;
        };
        let now = artalde_core::now();
        let sc = self.color(s.rec.state);
        ui.horizontal(|ui| {
            chip(ui, s.rec.state.as_str(), sc, &self.theme);
            ui.label(
                self.dim(format!("for {}", ago(now.saturating_sub(s.rec.since))))
                    .small(),
            );
        });
        ui.add(egui::Label::new(RichText::new(s.name()).heading()).wrap());
        if s.label.is_some()
            && let Some(t) = &s.ai_title
        {
            ui.label(self.dim(t));
        }
        if self.label_for.as_deref() != Some(s.rec.id.as_str()) {
            self.label_edit = s.label.clone().unwrap_or_default();
            self.label_for = Some(s.rec.id.clone());
        }
        ui.horizontal(|ui| {
            for (name, rgba) in tags::PALETTE {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                let on = s.tag.as_deref() == Some(name);
                ui.painter().circle_filled(
                    rect.center(),
                    if on { 8.0 } else { 6.0 },
                    style::c(rgba),
                );
                if on {
                    ui.painter().circle_stroke(
                        rect.center(),
                        8.5,
                        Stroke::new(1.5, style::c(self.theme.text)),
                    );
                }
                if resp.on_hover_text(name).clicked() {
                    let arg = if on { "none" } else { name };
                    self.queue.push(Command::new("session.tag", Some(arg)));
                }
            }
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.label_edit)
                    .hint_text("label")
                    .desired_width(ui.available_width().min(200.0)),
            );
            if edit.lost_focus() && self.label_edit.trim() != s.label.clone().unwrap_or_default() {
                self.queue
                    .push(Command::new("session.label", Some(self.label_edit.trim())));
            }
        });
        if let Some(m) = &s.rec.message {
            egui::Frame::new()
                .fill(tint(style::c(self.theme.surface), sc, 0.16))
                .stroke(Stroke::new(1.0, sc))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(m).color(style::c(self.theme.text)));
                });
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(s.rec.tty.is_some(), egui::Button::new("Focus terminal"))
                .clicked()
            {
                self.queue
                    .push(Command::new("session.focus", Some(&s.rec.id)));
            }
            if let Some(w) = &s.worktree
                && ui.button("Open folder").clicked()
            {
                self.queue
                    .push(Command::new("path.open", Some(&w.display().to_string())));
            }
            if ui.button("Copy id").clicked() {
                ui.ctx().copy_text(s.rec.id.clone());
            }
        });
        if !s.resources.is_empty() {
            ui.add_space(6.0);
            ui.label(self.dim("Related").small());
            for r in &s.resources {
                ui.horizontal(|ui| {
                    self.resource_chip(ui, r);
                    if let Some(state) = &r.state {
                        ui.label(RichText::new(state).small().color(self.resource_color(r)));
                    }
                    if let Some(t) = &r.title {
                        ui.label(self.dim(clip(t, 60)).small());
                    }
                });
            }
        }
        if s.rec.tty.is_some() {
            ui.add_space(6.0);
            ui.label(self.dim("Reply in its terminal").small());
            let send = ui
                .horizontal(|ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.reply)
                            .hint_text("type a reply, Enter sends")
                            .desired_width(ui.available_width() - 60.0),
                    );
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.button("Send").clicked() || enter
                })
                .inner;
            if send && !self.reply.trim().is_empty() {
                let text = std::mem::take(&mut self.reply);
                self.queue
                    .push(Command::new("session.send", Some(text.trim())));
            }
            if s.rec.state == AgentState::Blocked {
                ui.horizontal(|ui| {
                    ui.label(self.dim("keys").small());
                    for (label, key) in [
                        ("1", "1"),
                        ("2", "2"),
                        ("3", "3"),
                        ("enter", "enter"),
                        ("esc", "esc"),
                    ] {
                        if ui.small_button(label).clicked() {
                            self.queue.push(Command::new("session.key", Some(key)));
                        }
                    }
                });
            }
        }
        ui.add_space(8.0);
        egui::Grid::new("detail-meta")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(self.dim(k).small());
                    ui.label(RichText::new(v).small().monospace());
                    ui.end_row();
                };
                row("agent", s.rec.agent.clone());
                if let Some(h) = &s.handle {
                    row("session", h.clone());
                }
                if let Some(r) = &s.repo {
                    row("repo", r.clone());
                }
                if let Some(b) = &s.branch {
                    row("branch", b.clone());
                }
                if let Some(w) = &s.worktree {
                    row("worktree", w.display().to_string());
                }
                row("cwd", s.rec.cwd.display().to_string());
                if let Some(t) = &s.rec.tty {
                    row("tty", t.clone());
                }
                if let Some(p) = s.rec.pid {
                    row("pid", p.to_string());
                }
                if let Some(a) = &s.rec.activity {
                    row("doing", a.clone());
                }
                if let Some(task) = &s.task {
                    row("task", format!("{task} (+{} pending)", s.pending_tasks));
                }
            });
        if let Some(p) = &s.rec.last_prompt {
            ui.add_space(8.0);
            ui.label(self.dim("Last prompt").small());
            ui.label(RichText::new(p).italics());
        }
        if let Some(said) = &s.last_said {
            ui.add_space(8.0);
            ui.label(self.dim("Last said").small());
            egui::Frame::new()
                .fill(style::c(self.theme.raised))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(egui::Label::new(said).wrap());
                });
        }
        if s.discovered {
            ui.add_space(8.0);
            ui.label(
                self.dim("Found as a process; it has not reported through the hook.")
                    .small(),
            );
        }
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let counts: Vec<(AgentState, usize)> = [
            AgentState::Blocked,
            AgentState::Done,
            AgentState::Working,
            AgentState::Idle,
        ]
        .into_iter()
        .map(|s| (s, self.snap.count(s)))
        .collect();
        let errors: Vec<String> = self
            .config
            .as_ref()
            .map(|c| c.errors.clone())
            .unwrap_or_default()
            .into_iter()
            .chain(self.snap.errors.clone())
            .collect();
        let layout = self.current_layout_name();
        let theme = self.theme.name.clone();
        ui.horizontal(|ui| {
            for (s, n) in counts {
                dot(
                    ui,
                    if n == 0 {
                        style::c(self.theme.unknown)
                    } else {
                        self.color(s)
                    },
                );
                ui.label(RichText::new(format!("{n} {}", s.as_str())).small());
                ui.add_space(6.0);
            }
            if !errors.is_empty() {
                ui.label(
                    RichText::new(format!("! {} errors", errors.len()))
                        .small()
                        .color(style::c(self.theme.danger)),
                )
                .on_hover_text(errors.join("\n"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(self.dim("Cmd+K commands").small());
                if ui
                    .small_button(&theme)
                    .on_hover_text("theme.cycle")
                    .clicked()
                {
                    self.queue.push(Command::new("theme.cycle", None));
                }
                if ui
                    .small_button(&layout)
                    .on_hover_text("layout.cycle")
                    .clicked()
                {
                    self.queue.push(Command::new("layout.cycle", None));
                }
            });
        });
    }

    fn resource_color(&self, r: &Resource) -> egui::Color32 {
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

    /// `● data#7939`: coloured by state, titled on hover, opens on click.
    fn resource_chip(&mut self, ui: &mut egui::Ui, r: &Resource) {
        let color = self.resource_color(r);
        let text = RichText::new(format!(
            "{} {}",
            if r.kind == ResourceKind::Pr {
                "PR"
            } else {
                "#"
            },
            r.short()
        ))
        .small()
        .monospace()
        .color(color);
        let tip = format!(
            "{}{}{}",
            r.title.clone().unwrap_or_else(|| r.id.clone()),
            r.state
                .as_ref()
                .map(|s| format!(" — {s}"))
                .unwrap_or_default(),
            if r.mentions > 0 {
                format!(" · {} mentions", r.mentions)
            } else {
                String::new()
            },
        );
        let resp = egui::Frame::new()
            .fill(tint(style::c(self.theme.surface), color, 0.14))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.add(egui::Label::new(text).sense(Sense::click()))
            })
            .inner;
        if resp.on_hover_text(tip).clicked() {
            self.queue.push(Command::new("url.open", Some(&r.url)));
        }
    }

    fn log_panel(&mut self, ui: &mut egui::Ui) {
        for line in self.log.iter().rev() {
            ui.label(RichText::new(line).monospace().small());
        }
    }
}

fn dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

fn chip(ui: &mut egui::Ui, text: &str, color: egui::Color32, theme: &artalde_core::theme::Theme) {
    egui::Frame::new()
        .fill(tint(style::c(theme.surface), color, 0.2))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(color)));
}
