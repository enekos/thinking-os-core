//! Terminals in the window: the `term.*` commands, terminal tabs, closing,
//! and the bridge between agent sessions and the terminals they run in.
//! State is in `terms.rs`, drawing in `term_panes.rs` and `term_view.rs`.

use std::collections::BTreeMap;

use artalde_core::command::Command;
use artalde_core::panes::{Dir, Toward};
use artalde_host::RunOptions;
use eframe::egui::{self, Key, Modifiers, RichText};

use crate::app::App;
use crate::icons::Icon;
use crate::lifecycle_view::sheet_frame;
use crate::terms::{self, Change, HOME_WORKSPACE};
use crate::views::Tab;
use crate::{kit, style, term_panes, term_view};

/// Label on terminals that are plain shells: they close with their shell,
/// whatever it exited with.
const KIND: &str = "artalde.kind";

impl App {
    /// Whether agents and shells open inside the window.
    pub(crate) fn embedded_terminals(&self) -> bool {
        self.settings().terminal != "iterm"
    }

    /// Apply what the host reported since the last frame.
    pub(crate) fn tick_terminals(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        for change in self.terms.tick(now) {
            match change {
                Change::Exited { terminal, clean } => {
                    if self.terms.workspace_of(terminal).is_none() {
                        continue;
                    }
                    let shell = self
                        .terms
                        .infos
                        .get(&terminal)
                        .is_some_and(|i| i.labels.get(KIND).is_some_and(|k| k == "shell"));
                    if shell || clean {
                        self.close_terminal(terminal);
                    }
                }
                Change::Bell(terminal) => {
                    let looking = self
                        .current_workspace()
                        .and_then(|w| self.terms.workspace(w))
                        .is_some_and(|w| w.focus == terminal);
                    if !looking {
                        self.terms.bells.insert(terminal);
                    }
                }
            }
        }
        // A tab whose panes are all gone has nothing to show.
        for i in (1..self.tabs.len()).rev() {
            if let Tab::Terminal(ws) = self.tabs[i]
                && self.terms.workspace(ws).is_none()
            {
                self.remove_tab(i);
            }
        }
    }

    /// The workspace keys and `term.*` commands act on.
    pub(crate) fn current_workspace(&self) -> Option<u32> {
        match self.tabs.get(self.active) {
            Some(Tab::Terminal(ws)) => Some(*ws),
            Some(Tab::Home)
                if self.terms.workspace(HOME_WORKSPACE).is_some()
                    && self.layout_shows("terminal") =>
            {
                Some(HOME_WORKSPACE)
            }
            _ => None,
        }
    }

    fn focused_terminal(&self) -> Option<u32> {
        if let Some(Tab::Session(id)) = self.tabs.get(self.active) {
            return self.shown_session_terminal(id);
        }
        self.current_workspace()
            .and_then(|w| self.terms.workspace(w))
            .map(|w| w.focus)
    }

    /// The terminal a session tab is showing right now, if it shows one.
    pub(crate) fn shown_session_terminal(&self, id: &str) -> Option<u32> {
        self.session_terminal(id).filter(|_| {
            !self.session_transcripts.contains(id) && !self.session_changes.contains(id)
        })
    }

    /// Whether a terminal has the keyboard right now.
    pub(crate) fn terminal_has_keys(&self, ctx: &egui::Context) -> bool {
        self.focused_terminal()
            .is_some_and(|t| ctx.memory(|m| m.has_focus(term_view::widget_id(t))))
    }

    /// Let whatever holds the keyboard go, so the focused pane takes it.
    fn hand_keys_to_pane(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        self.terms.grab = true;
    }

    fn start_shell(&mut self, cwd: Option<String>) -> Result<u32, String> {
        let options = RunOptions {
            argv: self.settings().shell,
            cwd,
            labels: BTreeMap::from([(KIND.to_owned(), "shell".to_owned())]),
            ..RunOptions::default()
        };
        self.terms.run(&options)
    }

