//! The agent lifecycle, opinionated: start from a prompt, watch it work,
//! answer it, stop a turn, close it when it is done, sweep the stale ones,
//! reopen what was closed. Rules live in `artalde_core::lifecycle`; this is
//! the window's side: commands, the spawn sheet and the close confirmation.

use std::collections::HashSet;

use artalde_core::command::Command;
use artalde_core::lifecycle::{self, Closed, Launch, Stage};
use artalde_core::model::{AgentState, Session};
use artalde_core::text::{clip, shell_quote};
use eframe::egui::{self, Key, Modifiers, RichText};

use crate::actions::Job;
use crate::app::App;
use crate::kit::{self, Icon};
use crate::style;

/// A closed session stays hidden this long while its process exits.
const CLOSING_SECS: u64 = 30;
/// The spawn sheet's width.
const SHEET_W: f32 = 600.0;

/// Where a new agent starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// An existing folder: index into [`App::worktree_choices`].
    Existing(usize),
    /// A new worktree of a repo, on a new branch.
    NewWorktree,
}

pub struct SpawnForm {
    pub place: Place,
    pub repo: String,
    pub branch: String,
    /// The branch follows the prompt until the user edits it.
    pub branch_auto: bool,
    pub agent: String,
    pub prompt: String,
    pub focused: bool,
}

/// Sessions waiting for the user to confirm closing them.
pub struct CloseConfirm {
    pub ids: Vec<String>,
    pub title: String,
}

impl App {
    pub(crate) fn stale_after_secs(&self) -> u64 {
        self.settings().stale_after_mins.saturating_mul(60)
    }

    pub(crate) fn stage(&self, s: &Session) -> Stage {
        Stage::of(
            s.rec.state,
            s.rec.since,
            self.snap.now.max(artalde_core::now()),
            self.stale_after_secs(),
        )
    }

    /// `session.stop`: interrupt the turn in flight (Esc in its terminal).
    pub(crate) fn stop_session(&mut self, id: &str) {
        let Some(s) = self.snap.session(id) else {
            return;
        };
        match s.rec.tty.clone() {
            Some(tty) => {
                let name = s.name();
                self.submit_job(Job::Type {
                    tty,
                    text: "<esc>".into(),
                    enter: false,
                });
                self.push_log(format!("stopped the turn of {name}"));
            }
            None => self.push_log("stop: that session has no terminal"),
        }
    }

    /// `session.wrap`: ask a Claude session to close out its work.
    pub(crate) fn wrap_session(&mut self, id: &str) {
        let Some(tty) = self.snap.session(id).and_then(|s| s.rec.tty.clone()) else {
            return self.push_log("wrap: that session has no terminal");
        };
        self.submit_job(Job::Type {
            tty,
            text: "/wrap".into(),
            enter: true,
        });
    }

