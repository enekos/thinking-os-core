//! A workspace of terminal panes: the split tree laid out, each pane with
//! its header, and the dividers between them. The terminals themselves are
//! drawn by `term_view.rs`; `terms.rs` owns them.

use artalde_core::command::Command;
use artalde_core::panes::{Area, Dir};
use artalde_core::theme::Theme;
use artalde_host::{Exit, State, StatusState, TerminalInfo};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, pos2,
    vec2,
};

use crate::fonts;
use crate::icons::Icon;
use crate::kit::{self, Mark};
use crate::style::{self, tint};
use crate::term_view;
use crate::terms::{self, Terms};

/// Space between panes, and around them.
pub const GAP: f32 = 6.0;
const HEADER: f32 = 28.0;
const RADIUS: u8 = 8;
const TOOL: f32 = 22.0;
const TOOL_GLYPH: f32 = 13.0;
/// How far past its gap a divider can be grabbed.
const GRAB: f32 = 3.0;

#[derive(Clone, Copy, Debug)]
pub struct Env<'a> {
    pub theme: &'a Theme,
    pub font_size: f32,
    pub option_as_alt: bool,
    pub now: f64,
    /// Keys may go to this workspace: it is on screen and no sheet is open.
    pub active: bool,
    /// Take the keyboard whenever nothing else has it.
    pub sticky: bool,
    pub synthesize_text: bool,
}

/// What a pane's header says.
struct Head {
    title: String,
    cwd: String,
    mark: Mark,
    color: Color32,
    exited: Option<String>,
}

fn head(info: Option<&TerminalInfo>, bell: bool, busy: bool, theme: &Theme) -> Head {
    let Some(info) = info else {
        return Head {
            title: "terminal".into(),
            cwd: String::new(),
            mark: Mark::Ghost,
            color: style::c(theme.faint),
            exited: Some("gone".into()),
        };
    };
    let (mark, color) = match (&info.state, info.status.as_ref().map(|s| s.state)) {
        (State::Exited { .. }, _) => (Mark::Ghost, style::c(theme.faint)),
        (_, Some(StatusState::Blocked)) => (Mark::Alert, style::c(theme.blocked)),
        (_, Some(StatusState::Error)) => (Mark::Dot, style::c(theme.danger)),
        (_, Some(StatusState::Working)) => (Mark::Spin, style::c(theme.working)),
        (_, Some(StatusState::Done)) => (Mark::Dot, style::c(theme.done)),
        _ if bell => (Mark::Alert, style::c(theme.accent)),
        _ if busy => (Mark::Dot, style::c(theme.working)),
        _ => (Mark::Ring, style::c(theme.faint)),
    };
    let exited = match &info.state {
        State::Running => None,
        State::Exited { lost: true, .. } => Some("lost with its host".into()),
        State::Exited {
            signal: Some(s), ..
        } => Some(format!("ended by signal {s}")),
        State::Exited { code: Some(c), .. } => Some(format!("exited {c}")),
        State::Exited { .. } => Some("exited".into()),
    };
    Head {
        title: terms::display_name(info),
        cwd: terms::short_path(&info.cwd),
        mark,
        color,
        exited,
    }
}

fn area(r: Rect) -> Area {
    Area {
        x: r.min.x,
        y: r.min.y,
        w: r.width(),
        h: r.height(),
    }
}

fn rect(a: Area) -> Rect {
    Rect::from_min_size(pos2(a.x, a.y), vec2(a.w.max(0.0), a.h.max(0.0)))
}