    /// Run a shell line (an agent, usually) in a terminal tab of its own.
    /// `false` when terminals are not embedded and the caller should use
    /// the outside terminal.
    pub(crate) fn run_embedded(&mut self, line: &str, name: Option<&str>) -> bool {
        if !self.embedded_terminals() {
            return false;
        }
        if !self.terms.enabled() {
            self.push_log(format!("dry run: terminal {line}"));
            return true;
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let options = RunOptions {
            argv: vec![shell, "-lic".into(), line.to_owned()],
            name: name.map(str::to_owned),
            labels: BTreeMap::from([(KIND.to_owned(), "agent".to_owned())]),
            ..RunOptions::default()
        };
        match self.terms.run(&options) {
            Ok(id) => {
                let ws = self.terms.add_workspace(id, false);
                self.open_tab(Tab::Terminal(ws));
                self.hand_keys_to_pane(&self.egui_ctx.clone());
            }
            Err(e) => self.term_error(&e),
        }
        true
    }

    fn term_error(&mut self, e: &str) {
        let ctx = self.egui_ctx.clone();
        self.push_log(format!("terminal: {e}"));
        self.toast_line(&ctx, format!("Terminal failed: {e}"));
    }

    /// Show `terminal`, in the tab that already has it or a new one.
    pub(crate) fn open_terminal(&mut self, terminal: u32) {
        let ws = match self.terms.workspace_of(terminal) {
            Some(ws) => ws,
            None => self.terms.add_workspace(terminal, false),
        };
        if let Some(w) = self.terms.workspace_mut(ws) {
            if w.focus != terminal {
                w.zoomed = false;
            }
            w.focus = terminal;
        }
        self.terms.bells.remove(&terminal);
        if ws == HOME_WORKSPACE {
            self.active = 0;
        } else {
            self.open_tab(Tab::Terminal(ws));
        }
        self.hand_keys_to_pane(&self.egui_ctx.clone());
    }

    /// End `terminal` and take its pane away; an emptied tab closes.
    pub(crate) fn close_terminal(&mut self, terminal: u32) {
        self.terms.end(terminal);
        if let Some(ws) = self.terms.remove_pane(terminal)
            && let Some(i) = self.tabs.iter().position(|t| *t == Tab::Terminal(ws))
        {
            self.remove_tab(i);
        }
        self.hand_keys_to_pane(&self.egui_ctx.clone());
    }

    /// Close `terminals`, asking first when one of them is in the middle
    /// of something.
    pub(crate) fn request_close_terminals(&mut self, terminals: Vec<u32>) {
        if terminals.iter().any(|t| self.terms.busy(*t)) {
            self.term_confirm = Some(terminals);
        } else {
            for t in terminals {
                self.close_terminal(t);
            }
        }
    }

    /// `term.*`. Returns `false` for a name that is not a terminal command.
    pub(crate) fn term_command(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        arg: Option<&str>,
    ) -> bool {
        if !name.starts_with("term.") {
            return false;
        }
        if !self.terms.enabled() {
            self.push_log(format!("dry run: {name} {}", arg.unwrap_or_default()));
            return true;
        }
        match name {
            "term.new" => {
                let cwd = arg
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .or_else(|| self.context_dir());
                let home = self.tabs.get(self.active) == Some(&Tab::Home)
                    && self.layout_shows("terminal")
                    && self.terms.workspace(HOME_WORKSPACE).is_none();
                match self.start_shell(cwd) {
                    Ok(id) => {
                        let ws = self.terms.add_workspace(id, home);
                        if !home {
                            self.open_tab(Tab::Terminal(ws));
                        }
                        self.hand_keys_to_pane(ctx);
                    }
                    Err(e) => self.term_error(&e),
                }
            }
            "term.split" => {
                let dir = match arg {
                    Some("down") => Dir::Down,
                    Some("right") | None => Dir::Right,
                    Some(other) => {
                        self.push_log(format!("term.split: right or down, not {other:?}"));
                        return true;
                    }
                };
                let Some(ws) = self.current_workspace() else {
                    return self.term_command(ctx, "term.new", None);
                };
                let cwd = self
                    .focused_terminal()
                    .and_then(|t| self.terms.infos.get(&t))
                    .map(|i| i.cwd.clone())
                    .filter(|c| !c.is_empty());
                match self.start_shell(cwd) {
                    Ok(id) => {
                        self.terms.split(ws, id, dir);
                        self.hand_keys_to_pane(ctx);
                    }
                    Err(e) => self.term_error(&e),
                }
            }
            "term.close" => match arg.and_then(|a| a.parse::<u32>().ok()) {
                Some(t) => self.request_close_terminals(vec![t]),
                None => {
                    if let Some(t) = self.focused_terminal() {
                        self.request_close_terminals(vec![t]);
                    }
                }
            },
            "term.focus" => {
                let Some(toward) = arg.and_then(Toward::parse) else {
                    self.push_log("term.focus: left, right, up or down");
                    return true;
                };
                if let Some(ws) = self.current_workspace()
                    && let Some(w) = self.terms.workspace_mut(ws)
                    && !w.zoomed
                    && let Some(next) = w.root.neighbor(w.focus, toward)
                {
                    w.focus = next;
                    self.terms.bells.remove(&next);
                    self.terms.save();
                    self.hand_keys_to_pane(ctx);
                }
            }
            "term.zoom" => {
                if let Some(ws) = self.current_workspace()
                    && let Some(w) = self.terms.workspace_mut(ws)
                {
                    w.zoomed = !w.zoomed && w.root.leaves().len() > 1;
                    self.terms.save();
                }
            }
            "term.open" => match arg.and_then(|a| a.parse::<u32>().ok()) {
                Some(t) if self.terms.infos.contains_key(&t) => self.open_terminal(t),
                _ => self.push_log("term.open: no such terminal"),
            },
            "term.clear" => {
                if let Some(t) = self.focused_terminal()
                    && let Err(e) = self.terms.write(t, b"\x0c")
                {
                    self.push_log(format!("term.clear: {e}"));
                }
            }
            "term.reconnect" => self.terms.reconnect(),
            "term.find" => {
                if let Some(t) = self.focused_terminal()
                    && let Some(v) = self.terms.views.get_mut(&t)
                {
                    v.open_find();
                }
            }
            other => self.push_log(format!("unknown command {other:?}")),
        }
        true
    }

    /// The folder a new terminal starts in: the focused terminal's, else
    /// the selected agent's, else the tab's repository checkout.
    fn context_dir(&self) -> Option<String> {
        if let Some(t) = self.focused_terminal()
            && let Some(i) = self.terms.infos.get(&t)
            && !i.cwd.is_empty()
        {
            return Some(i.cwd.clone());
        }
        let session = match self.tabs.get(self.active) {
            Some(Tab::Session(id)) => self.snap.session(id),
            Some(Tab::Home) => self.selected_session(),
            _ => None,
        };
        session.map(|s| {
            s.worktree
                .clone()
                .unwrap_or_else(|| s.rec.cwd.clone())
                .display()
                .to_string()
        })
    }

    fn pane_env(
        &self,
        ctx: &egui::Context,
        sticky: bool,
    ) -> (artalde_core::theme::Theme, f32, bool, f64, bool) {
        let s = self.settings();
        (
            self.theme.clone(),
            s.terminal_font_size,
            s.option_as_alt,
            ctx.input(|i| i.time),
            sticky,
        )
    }

    /// Draw workspace `ws` in `rect`.
    pub(crate) fn terminal_page(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        ws: u32,
        sticky: bool,
    ) {
        let (theme, font_size, option_as_alt, now, sticky) = self.pane_env(ui.ctx(), sticky);
        let env = term_panes::Env {
            theme: &theme,
            font_size,
            option_as_alt,
            now,
            active: !self.palette_open() && self.term_confirm.is_none(),
            sticky,
            synthesize_text: self.dry_run,
        };
        let mut out = Vec::new();
        term_panes::show(ui, rect, &mut self.terms, ws, &env, &mut out);
        self.queue.extend(out);
    }

    /// Draw one terminal by itself, as a session tab does.
    pub(crate) fn terminal_alone(&mut self, ui: &mut egui::Ui, rect: egui::Rect, terminal: u32) {
        let (theme, font_size, option_as_alt, now, _) = self.pane_env(ui.ctx(), true);
        let active = !self.palette_open() && self.term_confirm.is_none();
        let radius = egui::CornerRadius::same(kit::ROW_RADIUS);
        ui.painter()
            .rect_filled(rect, radius, term_view::background(&theme));
        let body = rect.shrink(1.0);
        let mut out: Vec<Command> = Vec::new();
        match self.terms.view(terminal) {
            Ok((client, view)) => {
                let env = term_view::Env {
                    client,
                    theme: &theme,
                    font_size,
                    option_as_alt,
                    focused: active,
                    now,
                    synthesize_text: self.dry_run,
                };
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                child.set_clip_rect(body.intersect(ui.clip_rect()));
                view.show(&mut child, body, &env, &mut out);
            }
            Err(e) => {
                ui.painter().text(
                    body.center(),
                    egui::Align2::CENTER_CENTER,
                    e,
                    egui::FontId::proportional(12.0),
                    style::c(theme.danger),
                );
            }
        }
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, style::c(theme.border)),
            egui::StrokeKind::Inside,
        );
        // Pane commands make no sense for a terminal shown alone.
        self.queue
            .extend(out.into_iter().filter(|c| c.name == "url.open"));
    }

    /// The sheet that asks before interrupting a busy terminal.
    pub(crate) fn term_confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(terminals) = self.term_confirm.take() else {
            return;
        };
        let theme = self.theme.clone();
        let rows: Vec<(String, String)> = terminals
            .iter()
            .filter_map(|t| self.terms.infos.get(t))
            .map(|i| {
                (
                    terms::display_name(i),
                    i.foreground.clone().unwrap_or_default(),
                )
            })
            .collect();
        let (esc, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        let mut go = enter;
        let mut cancel = esc;
        let modal = egui::Modal::new(egui::Id::unique("term-close-confirm"))
            .frame(sheet_frame(&theme))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                let title = if terminals.len() == 1 {
                    "Close this terminal?".to_owned()
                } else {
                    format!("Close {} terminals?", terminals.len())
                };
                ui.label(RichText::new(title).font(crate::fonts::display(17.0)));
                ui.add_space(kit::S2);
                for (name, running) in &rows {
                    ui.horizontal(|ui| {
                        kit::icon(ui, Icon::Terminal, 14.0, style::c(theme.dim));
                        kit::text(ui, name, 13.0, style::c(theme.text));
                        if !running.is_empty() {
                            kit::right(
                                ui,
                                &format!("running {running}"),
                                11.0,
                                style::c(theme.working),
                            );
                        }
                    });
                }
                ui.add_space(kit::S2);
                ui.add(
                    egui::Label::new(
                        RichText::new("Whatever is running in it is ended: HUP, then KILL.")
                            .size(12.0)
                            .color(style::c(theme.dim)),
                    )
                    .wrap(),
                );
                ui.add_space(kit::S3);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::primary_with(
                        ui,
                        &theme,
                        Some(Icon::Close),
                        "Close",
                        Some("↵"),
                        style::c(theme.danger),
                    )
                    .clicked()
                    {
                        go = true;
                    }
                    if kit::button(ui, &theme, "Cancel", Some("esc")).clicked() {
                        cancel = true;
                    }
                });
            });
        if go {
            for t in terminals {
                self.close_terminal(t);
            }
        } else if !(cancel || modal.should_close()) {
            self.term_confirm = Some(terminals);
        }
    }

    /// The terminal an agent session runs in, when it is one of ours and
    /// still there.
    pub(crate) fn session_terminal(&self, id: &str) -> Option<u32> {
        self.snap
            .session(id)
            .and_then(|s| s.rec.terminal)
            .filter(|t| self.terms.infos.contains_key(t))
    }

    /// Type into a session's own terminal. `false` when it has none here.
    pub(crate) fn type_in_terminal(&mut self, id: &str, text: &str, enter: bool) -> bool {
        let Some(t) = self.session_terminal(id) else {
            return false;
        };
        if self.dry_run && std::env::var_os("ARTALDE_DRIVE_TERMINALS").is_none() {
            self.push_log(format!("dry run: terminal {t} <- {text:?}"));
            return true;
        }
        let res = if text == "<esc>" {
            self.terms.write(t, b"\x1b")
        } else {
            self.terms.send(t, text, enter)
        };
        if let Err(e) = res {
            self.push_log(format!("send: {e}"));
        }
        true
    }
}