    /// `session.close`: close now when nothing is in flight, else ask.
    pub(crate) fn request_close(&mut self, ids: Vec<String>, title: String) {
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| self.snap.session(id).is_some())
            .collect();
        if ids.is_empty() {
            return;
        }
        let busy = ids.iter().any(|id| {
            self.snap
                .session(id)
                .is_some_and(|s| lifecycle::close_needs_confirm(s.rec.state))
        });
        if busy || ids.len() > 1 {
            self.close_confirm = Some(CloseConfirm { ids, title });
        } else {
            self.close_now(&ids);
        }
    }

    /// End `ids`: log them for reopening, hide them, and queue the kill.
    pub(crate) fn close_now(&mut self, ids: &[String]) {
        let now = artalde_core::now();
        let close_tab = self.settings().close_terminal;
        let mut names = Vec::new();
        for id in ids {
            let Some(s) = self.snap.session(id).cloned() else {
                continue;
            };
            self.closed.push(Closed::of(&s, now));
            self.closing.insert(id.clone(), now);
            names.push(s.name());
            if self.shot.is_some() {
                continue;
            }
            self.submit_job(Job::Close {
                pid: s.rec.pid,
                agent: s.rec.agent.clone(),
                tty: s.rec.tty.clone(),
                close_tab,
                name: s.name(),
            });
        }
        if let Err(e) = self.closed.save() {
            self.push_log(format!("closed log: {e}"));
        }
        self.snap
            .sessions
            .retain(|s| !self.closing.contains_key(&s.rec.id));
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.closing.contains_key(id))
        {
            self.selected = None;
        }
        let what = match names.as_slice() {
            [one] => clip(one, 50),
            many => format!("{} agents", many.len()),
        };
        let ctx = self.egui_ctx.clone();
        self.toast_line(&ctx, format!("Closed {what} · ⌘⇧T reopens"));
        self.mod_ctx_dirty();
    }

    /// `session.sweep`: close every stale session, after one confirmation.
    pub(crate) fn sweep(&mut self) {
        let ids: Vec<String> = self
            .snap
            .sessions
            .iter()
            .filter(|s| self.stage(s) == Stage::Stale)
            .map(|s| s.rec.id.clone())
            .collect();
        if ids.is_empty() {
            let ctx = self.egui_ctx.clone();
            self.toast_line(&ctx, "Nothing stale".into());
            return;
        }
        let mins = self.settings().stale_after_mins;
        self.close_confirm = Some(CloseConfirm {
            title: format!("Close {} agents idle for over {mins} min?", ids.len()),
            ids,
        });
    }

    /// `session.reopen`: resume a closed session in a new terminal tab.
    pub(crate) fn reopen(&mut self, id: Option<&str>) {
        let entry = match id {
            Some(id) => self.closed.take(id),
            None => {
                let first = self
                    .closed
                    .entries
                    .iter()
                    .find(|c| c.reopen_line().is_some())
                    .map(|c| c.id.clone());
                first.and_then(|id| self.closed.take(&id))
            }
        };
        let Some(c) = entry else {
            let ctx = self.egui_ctx.clone();
            self.toast_line(&ctx, "Nothing to reopen".into());
            return;
        };
        let Some(line) = c.reopen_line() else {
            self.push_log(format!("reopen: {} cannot be resumed", c.name));
            self.closed.push(c);
            return;
        };
        self.submit_job(Job::Spawn { line });
        self.launches.push(Launch {
            repo: c.repo.clone().unwrap_or_default(),
            dir: Some(c.cwd.clone()),
            branch: None,
            agent: c.agent.clone(),
            prompt: c.name.clone(),
            at: artalde_core::now(),
        });
        if let Err(e) = self.closed.save() {
            self.push_log(format!("closed log: {e}"));
        }
    }

    /// Match new sessions to launches, drop expired launches, and keep
    /// closing sessions hidden until their process is gone.
    pub(crate) fn lifecycle_receive(&mut self, fresh: &HashSet<String>) {
        let now = artalde_core::now();
        self.closing.retain(|id, at| {
            now.saturating_sub(*at) < CLOSING_SECS && self.snap.session(id).is_some()
        });
        self.snap
            .sessions
            .retain(|s| !self.closing.contains_key(&s.rec.id));
        let mut claimed = Vec::new();
        for s in self
            .snap
            .sessions
            .iter()
            .filter(|s| fresh.contains(&s.rec.id))
        {
            if let Some(i) = self.launches.iter().position(|l| l.claims(s)) {
                let l = self.launches.remove(i);
                claimed.push((
                    s.rec.id.clone(),
                    format!("{} started: {}", l.agent, s.name()),
                ));
            }
        }
        for (id, line) in claimed {
            self.push_log(line);
            self.selected = Some(id);
        }
        let before = self.launches.len();
        self.launches.retain(|l| !l.expired(now));
        if self.launches.len() < before {
            self.push_log("a launched agent never showed up — check its terminal tab");
        }
    }

    pub(crate) fn open_spawn(&mut self, arg: Option<&str>) {
        let worktrees = self.worktree_choices();
        let by_path = arg.and_then(|a| {
            worktrees
                .iter()
                .position(|(_, path, _)| path.display().to_string() == a)
        });
        let by_repo = arg.and_then(|a| self.snap.spaces.iter().find(|s| s.repo == a));
        let selected_dir = self
            .selected_session()
            .and_then(|s| s.worktree.clone())
            .and_then(|w| worktrees.iter().position(|(_, p, _)| *p == w));
        let place = match (by_path, by_repo) {
            (Some(i), _) => Place::Existing(i),
            (None, Some(_)) => Place::NewWorktree,
            (None, None) => selected_dir
                .or(if worktrees.is_empty() { None } else { Some(0) })
                .map_or(Place::NewWorktree, Place::Existing),
        };
        let repo = by_repo
            .map(|s| s.repo.clone())
            .or_else(|| self.filter.clone())
            .or_else(|| self.selected_session().and_then(|s| s.repo.clone()))
            .or_else(|| self.snap.spaces.first().map(|s| s.repo.clone()))
            .unwrap_or_default();
        let agent = self
            .settings()
            .agents
            .first()
            .cloned()
            .unwrap_or_else(|| "claude".into());
        self.spawn = Some(SpawnForm {
            place,
            repo,
            branch: String::new(),
            branch_auto: true,
            agent,
            prompt: String::new(),
            focused: false,
        });
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
        let line = match form.place {
            Place::Existing(i) => {
                let choices = self.worktree_choices();
                let (_, dir, repo) = choices.get(i).ok_or("pick a folder")?;
                s.spawn_here
                    .replace("{dir}", &shell_quote(&dir.display().to_string()))
                    .replace("{repo}", &shell_quote(repo))
            }
            Place::NewWorktree => {
                let branch = form.branch.trim();
                if branch.is_empty() {
                    return Err("name the branch, or describe the task above".into());
                }
                if !branch
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

    fn launch_of(&self, form: &SpawnForm) -> Launch {
        let choices = self.worktree_choices();
        let (repo, dir, branch) = match form.place {
            Place::Existing(i) => {
                let (_, dir, repo) = choices.get(i).cloned().unwrap_or_default();
                (repo, Some(dir), None)
            }
            Place::NewWorktree => (
                form.repo.trim().to_owned(),
                None,
                Some(form.branch.trim().to_owned()),
            ),
        };
        Launch {
            repo,
            dir,
            branch,
            agent: form.agent.trim().to_owned(),
            prompt: form.prompt.trim().to_owned(),
            at: artalde_core::now(),
        }
    }

    pub(crate) fn spawn_ui(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.spawn.take() else {
            return;
        };
        if form.place == Place::NewWorktree && form.branch_auto {
            form.branch = lifecycle::branch_for(&form.prompt, &self.linear_teams());
        }
        let choices = self.worktree_choices();
        let repos: Vec<String> = self.snap.spaces.iter().map(|s| s.repo.clone()).collect();
        let agents = self.settings().agents;
        let preview = self.spawn_line(&form);
        let (mut close, submit_key) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
            )
        });
        let mut start = submit_key;
        let theme = self.theme.clone();
        let modal = egui::Modal::new(egui::Id::unique("spawn-sheet"))
            .frame(sheet_frame(&theme))
            .show(ctx, |ui| {
                ui.set_width(SHEET_W);
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("New agent").font(crate::fonts::display(20.0)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::icon_button(ui, &theme, Icon::Close, "Cancel (esc)").clicked() {
                            close = true;
                        }
                    });
                });
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut form.prompt)
                        .hint_text("What should it do? A ticket id like THU-4821 names the branch.")
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::same(10)),
                );
                if !form.focused {
                    edit.request_focus();
                    form.focused = true;
                }
                field_row(ui, &theme, "Where", |ui| {
                    let existing = matches!(form.place, Place::Existing(_));
                    if kit::pill_tab(
                        ui,
                        &theme,
                        "Existing folder",
                        None,
                        existing,
                        style::c(theme.accent),
                    )
                    .clicked()
                        && !existing
                    {
                        form.place = Place::Existing(0);
                    }
                    if kit::pill_tab(
                        ui,
                        &theme,
                        "New worktree",
                        None,
                        !existing,
                        style::c(theme.accent),
                    )
                    .clicked()
                    {
                        form.place = Place::NewWorktree;
                    }
                });
                match form.place {
                    Place::Existing(i) => field_row(ui, &theme, "Folder", |ui| {
                        let label = choices.get(i).map_or("pick a folder", |c| c.0.as_str());
                        let w = ui.available_width();
                        kit::dropdown(ui, &theme, "spawn-folder", label, w, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(320.0)
                                .show(ui, |ui| {
                                    for (j, (l, _, _)) in choices.iter().enumerate() {
                                        if kit::menu_item(ui, &theme, l, j == i) {
                                            form.place = Place::Existing(j);
                                        }
                                    }
                                });
                        });
                    }),
                    Place::NewWorktree => {
                        field_row(ui, &theme, "Repo", |ui| {
                            let w = ui.available_width();
                            let current = form.repo.clone();
                            kit::dropdown(ui, &theme, "spawn-repo", &current, w, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .show(ui, |ui| {
                                        for r in &repos {
                                            if kit::menu_item(ui, &theme, r, *r == current) {
                                                form.repo = r.clone();
                                            }
                                        }
                                    });
                            });
                        });
                        field_row(ui, &theme, "Branch", |ui| {
                            kit::icon(ui, Icon::Branch, 13.0, style::c(theme.faint));
                            let w = ui.available_width();
                            let r = kit::input(
                                ui,
                                &theme,
                                &mut form.branch,
                                "from the prompt",
                                w,
                                true,
                            );
                            if r.changed() {
                                form.branch_auto = form.branch.trim().is_empty();
                            }
                        });
                    }
                }
                field_row(ui, &theme, "Agent", |ui| {
                    for a in &agents {
                        if kit::pill_tab(
                            ui,
                            &theme,
                            a,
                            None,
                            form.agent == *a,
                            style::c(theme.accent),
                        )
                        .clicked()
                        {
                            form.agent.clone_from(a);
                        }
                    }
                });
                egui::Frame::new()
                    .fill(style::tint(
                        style::c(theme.surface),
                        style::c(theme.text),
                        0.05,
                    ))
                    .corner_radius(egui::CornerRadius::same(kit::PILL_RADIUS))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match &preview {
                            Ok(line) => ui.add(
                                egui::Label::new(
                                    RichText::new(format!("$ {line}"))
                                        .monospace()
                                        .size(11.5)
                                        .color(style::c(theme.dim)),
                                )
                                .wrap(),
                            ),
                            Err(e) => {
                                ui.label(RichText::new(e).size(12.0).color(style::c(theme.blocked)))
                            }
                        };
                    });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "Opens in a new terminal tab; it shows up here as it starts.",
                        )
                        .size(11.0)
                        .color(style::c(theme.faint)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let go = ui.add_enabled_ui(preview.is_ok(), |ui| {
                            kit::primary_with(
                                ui,
                                &theme,
                                Some(Icon::Plus),
                                "Start",
                                Some("⌘↵"),
                                style::c(theme.accent),
                            )
                        });
                        if go.inner.clicked() {
                            start = true;
                        }
                        if kit::button(ui, &theme, "Cancel", Some("esc")).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close || modal.should_close() {
            return;
        }
        if start && let Ok(line) = self.spawn_line(&form) {
            let launch = self.launch_of(&form);
            if self.shot.is_none() {
                self.submit_job(Job::Spawn { line });
            }
            self.launches.push(launch);
            return;
        }
        self.spawn = Some(form);
    }

    pub(crate) fn close_confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = self.close_confirm.take() else {
            return;
        };
        let theme = self.theme.clone();
        let sessions: Vec<Session> = c
            .ids
            .iter()
            .filter_map(|id| self.snap.session(id).cloned())
            .collect();
        if sessions.is_empty() {
            return;
        }
        let (esc, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        let mut go = enter;
        let mut cancel = esc;
        let close_tab = self.settings().close_terminal;
        let modal = egui::Modal::new(egui::Id::unique("close-confirm"))
            .frame(sheet_frame(&theme))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new(&c.title).font(crate::fonts::display(17.0)));
                ui.add_space(kit::S2);
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for s in &sessions {
                            ui.horizontal(|ui| {
                                kit::status_dot(ui, style::state(&theme, s.rec.state), 0.0);
                                kit::text(ui, &s.name(), 13.0, style::c(theme.text));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(s.rec.state.as_str())
                                                .size(11.0)
                                                .color(style::state(&theme, s.rec.state)),
                                        );
                                    },
                                );
                            });
                        }
                    });
                ui.add_space(kit::S2);
                let busy = sessions
                    .iter()
                    .filter(|s| lifecycle::close_needs_confirm(s.rec.state))
                    .count();
                let mut note = String::from("Each agent gets TERM, then KILL after 4 s");
                if close_tab {
                    note.push_str(", and its iTerm2 tab closes");
                }
                note.push_str(". Claude sessions can be reopened with ⌘⇧T.");
                if busy > 0 {
                    note = format!(
                        "{busy} still working or waiting on you — that turn is lost. {note}"
                    );
                }
                ui.add(
                    egui::Label::new(RichText::new(note).size(12.0).color(style::c(theme.dim)))
                        .wrap(),
                );
                ui.add_space(kit::S3);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if sessions.len() == 1 {
                        "Close agent".to_owned()
                    } else {
                        format!("Close {}", sessions.len())
                    };
                    if kit::primary_with(
                        ui,
                        &theme,
                        Some(Icon::Close),
                        &label,
                        Some("↵"),
                        style::c(theme.danger),
                    )
                    .clicked()
                    {
                        go = true;
                    }
                    if kit::button(ui, &theme, "Keep", Some("esc")).clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || modal.should_close() {
            return;
        }
        if go {
            let ids: Vec<String> = sessions.iter().map(|s| s.rec.id.clone()).collect();
            self.close_now(&ids);
            return;
        }
        self.close_confirm = Some(c);
    }

    /// Palette entries for the lifecycle.
    pub(crate) fn lifecycle_entries(&self) -> Vec<(String, String, Command)> {
        let mut out = Vec::new();
        let stale = self
            .snap
            .sessions
            .iter()
            .filter(|s| self.stage(s) == Stage::Stale)
            .count();
        if stale > 0 {
            out.push((
                format!("Close {stale} stale agents"),
                "sweep".into(),
                Command::new("session.sweep", None),
            ));
        }
        for s in &self.snap.sessions {
            out.push((
                format!("Close: {}", s.name()),
                s.rec.state.as_str().to_owned(),
                Command::new("session.close", Some(&s.rec.id)),
            ));
            if s.rec.state == AgentState::Working && s.rec.tty.is_some() {
                out.push((
                    format!("Stop turn: {}", s.name()),
                    "working".into(),
                    Command::new("session.stop", Some(&s.rec.id)),
                ));
            }
        }
        for c in &self.closed.entries {
            if c.reopen_line().is_some() {
                out.push((
                    format!("Reopen: {}", c.name),
                    artalde_core::text::ago(artalde_core::now().saturating_sub(c.at)),
                    Command::new("session.reopen", Some(&c.id)),
                ));
            }
        }
        out
    }
}

/// azala's sheet: a raised card with a 14 pt radius.
pub fn sheet_frame(theme: &artalde_core::theme::Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(style::c(theme.surface))
        .stroke(egui::Stroke::new(1.0, style::c(theme.border)))
        .corner_radius(egui::CornerRadius::same(kit::SHEET_RADIUS))
        .inner_margin(egui::Margin::same(20))
        .shadow(egui::Shadow {
            offset: [0, 12],
            blur: 40,
            spread: 0,
            color: egui::Color32::from_black_alpha(90),
        })
}

/// `Label  [control]`, the labels one width so controls line up.
fn field_row(
    ui: &mut egui::Ui,
    theme: &artalde_core::theme::Theme,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(kit::CONTROL);
        kit::caption(
            ui,
            &label.to_uppercase(),
            64.0,
            kit::CONTROL,
            crate::fonts::semibold(10.0),
            style::c(theme.faint),
        );
        ui.spacing_mut().item_spacing.x = 4.0;
        add(ui);
    });
}