/// A small header button. Returns whether it was clicked.
fn tool(
    ui: &mut egui::Ui,
    theme: &Theme,
    at: Rect,
    key: (u32, u8),
    icon: Icon,
    help: &str,
) -> bool {
    let resp = ui
        .interact(
            at,
            egui::Id::unique(("artalde-pane-tool", key)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    let hot = resp.hovered();
    if hot {
        ui.painter().rect_filled(
            at,
            CornerRadius::same(6),
            tint(
                style::c(theme.surface),
                style::c(theme.text),
                if resp.is_pointer_button_down_on() {
                    0.16
                } else {
                    0.09
                },
            ),
        );
    }
    kit::paint_icon(
        ui.painter(),
        Rect::from_center_size(at.center(), vec2(TOOL_GLYPH, TOOL_GLYPH)),
        icon,
        style::c(if hot { theme.text } else { theme.dim }),
    );
    resp.on_hover_text(help).clicked()
}

/// What the user did to the workspace this frame.
#[derive(Default)]
struct Edits {
    focus: Option<u32>,
    nudge: Option<(Vec<bool>, f32, bool)>,
    save: bool,
}

/// Draw workspace `id` in `page`.
pub fn show(
    ui: &mut egui::Ui,
    page: Rect,
    terms: &mut Terms,
    id: u32,
    env: &Env<'_>,
    out: &mut Vec<Command>,
) {
    let theme = env.theme;
    ui.painter()
        .rect_filled(page, CornerRadius::ZERO, style::c(theme.background));
    let Some(ws) = terms.workspace(id).cloned() else {
        vacant(ui, page, terms, theme, out);
        return;
    };
    if terms.enabled() && !terms.connected() && terms.error.is_some() {
        vacant(ui, page, terms, theme, out);
        return;
    }
    let inner = page.shrink(GAP);
    let panes = ws.root.leaves().len();
    let placed: Vec<(u32, Area)> = if ws.zoomed {
        vec![(ws.focus, area(inner))]
    } else {
        ws.root.place(area(inner), GAP)
    };
    let mut edits = Edits::default();
    let grab = std::mem::take(&mut terms.grab);

    for (terminal, a) in placed {
        let frame = rect(a);
        if frame.width() < 40.0 || frame.height() < HEADER + 20.0 {
            continue;
        }
        let focused = ws.focus == terminal;
        let bell = terms.bells.contains(&terminal);
        let h = head(
            terms.infos.get(&terminal),
            bell,
            terms.busy(terminal),
            theme,
        );
        let hovered = ui.rect_contains_pointer(frame);
        let bar = Rect::from_min_size(frame.min, vec2(frame.width(), HEADER));
        let body = Rect::from_min_max(pos2(frame.min.x, bar.max.y), frame.max).shrink(1.0);

        // The pane itself.
        let p = ui.painter().clone();
        p.rect_filled(
            frame,
            CornerRadius::same(RADIUS),
            term_view::background(theme),
        );
        p.rect_filled(
            bar,
            CornerRadius {
                nw: RADIUS,
                ne: RADIUS,
                sw: 0,
                se: 0,
            },
            tint(
                style::c(theme.background),
                style::c(theme.text),
                if focused { 0.055 } else { 0.03 },
            ),
        );

        // The terminal.
        let mut pressed = false;
        match terms.view(terminal) {
            Ok((client, view)) => {
                let venv = term_view::Env {
                    client,
                    theme,
                    font_size: env.font_size,
                    option_as_alt: env.option_as_alt,
                    focused: env.active && (env.sticky || grab) && focused,
                    now: env.now,
                    synthesize_text: env.synthesize_text,
                };
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                child.set_clip_rect(body.intersect(ui.clip_rect()));
                pressed = view.show(&mut child, body, &venv, out);
                let note = match (&view.error, &view.ended, &h.exited) {
                    (Some(e), _, _) => Some(e.clone()),
                    (None, Some(Exit::Failed(e)), _) => Some(e.clone()),
                    (None, _, Some(x)) => Some(format!("Process {x}")),
                    _ => None,
                };
                if let Some(note) = note {
                    banner(ui, theme, body, terminal, &note, &mut edits, out);
                }
            }
            Err(e) => {
                p.text(
                    body.center(),
                    Align2::CENTER_CENTER,
                    e,
                    FontId::proportional(12.0),
                    style::c(theme.danger),
                );
            }
        }

        // Header: state, name, folder, tools.
        let mut x = bar.min.x + kit::S3;
        kit::paint_mark(&p, pos2(x + 4.0, bar.center().y), h.mark, h.color, env.now);
        if matches!(h.mark, Mark::Spin | Mark::Alert) {
            ui.ctx().request_repaint();
        }
        x += 8.0 + kit::S2;
        let show_tools = hovered || focused;
        let tools: &[(u8, Icon, &str, &str)] = &[
            (0, Icon::SplitRight, "Split right (⌘D)", "term.split:right"),
            (1, Icon::SplitDown, "Split down (⇧⌘D)", "term.split:down"),
            (
                2,
                if ws.zoomed {
                    Icon::Minimize
                } else {
                    Icon::Maximize
                },
                if ws.zoomed {
                    "Show every pane (⇧⌘↵)"
                } else {
                    "Zoom this pane (⇧⌘↵)"
                },
                "term.zoom",
            ),
            (3, Icon::Close, "Close pane (⌘W)", "term.close"),
        ];
        let shown: Vec<_> = tools
            .iter()
            .filter(|t| t.0 != 2 || panes > 1)
            .filter(|t| t.0 == 3 || bar.width() > 220.0)
            .collect();
        let tools_w = shown.len() as f32 * (TOOL + 2.0) + kit::S1;
        let text_max = (bar.max.x - x - if show_tools { tools_w } else { kit::S2 }).max(0.0);
        let title = kit::ellipsized(
            &p,
            &h.title,
            fonts::semibold(12.0),
            style::c(if focused { theme.text } else { theme.dim }),
            text_max,
        );
        let title_w = title.size().x;
        p.galley(
            pos2(x, (bar.center().y - title.size().y / 2.0).round()),
            title,
            Color32::PLACEHOLDER,
        );
        x += title_w + kit::S2;
        let mut tail = h.cwd;
        if ws.zoomed && panes > 1 {
            tail = format!("zoomed · {panes} panes · {tail}");
        }
        if bar.max.x - x - tools_w > 60.0 && !tail.is_empty() {
            let cwd = kit::ellipsized(
                &p,
                &tail,
                FontId::proportional(11.5),
                style::c(theme.faint),
                text_max - title_w - kit::S2,
            );
            p.galley(
                pos2(x, (bar.center().y - cwd.size().y / 2.0).round()),
                cwd,
                Color32::PLACEHOLDER,
            );
        }
        let bar_resp = ui.interact(
            bar,
            egui::Id::unique(("artalde-pane-bar", terminal)),
            Sense::click(),
        );
        if bar_resp.double_clicked() && panes > 1 {
            edits.focus = Some(terminal);
            out.push(Command::new("term.zoom", None));
        } else if bar_resp.clicked() {
            edits.focus = Some(terminal);
        }
        if show_tools {
            let mut tx = bar.max.x - kit::S1 - TOOL;
            for (n, icon, help, cmd) in shown.into_iter().rev() {
                let at =
                    Rect::from_min_size(pos2(tx, bar.center().y - TOOL / 2.0), vec2(TOOL, TOOL));
                if tool(ui, theme, at, (terminal, *n), *icon, help) {
                    edits.focus = Some(terminal);
                    match cmd.split_once(':') {
                        Some((name, arg)) => out.push(Command::new(name, Some(arg))),
                        None => out.push(Command::new(cmd, None)),
                    }
                }
                tx -= TOOL + 2.0;
            }
        }

        // The border goes on last, over the terminal's square corners.
        let lit = focused && panes > 1 && !ws.zoomed && env.active;
        p.rect_stroke(
            frame,
            CornerRadius::same(RADIUS),
            Stroke::new(
                1.0,
                if lit {
                    tint(style::c(theme.border), style::c(theme.accent), 0.75)
                } else {
                    style::c(theme.border)
                },
            ),
            StrokeKind::Inside,
        );
        if pressed {
            edits.focus = Some(terminal);
        }
    }

    if !ws.zoomed {
        for d in ws.root.dividers(area(inner), GAP) {
            let gap = rect(d.area);
            let (grab, cursor) = match d.dir {
                Dir::Right => (gap.expand2(vec2(GRAB, 0.0)), CursorIcon::ResizeHorizontal),
                Dir::Down => (gap.expand2(vec2(0.0, GRAB)), CursorIcon::ResizeVertical),
            };
            let resp = ui
                .interact(
                    grab,
                    egui::Id::unique(("artalde-pane-divider", id, d.path.clone())),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(cursor);
            if resp.hovered() || resp.dragged() {
                let line = match d.dir {
                    Dir::Right => Rect::from_center_size(gap.center(), vec2(2.0, gap.height())),
                    Dir::Down => Rect::from_center_size(gap.center(), vec2(gap.width(), 2.0)),
                };
                ui.painter().rect_filled(
                    line.shrink2(match d.dir {
                        Dir::Right => vec2(0.0, RADIUS.into()),
                        Dir::Down => vec2(RADIUS.into(), 0.0),
                    }),
                    CornerRadius::same(1),
                    style::c(theme.accent).gamma_multiply(if resp.dragged() { 0.9 } else { 0.5 }),
                );
            }
            if resp.double_clicked() {
                edits.nudge = Some((d.path, 0.0, true));
                edits.save = true;
            } else if resp.dragged() && d.extent > 0.0 {
                let by = match d.dir {
                    Dir::Right => resp.drag_delta().x,
                    Dir::Down => resp.drag_delta().y,
                };
                if by != 0.0 {
                    edits.nudge = Some((d.path, by / d.extent, false));
                }
            } else if resp.drag_stopped() {
                edits.save = true;
            }
        }
    }

    let mut save = edits.save;
    if let Some(w) = terms.workspace_mut(id) {
        if let Some(f) = edits.focus
            && w.focus != f
            && w.root.contains(f)
        {
            w.focus = f;
            save = true;
            // The old pane's terminal lets the keyboard go.
            ui.memory_mut(|m| {
                if let Some(old) = m.focused() {
                    m.surrender_focus(old);
                }
            });
        }
        if let Some((path, by, reset)) = edits.nudge {
            w.root.nudge(&path, by, reset);
        }
    }
    if let Some(f) = edits.focus {
        terms.bells.remove(&f);
    }
    if save {
        terms.save();
    }
}

/// A strip along the bottom of a pane whose program is gone.
fn banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    body: Rect,
    terminal: u32,
    note: &str,
    edits: &mut Edits,
    out: &mut Vec<Command>,
) {
    let strip = Rect::from_min_max(pos2(body.min.x, body.max.y - 30.0), body.max);
    let p = ui.painter();
    p.rect_filled(
        strip,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: RADIUS - 1,
            se: RADIUS - 1,
        },
        tint(style::c(theme.background), style::c(theme.text), 0.06),
    );
    let close_w = 64.0;
    let text = kit::ellipsized(
        p,
        note,
        FontId::proportional(12.0),
        style::c(theme.dim),
        strip.width() - close_w - 3.0 * kit::S3,
    );
    p.galley(
        pos2(
            strip.min.x + kit::S3,
            (strip.center().y - text.size().y / 2.0).round(),
        ),
        text,
        Color32::PLACEHOLDER,
    );
    let at = Rect::from_min_size(
        pos2(strip.max.x - close_w - kit::S2, strip.center().y - 11.0),
        vec2(close_w, 22.0),
    );
    let resp = ui
        .interact(
            at,
            egui::Id::unique(("artalde-pane-dismiss", terminal)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    let p = ui.painter();
    p.rect_filled(
        at,
        CornerRadius::same(6),
        tint(
            style::c(theme.surface),
            style::c(theme.text),
            if resp.hovered() { 0.14 } else { 0.07 },
        ),
    );
    p.text(
        at.center(),
        Align2::CENTER_CENTER,
        "Close",
        FontId::proportional(12.0),
        style::c(theme.text),
    );
    if resp.clicked() {
        edits.focus = Some(terminal);
        out.push(Command::new("term.close", None));
    }
}

/// A workspace with nothing in it: how to start, or why it cannot.
fn vacant(ui: &mut egui::Ui, page: Rect, terms: &Terms, theme: &Theme, out: &mut Vec<Command>) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(page.shrink(kit::S4)));
    child.add_space((page.height() * 0.22).min(120.0));
    if !terms.enabled() {
        kit::empty_with(
            &mut child,
            theme,
            Icon::Terminal,
            "Terminals are off in a dry run",
            "Set ARTALDE_DRIVE_TERMINALS=1 to give the run a private host.",
        );
        return;
    }
    if let Some(e) = &terms.error {
        kit::empty_with(
            &mut child,
            theme,
            Icon::Terminal,
            "The terminal host is out of reach",
            e,
        );
    } else {
        kit::empty_with(
            &mut child,
            theme,
            Icon::Terminal,
            "No terminal here yet",
            "Terminals run in a host that outlives this window: close artalde and they keep going.",
        );
    }
    child.vertical_centered(|ui| {
        if terms.error.is_some() {
            if kit::button_icon(ui, theme, Icon::Refresh, "Try again", None).clicked() {
                out.push(Command::new("term.reconnect", None));
            }
        } else if kit::button_icon(ui, theme, Icon::Plus, "New terminal", Some("⌘T")).clicked() {
            out.push(Command::new("term.new", None));
        }
    });
}
